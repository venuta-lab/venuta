//! Voices: one song from one place, decoded ahead on a thread of its own (design 4.6; how it is
//! built is in the WP2 notes, 9.2).
//!
//! A [`Voice`] starts its decode thread at once and returns; the thread opens the song (a file,
//! or a stream on its shared download), seeks to the start place with the codec's pre-roll,
//! makes the voice's ring of blocks (contract 5.3) and keeps it full. The control thread takes
//! the ring's renderer side ([`Voice::take_tap`]) for a `StartAt` or `SwitchAt`, and reads how
//! the voice is doing ([`Voice::status`]): all through atomics and fields set once, never a
//! lock the render thread could meet.
//!
//! A voice plays forward from one place and never seeks: a seek, a realign or a new prepare is
//! a new voice (the spare slot) on the same download. So every block it hands over has epoch 0.
//!
//! A stream that falls behind makes the decode thread wait inside the stream's reader, wherever
//! the read falls; before it waits, everything decoded is handed over (a partly filled block
//! too) and the phase says so ([`Phase::WaitingForBytes`]). No packet is ever cut.
//!
//! The thread's waits go through its [`Pace`]: real time, or a `Timebase`'s sleeper, so a voice
//! runs inside a simulation's lockstep (decoding takes no virtual time).

use crate::decode::{
    ByteWait, DecodeState, FileInfo, Source, SourceFormat, StreamBytes, StreamWatch, Waiter,
};
use crate::voice_api::{
    BLOCK_FRAMES, Block, Dropped, END_OF_SOURCE, FIRST_AFTER_SEEK, MAX_CH, VoiceFeed, VoiceId,
    VoiceTap, voice_ring,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use together::time::Timebase;

/// Opens the voice's song, on its decode thread.
pub type Opener = Box<dyn FnOnce() -> Result<Source, String> + Send>;

/// Decoded ahead for a file, unless the spec says.
pub const FILE_DEPTH: Duration = Duration::from_secs(2);
/// Decoded ahead for a stream, unless the spec says.
pub const STREAM_DEPTH: Duration = Duration::from_secs(3);
/// How long the decode thread sleeps when its ring is full (or it waits for the renderer).
const POLL_MS: u64 = 10;

/// How the decode thread waits.
#[derive(Clone)]
pub enum Pace {
    /// Real time: sleeps, and a condition variable for a stream's bytes.
    Real,
    /// On this clock's sleeper (made before the thread starts), bytes polled each 1 ms: for a
    /// simulation's lockstep.
    Clock(Arc<dyn Timebase>),
}

/// What a voice plays and how.
pub struct VoiceSpec {
    pub id: VoiceId,
    pub open: Opener,
    /// The content frame to start at (0: the song's start).
    pub start: u64,
    /// Decoded ahead (`None`: 2 s for a file, 3 s for a stream).
    pub depth: Option<Duration>,
    pub pace: Pace,
}

/// Where a voice is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    /// Opening the song (for a stream: until enough of it is in), seeking to the start.
    Opening = 0,
    /// Decoding ahead.
    Decoding = 1,
    /// The ring is full: waiting for the renderer to give blocks back.
    Full = 2,
    /// A stream fell behind: everything decoded is handed over, and the thread waits for bytes.
    WaitingForBytes = 3,
    /// The song's last frame is handed over.
    Ended = 4,
    /// It could not go on (`VoiceStatus::error` says why); what was decoded is handed over and
    /// the last block says `END_OF_SOURCE`.
    Failed = 5,
    /// Told to stop.
    Stopped = 6,
}

impl Phase {
    fn from_u8(v: u8) -> Phase {
        match v {
            0 => Phase::Opening,
            1 => Phase::Decoding,
            2 => Phase::Full,
            3 => Phase::WaitingForBytes,
            4 => Phase::Ended,
            5 => Phase::Failed,
            _ => Phase::Stopped,
        }
    }
}

/// How a voice is doing.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceStatus {
    pub phase: Phase,
    /// Known once the song is open.
    pub format: Option<SourceFormat>,
    /// The song's length in frames, when known.
    pub frames: Option<u64>,
    /// The song's length in milliseconds: its frames, or (a stream that doesn't say) the
    /// catalog's length, when known.
    pub duration_ms: Option<u64>,
    /// About the file (size, bitrate, decoder), once open: for the signal path.
    pub info: FileInfo,
    /// An online song's loudness as the catalog says it, once open: its levelling when its tags
    /// give none (`Gain::or_loudness`).
    pub loudness_db: Option<f64>,
    /// The content frame it starts at.
    pub start: u64,
    /// The content frame after the last frame handed over.
    pub decoded_to: u64,
    /// Frames handed over and not yet taken back to fill again: while the voice isn't sounding,
    /// what it has decoded ahead (once it sounds, the renderer's markers say where it is).
    pub in_ring: u64,
    /// Times it waited for a stream's bytes.
    pub stalls: u64,
    pub error: Option<String>,
    /// For a stream: its bytes.
    pub stream: Option<StreamBytes>,
    /// For a stream: where its reader is in the bytes.
    pub bytes_read: Option<u64>,
    /// Its tap is out (taken by the control thread).
    pub tap_taken: bool,
    /// The decode thread has finished.
    pub exited: bool,
}

/// What the decode thread and the control thread share.
struct Shared {
    stop: Arc<AtomicBool>,
    phase: AtomicU8,
    /// The phase before a wait for bytes (put back when they come).
    before_wait: AtomicU8,
    decoded_to: AtomicU64,
    in_ring: AtomicU64,
    stalls: AtomicU64,
    exited: AtomicBool,
    start: u64,
    meta: Mutex<Meta>,
    /// The loudness of the blocks handed over, oldest first (the last `LOUDNESS_KEPT`), for the
    /// room's quiet-moment search. Shared with the control thread only.
    loud: Mutex<VecDeque<Loudness>>,
}

/// Blocks whose loudness a voice keeps: about 45 s at 48 kHz.
const LOUDNESS_KEPT: usize = 2_100;

/// One block's loudness, as the decode thread handed it over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loudness {
    /// Its first content frame and how many it has.
    pub first_frame: u64,
    pub frames: u16,
    /// dBFS (`i8::MIN` for digital silence).
    pub rms_db: i8,
}

#[derive(Default)]
struct Meta {
    format: Option<SourceFormat>,
    frames: Option<u64>,
    duration_ms: Option<u64>,
    info: FileInfo,
    loudness_db: Option<f64>,
    error: Option<String>,
    stream: Option<StreamWatch>,
    tap: Option<VoiceTap>,
    tap_taken: bool,
}

impl Shared {
    fn meta(&self) -> MutexGuard<'_, Meta> {
        self.meta.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_phase(&self, p: Phase) {
        self.phase.store(p as u8, Ordering::Release);
    }

    fn phase(&self) -> Phase {
        Phase::from_u8(self.phase.load(Ordering::Acquire))
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

/// The decode thread's side of the ring, and the block it is filling.
struct Filler {
    feed: Option<VoiceFeed>,
    /// The block being filled.
    cur: Option<Box<Block>>,
    channels: u8,
    /// The next block is the voice's first.
    first: bool,
    /// The voice starts partway (its first block says `FIRST_AFTER_SEEK`).
    partway: bool,
    voice: VoiceId,
}

/// A block came back or could not be had.
enum Take {
    Got,
    Wait,
    Dropped,
}

impl Filler {
    /// Hand the block being filled over, if it has frames.
    fn flush(&mut self, shared: &Shared) {
        if self.cur.as_ref().is_some_and(|b| b.frames > 0) {
            let b = self.cur.take().expect("a block");
            self.push(b, shared);
        }
    }

    fn push(&mut self, mut b: Box<Block>, shared: &Shared) {
        b.rms_db = rms_db(b.data());
        let loud = Loudness {
            first_frame: b.first_frame,
            frames: b.frames,
            rms_db: b.rms_db,
        };
        let (end, frames) = (b.first_frame + u64::from(b.frames), u64::from(b.frames));
        if let Some(feed) = self.feed.as_ref()
            && feed.push(b).is_ok()
        {
            shared.in_ring.fetch_add(frames, Ordering::AcqRel);
            shared.decoded_to.store(end, Ordering::Release);
            let mut l = shared.loud.lock().unwrap_or_else(|p| p.into_inner());
            if l.len() == LOUDNESS_KEPT {
                l.pop_front();
            }
            l.push_back(loud);
        }
    }

    /// Have an empty block in hand, starting at content frame `at`.
    fn take(&mut self, at: u64, shared: &Shared) -> Take {
        if let Some(b) = self.cur.as_mut() {
            if b.frames == 0 {
                b.first_frame = at;
            }
            return Take::Got;
        }
        let Some(feed) = self.feed.as_ref() else {
            return Take::Dropped;
        };
        match feed.take_empty() {
            Ok(Some(mut b)) => {
                // What it carried when handed over is out of the ring now.
                shared
                    .in_ring
                    .fetch_sub(u64::from(b.frames), Ordering::AcqRel);
                b.voice = self.voice;
                b.epoch = 0;
                b.first_frame = at;
                b.frames = 0;
                b.channels = self.channels;
                b.flags = if self.first && self.partway {
                    FIRST_AFTER_SEEK
                } else {
                    0
                };
                self.first = false;
                self.cur = Some(b);
                Take::Got
            }
            Ok(None) => Take::Wait,
            Err(Dropped) => Take::Dropped,
        }
    }

    /// After the decoding: take back blocks the renderer gives back (so the ring's count stays
    /// true); `Err` once the renderer's side is gone.
    fn reclaim(&mut self, shared: &Shared) -> Result<(), Dropped> {
        let feed = self.feed.as_ref().ok_or(Dropped)?;
        while let Some(b) = feed.take_empty()? {
            shared
                .in_ring
                .fetch_sub(u64::from(b.frames), Ordering::AcqRel);
            let mut b = b;
            b.frames = 0;
            // Kept here (and freed by this thread), never handed over again.
            if self.cur.is_none() {
                b.flags = 0;
                self.cur = Some(b);
            }
        }
        Ok(())
    }
}

/// A block's loudness in dBFS (`i8::MIN` for digital silence).
fn rms_db(samples: &[i32]) -> i8 {
    if samples.is_empty() {
        return i8::MIN;
    }
    let sum: f64 = samples
        .iter()
        .map(|&s| {
            let x = f64::from(s) / 2_147_483_648.0;
            x * x
        })
        .sum();
    if sum == 0.0 {
        return i8::MIN;
    }
    let db = 10.0 * (sum / samples.len() as f64).log10();
    db.round().clamp(-127.0, 0.0) as i8
}

/// One voice: its decode thread, and what the control thread sees of it.
pub struct Voice {
    id: VoiceId,
    shared: Arc<Shared>,
}

impl Voice {
    /// Start the voice's decode thread; returns at once.
    pub fn start(spec: VoiceSpec) -> Voice {
        let VoiceSpec {
            id,
            open,
            start,
            depth,
            pace,
        } = spec;
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared {
            stop: Arc::clone(&stop),
            phase: AtomicU8::new(Phase::Opening as u8),
            before_wait: AtomicU8::new(Phase::Opening as u8),
            decoded_to: AtomicU64::new(start),
            in_ring: AtomicU64::new(0),
            stalls: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            start,
            meta: Mutex::new(Meta::default()),
            loud: Mutex::new(VecDeque::with_capacity(LOUDNESS_KEPT)),
        });
        let filler = Arc::new(Mutex::new(Filler {
            feed: None,
            cur: None,
            channels: 0,
            first: true,
            partway: start > 0,
            voice: id,
        }));
        // Before a wait for bytes: hand over what is decoded, and say so.
        let hook = {
            let (shared, filler) = (Arc::clone(&shared), Arc::clone(&filler));
            move |waiting: bool| {
                if waiting {
                    lock(&filler).flush(&shared);
                    shared
                        .before_wait
                        .store(shared.phase.load(Ordering::Acquire), Ordering::Release);
                    shared.stalls.fetch_add(1, Ordering::AcqRel);
                    shared.set_phase(Phase::WaitingForBytes);
                } else if shared.phase() == Phase::WaitingForBytes {
                    shared.phase.store(
                        shared.before_wait.load(Ordering::Acquire),
                        Ordering::Release,
                    );
                }
            }
        };
        // The sleeper is made here, before the thread starts (a simulation counts the thread
        // from its first instant).
        let sleeper = match &pace {
            Pace::Real => None,
            Pace::Clock(t) => Some(t.sleeper()),
        };
        let waiter = Waiter::new(sleeper, stop, hook);
        let thread_shared = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name(format!("venuta-voice-{}", id.0))
            .spawn(move || {
                above_normal_priority();
                run(open, depth, &thread_shared, &waiter, &filler);
                thread_shared.set_phase(Phase::Stopped);
                thread_shared.exited.store(true, Ordering::Release);
                // The waiter (and its sleeper) goes with the thread.
                drop(waiter);
            });
        if let Err(e) = spawned {
            shared.meta().error = Some(format!("Could not start the voice ({e})."));
            shared.set_phase(Phase::Failed);
            shared.exited.store(true, Ordering::Release);
        }
        Voice { id, shared }
    }

    pub fn id(&self) -> VoiceId {
        self.id
    }

    /// The renderer's side of the ring, once the voice has opened: given once.
    pub fn take_tap(&self) -> Option<VoiceTap> {
        let mut meta = self.shared.meta();
        let tap = meta.tap.take();
        if tap.is_some() {
            meta.tap_taken = true;
        }
        tap
    }

    /// The loudness of the blocks it handed over that end after content frame `from`, oldest
    /// first (at most the last 45 s or so of them).
    pub fn loudness(&self, from: u64) -> Vec<Loudness> {
        let l = self.shared.loud.lock().unwrap_or_else(|p| p.into_inner());
        l.iter()
            .filter(|b| b.first_frame + u64::from(b.frames) > from)
            .copied()
            .collect()
    }

    pub fn status(&self) -> VoiceStatus {
        let s = &self.shared;
        let meta = s.meta();
        VoiceStatus {
            phase: s.phase(),
            format: meta.format.clone(),
            frames: meta.frames,
            duration_ms: meta.duration_ms,
            info: meta.info.clone(),
            loudness_db: meta.loudness_db,
            start: s.start,
            decoded_to: s.decoded_to.load(Ordering::Acquire),
            in_ring: s.in_ring.load(Ordering::Acquire),
            stalls: s.stalls.load(Ordering::Acquire),
            error: meta.error.clone(),
            stream: meta.stream.as_ref().map(StreamWatch::bytes),
            bytes_read: meta.stream.as_ref().map(StreamWatch::read),
            tap_taken: meta.tap_taken,
            exited: s.exited.load(Ordering::Acquire),
        }
    }
}

impl Drop for Voice {
    /// The thread stops decoding at its next look. A tap still out keeps the ring's other side
    /// alive until it is dropped (so a block given back is never freed on the render thread).
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

fn lock(f: &Mutex<Filler>) -> MutexGuard<'_, Filler> {
    f.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(windows)]
fn above_normal_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
    };
    // SAFETY: the pseudo handle of the calling thread, always valid.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
}

#[cfg(not(windows))]
fn above_normal_priority() {}

/// The decode thread.
fn run(
    open: Opener,
    depth: Option<Duration>,
    shared: &Shared,
    waiter: &Arc<Waiter>,
    filler: &Mutex<Filler>,
) {
    match decode(open, depth, shared, waiter, filler) {
        Ok(()) => {}
        Err(_) if shared.stopped() => {}
        Err(e) => {
            shared.meta().error = Some(e);
            end(shared, waiter, filler, Phase::Failed);
        }
    }
    // Keep the ring's decode side until the renderer's side is gone: a block given back to a
    // ring with no decode side would be freed by whoever gives it back (the render thread).
    // An untaken tap is let go only when the voice is stopped (a short song may be decoded
    // whole before the control thread takes it).
    loop {
        if shared.stopped() {
            shared.meta().tap = None;
        }
        if lock(filler).reclaim(shared).is_err() {
            break;
        }
        waiter.sleep_ms(POLL_MS);
    }
}

/// What `put` says when the voice is stopped or its tap is gone.
const STOP: &str = "stopped";

/// Open, seek, and decode to the end (or until told to stop).
fn decode(
    open: Opener,
    depth: Option<Duration>,
    shared: &Shared,
    waiter: &Arc<Waiter>,
    filler: &Mutex<Filler>,
) -> Result<(), String> {
    let mut source = open()?;
    source.set_byte_wait(ByteWait::Wait(Arc::clone(waiter)));
    source.open_now()?;
    let format = source.format.clone();
    let ch = usize::from(format.channels);
    if ch == 0 || ch > MAX_CH {
        return Err(format!(
            "A song with {ch} channels is more than Venuta can play."
        ));
    }
    let is_stream = source.is_stream();
    {
        let mut meta = shared.meta();
        meta.format = Some(format.clone());
        meta.frames = source.frames;
        meta.duration_ms = source.duration_ms();
        meta.info = source.info.clone();
        meta.loudness_db = source.loudness_db;
        meta.stream = source.stream_watch();
    }
    let start = if shared.start > 0 {
        source.seek_frame(shared.start)?
    } else {
        0
    };
    let depth = depth.unwrap_or(if is_stream { STREAM_DEPTH } else { FILE_DEPTH });
    let blocks = (depth.as_secs_f64() * f64::from(format.rate) / BLOCK_FRAMES as f64)
        .ceil()
        .max(2.0) as usize;
    let voice = lock(filler).voice;
    let (feed, tap) = voice_ring(voice, blocks);
    {
        let mut f = lock(filler);
        f.feed = Some(feed);
        f.channels = ch as u8;
    }
    shared.decoded_to.store(start, Ordering::Release);
    shared.meta().tap = Some(tap);
    shared.set_phase(Phase::Decoding);
    let mut pcm = Vec::new();
    loop {
        if shared.stopped() {
            return Ok(());
        }
        match source.next(&mut pcm)? {
            DecodeState::Samples => {
                let n = (pcm.len() / ch) as u64;
                let first = source.position() - n;
                if put(&pcm, first, ch, shared, waiter, filler).is_err() {
                    return Ok(());
                }
            }
            DecodeState::End => {
                end(shared, waiter, filler, Phase::Ended);
                return Ok(());
            }
            // Only a reader that never waits says this; a voice's reader waits.
            DecodeState::Pending => waiter.sleep_ms(POLL_MS),
        }
    }
}

/// Copy decoded frames (the first at content frame `first`) into blocks, handing each over as
/// it fills; waits (on the pace) for empty blocks. `Err` when stopped or the tap is gone.
fn put(
    pcm: &[i32],
    first: u64,
    ch: usize,
    shared: &Shared,
    waiter: &Waiter,
    filler: &Mutex<Filler>,
) -> Result<(), &'static str> {
    let frames = pcm.len() / ch;
    let mut i = 0;
    while i < frames {
        let mut f = lock(filler);
        let at = first + i as u64;
        // A block in hand goes on only where it ends (a decoder that skipped a bad packet
        // leaves a gap: the block is handed over and a new one starts).
        if f.cur
            .as_ref()
            .is_some_and(|b| b.frames > 0 && b.first_frame + u64::from(b.frames) != at)
        {
            f.flush(shared);
        }
        match f.take(at, shared) {
            Take::Got => {}
            Take::Dropped => return Err(STOP),
            Take::Wait => {
                drop(f);
                if shared.stopped() {
                    return Err(STOP);
                }
                if shared.phase() == Phase::Decoding {
                    shared.set_phase(Phase::Full);
                }
                waiter.sleep_ms(POLL_MS);
                continue;
            }
        }
        if shared.phase() == Phase::Full {
            shared.set_phase(Phase::Decoding);
        }
        let b = f.cur.as_mut().expect("a block in hand");
        let have = usize::from(b.frames);
        let k = (BLOCK_FRAMES - have).min(frames - i);
        b.samples[have * ch..(have + k) * ch].copy_from_slice(&pcm[i * ch..(i + k) * ch]);
        b.frames += k as u16;
        i += k;
        if usize::from(b.frames) == BLOCK_FRAMES {
            let b = f.cur.take().expect("a block in hand");
            f.push(b, shared);
        }
    }
    Ok(())
}

/// Mark the end: the block being filled (or an empty one) says `END_OF_SOURCE`.
fn end(shared: &Shared, waiter: &Waiter, filler: &Mutex<Filler>, phase: Phase) {
    loop {
        let mut f = lock(filler);
        let at = shared.decoded_to.load(Ordering::Acquire);
        match f.take(at, shared) {
            Take::Got => {
                let mut b = f.cur.take().expect("a block");
                b.flags |= END_OF_SOURCE;
                f.push(b, shared);
                shared.set_phase(phase);
                return;
            }
            Take::Dropped => {
                shared.set_phase(phase);
                return;
            }
            Take::Wait => {
                drop(f);
                if shared.stopped() {
                    return;
                }
                waiter.sleep_ms(POLL_MS);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::mock_host::{Serve, host};
    use crate::decode::{Download, OpenInfo, Streams};
    use media_access::EphemeralAudio;
    use std::path::{Path, PathBuf};
    use std::time::Instant;
    use together::time::{MonoClock, SimClock};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    /// Every frame of a straight decode, interleaved.
    fn straight(path: &Path) -> (usize, Vec<i32>) {
        let mut s = Source::open(path).unwrap();
        let ch = usize::from(s.format.channels);
        let (mut all, mut out) = (Vec::new(), Vec::new());
        while let DecodeState::Samples = s.next(&mut out).unwrap() {
            all.extend_from_slice(&out);
        }
        (ch, all)
    }

    fn file(path: PathBuf) -> Opener {
        Box::new(move || Source::open(&path))
    }

    /// What a renderer saw of a voice: its blocks' samples in order, and checks of every block.
    #[derive(Default)]
    struct Heard {
        samples: Vec<i32>,
        /// Where the next block must start.
        next: Option<u64>,
        first_frame: Option<u64>,
        blocks: usize,
        ended: bool,
    }

    impl Heard {
        /// Take every block there is, as a renderer would, and give each back.
        fn take(&mut self, tap: &VoiceTap) {
            while let Some(b) = tap.pop() {
                assert!(!self.ended, "nothing after END_OF_SOURCE");
                assert_eq!(b.epoch, 0, "a voice never seeks");
                assert_eq!(b.voice, tap.voice);
                if let Some(next) = self.next {
                    assert_eq!(b.first_frame, next, "blocks follow on, frame for frame");
                    assert_eq!(b.flags & FIRST_AFTER_SEEK, 0);
                } else {
                    self.first_frame = Some(b.first_frame);
                }
                if b.frames > 0 {
                    assert!(b.rms_db > i8::MIN || b.data().iter().all(|&s| s == 0));
                }
                self.samples.extend_from_slice(b.data());
                self.next = Some(b.first_frame + u64::from(b.frames));
                self.blocks += 1;
                self.ended = b.flags & END_OF_SOURCE != 0;
                tap.give_back(b);
            }
        }
    }

    /// Wait (real time) for the voice's tap.
    fn tap_of(v: &Voice) -> VoiceTap {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(t) = v.take_tap() {
                return t;
            }
            assert!(Instant::now() < until, "no tap: {:?}", v.status());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Play a voice to its end in real time.
    fn hear(v: &Voice) -> Heard {
        let tap = tap_of(v);
        let mut heard = Heard::default();
        let until = Instant::now() + Duration::from_secs(30);
        while !heard.ended {
            heard.take(&tap);
            assert!(Instant::now() < until, "never ended: {:?}", v.status());
            std::thread::sleep(Duration::from_millis(1));
        }
        heard
    }

    fn voice(id: u32, open: Opener, start: u64) -> Voice {
        Voice::start(VoiceSpec {
            id: VoiceId(id),
            open,
            start,
            depth: None,
            pace: Pace::Real,
        })
    }

    fn wait_for(v: &Voice, what: impl Fn(&VoiceStatus) -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while !what(&v.status()) {
            assert!(Instant::now() < until, "{:?}", v.status());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_voice_hands_over_exactly_what_a_straight_decode_gives() {
        for name in [
            "seek-music.ogg",
            "seek-music.mp3",
            "seek-music.opus",
            "seek-music.m4a",
        ] {
            let (ch, all) = straight(&fixture(name));
            for start in [0u64, 77_777] {
                let v = voice(1, file(fixture(name)), start);
                let heard = hear(&v);
                assert_eq!(heard.first_frame, Some(start), "{name}");
                assert!(
                    heard.samples == all[start as usize * ch..],
                    "{name} from {start}: the frames of a straight decode"
                );
                // The thread says so just after it hands the last block over.
                wait_for(&v, |s| s.phase == Phase::Ended);
                let s = v.status();
                assert_eq!(s.decoded_to, (all.len() / ch) as u64);
                assert_eq!(s.error, None);
            }
        }
    }

    #[test]
    fn the_first_block_says_it_starts_partway_and_the_last_says_the_end() {
        let v = voice(2, file(fixture("seek-music.ogg")), 5_000);
        let tap = tap_of(&v);
        let until = Instant::now() + Duration::from_secs(20);
        let first = loop {
            if let Some(b) = tap.pop() {
                break b;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(first.flags, FIRST_AFTER_SEEK);
        assert_eq!(first.first_frame, 5_000);
        assert_eq!(
            usize::from(first.frames),
            BLOCK_FRAMES,
            "blocks are filled whole"
        );
        tap.give_back(first);
        let mut last_flags = 0;
        while last_flags & END_OF_SOURCE == 0 {
            if let Some(b) = tap.pop() {
                last_flags = b.flags;
                tap.give_back(b);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(Instant::now() < until);
        }
        assert_eq!(last_flags, END_OF_SOURCE);
    }

    #[test]
    fn a_voice_keeps_the_loudness_of_every_block_it_handed_over() {
        let v = voice(4, file(fixture("seek-music.ogg")), 5_000);
        let tap = tap_of(&v);
        let mut seen = Vec::new();
        let until = Instant::now() + Duration::from_secs(20);
        while seen.len() < 40 {
            if let Some(b) = tap.pop() {
                seen.push((b.first_frame, b.frames, b.rms_db));
                tap.give_back(b);
            } else {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let kept: Vec<(u64, u16, i8)> = v
            .loudness(0)
            .iter()
            .map(|l| (l.first_frame, l.frames, l.rms_db))
            .collect();
        assert_eq!(kept[..40], seen[..], "block by block, as handed over");
        let later = v.loudness(seen[10].0 + 1);
        assert_eq!(later[0].first_frame, seen[10].0, "from the block holding the frame");
    }

    #[test]
    fn a_voice_decodes_its_depth_ahead_and_waits() {
        let v = voice(3, file(fixture("seek-music.ogg")), 0);
        wait_for(&v, |s| s.phase == Phase::Full);
        let s = v.status();
        // 2 s of 44.1 kHz, in whole blocks.
        let blocks = (2.0f64 * 44_100.0 / BLOCK_FRAMES as f64).ceil() as u64;
        assert_eq!(s.in_ring, blocks * BLOCK_FRAMES as u64);
        assert_eq!(s.decoded_to, s.in_ring, "everything decoded is in the ring");
        // Given back, the ring fills again, and what is in it is counted right.
        let tap = v.take_tap().unwrap();
        for _ in 0..10 {
            let b = tap.pop().unwrap();
            tap.give_back(b);
        }
        wait_for(&v, |t| {
            t.decoded_to == s.decoded_to + 10 * BLOCK_FRAMES as u64
        });
        assert_eq!(v.status().in_ring, s.in_ring);
    }

    #[test]
    fn an_open_voice_says_what_the_signal_path_and_levelling_need() {
        let path = fixture("seek-music.ogg");
        let v = voice(4, file(path.clone()), 0);
        wait_for(&v, |s| s.phase == Phase::Full);
        let s = v.status();
        let own = Source::open(&path).unwrap();
        assert_eq!(s.info, own.info, "the file's size, bitrate and decoder");
        assert_eq!(s.duration_ms, own.duration_ms());
        assert!(s.duration_ms.is_some_and(|d| d > 0));
        assert_eq!(s.loudness_db, None, "a file has no catalog loudness");
    }

    /// The gate: a stream that stops in the middle of a packet loses nothing. Bytes come in
    /// pieces of 997 (so nearly every read ends inside a packet) and stop for three seconds at an
    /// odd byte; the voice runs on a simulated clock. While it waits it says so, with everything
    /// decoded handed over; afterwards every frame equals a straight decode of the whole file.
    #[test]
    fn a_stall_in_the_middle_of_a_packet_loses_nothing() {
        for (name, mime) in [
            ("seek-music.webm", "audio/webm"),
            ("seek-music.mp3", "audio/mpeg"),
            ("seek-music.m4a", "audio/mp4"),
        ] {
            let bytes = std::fs::read(fixture(name)).unwrap();
            let (_, all) = straight(&fixture(name));
            let clock = SimClock::new(1_000_000_000);
            let info = OpenInfo {
                mime_type: Some(mime.into()),
                url_hint: format!("song.{}", name.rsplit('.').next().unwrap()),
                size_hint: Some(bytes.len() as u64),
                ..OpenInfo::default()
            };
            let (download, feeder) = Download::fed_opening_after(info, 16 * 1024);
            let feeder = Arc::new(Mutex::new(feeder));
            // The pieces, 2 ms apart, with a 3 s stop after an odd byte two fifths in.
            let stop_at = bytes.len() * 2 / 5 + 13;
            let (mut t, mut at) = (clock.now_ns(), 0usize);
            let mut stall = None;
            while at < bytes.len() {
                let end = (at + 997).min(bytes.len());
                let piece = if at < stop_at && end > stop_at {
                    stop_at - at
                } else {
                    end - at
                };
                let chunk = bytes[at..at + piece].to_vec();
                let f = Arc::clone(&feeder);
                clock.at(t, Box::new(move || f.lock().unwrap().append(&chunk)));
                at += piece;
                t += 2_000_000;
                if at == stop_at {
                    stall = Some((t, t + 3_000_000_000));
                    t += 3_000_000_000;
                }
            }
            let f = Arc::clone(&feeder);
            clock.at(t, Box::new(move || f.lock().unwrap().finish()));
            let all_in = t;
            let (stall_from, stall_to) = stall.unwrap();

            let d = Arc::clone(&download);
            let v = Voice::start(VoiceSpec {
                id: VoiceId(4),
                open: Box::new(move || Ok(Source::on_download(d, None))),
                start: 0,
                depth: None,
                pace: Pace::Clock(Arc::new(clock.clone())),
            });
            drop(download);
            let mut heard = Heard::default();
            let mut tap = None;
            let mut first_heard = None;
            let mut checked_stall = false;
            while !heard.ended {
                clock.pass(5_000_000);
                assert!(
                    clock.now_ns() < all_in + 10_000_000_000,
                    "{name}: {:?}",
                    v.status()
                );
                if tap.is_none() {
                    tap = v.take_tap();
                }
                let Some(tap) = tap.as_ref() else { continue };
                heard.take(tap);
                if first_heard.is_none() && heard.blocks > 0 {
                    first_heard = Some(clock.now_ns());
                }
                let now = clock.now_ns();
                if now > stall_from + 500_000_000 && now < stall_to {
                    let s = v.status();
                    assert_eq!(s.phase, Phase::WaitingForBytes, "{name}");
                    assert!(s.stalls >= 1);
                    assert_eq!(
                        heard.next,
                        Some(s.decoded_to),
                        "{name}: everything decoded was handed over"
                    );
                    checked_stall = true;
                }
            }
            assert!(checked_stall, "{name}: the stall was seen");
            assert!(
                first_heard.unwrap() < stall_from,
                "{name}: it played as the bytes came"
            );
            assert!(
                heard.samples == all,
                "{name}: every frame as a straight decode of the whole file"
            );
            assert_eq!(v.status().error, None);
            // Let the voice's thread go (it waits on the simulated clock).
            drop((v, tap));
            clock.pass(50_000_000);
        }
    }

    fn audio(url: &str, len: usize) -> EphemeralAudio {
        EphemeralAudio {
            track_id: "t".into(),
            provider: "test".into(),
            url: url.into(),
            mime_type: Some("audio/mpeg".into()),
            content_length: Some(len as u64),
            duration_ms: Some(6_000),
            headers: Vec::new(),
            via: "test",
            loudness_db: None,
        }
    }

    /// The gate: a seek on a stream (a new voice on the same song) doesn't download again. The
    /// host sends the song slowly; a second voice starts partway while the first plays, and a
    /// third after both are gone, kept by a hold. One request, and each voice gives exactly what
    /// a straight decode gives from its place.
    #[test]
    fn a_seek_on_a_stream_does_not_download_again() {
        let name = "seek-music.mp3";
        let bytes = Arc::new(std::fs::read(fixture(name)).unwrap());
        let (ch, all) = straight(&fixture(name));
        let host = host(Arc::clone(&bytes), |_, _| Serve::Paced {
            first: 70 * 1024,
            piece: 4 * 1024,
            every_ms: 20,
        });
        let streams = Arc::new(Streams::new());
        let stream = |id: u32, start: u64| {
            let (streams, url, len) = (Arc::clone(&streams), host.url.clone(), bytes.len());
            voice(
                id,
                Box::new(move || streams.open_remote("online:test:t", audio(&url, len))),
                start,
            )
        };
        let a = stream(5, 0);
        let tap_a = tap_of(&a);
        let mut heard_a = Heard::default();
        let until = Instant::now() + Duration::from_secs(20);
        while heard_a.blocks == 0 {
            heard_a.take(&tap_a);
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            !a.status().stream.unwrap().complete,
            "the second voice starts while the song downloads"
        );
        let b = stream(6, 150_000);
        let tap_b = tap_of(&b);
        let mut heard_b = Heard::default();
        while !(heard_a.ended && heard_b.ended) {
            heard_a.take(&tap_a);
            heard_b.take(&tap_b);
            assert!(Instant::now() < until, "{:?} {:?}", a.status(), b.status());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(heard_a.samples == all);
        assert!(heard_b.samples == all[150_000 * ch..]);
        assert!(b.status().stream.unwrap().complete);

        let hold = streams.hold("online:test:t").expect("the song's download");
        drop((a, b, tap_a, tap_b));
        let c = stream(7, 200_000);
        let heard_c = hear(&c);
        assert!(heard_c.samples == all[200_000 * ch..]);
        assert_eq!(host.asked(), vec![0], "one request for every voice");
        drop((c, hold));
    }

    /// A voice on a simulated clock decodes in no virtual time and waits only through its
    /// sleeper, so it runs in the simulation's lockstep.
    #[test]
    fn a_voice_runs_in_simulated_time() {
        let name = "seek-music.ogg";
        let (ch, all) = straight(&fixture(name));
        let clock = SimClock::new(0);
        let v = Voice::start(VoiceSpec {
            id: VoiceId(8),
            open: file(fixture(name)),
            start: 3_000,
            depth: Some(Duration::from_millis(500)),
            pace: Pace::Clock(Arc::new(clock.clone())),
        });
        clock.pass(1_000_000);
        assert_eq!(
            v.status().phase,
            Phase::Full,
            "decoded at once in virtual time"
        );
        let tap = v.take_tap().unwrap();
        let mut heard = Heard::default();
        while !heard.ended {
            heard.take(&tap);
            clock.pass(10_000_000);
        }
        assert!(heard.samples == all[3_000 * ch..]);
        drop(v);
        drop(tap);
        clock.pass(20_000_000);
    }

    #[test]
    fn a_stopped_voice_lets_go_of_its_ring_only_when_the_renderer_has() {
        // Never handed over: stopping ends the thread.
        let v = voice(9, file(fixture("seek-music.ogg")), 0);
        let shared = Arc::clone(&v.shared);
        wait_for(&v, |s| s.phase == Phase::Full);
        drop(v);
        let until = Instant::now() + Duration::from_secs(20);
        while !shared.exited.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        // Handed over: the thread keeps its side until the tap is gone, so a block given back
        // after the stop goes back to the ring, not to be freed by the renderer.
        let v = voice(10, file(fixture("seek-music.ogg")), 0);
        let tap = tap_of(&v);
        let shared = Arc::clone(&v.shared);
        let b = loop {
            if let Some(b) = tap.pop() {
                break b;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        };
        drop(v);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !shared.exited.load(Ordering::Acquire),
            "kept while the tap is out"
        );
        tap.give_back(b);
        drop(tap);
        while !shared.exited.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_song_that_cannot_open_says_why_and_ends() {
        let v = voice(11, Box::new(|| Err("no such song".into())), 0);
        wait_for(&v, |s| s.exited);
        assert_eq!(v.status().error.as_deref(), Some("no such song"));
        assert!(v.take_tap().is_none());
    }

    /// A stream that fails partway: what came is handed over, the last block says the end, and
    /// the voice says why.
    #[test]
    fn a_stream_that_fails_partway_plays_what_came_then_says_why() {
        let bytes = std::fs::read(fixture("seek-music.mp3")).unwrap();
        let (download, mut feeder) = Download::fed_opening_after(
            OpenInfo {
                mime_type: Some("audio/mpeg".into()),
                url_hint: "song.mp3".into(),
                ..OpenInfo::default()
            },
            16 * 1024,
        );
        feeder.append(&bytes[..bytes.len() / 2]);
        let v = voice(
            12,
            Box::new(move || Ok(Source::on_download(download, None))),
            0,
        );
        let tap = tap_of(&v);
        let mut heard = Heard::default();
        let until = Instant::now() + Duration::from_secs(20);
        while v.status().phase != Phase::WaitingForBytes {
            heard.take(&tap);
            assert!(Instant::now() < until, "{:?}", v.status());
            std::thread::sleep(Duration::from_millis(1));
        }
        feeder.fail("The stream stopped partway and couldn't be picked up again.");
        while !heard.ended {
            heard.take(&tap);
            assert!(Instant::now() < until, "{:?}", v.status());
            std::thread::sleep(Duration::from_millis(1));
        }
        let s = v.status();
        assert_eq!(s.phase, Phase::Failed);
        assert_eq!(
            s.error.as_deref(),
            Some("The stream stopped partway and couldn't be picked up again.")
        );
        assert!(heard.samples.len() > 44_100 * 2, "what came played");
    }

    #[test]
    fn a_block_s_loudness_is_its_rms_in_dbfs() {
        assert_eq!(rms_db(&[]), i8::MIN);
        assert_eq!(rms_db(&[0; 64]), i8::MIN);
        // Full-scale square: 0 dBFS. Half scale: -6 dB.
        assert_eq!(rms_db(&[i32::MAX, i32::MIN, i32::MAX, i32::MIN]), 0);
        assert_eq!(rms_db(&[1 << 30, -(1 << 30)]), -6);
        assert_eq!(rms_db(&[1; 8]), -127);
    }
}
