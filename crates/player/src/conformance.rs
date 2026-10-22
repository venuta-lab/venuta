//! The output conformance suite (contract 5.2): what every `Output` must do, checked the same way
//! on the simulated output in CI and on WASAPI and AAudio on hardware (those runs are ignored
//! in CI and run by hand).
//!
//! `run` drives an output through its whole life with a -6 dBFS tone: open; 10 s of a 1 kHz
//! tone with render's `at` contiguous and periods no longer than promised; the presentation
//! monotonic and within 1 ms of the frames rendered (never ahead of them, never more behind
//! than the most of the latency the output learned and its band, steady by the end of the
//! tone), at the DAC's own pace; the tone's 30 ms fade and silence, then a stop that hands
//! the renderer back, never calls it again, and lets the tail play; a restart from device frame
//! 0 (600 Hz), another (1.5 kHz) straight after a stop; close, and open again.
//!
//! What can only be heard is checked on the simulated output's tape by `check_tape`: every frame
//! the DAC played is the frame rendered for that place (bit for bit, so nothing stale plays
//! after a restart and nothing sounds after the silence before a stop); the tests that run the
//! suite also put the tape through the sine lock (`audio-check`). On hardware, the test kit's
//! loopback capture does that part.

use crate::output::{DeviceFormat, SampleKind};
use crate::output_api::{
    Latency, OpenRequest, Output, OutputEvent, OutputInfo, PresentationPair, Render,
};
use crate::sim::{Run, Tape};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use together::time::Timebase;

const MS: u64 = 1_000_000;
/// The tone's level: -6 dBFS.
const LEVEL: f64 = 0.5;
const PHASE: f64 = 0.3;
const FADE_MS: u32 = 30;
const NONE: u64 = u64::MAX;

/// A tone renderer: `LEVEL x sin(2 pi f k / rate + 0.3)` at device frame k, every channel, then
/// on `fade_out` a 30 ms raised-cosine fade from the next period on, and silence. It keeps count
/// of what render was asked, in atomics.
#[derive(Clone)]
pub struct ToneRender {
    s: Arc<Tone>,
}

struct Tone {
    freq: f64,
    fmt: DeviceFormat,
    fade: AtomicBool,
    tail_from: AtomicU64,
    first_at: AtomicU64,
    next: AtomicU64,
    calls: AtomicU64,
    rendered: AtomicU64,
    broken: AtomicU64,
    longest: AtomicU64,
}

struct ToneBox(Arc<Tone>);

impl ToneRender {
    pub fn new(freq: f64, fmt: DeviceFormat) -> ToneRender {
        ToneRender {
            s: Arc::new(Tone {
                freq,
                fmt,
                fade: AtomicBool::new(false),
                tail_from: AtomicU64::new(NONE),
                first_at: AtomicU64::new(NONE),
                next: AtomicU64::new(0),
                calls: AtomicU64::new(0),
                rendered: AtomicU64::new(0),
                broken: AtomicU64::new(0),
                longest: AtomicU64::new(0),
            }),
        }
    }

    /// The tone as it was rendered in a run whose fade began at device frame `tail_from` (a
    /// report's `tones`): to compare what an output played, or recorded, with it.
    pub fn faded(freq: f64, fmt: DeviceFormat, tail_from: u64) -> ToneRender {
        let t = ToneRender::new(freq, fmt);
        t.s.tail_from.store(tail_from, Ordering::SeqCst);
        t
    }

    /// The renderer to hand an output.
    pub fn render_box(&self) -> Box<dyn Render> {
        Box::new(ToneBox(self.s.clone()))
    }

    pub fn fade_out(&self) {
        self.s.fade.store(true, Ordering::SeqCst);
    }

    /// Where the fade began (device frame), once it has.
    pub fn tail_from(&self) -> Option<u64> {
        let t = self.s.tail_from.load(Ordering::SeqCst);
        (t != NONE).then_some(t)
    }

    /// Where the silence after the fade begins.
    pub fn silent_from(&self) -> Option<u64> {
        Some(self.tail_from()? + self.fade_frames())
    }

    fn fade_frames(&self) -> u64 {
        u64::from(self.s.fmt.rate * FADE_MS / 1000)
    }

    pub fn first_at(&self) -> Option<u64> {
        let t = self.s.first_at.load(Ordering::SeqCst);
        (t != NONE).then_some(t)
    }

    pub fn calls(&self) -> u64 {
        self.s.calls.load(Ordering::SeqCst)
    }

    /// Frames rendered so far (the device frame of the next period).
    pub fn rendered(&self) -> u64 {
        self.s.rendered.load(Ordering::SeqCst)
    }

    /// Calls whose `at` wasn't the frame after the last call's.
    pub fn broken(&self) -> u64 {
        self.s.broken.load(Ordering::SeqCst)
    }

    /// The most frames one call was asked for.
    pub fn longest(&self) -> u64 {
        self.s.longest.load(Ordering::SeqCst)
    }

    /// The tone's value at device frame `k`, -1..1, the fade included.
    pub fn value(&self, k: u64) -> f64 {
        self.s.value(k, self.s.tail_from.load(Ordering::SeqCst))
    }

    /// Frame `k` of one channel as the device holds it, left-justified (as the simulated
    /// output's tape keeps it).
    pub fn left_justified(&self, k: u64) -> i32 {
        let mut b = [0u8; 4];
        let size = usize::from(self.s.fmt.container_bits / 8);
        write_sample(&mut b[..size], &self.s.fmt, self.value(k));
        let mut v = Vec::with_capacity(1);
        crate::sim::decode_frames(&b[..size], &self.s.fmt, &mut v);
        v[0]
    }
}

impl Tone {
    fn value(&self, k: u64, tail_from: u64) -> f64 {
        let rate = f64::from(self.fmt.rate);
        let fade = u64::from(self.fmt.rate * FADE_MS / 1000);
        let g = if k < tail_from {
            1.0
        } else if k - tail_from < fade {
            let x = (k - tail_from) as f64 / fade as f64;
            0.5 + 0.5 * (std::f64::consts::PI * x).cos()
        } else {
            0.0
        };
        if g == 0.0 {
            return 0.0;
        }
        g * LEVEL * (std::f64::consts::TAU * self.freq * k as f64 / rate + PHASE).sin()
    }
}

/// One sample in the device's format.
fn write_sample(out: &mut [u8], fmt: &DeviceFormat, v: f64) {
    let v = v.clamp(-1.0, 1.0);
    match (fmt.kind, out.len()) {
        (SampleKind::Float, _) => out.copy_from_slice(&(v as f32).to_le_bytes()),
        (_, 2) => out.copy_from_slice(&((v * 32_767.0).round() as i16).to_le_bytes()),
        (_, 3) => out.copy_from_slice(&((v * 8_388_607.0).round() as i32).to_le_bytes()[..3]),
        _ => {
            let bits = u32::from(fmt.valid_bits.clamp(8, 32));
            let full = f64::from((1u32 << (bits - 1)) - 1);
            let s = ((v * full).round() as i64) << (32 - bits);
            out.copy_from_slice(&(s as i32).to_le_bytes());
        }
    }
}

impl Render for ToneBox {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        let s = &self.0;
        let _ = s
            .first_at
            .compare_exchange(NONE, at, Ordering::SeqCst, Ordering::SeqCst);
        if at != s.next.load(Ordering::SeqCst) {
            s.broken.fetch_add(1, Ordering::SeqCst);
        }
        if s.fade.load(Ordering::SeqCst) {
            let _ = s
                .tail_from
                .compare_exchange(NONE, at, Ordering::SeqCst, Ordering::SeqCst);
        }
        let tail = s.tail_from.load(Ordering::SeqCst);
        let size = usize::from(s.fmt.container_bits / 8);
        let bpf = size * usize::from(s.fmt.channels);
        for (k, frame) in out.chunks_exact_mut(bpf).take(frames as usize).enumerate() {
            let v = s.value(at + k as u64, tail);
            for c in frame.chunks_exact_mut(size) {
                write_sample(c, &s.fmt, v);
            }
        }
        s.next.store(at + u64::from(frames), Ordering::SeqCst);
        s.rendered.store(at + u64::from(frames), Ordering::SeqCst);
        s.longest.fetch_max(u64::from(frames), Ordering::SeqCst);
        s.calls.fetch_add(1, Ordering::SeqCst);
    }
}

/// What a run found, for `check_tape` and for the record.
#[derive(Debug, Clone)]
pub struct Report {
    pub info: OutputInfo,
    /// Frames rendered of the 10 s tone.
    pub rendered: u64,
    /// The DAC's pace against its rate, in ppm.
    pub pace_ppm: f64,
    /// The furthest the DAC was behind the frames rendered, in ms (at most the learned
    /// latency'"'"'s most and 1).
    pub worst_behind_ms: f64,
    /// The three tones (Hz), each with where its fade began and its silence began.
    pub tones: Vec<(f64, u64, u64)>,
    /// Gaps the output said it had.
    pub xruns: usize,
    /// The latency the output learned, at the end of the 10 s tone (steady, or the run fails);
    /// the driver'"'"'s own figure is `info.buffer_frames`.
    pub latency: Option<Latency>,
}

fn fail(what: impl Into<String>) -> String {
    format!("output conformance: {}", what.into())
}

/// Wait until `cond`, a period at a time, at most `max_ns`.
fn wait_for(time: &dyn Timebase, max_ns: u64, mut cond: impl FnMut() -> bool) -> bool {
    let until = time.now_ns() + max_ns;
    while !cond() {
        if time.now_ns() >= until {
            return false;
        }
        time.pass(10 * MS);
    }
    true
}

/// Watches the presentation against what was rendered.
struct Watch {
    rate: f64,

    tolerance: f64,
    last: Option<PresentationPair>,
    first: Option<PresentationPair>,
    worst_behind: f64,
}

impl Watch {
    fn new(info: &OutputInfo) -> Watch {
        let rate = f64::from(info.format.rate);
        Watch {
            rate,

            tolerance: rate / 1000.0,
            last: None,
            first: None,
            worst_behind: 0.0,
        }
    }

    fn look(&mut self, out: &dyn Output, tone: &ToneRender, now: u64) -> Result<(), String> {
        let before = tone.rendered() as f64;
        let Some(p) = out.presentation().read() else {
            return Ok(());
        };
        let after = tone.rendered() as f64;
        if let Some(l) = self.last
            && (p.frame < l.frame || p.time_ns < l.time_ns)
        {
            return Err(fail(format!(
                "the presentation went back: {l:?} then {p:?}"
            )));
        }
        self.last = Some(p);
        self.first.get_or_insert(p);
        // Where the DAC is now, from the pair.
        let at_dac = p.frame as f64 + now.saturating_sub(p.time_ns) as f64 * self.rate / 1e9;
        if p.frame as f64 >= after || at_dac > after + self.tolerance {
            return Err(fail(format!(
                "the DAC is at frame {at_dac:.0}, past the {after} rendered"
            )));
        }
        let behind = before - at_dac;
        self.worst_behind = self.worst_behind.max(behind);
        // Against the latency the output learned (the driver's own figure, `buffer_frames`,
        // is often less than it holds), once it is steady: at most its most and its band (the
        // fill-to-fill wobble, at least 1 ms).
        if let Some(l) = out.latency().filter(|l| l.settled)
            && behind > f64::from(l.most) + f64::from(l.band)
        {
            return Err(fail(format!(
                "the DAC is {behind:.0} frames behind what was rendered, more than the most it learned ({}) and its band ({})",
                l.most, l.band
            )));
        }
        Ok(())
    }

    fn pace_ppm(&self) -> f64 {
        match (self.first, self.last) {
            (Some(a), Some(b)) if b.time_ns > a.time_ns => {
                let per_s = (b.frame - a.frame) as f64 * 1e9 / (b.time_ns - a.time_ns) as f64;
                (per_s / self.rate - 1.0) * 1e6
            }
            _ => 0.0,
        }
    }
}

/// Play `tone` for `secs`, watching; then fade it out and wait until its fade has played.
fn play(
    out: &mut dyn Output,
    time: &dyn Timebase,
    info: &OutputInfo,
    tone: &ToneRender,
    secs: u64,
) -> Result<Watch, String> {
    out.start(tone.render_box())
        .map_err(|e| fail(format!("start: {e}")))?;
    let mut w = Watch::new(info);
    if !wait_for(time, 1_000 * MS, || {
        out.presentation().read().is_some() && tone.first_at().is_some()
    }) {
        return Err(fail(
            "no presentation and no render within 1 s of the start",
        ));
    }
    if tone.first_at() != Some(0) {
        return Err(fail(format!(
            "the first period after a start was at frame {:?}, not 0",
            tone.first_at()
        )));
    }
    let until = time.now_ns() + secs * 1_000 * MS;
    while time.now_ns() < until {
        time.pass(10 * MS);
        w.look(out, tone, time.now_ns())?;
    }
    if tone.broken() > 0 {
        return Err(fail(format!(
            "{} periods didn't start where the last one ended",
            tone.broken()
        )));
    }
    if tone.longest() > u64::from(info.period_frames) {
        return Err(fail(format!(
            "asked for {} frames at once, more than the period of {}",
            tone.longest(),
            info.period_frames
        )));
    }
    tone.fade_out();
    let played_fade = wait_for(time, 2_000 * MS, || {
        let p = out.presentation().read();
        tone.silent_from().zip(p).is_some_and(|(s, p)| p.frame >= s)
    });
    if !played_fade {
        return Err(fail("the tone's fade never reached the DAC"));
    }
    Ok(w)
}

/// Stop, and check render is never called again.
fn stop(out: &mut dyn Output, time: &dyn Timebase, tone: &ToneRender) -> Result<(), String> {
    if out.stop().is_none() {
        return Err(fail("stop didn't hand the renderer back"));
    }
    let after = tone.calls();
    time.pass(200 * MS);
    if tone.calls() != after {
        return Err(fail("render was called after stop returned"));
    }
    Ok(())
}

/// Run the suite on `out` (closed when given), with time passing on `time`.
pub fn run(
    out: &mut dyn Output,
    time: &dyn Timebase,
    want: &OpenRequest,
) -> Result<Report, String> {
    let info = out.open(want).map_err(|e| fail(format!("open: {e}")))?;
    if info.format.rate == 0
        || info.format.channels == 0
        || info.period_frames == 0
        || info.buffer_frames < info.period_frames
    {
        return Err(fail(format!("an output that can't be: {info:?}")));
    }
    let events = out.events();
    let mut tones = Vec::new();

    // 10 s of 1 kHz, watched; its fade and silence; a stop once the silence is all it holds.
    let a = ToneRender::new(1_000.0, info.format.clone());
    let w = play(out, time, &info, &a, 10)?;
    // After 10 s the output must have learned a steady latency (and the DAC was never further
    // behind than it said, checked all along once it was steady).
    let latency = out.latency();
    if !latency.is_some_and(|l| l.settled && l.frames <= l.most) {
        return Err(fail(format!(
            "no steady latency learned in 10 s: {latency:?}"
        )));
    }
    let buffer_ns = u64::from(info.buffer_frames) * 1_000_000_000 / u64::from(info.format.rate);
    let silent = a.silent_from().unwrap_or(0);
    if !wait_for(time, 2_000 * MS, || {
        a.rendered() >= silent + u64::from(info.buffer_frames)
    }) {
        return Err(fail("the silence after the fade was never rendered"));
    }
    let pace_ppm = w.pace_ppm();
    if pace_ppm.abs() > 1_000.0 {
        return Err(fail(format!("the DAC runs {pace_ppm:.0} ppm off its rate")));
    }
    let rendered = a.rendered();
    stop(out, time, &a)?;
    time.pass(buffer_ns);
    tones.push((1_000.0, a.tail_from().unwrap_or(0), silent));

    // A restart from frame 0, played out properly.
    let b = ToneRender::new(600.0, info.format.clone());
    play(out, time, &info, &b, 1)?;
    // Straight on, with the fade played and only its silence held: stop, and start again at
    // once. The new start must begin with its own frame 0.
    let b_calls = b.calls();
    if out.stop().is_none() {
        return Err(fail("stop didn't hand the renderer back"));
    }
    tones.push((
        600.0,
        b.tail_from().unwrap_or(0),
        b.silent_from().unwrap_or(0),
    ));
    let c = ToneRender::new(1_500.0, info.format.clone());
    play(out, time, &info, &c, 1)?;
    if b.calls() != b_calls {
        return Err(fail("render was called after stop returned"));
    }
    stop(out, time, &c)?;
    time.pass(buffer_ns);
    tones.push((
        1_500.0,
        c.tail_from().unwrap_or(0),
        c.silent_from().unwrap_or(0),
    ));

    out.close();
    out.open(want)
        .map_err(|e| fail(format!("open again after close: {e}")))?;
    out.close();

    let seen: Vec<OutputEvent> = events.try_iter().collect();
    if seen.contains(&OutputEvent::Lost) {
        return Err(fail("the device was lost during the run"));
    }
    Ok(Report {
        info,
        rendered,
        pace_ppm,
        worst_behind_ms: w.worst_behind * 1000.0 / w.rate,
        latency,
        tones,
        xruns: seen
            .iter()
            .filter(|e| matches!(e, OutputEvent::Xrun { .. }))
            .count(),
    })
}

/// What the simulated output's DAC played in `run`: every frame as rendered for its place, so
/// silence from each fade's end and each start beginning with its own frame 0.
pub fn check_tape(tape: &Tape, report: &Report) -> Result<(), String> {
    if tape.runs.len() != report.tones.len() {
        return Err(fail(format!(
            "{} starts on the tape, {} in the run",
            tape.runs.len(),
            report.tones.len()
        )));
    }
    for (k, (run, &(freq, tail_from, silent_from))) in
        tape.runs.iter().zip(&report.tones).enumerate()
    {
        check_run(run, &report.info.format, k, freq, tail_from, silent_from)?;
    }
    Ok(())
}

fn check_run(
    run: &Run,
    fmt: &DeviceFormat,
    k: usize,
    freq: f64,
    tail_from: u64,
    silent_from: u64,
) -> Result<(), String> {
    let tone = ToneRender::new(freq, fmt.clone());
    tone.s.tail_from.store(tail_from, Ordering::SeqCst);
    let heard = run.left_justified(0);
    if (heard.len() as u64) < silent_from {
        return Err(fail(format!(
            "start {k}: the DAC stopped at {} before the tone's fade ended at {silent_from}",
            heard.len()
        )));
    }
    for (i, &v) in heard.iter().enumerate() {
        let want = tone.left_justified(i as u64);
        if (i64::from(v) - i64::from(want)).abs() > 512 {
            return Err(fail(if i == 0 {
                format!(
                    "start {k}: stale frames after a restart (the first frame heard is {v}, its own frame 0 is {want})"
                )
            } else if i as u64 >= silent_from {
                format!("start {k}: sound at frame {i} after the silence before the stop began")
            } else {
                format!("start {k}: frame {i} heard as {v}, rendered as {want}")
            }));
        }
    }
    Ok(())
}
