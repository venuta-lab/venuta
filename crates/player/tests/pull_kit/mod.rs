//! A fake pull device for the output's own loop (`output::PullOutput`, what the WASAPI output
//! runs), in virtual time or real time (WP1). It models what WASAPI does: an engine (shared)
//! that takes one period from the buffer at each pass, or a double buffer (exclusive) the
//! hardware plays on over when it is not refilled; a DAC behind them by a latency; a position
//! that counts what reached the DAC (shared: frames of ours, so it stalls in a gap; exclusive:
//! every frame played, so it runs on over a stale buffer); a lost device, a device that stops
//! signalling, and readings it can't give (`S_FALSE`). It records what the DAC played, when,
//! and how the loop used it (an exclusive buffer written twice in one event, a shared write
//! over the buffer, a start while started are errors).

// Each test file uses only some of the kit.
#![allow(dead_code)]

use player::output::{
    DeviceFormat, DeviceNotes, Fault, Kick, Mode, Opened, PositionMap, PullDevice, PullOpener,
    PullOutput, SampleKind, Space, Wake,
};
use player::output_api::{OpenRequest, OutputError, Render};
use player::sim::{Run, Tape};
use std::cell::Cell;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use together::time::{MonoClock, RealClock, SimClock, Sleeper};

thread_local! {
    /// The render thread's own work is watched (the allocation test): set by the fake once the
    /// stream has run for `FakeCfg::watch_from` device wakes, cleared when the device halts.
    pub static WATCHED: Cell<bool> = const { Cell::new(false) };
    /// Inside the fake itself (its own allocations are not the loop's).
    pub static IN_FAKE: Cell<u32> = const { Cell::new(0) };
}

/// Marks the fake's own work while it lasts.
pub struct InFake;

impl InFake {
    pub fn enter() -> InFake {
        IN_FAKE.with(|c| c.set(c.get() + 1));
        InFake
    }
}

impl Drop for InFake {
    fn drop(&mut self) {
        IN_FAKE.with(|c| c.set(c.get() - 1));
    }
}

/// The loop's own work on this thread is being watched now.
pub fn watched() -> bool {
    WATCHED.try_with(Cell::get).unwrap_or(false) && IN_FAKE.try_with(Cell::get).unwrap_or(1) == 0
}

/// This thread is watched (the fake's own calls included).
pub fn watching() -> bool {
    WATCHED.try_with(Cell::get).unwrap_or(false)
}

pub const RATE: u32 = 48_000;
pub const MS: u64 = 1_000_000;

pub fn want() -> OpenRequest {
    OpenRequest {
        rate: RATE,
        channels: 2,
        bits: 24,
    }
}

// --- The fake device ------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FakeCfg {
    pub exclusive: bool,
    /// The engine's period (shared), the buffer (exclusive), in frames.
    pub period: u32,
    /// The device buffer (shared), in frames.
    pub buffer: u32,
    /// From the start of a pass's frames to the DAC, in frames.
    pub latency: u32,
    pub skew_ppm: f64,
    /// (event index counted from 0 at each start, how late the loop is woken for it, ns).
    pub late: Vec<(u64, u64)>,
    /// From the first start, ns: every call fails as a lost device does.
    pub lost_after: Option<u64>,
    /// From the first start, ns: the device stops (no more passes, no more events).
    pub silent_after: Option<u64>,
    /// Every n-th position reading can't be given (`S_FALSE`); 0: never.
    pub no_reading_every: u32,
    /// From this many device wakes on, the render thread's own work is watched.
    pub watch_from: Option<u64>,
}

impl Default for FakeCfg {
    fn default() -> Self {
        FakeCfg {
            exclusive: false,
            period: 480,
            buffer: 2_400,
            latency: 96,
            skew_ppm: 0.0,
            late: Vec::new(),
            lost_after: None,
            silent_after: None,
            no_reading_every: 0,
            watch_from: None,
        }
    }
}

/// One of the device's halts (stop and reset): when, and what it held.
#[derive(Debug, Clone)]
pub struct Halt {
    pub at_ns: u64,
    /// The position then (frames at the DAC since the start).
    pub raw: u64,
    /// Frames handed over since the start.
    pub written: u64,
}

#[derive(Debug, Default)]
pub struct Fake {
    pub cfg: FakeCfg,
    pub started: bool,
    pub start_ns: u64,
    pub first_start_ns: Option<u64>,
    pub lost: bool,
    pub passes: u64,
    /// Passes whose event the loop has had.
    pub seen: u64,
    /// Shared: samples handed over and not taken by the engine.
    pub fifo: VecDeque<i32>,
    /// Exclusive: buffers handed over and not yet played; what each half last held.
    pub queue: VecDeque<Vec<i32>>,
    pub halves: [Vec<i32>; 2],
    pub wrote_for_event: bool,
    pub prefilled: bool,
    pub written: u64,
    /// Frames handed over since the last halt or start (what a start may hold that is new).
    pub fresh: u64,
    pub taken: u64,
    /// Our frames on the DAC: (our first frame, frames, the tick it played at).
    pub spans: Vec<(u64, u64, u64)>,
    /// Ticks of each start that played no frame of ours (silence, or a stale buffer).
    pub gaps: Vec<Range<u64>>,
    /// Frames handed over when the first gap began.
    pub gap_at: Option<u64>,
    pub readings: u32,
    pub halts: Vec<Halt>,
    /// Device wakes since the open.
    pub wakes: u64,
    pub starts: u32,
    pub runs: Vec<Run>,
    pub errors: Vec<String>,
}

impl Fake {
    pub fn tps(&self) -> f64 {
        f64::from(RATE) * (1.0 + self.cfg.skew_ppm / 1e6)
    }

    pub fn pass_ns(&self, k: u64) -> u64 {
        self.start_ns + (k as f64 * f64::from(self.cfg.period) * 1e9 / self.tps()).round() as u64
    }

    /// When tick `tick` of the current start is heard.
    pub fn heard_ns(&self, tick: u64) -> f64 {
        self.start_ns as f64 + (tick + u64::from(self.cfg.latency)) as f64 * 1e9 / self.tps()
    }

    /// When our frame `f` of the current start was heard, if it was.
    pub fn heard_frame_ns(&self, f: u64) -> Option<f64> {
        let &(first, _, tick) = self
            .spans
            .iter()
            .rev()
            .find(|(first, n, _)| *first <= f && f < first + n)?;
        Some(self.heard_ns(tick + (f - first)))
    }

    pub fn silent_from(&self) -> Option<u64> {
        Some(self.first_start_ns? + self.cfg.silent_after?)
    }

    pub fn check_lost(&mut self, now: u64) {
        if let (Some(first), Some(after)) = (self.first_start_ns, self.cfg.lost_after)
            && now >= first + after
        {
            self.lost = true;
        }
    }

    pub fn fault(&mut self, now: u64) -> Result<(), Fault> {
        self.advance(now);
        if self.lost { Err(Fault::Lost) } else { Ok(()) }
    }

    pub fn advance(&mut self, now: u64) {
        self.check_lost(now);
        while self.started && !self.lost {
            let t = self.pass_ns(self.passes);
            if t > now || self.silent_from().is_some_and(|s| t >= s) {
                break;
            }
            let k = self.passes;
            self.pass(k);
            self.passes += 1;
        }
    }

    pub fn pass(&mut self, k: u64) {
        let p = u64::from(self.cfg.period);
        let tick = k * p;
        let mut played: Vec<i32> = Vec::new();
        if self.cfg.exclusive {
            let half = (k % 2) as usize;
            if let Some(b) = self.queue.pop_front() {
                self.spans.push((self.taken, p, tick));
                self.taken += p;
                self.halves[half] = b.clone();
                played = b;
            } else {
                // The hardware plays on over what that half last held.
                self.gap(tick..tick + p);
                played.extend_from_slice(&self.halves[half]);
                played.resize(p as usize * 2, 0);
            }
        } else {
            let have = (self.fifo.len() / 2) as u64;
            let n = have.min(p);
            if n > 0 {
                self.spans.push((self.taken, n, tick));
                self.taken += n;
                played.extend(self.fifo.drain(..n as usize * 2));
            }
            if n < p {
                self.gap(tick + n..tick + p);
                played.resize(p as usize * 2, 0);
            }
        }
        if let Some(run) = self.runs.last_mut() {
            run.samples.extend_from_slice(&played);
        }
    }

    pub fn gap(&mut self, r: Range<u64>) {
        self.gap_at.get_or_insert(self.written);
        if let Some(run) = self.runs.last_mut() {
            match run.dry.last_mut() {
                Some(d) if d.end == r.start => d.end = r.end,
                _ => run.dry.push(r.clone()),
            }
        }
        match self.gaps.last_mut() {
            Some(g) if g.end == r.start => g.end = r.end,
            _ => self.gaps.push(r),
        }
    }

    /// Frames at the DAC now (ours or not).
    pub fn ticks_heard(&self, now: u64) -> u64 {
        if !self.started || (now as f64) < self.heard_ns(0) {
            return 0;
        }
        let t = (now - self.start_ns) as f64 * self.tps() / 1e9 - f64::from(self.cfg.latency);
        ((t.floor() as u64) + 1).min(self.passes * u64::from(self.cfg.period))
    }

    /// What the device reports: shared, the frames of ours at the DAC (it stalls in a gap);
    /// exclusive, every frame the hardware played (it runs on over a stale buffer).
    pub fn raw(&self, now: u64) -> u64 {
        let ticks = self.ticks_heard(now);
        if self.cfg.exclusive {
            return ticks;
        }
        self.spans
            .iter()
            .map(|&(_, n, tick)| ticks.saturating_sub(tick).min(n))
            .sum()
    }
}

pub struct FakeDevice {
    pub fake: Arc<Mutex<Fake>>,
    pub clock: SimOrReal,
}

#[derive(Clone)]
pub enum SimOrReal {
    Sim(SimClock),
    Real,
}

impl SimOrReal {
    pub fn now(&self) -> u64 {
        match self {
            SimOrReal::Sim(c) => c.now_ns(),
            SimOrReal::Real => RealClock.now_ns(),
        }
    }
}

pub struct NoKick;

impl Kick for NoKick {
    fn kick(&self) {}
}

impl FakeDevice {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Fake> {
        self.fake.lock().unwrap()
    }
}

impl PullDevice for FakeDevice {
    fn wait(&mut self, sleeper: &Sleeper, running: bool, timeout_ns: u64) -> Result<Wake, Fault> {
        let _in = InFake::enter();
        let now = self.clock.now();
        if !running {
            // Waiting for a command: the fake has no kick, so the loop polls.
            sleeper.sleep_until(now + MS);
            return Ok(Wake::Kick);
        }
        let target = {
            let mut f = self.lock();
            f.advance(now);
            let k = f.seen;
            let t = f.pass_ns(k);
            let late = f
                .cfg
                .late
                .iter()
                .find(|(e, _)| *e == k)
                .map_or(0, |(_, l)| *l);
            (f.started && !f.silent_from().is_some_and(|s| t >= s)).then_some(t + late)
        };
        match target {
            Some(t) if t <= now + timeout_ns => {
                sleeper.sleep_until(t);
                let now = self.clock.now();
                let mut f = self.lock();
                f.advance(now);
                // The event fired once for every pass since the last wake (auto-reset).
                f.seen = f.passes;
                f.wrote_for_event = false;
                f.wakes += 1;
                if f.cfg.watch_from.is_some_and(|n| f.wakes >= n) {
                    WATCHED.with(|w| w.set(true));
                }
                Ok(Wake::Device)
            }
            _ => {
                sleeper.sleep_until(now + timeout_ns);
                Ok(Wake::Timeout)
            }
        }
    }

    fn space(&mut self) -> Result<Space, Fault> {
        let _in = InFake::enter();
        let now = self.clock.now();
        let mut f = self.lock();
        f.fault(now)?;
        if f.cfg.exclusive {
            return Ok(Space {
                free: f.cfg.period,
                held: None,
            });
        }
        let held = (f.written - f.taken) as u32;
        Ok(Space {
            free: f.cfg.buffer - held,
            held: Some(held),
        })
    }

    fn write(&mut self, frames: u32, fill: &mut dyn FnMut(&mut [u8])) -> Result<(), Fault> {
        let _in = InFake::enter();
        let now = self.clock.now();
        // As GetBuffer: a lost device gives no buffer to fill.
        self.lock().fault(now)?;
        let mut buf = vec![0u8; frames as usize * 8];
        fill(&mut buf);
        let samples: Vec<i32> = buf
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes(*b))
            .collect();
        let mut f = self.lock();
        if f.cfg.exclusive {
            let ok = frames == f.cfg.period
                && f.queue.len() < 2
                && if f.started {
                    !f.wrote_for_event
                } else {
                    !f.prefilled
                };
            if !ok {
                let e = format!(
                    "exclusive write of {frames} frames, {} queued, started {}",
                    f.queue.len(),
                    f.started
                );
                f.errors.push(e.clone());
                return Err(Fault::Other(e));
            }
            if f.started {
                f.wrote_for_event = true;
            } else {
                f.prefilled = true;
            }
            f.queue.push_back(samples);
        } else {
            let held = (f.written - f.taken) as u32;
            if held + frames > f.cfg.buffer {
                let e = format!("shared write of {frames} frames over the buffer");
                f.errors.push(e.clone());
                return Err(Fault::Other(e));
            }
            f.fifo.extend(samples);
        }
        f.written += u64::from(frames);
        f.fresh += u64::from(frames);
        Ok(())
    }

    fn position(&mut self) -> Result<Option<(u64, u64)>, Fault> {
        let _in = InFake::enter();
        let now = self.clock.now();
        let mut f = self.lock();
        f.fault(now)?;
        f.readings += 1;
        if f.cfg.no_reading_every > 0 && f.readings.is_multiple_of(f.cfg.no_reading_every) {
            return Ok(None);
        }
        Ok(Some((f.raw(now), now)))
    }

    fn start(&mut self) -> Result<(), Fault> {
        let _in = InFake::enter();
        let now = self.clock.now();
        let mut f = self.lock();
        f.fault(now)?;
        if f.started {
            f.errors.push("start while started".into());
            return Err(Fault::Other("started twice".into()));
        }
        f.started = true;
        f.start_ns = now;
        f.first_start_ns.get_or_insert(now);
        f.passes = 0;
        f.seen = 0;
        f.starts += 1;
        f.spans.clear();
        f.gaps.clear();
        let tps = f.tps();
        let start = f.heard_ns(0).round() as u64;
        f.runs.push(Run {
            start_ns: start,
            ticks_per_s: tps,
            channels: 2,
            ..Run::default()
        });
        // Frames held from before a start (a missing reset) play first.
        let held = (f.fifo.len() / 2) as u64 + f.queue.len() as u64 * u64::from(f.cfg.period);
        let stale = held.saturating_sub(f.fresh);
        f.fresh = 0;
        if let Some(r) = f.runs.last_mut() {
            r.stale = stale;
        }
        f.advance(now);
        Ok(())
    }

    fn halt(&mut self) -> Result<(), Fault> {
        let _in = InFake::enter();
        let now = self.clock.now();
        let mut f = self.lock();
        f.fault(now)?;
        let raw = f.raw(now);
        let written = f.written;
        f.halts.push(Halt {
            at_ns: now,
            raw,
            written,
        });
        f.started = false;
        WATCHED.with(|w| w.set(false));
        f.fifo.clear();
        f.queue.clear();
        f.prefilled = false;
        f.written = 0;
        f.fresh = 0;
        f.taken = 0;
        Ok(())
    }
}

#[derive(Clone)]
pub struct FakeOpener {
    pub cfg: FakeCfg,
    pub fake: Arc<Mutex<Fake>>,
    pub clock: SimOrReal,
}

impl FakeOpener {
    pub fn new(cfg: FakeCfg, clock: SimOrReal) -> FakeOpener {
        FakeOpener {
            fake: Arc::new(Mutex::new(Fake {
                cfg: cfg.clone(),
                ..Fake::default()
            })),
            cfg,
            clock,
        }
    }
}

impl PullOpener for FakeOpener {
    type Device = FakeDevice;

    fn name(&self) -> String {
        "Fake pull device".into()
    }

    fn mode(&self) -> Mode {
        if self.cfg.exclusive {
            Mode::Exclusive
        } else {
            Mode::Shared
        }
    }

    fn open(
        &mut self,
        want: &OpenRequest,
        _sink: &Arc<player::output::EventSink>,
    ) -> Result<Opened<FakeDevice>, OutputError> {
        let c = &self.cfg;
        // A new device each open: a lost one is gone, a new one works.
        {
            let mut f = self.fake.lock().unwrap();
            let runs = std::mem::take(&mut f.runs);
            let halts = std::mem::take(&mut f.halts);
            let errors = std::mem::take(&mut f.errors);
            let lost_before = f.lost;
            *f = Fake {
                cfg: FakeCfg {
                    lost_after: if lost_before { None } else { c.lost_after },
                    ..c.clone()
                },
                runs,
                halts,
                errors,
                ..Fake::default()
            };
        }
        let format = DeviceFormat {
            rate: RATE,
            channels: 2,
            container_bits: 32,
            valid_bits: want.bits.min(32),
            kind: SampleKind::Int,
        };
        let (map, buffer_frames, latency) = if c.exclusive {
            (
                PositionMap::exclusive(RATE, c.period),
                2 * c.period + c.latency,
                c.latency,
            )
        } else {
            (
                PositionMap::shared(RATE, c.period),
                c.buffer + c.period + c.latency,
                c.period + c.latency,
            )
        };
        Ok(Opened {
            device: FakeDevice {
                fake: self.fake.clone(),
                clock: self.clock.clone(),
            },
            info: player::output_api::OutputInfo {
                format,
                period_frames: c.period,
                buffer_frames,
            },
            mode: self.mode(),
            name: self.name(),
            kick: Arc::new(NoKick),
            map,
            notes: DeviceNotes {
                endpoint_id: Some("fake".into()),
                device_buffer_frames: if c.exclusive { c.period } else { c.buffer },
                stream_latency_frames: latency,
                pro_audio: false,
            },
        })
    }
}

pub fn sim_output(cfg: FakeCfg) -> (PullOutput<FakeOpener>, Arc<Mutex<Fake>>, SimClock) {
    let clock = SimClock::new(1_000 * MS);
    let opener = FakeOpener::new(cfg, SimOrReal::Sim(clock.clone()));
    let fake = opener.fake.clone();
    (
        PullOutput::new(opener, Arc::new(clock.clone())),
        fake,
        clock,
    )
}

pub fn tape(fake: &Arc<Mutex<Fake>>, clock: &SimClock) -> Tape {
    let mut f = fake.lock().unwrap();
    f.advance(clock.now_ns());
    Tape {
        runs: f.runs.clone(),
    }
}

// --- Renderers ------------------------------------------------------------------------------

/// Writes each frame's own device frame number plus one (low 24 bits, both channels), and
/// counts its calls and what it made.
pub struct Counter {
    pub calls: Arc<AtomicU64>,
    pub next: Arc<AtomicU64>,
    pub broken: Arc<AtomicU64>,
    pub longest: Arc<AtomicU64>,
}

#[derive(Clone, Default)]
pub struct Counts {
    pub calls: Arc<AtomicU64>,
    pub next: Arc<AtomicU64>,
    pub broken: Arc<AtomicU64>,
    pub longest: Arc<AtomicU64>,
}

impl Counts {
    pub fn render(&self) -> Box<dyn Render> {
        Box::new(Counter {
            calls: self.calls.clone(),
            next: self.next.clone(),
            broken: self.broken.clone(),
            longest: self.longest.clone(),
        })
    }
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
    pub fn made(&self) -> u64 {
        self.next.load(Ordering::SeqCst)
    }
}

pub fn counter_value(frame: u64) -> i32 {
    (((frame + 1) & 0x7F_FFFF) as i32) << 8
}

impl Render for Counter {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        if at != self.next.load(Ordering::SeqCst) {
            self.broken.fetch_add(1, Ordering::SeqCst);
        }
        for (k, f) in out
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .take(frames as usize)
            .enumerate()
        {
            let v = counter_value(at + k as u64);
            f[..4].copy_from_slice(&v.to_le_bytes());
            f[4..].copy_from_slice(&v.to_le_bytes());
        }
        self.next.store(at + u64::from(frames), Ordering::SeqCst);
        self.longest.fetch_max(u64::from(frames), Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
