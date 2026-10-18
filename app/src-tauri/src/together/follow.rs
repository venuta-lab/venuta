//! Each person's player follows the room (the host's too): it gets each song ready where the
//! room asks, says when it's ready, starts at the room's moment, and stays there (the engine's
//! sync plan). The next song is readied ahead and follows without a gap.

use super::sources::{self, Copy, HostStream};
use crate::playback::{Playback, RoomTick};
use engine::library::Library;
use media_access::MediaAccess;
use player::sync::SyncPlan;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use together::proto::{Hearing, RoomView, Song};
use together::timeline::Timeline;

/// What the follower hears.
pub enum In {
    Room(RoomView),
    Prepare {
        rev: u64,
        index: usize,
        position_ms: u64,
    },
    Tick(RoomTick),
    StepOut,
    CatchUp,
    Delay(i32),
    Stop,
}

/// What the follower tells the room (the host's room directly; a guest's through the host).
pub trait Report: Send {
    fn ready(&self, rev: u64, index: usize);
    fn hearing(&self, rev: u64, index: usize, how: Hearing);
    fn sync(&self, error_ms: i32);
    /// The playing song's real length (only the host's counts).
    fn length(&self, _ms: u64) {}
}

pub struct Follower {
    pub app: AppHandle,
    pub root: String,
    pub lib: Option<Arc<Library>>,
    pub catalog: Arc<dyn MediaAccess>,
    pub local_only: bool,
    /// A guest's way to the host's files.
    pub host_stream: Option<Arc<HostStream>>,
    /// The host's own: its files by key.
    pub hosting: bool,
    /// The room's time (microseconds) as a moment on this computer, once the clock is known.
    pub to_local: Arc<dyn Fn(i64) -> Option<Instant> + Send + Sync>,
    pub report: Box<dyn Report>,
    pub delay_ms: i32,
    /// Its hold on the player (set as it starts).
    pub owner: u64,
}

#[derive(Clone)]
struct Loaded {
    index: usize,
    key: String,
}

struct State {
    queue: Vec<Song>,
    same_room: bool,
    timeline: Option<Timeline>,
    /// The song in the player now (its room entry), and the next one readied after it.
    now: Option<Loaded>,
    next: Option<Loaded>,
    /// Waiting to say ready: (rev, index, for the next song).
    pending: Option<(u64, usize, bool)>,
    stepped_out: bool,
    last_sync_report: Instant,
    /// The host's files by key (the host's own copies).
    own_keys: HashMap<String, usize>,
    /// When the player should have moved on to the next song (its start), to nudge it if not.
    next_due: Option<Instant>,
    /// The start time the player was last given for the song playing (to see when a better
    /// estimate of the host's clock moves it).
    anchor: Option<Instant>,
    /// How each song is heard here (by its key), to say again once the player plays after
    /// failing.
    heard: HashMap<String, Hearing>,
    /// The player stopped on an error with the room's song (since when, to try again).
    failing: Option<Instant>,
    player_error: bool,
}

/// How long between tries while the player can't play (the output may be free again).
const RETRY: Duration = Duration::from_secs(5);

impl Follower {
    fn pb(&self) -> tauri::State<'_, Playback> {
        self.app.state::<Playback>()
    }

    /// The copy this person plays of a room song.
    fn copy_of(&self, song: &Song, st: &State) -> Option<Copy> {
        // The host plays its own file for its own songs.
        if self.hosting && song.host_file {
            let lib = self.lib.as_ref()?;
            let i = *st.own_keys.get(&song.key)?;
            return Some(Copy {
                item: sources::item_of_track(&lib.tracks[i]),
                own: None,
                hearing: Hearing::Local,
            });
        }
        sources::best_copy(
            song,
            self.lib.as_deref(),
            &self.catalog,
            self.local_only,
            self.host_stream.clone(),
        )
    }

    fn plan(&self, t: &Timeline, same_room: bool) -> Option<SyncPlan> {
        Some(SyncPlan {
            anchor: Some((self.to_local)(t.at_us)?),
            position_ms: t.position_ms,
            playing: t.playing,
            same_room,
            extra_latency_ms: self.delay_ms,
        })
    }

    /// The player follows this plan (its start time kept, to see when the clock moves it).
    fn follow_plan(&self, st: &mut State, plan: SyncPlan) {
        st.anchor = plan.anchor;
        let _ = self.pb().room_sync(Some(plan));
    }

    /// The host's clock is known better now (or at all, just after joining): the song playing
    /// follows the room's start by it, when it moved enough to matter.
    fn follow_the_clock(&self, st: &mut State) {
        let Some(t) = st.timeline else { return };
        if st.stepped_out
            || st.pending.is_some()
            || !st.now.as_ref().is_some_and(|n| n.index == t.index)
        {
            return;
        }
        let Some(plan) = self.plan(&t, st.same_room) else {
            return;
        };
        let tolerance = Duration::from_micros(if st.same_room { 1_000 } else { 5_000 });
        let moved = match (st.anchor, plan.anchor) {
            (None, Some(_)) => true,
            (Some(a), Some(b)) => a.max(b) - a.min(b) > tolerance,
            _ => false,
        };
        if moved {
            self.follow_plan(st, plan);
        }
    }

    /// Get this entry ready at `position_ms`, silent, and wait to say so.
    fn load(&self, st: &mut State, rev: u64, index: usize, position_ms: u64) {
        let Some(song) = st.queue.get(index).cloned() else {
            super::trace("follow: no such song in the queue");
            return;
        };
        st.next = None;
        st.next_due = None;
        match self.copy_of(&song, st) {
            Some(c) => {
                let _ = self.pb().room_load(&self.root, c.item, position_ms, c.own);
                st.anchor = None;
                let _ = self.pb().room_sync(Some(SyncPlan {
                    anchor: None,
                    position_ms,
                    playing: true,
                    same_room: st.same_room,
                    extra_latency_ms: self.delay_ms,
                }));
                st.now = Some(Loaded {
                    index,
                    key: song.key.clone(),
                });
                st.pending = Some((rev, index, false));
                self.report.hearing(rev, index, Hearing::Opening);
                self.report_hearing_later(st, rev, index, c.hearing);
            }
            None => {
                let _ = self.pb().room_sync(None);
                let _ = self.pb().room_pause();
                st.now = None;
                st.pending = None;
                self.report.hearing(rev, index, Hearing::Cannot);
            }
        }
    }

    /// The next entry, readied after the one playing (to follow it at its end).
    fn load_next(&self, st: &mut State, rev: u64, index: usize) {
        let Some(song) = st.queue.get(index).cloned() else {
            return;
        };
        match self.copy_of(&song, st) {
            Some(c) => {
                let _ = self.pb().room_queue_next(&self.root, c.item, c.own);
                st.next = Some(Loaded {
                    index,
                    key: song.key.clone(),
                });
                st.pending = Some((rev, index, true));
                self.report_hearing_later(st, rev, index, c.hearing);
            }
            None => {
                st.next = None;
                self.report.hearing(rev, index, Hearing::Cannot);
            }
        }
    }

    fn report_hearing_later(&self, st: &mut State, rev: u64, index: usize, how: Hearing) {
        // Said once the song is ready (until then it's opening).
        HEARING.with(|h| h.borrow_mut().insert((rev, index), how));
        if let Some(song) = st.queue.get(index) {
            st.heard.insert(song.key.clone(), how);
        }
    }

    /// The player stopped on an error with the room's song: the room says this person can't
    /// hear it, and it's tried again where the room is now and then; once it plays in step
    /// again, how it's heard is said again.
    fn watch_failure(&self, st: &mut State, in_step: bool) {
        let Some(t) = st.timeline else { return };
        let ours = !st.stepped_out && st.now.as_ref().is_some_and(|n| n.index == t.index);
        if !ours {
            if !st.player_error {
                st.failing = None;
            }
            return;
        }
        if st.player_error {
            match st.failing {
                None => {
                    st.failing = Some(Instant::now());
                    self.report.hearing(t.rev, t.index, Hearing::Cannot);
                }
                Some(since) if since.elapsed() > RETRY => {
                    st.failing = Some(Instant::now());
                    st.player_error = false;
                    st.now = None;
                    self.apply_timeline(st, t);
                }
                Some(_) => {}
            }
        } else if st.failing.is_some() && in_step {
            st.failing = None;
            if let Some(how) = st.queue.get(t.index).and_then(|s| st.heard.get(&s.key)) {
                self.report.hearing(t.rev, t.index, *how);
            }
        }
    }

    pub fn run(self, rx: Receiver<In>) {
        let mut st = State {
            queue: Vec::new(),
            same_room: false,
            timeline: None,
            now: None,
            next: None,
            pending: None,
            stepped_out: false,
            last_sync_report: Instant::now(),
            own_keys: HashMap::new(),
            next_due: None,
            anchor: None,
            heard: HashMap::new(),
            failing: None,
            player_error: false,
        };
        if self.hosting
            && let Some(lib) = &self.lib
        {
            st.own_keys = lib
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| (Song::host_key(&t.path), i))
                .collect();
        }
        let mut me = self;
        while let Ok(m) = rx
            .recv_timeout(Duration::from_millis(250))
            .or_else(|e| match e {
                std::sync::mpsc::RecvTimeoutError::Timeout => Ok(In::Tick(RoomTick::idle())),
                std::sync::mpsc::RecvTimeoutError::Disconnected => Err(()),
            })
        {
            match m {
                In::Stop => break,
                In::Delay(ms) => {
                    me.delay_ms = ms;
                    if let Some(t) = st.timeline {
                        me.apply_timeline(&mut st, t);
                    }
                }
                In::StepOut => {
                    st.stepped_out = true;
                    let _ = me.pb().room_sync(None);
                    let _ = me.pb().room_pause();
                    if let Some(t) = st.timeline {
                        me.report.hearing(t.rev, t.index, Hearing::SteppedOut);
                    }
                }
                In::CatchUp => {
                    st.stepped_out = false;
                    st.now = None;
                    if let Some(t) = st.timeline {
                        me.apply_timeline(&mut st, t);
                    }
                }
                In::Prepare {
                    rev,
                    index,
                    position_ms,
                } => {
                    super::trace(&format!("follow: prepare {index} at {position_ms}"));
                    if st.stepped_out {
                        continue;
                    }
                    let ahead = st.timeline.is_some_and(|t| t.playing)
                        && position_ms == 0
                        && st.now.as_ref().is_some_and(|n| {
                            n.index + 1 == index
                                && st.queue.get(n.index).is_some_and(|s| s.key == n.key)
                        });
                    if ahead {
                        me.load_next(&mut st, rev, index);
                    } else {
                        me.load(&mut st, rev, index, position_ms);
                    }
                }
                In::Room(view) => {
                    st.queue = view.queue;
                    let way_changed = st.same_room != view.same_room;
                    st.same_room = view.same_room;
                    // Same room switched on or off: the player follows the new way at once.
                    if way_changed
                        && !st.stepped_out
                        && view.timeline == st.timeline
                        && let Some(t) = st.timeline
                        && st.now.as_ref().is_some_and(|n| n.index == t.index)
                        && let Some(plan) = me.plan(&t, st.same_room)
                    {
                        me.follow_plan(&mut st, plan);
                    }
                    match view.timeline {
                        Some(t) if Some(t) != st.timeline => {
                            st.timeline = Some(t);
                            if !st.stepped_out {
                                me.apply_timeline(&mut st, t);
                            }
                        }
                        Some(_) => {}
                        None => st.timeline = None,
                    }
                }
                In::Tick(tick) => me.tick(&mut st, tick),
            }
        }
        // On its own again, unless the next room's follower has the player already.
        let pb = me.pb();
        if pb.release_room(me.owner) {
            let _ = pb.room_sync(None);
            pb.set_room_tap(None);
            pb.set_room_buttons(None);
        }
    }

    /// The room's timeline changed: follow it with what's loaded, or get it.
    fn apply_timeline(&self, st: &mut State, t: Timeline) {
        let key = st.queue.get(t.index).map(|s| s.key.clone());
        let loaded_now = st
            .now
            .as_ref()
            .is_some_and(|n| n.index == t.index && Some(&n.key) == key.as_ref());
        let loaded_next = st
            .next
            .as_ref()
            .is_some_and(|n| n.index == t.index && Some(&n.key) == key.as_ref());
        if loaded_now {
            if let Some(plan) = self.plan(&t, st.same_room) {
                self.follow_plan(st, plan);
            }
        } else if loaded_next {
            // It follows the one playing at its end; the player moves on by itself then.
            st.next_due = self.to_local.as_ref()(t.at_us);
        } else if key.is_some() {
            // Not loaded (joined mid-song, back after stepping out): get it where the room is.
            self.load(st, t.rev, t.index, t.position_ms);
            if let Some(plan) = self.plan(&t, st.same_room) {
                self.follow_plan(st, plan);
            }
            // Playing at once (no start to wait for): say how it's heard now (after failing,
            // once it plays).
            if st.pending.take().is_some()
                && st.failing.is_none()
                && let Some(how) = HEARING.with(|h| h.borrow_mut().remove(&(t.rev, t.index)))
            {
                self.report.hearing(t.rev, t.index, how);
            }
        }
    }

    fn tick(&self, st: &mut State, tick: RoomTick) {
        if tick.idle {
            // No status lately: just see whether a next song is overdue.
        } else {
            // Ready to say?
            if let (Some((rev, index, ahead)), Some(sync)) = (st.pending, tick.sync) {
                let ready = if ahead { sync.next_ready } else { sync.ready };
                if ready {
                    st.pending = None;
                    self.report.ready(rev, index);
                    if let Some(how) = HEARING.with(|h| h.borrow_mut().remove(&(rev, index))) {
                        self.report.hearing(rev, index, how);
                    }
                }
            }
            // The player moved on to the readied next song: it follows the room from there.
            if tick.index == Some(1)
                && let Some(next) = st.next.take()
            {
                st.now = Some(next);
                st.next_due = None;
                if let Some(t) = st.timeline
                    && st.now.as_ref().is_some_and(|n| n.index == t.index)
                    && let Some(plan) = self.plan(&t, st.same_room)
                {
                    self.follow_plan(st, plan);
                }
            }
            // The host's player knows the song's real length.
            if self.hosting
                && tick.duration_ms > 0
                && st
                    .timeline
                    .is_some_and(|t| st.now.as_ref().is_some_and(|n| n.index == t.index))
            {
                self.report.length(tick.duration_ms);
            }
            // How far off, now and then.
            if let Some(e) = tick.sync.and_then(|s| s.error_ms)
                && st.last_sync_report.elapsed() > Duration::from_secs(2)
            {
                st.last_sync_report = Instant::now();
                self.report.sync(e.round() as i32);
            }
        }
        if !tick.idle {
            st.player_error = tick.error;
        }
        self.watch_failure(
            st,
            tick.sync.is_some_and(|s| s.error_ms.is_some()) && !tick.error,
        );
        self.follow_the_clock(st);
        // This copy is longer than the room's: the room has moved on, so does this player.
        if let Some(due) = st.next_due
            && Instant::now() > due + Duration::from_millis(400)
            && st.next.is_some()
        {
            st.next_due = None;
            let _ = self.pb().room_next();
        }
    }
}

thread_local! {
    /// How each prepared song will be heard, said once it's ready.
    static HEARING: std::cell::RefCell<HashMap<(u64, usize), Hearing>> = Default::default();
}

/// Start a follower on its own thread; its player's statuses come to it.
pub fn start(mut f: Follower) -> Sender<In> {
    let (tx, rx) = std::sync::mpsc::channel::<In>();
    let (tap_tx, tap_rx) = std::sync::mpsc::channel::<RoomTick>();
    f.owner = f.app.state::<Playback>().claim_room();
    f.app.state::<Playback>().set_room_tap(Some(tap_tx));
    super::take_buttons(&f.app);
    let fwd = tx.clone();
    std::thread::Builder::new()
        .name("venuta-room-tap".into())
        .spawn(move || {
            for t in tap_rx {
                if fwd.send(In::Tick(t)).is_err() {
                    break;
                }
            }
        })
        .ok();
    std::thread::Builder::new()
        .name("venuta-room-follow".into())
        .spawn(move || f.run(rx))
        .ok();
    tx
}
