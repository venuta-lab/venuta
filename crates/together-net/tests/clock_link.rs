//! The clock's datagram pings end to end (design 4.1, 4.2): the guest's pinger and the host's
//! answer over a `Transport`, feeding the clock: on the simulated network in virtual time
//! (computers whose clocks differ and drift, a change of path, a cut and back), and over iroh
//! on this computer in real time.

mod common;
use common::*;

use std::sync::mpsc::Receiver;
use together::clock::Clock;
use together::sim_net::{SentKind, SimNet, SimTransport, Way};
use together::time::{MonoClock, RealClock, SimClock, SkewedClock, Timebase};
use together::transport::{MediaServer, PeerId, Transport, TransportEvent};
use together_net::clock_link::{Pinger, answer};
use together_net::transport::{HOST, IrohTransport, Timing};

const MS: u64 = 1_000_000;
const S: u64 = 1_000 * MS;

struct Room {
    clock: SimClock,
    net: Arc<SimNet>,
    host: SimTransport,
    guest: SimTransport,
    host_clock: Arc<SkewedClock>,
    guest_clock: Arc<SkewedClock>,
    hrx: Receiver<TransportEvent>,
    grx: Receiver<TransportEvent>,
    pinger: Pinger,
    c: Clock,
}

impl Room {
    fn new(up: Way, down: Way) -> Room {
        let clock = SimClock::new(0);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb, 11);
        let base: Arc<dyn MonoClock> = Arc::new(clock.clone());
        // Two computers: the host's clock 900 s in and 40 ppm fast, the guest's 3 s in, 25 slow.
        let host_clock = Arc::new(SkewedClock::new(base.clone(), 900 * S, 40.0));
        let guest_clock = Arc::new(SkewedClock::new(base, 3 * S, -25.0));
        let host = net.peer(host_clock.clone());
        let guest = net.peer(guest_clock.clone());
        let hrx = host.events();
        let grx = guest.events();
        net.connect(&guest, &host, up, down);
        let pinger = Pinger::new(host.id(), guest_clock.now_ns());
        Room {
            clock,
            net,
            host,
            guest,
            host_clock,
            guest_clock,
            hrx,
            grx,
            pinger,
            c: Clock::new(),
        }
    }

    /// Let `ms` pass, a millisecond at a time: the host answers, the guest takes the answers
    /// and pings when due.
    fn run(&mut self, ms: u64) {
        for _ in 0..ms {
            self.clock.pass(MS);
            for e in self.hrx.try_iter() {
                answer(&self.host, self.host_clock.as_ref(), &e);
            }
            let now = self.guest_clock.now_ns();
            for e in self.grx.try_iter() {
                self.pinger.on_event(&e, &mut self.c, now);
            }
            self.pinger.tick(&self.guest, self.guest_clock.as_ref());
        }
    }

    /// The clock's error now: its host time for the guest's now against the host's true now.
    fn error_us(&self) -> Option<i64> {
        let local = (self.guest_clock.now_ns() / 1_000) as i64;
        let truth = (self.host_clock.now_ns() / 1_000) as i64;
        self.c.host_time(local).map(|h| h - truth)
    }

    /// Pings the guest sent between these network times.
    fn pings_between(&self, from_ns: u64, to_ns: u64) -> usize {
        self.net
            .sent()
            .iter()
            .filter(|s| {
                s.from == self.guest.id()
                    && s.kind == SentKind::Datagram
                    && (from_ns..to_ns).contains(&s.at_ns)
            })
            .count()
    }
}

#[test]
fn on_one_wifi_the_clock_is_ready_within_the_burst_and_well_under_a_millisecond_off() {
    let mut r = Room::new(Way::LAN, Way::LAN);
    r.run(800);
    assert!(!r.c.ready(), "not before the burst has gone 900 ms");
    r.run(400);
    assert!(r.c.ready(), "ready by 1.2 s");
    assert_eq!(r.pings_between(0, 1_000 * MS), 20, "a burst of 20");
    r.run(30_000);
    let e = r.error_us().unwrap();
    assert!(e.abs() < 500, "{e} µs");
    assert!(r.c.map().is_some());
    // Then one a second.
    let now = r.clock.now_ns();
    r.run(10_000);
    let n = r.pings_between(now, now + 10 * S);
    assert!((9..=11).contains(&n), "{n}");
}

#[test]
fn a_change_of_path_starts_a_burst_and_the_clock_follows_the_new_path() {
    let mut r = Room::new(Way::internet(10.0, 1.0), Way::internet(10.0, 1.0));
    r.run(20_000);
    let before = r.error_us().unwrap();
    assert!(before.abs() < 2_000, "{before} µs");
    // To a relay: slower, and slower one way than the other.
    r.net.set_way(
        &r.guest,
        &r.host,
        Way {
            relay: true,
            ..Way::internet(30.0, 2.0)
        },
        Way {
            relay: true,
            ..Way::internet(50.0, 2.0)
        },
    );
    let at = r.clock.now_ns();
    r.run(1_000);
    assert!(
        r.pings_between(at, at + S) >= 19,
        "a burst at once: {}",
        r.pings_between(at, at + S)
    );
    r.run(60_000);
    // The new path's asymmetry (10 ms) is all the clock can't know: within half of it and a bit.
    let after = r.error_us().unwrap();
    assert!(after.abs() < 12_000, "{after} µs");
}

#[test]
fn back_after_a_cut_the_clock_is_told_and_a_burst_follows() {
    let mut r = Room::new(Way::LAN, Way::LAN);
    r.net.set_idle_timeout_ms(2_000);
    r.run(10_000);
    assert!(r.c.ready());
    r.net.cut(&r.guest, &r.host);
    r.run(5_000);
    r.net.restore(&r.guest, &r.host);
    let at = r.clock.now_ns();
    r.run(1_000);
    assert!(r.pings_between(at, at + S) >= 19);
    r.run(10_000);
    assert!(r.error_us().unwrap().abs() < 500);
}

#[derive(Default)]
struct Nothing;
impl MediaServer for Nothing {
    fn open(&self, _: PeerId, _: &together::transport::Get) -> Option<Arc<[u8]>> {
        None
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_iroh_the_clock_is_ready_from_datagram_pings_within_about_a_second() {
    let hep = endpoint_on(false, true, None).await;
    let addr = hep.addr();
    let host = IrohTransport::host(hep, Arc::new(RealClock), Arc::new(Nothing));
    let hrx = host.events();
    let gep = endpoint_on(false, false, Some(key(1))).await;
    let guest = IrohTransport::guest(gep, addr, Arc::new(RealClock), Timing::default())
        .await
        .unwrap();
    let grx = guest.events();
    // The session's hello opens the control stream; the host answers pings from then on.
    guest
        .send(
            HOST,
            together::transport::ControlMsg(b"{\"hello\":1}\n".to_vec()),
        )
        .unwrap();
    let answering = tokio::spawn(async move {
        loop {
            for e in hrx.try_iter() {
                answer(&host, &RealClock, &e);
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    let t = Instant::now();
    let mut p = Pinger::new(HOST, RealClock.now_ns());
    let mut c = Clock::new();
    while !c.ready() {
        assert!(
            t.elapsed() < Duration::from_secs(3),
            "{} samples",
            c.samples()
        );
        for e in grx.try_iter() {
            p.on_event(&e, &mut c, RealClock.now_ns());
        }
        p.tick(&guest, &RealClock);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(
        t.elapsed() < Duration::from_millis(1_800),
        "{:?}",
        t.elapsed()
    );
    // One computer, one clock: the host's time is this one's.
    let now = (RealClock.now_ns() / 1_000) as i64;
    let off = c.host_time(now).unwrap() - now;
    assert!(off.abs() < 1_000, "{off} µs");
    answering.abort();
}
