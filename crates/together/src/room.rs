//! The room, as the host's Venuta keeps it: the one place that decides what plays.
//!
//! People knock (with the invite's proof), wait for the host or come straight in, and get a
//! role. What anyone asks is checked against their role, done, and written in the activity line
//! so everyone sees who did what. A new song is first *prepared*: everyone gets it ready, says
//! so, and the room starts it for all at one moment (waiting a few seconds at most; whoever is
//! late catches up alone). Songs follow each other by the room's own clock: the next is prepared
//! a little before the end and starts exactly when the last one ends.
//!
//! Pure: the time is passed in (the host's clock, microseconds), and what changed is returned
//! for the app to send.

use crate::MAX_PEOPLE;
use crate::invite::proof_ok;
use crate::proto::{Action, Activity, Hearing, MAX_QUEUE, Member, Role, RoomView, Song};
use crate::timeline::{Timeline, lead_us};
use std::collections::{HashSet, VecDeque};

/// The host's member id.
pub const HOST: u32 = 0;
/// How long a start waits for everyone to be ready before going without the slow ones.
pub const READY_WAIT_US: i64 = 2_500_000;
/// How long before a song ends the next one is prepared.
pub const PREPARE_AHEAD_US: i64 = 10_000_000;
/// How many activity lines are kept.
const ACTIVITY: usize = 6;

#[derive(Debug, Clone)]
struct Person {
    id: u32,
    /// Their connection's key (stable for one Venuta), for blocking.
    key: String,
    name: String,
    role: Role,
    hearing: Hearing,
    error_ms: Option<i32>,
    away: bool,
}

#[derive(Debug, Clone)]
struct Preparing {
    rev: u64,
    index: usize,
    position_ms: u64,
    /// Who it still waits for.
    waiting: HashSet<u32>,
    since_us: i64,
    /// For a next song prepared ahead: when it starts (the last one's end), else as soon as ready.
    starts_at_us: Option<i64>,
    /// The same song as it was known before the queue changed (a Ready for it still counts).
    also: Vec<(u64, usize)>,
}

#[derive(Debug, Clone)]
struct Undo {
    queue: Vec<Song>,
    timeline: Option<Timeline>,
}

/// What a knock at the door comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Knock {
    /// Let in straight away (the host lets people with the link in without asking).
    In(u32),
    /// Waiting for the host.
    Waiting(u32),
    /// Not let in, with why.
    Refused(String),
}

/// What changed, for the app to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// The room view changed: send it to everyone.
    pub room: bool,
    /// Everyone should get this entry ready at this position (and answer Ready).
    pub prepare: Option<(u64, usize, u64)>,
}

/// Within this, the room panel says someone is in sync (past it: catching up).
const SHOWN_IN_SYNC_MS: i32 = 60;

impl Changes {
    fn room() -> Self {
        Changes {
            room: true,
            prepare: None,
        }
    }
    fn merge(mut self, o: Changes) -> Self {
        self.room |= o.room;
        self.prepare = o.prepare.or(self.prepare);
        self
    }
}

#[derive(Debug, Clone)]
pub struct Room {
    host_name: String,
    secret: String,
    people: Vec<Person>,
    waiting: Vec<Person>,
    blocked: HashSet<String>,
    queue: Vec<Song>,
    rev: u64,
    timeline: Option<Timeline>,
    /// The playing song's length, as the host's player knows it (milliseconds).
    length_ms: Option<u64>,
    preparing: Option<Preparing>,
    activity: VecDeque<Activity>,
    undo: Option<Undo>,
    new_role: Role,
    same_room: bool,
    let_in_without_asking: bool,
    /// The slowest round trip among the guests (microseconds), for how far ahead to start.
    slowest_rtt_us: i64,
    next_id: u32,
}

impl Room {
    /// A new room, with the host's queue (what was playing becomes the room's).
    pub fn new(host_name: &str, secret: &str, queue: Vec<Song>) -> Self {
        let host = Person {
            id: HOST,
            key: String::new(),
            name: host_name.to_owned(),
            role: Role::Control,
            hearing: Hearing::Opening,
            error_ms: None,
            away: false,
        };
        let mut queue = queue;
        queue.truncate(MAX_QUEUE);
        Room {
            host_name: host_name.to_owned(),
            secret: secret.to_owned(),
            people: vec![host],
            waiting: Vec::new(),
            blocked: HashSet::new(),
            queue,
            rev: 1,
            timeline: None,
            length_ms: None,
            preparing: None,
            activity: VecDeque::new(),
            undo: None,
            new_role: Role::Control,
            same_room: false,
            let_in_without_asking: false,
            slowest_rtt_us: 0,
            next_id: 1,
        }
    }

    // --- Who's here ------------------------------------------------------------------------

    /// Someone knocks: their connection key, the name they give, and their invite proof.
    pub fn knock(&mut self, key: &str, name: &str, proof: &str) -> Knock {
        if !proof_ok(&self.secret, key, proof) {
            return Knock::Refused("That invite isn't for this room.".into());
        }
        if self.blocked.contains(key) {
            return Knock::Refused("The host isn't letting you in.".into());
        }
        // Back after a drop: the same person, at once.
        if let Some(p) = self
            .people
            .iter_mut()
            .find(|p| p.key == key && p.id != HOST)
        {
            p.away = false;
            return Knock::In(p.id);
        }
        if self.people.len() >= MAX_PEOPLE {
            return Knock::Refused(format!("The room is full ({MAX_PEOPLE} people)."));
        }
        if let Some(w) = self.waiting.iter().find(|w| w.key == key) {
            return Knock::Waiting(w.id);
        }
        let id = self.next_id;
        self.next_id += 1;
        let person = Person {
            id,
            key: key.to_owned(),
            name: name.trim().to_owned(),
            role: self.new_role,
            hearing: Hearing::Opening,
            error_ms: None,
            away: false,
        };
        if self.let_in_without_asking {
            self.people.push(person);
            Knock::In(id)
        } else {
            self.waiting.push(person);
            Knock::Waiting(id)
        }
    }

    /// The host lets someone in.
    pub fn admit(&mut self, id: u32) -> Option<Changes> {
        let i = self.waiting.iter().position(|w| w.id == id)?;
        if self.people.len() >= MAX_PEOPLE {
            return None;
        }
        let p = self.waiting.remove(i);
        self.people.push(p);
        Some(Changes::room())
    }

    /// The host says no (they can ask again with the link).
    pub fn deny(&mut self, id: u32) -> Changes {
        self.waiting.retain(|w| w.id != id);
        Changes::room()
    }

    /// Someone leaves (or the host removes them; `block` keeps them out of this room).
    pub fn remove(&mut self, id: u32, block: bool) -> Changes {
        if id == HOST {
            return Changes::default();
        }
        if block
            && let Some(p) = self
                .people
                .iter()
                .chain(self.waiting.iter())
                .find(|p| p.id == id)
        {
            self.blocked.insert(p.key.clone());
        }
        self.people.retain(|p| p.id != id);
        self.waiting.retain(|p| p.id != id);
        if let Some(prep) = &mut self.preparing {
            prep.waiting.remove(&id);
        }
        Changes::room()
    }

    /// They said goodbye: out at once (no waiting for them to come back).
    pub fn left(&mut self, id: u32, now_us: i64) -> Changes {
        if id == HOST || !self.is_member(id) {
            return self.remove(id, false);
        }
        self.note(id, "left", false, now_us);
        self.remove(id, false)
    }

    /// Their connection dropped: shown as away until they're back (or removed).
    pub fn away(&mut self, id: u32) -> Changes {
        if let Some(p) = self.person(id) {
            p.away = true;
        }
        if let Some(prep) = &mut self.preparing {
            prep.waiting.remove(&id);
        }
        Changes::room()
    }

    pub fn set_role(&mut self, id: u32, role: Role) -> Changes {
        if id != HOST
            && let Some(p) = self.person(id)
        {
            p.role = role;
        }
        Changes::room()
    }

    pub fn set_new_role(&mut self, role: Role) -> Changes {
        self.new_role = role;
        Changes::room()
    }

    pub fn set_same_room(&mut self, on: bool) -> Changes {
        self.same_room = on;
        Changes::room()
    }

    pub fn lets_in_without_asking(&self) -> bool {
        self.let_in_without_asking
    }

    pub fn set_let_in_without_asking(&mut self, on: bool) {
        self.let_in_without_asking = on;
    }

    pub fn is_member(&self, id: u32) -> bool {
        self.people.iter().any(|p| p.id == id)
    }

    pub fn role_of(&self, id: u32) -> Option<Role> {
        self.people.iter().find(|p| p.id == id).map(|p| p.role)
    }

    /// The people in the room now (the host first), for sending to.
    pub fn member_ids(&self) -> Vec<u32> {
        self.people.iter().map(|p| p.id).collect()
    }

    fn person(&mut self, id: u32) -> Option<&mut Person> {
        self.people.iter_mut().find(|p| p.id == id)
    }

    // --- What people report -----------------------------------------------------------------

    pub fn hearing(&mut self, id: u32, rev: u64, index: usize, how: Hearing) -> Changes {
        let current = self.timeline.map(|t| (t.rev, t.index)) == Some((rev, index))
            || self.preparing.as_ref().map(|p| (p.rev, p.index)) == Some((rev, index));
        if !current {
            return Changes::default();
        }
        if let Some(p) = self.person(id) {
            p.hearing = how;
        }
        // Someone who can't play it isn't waited for.
        if how == Hearing::Cannot
            && let Some(prep) = &mut self.preparing
        {
            prep.waiting.remove(&id);
        }
        Changes::room()
    }

    /// How far off someone is, as their player says. Everyone's panel shows it, so the view goes
    /// out when what it shows changes: in sync or catching up (60 ms), or the number by 10 ms.
    pub fn sync(&mut self, id: u32, error_ms: i32) -> Changes {
        let Some(p) = self.person(id) else {
            return Changes::default();
        };
        let was = p.error_ms.replace(error_ms);
        let shown = |e: i32| e.abs() <= SHOWN_IN_SYNC_MS;
        match was {
            Some(w) if shown(w) == shown(error_ms) && (w - error_ms).abs() < 10 => {
                Changes::default()
            }
            _ => Changes::room(),
        }
    }

    /// A guest's round trip, for how far ahead starts are set: the slowest lately, easing back
    /// down after one slow reading (so one bad moment doesn't slow every start after it).
    pub fn round_trip(&mut self, rtt_us: i64) {
        let eased = self.slowest_rtt_us - (self.slowest_rtt_us - rtt_us).max(0) / 8;
        self.slowest_rtt_us = rtt_us.max(eased);
    }

    /// The playing song's real length, from the host's player.
    pub fn set_length(&mut self, ms: u64) {
        self.length_ms = Some(ms);
    }

    /// Someone has the prepared song ready.
    pub fn ready(&mut self, id: u32, rev: u64, index: usize, now_us: i64) -> Changes {
        let Some(prep) = &mut self.preparing else {
            return Changes::default();
        };
        if (prep.rev, prep.index) != (rev, index) && !prep.also.contains(&(rev, index)) {
            return Changes::default();
        }
        prep.waiting.remove(&id);
        self.start_if_ready(now_us)
    }

    // --- Doing things ------------------------------------------------------------------------

    /// Someone asks the room to do something. Their role is checked; the host may do anything.
    pub fn ask(&mut self, id: u32, action: Action, now_us: i64) -> Result<Changes, String> {
        let Some(role) = self.role_of(id) else {
            return Err("You're not in the room.".into());
        };
        let allowed = id == HOST
            || match action.needs() {
                Role::Control => role == Role::Control,
                Role::Add => matches!(role, Role::Control | Role::Add),
                Role::Listen => true,
            };
        if !allowed {
            return Err(match action.needs() {
                Role::Add => "The host hasn't let you add songs.".into(),
                _ => "The host is in control.".into(),
            });
        }
        Ok(match action {
            Action::Play => {
                // A song on its way already: it stays on its way (pressing again because it
                // seemed slow must never send the room back to the song before).
                if self
                    .preparing
                    .as_ref()
                    .is_some_and(|p| p.starts_at_us.is_none())
                {
                    return Ok(Changes::default());
                }
                let Some(t) = self.timeline else {
                    return Ok(self.prepare(self.current_index().unwrap_or(0), 0, None, now_us));
                };
                if t.playing {
                    return Ok(Changes::default());
                }
                // Resuming: ready where it paused, everyone together.
                self.prepare(t.index, t.position_ms, None, now_us)
            }
            Action::Pause => {
                // A song getting ready stays ready, paused where it was to start (else it would
                // start once everyone is ready); a playing one pauses where it is.
                let paused = match (&self.preparing, self.timeline) {
                    // The next song readied ahead (to start at this one's end): this one pauses.
                    (Some(p), Some(t)) if p.starts_at_us.is_some() => t.paused_at(now_us),
                    (Some(p), _) => Timeline {
                        index: p.index,
                        rev: p.rev,
                        position_ms: p.position_ms,
                        at_us: now_us,
                        playing: false,
                    },
                    (None, Some(t)) => t.paused_at(now_us),
                    (None, None) => return Ok(Changes::default()),
                };
                self.preparing = None;
                self.timeline = Some(paused);
                self.note(id, "paused", false, now_us);
                Changes::room()
            }
            Action::Seek { position_ms } => {
                // While a song gets ready, the seek moves where it starts.
                if let Some(p) = self.preparing.as_ref().filter(|p| p.starts_at_us.is_none()) {
                    let index = p.index;
                    return Ok(self.prepare(index, position_ms, None, now_us));
                }
                let Some(t) = self.timeline.or(self.preparing.as_ref().map(|p| Timeline {
                    index: p.index,
                    rev: p.rev,
                    position_ms: p.position_ms,
                    at_us: now_us,
                    playing: false,
                })) else {
                    return Ok(Changes::default());
                };
                let pos = self.length_ms.map_or(position_ms, |l| position_ms.min(l));
                if t.playing {
                    self.prepare(t.index, pos, None, now_us)
                } else {
                    self.timeline = Some(t.sought(pos, now_us));
                    Changes::room()
                }
            }
            Action::Next => {
                let Some(i) = self.current_index() else {
                    return Ok(Changes::default());
                };
                if i + 1 >= self.queue.len() {
                    return Ok(Changes::default());
                }
                self.keep_undo();
                self.note(id, "skipped", true, now_us);
                self.prepare(i + 1, 0, None, now_us)
            }
            Action::Previous => {
                let Some(i) = self.current_index() else {
                    return Ok(Changes::default());
                };
                // Early in a song: the one before; later: this one from the start (players do this).
                let at = self.timeline.map(|t| t.position_at(now_us)).unwrap_or(0);
                let to = if at > 3_000 || i == 0 { i } else { i - 1 };
                self.keep_undo();
                self.note(
                    id,
                    if to == i {
                        "went back to the start"
                    } else {
                        "went back a song"
                    },
                    true,
                    now_us,
                );
                self.prepare(to, 0, None, now_us)
            }
            Action::Jump { index } => {
                if index >= self.queue.len() {
                    return Err("That song isn't in the queue any more.".into());
                }
                self.keep_undo();
                let title = self.queue[index].title.clone();
                self.note(id, &format!("played “{}”", short(&title)), true, now_us);
                self.prepare(index, 0, None, now_us)
            }
            Action::Add { mut songs, next } => {
                let room = MAX_QUEUE.saturating_sub(self.queue.len());
                songs.truncate(room);
                if songs.is_empty() {
                    return Err("The room's queue is full.".into());
                }
                for s in &mut songs {
                    s.added_by = Some(id);
                }
                self.keep_undo();
                let n = songs.len();
                let what = if n == 1 {
                    format!("added “{}”", short(&songs[0].title))
                } else {
                    format!("added {n} songs")
                };
                let at = if next {
                    self.current_index().map_or(self.queue.len(), |i| i + 1)
                } else {
                    self.queue.len()
                };
                for (k, s) in songs.into_iter().enumerate() {
                    self.queue.insert(at + k, s);
                }
                self.queue_changed(at, n as isize);
                self.note(id, &what, true, now_us);
                // Added to an empty room: it starts.
                if self.timeline.is_none() && self.preparing.is_none() {
                    return Ok(Changes::room().merge(self.prepare(0, 0, None, now_us)));
                }
                Changes::room()
            }
            Action::Remove { index } => {
                if index >= self.queue.len() || Some(index) == self.current_index() {
                    return Err("The song playing can't be taken out; skip it instead.".into());
                }
                self.keep_undo();
                let title = self.queue[index].title.clone();
                self.queue.remove(index);
                self.queue_changed(index + 1, -1);
                self.note(id, &format!("took out “{}”", short(&title)), true, now_us);
                Changes::room()
            }
            Action::Move { from, to } => {
                if from >= self.queue.len() || to >= self.queue.len() || from == to {
                    return Ok(Changes::default());
                }
                self.keep_undo();
                self.remember_preparing();
                let s = self.queue.remove(from);
                self.queue.insert(to, s);
                // The playing entry keeps playing wherever it moves.
                let cur = self.current_index();
                let moved = |i: usize| {
                    if i == from {
                        to
                    } else if from < i && i <= to {
                        i - 1
                    } else if to <= i && i < from {
                        i + 1
                    } else {
                        i
                    }
                };
                if let Some(c) = cur {
                    let n = moved(c);
                    if let Some(t) = &mut self.timeline {
                        t.index = n;
                    }
                    if let Some(p) = &mut self.preparing {
                        p.index = n;
                    }
                }
                self.rev += 1;
                self.sync_rev();
                self.note(id, "moved a song", true, now_us);
                Changes::room()
            }
        })
    }

    /// The host undoes the last skip or queue change.
    pub fn undo(&mut self, id: u32, now_us: i64) -> Result<Changes, String> {
        if id != HOST {
            return Err("Only the host can undo.".into());
        }
        let Some(u) = self.undo.take() else {
            return Err("Nothing to undo.".into());
        };
        let playing_before = u.timeline;
        self.queue = u.queue;
        self.rev += 1;
        self.note(HOST, "undid the last change", false, now_us);
        if let Some(a) = self.activity.iter_mut().rev().find(|a| a.undoable) {
            a.undoable = false;
        }
        match playing_before {
            // The song before the change, where it would be now had nothing changed.
            Some(t) if t.playing => {
                let pos = t.position_at(now_us);
                Ok(Changes::room().merge(self.prepare(t.index, pos, None, now_us)))
            }
            Some(t) => {
                self.preparing = None;
                self.timeline = Some(Timeline { rev: self.rev, ..t });
                Ok(Changes::room())
            }
            None => Ok(Changes::room()),
        }
    }

    /// Time passes: starts that waited long enough go, and songs follow each other.
    pub fn tick(&mut self, now_us: i64) -> Changes {
        let mut c = self.start_if_ready(now_us);
        // Near the end: the next song gets ready, to start exactly when this one ends.
        if self.preparing.is_none()
            && let (Some(t), Some(len)) = (self.timeline, self.length_ms)
            && t.playing
        {
            let ends = t.at_us + (len as i64 - t.position_ms as i64) * 1_000;
            if now_us >= ends - PREPARE_AHEAD_US {
                if t.index + 1 < self.queue.len() {
                    c = c.merge(self.prepare(t.index + 1, 0, Some(ends.max(now_us)), now_us));
                } else if now_us >= ends {
                    // The end of the queue: the room rests.
                    self.timeline = Some(t.paused_at(ends));
                    c = c.merge(Changes::room());
                }
            }
        }
        c
    }

    // --- Inside ------------------------------------------------------------------------------

    /// Get an entry ready for everyone; it starts when all are ready (or after a wait), or at
    /// `starts_at_us` for the next song prepared ahead.
    fn prepare(
        &mut self,
        index: usize,
        position_ms: u64,
        starts_at_us: Option<i64>,
        now_us: i64,
    ) -> Changes {
        if index >= self.queue.len() {
            return Changes::default();
        }
        if starts_at_us.is_none()
            && let Some(t) = self.timeline
        {
            // Everyone holds where they are while the new song gets ready. The same song moved
            // (a seek, a resume) is held where it will start: a player told the held place again
            // as it gets ready stays ready there. A song already held stays exactly as it was: a
            // new moment for it would be a new timeline, which every player would follow again
            // (opening the held song once more, or moving back to it).
            self.timeline = Some(if t.index == index {
                if !t.playing && t.position_ms == position_ms {
                    t
                } else {
                    Timeline {
                        position_ms,
                        at_us: now_us,
                        playing: false,
                        ..t
                    }
                }
            } else if t.playing {
                t.paused_at(now_us)
            } else {
                t
            });
        }
        let waiting = self
            .people
            .iter()
            .filter(|p| !p.away && p.hearing != Hearing::SteppedOut)
            .map(|p| p.id)
            .collect();
        for p in &mut self.people {
            if p.hearing != Hearing::SteppedOut {
                p.hearing = Hearing::Opening;
            }
        }
        self.preparing = Some(Preparing {
            rev: self.rev,
            index,
            position_ms,
            waiting,
            since_us: now_us,
            starts_at_us,
            also: Vec::new(),
        });
        self.length_ms = None;
        Changes {
            room: true,
            prepare: Some((self.rev, index, position_ms)),
        }
    }

    fn start_if_ready(&mut self, now_us: i64) -> Changes {
        let Some(prep) = &self.preparing else {
            return Changes::default();
        };
        let all_ready = prep.waiting.is_empty();
        let waited = now_us - prep.since_us >= READY_WAIT_US;
        let start = match prep.starts_at_us {
            // The next song starts when the last ends, whoever is ready (the rest catch up).
            Some(at) => (now_us >= at - lead_us(self.slowest_rtt_us)).then_some(at.max(now_us)),
            None => (all_ready || waited).then(|| now_us + lead_us(self.slowest_rtt_us)),
        };
        let Some(at) = start else {
            return Changes::default();
        };
        let p = self.preparing.take().expect("preparing");
        self.timeline = Some(Timeline {
            index: p.index,
            rev: p.rev,
            position_ms: p.position_ms,
            at_us: at,
            playing: true,
        });
        Changes::room()
    }

    fn current_index(&self) -> Option<usize> {
        self.preparing
            .as_ref()
            .map(|p| p.index)
            .or(self.timeline.map(|t| t.index))
    }

    /// The song being readied, as people know it now: still known so after the queue changes
    /// (their Ready, sent for it as it was, still counts).
    fn remember_preparing(&mut self) {
        if let Some(p) = &mut self.preparing {
            let was = (p.rev, p.index);
            if !p.also.contains(&was) {
                p.also.push(was);
            }
        }
    }

    /// Entries at or after `at` moved by `by`: the playing one keeps its song.
    fn queue_changed(&mut self, at: usize, by: isize) {
        self.remember_preparing();
        let shift = |i: usize| {
            if i >= at {
                (i as isize + by).max(0) as usize
            } else {
                i
            }
        };
        if let Some(t) = &mut self.timeline {
            t.index = shift(t.index);
        }
        if let Some(p) = &mut self.preparing {
            p.index = shift(p.index);
        }
        self.rev += 1;
        self.sync_rev();
    }

    /// The playing entry is the same song under the new revision.
    fn sync_rev(&mut self) {
        let rev = self.rev;
        if let Some(t) = &mut self.timeline {
            t.rev = rev;
        }
        if let Some(p) = &mut self.preparing {
            p.rev = rev;
        }
    }

    fn keep_undo(&mut self) {
        self.undo = Some(Undo {
            queue: self.queue.clone(),
            timeline: self.timeline,
        });
    }

    fn note(&mut self, who: u32, what: &str, undoable: bool, now_us: i64) {
        if undoable {
            for a in &mut self.activity {
                a.undoable = false;
            }
        }
        let name = self
            .people
            .iter()
            .find(|p| p.id == who)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.activity.push_back(Activity {
            who,
            name,
            what: what.to_owned(),
            at_us: now_us,
            undoable,
        });
        while self.activity.len() > ACTIVITY {
            self.activity.pop_front();
        }
    }

    /// The room as everyone sees it (the host also sees who's waiting by name).
    pub fn view(&self, for_host: bool) -> RoomView {
        RoomView {
            host_name: self.host_name.clone(),
            members: self
                .people
                .iter()
                .map(|p| Member {
                    id: p.id,
                    name: p.name.clone(),
                    host: p.id == HOST,
                    role: p.role,
                    hearing: p.hearing,
                    error_ms: p.error_ms,
                    away: p.away,
                })
                .collect(),
            waiting: if for_host {
                self.waiting
                    .iter()
                    .map(|w| (w.id, w.name.clone()))
                    .collect()
            } else {
                self.waiting.iter().map(|w| (w.id, String::new())).collect()
            },
            queue: self.queue.clone(),
            rev: self.rev,
            timeline: self.timeline,
            activity: self.activity.iter().cloned().collect(),
            new_role: self.new_role,
            same_room: self.same_room,
            starting: self
                .preparing
                .as_ref()
                .filter(|p| p.starts_at_us.is_none())
                .map(|p| {
                    self.people
                        .iter()
                        .filter(|m| p.waiting.contains(&m.id))
                        .map(|m| m.name.clone())
                        .collect()
                }),
        }
    }

    /// What's being prepared now (for someone joining mid-way, or back after a drop).
    pub fn preparing(&self) -> Option<(u64, usize, u64)> {
        self.preparing
            .as_ref()
            .map(|p| (p.rev, p.index, p.position_ms))
    }

    pub fn timeline(&self) -> Option<Timeline> {
        self.timeline
    }

    pub fn queue(&self) -> &[Song] {
        &self.queue
    }
}

fn short(title: &str) -> String {
    let t: String = title.chars().take(40).collect();
    if t.chars().count() < title.chars().count() {
        format!("{t}…")
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invite::proof;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    fn song(n: u8) -> Song {
        Song {
            key: format!("online:song{n:07}"),
            title: format!("Song {n}"),
            artists: vec!["Someone".into()],
            album: None,
            duration_ms: Some(200_000),
            isrc: None,
            mbid: None,
            online_id: Some(format!("song{n:07}")),
            host_file: false,
            format: None,
            artwork_url: None,
            added_by: None,
        }
    }

    fn room() -> Room {
        Room::new("Aakash", SECRET, vec![song(1), song(2), song(3)])
    }

    fn join(r: &mut Room, key: &str, name: &str) -> u32 {
        match r.knock(key, name, &proof(SECRET, key)) {
            Knock::Waiting(id) => {
                r.admit(id).unwrap();
                id
            }
            Knock::In(id) => id,
            Knock::Refused(w) => panic!("refused: {w}"),
        }
    }

    #[test]
    fn people_knock_wait_and_come_in() {
        let mut r = room();
        assert!(
            matches!(r.knock("k1", "Ravi", "00"), Knock::Refused(_)),
            "no proof, no knock"
        );
        let Knock::Waiting(id) = r.knock("k1", "Ravi", &proof(SECRET, "k1")) else {
            panic!()
        };
        assert_eq!(r.view(true).waiting, vec![(id, "Ravi".to_owned())]);
        assert_eq!(
            r.view(false).waiting[0].1,
            "",
            "guests don't see who's waiting"
        );
        r.admit(id).unwrap();
        assert!(r.is_member(id));
        assert_eq!(
            r.role_of(id),
            Some(Role::Control),
            "full control by default"
        );
        // Back after a drop, at once and the same person.
        r.away(id);
        assert_eq!(r.knock("k1", "Ravi", &proof(SECRET, "k1")), Knock::In(id));
    }

    #[test]
    fn blocked_people_stay_out_and_rooms_fill_up() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        r.remove(ravi, true);
        assert!(matches!(
            r.knock("k1", "Ravi", &proof(SECRET, "k1")),
            Knock::Refused(_)
        ));
        for i in 2..MAX_PEOPLE + 1 {
            join(&mut r, &format!("k{i}"), "Friend");
        }
        assert_eq!(r.member_ids().len(), MAX_PEOPLE);
        assert!(matches!(
            r.knock("late", "Late", &proof(SECRET, "late")),
            Knock::Refused(_)
        ));
    }

    #[test]
    fn a_song_starts_for_everyone_once_all_are_ready() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        let c = r.ask(HOST, Action::Play, 0).unwrap();
        assert_eq!(c.prepare, Some((1, 0, 0)));
        assert!(r.timeline().is_none(), "not until ready");
        r.ready(HOST, 1, 0, 100_000);
        assert!(r.timeline().is_none(), "still waiting for Ravi");
        r.ready(ravi, 1, 0, 300_000);
        let t = r.timeline().unwrap();
        assert!(t.playing);
        assert_eq!(t.at_us, 800_000, "half a second ahead of the last ready");
    }

    #[test]
    fn play_while_a_song_starts_changes_nothing() {
        let mut r = room();
        let _ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.tick(READY_WAIT_US);
        r.ask(HOST, Action::Next, 10_000_000).unwrap();
        // Pressed again while the next song gets ready (the button looked like nothing
        // happened): it stays on its way, never back to the song before.
        let c = r.ask(HOST, Action::Play, 10_200_000).unwrap();
        assert_eq!(c.prepare, None);
        assert_eq!(r.preparing().map(|p| p.1), Some(1));
    }

    #[test]
    fn a_seek_while_a_song_starts_moves_its_start() {
        let mut r = room();
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.ask(HOST, Action::Next, 5_000_000).unwrap();
        let c = r
            .ask(
                HOST,
                Action::Seek {
                    position_ms: 30_000,
                },
                5_100_000,
            )
            .unwrap();
        assert_eq!(c.prepare.map(|p| (p.1, p.2)), Some((1, 30_000)));
    }

    #[test]
    fn a_seek_while_a_resume_gets_ready_holds_the_song_where_it_will_start() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.ready(ravi, 1, 0, 0);
        r.ask(HOST, Action::Pause, 10_500_000).unwrap();
        let paused = r.timeline().unwrap();
        assert_eq!((paused.position_ms, paused.playing), (10_000, false));
        // Play: held as it was (no new moment: the players are there already).
        r.ask(HOST, Action::Play, 11_000_000).unwrap();
        assert_eq!(r.timeline(), Some(paused), "held exactly as it paused");
        // A seek before it starts: held where it will now start, so a player told the held
        // place again (as it says ready) stays ready there, not back at the pause.
        let c = r
            .ask(
                HOST,
                Action::Seek {
                    position_ms: 30_000,
                },
                11_100_000,
            )
            .unwrap();
        assert_eq!(c.prepare, Some((1, 0, 30_000)));
        let held = r.timeline().unwrap();
        assert_eq!(
            (held.index, held.position_ms, held.playing),
            (0, 30_000, false)
        );
        r.ready(HOST, 1, 0, 11_200_000);
        r.ready(ravi, 1, 0, 11_300_000);
        let t = r.timeline().unwrap();
        assert_eq!((t.position_ms, t.playing), (30_000, true));
    }

    #[test]
    fn a_seek_while_the_next_song_gets_ready_leaves_the_last_one_held_as_it_was() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.ready(ravi, 1, 0, 0);
        r.ask(HOST, Action::Next, 10_000_000).unwrap();
        let held = r.timeline().unwrap();
        assert_eq!((held.index, held.playing), (0, false));
        // A seek before the next song starts moves its start; the song before stays held exactly
        // as it was (a new moment for it would have every player open it again).
        let c = r
            .ask(
                HOST,
                Action::Seek {
                    position_ms: 30_000,
                },
                10_200_000,
            )
            .unwrap();
        assert_eq!(c.prepare.map(|p| (p.1, p.2)), Some((1, 30_000)));
        assert_eq!(r.timeline(), Some(held));
    }

    #[test]
    fn play_while_playing_leaves_the_rooms_timeline_alone() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.ready(ravi, 1, 0, 0);
        let t = r.timeline().unwrap();
        let c = r.ask(ravi, Action::Play, 6_000_000).unwrap();
        assert_eq!(c, Changes::default());
        assert_eq!(r.timeline(), Some(t), "the same start, the same place");
        assert_eq!(r.timeline().unwrap().position_at(6_500_000), 6_000);
    }

    #[test]
    fn how_far_off_someone_is_reaches_everyone_when_what_they_see_changes() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        // The first word of it: everyone's panel shows it.
        assert!(r.sync(ravi, 3).room);
        // A millisecond or two either way: nothing anyone would see change.
        assert!(!r.sync(ravi, 5).room);
        // Ten or more: the number shown moves.
        assert!(r.sync(ravi, 15).room);
        // Past 60 ms ("catching up") and back in sync: shown at once.
        assert!(r.sync(ravi, 70).room);
        assert!(r.sync(ravi, 58).room);
        // Whatever is said, the room keeps the latest.
        assert!(!r.sync(ravi, 55).room);
        let me = r
            .view(false)
            .members
            .into_iter()
            .find(|m| m.id == ravi)
            .unwrap();
        assert_eq!(me.error_ms, Some(55));
    }

    #[test]
    fn ready_still_counts_after_the_queue_changes_mid_start() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        let (rev, index, _) = r.ask(HOST, Action::Play, 0).unwrap().prepare.unwrap();
        // Someone adds a song while it gets ready: the start isn't held up by it.
        r.ask(
            ravi,
            Action::Add {
                songs: vec![song(9)],
                next: false,
            },
            100_000,
        )
        .unwrap();
        r.ready(HOST, rev, index, 200_000);
        r.ready(ravi, rev, index, 300_000);
        assert!(r.timeline().is_some_and(|t| t.playing), "started at once");
    }

    #[test]
    fn a_slow_guest_holds_a_start_two_and_a_half_seconds_at_most() {
        assert_eq!(READY_WAIT_US, 2_500_000);
    }

    #[test]
    fn one_slow_round_trip_doesnt_slow_every_start_after() {
        let mut r = room();
        r.round_trip(900_000);
        let spiked = r.slowest_rtt_us;
        for _ in 0..30 {
            r.round_trip(40_000);
        }
        assert!(r.slowest_rtt_us < spiked / 4, "{}", r.slowest_rtt_us);
        assert!(r.slowest_rtt_us >= 40_000);
    }

    #[test]
    fn the_room_says_a_song_is_starting_and_who_it_waits_for() {
        let mut r = room();
        let _ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        let v = r.view(false);
        assert_eq!(v.starting, Some(vec!["Ravi".to_owned()]));
        r.tick(READY_WAIT_US);
        assert_eq!(r.view(false).starting, None, "playing");
    }

    #[test]
    fn a_slow_guest_is_not_waited_for_forever() {
        let mut r = room();
        let _ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        assert!(r.tick(READY_WAIT_US - 1).prepare.is_none());
        assert!(r.timeline().is_none());
        r.tick(READY_WAIT_US);
        assert!(r.timeline().unwrap().playing, "it goes on; Ravi catches up");
    }

    #[test]
    fn someone_who_cant_play_it_isnt_waited_for() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.hearing(ravi, 1, 0, Hearing::Cannot);
        r.tick(10);
        assert!(r.timeline().is_some());
    }

    #[test]
    fn roles_decide_what_guests_may_do() {
        let mut r = room();
        let meera = join(&mut r, "k1", "Meera");
        r.set_role(meera, Role::Add);
        assert!(r.ask(meera, Action::Next, 0).is_err());
        assert!(
            r.ask(
                meera,
                Action::Add {
                    songs: vec![song(9)],
                    next: false
                },
                0
            )
            .is_ok()
        );
        r.set_role(meera, Role::Listen);
        assert!(
            r.ask(
                meera,
                Action::Add {
                    songs: vec![song(8)],
                    next: false
                },
                0
            )
            .is_err()
        );
        assert!(r.ask(99, Action::Play, 0).is_err(), "not in the room");
        r.set_new_role(Role::Listen);
        let sam = join(&mut r, "k2", "Sam");
        assert_eq!(r.role_of(sam), Some(Role::Listen));
    }

    #[test]
    fn everyone_sees_who_did_what_and_the_host_can_undo_it() {
        let mut r = room();
        let meera = join(&mut r, "k1", "Meera");
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.ready(meera, 1, 0, 0);
        assert_eq!(r.timeline().unwrap().index, 0);
        r.ask(meera, Action::Next, 10_000_000).unwrap();
        let a = r.view(false).activity.last().cloned().unwrap();
        assert_eq!(
            (a.who, a.what.as_str(), a.undoable),
            (meera, "skipped", true)
        );
        assert!(r.undo(meera, 11_000_000).is_err(), "only the host undoes");
        let c = r.undo(HOST, 11_000_000).unwrap();
        let (_, index, position) = c.prepare.unwrap();
        assert_eq!(index, 0, "back to the song before");
        assert!(
            (10_400..=11_000).contains(&position),
            "where it would be now: {position}"
        );
    }

    #[test]
    fn added_songs_carry_who_added_them_and_keep_the_playing_one() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        r.ask(HOST, Action::Jump { index: 1 }, 0).unwrap();
        r.ready(HOST, r.view(true).rev, 1, 0);
        r.ready(ravi, r.view(true).rev, 1, 0);
        r.ask(
            ravi,
            Action::Add {
                songs: vec![song(7), song(8)],
                next: true,
            },
            1_000,
        )
        .unwrap();
        let v = r.view(false);
        assert_eq!(
            v.queue[2].title, "Song 7",
            "next: right after the playing one"
        );
        assert_eq!(v.queue[2].added_by, Some(ravi));
        assert_eq!(v.timeline.unwrap().index, 1, "still playing Song 2");
        assert_eq!(v.activity.last().unwrap().what, "added 2 songs");
        // Taking out a song before the playing one moves its index, not the song.
        r.ask(HOST, Action::Remove { index: 0 }, 2_000).unwrap();
        assert_eq!(
            r.view(false).queue[r.timeline().unwrap().index].title,
            "Song 2"
        );
        assert!(
            r.ask(
                HOST,
                Action::Remove {
                    index: r.timeline().unwrap().index
                },
                3_000
            )
            .is_err()
        );
        // Moving the playing song keeps it playing.
        let i = r.timeline().unwrap().index;
        r.ask(HOST, Action::Move { from: i, to: 3 }, 4_000).unwrap();
        assert_eq!(
            r.view(false).queue[r.timeline().unwrap().index].title,
            "Song 2"
        );
    }

    #[test]
    fn songs_follow_each_other_on_the_rooms_clock() {
        let mut r = room();
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        let t = r.timeline().unwrap();
        r.set_length(200_000);
        let ends = t.at_us + 200_000_000;
        assert!(r.tick(ends - PREPARE_AHEAD_US - 1).prepare.is_none());
        let c = r.tick(ends - PREPARE_AHEAD_US);
        assert_eq!(
            c.prepare.map(|p| p.1),
            Some(1),
            "the next one gets ready ahead"
        );
        assert_eq!(
            r.timeline().unwrap().index,
            0,
            "this one plays on meanwhile"
        );
        r.tick(ends - 1_000_000);
        assert_eq!(
            r.timeline().unwrap().index,
            0,
            "a second before the end it still plays"
        );
        r.tick(ends - 400_000);
        let next = r.timeline().unwrap();
        assert_eq!(
            (next.index, next.at_us, next.position_ms),
            (1, ends, 0),
            "starts exactly at the end"
        );
    }

    #[test]
    fn pausing_seeking_and_the_end_of_the_queue() {
        let mut r = Room::new("Aakash", SECRET, vec![song(1)]);
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        r.ask(HOST, Action::Pause, 10_500_000).unwrap();
        let p = r.timeline().unwrap();
        assert!(!p.playing);
        assert_eq!(p.position_ms, 10_000);
        r.ask(
            HOST,
            Action::Seek {
                position_ms: 50_000,
            },
            11_000_000,
        )
        .unwrap();
        assert_eq!(
            r.timeline().unwrap().position_ms,
            50_000,
            "paused, it just moves"
        );
        let c = r.ask(HOST, Action::Play, 12_000_000).unwrap();
        assert_eq!(
            c.prepare,
            Some((1, 0, 50_000)),
            "resuming gets everyone ready there"
        );
        r.ready(HOST, 1, 0, 12_000_000);
        r.set_length(60_000);
        let ends = r.timeline().unwrap().at_us + 10_000_000;
        r.tick(ends);
        assert!(
            !r.timeline().unwrap().playing,
            "nothing after: the room rests"
        );
        assert!(r.ask(HOST, Action::Next, ends).unwrap().prepare.is_none());
    }

    #[test]
    fn a_goodbye_takes_them_out_at_once_and_back_is_the_same_person() {
        let mut r = room();
        let ravi = join(&mut r, "k1", "Ravi");
        // A dropped connection: away, still in the room, the same person when back.
        r.away(ravi);
        assert!(r.is_member(ravi));
        assert!(matches!(r.knock("k1", "Ravi", &proof(SECRET, "k1")), Knock::In(id) if id == ravi));
        // A goodbye: out at once, and the room says so.
        r.left(ravi, 5_000_000);
        assert!(!r.is_member(ravi));
        assert_eq!(r.view(true).members.len(), 1);
        let said = r.view(true).activity.last().cloned().unwrap();
        assert_eq!((said.name.as_str(), said.what.as_str()), ("Ravi", "left"));
        // Back with the same invite: asks again (a new arrival, waiting for the host).
        assert!(matches!(
            r.knock("k1", "Ravi", &proof(SECRET, "k1")),
            Knock::Waiting(_)
        ));
    }

    #[test]
    fn pausing_a_song_getting_ready_keeps_it_paused_there() {
        // A room opened paused: its song gets ready, and stays paused once everyone is.
        let mut r = room();
        r.ask(HOST, Action::Jump { index: 1 }, 1_000).unwrap();
        r.ask(HOST, Action::Pause, 2_000).unwrap();
        let t = r.timeline().expect("a timeline to hold it");
        assert_eq!((t.index, t.position_ms, t.playing), (1, 0, false));
        r.ready(HOST, t.rev, 1, 3_000);
        r.tick(3_000 + READY_WAIT_US * 2);
        assert!(
            !r.timeline().unwrap().playing,
            "still paused after the wait"
        );
        // Playing it gets everyone ready again, then starts.
        let c = r.ask(HOST, Action::Play, 50_000_000).unwrap();
        assert_eq!(c.prepare.map(|p| (p.1, p.2)), Some((1, 0)));
    }

    #[test]
    fn pausing_near_the_end_pauses_this_song_not_the_next() {
        let mut r = room();
        r.ask(HOST, Action::Play, 0).unwrap();
        r.ready(HOST, 1, 0, 0);
        let t = r.timeline().unwrap();
        r.set_length(200_000);
        let ends = t.at_us + 200_000_000;
        // The next one is getting ready ahead.
        assert!(r.tick(ends - PREPARE_AHEAD_US).prepare.is_some());
        let at = ends - 5_000_000;
        r.ask(HOST, Action::Pause, at).unwrap();
        let p = r.timeline().unwrap();
        assert_eq!((p.index, p.playing), (0, false), "this song, paused");
        assert_eq!(p.position_ms, 195_000, "where it was");
        r.tick(ends + READY_WAIT_US);
        assert_eq!(
            r.timeline().unwrap().index,
            0,
            "the next one never starts by itself"
        );
    }
}
