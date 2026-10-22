//! The clock's pings on any `Transport` (design 4.1, 4.2): the guest's pinger and the host's
//! answer, as plain state driven by transport events and the time, so the same code runs on
//! iroh in real time and on the simulated network in virtual time. The clock math is
//! `together::clock`'s; this only times the pings and turns datagrams into samples.
//!
//! - The guest pings in a burst of 20, 50 ms apart, when it starts, when it is back after a
//!   loss (`Joined`) and when the path changes (`PathChanged`), then once a second. It tells
//!   the clock of the reconnect or the change first, so the new path's samples are fitted
//!   apart.
//! - Its `t1` is its `MonoClock` as it sends, in µs; `t4` the transport's stamp as the answer
//!   was read. The host's `t2` is the transport's stamp as the ping was read, `t3` its clock
//!   as it answers.

use together::clock::{Clock, Sample};
use together::time::MonoClock;
use together::transport::{ClockDatagram, PeerId, Transport, TransportEvent};

/// Pings in a burst, and how far apart (ns).
pub const BURST: u32 = 20;
pub const BURST_EVERY_NS: u64 = 50_000_000;
/// After a burst, one ping this often (ns).
pub const PING_EVERY_NS: u64 = 1_000_000_000;

/// The host's side: answer a ping that came as `e`. True when `e` was one.
pub fn answer(t: &dyn Transport, clock: &dyn MonoClock, e: &TransportEvent) -> bool {
    let TransportEvent::Datagram { from, bytes, at_ns } = e else {
        return false;
    };
    let Some(ClockDatagram::Ping { id, t1 }) = ClockDatagram::decode(bytes) else {
        return false;
    };
    let pong = ClockDatagram::Pong {
        id,
        t1,
        t2: us(*at_ns),
        t3: us(clock.now_ns()),
    };
    let _ = t.send_datagram(*from, pong.encode());
    true
}

/// The guest's pinger.
#[derive(Debug, Clone)]
pub struct Pinger {
    host: PeerId,
    next_id: u32,
    /// Pings left in the burst under way.
    burst: u32,
    next_ns: u64,
}

impl Pinger {
    /// A pinger to `host`, its first burst due at once.
    pub fn new(host: PeerId, now_ns: u64) -> Self {
        Pinger {
            host,
            next_id: 0,
            burst: BURST,
            next_ns: now_ns,
        }
    }

    /// When the next ping is due (ns on the guest's clock).
    pub fn due_ns(&self) -> u64 {
        self.next_ns
    }

    /// Send the ping due now, if one is.
    pub fn tick(&mut self, t: &dyn Transport, local: &dyn MonoClock) {
        let now = local.now_ns();
        if now < self.next_ns {
            return;
        }
        let ping = ClockDatagram::Ping {
            id: self.next_id,
            t1: us(now),
        };
        self.next_id = self.next_id.wrapping_add(1);
        // A ping that can't go (the connection is down) is just a missing sample.
        let _ = t.send_datagram(self.host, ping.encode());
        self.burst = self.burst.saturating_sub(1);
        self.next_ns = now
            + if self.burst > 0 {
                BURST_EVERY_NS
            } else {
                PING_EVERY_NS
            };
    }

    /// Take an event from the host: an answer becomes a sample; back after a loss or on
    /// another path, the clock is told and a burst starts. True when the event was the clock's.
    pub fn on_event(&mut self, e: &TransportEvent, clock: &mut Clock, now_ns: u64) -> bool {
        match e {
            TransportEvent::Datagram { from, bytes, at_ns } if *from == self.host => {
                let Some(ClockDatagram::Pong { t1, t2, t3, .. }) = ClockDatagram::decode(bytes)
                else {
                    return false;
                };
                clock.add(Sample {
                    t1,
                    t2,
                    t3,
                    t4: us(*at_ns),
                });
                true
            }
            TransportEvent::Joined { peer } if *peer == self.host => {
                clock.reconnected(us(now_ns));
                self.start_burst(now_ns);
                true
            }
            TransportEvent::PathChanged { peer, .. } if *peer == self.host => {
                clock.path_changed(us(now_ns));
                self.start_burst(now_ns);
                true
            }
            _ => false,
        }
    }

    fn start_burst(&mut self, now_ns: u64) {
        self.burst = BURST;
        self.next_ns = now_ns;
    }
}

fn us(ns: u64) -> i64 {
    (ns / 1_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use together::transport::{BoxFuture, ControlMsg, DATAGRAM_LEN, Get, Gone, MediaStream};

    struct At(AtomicU64);
    impl MonoClock for At {
        fn now_ns(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    /// Records the datagrams sent, with the time.
    struct Sends<'a>(&'a At, Mutex<Vec<(u64, ClockDatagram)>>);
    impl Transport for Sends<'_> {
        fn send(&self, _: PeerId, _: ControlMsg) -> Result<(), Gone> {
            Ok(())
        }
        fn send_datagram(&self, _: PeerId, b: [u8; DATAGRAM_LEN]) -> Result<(), Gone> {
            let d = ClockDatagram::decode(&b).unwrap();
            self.1.lock().unwrap().push((self.0.now_ns(), d));
            Ok(())
        }
        fn open_media(&self, _: PeerId, _: Get) -> BoxFuture<'static, Result<MediaStream, Gone>> {
            Box::pin(async { Err(Gone) })
        }
        fn events(&self) -> std::sync::mpsc::Receiver<TransportEvent> {
            std::sync::mpsc::channel().1
        }
    }

    /// Run a pinger for `ms`, ticking each millisecond.
    fn run(p: &mut Pinger, t: &Sends, clock: &At, ms: u64) {
        for _ in 0..ms {
            p.tick(t, clock);
            clock.0.fetch_add(1_000_000, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_burst_of_twenty_fifty_ms_apart_then_one_a_second_and_a_new_burst_on_a_change() {
        let clock = At(AtomicU64::new(5_000_000_000));
        let t = Sends(&clock, Mutex::default());
        let mut p = Pinger::new(PeerId(0), clock.now_ns());
        run(&mut p, &t, &clock, 3_500);
        let at: Vec<u64> = t.1.lock().unwrap().iter().map(|(a, _)| *a).collect();
        assert_eq!(at.len(), 22, "20 in the burst, then at 1.95 s and 2.95 s");
        assert_eq!(at[19] - at[0], 19 * BURST_EVERY_NS);
        assert!(
            at[20..]
                .iter()
                .zip(&at[19..])
                .all(|(b, a)| b - a == PING_EVERY_NS)
        );
        let Some((sent, ClockDatagram::Ping { id: 3, t1 })) = t.1.lock().unwrap().get(3).copied()
        else {
            panic!("pings numbered from 0")
        };
        assert_eq!(t1, (sent / 1_000) as i64, "t1 is the guest's clock in µs");
        // A change of path: the clock is told, a burst at once.
        let mut c = Clock::new();
        t.1.lock().unwrap().clear();
        let change = TransportEvent::PathChanged {
            peer: PeerId(0),
            path: together::transport::PathInfo {
                direct: true,
                relay: false,
                rtt_us: Some(1_000),
            },
        };
        assert!(p.on_event(&change, &mut c, clock.now_ns()));
        run(&mut p, &t, &clock, 1_000);
        assert_eq!(t.1.lock().unwrap().len(), 20);
        // Back after a loss: the same.
        t.1.lock().unwrap().clear();
        run(&mut p, &t, &clock, 1_500);
        assert!(t.1.lock().unwrap().len() <= 2);
        t.1.lock().unwrap().clear();
        assert!(p.on_event(
            &TransportEvent::Joined { peer: PeerId(0) },
            &mut c,
            clock.now_ns()
        ));
        run(&mut p, &t, &clock, 1_000);
        assert_eq!(t.1.lock().unwrap().len(), 20);
    }

    #[test]
    fn an_answer_is_a_sample_and_others_are_not_the_clocks() {
        let mut p = Pinger::new(PeerId(0), 0);
        let mut c = Clock::new();
        let pong = ClockDatagram::Pong {
            id: 1,
            t1: 1_000,
            t2: 9_000_500,
            t3: 9_000_600,
        };
        let e = TransportEvent::Datagram {
            from: PeerId(0),
            bytes: pong.encode(),
            at_ns: 2_100_000,
        };
        assert!(p.on_event(&e, &mut c, 2_100_000));
        assert_eq!(c.samples(), 1);
        let other = TransportEvent::Datagram {
            from: PeerId(5),
            bytes: pong.encode(),
            at_ns: 2_100_000,
        };
        assert!(!p.on_event(&other, &mut c, 2_100_000), "not from the host");
        assert_eq!(c.samples(), 1);
    }

    #[test]
    fn the_host_answers_with_when_it_read_the_ping_and_when_it_answers() {
        let clock = At(AtomicU64::new(7_000_000_000));
        let t = Sends(&clock, Mutex::default());
        let ping = TransportEvent::Datagram {
            from: PeerId(3),
            bytes: ClockDatagram::Ping { id: 9, t1: 42 }.encode(),
            at_ns: 6_999_000_000,
        };
        assert!(answer(&t, &clock, &ping));
        assert_eq!(
            t.1.lock().unwrap()[0].1,
            ClockDatagram::Pong {
                id: 9,
                t1: 42,
                t2: 6_999_000,
                t3: 7_000_000,
            }
        );
        let msg = TransportEvent::Joined { peer: PeerId(3) };
        assert!(!answer(&t, &clock, &msg));
    }
}
