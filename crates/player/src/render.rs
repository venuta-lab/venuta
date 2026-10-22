//! The renderer (section 4.7 of the Listen together design): the one stage that splices, gains
//! and resamples, on the output's own real-time thread.
//!
//! Each call: take the waiting render commands (never waiting for one), then, in chunks of at
//! most `CHUNK` frames, for each voice: take the blocks its decode thread has filled, check the
//! guard, copy what the chunk needs into the voice's window, resample (or pass the frames as
//! they are), run its sound tools, apply its envelope and add it into the mix; then the master
//! gain and the device's packing. At the end of the call: one marker per voice that sounded,
//! the status, and the call's deadline margin.
//!
//! Everything it ever uses is made in `Renderer::new` (on the control thread): voice slots with
//! their windows and block holds, the mix, the resampler's kernels. On the render thread it
//! allocates nothing, frees nothing, takes no lock and never waits: it only tries the queues
//! (`try_recv`, `try_send`) and reads atomics. The control thread polls its ends of the queues
//! the same way (a receiver blocked in `recv` would make the renderer's `try_send` take a lock
//! to wake it), and a decode thread keeps its feed until its voice is dropped, so a block given
//! back never frees here.
//!
//! The renderer enforces the splice table itself (section 4.7), so no command can make a hard
//! edge: a start partway fades in over at least 15 ms, a stop fades over at least 30 ms, a
//! dropped voice still sounding fades first, a voice that runs short fades out over frames it
//! holds (the guard), and gapless joins happen at the old voice's own end.

use crate::dsp::Chain;
use crate::output::{DeviceFormat, SampleKind};
use crate::output_api::Render;
use crate::sync::{Converter, Kernels, MAX_PPM};
use crate::voice_api::{
    Block, END_OF_SOURCE, FadeKind, MAX_CH, Marker, RateTarget, Refusal, RenderCmd, RenderEnds,
    RenderNote, Returned, Splice, VoiceId, VoiceTap,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering, fence};
use std::sync::mpsc::{SyncSender, TrySendError};
use together::time::MonoClock;

/// Voice slots: the control thread's three, and one for a voice fading out after its `Drop`
/// while the control thread already uses its slot again.
pub const SLOTS: usize = 4;
/// The most frames worked at once (so no buffer depends on the device's period).
pub const CHUNK: usize = 512;
/// The most blocks a voice holds (1 s of 384 kHz is 375).
pub const HOLD: usize = 512;
/// Things waiting to go back to the control thread.
const OUTBOX: usize = 64;
/// The least fade-in partway into a song, the least fade-out, the gain ramp, ms.
pub const FADE_IN_MS: u32 = 15;
pub const FADE_OUT_MS: u32 = 30;
pub const GAIN_MS: u32 = 20;
/// A voice that ran dry comes back once this much of its source is in hand (or its end), ms.
pub const RESUME_MS: u32 = 1_000;
/// The most input frames per device frame a trim adds.
const TRIM_MAX: f64 = 1.0 + MAX_PPM / 1e6;

/// What the renderer needs, made on the control thread once the output is open.
pub struct RenderSetup {
    /// The format the output runs at (`OutputInfo::format`).
    pub format: DeviceFormat,
    /// The most frames one call is asked for (`OutputInfo::period_frames`).
    pub max_period: u32,
    /// The renderer's ends of the queues (`render_link`).
    pub ends: RenderEnds,
    /// The trim the sync tick asks for (0 outside rooms).
    pub rate: Arc<RateTarget>,
    /// The clock the deadline margin is measured on (the platform's monotonic clock).
    pub clock: Arc<dyn MonoClock>,
}

// --- Envelopes -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Curve {
    /// Raised cosine (fades in and out, gain ramps).
    Raised,
    /// Equal power, rising (the incoming side of a crossfade): sin.
    PowerIn,
    /// Equal power, falling (the outgoing side): cos.
    PowerOut,
}

fn shape(c: Curve, t: f64) -> f64 {
    match c {
        Curve::Raised => (1.0 - (std::f64::consts::PI * t).cos()) / 2.0,
        Curve::PowerIn => (t * std::f64::consts::FRAC_PI_2).sin(),
        Curve::PowerOut => 1.0 - (t * std::f64::consts::FRAC_PI_2).cos(),
    }
}

/// One stretch of a level: `from` before `start`, `to` from `start + len`, the curve between.
#[derive(Debug, Clone, Copy)]
struct Seg {
    from: f64,
    to: f64,
    start: u64,
    len: u64,
    curve: Curve,
}

impl Seg {
    fn steady(level: f64) -> Seg {
        Seg {
            from: level,
            to: level,
            start: 0,
            len: 0,
            curve: Curve::Raised,
        }
    }

    fn end(&self) -> u64 {
        self.start + self.len
    }

    fn at(&self, f: u64) -> f64 {
        if f < self.start {
            self.from
        } else if f >= self.end() {
            self.to
        } else {
            let t = (f - self.start) as f64 / self.len as f64;
            self.from + (self.to - self.from) * shape(self.curve, t)
        }
    }
}

/// A level that moves by fades, carried across calls: the stretch under way, and the next one,
/// which begins from whatever level the first is at when it starts (so a fade that begins during
/// another never steps).
#[derive(Debug, Clone, Copy)]
struct Envelope {
    seg: Seg,
    next: Option<Seg>,
}

impl Envelope {
    fn steady(level: f64) -> Envelope {
        Envelope {
            seg: Seg::steady(level),
            next: None,
        }
    }

    /// Up from silence to 1 over `len` frames from `at` (at once when `len` is 0).
    fn rise(at: u64, len: u64, curve: Curve) -> Envelope {
        Envelope {
            seg: Seg {
                from: 0.0,
                to: 1.0,
                start: at,
                len,
                curve,
            },
            next: None,
        }
    }

    /// The level at frame `f` (asked in order).
    fn level(&mut self, f: u64) -> f64 {
        if let Some(n) = self.next
            && f >= n.start
        {
            self.seg = Seg {
                from: self.seg.at(n.start),
                ..n
            };
            self.next = None;
        }
        self.seg.at(f)
    }

    /// Fade to `to` over `len` frames from `at`, from the level it is at then. A fade scheduled
    /// to begin later is replaced.
    fn fade(&mut self, at: u64, len: u64, to: f64, curve: Curve) {
        if let Some(n) = self.next.take()
            && n.start <= at
        {
            self.seg = Seg {
                from: self.seg.at(n.start),
                ..n
            };
        }
        self.next = Some(Seg {
            from: 0.0,
            to,
            start: at,
            len,
            curve,
        });
    }

    /// The frame from which it is silent for good, when it is heading there.
    fn silent_from(&self) -> Option<u64> {
        match self.next {
            Some(n) => (n.to == 0.0).then(|| n.end()),
            None => (self.seg.to == 0.0).then(|| self.seg.end()),
        }
    }

    /// At exactly `level` all through frames `from..to`.
    fn steady_over(&self, from: u64, to: u64, level: f64) -> bool {
        self.seg.to == level && self.seg.end() <= from && self.next.is_none_or(|n| n.start >= to)
    }

    /// Moving at frame `f`.
    fn moving_at(&self, f: u64) -> bool {
        f < self.seg.end() || self.next.is_some_and(|n| n.start <= f)
    }
}

// --- What a voice holds ------------------------------------------------------------------------

/// A block taken from the voice's ring: `from` frames of it already used; `brk` when it does not
/// follow the block before it (a newer epoch, or a hole in the content frames).
struct HeldBlock {
    b: Box<Block>,
    from: usize,
    brk: bool,
}

impl HeldBlock {
    fn left(&self) -> usize {
        usize::from(self.b.frames) - self.from
    }

    fn content(&self) -> u64 {
        self.b.first_frame + self.from as u64
    }

    fn last(&self) -> bool {
        self.b.flags & END_OF_SOURCE != 0
    }
}

/// The blocks a voice holds, in order (a fixed ring of up to `HOLD`).
struct Held {
    q: Box<[Option<HeldBlock>]>,
    head: usize,
    len: usize,
}

impl Held {
    fn new() -> Held {
        Held {
            q: (0..HOLD).map(|_| None).collect(),
            head: 0,
            len: 0,
        }
    }

    fn full(&self) -> bool {
        self.len == HOLD
    }

    fn push(&mut self, h: HeldBlock) {
        let i = (self.head + self.len) % HOLD;
        self.q[i] = Some(h);
        self.len += 1;
    }

    fn get(&self, k: usize) -> Option<&HeldBlock> {
        if k >= self.len {
            return None;
        }
        self.q[(self.head + k) % HOLD].as_ref()
    }

    fn front_mut(&mut self) -> Option<&mut HeldBlock> {
        if self.len == 0 {
            return None;
        }
        self.q[self.head].as_mut()
    }

    fn pop(&mut self) -> Option<HeldBlock> {
        if self.len == 0 {
            return None;
        }
        let h = self.q[self.head].take();
        self.head = (self.head + 1) % HOLD;
        self.len -= 1;
        h
    }

    /// Give every block back to its ring.
    fn clear(&mut self, tap: Option<&VoiceTap>) {
        while let Some(h) = self.pop() {
            give(tap, h.b);
        }
    }

    /// Give back what lies before content frame `want` (the place a voice not yet sounding is
    /// to start at); the block holding `want` is used from there. The source's last block is
    /// kept (empty) when `want` is past it, so the voice sees its end.
    fn skip_before(&mut self, tap: Option<&VoiceTap>, want: u64) {
        while let Some(h) = self.front_mut() {
            let end = h.b.first_frame + u64::from(h.b.frames);
            if end <= want && !h.last() {
                if let Some(h) = self.pop() {
                    give(tap, h.b);
                }
                continue;
            }
            if h.content() < want {
                h.from = (want.min(end) - h.b.first_frame) as usize;
            }
            // The voice starts here: what came before is no longer a break.
            h.brk = false;
            return;
        }
    }

    /// Frames in hand from its front, after `n` already counted, until a break, the source's
    /// end, or past `limit`.
    fn run(&self, mut n: usize, ch: usize, limit: usize) -> Run {
        let mut k = 0;
        while let Some(h) = self.get(k) {
            if h.brk || usize::from(h.b.channels) != ch {
                return Run {
                    frames: n,
                    ended: false,
                    brk: true,
                };
            }
            n += h.left();
            if h.last() {
                return Run {
                    frames: n,
                    ended: true,
                    brk: false,
                };
            }
            if n >= limit {
                break;
            }
            k += 1;
        }
        Run {
            frames: n,
            ended: false,
            brk: false,
        }
    }
}

fn give(tap: Option<&VoiceTap>, b: Box<Block>) {
    if let Some(t) = tap {
        t.give_back(b);
    }
}

/// Frames in hand, contiguous: how many, and whether the run ends with the source's end or a
/// break.
#[derive(Debug, Clone, Copy)]
struct Run {
    frames: usize,
    ended: bool,
    brk: bool,
}

/// A voice's window for the filter: `at` is the read point, the `half` frames before it the
/// filter's left side, the frames after it copied from the held blocks; and its resampler.
struct Lane {
    win: Box<[i32]>,
    ch: usize,
    len: usize,
    at: usize,
    /// The content frame of window frame 0 (`content = base + index`).
    base: i64,
    /// The window frame where the source ended (silence from there).
    end: Option<usize>,
    /// Gapless: the window frame where the following voice's first frame is, and that voice's
    /// content base.
    join: Option<(usize, i64)>,
    /// The window's frames stop at a break in the blocks.
    brk: bool,
    /// The furthest the resampler may have read.
    read_hi: usize,
    conv: Converter,
}

impl Lane {
    fn new(frames: usize) -> Lane {
        Lane {
            win: vec![0; frames * MAX_CH].into_boxed_slice(),
            ch: 1,
            len: 0,
            at: 0,
            base: 0,
            end: None,
            join: None,
            brk: false,
            read_hi: 0,
            conv: Converter::new(1, 1, 0.0),
        }
    }

    /// Start over at content frame `content`, silence before it.
    fn reset(&mut self, half: usize, ch: usize, content: u64, conv: Converter) {
        self.ch = ch.clamp(1, MAX_CH);
        self.win[..half * self.ch].fill(0);
        self.len = half;
        self.at = half;
        self.base = content as i64 - half as i64;
        self.end = None;
        self.join = None;
        self.brk = false;
        self.read_hi = 0;
        self.conv = conv;
    }

    /// Keep only the filter's left side before the read point.
    fn compact(&mut self, half: usize) {
        if self.at <= half {
            return;
        }
        let cut = self.at - half;
        let ch = self.ch;
        self.win.copy_within(cut * ch..self.len * ch, 0);
        self.len -= cut;
        self.at -= cut;
        self.base += cut as i64;
        self.end = self.end.map(|e| e.saturating_sub(cut));
        self.join = self
            .join
            .map(|(j, b)| (j.saturating_sub(cut), b + cut as i64));
        self.read_hi = self.read_hi.saturating_sub(cut);
    }

    /// The content place of the next frame out.
    fn content(&self) -> f64 {
        (self.base + self.at as i64) as f64 + self.conv.phase()
    }

    /// Copy frames from `held` until the window holds `upto` frames, or the run stops (the
    /// source's end, a break, nothing more in hand).
    fn fill(&mut self, held: &mut Held, tap: Option<&VoiceTap>, upto: usize) {
        let ch = self.ch;
        while self.len < upto && self.end.is_none() && !self.brk {
            let Some(h) = held.front_mut() else {
                return;
            };
            if h.brk || usize::from(h.b.channels) != ch {
                self.brk = true;
                return;
            }
            let n = h.left().min(upto - self.len);
            self.win[self.len * ch..(self.len + n) * ch]
                .copy_from_slice(&h.b.samples[h.from * ch..(h.from + n) * ch]);
            self.len += n;
            h.from += n;
            if h.left() == 0 {
                let last = h.last();
                if let Some(h) = held.pop() {
                    give(tap, h.b);
                }
                if last {
                    self.end = Some(self.len);
                }
            }
        }
    }
}

/// Device frames until the read point passes window frame `x`.
fn frames_until(c: &Converter, at: usize, x: usize) -> usize {
    if x <= at {
        return 0;
    }
    if c.idle() {
        return x - at;
    }
    (((x - at) as f64 - c.phase()) / c.ratio()).ceil().max(1.0) as usize
}

// --- Voice slots -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    /// Given, not sounding yet: it starts at `start`.
    Waiting,
    /// Gapless after the voice in that slot: it starts at that voice's end.
    Following(usize),
    Sounding,
    /// Ran dry and faded out: back when refilled.
    Starved,
    /// Stopped or ended: silent until dropped.
    Silent,
    /// Dropped: its tap goes back at the end of the call.
    Releasing,
}

/// Why a voice is fading to silence on the guard's account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dry {
    No,
    /// It ran short: it waits to be refilled.
    Starve,
    /// Its blocks break (a newer epoch, or a hole): it comes back at the place after the break.
    Break,
}

#[derive(Debug, Clone, Copy)]
struct Start {
    at: u64,
    content: u64,
    fade: u64,
    curve: Curve,
    kind: FadeKind,
}

struct Slot {
    voice: VoiceId,
    tap: Option<VoiceTap>,
    state: State,
    rate: u32,
    /// Source frames per device frame, untrimmed.
    base: f64,
    kernel: usize,
    held: Held,
    lane: Lane,
    epoch: u32,
    /// The content frame after the last block taken (same epoch).
    next_content: Option<u64>,
    start: Start,
    /// The slot that follows this voice gaplessly.
    then: Option<usize>,
    /// Started at the frame after this voice's last (a gapless join between formats).
    joined_from: Option<VoiceId>,
    env: Envelope,
    dry: Dry,
    /// A newer epoch came while it sounded: move to it (faded).
    flush: bool,
    /// After the dry fade, start the window again after the break.
    relane: bool,
    drop_when_silent: bool,
    chain: Option<Box<Chain>>,
    /// What it plays is its source bit for bit (for the `Resampling` notes).
    exact: bool,
    /// Device frames done up to (in this call).
    done: u64,
    /// This call's first frame sounded and its content place, and the last.
    mark: Option<(u64, f64)>,
    last: (u64, f64),
}

impl Slot {
    fn new(window: usize) -> Slot {
        Slot {
            voice: VoiceId(0),
            tap: None,
            state: State::Free,
            rate: 1,
            base: 1.0,
            kernel: 0,
            held: Held::new(),
            lane: Lane::new(window),
            epoch: 0,
            next_content: None,
            start: Start {
                at: 0,
                content: 0,
                fade: 0,
                curve: Curve::Raised,
                kind: FadeKind::In,
            },
            then: None,
            joined_from: None,
            env: Envelope::steady(0.0),
            dry: Dry::No,
            flush: false,
            relane: false,
            drop_when_silent: false,
            chain: None,
            exact: true,
            done: 0,
            mark: None,
            last: (0, 0.0),
        }
    }

    fn in_use(&self) -> bool {
        self.state != State::Free
    }

    /// The content frame a voice not yet sounding wants at device frame `f`.
    fn wanted(&self, f: u64) -> u64 {
        match self.state {
            State::Waiting if f > self.start.at => {
                self.start.content + ((f - self.start.at) as f64 * self.base).round() as u64
            }
            _ => self.start.content,
        }
    }
}

/// `slots[i]`, and `slots[j]` when `j` is another slot.
fn two(slots: &mut [Slot], i: usize, j: Option<usize>) -> (&mut Slot, Option<&mut Slot>) {
    match j {
        Some(j) if j != i => {
            if i < j {
                let (a, b) = slots.split_at_mut(j);
                (&mut a[i], Some(&mut b[0]))
            } else {
                let (a, b) = slots.split_at_mut(i);
                (&mut b[0], Some(&mut a[j]))
            }
        }
        _ => (&mut slots[i], None),
    }
}

/// Add `m` frames of a voice (`vch` channels) into the mix (`dch` channels) under its envelope,
/// the channels fitted as `output::remap` does (mono doubles; extra dropped; missing silent).
#[allow(clippy::too_many_arguments)]
fn mix_in(
    env: &mut Envelope,
    v: &[i32],
    vch: usize,
    mix: &mut [f64],
    dch: usize,
    s: u64,
    m: usize,
) {
    let full = env.steady_over(s, s + m as u64, 1.0);
    for k in 0..m {
        let g = if full { 1.0 } else { env.level(s + k as u64) };
        if g == 0.0 {
            continue;
        }
        let x = &v[k * vch..(k + 1) * vch];
        let o = &mut mix[k * dch..(k + 1) * dch];
        if vch == dch {
            for (o, &x) in o.iter_mut().zip(x) {
                *o += g * f64::from(x);
            }
        } else if vch == 1 {
            let x = g * f64::from(x[0]);
            for o in o.iter_mut() {
                *o += x;
            }
        } else {
            for (o, &x) in o.iter_mut().zip(x) {
                *o += g * f64::from(x);
            }
        }
    }
}

/// Pack left-justified samples for the device: exactly what `output::encode` gives at gain 1,
/// into a given slice (`samples.len()` samples' worth of bytes).
pub fn encode_into(samples: &[i32], fmt: &DeviceFormat, out: &mut [u8]) {
    match (fmt.kind, fmt.container_bits) {
        (SampleKind::Float, _) => {
            for (&s, o) in samples.iter().zip(out.as_chunks_mut::<4>().0) {
                let v = s as f32 / 2_147_483_648.0;
                o.copy_from_slice(&v.to_le_bytes());
            }
        }
        (SampleKind::Int, 16) => {
            for (&s, o) in samples.iter().zip(out.as_chunks_mut::<2>().0) {
                o.copy_from_slice(&((s >> 16) as i16).to_le_bytes());
            }
        }
        (SampleKind::Int, 24) => {
            for (&s, o) in samples.iter().zip(out.as_chunks_mut::<3>().0) {
                o.copy_from_slice(&(s >> 8).to_le_bytes()[..3]);
            }
        }
        (SampleKind::Int, _) => {
            let mask = if fmt.valid_bits >= 32 {
                -1i32
            } else {
                !((1i32 << (32 - fmt.valid_bits)) - 1)
            };
            for (&s, o) in samples.iter().zip(out.as_chunks_mut::<4>().0) {
                o.copy_from_slice(&(s & mask).to_le_bytes());
            }
        }
    }
}

// --- What goes back ------------------------------------------------------------------------------

/// Notes and markers to the control thread: only tried, what a full queue loses counted (and
/// said in a `Dropped` note once one gets through).
struct Out {
    notes: SyncSender<RenderNote>,
    markers: SyncSender<Marker>,
    lost_notes: u32,
    lost_markers: u32,
    status: Arc<RenderStatus>,
}

impl Out {
    fn note(&mut self, n: RenderNote) {
        if self.lost_notes + self.lost_markers > 0 {
            let d = RenderNote::Dropped {
                notes: self.lost_notes,
                markers: self.lost_markers,
            };
            if self.notes.try_send(d).is_err() {
                self.lost_notes += 1;
                self.status.lost.fetch_add(1, Ordering::Relaxed);
                return;
            }
            self.lost_notes = 0;
            self.lost_markers = 0;
        }
        match n {
            RenderNote::Late { .. } => {
                self.status.late.fetch_add(1, Ordering::Relaxed);
            }
            RenderNote::Refused { .. } => {
                self.status.refused.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        if self.notes.try_send(n).is_err() {
            self.lost_notes += 1;
            self.status.lost.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn marker(&mut self, m: Marker) {
        if self.markers.try_send(m).is_err() {
            self.lost_markers += 1;
            self.status.lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Taps and chains going back to be dropped off this thread, held here while the queue is full.
struct Back {
    tx: SyncSender<Returned>,
    q: Box<[Option<Returned>]>,
    len: usize,
}

impl Back {
    fn room(&self) -> usize {
        OUTBOX - self.len
    }

    fn push(&mut self, r: Returned) {
        match self.tx.try_send(r) {
            Ok(()) => {}
            Err(TrySendError::Full(r) | TrySendError::Disconnected(r)) => {
                if self.len < OUTBOX {
                    self.q[self.len] = Some(r);
                    self.len += 1;
                }
                // (No room is never reached: a command is taken only while there is room.)
            }
        }
    }

    fn flush(&mut self) {
        let mut k = 0;
        while k < self.len {
            let Some(r) = self.q[k].take() else {
                k += 1;
                continue;
            };
            match self.tx.try_send(r) {
                Ok(()) => k += 1,
                Err(TrySendError::Full(r) | TrySendError::Disconnected(r)) => {
                    self.q[k] = Some(r);
                    break;
                }
            }
        }
        // Close up what went.
        let mut w = 0;
        for r in 0..self.len {
            if let Some(x) = self.q[r].take() {
                self.q[w] = Some(x);
                w += 1;
            }
        }
        self.len = w;
    }
}

// --- The renderer ------------------------------------------------------------------------------

/// The render stage, handed to `Output::start`.
pub struct Renderer {
    fmt: DeviceFormat,
    dch: usize,
    bpf: usize,
    rate: u32,
    period: u64,
    f15: u64,
    f20: u64,
    f30: u64,
    kernels: Kernels,
    slots: Box<[Slot]>,
    mix: Box<[f64]>,
    vbuf: Box<[i32]>,
    obuf: Box<[i32]>,
    gain: Envelope,
    commands: std::sync::mpsc::Receiver<RenderCmd>,
    out: Out,
    back: Back,
    target: Arc<RateTarget>,
    clock: Arc<dyn MonoClock>,
    status: Arc<RenderStatus>,
    trim: f64,
}

impl Renderer {
    /// Everything the renderer will use, made now; and its status, for the status thread.
    pub fn new(setup: RenderSetup) -> (Renderer, Arc<RenderStatus>) {
        let fmt = setup.format;
        let rate = fmt.rate.max(1);
        let dch = usize::from(fmt.channels.max(1));
        let kernels = Kernels::for_device(rate);
        let half = kernels.max_half();
        let window = 2 * half + (CHUNK as f64 * kernels.max_ratio() * TRIM_MAX).ceil() as usize + 8;
        let ms = |m: u32| u64::from(rate) * u64::from(m) / 1000;
        let status = Arc::new(RenderStatus::default());
        let RenderEnds {
            commands,
            markers,
            notes,
            returned,
        } = setup.ends;
        // Read once here, so nothing of the clock is set up on the render thread.
        let _ = setup.clock.now_ns();
        let r = Renderer {
            dch,
            bpf: fmt.bytes_per_frame(),
            rate,
            period: u64::from(setup.max_period.max(1)),
            f15: ms(FADE_IN_MS),
            f20: ms(GAIN_MS),
            f30: ms(FADE_OUT_MS),
            kernels,
            slots: (0..SLOTS).map(|_| Slot::new(window)).collect(),
            mix: vec![0.0; CHUNK * dch].into_boxed_slice(),
            vbuf: vec![0; CHUNK * MAX_CH].into_boxed_slice(),
            obuf: vec![0; CHUNK * dch].into_boxed_slice(),
            gain: Envelope::steady(1.0),
            commands,
            out: Out {
                notes,
                markers,
                lost_notes: 0,
                lost_markers: 0,
                status: status.clone(),
            },
            back: Back {
                tx: returned,
                q: (0..OUTBOX).map(|_| None).collect(),
                len: 0,
            },
            target: setup.rate,
            clock: setup.clock,
            status: status.clone(),
            trim: 0.0,
            fmt,
        };
        (r, status)
    }

    /// The guard, in input frames: a 30 ms fade, one period more, and the filter's reach.
    fn guard(&self, r_hi: f64, half: usize) -> usize {
        ((self.f30 + self.period) as f64 * r_hi).ceil() as usize + half + 1
    }

    fn find(&self, v: VoiceId) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.in_use() && s.state != State::Releasing && s.voice == v)
    }

    /// The slot following `i` gaplessly, if one does.
    fn follower(&self, i: usize) -> Option<usize> {
        self.slots[i]
            .then
            .filter(|&j| self.slots[j].state == State::Following(i))
    }

    /// Voices following `i` start on their own frames instead (`i` is gone or stopped).
    fn orphan(&mut self, i: usize) {
        self.slots[i].then = None;
        for s in self.slots.iter_mut() {
            if s.state == State::Following(i) {
                s.state = State::Waiting;
            }
        }
    }

    // --- Commands ---

    /// A free slot made ready for `voice` (or the tap back, refused).
    fn take_slot(&mut self, voice: VoiceId, tap: VoiceTap, rate: u32) -> Option<usize> {
        let Some(kernel) = self.kernels.index(rate) else {
            self.out.note(RenderNote::Refused {
                voice,
                why: Refusal::Rate,
            });
            self.back.push(Returned::Tap(tap));
            return None;
        };
        let Some(i) = self.slots.iter().position(|s| s.state == State::Free) else {
            self.out.note(RenderNote::Refused {
                voice,
                why: Refusal::NoSlot,
            });
            self.back.push(Returned::Tap(tap));
            return None;
        };
        let s = &mut self.slots[i];
        s.voice = voice;
        s.tap = Some(tap);
        s.state = State::Waiting;
        s.rate = rate.max(1);
        s.base = f64::from(s.rate) / f64::from(self.rate);
        s.kernel = kernel;
        s.epoch = 0;
        s.next_content = None;
        s.then = None;
        s.joined_from = None;
        s.env = Envelope::steady(0.0);
        s.dry = Dry::No;
        s.flush = false;
        s.relane = false;
        s.drop_when_silent = false;
        s.exact = true;
        s.done = 0;
        s.mark = None;
        Some(i)
    }

    fn commands(&mut self, now: u64) {
        while self.back.room() >= 2 {
            let Ok(cmd) = self.commands.try_recv() else {
                break;
            };
            match cmd {
                RenderCmd::StartAt {
                    voice,
                    tap,
                    at,
                    content,
                    rate,
                    fade_in,
                } => {
                    if let Some(i) = self.take_slot(voice, tap, rate) {
                        self.slots[i].start = Start {
                            at,
                            content,
                            fade: u64::from(fade_in),
                            curve: Curve::Raised,
                            kind: FadeKind::In,
                        };
                    }
                }
                RenderCmd::SwitchAt {
                    from,
                    to,
                    tap,
                    at,
                    to_content,
                    rate,
                    how,
                } => self.switch(now, from, to, tap, at, to_content, rate, how),
                RenderCmd::StopAt { voice, at, fade } => {
                    let Some(i) = self.find(voice) else {
                        self.out.note(RenderNote::Refused {
                            voice,
                            why: Refusal::NoVoice,
                        });
                        continue;
                    };
                    self.stop(i, now, at, u64::from(fade));
                }
                RenderCmd::Drop { voice } => {
                    let Some(i) = self.find(voice) else {
                        self.out.note(RenderNote::Refused {
                            voice,
                            why: Refusal::NoVoice,
                        });
                        continue;
                    };
                    if self.slots[i].state == State::Sounding {
                        let f30 = self.f30;
                        let s = &mut self.slots[i];
                        if s.env.silent_from().is_none_or(|f| f > now + f30) {
                            s.env.fade(now, f30, 0.0, Curve::Raised);
                            self.out.note(RenderNote::Faded {
                                voice,
                                from: now,
                                to: now + f30,
                                kind: FadeKind::Out,
                            });
                        }
                        s.dry = Dry::No;
                        s.flush = false;
                        s.drop_when_silent = true;
                    } else {
                        self.slots[i].state = State::Releasing;
                    }
                }
                RenderCmd::Gain { target } => {
                    let g0 = self.gain.level(now);
                    let to = f64::from(target);
                    if to != g0 || self.gain.moving_at(now) {
                        self.gain.fade(now, self.f20, to, Curve::Raised);
                        self.out.note(RenderNote::Gain {
                            from: now,
                            to: now + self.f20,
                            target,
                        });
                    }
                }
                RenderCmd::Chain { voice, chain } => match self.find(voice) {
                    Some(i) => {
                        if let Some(old) = self.slots[i].chain.replace(chain) {
                            self.back.push(Returned::Chain(old));
                        }
                    }
                    None => {
                        self.out.note(RenderNote::Refused {
                            voice,
                            why: Refusal::NoVoice,
                        });
                        self.back.push(Returned::Chain(chain));
                    }
                },
            }
        }
    }

    /// Fade `i` out over `fade` (at least 30 ms) from `at` (now, if that has passed).
    fn stop(&mut self, i: usize, now: u64, at: u64, fade: u64) {
        let f30 = self.f30;
        let s = &mut self.slots[i];
        match s.state {
            State::Sounding => {
                let len = fade.max(f30);
                let from = at.max(now);
                s.env.fade(from, len, 0.0, Curve::Raised);
                s.dry = Dry::No;
                s.flush = false;
                let voice = s.voice;
                self.out.note(RenderNote::Faded {
                    voice,
                    from,
                    to: from + len,
                    kind: FadeKind::Out,
                });
                if at < now {
                    self.out.note(RenderNote::Late {
                        voice,
                        at: now,
                        by: now - at,
                    });
                }
            }
            State::Waiting | State::Following(_) | State::Starved => {
                s.state = State::Silent;
                self.orphan(i);
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn switch(
        &mut self,
        now: u64,
        from: VoiceId,
        to: VoiceId,
        tap: VoiceTap,
        at: u64,
        to_content: u64,
        rate: u32,
        how: Splice,
    ) {
        let a = self.find(from);
        let Some(j) = self.take_slot(to, tap, rate) else {
            return;
        };
        if a.is_none() {
            self.out.note(RenderNote::Refused {
                voice: from,
                why: Refusal::NoVoice,
            });
        }
        let sounding = a.filter(|&i| self.slots[i].state == State::Sounding);
        let mut start = Start {
            at,
            content: to_content,
            fade: 0,
            curve: Curve::Raised,
            kind: FadeKind::In,
        };
        let f30 = self.f30;
        match how {
            Splice::Crossfade { frames } => {
                let len = u64::from(frames).max(f30);
                start.fade = len;
                start.curve = Curve::PowerIn;
                start.kind = FadeKind::CrossIn;
                if let Some(i) = sounding {
                    let from_at = at.max(now);
                    self.slots[i].env.fade(from_at, len, 0.0, Curve::PowerOut);
                    self.slots[i].dry = Dry::No;
                    self.out.note(RenderNote::Faded {
                        voice: from,
                        from: from_at,
                        to: from_at + len,
                        kind: FadeKind::CrossOut,
                    });
                    if at < now {
                        self.out.note(RenderNote::Late {
                            voice: from,
                            at: now,
                            by: now - at,
                        });
                    }
                }
            }
            Splice::FadeOutThenIn { out_frames: n } | Splice::EndFade { frames: n } => {
                if let Some(i) = sounding {
                    let len = u64::from(n).max(f30);
                    let asked = at.saturating_sub(len);
                    let from_at = asked.max(now);
                    self.slots[i].env.fade(from_at, len, 0.0, Curve::Raised);
                    self.slots[i].dry = Dry::No;
                    self.out.note(RenderNote::Faded {
                        voice: from,
                        from: from_at,
                        to: from_at + len,
                        kind: FadeKind::Out,
                    });
                    if from_at > asked {
                        // Late: the new voice starts where the fade ends, as far into its song as
                        // the room is by then.
                        let by = from_at - asked;
                        start.at = at + by;
                        start.content = to_content
                            + (by as f64 * f64::from(rate) / f64::from(self.rate)).round() as u64;
                        self.out.note(RenderNote::Late {
                            voice: from,
                            at: from_at,
                            by,
                        });
                    }
                }
            }
            Splice::Gapless => {
                if let Some(i) = a.filter(|&i| {
                    matches!(
                        self.slots[i].state,
                        State::Sounding | State::Waiting | State::Starved | State::Following(_)
                    )
                }) {
                    if let Some(old) = self.slots[i].then.replace(j)
                        && self.slots[old].state == State::Following(i)
                    {
                        self.slots[old].state = State::Waiting;
                    }
                    self.slots[j].state = State::Following(i);
                }
            }
        }
        if let Some(i) = a
            && sounding.is_none()
            && !matches!(how, Splice::Gapless)
            && matches!(self.slots[i].state, State::Waiting | State::Following(_))
        {
            // It never sounded: it simply won't.
            self.slots[i].state = State::Silent;
            self.orphan(i);
        }
        self.slots[j].start = start;
    }

    // --- Taking blocks ---

    fn pull(&mut self, i: usize, c0: u64) {
        let s = &mut self.slots[i];
        if matches!(s.state, State::Free | State::Silent | State::Releasing) {
            return;
        }
        let Some(tap) = s.tap.as_ref() else {
            return;
        };
        while !s.held.full() {
            let Some(b) = tap.pop() else {
                break;
            };
            if b.epoch < s.epoch {
                tap.give_back(b);
                continue;
            }
            let mut brk = false;
            if b.epoch > s.epoch {
                s.epoch = b.epoch;
                s.next_content = None;
                match s.state {
                    State::Waiting | State::Following(_) => s.held.clear(Some(tap)),
                    _ => {
                        brk = true;
                        s.flush = true;
                    }
                }
            }
            let first = b.first_frame;
            let frames = u64::from(b.frames);
            let mut from = 0;
            if let Some(nx) = s.next_content
                && !brk
            {
                if first < nx {
                    let skip = nx - first;
                    if skip >= frames && b.flags & END_OF_SOURCE == 0 {
                        tap.give_back(b);
                        continue;
                    }
                    from = skip.min(frames) as usize;
                } else if first > nx {
                    brk = true;
                }
            }
            s.next_content = Some(first + frames);
            s.held.push(HeldBlock { b, from, brk });
        }
        if matches!(s.state, State::Waiting | State::Following(_)) {
            let want = s.wanted(c0.max(s.start.at));
            s.held.skip_before(s.tap.as_ref(), want);
        }
    }

    // --- Starting, coming back ---

    /// A waiting voice starts in this chunk if it is ready (what the chunk needs plus the guard
    /// in hand, or its end): at its frame, or, late, at the place the mapping says for now.
    fn begin(&mut self, i: usize, s: u64, c1: u64) -> bool {
        let st = self.slots[i].start;
        if st.at >= c1 {
            return false;
        }
        let rate = self.slots[i].rate;
        let base = f64::from(rate) / f64::from(self.rate);
        let r_hi = base * TRIM_MAX;
        let half = self.kernels.get(self.slots[i].kernel).half();
        let guard = self.guard(r_hi, half);
        let f0 = s.max(st.at);
        let want = self.slots[i].wanted(f0);
        let slot = &mut self.slots[i];
        slot.held.skip_before(slot.tap.as_ref(), want);
        let Some(front) = slot.held.get(0) else {
            return false;
        };
        let first = front.content();
        let ch = usize::from(front.b.channels).clamp(1, MAX_CH);
        // Decoded from later than wanted: the voice starts where its first frame belongs.
        let (f, content) = if first > want {
            let f = st.at + ((first - st.content) as f64 / base).ceil() as u64;
            (f.max(f0), first)
        } else {
            (f0, want)
        };
        if f >= c1 {
            return false;
        }
        let need = ((c1 - f) as f64 * r_hi).ceil() as usize + guard;
        let run = slot.held.run(0, ch, need);
        if !run.ended && run.frames < need {
            return false;
        }
        let late = f > st.at;
        let fade = match st.curve {
            Curve::PowerIn => st.fade.max(self.f30),
            _ if content > 0 || late => st.fade.max(self.f15),
            _ => st.fade,
        };
        let conv = Converter::new(rate, self.rate, self.trim);
        slot.lane.reset(half, ch, content, conv);
        slot.env = Envelope::rise(f, fade, st.curve);
        slot.state = State::Sounding;
        slot.dry = Dry::No;
        slot.flush = false;
        slot.exact = true;
        slot.done = f;
        let voice = slot.voice;
        let joined = slot.joined_from.take();
        if let Some(from) = joined
            && fade == 0
            && !late
        {
            self.out.note(RenderNote::Joined {
                from,
                to: voice,
                at: f,
            });
        }
        self.out.note(RenderNote::Started {
            voice,
            at: f,
            content,
            fade: fade as u32,
        });
        if fade > 0 {
            self.out.note(RenderNote::Faded {
                voice,
                from: f,
                to: f + fade,
                kind: st.kind,
            });
        }
        if late && joined.is_none() {
            self.out.note(RenderNote::Late {
                voice,
                at: f,
                by: f - st.at,
            });
        }
        true
    }

    /// A voice that ran dry comes back (15 ms fade-in) once 1 s of its source is in hand, or its
    /// end.
    fn resume(&mut self, i: usize, s: u64, c1: u64) -> bool {
        let rate = self.slots[i].rate;
        let half = self.kernels.get(self.slots[i].kernel).half();
        let r_hi = f64::from(rate) / f64::from(self.rate) * TRIM_MAX;
        let guard = self.guard(r_hi, half);
        let trim = self.trim;
        let device = self.rate;
        let slot = &mut self.slots[i];
        if slot.relane {
            // After a break: start the window again at the place after it.
            let Some(front) = slot.held.front_mut() else {
                return false;
            };
            front.brk = false;
            let content = front.content();
            let ch = usize::from(front.b.channels);
            slot.lane
                .reset(half, ch, content, Converter::new(rate, device, trim));
            slot.relane = false;
        }
        let need = ((c1 - s) as f64 * r_hi).ceil() as usize + guard;
        let enough = (u64::from(rate) * u64::from(RESUME_MS) / 1000) as usize;
        let lane = &slot.lane;
        let ahead = lane.len - lane.at;
        let run = if lane.end.is_some() {
            Run {
                frames: ahead,
                ended: true,
                brk: false,
            }
        } else {
            slot.held.run(ahead, lane.ch, enough.max(need))
        };
        if !(run.ended || run.frames >= enough.max(need)) {
            return false;
        }
        slot.env = Envelope::rise(s, self.f15, Curve::Raised);
        slot.state = State::Sounding;
        slot.dry = Dry::No;
        slot.done = s;
        let voice = slot.voice;
        let content = slot.lane.content().round() as u64;
        self.out.note(RenderNote::Resumed {
            voice,
            at: s,
            content,
        });
        self.out.note(RenderNote::Faded {
            voice,
            from: s,
            to: s + self.f15,
            kind: FadeKind::In,
        });
        true
    }

    // --- Sounding ---

    /// One voice's part of the chunk `c0..c1`. Returns a voice to start at this one's end (a
    /// gapless follower that could not continue its window).
    fn voice(&mut self, i: usize, c0: u64, c1: u64) -> Option<usize> {
        let s = c0.max(self.slots[i].done);
        if s >= c1 {
            return None;
        }
        match self.slots[i].state {
            State::Waiting => {
                if !self.begin(i, s, c1) {
                    return None;
                }
            }
            State::Starved => {
                if !self.resume(i, s, c1) {
                    return None;
                }
            }
            State::Sounding => {}
            _ => return None,
        }
        self.sound(i, c0, c1)
    }

    fn sound(&mut self, mut i: usize, c0: u64, c1: u64) -> Option<usize> {
        let guard_span = self.f30 + self.period;
        let f30 = self.f30;
        let dch = self.dch;
        let trim = self.trim;
        loop {
            let fol = self.follower(i);
            let Renderer {
                slots,
                kernels,
                mix,
                vbuf,
                out,
                status,
                ..
            } = self;
            let (slot, mut fslot) = two(slots, i, fol);
            let s = slot.done;
            if s >= c1 || slot.state != State::Sounding {
                return None;
            }
            let k = kernels.get(slot.kernel);
            let half = k.half();
            let r_hi = slot.lane.conv.base() * TRIM_MAX;
            let voice = slot.voice;

            // 1. The guard: what this chunk needs and a fade's worth more must be in hand, or
            //    it fades out now over what it holds.
            if slot.dry == Dry::No {
                let want = (c1 - s) as usize;
                let guard = (guard_span as f64 * r_hi).ceil() as usize + half + 1;
                let need = (slot.lane.conv.phase() + want as f64 * r_hi).ceil() as usize + guard;
                let ahead = slot.lane.len - slot.lane.at;
                let run = if slot.lane.end.is_some() {
                    Run {
                        frames: ahead,
                        ended: true,
                        brk: false,
                    }
                } else if slot.lane.brk {
                    Run {
                        frames: ahead,
                        ended: false,
                        brk: true,
                    }
                } else if slot.lane.join.is_some() {
                    match fslot.as_deref() {
                        Some(f) => f.held.run(ahead, slot.lane.ch, need),
                        None => Run {
                            frames: ahead,
                            ended: true,
                            brk: false,
                        },
                    }
                } else {
                    slot.held.run(ahead, slot.lane.ch, need)
                };
                if slot.flush || (!run.ended && run.frames < need) {
                    let soon = slot.env.silent_from().is_some_and(|f| f <= s + f30);
                    if !soon {
                        let reach = half + 1;
                        let most = ((run.frames.saturating_sub(reach) as f64
                            - slot.lane.conv.phase())
                            / r_hi)
                            .floor()
                            .max(1.0) as u64;
                        let len = f30.min(most);
                        slot.env.fade(s, len, 0.0, Curve::Raised);
                        out.note(RenderNote::Faded {
                            voice,
                            from: s,
                            to: s + len,
                            kind: FadeKind::Dry,
                        });
                        if slot.flush || run.brk {
                            slot.dry = Dry::Break;
                        } else {
                            slot.dry = Dry::Starve;
                            status.starved.fetch_add(1, Ordering::Relaxed);
                            out.note(RenderNote::Starved { voice, at: s });
                        }
                    }
                    slot.flush = false;
                }
            }

            // 2. How far this piece goes: the chunk's end, or where the voice falls silent; the
            //    window filled for it (the follower continuing it, gapless).
            let mut e = c1;
            if let Some(f) = slot.env.silent_from() {
                e = e.min(f.max(s));
            }
            slot.lane.compact(half);
            let nf = (e - s) as usize;
            let reach = slot.lane.conv.reach(k, nf);
            let upto = slot.lane.at + reach;
            if slot.lane.join.is_some() {
                if let Some(f) = fslot.as_deref_mut() {
                    slot.lane.fill(&mut f.held, f.tap.as_ref(), upto);
                }
            } else {
                slot.lane.fill(&mut slot.held, slot.tap.as_ref(), upto);
                if let Some(f) = fslot.as_deref_mut()
                    && slot.lane.end == Some(slot.lane.len)
                    && slot.lane.read_hi <= slot.lane.len
                    && f.rate == slot.rate
                    && f.held.get(0).is_some_and(|h| {
                        h.content() == f.start.content && usize::from(h.b.channels) == slot.lane.ch
                    })
                {
                    let j_at = slot.lane.len;
                    slot.lane.end = None;
                    slot.lane.join = Some((j_at, f.start.content as i64 - j_at as i64));
                    if let Some(h) = f.held.front_mut() {
                        h.brk = false;
                    }
                    slot.lane.fill(&mut f.held, f.tap.as_ref(), upto);
                }
            }
            if slot.lane.len < upto {
                let ch = slot.lane.ch;
                slot.lane.win[slot.lane.len * ch..upto * ch].fill(0);
            }
            let cross = slot.lane.end.or(slot.lane.join.map(|j| j.0));
            let until = cross.map(|x| frames_until(&slot.lane.conv, slot.lane.at, x).min(nf));
            // The piece stops at the source's end (nothing of the voice plays past it), but not at
            // a join: that is one signal, and cutting a resampled call in two would move its
            // rounding. The join's frame is only noted.
            let m = match (slot.lane.end, until) {
                (Some(_), Some(u)) => u,
                _ => nf,
            };
            let cross_frame = s + until.unwrap_or(m) as u64;

            // 3. Resample (frames at the device's rate on a whole frame pass as they are), the
            //    voice's sound tools, its envelope, into the mix.
            let lch = slot.lane.ch;
            if slot.mark.is_none() && m > 0 {
                slot.mark = Some((s, slot.lane.content()));
            }
            let at0 = slot.lane.at;
            let read = at0 + slot.lane.conv.reach(k, m);
            let v = &mut vbuf[..m * lch];
            let used = slot
                .lane
                .conv
                .process(k, &slot.lane.win, lch, at0, m, trim, v);
            slot.lane.at += used;
            slot.lane.read_hi = slot.lane.read_hi.max(read);
            if let Some(c) = slot.chain.as_mut() {
                c.process(v, lch as u16);
            }
            let off = (s - c0) as usize * dch;
            mix_in(&mut slot.env, v, lch, &mut mix[off..], dch, s, m);
            slot.done = s + m as u64;
            slot.last = (slot.done, slot.lane.content());
            let exact = slot.lane.conv.idle() && slot.chain.as_ref().is_none_or(|c| !c.active);
            if exact != slot.exact {
                out.note(RenderNote::Resampling {
                    voice,
                    at: if exact { slot.done } else { s },
                    on: !exact,
                });
                slot.exact = exact;
            }

            // 4. The source's end, or the join: the follower takes over the window.
            if let Some(x) = cross
                && slot.lane.at >= x
            {
                let at = cross_frame;
                out.note(RenderNote::Ended { voice, at });
                slot.state = State::Silent;
                if let (Some((j_at, base)), Some(f)) = (slot.lane.join, fslot.as_deref_mut()) {
                    slot.last = (at, (slot.lane.base + j_at as i64) as f64);
                    std::mem::swap(&mut slot.lane, &mut f.lane);
                    f.lane.base = base;
                    f.lane.join = None;
                    f.env = slot.env;
                    f.exact = slot.exact;
                    f.done = slot.done;
                    f.state = State::Sounding;
                    f.dry = Dry::No;
                    f.flush = false;
                    f.mark = Some((at, f.start.content as f64));
                    f.last = (f.done, f.lane.content());
                    slot.then = None;
                    out.note(RenderNote::Joined {
                        from: voice,
                        to: f.voice,
                        at,
                    });
                    out.note(RenderNote::Started {
                        voice: f.voice,
                        at,
                        content: f.start.content,
                        fade: 0,
                    });
                    if let Some(j) = fol {
                        i = j;
                    }
                    continue;
                }
                let next = slot.then.take();
                if slot.drop_when_silent {
                    slot.state = State::Releasing;
                }
                let follows = next.filter(|_| fol == next);
                if let (Some(j), Some(f)) = (follows, fslot) {
                    // Another rate or channels: it starts on the frame after.
                    f.state = State::Waiting;
                    f.start.at = at;
                    f.done = at;
                    f.joined_from = Some(voice);
                    return Some(j);
                }
                return None;
            }

            // 5. Silent: stopped, dropped, or faded out by the guard.
            if let Some(f) = slot.env.silent_from()
                && f <= slot.done
            {
                out.note(RenderNote::Silent { voice, at: f });
                match slot.dry {
                    Dry::Starve => slot.state = State::Starved,
                    Dry::Break => {
                        // Everything before the break goes; it comes back after it.
                        while slot.held.get(0).is_some_and(|h| !h.brk) {
                            if let Some(h) = slot.held.pop() {
                                give(slot.tap.as_ref(), h.b);
                            }
                        }
                        slot.relane = true;
                        slot.state = State::Starved;
                    }
                    Dry::No => {
                        slot.state = if slot.drop_when_silent {
                            State::Releasing
                        } else {
                            State::Silent
                        };
                    }
                }
                slot.dry = Dry::No;
                let gone = matches!(slot.state, State::Silent | State::Releasing);
                if gone {
                    self.orphan(i);
                }
                return None;
            }
            if slot.done >= c1 {
                return None;
            }
            // (A piece that stopped short of the join or the end, by a frame: on.)
        }
    }

    /// One chunk of `n` frames from device frame `c0`, packed into `out`.
    fn chunk(&mut self, c0: u64, n: usize, out: &mut [u8]) {
        let c1 = c0 + n as u64;
        self.mix[..n * self.dch].fill(0.0);
        for i in 0..SLOTS {
            self.pull(i, c0);
        }
        for i in 0..SLOTS {
            let mut next = self.voice(i, c0, c1);
            while let Some(j) = next {
                self.pull(j, c0);
                next = self.voice(j, c0, c1);
            }
        }
        // The master gain, then the device's packing.
        let dch = self.dch;
        let steady = self.gain.steady_over(c0, c1, self.gain.seg.to);
        let g_steady = self.gain.seg.to;
        for k in 0..n {
            let g = if steady {
                g_steady
            } else {
                self.gain.level(c0 + k as u64)
            };
            let m = &self.mix[k * dch..(k + 1) * dch];
            let o = &mut self.obuf[k * dch..(k + 1) * dch];
            for (o, &x) in o.iter_mut().zip(m) {
                let y = if g == 1.0 { x } else { x * g };
                *o = y.round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
            }
        }
        encode_into(&self.obuf[..n * dch], &self.fmt, out);
    }

    /// Dropped voices: their blocks and taps go back.
    fn release(&mut self) {
        for i in 0..SLOTS {
            if self.slots[i].state != State::Releasing || self.back.room() < 2 {
                continue;
            }
            self.orphan(i);
            let s = &mut self.slots[i];
            s.held.clear(s.tap.as_ref());
            if let Some(t) = s.tap.take() {
                self.back.push(Returned::Tap(t));
            }
            if let Some(c) = s.chain.take() {
                self.back.push(Returned::Chain(c));
            }
            s.state = State::Free;
            s.mark = None;
        }
    }

    fn markers(&mut self) {
        for s in self.slots.iter_mut() {
            let Some((frame, content)) = s.mark.take() else {
                continue;
            };
            let (lf, lc) = s.last;
            let ratio = if lf > frame {
                (lc - content) / (lf - frame) as f64
            } else {
                s.lane.conv.ratio()
            };
            self.out.marker(Marker {
                frame,
                voice: s.voice,
                content,
                ratio,
            });
        }
    }

    fn publish(&self, frames: u64) {
        let st = &self.status;
        let seq = st.seq.load(Ordering::Relaxed);
        st.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        let mut playing = false;
        let mut fading = false;
        let mut starved = false;
        let mut ppm = None;
        for (s, cell) in self.slots.iter().zip(&st.cells) {
            let state = match s.state {
                State::Free => 0,
                State::Waiting | State::Following(_) => VoiceState::Waiting as u8 + 1,
                State::Sounding if s.env.moving_at(frames) => {
                    fading = true;
                    VoiceState::Fading as u8 + 1
                }
                State::Sounding => {
                    playing = true;
                    VoiceState::Sounding as u8 + 1
                }
                State::Starved => {
                    starved = true;
                    VoiceState::Starved as u8 + 1
                }
                State::Silent | State::Releasing => VoiceState::Silent as u8 + 1,
            };
            if s.state == State::Sounding && ppm.is_none() {
                ppm = Some(s.lane.conv.ppm());
            }
            cell.state.store(state, Ordering::Relaxed);
            cell.voice.store(u64::from(s.voice.0), Ordering::Relaxed);
            cell.content
                .store(s.lane.content().to_bits(), Ordering::Relaxed);
            cell.ratio
                .store(s.lane.conv.ratio().to_bits(), Ordering::Relaxed);
        }
        st.frames.store(frames, Ordering::Relaxed);
        st.ppm
            .store(ppm.unwrap_or(self.trim).to_bits(), Ordering::Relaxed);
        let overall = if playing {
            RenderState::Playing
        } else if fading {
            RenderState::Fading
        } else if starved {
            RenderState::Starved
        } else {
            RenderState::Silent
        };
        st.state.store(overall as u8, Ordering::Relaxed);
        st.seq.store(seq.wrapping_add(2), Ordering::Release);
    }
}

impl Render for Renderer {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        let t0 = self.clock.now_ns();
        self.trim = self.target.ppm().clamp(-MAX_PPM, MAX_PPM);
        self.back.flush();
        self.commands(at);
        let total = (frames as usize).min(out.len() / self.bpf.max(1));
        let mut k = 0;
        while k < total {
            let n = (total - k).min(CHUNK);
            let bytes = &mut out[k * self.bpf..(k + n) * self.bpf];
            self.chunk(at + k as u64, n, bytes);
            k += n;
        }
        self.release();
        self.markers();
        self.publish(at + total as u64);
        // The call's margin: what was left of the period.
        let used = self.clock.now_ns().saturating_sub(t0) as f64;
        let period = f64::from(frames.max(1)) * 1e9 / f64::from(self.rate);
        let margin = 1.0 - used / period;
        if margin > 0.0 {
            let b = ((margin * 100.0).floor() as usize).min(99);
            self.status.buckets[b].fetch_add(1, Ordering::Relaxed);
        } else {
            self.status.late_calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// --- Status --------------------------------------------------------------------------------------

/// What the renderer is doing, by atomics: written once per call, read by anyone (the status
/// thread, 10 times a second). The voices are under a sequence lock (a reader retries the rare
/// read that overlaps a write); the counters only grow.
pub struct RenderStatus {
    seq: AtomicU64,
    cells: [Cell; SLOTS],
    frames: AtomicU64,
    ppm: AtomicU64,
    state: AtomicU8,
    starved: AtomicU64,
    late: AtomicU64,
    refused: AtomicU64,
    lost: AtomicU64,
    buckets: [AtomicU64; 100],
    late_calls: AtomicU64,
}

#[derive(Default)]
struct Cell {
    voice: AtomicU64,
    /// 0: free; else `VoiceState as u8 + 1`.
    state: AtomicU8,
    content: AtomicU64,
    ratio: AtomicU64,
}

impl Default for RenderStatus {
    fn default() -> Self {
        RenderStatus {
            seq: AtomicU64::new(0),
            cells: std::array::from_fn(|_| Cell::default()),
            frames: AtomicU64::new(0),
            ppm: AtomicU64::new(0),
            state: AtomicU8::new(RenderState::Silent as u8),
            starved: AtomicU64::new(0),
            late: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            lost: AtomicU64::new(0),
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            late_calls: AtomicU64::new(0),
        }
    }
}

/// The renderer as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RenderState {
    Silent,
    Playing,
    /// Something is fading (and nothing plays steadily).
    Fading,
    /// A voice ran dry (and nothing plays).
    Starved,
}

/// One voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VoiceState {
    /// Given, not sounding yet.
    Waiting,
    Sounding,
    Fading,
    Starved,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceStatus {
    pub voice: VoiceId,
    pub state: VoiceState,
    /// Its content place (source frames) at the end of the last call.
    pub content: f64,
    /// Source frames per device frame.
    pub ratio: f64,
}

#[derive(Debug, Clone)]
pub struct RenderSnapshot {
    pub voices: [Option<VoiceStatus>; SLOTS],
    /// Device frames rendered (the next call's `at`).
    pub frames: u64,
    /// The trim the sounding voice plays at, ppm.
    pub ppm: f64,
    pub state: RenderState,
    /// Times a voice ran dry.
    pub starved: u64,
    /// Commands done after their frame.
    pub late: u64,
    /// Commands not followed.
    pub refused: u64,
    /// Notes and markers lost to full queues.
    pub lost: u64,
    pub deadline: Deadline,
}

impl RenderStatus {
    pub fn read(&self) -> RenderSnapshot {
        let voices = loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let v: [Option<VoiceStatus>; SLOTS] = std::array::from_fn(|k| {
                let c = &self.cells[k];
                let state = match c.state.load(Ordering::Relaxed) {
                    1 => VoiceState::Waiting,
                    2 => VoiceState::Sounding,
                    3 => VoiceState::Fading,
                    4 => VoiceState::Starved,
                    5 => VoiceState::Silent,
                    _ => return None,
                };
                Some(VoiceStatus {
                    voice: VoiceId(c.voice.load(Ordering::Relaxed) as u32),
                    state,
                    content: f64::from_bits(c.content.load(Ordering::Relaxed)),
                    ratio: f64::from_bits(c.ratio.load(Ordering::Relaxed)),
                })
            });
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                break v;
            }
        };
        let state = match self.state.load(Ordering::Relaxed) {
            1 => RenderState::Playing,
            2 => RenderState::Fading,
            3 => RenderState::Starved,
            _ => RenderState::Silent,
        };
        RenderSnapshot {
            voices,
            frames: self.frames.load(Ordering::Relaxed),
            ppm: f64::from_bits(self.ppm.load(Ordering::Relaxed)),
            state,
            starved: self.starved.load(Ordering::Relaxed),
            late: self.late.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            lost: self.lost.load(Ordering::Relaxed),
            deadline: Deadline {
                buckets: std::array::from_fn(|b| self.buckets[b].load(Ordering::Relaxed)),
                late: self.late_calls.load(Ordering::Relaxed),
            },
        }
    }
}

/// The deadline margin histogram (sections 4.7 and 7.5): calls by what was left of their period,
/// in buckets of 1% (bucket 0: 0 to 1%), and calls with nothing left (late). Counts only grow; a
/// stretch is the difference of two readings (`since`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    pub buckets: [u64; 100],
    pub late: u64,
}

impl Default for Deadline {
    fn default() -> Self {
        Deadline {
            buckets: [0; 100],
            late: 0,
        }
    }
}

impl Deadline {
    pub fn periods(&self) -> u64 {
        self.late + self.buckets.iter().sum::<u64>()
    }

    /// What happened since `earlier` (a reading of the same renderer).
    pub fn since(&self, earlier: &Deadline) -> Deadline {
        Deadline {
            buckets: std::array::from_fn(|b| self.buckets[b].saturating_sub(earlier.buckets[b])),
            late: self.late.saturating_sub(earlier.late),
        }
    }

    /// The playback log's line for a stretch of `seconds` in which the output ran dry `ran_dry`
    /// times (its `Xrun` events): exactly the format of WP14's `Histogram::to_log_line`
    /// (`crates/player/examples/live_kit/deadline.rs`), which `deadline_probe log` reads.
    pub fn to_log_line(&self, seconds: u64, ran_dry: u64) -> String {
        let cells: Vec<String> = self.buckets.iter().map(u64::to_string).collect();
        format!(
            "render: deadline margin over {} s: {} periods, ran dry {}, late {}; per 1% of the period from 0%: {}",
            seconds,
            self.periods(),
            ran_dry,
            self.late,
            cells.join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_that_begins_during_another_starts_from_its_level() {
        let mut e = Envelope::rise(0, 100, Curve::Raised);
        assert_eq!(e.level(0), 0.0);
        let mid = Seg {
            from: 0.0,
            to: 1.0,
            start: 0,
            len: 100,
            curve: Curve::Raised,
        }
        .at(50);
        e.fade(50, 100, 0.0, Curve::Raised);
        assert_eq!(e.level(50), mid, "no step where the fade-out begins");
        assert_eq!(e.silent_from(), Some(150));
        let mut last = mid;
        for f in 51..=150 {
            let g = e.level(f);
            assert!(g <= last, "only down");
            last = g;
        }
        assert_eq!(e.level(150), 0.0);
    }

    #[test]
    fn equal_power_sides_keep_the_power() {
        for k in 0..=100 {
            let t = f64::from(k) / 100.0;
            let a = 1.0 - shape(Curve::PowerOut, t);
            let b = shape(Curve::PowerIn, t);
            assert!((a * a + b * b - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn the_encoder_is_the_outputs_bit_for_bit() {
        let samples: Vec<i32> = (0..4_096i64)
            .map(|i| ((i * 2_654_435_761) % (1 << 32) - (1 << 31)) as i32)
            .collect();
        for (container_bits, valid_bits, kind) in [
            (16, 16, SampleKind::Int),
            (24, 24, SampleKind::Int),
            (32, 20, SampleKind::Int),
            (32, 24, SampleKind::Int),
            (32, 32, SampleKind::Int),
            (32, 32, SampleKind::Float),
        ] {
            let fmt = DeviceFormat {
                rate: 48_000,
                channels: 2,
                container_bits,
                valid_bits,
                kind,
            };
            let mut want = Vec::new();
            crate::output::encode(&samples, &fmt, 1.0, &mut want);
            let mut got = vec![0u8; want.len()];
            encode_into(&samples, &fmt, &mut got);
            assert!(got == want, "{container_bits}/{valid_bits} {kind:?}");
        }
    }
}
