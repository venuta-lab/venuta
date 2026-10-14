//! The player: one thread that decodes ahead, feeds the device, and moves through the queue.
//!
//! About a second of audio is kept decoded. When a song ends, the next one (already opened
//! and buffered) continues at the very next sample; if its format differs, the device is
//! reopened for it. Status (position, what's playing, the signal path) goes out about ten
//! times a second, and the queue whenever it changes.

use crate::decode::{FileInfo, Source, SourceFormat};
use crate::dsp::{Chain, Gain, Sound};
use crate::output::{self, Device, DeviceFormat, Mode};
use crate::queue::{Queue, Repeat};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Something the queue holds: a local file or an ephemeral resolved media source.
pub trait Item: Clone + Send + 'static {
    fn path(&self) -> Option<&Path> {
        None
    }
    fn open(&self) -> Result<Source, String> {
        self.path()
            .ok_or_else(|| "Queue item has no media source.".to_owned())
            .and_then(Source::open)
    }
    fn label(&self) -> String {
        self.path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Remote media".into())
    }
    /// ReplayGain values from the song's tags, if any.
    fn gain(&self) -> Gain {
        Gain::default()
    }
    /// Which album it's from, so songs of one album are never crossfaded.
    fn album(&self) -> Option<String> {
        None
    }
    /// Whether to reopen the current item after playback ends, for its stopped status.
    fn reopen_when_stopped(&self) -> bool {
        true
    }
}

impl Item for PathBuf {
    fn path(&self) -> Option<&Path> {
        Some(self.as_path())
    }
}

/// Where to send the sound.
#[derive(Clone)]
pub enum OutputChoice {
    /// A sound device (`None`: the Windows default), exclusive or shared.
    Device { id: Option<String>, exclusive: bool },
    /// Memory, for checks.
    Memory(std::sync::Arc<std::sync::Mutex<output::Captured>>),
}

pub enum Cmd<T> {
    /// Replace the queue and start at `index` (`play`: start playing).
    Set {
        items: Vec<T>,
        index: usize,
        start_ms: u64,
        play: bool,
        from: String,
    },
    Play,
    Pause,
    Toggle,
    Seek(u64),
    Next,
    Previous,
    Jump(usize),
    Volume(f32),
    Options {
        shuffle: bool,
        repeat: Repeat,
    },
    Insert {
        items: Vec<T>,
        next: bool,
    },
    Remove(usize),
    Move(usize, usize),
    /// The same source with a new list (a playlist edited while it plays): `index` is the song
    /// playing now, which plays on (see `Queue::follow`).
    Follow {
        items: Vec<T>,
        index: usize,
    },
    /// After a restart: the queue is a shuffled one, and these are its positions in the order
    /// before shuffling.
    RestoreOrder(Vec<usize>),
    Output(OutputChoice),
    /// The sound tools (equalizer, levelling…).
    Sound(Sound),
    /// Stop when the playing song ends, instead of going on.
    StopAfterCurrent(bool),
    /// Fade out and pause after this long (`None`: cancel).
    SleepIn(Option<Duration>),
    /// Measure band levels of what's playing (for a spectrum display), or stop measuring.
    Spectrum(bool),
    /// Send the whole status again, queue included (a new window listening).
    Refresh,
    Quit,
}

/// Where the sound is going and what happens on the way.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Signal {
    pub device: String,
    pub mode: Mode,
    pub source: SourceFormat,
    pub file: FileInfo,
    pub output: DeviceFormat,
    /// Decoded ahead, waiting to play.
    pub buffer_ms: u32,
    /// Held by the device before you hear it.
    pub latency_ms: f32,
    pub bit_perfect: bool,
    /// What changes the samples, in words ("Volume at 60%").
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status<T: Serialize> {
    /// The whole queue: only when it changed since the last status (it can be long).
    pub queue: Option<Vec<T>>,
    pub index: Option<usize>,
    pub playing: bool,
    /// True while the current song is still opening (e.g. a remote body downloading).
    pub buffering: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    pub signal: Option<Signal>,
    /// With `queue`, while shuffled: where each song of the order before shuffling is.
    pub unshuffled: Option<Vec<usize>>,
    pub from: String,
    pub error: String,
    pub sound: Sound,
    pub stop_after_current: bool,
    /// Seconds until the sleep timer pauses playback.
    pub sleep_in_s: Option<u64>,
    /// Band levels of what's playing, 0-255 (see `spectrum`), while asked for and playing.
    pub bands: Option<Vec<u8>>,
}

/// What the last report said, apart from the queue and the band levels (which only come with a
/// change, or while playing): a report that would repeat it word for word is not sent.
#[derive(PartialEq)]
struct Said {
    index: Option<usize>,
    playing: bool,
    buffering: bool,
    position_ms: u64,
    duration_ms: u64,
    shuffle: bool,
    repeat: Repeat,
    volume: f32,
    signal: Option<Signal>,
    from: String,
    error: String,
    sound: Sound,
    stop_after_current: bool,
    sleep_in_s: Option<u64>,
}

impl Said {
    fn of<T: Serialize>(s: &Status<T>) -> Self {
        Self {
            index: s.index,
            playing: s.playing,
            buffering: s.buffering,
            position_ms: s.position_ms,
            duration_ms: s.duration_ms,
            shuffle: s.shuffle,
            repeat: s.repeat,
            volume: s.volume,
            signal: s.signal.clone(),
            from: s.from.clone(),
            error: s.error.clone(),
            sound: s.sound.clone(),
            stop_after_current: s.stop_after_current,
            sleep_in_s: s.sleep_in_s,
        }
    }
}

pub struct Player<T> {
    tx: Sender<Cmd<T>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Item + Serialize> Player<T> {
    /// Start the player thread. `on_status` is called from it.
    pub fn spawn(output: OutputChoice, on_status: impl FnMut(Status<T>) + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("venuta-player".into())
            .spawn(move || Engine::new(output, Box::new(on_status)).run(rx))
            .expect("start the player thread");
        Self {
            tx,
            thread: Some(thread),
        }
    }

    pub fn send(&self, cmd: Cmd<T>) {
        let _ = self.tx.send(cmd);
    }

    /// Whether the player thread is still running (it ends only on Quit, or a panic).
    pub fn is_alive(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl<T> Drop for Player<T> {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// One song's decoded audio, waiting to be played.
struct Segment {
    source: Source,
    samples: VecDeque<i32>,
    /// The file has no more to decode.
    ended: bool,
    /// Frames already sent to the device, counted from `start_frame`.
    played: u64,
    start_frame: u64,
    gain: Gain,
    album: Option<String>,
    /// The sound tools for this song (made when it starts playing).
    chain: Option<Chain>,
}

impl Segment {
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.source.format.channels.max(1))
    }
}

const BUFFER_SECONDS: f32 = 1.0;
/// Let go of the device after this long paused, so other apps can use it.
const RELEASE_AFTER: Duration = Duration::from_secs(10);

struct Engine<T: Item + Serialize> {
    queue: Queue<T>,
    queue_changed: bool,
    playing: bool,
    volume: f32,
    from: String,
    error: String,
    choice: OutputChoice,
    device: Option<Box<dyn Device>>,
    dev_fmt: Option<DeviceFormat>,
    /// The song format the device was opened for.
    opened_for: Option<SourceFormat>,
    started: bool,
    /// The playing song first; the next (decoded ahead) after it.
    segs: VecDeque<Segment>,
    on_status: Box<dyn FnMut(Status<T>) + Send>,
    last_status: Instant,
    /// What the last report said (see `Said`).
    said: Option<Said>,
    paused_at: Option<Instant>,
    sound: Sound,
    stop_after_current: bool,
    sleep_at: Option<Instant>,
    /// Measuring band levels (a spectrum display is showing), and the recent sound for it.
    spectrum: bool,
    recent: std::collections::VecDeque<f32>,
    /// A crossfade is blending two songs right now (for the signal path).
    fading: bool,
    scratch: Vec<i32>,
    mapped: Vec<i32>,
    bytes: Vec<u8>,
}

impl<T: Item + Serialize> Engine<T> {
    fn new(choice: OutputChoice, on_status: Box<dyn FnMut(Status<T>) + Send>) -> Self {
        Self {
            queue: Queue::default(),
            queue_changed: true,
            playing: false,
            volume: 1.0,
            from: String::new(),
            error: String::new(),
            choice,
            device: None,
            dev_fmt: None,
            opened_for: None,
            started: false,
            segs: VecDeque::new(),
            on_status,
            last_status: Instant::now(),
            said: None,
            paused_at: None,
            sound: Sound::default(),
            stop_after_current: false,
            sleep_at: None,
            spectrum: false,
            recent: std::collections::VecDeque::new(),
            fading: false,
            scratch: Vec::new(),
            mapped: Vec::new(),
            bytes: Vec::new(),
        }
    }

    fn run(mut self, rx: Receiver<Cmd<T>>) {
        loop {
            // Commands first; when not playing, wait for one.
            let timeout = if self.playing && self.device.is_some() {
                Duration::ZERO
            } else {
                Duration::from_millis(200)
            };
            loop {
                match rx.recv_timeout(timeout) {
                    Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => {
                        self.release();
                        return;
                    }
                    Ok(cmd) => self.handle(cmd),
                    Err(RecvTimeoutError::Timeout) => break,
                }
                if !self.playing {
                    break;
                }
            }
            if self.playing {
                if let Err(e) = self.step() {
                    self.error = e;
                    self.playing = false;
                    self.release();
                    self.publish(true);
                }
            } else if self.paused_at.is_some_and(|t| t.elapsed() > RELEASE_AFTER)
                && self.device.is_some()
            {
                self.release();
            }
            self.publish(false);
        }
    }

    // --- Commands -------------------------------------------------------------------------

    fn handle(&mut self, cmd: Cmd<T>) {
        match cmd {
            Cmd::Set {
                items,
                index,
                start_ms,
                play,
                from,
            } => {
                let shuffle = self.queue.shuffled();
                self.queue.set(items, index);
                if shuffle {
                    self.queue.set_shuffle(true);
                }
                self.from = from;
                self.queue_changed = true;
                self.error.clear();
                self.load_current(start_ms);
                self.set_playing(play);
            }
            Cmd::Play => self.set_playing(true),
            Cmd::Pause => self.set_playing(false),
            Cmd::Toggle => self.set_playing(!self.playing),
            Cmd::Seek(ms) => self.seek(ms),
            Cmd::Next => {
                if self.queue.skip_next().is_some() {
                    self.load_current(0);
                } else {
                    self.set_playing(false);
                }
            }
            Cmd::Previous => {
                // Into a song: back to its start. At its start: the one before.
                if self.position_ms() > 3000 || self.queue.skip_previous().is_none() {
                    self.seek(0);
                } else {
                    self.load_current(0);
                }
            }
            Cmd::Jump(i) => {
                // Same song again: rewind in place (no re-resolve / re-download).
                if self.queue.index == Some(i) {
                    self.seek(0);
                    self.set_playing(true);
                } else if self.queue.jump(i) {
                    self.load_current(0);
                    self.set_playing(true);
                }
            }
            Cmd::Volume(v) => self.volume = v.clamp(0.0, 1.0),
            Cmd::Options { shuffle, repeat } => {
                self.queue.repeat = repeat;
                if shuffle != self.queue.shuffled() {
                    self.queue.set_shuffle(shuffle);
                    self.queue_changed = true;
                }
                self.drop_prefetch();
            }
            Cmd::Insert { items, next } => {
                let was_empty = self.queue.items().is_empty();
                self.queue.insert(items, next);
                self.queue_changed = true;
                self.drop_prefetch();
                if was_empty {
                    self.load_current(0);
                    self.set_playing(true);
                }
            }
            Cmd::Remove(i) => {
                let current = self.queue.index == Some(i);
                self.queue.remove(i);
                self.queue_changed = true;
                if current {
                    self.load_current(0);
                } else {
                    self.drop_prefetch();
                }
            }
            Cmd::Move(a, b) => {
                self.queue.move_item(a, b);
                self.queue_changed = true;
                self.drop_prefetch();
            }
            Cmd::Follow { items, index } => {
                self.queue.follow(items, index);
                self.queue_changed = true;
                self.drop_prefetch();
            }
            Cmd::RestoreOrder(positions) => {
                self.queue.restore_order(&positions);
                self.queue_changed = true;
            }
            Cmd::Output(choice) => {
                self.release();
                self.choice = choice;
            }
            Cmd::Sound(s) => {
                self.sound = s;
                // Every song makes its tools again with the new settings.
                for seg in &mut self.segs {
                    seg.chain = None;
                }
            }
            Cmd::StopAfterCurrent(on) => self.stop_after_current = on,
            Cmd::SleepIn(d) => self.sleep_at = d.map(|d| Instant::now() + d),
            Cmd::Spectrum(on) => {
                self.spectrum = on;
                if !on {
                    self.recent = std::collections::VecDeque::new();
                }
            }
            Cmd::Refresh => self.queue_changed = true,
            Cmd::Quit => {}
        }
        self.publish(true);
    }

    fn set_playing(&mut self, on: bool) {
        if on && self.segs.is_empty() && self.queue.current().is_some() {
            self.load_current(0);
        }
        self.playing = on && !self.segs.is_empty();
        if self.playing {
            self.paused_at = None;
            if self.started
                && let Some(d) = self.device.as_mut()
            {
                let _ = d.start();
            }
        } else {
            self.paused_at = Some(Instant::now());
            if let Some(d) = self.device.as_mut() {
                let _ = d.stop();
            }
            self.started = false;
        }
    }

    /// Open the current song (skipping ones that can't be opened), from `start_ms`.
    fn load_current(&mut self, start_ms: u64) {
        self.segs.clear();
        let mut tries = self.queue.items().len();
        while let Some(item) = self.queue.current().cloned() {
            match item.open() {
                Ok(mut source) => {
                    let start_frame = if start_ms > 0 {
                        source.seek(start_ms).unwrap_or(0)
                    } else {
                        0
                    };
                    self.segs.push_back(Segment {
                        source,
                        samples: VecDeque::new(),
                        ended: false,
                        played: 0,
                        start_frame,
                        gain: item.gain(),
                        album: item.album(),
                        chain: None,
                    });
                    return;
                }
                Err(e) => {
                    self.error = format!("{}: {e}", item.label());
                    tries = tries.saturating_sub(1);
                    if tries == 0 || self.queue.skip_next().is_none() {
                        self.playing = false;
                        return;
                    }
                }
            }
        }
    }

    fn seek(&mut self, ms: u64) {
        let Some(front) = self.segs.front_mut() else {
            return;
        };
        match front.source.seek(ms) {
            Ok(frame) => {
                front.samples.clear();
                front.ended = false;
                front.played = 0;
                front.start_frame = frame;
                self.segs.truncate(1);
            }
            Err(e) => self.error = e,
        }
    }

    /// Forget the song decoded ahead (the queue changed, so the next song may be another).
    fn drop_prefetch(&mut self) {
        self.segs.truncate(1);
    }

    /// Open the next queue item into `segs` when we only have the current one. Returns whether
    /// a segment was added. Remote open starts the download without blocking the audio thread.
    fn prefetch_next(&mut self) -> bool {
        if self.segs.len() >= 2 || self.stop_after_current {
            return false;
        }
        let Some(next) = self.queue.peek_next() else {
            return false;
        };
        let Some(item) = self.queue.items().get(next).cloned() else {
            return false;
        };
        match item.open() {
            Ok(source) => {
                self.segs.push_back(Segment {
                    source,
                    samples: VecDeque::new(),
                    ended: false,
                    played: 0,
                    start_frame: 0,
                    gain: item.gain(),
                    album: item.album(),
                    chain: None,
                });
                true
            }
            // It will be reported when its turn comes.
            Err(_) => false,
        }
    }

    // --- Playing --------------------------------------------------------------------------

    fn ensure_device(&mut self, fmt: &SourceFormat) -> Result<(), String> {
        if self.device.is_none() {
            self.device = Some(match &self.choice {
                OutputChoice::Memory(c) => Box::new(output::MemoryDevice {
                    captured: c.clone(),
                    period: 4096,
                }),
                #[cfg(windows)]
                OutputChoice::Device { id, exclusive } => Box::new(
                    crate::wasapi_out::WasapiDevice::new(id.as_deref(), *exclusive)?,
                ),
                #[cfg(not(windows))]
                OutputChoice::Device { .. } => {
                    return Err("Playing to a sound device needs Windows for now.".into());
                }
            });
            self.dev_fmt = None;
        }
        // Reopen only for a song of another format than the one the device was opened for
        // (a lossy song's nominal depth is not what the device runs at).
        let needs_open = self.dev_fmt.is_none() || self.opened_for.as_ref() != Some(fmt);
        if needs_open {
            let dev = self.device.as_mut().expect("just made");
            let _ = dev.stop();
            self.started = false;
            self.dev_fmt = Some(dev.open(fmt)?);
            self.opened_for = Some(fmt.clone());
        }
        Ok(())
    }
    /// Keep an already-open device clocked while the first remote audio chunk is arriving.
    fn wait_for_remote(&mut self) -> Result<(), String> {
        let Some(format) = self.dev_fmt.as_ref() else {
            return Ok(());
        };
        let Some(device) = self.device.as_mut() else {
            return Ok(());
        };
        let want = if self.started {
            device.wait()?
        } else {
            device.available()?
        };
        if want == 0 {
            return Ok(());
        }
        self.bytes.clear();
        self.bytes.resize(want * format.bytes_per_frame(), 0);
        device.write(&self.bytes, want)?;
        if !self.started {
            device.start()?;
            self.started = true;
        }
        Ok(())
    }

    /// Keep the playing song decoded ahead; only then warm the next one.
    ///
    /// Always refill the **front** first. Decoding only the back (a prefetched remote still
    /// Pending) used to starve the current song after ~1s of buffer and freeze playback.
    fn fill(&mut self) -> Result<(), String> {
        let rate = self
            .segs
            .front()
            .map(|s| s.source.format.rate)
            .unwrap_or(44_100);
        let front_target =
            (rate as f32 * (BUFFER_SECONDS + self.sound.crossfade_s.max(0.0))) as usize;
        // 1) Current song must stay full enough to play without gaps.
        loop {
            let Some(front) = self.segs.front_mut() else {
                return Ok(());
            };
            if front.ended || front.frames() >= front_target {
                break;
            }
            match front.source.next(&mut self.scratch) {
                Ok(crate::decode::DecodeState::Samples) => {
                    front.samples.extend(self.scratch.iter());
                }
                Ok(crate::decode::DecodeState::Pending) => {
                    // Waiting on network: don't touch the next song yet; keep the pump alive.
                    return Ok(());
                }
                Ok(crate::decode::DecodeState::End) => front.ended = true,
                Err(e) => {
                    self.error = e;
                    front.ended = true;
                }
            }
        }
        // 2) Open / warm the following song once the current one is healthy.
        if self.segs.len() < 2 {
            if self
                .segs
                .front()
                .is_some_and(|f| f.ended || f.frames() >= front_target / 2)
            {
                self.prefetch_next();
            }
        }
        if self.segs.len() >= 2 {
            let next_target = (rate as f32 * 0.35) as usize; // light warmup only
            loop {
                let Some(next) = self.segs.get_mut(1) else {
                    break;
                };
                if next.ended || next.frames() >= next_target || !next.source.is_ready() {
                    // Not ready yet: still poll so progressive open can complete off-thread.
                    if !next.ended && !next.source.is_ready() {
                        match next.source.next(&mut self.scratch) {
                            Ok(crate::decode::DecodeState::Samples) => {
                                next.samples.extend(self.scratch.iter());
                            }
                            Ok(crate::decode::DecodeState::Pending) => break,
                            Ok(crate::decode::DecodeState::End) => next.ended = true,
                            Err(e) => {
                                // Prefetch failure must not kill the song that is playing.
                                let _ = e;
                                next.ended = true;
                            }
                        }
                        continue;
                    }
                    break;
                }
                match next.source.next(&mut self.scratch) {
                    Ok(crate::decode::DecodeState::Samples) => {
                        next.samples.extend(self.scratch.iter());
                    }
                    Ok(crate::decode::DecodeState::Pending) => break,
                    Ok(crate::decode::DecodeState::End) => next.ended = true,
                    Err(_) => next.ended = true,
                }
            }
        }
        Ok(())
    }

    /// One device period: take frames from the playing song (and on into the next one when
    /// it has the same format), write them, and move through the queue as songs end.
    fn step(&mut self) -> Result<(), String> {
        if self.segs.is_empty() {
            self.playing = false;
            return Ok(());
        }
        if self
            .segs
            .front()
            .is_some_and(|front| !front.source.is_ready())
        {
            self.fill()?;
            if self
                .segs
                .front()
                .is_some_and(|front| !front.source.is_ready())
            {
                return self.wait_for_remote();
            }
        }
        let Some(front) = self.segs.front() else {
            return Ok(());
        };
        let fmt = front.source.format.clone();
        self.ensure_device(&fmt)?;
        self.fill()?;
        let dev_fmt = self.dev_fmt.clone().expect("opened");
        let dev = self.device.as_mut().expect("opened");
        let want = if self.started {
            dev.wait()?
        } else {
            dev.available()?
        };
        if want == 0 {
            return Ok(());
        }
        let src_ch = usize::from(fmt.channels.max(1));
        let mut taken: Vec<i32> = Vec::with_capacity(want * src_ch);
        let mut frames = 0;
        let mut reopen_after = false;
        while frames < want {
            let Some(front) = self.segs.front_mut() else {
                break;
            };
            let n = (want - frames).min(front.frames());
            let at = taken.len();
            taken.extend(front.samples.drain(..n * src_ch));
            if front.chain.is_none() {
                front.chain = Some(Chain::new(
                    &self.sound,
                    &front.gain,
                    fmt.rate,
                    fmt.channels,
                    dev_fmt.valid_bits,
                ));
            }
            if let Some(c) = front.chain.as_mut() {
                c.process(&mut taken[at..], fmt.channels);
            }
            front.played += n as u64;
            let ended = front.ended;
            if self.sound.crossfade_s > 0.0 {
                self.blend_next(&mut taken[at..], n, &fmt, dev_fmt.valid_bits);
            }
            frames += n;
            if frames >= want {
                break;
            }
            if !ended {
                // Decoding fell behind: decode now, then carry on.
                self.fill()?;
                if self
                    .segs
                    .front()
                    .is_some_and(|f| f.frames() == 0 && !f.ended)
                {
                    break;
                }
                continue;
            }
            // This song is over: on to the next.
            self.segs.pop_front();
            if self.stop_after_current {
                // Stop here, with the next song ready.
                self.stop_after_current = false;
                let _ = self.queue.advance();
                self.segs.clear();
                break;
            }
            if self.queue.advance().is_none() {
                self.queue.index = self.queue.index.or(Some(0));
                break;
            }
            if self.segs.is_empty() {
                // Not decoded ahead (it couldn't be opened, or the queue just changed).
                self.load_current(0);
            }
            match self.segs.front() {
                Some(next) if !next.source.is_ready() => break,
                Some(next) if next.source.format == fmt => continue,
                Some(_) => {
                    reopen_after = true;
                    break;
                }
                None => break,
            }
        }
        // Silence for the rest of the period (end of the queue, or before a format change).
        taken.resize(want * src_ch, 0);
        // What's heard (after the sound tools), kept as mono for a spectrum display.
        if self.spectrum {
            let full = i32::MAX as f32;
            for frame in taken.chunks_exact(src_ch) {
                let sum: f32 = frame.iter().map(|&s| s as f32 / full).sum();
                self.recent.push_back(sum / src_ch as f32);
            }
            while self.recent.len() > crate::spectrum::WINDOW {
                self.recent.pop_front();
            }
        }
        output::remap(&taken, fmt.channels, dev_fmt.channels, &mut self.mapped);
        // The sleep timer fades the last ten seconds out, then pauses.
        let mut fade = 1.0f32;
        if let Some(at) = self.sleep_at {
            let left = at.saturating_duration_since(Instant::now()).as_secs_f32();
            if left <= 0.0 {
                self.sleep_at = None;
                taken.iter_mut().for_each(|x| *x = 0);
                self.segs.truncate(1);
                self.set_playing(false);
                return Ok(());
            }
            fade = (left / 10.0).min(1.0);
        }
        output::encode(&self.mapped, &dev_fmt, self.volume * fade, &mut self.bytes);
        let dev = self.device.as_mut().expect("opened");
        dev.write(&self.bytes, want)?;
        if !self.started {
            dev.start()?;
            self.started = true;
        }
        if self.segs.is_empty() {
            // The end of the queue.
            self.playing = false;
            let _ = dev.stop();
            self.started = false;
            self.paused_at = Some(Instant::now());
            if self
                .queue
                .current()
                .is_some_and(|item| item.reopen_when_stopped())
            {
                self.load_current(0);
            }
            self.publish(true);
        } else if reopen_after {
            // The next song has another format: give the device a moment to play out, then
            // reopen it (the next step does).
            std::thread::sleep(Duration::from_millis(20));
            self.dev_fmt = None;
            self.publish(true);
        }
        Ok(())
    }

    /// Mix the start of the next song into the end of this one (the last `crossfade_s`
    /// seconds), on an equal-power curve. `chunk` holds the `n` frames just taken from the
    /// playing song. The next song then continues from where the blend left it.
    fn blend_next(&mut self, chunk: &mut [i32], n: usize, fmt: &SourceFormat, out_bits: u16) {
        self.fading = false;
        let rate = f64::from(fmt.rate.max(1));
        let fade = (f64::from(self.sound.crossfade_s) * rate) as u64;
        let segs = self.segs.make_contiguous();
        let [front, next, ..] = segs else { return };
        let Some(total) = front.source.frames else {
            return;
        };
        let same_album = front.album.is_some() && front.album == next.album;
        if fade == 0 || same_album || next.source.format != *fmt {
            return;
        }
        let end = total.saturating_sub(front.start_frame);
        let after = front.played;
        let before = after.saturating_sub(n as u64);
        let from = before.max(end.saturating_sub(fade));
        if from >= after {
            return;
        }
        let ch = usize::from(fmt.channels.max(1));
        let want = (after - from) as usize;
        let have = next.frames().min(want);
        if have == 0 {
            return;
        }
        let mut incoming: Vec<i32> = next.samples.drain(..have * ch).collect();
        if next.chain.is_none() {
            next.chain = Some(Chain::new(
                &self.sound,
                &next.gain,
                fmt.rate,
                fmt.channels,
                out_bits,
            ));
        }
        if let Some(c) = next.chain.as_mut() {
            c.process(&mut incoming, fmt.channels);
        }
        next.played += have as u64;
        let skip = (from - before) as usize;
        for k in 0..have {
            let pos = from + k as u64;
            let t = ((pos + fade).saturating_sub(end)) as f64 / fade as f64;
            let (out_g, in_g) = (
                (t * std::f64::consts::FRAC_PI_2).cos(),
                (t * std::f64::consts::FRAC_PI_2).sin(),
            );
            for c in 0..ch {
                let i = (skip + k) * ch + c;
                if let Some(x) = chunk.get_mut(i) {
                    let mixed = f64::from(*x) * out_g + f64::from(incoming[k * ch + c]) * in_g;
                    *x = mixed
                        .round()
                        .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
                        as i32;
                }
            }
        }
        self.fading = true;
    }

    fn release(&mut self) {
        if let Some(mut d) = self.device.take() {
            d.close();
        }
        self.dev_fmt = None;
        self.started = false;
    }

    // --- Status ---------------------------------------------------------------------------

    fn position_ms(&self) -> u64 {
        self.segs
            .front()
            .map(|s| (s.start_frame + s.played) * 1000 / u64::from(s.source.format.rate.max(1)))
            .unwrap_or(0)
    }

    fn signal(&self) -> Option<Signal> {
        let seg = self.segs.front()?;
        let dev_fmt = self.dev_fmt.clone()?;
        let dev = self.device.as_ref()?;
        let src = seg.source.format.clone();
        let mode = dev.mode();
        let mut steps: Vec<String> = seg
            .chain
            .as_ref()
            .map(|c| c.steps.clone())
            .unwrap_or_default();
        let processed = seg.chain.as_ref().is_some_and(|c| c.active) || self.fading;
        if self.fading {
            steps.push(format!(
                "Crossfading into the next song ({} s)",
                self.sound.crossfade_s
            ));
        }
        if mode == Mode::Shared {
            steps.push("Mixed by Windows (shared mode), which may resample".into());
        }
        if dev_fmt.rate != src.rate {
            steps.push(format!("Resampled to {} kHz", dev_fmt.rate as f32 / 1000.0));
        }
        if self.volume < 0.999_999 {
            steps.push(format!("Volume at {}%", (self.volume * 100.0).round()));
        }
        if !src.lossless {
            steps.push(format!("Decoded from {}", src.codec));
        }
        let buffered: usize = self.segs.iter().map(Segment::frames).sum();
        let rate = src.rate.max(1) as f32;
        Some(Signal {
            device: dev.name(),
            mode,
            file: seg.source.info.clone(),
            buffer_ms: (buffered as f32 * 1000.0 / rate) as u32,
            latency_ms: dev.latency_frames() as f32 * 1000.0 / dev_fmt.rate.max(1) as f32,
            bit_perfect: !processed && output::bit_perfect(&src, &dev_fmt, mode, self.volume),
            source: src,
            output: dev_fmt,
            steps,
        })
    }

    fn publish(&mut self, now: bool) {
        if !now && self.last_status.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_status = Instant::now();
        let duration_ms = self
            .segs
            .front()
            .and_then(|s| s.source.duration_ms())
            .unwrap_or(0);
        let buffering = self
            .segs
            .front()
            .is_some_and(|s| !s.source.is_ready());
        let status = Status {
            queue: self.queue_changed.then(|| self.queue.items().to_vec()),
            index: self.queue.index,
            playing: self.playing,
            buffering,
            position_ms: self.position_ms(),
            duration_ms,
            shuffle: self.queue.shuffled(),
            repeat: self.queue.repeat,
            volume: self.volume,
            signal: self.signal(),
            unshuffled: self
                .queue_changed
                .then(|| self.queue.unshuffled())
                .flatten(),
            from: self.from.clone(),
            error: self.error.clone(),
            sound: self.sound.clone(),
            stop_after_current: self.stop_after_current,
            sleep_in_s: self
                .sleep_at
                .map(|t| t.saturating_duration_since(Instant::now()).as_secs()),
            bands: (self.spectrum && self.playing).then(|| {
                let rate = self
                    .segs
                    .front()
                    .map(|s| s.source.format.rate)
                    .unwrap_or(44_100);
                crate::spectrum::levels(self.recent.make_contiguous(), rate)
            }),
        };
        // The regular report, when it would only repeat the last one (paused, or nothing loaded),
        // is not sent: every window would otherwise redraw several times a second for nothing.
        // A command always gets its answer (`now`), and so does a new queue.
        let said = Said::of(&status);
        if !now && status.queue.is_none() && self.said.as_ref() == Some(&said) {
            return;
        }
        self.said = Some(said);
        self.queue_changed = false;
        (self.on_status)(status);
    }
}
