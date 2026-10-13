//! The player: one thread that decodes ahead, feeds the device, and moves through the queue.
//!
//! About a second of audio is kept decoded. When a song ends, the next one (already opened
//! and buffered) continues at the very next sample; if its format differs, the device is
//! reopened for it. Status (position, what's playing, the signal path) goes out about ten
//! times a second, and the queue whenever it changes.

use crate::decode::{Source, SourceFormat};
use crate::output::{self, Device, DeviceFormat, Mode};
use crate::queue::{Queue, Repeat};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Something the queue holds: anything with a file to play.
pub trait Item: Clone + Send + 'static {
    fn path(&self) -> &Path;
}

impl Item for PathBuf {
    fn path(&self) -> &Path {
        self
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
    Output(OutputChoice),
    Quit,
}

/// Where the sound is going and what happens on the way.
#[derive(Debug, Clone, Serialize)]
pub struct Signal {
    pub device: String,
    pub mode: Mode,
    pub source: SourceFormat,
    pub output: DeviceFormat,
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
    pub position_ms: u64,
    pub duration_ms: u64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    pub signal: Option<Signal>,
    pub from: String,
    pub error: String,
}

pub struct Player<T> {
    tx: Sender<Cmd<T>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Item + Serialize> Player<T> {
    /// Start the player thread. `on_status` is called from it.
    pub fn spawn(output: OutputChoice, on_status: impl Fn(Status<T>) + Send + 'static) -> Self {
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
    on_status: Box<dyn Fn(Status<T>) + Send>,
    last_status: Instant,
    paused_at: Option<Instant>,
    scratch: Vec<i32>,
    mapped: Vec<i32>,
    bytes: Vec<u8>,
}

impl<T: Item + Serialize> Engine<T> {
    fn new(choice: OutputChoice, on_status: Box<dyn Fn(Status<T>) + Send>) -> Self {
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
            paused_at: None,
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
                if self.queue.jump(i) {
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
            Cmd::Output(choice) => {
                self.release();
                self.choice = choice;
            }
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
            match Source::open(item.path()) {
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
                    });
                    return;
                }
                Err(e) => {
                    self.error = format!("{}: {e}", item.path().display());
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

    /// Keep the playing song (and the next one) decoded ahead.
    fn fill(&mut self) -> Result<(), String> {
        let rate = self
            .segs
            .front()
            .map(|s| s.source.format.rate)
            .unwrap_or(44_100);
        let target = (rate as f32 * BUFFER_SECONDS) as usize;
        loop {
            let buffered: usize = self.segs.iter().map(Segment::frames).sum();
            if buffered >= target {
                return Ok(());
            }
            let Some(back) = self.segs.back_mut() else {
                return Ok(());
            };
            if back.ended {
                // Open the next song, once, right behind this one.
                if self.segs.len() >= 2 {
                    return Ok(());
                }
                let Some(next) = self.queue.peek_next() else {
                    return Ok(());
                };
                let Some(item) = self.queue.items().get(next).cloned() else {
                    return Ok(());
                };
                match Source::open(item.path()) {
                    Ok(source) => self.segs.push_back(Segment {
                        source,
                        samples: VecDeque::new(),
                        ended: false,
                        played: 0,
                        start_frame: 0,
                    }),
                    // It will be reported when its turn comes.
                    Err(_) => return Ok(()),
                }
                continue;
            }
            match back.source.next(&mut self.scratch) {
                Ok(true) => back.samples.extend(self.scratch.iter()),
                Ok(false) => back.ended = true,
                Err(e) => {
                    // A damaged stretch ends this song early; the next one still plays.
                    self.error = e;
                    back.ended = true;
                }
            }
        }
    }

    /// One device period: take frames from the playing song (and on into the next one when
    /// it has the same format), write them, and move through the queue as songs end.
    fn step(&mut self) -> Result<(), String> {
        let Some(front) = self.segs.front() else {
            self.playing = false;
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
            taken.extend(front.samples.drain(..n * src_ch));
            front.played += n as u64;
            frames += n;
            if frames >= want {
                break;
            }
            if !front.ended {
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
            if self.queue.advance().is_none() {
                self.queue.index = self.queue.index.or(Some(0));
                break;
            }
            if self.segs.is_empty() {
                // Not decoded ahead (it couldn't be opened, or the queue just changed).
                self.load_current(0);
            }
            match self.segs.front() {
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
        output::remap(&taken, fmt.channels, dev_fmt.channels, &mut self.mapped);
        output::encode(&self.mapped, &dev_fmt, self.volume, &mut self.bytes);
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
            self.load_current(0);
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
        let mut steps = Vec::new();
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
        Some(Signal {
            device: dev.name(),
            mode,
            bit_perfect: output::bit_perfect(&src, &dev_fmt, mode, self.volume),
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
        let status = Status {
            queue: self.queue_changed.then(|| self.queue.items().to_vec()),
            index: self.queue.index,
            playing: self.playing,
            position_ms: self.position_ms(),
            duration_ms,
            shuffle: self.queue.shuffled(),
            repeat: self.queue.repeat,
            volume: self.volume,
            signal: self.signal(),
            from: self.from.clone(),
            error: self.error.clone(),
        };
        self.queue_changed = false;
        (self.on_status)(status);
    }
}
