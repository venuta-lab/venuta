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
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, TrySendError, channel};
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

pub(super) struct Spawned<T> {
    pub commands: Sender<Cmd<T>>,
    pub rooms: Sender<RoomMsg<T>>,
    pub thread: JoinHandle<()>,
}

/// Start engine 2: its control thread, its status thread, and (for the fast memory output) the
/// driver of its simulated clock.
pub(super) fn spawn<T: Item + Serialize>(
    output: OutputChoice,
    on_status: Box<dyn FnMut(Status<T>) + Send>,
    watch: Watch,
) -> Spawned<T> {
    let (cmd_tx, cmd_rx) = channel();
    let (room_tx, room_rx) = channel();
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
            let deck = Deck::new(tb, pace, output, shared.common.clone(), watch);
            let mut control = Control {
                shared: shared.clone(),
                pacer: control_pacer,
                cmds: cmd_rx,
                rooms: room_rx,
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
        commands: cmd_tx,
        rooms: room_tx,
        thread,
    }
}

struct Control<T: Item + Serialize> {
    shared: Arc<Shared<T>>,
    pacer: Pacer,
    cmds: Receiver<Cmd<T>>,
    rooms: Receiver<RoomMsg<T>>,
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
            match self.pacer.recv_until(&self.cmds, until) {
                Ok(cmd) => {
                    quitting |= self.command(cmd);
                    self.answer();
                    // Every command waiting now, in order, each with its answer.
                    while let Ok(cmd) = self.cmds.try_recv() {
                        quitting |= self.command(cmd);
                        self.answer();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => quitting |= self.command(Cmd::Quit),
            }
            loop {
                match self.rooms.try_recv() {
                    Ok(m) => {
                        self.program.room(m, &mut self.deck);
                        self.answer();
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
            self.turn();
            self.publish();
            if quitting && self.deck.quit_done() {
                break;
            }
        }
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
            // Engine 1's rooms: taken by engine 2 once WP5's adapter is in.
            Cmd::Sync(_) => {}
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
    /// Frames before the first render call: none rendered yet.
    fresh: bool,
}

enum Dev {
    #[cfg(windows)]
    Wasapi(crate::wasapi_out::WasapiOutput),
    Other(Box<dyn Output>),
}

impl Dev {
    fn out(&mut self) -> &mut dyn Output {
        match self {
            #[cfg(windows)]
            Dev::Wasapi(w) => w,
            Dev::Other(o) => o.as_mut(),
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
    live: Vec<Live>,
    marks: Vec<(VoiceId, VecDeque<Marker>)>,
    /// No fade commanded ends after this device frame.
    quiet_from: u64,
    /// The output is to be stopped and closed, then opened again for another format.
    reopen: bool,
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
    ) -> Deck {
        let mono: Arc<dyn MonoClock> = tb.clone();
        Deck {
            tb,
            mono,
            pace,
            choice,
            common,
            watch,
            streams: Arc::new(Streams::new()),
            next_vid: 1,
            open: None,
            run: None,
            generation: 0,
            offset: 0,
            pending: VecDeque::new(),
            unstarted: None,
            going: Vec::new(),
            live: Vec::new(),
            marks: Vec::new(),
            quiet_from: 0,
            reopen: false,
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
            || self.switching.is_some()
    }

    fn quit_done(&self) -> bool {
        self.run.is_none() && self.open.is_none()
    }

    // --- Voices ---

    /// A new voice of `item` from `place`: its decode thread opens it (on the shared downloads)
    /// and decodes ahead; nothing waits here.
    pub(super) fn lane<T: Item>(&mut self, item: &T, place: Place) -> Lane {
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
            depth: None,
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
            if self.fits(f) && !self.reopen && self.switching.is_none() {
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
            OutputChoice::Device { id, exclusive } => {
                Dev::Wasapi(crate::wasapi_out::wasapi_output(id.as_deref(), *exclusive))
            }
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
        let (renderer, status) = Renderer::new(RenderSetup {
            format: o.info.format.clone(),
            max_period: o.info.period_frames,
            ends,
            rate: Arc::new(RateTarget::default()),
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
            fresh: true,
        });
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
        let s = lane.status();
        let (Some(fmt), Some(content)) = (s.format.clone(), lane.start_of(&s)) else {
            return false;
        };
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
        if let Some(r) = &mut self.run {
            let snap = r.status.read();
            if snap.frames > 0 {
                r.fresh = false;
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
    /// (before one comes: what has been rendered less the output's buffer, now).
    pub(super) fn frame_at(&self, t_ns: u64) -> Option<u64> {
        let o = self.open.as_ref()?;
        let rate = f64::from(o.info.format.rate.max(1));
        let (f, at) = match o.presentation.read() {
            Some(p) => (p.frame, p.time_ns),
            None => (
                self.frames()
                    .saturating_sub(u64::from(o.info.buffer_frames)),
                self.now_ns(),
            ),
        };
        let d = (t_ns as f64 - at as f64) / 1e9 * rate;
        Some((f as f64 + d).max(0.0) as u64)
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
        self.pending.clear();
        self.unstarted = None;
        self.going.retain(|g| g.lane.given != Some(generation));
        self.live.clear();
        self.snapshot = None;
        self.quiet_from = 0;
        let now = self.now_ns();
        self.idle_since = Some(now);
        let tail = self.open.as_ref().map_or(0, |o| {
            u64::from(o.info.buffer_frames + 2 * o.info.period_frames) * 1_000_000_000
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
        let must = self.reopen || self.switching.is_some() || !wanted;
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
        let latency = o.info.buffer_frames as f32 * 1000.0 / o.info.format.rate.max(1) as f32;
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
