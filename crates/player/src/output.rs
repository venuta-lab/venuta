//! Where the sound goes: a sound device (WASAPI exclusive or shared on Windows), or memory
//! (for checks). Samples arrive left-justified `i32`; each device format gets them packed its
//! way. Integer formats at least as deep as the source carry the exact values.
//!
//! Two generations live here. Engine 1 writes to a `Device` (`MemoryDevice`, and
//! `wasapi_out::WasapiDevice`), waiting and writing on its own thread. Output v2 (contract 5.2,
//! `output_api`) is callback shaped: the output owns a real-time thread and calls the renderer
//! from it. Its platform-neutral parts are here, for every platform's output to share (WASAPI
//! now, AAudio later): the renderer's hand-over (`RenderCell`), the period filler (`Pump`), the
//! device position to presentation logic (`PositionMap`), the events (`RtEvents`, `EventSink`)
//! and a pull loop for APIs that wake us when they want frames (`PullDevice`, `PullOpener`,
//! `PullOutput`, which is an `Output`).

use crate::decode::SourceFormat;
use crate::output_api::{
    Latency, LatencyCell, LatencyTracker, OpenRequest, Output, OutputError, OutputEvent,
    OutputInfo, Presentation, PresentationPair, Render,
};
use serde::Serialize;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use together::time::{Sleeper, Timebase};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    Int,
    Float,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceFormat {
    pub rate: u32,
    pub channels: u16,
    /// Bytes per sample × 8: 16, 24 or 32.
    pub container_bits: u16,
    /// Bits that carry the signal (24 in a 32-bit container…).
    pub valid_bits: u16,
    pub kind: SampleKind,
}

impl DeviceFormat {
    pub fn bytes_per_frame(&self) -> usize {
        usize::from(self.container_bits / 8) * usize::from(self.channels)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Straight to the device, at the song's own format; nothing else can play meanwhile.
    Exclusive,
    /// Through Windows' mixer, which converts to its own rate.
    Shared,
    /// Into memory, for checks.
    Memory,
}

/// A place to send sound.
pub trait Device {
    fn name(&self) -> String;
    fn mode(&self) -> Mode;
    /// Get ready for songs of this format. Returns the format the device will run at.
    fn open(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String>;
    /// Frames the device can take right now, without waiting.
    fn available(&mut self) -> Result<usize, String>;
    /// Wait until the device wants more, then say how many frames.
    fn wait(&mut self) -> Result<usize, String>;
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<(), String>;
    /// Frames the device holds before you hear them (its buffer): the output latency.
    fn latency_frames(&self) -> usize {
        0
    }
    /// Let go of what it holds and not played yet (stopped only).
    fn flush(&mut self) {}
    /// Frames written but not heard yet, by the device's own clock (exact, where it has one).
    fn queued_frames(&self) -> Option<usize> {
        None
    }
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self) -> Result<(), String>;
    /// Let go of the device (other apps can use it).
    fn close(&mut self);
}

/// Pack left-justified samples for the device, applying `gain` when it isn't 1 (which makes
/// the output no longer bit-perfect).
pub fn encode(samples: &[i32], fmt: &DeviceFormat, gain: f32, out: &mut Vec<u8>) {
    out.clear();
    let scaled = |s: i32| -> i32 {
        if gain >= 0.999_999 {
            s
        } else {
            (f64::from(s) * f64::from(gain))
                .round()
                .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
        }
    };
    match (fmt.kind, fmt.container_bits) {
        (SampleKind::Float, _) => {
            out.reserve(samples.len() * 4);
            for &s in samples {
                let v = s as f32 / 2_147_483_648.0 * gain.min(1.0);
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        (SampleKind::Int, 16) => {
            out.reserve(samples.len() * 2);
            for &s in samples {
                out.extend_from_slice(&((scaled(s) >> 16) as i16).to_le_bytes());
            }
        }
        (SampleKind::Int, 24) => {
            out.reserve(samples.len() * 3);
            for &s in samples {
                out.extend_from_slice(&(scaled(s) >> 8).to_le_bytes()[..3]);
            }
        }
        (SampleKind::Int, _) => {
            out.reserve(samples.len() * 4);
            // A 32-bit container with fewer valid bits keeps the value left-justified, as
            // the source already is: the low bits are zero for a 16- or 24-bit song.
            let mask = if fmt.valid_bits >= 32 {
                -1i32
            } else {
                !((1i32 << (32 - fmt.valid_bits)) - 1)
            };
            for &s in samples {
                out.extend_from_slice(&(scaled(s) & mask).to_le_bytes());
            }
        }
    }
}

/// Fit `samples` (interleaved, `from` channels) to `to` channels: mono doubles to stereo;
/// extra channels are dropped; missing ones are silent.
pub fn remap(samples: &[i32], from: u16, to: u16, out: &mut Vec<i32>) {
    out.clear();
    let (f, t) = (usize::from(from.max(1)), usize::from(to.max(1)));
    if f == t {
        out.extend_from_slice(samples);
        return;
    }
    for frame in samples.chunks_exact(f) {
        for c in 0..t {
            out.push(if f == 1 {
                frame[0]
            } else {
                frame.get(c).copied().unwrap_or(0)
            });
        }
    }
}

/// Nothing changes the samples between the file and the device.
pub fn bit_perfect(src: &SourceFormat, dev: &DeviceFormat, mode: Mode, gain: f32) -> bool {
    src.lossless
        && mode != Mode::Shared
        && dev.kind == SampleKind::Int
        && dev.valid_bits >= src.bits
        && dev.rate == src.rate
        && dev.channels == src.channels
        && gain >= 0.999_999
}

// --- Memory -------------------------------------------------------------------------------

/// What a memory device received: each format it was opened with, and every byte written.
#[derive(Debug, Default)]
pub struct Captured {
    pub formats: Vec<DeviceFormat>,
    pub bytes: Vec<u8>,
    /// Frames written while each format was open (same order as `formats`).
    pub frames: Vec<usize>,
    /// A paced device: when its first frame played (frame k plays k/rate after it).
    pub started_at: Option<std::time::Instant>,
    /// How many times the device was told to let go of what it held (stopped).
    pub flushes: usize,
    /// How long opening it takes (a real device can take a moment): for checks.
    pub open_delay: Option<std::time::Duration>,
    /// A paced device: for each write, where it starts in `bytes` and when its first frame is
    /// heard (after what the device still held; at once when it had run dry).
    pub heard: Vec<(usize, std::time::Instant)>,
    /// A paced device whose clock runs this many parts per million fast (negative: slow) against
    /// the computer's, as every real sound card's does a little: for checks.
    pub drift_ppm: f64,
}

impl Captured {
    /// When the frame starting at byte `at` of `bytes` was heard, on a paced device, given its
    /// bytes per frame and rate.
    pub fn heard_at(&self, at: usize, frame_bytes: usize, rate: u32) -> Option<std::time::Instant> {
        let k = self
            .heard
            .partition_point(|(from, _)| *from <= at)
            .checked_sub(1)?;
        let (from, when) = self.heard[k];
        let frames = (at - from) / frame_bytes.max(1);
        Some(when + std::time::Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1))))
    }
}

pub struct MemoryDevice {
    pub captured: Arc<Mutex<Captured>>,
    /// Frames it takes per wait (like a device period).
    pub period: usize,
    /// Taking frames no faster than real time, holding a device's worth of buffer.
    pub paced: Option<Pace>,
}

/// A memory device's clock: when it opened and how many frames it has taken, so a wait lasts
/// until that many frames would have played (keeping `BUFFER_FRAMES` ahead, like a device).
#[derive(Debug)]
pub struct Pace {
    since: Option<std::time::Instant>,
    taken: u64,
    rate: u32,
    /// Its clock against the computer's (1: the same).
    speed: f64,
}

impl Pace {
    /// Frames played by now.
    fn played(&self, since: std::time::Instant) -> u64 {
        (since.elapsed().as_secs_f64() * f64::from(self.rate.max(1)) * self.speed) as u64
    }

    /// When frame `k` (counted from the first) is heard.
    fn at(&self, since: std::time::Instant, k: u64) -> std::time::Instant {
        since
            + std::time::Duration::from_secs_f64(
                k as f64 / f64::from(self.rate.max(1)) / self.speed,
            )
    }
}

impl Default for Pace {
    fn default() -> Self {
        Pace {
            since: None,
            taken: 0,
            rate: 0,
            speed: 1.0,
        }
    }
}

impl Pace {
    /// How far ahead of what's been played the device holds, in frames.
    pub const BUFFER_FRAMES: u64 = 960;
}

impl Device for MemoryDevice {
    fn name(&self) -> String {
        "Memory".into()
    }
    fn mode(&self) -> Mode {
        Mode::Memory
    }
    fn open(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String> {
        // Like an ideal DAC: takes every song at its own rate and depth.
        let f = DeviceFormat {
            rate: src.rate,
            channels: src.channels,
            container_bits: 32,
            valid_bits: src.bits.min(32),
            kind: SampleKind::Int,
        };
        let (delay, drift) = if let Ok(mut c) = self.captured.lock() {
            c.formats.push(f.clone());
            c.frames.push(0);
            (c.open_delay, c.drift_ppm)
        } else {
            (None, 0.0)
        };
        if let Some(d) = delay {
            std::thread::sleep(d);
        }
        if let Some(p) = &mut self.paced {
            *p = Pace {
                since: None,
                taken: 0,
                rate: f.rate,
                speed: 1.0 + drift / 1e6,
            };
        }
        Ok(f)
    }
    fn available(&mut self) -> Result<usize, String> {
        Ok(self.period)
    }
    fn wait(&mut self) -> Result<usize, String> {
        if let Some(p) = &mut self.paced {
            let since = *p.since.get_or_insert_with(std::time::Instant::now);
            // Until there's room for another period after what has played.
            let ahead = p.taken.saturating_sub(p.played(since));
            if ahead + self.period as u64 > Pace::BUFFER_FRAMES {
                let over = ahead + self.period as u64 - Pace::BUFFER_FRAMES;
                std::thread::sleep(std::time::Duration::from_secs_f64(
                    over as f64 / f64::from(p.rate.max(1)) / p.speed,
                ));
            }
        }
        Ok(self.period)
    }
    fn latency_frames(&self) -> usize {
        if self.paced.is_some() {
            Pace::BUFFER_FRAMES as usize
        } else {
            0
        }
    }
    fn queued_frames(&self) -> Option<usize> {
        // Frame k plays k/rate after the first: what's taken and not played yet.
        let p = self.paced.as_ref()?;
        Some(p.taken.saturating_sub(p.played(p.since?)) as usize)
    }
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<(), String> {
        if let Some(p) = &mut self.paced {
            let since = *p.since.get_or_insert_with(std::time::Instant::now);
            // Run dry (nothing written for a while, as when stopped): what comes now is heard
            // from now, as on a real device.
            p.taken = p.taken.max(p.played(since));
            let at = p.at(since, p.taken);
            p.taken += frames as u64;
            if let Ok(mut c) = self.captured.lock() {
                c.started_at.get_or_insert(since);
                let from = c.bytes.len();
                c.heard.push((from, at));
            }
        }
        if let Ok(mut c) = self.captured.lock() {
            c.bytes.extend_from_slice(bytes);
            if let Some(f) = c.frames.last_mut() {
                *f += frames;
            }
        }
        Ok(())
    }
    fn flush(&mut self) {
        if let Ok(mut c) = self.captured.lock() {
            c.flushes += 1;
        }
    }
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) {}
}

// --- Output v2: the renderer's hand-over ----------------------------------------------------

const CELL_EMPTY: u8 = 0;
const CELL_HELD: u8 = 1;
const CELL_CALLING: u8 = 2;
const CELL_BUSY: u8 = 3;

/// The renderer, between the control thread (`install`, `take`) and an output's real-time
/// thread (`render`). An atomic state guards the one box: empty, held, in a render call, or
/// being put in or taken out. The real-time side never waits (a call finds the cell busy and
/// does nothing); `take` waits only for a render call already running, never for the device.
///
/// Each renderer put in has its own generation (one per start), and the real-time side calls
/// only the generation it started with: a renderer put in for the next start, before the
/// render thread has seen that start, is never called with the last start's frames.
pub struct RenderCell {
    state: AtomicU8,
    slot: UnsafeCell<Option<Box<dyn Render>>>,
    /// The generation of the renderer in the slot (or of the last one): written only while busy.
    generation: AtomicU64,
    /// A take is waiting for the call running to end.
    taking: AtomicBool,
}

// SAFETY: `slot` is read or written only by the thread that moved `state` from empty or held to
// busy or calling, and only until it moves `state` back (Acquire on the move in, Release on the
// move out), so no two threads ever touch it at once. The box it holds is `Send`.
unsafe impl Sync for RenderCell {}

/// Puts the state back to held when a render call ends, even by a panic.
struct Called<'a>(&'a AtomicU8);

impl Drop for Called<'_> {
    fn drop(&mut self) {
        self.0.store(CELL_HELD, Ordering::Release);
    }
}

impl RenderCell {
    pub fn new() -> RenderCell {
        RenderCell {
            state: AtomicU8::new(CELL_EMPTY),
            slot: UnsafeCell::new(None),
            generation: AtomicU64::new(0),
            taking: AtomicBool::new(false),
        }
    }

    /// Put a renderer in (an empty cell only; else it comes back). Returns its generation.
    pub fn install(&self, r: Box<dyn Render>) -> Result<u64, Box<dyn Render>> {
        if self
            .state
            .compare_exchange(CELL_EMPTY, CELL_BUSY, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(r);
        }
        // SAFETY: the state is busy: this thread alone has the slot.
        unsafe { *self.slot.get() = Some(r) };
        let generation = self.generation.load(Ordering::Relaxed) + 1;
        self.generation.store(generation, Ordering::Relaxed);
        self.state.store(CELL_HELD, Ordering::Release);
        Ok(generation)
    }

    /// Take the renderer out: at once, or after the render call running now. `None` if empty.
    /// Once this returns, `render` is never called on it again.
    pub fn take(&self) -> Option<Box<dyn Render>> {
        // Asked first, so the real-time side starts no new call; then the running one ends.
        self.taking.store(true, Ordering::Release);
        let mut spins = 0u32;
        loop {
            match self.state.compare_exchange(
                CELL_HELD,
                CELL_BUSY,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // SAFETY: the state is busy: this thread alone has the slot.
                    let r = unsafe { (*self.slot.get()).take() };
                    self.state.store(CELL_EMPTY, Ordering::Release);
                    self.taking.store(false, Ordering::Release);
                    return r;
                }
                Err(CELL_EMPTY) => {
                    self.taking.store(false, Ordering::Release);
                    return None;
                }
                // A render call (well under a millisecond), or another install or take.
                Err(_) => {
                    spins += 1;
                    if spins < 64 {
                        std::hint::spin_loop();
                    } else {
                        std::thread::yield_now();
                    }
                }
            }
        }
    }

    /// From the real-time thread: call the renderer if the cell holds the one of `generation`.
    /// Never waits: an empty cell, another generation, or one being taken this instant gives
    /// `false` and nothing is called.
    pub fn render(&self, generation: u64, out: &mut [u8], frames: u32, at: u64) -> bool {
        // A take waiting: no new call (so it waits for the one running at most).
        if self.taking.load(Ordering::Acquire) {
            return false;
        }
        if self
            .state
            .compare_exchange(
                CELL_HELD,
                CELL_CALLING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }
        let _back = Called(&self.state);
        if self.generation.load(Ordering::Relaxed) != generation {
            return false;
        }
        // SAFETY: the state is calling: this thread alone has the slot until `_back` drops.
        if let Some(r) = unsafe { (*self.slot.get()).as_mut() } {
            r.render(out, frames, at);
        }
        true
    }

    /// The generation of the renderer in the cell (or of the last one).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.state.load(Ordering::Acquire) == CELL_EMPTY
    }
}

impl Default for RenderCell {
    fn default() -> Self {
        RenderCell::new()
    }
}

// --- Output v2: filling a period ------------------------------------------------------------

/// Fills the device's buffer from the renderer: calls of at most `period_frames`, `at`
/// counting device frames from 0 at each start, contiguous; silence for any part with no
/// renderer (taken during the fill, or none given: draining).
#[derive(Debug, Clone)]
pub struct Pump {
    bpf: usize,
    period: u32,
    written: u64,
    made: u64,
}

impl Pump {
    pub fn new(format: &DeviceFormat, period_frames: u32) -> Pump {
        Pump {
            bpf: format.bytes_per_frame().max(1),
            period: period_frames.max(1),
            written: 0,
            made: 0,
        }
    }

    /// A start: device frame 0 again.
    pub fn restart(&mut self) {
        self.written = 0;
        self.made = 0;
    }

    /// Fill `out` (`frames` frames of the device's format) from the renderer of `cell`'s
    /// generation. Returns the frames the renderer made; the rest is silence.
    pub fn fill(&mut self, cell: Option<(&RenderCell, u64)>, out: &mut [u8], frames: u32) -> u32 {
        let frames = frames.min((out.len() / self.bpf) as u32);
        let (mut done, mut made) = (0u32, 0u32);
        let mut live = cell;
        while done < frames {
            let n = (frames - done).min(self.period);
            let chunk = &mut out[done as usize * self.bpf..(done + n) as usize * self.bpf];
            let at = self.written + u64::from(done);
            match live {
                Some((c, generation)) if c.render(generation, chunk, n, at) => {
                    made += n;
                    self.made = at + u64::from(n);
                }
                _ => {
                    // Once taken, the rest of the fill is silence too.
                    live = None;
                    chunk.fill(0);
                }
            }
            done += n;
        }
        self.written += u64::from(frames);
        made
    }

    /// Device frames handed over since the start.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// The device frame after the last one the renderer made.
    pub fn made(&self) -> u64 {
        self.made
    }
}

// --- Output v2: presentation and running dry ------------------------------------------------

/// What one position reading says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    /// A pair to publish: this device frame (as `render` was given them) at the DAC then.
    pub pair: Option<PresentationPair>,
    /// The DAC ran dry: the gap began at this device frame.
    pub xrun: Option<u64>,
}

/// Turns a device's readings into presentation pairs, and finds where the DAC ran dry.
///
/// Shared mode (an engine that reads the buffer about a period at a time and never stops: it
/// mixes silence for a stream with nothing to give). Two clocks of the stream's own frames
/// are read at every wake: what the engine has read (frames handed over less what the device
/// still holds) and what is at the DAC (the position). Between them is the engine's pipeline,
/// which the stream's own gaps don't change, so their difference, taken at the same point of
/// every period, is steady (the usual pipeline: the median of 16 samples, once the DAC has run
/// for 100 ms).
///
/// - A gap is suspected when, at a wake, the device holds less than a period (its next pass
///   can come short), or a fill took nearly as long as what it held. Only a suspect: an engine
///   that reads a burst (as at a start, filling its pipeline) leaves little held without any
///   gap, and at a start there is no usual pipeline yet, so suspects are not taken then.
/// - A gap is certain when the position falls behind the time passed by more than 1 ms (the
///   DAC sat in silence), or when, after a suspect, the pipeline differs from the usual by more
///   than a quarter of a millisecond (the position counted silence as the stream's frames, as
///   for a pass that found less than a period). That difference is the slip taken off later
///   positions. A suspect that shows neither is a near miss, and nothing is said.
/// - From a suspect until it is settled (100 ms for the pipeline to refill, then 16 samples) no
///   pair is published, and none afterwards for frames up to where the gap began.
///
/// Exclusive mode (a double buffer the hardware plays on over when it isn't refilled): how
/// far what was handed over before this fill is ahead of the DAC (the lead) is steady from
/// period to period, less only the wake's own delay; a wake a whole buffer later than the
/// usual (the median of the last 16 on-time leads) found the hardware already playing on over
/// the next buffer. That is a gap (as is a position past what was handed over); a wake within
/// an eighth of a buffer of it counts as a near miss. The buffers played again are counted as
/// a slip, in whole buffers, and taken off later positions, so a pair keeps meaning the device
/// frames `render` was given. The count can only come short (by a wake's usual delay), and a
/// short count shows as a lead a buffer short at the next period, which adds it.
///
/// Exclusive mode on a device that queues what it is given (three events in a row less than
/// half a buffer apart: the owner's Realtek laptop takes ten buffers at a start, then signals
/// unevenly, 0.1 to 75 ms apart, keeping about 100 ms queued): the lead says nothing of
/// lateness, only of the queue. The DAC runs dry when it reaches what was handed over (the
/// position at or past it is certain; within an eighth of a buffer, a suspect), and the
/// position runs on through the silence, which the settle (as shared mode's, on the queue's
/// depth by the device's count) measures as a slip.
///
/// Pairs: none while nothing is at the DAC yet (position 0), none going back, none at or past
/// what was handed over before this fill, and after a gap none for frames up to it (the DAC
/// may be in the gap, or in a stale buffer, then).
#[derive(Debug, Clone, Default)]
pub struct PositionMap {
    rate: u32,
    period: u32,
    exclusive: bool,
    /// Frames the position counted that were not the stream's (positive) or the other way.
    slip: i64,
    last: Option<PresentationPair>,
    dry: bool,
    /// After a gap: pairs only for frames above this.
    trust_above: Option<u64>,
    /// Shared: what the device held at this period's wake, of what had been handed over, when.
    pending: Option<(u32, u64, u64)>,
    /// Shared: frames the engine had read at this period's wake.
    read: Option<u64>,
    /// Shared: the last reading (raw position, time), and since when the DAC moves.
    prev: Option<(u64, u64)>,
    moving_since: Option<u64>,
    /// Shared: the usual pipeline; exclusive: the last on-time leads.
    usual: Ring,
    /// Shared: a suspect being settled.
    watch: Option<Watch>,
    /// Why the last reading decided as it did.
    why: Why,
    /// Exclusive mode: the device queues what it is given (see `note_wake`), and the wakes
    /// that tell.
    queued: bool,
    short_wakes: u32,
    last_wake_ns: Option<u64>,
}

/// Samples kept for a usual value.
const SAMPLES: usize = 16;

/// Why the last reading decided as it did (for traces): a value (exclusive: the lead; shared:
/// the pipeline, or after a settle the shift), the usual one (`i64::MIN` when not known yet), a
/// difference (exclusive: frames later than usual; shared: the position'"'"'s lag behind time),
/// buffers counted as played again, flags (`WHY_*`) and the slip after it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Why {
    pub value: i64,
    pub usual: i64,
    pub delta: i64,
    pub replayed: i64,
    pub flags: u32,
    pub slip: i64,
}

pub const WHY_NEAR: u32 = 1;
pub const WHY_PASSED: u32 = 2;
pub const WHY_SUSPECT: u32 = 4;
pub const WHY_CERTAIN: u32 = 8;
pub const WHY_SETTLED: u32 = 16;
pub const WHY_COUNTED: u32 = 32;
pub const WHY_DRY: u32 = 64;
pub const WHY_WATCHING: u32 = 128;
pub const WHY_QUEUED: u32 = 256;
/// Shared mode: the DAC moves this long before the pipeline counts as usual, and after a
/// suspect the pipeline has this long to refill before it is judged.
const SETTLE_NS: u64 = 100_000_000;

/// The last `SAMPLES` values, and their median.
#[derive(Debug, Clone, Copy, Default)]
struct Ring {
    v: [i64; SAMPLES],
    n: usize,
}

impl Ring {
    fn push(&mut self, x: i64) {
        self.v[self.n % SAMPLES] = x;
        self.n += 1;
    }

    fn full(&self) -> bool {
        self.n >= SAMPLES
    }

    fn median(&self) -> Option<i64> {
        let n = self.n.min(SAMPLES);
        if n == 0 {
            return None;
        }
        let mut v = self.v;
        let v = &mut v[..n];
        v.sort_unstable();
        Some(v[n / 2])
    }

    fn max(&self) -> Option<i64> {
        self.v[..self.n.min(SAMPLES)].iter().copied().max()
    }
}

/// Shared mode: a suspected gap, settled once the pipeline has refilled.
#[derive(Debug, Clone, Copy)]
struct Watch {
    /// Frames handed over when it was suspected (where a gap would have begun).
    at: u64,
    /// Already certain (the position fell behind).
    certain: bool,
    /// Pipeline samples from then on count.
    from_ns: u64,
    after: Ring,
    /// The slip when it began.
    slip_at: i64,
}

impl PositionMap {
    /// Shared mode at `rate`, with the engine's period.
    pub fn shared(rate: u32, period: u32) -> PositionMap {
        PositionMap {
            rate: rate.max(1),
            period: period.max(1),
            ..PositionMap::default()
        }
    }

    /// Exclusive mode at `rate`, with its buffer (one per event).
    pub fn exclusive(rate: u32, buffer: u32) -> PositionMap {
        PositionMap {
            rate: rate.max(1),
            period: buffer.max(1),
            exclusive: true,
            ..PositionMap::default()
        }
    }

    /// A start: device frame 0 again, nothing known.
    pub fn restart(&mut self) {
        // What the device is (a queue) is kept: it decides how the next start is made.
        *self = PositionMap {
            rate: self.rate,
            period: self.period,
            exclusive: self.exclusive,
            queued: self.queued,
            ..PositionMap::default()
        };
    }

    /// The device queues what it is given (exclusive mode; see `note_wake`).
    pub fn queued(&self) -> bool {
        self.queued
    }

    fn frames_of_ns(&self, ns: u64) -> f64 {
        ns as f64 * f64::from(self.rate) / 1e9
    }

    fn gap(&mut self, at: u64) -> Option<u64> {
        self.trust_above = Some(self.trust_above.map_or(at, |t| t.max(at)));
        if self.dry {
            return None;
        }
        self.dry = true;
        Some(at)
    }

    /// Shared mode: a gap may have begun at `at`.
    fn suspect(&mut self, at: u64, now_ns: u64) {
        if !self.usual.full() {
            // No usual pipeline yet (a start): only the position can tell.
            return;
        }
        if self.watch.is_none() {
            self.watch = Some(Watch {
                at,
                certain: false,
                from_ns: now_ns + SETTLE_NS,
                after: Ring::default(),
                slip_at: self.slip,
            });
        }
    }

    /// A wake, before the fill: the device still held `held` frames unread (shared mode; `None`
    /// in exclusive mode) of the `before` handed over, at mono time `now_ns`. Shared mode's
    /// gaps are said by the readings that settle what this suspects.
    pub fn space(&mut self, held: Option<u32>, before: u64, now_ns: u64) {
        self.pending = None;
        self.why.flags &= !WHY_SUSPECT;
        let Some(held) = held else {
            return;
        };
        self.read = Some(before.saturating_sub(u64::from(held)));
        if before == 0 {
            return;
        }
        if held < self.period {
            self.suspect(before, now_ns);
            self.why.flags |= WHY_SUSPECT;
        } else {
            self.pending = Some((held, before, now_ns));
        }
    }

    /// The fill handed over, at mono time `now_ns`: a fill that took nearly as long as what the
    /// device held could last is a suspect.
    pub fn filled(&mut self, now_ns: u64) {
        if let Some((held, before, since)) = self.pending.take() {
            let used = self.frames_of_ns(now_ns.saturating_sub(since));
            if used + f64::from(self.period) > f64::from(held) {
                self.suspect(before, since);
                self.why.flags |= WHY_SUSPECT;
            }
        }
    }

    /// Exclusive mode, a double buffer: the buffers the hardware played again since the last
    /// reading, and whether this wake was (nearly) too late.
    fn replays(&mut self, raw: u64, before: u64) -> (i64, bool) {
        let p = i64::from(self.period);
        let lead = before as i64 - (raw as i64 - self.slip);
        // Past what it held: certainly played on.
        let passed = if lead <= 0 { (-lead) / p + 1 } else { 0 };
        let late = self.usual.median().map_or(0, |u| (u - lead).max(0));
        let replayed = (late / p).max(passed);
        self.why = Why {
            value: lead,
            usual: self.usual.median().unwrap_or(i64::MIN),
            delta: late,
            replayed,
            flags: if passed > 0 { WHY_PASSED } else { 0 }
                | if late + p / 8 >= p { WHY_NEAR } else { 0 },
            slip: 0,
        };
        let near = late + p / 8 >= p;
        if replayed == 0 && !near {
            self.usual.push(lead);
        }
        (replayed, replayed > 0 || near)
    }

    /// Exclusive mode: a device whose events come three times in a row less than half a buffer
    /// apart queues what it is given (a double buffer can't: each of its events is a buffer
    /// played). From then on it is judged as a queue.
    fn note_wake(&mut self, time_ns: u64) {
        let half_ns = u64::from(self.period) * 500_000_000 / u64::from(self.rate);
        if let Some(lt) = self.last_wake_ns {
            if time_ns.saturating_sub(lt) < half_ns {
                self.short_wakes += 1;
            } else {
                self.short_wakes = 0;
            }
        }
        self.last_wake_ns = Some(time_ns);
        if !self.queued && self.short_wakes >= 3 {
            self.queued = true;
            // The double buffer's usual lead means nothing for a queue.
            self.usual = Ring::default();
        }
    }

    /// A gap that is certain, beginning at `at`: said once per dry stretch, and watched until
    /// the device's steady value (pipeline or queue) settles.
    fn certain(&mut self, at: u64, time_ns: u64) -> Option<u64> {
        self.why.flags |= WHY_CERTAIN;
        let at = self.watch.map_or(at, |w| w.at);
        let xrun = self.gap(at);
        match &mut self.watch {
            Some(w) => w.certain = true,
            None => {
                self.watch = Some(Watch {
                    at,
                    certain: true,
                    from_ns: time_ns + SETTLE_NS,
                    after: Ring::default(),
                    slip_at: self.slip,
                });
            }
        }
        xrun
    }

    /// A sample of the steady value (shared mode: the pipeline; a queue: frames handed over
    /// ahead of the DAC): kept as the usual, or, while a gap is watched, taken until it settles.
    /// Settled, the difference from the usual is silence the position counted: a slip.
    fn settle(&mut self, sample: i64, time_ns: u64) -> Option<u64> {
        self.why.value = sample;
        self.why.usual = self.usual.median().unwrap_or(i64::MIN);
        let Some(w) = &mut self.watch else {
            if self.moving_since.is_some_and(|m| time_ns >= m + SETTLE_NS) {
                self.usual.push(sample);
            }
            return None;
        };
        if time_ns >= w.from_ns {
            w.after.push(sample);
        }
        if !w.after.full() {
            return None;
        }
        let w = *w;
        self.watch = None;
        let shift = if self.queued {
            // A queue: its depth wobbles with the driver's events, and after a gap the events
            // can come more unevenly, so the usual depth moves without any more silence. What
            // it refills to does not: the most of its depth before the gap less the most after
            // is the silence counted meanwhile (never less than what the overrun proved).
            match (self.usual.max(), w.after.max()) {
                (Some(u), Some(a)) => (w.slip_at + u - a - self.slip).max(0),
                _ => 0,
            }
        } else {
            match (self.usual.median(), w.after.median()) {
                (Some(u), Some(a)) if self.usual.full() => u - a,
                _ => 0,
            }
        };
        let counted = (shift.unsigned_abs() as f64) * 4_000.0 > f64::from(self.rate);
        self.why.flags |= WHY_SETTLED;
        self.why.value = shift;
        if counted || (self.queued && shift > 0) {
            self.why.flags |= WHY_COUNTED;
            self.slip += shift;
        }
        let xrun = if counted || w.certain {
            self.gap(w.at)
        } else {
            None
        };
        // Settled: the value from now on is the usual one; a new gap is new.
        self.usual = w.after;
        self.dry = false;
        xrun
    }

    /// Exclusive mode, a queue: the DAC runs dry only when it reaches what was handed over
    /// (the position at or past it: certain; within an eighth of a buffer of it: a suspect).
    /// What it counts past that is silence: the settle measures it as a slip.
    fn queue_reading(&mut self, raw: u64, time_ns: u64, before: u64) -> Option<u64> {
        let mut xrun = None;
        if raw > 0 && self.moving_since.is_none() {
            self.moving_since = Some(time_ns);
        }
        if before == 0 {
            return None;
        }
        let lead = before as i64 - (raw as i64 - self.slip);
        self.why.delta = lead;
        if lead <= 0 {
            self.why.flags |= WHY_PASSED;
            xrun = self.certain(before, time_ns);
            // The position is past all that was handed over: at least that much of what it
            // counted was silence. Taken off at once, so no pair claims a frame not handed over.
            self.slip = raw as i64 - before as i64;
        }
        // The queue's depth by the device's own count (the slip apart): steady, less the
        // silence the position counts in a gap.
        let settled = self.settle(before as i64 - raw as i64, time_ns);
        xrun.or(settled)
    }

    /// Shared mode: what this reading says of gaps and the pipeline.
    fn shared_reading(&mut self, raw: u64, time_ns: u64) -> Option<u64> {
        let mut xrun = None;
        // The position against the time passed since the last reading.
        if let Some((pr, pt)) = self.prev
            && pr > 0
            && time_ns > pt
        {
            let lag = pr as f64 + self.frames_of_ns(time_ns - pt) - raw as f64;
            self.why.delta = lag.round() as i64;
            if lag > f64::from(self.rate) / 1_000.0 {
                xrun = self.certain(raw, time_ns);
            }
        }
        if raw > 0 && self.moving_since.is_none() {
            self.moving_since = Some(time_ns);
        }
        self.prev = Some((raw, time_ns));
        // The pipeline, sampled at this wake.
        let Some(read) = self.read.take() else {
            return xrun;
        };
        let settled = self.settle(read as i64 - raw as i64, time_ns);
        xrun.or(settled)
    }

    /// A reading after the fill: `raw` frames have reached the DAC (the device's count since the
    /// start) at mono time `time_ns`; `before` frames had been handed over before this period's
    /// fill, `after` after it.
    pub fn reading(&mut self, raw: u64, time_ns: u64, before: u64, after: u64) -> Reading {
        self.why = Why {
            flags: self.why.flags & WHY_SUSPECT,
            ..Why::default()
        };
        let mut xrun = None;
        if self.exclusive {
            self.note_wake(time_ns);
            if self.queued {
                self.why.flags |= WHY_QUEUED;
                xrun = self.queue_reading(raw, time_ns, before);
            } else if before > 0 {
                let (replayed, dry) = self.replays(raw, before);
                if dry {
                    xrun = self.gap(before);
                    self.slip += i64::from(self.period) * replayed;
                } else {
                    self.dry = false;
                }
            }
        } else {
            xrun = self.shared_reading(raw, time_ns);
        }
        let frame = raw as i64 - self.slip;
        let valid = raw > 0
            && frame >= 0
            && (frame as u64) < before.min(after)
            && self.watch.is_none()
            && self.trust_above.is_none_or(|t| frame as u64 > t)
            && self
                .last
                .is_none_or(|l| frame as u64 >= l.frame && time_ns >= l.time_ns);
        let pair = valid.then_some(PresentationPair {
            frame: frame as u64,
            time_ns,
        });
        if pair.is_some() {
            self.last = pair;
        }
        if self.dry {
            self.why.flags |= WHY_DRY;
        }
        if self.watch.is_some() {
            self.why.flags |= WHY_WATCHING;
        }
        self.why.slip = self.slip;
        Reading { pair, xrun }
    }

    /// Why the last reading decided as it did (for traces).
    pub fn why(&self) -> Why {
        self.why
    }

    /// Frames the position counted that were never the stream's (exclusive mode's replayed
    /// buffers; shared mode's silence counted as frames), taken off every position.
    pub fn slip(&self) -> i64 {
        self.slip
    }
}

// --- Output v2: events ----------------------------------------------------------------------

/// Slots in the real-time event queue (one always stays free).
pub const RT_EVENTS: usize = 64;

const EV_LOST: u64 = u64::MAX;
const EV_DEFAULT: u64 = u64::MAX - 1;

/// Events from a real-time thread to the thread that sends them on: a fixed ring of atomics,
/// one producer and one consumer; neither waits, nothing is allocated. A full ring drops the
/// event and counts it.
pub struct RtEvents {
    slots: [AtomicU64; RT_EVENTS],
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

impl RtEvents {
    pub fn new() -> RtEvents {
        RtEvents {
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// From the producer (the real-time thread).
    pub fn push(&self, e: &OutputEvent) -> bool {
        let code = match e {
            OutputEvent::Lost => EV_LOST,
            OutputEvent::DefaultChanged => EV_DEFAULT,
            OutputEvent::Xrun { at } => (*at).min(EV_DEFAULT - 1),
        };
        let t = self.tail.load(Ordering::Relaxed);
        let next = (t + 1) % RT_EVENTS;
        if next == self.head.load(Ordering::Acquire) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.slots[t].store(code, Ordering::Relaxed);
        self.tail.store(next, Ordering::Release);
        true
    }

    /// From the consumer.
    pub fn pop(&self) -> Option<OutputEvent> {
        let h = self.head.load(Ordering::Relaxed);
        if h == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let code = self.slots[h].load(Ordering::Relaxed);
        self.head.store((h + 1) % RT_EVENTS, Ordering::Release);
        Some(match code {
            EV_LOST => OutputEvent::Lost,
            EV_DEFAULT => OutputEvent::DefaultChanged,
            at => OutputEvent::Xrun { at },
        })
    }

    /// Events lost to a full ring.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Default for RtEvents {
    fn default() -> Self {
        RtEvents::new()
    }
}

/// Where an output's events go: the newest receiver (`subscribe`), with `Lost` said once per
/// open whoever sees it first (the real-time thread claims it with `claim_lost` and queues it;
/// a platform's own callbacks call `send`). Not for a real-time thread: it locks.
#[derive(Default)]
pub struct EventSink {
    tx: Mutex<Option<Sender<OutputEvent>>>,
    lost: AtomicBool,
}

impl EventSink {
    /// A new receiver; the older one hears no more.
    pub fn subscribe(&self) -> Receiver<OutputEvent> {
        let (tx, rx) = channel();
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        rx
    }

    /// Say `e` (a `Lost` already said or claimed is not said again).
    pub fn send(&self, e: OutputEvent) {
        if e == OutputEvent::Lost && !self.claim_lost() {
            return;
        }
        self.deliver(e);
    }

    /// Send as it is (a `Lost` claimed before).
    pub(crate) fn deliver(&self, e: OutputEvent) {
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(e);
        }
    }

    /// Claim the one `Lost` of this open: true the first time (the caller says it).
    pub fn claim_lost(&self) -> bool {
        !self.lost.swap(true, Ordering::SeqCst)
    }

    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    /// A new open: not lost.
    pub fn rearm(&self) {
        self.lost.store(false, Ordering::SeqCst);
    }
}

// --- Output v2: traces ----------------------------------------------------------------------

/// The columns of a trace row, in order (the CSV header).
pub const TRACE_COLUMNS: [&str; TRACE_FIELDS] = [
    "wake_ns",
    "filled_ns",
    "free",
    "held",
    "padding",
    "before",
    "after",
    "made",
    "pos_ok",
    "raw",
    "pos_units",
    "freq",
    "pos_ns",
    "xrun_at",
    "pair_frame",
    "value",
    "usual",
    "delta",
    "replayed",
    "flags",
    "slip",
    "state",
];
pub const TRACE_FIELDS: usize = 22;
const T_WAKE: usize = 0;
const T_FILLED: usize = 1;
const T_FREE: usize = 2;
const T_HELD: usize = 3;
const T_PADDING: usize = 4;
const T_BEFORE: usize = 5;
const T_AFTER: usize = 6;
const T_MADE: usize = 7;
const T_POS_OK: usize = 8;
const T_RAW: usize = 9;
const T_POS_UNITS: usize = 10;
const T_FREQ: usize = 11;
const T_POS_NS: usize = 12;
const T_XRUN: usize = 13;
const T_PAIR: usize = 14;
const T_VALUE: usize = 15;
const T_USUAL: usize = 16;
const T_DELTA: usize = 17;
const T_REPLAYED: usize = 18;
const T_FLAGS: usize = 19;
const T_SLIP: usize = 20;
const T_STATE: usize = 21;

/// A diagnostic trace of an output's render thread: one row per device event (what woke it,
/// what the device held, what was handed over, the position as the device gave it, and what
/// the position map decided and why, `Why` and its `WHY_*` flags). Every row is made before
/// the run, so the render thread only stores into atomics; rows past the last are dropped.
/// Times are mono ns; frame counts are device frames since the start; -1 is "none".
pub struct Trace {
    rows: Box<[[AtomicI64; TRACE_FIELDS]]>,
    next: AtomicUsize,
}

impl Trace {
    pub fn new(rows: usize) -> Trace {
        Trace {
            rows: (0..rows)
                .map(|_| std::array::from_fn(|_| AtomicI64::new(0)))
                .collect(),
            next: AtomicUsize::new(0),
        }
    }

    fn push(&self, row: &[i64; TRACE_FIELDS]) {
        let k = self.next.fetch_add(1, Ordering::Relaxed);
        if let Some(slot) = self.rows.get(k) {
            for (s, v) in slot.iter().zip(row) {
                s.store(*v, Ordering::Relaxed);
            }
        }
    }

    /// The rows so far (read once the output has stopped).
    pub fn rows(&self) -> Vec<[i64; TRACE_FIELDS]> {
        let n = self.next.load(Ordering::Acquire).min(self.rows.len());
        self.rows[..n]
            .iter()
            .map(|r| std::array::from_fn(|i| r[i].load(Ordering::Relaxed)))
            .collect()
    }

    /// Rows that didn't fit.
    pub fn dropped(&self) -> usize {
        self.next
            .load(Ordering::Acquire)
            .saturating_sub(self.rows.len())
    }

    /// The rows as CSV, with the header.
    pub fn csv(&self) -> String {
        let mut s = TRACE_COLUMNS.join(",");
        s.push('\n');
        for r in self.rows() {
            let line: Vec<String> = r.iter().map(i64::to_string).collect();
            s.push_str(&line.join(","));
            s.push('\n');
        }
        s
    }
}

// --- Output v2: the pull loop ---------------------------------------------------------------

/// Why a pull device's wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// The device wants frames (its event).
    Device,
    /// A command may have come (a kick, or the wait's own poll).
    Kick,
    Timeout,
}

/// What went wrong in a device call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The device is gone (invalidated, its service stopped).
    Lost,
    Other(String),
}

/// Wakes an output's render thread from another thread (a command came).
pub trait Kick: Send + Sync {
    fn kick(&self);
}

/// Room in the device's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    /// Frames it can take now.
    pub free: u32,
    /// Frames handed over that it hasn't read yet (shared mode's padding; `None` where the
    /// device doesn't say, as in exclusive event mode).
    pub held: Option<u32>,
}

/// A device that wakes its output's render thread when it wants frames (WASAPI's event-driven
/// client; a fake in tests). Made by a `PullOpener` on the render thread and used only there.
pub trait PullDevice {
    /// Wait for the device's event (`running`) or a kick, at most `timeout_ns`, on `sleeper`'s
    /// clock where the device has none of its own.
    fn wait(&mut self, sleeper: &Sleeper, running: bool, timeout_ns: u64) -> Result<Wake, Fault>;
    fn space(&mut self) -> Result<Space, Fault>;
    /// Hand over `frames` frames: the device's buffer for them, filled by `fill`.
    fn write(&mut self, frames: u32, fill: &mut dyn FnMut(&mut [u8])) -> Result<(), Fault>;
    /// (Frames that have reached the DAC since the start, the mono time of the reading), or
    /// `None` when the device can't say now.
    fn position(&mut self) -> Result<Option<(u64, u64)>, Fault>;
    fn start(&mut self) -> Result<(), Fault>;
    /// Stop, and let go of everything held (the position goes back to 0).
    fn halt(&mut self) -> Result<(), Fault>;
    /// Stop, and make the stream anew, as it was when the device was opened: for a device whose
    /// stop and reset do not bring back what a fresh stream does (the Realtek laptop's queue in
    /// exclusive mode). The default halts.
    fn renew(&mut self) -> Result<(), Fault> {
        self.halt()
    }
    /// For traces: the last position in the device'"'"'s own units, and its clock'"'"'s frequency.
    fn raw_clock(&self) -> (u64, u64) {
        (0, 0)
    }
    /// For traces: what the device holds unread, where it can say (asked only when tracing,
    /// so exclusive event mode, which doesn'"'"'t use it, can be asked too).
    fn probe_padding(&mut self) -> Option<u32> {
        None
    }
}

/// What a device says about itself, beyond `OutputInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceNotes {
    /// The output's endpoint id (the key of its remembered delay, section 4.14).
    pub endpoint_id: Option<String>,
    /// The device's own buffer, in frames.
    pub device_buffer_frames: u32,
    /// The stream latency the platform reports (beyond the buffer), in frames.
    pub stream_latency_frames: u32,
    /// The render thread is scheduled as pro audio (MMCSS on Windows).
    pub pro_audio: bool,
    /// The exclusive formats the device takes (probed once per device, `devices::ProbeCache`),
    /// when exclusive mode was asked.
    pub exclusive_formats: Option<Arc<crate::devices::FormatTable>>,
    /// Exclusive mode was asked and shared mode opened: why, in words.
    pub why_shared: Option<String>,
}

/// An opened pull device and what it runs at.
pub struct Opened<D> {
    pub device: D,
    pub info: OutputInfo,
    pub mode: Mode,
    pub name: String,
    pub kick: Arc<dyn Kick>,
    /// How its positions are read (`PositionMap::shared` or `::exclusive`).
    pub map: PositionMap,
    pub notes: DeviceNotes,
}

/// Opens a pull device. `open` runs on the output's render thread, so what it makes (COM
/// objects and the like) is made there, used there and dropped there.
pub trait PullOpener: Clone + Send + 'static {
    type Device: PullDevice;
    fn name(&self) -> String;
    /// The mode asked for (the device may answer with another: exclusive refused, shared).
    fn mode(&self) -> Mode;
    /// `sink` is for the device's own notifications (a lost device, a new default).
    fn open(
        &mut self,
        want: &OpenRequest,
        sink: &Arc<EventSink>,
    ) -> Result<Opened<Self::Device>, OutputError>;
}

/// A device that never answers its event for this long is lost (a sleep, a dead driver).
pub const DEVICE_SILENCE_NS: u64 = 2_000_000_000;
/// An idle render thread looks for commands at least this often (a kick wakes it at once).
const IDLE_WAIT_NS: u64 = 1_000_000_000;
/// The most an open or a start may take before the output says it did not answer.
const ANSWER_WAIT_NS: u64 = 10_000_000_000;
/// After a stop, the tail drains for at most the buffer's length and this.
const DRAIN_SLACK_NS: u64 = 200_000_000;
/// The control thread's poll while it waits for the render thread.
const POLL_NS: u64 = 1_000_000;

enum Cmd {
    Start(Sender<Result<(), OutputError>>),
    Close,
}

/// The real-time thread's way out: its event queue, and closing.
#[derive(Default)]
struct RtLink {
    events: RtEvents,
    closing: AtomicBool,
}

struct OpenAnswer {
    info: OutputInfo,
    mode: Mode,
    name: String,
    notes: DeviceNotes,
    kick: Arc<dyn Kick>,
}

struct OpenState {
    cmds: Sender<Cmd>,
    kick: Arc<dyn Kick>,
    render: JoinHandle<()>,
    forwarder: JoinHandle<()>,
    link: Arc<RtLink>,
}

/// An `Output` over a pull device (contract 5.2): `open` makes the render thread (which opens
/// the device on itself) and an event forwarder; `start` hands the renderer over and waits for
/// the device to start; `stop` takes the renderer back at once, and the render thread lets the
/// tail play out before it stops the device; `close` ends both threads.
pub struct PullOutput<O: PullOpener> {
    opener: O,
    tb: Arc<dyn Timebase>,
    presentation: Arc<Presentation>,
    latency: Arc<LatencyCell>,
    sink: Arc<EventSink>,
    cell: Arc<RenderCell>,
    open: Option<OpenState>,
    started: bool,
    /// The last open's mode, name and notes.
    last: Option<(Mode, String, DeviceNotes)>,
    trace: Option<Arc<Trace>>,
}

impl<O: PullOpener> PullOutput<O> {
    /// An output over `opener`'s devices, waiting for its render thread on `tb`.
    pub fn new(opener: O, tb: Arc<dyn Timebase>) -> PullOutput<O> {
        PullOutput {
            opener,
            tb,
            presentation: Arc::new(Presentation::default()),
            latency: Arc::new(LatencyCell::default()),
            sink: Arc::new(EventSink::default()),
            cell: Arc::new(RenderCell::new()),
            open: None,
            started: false,
            last: None,
            trace: None,
        }
    }

    /// What the device said about itself at the last open (kept after a close).
    /// Trace every period of the opens from now on into `rows` rows made now (a diagnostic:
    /// see `Trace`).
    pub fn trace(&mut self, rows: usize) -> Arc<Trace> {
        let t = Arc::new(Trace::new(rows));
        self.trace = Some(t.clone());
        t
    }

    pub fn notes(&self) -> Option<DeviceNotes> {
        self.last.as_ref().map(|l| l.2.clone())
    }

    /// Wait for the render thread's answer, letting time pass on the timebase (so a simulated
    /// render thread runs meanwhile).
    fn answer<T>(&self, rx: &Receiver<T>) -> Option<T> {
        let until = self.tb.now_ns() + ANSWER_WAIT_NS;
        loop {
            match rx.try_recv() {
                Ok(v) => return Some(v),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) if self.tb.now_ns() >= until => return None,
                Err(TryRecvError::Empty) => self.tb.pass(POLL_NS),
            }
        }
    }

    /// Wait for a thread to end, as `answer` waits; false if it didn't in time.
    fn ended(&self, h: &JoinHandle<()>) -> bool {
        let until = self.tb.now_ns() + ANSWER_WAIT_NS;
        while !h.is_finished() {
            if self.tb.now_ns() >= until {
                return false;
            }
            self.tb.pass(POLL_NS);
        }
        true
    }

    fn stop_forwarder(link: &RtLink, forwarder: JoinHandle<()>) {
        link.closing.store(true, Ordering::Release);
        forwarder.thread().unpark();
        let _ = forwarder.join();
    }
}

impl<O: PullOpener> Output for PullOutput<O> {
    /// The device's own name once opened (kept after a close), the one asked for before.
    fn name(&self) -> String {
        self.last
            .as_ref()
            .map_or_else(|| self.opener.name(), |l| l.1.clone())
    }

    /// The mode the device opened in (kept after a close), the one asked for before.
    fn mode(&self) -> Mode {
        self.last.as_ref().map_or(self.opener.mode(), |l| l.0)
    }

    fn open(&mut self, want: &OpenRequest) -> Result<OutputInfo, OutputError> {
        if self.open.is_some() {
            return Err(OutputError::State("The output is already open.".into()));
        }
        self.sink.rearm();
        self.presentation.clear();
        let link = Arc::new(RtLink::default());
        let forwarder = {
            let (link, sink) = (link.clone(), self.sink.clone());
            std::thread::Builder::new()
                .name("output-events".into())
                .spawn(move || forward(&link, &sink))
                .map_err(|e| OutputError::Other(e.to_string()))?
        };
        let (cmd_tx, cmd_rx) = channel();
        let (ans_tx, ans_rx) = channel();
        let ctx = Ctx {
            cell: self.cell.clone(),
            presentation: self.presentation.clone(),
            sink: self.sink.clone(),
            link: link.clone(),
            forwarder: forwarder.thread().clone(),
            trace: self.trace.clone(),
            latency: self.latency.clone(),
        };
        let (opener, want2) = (self.opener.clone(), want.clone());
        // Made here, before the thread starts, so a simulated clock counts it from its start.
        let sleeper = self.tb.sleeper();
        let render = match std::thread::Builder::new()
            .name("output-render".into())
            .spawn(move || render_thread(opener, &want2, ctx, &cmd_rx, &ans_tx, &sleeper))
        {
            Ok(h) => h,
            Err(e) => {
                Self::stop_forwarder(&link, forwarder);
                return Err(OutputError::Other(e.to_string()));
            }
        };
        match self.answer(&ans_rx) {
            Some(Ok(a)) => {
                let info = a.info.clone();
                self.last = Some((a.mode, a.name, a.notes));
                self.open = Some(OpenState {
                    cmds: cmd_tx,
                    kick: a.kick,
                    render,
                    forwarder,
                    link,
                });
                Ok(info)
            }
            Some(Err(e)) => {
                if self.ended(&render) {
                    let _ = render.join();
                }
                Self::stop_forwarder(&link, forwarder);
                Err(e)
            }
            None => {
                // A render thread that never answered: told to close, and left.
                let _ = cmd_tx.send(Cmd::Close);
                Self::stop_forwarder(&link, forwarder);
                Err(OutputError::Other(
                    "The sound output did not answer.".into(),
                ))
            }
        }
    }

    fn start(&mut self, render: Box<dyn Render>) -> Result<(), OutputError> {
        let Some(st) = self.open.as_ref() else {
            return Err(OutputError::State("The output isn't open.".into()));
        };
        if self.started {
            return Err(OutputError::State("The output is already started.".into()));
        }
        if self.sink.is_lost() {
            return Err(OutputError::Lost);
        }
        if self.cell.install(render).is_err() {
            return Err(OutputError::State("A renderer is still in.".into()));
        }
        let (tx, rx) = channel();
        if st.cmds.send(Cmd::Start(tx)).is_err() {
            self.cell.take();
            return Err(OutputError::Lost);
        }
        st.kick.kick();
        match self.answer(&rx) {
            Some(Ok(())) => {
                self.started = true;
                Ok(())
            }
            Some(Err(e)) => {
                self.cell.take();
                Err(e)
            }
            None => {
                self.cell.take();
                Err(OutputError::Other(
                    "The sound output did not answer.".into(),
                ))
            }
        }
    }

    fn stop(&mut self) -> Option<Box<dyn Render>> {
        if !self.started {
            return None;
        }
        self.started = false;
        let r = self.cell.take();
        // The render thread sees the empty cell at its next device event and drains the tail
        // (no kick: only a device event leads to a buffer call).
        r
    }

    fn close(&mut self) {
        let Some(st) = self.open.take() else {
            return;
        };
        self.started = false;
        let _ = st.cmds.send(Cmd::Close);
        st.kick.kick();
        if self.ended(&st.render) {
            let _ = st.render.join();
        }
        Self::stop_forwarder(&st.link, st.forwarder);
        drop(self.cell.take());
        self.presentation.clear();
    }

    fn latency(&self) -> Option<Latency> {
        self.latency.read()
    }

    fn notes(&self) -> Option<DeviceNotes> {
        PullOutput::notes(self)
    }

    fn presentation(&self) -> Arc<Presentation> {
        self.presentation.clone()
    }

    fn events(&self) -> Receiver<OutputEvent> {
        self.sink.subscribe()
    }
}

impl<O: PullOpener> Drop for PullOutput<O> {
    fn drop(&mut self) {
        self.close();
    }
}

/// The event forwarder: sends on what the render thread queued, woken by `unpark`.
fn forward(link: &RtLink, sink: &EventSink) {
    loop {
        while let Some(e) = link.events.pop() {
            sink.deliver(e);
        }
        if link.closing.load(Ordering::Acquire) {
            while let Some(e) = link.events.pop() {
                sink.deliver(e);
            }
            return;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

/// What the render thread shares with the rest of the output.
struct Ctx {
    cell: Arc<RenderCell>,
    presentation: Arc<Presentation>,
    sink: Arc<EventSink>,
    link: Arc<RtLink>,
    forwarder: std::thread::Thread,
    /// Every period's row, when tracing.
    trace: Option<Arc<Trace>>,
    latency: Arc<LatencyCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RtState {
    /// Open; not started, or stopped and drained.
    Idle,
    Running,
    /// Stopped: the tail plays out, then the device stops.
    Draining,
    /// The device was lost.
    Dead,
}

struct Loop<D: PullDevice> {
    dev: D,
    pump: Pump,
    map: PositionMap,
    state: RtState,
    ctx: Ctx,
    /// Draining: until the DAC passes this frame, or this time.
    drain_until: u64,
    drain_deadline: u64,
    buffer_ns: u64,
    /// The latency as this start learns it.
    learned: LatencyTracker,
    /// The generation of the renderer this start plays (`RenderCell`).
    generation: u64,
}

fn render_thread<O: PullOpener>(
    mut opener: O,
    want: &OpenRequest,
    ctx: Ctx,
    cmds: &Receiver<Cmd>,
    answer: &Sender<Result<OpenAnswer, OutputError>>,
    sleeper: &Sleeper,
) {
    let opened = match opener.open(want, &ctx.sink) {
        Ok(o) => o,
        Err(e) => {
            let _ = answer.send(Err(e));
            return;
        }
    };
    let Opened {
        device,
        info,
        mode,
        name,
        kick,
        map,
        notes,
    } = opened;
    let buffer_ns =
        u64::from(info.buffer_frames) * 1_000_000_000 / u64::from(info.format.rate.max(1));
    let pump = Pump::new(&info.format, info.period_frames);
    let learned = LatencyTracker::new(info.format.rate);
    if answer
        .send(Ok(OpenAnswer {
            info,
            mode,
            name,
            notes,
            kick,
        }))
        .is_err()
    {
        return;
    }
    let mut lp = Loop {
        dev: device,
        pump,
        map,
        state: RtState::Idle,
        ctx,
        drain_until: 0,
        drain_deadline: 0,
        buffer_ns,
        learned,
        generation: 0,
    };
    lp.run(cmds, sleeper);
    // The device (its COM objects, on Windows) is dropped here, on this thread.
}

impl<D: PullDevice> Loop<D> {
    fn run(&mut self, cmds: &Receiver<Cmd>, sleeper: &Sleeper) {
        loop {
            loop {
                match cmds.try_recv() {
                    Ok(Cmd::Start(reply)) => {
                        let r = self.start();
                        let _ = reply.send(r);
                    }
                    Ok(Cmd::Close) | Err(TryRecvError::Disconnected) => {
                        if matches!(self.state, RtState::Running | RtState::Draining) {
                            let _ = self.dev.halt();
                        }
                        return;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            let running = matches!(self.state, RtState::Running | RtState::Draining);
            let timeout = if running {
                DEVICE_SILENCE_NS
            } else {
                IDLE_WAIT_NS
            };
            match self.dev.wait(sleeper, running, timeout) {
                // Only the device's own event leads to a buffer call (in exclusive event mode a
                // buffer may be written once per event, never on a kick).
                Ok(Wake::Device) if running => self.period(sleeper),
                Ok(Wake::Timeout) if running => self.lost(),
                Ok(_) => {}
                Err(_) if running => self.lost(),
                Err(_) => {}
            }
        }
    }

    fn start(&mut self) -> Result<(), OutputError> {
        if self.state == RtState::Dead || self.ctx.sink.is_lost() {
            return Err(OutputError::Lost);
        }
        // Whatever the device still holds (a tail still draining) is from before: let it go,
        // so nothing held from before plays and the position starts again from 0. A device that
        // queues gets a new stream: after a stop and reset the Realtek laptop's queue took one
        // buffer per event, never filled, and its clock ran past everything handed over, where
        // a fresh stream takes ten buffers at once.
        self.state = RtState::Idle;
        let renewed = if self.map.queued() {
            self.dev.renew()
        } else {
            self.dev.halt()
        };
        renewed.map_err(|f| self.fault(f))?;
        self.generation = self.ctx.cell.generation();
        self.pump.restart();
        self.map.restart();
        self.ctx.presentation.clear();
        self.ctx.latency.clear();
        self.learned.restart();
        let space = self.dev.space().map_err(|f| self.fault(f))?;
        self.fill(space.free, true).map_err(|f| self.fault(f))?;
        self.dev.start().map_err(|f| self.fault(f))?;
        self.state = RtState::Running;
        Ok(())
    }

    /// Hand over `frames` frames: the renderer's (`live`), or silence.
    fn fill(&mut self, frames: u32, live: bool) -> Result<(), Fault> {
        if frames == 0 {
            return Ok(());
        }
        let Loop {
            dev,
            pump,
            ctx,
            generation,
            ..
        } = self;
        let cell = live.then_some((&*ctx.cell, *generation));
        dev.write(frames, &mut |buf| {
            pump.fill(cell, buf, frames);
        })
    }

    /// One device event.
    fn period(&mut self, sleeper: &Sleeper) {
        let mut row = self.ctx.trace.as_ref().map(|_| [-1i64; TRACE_FIELDS]);
        let wake_ns = sleeper.now_ns();
        let cell = &self.ctx.cell;
        if self.state == RtState::Running
            && (cell.is_empty() || cell.generation() != self.generation)
        {
            self.state = RtState::Draining;
            self.drain_until = self.pump.made();
            self.drain_deadline = wake_ns + self.buffer_ns + DRAIN_SLACK_NS;
        }
        let draining = self.state == RtState::Draining;
        let space = match self.dev.space() {
            Ok(s) => s,
            Err(f) => {
                self.fault(f);
                return;
            }
        };
        let probe = if row.is_some() {
            self.dev.probe_padding()
        } else {
            None
        };
        let before = self.pump.written();
        if !draining {
            self.map.space(space.held, before, wake_ns);
        }
        if let Err(f) = self.fill(space.free, !draining) {
            self.fault(f);
            return;
        }
        let filled_ns = sleeper.now_ns();
        if !draining {
            self.map.filled(filled_ns);
        }
        let after = self.pump.written();
        let mut done = false;
        let position = self.dev.position();
        if let Some(r) = row.as_mut() {
            let (units, freq) = self.dev.raw_clock();
            r[T_WAKE] = wake_ns as i64;
            r[T_FILLED] = filled_ns as i64;
            r[T_FREE] = i64::from(space.free);
            r[T_HELD] = space.held.map_or(-1, i64::from);
            r[T_PADDING] = probe.map_or(-1, i64::from);
            r[T_BEFORE] = before as i64;
            r[T_AFTER] = after as i64;
            r[T_MADE] = self.pump.made() as i64;
            r[T_POS_UNITS] = units as i64;
            r[T_FREQ] = freq as i64;
            r[T_STATE] = i64::from(draining) + 1;
            r[T_POS_OK] = match &position {
                Ok(Some(_)) => 1,
                Ok(None) => 0,
                Err(_) => -1,
            };
        }
        match position {
            Ok(Some((raw, time_ns))) => {
                let r = self.map.reading(raw, time_ns, before, after);
                if let Some(t) = row.as_mut() {
                    let why = self.map.why();
                    t[T_RAW] = raw as i64;
                    t[T_POS_NS] = time_ns as i64;
                    t[T_XRUN] = r.xrun.map_or(-1, |a| a as i64);
                    t[T_PAIR] = r.pair.map_or(-1, |p| p.frame as i64);
                    t[T_VALUE] = why.value;
                    t[T_USUAL] = why.usual;
                    t[T_DELTA] = why.delta;
                    t[T_REPLAYED] = why.replayed;
                    t[T_FLAGS] = i64::from(why.flags);
                    t[T_SLIP] = why.slip;
                }
                if let Some(p) = r.pair {
                    self.ctx.presentation.publish(p);
                    // What was handed over and is not yet at the DAC, right after this fill.
                    let l = self.learned.push(after.saturating_sub(p.frame));
                    self.ctx.latency.publish(l);
                }
                if !draining && let Some(at) = r.xrun {
                    self.xrun(at);
                }
                done = draining && raw as i64 - self.map.slip() >= self.drain_until as i64;
            }
            Ok(None) => {}
            Err(f) => {
                self.fault(f);
                return;
            }
        }
        if let (Some(t), Some(r)) = (self.ctx.trace.as_ref(), row.as_ref()) {
            t.push(r);
        }
        if done || (draining && sleeper.now_ns() >= self.drain_deadline) {
            self.drained();
        }
    }

    /// The tail has played: stop the device (and let go of the silence after it).
    fn drained(&mut self) {
        match self.dev.halt() {
            Ok(()) => self.state = RtState::Idle,
            Err(f) => {
                self.fault(f);
            }
        }
    }

    fn xrun(&mut self, at: u64) {
        if self.ctx.link.events.push(&OutputEvent::Xrun { at }) {
            self.ctx.forwarder.unpark();
        }
    }

    fn lost(&mut self) {
        self.state = RtState::Dead;
        if self.ctx.sink.claim_lost() && self.ctx.link.events.push(&OutputEvent::Lost) {
            self.ctx.forwarder.unpark();
        }
    }

    /// Any fault in a device call: the device can't be trusted to play on, so it is lost (the
    /// control thread reopens).
    fn fault(&mut self, _f: Fault) -> OutputError {
        self.lost();
        OutputError::Lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(container: u16, valid: u16, kind: SampleKind) -> DeviceFormat {
        DeviceFormat {
            rate: 96_000,
            channels: 2,
            container_bits: container,
            valid_bits: valid,
            kind,
        }
    }

    #[test]
    fn integer_formats_carry_the_exact_values() {
        // 24-bit samples, left-justified.
        let src: Vec<i32> = [0x7F_FFFF, -0x80_0000, 0x12_3456, -1, 0]
            .iter()
            .map(|v| v << 8)
            .collect();
        let mut out = Vec::new();
        encode(&src, &fmt(24, 24, SampleKind::Int), 1.0, &mut out);
        let back: Vec<i32> = out
            .as_chunks::<3>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes([0, b[0], b[1], b[2]]))
            .collect();
        assert_eq!(back, src, "packed 24-bit");
        encode(&src, &fmt(32, 24, SampleKind::Int), 1.0, &mut out);
        let back: Vec<i32> = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(back, src, "24 in a 32-bit container");
        let s16: Vec<i32> = [32767, -32768, 1234].iter().map(|v| v << 16).collect();
        encode(&s16, &fmt(16, 16, SampleKind::Int), 1.0, &mut out);
        assert_eq!(out, [0xFF, 0x7F, 0x00, 0x80, 0xD2, 0x04]);
    }

    #[test]
    fn mono_becomes_stereo() {
        let mut out = Vec::new();
        remap(&[1, 2, 3], 1, 2, &mut out);
        assert_eq!(out, [1, 1, 2, 2, 3, 3]);
    }

    // --- Output v2 ------------------------------------------------------------------------

    /// Writes 1 into each byte and counts calls; can be told to take its time.
    struct Ones {
        calls: Arc<AtomicU64>,
        frames: Arc<Mutex<Vec<(u32, u64)>>>,
        busy: std::time::Duration,
    }

    impl Render for Ones {
        fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
            out.fill(1);
            if let Ok(mut f) = self.frames.lock() {
                f.push((frames, at));
            }
            if !self.busy.is_zero() {
                std::thread::sleep(self.busy);
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    type Seen = Arc<Mutex<Vec<(u32, u64)>>>;

    fn ones(busy_ms: u64) -> (Box<Ones>, Arc<AtomicU64>, Seen) {
        let calls = Arc::new(AtomicU64::new(0));
        let frames = Arc::new(Mutex::new(Vec::new()));
        (
            Box::new(Ones {
                calls: calls.clone(),
                frames: frames.clone(),
                busy: std::time::Duration::from_millis(busy_ms),
            }),
            calls,
            frames,
        )
    }

    fn stereo32() -> DeviceFormat {
        fmt(32, 24, SampleKind::Int)
    }

    #[test]
    fn a_cell_holds_one_renderer_and_gives_it_back() {
        let cell = RenderCell::new();
        assert!(cell.is_empty());
        let mut buf = [0u8; 16];
        assert!(!cell.render(1, &mut buf, 2, 0), "nothing to call");
        let (r, calls, _) = ones(0);
        assert_eq!(
            cell.install(r).ok(),
            Some(1),
            "the first start's generation"
        );
        assert!(cell.install(ones(0).0).is_err(), "one at a time");
        assert!(!cell.render(0, &mut buf, 2, 0), "another start's renderer");
        assert!(cell.render(1, &mut buf, 2, 0));
        assert_eq!(buf, [1; 16]);
        assert!(cell.take().is_some());
        assert!(cell.take().is_none());
        assert!(!cell.render(1, &mut buf, 2, 2));
        assert_eq!(cell.install(ones(0).0).ok(), Some(2), "the next start's");
        assert_eq!(cell.generation(), 2);
        assert!(
            !cell.render(1, &mut buf, 2, 2),
            "never the last start's frames"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// A renderer whose every call says it has begun, waits to be let go, and counts itself
    /// ended before it returns.
    struct Held {
        begun: std::sync::mpsc::Sender<()>,
        go: std::sync::mpsc::Receiver<()>,
        ended: Arc<AtomicU64>,
    }

    impl Render for Held {
        fn render(&mut self, _out: &mut [u8], _frames: u32, _at: u64) {
            let _ = self.begun.send(());
            let _ = self.go.recv();
            self.ended.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Only against a deadlock: nothing here is timed.
    const STUCK: std::time::Duration = std::time::Duration::from_secs(60);

    #[test]
    fn take_waits_only_for_the_call_running_and_render_never_follows_it() {
        use std::sync::mpsc::{TryRecvError, channel};
        // A call held inside the renderer: a take made meanwhile comes back only once that
        // call has ended (never with the renderer in use), and then without the render thread
        // doing anything more (it waits to be told to try again).
        let cell = Arc::new(RenderCell::new());
        let (begun_tx, begun) = channel();
        let (go, go_rx) = channel();
        let ended = Arc::new(AtomicU64::new(0));
        let generation = cell
            .install(Box::new(Held {
                begun: begun_tx,
                go: go_rx,
                ended: ended.clone(),
            }))
            .ok()
            .unwrap();
        let (again, again_rx) = channel::<()>();
        let (tried_tx, tried) = channel::<bool>();
        let c = cell.clone();
        let rt = std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            assert!(c.render(generation, &mut buf, 8, 0), "the first call");
            while again_rx.recv().is_ok() {
                let _ = tried_tx.send(c.render(generation, &mut buf, 8, 8));
            }
        });
        begun.recv_timeout(STUCK).expect("the call began");
        let (about_tx, about) = channel();
        let (taken_tx, taken) = channel();
        let (c, e) = (cell.clone(), ended.clone());
        let taker = std::thread::spawn(move || {
            let _ = about_tx.send(());
            let r = c.take();
            let _ = taken_tx.send((r.is_some(), e.load(Ordering::SeqCst)));
        });
        about.recv_timeout(STUCK).expect("the taker runs");
        // Give the taker every chance to come back early (it must not, whatever the load).
        for _ in 0..10_000 {
            std::thread::yield_now();
        }
        assert_eq!(
            taken.try_recv(),
            Err(TryRecvError::Empty),
            "take came back while the call ran"
        );
        go.send(()).unwrap();
        let (got, ended_then) = taken.recv_timeout(STUCK).expect("take came back");
        assert!(got, "with the renderer");
        assert_eq!(ended_then, 1, "only once the call running had ended");
        taker.join().unwrap();
        // Once taken, the render thread's tries call nothing.
        for _ in 0..100 {
            again.send(()).unwrap();
            assert!(!tried.recv_timeout(STUCK).unwrap(), "called after take");
        }
        assert_eq!(ended.load(Ordering::SeqCst), 1);
        // And with no call running, a take needs nothing of the render thread (it is waiting).
        assert!(cell.install(ones(0).0).is_ok());
        assert!(cell.take().is_some());
        drop(again);
        rt.join().unwrap();

        // A render thread calling over and over: a take still gets in between two calls, and
        // after it nothing is called, over as many tries as the thread makes.
        for _ in 0..20 {
            let cell = Arc::new(RenderCell::new());
            let (r, calls, _) = ones(2);
            let generation = cell.install(r).ok().unwrap();
            let c = cell.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let tries = Arc::new(AtomicU64::new(0));
            let (s, t) = (stop.clone(), tries.clone());
            let rt = std::thread::spawn(move || {
                let mut buf = [0u8; 64];
                let mut at = 0;
                while !s.load(Ordering::SeqCst) {
                    if c.render(generation, &mut buf, 8, at) {
                        at += 8;
                    }
                    t.fetch_add(1, Ordering::SeqCst);
                }
            });
            while calls.load(Ordering::SeqCst) == 0 {
                std::thread::yield_now();
            }
            let (taken_tx, taken) = channel();
            let c = cell.clone();
            std::thread::spawn(move || {
                let _ = taken_tx.send(c.take().is_some());
            });
            assert!(taken.recv_timeout(STUCK).expect("take came back"));
            let after = calls.load(Ordering::SeqCst);
            let from = tries.load(Ordering::SeqCst);
            while tries.load(Ordering::SeqCst) < from + 1_000 {
                std::thread::yield_now();
            }
            assert_eq!(calls.load(Ordering::SeqCst), after, "called after take");
            stop.store(true, Ordering::SeqCst);
            rt.join().unwrap();
        }
    }

    #[test]
    fn a_pump_asks_for_at_most_a_period_contiguous_and_fills_silence_without_a_renderer() {
        let f = stereo32();
        let mut pump = Pump::new(&f, 480);
        let cell = RenderCell::new();
        let (r, _, seen) = ones(0);
        let generation = cell.install(r).ok().unwrap();
        let mut buf = vec![9u8; 1_300 * 8];
        assert_eq!(pump.fill(Some((&cell, generation)), &mut buf, 1_300), 1_300);
        assert_eq!(*seen.lock().unwrap(), [(480, 0), (480, 480), (340, 960)]);
        assert!(buf.iter().all(|&b| b == 1));
        assert_eq!((pump.written(), pump.made()), (1_300, 1_300));
        cell.take();
        let mut buf = vec![9u8; 100 * 8];
        assert_eq!(pump.fill(Some((&cell, generation)), &mut buf, 100), 0);
        assert!(buf.iter().all(|&b| b == 0), "silence");
        assert_eq!((pump.written(), pump.made()), (1_400, 1_300));
        let mut buf = vec![9u8; 10 * 8];
        assert_eq!(pump.fill(None, &mut buf, 10), 0);
        assert!(buf.iter().all(|&b| b == 0));
        pump.restart();
        assert_eq!((pump.written(), pump.made()), (0, 0));
    }

    fn pair(frame: u64, time_ns: u64) -> Option<PresentationPair> {
        Some(PresentationPair { frame, time_ns })
    }

    #[test]
    fn positions_become_pairs_only_when_they_mean_a_frame_at_the_dac() {
        let mut m = PositionMap::shared(48_000, 480);
        m.space(Some(1_920), 2_400, 0);
        // Nothing at the DAC yet.
        assert_eq!(m.reading(0, 1_000, 2_400, 2_880).pair, None);
        assert_eq!(m.reading(100, 2_000, 2_400, 2_880).pair, pair(100, 2_000));
        // Going back (a driver's glitch) is skipped, in frame or in time.
        assert_eq!(m.reading(90, 3_000, 2_880, 3_360).pair, None);
        assert_eq!(m.reading(200, 1_500, 2_880, 3_360).pair, None);
        assert_eq!(m.reading(300, 4_000, 2_880, 3_360).pair, pair(300, 4_000));
        // At or past what was handed over: no valid pair.
        assert_eq!(m.reading(3_360, 5_000, 2_880, 3_360).pair, None);
        m.restart();
        assert_eq!(
            m.reading(10, 100, 480, 960).pair,
            pair(10, 100),
            "a new start"
        );
    }

    /// A shared engine as the position map sees it, one wake per 10 ms pass of 480 frames:
    /// the engine has read `480 k - lost`, the device holds `held` more, and the DAC is 1440
    /// frames behind what the engine read, plus `extra` the position counted that was silence.
    struct Engine {
        m: PositionMap,
        k: u64,
        lost: u64,
        extra: u64,
    }

    impl Engine {
        fn new() -> Engine {
            Engine {
                m: PositionMap::shared(48_000, 480),
                k: 0,
                lost: 0,
                extra: 0,
            }
        }

        /// The next wake; `stall`: a pass found nothing (the engine read nothing, the DAC
        /// stopped).
        fn wake(&mut self, held: u32, stall: bool) -> (Reading, u64) {
            self.k += 1;
            if stall {
                self.lost += 480;
            }
            let t = self.k * 10_000_000;
            let read = (480 * self.k).saturating_sub(self.lost);
            let before = read + u64::from(held);
            self.m.space(Some(held), before, t);
            self.m.filled(t);
            let raw = (read + self.extra).saturating_sub(1_440);
            (self.m.reading(raw, t, before, before + 480), before)
        }
    }

    #[test]
    fn shared_mode_takes_no_suspect_before_it_knows_the_usual_pipeline() {
        // The engine reads a burst at a start (here all but 300 frames by the first wake):
        // little held, but no gap, and nothing to compare with yet.
        let mut e = Engine::new();
        for k in 1..=60 {
            let (r, _) = e.wake(if k == 1 { 300 } else { 1_920 }, false);
            assert_eq!(r.xrun, None, "wake {k}");
        }
        assert_eq!(e.m.slip(), 0);
    }

    #[test]
    fn shared_mode_settles_a_near_miss_without_an_xrun() {
        let mut e = Engine::new();
        for _ in 1..=40 {
            e.wake(1_920, false);
        }
        // Nothing held at a wake, but the engine found what it needed: the position kept
        // time and the pipeline is as usual.
        let (r, _) = e.wake(0, false);
        assert_eq!((r.xrun, r.pair), (None, None), "withheld while it settles");
        let mut first = None;
        for k in 42..=80 {
            let (r, _) = e.wake(1_920, false);
            assert_eq!(r.xrun, None, "wake {k}");
            if r.pair.is_some() && first.is_none() {
                first = Some(k);
            }
        }
        // 100 ms to refill, then 16 samples: pairs again from wake 66.
        assert_eq!(first, Some(66));
        assert_eq!(e.m.slip(), 0);
    }

    #[test]
    fn shared_mode_takes_off_silence_the_position_counted() {
        let mut e = Engine::new();
        for _ in 1..=40 {
            e.wake(1_920, false);
        }
        // A pass found 280 frames of a period: the engine played 200 frames of silence, and
        // the position counted them as the stream's.
        e.extra = 200;
        let (r, at) = e.wake(0, false);
        assert_eq!(r.xrun, None, "only a suspect so far");
        let mut said = None;
        for k in 42..=70 {
            let (r, _) = e.wake(1_920, false);
            if let Some(x) = r.xrun {
                said = Some((k, x));
            }
            if k >= 66 {
                let p = r.pair.expect("pairs again once settled");
                assert_eq!(p.frame, 480 * k - 1_440, "the true frame at the DAC");
            }
        }
        assert_eq!(said, Some((66, at)), "the gap, where it began");
        assert_eq!(e.m.slip(), 200);
    }

    #[test]
    fn shared_mode_says_a_gap_at_once_when_the_position_falls_behind_time() {
        let mut e = Engine::new();
        for _ in 1..=40 {
            e.wake(1_920, false);
        }
        // Nothing held, and a pass found nothing: the DAC sat 10 ms in silence.
        let (r, at) = e.wake(0, true);
        assert_eq!(r.xrun, Some(at), "certain at once");
        let mut more = 0;
        for k in 42..=80 {
            let (r, _) = e.wake(1_920, false);
            more += usize::from(r.xrun.is_some());
            if k >= 66 {
                assert!(r.pair.is_some(), "wake {k}");
            }
        }
        assert_eq!(more, 0, "one xrun for one gap");
        assert_eq!(e.m.slip(), 0, "a position that stalls needs no slip");
        // Exclusive mode judges by its positions alone.
        let mut x = PositionMap::exclusive(48_000, 480);
        x.space(None, 960, 0);
        x.filled(500_000_000);
        assert_eq!(x.slip(), 0);
    }

    #[test]
    fn exclusive_mode_runs_dry_a_whole_buffer_late_and_slips_whole_buffers() {
        // 480-frame buffers; the DAC 48 frames behind the hardware's read. At an on-time wake
        // for pass j, j + 1 buffers were handed over before its fill and the DAC is at
        // j x 480 - 48: a lead of 528.
        const P: u64 = 480;
        let mut m = PositionMap::exclusive(48_000, 480);
        m.space(None, P, 0);
        assert_eq!(
            m.reading(0, 0, P, 2 * P).pair,
            None,
            "nothing at the DAC yet"
        );
        for j in 1..=10 {
            let r = m.reading(j * P - 48, j * 10_000_000, (j + 1) * P, (j + 2) * P);
            assert_eq!(
                (r.pair, r.xrun),
                (pair(j * P - 48, j * 10_000_000), None),
                "pass {j}"
            );
        }
        // The wake for pass 11 comes 15 ms (720 frames) late: pass 12 played a stale buffer
        // and the buffer this fill writes plays at pass 13.
        let r = m.reading(11 * P - 48 + 720, 115_000_000, 12 * P, 13 * P);
        assert_eq!(r.xrun, Some(12 * P));
        assert_eq!(m.slip() as u64, P, "one buffer played again");
        assert_eq!(r.pair, None, "the DAC is in the stale buffer");
        // Pass 13: the DAC is still in the stale buffer (our frames up to the gap are not at
        // the DAC): no pair. Pass 14: our frames again, a buffer later than they would be.
        let r = m.reading(13 * P - 48, 130_000_000, 13 * P, 14 * P);
        assert_eq!((r.pair, r.xrun), (None, None));
        let r = m.reading(14 * P - 48, 140_000_000, 14 * P, 15 * P);
        assert_eq!((r.pair, r.xrun), (pair(13 * P - 48, 140_000_000), None));
        for j in 15..=19 {
            m.reading(j * P - 48, j * 10_000_000, j * P, (j + 1) * P);
        }
        // Pass 20, 446 frames late (within an eighth of a buffer of a replay): a near miss is
        // counted, but nothing slipped.
        let r = m.reading(20 * P - 48 + 446, 209_300_000, 20 * P, 21 * P);
        assert_eq!(r.xrun, Some(20 * P));
        assert_eq!(m.slip() as u64, P);
        let r = m.reading(21 * P - 48, 210_000_000, 21 * P, 22 * P);
        assert_eq!(r.xrun, None);
        assert_eq!(r.pair, None, "up to the near miss, not trusted");
        let r = m.reading(22 * P - 48, 220_000_000, 22 * P, 23 * P);
        assert_eq!(r.pair, pair(21 * P - 48, 220_000_000));
        for j in 23..=29 {
            m.reading(j * P - 48, j * 10_000_000, j * P, (j + 1) * P);
        }
        // A replay that looked like a near miss (470 frames late): counted short, and the next
        // period's lead, a buffer short, adds the buffer without a second xrun.
        let r = m.reading(30 * P - 48 + 470, 309_800_000, 30 * P, 31 * P);
        assert_eq!((r.xrun, m.slip() as u64), (Some(30 * P), P));
        let r = m.reading(31 * P - 48 + P, 310_000_000, 31 * P, 32 * P);
        assert_eq!((r.xrun, m.slip() as u64), (None, 2 * P));
        // Two buffers late at once: two slipped.
        for j in 32..=40 {
            m.reading(j * P - 48 + P, j * 10_000_000, j * P, (j + 1) * P);
        }
        let r = m.reading(41 * P - 48 + P + 1_200, 425_000_000, 41 * P, 42 * P);
        assert_eq!((r.xrun, m.slip() as u64), (Some(41 * P), 4 * P));
        m.restart();
        assert_eq!(m.slip() as u64, 0);
    }

    #[test]
    fn the_real_time_queue_keeps_order_and_counts_what_it_loses() {
        let q = RtEvents::new();
        assert_eq!(q.pop(), None);
        assert!(q.push(&OutputEvent::Xrun { at: 7 }));
        assert!(q.push(&OutputEvent::Lost));
        assert!(q.push(&OutputEvent::DefaultChanged));
        assert_eq!(q.pop(), Some(OutputEvent::Xrun { at: 7 }));
        assert_eq!(q.pop(), Some(OutputEvent::Lost));
        assert_eq!(q.pop(), Some(OutputEvent::DefaultChanged));
        assert_eq!(q.pop(), None);
        for k in 0..RT_EVENTS as u64 + 5 {
            q.push(&OutputEvent::Xrun { at: k });
        }
        assert_eq!(q.dropped(), 6, "one slot stays free");
        for k in 0..RT_EVENTS as u64 - 1 {
            assert_eq!(q.pop(), Some(OutputEvent::Xrun { at: k }));
        }
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn the_sink_tells_the_newest_receiver_and_says_lost_once_per_open() {
        let sink = EventSink::default();
        sink.send(OutputEvent::DefaultChanged);
        let old = sink.subscribe();
        let new = sink.subscribe();
        sink.send(OutputEvent::Xrun { at: 3 });
        sink.send(OutputEvent::Lost);
        sink.send(OutputEvent::Lost);
        assert!(sink.is_lost());
        assert!(!sink.claim_lost(), "already said");
        assert_eq!(
            new.try_iter().collect::<Vec<_>>(),
            [OutputEvent::Xrun { at: 3 }, OutputEvent::Lost]
        );
        assert!(old.try_recv().is_err());
        sink.rearm();
        assert!(!sink.is_lost());
        assert!(sink.claim_lost(), "a real-time thread claims it");
        sink.send(OutputEvent::Lost);
        assert!(new.try_recv().is_err(), "claimed, so not said again here");
    }
}
