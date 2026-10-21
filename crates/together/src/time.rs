//! Time (contract 5.1 of the Listen together design): the platform's monotonic clock, the same
//! one audio timestamps use, and a virtual one for simulations.
//!
//! - [`RealClock`]: QPC on Windows, `CLOCK_MONOTONIC` on Android and other Unix systems, counted
//!   from boot (not from when the app started), so a host that restarts keeps its time base.
//! - [`SimClock`]: virtual time that moves only when the test says so, as fast as the work
//!   allows. Threads that wait on it (a simulated output's render thread, a sync tick) hold a
//!   [`Sleeper`]; callbacks (a simulated network's deliveries) are scheduled on it with `at`.
//!   `pass` runs everything due, in time order, one at a time: each sleeper woken runs until it
//!   sleeps again, so a run is the same every time, whatever the machine's load.
//! - [`SkewedClock`]: another computer's clock over a shared one (an offset, a rate in ppm, and
//!   jumps), for whole rooms in one process.
//!
//! Rules for anything that runs on a `SimClock`: a thread that waits for time waits through its
//! `Sleeper`, never `std::thread::sleep`; it never blocks on another sleeper (poll channels
//! with `try_recv` between sleeps); only the test's own thread calls `pass`.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

/// The platform's monotonic clock, the same one audio timestamps use (QPC, `CLOCK_MONOTONIC`).
pub trait MonoClock: Send + Sync {
    fn now_ns(&self) -> u64;
}

/// A clock things can wait on: real time, or a simulation's.
pub trait Timebase: MonoClock {
    /// Run `f` at time `t_ns` (at once if that has passed): on a timer thread (real time) or on
    /// the thread that runs the simulation, at exactly that virtual time. `f` must be quick and
    /// must not wait.
    fn at(&self, t_ns: u64, f: Box<dyn FnOnce() + Send>);
    /// Let `d_ns` pass: sleep (real time) or run the simulation that far (virtual time; only the
    /// test's own thread calls this).
    fn pass(&self, d_ns: u64);
    /// A way for one thread to wait on this clock. Make it on the thread that starts the waiting
    /// thread, before starting it, so a simulation counts that thread from its first instant.
    fn sleeper(&self) -> Sleeper;
}

// --- Real time ------------------------------------------------------------------------------

/// The platform's monotonic clock, in ns since boot.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealClock;

impl MonoClock for RealClock {
    fn now_ns(&self) -> u64 {
        platform_now_ns()
    }
}

#[cfg(windows)]
fn platform_now_ns() -> u64 {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };
    static FREQ: OnceLock<u64> = OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: a valid pointer to an i64 the call writes.
        unsafe { QueryPerformanceFrequency(&mut f) };
        f.max(1) as u64
    });
    let mut c = 0i64;
    // SAFETY: a valid pointer to an i64 the call writes.
    unsafe { QueryPerformanceCounter(&mut c) };
    let c = c.max(0) as u64;
    // Whole seconds and the rest apart, so nothing overflows in years of uptime.
    (c / freq) * 1_000_000_000 + (c % freq) * 1_000_000_000 / freq
}

#[cfg(unix)]
fn platform_now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid pointer to a timespec the call writes.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec.max(0) as u64 * 1_000_000_000 + ts.tv_nsec.max(0) as u64
}

#[cfg(not(any(windows, unix)))]
fn platform_now_ns() -> u64 {
    // No boot-based clock known here: from the first reading instead.
    static BASE: OnceLock<std::time::Instant> = OnceLock::new();
    BASE.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos() as u64
}

type Job = Box<dyn FnOnce() + Send>;

/// Callbacks waiting, by (when, order made).
#[derive(Default)]
struct Due {
    jobs: BTreeMap<(u64, u64), Job>,
    made: u64,
}

impl Due {
    fn add(&mut self, t_ns: u64, f: Job) {
        self.made += 1;
        self.jobs.insert((t_ns, self.made), f);
    }
}

/// Real time's timer thread: one for the process, started when first needed.
struct Timers {
    due: Mutex<Due>,
    wake: Condvar,
}

fn timers() -> &'static Timers {
    static T: OnceLock<&'static Timers> = OnceLock::new();
    T.get_or_init(|| {
        let t: &'static Timers = Box::leak(Box::new(Timers {
            due: Mutex::default(),
            wake: Condvar::new(),
        }));
        std::thread::Builder::new()
            .name("timebase-timers".into())
            .spawn(move || {
                let mut g = t.due.lock().unwrap();
                loop {
                    let now = RealClock.now_ns();
                    match g.jobs.first_key_value().map(|(k, _)| *k) {
                        Some(key) if key.0 <= now => {
                            let job = g.jobs.remove(&key);
                            drop(g);
                            if let Some(job) = job {
                                job();
                            }
                            g = t.due.lock().unwrap();
                        }
                        Some((when, _)) => {
                            g = t
                                .wake
                                .wait_timeout(g, Duration::from_nanos(when - now))
                                .unwrap()
                                .0;
                        }
                        None => g = t.wake.wait(g).unwrap(),
                    }
                }
            })
            .expect("the timer thread starts");
        t
    })
}

impl Timebase for RealClock {
    fn at(&self, t_ns: u64, f: Box<dyn FnOnce() + Send>) {
        let t = timers();
        t.due.lock().unwrap().add(t_ns, f);
        t.wake.notify_one();
    }
    fn pass(&self, d_ns: u64) {
        std::thread::sleep(Duration::from_nanos(d_ns));
    }
    fn sleeper(&self) -> Sleeper {
        Sleeper(SleeperKind::Real)
    }
}

// --- Waiting on a clock ---------------------------------------------------------------------

/// One thread's way to wait on a `Timebase`.
pub struct Sleeper(SleeperKind);

enum SleeperKind {
    Real,
    Virtual { clock: SimClock },
}

impl Sleeper {
    /// Wait until the clock reads `t_ns` (at once if it already does).
    pub fn sleep_until(&self, t_ns: u64) {
        match &self.0 {
            SleeperKind::Real => {
                let now = RealClock.now_ns();
                if t_ns > now {
                    std::thread::sleep(Duration::from_nanos(t_ns - now));
                }
            }
            SleeperKind::Virtual { clock } => clock.sleep_until(t_ns),
        }
    }

    pub fn now_ns(&self) -> u64 {
        match &self.0 {
            SleeperKind::Real => RealClock.now_ns(),
            SleeperKind::Virtual { clock } => clock.now_ns(),
        }
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        if let SleeperKind::Virtual { clock } = &self.0 {
            clock.leave();
        }
    }
}

// --- Virtual time ---------------------------------------------------------------------------

/// How long (real time) `pass` waits for a woken thread to sleep again before it calls the run
/// stuck: a thread blocked on something other than its sleeper.
const STUCK: Duration = Duration::from_secs(30);

/// Virtual monotonic time for simulations (cheap to clone; every clone is the same clock).
#[derive(Clone)]
pub struct SimClock(Arc<Sim>);

struct Sim {
    now: AtomicU64,
    state: Mutex<SimState>,
    changed: Condvar,
}

#[derive(Default)]
struct SimState {
    next: u64,
    /// Callbacks by (when, order made).
    jobs: BTreeMap<(u64, u64), Job>,
    /// Sleeping threads by (when, ticket).
    sleeping: BTreeMap<(u64, u64), ()>,
    /// Tickets woken and not yet back.
    woken: HashSet<u64>,
    /// Threads awake (made a sleeper and not sleeping now).
    awake: usize,
    passing: bool,
}

impl SimClock {
    /// A virtual clock reading `start_ns`.
    pub fn new(start_ns: u64) -> SimClock {
        SimClock(Arc::new(Sim {
            now: AtomicU64::new(start_ns),
            state: Mutex::default(),
            changed: Condvar::new(),
        }))
    }

    /// Run the simulation to `t_ns`.
    pub fn run_until(&self, t_ns: u64) {
        let s = &self.0;
        let mut g = s.state.lock().unwrap();
        assert!(!g.passing, "only one thread runs a simulation");
        g.passing = true;
        loop {
            g = Self::settle(s, g);
            let job = g.jobs.keys().next().copied();
            let sleeper = g.sleeping.keys().next().copied();
            // Callbacks before sleepers due at the same moment.
            let pick = match (job, sleeper) {
                (Some(j), Some(z)) if j.0 <= z.0 => Some((j, true)),
                (Some(j), None) => Some((j, true)),
                (_, Some(z)) => Some((z, false)),
                (None, None) => None,
            };
            match pick {
                Some((key, is_job)) if key.0 <= t_ns => {
                    let now = s.now.load(Ordering::SeqCst).max(key.0);
                    s.now.store(now, Ordering::SeqCst);
                    if is_job {
                        let f = g.jobs.remove(&key).unwrap();
                        drop(g);
                        f();
                        g = s.state.lock().unwrap();
                    } else {
                        g.sleeping.remove(&key);
                        g.woken.insert(key.1);
                        g.awake += 1;
                        s.changed.notify_all();
                    }
                }
                _ => break,
            }
        }
        let now = s.now.load(Ordering::SeqCst).max(t_ns);
        s.now.store(now, Ordering::SeqCst);
        g.passing = false;
    }

    /// Wait until every awake thread sleeps again (or leaves).
    fn settle<'a>(
        s: &'a Sim,
        mut g: std::sync::MutexGuard<'a, SimState>,
    ) -> std::sync::MutexGuard<'a, SimState> {
        while g.awake > 0 {
            let (ng, to) = s.changed.wait_timeout(g, STUCK).unwrap();
            g = ng;
            assert!(
                !(to.timed_out() && g.awake > 0),
                "a simulated thread neither slept nor left for {STUCK:?}: it waits on something other than its sleeper"
            );
        }
        g
    }

    fn sleep_until(&self, t_ns: u64) {
        let s = &self.0;
        let mut g = s.state.lock().unwrap();
        if t_ns <= s.now.load(Ordering::SeqCst) {
            return;
        }
        g.next += 1;
        let ticket = g.next;
        g.sleeping.insert((t_ns, ticket), ());
        g.awake -= 1;
        s.changed.notify_all();
        while !g.woken.remove(&ticket) {
            g = s.changed.wait(g).unwrap();
        }
    }

    fn leave(&self) {
        let s = &self.0;
        let mut g = s.state.lock().unwrap();
        g.awake -= 1;
        s.changed.notify_all();
    }
}

impl MonoClock for SimClock {
    fn now_ns(&self) -> u64 {
        self.0.now.load(Ordering::SeqCst)
    }
}

impl Timebase for SimClock {
    fn at(&self, t_ns: u64, f: Box<dyn FnOnce() + Send>) {
        let mut g = self.0.state.lock().unwrap();
        g.next += 1;
        let order = g.next;
        let when = t_ns.max(self.now_ns());
        g.jobs.insert((when, order), f);
    }
    fn pass(&self, d_ns: u64) {
        self.run_until(self.now_ns() + d_ns);
    }
    fn sleeper(&self) -> Sleeper {
        self.0.state.lock().unwrap().awake += 1;
        Sleeper(SleeperKind::Virtual {
            clock: self.clone(),
        })
    }
}

// --- Another computer's clock ---------------------------------------------------------------

/// Another computer's monotonic clock over `base`: `offset + base x (1 + ppm / 1e6)`, plus any
/// jumps (a machine that slept).
pub struct SkewedClock {
    base: Arc<dyn MonoClock>,
    ppm: f64,
    offset_ns: AtomicI64,
    /// `base`'s reading the rate counts from.
    from_ns: u64,
}

impl SkewedClock {
    /// Reads `start_ns` now, and runs `ppm` fast (negative: slow) against `base`.
    pub fn new(base: Arc<dyn MonoClock>, start_ns: u64, ppm: f64) -> SkewedClock {
        let from_ns = base.now_ns();
        SkewedClock {
            base,
            ppm,
            offset_ns: AtomicI64::new(start_ns as i64),
            from_ns,
        }
    }

    /// Jump ahead (a computer that slept and woke: its monotonic clock went on meanwhile, but
    /// nothing it measured before holds).
    pub fn jump(&self, ns: i64) {
        self.offset_ns.fetch_add(ns, Ordering::SeqCst);
    }

    /// This clock's reading when `base` reads `base_ns`.
    pub fn at(&self, base_ns: u64) -> u64 {
        let d = base_ns as f64 - self.from_ns as f64;
        let v = self.offset_ns.load(Ordering::SeqCst) as f64 + d * (1.0 + self.ppm / 1e6);
        v.round().max(0.0) as u64
    }
}

impl MonoClock for SkewedClock {
    fn now_ns(&self) -> u64 {
        self.at(self.base.now_ns())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn real_time_moves_on_and_never_back() {
        let a = RealClock.now_ns();
        std::thread::sleep(Duration::from_millis(5));
        let b = RealClock.now_ns();
        assert!(b > a + 4_000_000, "{a} then {b}");
        // Counted from boot, not from this test: well past the moment the process started.
        assert!(a > 0);
    }

    #[test]
    fn real_timers_run_in_time() {
        let (tx, rx) = std::sync::mpsc::channel();
        let t0 = RealClock.now_ns();
        RealClock.at(
            t0 + 20_000_000,
            Box::new(move || tx.send(RealClock.now_ns()).unwrap()),
        );
        let ran = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(ran >= t0 + 20_000_000);
    }

    #[test]
    fn virtual_time_moves_only_when_passed() {
        let c = SimClock::new(1_000);
        assert_eq!(c.now_ns(), 1_000);
        c.pass(500);
        assert_eq!(c.now_ns(), 1_500);
    }

    #[test]
    fn callbacks_run_at_their_virtual_time_in_order() {
        let c = SimClock::new(0);
        let seen = Arc::new(Mutex::new(Vec::new()));
        for (t, tag) in [(30, "c"), (10, "a"), (20, "b"), (20, "b2")] {
            let (s, c2) = (seen.clone(), c.clone());
            c.at(
                t,
                Box::new(move || s.lock().unwrap().push((tag, c2.now_ns()))),
            );
        }
        c.pass(25);
        assert_eq!(*seen.lock().unwrap(), [("a", 10), ("b", 20), ("b2", 20)]);
        c.pass(100);
        assert_eq!(seen.lock().unwrap().last(), Some(&("c", 30)));
        assert_eq!(c.now_ns(), 125);
    }

    #[test]
    fn a_callback_may_schedule_another() {
        let c = SimClock::new(0);
        let n = Arc::new(AtomicU64::new(0));
        fn tick(c: SimClock, n: Arc<AtomicU64>) {
            if n.fetch_add(1, Ordering::SeqCst) < 99 {
                let t = c.now_ns() + 10;
                let c2 = c.clone();
                c.at(t, Box::new(move || tick(c2, n)));
            }
        }
        let (c2, n2) = (c.clone(), n.clone());
        c.at(0, Box::new(move || tick(c2, n2)));
        c.pass(10_000);
        assert_eq!(n.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn sleeping_threads_run_in_lockstep_with_virtual_time() {
        let c = SimClock::new(0);
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut threads = Vec::new();
        for (who, every) in [("fast", 3u64), ("slow", 7u64)] {
            let z = c.sleeper();
            let l = log.clone();
            threads.push(std::thread::spawn(move || {
                let mut t = 0;
                while t < 21 {
                    t += every;
                    z.sleep_until(t);
                    l.lock().unwrap().push((z.now_ns(), who));
                }
            }));
        }
        c.pass(100);
        for t in threads {
            t.join().unwrap();
        }
        let log = log.lock().unwrap().clone();
        let times: Vec<u64> = log.iter().map(|x| x.0).collect();
        assert!(
            times.windows(2).all(|w| w[0] <= w[1]),
            "in time order: {log:?}"
        );
        assert_eq!(log.iter().filter(|x| x.1 == "fast").count(), 7);
        assert_eq!(log.iter().filter(|x| x.1 == "slow").count(), 3);
        assert_eq!(log[0], (3, "fast"));
    }

    #[test]
    fn a_simulation_runs_far_faster_than_real_time() {
        let c = SimClock::new(0);
        let z = c.sleeper();
        let done = Arc::new(AtomicBool::new(false));
        let d = done.clone();
        let th = std::thread::spawn(move || {
            // An hour of 10 ms periods.
            for k in 1..=360_000u64 {
                z.sleep_until(k * 10_000_000);
            }
            d.store(true, Ordering::SeqCst);
        });
        let t0 = std::time::Instant::now();
        c.pass(3_600_000_000_000);
        th.join().unwrap();
        assert!(done.load(Ordering::SeqCst));
        assert!(t0.elapsed() < Duration::from_secs(60), "{:?}", t0.elapsed());
    }

    #[test]
    fn a_skewed_clock_runs_at_its_own_rate_and_jumps() {
        let c = SimClock::new(0);
        let base: Arc<dyn MonoClock> = Arc::new(c.clone());
        let s = SkewedClock::new(base, 5_000_000_000, 100.0);
        c.pass(10_000_000_000);
        assert_eq!(s.now_ns(), 5_000_000_000 + 10_001_000_000);
        s.jump(60_000_000_000);
        assert_eq!(s.now_ns(), 75_001_000_000);
    }
}
