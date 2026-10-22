//! The simulated output (contract 5.2): an `Output` whose DAC runs on a model clock, for tests.
//!
//! The DAC plays one frame per tick, tick n at `start + n / (rate x (1 + skew))` on the
//! `Timebase` it runs on (virtual time for fast, repeatable runs; real time too). Its render
//! thread wakes once a period (later by the wake jitter, never on a missed period), lets the DAC
//! catch up to now, publishes the presentation pair, and renders as many periods as its buffer
//! has room for. When the buffer runs out the DAC plays silence (an xrun) and waits for frames,
//! as hardware does. Everything the DAC played goes on a tape: every frame, the silence of each
//! underrun, and when each was heard.
//!
//! Faults on demand: clock skew, wake jitter, coarse positions (the frame reported in steps,
//! stamped with the reading's time, as a coarse driver's is), missed periods, a buffer held
//! over a stop that plays on after the next start (no reset), and a lost device.

use crate::output::{DeviceFormat, Mode, SampleKind};
use crate::output_api::{
    OpenRequest, Output, OutputError, OutputEvent, OutputInfo, Presentation, PresentationPair,
    Render,
};
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
pub use together::time::{MonoClock, RealClock, SimClock, SkewedClock, Sleeper, Timebase};

/// How the simulated device behaves.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// The DAC's clock against the mono clock, in ppm (positive: fast).
    pub skew_ppm: f64,
    /// Each wake comes up to this late (0 to 3 ms in the design's runs), in µs.
    pub wake_jitter_us: u32,
    /// The position is reported in steps of this (0: exact; 1 or 10 ms), in µs.
    pub position_step_us: u32,
    pub period_us: u32,
    /// The buffer, in periods (shared mode: 10 ms periods, 50 ms of buffer).
    pub buffer_periods: u32,
    /// Wakes (counted from 1 at each start) that never come.
    pub missed: Vec<u64>,
    /// A stop that doesn't let go of what it holds: the next start plays it first.
    pub held_buffer: bool,
    /// The format it runs at (`None`: whatever is asked, 32-bit integer containers).
    pub format: Option<DeviceFormat>,
    /// For the jitter.
    pub seed: u64,
    /// Keep what was heard (off for very long runs that judge as they go).
    pub keep_tape: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            skew_ppm: 0.0,
            wake_jitter_us: 0,
            position_step_us: 0,
            period_us: 10_000,
            buffer_periods: 5,
            missed: Vec::new(),
            held_buffer: false,
            format: None,
            seed: 1,
            keep_tape: true,
        }
    }
}

/// What the DAC played from one start to its stop.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// When tick 0 was at the DAC (mono ns).
    pub start_ns: u64,
    /// Ticks per second of mono time (the rate, skewed).
    pub ticks_per_s: f64,
    pub channels: u16,
    /// Every tick's frame, interleaved, left-justified (underruns as zeros).
    pub samples: Vec<i32>,
    /// Ticks the DAC had nothing to play.
    pub dry: Vec<Range<u64>>,
    /// Frames held from before the start, played first (the held-buffer fault).
    pub stale: u64,
}

impl Run {
    pub fn ticks(&self) -> u64 {
        (self.samples.len() / usize::from(self.channels.max(1))) as u64
    }

    /// One channel, as 24-bit values.
    pub fn channel(&self, c: u16) -> Vec<i32> {
        self.left_justified(c).iter().map(|v| v >> 8).collect()
    }

    /// One channel, as left-justified 32-bit values.
    pub fn left_justified(&self, c: u16) -> Vec<i32> {
        let ch = usize::from(self.channels.max(1));
        self.samples
            .iter()
            .skip(usize::from(c))
            .step_by(ch)
            .copied()
            .collect()
    }

    /// One channel, as -1..1.
    pub fn channel_f64(&self, c: u16) -> Vec<f64> {
        self.left_justified(c)
            .iter()
            .map(|&v| f64::from(v) / 2_147_483_648.0)
            .collect()
    }

    /// The tick that played device frame `frame` (later than the frame by every underrun before
    /// it; stale frames held from before count as this start's, as the device counts them).
    pub fn tick_of(&self, frame: u64) -> u64 {
        let mut tick = frame;
        for d in &self.dry {
            if d.start <= tick {
                tick += d.end - d.start;
            }
        }
        tick
    }

    /// When tick `tick` was heard (mono ns, exact).
    pub fn heard_ns(&self, tick: u64) -> f64 {
        self.start_ns as f64 + tick as f64 * 1e9 / self.ticks_per_s
    }
}

/// Everything the DAC played, one run per start.
#[derive(Debug, Clone, Default)]
pub struct Tape {
    pub runs: Vec<Run>,
}

impl Tape {
    /// Frames played (underrun silence not counted).
    pub fn played_frames(&self) -> u64 {
        self.runs
            .iter()
            .map(|r| r.ticks() - r.dry.iter().map(|d| d.end - d.start).sum::<u64>())
            .sum()
    }
}

struct State {
    cfg: SimConfig,
    info: Option<OutputInfo>,
    renderer: Option<Box<dyn Render>>,
    /// Render is being called (between start and stop).
    rendering: bool,
    /// The DAC is playing (from start until what it holds after a stop has played).
    running: bool,
    lost: bool,
    closing: bool,
    /// Bytes rendered and not yet played.
    fifo: VecDeque<u8>,
    start_ns: u64,
    ticks_per_s: f64,
    /// Ticks the DAC has played since the start.
    ticks: u64,
    /// The last wake counted, and when the next is.
    wake: u64,
    next_wake_ns: u64,
    /// Device frames rendered and played since the start.
    written: u64,
    popped: u64,
    /// Tick of the newest frame played.
    last_tick: Option<u64>,
    in_dry: bool,
    rendered_total: u64,
    scratch: Vec<u8>,
    tape: Tape,
    events: Option<Sender<OutputEvent>>,
    rng: u64,
}

impl State {
    fn bpf(&self) -> usize {
        self.info.as_ref().map_or(1, |i| i.format.bytes_per_frame())
    }

    fn fifo_frames(&self) -> u64 {
        (self.fifo.len() / self.bpf()) as u64
    }

    fn period_ns(&self) -> f64 {
        let i = self.info.as_ref().expect("open");
        f64::from(i.period_frames) * 1e9 / self.ticks_per_s
    }

    fn tell(&self, e: OutputEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(e);
        }
    }

    /// Render whole periods while the buffer has room.
    fn fill(&mut self) {
        let Some(info) = self.info.clone() else {
            return;
        };
        let bpf = self.bpf();
        let period = u64::from(info.period_frames);
        while self.rendering
            && !self.lost
            && self.fifo_frames() + period <= u64::from(info.buffer_frames)
        {
            let Some(r) = self.renderer.as_mut() else {
                break;
            };
            let bytes = period as usize * bpf;
            self.scratch.resize(bytes, 0);
            r.render(&mut self.scratch[..bytes], info.period_frames, self.written);
            self.fifo.extend(&self.scratch[..bytes]);
            self.written += period;
            self.rendered_total += period;
        }
    }

    /// Let the DAC play every tick due by `now`.
    fn advance(&mut self, now: u64) {
        if !self.running || now < self.start_ns {
            return;
        }
        let due = ((now - self.start_ns) as f64 * self.ticks_per_s / 1e9).floor() as u64 + 1;
        if due <= self.ticks {
            return;
        }
        let n = due - self.ticks;
        let avail = self.fifo_frames().min(n);
        let bpf = self.bpf();
        let fmt = self.info.as_ref().expect("open").format.clone();
        let keep = self.cfg.keep_tape;
        let run = self.tape.runs.last_mut().expect("a run per start");
        if avail > 0 {
            let bytes = avail as usize * bpf;
            if keep {
                self.fifo.make_contiguous();
                decode_frames(&self.fifo.as_slices().0[..bytes], &fmt, &mut run.samples);
            }
            self.fifo.drain(..bytes);
            self.popped += avail;
            self.last_tick = Some(self.ticks + avail - 1);
            self.in_dry = false;
        }
        let short = n - avail;
        if short > 0 {
            if !self.rendering {
                // Stopped, and all it held has played: the DAC stops here.
                self.ticks += avail;
                self.running = false;
                return;
            }
            let from = self.ticks + avail;
            match run.dry.last_mut() {
                Some(d) if d.end == from => d.end += short,
                _ => run.dry.push(from..from + short),
            }
            if keep {
                run.samples.extend(std::iter::repeat_n(
                    0,
                    short as usize * usize::from(fmt.channels),
                ));
            }
            if !self.in_dry {
                self.in_dry = true;
                self.tell(OutputEvent::Xrun { at: self.popped });
            }
        }
        self.ticks = due;
    }

    fn pair(&self, now: u64) -> Option<PresentationPair> {
        let tick = self.last_tick?;
        let frame = self.popped - 1;
        let rate = f64::from(self.info.as_ref()?.format.rate);
        let step = (rate * f64::from(self.cfg.position_step_us) / 1e6).round() as u64;
        if step > 1 {
            return Some(PresentationPair {
                frame: frame / step * step,
                time_ns: now,
            });
        }
        let t = self.start_ns as f64 + tick as f64 * 1e9 / self.ticks_per_s;
        Some(PresentationPair {
            frame,
            time_ns: t.round() as u64,
        })
    }

    fn next_jitter(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (self.rng >> 33) as f64 / (1u64 << 31) as f64;
        (r * f64::from(self.cfg.wake_jitter_us) * 1_000.0) as u64
    }

    fn schedule(&mut self) {
        self.wake += 1;
        while self.cfg.missed.contains(&self.wake) {
            self.wake += 1;
        }
        let ideal = self.start_ns as f64 + self.wake as f64 * self.period_ns();
        self.next_wake_ns = ideal.ceil() as u64 + self.next_jitter();
    }
}

/// Puts frames of device bytes on the tape, left-justified.
pub(crate) fn decode_frames(b: &[u8], fmt: &DeviceFormat, out: &mut Vec<i32>) {
    let size = usize::from(fmt.container_bits / 8);
    for s in b.chunks_exact(size) {
        out.push(match (fmt.kind, size) {
            (SampleKind::Float, _) => {
                let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                (f64::from(v) * 2_147_483_648.0).clamp(-2_147_483_648.0, 2_147_483_647.0) as i32
            }
            (_, 2) => i32::from(i16::from_le_bytes([s[0], s[1]])) << 16,
            (_, 3) => i32::from_le_bytes([0, s[0], s[1], s[2]]),
            _ => i32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        });
    }
}

struct Shared {
    state: Mutex<State>,
    presentation: Arc<Presentation>,
    tb: Arc<dyn Timebase>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The simulated output.
pub struct SimOutput {
    shared: Arc<Shared>,
}

/// A test's hold on a simulated output: what it played, and faults on demand.
#[derive(Clone)]
pub struct SimHandle {
    shared: Arc<Shared>,
}

impl SimOutput {
    pub fn new(tb: Arc<dyn Timebase>, cfg: SimConfig) -> (SimOutput, SimHandle) {
        let rng = cfg.seed.max(1);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                cfg,
                info: None,
                renderer: None,
                rendering: false,
                running: false,
                lost: false,
                closing: false,
                fifo: VecDeque::new(),
                start_ns: 0,
                ticks_per_s: 0.0,
                ticks: 0,
                wake: 0,
                next_wake_ns: 0,
                written: 0,
                popped: 0,
                last_tick: None,
                in_dry: false,
                rendered_total: 0,
                scratch: Vec::new(),
                tape: Tape::default(),
                events: None,
                rng,
            }),
            presentation: Arc::new(Presentation::default()),
            tb,
        });
        (
            SimOutput {
                shared: shared.clone(),
            },
            SimHandle { shared },
        )
    }
}

/// The render thread: wakes once a period while the DAC runs, else polls every period.
fn thread(shared: Arc<Shared>, sleeper: Sleeper) {
    loop {
        let wake = {
            let s = shared.lock();
            if s.closing {
                return;
            }
            if s.running {
                s.next_wake_ns
            } else {
                let p = s.info.as_ref().map_or(10_000_000, |i| {
                    (f64::from(i.period_frames) * 1e9 / f64::from(i.format.rate)) as u64
                });
                sleeper.now_ns() + p.max(1)
            }
        };
        sleeper.sleep_until(wake);
        let now = shared.tb.now_ns();
        let mut s = shared.lock();
        if s.closing {
            return;
        }
        if !s.running || now < s.next_wake_ns {
            continue;
        }
        s.advance(now);
        if let Some(p) = s.pair(now) {
            shared.presentation.publish(p);
        }
        s.fill();
        s.schedule();
    }
}

impl Output for SimOutput {
    fn name(&self) -> String {
        "Simulated output".into()
    }

    fn mode(&self) -> Mode {
        Mode::Memory
    }

    fn open(&mut self, want: &OpenRequest) -> Result<OutputInfo, OutputError> {
        let mut s = self.shared.lock();
        if s.info.is_some() {
            return Err(OutputError::State("already open".into()));
        }
        let format = s.cfg.format.clone().unwrap_or(DeviceFormat {
            rate: want.rate,
            channels: want.channels,
            container_bits: 32,
            valid_bits: want.bits.min(32),
            kind: SampleKind::Int,
        });
        let period_frames =
            ((u64::from(format.rate) * u64::from(s.cfg.period_us)) / 1_000_000).max(1) as u32;
        let info = OutputInfo {
            format,
            period_frames,
            buffer_frames: period_frames * s.cfg.buffer_periods.max(1),
        };
        s.info = Some(info.clone());
        s.lost = false;
        s.closing = false;
        s.fifo.clear();
        drop(s);
        let sleeper = self.shared.tb.sleeper();
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("sim-output".into())
            .spawn(move || thread(shared, sleeper))
            .map_err(|e| OutputError::Other(e.to_string()))?;
        Ok(info)
    }

    fn start(&mut self, render: Box<dyn Render>) -> Result<(), OutputError> {
        let now = self.shared.tb.now_ns();
        let mut s = self.shared.lock();
        if s.lost {
            return Err(OutputError::Lost);
        }
        let Some(info) = s.info.clone() else {
            return Err(OutputError::State("start before open".into()));
        };
        if s.rendering {
            return Err(OutputError::State("already started".into()));
        }
        let stale = if s.cfg.held_buffer {
            s.fifo_frames()
        } else {
            s.fifo.clear();
            0
        };
        s.ticks_per_s = f64::from(info.format.rate) * (1.0 + s.cfg.skew_ppm / 1e6);
        s.start_ns = now;
        s.ticks = 0;
        s.wake = 0;
        s.written = 0;
        s.popped = 0;
        s.last_tick = None;
        s.in_dry = false;
        s.renderer = Some(render);
        s.rendering = true;
        s.running = true;
        let tps = s.ticks_per_s;
        s.tape.runs.push(Run {
            start_ns: now,
            ticks_per_s: tps,
            channels: info.format.channels,
            stale,
            ..Run::default()
        });
        self.shared.presentation.clear();
        // Filled before the DAC starts, as a real device is; then the DAC plays tick 0 now.
        s.fill();
        s.advance(now);
        if let Some(p) = s.pair(now) {
            self.shared.presentation.publish(p);
        }
        s.schedule();
        Ok(())
    }

    fn stop(&mut self) -> Option<Box<dyn Render>> {
        let mut s = self.shared.lock();
        s.rendering = false;
        s.renderer.take()
    }

    fn close(&mut self) {
        let mut s = self.shared.lock();
        s.rendering = false;
        s.running = false;
        s.renderer = None;
        s.closing = true;
        s.info = None;
    }

    fn presentation(&self) -> Arc<Presentation> {
        self.shared.presentation.clone()
    }

    fn events(&self) -> Receiver<OutputEvent> {
        let (tx, rx) = channel();
        self.shared.lock().events = Some(tx);
        rx
    }
}

impl Drop for SimOutput {
    fn drop(&mut self) {
        self.close();
    }
}

impl SimHandle {
    /// What the DAC played so far.
    pub fn tape(&self) -> Tape {
        let now = self.shared.tb.now_ns();
        let mut s = self.shared.lock();
        // The DAC plays on between wakes: up to now.
        s.advance(now);
        s.tape.clone()
    }

    /// Frames rendered since the output was made, over every start.
    pub fn rendered_frames(&self) -> u64 {
        self.shared.lock().rendered_total
    }

    pub fn info(&self) -> Option<OutputInfo> {
        self.shared.lock().info.clone()
    }

    /// The device is lost: render is no longer called, nothing more plays, `Lost` is told.
    pub fn lose(&self) {
        let mut s = self.shared.lock();
        s.lost = true;
        s.rendering = false;
        s.running = false;
        s.renderer = None;
        s.fifo.clear();
        s.tell(OutputEvent::Lost);
    }
}
