//! Room sessions (design 4.1, 4.3, its WP9 notes in 9.2): the host's and a guest's, each on
//! any `Transport` (iroh in the app, the simulated network in tests), each driven by `step`.
//!
//! - [`HostSession`]: Room v2 behind its door (protocol 3 only: another version is told so in
//!   words), every member's messages and clock pings, their paths, the host's own conductor,
//!   and the host's files and covers served to members only ([`HostSession::media`]).
//! - [`GuestSession`]: the knock, the clock (pings on datagrams, the map published for the
//!   player), the conductor, presses held while reconnecting (only while fresh), the 2-minute
//!   rule, and waking from a sleep.
//! - [`window`]: the window's `View` from either.
//!
//! Pure and single-threaded: the app's runner owns a session, gives it commands, and steps it
//! at least every 10 ms; a test steps it in simulated time. Nothing here blocks.

use crate::anchor::{Anchor, EntryId};
use crate::clock::{Clock, Steps};
use crate::clock_link::{Pinger, answer};
use crate::conductor::Conductor;
use crate::proto::Song;
use crate::proto::v3::{
    self, Action, Advice, Member, Phase as WirePhase, RoomView as WireRoom, ToGuest, ToHost,
    check_to_guest, check_to_host,
};
use crate::room::v2::{Changes, HOST, HostNote, Knock, Room};
use crate::room_player::{ListenMode, SubState, Suspension};
use crate::time::MonoClock;
use crate::transport::{
    ControlMsg, Get, MediaInfo, MediaServer, PathInfo, PeerId, Transport, TransportEvent,
};
use crate::view::{self, CopyView, View};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// A step this long after the one before means the computer slept (or was stopped), ns.
pub const WOKE_AFTER_NS: u64 = 3_000_000_000;
/// Someone connected who hasn't said hello by then is closed, ns.
pub const HELLO_WITHIN_NS: u64 = 10_000_000_000;
/// A peer's last line goes before its connection is closed: closed this long after, ns.
pub const CLOSE_AFTER_NS: u64 = 300_000_000;
/// A guest lost this long gives up, at the end of the song playing (D17), ns.
pub const GIVE_UP_NS: u64 = 120_000_000_000;
/// A press held while reconnecting goes only this long after it was pressed (D10), ns.
pub const HELD_FOR_NS: u64 = 10_000_000_000;
/// A press shows as under way at most this long (until the room's next anchor), ns.
pub const PRESSING_NS: u64 = 3_000_000_000;
/// The host asks the transport for each member's path this often, ns.
pub const PATH_EVERY_NS: u64 = 1_000_000_000;

/// Where a guest is with the room (section 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Connecting; the hello not sent yet.
    Connecting,
    /// The hello sent, no answer yet.
    Knocking,
    /// Waiting for the host to let them in.
    AtTheDoor,
    In,
    /// The connection lost; the music plays on.
    Reconnecting,
    Ended,
}

fn us(ns: u64) -> i64 {
    (ns / 1_000) as i64
}

/// What a press says while it is on its way.
fn pressing_of(a: &Action) -> Option<String> {
    let p = match a {
        Action::Play => view::Press::Play,
        Action::Pause => view::Press::Pause,
        Action::Seek { content_us } => view::Press::Seek {
            to_ms: (*content_us).max(0) as u64 / 1_000,
        },
        Action::Next | Action::Jump { .. } => view::Press::Skip,
        Action::Previous => view::Press::Previous,
        _ => return None,
    };
    Some(view::pressing(p))
}

/// Why a press held while reconnecting didn't go, in words for the window.
pub fn too_late(action: &Action) -> String {
    let (what, verb) = match action {
        Action::Play => ("Your play", "was"),
        Action::Pause => ("Your pause", "was"),
        Action::Seek { .. } => ("Your seek", "was"),
        Action::Next => ("Your skip", "was"),
        Action::Previous => ("Your skip back", "was"),
        Action::Jump { .. } => ("Your jump", "was"),
        Action::Add { .. } => ("The songs you added", "were"),
        Action::Remove { .. } => ("Your removal", "was"),
        Action::Move { .. } => ("Your move in the queue", "was"),
        Action::Volume { .. } => ("Your volume change", "was"),
        Action::Accept { .. } | Action::Decline { .. } => ("Your answer", "was"),
    };
    format!("{what} {verb} too late to send.")
}

// --- The host's files, to members only ------------------------------------------------------

/// Who may be served what, as the session last stepped: members' peers, and the keys of the
/// host's songs in the queue.
#[derive(Default)]
struct Allowed {
    peers: HashSet<PeerId>,
    keys: HashSet<String>,
}

/// The host's media, gated: only members, only the host's songs in the room (their file, their
/// cover, their envelope).
struct Gate {
    allowed: Arc<Mutex<Allowed>>,
    inner: Arc<dyn MediaServer>,
}

impl Gate {
    fn lets(&self, from: PeerId, get: &Get) -> bool {
        let key = get
            .get
            .strip_prefix("cover:")
            .or_else(|| get.get.strip_prefix("env:"))
            .unwrap_or(&get.get);
        let a = self.allowed.lock().unwrap_or_else(|e| e.into_inner());
        a.peers.contains(&from) && a.keys.contains(key)
    }
}

impl MediaServer for Gate {
    fn open(&self, from: PeerId, get: &Get) -> Option<Arc<[u8]>> {
        if self.lets(from, get) {
            self.inner.open(from, get)
        } else {
            None
        }
    }
    fn describe(&self, from: PeerId, get: &Get) -> Option<MediaInfo> {
        if self.lets(from, get) {
            self.inner.describe(from, get)
        } else {
            None
        }
    }
}

// --- The host ---------------------------------------------------------------------------------

/// What a hosted room needs of the app.
#[derive(Clone)]
pub struct HostConfig {
    /// Whether the invite still lets people in (it has an end).
    pub invite_open: Arc<dyn Fn() -> bool + Send + Sync>,
}

/// A room hosted here.
pub struct HostSession<I> {
    t: Arc<dyn Transport>,
    rx: Receiver<TransportEvent>,
    clock: Arc<dyn MonoClock>,
    room: Room,
    conductor: Conductor<I>,
    cfg: HostConfig,
    member_of: HashMap<PeerId, u32>,
    peer_of: HashMap<u32, PeerId>,
    /// Connected, no hello yet: since when.
    door: HashMap<PeerId, u64>,
    closing: Vec<(PeerId, u64)>,
    notes: Vec<HostNote>,
    volume: Option<u8>,
    slow: HashSet<u32>,
    ended: bool,
    last_step_ns: u64,
    next_path_ns: u64,
    waking: bool,
    allowed: Arc<Mutex<Allowed>>,
    now_ns: u64,
}

impl<I: Clone + Send + 'static> HostSession<I> {
    /// A room on transport `t`, its time `clock` (the host's monotonic clock: the room's time),
    /// with Room v2 made as the host's player was (`Room::new`), and the host's conductor (its
    /// player taken already). The host is told everything a guest is.
    pub fn new(
        t: Arc<dyn Transport>,
        clock: Arc<dyn MonoClock>,
        room: Room,
        mut conductor: Conductor<I>,
        cfg: HostConfig,
    ) -> Self {
        let rx = t.events();
        let now_ns = clock.now_ns();
        let welcome = ToGuest::Welcome {
            you: HOST,
            room: room.view_for(HOST),
        };
        conductor.message(&welcome, now_ns);
        let mut s = HostSession {
            t,
            rx,
            clock,
            room,
            conductor,
            cfg,
            member_of: HashMap::new(),
            peer_of: HashMap::new(),
            door: HashMap::new(),
            closing: Vec::new(),
            notes: Vec::new(),
            volume: None,
            slow: HashSet::new(),
            ended: false,
            last_step_ns: now_ns,
            next_path_ns: now_ns,
            waking: false,
            allowed: Arc::default(),
            now_ns,
        };
        s.update_allowed();
        s
    }

    /// The room's time now (µs of the host's clock).
    pub fn now_us(&self) -> i64 {
        us(self.clock.now_ns())
    }

    pub fn room(&self) -> &Room {
        &self.room
    }

    pub fn conductor(&mut self) -> &mut Conductor<I> {
        &mut self.conductor
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The host's media, served to members only (give the result to the transport).
    pub fn media(&mut self, inner: Arc<dyn MediaServer>) -> Arc<dyn MediaServer> {
        Arc::new(Gate {
            allowed: self.allowed.clone(),
            inner,
        })
    }

    /// Something for the host's window (three starts missed, every device turned off).
    pub fn take_notes(&mut self) -> Vec<HostNote> {
        std::mem::take(&mut self.notes)
    }

    /// A volume for the host's speaker that a remote set (B17).
    pub fn take_volume(&mut self) -> Option<u8> {
        self.volume.take()
    }

    /// Who missed three starts in a row (until they're ready again).
    pub fn slow(&self) -> &HashSet<u32> {
        &self.slow
    }

    /// Change the room as the host's window asks, and carry out what it says.
    pub fn act(&mut self, f: impl FnOnce(&mut Room, i64) -> Changes) {
        let now = self.now_us();
        let c = f(&mut self.room, now);
        self.apply(c);
    }

    pub fn ask(&mut self, a: Action) -> Result<(), String> {
        let now = self.now_us();
        let c = self.room.ask(HOST, a, now)?;
        self.apply(c);
        Ok(())
    }

    pub fn request(&mut self, a: Action) -> Result<(), String> {
        let now = self.now_us();
        let c = self.room.request(HOST, a, now)?;
        self.apply(c);
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), String> {
        let now = self.now_us();
        let c = self.room.undo(HOST, now)?;
        self.apply(c);
        Ok(())
    }

    pub fn admit(&mut self, id: u32) {
        let now = self.now_us();
        if let Some(c) = self.room.admit(id, now) {
            self.apply(c);
            self.path_now(id);
        }
    }

    pub fn deny(&mut self, id: u32) {
        let now = self.now_us();
        let c = self.room.deny(id, now);
        self.apply(c);
    }

    pub fn remove(&mut self, id: u32, block: bool) {
        let now = self.now_us();
        let c = self.room.remove(id, block, now);
        self.apply(c);
    }

    pub fn set_let_in(&mut self, on: bool) {
        self.room.set_let_in_without_asking(on);
    }

    /// The host suggests something to one person (G7).
    pub fn advise(&mut self, id: u32, advice: Advice) {
        if let Some(&p) = self.peer_of.get(&id) {
            let _ = self.t.send(p, ControlMsg::of(&ToGuest::Advise { advice }));
        }
    }

    /// End the room for everyone (the host's, confirmed): everyone is told, the host's own
    /// player let go at once.
    pub fn end(&mut self) {
        let now = self.now_us();
        let c = self.room.end(now);
        self.apply(c);
    }

    fn update_allowed(&mut self) {
        let mut a = self.allowed.lock().unwrap_or_else(|e| e.into_inner());
        a.peers = self
            .member_of
            .iter()
            .filter(|(_, id)| self.room.is_member(**id))
            .map(|(p, _)| *p)
            .collect();
        a.keys = self
            .room
            .queue()
            .iter()
            .filter(|e| e.song.host_file)
            .map(|e| e.song.key.clone())
            .collect();
    }

    /// Everything waiting on the network, then time.
    pub fn step(&mut self) {
        let now_ns = self.clock.now_ns();
        self.now_ns = now_ns;
        if self.ended {
            self.close_due(now_ns);
            return;
        }
        if now_ns > self.last_step_ns + WOKE_AFTER_NS {
            // The host's computer slept: what its player still holds is dropped, and it comes
            // back in at the room's place (its map is the identity: no clock to measure).
            self.conductor.suspend(Suspension::Sleeping);
            self.waking = true;
        } else if self.waking {
            self.waking = false;
            self.conductor.resume();
        }
        self.last_step_ns = now_ns;
        while let Ok(e) = self.rx.try_recv() {
            self.event(e, now_ns);
            if self.ended {
                break;
            }
        }
        let now = us(now_ns);
        if now_ns >= self.next_path_ns {
            self.next_path_ns = now_ns + PATH_EVERY_NS;
            let members: Vec<(PeerId, u32)> =
                self.member_of.iter().map(|(p, i)| (*p, *i)).collect();
            for (p, id) in members {
                if let Some(path) = self.t.path(p) {
                    let c = self.room.path(id, path, now);
                    self.apply(c);
                }
            }
        }
        let late: Vec<PeerId> = self
            .door
            .iter()
            .filter(|(_, at)| now_ns >= **at + HELLO_WITHIN_NS)
            .map(|(p, _)| *p)
            .collect();
        for p in late {
            self.door.remove(&p);
            self.t.close(p);
        }
        let c = self.room.tick(now);
        self.apply(c);
        self.conductor_step(now_ns);
        self.close_due(now_ns);
        self.update_allowed();
    }

    fn conductor_step(&mut self, now_ns: u64) {
        self.conductor.step(now_ns);
        for m in self.conductor.take_out() {
            let now = self.now_us();
            if let Ok(c) = self.room.handle(HOST, m, now) {
                self.apply(c);
            }
        }
        if let Some((entry, len)) = self.conductor.take_length() {
            let now = self.now_us();
            let c = self.room.set_length(entry, len, now);
            self.apply(c);
        }
    }

    fn close_due(&mut self, now_ns: u64) {
        let (due, later): (Vec<_>, Vec<_>) =
            self.closing.drain(..).partition(|(_, at)| now_ns >= *at);
        self.closing = later;
        for (p, _) in due {
            self.t.close(p);
        }
    }

    fn event(&mut self, e: TransportEvent, now_ns: u64) {
        let now = us(now_ns);
        match e {
            TransportEvent::Message { from, msg, .. } => match self.member_of.get(&from) {
                Some(&id) => self.member_said(from, id, msg, now),
                None => self.at_door(from, msg, now),
            },
            TransportEvent::Datagram { from, .. } => {
                if answer(self.t.as_ref(), self.clock.as_ref(), &e)
                    && let Some(&id) = self.member_of.get(&from)
                {
                    let c = self.room.heard(id, now);
                    self.apply(c);
                }
            }
            TransportEvent::Joined { peer } => {
                self.door.insert(peer, now_ns);
            }
            TransportEvent::Lost { peer } => {
                self.door.remove(&peer);
                if let Some(&id) = self.member_of.get(&peer) {
                    let c = self.room.lost(id, now);
                    self.apply(c);
                }
            }
            TransportEvent::PathChanged { peer, path } => {
                if let Some(&id) = self.member_of.get(&peer) {
                    let c = self.room.path(id, path, now);
                    self.apply(c);
                }
            }
        }
    }

    /// A first line: a hello of this protocol knocks; another protocol's is told "different
    /// versions"; anything else closes the connection.
    fn at_door(&mut self, from: PeerId, msg: ControlMsg, now: i64) {
        self.door.remove(&from);
        let line = msg.0.strip_suffix(b"\n").unwrap_or(&msg.0);
        let hello = match v3::door(line) {
            Ok(h) => h,
            Err(not) => {
                match not.answer() {
                    Some(a) => {
                        let _ = self.t.send(from, ControlMsg::of(&a));
                        self.closing.push((from, self.now_ns + CLOSE_AFTER_NS));
                    }
                    // Not a hello at all: closed without a word.
                    None => self.t.close(from),
                }
                return;
            }
        };
        if !(self.cfg.invite_open)() {
            let why = "This room's invite has ended. Ask for a new one.";
            self.refuse(from, why);
            return;
        }
        let key = self
            .t
            .peer_key(from)
            .unwrap_or_else(|| format!("peer{}", from.0));
        let (k, c) = self.room.knock(&key, &hello.name, &hello.proof, now);
        match k {
            Knock::Refused(why) => self.refuse(from, &why),
            Knock::In(id) | Knock::Waiting(id) => {
                if let Some(old) = self.peer_of.insert(id, from)
                    && old != from
                {
                    self.member_of.remove(&old);
                }
                self.member_of.insert(from, id);
            }
        }
        self.apply(c);
        if let Some(&id) = self.member_of.get(&from) {
            self.path_now(id);
        }
    }

    /// What the transport says of a member's path, at once (a mode asked for right after
    /// coming in is judged on it, not on nothing).
    fn path_now(&mut self, id: u32) {
        let Some(&p) = self.peer_of.get(&id) else {
            return;
        };
        if let Some(path) = self.t.path(p) {
            let now = self.now_us();
            let c = self.room.path(id, path, now);
            self.apply(c);
        }
    }

    fn refuse(&mut self, from: PeerId, why: &str) {
        let _ = self.t.send(
            from,
            ControlMsg::of(&ToGuest::Refused {
                why: why.to_owned(),
            }),
        );
        self.closing.push((from, self.now_ns + CLOSE_AFTER_NS));
    }

    fn member_said(&mut self, from: PeerId, id: u32, msg: ControlMsg, now: i64) {
        let Ok(m) = msg.read::<ToHost>() else {
            return;
        };
        if check_to_host(&m).is_err() {
            return;
        }
        if let ToHost::Hello { .. } = m {
            // Back on the same connection's peer (a reconnect): through the door again.
            self.member_of.remove(&from);
            self.at_door(from, msg, now);
            return;
        }
        let asked = matches!(m, ToHost::Ask { .. });
        let c = self.room.heard(id, now);
        self.apply(c);
        match self.room.handle(id, m, now) {
            Ok(c) => self.apply(c),
            Err(_) if asked => {
                // Refused (their role changed meanwhile): their view again, so their buttons
                // are right.
                let v = self.room.view_for(id);
                let _ = self
                    .t
                    .send(from, ControlMsg::of(&ToGuest::Room { room: v }));
            }
            Err(_) => {}
        }
    }

    /// Carry out what the room said, in order.
    fn apply(&mut self, c: Changes) {
        let now_ns = self.now_ns;
        for o in c.out {
            if o.to == HOST {
                self.conductor.message(&o.msg, now_ns);
            } else if let Some(&p) = self.peer_of.get(&o.to) {
                let _ = self.t.send(p, ControlMsg::of(&o.msg));
            }
        }
        for n in c.notes {
            if let HostNote::Slow { id } = n {
                self.slow.insert(id);
            }
            self.notes.push(n);
        }
        if c.volume.is_some() {
            self.volume = c.volume;
        }
        for id in c.gone {
            self.slow.remove(&id);
            if let Some(p) = self.peer_of.remove(&id) {
                self.member_of.remove(&p);
                self.closing.push((p, now_ns + CLOSE_AFTER_NS));
            }
        }
        if c.ended && !self.ended {
            self.ended = true;
            self.conductor.end();
            for (_, p) in self.peer_of.drain() {
                self.closing.push((p, now_ns + CLOSE_AFTER_NS));
            }
            self.member_of.clear();
        }
    }

    /// The window's view of this room (see [`window`]).
    pub fn window(&self, extras: &WindowExtras) -> View {
        let state = if self.ended {
            view::SessionState::Ended
        } else {
            view::SessionState::Hosting
        };
        let wire = self.room.view_for(HOST);
        window(Window {
            state,
            host: true,
            me: Some(HOST),
            wire: Some(&wire),
            conductor_mode: self.conductor.mode(),
            sub_state: self.conductor.status().map(|s| s.sub_state),
            error_us: self.conductor.status().and_then(|s| s.error_us),
            bound_us: self.conductor.status().and_then(|s| s.bound_us),
            stepped_out: self.conductor.stepped_out(),
            pressing: None,
            path: None,
            host_now_us: Some(us(self.clock.now_ns())),
            slow: &self.slow,
            why: None,
            host_away: false,
            extras,
        })
    }
}

// --- A guest ----------------------------------------------------------------------------------

/// What joining needs.
#[derive(Debug, Clone)]
pub struct GuestConfig {
    /// The name others see.
    pub name: String,
    /// The invite's proof for this computer's key in this room.
    pub proof: String,
    /// What this Venuta says it is ("Venuta 0.4.0").
    pub app: String,
    /// The host's name, from the invite (until the room says it).
    pub host_name: String,
}

struct Held {
    at_ns: u64,
    action: Action,
    request: bool,
}

/// A room joined from here.
pub struct GuestSession<I> {
    t: Arc<dyn Transport>,
    rx: Receiver<TransportEvent>,
    host: PeerId,
    clock: Arc<dyn MonoClock>,
    conductor: Conductor<I>,
    cfg: GuestConfig,
    stage: Stage,
    why: Option<String>,
    cl: Clock,
    pinger: Pinger,
    lost_at: Option<u64>,
    held: Vec<Held>,
    notes: Vec<String>,
    last_step_ns: u64,
    waking: bool,
    ready_at: Option<i64>,
    pressing: Option<(String, u64)>,
    path: Option<PathInfo>,
    close_at: Option<u64>,
    last_queue: Option<u32>,
    now_ns: u64,
}

impl<I: Clone + Send + 'static> GuestSession<I> {
    /// Join the room at `host` on transport `t` (connected, or connecting: the hello goes as
    /// soon as it can), this computer's clock `clock`, the person's conductor.
    pub fn new(
        t: Arc<dyn Transport>,
        host: PeerId,
        clock: Arc<dyn MonoClock>,
        conductor: Conductor<I>,
        cfg: GuestConfig,
    ) -> Self {
        let rx = t.events();
        let now_ns = clock.now_ns();
        let mut g = GuestSession {
            t,
            rx,
            host,
            clock,
            conductor,
            cfg,
            stage: Stage::Connecting,
            why: None,
            cl: Clock::new(),
            pinger: Pinger::new(host, now_ns),
            lost_at: None,
            held: Vec::new(),
            notes: Vec::new(),
            last_step_ns: now_ns,
            waking: false,
            ready_at: None,
            pressing: None,
            path: None,
            close_at: None,
            last_queue: None,
            now_ns,
        };
        g.hello();
        g
    }

    fn hello(&mut self) {
        let h = ToHost::Hello {
            protocol: v3::PROTOCOL,
            name: self.cfg.name.clone(),
            proof: self.cfg.proof.clone(),
            app: self.cfg.app.clone(),
        };
        if self.t.send(self.host, ControlMsg::of(&h)).is_ok() && self.stage == Stage::Connecting {
            self.stage = Stage::Knocking;
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Why it ended, in words.
    pub fn why(&self) -> Option<String> {
        self.why.clone()
    }

    pub fn me(&self) -> Option<u32> {
        self.conductor.me()
    }

    pub fn conductor(&mut self) -> &mut Conductor<I> {
        &mut self.conductor
    }

    /// The host's clock is known (this epoch).
    pub fn clock_ready(&self) -> bool {
        self.cl.ready()
    }

    /// When the clock was first ready, in the host's time (µs).
    pub fn clock_ready_at(&self) -> Option<i64> {
        self.ready_at
    }

    pub fn clock_epoch(&self) -> u32 {
        self.cl.epoch()
    }

    /// How the connection reaches the host, as the transport last said.
    pub fn path(&self) -> Option<PathInfo> {
        self.path.or_else(|| self.t.path(self.host))
    }

    /// Short notes for the window ("Your pause was too late to send.").
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// After the room: how many songs its queue had.
    pub fn last_queue(&self) -> Option<u32> {
        self.last_queue
    }

    /// A press: to the room now, or held while reconnecting (sent when back, if fresh).
    pub fn ask(&mut self, action: Action) {
        self.press(action, false);
    }

    /// Something asked of whoever controls (no role needed).
    pub fn request(&mut self, action: Action) {
        self.press(action, true);
    }

    fn press(&mut self, action: Action, request: bool) {
        if self.stage == Stage::Ended {
            return;
        }
        let now_ns = self.clock.now_ns();
        if !request && let Some(words) = pressing_of(&action) {
            self.pressing = Some((words, now_ns));
        }
        let m = if request {
            ToHost::Request {
                action: action.clone(),
            }
        } else {
            ToHost::Ask {
                action: action.clone(),
            }
        };
        if self.stage == Stage::In && self.t.send(self.host, ControlMsg::of(&m)).is_ok() {
            return;
        }
        if self.held.len() < 20 {
            self.held.push(Held {
                at_ns: now_ns,
                action,
                request,
            });
        }
        self.notes.push(
            "Reconnecting: your press will reach the room as soon as the connection is back."
                .into(),
        );
    }

    /// Leave: the goodbye, then the player let go at once, the connection closed a moment on.
    pub fn leave(&mut self) {
        let _ = self.t.send(self.host, ControlMsg::of(&ToHost::Bye));
        self.finish(None);
    }

    fn finish(&mut self, why: Option<String>) {
        if self.stage == Stage::Ended {
            return;
        }
        self.stage = Stage::Ended;
        self.why = why;
        self.last_queue = self
            .conductor
            .view()
            .map(|v| v.queue.len() as u32)
            .filter(|n| *n > 0);
        self.conductor.end();
        self.close_at = Some(self.clock.now_ns() + CLOSE_AFTER_NS);
    }

    /// Everything waiting on the network, then time.
    pub fn step(&mut self) {
        let now_ns = self.clock.now_ns();
        self.now_ns = now_ns;
        if self.stage == Stage::Ended {
            if self.close_at.is_some_and(|at| now_ns >= at) {
                self.close_at = None;
                self.t.close(self.host);
            }
            return;
        }
        if now_ns > self.last_step_ns + WOKE_AFTER_NS {
            // Slept: a new epoch, a burst, and the player out until the clock is ready again.
            self.pinger.woke(&mut self.cl, now_ns);
            self.cl.publish(self.conductor.clock());
            self.conductor.suspend(Suspension::Sleeping);
            self.waking = true;
        }
        self.last_step_ns = now_ns;
        while let Ok(e) = self.rx.try_recv() {
            self.event(e, now_ns);
            if self.stage == Stage::Ended {
                return;
            }
        }
        self.pinger.tick(self.t.as_ref(), self.clock.as_ref());
        if self.cl.ready() && self.ready_at.is_none() {
            self.ready_at = self.conductor.clock().load().map(|m| m.host_of(now_ns));
        }
        if self.waking && self.cl.ready() {
            self.waking = false;
            self.conductor.resume();
        }
        if let Some(at) = self.lost_at
            && now_ns >= at + GIVE_UP_NS
        {
            self.conductor.finish_at_song_end();
        }
        self.cl.set_steps(match self.conductor.mode() {
            ListenMode::EveryDevice => Steps::SameRoom,
            _ => Steps::Apart,
        });
        self.conductor.step(now_ns);
        if self.stage == Stage::In {
            for m in self.conductor.take_out() {
                let _ = self.t.send(self.host, ControlMsg::of(&m));
            }
        } else {
            // Reports while away are stale by the time it's back; the welcome asks again.
            let _ = self.conductor.take_out();
        }
        if self.conductor.ended() {
            let why = format!("Lost the connection to {}'s room.", self.host_name());
            self.finish(Some(why));
        }
        if self
            .pressing
            .as_ref()
            .is_some_and(|(_, at)| now_ns >= at + PRESSING_NS)
        {
            self.pressing = None;
        }
    }

    fn host_name(&self) -> String {
        self.conductor
            .view()
            .map(|v| v.host_name.clone())
            .unwrap_or_else(|| self.cfg.host_name.clone())
    }

    fn event(&mut self, e: TransportEvent, now_ns: u64) {
        if self.pinger.on_event(&e, &mut self.cl, now_ns) {
            self.cl.publish(self.conductor.clock());
        }
        match e {
            TransportEvent::Message { from, msg, .. } if from == self.host => {
                let Ok(m) = msg.read::<ToGuest>() else {
                    return;
                };
                if check_to_guest(&m).is_err() {
                    return;
                }
                self.host_said(m, now_ns);
            }
            TransportEvent::Joined { peer } if peer == self.host => {
                // Connected (again): knock; the host knows this key.
                self.hello();
            }
            TransportEvent::Lost { peer } if peer == self.host => {
                if self.stage != Stage::Reconnecting {
                    self.stage = Stage::Reconnecting;
                    self.lost_at = Some(now_ns);
                }
            }
            TransportEvent::PathChanged { peer, path } if peer == self.host => {
                self.path = Some(path);
            }
            _ => {}
        }
    }

    fn host_said(&mut self, m: ToGuest, now_ns: u64) {
        match &m {
            ToGuest::Waiting => self.stage = Stage::AtTheDoor,
            ToGuest::Welcome { .. } => {
                self.stage = Stage::In;
                self.lost_at = None;
                self.conductor.message(&m, now_ns);
                self.send_held(now_ns);
                return;
            }
            ToGuest::Refused { why } | ToGuest::Ended { why } => {
                let why = why.clone();
                self.conductor.message(&m, now_ns);
                self.finish(Some(why));
                return;
            }
            ToGuest::Commit { .. } | ToGuest::Hold { .. } => self.pressing = None,
            _ => {}
        }
        self.conductor.message(&m, now_ns);
    }

    /// Presses held while away: those still fresh go, the others are said too late.
    fn send_held(&mut self, now_ns: u64) {
        for h in std::mem::take(&mut self.held) {
            if now_ns < h.at_ns + HELD_FOR_NS {
                let m = if h.request {
                    ToHost::Request { action: h.action }
                } else {
                    ToHost::Ask { action: h.action }
                };
                let _ = self.t.send(self.host, ControlMsg::of(&m));
            } else {
                self.notes.push(too_late(&h.action));
            }
        }
    }

    /// The window's view of this room (see [`window`]).
    pub fn window(&self, extras: &WindowExtras) -> View {
        let state = match self.stage {
            Stage::Connecting | Stage::Knocking => view::SessionState::Joining,
            Stage::AtTheDoor => view::SessionState::Waiting,
            Stage::In if self.conductor.anchor().is_none() && self.conductor.view().is_none() => {
                view::SessionState::Joining
            }
            Stage::In => view::SessionState::In,
            Stage::Reconnecting => view::SessionState::Reconnecting,
            Stage::Ended => view::SessionState::Ended,
        };
        let map = **self.conductor.clock().load();
        let now = self.clock.now_ns();
        let st = self.conductor.status();
        window(Window {
            state,
            host: false,
            me: self.conductor.me(),
            wire: self.conductor.view(),
            conductor_mode: self.conductor.mode(),
            sub_state: st.map(|s| s.sub_state),
            error_us: st.and_then(|s| s.error_us),
            bound_us: st.and_then(|s| s.bound_us),
            stepped_out: self.conductor.stepped_out(),
            pressing: self.pressing.as_ref().map(|p| p.0.clone()),
            path: self.path(),
            host_now_us: map.map(|m| m.host_of(now)),
            slow: &HashSet::new(),
            why: self.why.clone(),
            host_away: self.stage == Stage::Reconnecting,
            extras,
        })
    }
}

// --- The window -------------------------------------------------------------------------------

/// What the window shows that the session doesn't know: the app's.
#[derive(Debug, Clone)]
pub struct WindowExtras {
    pub name: String,
    pub local_only: bool,
    pub let_in: bool,
    pub visible: bool,
    pub output: view::OutputView,
    pub invite: Option<view::InviteView>,
    /// The host's speaker's volume (0 to 100).
    pub volume: Option<u8>,
    /// What went wrong, if the app has something to say.
    pub message: Option<String>,
}

impl Default for WindowExtras {
    fn default() -> Self {
        WindowExtras {
            name: String::new(),
            local_only: false,
            let_in: false,
            visible: false,
            output: view::OutputView {
                name: String::new(),
                delay_ms: 0,
                bluetooth: false,
                delay_known: true,
            },
            invite: None,
            volume: None,
            message: None,
        }
    }
}

struct Window<'a> {
    state: view::SessionState,
    host: bool,
    me: Option<u32>,
    wire: Option<&'a WireRoom>,
    conductor_mode: ListenMode,
    sub_state: Option<SubState>,
    error_us: Option<i64>,
    bound_us: Option<u32>,
    stepped_out: bool,
    pressing: Option<String>,
    path: Option<PathInfo>,
    host_now_us: Option<i64>,
    slow: &'a HashSet<u32>,
    why: Option<String>,
    host_away: bool,
    extras: &'a WindowExtras,
}

fn ms_of(us: Option<i64>) -> Option<f32> {
    us.map(|u| u as f32 / 1000.0)
}

/// What a request asks, in words ("Ravi asks to skip").
fn request_words(name: &str, a: &Action, queue: &[v3::Entry]) -> String {
    let what = match a {
        Action::Play => "play".to_owned(),
        Action::Pause => "pause".to_owned(),
        Action::Seek { content_us } => {
            let s = (*content_us).max(0) / 1_000_000;
            format!("move to {}:{:02}", s / 60, s % 60)
        }
        Action::Next => "skip".to_owned(),
        Action::Previous => "go back".to_owned(),
        Action::Jump { entry } => match queue.iter().find(|e| e.id == *entry) {
            Some(e) => format!("play “{}”", e.song.title),
            None => "play another song".to_owned(),
        },
        Action::Add { .. } => "add songs".to_owned(),
        Action::Remove { .. } => "take a song out".to_owned(),
        Action::Move { .. } => "move a song".to_owned(),
        Action::Volume { .. } => "change the volume".to_owned(),
        Action::Accept { .. } | Action::Decline { .. } => "answer a request".to_owned(),
    };
    format!("{name} asks to {what}")
}

fn member_view(
    m: &Member,
    me: Option<u32>,
    host_name: &str,
    slow: &HashSet<u32>,
) -> view::MemberView {
    let (e, b) = (ms_of(m.error_us), m.bound_us.map(|b| b as f32 / 1000.0));
    let line = if Some(m.id) == me {
        view::line(m.sub_state, m.mode, m.away, e, b, host_name)
    } else {
        view::others_line(m.sub_state, m.mode, m.away, e, b, host_name)
    };
    view::MemberView {
        id: m.id,
        name: m.name.clone(),
        host: m.host,
        role: m.role,
        mode: m.mode,
        sub_state: m.sub_state,
        away: m.away,
        line,
        copy: m.copy.map(CopyView::from),
        error_ms: e,
        bound_ms: b,
        slow: slow.contains(&m.id),
        // Handing over is WP20's: nobody can take the room over yet.
        hand_over: None,
    }
}

/// The window's `View` of a session as it is.
fn window(w: Window<'_>) -> View {
    let x = w.extras;
    let host_name = w.wire.map(|r| r.host_name.clone()).unwrap_or_default();
    let room = w.wire.map(|r| view::RoomView {
        host_name: r.host_name.clone(),
        members: r
            .members
            .iter()
            .map(|m| member_view(m, w.me, &r.host_name, w.slow))
            .collect(),
        waiting: r.waiting.clone(),
        queue: r
            .queue
            .iter()
            .map(|e| view::EntryView {
                id: e.id,
                song: e.song.clone(),
            })
            .collect(),
        anchor: r.anchor,
        next: r.next,
        transition: r.settings.transition,
        activity: r.activity.clone(),
        new_role: r.settings.new_role,
        starting: r.starting.clone(),
        host_away: w.host_away || r.members.iter().any(|m| m.host && m.away),
        phase: match r.phase {
            WirePhase::Resting => view::Phase::Resting,
            WirePhase::QueueEnded => view::Phase::QueueEnded,
            WirePhase::Playing => view::Phase::Playing,
            WirePhase::Held => view::Phase::Held,
        },
        requests: r
            .requests
            .iter()
            .map(|q| view::RequestView {
                id: q.id,
                from: q.from,
                words: request_words(
                    &r.members
                        .iter()
                        .find(|m| m.id == q.from)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| "Someone".into()),
                    &q.action,
                    &r.queue,
                ),
            })
            .collect(),
        auto_roles: r.settings.auto_roles,
        guests_volume: r.settings.guests_volume,
        volume: x.volume,
    });
    let in_room = matches!(
        w.state,
        view::SessionState::Hosting | view::SessionState::In | view::SessionState::Reconnecting
    );
    let you = (in_room || w.state == view::SessionState::Joining).then(|| {
        let sub = if w.conductor_mode == ListenMode::OneSpeaker {
            SubState::Remote
        } else {
            w.sub_state.unwrap_or(SubState::Opening)
        };
        let stepped = match (w.stepped_out, sub) {
            (true, _) => Some(view::StepOut::Chosen),
            (false, SubState::SteppedOut) => Some(view::StepOut::SleepTimer),
            _ => None,
        };
        let line = match stepped {
            Some(why) => view::stepped_out_line(why),
            None => view::line(
                Some(sub),
                w.conductor_mode,
                false,
                ms_of(w.error_us),
                w.bound_us.map(|b| b as f32 / 1000.0),
                &host_name,
            ),
        };
        let near = w
            .path
            .is_some_and(|p| p.direct && !p.relay && p.rtt_us.is_some_and(|r| r < 8_000));
        let choices = if !w.host && near {
            [
                ListenMode::OneSpeaker,
                ListenMode::EveryDevice,
                ListenMode::Apart,
            ]
            .into_iter()
            .map(|mode| view::ModeChoice {
                mode,
                why_not: None,
            })
            .collect()
        } else {
            Vec::new()
        };
        view::You {
            mode: w.conductor_mode,
            sub_state: sub,
            line,
            pressing: w.pressing.clone(),
            joining_pct: None,
            choices,
            stepped_out: stepped,
        }
    });
    View {
        state: w.state,
        host: w.host,
        me: w.me,
        room,
        invite: x.invite.clone(),
        path: w.path.map(|p| view::PathView {
            direct: p.direct,
            relay: p.relay,
            rtt_ms: p.rtt_us.map(|r| r.div_ceil(1_000)),
        }),
        message: w.why.or_else(|| x.message.clone()),
        name: x.name.clone(),
        local_only: x.local_only,
        let_in: x.let_in,
        you,
        output: x.output.clone(),
        host_now_us: w.host_now_us,
        nearby: Vec::new(),
        visible: x.visible,
        resume: None,
        last_queue: None,
    }
}

/// The song and place the room plays now, for the player's own bar while in a room: the
/// entry, its song, the room's content now (µs) and whether it plays.
pub fn now_playing<I: Clone + Send + 'static>(
    c: &Conductor<I>,
    host_now_us: Option<i64>,
) -> Option<(EntryId, Song, i64, bool)> {
    let a: Anchor = c.anchor()?;
    let song = c
        .view()?
        .queue
        .iter()
        .find(|e| e.id == a.entry)?
        .song
        .clone();
    let content = host_now_us.map_or(a.content_us, |h| a.content_at(h));
    Some((a.entry, song, content.max(0), a.playing))
}
