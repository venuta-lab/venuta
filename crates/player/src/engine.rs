//! The player: one thread that decodes ahead, feeds the device, and moves through the queue.
//!
//! About a second of audio is kept decoded. When a song ends, the next one (already opened
//! and buffered) continues at the very next sample; if its format differs, the device is
//! reopened for it. Status (position, what's playing, the signal path) goes out about ten
//! times a second, and the queue whenever it changes.
//!
//! That is engine 1. Engine 2 (section 4 of the Listen together design, its WP4 notes in 9.2)
//! is built beside it in the submodules (`control`, `program`, `status`, `memory`) and chosen
//! with [`Player::spawn_with`]: a control thread that only sends render commands at device
//! frames, decode threads per voice, the renderer on the output's own thread, and a status
//! thread for the app's callback. Both take the same `Cmd`s and give the same `Status`.

use crate::decode::{FileInfo, Source, SourceFormat, Streams};
use crate::dsp::{Chain, Gain, Sound};
use crate::output::{self, Device, DeviceFormat, Mode};
use crate::output_api::{Output, Render};
use crate::queue::{Queue, Repeat};
use crate::sync::{Controller, Correction, Resampler, SyncNote, SyncPlan};
use crate::voice_api::RenderNote;
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use together::room_player::{RoomHandle, RoomPlayer, SyncMode};
use together::time::{SimClock, Timebase};

mod control;
mod memory;
mod program;
mod status;

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
    /// Names an online song's download (engine 2), so it is kept while no voice reads it (a
    /// pause, the moment between a voice and the next of the same song). `None`: nothing to
    /// keep.
    fn stream_key(&self) -> Option<String> {
        None
    }
    /// Open on the player's shared downloads (engine 2: a seek in a stream is a new voice on
    /// the same bytes, never a new download). By default as `open`.
    fn open_on(&self, streams: &Streams) -> Result<Source, String> {
        let _ = streams;
        self.open()
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
    /// Memory taking sound at the pace a real device would (for checks that depend on time,
    /// like playing in step with a room).
    PacedMemory(std::sync::Arc<std::sync::Mutex<output::Captured>>),
    /// Engine 2 only: an output a test makes, on the test's simulated clock (see [`SimChoice`]).
    Sim(SimChoice),
    /// Engine 2 only: a sound device chosen as `Device` is, among a test's simulated devices
    /// (see [`SimDevicesChoice`]). A later `Cmd::Output(Device { .. })` chooses among them.
    SimDevices(SimDevicesChoice),
}

/// Sound devices a test simulates (`sim_devices::SimDevices`), on its clock, and the choice
/// among them (`id: None` follows the default, as `Device` does).
#[derive(Clone)]
pub struct SimDevicesChoice {
    pub clock: SimClock,
    pub devices: Arc<dyn crate::devices::Devices>,
    pub id: Option<String>,
    pub exclusive: bool,
}

/// An output on a simulated clock that the test moves (`pass`): engine 2 runs every thread of
/// its own on that clock, so a run is the same every time. Dropping the player moves the clock
/// until the engine has quit: drop it from the thread that runs the simulation.
#[derive(Clone)]
pub struct SimChoice {
    pub clock: SimClock,
    /// Makes the output (called on the control thread, each time an output is needed).
    pub output: Arc<dyn Fn() -> Box<dyn Output> + Send + Sync>,
}

/// Which engine plays (section 9.1: engine 2 is built beside engine 1, chosen by the setting
/// `player.engine`, until it passes every gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineKind {
    One,
    #[default]
    Two,
}

impl EngineKind {
    /// From the setting's number: engine 2 passed every gate and is the default (release R1);
    /// 1 still chooses engine 1, until it is removed.
    pub fn from_setting(n: u32) -> EngineKind {
        if n == 1 {
            EngineKind::One
        } else {
            EngineKind::Two
        }
    }
}

#[cfg(test)]
mod engine_kind_tests {
    use super::EngineKind;

    #[test]
    fn engine_two_is_the_default_and_one_still_chooses_engine_one() {
        assert_eq!(EngineKind::from_setting(0), EngineKind::Two);
        assert_eq!(EngineKind::from_setting(2), EngineKind::Two);
        assert_eq!(EngineKind::from_setting(1), EngineKind::One);
        assert_eq!(EngineKind::default(), EngineKind::Two);
    }
}

/// What a test may watch of engine 2 as it plays (engine 1 ignores it).
#[derive(Default)]
pub struct Watch {
    /// Every note the renderer sends, with its frames counted from the first frame the player
    /// ever rendered, every start of every output end to end (as a capture lays them).
    pub notes: Option<Sender<RenderNote>>,
    /// Wraps the render path handed to every output (a counting allocator's switch, say).
    #[allow(clippy::type_complexity)]
    pub wrap: Option<Box<dyn Fn(Box<dyn Render>) -> Box<dyn Render> + Send>>,
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
    /// The extra delay set for each output, ms, by endpoint id (the store in Settings; an
    /// output not in it has none set): what you hear is the frame at the DAC that long ago.
    /// Engine 2 puts it into the place it reports; engine 1 ignores it.
    OutputDelays(std::collections::BTreeMap<String, i32>),
    /// Pause, rather than carry on from another output, when the device the music is on goes
    /// away and it is headphones, a headset or Bluetooth (so music never suddenly plays out loud
    /// through the speakers). On unless turned off. Engine 2 only; engine 1 ignores it.
    PauseWhenHeadphonesGo(bool),
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
    /// Play in step with a room (listening together), or on its own again (`None`).
    Sync(Option<SyncPlan>),
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
    /// The endpoint it plays on (the key of its delay), for a sound device.
    pub endpoint: Option<String>,
    /// What kind of output it is (`None`: not a sound device, or engine 1).
    pub kind: Option<crate::devices::Kind>,
    /// What was done about the choice: its stereo twin plays, the chosen device is away...
    pub note: Option<String>,
    /// The extra delay set for this output, ms (`None`: not set).
    pub delay_ms: Option<i32>,
    /// A starting value to offer while it is not set (Bluetooth).
    pub suggested_delay_ms: Option<i32>,
    /// Following Windows' default output.
    pub follows_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status<T: Serialize> {
    /// The whole queue: only when it changed since the last status (it can be long).
    pub queue: Option<Vec<T>>,
    pub index: Option<usize>,
    pub playing: bool,
    /// True while the current song waits for its stream: still opening (a remote body
    /// downloading), or partway through with all that has arrived played (see `stalled`).
    pub buffering: bool,
    /// Buffering partway through the song, not opening: it plays on when the bytes come.
    pub stalled: bool,
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
    /// While playing in step with a room: ready to start, and how far off it is.
    pub sync: Option<SyncStatus>,
    /// What was done to the music to stay with a room since the last status (every seek, every
    /// change of rate): none at all outside a room.
    pub sync_notes: Vec<SyncNote>,
    /// How many times, since the player started, the device played all it had while music was
    /// coming out (this thread late with the next period): each is a gap, heard as a click.
    pub ran_dry: u64,
}

/// How playing in step with a room is going.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SyncStatus {
    /// The song is ready where the room asked (decoded ahead, nothing waiting).
    pub ready: bool,
    /// The song after it is ready too (prepared ahead, to start at the end of this one).
    pub next_ready: bool,
    /// What's heard minus where the room is, milliseconds (positive: ahead).
    pub error_ms: Option<f32>,
}

/// What the last report said, apart from the queue and the band levels (which only come with a
/// change, or while playing): a report that would repeat it word for word is not sent.
#[derive(PartialEq)]
struct Said {
    index: Option<usize>,
    playing: bool,
    buffering: bool,
    stalled: bool,
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
    sync: Option<(bool, bool, Option<i32>)>,
}

impl Said {
    fn of<T: Serialize>(s: &Status<T>) -> Self {
        Self {
            index: s.index,
            playing: s.playing,
            buffering: s.buffering,
            stalled: s.stalled,
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
            // Readiness, and the error to the millisecond (finer changes aren't news).
            sync: s
                .sync
                .map(|y| (y.ready, y.next_ready, y.error_ms.map(|e| e.round() as i32))),
        }
    }
}

pub struct Player<T> {
    /// Engine 1's commands.
    tx: Option<Sender<Cmd<T>>>,
    /// Engine 2's: commands and a room's calls, in one queue, in order.
    msgs: Option<Sender<control::Msg<T>>>,
    thread: Option<JoinHandle<()>>,
    kind: EngineKind,
    /// An engine on a test's simulated clock: dropping the player moves that clock until the
    /// engine has quit.
    sim: Option<SimClock>,
    /// Engine 2's downloads, shared by every source of a song.
    streams: Option<Arc<Streams>>,
}

impl<T: Item + Serialize> Player<T> {
    /// Start engine 1's player thread. `on_status` is called from it.
    pub fn spawn(output: OutputChoice, on_status: impl FnMut(Status<T>) + Send + 'static) -> Self {
        Self::spawn_with(EngineKind::One, output, on_status)
    }

    /// Start the engine `kind`. Engine 1 calls `on_status` from its one thread; engine 2 from
    /// its status thread.
    pub fn spawn_with(
        kind: EngineKind,
        output: OutputChoice,
        on_status: impl FnMut(Status<T>) + Send + 'static,
    ) -> Self {
        Self::spawn_watched(kind, output, on_status, Watch::default())
    }

    /// `spawn_with`, with what a test watches of engine 2 (see [`Watch`]).
    pub fn spawn_watched(
        kind: EngineKind,
        output: OutputChoice,
        on_status: impl FnMut(Status<T>) + Send + 'static,
        watch: Watch,
    ) -> Self {
        match kind {
            EngineKind::One => {
                let (tx, rx) = channel();
                let thread = std::thread::Builder::new()
                    .name("venuta-player".into())
                    .spawn(move || Engine::new(output, Box::new(on_status)).run(rx))
                    .expect("start the player thread");
                Self {
                    tx: Some(tx),
                    msgs: None,
                    thread: Some(thread),
                    kind,
                    sim: None,
                    streams: None,
                }
            }
            EngineKind::Two => {
                let sim = match &output {
                    OutputChoice::Sim(s) => Some(s.clock.clone()),
                    OutputChoice::SimDevices(s) => Some(s.clock.clone()),
                    _ => None,
                };
                let spawned = control::spawn(output, Box::new(on_status), watch);
                Self {
                    tx: None,
                    msgs: Some(spawned.msgs),
                    thread: Some(spawned.thread),
                    kind,
                    sim,
                    streams: Some(spawned.streams),
                }
            }
        }
    }

    pub fn send(&self, cmd: Cmd<T>) {
        match (&self.tx, &self.msgs) {
            (Some(tx), _) => {
                let _ = tx.send(cmd);
            }
            (None, Some(m)) => {
                let _ = m.send(control::Msg::Cmd(cmd));
            }
            (None, None) => {}
        }
    }

    /// Where rooms keep the drift they learn of each output (engine 2's; WP15). Engine 1's
    /// rooms learn nothing to keep.
    pub fn set_drift_memory(&self, memory: Arc<crate::drift_memory::DriftMemory>) {
        if let Some(m) = &self.msgs {
            let _ = m.send(control::Msg::Drift(memory));
        }
    }

    /// Engine 2's shared downloads (`None` on engine 1): a source of a song opened on them
    /// reads the bytes its voices read, so measuring a stream's copy (the room's alignment)
    /// never downloads it twice.
    pub fn streams(&self) -> Option<Arc<Streams>> {
        self.streams.clone()
    }

    /// Whether the player thread is still running (it ends only on Quit, or a panic).
    pub fn is_alive(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Which engine this is.
    pub fn engine(&self) -> EngineKind {
        self.kind
    }
}

impl<T> Drop for Player<T> {
    fn drop(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Quit);
        }
        if let Some(m) = &self.msgs {
            let _ = m.send(control::Msg::Cmd(Cmd::Quit));
        }
        if let Some(t) = self.thread.take() {
            // On a test's clock the engine quits only as that clock moves (its fade, its
            // output's tail): moved here, on the thread that runs the simulation.
            if let Some(clock) = &self.sim {
                while !t.is_finished() {
                    clock.pass(1_000_000);
                }
            }
            let _ = t.join();
        }
    }
}

/// Engine 2 is a player rooms can take (contract 5.4); engine 1's rooms go through
/// `Cmd::Sync`, so its handle does nothing and says it can't play.
impl<T: Item + Serialize> RoomPlayer<T> for Player<T> {
    fn attach(
        &self,
        clock: together::clock_map::ClockMapHandle,
        mode: SyncMode,
    ) -> Box<dyn RoomHandle<T>> {
        match &self.msgs {
            Some(tx) => program::attach(tx.clone(), clock, mode),
            None => Box::new(program::Inert),
        }
    }

    fn adopt(
        &self,
        clock: together::clock_map::ClockMapHandle,
        mode: SyncMode,
        entry: together::anchor::EntryId,
    ) -> Box<dyn RoomHandle<T>> {
        match &self.msgs {
            Some(tx) => program::adopt(tx.clone(), clock, mode, entry),
            None => Box::new(program::Inert),
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
    /// A seek past what has downloaded so far, waiting for those bytes. The position already
    /// shows it; it completes as soon as they arrive.
    seek_waiting: Option<u64>,
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
    /// Playing in step with a room: the room's plan, the last error, and until when a fresh
    /// start or seek is left to settle.
    sync: Option<SyncPlan>,
    /// One room: plays the song a hair faster or slower (idle, and bit-perfect, otherwise), the
    /// rate it is asked for (parts per million faster), and where its frames are made.
    resampler: Resampler,
    rate_target: f64,
    controller: Controller,
    resampled: Vec<i32>,
    sync_error_ms: Option<f32>,
    settle_until: Option<Instant>,
    /// A new plan whose sound hasn't come out yet: lined up exactly when it first does.
    sync_fresh: bool,
    /// Just lined up by a seek: what's left (the seek's own moment) is closed exactly on the
    /// next period, past or before a few just-decoded frames.
    sync_fine: bool,
    /// The room's next song just took over: it plays straight on, no correction, until the
    /// room's plan for it comes (the old song's plan would put it minutes in).
    sync_hold: bool,
    /// The error smoothed over about a second (what corrections act on, so noise isn't chased).
    sync_avg: Option<f64>,
    /// Since when the error has been past the seek line (it is lined up only once it lasts).
    sync_far_since: Option<Instant>,
    /// Decoding last fell behind (a stream waiting for bytes): no lining up for a while, so a
    /// stall plays late instead of skipping ahead into what hasn't arrived.
    starved_at: Option<Instant>,
    /// Music came out in the last period (a seek now would be heard as a cut).
    sounding: bool,
    /// A seek to make after this period, which fades out first (milliseconds into the song).
    seek_after_fade: Option<f64>,
    /// Moved while stopped (a seek while paused): what the device still holds is from the old
    /// place, so it is let go before the device starts again.
    flush_before_start: bool,
    /// Frames of fade-in still to come after such a seek.
    fade_in_left: usize,
    /// A stream that ran out faded out, waiting in silence to have enough again (see
    /// `suspend_or_resume`).
    suspended: bool,
    /// Music fading out to silence, across periods (before a seek, a pause, another song), and
    /// whether a command is waiting for it to be done (`fade_out_now`).
    fade_out: Option<FadeOut>,
    fading_out: bool,
    /// What was done to stay with the room, to say in the next status.
    notes: Vec<SyncNote>,
    /// The rate last said (parts per million faster), to say only when it changes.
    rate_said: i32,
    /// How many times the device ran dry while music played (see `Status::ran_dry`).
    ran_dry: u64,
    /// The volume the last period ended at, and a change of it gliding (see `GainRamp`).
    gain_out: Option<f64>,
    gain_ramp: Option<GainRamp>,
}

/// Music fading out: `done` of `len` frames so far; once all are, silence comes out (`silent`
/// frames of it so far) and nothing more is taken from the song.
#[derive(Debug, Clone, Copy)]
struct FadeOut {
    done: usize,
    len: usize,
    silent: usize,
}

impl FadeOut {
    fn over(&self) -> bool {
        self.done >= self.len
    }
}

/// A change of volume under way: from one gain to another over `len` frames, `done` so far.
#[derive(Debug, Clone, Copy)]
struct GainRamp {
    from: f64,
    to: f64,
    done: usize,
    len: usize,
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
            seek_waiting: None,
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
            sync: None,
            resampler: Resampler::default(),
            rate_target: 0.0,
            controller: Controller::default(),
            resampled: Vec::new(),
            sync_error_ms: None,
            settle_until: None,
            sync_fresh: false,
            sync_fine: false,
            sync_hold: false,
            sync_avg: None,
            sync_far_since: None,
            starved_at: None,
            sounding: false,
            seek_after_fade: None,
            flush_before_start: false,
            fade_in_left: 0,
            fade_out: None,
            fading_out: false,
            suspended: false,
            notes: Vec::new(),
            rate_said: 0,
            ran_dry: 0,
            gain_out: None,
            gain_ramp: None,
        }
    }

    fn run(mut self, rx: Receiver<Cmd<T>>) {
        crate::sync::Resampler::warm();
        // The sound comes first: Windows schedules this thread as pro audio (MMCSS), so nothing
        // else on the computer (a room's network, the window drawing) makes it late.
        #[cfg(windows)]
        let _pro_audio = crate::wasapi_out::ProAudio::enter();
        loop {
            // Commands first; when not playing, wait for one.
            // Playing with no device yet: the first song is still arriving, so look again soon
            // (its first audio is noticed within 15 ms, not up to 200).
            let timeout = if self.playing && self.device.is_some() {
                Duration::ZERO
            } else if self.playing {
                Duration::from_millis(15)
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
                // What's playing fades out first (never cut off mid-wave).
                self.fade_out_now();
                // A new queue is never under an old room's plan: a room's follower sends its
                // plan after loading its song; your own queue (a room's goodbye that never came,
                // a window that thought it was out of the room) plays as it does alone.
                self.set_sync(None);
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
            Cmd::Seek(ms) => {
                // Out of the old place, in at the new one.
                self.fade_out_now();
                self.seek(ms);
                self.fade_in_mid_song();
            }
            Cmd::Next => {
                self.fade_out_now();
                let ahead = self.queue.peek_next();
                if self.queue.skip_next().is_some() {
                    self.play_ahead_or_load(ahead);
                } else {
                    self.set_playing(false);
                }
            }
            Cmd::Previous => {
                self.fade_out_now();
                // Into a song: back to its start. At its start: the one before.
                if self.position_ms() > 3000 || self.queue.skip_previous().is_none() {
                    self.seek(0);
                } else {
                    self.load_current(0);
                }
            }
            Cmd::Jump(i) => {
                self.fade_out_now();
                // Same song again: rewind in place (no re-resolve / re-download).
                if self.queue.index == Some(i) {
                    self.seek(0);
                    self.set_playing(true);
                } else {
                    let ahead = self.queue.peek_next();
                    if self.queue.jump(i) {
                        self.play_ahead_or_load(ahead);
                        self.set_playing(true);
                    }
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
                let next_before = self.queue.peek_next();
                self.queue.insert(items, next);
                self.queue_changed = true;
                // The song decoded ahead is kept when it is still the next one (songs added
                // after it): a stream readied ahead isn't opened again.
                if next || self.queue.peek_next() != next_before {
                    self.drop_prefetch();
                }
                if was_empty {
                    self.load_current(0);
                    self.set_playing(true);
                }
            }
            Cmd::Remove(i) => {
                let current = self.queue.index == Some(i);
                if current {
                    self.fade_out_now();
                }
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
                self.fade_out_now();
                self.release();
                self.choice = choice;
            }
            // Engine 2's (the place it reports is the one heard through the output's delay).
            Cmd::OutputDelays(_) | Cmd::PauseWhenHeadphonesGo(_) => {}
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
            Cmd::Sync(plan) => self.set_sync(plan),
            Cmd::Quit => {}
        }
        self.publish(true);
    }

    fn set_playing(&mut self, on: bool) {
        if !on {
            // Down to silence before the device stops (a stop mid-wave is a click), and only
            // silence left in it for the next start.
            self.fade_out_now();
        }
        if on && self.segs.is_empty() && self.queue.current().is_some() {
            self.load_current(0);
        }
        let resuming = on && !self.playing;
        self.playing = on && !self.segs.is_empty();
        if self.playing {
            if resuming && !self.started {
                // On again partway into the song: up from silence.
                self.fade_in_mid_song();
            }
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
            // Nothing comes out now: a seek from here is not heard as a cut.
            self.sounding = false;
        }
    }

    /// Open the current song (skipping ones that can't be opened), from `start_ms`.
    fn load_current(&mut self, start_ms: u64) {
        // Stopped, the device may still hold the last song's end: let it go, so the new one
        // starts clean (and in a room, on time by the device's clock).
        if !self.started
            && let Some(d) = self.device.as_mut()
        {
            d.flush();
        }
        self.segs.clear();
        self.seek_waiting = None;
        self.resampler.reset();
        self.suspended = false;
        self.fade_out = None;
        // A seek or fade-in meant for the song before is not this one's.
        self.seek_after_fade = None;
        self.fade_in_left = 0;
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
        self.seek_waiting = None;
        // Another place: nothing before it for the resampler to carry on from.
        self.resampler.reset();
        if !self.playing {
            self.flush_before_start = true;
        }
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
            // An online song not downloaded that far yet: move there now and finish the seek
            // when the bytes arrive, instead of refusing it.
            Err(e) if e.contains("waiting") => {
                front.samples.clear();
                front.ended = false;
                front.played = 0;
                front.start_frame = ms * u64::from(front.source.format.rate.max(1)) / 1000;
                self.segs.truncate(1);
                self.seek_waiting = Some(ms);
            }
            Err(e) => self.error = e,
        }
    }

    /// Try the waiting seek again. True while it still waits (nothing to decode yet).
    fn retry_waiting_seek(&mut self) -> bool {
        let Some(ms) = self.seek_waiting else {
            return false;
        };
        let Some(front) = self.segs.front_mut() else {
            self.seek_waiting = None;
            return false;
        };
        match front.source.seek(ms) {
            Ok(frame) => {
                front.start_frame = frame;
                self.seek_waiting = None;
                false
            }
            Err(e) if e.contains("waiting") => true,
            Err(e) => {
                self.error = e;
                self.seek_waiting = None;
                false
            }
        }
    }

    /// After Next or a jump: when the song now current is the one already decoded ahead
    /// (`ahead` was the queue's next index before the move), play that one. An online song is
    /// then already resolved and downloading, so it starts at once instead of from nothing.
    fn play_ahead_or_load(&mut self, ahead: Option<usize>) {
        let usable = self.segs.len() >= 2
            && ahead.is_some()
            && self.queue.index == ahead
            && self
                .segs
                .get(1)
                .is_some_and(|s| !(s.ended && s.frames() == 0));
        if usable {
            self.resampler.reset();
            self.segs.pop_front();
            self.segs.truncate(1);
            self.seek_waiting = None;
        } else {
            self.load_current(0);
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
                    paced: None,
                }),
                OutputChoice::PacedMemory(c) => Box::new(output::MemoryDevice {
                    captured: c.clone(),
                    period: 480,
                    paced: Some(output::Pace::default()),
                }),
                #[cfg(windows)]
                OutputChoice::Device { id, exclusive } => Box::new(
                    crate::wasapi_out::WasapiDevice::new(id.as_deref(), *exclusive)?,
                ),
                #[cfg(not(windows))]
                OutputChoice::Device { .. } => {
                    return Err("Playing to a sound device needs Windows for now.".into());
                }
                OutputChoice::Sim(_) | OutputChoice::SimDevices(_) => {
                    return Err("A simulated output plays only with engine 2.".into());
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
    /// Stopped after a move: let go of what the device holds from the old place, so the first
    /// thing heard when it starts again is the new place (not a blip of the old one).
    fn flush_if_moved(&mut self) {
        if !self.flush_before_start || self.started {
            return;
        }
        self.flush_before_start = false;
        if let Some(d) = self.device.as_mut() {
            d.flush();
        }
    }

    /// Keep an already-open device clocked while the first remote audio chunk is arriving.
    fn wait_for_remote(&mut self) -> Result<(), String> {
        self.flush_if_moved();
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
            // Stopped with its buffer full (paused mid-song): on again from what it holds.
            if !self.started {
                device.start()?;
                self.started = true;
            }
            return Ok(());
        }
        self.bytes.clear();
        self.bytes.resize(want * format.bytes_per_frame(), 0);
        device.write(&self.bytes, want)?;
        if !self.started {
            device.start()?;
            self.started = true;
        }
        self.sounding = false;
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
        // A seek waiting for its bytes: nothing to decode until it lands.
        if self.retry_waiting_seek() {
            return Ok(());
        }
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
                    // Said with the song's name: the queue goes on to the next one.
                    self.error = match self.queue.current() {
                        Some(item) => format!("{}: {e}", item.label()),
                        None => e,
                    };
                    front.ended = true;
                }
            }
        }
        // 2) Open / warm the following song once the current one is healthy.
        if self.segs.len() < 2
            && self
                .segs
                .front()
                .is_some_and(|f| f.ended || f.frames() >= front_target / 2)
        {
            self.prefetch_next();
        }
        if self.segs.len() >= 2 {
            let next_target = (rate as f32 * 0.35) as usize; // light warmup only
            while let Some(next) = self.segs.get_mut(1) {
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
        self.flush_if_moved();
        let dev = self.device.as_mut().expect("opened");
        let want = if self.started {
            dev.wait()?
        } else {
            dev.available()?
        };
        // Music was coming out and the device has played all it was given: it ran dry (a gap,
        // heard as a click), because this thread was late.
        if self.started && self.sounding && dev.queued_frames() == Some(0) {
            self.ran_dry += 1;
        }
        if want == 0 {
            // Stopped with its buffer full (paused mid-song): on again from what it holds.
            if !self.started {
                dev.start()?;
                self.started = true;
            }
            return Ok(());
        }
        // In step with a room: silent until its start, then kept where the room is.
        let lead = if self.sync.is_some() {
            match self.follow_room(want, &fmt)? {
                Some(lead) => lead,
                None => return Ok(()),
            }
        } else {
            0
        };
        self.suspend_or_resume(want, &fmt);
        // One room's rate (or gliding back from it): the period's frames made at that rate
        // (none while held silent).
        let held = self.fade_out.is_some_and(|f| f.over());
        if lead == 0 && !held && (self.sync.is_some() || !self.resampler.idle()) {
            self.rate_front(want);
        }
        let src_ch = usize::from(fmt.channels.max(1));
        let mut taken: Vec<i32> = Vec::with_capacity(want * src_ch);
        // The room starts within this period: silence up to the very frame it starts on.
        taken.resize(lead * src_ch, 0);
        // Faded out all the way: silence, the song held where the fade ended.
        let mut frames = if self.fade_out.is_some_and(|f| f.over()) {
            want
        } else {
            lead
        };
        let mut reopen_after = false;
        while frames < want {
            let Some(front) = self.segs.front_mut() else {
                break;
            };
            let n = (want - frames).min(front.frames());
            let at = taken.len();
            taken.extend(front.samples.drain(..n * src_ch));
            if front.chain.is_none() {
                // An online song's levelling comes from the catalog's loudness, known once open.
                let gain = front.gain.or_loudness(front.source.loudness_db);
                front.chain = Some(Chain::new(
                    &self.sound,
                    &gain,
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
                    self.starved_at = Some(Instant::now());
                    break;
                }
                continue;
            }
            // This song is over: on to the next (the rate came back to its own before its end).
            self.segs.pop_front();
            self.resampler.reset();
            // In a room, the next song plays straight on until the room's plan for it comes.
            if self.sync.is_some() {
                self.sync_hold = true;
                self.sync_avg = None;
                self.sync_far_since = None;
                // A lining-up meant for the song that just ended: not into this one.
                self.seek_after_fade = None;
            }
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
        let held = self.fade_out.is_some_and(|f| f.over());
        let music = if held { 0 } else { frames.saturating_sub(lead) };
        taken.resize(want * src_ch, 0);
        // Fading out (carried on across periods until it's all the way down, then silence), or
        // fading in from silence.
        if let Some(mut f) = self.fade_out {
            for frame in taken.chunks_exact_mut(src_ch).skip(lead).take(music) {
                let g = if f.done < f.len {
                    crate::sync::fade_gain(f.done, f.len, false)
                } else {
                    0.0
                };
                f.done += 1;
                frame
                    .iter_mut()
                    .for_each(|x| *x = (f64::from(*x) * f64::from(g)) as i32);
            }
            if held {
                f.silent += want;
            }
            self.fade_out = Some(f);
        } else if self.fade_in_left > 0 {
            let len = (crate::sync::FADE.as_secs_f64() * f64::from(fmt.rate)) as usize;
            for (k, frame) in taken
                .chunks_exact_mut(src_ch)
                .skip(lead)
                .take(music)
                .enumerate()
            {
                if k >= self.fade_in_left {
                    break;
                }
                let g = crate::sync::fade_gain(len - self.fade_in_left + k + 1, len, true);
                frame
                    .iter_mut()
                    .for_each(|x| *x = (f64::from(*x) * f64::from(g)) as i32);
            }
            self.fade_in_left = self.fade_in_left.saturating_sub(music);
        }
        self.sounding = music > 0 && !self.fade_out.is_some_and(|f| f.over());
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
        // A change of volume glides over 20 ms on a raised-cosine curve, across periods, instead
        // of stepping (a step is a click); a steady volume is applied as it is (bit-perfect at 1).
        let gain = f64::from(self.volume * fade);
        let gain_now = self.gain_out.unwrap_or(gain);
        if self.gain_ramp.is_none_or(|r| r.to != gain) && (gain - gain_now).abs() > 1e-9 {
            let len = (0.020 * f64::from(dev_fmt.rate)) as usize;
            self.gain_ramp = Some(GainRamp {
                from: gain_now,
                to: gain,
                done: 0,
                len: len.max(1),
            });
        }
        if let Some(mut r) = self.gain_ramp {
            let ch = usize::from(dev_fmt.channels.max(1));
            let mut g = gain_now;
            for frame in self.mapped.chunks_exact_mut(ch) {
                if r.done < r.len {
                    r.done += 1;
                    let t = r.done as f64 / r.len as f64;
                    g = r.from + (r.to - r.from) * (1.0 - (std::f64::consts::PI * t).cos()) / 2.0;
                } else {
                    g = r.to;
                }
                for x in frame {
                    *x = (f64::from(*x) * g)
                        .round()
                        .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
                        as i32;
                }
            }
            output::encode(&self.mapped, &dev_fmt, 1.0, &mut self.bytes);
            self.gain_out = Some(g);
            self.gain_ramp = (r.done < r.len).then_some(r);
        } else {
            output::encode(&self.mapped, &dev_fmt, gain as f32, &mut self.bytes);
            self.gain_out = Some(gain);
        }
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
        // Faded out all the way: now the seek, and a fade back in from it.
        if self.fade_out.is_some_and(|f| f.over())
            && let Some(to) = self.seek_after_fade.take()
        {
            self.fade_out = None;
            let to = self.room_place_now().unwrap_or(to);
            self.realign(to, 600);
            // No closing-up a few frames after it: that would cut into the fade-in. The few ms a
            // seek lands off aren't heard apart, and in one room the rate takes them up.
            self.sync_fine = false;
            self.fade_in_left = (crate::sync::FADE.as_secs_f64() * f64::from(fmt.rate)) as usize;
        }
        Ok(())
    }

    /// A stream that can't keep up: rather than play what has come a few frames at a time (gaps
    /// with hard edges, heard as crackle), the music fades out over what is left, waits in
    /// silence until a second of it is ready, and fades back in from where it stopped. In a
    /// room it then plays a little late, and catches up gently as the room's rules say.
    fn suspend_or_resume(&mut self, want: usize, fmt: &SourceFormat) {
        let Some(front) = self.segs.front() else {
            return;
        };
        let rate = f64::from(fmt.rate.max(1));
        if self.suspended {
            self.starved_at = Some(Instant::now());
            let refilled = front.ended || front.frames() as f64 >= rate * 0.9;
            if refilled {
                self.suspended = false;
                self.fade_out = None;
                // Back from silence: nothing before it for the resampler to carry on from.
                self.resampler.reset();
                self.fade_in_mid_song();
            }
            return;
        }
        // Enough left to fade out over in full, the resampler's look-ahead included.
        let reserve = (crate::sync::FADE_OUT.as_secs_f64() * rate) as usize + 64;
        let running_out = !front.ended
            && front.source.is_waiting()
            && front.frames() < want + reserve
            && self.seek_waiting.is_none();
        if running_out && self.fade_out.is_none() && self.sounding {
            let len = (crate::sync::FADE_OUT.as_secs_f64() * rate) as usize;
            self.fade_out = Some(FadeOut {
                done: 0,
                len: len.min(front.frames()).max(1),
                silent: 0,
            });
            self.suspended = true;
            self.starved_at = Some(Instant::now());
        }
    }

    /// Music is coming out: fade it out to silence now, and wait until the device holds only
    /// silence, so whatever comes next (a pause, a stop, a seek, another song, another output)
    /// starts from silence instead of cutting the wave. Nothing happens when nothing sounds.
    fn fade_out_now(&mut self) {
        if self.fading_out
            || !(self.playing && self.started && self.sounding)
            || self.device.is_none()
        {
            return;
        }
        self.fading_out = true;
        let Some(rate) = self.segs.front().map(|f| f.source.format.rate) else {
            return;
        };
        let len = (crate::sync::FADE_OUT.as_secs_f64() * f64::from(rate)) as usize;
        let hold = self
            .device
            .as_ref()
            .map_or(0, |d| d.latency_frames())
            .max(len / 2);
        // A seek waiting for its fade is the fade already under way: it comes after.
        let pending = self.seek_after_fade.take();
        if !self.fade_out.is_some() {
            self.fade_out = Some(FadeOut {
                done: 0,
                len,
                silent: 0,
            });
        }
        // Each step is a device period: the fade and then a device's worth of silence.
        for _ in 0..200 {
            if self.step().is_err() || !self.playing {
                break;
            }
            match self.fade_out {
                Some(f) if f.over() && f.silent >= hold => break,
                Some(_) => {}
                None => break,
            }
        }
        self.fade_out = None;
        self.sounding = false;
        self.fading_out = false;
        if let Some(to) = pending {
            let to = self.room_place_now().unwrap_or(to);
            self.realign(to, 600);
            self.sync_fine = false;
            self.fade_in_mid_song();
        }
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
            let gain = next.gain.or_loudness(next.source.loudness_db);
            next.chain = Some(Chain::new(
                &self.sound,
                &gain,
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
        self.resampler.reset();
        // Another device (or this one again) drifts its own way.
        self.controller.forget();
        self.dev_fmt = None;
        self.started = false;
        // What the old device held is gone with it, and nothing comes out until another opens.
        self.sounding = false;
        self.flush_before_start = false;
        // In a room, the next device (another output, or this one opened again) takes a moment
        // to open while the room plays on: the song lines up again from silence when it starts,
        // as at any start, instead of coming back behind and waiting for the drift rule.
        if self.sync.is_some() {
            self.sync_fresh = true;
            self.sync_fine = false;
            self.seek_after_fade = None;
            self.fade_in_left = 0;
            self.sync_avg = None;
            self.sync_far_since = None;
            self.sync_error_ms = None;
        }
    }

    // --- Playing in step with a room ---------------------------------------------------------

    /// A room's plan: make the song ready where it says, hold silent until its start, or pause
    /// there. `None`: on its own again.
    fn set_sync(&mut self, plan: Option<SyncPlan>) {
        // The same plan, its start known a little better: kept on, the usual corrections
        // taking up the difference (no settling again, trimming carries on).
        if let (Some(old), Some(new)) = (self.sync, plan)
            && crate::sync::is_nudge(&old, &new, Instant::now())
        {
            self.sync = Some(new);
            self.sync_hold = false;
            self.controller.plan_moved();
            return;
        }
        // A plan that doesn't simply play on from here (a hold, a start ahead, a pause): the
        // music fades out first, so the hold is silence, not a cut.
        if let Some(p) = plan
            && !(p.playing && p.anchor.is_some_and(|a| a <= Instant::now()))
        {
            self.fade_out_now();
        }
        // The next song just took over: its first plan is lined up with only if off by more
        // than anyone hears (copies of a song differ by a few tens of ms; apart, the drift rule
        // takes up what's left; in one room, the rate).
        let limit_ms: i64 = match plan {
            Some(p) if self.sync_hold && p.same_room => crate::sync::NUDGE_MS as i64,
            Some(_) if self.sync_hold => crate::sync::APART_SEEK_MS as i64,
            _ => 30,
        };
        // Back to the song's own rate (gliding there, so nothing is heard).
        self.rate_target = 0.0;
        self.controller.reset();
        self.sync_error_ms = None;
        self.sync_hold = false;
        self.sync_avg = None;
        self.sync_far_since = None;
        // A seek still waiting for its fade-out was the old plan's place: never carried into
        // this one (a pause stops the device before it runs, and it would land later, in a
        // resumed or the next song, as a jump back to where that pause was).
        self.seek_after_fade = None;
        let Some(plan) = plan else {
            // On its own again: nothing of the room is left to act on the music.
            self.sync = None;
            self.sync_fresh = false;
            self.sync_fine = false;
            self.settle_until = None;
            self.starved_at = None;
            return;
        };
        let now = Instant::now();
        // Where the song should be when sound next comes out: the room's place, a moment on.
        let started = plan.anchor.is_some_and(|a| a <= now);
        let target = match plan.expected_ms(now) {
            Some(at) if plan.playing && started => {
                (at + self.heard_delay_ms(&plan)).max(0.0) as u64
            }
            _ => plan.position_ms,
        };
        if self.segs.is_empty() && self.queue.current().is_some() {
            self.load_current(target);
        } else if (self.position_ms() as i64 - target as i64).abs() > limit_ms {
            if plan.playing {
                self.seek_heard(target as f64);
            } else {
                // Paused: the device stops now, so nothing of the move is heard; made at once,
                // and what it holds from before the move is let go before it starts again.
                self.notes.push(SyncNote::Seek {
                    from_ms: self.position_ms(),
                    to_ms: target,
                    faded: false,
                });
                self.realign(target as f64, 600);
                self.flush_before_start = true;
            }
        }
        self.sync = Some(plan);
        self.sync_fresh = true;
        self.sync_fine = false;
        self.settle_until = Some(now + Duration::from_millis(600));
        // Playing (or waiting to) keeps the device going: silence until the start, which also
        // keeps the song decoding ahead. Paused: paused there.
        self.set_playing(plan.playing);
    }

    /// How long from writing a frame to hearing it: the device's buffer and any extra delay set
    /// for this output (Bluetooth), milliseconds.
    fn heard_delay_ms(&self, plan: &SyncPlan) -> f64 {
        let device = match (&self.device, &self.dev_fmt) {
            (Some(d), Some(f)) => {
                d.queued_frames().unwrap_or_else(|| d.latency_frames()) as f64 * 1_000.0
                    / f64::from(f.rate.max(1))
            }
            _ => 0.0,
        };
        device + f64::from(plan.extra_latency_ms)
    }

    /// One device period in step with the room. `None` when the period was handled here (silence
    /// before the start); else play it, after that many frames of silence (the start falling
    /// within it), and after any seek or trimming.
    fn follow_room(&mut self, want: usize, fmt: &SourceFormat) -> Result<Option<usize>, String> {
        let Some(plan) = self.sync else {
            return Ok(Some(0));
        };
        // The device not going yet: a period of silence starts it, so every decision after this
        // one knows its real delay (before it runs, only its whole buffer is known, and a start
        // judged by that comes out a period early, then stops).
        if !self.started && plan.playing {
            self.fill()?;
            self.write_silence(want)?;
            return Ok(None);
        }
        let rate = f64::from(fmt.rate.max(1));
        let now = Instant::now();
        let delay = self.heard_delay_ms(&plan);
        let starts = plan.anchor.map(|a| {
            a.checked_sub(Duration::from_secs_f64(delay.max(0.0) / 1_000.0))
                .unwrap_or(a)
        });
        // Before the start (or not started yet): silence, the song decoding ahead meanwhile.
        // Once this plan's music is coming out, never again: a start known a little later
        // (the host's clock estimated better just after a start) would otherwise cut silence
        // into the music; the usual rules take up the difference.
        let begun = !self.sync_fresh && self.sounding;
        if !begun && !starts.is_some_and(|s| now >= s) {
            // The start falls within this period: its first sound on the very frame (a start
            // from silence needs no lining up after).
            if let Some(s) = starts
                && plan.playing
                && self.sync_fresh
                && self.segs.front().is_some_and(|f| f.frames() > 0)
            {
                let lead = (s.duration_since(now).as_secs_f64() * rate).round() as usize;
                if lead < want {
                    self.sync_fresh = false;
                    self.settle_until = Some(now + Duration::from_millis(400));
                    self.notes.push(SyncNote::Started {
                        position_ms: self.position_ms(),
                    });
                    self.fade_in_mid_song();
                    return Ok(Some(lead));
                }
            }
            self.fill()?;
            self.write_silence(want)?;
            return Ok(None);
        }
        // The next song took over: straight on until the room's plan for it comes; and this
        // period fades out before a seek, so nothing more to decide now.
        if self.sync_hold || self.seek_after_fade.is_some() {
            return Ok(Some(0));
        }
        let Some(front) = self.segs.front() else {
            return Ok(Some(0));
        };
        let written_ms = (front.start_frame + front.played) as f64 * 1_000.0 / rate;
        let heard_ms = written_ms - delay;
        let Some(expected) = plan.expected_ms(now) else {
            return Ok(Some(0));
        };
        // The first sound of a plan: put the song exactly where the room will be when it's heard
        // (the device may have taken a moment to open since the plan came).
        if self.sync_fresh {
            self.sync_fresh = false;
            let off = (written_ms - (expected + delay)).abs();
            // From silence, exactly where the room is; with music already coming out (a new plan
            // mid-song, the next song after this one), only an offset someone could hear.
            let limit = if !self.sounding {
                15.0
            } else if plan.same_room {
                crate::sync::NUDGE_MS
            } else {
                crate::sync::APART_NUDGE_MS
            };
            // From silence partway into the song (joining, another output): faded in, not cut in.
            if !self.sounding {
                self.fade_in_mid_song();
            }
            if off > limit {
                self.seek_heard(expected + delay);
            } else {
                self.settle_until = Some(now + Duration::from_millis(400));
            }
            return Ok(Some(0));
        }
        let error = heard_ms - expected;
        if self.sync_fine {
            self.sync_fine = false;
            // A fresh measure from here on (this isn't a jump in the sound).
            self.sync_error_ms = None;
            let frames = (error.abs() * rate / 1_000.0).round() as usize;
            if error < -1.0 && frames > 0 {
                // Behind: past the first few frames just decoded.
                let skipped = self.skip_front(frames);
                if skipped < frames {
                    self.sync_fine = true;
                }
                self.notes.push(SyncNote::Fine {
                    frames: skipped as i64,
                });
                return Ok(Some(0));
            }
            if error > 1.0 && frames > 0 && frames < want {
                // Ahead: a moment of silence first.
                self.notes.push(SyncNote::Fine {
                    frames: -(frames as i64),
                });
                return Ok(Some(frames));
            }
        }
        self.sync_error_ms = Some(error as f32);
        let settling = self.settle_until.is_some_and(|t| now < t);
        // Smoothed over about a second: one late clock reading or a jittery position is noise,
        // not a reason to touch the music.
        let share = (want as f64 / rate / crate::sync::SMOOTHING.as_secs_f64()).min(1.0);
        let avg = match self.sync_avg {
            Some(a) if !settling => a + (error - a) * share,
            _ => error,
        };
        self.sync_avg = Some(avg);
        // A stream that stalled plays late for a while instead of skipping ahead into bytes that
        // haven't come yet (which would only stall it again).
        let starved = self
            .starved_at
            .is_some_and(|t| now.duration_since(t) < Duration::from_secs(3));
        if starved {
            // Nothing chased while a stream catches up.
            self.rate_target = 0.0;
            self.controller.reset();
        }
        if settling || starved {
            self.sync_far_since = None;
            return Ok(Some(0));
        }
        match crate::sync::correction(avg, plan.same_room, false) {
            Correction::None | Correction::Trim { .. } => {
                self.sync_far_since = None;
                // A hair faster or slower, gliding, until in step (apart: past 10 ms only).
                self.rate_target = self
                    .controller
                    .rate(avg, want as f64 / rate, plan.same_room);
                Ok(Some(0))
            }
            Correction::Seek => {
                // Only once it lasts: a burst of late clock readings passes, a drift doesn't.
                let since = *self.sync_far_since.get_or_insert(now);
                if now.duration_since(since) >= crate::sync::far_for(plan.same_room) {
                    self.sync_far_since = None;
                    self.seek_heard(expected + delay);
                }
                Ok(Some(0))
            }
        }
    }

    /// The next `want` frames of the playing song, a hair faster or slower when one room asks
    /// (and while the rate glides back to the song's own, and lands on a whole frame). Idle, it
    /// only keeps the last frames in one room (the filter's left side for a correction to come);
    /// outside a room it does nothing at all.
    fn rate_front(&mut self, want: usize) {
        let in_room = self.sync.is_some();
        let Some(front) = self.segs.front_mut() else {
            return;
        };
        let ch = usize::from(front.source.format.channels.max(1));
        let rate = front.source.format.rate;
        // The last second of a song: back to its own rate, so the next one follows exactly.
        let near_end = front.source.frames.is_some_and(|total| {
            total.saturating_sub(front.start_frame + front.played) < u64::from(rate)
        });
        let target = if in_room && !near_end {
            self.rate_target
        } else {
            0.0
        };
        if self.resampler.idle() && target == 0.0 {
            if in_room {
                let n = want.min(front.frames()) * ch;
                let head = front.samples.make_contiguous();
                self.resampler.played_as_is(&head[..n], ch);
            }
            return;
        }
        let mut out = std::mem::take(&mut self.resampled);
        out.clear();
        let ended = front.ended;
        let input = front.samples.make_contiguous();
        match self
            .resampler
            .process(input, ch, rate, ended, want, target, &mut out, None)
        {
            Some(used) => {
                let used = used.min(front.frames());
                front.samples.drain(..used * ch);
                for &v in out.iter().rev() {
                    front.samples.push_front(v);
                }
                // The count of frames played stays true to the song (the loop adds `want`).
                front.played = (front.played + used as u64).saturating_sub(want as u64);
            }
            // Not decoded far enough to work out (a stream that stalled): as it is, from here.
            None => {
                let n = (want * ch).min(input.len());
                self.resampler.as_is_for_now(&input[..n], ch);
            }
        }
        self.resampled = out;
        let ppm = self.resampler.ppm().round() as i32;
        self.say_rate(ppm);
    }

    /// A seek that may be heard: while music is coming out, this period fades out and the seek
    /// comes after it, fading back in; from silence, at once.
    fn seek_heard(&mut self, to_ms: f64) {
        self.notes.push(SyncNote::Seek {
            from_ms: self.position_ms(),
            to_ms: to_ms.max(0.0) as u64,
            faded: self.sounding && self.playing,
        });
        self.say_rate(0);
        if self.sounding && self.playing {
            self.seek_after_fade = Some(to_ms);
            if self.fade_out.is_none() {
                let rate = self.segs.front().map_or(48_000, |f| f.source.format.rate);
                self.fade_out = Some(FadeOut {
                    done: 0,
                    len: (crate::sync::FADE_OUT.as_secs_f64() * f64::from(rate)) as usize,
                    silent: 0,
                });
            }
        } else {
            self.realign(to_ms, 600);
        }
    }

    /// Where the room's song is heard now (milliseconds), when it plays: where a seek lands that
    /// waited for a fade-out (the room went on meanwhile).
    fn room_place_now(&self) -> Option<f64> {
        let p = self.sync.filter(|p| p.playing)?;
        Some(p.expected_ms(Instant::now())? + self.heard_delay_ms(&p))
    }

    /// Music about to come out of silence partway into the song: it fades in over a moment
    /// (from its very start it rises on its own).
    fn fade_in_mid_song(&mut self) {
        if let Some(front) = self.segs.front()
            && front.start_frame + front.played > 0
        {
            self.fade_in_left =
                (crate::sync::FADE.as_secs_f64() * f64::from(front.source.format.rate)) as usize;
        }
    }

    /// Say the rate when it changed by enough to matter (or to or from the song's own).
    fn say_rate(&mut self, ppm: i32) {
        let changed = (ppm == 0) != (self.rate_said == 0) || (ppm - self.rate_said).abs() >= 100;
        if changed {
            self.rate_said = ppm;
            self.notes.push(SyncNote::Rate { ppm });
        }
    }

    /// Line up with the room by a seek to `to_ms` (where it will be when this is heard); what
    /// the seek itself takes is closed on the next period.
    fn realign(&mut self, to_ms: f64, settle_ms: u64) {
        self.seek(to_ms.max(0.0) as u64);
        self.resampler.reset();
        self.rate_target = 0.0;
        self.controller.reset();
        self.sync_fine = true;
        self.settle_until = Some(Instant::now() + Duration::from_millis(settle_ms));
    }

    /// Past the next `n` frames of the playing song (as many as are decoded): how many.
    fn skip_front(&mut self, n: usize) -> usize {
        let Some(front) = self.segs.front_mut() else {
            return 0;
        };
        let ch = usize::from(front.source.format.channels.max(1));
        let k = n.min(front.frames());
        front.samples.drain(..k * ch);
        front.played += k as u64;
        k
    }

    /// A period of silence (the device keeps its clock while the room hasn't started).
    fn write_silence(&mut self, want: usize) -> Result<(), String> {
        let (Some(fmt), Some(device)) = (self.dev_fmt.as_ref(), self.device.as_mut()) else {
            return Ok(());
        };
        self.bytes.clear();
        self.bytes.resize(want * fmt.bytes_per_frame(), 0);
        device.write(&self.bytes, want)?;
        if !self.started {
            device.start()?;
            self.started = true;
        }
        // Silence coming out (waiting for the room's start): a start or a seek from here is
        // from silence, to be exact, and needs no fade.
        self.sounding = false;
        Ok(())
    }

    fn sync_status(&self) -> SyncStatus {
        let ready_seg = |s: &Segment| {
            s.source.is_ready()
                && (s.ended || s.frames() as f64 >= f64::from(s.source.format.rate.max(1)) * 0.5)
        };
        SyncStatus {
            ready: self.seek_waiting.is_none() && self.segs.front().is_some_and(ready_seg),
            next_ready: self
                .segs
                .get(1)
                .is_some_and(|s| s.source.is_ready() && s.frames() > 0),
            error_ms: self.sync_error_ms,
        }
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
        // An online song still opening has no format yet (its placeholder's is made up): nothing
        // is said about the sound until the stream is read.
        if !seg.source.is_ready() {
            return None;
        }
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
            endpoint: None,
            kind: None,
            note: None,
            delay_ms: None,
            suggested_delay_ms: None,
            follows_default: matches!(self.choice, OutputChoice::Device { id: None, .. }),
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
        // Opening… is only said while it is meant to play: a paused online song (one put back
        // after a restart) waits quietly until play is pressed.
        let opening = self.segs.front().is_some_and(|s| !s.source.is_ready());
        // Partway through, everything that arrived played (or a seek past it): waiting for the
        // stream, not playing, though the device runs on in silence.
        let stalled = !opening
            && (self.seek_waiting.is_some()
                || self.suspended
                || self
                    .segs
                    .front()
                    .is_some_and(|s| !s.ended && s.frames() == 0 && s.source.is_waiting()));
        let buffering = self.playing && (opening || stalled);
        let stalled = self.playing && stalled;
        let status = Status {
            queue: self.queue_changed.then(|| self.queue.items().to_vec()),
            index: self.queue.index,
            playing: self.playing,
            buffering,
            stalled,
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
            sync: self.sync.map(|_| self.sync_status()),
            sync_notes: std::mem::take(&mut self.notes),
            ran_dry: self.ran_dry,
        };
        // The regular report, when it would only repeat the last one (paused, or nothing loaded),
        // is not sent: every window would otherwise redraw several times a second for nothing.
        // A command always gets its answer (`now`), and so does a new queue.
        let said = Said::of(&status);
        if !now
            && status.queue.is_none()
            && status.sync_notes.is_empty()
            && self.said.as_ref() == Some(&said)
        {
            return;
        }
        self.said = Some(said);
        self.queue_changed = false;
        (self.on_status)(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A song that counts how many times the engine opened it.
    #[derive(Clone, Serialize)]
    struct Counted {
        path: PathBuf,
        #[serde(skip)]
        opens: Arc<AtomicUsize>,
    }

    impl Item for Counted {
        fn path(&self) -> Option<&Path> {
            Some(&self.path)
        }
        fn open(&self) -> Result<Source, String> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Source::open(&self.path)
        }
    }

    /// A silent 16-bit stereo WAV of `frames` frames.
    fn wav(path: &Path, frames: usize) {
        std::fs::write(path, wav_data(frames)).unwrap();
    }

    /// A silent 16-bit stereo WAV of `frames` frames, as bytes.
    fn wav_data(frames: usize) -> Vec<u8> {
        let data_len = frames * 4;
        let mut v = Vec::with_capacity(44 + data_len);
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&44_100u32.to_le_bytes());
        v.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data_len as u32).to_le_bytes());
        v.resize(44 + data_len, 0);
        v
    }

    fn songs(n: usize) -> (PathBuf, Vec<Counted>) {
        // A folder of its own for each test (they run at once and name their files alike: the
        // clock alone can give two of them the same folder).
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = crate::test_support::temp_dir().join(format!(
            "venuta-engine-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let items = (0..n)
            .map(|i| {
                let path = dir.join(format!("{i}.wav"));
                wav(&path, 3 * 44_100);
                Counted {
                    path,
                    opens: Arc::new(AtomicUsize::new(0)),
                }
            })
            .collect();
        (dir, items)
    }

    fn engine(items: Vec<Counted>) -> Engine<Counted> {
        let captured = Arc::new(std::sync::Mutex::new(output::Captured::default()));
        let mut e = Engine::new(OutputChoice::Memory(captured), Box::new(|_| {}));
        e.handle(Cmd::Set {
            items,
            index: 0,
            start_ms: 0,
            play: false,
            from: String::new(),
        });
        e
    }

    #[test]
    fn next_plays_the_song_already_decoded_ahead_without_opening_it_again() {
        let (dir, items) = songs(3);
        let opens: Vec<_> = items.iter().map(|i| i.opens.clone()).collect();
        let mut e = engine(items);
        e.fill().unwrap();
        assert_eq!(
            opens[1].load(Ordering::SeqCst),
            1,
            "the next song was opened ahead"
        );
        e.handle(Cmd::Next);
        assert_eq!(e.queue.index, Some(1));
        assert_eq!(
            e.segs.len(),
            1,
            "the decoded-ahead song is now the one playing"
        );
        assert_eq!(
            opens[1].load(Ordering::SeqCst),
            1,
            "and it was not opened a second time"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn jumping_to_the_song_decoded_ahead_reuses_it_and_any_other_opens_fresh() {
        let (dir, items) = songs(3);
        let opens: Vec<_> = items.iter().map(|i| i.opens.clone()).collect();
        let mut e = engine(items.clone());
        e.fill().unwrap();
        e.handle(Cmd::Jump(1));
        assert_eq!(opens[1].load(Ordering::SeqCst), 1, "song 2 reused");

        let mut e = engine(items);
        e.fill().unwrap();
        e.handle(Cmd::Jump(2));
        assert_eq!(e.queue.index, Some(2));
        assert_eq!(
            opens[2].load(Ordering::SeqCst),
            1,
            "song 3 opened once, now"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// A queue of an online song (from a host on this computer) and local files.
    #[derive(Clone, Serialize)]
    enum Song {
        Streamed {
            url: String,
            len: u64,
            #[serde(skip)]
            stall_ms: u64,
        },
        File(PathBuf),
    }

    impl Item for Song {
        fn path(&self) -> Option<&Path> {
            match self {
                Song::File(p) => Some(p),
                Song::Streamed { .. } => None,
            }
        }
        fn open(&self) -> Result<Source, String> {
            match self {
                Song::File(p) => Source::open(p),
                Song::Streamed { url, len, stall_ms } => Source::open_remote_watched(
                    media_access::EphemeralAudio {
                        track_id: "s".into(),
                        provider: "test".into(),
                        url: url.clone(),
                        mime_type: Some("audio/wav".into()),
                        content_length: Some(*len),
                        duration_ms: Some(*len * 1000 / (44_100 * 4)),
                        headers: Vec::new(),
                        via: "test",
                        loudness_db: None,
                    },
                    None,
                    crate::decode::mock_host::quick_watch(*stall_ms),
                    crate::decode::mock_host::log().0,
                ),
            }
        }
        fn label(&self) -> String {
            match self {
                Song::File(p) => p.display().to_string(),
                Song::Streamed { .. } => "The streamed song".into(),
            }
        }
        fn reopen_when_stopped(&self) -> bool {
            false
        }
    }

    /// A 16-bit stereo 44.1 kHz WAV of `secs` seconds, as bytes (a quiet ramp, never silent).
    /// Made in memory: no folder for tests running at once to share (two of them making the
    /// same one and one removing it under the other made them fail now and then).
    fn wav_bytes(secs: usize) -> Vec<u8> {
        let mut v = wav_data(secs * 44_100);
        for (i, s) in v[44..].as_chunks_mut::<2>().0.iter_mut().enumerate() {
            *s = ((((i / 2) % 400) as i16 - 200) * 8).to_le_bytes();
        }
        v
    }

    /// Plays `items` at a device's pace on the engine `kind`, and hands back each status as it
    /// comes.
    fn play(
        kind: EngineKind,
        items: Vec<Song>,
    ) -> (Player<Song>, std::sync::mpsc::Receiver<Status<Song>>) {
        let captured = Arc::new(std::sync::Mutex::new(output::Captured::default()));
        let (tx, rx) = channel::<Status<Song>>();
        let player = Player::spawn_with(kind, OutputChoice::PacedMemory(captured), move |s| {
            let _ = tx.send(s);
        });
        player.send(Cmd::Set {
            items,
            index: 0,
            start_ms: 0,
            play: true,
            from: String::new(),
        });
        (player, rx)
    }

    /// Each stream test runs on both engines: `name::engine_1`, `name::engine_2`.
    macro_rules! on_both {
        ($($name:ident),*) => {
            $(
                mod $name {
                    #[test]
                    fn engine_1() {
                        super::$name(super::EngineKind::One)
                    }
                    #[test]
                    fn engine_2() {
                        super::$name(super::EngineKind::Two)
                    }
                }
            )*
        };
    }

    on_both!(
        a_stream_waiting_for_bytes_partway_says_so_and_plays_on_when_they_come,
        a_stream_that_cannot_be_picked_up_again_says_so_and_moves_on,
        a_stream_that_cant_keep_up_fades_out_and_back_in_never_cuts
    );

    fn a_stream_waiting_for_bytes_partway_says_so_and_plays_on_when_they_come(kind: EngineKind) {
        use crate::decode::mock_host::{Serve, host};
        let song = Arc::new(wav_bytes(3));
        // About 0.6 s arrives, then the host stalls; asked again, it sends the rest.
        let host = host(song.clone(), |n, _| {
            if n == 0 {
                Serve::ThenStall(100 * 1024)
            } else {
                Serve::Full
            }
        });
        let (_player, rx) = play(
            kind,
            vec![Song::Streamed {
                url: host.url.clone(),
                len: song.len() as u64,
                stall_ms: 1_500,
            }],
        );
        let started = Instant::now();
        let (mut waited_at, mut moved_on) = (None, false);
        while started.elapsed() < Duration::from_secs(15) {
            let Ok(s) = rx.recv_timeout(Duration::from_secs(5)) else {
                break;
            };
            assert!(s.error.is_empty(), "no error: {}", s.error);
            if s.playing && s.position_ms > 0 && s.buffering {
                assert!(s.stalled, "waiting partway, not opening");
                waited_at.get_or_insert(s.position_ms);
            }
            if let Some(at) = waited_at
                && !s.buffering
                && s.position_ms > at + 500
            {
                moved_on = true;
                break;
            }
        }
        let at = waited_at.expect("the player said it was waiting for the stream");
        assert!(at < 1_000, "it waited where the bytes ran out, at {at} ms");
        assert!(moved_on, "and played on once they came");
    }

    fn a_stream_that_cannot_be_picked_up_again_says_so_and_moves_on(kind: EngineKind) {
        use crate::decode::mock_host::{Serve, host};
        let song = Arc::new(wav_bytes(3));
        let host = host(song.clone(), |n, _| {
            Serve::ThenStall(if n == 0 { 100 * 1024 } else { 0 })
        });
        let (dir, files) = songs(1);
        let (_player, rx) = play(
            kind,
            vec![
                Song::Streamed {
                    url: host.url.clone(),
                    len: song.len() as u64,
                    stall_ms: 600,
                },
                Song::File(files[0].path.clone()),
            ],
        );
        let started = Instant::now();
        let (mut said, mut moved_on) = (None, false);
        while started.elapsed() < Duration::from_secs(15) {
            let Ok(s) = rx.recv_timeout(Duration::from_secs(5)) else {
                break;
            };
            if !s.error.is_empty() {
                said.get_or_insert(s.error.clone());
            }
            if said.is_some() && s.index == Some(1) && s.playing && s.position_ms > 0 {
                moved_on = true;
                break;
            }
        }
        let said = said.expect("the player said the stream was lost");
        assert!(moved_on, "and went on to the next song");
        assert!(said.contains("The streamed song"), "{said}");
        assert!(said.contains(crate::decode::STREAM_LOST), "{said}");
        std::fs::remove_dir_all(dir).ok();
    }

    fn a_stream_that_cant_keep_up_fades_out_and_back_in_never_cuts(kind: EngineKind) {
        use crate::decode::mock_host::{Serve, host};
        // A 440 Hz sine (16-bit stereo, 44.1 kHz), served at about three quarters of the pace it
        // plays at: it runs out again and again.
        let mut song = wav_bytes(6);
        for (i, s) in song[44..].as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let t = (i / 2) as f64 / 44_100.0;
            *s = ((8_000.0 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as i16).to_le_bytes();
        }
        let song = Arc::new(song);
        let len = song.len() as u64;
        let host = host(song, |_, _| Serve::Paced {
            first: 96 * 1024,
            piece: 8 * 1024,
            every_ms: 60,
        });
        let captured = Arc::new(std::sync::Mutex::new(output::Captured::default()));
        let (tx, rx) = channel::<Status<Song>>();
        let player = Player::spawn_with(
            kind,
            OutputChoice::PacedMemory(captured.clone()),
            move |s| {
                let _ = tx.send(s);
            },
        );
        player.send(Cmd::Set {
            items: vec![Song::Streamed {
                url: host.url.clone(),
                len,
                stall_ms: 30_000,
            }],
            index: 0,
            start_ms: 0,
            play: true,
            from: String::new(),
        });
        let (mut waited, t0) = (false, Instant::now());
        while t0.elapsed() < Duration::from_secs(5) {
            if let Ok(s) = rx.recv_timeout(Duration::from_millis(200)) {
                waited |= s.playing && s.position_ms > 500 && s.buffering;
            }
        }
        drop(player);
        let c = std::mem::take(&mut *captured.lock().unwrap());
        // Left channel, 16-bit values.
        let x: Vec<i32> = c
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .step_by(2)
            .map(|b| i32::from_le_bytes(*b) >> 16)
            .collect();
        let first = x.iter().position(|&v| v != 0).expect("it played");
        let last = x.iter().rposition(|&v| v != 0).unwrap();
        // The sine's own largest step from one frame to the next, and gaps inside the music.
        let step = 8_000.0 * 2.0 * std::f64::consts::PI * 440.0 / 44_100.0;
        let (mut gaps, mut hard) = (0, 0);
        let mut i = first;
        while i <= last {
            if x[i] == 0 && x.get(i + 1) == Some(&0) {
                let start = i;
                while i <= last && x[i] == 0 {
                    i += 1;
                }
                gaps += 1;
                let edge = f64::from(x[start - 1].abs().max(x[i].abs()));
                if edge > step * 0.5 {
                    hard += 1;
                }
                continue;
            }
            i += 1;
        }
        assert!(gaps > 0, "it did run out (or the test proves nothing)");
        assert!(waited, "and said it was buffering meanwhile");
        assert_eq!(hard, 0, "every wait faded out and back in ({gaps} waits)");
    }
}
