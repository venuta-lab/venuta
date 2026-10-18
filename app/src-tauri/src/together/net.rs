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

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl, endpoint::presets};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Rooms speak this protocol on the connection.
pub const ALPN: &[u8] = b"venuta/together/1";

/// How this Venuta connects (from Settings).
#[derive(Debug, Clone, Default)]
pub struct Reach {
    pub local_only: bool,
    pub hide_ip: bool,
    /// Your own relay (https://…), instead of the public ones.
    pub relay: Option<String>,
}

impl Reach {
    pub fn from_settings() -> Reach {
        let s = engine::settings::load();
        Reach {
            local_only: s.listening.local_only,
            hide_ip: s.listening.together_hide_ip,
            relay: s
                .listening
                .together_relay
                .clone()
                .filter(|r| !r.trim().is_empty()),
        }
    }
}

/// This Venuta's key in one room: the same each time you join that room, so the host knows it's
/// you back (and a person taken out and blocked stays out); a different one in every other room,
/// so hosts can't tell it's you elsewhere. Made from a secret kept on this computer.
pub fn guest_key(room_secret: &str) -> iroh::SecretKey {
    let path = engine::paths::app_data_dir().join("together.key");
    let mine: [u8; 32] = std::fs::read(&path)
        .ok()
        .and_then(|b| b.try_into().ok())
        .unwrap_or_else(|| {
            let mut k = [0u8; 32];
            getrandom::fill(&mut k).expect("random bytes");
            let _ = std::fs::create_dir_all(engine::paths::app_data_dir());
            let _ = std::fs::write(&path, k);
            k
        });
    key_in_room(&mine, room_secret)
}

fn key_in_room(mine: &[u8; 32], room_secret: &str) -> iroh::SecretKey {
    iroh::SecretKey::from_bytes(blake3::keyed_hash(mine, room_secret.as_bytes()).as_bytes())
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
#[derive(Debug, Clone, Default, serde::Serialize, PartialEq)]
pub struct Path {
    pub direct: bool,
    pub relay: bool,
    pub rtt_ms: Option<u32>,
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

#[cfg(test)]
mod tests {
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
