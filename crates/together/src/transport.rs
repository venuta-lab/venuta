//! What a room session sends and receives, whatever carries it (contract 5.6 of the Listen
//! together design): iroh QUIC in `together-net`, or the simulated network in `sim_net`.
//!
//! One connection per guest, guest to host (a star). Control messages are JSON lines on one
//! ordered stream (`proto` makes and reads them; the transport only carries them and refuses
//! lines over `proto::MAX_MESSAGE`). Clock pings go as 29-byte datagrams: never retransmitted,
//! never queued behind anything. Media (host files, covers, envelopes) go on streams of their
//! own, asked for with a `Get`.
//!
//! Every message and datagram is stamped with the receiver's monotonic time as the transport
//! read it, so a clock sample's `t4` (and the host's `t2`) is right however late the session
//! takes the event from its channel.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::Receiver;

/// A future the transport hands back (no runtime is assumed: tokio in the app, the simulation's
/// own time in tests).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One peer, as this side's transport knows it (a guest knows only the host).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u64);

/// The peer is gone: the connection closed or was declared lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gone;

impl std::fmt::Display for Gone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the other side is gone")
    }
}

impl std::error::Error for Gone {}

/// One control message: a JSON line made by `proto::line` (ends in a newline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlMsg(pub Vec<u8>);

impl ControlMsg {
    pub fn of<T: serde::Serialize>(m: &T) -> ControlMsg {
        ControlMsg(crate::proto::line(m))
    }

    /// Read it as `T`, checked as every peer message is (`proto::read`).
    pub fn read<T: for<'de> serde::Deserialize<'de>>(&self) -> Result<T, String> {
        let line = self.0.strip_suffix(b"\n").unwrap_or(&self.0);
        crate::proto::read(line)
    }
}

/// A stream to ask for: a host file by its key, a cover (`cover:<key>`) or an envelope
/// (`env:<key>`), from byte `from` to `to` (exclusive; `None`: to the end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Get {
    pub get: String,
    pub from: u64,
    pub to: Option<u64>,
}

/// A media stream being read: its chunks in order, then the end.
pub struct MediaStream {
    /// The whole length of what was asked for, when the other side said it.
    pub total: Option<u64>,
    /// Its type, when the other side said it ("audio/flac", "image/jpeg").
    pub mime: Option<String>,
    pub body: Box<dyn MediaRead>,
}

/// The reading side of a media stream.
pub trait MediaRead: Send {
    /// The next chunk (`None`: the end).
    fn read_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, Gone>>;
}

/// What the host serves for a request, beyond bytes in memory: a file read from disk as it
/// goes (a song's file is never held whole in memory), its type, and the song's length (a song's
/// file is paced by it; covers and envelopes have none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfo {
    pub file: Option<std::path::PathBuf>,
    pub mime: Option<String>,
    pub duration_ms: Option<u64>,
}

/// What the host serves when a guest asks for a stream (built into the host's transport).
pub trait MediaServer: Send + Sync {
    /// The bytes asked for, whole (the transport cuts the range and paces it), or `None` when
    /// there is no such thing.
    fn open(&self, from: PeerId, get: &Get) -> Option<std::sync::Arc<[u8]>>;
    /// The same thing described, for a transport that streams from disk and paces: `None`
    /// (the default) means take `open`'s bytes as they are.
    fn describe(&self, _from: PeerId, _get: &Get) -> Option<MediaInfo> {
        None
    }
}

/// How a connection reaches its peer now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathInfo {
    pub direct: bool,
    pub relay: bool,
    pub rtt_us: Option<u32>,
}

/// What the transport tells its session.
#[derive(Debug, Clone, PartialEq)]
pub enum TransportEvent {
    Message {
        from: PeerId,
        msg: ControlMsg,
        /// This side's monotonic time as the line was read, in ns.
        at_ns: u64,
    },
    Datagram {
        from: PeerId,
        bytes: [u8; DATAGRAM_LEN],
        /// This side's monotonic time as `read_datagram` returned, in ns.
        at_ns: u64,
    },
    /// A peer connected (a guest at the host; the host at a guest), or reconnected.
    Joined { peer: PeerId },
    /// The connection was declared lost (after the idle timeout).
    Lost { peer: PeerId },
    /// The selected path changed (relay to direct, one address to another): the clock measures
    /// again.
    PathChanged { peer: PeerId, path: PathInfo },
}

/// What a session needs from the network.
pub trait Transport: Send + Sync {
    fn send(&self, to: PeerId, msg: ControlMsg) -> Result<(), Gone>;
    fn send_datagram(&self, to: PeerId, bytes: [u8; DATAGRAM_LEN]) -> Result<(), Gone>;
    fn open_media(&self, to: PeerId, get: Get) -> BoxFuture<'static, Result<MediaStream, Gone>>;
    /// Events from now on; each call gives a new receiver and the older one hears no more.
    fn events(&self) -> Receiver<TransportEvent>;
}

/// A clock datagram's fixed length.
pub const DATAGRAM_LEN: usize = 29;

/// A clock datagram (little-endian: `kind: u8` (1 ping, 2 pong), `id: u32`, `t1`, `t2`, `t3`:
/// `i64` µs; a ping leaves t2 and t3 zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockDatagram {
    Ping { id: u32, t1: i64 },
    Pong { id: u32, t1: i64, t2: i64, t3: i64 },
}

impl ClockDatagram {
    pub fn encode(&self) -> [u8; DATAGRAM_LEN] {
        let (kind, id, t1, t2, t3) = match *self {
            ClockDatagram::Ping { id, t1 } => (1u8, id, t1, 0, 0),
            ClockDatagram::Pong { id, t1, t2, t3 } => (2u8, id, t1, t2, t3),
        };
        let mut b = [0u8; DATAGRAM_LEN];
        b[0] = kind;
        b[1..5].copy_from_slice(&id.to_le_bytes());
        b[5..13].copy_from_slice(&t1.to_le_bytes());
        b[13..21].copy_from_slice(&t2.to_le_bytes());
        b[21..29].copy_from_slice(&t3.to_le_bytes());
        b
    }

    /// Read one; anything else (another length, an unknown kind, a ping carrying t2 or t3) is
    /// dropped.
    pub fn decode(b: &[u8]) -> Option<ClockDatagram> {
        let b: &[u8; DATAGRAM_LEN] = b.try_into().ok()?;
        let id = u32::from_le_bytes(b[1..5].try_into().ok()?);
        let t = |r: std::ops::Range<usize>| i64::from_le_bytes(b[r].try_into().unwrap());
        let (t1, t2, t3) = (t(5..13), t(13..21), t(21..29));
        match b[0] {
            1 if t2 == 0 && t3 == 0 => Some(ClockDatagram::Ping { id, t1 }),
            2 => Some(ClockDatagram::Pong { id, t1, t2, t3 }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_datagrams_go_and_come_back_the_same() {
        for d in [
            ClockDatagram::Ping { id: 7, t1: -3 },
            ClockDatagram::Pong {
                id: u32::MAX,
                t1: 1,
                t2: i64::MAX,
                t3: i64::MIN,
            },
        ] {
            assert_eq!(ClockDatagram::decode(&d.encode()), Some(d));
        }
        let ping = ClockDatagram::Ping { id: 1, t1: 2 }.encode();
        assert_eq!(ping[0], 1);
        assert_eq!(&ping[1..5], &1u32.to_le_bytes());
        assert_eq!(ClockDatagram::decode(&ping[..28]), None, "too short");
        let mut odd = ping;
        odd[0] = 3;
        assert_eq!(ClockDatagram::decode(&odd), None, "an unknown kind");
        let mut odd = ping;
        odd[20] = 1;
        assert_eq!(ClockDatagram::decode(&odd), None, "a ping with t2");
    }

    #[test]
    fn a_control_message_is_a_checked_line() {
        let m = ControlMsg::of(&crate::proto::ToHost::Bye);
        assert_eq!(m.0.last(), Some(&b'\n'));
        assert_eq!(
            m.read::<crate::proto::ToHost>(),
            Ok(crate::proto::ToHost::Bye)
        );
        assert!(
            ControlMsg(b"{\"t\":\"nope\"}\n".to_vec())
                .read::<crate::proto::ToHost>()
                .is_err()
        );
    }
}
