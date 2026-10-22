//! Engine 2's control thread (sections 3.2 and 4.6 of the Listen together design, its WP4
//! notes in 9.2): the command loop, the output's life, voices, and render commands at device
//! frames. It never waits for audio: a fade is a command at a frame, and what follows it is done
//! when the renderer's note comes back. It polls its ends of the render queues with `try_recv`
//! and sends with `try_send` only (a thread blocked on them would make the renderer's side take
//! a lock to wake it).
//!
//! `Deck` is the engine's part every program uses: the output (opened, started with a new
//! renderer, stopped once quiet, closed), the voices given to the renderer and those on their
//! way out, the markers that map device frames to places, the volume and the sound tools. The
//! programs (`program.rs`) decide what plays.

use super::memory::{Driver, MemoryOutput, Pacer};
use super::program::{Program, RoomMsg};
use super::status::{self, Shared, Tap};
use super::{Cmd, Item, OutputChoice, Status, Watch};
use crate::decode::{Download, SourceFormat, Streams};
use crate::dsp::{Chain, Gain, Sound};
use crate::output::{DeviceFormat, Mode};
use crate::output_api::{OpenRequest, Output, OutputError, OutputEvent, OutputInfo, Render};
use crate::render::{RenderSetup, RenderSnapshot, RenderStatus, Renderer, VoiceState};
use crate::voice::{Pace, Phase, Voice, VoiceSpec, VoiceStatus};
use crate::voice_api::{
    Marker, RateTarget, RenderCmd, RenderLink, RenderNote, Returned, Splice, VoiceId, render_link,
};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TrySendError, channel};
use std::thread::JoinHandle;
use together::time::{MonoClock, RealClock, SimClock, Timebase};

/// How often the control thread looks while anything is under way, and otherwise, ns.
const BUSY_NS: u64 = 5_000_000;
const IDLE_NS: u64 = 100_000_000;
/// A sound device is let go after this long paused, so other apps can use it, ns.
const RELEASE_NS: u64 = 10_000_000_000;
/// A lost output is opened again after these waits, ns; then the player pauses.
const RETRY_NS: [u64; 4] = [500_000_000, 1_000_000_000, 2_000_000_000, 5_000_000_000];
/// Markers kept per voice.
const MARKS: usize = 256;
/// Decoded ahead for a solo start, s.
const READY_S: f64 = 0.5;
/// A change of the sound tools glides over this long, ms.
pub(super) const GLIDE_MS: u32 = 50;

/// What the control thread is told: your commands and a room's calls, in one queue, in the
/// order they were made.
pub(crate) enum Msg<T> {
    Cmd(Cmd<T>),
    Room(RoomMsg<T>),
}

pub(super) struct Spawned<T> {
    pub msgs: Sender<Msg<T>>,
    pub thread: JoinHandle<()>,
    /// Every song's download, shared by every source of it.
    pub streams: Arc<Streams>,
}

/// Start engine 2: its control thread, its status thread, and (for the fast memory output) the
/// driver of its simulated clock.
pub(super) fn spawn<T: Item + Serialize>(
    output: OutputChoice,
    on_status: Box<dyn FnMut(Status<T>) + Send>,
    watch: Watch,
) -> Spawned<T> {
    let (msg_tx, msg_rx) = channel();
    let (wake_tx, wake_rx) = channel();
    // The clock everything waits on; sleepers made here, before the threads start.
    let (tb, own_clock): (Arc<dyn Timebase>, Option<SimClock>) = match &output {
        OutputChoice::Sim(s) => (Arc::new(s.clock.clone()), None),
        OutputChoice::Memory(_) => {
            let c = SimClock::new(RealClock.now_ns());
            (Arc::new(c.clone()), Some(c))
        }
        _ => (Arc::new(RealClock), None),
    };
    let virtual_time = matches!(output, OutputChoice::Sim(_) | OutputChoice::Memory(_));
    let pacer = |tb: &Arc<dyn Timebase>| {
        if virtual_time {
            Pacer::Virtual(tb.sleeper())
        } else {
            Pacer::Real
        }
    };
    let (control_pacer, status_pacer) = (pacer(&tb), pacer(&tb));
    let driver = own_clock.map(Driver::start);
    let shared = Arc::new(Shared::new());
    let streams = Arc::new(Streams::new());
    let deck_streams = streams.clone();
    let status_thread = {
        let shared = shared.clone();
        std::thread::Builder::new()
            .name("venuta-status".into())
            .spawn(move || status::run(shared, status_pacer, wake_rx, on_status))
            .expect("start the status thread")
    };
    let pace = if virtual_time {
        Pace::Clock(tb.clone())
    } else {
        Pace::Real
    };
    let thread = std::thread::Builder::new()
        .name("venuta-player".into())
        .spawn(move || {
            let deck = Deck::new(
                tb,
                pace,
                output,
                shared.common.clone(),
                watch,
                deck_streams,
            );
            let mut control = Control {
                shared: shared.clone(),
                pacer: control_pacer,
                msgs: msg_rx,
                wake: wake_tx,
                program: Program::new(),
                deck,
            };
            control.run();
            // Nothing of the engine waits on the clock after this: the status thread is told
            // to go, this thread stops counting as one of the clock's (its sleeper goes), and
            // only then is it waited for.
            let Control { pacer, deck, .. } = control;
            drop(deck);
            shared.common.quit.store(true, Ordering::Release);
            drop(pacer);
            let _ = status_thread.join();
            if let Some(d) = driver {
                d.finish();
            }
        })
        .expect("start the player thread");
    Spawned {
        msgs: msg_tx,
        thread,
        streams,
    }
}

struct Control<T: Item + Serialize> {
    shared: Arc<Shared<T>>,
    pacer: Pacer,
    msgs: Receiver<Msg<T>>,
    wake: Sender<()>,
    program: Program<T>,
    deck: Deck,
}

impl<T: Item + Serialize> Control<T> {
    fn run(&mut self) {
        let mut quitting = false;
        loop {
            let busy = self.program.busy() || self.deck.busy();
            let until = self.pacer.now() + if busy { BUSY_NS } else { IDLE_NS };
            match self.pacer.recv_until(&self.msgs, until) {
                Ok(m) => {
                    quitting |= self.take(m);
                    // Every message waiting now, in order, each with its answer.
                    while let Ok(m) = self.msgs.try_recv() {
                        quitting |= self.take(m);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => quitting |= self.command(Cmd::Quit),
            }
            self.turn();
            self.publish();
            if quitting && self.deck.quit_done() {
                break;
            }
        }
    }

    /// A command or a room's call, answered. True for Quit.
    fn take(&mut self, m: Msg<T>) -> bool {
        let quit = match m {
            Msg::Cmd(c) => self.command(c),
            Msg::Room(r) => {
                self.program.room(r, &mut self.deck);
                false
            }
        };
        self.answer();
        quit
    }

    /// True for Quit.
    fn command(&mut self, cmd: Cmd<T>) -> bool {
        match cmd {
            Cmd::Volume(v) => self.deck.set_volume(v),
            Cmd::Sound(s) => self.deck.set_sound(s),
            Cmd::SleepIn(d) => {
                self.deck.sleep_at = d.map(|d| self.deck.now_ns() + d.as_nanos() as u64);
            }
            Cmd::Spectrum(on) => self.deck.set_spectrum(on),
            Cmd::Output(choice) => {
                self.deck.switch_output(choice);
                self.program.hush(&mut self.deck);
            }
            Cmd::Quit => {
                self.program.quit(&mut self.deck);
                self.deck.quitting = true;
                return true;
            }
            // A protocol 2 room (WP5's adapter).
            Cmd::Sync(plan) => self.program.sync(plan, &mut self.deck),
            other => self.program.command(other, &mut self.deck),
        }
        false
    }

    /// One turn: what came back from the renderer and the output, then the program, then the
    /// output's life.
    fn turn(&mut self) {
        let notes = self.deck.poll();
        for n in &notes {
            self.program.note(n, &mut self.deck);
        }
        if self.deck.sleep_due() {
            self.deck.sleep_at = None;
            self.program.sleep_ended(&mut self.deck);
        }
        self.deck.sleep_gain();
        self.program.step(&mut self.deck);
        let wanted = self.program.wants_output();
        if let Some(ev) = self.deck.step(wanted) {
            self.program.stopped(ev, &mut self.deck);
            self.program.step(&mut self.deck);
        }
        self.deck.flush();
    }

    /// What the status thread reports when it next looks.
    fn publish(&mut self) {
        let notes = self.program.take_notes();
        if !notes.is_empty() {
            self.shared.sync_notes().extend(notes);
        }
        let mut v = self.shared.view();
        self.program.view(&self.deck, &mut v);
        self.deck.fill_view(&mut v);
    }

    /// A command's answer: the view as it left it, reported at once.
    fn answer(&mut self) {
        self.publish();
        let answer = {
            let mut v = self.shared.view();
            let a = v.clone();
            // The queue goes with the answer only.
            v.queue = None;
            v.unshuffled = None;
            a
        };
        self.shared.answers().push(answer);
        let _ = self.wake.send(());
    }
}

// --- Places and voices ---------------------------------------------------------------------------

/// Where a voice starts: a content frame, or (its rate not known yet) a time, which the voice's
/// opener seeks to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    Frame(u64),
    Us(u64),
}

/// One voice of a program: a song from a place, decoded ahead on its own thread.
pub(super) struct Lane {
    pub vid: VoiceId,
    voice: Voice,
    pub place: Place,
    /// A stream's download, kept while this voice opens on it.
    _hold: Option<Arc<Download>>,
    /// The renderer start it was given to.
    pub given: Option<u64>,
    pub label: String,
    pub gain: Gain,
    pub album: Option<String>,
    /// The song's rate when known before its voice opens (a new voice of a song playing).
    pub rate_hint: u32,
}

impl Lane {
    /// The song's rate: its own once open, the hint before.
    pub(super) fn rate(&self) -> Option<u32> {
        self.status()
            .format
            .map(|f| f.rate)
            .or((self.rate_hint > 0).then_some(self.rate_hint))
    }

    pub(super) fn status(&self) -> VoiceStatus {
        self.voice.status()
    }

    /// The content frame it starts at (once its rate is known).
    pub(super) fn start(&self) -> Option<u64> {
        self.start_of(&self.status())
    }

    fn start_of(&self, s: &VoiceStatus) -> Option<u64> {
        match self.place {
            Place::Frame(f) => Some(f),
            Place::Us(us) => {
                let rate = u64::from(s.format.as_ref()?.rate.max(1));
                // As the opener sought: a stream in whole ms (`Source::seek`), a file to the
                // frame.
                Some(if s.stream.is_some() {
                    (us / 1000) * rate / 1000
                } else {
                    us * rate / 1_000_000
                })
            }
        }
    }

    /// Ready to start, solo: its ring full, its end decoded, or half a second in hand (a stream
    /// that falls behind later fades out and back by the guard). A voice that failed is ready
    /// to be found failed.
    pub(super) fn ready(&self) -> bool {
        let s = self.status();
        if matches!(s.phase, Phase::Failed | Phase::Stopped) {
            return true;
        }
        let Some(f) = s.format.as_ref() else {
            return false;
        };
        let need = f64::from(f.rate) * READY_S;
        matches!(s.phase, Phase::Full | Phase::Ended) || s.in_ring as f64 >= need
    }

    /// It can't play: why.
    pub(super) fn failed(&self) -> Option<String> {
        let s = self.status();
        match s.phase {
            Phase::Failed if s.format.is_none() || !s.tap_taken => {
                Some(s.error.unwrap_or_else(|| "It could not be played.".into()))
            }
            Phase::Stopped if !s.tap_taken => Some("It could not be played.".into()),
            _ => None,
        }
    }

    /// Why its stream stopped short, once its end is played.
    pub(super) fn error(&self) -> Option<String> {
        self.status().error
    }

    /// The loudness of the blocks it decoded that end after content frame `from`.
    pub(super) fn loudness(&self, from: u64) -> Vec<crate::voice::Loudness> {
        self.voice.loudness(from)
    }
}

/// A voice on its way out: dropped once its tap is back, or the renderer it was in goes.
struct Going {
    lane: Lane,
    /// `Drop` sent (else it waits for its fade's `Silent` or its `Ended` first).
    dropped: bool,
}

/// A voice the renderer has: what its chain is made of, to make it again when the tools change.
struct Live {
    vid: VoiceId,
    channels: u16,
    gain: Gain,
    loudness: Option<f64>,
    /// The chain it has changes the sound.
    touching: bool,
}

/// What happened to the output that the program must know.
pub(super) struct Stopped {
    /// The renderer start that ended: its voices are gone.
    pub generation: u64,
    /// It was lost (or the default output changed): the device frame heard last, for the place
    /// to come back at.
    pub lost_at: Option<u64>,
}

/// The output that is open.
struct Open {
    dev: Dev,
    info: OutputInfo,
    request: OpenRequest,
    mode: Mode,
    name: String,
    events: Receiver<OutputEvent>,
    presentation: Arc<crate::output_api::Presentation>,
}

/// A started output's renderer.
struct Running {
    generation: u64,
    link: RenderLink,
    status: Arc<RenderStatus>,
    /// The trim the sync tick asks of this renderer (a room's only; 0 otherwise).
    rate: Arc<RateTarget>,
    /// Frames before the first render call: none rendered yet.
    fresh: bool,
}

enum Dev {
    #[cfg(windows)]
    Wasapi(Box<crate::wasapi_out::WasapiOutput>),
    Other(Box<dyn Output>),
}

impl Dev {
    fn out(&mut self) -> &mut dyn Output {
        match self {
            #[cfg(windows)]
            Dev::Wasapi(w) => w.as_mut(),
            Dev::Other(o) => o.as_mut(),
        }
    }

    /// The latency the output learned while running (WP1d).
    fn latency(&self) -> Option<crate::output_api::Latency> {
        match self {
            #[cfg(windows)]
            Dev::Wasapi(w) => w.latency(),
            Dev::Other(o) => o.latency(),
        }
    }

    fn endpoint(&self) -> Option<String> {
        match self {
            #[cfg(windows)]
            Dev::Wasapi(w) => w.notes().and_then(|n| n.endpoint_id),
            Dev::Other(_) => None,
        }
    }
}

/// The render path handed to an output: the renderer, then the spectrum's ring.
struct Tapped {
    renderer: Renderer,
    tap: Arc<Tap>,
    format: DeviceFormat,
}

impl Render for Tapped {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        self.renderer.render(out, frames, at);
        self.tap.write(out, frames as usize, &self.format);
    }
}

// --- The deck --------------------------------------------------------------------------------------

/// How a voice leaves.
pub(super) enum Retire {
    /// `Drop` now (one still sounding fades out over 30 ms first).
    Now,
    /// Its fade is under way (a stop, a crossfade, its song's end): `Drop` once it is silent.
    AfterFade,
}

pub(super) struct Deck {
    tb: Arc<dyn Timebase>,
    mono: Arc<dyn MonoClock>,
    pace: Pace,
    choice: OutputChoice,
    common: Arc<status::Common>,
    watch: Watch,
    streams: Arc<Streams>,
    next_vid: u32,
    open: Option<Open>,
    run: Option<Running>,
    generation: u64,
    /// Frames rendered by every start before the one running (for the watched notes).
    offset: u64,
    /// Commands not yet in the renderer's queue (and, for a new renderer, the start that waits
    /// for them).
    pending: VecDeque<RenderCmd>,
    unstarted: Option<Box<dyn Render>>,
    going: Vec<Going>,
    /// Voices the last notes said fell silent or ended: one retired after its fade in the
    /// same turn is dropped at once (its note is already read).
    silenced: Vec<VoiceId>,
    live: Vec<Live>,
    marks: Vec<(VoiceId, VecDeque<Marker>)>,
    /// No fade commanded ends after this device frame.
    quiet_from: u64,
    /// The output is to be stopped and closed, then opened again for another format.
    reopen: bool,
    /// The running renderer is not to be given anything more: stopped once quiet, the next
    /// start gets a fresh one (a room let go of the player).
    renew: bool,
    /// Another output chosen: the open one is closed once quiet.
    switching: Option<OutputChoice>,
    /// Open and not running since (for letting a sound device go); its tail played out by.
    idle_since: Option<u64>,
    drained_at: u64,
    /// A lost output: tries so far, and when the next one is.
    retry: Option<(usize, u64)>,
    /// Every try to open it failed: the program pauses, with the error.
    gave_up: Option<String>,
    event: Option<Stopped>,
    pub(super) sound: Sound,
    pub(super) volume: f32,
    pub(super) sleep_at: Option<u64>,
    spectrum: bool,
    /// The master gain the renderer was last told, and the frame its first ramp ends.
    gain_sent: f32,
    gain_ready_at: u64,
    /// The output's endpoint (the key of its remembered delay, WP19).
    pub(super) endpoint: Option<String>,
    pub(super) quitting: bool,
    snapshot: Option<RenderSnapshot>,
    /// How far what was handed over runs ahead of the DAC, as the presentation pairs say (the
    /// output's real latency: `OutputInfo::buffer_frames` can say far less).
    lead: Option<u64>,
}

/// What the output is asked to open for a song.
fn request(f: &SourceFormat) -> OpenRequest {
    OpenRequest {
        rate: f.rate,
        channels: f.channels,
        bits: f.bits,
    }
}

impl Deck {
    fn new(
        tb: Arc<dyn Timebase>,
        pace: Pace,
        choice: OutputChoice,
        common: Arc<status::Common>,
        watch: Watch,
        streams: Arc<Streams>,
    ) -> Deck {
        let mono: Arc<dyn MonoClock> = tb.clone();
        Deck {
            tb,
            mono,
            pace,
            choice,
            common,
            watch,
            streams,
            next_vid: 1,
            open: None,
            run: None,
            generation: 0,
            offset: 0,
            pending: VecDeque::new(),
            unstarted: None,
            going: Vec::new(),
            silenced: Vec::new(),
            live: Vec::new(),
            marks: Vec::new(),
            quiet_from: 0,
            reopen: false,
            renew: false,
            switching: None,
            idle_since: None,
            drained_at: 0,
            retry: None,
            gave_up: None,
            event: None,
            sound: Sound::default(),
            volume: 1.0,
            sleep_at: None,
            spectrum: false,
            gain_sent: 1.0,
            gain_ready_at: 0,
            endpoint: None,
            quitting: false,
            snapshot: None,
            lead: None,
        }
    }

    pub(super) fn now_ns(&self) -> u64 {
        self.tb.now_ns()
    }

    /// Anything of the output under way (the control thread looks every 5 ms meanwhile).
    fn busy(&self) -> bool {
        self.run.is_some()
            || !self.going.is_empty()
            || !self.pending.is_empty()
            || self.retry.is_some()
            || self.reopen
            || self.renew
            || self.switching.is_some()
    }

    fn quit_done(&self) -> bool {
        self.run.is_none() && self.open.is_none()
    }

    // --- Voices ---

    /// A new voice of `item` from `place`: its decode thread opens it (on the shared downloads)
    /// and decodes ahead; nothing waits here.
    pub(super) fn lane<T: Item>(&mut self, item: &T, place: Place) -> Lane {
        self.lane_with(item, place, None)
    }

    /// A new voice decoding `depth` ahead (`None`: the voice's own, 2 s for a file and 3 s for a
    /// stream).
    pub(super) fn lane_with<T: Item>(
        &mut self,
        item: &T,
        place: Place,
        depth: Option<std::time::Duration>,
    ) -> Lane {
        let vid = VoiceId(self.next_vid);
        self.next_vid = self.next_vid.wrapping_add(1).max(1);
        let hold = item.stream_key().and_then(|k| self.streams.hold(&k));
        let streams = self.streams.clone();
        let it = item.clone();
        let open: crate::voice::Opener = Box::new(move || {
            let mut s = it.open_on(&streams)?;
            if let Place::Us(us) = place
                && us > 0
            {
                if s.is_stream() && !s.is_open() {
                    // Applied in whole ms once the stream opens (its rate isn't known before).
                    s.seek(us / 1000)?;
                } else {
                    let rate = u64::from(s.format.rate.max(1));
                    s.seek_frame(us * rate / 1_000_000)?;
                }
            }
            Ok(s)
        });
        let start = match place {
            Place::Frame(f) => f,
            Place::Us(_) => 0,
        };
        let voice = Voice::start(VoiceSpec {
            id: vid,
            open,
            start,
            depth,
            pace: self.pace.clone(),
        });
        Lane {
            vid,
            voice,
            place,
            _hold: hold,
            given: None,
            label: item.label(),
            gain: item.gain(),
            album: item.album(),
            rate_hint: 0,
        }
    }

    /// The renderer start running now.
    pub(super) fn generation(&self) -> Option<u64> {
        self.run.as_ref().map(|r| r.generation)
    }

    /// Device frames rendered so far in this start (the next call's first frame).
    pub(super) fn frames(&self) -> u64 {
        self.snapshot.as_ref().map_or(0, |s| s.frames)
    }

    /// The output's rate (0 when none is open).
    pub(super) fn rate(&self) -> u32 {
        self.open.as_ref().map_or(0, |o| o.info.format.rate)
    }

    /// `ms` in device frames.
    pub(super) fn ms(&self, ms: u32) -> u64 {
        u64::from(self.rate()) * u64::from(ms) / 1000
    }

    /// The output's lead (frames handed over and not yet at the DAC): the latency the output
    /// learned (WP1d), else as measured here from the presentation, else the reported buffer
    /// until the device has said.
    fn lead_frames(&self) -> u64 {
        if let Some(l) = self.open.as_ref().and_then(|o| o.dev.latency()) {
            return u64::from(l.frames);
        }
        self.lead.unwrap_or_else(|| {
            self.open
                .as_ref()
                .map_or(0, |o| u64::from(o.info.buffer_frames))
        })
    }

    fn period(&self) -> u64 {
        self.open
            .as_ref()
            .map_or(0, |o| u64::from(o.info.period_frames))
    }

    /// The first frame a solo command can be at: the next frames to render plus two periods,
    /// or, on a renderer that hasn't rendered yet, its very first frame (once its gain is
    /// set).
    pub(super) fn soon(&self) -> u64 {
        match &self.run {
            Some(r) if r.fresh => self.gain_ready_at,
            Some(_) => self.frames() + 2 * self.period(),
            None => 0,
        }
    }

    /// `soon`, and not before every fade under way has ended (a start from silence).
    pub(super) fn soon_quiet(&self) -> u64 {
        self.soon().max(self.quiet_from)
    }

    /// The output can play `f` as it is open: shared mode converts any format (D2); the
    /// others play only the one they were opened for.
    pub(super) fn fits(&self, f: &SourceFormat) -> bool {
        self.open
            .as_ref()
            .is_some_and(|o| o.mode == Mode::Shared || o.request == request(f))
    }

    /// A renderer that can play `f` is running (commands go to it): opened and started now if
    /// need be. False while that can't be yet: another format's output to stop first, a lost
    /// output waiting for its next try.
    pub(super) fn ready_for(&mut self, f: &SourceFormat) -> bool {
        if self.run.is_some() {
            if self.fits(f) && !self.reopen && !self.renew && self.switching.is_none() {
                return true;
            }
            self.reopen |= !self.fits(f);
            return false;
        }
        if self.switching.is_some() || self.gave_up.is_some() {
            return false;
        }
        let now = self.now_ns();
        if self.retry.is_some_and(|(_, at)| now < at) {
            return false;
        }
        if self.open.is_some() && !self.fits(f) {
            self.reopen = true;
            return false;
        }
        if self.open.is_none() {
            match self.open_output(&request(f)) {
                Ok(()) => self.retry = None,
                Err(e) => {
                    self.failed_open(e);
                    return false;
                }
            }
        }
        self.start_renderer();
        self.run.is_some()
    }

    /// An open failed: tried again 0.5, 1, 2 and 5 s later after a lost output, then given up;
    /// any other failure is said at once.
    fn failed_open(&mut self, e: OutputError) {
        let now = self.now_ns();
        match self.retry {
            Some((k, _)) if k + 1 < RETRY_NS.len() => {
                self.retry = Some((k + 1, now + RETRY_NS[k + 1]));
            }
            _ => {
                self.retry = None;
                self.gave_up = Some(e.to_string());
            }
        }
    }

    /// Every try to open the output failed: said once, to the program.
    pub(super) fn take_gave_up(&mut self) -> Option<String> {
        self.gave_up.take()
    }

    fn open_output(&mut self, want: &OpenRequest) -> Result<(), OutputError> {
        let mut dev = match &self.choice {
            #[cfg(windows)]
            OutputChoice::Device { id, exclusive } => Dev::Wasapi(Box::new(
                crate::wasapi_out::wasapi_output(id.as_deref(), *exclusive),
            )),
            #[cfg(not(windows))]
            OutputChoice::Device { .. } => {
                return Err(OutputError::Other(
                    "Playing to a sound device needs Windows for now.".into(),
                ));
            }
            OutputChoice::Memory(c) => Dev::Other(Box::new(MemoryOutput::new(
                self.tb.clone(),
                c.clone(),
                false,
            ))),
            OutputChoice::PacedMemory(c) => Dev::Other(Box::new(MemoryOutput::new(
                self.tb.clone(),
                c.clone(),
                true,
            ))),
            OutputChoice::Sim(s) => Dev::Other((s.output)()),
        };
        let info = dev.out().open(want)?;
        let events = dev.out().events();
        let presentation = dev.out().presentation();
        let mode = dev.out().mode();
        let name = dev.out().name();
        self.endpoint = dev.endpoint();
        self.lead = None;
        self.open = Some(Open {
            dev,
            info,
            request: want.clone(),
            mode,
            name,
            events,
            presentation,
        });
        self.idle_since = Some(self.now_ns());
        Ok(())
    }

    /// A new renderer for the open output; the output starts once the commands given before
    /// its first call are in its queue (`flush`), so the first of them is on time.
    fn start_renderer(&mut self) {
        let Some(o) = self.open.as_ref() else {
            return;
        };
        let (link, ends) = render_link(64, 1_024, 1_024);
        // A new renderer for every start, with a rate of its own at 0: no trim of a room's
        // reaches a start made after it.
        let rate = Arc::new(RateTarget::default());
        let (renderer, status) = Renderer::new(RenderSetup {
            format: o.info.format.clone(),
            max_period: o.info.period_frames,
            ends,
            rate: rate.clone(),
            clock: self.mono.clone(),
        });
        let format = o.info.format.clone();
        let mut render: Box<dyn Render> = Box::new(Tapped {
            renderer,
            tap: self.common.tap.clone(),
            format,
        });
        if let Some(w) = &self.watch.wrap {
            render = w(render);
        }
        self.generation += 1;
        self.pending.clear();
        // A renderer starts at gain 1: another volume ramps first, and the music begins once
        // the ramp is done, so its first frame already has the volume.
        let g = self.target_gain();
        self.gain_sent = g;
        self.gain_ready_at = 0;
        if (g - 1.0).abs() > 1e-6 {
            self.pending.push_back(RenderCmd::Gain { target: g });
            self.gain_ready_at = self.ms(crate::render::GAIN_MS);
        }
        self.unstarted = Some(render);
        self.common.renders().running = Some(status.clone());
        self.run = Some(Running {
            generation: self.generation,
            link,
            status,
            rate,
            fresh: true,
        });
        self.renew = false;
        self.snapshot = None;
        self.quiet_from = 0;
        self.idle_since = None;
    }

    /// Commands into the renderer's queue; a renderer waiting to start starts.
    fn flush(&mut self) {
        let Some(r) = self.run.as_ref() else {
            self.pending.clear();
            return;
        };
        while let Some(c) = self.pending.pop_front() {
            match r.link.commands.try_send(c) {
                Ok(()) => {}
                Err(TrySendError::Full(c)) => {
                    self.pending.push_front(c);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {}
            }
        }
        if let Some(render) = self.unstarted.take()
            && let Some(o) = self.open.as_mut()
            && let Err(e) = o.dev.out().start(render)
        {
            self.gave_up = Some(e.to_string());
            if let Some(r) = self.run.take() {
                self.end_run(r, None);
            }
        }
    }

    pub(super) fn send(&mut self, cmd: RenderCmd) {
        if let RenderCmd::StopAt { at, fade, .. } = &cmd {
            let end = (*at).max(self.frames()) + u64::from(*fade).max(self.ms(30));
            self.quiet_from = self.quiet_from.max(end);
        }
        self.pending.push_back(cmd);
    }

    /// Fade `vid` out over `fade_ms` from device frame `at`.
    pub(super) fn stop_at(&mut self, vid: VoiceId, at: u64, fade_ms: u32) {
        let fade = self.ms(fade_ms) as u32;
        self.send(RenderCmd::StopAt {
            voice: vid,
            at,
            fade,
        });
    }

    /// Give `lane` to the renderer, from silence at device frame `at`, fading in over `fade`
    /// frames. False when it can't be given (no tap: it failed).
    pub(super) fn start(&mut self, lane: &mut Lane, at: u64, fade: u32) -> bool {
        self.start_from(lane, at, None, fade)
    }

    /// `start`, with the content frame that is to be at `at` (`None`: the lane's start; a
    /// later one is reached by the renderer skipping what the voice decoded before it).
    pub(super) fn start_from(
        &mut self,
        lane: &mut Lane,
        at: u64,
        content: Option<u64>,
        fade: u32,
    ) -> bool {
        let s = lane.status();
        let (Some(fmt), Some(first)) = (s.format.clone(), lane.start_of(&s)) else {
            return false;
        };
        let content = content.unwrap_or(first).max(first);
        let Some(tap) = lane.voice.take_tap() else {
            return false;
        };
        self.send(RenderCmd::StartAt {
            voice: lane.vid,
            tap,
            at,
            content,
            rate: fmt.rate,
            fade_in: fade,
        });
        self.chain_new(lane.vid, &s, &lane.gain, fmt.channels);
        lane.given = self.generation();
        true
    }

    /// Move from `from` to `lane` at device frame `at`, shaped as `how`.
    pub(super) fn switch(&mut self, from: VoiceId, lane: &mut Lane, at: u64, how: Splice) -> bool {
        let s = lane.status();
        let (Some(fmt), Some(content)) = (s.format.clone(), lane.start_of(&s)) else {
            return false;
        };
        let Some(tap) = lane.voice.take_tap() else {
            return false;
        };
        let reach = match how {
            Splice::Crossfade { frames } => at + u64::from(frames),
            Splice::FadeOutThenIn { .. } | Splice::EndFade { .. } => at,
            Splice::Gapless => 0,
        };
        self.quiet_from = self.quiet_from.max(reach);
        self.send(RenderCmd::SwitchAt {
            from,
            to: lane.vid,
            tap,
            at,
            to_content: content,
            rate: fmt.rate,
            how,
        });
        self.chain_new(lane.vid, &s, &lane.gain, fmt.channels);
        lane.given = self.generation();
        true
    }

    /// A lane leaves: at once if the renderer never had it (or the start it was in is over),
    /// else once its tap is back.
    pub(super) fn retire(&mut self, lane: Lane, how: Retire) {
        if lane.given.is_none() || lane.given != self.generation() {
            self.forget(lane.vid);
            return;
        }
        let dropped = match how {
            Retire::Now => {
                let end = self.frames() + self.ms(30) + 2 * self.period();
                self.quiet_from = self.quiet_from.max(end);
                self.send(RenderCmd::Drop { voice: lane.vid });
                true
            }
            Retire::AfterFade if self.silenced.contains(&lane.vid) => {
                self.send(RenderCmd::Drop { voice: lane.vid });
                true
            }
            Retire::AfterFade => false,
        };
        self.going.push(Going { lane, dropped });
    }

    fn forget(&mut self, vid: VoiceId) {
        self.live.retain(|l| l.vid != vid);
        self.marks.retain(|(v, _)| *v != vid);
    }

    /// The sound tools for a voice just given: made for the device's rate and the voice's
    /// channels (they run after the resampler), sent only when they change anything.
    fn chain_new(&mut self, vid: VoiceId, s: &VoiceStatus, gain: &Gain, channels: u16) {
        let Some(o) = self.open.as_ref() else {
            return;
        };
        let (rate, bits) = (o.info.format.rate, o.info.format.valid_bits);
        let chain = Chain::new(
            &self.sound,
            &gain.or_loudness(s.loudness_db),
            rate,
            channels,
            bits,
        );
        let touching = chain.active;
        if touching {
            self.send(RenderCmd::Chain {
                voice: vid,
                chain: Box::new(chain),
            });
        }
        self.live.push(Live {
            vid,
            channels,
            gain: *gain,
            loudness: s.loudness_db,
            touching,
        });
    }

    /// New sound tools: every voice the renderer has glides to its new chain.
    pub(super) fn set_sound(&mut self, sound: Sound) {
        self.sound = sound;
        let Some(o) = self.open.as_ref() else {
            return;
        };
        let (rate, bits) = (o.info.format.rate, o.info.format.valid_bits);
        let glide = self.ms(GLIDE_MS) as u32;
        let mut cmds = Vec::new();
        for l in &mut self.live {
            let chain = Chain::new(
                &self.sound,
                &l.gain.or_loudness(l.loudness),
                rate,
                l.channels,
                bits,
            );
            let touching = chain.active;
            if touching || l.touching {
                cmds.push(RenderCmd::Chain {
                    voice: l.vid,
                    chain: Box::new(chain.gliding(glide)),
                });
            }
            l.touching = touching;
        }
        if self.run.is_some() {
            self.pending.extend(cmds);
        }
    }

    /// The signal path's steps for a song (what its tools do, in words), and whether any
    /// changes the sound.
    pub(super) fn steps(
        &self,
        gain: &Gain,
        loudness: Option<f64>,
        rate: u32,
        ch: u16,
    ) -> (Vec<String>, bool) {
        let bits = self.open.as_ref().map_or(24, |o| o.info.format.valid_bits);
        let c = Chain::new(&self.sound, &gain.or_loudness(loudness), rate, ch, bits);
        (c.steps, c.active)
    }

    // --- Volume, the sleep timer, the spectrum ---

    fn sleep_fade(&self) -> f32 {
        match self.sleep_at {
            Some(at) => {
                let left = at.saturating_sub(self.now_ns()) as f64 / 1e9;
                // In steps of 0.5%: a ramp each, about 50 ms apart over the last 10 s.
                ((left / 10.0).min(1.0) * 200.0).ceil() as f32 / 200.0
            }
            None => 1.0,
        }
    }

    fn target_gain(&self) -> f32 {
        self.volume * self.sleep_fade()
    }

    fn apply_gain(&mut self) {
        let g = self.target_gain();
        if self.run.is_some() && (g - self.gain_sent).abs() > 1e-6 {
            self.pending.push_back(RenderCmd::Gain { target: g });
            self.gain_sent = g;
        }
    }

    pub(super) fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.apply_gain();
    }

    fn sleep_gain(&mut self) {
        if self.sleep_at.is_some() {
            self.apply_gain();
        }
    }

    fn sleep_due(&self) -> bool {
        self.sleep_at.is_some_and(|t| self.now_ns() >= t)
    }

    pub(super) fn set_spectrum(&mut self, on: bool) {
        self.spectrum = on;
        self.common.tap.on.store(on, Ordering::Relaxed);
    }

    /// Another output: the program hushes, the open one is closed once quiet, and the next
    /// start opens the new one.
    pub(super) fn switch_output(&mut self, choice: OutputChoice) {
        if self.open.is_none() {
            self.choice = choice;
        } else {
            self.switching = Some(choice);
        }
    }

    // --- What comes back ---

    /// The output's events, the renderer's markers, notes and returned things.
    fn poll(&mut self) -> Vec<RenderNote> {
        let mut lost = false;
        if let Some(o) = &self.open {
            while let Ok(e) = o.events.try_recv() {
                match e {
                    OutputEvent::Xrun { .. } => {
                        self.common.ran_dry.fetch_add(1, Ordering::Relaxed);
                    }
                    OutputEvent::Lost => lost = true,
                    OutputEvent::DefaultChanged => {
                        lost |= matches!(self.choice, OutputChoice::Device { id: None, .. });
                    }
                }
            }
        }
        let mut notes = Vec::new();
        self.silenced.clear();
        if let Some(r) = &mut self.run {
            let snap = r.status.read();
            if snap.frames > 0 {
                r.fresh = false;
            }
            if let Some(o) = self.open.as_ref()
                && let Some(p) = o.presentation.read()
            {
                let rate = f64::from(o.info.format.rate.max(1));
                let since = self.tb.now_ns().saturating_sub(p.time_ns) as f64 / 1e9;
                let at_dac = p.frame + (since * rate) as u64;
                self.lead = Some(snap.frames.saturating_sub(at_dac));
            }
            self.snapshot = Some(snap);
            let mut marks = Vec::new();
            while let Ok(m) = r.link.markers.try_recv() {
                marks.push(m);
            }
            while let Ok(n) = r.link.notes.try_recv() {
                notes.push(n);
            }
            // Markers sent while the notes were read (a note never comes before the markers of
            // the calls before its own).
            while let Ok(m) = r.link.markers.try_recv() {
                marks.push(m);
            }
            let mut back = Vec::new();
            while let Ok(x) = r.link.returned.try_recv() {
                back.push(x);
            }
            for m in marks {
                self.mark(m);
            }
            for x in back {
                if let Returned::Tap(t) = x {
                    let vid = t.voice;
                    drop(t);
                    self.going.retain(|g| g.lane.vid != vid);
                    self.forget(vid);
                }
            }
        }
        for n in &notes {
            if let Some(tx) = &self.watch.notes {
                let _ = tx.send(shifted(*n, self.offset));
            }
            if let RenderNote::Silent { voice, .. } | RenderNote::Ended { voice, .. } = *n {
                self.silenced.push(voice);
            }
            if let RenderNote::Silent { voice, .. } | RenderNote::Ended { voice, .. } = *n
                && let Some(g) = self
                    .going
                    .iter_mut()
                    .find(|g| g.lane.vid == voice && !g.dropped)
            {
                g.dropped = true;
                self.pending.push_back(RenderCmd::Drop { voice });
            }
        }
        if lost {
            self.lose();
        }
        notes
    }

    fn mark(&mut self, m: Marker) {
        let k = match self.marks.iter().position(|(v, _)| *v == m.voice) {
            Some(k) => k,
            None => {
                self.marks.push((m.voice, VecDeque::new()));
                self.marks.len() - 1
            }
        };
        let q = &mut self.marks[k].1;
        if q.len() == MARKS {
            q.pop_front();
        }
        q.push_back(m);
    }

    /// The content place `vid` had (or will have, carried on) at device frame `f` of this
    /// start: from the last of its markers at or before `f`.
    pub(super) fn content_at(&self, vid: VoiceId, f: u64) -> Option<f64> {
        let (_, q) = self.marks.iter().find(|(v, _)| *v == vid)?;
        let m = q.iter().rev().find(|m| m.frame <= f)?;
        Some(m.content + (f - m.frame) as f64 * m.ratio)
    }

    /// The device frame at the DAC now: the newest presentation pair carried to now (before
    /// one comes, what has been rendered less the output's buffer).
    pub(super) fn heard_frame(&self) -> Option<u64> {
        self.run.as_ref()?;
        self.frame_at(self.now_ns())
    }

    /// Where `vid` is heard now.
    pub(super) fn heard_content(&self, vid: VoiceId) -> Option<f64> {
        let f = self.heard_frame()?;
        self.content_at(vid, f).or_else(|| {
            // Not heard yet (still in the buffer): its first marker's place.
            let (_, q) = self.marks.iter().find(|(v, _)| *v == vid)?;
            q.front().map(|m| m.content)
        })
    }

    /// The renderer's state of `vid` at its last call.
    pub(super) fn voice_state(&self, vid: VoiceId) -> Option<VoiceState> {
        let s = self.snapshot.as_ref()?;
        s.voices
            .iter()
            .flatten()
            .find(|v| v.voice == vid)
            .map(|v| v.state)
    }

    /// `vid`'s content place at the end of the renderer's last call, and its ratio.
    pub(super) fn rendered_place(&self, vid: VoiceId) -> Option<(f64, f64)> {
        let s = self.snapshot.as_ref()?;
        s.voices
            .iter()
            .flatten()
            .find(|v| v.voice == vid)
            .map(|v| (v.content, v.ratio))
    }

    /// The device frame at the DAC at local time `t_ns`: from the newest presentation pair
    /// (before one comes: what has been rendered less the output's lead, now).
    pub(super) fn frame_at(&self, t_ns: u64) -> Option<u64> {
        let o = self.open.as_ref()?;
        let rate = f64::from(o.info.format.rate.max(1));
        let (f, at) = match self.pair() {
            Some((frame, time)) => (frame as f64, time),
            // Before this start's first pair: what is rendered less what the output holds
            // (a start's first frame is heard a lead after now).
            None => (
                self.frames() as f64 - self.lead_frames() as f64,
                self.now_ns(),
            ),
        };
        let d = (t_ns as f64 - at as f64) / 1e9 * rate;
        Some((f + d).max(0.0) as u64)
    }

    // --- The output's life ---

    /// The output lost (or the default changed under one following it): its renderer and
    /// voices go; it is opened again on the next start, after a moment.
    fn lose(&mut self) {
        let heard = self.heard_frame();
        if let Some(o) = self.open.as_mut() {
            let back = o.dev.out().stop();
            drop(back);
        }
        if let Some(r) = self.run.take() {
            self.end_run(r, heard);
        }
        if let Some(o) = self.open.as_mut() {
            o.dev.out().close();
        }
        self.open = None;
        self.endpoint = None;
        let changed = !matches!(self.choice, OutputChoice::Device { id: Some(_), .. });
        let now = self.now_ns();
        self.retry = Some((0, now + if changed { 0 } else { RETRY_NS[0] }));
    }

    /// A start is over (its renderer back from the output and dropped): its voices go.
    fn end_run(&mut self, r: Running, lost_at: Option<u64>) {
        let frames = r.status.read().frames;
        self.offset += frames;
        {
            let mut renders = self.common.renders();
            renders.running = None;
            renders.ended.push(r.status.clone());
        }
        let generation = r.generation;
        drop(r);
        self.renew = false;
        self.pending.clear();
        self.unstarted = None;
        self.going.retain(|g| g.lane.given != Some(generation));
        self.live.clear();
        self.snapshot = None;
        self.quiet_from = 0;
        let now = self.now_ns();
        self.idle_since = Some(now);
        // What the output still holds plays out: its lead as the presentation measured it (a
        // queueing driver holds far more than its reported buffer), and two periods more.
        let held = self.lead_frames();
        let tail = self.open.as_ref().map_or(0, |o| {
            (held.max(u64::from(o.info.buffer_frames)) + 2 * u64::from(o.info.period_frames))
                * 1_000_000_000
                / u64::from(o.info.format.rate.max(1))
        });
        self.drained_at = now + tail;
        self.event = Some(Stopped {
            generation,
            lost_at,
        });
    }

    /// Nothing sounds or is about to: every voice silent or gone, every fade over a period
    /// ago, nothing waiting to be sent.
    fn quiet(&self) -> bool {
        if !self.pending.is_empty() || self.unstarted.is_some() || !self.going.is_empty() {
            return false;
        }
        let Some(s) = self.snapshot.as_ref() else {
            return true;
        };
        s.frames >= self.quiet_from + self.period()
            && s.voices
                .iter()
                .flatten()
                .all(|v| v.state == VoiceState::Silent)
    }

    /// The output's life for this turn: stop it once quiet when the program wants no sound or
    /// it must reopen or switch; close it once its tail has played when it must, or (a sound
    /// device) after 10 s unused. Returns what the program must know.
    fn step(&mut self, wanted: bool) -> Option<Stopped> {
        let must = self.reopen || self.renew || self.switching.is_some() || !wanted;
        if self.run.is_some() && must && self.quiet() {
            if let Some(o) = self.open.as_mut() {
                let back = o.dev.out().stop();
                drop(back);
            }
            if let Some(r) = self.run.take() {
                self.end_run(r, None);
            }
        }
        if self.run.is_none()
            && let Some(o) = self.open.as_mut()
        {
            let now = self.tb.now_ns();
            let drained = now >= self.drained_at;
            let unused = !wanted
                && o.mode != Mode::Memory
                && self.idle_since.is_some_and(|t| now >= t + RELEASE_NS);
            if drained && (self.reopen || self.switching.is_some() || self.quitting || unused) {
                o.dev.out().close();
                self.open = None;
                self.reopen = false;
                self.endpoint = None;
            }
        }
        if self.open.is_none()
            && let Some(c) = self.switching.take()
        {
            self.choice = c;
        }
        self.event.take()
    }

    // --- What the window sees ---

    /// The output's part of the signal path: its name, mode, format and latency (ms).
    pub(super) fn output_signal(&self) -> Option<(String, Mode, DeviceFormat, f32)> {
        let o = self.open.as_ref()?;
        let latency = self.lead_frames() as f32 * 1000.0 / o.info.format.rate.max(1) as f32;
        Some((o.name.clone(), o.mode, o.info.format.clone(), latency))
    }

    fn fill_view<T>(&self, v: &mut status::View<T>) {
        v.volume = self.volume;
        v.sound = self.sound.clone();
        v.sleep_in_s = self
            .sleep_at
            .map(|t| t.saturating_sub(self.now_ns()) / 1_000_000_000);
        v.spectrum = self.spectrum;
        v.rate = self.rate().max(1);
    }
}

/// `n` with its frames counted from the first frame the player ever rendered.
fn shifted(n: RenderNote, by: u64) -> RenderNote {
    match n {
        RenderNote::Started {
            voice,
            at,
            content,
            fade,
        } => RenderNote::Started {
            voice,
            at: at + by,
            content,
            fade,
        },
        RenderNote::Joined { from, to, at } => RenderNote::Joined {
            from,
            to,
            at: at + by,
        },
        RenderNote::Faded {
            voice,
            from,
            to,
            kind,
        } => RenderNote::Faded {
            voice,
            from: from + by,
            to: to + by,
            kind,
        },
        RenderNote::Gain { from, to, target } => RenderNote::Gain {
            from: from + by,
            to: to + by,
            target,
        },
        RenderNote::Resampling { voice, at, on } => RenderNote::Resampling {
            voice,
            at: at + by,
            on,
        },
        RenderNote::Starved { voice, at } => RenderNote::Starved { voice, at: at + by },
        RenderNote::Resumed { voice, at, content } => RenderNote::Resumed {
            voice,
            at: at + by,
            content,
        },
        RenderNote::Ended { voice, at } => RenderNote::Ended { voice, at: at + by },
        RenderNote::Silent { voice, at } => RenderNote::Silent { voice, at: at + by },
        RenderNote::Late {
            voice,
            at,
            by: late,
        } => RenderNote::Late {
            voice,
            at: at + by,
            by: late,
        },
        other => other,
    }
}

// --- The room's view of the deck (WP5) -------------------------------------------------------

impl Deck {
    /// Ask the running renderer for this trim (ppm; it glides there at 1000 ppm/s and holds it
    /// within ±500 ppm).
    pub(super) fn set_trim(&self, ppm: f64) {
        if let Some(r) = &self.run {
            r.rate.set_ppm(ppm);
        }
    }

    /// The trim the sounding voice plays at now, as the renderer said at its last call.
    pub(super) fn trim_now(&self) -> f64 {
        self.snapshot.as_ref().map_or(0.0, |s| s.ppm)
    }

    /// The running renderer gets nothing more: it stops once quiet, and the next start has a
    /// fresh one (a room let go of the player: nothing of its trim can reach your own music).
    pub(super) fn renew_renderer(&mut self) {
        if self.run.is_some() {
            self.renew = true;
        }
    }

    /// The newest presentation pair of the start running now: (its generation, device frame,
    /// mono time).
    pub(super) fn presented(&self) -> Option<(u64, u64, u64)> {
        let (frame, time) = self.pair()?;
        Some((self.run.as_ref()?.generation, frame, time))
    }

    /// The output has said which frame is at its DAC when (a pair of this start): frames for
    /// times are exact from here on. A start from silence in a room waits for it (the output
    /// runs in silence meanwhile, a period or two).
    pub(super) fn placed(&self) -> bool {
        self.pair().is_some()
    }

    /// The newest presentation pair, once this start has rendered (a pair read before its
    /// first call may be the last start's, on the same output).
    fn pair(&self) -> Option<(u64, u64)> {
        if !self.run.as_ref().is_some_and(|r| !r.fresh) {
            return None;
        }
        let p = self.open.as_ref()?.presentation.read()?;
        Some((p.frame, p.time_ns))
    }

    /// The device frame at the DAC at local time `t_ns`, from a presentation pair only (no
    /// guess from the lead): `None` before the first pair of this start.
    pub(super) fn dac_frame(&self, t_ns: u64) -> Option<f64> {
        let (_, frame, time) = self.presented()?;
        let rate = f64::from(self.rate().max(1));
        Some(frame as f64 + (t_ns as f64 - time as f64) / 1e9 * rate)
    }

    /// The content place (source frames) `vid` had at device frame `f` (fractional), from its
    /// markers.
    pub(super) fn content_at_f(&self, vid: VoiceId, f: f64) -> Option<f64> {
        let (_, q) = self.marks.iter().find(|(v, _)| *v == vid)?;
        let m = q.iter().rev().find(|m| (m.frame as f64) <= f)?;
        Some(m.content + (f - m.frame as f64) * m.ratio)
    }

    /// The local time at which device frame `f` is at the DAC (from the newest presentation
    /// pair; before one comes, from what is rendered and the output's lead).
    pub(super) fn time_of(&self, f: u64) -> Option<u64> {
        let o = self.open.as_ref()?;
        let rate = f64::from(o.info.format.rate.max(1));
        let (pf, at) = match self.pair() {
            Some((frame, time)) => (frame as f64, time),
            None => (
                self.frames() as f64 - self.lead_frames() as f64,
                self.now_ns(),
            ),
        };
        let t = at as f64 + (f as f64 - pf) / rate * 1e9;
        Some(t.max(0.0) as u64)
    }

    /// No fade commanded ends after this device frame.
    pub(super) fn quiet_after(&self) -> u64 {
        self.quiet_from
    }

    /// This computer's mono time (ns) at `i` (an `Instant` of the app's: the same clock as the
    /// engine's on a real output, QPC on Windows).
    pub(super) fn ns_of_instant(&self, i: std::time::Instant) -> u64 {
        let (now_i, now) = (std::time::Instant::now(), self.now_ns());
        if i >= now_i {
            now + (i - now_i).as_nanos() as u64
        } else {
            now.saturating_sub((now_i - i).as_nanos() as u64)
        }
    }
}

// --- The sync tick: the controller of section 4.9 ---------------------------------------------

/// The sync tick's period, ns.
pub(super) const TICK_NS: u64 = 20_000_000;
/// No integral and no realign for this long after a start, a switch or a realign, ns.
pub(super) const SETTLE_NS: u64 = 400_000_000;
/// The errors the median is taken over (200 ms of ticks).
const MEDIAN_OF: usize = 10;
/// The average after the median: its time constant, s.
const AVERAGE_S: f64 = 1.0;
/// The integral learns only while the error is under this, ms.
const INTEGRAL_UNDER_MS: f64 = 20.0;
/// What the integral may add at most, ppm.
const INTEGRAL_MAX: f64 = 200.0;
/// The whole trim at most, ppm (under one cent of pitch).
pub(super) const TRIM_MAX: f64 = 500.0;

/// One column of the controller's table (4.9).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Column {
    /// ppm per ms of error, while correcting.
    pub gain: f64,
    /// Correcting starts past this and stops under `band_out`, ms.
    pub band_in: f64,
    pub band_out: f64,
    /// The integral's time constant, s.
    pub integral_s: f64,
    /// Past this for `line_ns`: a realign, ms.
    pub line_ms: f64,
    pub line_ns: u64,
    /// At most one realign per this, ns.
    pub realign_every_ns: u64,
    /// How long a quiet moment is looked for, ns.
    pub quiet_look_ns: u64,
}

/// Speakers heard together.
pub(super) const SAME_ROOM: Column = Column {
    gain: 250.0,
    band_in: 1.0,
    band_out: 0.3,
    integral_s: 30.0,
    line_ms: 50.0,
    line_ns: 2_000_000_000,
    realign_every_ns: 10_000_000_000,
    quiet_look_ns: 10_000_000_000,
};

/// Each on their own.
pub(super) const APART: Column = Column {
    gain: 25.0,
    band_in: 10.0,
    band_out: 3.0,
    integral_s: 60.0,
    line_ms: 250.0,
    line_ns: 3_000_000_000,
    realign_every_ns: 30_000_000_000,
    quiet_look_ns: 30_000_000_000,
};

/// What the controller says after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct Steer {
    /// The trim to ask for, ppm.
    pub ppm: f64,
    /// The filtered error, ms (positive: ahead).
    pub error_ms: Option<f64>,
    /// The proportional part acts (outside the band).
    pub correcting: bool,
    /// The error has been past the realign line for the column's time.
    pub far: bool,
    pub settling: bool,
}

/// The controller (4.9): a median and an average on the error, feed-forward from the clock's
/// and the sound card's rates, a proportional part outside a band, an integral for what the
/// feed-forward misses.
#[derive(Debug, Default)]
pub(super) struct Controller {
    errs: std::collections::VecDeque<f64>,
    avg: Option<f64>,
    integral: f64,
    correcting: bool,
    far_since: Option<u64>,
    settle_until: u64,
    last_ns: Option<u64>,
    /// The proportional part as applied at the last tick (after the cap), ppm.
    p_last: f64,
    out: Steer,
}

impl Controller {
    /// A start, a switch or a realign: the error is measured afresh and nothing slow acts until
    /// `until` (at least 400 ms). The integral is kept: it is the sound card's and the clock's,
    /// not the place's.
    pub(super) fn settle(&mut self, now: u64, until: u64) {
        self.errs.clear();
        self.avg = None;
        self.correcting = false;
        self.far_since = None;
        self.settle_until = until.max(now + SETTLE_NS);
    }

    /// Frozen (suspended, stepped out): nothing measured, the trim kept.
    pub(super) fn freeze(&mut self) {
        self.last_ns = None;
        self.errs.clear();
        self.avg = None;
        self.far_since = None;
        self.out.error_ms = None;
        self.out.far = false;
        self.out.correcting = false;
    }

    pub(super) fn last(&self) -> Steer {
        self.out
    }

    /// One tick: `err_ms` the error measured now (none: nothing to measure, the trim kept);
    /// `ff` the feed-forward (ppm); `bound_ms` the clock's bound.
    pub(super) fn tick(
        &mut self,
        now: u64,
        err_ms: Option<f64>,
        col: &Column,
        bound_ms: f64,
        ff: f64,
    ) -> Steer {
        let dt = self
            .last_ns
            .map_or(0.0, |t| now.saturating_sub(t) as f64 / 1e9)
            .min(0.2);
        self.last_ns = Some(now);
        let settling = now < self.settle_until;
        self.out.settling = settling;
        let Some(e) = err_ms else {
            return self.out;
        };
        if self.errs.len() == MEDIAN_OF {
            self.errs.pop_front();
        }
        self.errs.push_back(e);
        let mut sorted: Vec<f64> = self.errs.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        let prev = self.avg.filter(|_| !settling && dt > 0.0);
        let avg = match self.avg {
            Some(a) if !settling => a + (median - a) * (dt / AVERAGE_S).min(1.0),
            _ => median,
        };
        self.avg = Some(avg);
        // The band is at least half the clock's bound: what the clock can't rule out is not
        // chased.
        let band_in = col.band_in.max(bound_ms / 2.0);
        let band_out = band_in * col.band_out / col.band_in;
        if avg.abs() > band_in {
            self.correcting = true;
        } else if avg.abs() < band_out {
            self.correcting = false;
        }
        let p = if self.correcting {
            -col.gain * avg
        } else {
            0.0
        };
        // The integral learns the rate the feed-forward misses: the error drifting at `slope`
        // while `p_last` acted says the missing rate is the integral plus `p_last` less the
        // slope; the integral moves toward it with the column's time constant. (An integral of
        // the error itself, with the proportional part off inside the band, would swing there.)
        if let Some(prev) = prev
            && avg.abs() < INTEGRAL_UNDER_MS
        {
            let slope = (avg - prev) / dt * 1_000.0;
            self.integral = (self.integral + (self.p_last - slope) * dt / col.integral_s)
                .clamp(-INTEGRAL_MAX, INTEGRAL_MAX);
        }
        let ppm = (ff + self.integral + p).clamp(-TRIM_MAX, TRIM_MAX);
        self.p_last = ppm - ff - self.integral;
        // To a tenth of a ppm: a player exactly in place keeps its rate exactly.
        let ppm = (ppm * 10.0).round() / 10.0;
        let far = if !settling && avg.abs() > col.line_ms {
            let since = *self.far_since.get_or_insert(now);
            now - since >= col.line_ns
        } else {
            self.far_since = None;
            false
        };
        self.out = Steer {
            ppm,
            error_ms: Some(avg),
            correcting: self.correcting,
            far,
            settling,
        };
        self.out
    }
}

/// The sound card's rate against the mono clock, from the presentation pairs of the last
/// 10 s (a least-squares line), for the feed-forward.
#[derive(Debug, Default)]
pub(super) struct DacRate {
    pts: std::collections::VecDeque<(f64, f64)>,
    generation: u64,
    last_time: u64,
}

/// The window, and the least span used, s.
const DAC_WINDOW_S: f64 = 10.0;
const DAC_LEAST_S: f64 = 2.0;
/// A pair this far off the line is a gap or a slip: the window starts again, s.
const DAC_JUMP_S: f64 = 0.002;

impl DacRate {
    /// Take the newest pair (`generation`, device frame, mono ns) at `rate`.
    pub(super) fn push(&mut self, generation: u64, frame: u64, time_ns: u64, rate: u32) {
        if generation != self.generation {
            self.pts.clear();
            self.generation = generation;
        }
        if time_ns == self.last_time {
            return;
        }
        self.last_time = time_ns;
        let (t, f) = (time_ns as f64 / 1e9, frame as f64 / f64::from(rate.max(1)));
        if let Some(&(t0, f0)) = self.pts.back() {
            let slope = self.slope().unwrap_or(1.0);
            if ((f - f0) - (t - t0) * slope).abs() > DAC_JUMP_S {
                self.pts.clear();
            }
        }
        self.pts.push_back((t, f));
        while self
            .pts
            .front()
            .is_some_and(|&(t0, _)| t - t0 > DAC_WINDOW_S)
        {
            self.pts.pop_front();
        }
    }

    /// Device seconds per mono second, once the pairs span 2 s.
    fn slope(&self) -> Option<f64> {
        let (&(t0, _), &(t1, _)) = (self.pts.front()?, self.pts.back()?);
        if t1 - t0 < DAC_LEAST_S {
            return None;
        }
        let n = self.pts.len() as f64;
        let (mt, mf) = self
            .pts
            .iter()
            .fold((0.0, 0.0), |(a, b), &(t, f)| (a + t - t0, b + f));
        let (mt, mf) = (mt / n, mf / n);
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for &(t, f) in &self.pts {
            let x = t - t0 - mt;
            sxy += x * (f - mf);
            sxx += x * x;
        }
        (sxx > 0.0).then(|| sxy / sxx)
    }

    /// How fast the card runs against the mono clock, ppm (positive: fast); 0 until known.
    pub(super) fn ppm(&self) -> f64 {
        self.slope().map_or(0.0, |s| (s - 1.0) * 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn the_controller_closes_an_error_by_rate_within_the_cap() {
        let mut c = Controller::default();
        let mut err = 8.0;
        let mut now = 0;
        for _ in 0..1_500 {
            now += 20 * MS;
            let s = c.tick(now, Some(err), &SAME_ROOM, 0.0, 0.0);
            assert!(s.ppm.abs() <= TRIM_MAX);
            // Playing faster (positive ppm) takes the error down (ahead is positive).
            // A ppm faster moves the content on by a microsecond a second.
            err += s.ppm * 1e-3 * 0.02;
        }
        // 8 ms ahead: slowed (negative), until inside the band.
        assert!(err.abs() < SAME_ROOM.band_out * 2.0, "{err}");
    }

    #[test]
    fn feed_forward_alone_holds_a_drift_and_a_player_in_place_keeps_rate_zero() {
        let mut c = Controller::default();
        let s = c.tick(20 * MS, Some(0.0), &SAME_ROOM, 0.0, -300.0);
        assert_eq!(s.ppm, -300.0);
        let mut c = Controller::default();
        for k in 1..500 {
            let s = c.tick(k * 20 * MS, Some(0.01), &SAME_ROOM, 0.0, 0.0);
            assert_eq!(s.ppm, 0.0, "a frame's worth of noise moves nothing");
        }
    }

    #[test]
    fn a_rate_the_feed_forward_misses_is_learned_without_a_swing() {
        // The card runs 40 ppm faster than the feed-forward knows: the error drifts until the
        // integral has learned it, never leaving the band once inside.
        let (mut c, mut err, mut now) = (Controller::default(), 0.0, 0);
        let mut u = 0.0;
        for k in 0..30_000 {
            now += 20 * MS;
            let s = c.tick(now, Some(err), &SAME_ROOM, 0.0, 0.0);
            u = s.ppm;
            // Ahead by 40 ppm unless slowed.
            err += (u + 40.0) * 1e-3 * 0.02;
            if k > 15_000 {
                assert!(err.abs() < SAME_ROOM.band_in, "{err} ms at {k}");
            }
        }
        assert!((u + 40.0).abs() < 2.0, "learned: {u}");
    }

    #[test]
    fn the_band_widens_to_half_the_clocks_bound() {
        for (err, corrects) in [(4.0, false), (6.0, true)] {
            let mut c = Controller::default();
            let mut s = Steer::default();
            for k in 1..200 {
                s = c.tick(k * 20 * MS, Some(err), &SAME_ROOM, 10.0, 0.0);
                if s.correcting {
                    break;
                }
            }
            assert_eq!(s.correcting, corrects, "{err} ms against a 5 ms band");
        }
    }

    #[test]
    fn far_only_after_the_line_has_held() {
        let mut c = Controller::default();
        c.settle(0, 0);
        let mut now = 0;
        let mut far_at = None;
        for _ in 0..200 {
            now += 20 * MS;
            if c.tick(now, Some(300.0), &APART, 0.0, 0.0).far {
                far_at.get_or_insert(now);
            }
        }
        let t = far_at.expect("far");
        let from = APART.line_ns + SETTLE_NS;
        assert!((from..from + 100 * MS).contains(&t), "{t}");
    }

    #[test]
    fn the_dac_rate_is_read_off_the_pairs_and_a_gap_starts_again() {
        let mut d = DacRate::default();
        for k in 0..400u64 {
            let t = 1_000_000_000 + k * 20 * MS;
            let f = ((k * 20 * MS) as f64 * 48_000.0 / 1e9 * (1.0 + 300e-6)) as u64;
            d.push(1, f, t, 48_000);
        }
        assert!((d.ppm() - 300.0).abs() < 1.0, "{}", d.ppm());
        // A gap: frames 100 ms behind.
        d.push(1, 400 * 960, 1_000_000_000 + 400 * 20 * MS, 48_000);
        assert_eq!(d.ppm(), 0.0, "measured again");
    }
}
