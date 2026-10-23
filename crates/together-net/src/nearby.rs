//! Rooms nearby (design A15, decision D19, the WP15 notes in 9.2): a host's room says it is
//! open on the local network, and a Venuta in no room lists the rooms it hears.
//!
//! Not mDNS (iroh 1.3 has no local discovery; a DNS-SD crate would be a new dependency for one
//! datagram): a small beacon of our own. One UDP datagram every 2 s to a multicast group with
//! a TTL of 1, so it never leaves the network: a JSON object naming the room's door code, the
//! host, how many listen, how to reach the host and when the invite ends, and `gone` once when
//! it stops. The door code (`together::invite::nearby_secret`) lets a person knock and wait for
//! the host, never come in by itself.
//!
//! - [`Beacon`]: the datagram, checked strictly (peers are never trusted).
//! - [`Seen`]: the rooms heard, each with a small id for the window, forgotten after 7 s.
//! - [`Announcer`] and [`Listener`]: the two ends on a UDP socket, each on a thread of its own.
//!   Tests run them on 127.0.0.1 (unicast); the app on the group.

use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The group rooms are announced to (organisation-local scope).
pub const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 31);
pub const PORT: u16 = 47_731;
/// A room says it is open this often.
pub const EVERY: Duration = Duration::from_secs(2);
/// A room not heard this long is gone from the list, ms.
pub const FORGET_AFTER_MS: u64 = 7_000;
/// The most a beacon may be (one datagram, never fragmented).
pub const MAX_BYTES: usize = 1_200;
const WHAT: &str = "room";
const VERSION: u32 = 1;

/// The datagram a room sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Beacon {
    pub venuta: String,
    pub v: u32,
    /// The room's door code (32 hex).
    pub door: String,
    /// The host's name.
    pub host: String,
    /// How many people are in the room, the host included.
    pub listening: u32,
    /// How to reach the host (the network's own ticket).
    pub to: String,
    /// When the invite ends (seconds since 1970).
    pub until: u64,
    /// The room stopped being open here.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
}

impl Beacon {
    pub fn new(door: &str, host: &str, listening: u32, to: &str, until: u64) -> Beacon {
        Beacon {
            venuta: WHAT.into(),
            v: VERSION,
            door: door.into(),
            host: host.chars().take(40).collect(),
            listening,
            to: to.into(),
            until,
            gone: false,
        }
    }

    /// The datagram (`None` when it would be too big to send whole).
    pub fn encode(&self) -> Option<Vec<u8>> {
        let b = serde_json::to_vec(self).ok()?;
        (b.len() <= MAX_BYTES).then_some(b)
    }

    /// A datagram heard, if it is a room's beacon that holds together.
    pub fn decode(bytes: &[u8]) -> Option<Beacon> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let b: Beacon = serde_json::from_slice(bytes).ok()?;
        let hex = |s: &str| s.len() == 32 && s.bytes().all(|c| c.is_ascii_hexdigit());
        let ok = b.venuta == WHAT
            && b.v == VERSION
            && hex(&b.door)
            && !b.host.trim().is_empty()
            && b.host.chars().count() <= 40
            && !b.host.chars().any(char::is_control)
            && !b.to.is_empty()
            && b.listening <= 100;
        ok.then_some(b)
    }
}

/// The rooms heard lately.
#[derive(Debug, Default)]
pub struct Seen {
    rooms: Vec<(u32, Beacon, u64)>,
    next_id: u32,
}

impl Seen {
    /// A beacon heard at `now_ms` (any monotonic ms); whether the list changed.
    pub fn heard(&mut self, b: Beacon, now_ms: u64) -> bool {
        let at = self.rooms.iter().position(|(_, r, _)| r.door == b.door);
        if b.gone {
            return at.map(|i| self.rooms.remove(i)).is_some();
        }
        match at {
            Some(i) => {
                let (_, r, t) = &mut self.rooms[i];
                *t = now_ms;
                let changed = *r != b;
                *r = b;
                changed
            }
            None => {
                self.next_id += 1;
                self.rooms.push((self.next_id, b, now_ms));
                true
            }
        }
    }

    /// Time passes: rooms not heard for 7 s are forgotten; whether the list changed.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let before = self.rooms.len();
        self.rooms
            .retain(|(_, _, t)| now_ms.saturating_sub(*t) < FORGET_AFTER_MS);
        self.rooms.len() != before
    }

    /// The rooms, as first heard, each with its id.
    pub fn rooms(&self) -> Vec<(u32, Beacon)> {
        self.rooms.iter().map(|(i, b, _)| (*i, b.clone())).collect()
    }

    pub fn get(&self, id: u32) -> Option<Beacon> {
        self.rooms
            .iter()
            .find(|(i, _, _)| *i == id)
            .map(|(_, b, _)| b.clone())
    }
}

/// Where rooms are announced on this network.
pub fn group() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(GROUP, PORT))
}

/// Says a room is open, every 2 s, until stopped.
pub struct Announcer {
    beacon: Arc<Mutex<Beacon>>,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Announcer {
    /// Announce `beacon` to `to` (the group, or an address of this computer in tests).
    pub fn start(to: SocketAddr, beacon: Beacon) -> std::io::Result<Announcer> {
        let from: SocketAddr = if to.ip().is_loopback() {
            (Ipv4Addr::LOCALHOST, 0).into()
        } else {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        };
        let sock = UdpSocket::bind(from)?;
        if to.ip().is_multicast() {
            sock.set_multicast_ttl_v4(1)?;
        }
        let beacon = Arc::new(Mutex::new(beacon));
        let (stop, stopped) = channel();
        let b = beacon.clone();
        let thread = std::thread::Builder::new()
            .name("venuta-nearby-say".into())
            .spawn(move || {
                let send = |b: &Beacon| {
                    if let Some(bytes) = b.encode() {
                        let _ = sock.send_to(&bytes, to);
                    }
                };
                loop {
                    let now = b.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    send(&now);
                    match stopped.recv_timeout(EVERY) {
                        Err(RecvTimeoutError::Timeout) => {}
                        _ => {
                            send(&Beacon { gone: true, ..now });
                            return;
                        }
                    }
                }
            })?;
        Ok(Announcer {
            beacon,
            stop,
            thread: Some(thread),
        })
    }

    /// What the next beacons say (how many listen).
    pub fn update(&self, beacon: Beacon) {
        *self.beacon.lock().unwrap_or_else(|e| e.into_inner()) = beacon;
    }
}

impl Drop for Announcer {
    /// Stops: says the room is gone, once.
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Lists the rooms heard, until stopped.
pub struct Listener {
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    addr: SocketAddr,
}

impl Listener {
    /// Hear rooms on `at` (the group's port on every interface, joining `group`; or an address
    /// of this computer in tests). `changed` is called, on the listener's thread, with the
    /// rooms each time the list changes.
    pub fn start(
        at: SocketAddr,
        group: Option<Ipv4Addr>,
        changed: impl Fn(Vec<(u32, Beacon)>) + Send + 'static,
    ) -> std::io::Result<Listener> {
        let sock = UdpSocket::bind(at)?;
        if let Some(g) = group {
            sock.join_multicast_v4(&g, &Ipv4Addr::UNSPECIFIED)?;
        }
        sock.set_read_timeout(Some(Duration::from_millis(250)))?;
        let addr = sock.local_addr()?;
        let seen = Arc::new(Mutex::new(Seen::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (seen.clone(), stop.clone());
        let t0 = Instant::now();
        let thread = std::thread::Builder::new()
            .name("venuta-nearby-hear".into())
            .spawn(move || {
                let mut buf = [0u8; MAX_BYTES + 1];
                while !st.load(Ordering::Relaxed) {
                    let heard = sock.recv_from(&mut buf).ok();
                    let now = t0.elapsed().as_millis() as u64;
                    let mut seen = s.lock().unwrap_or_else(|e| e.into_inner());
                    let mut changed_now = seen.tick(now);
                    if let Some((n, _)) = heard
                        && let Some(b) = Beacon::decode(&buf[..n])
                    {
                        changed_now |= seen.heard(b, now);
                    }
                    if changed_now {
                        let rooms = seen.rooms();
                        drop(seen);
                        changed(rooms);
                    }
                }
            })?;
        Ok(Listener {
            seen,
            stop,
            thread: Some(thread),
            addr,
        })
    }

    /// Where it listens (its port, for tests).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn rooms(&self) -> Vec<(u32, Beacon)> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).rooms()
    }

    pub fn get(&self, id: u32) -> Option<Beacon> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).get(id)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon(door: char, listening: u32) -> Beacon {
        Beacon::new(
            &door.to_string().repeat(32),
            "Asha",
            listening,
            "ticket",
            2_000_000_000,
        )
    }

    #[test]
    fn a_beacon_goes_through_its_datagram_and_nothing_else_reads_as_one() {
        let b = beacon('a', 3);
        let bytes = b.encode().unwrap();
        assert_eq!(Beacon::decode(&bytes), Some(b.clone()));
        let gone = Beacon {
            gone: true,
            ..b.clone()
        };
        assert_eq!(Beacon::decode(&gone.encode().unwrap()), Some(gone));
        for bad in [
            &br#"{"venuta":"room","v":2,"door":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","host":"A","listening":1,"to":"t","until":1}"#[..],
            br#"{"venuta":"room","v":1,"door":"secret","host":"A","listening":1,"to":"t","until":1}"#,
            br#"{"venuta":"room","v":1,"door":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","host":"","listening":1,"to":"t","until":1}"#,
            br#"{"venuta":"room","v":1,"door":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","host":"A","listening":1,"to":"t","until":1,"secret":"x"}"#,
            b"hello",
        ] {
            assert_eq!(Beacon::decode(bad), None, "{}", String::from_utf8_lossy(bad));
        }
        let long = Beacon::new(&"a".repeat(32), "Asha", 1, &"t".repeat(MAX_BYTES), 1);
        assert_eq!(long.encode(), None, "never more than one datagram");
    }

    #[test]
    fn rooms_heard_keep_their_ids_and_are_forgotten_when_gone_or_silent() {
        let mut s = Seen::default();
        assert!(s.heard(beacon('a', 1), 0));
        assert!(s.heard(beacon('b', 2), 100));
        assert!(!s.heard(beacon('a', 1), 2_000), "the same: no change");
        assert!(s.heard(beacon('a', 2), 4_000), "someone joined");
        let ids: Vec<u32> = s.rooms().iter().map(|r| r.0).collect();
        assert_eq!(ids, vec![1, 2]);
        assert!(s.tick(7_100), "b not heard for 7 s");
        assert_eq!(s.rooms().len(), 1);
        assert_eq!(s.get(1).unwrap().listening, 2);
        let gone = Beacon {
            gone: true,
            ..beacon('a', 2)
        };
        assert!(s.heard(gone, 7_200));
        assert!(s.rooms().is_empty());
        assert!(s.heard(beacon('a', 1), 8_000));
        assert_eq!(s.rooms()[0].0, 3, "a new id after it went");
    }

    #[test]
    fn an_announced_room_is_heard_and_goes_when_its_announcer_stops() {
        let (tx, rx) = channel();
        let l = Listener::start((Ipv4Addr::LOCALHOST, 0).into(), None, move |rooms| {
            let _ = tx.send(rooms);
        })
        .unwrap();
        let a = Announcer::start(l.addr(), beacon('c', 2)).unwrap();
        let heard = rx.recv_timeout(Duration::from_secs(5)).expect("heard");
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].1.door, "c".repeat(32));
        assert_eq!(l.get(heard[0].0).unwrap().host, "Asha");
        a.update(beacon('c', 3));
        let more = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the next beacon");
        assert_eq!(more[0].1.listening, 3);
        drop(a);
        let after = rx.recv_timeout(Duration::from_secs(5)).expect("gone");
        assert!(after.is_empty(), "said gone at once, not 7 s later");
    }
}
