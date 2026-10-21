//! The clock's gates (design 9.2, WP6), in simulated time: a guest's clock against a host's
//! over simulated paths (asymmetry, jitter, loss, bufferbloat, a change of path, a sleep, a
//! cut), each scenario checked every 100 ms against the true host time.
//!
//! The gates: the error is within the published bound in at least 99% of the checks; a new
//! epoch when the old line no longer describes the clocks; after a change of path, a lasting
//! jump is stepped after 2 s, not before; never a step for bufferbloat. Deterministic (seeded),
//! no real time, so the results don't depend on how busy the machine is.

use together::clock::{Clock, Sample, Steps};
use together::clock_map::{ClockMap, ClockMapHandle};

const MS: f64 = 1_000.0;
const S: f64 = 1_000_000.0;

/// Where the guest's monotonic clock was at the start (it booted 1000 s before).
const LOCAL0: f64 = 1_000.0 * S;
/// Where the host's was (it booted a few days before).
const HOST0: f64 = 300_000.0 * S;
/// The host's time to answer a ping, between its two stamps.
const ANSWER_US: f64 = 50.0;

/// A small deterministic random source (xorshift64*).
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    fn exp(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.unit()).ln()
    }
}

/// One direction's way through the network at some moment (microseconds).
#[derive(Clone, Copy, Debug)]
struct Leg {
    base: f64,
    /// Mean of an exponential extra delay on every packet.
    jitter: f64,
    /// The share of packets that are much later (Wi-Fi retries, a busy router).
    spikes: f64,
    spike_max: f64,
    /// A standing queue (bufferbloat) in front of this direction.
    queue: f64,
}

impl Leg {
    fn new(base_ms: f64, jitter_ms: f64) -> Self {
        Leg {
            base: base_ms * MS,
            jitter: jitter_ms * MS,
            spikes: 0.0,
            spike_max: 0.0,
            queue: 0.0,
        }
    }

    fn spiky(mut self, share: f64, max_ms: f64) -> Self {
        self.spikes = share;
        self.spike_max = max_ms * MS;
        self
    }

    fn delay(&self, rng: &mut Rng) -> f64 {
        let mut d = self.base + self.queue + rng.exp(self.jitter);
        if rng.unit() < self.spikes {
            d += rng.unit() * self.spike_max;
        }
        d
    }
}

#[derive(Clone, Copy, Debug)]
struct Path {
    up: Leg,
    down: Leg,
    loss: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Who {
    Guest,
    Host,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    /// The transport says the selected path changed: the clock is told, and a burst follows.
    PathChanged,
    /// The connection came back: the clock is told, and a burst follows.
    Reconnected,
    /// The session starts a new epoch itself (a wake it saw).
    NewEpoch,
}

struct World {
    /// How much faster the host's clock runs, parts per million.
    skew_ppm: f64,
    /// (start, length) in true time (µs), and whose monotonic clock stops meanwhile (an
    /// Android phone's `CLOCK_MONOTONIC` doesn't count a suspend). The sleeper sends and
    /// answers nothing while asleep.
    sleeps: Vec<(f64, f64, Who, bool)>,
    path: Box<dyn Fn(f64) -> Path>,
    events: Vec<(f64, Event)>,
    steps: Steps,
    secs: f64,
    seed: u64,
}

impl World {
    fn new(secs: f64, steps: Steps, path: impl Fn(f64) -> Path + 'static) -> Self {
        World {
            skew_ppm: 0.0,
            sleeps: Vec::new(),
            path: Box::new(path),
            events: Vec::new(),
            steps,
            secs,
            seed: 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn skew(mut self, ppm: f64) -> Self {
        self.skew_ppm = ppm;
        self
    }

    fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// `who` sleeps from `at_s` for `len_s`; `stops` whether its monotonic clock stops too.
    fn sleep(mut self, who: Who, at_s: f64, len_s: f64, stops: bool) -> Self {
        self.sleeps.push((at_s * S, len_s * S, who, stops));
        self
    }

    fn event(mut self, at_s: f64, e: Event) -> Self {
        self.events.push((at_s * S, e));
        self
    }

    fn asleep(&self, who: Who, tau: f64) -> bool {
        self.sleeps
            .iter()
            .any(|&(s, l, w, _)| w == who && tau >= s && tau < s + l)
    }

    /// How long `who`'s clock has been stopped by `tau`.
    fn paused(&self, who: Who, tau: f64) -> f64 {
        self.sleeps
            .iter()
            .filter(|&&(_, _, w, stops)| w == who && stops)
            .map(|&(s, l, _, _)| (tau - s).clamp(0.0, l))
            .sum()
    }

    fn local(&self, tau: f64) -> f64 {
        LOCAL0 + tau - self.paused(Who::Guest, tau)
    }

    fn host(&self, tau: f64) -> f64 {
        HOST0 + (tau - self.paused(Who::Host, tau)) * (1.0 + self.skew_ppm * 1e-6)
    }
}

#[derive(Clone, Copy, Debug)]
struct Check {
    at_s: f64,
    err_us: f64,
    bound_us: u32,
    epoch: u32,
}

#[derive(Default)]
struct Run {
    checks: Vec<Check>,
    /// When a map was first there in each epoch (true seconds), and which epoch.
    ready: Vec<(f64, u32)>,
    /// When the clock stepped (true seconds).
    steps: Vec<f64>,
    /// Every map published, by when (true seconds) and the local time then (µs).
    maps: Vec<(f64, f64, ClockMap)>,
    /// How many checks found no map while awake.
    unmapped: usize,
}

impl Run {
    fn checks_from(&self, from_s: f64) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(move |c| c.at_s >= from_s)
    }

    /// The share of checks (from `from_s` on) whose error is within the published bound.
    fn within_bound(&self, from_s: f64) -> f64 {
        let all: Vec<_> = self.checks_from(from_s).collect();
        assert!(!all.is_empty(), "no checks from {from_s} s");
        let ok = all
            .iter()
            .filter(|c| c.err_us.abs() <= c.bound_us as f64)
            .count();
        ok as f64 / all.len() as f64
    }

    fn percentile_err(&self, from_s: f64, p: f64) -> f64 {
        let mut e: Vec<f64> = self.checks_from(from_s).map(|c| c.err_us.abs()).collect();
        assert!(!e.is_empty());
        e.sort_by(f64::total_cmp);
        e[((e.len() as f64 * p).ceil() as usize).clamp(1, e.len()) - 1]
    }

    fn max_err(&self, from_s: f64, to_s: f64) -> f64 {
        self.checks
            .iter()
            .filter(|c| c.at_s >= from_s && c.at_s < to_s)
            .map(|c| c.err_us.abs())
            .fold(0.0, f64::max)
    }

    fn mean_bound(&self, from_s: f64) -> f64 {
        let b: Vec<f64> = self
            .checks_from(from_s)
            .map(|c| c.bound_us as f64)
            .collect();
        b.iter().sum::<f64>() / b.len() as f64
    }

    fn summary(&self, from_s: f64) -> String {
        let s = format!(
            "within bound {:.4}, p50 {:.0} µs, p95 {:.0} µs, p99 {:.0} µs, mean bound {:.0} µs, steps {:?}, ready {:?}",
            self.within_bound(from_s),
            self.percentile_err(from_s, 0.5),
            self.percentile_err(from_s, 0.95),
            self.percentile_err(from_s, 0.99),
            self.mean_bound(from_s),
            self.steps,
            self.ready,
        );
        // Seen with --nocapture: how close each scenario is to its gates.
        println!("{s}");
        s
    }
}

/// Runs the world: pings (a burst of 20 at 50 ms on connect, on a change of path, a reconnect
/// or a wake, else one a second), their answers fed to the clock as they arrive, the events,
/// and a check every 100 ms.
fn run(w: &World) -> Run {
    let mut rng = Rng(w.seed);
    let mut clock = Clock::with_steps(w.steps);
    let handle: ClockMapHandle = Default::default();
    let end = w.secs * S;
    let mut out = Run::default();

    let mut burst = 20u32;
    let mut next_ping = 0.0f64;
    let mut next_check = 0.0f64;
    // (arrival, sample), kept sorted by arrival.
    let mut flying: Vec<(f64, Sample)> = Vec::new();
    let mut events = w.events.clone();
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut events = events.into_iter().peekable();
    let mut was_asleep = false;
    let mut last_epoch: Option<u32> = None;
    let mut last_steps = 0;

    loop {
        let next_arrival = flying.first().map(|f| f.0).unwrap_or(f64::INFINITY);
        let next_event = events.peek().map(|e| e.0).unwrap_or(f64::INFINITY);
        let tau = next_ping.min(next_check).min(next_arrival).min(next_event);
        if tau >= end {
            break;
        }
        let guest_asleep = w.asleep(Who::Guest, tau);
        if was_asleep && !guest_asleep {
            // Awake again: a burst, as on a reconnect.
            burst = 20;
            next_ping = tau;
        }
        was_asleep = guest_asleep;

        if tau == next_event {
            let (_, e) = events.next().unwrap();
            let local = w.local(tau) as i64;
            match e {
                Event::PathChanged => clock.path_changed(local),
                Event::Reconnected => clock.reconnected(local),
                Event::NewEpoch => clock.new_epoch(),
            }
            if e != Event::NewEpoch {
                burst = 20;
                next_ping = tau;
            }
            continue;
        }
        if tau == next_arrival {
            let (_, s) = flying.remove(0);
            if !guest_asleep {
                clock.add(s);
                clock.publish(&handle);
                if clock.steps_taken() != last_steps {
                    last_steps = clock.steps_taken();
                    out.steps.push(tau / S);
                }
                if let Some(m) = **handle.load() {
                    out.maps.push((tau / S, w.local(tau), m));
                    if last_epoch != Some(m.epoch) {
                        last_epoch = Some(m.epoch);
                        out.ready.push((tau / S, m.epoch));
                    }
                }
            }
            continue;
        }
        if tau == next_ping {
            if !guest_asleep {
                let p = (w.path)(tau);
                let up = p.up.delay(&mut rng);
                let down = p.down.delay(&mut rng);
                let lost = rng.unit() < p.loss || rng.unit() < p.loss;
                let at_host = tau + up;
                if !lost && !w.asleep(Who::Host, at_host) {
                    let t1 = w.local(tau);
                    let t2 = w.host(at_host);
                    let t3 = w.host(at_host + ANSWER_US);
                    let back = at_host + ANSWER_US + down;
                    let t4 = w.local(back);
                    let s = Sample {
                        t1: t1 as i64,
                        t2: t2 as i64,
                        t3: t3 as i64,
                        t4: t4 as i64,
                    };
                    let i = flying.partition_point(|f| f.0 <= back);
                    flying.insert(i, (back, s));
                }
            }
            burst = burst.saturating_sub(1);
            next_ping = tau + if burst > 0 { 50.0 * MS } else { S };
            continue;
        }
        // A check.
        next_check = tau + 100.0 * MS;
        // Nobody plays while asleep; and while the host sleeps its clock has no time to match.
        if guest_asleep || w.asleep(Who::Host, tau) {
            continue;
        }
        match **handle.load() {
            Some(m) => {
                let local_ns = (w.local(tau) * 1_000.0) as u64;
                let err = m.host_of(local_ns) as f64 - w.host(tau);
                out.checks.push(Check {
                    at_s: tau / S,
                    err_us: err,
                    bound_us: m.bound_us,
                    epoch: m.epoch,
                });
            }
            None => out.unmapped += 1,
        }
    }
    out
}

fn same_wifi(_: f64) -> Path {
    Path {
        up: Leg::new(1.5, 0.4).spiky(0.1, 30.0),
        down: Leg::new(1.5, 0.4).spiky(0.1, 30.0),
        loss: 0.02,
    }
}

/// The design's internet case (7.2): 20 ms up, 40 ms down, 10 ms of jitter, 1% loss.
fn internet(_: f64) -> Path {
    Path {
        up: Leg::new(20.0, 3.0).spiky(0.05, 40.0),
        down: Leg::new(40.0, 3.0).spiky(0.05, 40.0),
        loss: 0.01,
    }
}

fn relay(_: f64) -> Path {
    Path {
        up: Leg::new(80.0, 3.0).spiky(0.05, 60.0),
        down: Leg::new(80.0, 3.0).spiky(0.05, 60.0),
        loss: 0.01,
    }
}

/// Direct, 25 ms of offset bias against the relay (60 ms of asymmetry, 70 ms round trip).
fn direct(_: f64) -> Path {
    Path {
        up: Leg::new(5.0, 2.0).spiky(0.05, 40.0),
        down: Leg::new(65.0, 2.0).spiky(0.05, 40.0),
        loss: 0.01,
    }
}

// ---------------------------------------------------------------- the line, and its math

#[test]
fn the_map_turns_times_both_ways() {
    let m = ClockMap {
        epoch: 3,
        local_ref_ns: 7_000_000_000_000,
        host_ref_us: 250_000_000_000,
        rate: 1.0 + 80e-6,
        bound_us: 900,
    };
    assert_eq!(m.host_of(m.local_ref_ns), m.host_ref_us);
    // 100 s later on this computer the host has gone 100 s and 8 ms.
    assert_eq!(
        m.host_of(m.local_ref_ns + 100_000_000_000),
        m.host_ref_us + 100_008_000
    );
    // And back, to the microsecond, before the reference too.
    for d in [-3_600_000_000i64, -1, 0, 1, 12_345_678, 86_400_000_000] {
        let host = m.host_ref_us + d;
        let local = m.local_of(host);
        assert!((m.host_of(local) - host).abs() <= 1, "{d}");
    }
    // The host's own map is the identity, exactly, whatever the uptime.
    let id = ClockMap::identity();
    for ns in [
        0u64,
        999,
        1_000,
        123_456_789_012_345,
        40 * 86_400 * 1_000_000_000_000,
    ] {
        assert_eq!(id.host_of(ns), (ns / 1_000) as i64);
        assert_eq!(id.local_of((ns / 1_000) as i64), ns / 1_000 * 1_000);
    }
}

#[test]
fn ready_after_eight_samples_and_the_burst() {
    // The burst: 20 pings 50 ms apart, 2 ms round trip.
    let ping = |i: i64| {
        let t1 = 1_000_000_000 + i * 50_000;
        Sample {
            t1,
            t2: t1 + 5_000_000 + 1_000,
            t3: t1 + 5_000_000 + 1_050,
            t4: t1 + 2_050,
        }
    };
    let mut c = Clock::new();
    for i in 0..18 {
        c.add(ping(i));
        assert!(
            !c.ready(),
            "not ready at {} samples over {} ms",
            i + 1,
            i * 50
        );
        assert_eq!(c.map(), None);
    }
    c.add(ping(18));
    assert!(c.ready(), "ready once the burst has gone 900 ms");
    let m = c.map().unwrap();
    assert!((m.host_of(m.local_ref_ns) - (m.local_ref_ns / 1_000) as i64 - 5_000_000).abs() <= 1);
    // A few samples over a long time are not enough either.
    let mut c = Clock::new();
    for i in 0..7 {
        c.add(ping(i * 20));
    }
    assert!(!c.ready(), "7 samples over 7 s");
    c.add(ping(140));
    assert!(c.ready());
}

#[test]
fn the_map_is_published_to_the_handle() {
    let handle: ClockMapHandle = Default::default();
    let mut c = Clock::new();
    c.publish(&handle);
    assert_eq!(**handle.load(), None);
    for i in 0..20 {
        let t1 = 2_000_000_000 + i * 50_000;
        c.add(Sample {
            t1,
            t2: t1 - 7_000 + 500,
            t3: t1 - 7_000 + 550,
            t4: t1 + 1_050,
        });
    }
    c.publish(&handle);
    assert_eq!(**handle.load(), c.map());
    assert!(c.map().is_some());
}

// ---------------------------------------------------------------- the gates, by scenario

#[test]
fn one_wifi_is_within_its_bound_and_well_under_a_millisecond() {
    for seed in [1, 2, 3] {
        let w = World::new(600.0, Steps::SameRoom, same_wifi)
            .skew(-60.0)
            .seed(seed);
        let r = run(&w);
        let s = r.summary(0.0);
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        assert!(r.percentile_err(5.0, 0.95) <= 250.0, "seed {seed}: {s}");
        assert!(r.max_err(5.0, 600.0) <= 1_000.0, "seed {seed}: {s}");
        assert!(r.steps.is_empty(), "seed {seed}: {s}");
        // Gate 7.3: ready within 1 s of connecting on one Wi-Fi.
        assert!(r.ready[0].0 <= 1.0, "seed {seed}: {s}");
        assert_eq!(r.ready.len(), 1, "one epoch: {s}");
    }
}

#[test]
fn an_asymmetric_internet_path_is_within_its_bound() {
    for seed in [1, 2, 3] {
        let w = World::new(600.0, Steps::Apart, internet)
            .skew(80.0)
            .seed(seed);
        let r = run(&w);
        let s = r.summary(0.0);
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        // The honest floor is half the path asymmetry (10 ms); design 7.2 allows 5 ms more.
        assert!(r.percentile_err(5.0, 0.95) <= 15_000.0, "seed {seed}: {s}");
        assert!(r.steps.is_empty(), "seed {seed}: {s}");
        // Gate 7.3: ready within 1.5 s over the internet.
        assert!(r.ready[0].0 <= 1.5, "seed {seed}: {s}");
    }
}

#[test]
fn heavy_jitter_is_within_its_bound() {
    let path = |_| Path {
        up: Leg::new(30.0, 15.0).spiky(0.2, 200.0),
        down: Leg::new(30.0, 15.0).spiky(0.2, 200.0),
        loss: 0.05,
    };
    for seed in [1, 2, 3] {
        let w = World::new(600.0, Steps::Apart, path).skew(120.0).seed(seed);
        let r = run(&w);
        let s = r.summary(0.0);
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        assert!(r.percentile_err(5.0, 0.95) <= 10_000.0, "seed {seed}: {s}");
        assert!(r.steps.is_empty(), "seed {seed}: {s}");
    }
}

#[test]
fn the_relay_is_within_its_bound_and_ready_in_two_seconds() {
    for seed in [1, 2, 3] {
        let w = World::new(300.0, Steps::Apart, relay)
            .skew(-150.0)
            .seed(seed);
        let r = run(&w);
        let s = r.summary(0.0);
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        assert!(r.ready[0].0 <= 2.0, "seed {seed}: {s}");
        assert!(r.steps.is_empty(), "seed {seed}: {s}");
    }
}

#[test]
fn bufferbloat_is_never_followed_or_stepped_to() {
    // Someone starts an upload two minutes in: a 400 ms queue builds up in front of the way
    // to the host within 5 s, stays for 3 minutes (longer than the window of samples), drains.
    let path = |tau: f64| {
        let t = tau / S;
        let queue = if (120.0..305.0).contains(&t) {
            ((t - 120.0) / 5.0).min(1.0) * 400.0 * MS
        } else {
            0.0
        };
        let mut up = Leg::new(4.0, 1.0).spiky(0.05, 20.0);
        up.queue = queue;
        Path {
            up,
            down: Leg::new(4.0, 1.0).spiky(0.05, 20.0),
            loss: 0.01,
        }
    };
    for seed in [1, 2, 3] {
        let w = World::new(500.0, Steps::SameRoom, path)
            .skew(40.0)
            .seed(seed);
        let r = run(&w);
        let s = r.summary(0.0);
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        assert!(r.steps.is_empty(), "seed {seed}: {s}");
        // The error during the upload stays what it was before it (within a millisecond),
        // not the 200 ms half of the queue would make it.
        let quiet = r.max_err(30.0, 120.0);
        let during = r.max_err(120.0, 320.0);
        assert!(
            during <= quiet + 1_000.0,
            "seed {seed}: {quiet} then {during}; {s}"
        );
    }
}

/// When the clock stepped after a change of path at `change_s`, and what it read meanwhile.
fn step_after(r: &Run, change_s: f64) -> f64 {
    let after: Vec<f64> = r.steps.iter().copied().filter(|&t| t >= change_s).collect();
    assert_eq!(after.len(), 1, "one step after the change: {:?}", r.steps);
    after[0] - change_s
}

/// How far the map moved (at the local time of each map) between `from_s` and `to_s`, the
/// host's rate taken out: the most any two maps disagree about the same moment.
fn moved_between(r: &Run, from_s: f64, to_s: f64) -> f64 {
    let maps: Vec<_> = r
        .maps
        .iter()
        .filter(|(t, _, _)| *t >= from_s && *t < to_s)
        .collect();
    let (_, local, first) = maps[0];
    let at = (*local * 1_000.0) as u64;
    maps.iter()
        .map(|(_, _, m)| (m.host_of(at) - first.host_of(at)).abs() as f64)
        .fold(0.0, f64::max)
}

#[test]
fn a_change_of_path_to_a_quicker_one_is_stepped_after_two_seconds_not_before() {
    let change = 200.0;
    for seed in [1, 2, 3] {
        let w = World::new(320.0, Steps::Apart, move |tau| {
            if tau < change * S {
                relay(tau)
            } else {
                direct(tau)
            }
        })
        .skew(60.0)
        .seed(seed)
        .event(change, Event::PathChanged);
        let r = run(&w);
        let s = r.summary(0.0);
        let after = step_after(&r, change);
        assert!(
            (2.0..3.5).contains(&after),
            "stepped {after:.2} s after the change; {s}"
        );
        // Before the step, only slewed: 50 µs a second at most.
        let moved = moved_between(&r, change - 1.0, change + after - 0.01);
        assert!(
            moved <= 50.0 * (after + 1.0) + 2.0,
            "moved {moved:.0} µs before the step; {s}"
        );
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        // On the quicker path the error is what that path allows (half its asymmetry, 30 ms,
        // plus a little), and inside the band within 30 s (gate 7.4).
        assert!(
            r.max_err(change + 30.0, 320.0) <= 35_000.0,
            "seed {seed}: {s}"
        );
    }
}

#[test]
fn an_unannounced_change_to_a_quicker_path_is_stepped_after_ten_seconds() {
    let change = 200.0;
    let w = World::new(300.0, Steps::Apart, move |tau| {
        if tau < change * S {
            relay(tau)
        } else {
            direct(tau)
        }
    })
    .skew(60.0);
    let r = run(&w);
    let s = r.summary(0.0);
    let after = step_after(&r, change);
    assert!(
        (10.0..12.5).contains(&after),
        "stepped {after:.2} s after; {s}"
    );
    assert!(r.within_bound(0.0) >= 0.99, "{s}");
}

#[test]
fn a_change_to_a_slower_path_keeps_the_better_founded_line() {
    let change = 200.0;
    for seed in [1, 2, 3] {
        let w = World::new(400.0, Steps::Apart, move |tau| {
            if tau < change * S {
                direct(tau)
            } else {
                relay(tau)
            }
        })
        .skew(60.0)
        .seed(seed)
        .event(change, Event::PathChanged);
        let r = run(&w);
        let s = r.summary(0.0);
        assert!(r.steps.is_empty(), "seed {seed}: {s}");
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
    }
}

#[test]
fn a_small_change_of_path_is_never_stepped() {
    // The same room: onto a quicker path whose offset reads 4 ms away (under the 5 ms step):
    // taken up by slewing, 50 µs a second, never stepped.
    let change = 120.0;
    let w = World::new(300.0, Steps::SameRoom, move |tau| {
        if tau < change * S {
            Path {
                up: Leg::new(1.5, 0.4).spiky(0.1, 30.0),
                down: Leg::new(9.5, 0.4).spiky(0.1, 30.0),
                loss: 0.02,
            }
        } else {
            same_wifi(tau)
        }
    })
    .event(change, Event::PathChanged);
    let r = run(&w);
    let s = r.summary(0.0);
    assert!(r.steps.is_empty(), "{s}");
    assert!(r.within_bound(0.0) >= 0.99, "{s}");
    // Slewing: 50 µs a second for the 180 s after the change, about 4 ms in all.
    let moved = moved_between(&r, change - 1.0, 300.0);
    assert!(
        (3_000.0..=50.0 * 181.0 + 2.0).contains(&moved),
        "moved {moved:.0} µs; {s}"
    );
}

#[test]
fn a_reconnect_steps_a_lasting_jump_after_two_seconds() {
    let change = 150.0;
    let w = World::new(250.0, Steps::Apart, move |tau| {
        if tau < change * S {
            relay(tau)
        } else {
            direct(tau)
        }
    })
    .skew(-30.0)
    .event(change, Event::Reconnected);
    let r = run(&w);
    let s = r.summary(0.0);
    let after = step_after(&r, change);
    assert!(
        (2.0..3.5).contains(&after),
        "stepped {after:.2} s after; {s}"
    );
    assert!(r.within_bound(0.0) >= 0.99, "{s}");
}

// ---------------------------------------------------------------- sleeps and epochs

#[test]
fn a_phone_that_slept_starts_a_new_epoch_and_is_ready_again_in_a_second() {
    // The guest's clock stops while it sleeps (Android's CLOCK_MONOTONIC): on waking its
    // clock is 5 minutes behind the host's, against what the line says.
    for seed in [1, 2] {
        let w = World::new(600.0, Steps::SameRoom, same_wifi)
            .skew(70.0)
            .seed(seed)
            .sleep(Who::Guest, 200.0, 300.0, true);
        let r = run(&w);
        let s = r.summary(0.0);
        assert_eq!(r.ready.len(), 2, "a second epoch: {s}");
        let (woke, epoch) = r.ready[1];
        assert_eq!(epoch, r.ready[0].1 + 1, "{s}");
        assert!(
            (500.0..501.0).contains(&woke),
            "ready again {woke} s, within a second of waking; {s}"
        );
        // Nothing from the old epoch is shown once the first answer after waking is in.
        assert!(
            r.checks_from(500.05).all(|c| c.epoch == epoch),
            "old epoch shown after waking; {s}"
        );
        assert!(r.within_bound(0.0) >= 0.99, "seed {seed}: {s}");
        assert!(r.percentile_err(502.0, 0.95) <= 250.0, "seed {seed}: {s}");
        assert!(r.steps.is_empty(), "an epoch is not a step: {s}");
    }
}

#[test]
fn a_host_that_slept_starts_a_new_epoch() {
    // The host's clock stops for a minute; the guest's goes on and keeps pinging (unanswered).
    let w = World::new(400.0, Steps::Apart, internet)
        .skew(-40.0)
        .sleep(Who::Host, 150.0, 60.0, true)
        .event(211.0, Event::Reconnected);
    let r = run(&w);
    let s = r.summary(0.0);
    assert_eq!(r.ready.len(), 2, "{s}");
    assert!(r.ready[1].0 < 213.0, "{s}");
    assert!(r.within_bound(0.0) >= 0.99, "{s}");
}

#[test]
fn a_computer_whose_clock_counts_sleep_gets_its_new_epoch_from_the_session() {
    // Windows' QPC counts a sleep, so the line still holds; the session starts the new epoch
    // on waking anyway (it dropped every buffered sample), and the clock is ready again after
    // the burst.
    let w = World::new(400.0, Steps::SameRoom, same_wifi)
        .skew(30.0)
        .sleep(Who::Guest, 100.0, 120.0, false)
        .event(220.0, Event::NewEpoch);
    let r = run(&w);
    let s = r.summary(0.0);
    assert_eq!(r.ready.len(), 2, "{s}");
    assert!((220.0..221.0).contains(&r.ready[1].0), "{s}");
    assert!(r.within_bound(0.0) >= 0.99, "{s}");
}

/// A ping at local `at` (µs) whose answer says the host is `offset` ahead, `rtt` round trip.
fn ping(at: i64, offset: i64, rtt: i64) -> Sample {
    Sample {
        t1: at,
        t2: at + rtt / 2 + offset,
        t3: at + rtt / 2 + offset + 50,
        t4: at + rtt + 50,
    }
}

fn settled(offset: i64) -> Clock {
    let mut c = Clock::with_steps(Steps::Apart);
    for i in 0..20 {
        c.add(ping(1_000_000_000 + i * 50_000, offset, 4_000));
    }
    for i in 1..40 {
        c.add(ping(1_001_000_000 + i * 1_000_000, offset, 4_000));
    }
    assert!(c.ready());
    c
}

#[test]
fn a_sample_far_off_the_line_starts_a_new_epoch() {
    let mut c = settled(10_000_000);
    let e = c.epoch();
    let at = 1_100_000_000;
    // An answer 2 s late whose offset reads 900 ms off: its own timestamps allow the line,
    // so it says nothing against it.
    c.add(ping(at - 2_000_000, 10_900_000, 2_000_000));
    assert_eq!(c.epoch(), e);
    assert_eq!(c.steps_taken(), 0);
    // 400 ms off, quickly: the line is proven wrong, but not by enough for a new epoch, and
    // it isn't taken up at once (a lasting jump is stepped after 2 s).
    c.add(ping(at, 10_400_000, 4_000));
    assert_eq!(c.epoch(), e);
    let m = c.map().unwrap();
    assert!(
        (m.host_of(m.local_ref_ns) - (m.local_ref_ns / 1_000) as i64 - 10_000_000).abs() < 1_000
    );
    // 600 ms off, quickly: the line no longer describes the clocks.
    c.add(ping(at + 2_000_000, 10_600_000, 4_000));
    assert_eq!(c.epoch(), e + 1, "a new epoch");
    assert_eq!(c.map(), None, "not ready until the new epoch has its burst");
    assert_eq!(c.samples(), 1, "the window cleared");
    for i in 1..20 {
        c.add(ping(at + 2_000_000 + i * 50_000, 10_600_000, 4_000));
    }
    let m = c.map().expect("ready again");
    assert_eq!(m.epoch, e + 1);
    let local = 1_102_950_000u64 * 1_000;
    let off = m.host_of(local) - (local / 1_000) as i64;
    assert!((off - 10_600_000).abs() < 100, "offset {off}, {m:?}");
}

#[test]
fn a_new_epoch_by_hand_clears_the_window() {
    let mut c = settled(-3_000_000);
    let e = c.epoch();
    c.new_epoch();
    assert_eq!(c.epoch(), e + 1);
    assert_eq!(c.map(), None);
    assert_eq!(c.samples(), 0);
    assert!(!c.ready());
}

// ---------------------------------------------------------------- the line between samples

#[test]
fn a_cut_drifts_under_a_millisecond_a_minute() {
    // Three minutes of pings, then two minutes of nothing: the line carries on by its rate.
    let path = |tau: f64| {
        if tau < 180.0 * S {
            same_wifi(tau)
        } else {
            Path {
                loss: 1.0,
                ..same_wifi(tau)
            }
        }
    };
    let w = World::new(300.0, Steps::SameRoom, path).skew(100.0);
    let r = run(&w);
    let s = r.summary(0.0);
    let at_cut = r.max_err(179.0, 180.0);
    let after = r.max_err(299.0, 300.0);
    assert!(
        after - at_cut <= 2_000.0,
        "{at_cut:.0} µs at the cut, {after:.0} µs two minutes later; {s}"
    );
}

#[test]
fn successive_maps_never_jump_more_than_the_slew() {
    let w = World::new(300.0, Steps::SameRoom, same_wifi).skew(-90.0);
    let r = run(&w);
    assert!(r.maps.len() > 250, "{} maps", r.maps.len());
    for pair in r.maps.windows(2) {
        let (t0, _, a) = pair[0];
        let (t1, local, b) = pair[1];
        if a.epoch != b.epoch {
            continue;
        }
        let at = (local * 1_000.0) as u64;
        let jump = (b.host_of(at) - a.host_of(at)).abs() as f64;
        // 50 µs per second since the last map (2 s at most), and two microseconds of rounding.
        let allowed = 50.0 * (t1 - t0).min(2.0) + 2.0;
        assert!(jump <= allowed, "jumped {jump:.1} µs in {:.3} s", t1 - t0);
    }
}

#[test]
fn the_hosts_faster_clock_is_learned_early() {
    // On one Wi-Fi a skew of 150 ppm is known well within the first 30 s, so the line doesn't
    // lag the host while the fit waits for its full span.
    let w = World::new(120.0, Steps::SameRoom, same_wifi).skew(150.0);
    let r = run(&w);
    let s = r.summary(0.0);
    assert!(r.max_err(5.0, 120.0) <= 1_000.0, "{s}");
    let last = r.maps.last().unwrap().2;
    assert!(
        ((last.rate - 1.0) * 1e6 - 150.0).abs() < 5.0,
        "rate {:.1} ppm; {s}",
        (last.rate - 1.0) * 1e6
    );
}
