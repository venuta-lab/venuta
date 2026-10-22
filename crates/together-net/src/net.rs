//! The connection under a room: an iroh endpoint (QUIC with TLS 1.3, encrypted end to end;
//! a peer is dialled by its public key), messages as checked JSON lines on one stream, and the
//! host's file streamed on its own stream.
//!
//! Three ways to connect, from Settings:
//! - **Everywhere** (the default): direct when the routers allow it (IPv6 helps), else through
//!   a relay: iroh's free public ones, or your own relay address.
//! - **Hide my IP address**: always through the relay, so the other side never sees your IP.
//! - **Local only**: no relay and no address lookup, nothing online; the invite carries the
//!   host's local addresses, so people on the same Wi-Fi connect directly.

use iroh::endpoint::{Connection, QuicTransportConfig, RecvStream, SendStream};
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl, endpoint::presets};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Rooms speak this protocol on the connection.
pub const ALPN: &[u8] = b"venuta/together/1";

/// How the streams of one connection go out against each other (`SendStream::set_priority`):
/// higher first. Room messages never wait behind file bytes; covers come last.
pub mod priority {
    /// The room's messages, the first stream.
    pub const CONTROL: i32 = 100;
    /// A song's alignment envelope.
    pub const ENVELOPE: i32 = 50;
    /// The host's files.
    pub const FILE: i32 = 0;
    /// Covers.
    pub const COVER: i32 = -10;
}

/// How this Venuta connects (from Settings).
#[derive(Debug, Clone, Default)]
pub struct Reach {
    pub local_only: bool,
    pub hide_ip: bool,
    /// Your own relay (https://…), instead of the public ones.
    pub relay: Option<String>,
}

/// This Venuta's key in one room: the same each time you join that room, so the host knows it's
/// you back (and a person taken out and blocked stays out); a different one in every other room,
/// so hosts can't tell it's you elsewhere. Made from a secret kept on this computer.
/// (The secret is kept in `dir`, the app's data folder.)
pub fn guest_key(dir: &std::path::Path, room_secret: &str) -> iroh::SecretKey {
    let path = dir.join("together.key");
    let mine: [u8; 32] = std::fs::read(&path)
        .ok()
        .and_then(|b| b.try_into().ok())
        .unwrap_or_else(|| {
            let mut k = [0u8; 32];
            getrandom::fill(&mut k).expect("random bytes");
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&path, k);
            k
        });
    key_in_room(&mine, room_secret)
}

/// What the room tells the player that follows it (the host's own, or a guest's).
#[derive(Debug, Clone, PartialEq)]
pub enum News {
    /// The room as it is now.
    Room(together::proto::RoomView),
    /// Get this queue entry ready at this position.
    Prepare {
        rev: u64,
        index: usize,
        position_ms: u64,
    },
    /// The room is over here: the player is on its own again.
    Stop,
}

/// Seconds since 1970, for when an invite ends.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `n` random bytes as hex, from the operating system's randomness.
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("the system's randomness");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn key_in_room(mine: &[u8; 32], room_secret: &str) -> iroh::SecretKey {
    iroh::SecretKey::from_bytes(blake3::keyed_hash(mine, room_secret.as_bytes()).as_bytes())
}

/// A connection silent this long is lost (design 4.1; clock pings keep a live one busy).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15);

/// The QUIC settings rooms use: iroh's own, with this idle timeout, and datagrams received or
/// not (`false` only to test a connection without them).
pub fn transport_config(idle: Duration, datagrams: bool) -> QuicTransportConfig {
    let mut b = QuicTransportConfig::builder().max_idle_timeout(Some(
        idle.try_into().expect("an idle timeout QUIC can carry"),
    ));
    if !datagrams {
        b = b.datagram_receive_buffer_size(None);
    }
    b.build()
}

/// An endpoint for a room (`host`: one that takes connections; `key`: who it is, else new).
pub async fn endpoint(
    reach: &Reach,
    host: bool,
    key: Option<iroh::SecretKey>,
) -> Result<Endpoint, String> {
    let mut b = if reach.local_only {
        Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
    } else {
        Endpoint::builder(presets::N0)
    };
    if !reach.local_only
        && let Some(r) = &reach.relay
    {
        let url: RelayUrl = r
            .trim()
            .parse()
            .map_err(|_| format!("{r} isn't a relay address."))?;
        b = b.relay_mode(RelayMode::Custom(RelayMap::from(url)));
    }
    if reach.hide_ip && !reach.local_only {
        b = b.clear_ip_transports();
    }
    if host {
        b = b.alpns(vec![ALPN.to_vec()]);
    }
    if let Some(k) = key {
        b = b.secret_key(k);
    }
    b = b.transport_config(transport_config(IDLE_TIMEOUT, true));
    b.bind()
        .await
        .map_err(|e| format!("Couldn't start listening together: {e}"))
}

/// The host's way in, for the invite: once its addresses are known (a few seconds online;
/// at once on the local network).
pub async fn ticket(ep: &Endpoint, reach: &Reach) -> String {
    if !reach.local_only {
        let _ = tokio::time::timeout(Duration::from_secs(6), ep.online()).await;
    }
    iroh_tickets::endpoint::EndpointTicket::new(ep.addr()).to_string()
}

/// How a connection reaches the other side now (for the panel and the connection check).
#[derive(Debug, Clone, Copy, Default, serde::Serialize, PartialEq)]
pub struct Path {
    pub direct: bool,
    pub relay: bool,
    pub rtt_ms: Option<u32>,
}

/// The longest round trip that still means one network (Wi-Fi at home, a LAN).
pub const SAME_NETWORK_RTT_MS: u32 = 8;

/// Whether speakers that hear each other can be kept within a few ms: every connection direct
/// and as quick as one network. Over the internet the clock can't be known that well (the way
/// there and back differ by unknown ms), and trying only makes the music crackle.
pub fn same_room_possible(paths: &[Path]) -> bool {
    paths
        .iter()
        .all(|p| p.direct && p.rtt_ms.is_some_and(|r| r <= SAME_NETWORK_RTT_MS))
}

/// Past this round trip for a while, same room goes back to apart (well over the line it is
/// switched on at, so it doesn't flicker on a network at the edge).
pub const LEAVE_SAME_ROOM_RTT_MS: u32 = 15;

/// Whether a guest's recent round trips (ms, oldest first) say they are too far for same room
/// now: the middle one of the last three over the line. Fewer than three readings: not yet.
pub fn too_far(trips: &[u32]) -> bool {
    if trips.len() < 3 {
        return false;
    }
    let mut last: Vec<u32> = trips[trips.len() - 3..].to_vec();
    last.sort_unstable();
    last[1] > LEAVE_SAME_ROOM_RTT_MS
}

pub fn path_of(conn: &Connection) -> Path {
    let paths = conn.paths();
    let selected = paths.iter().find(|p| p.is_selected());
    Path {
        direct: selected.as_ref().is_some_and(|p| p.is_ip()),
        relay: selected.as_ref().is_some_and(|p| p.is_relay()),
        rtt_ms: selected.map(|p| p.rtt().as_millis() as u32),
    }
}

/// Reads message lines from a stream, refusing any longer than the protocol allows.
pub struct Lines {
    r: BufReader<RecvStream>,
    buf: Vec<u8>,
}

impl Lines {
    pub fn new(r: RecvStream) -> Self {
        Lines {
            r: BufReader::new(r),
            buf: Vec::new(),
        }
    }

    /// The next line, or `None` when the stream ends.
    pub async fn next(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.buf.clear();
        loop {
            let chunk = self.r.fill_buf().await.map_err(|e| e.to_string())?;
            if chunk.is_empty() {
                return Ok(if self.buf.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut self.buf))
                });
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    self.buf.extend_from_slice(&chunk[..i]);
                    self.r.consume(i + 1);
                    return Ok(Some(std::mem::take(&mut self.buf)));
                }
                None => {
                    let n = chunk.len();
                    self.buf.extend_from_slice(chunk);
                    self.r.consume(n);
                }
            }
            if self.buf.len() > together::proto::MAX_MESSAGE {
                return Err("a message too large".into());
            }
        }
    }

    /// The stream itself, for reading raw bytes after a header line.
    pub fn into_inner(self) -> BufReader<RecvStream> {
        self.r
    }
}

pub async fn send_line(w: &mut SendStream, line: &[u8]) -> Result<(), String> {
    w.write_all(line).await.map_err(|e| e.to_string())
}

/// This session's clock: microseconds since it began (monotonic; the host's is the room's).
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub base: Instant,
}

impl Clock {
    pub fn new() -> Self {
        Clock {
            base: Instant::now(),
        }
    }
    pub fn now_us(&self) -> i64 {
        self.base.elapsed().as_micros() as i64
    }
    /// The moment for a time on this clock.
    pub fn instant(&self, us: i64) -> Instant {
        if us >= 0 {
            self.base + Duration::from_micros(us as u64)
        } else {
            self.base
                .checked_sub(Duration::from_micros((-us) as u64))
                .unwrap_or(self.base)
        }
    }
}

impl Default for Clock {
    /// A clock starting now.
    fn default() -> Self {
        Clock::new()
    }
}

/// The host's address in an invite's ticket.
pub fn host_addr(ticket: &str) -> Result<iroh::EndpointAddr, String> {
    let t: iroh_tickets::endpoint::EndpointTicket = ticket
        .parse()
        .map_err(|_| "That invite isn't readable.".to_owned())?;
    Ok(t.endpoint_addr().clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn same_room_goes_only_when_someone_stays_far() {
        use super::too_far;
        // One slow reading (a busy moment) doesn't end it; three mostly slow ones do.
        assert!(!too_far(&[3]));
        assert!(!too_far(&[3, 40]));
        assert!(!too_far(&[3, 40, 4]));
        assert!(!too_far(&[40, 4, 5]));
        assert!(too_far(&[4, 40, 22]));
        assert!(too_far(&[16, 17, 16]));
        assert!(!too_far(&[15, 15, 15]), "on the line still counts as near");
    }

    #[test]
    fn same_room_needs_one_network() {
        use super::{Path, same_room_possible};
        let lan = Path {
            direct: true,
            relay: false,
            rtt_ms: Some(3),
        };
        let far = Path {
            direct: true,
            relay: false,
            rtt_ms: Some(40),
        };
        let relayed = Path {
            direct: false,
            relay: true,
            rtt_ms: Some(2),
        };
        assert!(same_room_possible(&[lan, lan]));
        assert!(same_room_possible(&[]), "the host alone");
        assert!(
            !same_room_possible(&[lan, far]),
            "one friend over the internet"
        );
        assert!(!same_room_possible(&[relayed]));
        assert!(
            !same_room_possible(&[Path {
                rtt_ms: None,
                ..lan
            }]),
            "not measured yet"
        );
    }

    #[test]
    fn your_key_is_the_same_in_one_room_and_different_in_others() {
        let mine = [7u8; 32];
        let a = super::key_in_room(&mine, "room-a").public();
        assert_eq!(
            a,
            super::key_in_room(&mine, "room-a").public(),
            "back as yourself"
        );
        assert_ne!(
            a,
            super::key_in_room(&mine, "room-b").public(),
            "unlinkable elsewhere"
        );
        assert_ne!(
            a,
            super::key_in_room(&[8u8; 32], "room-a").public(),
            "someone else"
        );
    }

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_only_rooms_connect_on_this_network_with_the_invites_ticket() {
        let reach = Reach {
            local_only: true,
            ..Reach::default()
        };
        let host = endpoint(&reach, true, None).await.unwrap();
        let ticket = ticket(&host, &reach).await;
        let accept = tokio::spawn({
            let host = host.clone();
            async move {
                let conn = host.accept().await.unwrap().await.unwrap();
                let (mut send, recv) = conn.accept_bi().await.unwrap();
                let mut lines = Lines::new(recv);
                let got = lines.next().await.unwrap().unwrap();
                send_line(&mut send, b"welcome\n").await.unwrap();
                send.finish().unwrap();
                conn.closed().await;
                got
            }
        });
        let guest = endpoint(&reach, false, None).await.unwrap();
        let t: iroh_tickets::endpoint::EndpointTicket = ticket.parse().unwrap();
        let conn = tokio::time::timeout(
            Duration::from_secs(10),
            guest.connect(t.endpoint_addr().clone(), ALPN),
        )
        .await
        .expect("connects without the internet")
        .unwrap();
        let (mut send, recv) = conn.open_bi().await.unwrap();
        send_line(&mut send, b"hello\n").await.unwrap();
        let mut lines = Lines::new(recv);
        assert_eq!(lines.next().await.unwrap().unwrap(), b"welcome");
        assert!(path_of(&conn).direct, "straight across, no relay");
        conn.close(0u32.into(), b"bye");
        assert_eq!(accept.await.unwrap(), b"hello");
        guest.close().await;
        host.close().await;
    }

    /// A stream from a guest to a host over this network, and the endpoints to close after.
    async fn stream_pair() -> (SendStream, RecvStream, [Connection; 2], Endpoint, Endpoint) {
        let reach = Reach {
            local_only: true,
            ..Reach::default()
        };
        let host = endpoint(&reach, true, None).await.unwrap();
        let ticket = ticket(&host, &reach).await;
        let accept = tokio::spawn({
            let host = host.clone();
            async move {
                let conn = host.accept().await.unwrap().await.unwrap();
                let (_send, recv) = conn.accept_bi().await.unwrap();
                (recv, conn)
            }
        });
        let guest = endpoint(&reach, false, None).await.unwrap();
        let t: iroh_tickets::endpoint::EndpointTicket = ticket.parse().unwrap();
        let conn = guest
            .connect(t.endpoint_addr().clone(), ALPN)
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        // A stream is only seen by the other side once something is written on it.
        send_line(&mut send, b"first\n").await.unwrap();
        let (recv, host_conn) = accept.await.unwrap();
        (send, recv, [conn, host_conn], guest, host)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lines_come_whole_and_in_order_however_they_were_written() {
        let (mut send, recv, _conns, guest, host) = stream_pair().await;
        let mut lines = Lines::new(recv);
        assert_eq!(lines.next().await.unwrap().unwrap(), b"first");
        // Several in one write, one split over writes, an empty one, and a last one with no end.
        send_line(&mut send, b"a\nbb\n").await.unwrap();
        send_line(&mut send, b"cc").await.unwrap();
        tokio::io::AsyncWriteExt::flush(&mut send).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        send_line(&mut send, b"c\n\nlast").await.unwrap();
        send.finish().unwrap();
        assert_eq!(lines.next().await.unwrap().unwrap(), b"a");
        assert_eq!(lines.next().await.unwrap().unwrap(), b"bb");
        assert_eq!(lines.next().await.unwrap().unwrap(), b"ccc");
        assert_eq!(
            lines.next().await.unwrap().unwrap(),
            b"",
            "an empty line is a line"
        );
        assert_eq!(
            lines.next().await.unwrap().unwrap(),
            b"last",
            "the end of the stream ends the last line"
        );
        assert_eq!(lines.next().await.unwrap(), None, "then nothing more");
        guest.close().await;
        host.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_line_as_long_as_the_protocol_allows_is_read_and_the_bytes_after_it_stay() {
        let (mut send, recv, _conns, guest, host) = stream_pair().await;
        let mut lines = Lines::new(recv);
        assert_eq!(lines.next().await.unwrap().unwrap(), b"first");
        let mut big = vec![b'y'; together::proto::MAX_MESSAGE];
        big.push(b'\n');
        big.extend_from_slice(b"raw bytes after");
        send_line(&mut send, &big).await.unwrap();
        send.finish().unwrap();
        assert_eq!(
            lines.next().await.unwrap().unwrap().len(),
            together::proto::MAX_MESSAGE
        );
        // What follows a header line is read raw from the same stream.
        use tokio::io::AsyncReadExt;
        let mut rest = Vec::new();
        lines.into_inner().read_to_end(&mut rest).await.unwrap();
        assert_eq!(rest, b"raw bytes after");
        guest.close().await;
        host.close().await;
    }

    #[test]
    fn the_sessions_clock_counts_from_its_start_and_maps_back_to_moments() {
        let c = Clock::new();
        let a = c.now_us();
        assert!((0..1_000_000).contains(&a));
        assert_eq!(c.instant(0), c.base);
        assert_eq!(c.instant(1_500), c.base + Duration::from_micros(1_500));
        assert_eq!(
            c.instant(-1),
            c.base
                .checked_sub(Duration::from_micros(1))
                .unwrap_or(c.base),
            "before its start"
        );
        assert!(c.now_us() >= a, "monotonic");
    }

    #[tokio::test]
    async fn a_line_too_long_is_refused() {
        // Lines are checked as they're read; a peer can't make Venuta hold a huge message.
        let reach = Reach {
            local_only: true,
            ..Reach::default()
        };
        let host = endpoint(&reach, true, None).await.unwrap();
        let ticket = ticket(&host, &reach).await;
        let accept = tokio::spawn({
            let host = host.clone();
            async move {
                let conn = host.accept().await.unwrap().await.unwrap();
                let (_send, recv) = conn.accept_bi().await.unwrap();
                Lines::new(recv).next().await
            }
        });
        let guest = endpoint(&reach, false, None).await.unwrap();
        let t: iroh_tickets::endpoint::EndpointTicket = ticket.parse().unwrap();
        let conn = guest
            .connect(t.endpoint_addr().clone(), ALPN)
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        let big = vec![b'x'; together::proto::MAX_MESSAGE + 10];
        let _ = send_line(&mut send, &big).await;
        assert!(accept.await.unwrap().is_err());
        guest.close().await;
        host.close().await;
    }
}
