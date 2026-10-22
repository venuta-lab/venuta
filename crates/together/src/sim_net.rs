//! A simulated network for whole rooms in one process (contract 5.6): `SimNet` holds the peers
//! and the links between them, and each peer's `SimTransport` is a `Transport` like the real
//! one. Grown from the room check's `Link` (one way across the network, in order, each message
//! delayed, a little differently each time, never reordered on one stream), with what the room
//! cases need on top: a different delay each way, loss, bufferbloat episodes, a bandwidth for
//! media, a path change, and cuts.
//!
//! It runs on any `Timebase`: deliveries are callbacks at their arrival time, so on a `SimClock`
//! a run is exact and repeatable, and on `RealClock` it runs in real time. Each peer reads its
//! own `MonoClock` (a `time::SkewedClock` for another computer), and every message and datagram
//! is stamped with the receiver's clock as it arrives.
//!
//! - Control lines arrive in order; one lost on the way arrives a retransmit later (as QUIC).
//! - Datagrams arrive on their own: lost with the way's loss, and in any order.
//! - Media streams come in 64 KiB chunks at the way's bandwidth, after a round trip.
//! - A cut delivers nothing more; after the idle timeout both sides are told `Lost`, and sends
//!   fail with `Gone` until the link is restored (both told `Joined`).
//! - Every send is logged, for checks.

use crate::proto::MAX_MESSAGE;
use crate::time::{MonoClock, Timebase};
use crate::transport::{
    BoxFuture, ControlMsg, DATAGRAM_LEN, Get, Gone, MediaRead, MediaServer, MediaStream, PathInfo,
    PeerId, Transport, TransportEvent,
};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

const MS: f64 = 1_000_000.0;
/// Media come in chunks of this many bytes.
pub const CHUNK: usize = 64 * 1024;

/// How one direction of a link behaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Way {
    pub delay_ms: f64,
    /// Up to this much more, at random, per message.
    pub jitter_ms: f64,
    /// The share of packets lost (0 to 1).
    pub loss: f64,
    /// For media, in kbit/s (0: as fast as the delay allows).
    pub bandwidth_kbps: u32,
    /// Someone's upload filling a queue now and then.
    pub bloat: Option<Bloat>,
    /// Through a relay (what `PathInfo` says).
    pub relay: bool,
}

/// A bufferbloat episode every `every_s`, lasting `lasts_s`, adding `extra_ms` to the delay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bloat {
    pub every_s: f64,
    pub lasts_s: f64,
    pub extra_ms: f64,
}

impl Way {
    /// One Wi-Fi: about a millisecond.
    pub const LAN: Way = Way {
        delay_ms: 1.0,
        jitter_ms: 0.4,
        loss: 0.0,
        bandwidth_kbps: 0,
        bloat: None,
        relay: false,
    };

    /// The internet, direct: `delay_ms` with `jitter_ms`.
    pub fn internet(delay_ms: f64, jitter_ms: f64) -> Way {
        Way {
            delay_ms,
            jitter_ms,
            ..Way::LAN
        }
    }

    fn extra_ms(&self, true_ns: u64) -> f64 {
        match self.bloat {
            Some(b) if b.every_s > 0.0 => {
                let t = true_ns as f64 / 1e9;
                if t % b.every_s < b.lasts_s {
                    b.extra_ms
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }
}

/// What a check sees of one send.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub from: PeerId,
    pub to: PeerId,
    pub kind: SentKind,
    /// The network's time it was sent (the `Timebase`'s, not either peer's).
    pub at_ns: u64,
    pub bytes: usize,
    /// It never arrived (lost, or the link was cut).
    pub dropped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentKind {
    Control,
    Datagram,
    Media,
}

struct Peer {
    clock: Arc<dyn MonoClock>,
    events: Option<Sender<TransportEvent>>,
    server: Option<Arc<dyn MediaServer>>,
}

struct Link {
    /// a to b, b to a (a < b).
    ways: [Way; 2],
    /// When the last control line each way arrives (lines never overtake).
    last_due: [u64; 2],
    cut: bool,
    lost: bool,
    /// Grows at every cut: what was in flight before belongs to an older one.
    generation: u64,
}

struct Net {
    peers: HashMap<PeerId, Peer>,
    links: HashMap<(PeerId, PeerId), Link>,
    log: Vec<Sent>,
    rng: u64,
    next: u64,
    idle_timeout_ns: u64,
}

impl Net {
    fn random(&mut self) -> f64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 33) as f64 / (1u64 << 31) as f64
    }

    fn tell(&self, who: PeerId, e: TransportEvent) {
        if let Some(tx) = self.peers.get(&who).and_then(|p| p.events.as_ref()) {
            let _ = tx.send(e);
        }
    }
}

fn key(a: PeerId, b: PeerId) -> ((PeerId, PeerId), usize) {
    if a < b { ((a, b), 0) } else { ((b, a), 1) }
}

/// The simulated network.
pub struct SimNet {
    tb: Arc<dyn Timebase>,
    net: Mutex<Net>,
}

impl SimNet {
    /// A network on `tb`, packets lost and jittered from `seed`.
    pub fn new(tb: Arc<dyn Timebase>, seed: u64) -> Arc<SimNet> {
        Arc::new(SimNet {
            tb,
            net: Mutex::new(Net {
                peers: HashMap::new(),
                links: HashMap::new(),
                log: Vec::new(),
                rng: seed.max(1),
                next: 1,
                idle_timeout_ns: 15_000_000_000,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Net> {
        self.net.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How long a cut lasts before both sides are told `Lost` (QUIC's idle timeout: 15 s).
    pub fn set_idle_timeout_ms(&self, ms: u64) {
        self.lock().idle_timeout_ns = ms * 1_000_000;
    }

    /// A peer on its own clock.
    pub fn peer(self: &Arc<Self>, clock: Arc<dyn MonoClock>) -> SimTransport {
        let mut n = self.lock();
        let id = PeerId(n.next);
        n.next += 1;
        n.peers.insert(
            id,
            Peer {
                clock,
                events: None,
                server: None,
            },
        );
        SimTransport {
            net: self.clone(),
            me: id,
        }
    }

    /// What `peer` serves when asked for a stream.
    pub fn serve(&self, peer: &SimTransport, server: Arc<dyn MediaServer>) {
        if let Some(p) = self.lock().peers.get_mut(&peer.me) {
            p.server = Some(server);
        }
    }

    /// Connect two peers (`a_to_b` one way, `b_to_a` the other); both are told `Joined`.
    pub fn connect(&self, a: &SimTransport, b: &SimTransport, a_to_b: Way, b_to_a: Way) {
        let (k, side) = key(a.me, b.me);
        let ways = if side == 0 {
            [a_to_b, b_to_a]
        } else {
            [b_to_a, a_to_b]
        };
        let mut n = self.lock();
        n.links.insert(
            k,
            Link {
                ways,
                last_due: [0, 0],
                cut: false,
                lost: false,
                generation: 0,
            },
        );
        n.tell(a.me, TransportEvent::Joined { peer: b.me });
        n.tell(b.me, TransportEvent::Joined { peer: a.me });
    }

    /// The path changes (relay to direct, another address): new ways from now on, and both
    /// sides told `PathChanged`.
    pub fn set_way(&self, a: &SimTransport, b: &SimTransport, a_to_b: Way, b_to_a: Way) {
        let (k, side) = key(a.me, b.me);
        let mut n = self.lock();
        if let Some(l) = n.links.get_mut(&k) {
            l.ways = if side == 0 {
                [a_to_b, b_to_a]
            } else {
                [b_to_a, a_to_b]
            };
        }
        let rtt = ((a_to_b.delay_ms + b_to_a.delay_ms) * 1_000.0) as u32;
        let path = |w: Way| PathInfo {
            direct: !w.relay,
            relay: w.relay,
            rtt_us: Some(rtt),
        };
        n.tell(
            a.me,
            TransportEvent::PathChanged {
                peer: b.me,
                path: path(a_to_b),
            },
        );
        n.tell(
            b.me,
            TransportEvent::PathChanged {
                peer: a.me,
                path: path(b_to_a),
            },
        );
    }

    /// Nothing gets through either way; after the idle timeout both sides are told `Lost`.
    pub fn cut(self: &Arc<Self>, a: &SimTransport, b: &SimTransport) {
        let (k, _) = key(a.me, b.me);
        let (gen_now, timeout) = {
            let mut n = self.lock();
            let timeout = n.idle_timeout_ns;
            let Some(l) = n.links.get_mut(&k) else {
                return;
            };
            l.cut = true;
            l.generation += 1;
            (l.generation, timeout)
        };
        let net = self.clone();
        let (am, bm) = (a.me, b.me);
        self.tb.at(
            self.tb.now_ns() + timeout,
            Box::new(move || {
                let mut n = net.lock();
                let lost = match n.links.get_mut(&k) {
                    Some(l) if l.cut && l.generation == gen_now && !l.lost => {
                        l.lost = true;
                        true
                    }
                    _ => false,
                };
                if lost {
                    n.tell(am, TransportEvent::Lost { peer: bm });
                    n.tell(bm, TransportEvent::Lost { peer: am });
                }
            }),
        );
    }

    /// The link carries again; both sides are told `Joined` (a reconnect).
    pub fn restore(&self, a: &SimTransport, b: &SimTransport) {
        let (k, _) = key(a.me, b.me);
        let mut n = self.lock();
        if let Some(l) = n.links.get_mut(&k) {
            l.cut = false;
            l.lost = false;
            l.generation += 1;
            l.last_due = [0, 0];
        }
        n.tell(a.me, TransportEvent::Joined { peer: b.me });
        n.tell(b.me, TransportEvent::Joined { peer: a.me });
    }

    /// Every send so far.
    pub fn sent(&self) -> Vec<Sent> {
        self.lock().log.clone()
    }

    /// When something sent now from `from` to `to` arrives (network time), and whether it gets
    /// there at all, or `Err(Gone)` with no link or a lost one.
    fn route(&self, from: PeerId, to: PeerId, kind: SentKind, bytes: usize) -> Result<Route, Gone> {
        let now = self.tb.now_ns();
        let mut n = self.lock();
        let (k, side) = key(from, to);
        let (way, cut, generation, lost) = match n.links.get(&k) {
            Some(l) => (l.ways[side], l.cut, l.generation, l.lost),
            None => return Err(Gone),
        };
        if lost {
            return Err(Gone);
        }
        let r = n.random();
        let delay = way.delay_ms + way.jitter_ms * r + way.extra_ms(now);
        let lost_packet = n.random() < way.loss;
        let mut due = now + (delay * MS) as u64;
        let mut dropped = cut;
        match kind {
            SentKind::Datagram => dropped |= lost_packet,
            SentKind::Control | SentKind::Media => {
                if lost_packet {
                    // Sent again once the loss is noticed: about three times the way's delay.
                    due += (3.0 * way.delay_ms * MS) as u64;
                }
                if kind == SentKind::Control {
                    let l = n.links.get_mut(&k).unwrap();
                    due = due.max(l.last_due[side]);
                    l.last_due[side] = due;
                }
            }
        }
        n.log.push(Sent {
            from,
            to,
            kind,
            at_ns: now,
            bytes,
            dropped,
        });
        Ok(Route {
            due,
            dropped,
            generation,
            way,
            key: k,
        })
    }

    /// Deliver `e` to `to` at `route.due`, unless the link was cut since.
    fn deliver(
        self: &Arc<Self>,
        to: PeerId,
        route: &Route,
        e: impl FnOnce(u64) -> TransportEvent + Send + 'static,
    ) {
        if route.dropped {
            return;
        }
        let net = self.clone();
        let (k, generation) = (route.key, route.generation);
        self.tb.at(
            route.due,
            Box::new(move || {
                let n = net.lock();
                let live = n
                    .links
                    .get(&k)
                    .is_some_and(|l| !l.cut && l.generation == generation);
                if live && let Some(p) = n.peers.get(&to) {
                    let at = p.clock.now_ns();
                    n.tell(to, e(at));
                }
            }),
        );
    }
}

struct Route {
    due: u64,
    dropped: bool,
    generation: u64,
    way: Way,
    key: (PeerId, PeerId),
}

/// One peer's side of the simulated network.
#[derive(Clone)]
pub struct SimTransport {
    net: Arc<SimNet>,
    me: PeerId,
}

impl SimTransport {
    /// How the other peers know this one.
    pub fn id(&self) -> PeerId {
        self.me
    }

    /// This peer's own clock.
    pub fn now_ns(&self) -> u64 {
        self.net
            .lock()
            .peers
            .get(&self.me)
            .map_or(0, |p| p.clock.now_ns())
    }
}

impl Transport for SimTransport {
    fn send(&self, to: PeerId, msg: ControlMsg) -> Result<(), Gone> {
        let r = self
            .net
            .route(self.me, to, SentKind::Control, msg.0.len())?;
        // An oversized line never arrives whole (the reader refuses it).
        if msg.0.len() > MAX_MESSAGE + 1 {
            return Ok(());
        }
        let from = self.me;
        self.net
            .deliver(to, &r, move |at_ns| TransportEvent::Message {
                from,
                msg,
                at_ns,
            });
        Ok(())
    }

    fn send_datagram(&self, to: PeerId, bytes: [u8; DATAGRAM_LEN]) -> Result<(), Gone> {
        let r = self
            .net
            .route(self.me, to, SentKind::Datagram, DATAGRAM_LEN)?;
        let from = self.me;
        self.net
            .deliver(to, &r, move |at_ns| TransportEvent::Datagram {
                from,
                bytes,
                at_ns,
            });
        Ok(())
    }

    fn open_media(&self, to: PeerId, get: Get) -> BoxFuture<'static, Result<MediaStream, Gone>> {
        let net = self.net.clone();
        let me = self.me;
        let opened = (|| {
            // The ask goes there, the first bytes come back.
            let there = net.route(me, to, SentKind::Media, 0)?;
            let back = net.route(to, me, SentKind::Media, 0)?;
            let server = net.lock().peers.get(&to).and_then(|p| p.server.clone());
            let server = server.ok_or(Gone)?;
            // Bytes in memory, or a file the server describes (read whole: test files are small).
            let info = server.describe(me, &get);
            let mime = info.as_ref().and_then(|i| i.mime.clone());
            let body: Arc<[u8]> = match server.open(me, &get) {
                Some(b) => b,
                None => {
                    let file = info.and_then(|i| i.file).ok_or(Gone)?;
                    std::fs::read(file).map_err(|_| Gone)?.into()
                }
            };
            let from = (get.from as usize).min(body.len());
            let end = get
                .to
                .map_or(body.len(), |t| (t as usize).clamp(from, body.len()));
            let open_at = there.due + back.due.saturating_sub(net.tb.now_ns());
            let kbps = back.way.bandwidth_kbps;
            let mut chunks = Vec::new();
            let mut at = open_at;
            for c in body[from..end].chunks(CHUNK) {
                if kbps > 0 {
                    at += (c.len() as f64 * 8.0 / f64::from(kbps) * MS) as u64;
                }
                chunks.push((at, c.to_vec()));
            }
            let live = (back.key, back.generation);
            let size = body.len() as u64;
            Ok::<_, Gone>((open_at, (end - from) as u64, chunks, live, mime, size))
        })();
        let tb = self.net.tb.clone();
        let net2 = self.net.clone();
        Box::pin(async move {
            let (open_at, total, chunks, live, mime, size) = opened?;
            Wait::until(tb.clone(), open_at).await;
            Ok(MediaStream {
                total: Some(total),
                size: Some(size),
                mime,
                body: Box::new(SimBody {
                    tb,
                    net: net2,
                    live,
                    chunks: chunks.into(),
                }),
            })
        })
    }

    fn events(&self) -> Receiver<TransportEvent> {
        let (tx, rx) = channel();
        if let Some(p) = self.net.lock().peers.get_mut(&self.me) {
            p.events = Some(tx);
        }
        rx
    }

    /// A simulated peer's key is its name on the network: `peer<n>`.
    fn peer_key(&self, peer: PeerId) -> Option<String> {
        self.net
            .lock()
            .peers
            .contains_key(&peer)
            .then(|| format!("peer{}", peer.0))
    }

    /// The link's ways from here, as a transport sees them: direct or relayed, and the round
    /// trip (both ways' delays).
    fn path(&self, peer: PeerId) -> Option<PathInfo> {
        let n = self.net.lock();
        let (k, side) = key(self.me, peer);
        let l = n.links.get(&k).filter(|l| !l.lost)?;
        let (out, back) = (l.ways[side], l.ways[1 - side]);
        Some(PathInfo {
            direct: !(out.relay || back.relay),
            relay: out.relay || back.relay,
            rtt_us: Some(((out.delay_ms + back.delay_ms) * 1_000.0) as u32),
        })
    }

    /// Nothing more either way; both sides are told `Lost` at once (a closed connection is
    /// known at once, unlike a cut one). `restore` connects them again.
    fn close(&self, peer: PeerId) {
        let (k, _) = key(self.me, peer);
        let mut n = self.net.lock();
        let closed = match n.links.get_mut(&k) {
            Some(l) if !l.lost => {
                l.cut = true;
                l.lost = true;
                l.generation += 1;
                true
            }
            _ => false,
        };
        if closed {
            n.tell(self.me, TransportEvent::Lost { peer });
            n.tell(peer, TransportEvent::Lost { peer: self.me });
        }
    }
}

/// A media stream's body, each chunk arriving at its time.
struct SimBody {
    tb: Arc<dyn Timebase>,
    net: Arc<SimNet>,
    live: ((PeerId, PeerId), u64),
    chunks: std::collections::VecDeque<(u64, Vec<u8>)>,
}

impl MediaRead for SimBody {
    fn read_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, Gone>> {
        Box::pin(async move {
            let Some((at, _)) = self.chunks.front() else {
                return Ok(None);
            };
            Wait::until(self.tb.clone(), *at).await;
            let (k, generation) = self.live;
            let live = self
                .net
                .lock()
                .links
                .get(&k)
                .is_some_and(|l| !l.cut && l.generation == generation);
            if !live {
                return Err(Gone);
            }
            Ok(self.chunks.pop_front().map(|c| c.1))
        })
    }
}

/// A future ready at a time on a `Timebase` (woken by its callback).
struct Wait {
    tb: Arc<dyn Timebase>,
    at: u64,
    waker: Arc<Mutex<(bool, Option<Waker>)>>,
}

impl Wait {
    fn until(tb: Arc<dyn Timebase>, at: u64) -> Wait {
        Wait {
            tb,
            at,
            waker: Arc::default(),
        }
    }
}

impl Future for Wait {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.tb.now_ns() >= self.at {
            return Poll::Ready(());
        }
        let mut w = self.waker.lock().unwrap_or_else(|e| e.into_inner());
        let first = !w.0;
        w.0 = true;
        w.1 = Some(cx.waker().clone());
        drop(w);
        if first {
            let slot = self.waker.clone();
            self.tb.at(
                self.at,
                Box::new(move || {
                    if let Some(w) = slot.lock().unwrap_or_else(|e| e.into_inner()).1.take() {
                        w.wake();
                    }
                }),
            );
        }
        Poll::Pending
    }
}

/// Poll a future once with a waker that does nothing: for checks on virtual time, which poll
/// again after letting time pass.
pub fn poll_once<T>(f: &mut BoxFuture<'_, T>) -> Poll<T> {
    f.as_mut().poll(&mut Context::from_waker(Waker::noop()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{ToGuest, ToHost};
    use crate::time::{RealClock, SimClock, SkewedClock};
    use crate::transport::ClockDatagram;

    const S: u64 = 1_000_000_000;

    fn room(a_to_b: Way, b_to_a: Way) -> (SimClock, Arc<SimNet>, SimTransport, SimTransport) {
        let clock = SimClock::new(0);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb.clone(), 7);
        let base: Arc<dyn MonoClock> = Arc::new(clock.clone());
        let host = net.peer(Arc::new(SkewedClock::new(base.clone(), 900 * S, 40.0)));
        let guest = net.peer(Arc::new(SkewedClock::new(base, 3 * S, -25.0)));
        net.connect(&guest, &host, a_to_b, b_to_a);
        (clock, net, guest, host)
    }

    fn messages(rx: &Receiver<TransportEvent>) -> Vec<(ControlMsg, u64)> {
        rx.try_iter()
            .filter_map(|e| match e {
                TransportEvent::Message { msg, at_ns, .. } => Some((msg, at_ns)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn control_lines_arrive_in_order_late_by_the_way_stamped_by_the_receiver() {
        let (clock, net, guest, host) = room(Way::internet(30.0, 15.0), Way::internet(50.0, 15.0));
        let rx = host.events();
        let mut sent = Vec::new();
        for k in 0..200u32 {
            let m = ControlMsg::of(&ToHost::Ask {
                action: crate::proto::Action::Seek {
                    position_ms: u64::from(k),
                },
            });
            guest.send(host.id(), m.clone()).unwrap();
            sent.push((m, host.now_ns()));
            clock.pass(2_000_000);
        }
        clock.pass(S);
        let got = messages(&rx);
        assert_eq!(got.len(), 200);
        for ((m, sent_at), (g, at)) in sent.iter().zip(&got) {
            assert_eq!(m, g, "in order");
            // The receiver's clock runs 40 ppm fast: 30 to 45 ms of network time is a hair more.
            let late = (*at - *sent_at) as f64 / 1e6;
            assert!((29.9..=45.1).contains(&late), "{late} ms");
        }
        assert!(
            net.sent()
                .iter()
                .all(|s| s.kind == SentKind::Control && !s.dropped)
        );
    }

    #[test]
    fn datagrams_are_lost_at_the_ways_rate_and_never_wait() {
        let lossy = Way {
            loss: 0.05,
            ..Way::internet(20.0, 30.0)
        };
        let (clock, _, guest, host) = room(lossy, lossy);
        let rx = host.events();
        for id in 0..2_000u32 {
            let d = ClockDatagram::Ping { id, t1: 0 }.encode();
            guest.send_datagram(host.id(), d).unwrap();
            clock.pass(1_000_000);
        }
        clock.pass(S);
        let ids: Vec<u32> = rx
            .try_iter()
            .filter_map(|e| match e {
                TransportEvent::Datagram { bytes, .. } => match ClockDatagram::decode(&bytes) {
                    Some(ClockDatagram::Ping { id, .. }) => Some(id),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let lost = 2_000 - ids.len();
        assert!((50..=150).contains(&lost), "{lost} of 2000 lost");
        assert!(
            ids.windows(2).any(|w| w[1] < w[0]),
            "jitter reorders datagrams"
        );
    }

    #[test]
    fn clock_pings_end_to_end_find_the_offset_within_half_the_asymmetry() {
        // 20 ms up, 40 ms down: the clock can't tell the two apart, so its error is 10 ms.
        let (clock, _, guest, host) = room(Way::internet(20.0, 0.0), Way::internet(40.0, 0.0));
        let (grx, hrx) = (guest.events(), host.events());
        let mut offsets = Vec::new();
        for id in 0..20u32 {
            let t1 = (guest.now_ns() / 1_000) as i64;
            guest
                .send_datagram(host.id(), ClockDatagram::Ping { id, t1 }.encode())
                .unwrap();
            clock.pass(25_000_000);
            // The host answers at once, t2 as stamped on arrival, t3 as it sends.
            for e in hrx.try_iter() {
                if let TransportEvent::Datagram { from, bytes, at_ns } = e
                    && let Some(ClockDatagram::Ping { id, t1 }) = ClockDatagram::decode(&bytes)
                {
                    let t2 = (at_ns / 1_000) as i64;
                    let t3 = (host.now_ns() / 1_000) as i64;
                    host.send_datagram(from, ClockDatagram::Pong { id, t1, t2, t3 }.encode())
                        .unwrap();
                }
            }
            clock.pass(50_000_000);
            for e in grx.try_iter() {
                if let TransportEvent::Datagram { bytes, at_ns, .. } = e
                    && let Some(ClockDatagram::Pong { t1, t2, t3, .. }) =
                        ClockDatagram::decode(&bytes)
                {
                    let s = crate::clock::Sample {
                        t1,
                        t2,
                        t3,
                        t4: (at_ns / 1_000) as i64,
                    };
                    offsets.push(s.offset());
                }
            }
        }
        assert_eq!(offsets.len(), 20);
        // The truth: the host's clock minus the guest's, at about this moment.
        let truth = (host.now_ns() as i64 - guest.now_ns() as i64) / 1_000;
        for o in offsets {
            let err_ms = (o - truth) as f64 / 1_000.0;
            assert!((err_ms + 10.0).abs() < 1.0, "{err_ms} ms off");
        }
    }

    #[test]
    fn bufferbloat_episodes_add_their_delay_while_they_last() {
        let bloated = Way {
            bloat: Some(Bloat {
                every_s: 10.0,
                lasts_s: 2.0,
                extra_ms: 200.0,
            }),
            ..Way::internet(10.0, 0.0)
        };
        let (clock, _, guest, host) = room(bloated, Way::LAN);
        let rx = host.events();
        let mut late = Vec::new();
        for k in 0..40u32 {
            let sent = host.now_ns();
            guest
                .send_datagram(host.id(), ClockDatagram::Ping { id: k, t1: 0 }.encode())
                .unwrap();
            clock.pass(500_000_000);
            for e in rx.try_iter() {
                if let TransportEvent::Datagram { at_ns, .. } = e {
                    late.push(((at_ns - sent) as f64 / 1e6).round());
                }
            }
        }
        let bloated_ones = late.iter().filter(|&&l| l > 100.0).count();
        assert_eq!(bloated_ones, 8, "2 s of every 10, at 2 a second: {late:?}");
    }

    #[test]
    fn a_path_change_is_told_and_takes_effect() {
        let relay = Way {
            relay: true,
            ..Way::internet(60.0, 0.0)
        };
        let (clock, net, guest, host) = room(relay, relay);
        let rx = guest.events();
        net.set_way(
            &guest,
            &host,
            Way::internet(15.0, 0.0),
            Way::internet(15.0, 0.0),
        );
        let changed: Vec<_> = rx
            .try_iter()
            .filter_map(|e| match e {
                TransportEvent::PathChanged { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(
            changed,
            [PathInfo {
                direct: true,
                relay: false,
                rtt_us: Some(30_000)
            }]
        );
        let hrx = host.events();
        let sent = host.now_ns();
        guest.send(host.id(), ControlMsg::of(&ToHost::Bye)).unwrap();
        clock.pass(S);
        let (_, at) = messages(&hrx).remove(0);
        assert!(((at - sent) as f64 / 1e6 - 15.0).abs() < 0.1);
    }

    #[test]
    fn a_cut_delivers_nothing_then_is_lost_and_a_restore_joins_again() {
        let (clock, net, guest, host) = room(Way::internet(20.0, 0.0), Way::internet(20.0, 0.0));
        net.set_idle_timeout_ms(15_000);
        let (grx, hrx) = (guest.events(), host.events());
        guest.send(host.id(), ControlMsg::of(&ToHost::Bye)).unwrap();
        net.cut(&guest, &host);
        clock.pass(S);
        assert!(
            messages(&hrx).is_empty(),
            "in flight when cut: never arrives"
        );
        assert!(
            guest.send(host.id(), ControlMsg::of(&ToHost::Bye)).is_ok(),
            "not lost yet"
        );
        clock.pass(15 * S);
        assert!(
            grx.try_iter()
                .any(|e| e == TransportEvent::Lost { peer: host.id() })
        );
        assert!(
            hrx.try_iter()
                .any(|e| e == TransportEvent::Lost { peer: guest.id() })
        );
        assert_eq!(
            guest.send(host.id(), ControlMsg::of(&ToHost::Bye)),
            Err(Gone)
        );
        net.restore(&guest, &host);
        assert!(
            grx.try_iter()
                .any(|e| e == TransportEvent::Joined { peer: host.id() })
        );
        host.send(guest.id(), ControlMsg::of(&ToGuest::Waiting))
            .unwrap();
        clock.pass(S);
        let got = messages(&grx);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.read::<ToGuest>(), Ok(ToGuest::Waiting));
        // A cut that heals before the idle timeout is never told lost.
        net.cut(&guest, &host);
        clock.pass(5 * S);
        net.restore(&guest, &host);
        clock.pass(20 * S);
        assert!(
            !grx.try_iter()
                .any(|e| matches!(e, TransportEvent::Lost { .. }))
        );
    }

    struct Files(Arc<[u8]>);

    impl MediaServer for Files {
        fn open(&self, _from: PeerId, get: &Get) -> Option<Arc<[u8]>> {
            (get.get == "host:song").then(|| self.0.clone())
        }
    }

    #[test]
    fn media_come_after_a_round_trip_at_the_ways_bandwidth() {
        // 8 Mbit/s back: 1 MB in a second.
        let down = Way {
            bandwidth_kbps: 8_000,
            ..Way::internet(25.0, 0.0)
        };
        let (clock, net, guest, host) = room(Way::internet(25.0, 0.0), down);
        let body: Arc<[u8]> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
        net.serve(&host, Arc::new(Files(body.clone())));
        let t0 = clock.now_ns();
        let mut open = guest.open_media(
            host.id(),
            Get {
                get: "host:song".into(),
                from: 1_000,
                to: Some(1_001_000),
            },
        );
        assert!(poll_once(&mut open).is_pending());
        clock.pass(40_000_000);
        assert!(poll_once(&mut open).is_pending(), "a round trip is 50 ms");
        clock.pass(20_000_000);
        let Poll::Ready(Ok(mut stream)) = poll_once(&mut open) else {
            panic!("open after a round trip");
        };
        assert_eq!(stream.total, Some(1_000_000));
        let mut got = Vec::new();
        let mut last_at = 0;
        loop {
            let mut f = stream.body.read_chunk();
            let chunk = loop {
                if let Poll::Ready(c) = poll_once(&mut f) {
                    break c;
                }
                clock.pass(1_000_000);
            };
            match chunk.unwrap() {
                Some(c) => {
                    got.extend(c);
                    last_at = clock.now_ns();
                }
                None => break,
            }
        }
        assert_eq!(&got[..], &body[1_000..1_001_000]);
        let secs = (last_at - t0) as f64 / 1e9;
        assert!((1.0..1.2).contains(&secs), "{secs} s for 1 MB at 8 Mbit/s");
        let missing = guest.open_media(
            host.id(),
            Get {
                get: "host:other".into(),
                from: 0,
                to: None,
            },
        );
        let mut missing = missing;
        assert!(matches!(poll_once(&mut missing), Poll::Ready(Err(Gone))));
    }

    #[test]
    fn it_runs_in_real_time_too() {
        let tb: Arc<dyn Timebase> = Arc::new(RealClock);
        let net = SimNet::new(tb, 3);
        let (a, b) = (net.peer(Arc::new(RealClock)), net.peer(Arc::new(RealClock)));
        net.connect(&a, &b, Way::internet(30.0, 0.0), Way::LAN);
        let rx = b.events();
        let t0 = RealClock.now_ns();
        a.send(b.id(), ControlMsg::of(&ToHost::Bye)).unwrap();
        let e = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let TransportEvent::Message { at_ns, .. } = e else {
            panic!("{e:?}");
        };
        assert!(at_ns >= t0 + 30_000_000);
    }
}

#[cfg(test)]
mod peer_tests {
    use super::*;
    use crate::time::{SimClock, Timebase};

    #[test]
    fn a_peer_has_a_key_a_path_and_can_be_closed() {
        let clock = SimClock::new(0);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb, 3);
        let base: Arc<dyn MonoClock> = Arc::new(clock.clone());
        let host = net.peer(base.clone());
        let guest = net.peer(base);
        let (hrx, grx) = (host.events(), guest.events());
        let relayed = Way {
            relay: true,
            ..Way::internet(40.0, 0.0)
        };
        net.connect(&guest, &host, Way::internet(10.0, 0.0), relayed);
        assert_eq!(
            host.peer_key(guest.id()),
            Some(format!("peer{}", guest.id().0))
        );
        assert_eq!(
            host.path(guest.id()),
            Some(PathInfo {
                direct: false,
                relay: true,
                rtt_us: Some(50_000)
            })
        );
        host.close(guest.id());
        let lost = |rx: &Receiver<TransportEvent>, p: PeerId| {
            rx.try_iter().any(|e| e == TransportEvent::Lost { peer: p })
        };
        assert!(
            lost(&hrx, guest.id()) && lost(&grx, host.id()),
            "both told at once"
        );
        assert_eq!(
            guest.send(host.id(), ControlMsg(b"x\n".to_vec())),
            Err(Gone)
        );
        assert_eq!(host.path(guest.id()), None);
        net.restore(&guest, &host);
        assert!(guest.send(host.id(), ControlMsg(b"x\n".to_vec())).is_ok());
    }
}
