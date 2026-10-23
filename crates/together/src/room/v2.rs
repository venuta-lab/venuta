//! Room v2 (sections 4.3, 4.4, 4.10 and 4.15 of the Listen together design): the room the host
//! keeps, on protocol 3. It is the one place that decides what plays.
//!
//! - The queue is entries with ids that never change, so moving, adding or removing songs never
//!   touches what plays or what is armed.
//! - A press (play, resume, seek, skip, jump) is *prepared*: everyone gets ready while whatever
//!   plays keeps playing, and the room commits one anchor for all ("this content is heard at
//!   this host time"), waiting 2.5 s at most. Mid-song nobody ever waits for anybody.
//! - A pause is scheduled a little ahead, so everyone falls silent from one moment.
//! - The next song is armed 20 s ahead with its boundary; each player switches by itself.
//! - People knock, wait for the host or come straight in, and have a role; a press without the
//!   role can be asked of whoever controls. Every action is written in the activity line.
//!
//! Pure: the host's time is passed in (µs of its monotonic clock), and every call returns what
//! to send, addressed to each member and already in order (`Changes`). The host's own conductor
//! is member `HOST` and is told everything a guest is.

use crate::MAX_PEOPLE;
use crate::anchor::{Anchor, EntryId, Next, PrepId, Purpose, Transition};
use crate::invite::{nearby_secret, proof_ok};
use crate::proto::v3::{
    Action, CopyReport, Entry, MAX_NAME, MAX_REQUESTS, MAX_WAITING, Member, Phase, RequestView,
    RoomView, Settings, ToGuest, ToHost,
};
use crate::proto::{Activity, MAX_QUEUE, Role, Song};
use crate::room_player::{ListenMode, SubState};
use crate::timeline::v3 as tl;
use crate::transport::PathInfo;
use crate::view;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// The host's member id.
pub const HOST: u32 = 0;
/// A prepare waits this long at most for everyone to be ready (rule 3).
pub const READY_WAIT_US: i64 = 2_500_000;
/// A member not heard from this long is shown away (pings come once a second).
pub const AWAY_AFTER_US: i64 = 5_000_000;
/// A member not heard from this long is taken as lost (QUIC's idle timeout).
pub const SILENT_LOST_US: i64 = 15_000_000;
/// A lost member is kept this long for their reconnect (decision D17).
pub const KEEP_AWAY_US: i64 = 120_000_000;
/// Every playing member suspended this long holds the room (decision D11).
pub const AUTO_HOLD_US: i64 = 3_000_000;
/// Nobody able to play the entry this long skips it.
pub const AUTO_SKIP_US: i64 = 5_000_000;
/// A held or resting room with no action this long ends itself (decision D18).
pub const IDLE_END_US: i64 = 3_600_000_000;
/// Within this of a boundary, a queue change leaves the armed next as it is (case C6).
pub const FREEZE_US: i64 = 3_000_000;
/// A one-speaker host's heard place this far from the anchor becomes the anchor (4.13), µs ...
pub const REANCHOR_US: i64 = 20_000;
/// ... and, once, this far when the host stops leading (someone plays: they follow the host's
/// real place, not one up to 20 ms off), µs.
pub const REANCHOR_ONCE_US: i64 = 1_000;
/// Previous over this far into a song starts it again; under it, the song before.
pub const PREVIOUS_RESTARTS_US: i64 = 3_000_000;
/// A request is kept this long.
pub const REQUEST_KEEP_US: i64 = 60_000_000;
/// This many prepares missed in a row tells the host.
pub const SLOW_AFTER: u32 = 3;
/// On the host's network: direct, and a median round trip under this.
pub const SAME_NETWORK_RTT_US: u32 = 8_000;
/// Every device is turned off for someone whose median round trip is over this...
pub const SAME_NETWORK_OFF_RTT_US: u32 = 15_000;
/// ... for this long.
pub const SAME_NETWORK_OFF_US: i64 = 30_000_000;
/// Round trips are kept over this long (and 64 at most).
pub const RTT_WINDOW_US: i64 = 60_000_000;
const RTT_SAMPLES: usize = 64;
/// Guests control playback by default up to this many people (decision D20).
pub const AUTO_CONTROL_UP_TO: usize = 4;
/// How many activity lines are kept.
pub const ACTIVITY: usize = 6;
/// How many entries someone is remembered unable to play.
const UNABLE_KEPT: usize = 8;

/// What a knock at the door comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Knock {
    /// In (let in without asking, or back after a drop): their welcome is in the changes.
    In(u32),
    /// Waiting for the host.
    Waiting(u32),
    /// Not let in, with why: send `Refused` and close.
    Refused(String),
}

/// One message to send to one member (the host's own, `HOST`, go to the host's conductor and
/// window).
#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub to: u32,
    pub msg: ToGuest,
}

/// Something for the host's window alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostNote {
    /// Missed three prepares in a row: "Ravi's connection is slow".
    Slow { id: u32 },
    /// Moved from every device to apart: "Same room turned off: Ali's connection is too slow".
    SameRoomOff { id: u32 },
}

/// What a call changed, for the session to carry out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    /// Messages, in the order to send them.
    pub out: Vec<Out>,
    pub notes: Vec<HostNote>,
    /// The host's speaker's volume to set (0 to 100).
    pub volume: Option<u8>,
    /// Out of the room now (left, removed, gone after the grace): close and forget them.
    pub gone: Vec<u32>,
    /// The room ended: send what's in `out`, then close everything.
    pub ended: bool,
}

impl Changes {
    /// The messages for one member, in order.
    pub fn to(&self, id: u32) -> Vec<&ToGuest> {
        self.out
            .iter()
            .filter(|o| o.to == id)
            .map(|o| &o.msg)
            .collect()
    }

    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        *self == Changes::default()
    }

    /// `o` after these.
    pub fn and(mut self, o: Changes) -> Changes {
        self.out.extend(o.out);
        self.notes.extend(o.notes);
        self.volume = o.volume.or(self.volume);
        self.gone.extend(o.gone);
        self.ended |= o.ended;
        self
    }
}

/// How the room starts, from the host's player at the moment it opens (case A1: the host's
/// audio is never touched).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Nothing playing.
    Resting,
    /// Paused at this place of the song at `index`.
    Held { index: usize, content_us: i64 },
    /// Playing: the song at `index`, its `content_us` heard at host time `host_us`.
    Playing {
        index: usize,
        content_us: i64,
        host_us: i64,
    },
}

/// Where a kept room was (`RoomState`): it comes back held there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    /// Nothing had played.
    Resting,
    /// The queue had ended after this entry.
    QueueEnded { after: EntryId },
    /// At this place of this entry (playing or held).
    At { entry: EntryId, content_us: i64 },
}

/// A member as a kept room has them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptPerson {
    pub id: u32,
    pub key: String,
    pub name: String,
    pub role: Option<Role>,
    pub mode: ListenMode,
}

/// What a room needs to come back after the host's app restarts (decision D8, the WP15 notes
/// in 9.2): written by the app, `Room::reopen` takes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomState {
    pub host_name: String,
    pub secret: String,
    pub queue: Vec<Entry>,
    /// The reference copies' lengths the host's player gave (µs).
    pub lengths: Vec<(EntryId, i64)>,
    pub place: Place,
    /// The guests in the room.
    pub people: Vec<KeptPerson>,
    /// Members dropped for silence, known again when they come back.
    pub returning: Vec<KeptPerson>,
    pub blocked: Vec<String>,
    pub transition: Transition,
    pub new_role: Option<Role>,
    pub guests_volume: bool,
    pub let_in_without_asking: bool,
    pub visible: bool,
    pub next_member: u32,
    pub next_entry: u64,
    pub next_prep: u64,
    pub seq: u64,
    pub next_request: u32,
}

/// A reopened room's ids and sequence go on from the kept ones by this much: the record can
/// lag the room by one write, and nothing given in that moment may be given again.
pub const REOPEN_MARGIN: u64 = 1_000;
/// How many members dropped for silence a room remembers.
pub const RETURNING_KEPT: usize = 32;

/// A person's player, as they report it (`ToHost::State`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    pub entry: Option<EntryId>,
    pub sub_state: SubState,
    pub error_us: Option<i64>,
    pub bound_us: Option<u32>,
    pub copy: Option<CopyReport>,
}

#[derive(Debug, Clone)]
struct Person {
    id: u32,
    /// Their connection's key (stable for one Venuta), for knowing them again and blocking.
    key: String,
    name: String,
    /// Set by the host; `None`: the room's default.
    role: Option<Role>,
    mode: ListenMode,
    /// Their welcome went out: views follow.
    welcomed: bool,
    last_heard_us: i64,
    away: bool,
    /// When their connection was lost (they are kept 2 minutes from then).
    lost_at: Option<i64>,
    sub_state: Option<SubState>,
    entry: Option<EntryId>,
    error_us: Option<i64>,
    bound_us: Option<u32>,
    copy: Option<CopyReport>,
    suspended_since: Option<i64>,
    /// Entries they can't play (the latest few).
    unable: VecDeque<EntryId>,
    /// (when, round trip µs), the last 60 s.
    rtts: VecDeque<(i64, u32)>,
    direct: bool,
    /// Every device: their round trip too slow since.
    slow_since: Option<i64>,
    /// Prepares missed in a row, and whether the host was told.
    missed: u32,
    told_slow: bool,
}

impl Person {
    fn new(id: u32, key: &str, name: &str, now_us: i64) -> Person {
        Person {
            id,
            key: key.to_owned(),
            name: fit_name(name),
            role: None,
            mode: ListenMode::Apart,
            welcomed: false,
            last_heard_us: now_us,
            away: false,
            lost_at: None,
            sub_state: None,
            entry: None,
            error_us: None,
            bound_us: None,
            copy: None,
            suspended_since: None,
            unable: VecDeque::new(),
            rtts: VecDeque::new(),
            direct: false,
            slow_since: None,
            missed: 0,
            told_slow: false,
        }
    }

    fn plays(&self) -> bool {
        !self.away && self.mode != ListenMode::OneSpeaker
    }

    fn suspended(&self) -> bool {
        matches!(self.sub_state, Some(SubState::Suspended(_)))
    }

    fn stepped_out(&self) -> bool {
        self.sub_state == Some(SubState::SteppedOut)
    }

    fn cannot(&self, entry: EntryId) -> bool {
        self.unable.contains(&entry)
    }

    fn mark_unable(&mut self, entry: EntryId) {
        if !self.cannot(entry) {
            self.unable.push_back(entry);
            while self.unable.len() > UNABLE_KEPT {
                self.unable.pop_front();
            }
        }
    }

    fn rtt_median(&self) -> Option<u32> {
        percentile(&self.rtts, 0.5)
    }

    fn rtt_p95(&self) -> Option<u32> {
        percentile(&self.rtts, 0.95)
    }

    /// Direct, with a median round trip under 8 ms.
    fn same_network(&self) -> bool {
        self.direct && self.rtt_median().is_some_and(|m| m < SAME_NETWORK_RTT_US)
    }

    fn line(&self, host_name: &str) -> String {
        let ms = |us: i64| us as f32 / 1_000.0;
        view::line(
            self.sub_state,
            self.mode,
            self.away,
            self.error_us.map(ms),
            self.bound_us.map(|b| ms(i64::from(b))),
            host_name,
        )
    }
}

/// A name as views carry it: trimmed, no control characters, at most 40 letters.
fn fit_name(name: &str) -> String {
    let n: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    let n = n.trim().to_owned();
    if n.is_empty() { "Someone".into() } else { n }
}

fn percentile(samples: &VecDeque<(i64, u32)>, p: f64) -> Option<u32> {
    if samples.is_empty() {
        return None;
    }
    let mut v: Vec<u32> = samples.iter().map(|s| s.1).collect();
    v.sort_unstable();
    let i = ((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    Some(v[i])
}

#[derive(Debug, Clone)]
struct Prep {
    prep: PrepId,
    entry: EntryId,
    content_us: i64,
    purpose: Purpose,
    /// Who it still waits for.
    waiting: BTreeSet<u32>,
    since_us: i64,
}

#[derive(Debug, Clone, Copy)]
enum Play {
    /// Nothing plays: nothing has yet (`None`), or the queue ended after this entry.
    Resting {
        after: Option<EntryId>,
    },
    Playing(Anchor),
    /// Paused; `auto` when the room held itself (everyone suspended).
    Held {
        anchor: Anchor,
        auto: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct Armed {
    prep: PrepId,
    next: Next,
    /// The anchor it was armed for.
    anchor_seq: u64,
}

#[derive(Debug, Clone)]
struct Undo {
    queue: Vec<Entry>,
    anchor: Option<Anchor>,
}

#[derive(Debug, Clone)]
struct Request {
    id: u32,
    from: u32,
    action: Action,
    at_us: i64,
    /// What played when it was asked (a song change for another one is out of date).
    entry: Option<EntryId>,
}

/// The room the host keeps.
#[derive(Debug, Clone)]
pub struct Room {
    host_name: String,
    secret: String,
    people: Vec<Person>,
    waiting: Vec<Person>,
    blocked: HashSet<String>,
    queue: Vec<Entry>,
    /// The reference copies' lengths the host's player gave (µs).
    lengths: HashMap<EntryId, i64>,
    play: Play,
    prep: Option<Prep>,
    armed: Option<Armed>,
    /// The end of the queue, said ahead (its `Hold`).
    ending: Option<Anchor>,
    activity: VecDeque<Activity>,
    undo: Option<Undo>,
    requests: Vec<Request>,
    transition: Transition,
    /// The role for guests the host chose (`None`: by the room's size, D20).
    new_role: Option<Role>,
    guests_volume: bool,
    /// The host's speaker's volume, 0 to 100, as the host's player has it (B17).
    volume: Option<u8>,
    /// The host led (every other member a remote) when its heard place came last.
    led: bool,
    let_in_without_asking: bool,
    /// Seen on this network: people nearby may knock (D19, on by default).
    visible: bool,
    /// Members dropped for silence (the newest last), let back in as themselves.
    returning: VecDeque<KeptPerson>,
    next_member: u32,
    next_entry: u64,
    next_prep: u64,
    seq: u64,
    next_request: u32,
    /// Since when nothing was done (an action, or the room falling held or resting).
    quiet_since: Option<i64>,
    /// Everyone who plays has been unable to play this entry since.
    unable_since: Option<(EntryId, i64)>,
    /// Who said yes to the request being done now ("Ravi skipped (Asha said yes)").
    said_yes: Option<String>,
    ended: bool,
    out: Changes,
    view_dirty: bool,
}

fn short(title: &str) -> String {
    let t: String = title.chars().take(40).collect();
    if t.chars().count() < title.chars().count() {
        format!("{t}…")
    } else {
        t
    }
}

fn same_album(a: &Song, b: &Song) -> bool {
    match (&a.album, &b.album) {
        (Some(x), Some(y)) => {
            !x.trim().is_empty() && x.trim().to_lowercase() == y.trim().to_lowercase()
        }
        _ => false,
    }
}

impl Room {
    /// A new room with the host's queue, starting as the host's player is (`opening`).
    pub fn new(host_name: &str, secret: &str, songs: Vec<Song>, opening: Opening) -> Room {
        let host = Person {
            welcomed: true,
            ..Person::new(HOST, "", host_name, 0)
        };
        let mut r = Room {
            host_name: host.name.clone(),
            secret: secret.to_owned(),
            people: vec![host],
            waiting: Vec::new(),
            blocked: HashSet::new(),
            queue: Vec::new(),
            lengths: HashMap::new(),
            play: Play::Resting { after: None },
            prep: None,
            armed: None,
            ending: None,
            activity: VecDeque::new(),
            undo: None,
            requests: Vec::new(),
            transition: Transition::Gapless,
            new_role: None,
            guests_volume: false,
            volume: None,
            led: false,
            let_in_without_asking: false,
            visible: true,
            returning: VecDeque::new(),
            next_member: 1,
            next_entry: 1,
            next_prep: 1,
            seq: 0,
            next_request: 1,
            quiet_since: None,
            unable_since: None,
            said_yes: None,
            ended: false,
            out: Changes::default(),
            view_dirty: false,
        };
        for s in songs.into_iter().take(MAX_QUEUE) {
            let id = r.new_entry_id();
            r.queue.push(Entry { id, song: s });
        }
        let at = |r: &Room, i: usize| r.queue.get(i).map(|e| e.id);
        r.play = match opening {
            Opening::Resting => Play::Resting { after: None },
            Opening::Held { index, content_us } => match at(&r, index) {
                Some(entry) => Play::Held {
                    anchor: Anchor {
                        entry,
                        content_us: content_us.max(0),
                        host_us: 0,
                        playing: false,
                        seq: r.new_seq(),
                    },
                    auto: false,
                },
                None => Play::Resting { after: None },
            },
            Opening::Playing {
                index,
                content_us,
                host_us,
            } => match at(&r, index) {
                Some(entry) => Play::Playing(Anchor {
                    entry,
                    content_us: content_us.max(0),
                    host_us,
                    playing: true,
                    seq: r.new_seq(),
                }),
                None => Play::Resting { after: None },
            },
        };
        r
    }

    // --- Who's here --------------------------------------------------------------------

    /// Someone knocks: their connection key, the name they give, and their invite proof. Back
    /// after a drop (the same key, still kept), they're in at once as the same member.
    pub fn knock(&mut self, key: &str, name: &str, proof: &str, now_us: i64) -> (Knock, Changes) {
        if self.ended {
            return (
                Knock::Refused("This room has ended.".into()),
                Changes::default(),
            );
        }
        // An invite's proof, or the door code people nearby have (it lets them knock, never
        // in: D19).
        let invited = proof_ok(&self.secret, key, proof);
        let nearby = !invited && proof_ok(&nearby_secret(&self.secret), key, proof);
        if !invited && !nearby {
            return (
                Knock::Refused("That invite isn't for this room.".into()),
                Changes::default(),
            );
        }
        if self.blocked.contains(key) {
            return (
                Knock::Refused("The host isn't letting you in.".into()),
                Changes::default(),
            );
        }
        if let Some(p) = self
            .people
            .iter_mut()
            .find(|p| p.key == key && p.id != HOST)
        {
            p.away = false;
            p.lost_at = None;
            p.last_heard_us = now_us;
            p.welcomed = false;
            let id = p.id;
            self.welcome(id, now_us);
            return (Knock::In(id), self.done(now_us));
        }
        if self.people.len() >= MAX_PEOPLE {
            return (
                Knock::Refused(format!("The room is full ({MAX_PEOPLE} people).")),
                Changes::default(),
            );
        }
        if let Some(i) = self.returning.iter().position(|r| r.key == key) {
            // Dropped for silence, back (a rejoin after a restart): the same member, at once.
            let r = self.returning.remove(i).expect("found");
            let mut p = Person::new(r.id, &r.key, &r.name, now_us);
            p.role = r.role;
            p.mode = r.mode;
            let id = p.id;
            self.people.push(p);
            self.note(id, "is back", false, now_us);
            self.welcome(id, now_us);
            return (Knock::In(id), self.done(now_us));
        }
        if nearby && !self.visible {
            return (
                Knock::Refused("This room isn't open to people nearby.".into()),
                Changes::default(),
            );
        }
        if let Some(w) = self.waiting.iter().find(|w| w.key == key) {
            let id = w.id;
            self.push(id, ToGuest::Waiting);
            return (Knock::Waiting(id), self.take());
        }
        if self.waiting.len() >= MAX_WAITING {
            return (
                Knock::Refused("Too many people are waiting to come in.".into()),
                Changes::default(),
            );
        }
        let id = self.next_member;
        self.next_member += 1;
        let p = Person::new(id, key, name, now_us);
        // Someone nearby always waits for the host (D19), whatever the invite's setting.
        if self.let_in_without_asking && !nearby {
            self.people.push(p);
            self.welcome(id, now_us);
            (Knock::In(id), self.done(now_us))
        } else {
            self.waiting.push(p);
            self.view_dirty = true;
            self.push(id, ToGuest::Waiting);
            (Knock::Waiting(id), self.take())
        }
    }

    /// The host lets someone in.
    pub fn admit(&mut self, id: u32, now_us: i64) -> Option<Changes> {
        let i = self.waiting.iter().position(|w| w.id == id)?;
        if self.people.len() >= MAX_PEOPLE || self.ended {
            return None;
        }
        let mut p = self.waiting.remove(i);
        p.last_heard_us = now_us;
        self.people.push(p);
        self.welcome(id, now_us);
        Some(self.done(now_us))
    }

    /// The host says no (they can ask again with the link).
    pub fn deny(&mut self, id: u32, now_us: i64) -> Changes {
        if let Some(i) = self.waiting.iter().position(|w| w.id == id) {
            self.waiting.remove(i);
            self.push(
                id,
                ToGuest::Refused {
                    why: "The host didn't let you in this time.".into(),
                },
            );
            self.out.gone.push(id);
            self.view_dirty = true;
        }
        self.done(now_us)
    }

    /// The host takes someone out (`block`: and keeps them out of this room).
    pub fn remove(&mut self, id: u32, block: bool, now_us: i64) -> Changes {
        if id == HOST {
            return Changes::default();
        }
        let key = self
            .people
            .iter()
            .chain(self.waiting.iter())
            .find(|p| p.id == id)
            .map(|p| p.key.clone());
        let Some(key) = key else {
            return Changes::default();
        };
        if block {
            self.blocked.insert(key);
        }
        self.push(
            id,
            ToGuest::Refused {
                why: format!("{} took you out of the room.", self.host_name),
            },
        );
        self.out_of_room(id);
        self.done(now_us)
    }

    /// They said goodbye: out at once (no waiting for them to come back).
    pub fn left(&mut self, id: u32, now_us: i64) -> Changes {
        if id == HOST || !self.is_member(id) {
            return Changes::default();
        }
        self.note(id, "left", false, now_us);
        self.out_of_room(id);
        self.done(now_us)
    }

    /// Anything from them (a message or a clock ping): a member shown away for silence, whose
    /// connection never closed, is back (WP8's ask).
    pub fn heard(&mut self, id: u32, now_us: i64) -> Changes {
        if let Some(p) = self.person_mut(id) {
            p.last_heard_us = now_us;
            if p.away && p.lost_at.is_none() {
                p.away = false;
                self.view_dirty = true;
            }
        }
        self.done(now_us)
    }

    /// Their connection was lost: away now, kept 2 minutes for a reconnect.
    pub fn lost(&mut self, id: u32, now_us: i64) -> Changes {
        // At the door: gone (they knock again when back).
        if let Some(i) = self.waiting.iter().position(|w| w.id == id) {
            self.waiting.remove(i);
            self.out.gone.push(id);
            self.view_dirty = true;
        }
        if id != HOST
            && let Some(p) = self.person_mut(id)
        {
            p.away = true;
            p.lost_at.get_or_insert(now_us);
            self.not_waiting_for(id);
            self.view_dirty = true;
        }
        self.done(now_us)
    }

    /// The host's transport's word on someone's path (polled, or on a change): whether it is
    /// direct, and its round trip.
    pub fn path(&mut self, id: u32, path: PathInfo, now_us: i64) -> Changes {
        let Some(p) = self.person_mut(id) else {
            return Changes::default();
        };
        p.direct = path.direct && !path.relay;
        if let Some(rtt) = path.rtt_us {
            p.rtts.push_back((now_us, rtt));
        }
        while p
            .rtts
            .front()
            .is_some_and(|s| now_us - s.0 > RTT_WINDOW_US || p.rtts.len() > RTT_SAMPLES)
        {
            p.rtts.pop_front();
        }
        // Every device needs the host's network: too slow (or relayed) for 30 s, apart.
        if p.mode == ListenMode::EveryDevice {
            let slow = !p.direct || p.rtt_median().is_some_and(|m| m > SAME_NETWORK_OFF_RTT_US);
            if !slow {
                p.slow_since = None;
            } else if now_us - *p.slow_since.get_or_insert(now_us) >= SAME_NETWORK_OFF_US {
                p.mode = ListenMode::Apart;
                p.slow_since = None;
                self.out.notes.push(HostNote::SameRoomOff { id });
                self.view_dirty = true;
            }
        }
        self.done(now_us)
    }

    // --- What people report -------------------------------------------------------------

    /// Ready for a prepare (or an armed next: nothing waits for that).
    pub fn ready(&mut self, id: u32, prep: PrepId, entry: EntryId, now_us: i64) -> Changes {
        let waited = self
            .prep
            .as_mut()
            .filter(|p| p.prep == prep && p.entry == entry)
            .is_some_and(|p| p.waiting.remove(&id));
        if waited && let Some(p) = self.person_mut(id) {
            p.missed = 0;
            p.told_slow = false;
        }
        if waited {
            self.view_dirty = true;
        }
        self.done(now_us)
    }

    /// Can't play an entry (a prepare's or an armed next's): not waited for it.
    pub fn unable(&mut self, id: u32, prep: PrepId, entry: EntryId, now_us: i64) -> Changes {
        let _ = prep;
        if let Some(p) = self.person_mut(id) {
            p.mark_unable(entry);
        }
        if self.prep.as_ref().is_some_and(|p| p.entry == entry) {
            self.not_waiting_for(id);
        }
        self.done(now_us)
    }

    /// How someone's player is now. Everyone's panel shows it, so the views go out when what
    /// they show changes (the line beside their name, their copy, the entry they hear).
    pub fn state(&mut self, id: u32, report: Report, now_us: i64) -> Changes {
        let host_name = self.host_name.clone();
        let Some(p) = self.person_mut(id) else {
            return Changes::default();
        };
        let before = (p.line(&host_name), p.copy, p.entry);
        let was_suspended = p.suspended();
        p.sub_state = Some(report.sub_state);
        p.entry = report.entry;
        p.error_us = report.error_us;
        p.bound_us = report.bound_us;
        p.copy = report.copy;
        if p.suspended() {
            p.suspended_since.get_or_insert(now_us);
        } else {
            p.suspended_since = None;
        }
        if report.sub_state == SubState::CannotPlay
            && let Some(e) = report.entry
        {
            p.mark_unable(e);
        }
        let back = was_suspended
            && matches!(
                report.sub_state,
                SubState::Ready | SubState::InStep | SubState::Correcting | SubState::Realigning
            );
        if before != (p.line(&host_name), p.copy, p.entry) {
            self.view_dirty = true;
        }
        if let Some(pr) = &self.prep {
            let entry = pr.entry;
            if !self.waits_for_id(id, entry) {
                self.not_waiting_for(id);
            }
        }
        // The room held itself for everyone's stall: someone has the music again.
        if back
            && self.prep.is_none()
            && let Play::Held { anchor, auto: true } = self.play
        {
            self.start_prep(anchor.entry, anchor.content_us, Purpose::Resume, now_us);
        }
        self.done(now_us)
    }

    /// How someone listens (D4: per person). Every device needs the host's network; asked
    /// for elsewhere, it is apart.
    pub fn mode(&mut self, id: u32, mode: ListenMode, now_us: i64) -> Changes {
        // The host's speaker is the one a remote listens on: the host is never a remote.
        if id == HOST && mode == ListenMode::OneSpeaker {
            return Changes::default();
        }
        let Some(p) = self.person_mut(id) else {
            return Changes::default();
        };
        let mode = if mode == ListenMode::EveryDevice && id != HOST && !p.same_network() {
            ListenMode::Apart
        } else {
            mode
        };
        if p.mode != mode {
            p.mode = mode;
            p.slow_since = None;
            self.view_dirty = true;
        }
        if mode == ListenMode::OneSpeaker {
            self.not_waiting_for(id);
        }
        self.done(now_us)
    }

    /// An entry's reference length, from the host's own player (µs); until then the song's
    /// stated length stands in.
    pub fn set_length(&mut self, entry: EntryId, length_us: i64, now_us: i64) -> Changes {
        if length_us > 0 && self.index_of(entry).is_some() {
            self.lengths.insert(entry, length_us);
        }
        self.done(now_us)
    }

    // --- Doing things -------------------------------------------------------------------

    /// Someone asks the room to do something. Their role is checked; the host may do anything.
    pub fn ask(&mut self, id: u32, action: Action, now_us: i64) -> Result<Changes, String> {
        if self.ended {
            return Err("The room has ended.".into());
        }
        let Some(role) = self.role_of(id) else {
            return Err("You're not in the room.".into());
        };
        if let Action::Volume { .. } = action {
            let remote = self.mode_of(id) == Some(ListenMode::OneSpeaker);
            if id != HOST && !(self.guests_volume && remote) {
                return Err("The host hasn't let you change the speaker's volume.".into());
            }
        } else if id != HOST && !allows(role, action.needs()) {
            return Err(match action.needs() {
                Role::Add => "The host hasn't let you add songs.".into(),
                _ => "The host is in control.".into(),
            });
        }
        self.act(id, action, now_us)?;
        self.quiet_since = Some(now_us);
        Ok(self.done(now_us))
    }

    /// Someone asks whoever controls (B13, and B16's "Ask the room to wait"); kept for 60 s, one
    /// per person. A request is never done by itself, whoever asks: someone with control
    /// accepts it. Adding is never a request: someone who may add just adds.
    pub fn request(&mut self, id: u32, action: Action, now_us: i64) -> Result<Changes, String> {
        if self.ended {
            return Err("The room has ended.".into());
        }
        let Some(role) = self.role_of(id) else {
            return Err("You're not in the room.".into());
        };
        match action {
            Action::Accept { .. } | Action::Decline { .. } | Action::Volume { .. } => {
                return Err("That isn't something to ask for.".into());
            }
            Action::Add { .. } if id == HOST || allows(role, Role::Add) => {
                return self.ask(id, action, now_us);
            }
            Action::Add { .. } => return Err("The host hasn't let you add songs.".into()),
            _ => {}
        }
        let entry = self.base(now_us).map(|b| b.0);
        self.requests.retain(|r| r.from != id);
        let rid = self.next_request;
        self.next_request += 1;
        self.requests.push(Request {
            id: rid,
            from: id,
            action,
            at_us: now_us,
            entry,
        });
        while self.requests.len() > MAX_REQUESTS {
            self.requests.remove(0);
        }
        self.view_dirty = true;
        Ok(self.done(now_us))
    }

    /// The host undoes the last skip or queue change: the queue as it was, and the song before
    /// the change where it would be now.
    pub fn undo(&mut self, id: u32, now_us: i64) -> Result<Changes, String> {
        if self.ended {
            return Err("The room has ended.".into());
        }
        if id != HOST {
            return Err("Only the host can undo.".into());
        }
        let Some(u) = self.undo.take() else {
            return Err("Nothing to undo.".into());
        };
        // The song playing stays in the queue, unless the undo goes back to the one before.
        let playing = self.anchor().map(|a| a.entry);
        let goes_back = u.anchor.is_some_and(|a| Some(a.entry) != playing);
        if let Some(e) = playing
            && !goes_back
            && !u.queue.iter().any(|q| q.id == e)
        {
            self.undo = Some(u);
            return Err("The song playing can't be taken out; skip it instead.".into());
        }
        // A prepare the undone change made (a skip on its way) goes; a seek stays.
        let back_to = u.anchor.map(|a| a.entry).or(playing);
        if self.prep.as_ref().is_some_and(|p| Some(p.entry) != back_to) {
            self.prep = None;
        }
        self.queue = u.queue;
        if let Some(a) = self.activity.iter_mut().rev().find(|a| a.undoable) {
            a.undoable = false;
        }
        self.note(HOST, "undid the last change", false, now_us);
        let now_entry = self.anchor().map(|a| a.entry);
        if let Some(was) = u.anchor
            && Some(was.entry) != now_entry
            && self.index_of(was.entry).is_some()
        {
            if was.playing && matches!(self.play, Play::Playing(_)) {
                let purpose = self.purpose_for(was.entry, false);
                self.start_prep(was.entry, was.content_at(now_us), purpose, now_us);
            } else {
                let anchor = Anchor {
                    host_us: now_us,
                    playing: false,
                    seq: self.new_seq(),
                    ..was
                };
                self.hold_at(anchor, false, now_us);
            }
        }
        if let Play::Resting { after: Some(e) } = self.play
            && self.index_of(e).is_none()
        {
            self.play = Play::Resting { after: None };
        }
        self.queue_changed(now_us);
        self.quiet_since = Some(now_us);
        Ok(self.done(now_us))
    }

    /// One message from a member, as the session reads it (a hello goes through `knock`).
    /// The session also calls `heard` for every message and clock ping.
    pub fn handle(&mut self, id: u32, msg: ToHost, now_us: i64) -> Result<Changes, String> {
        Ok(match msg {
            ToHost::Hello { .. } => return Err("Already in the room.".into()),
            ToHost::Ready { prep, entry } => self.ready(id, prep, entry, now_us),
            ToHost::Unable { prep, entry, .. } => self.unable(id, prep, entry, now_us),
            ToHost::State {
                entry,
                sub_state,
                error_us,
                bound_us,
                copy,
            } => self.state(
                id,
                Report {
                    entry,
                    sub_state,
                    error_us,
                    bound_us,
                    copy,
                },
                now_us,
            ),
            ToHost::Mode { mode } => self.mode(id, mode, now_us),
            ToHost::Ask { action } => return self.ask(id, action, now_us),
            ToHost::Request { action } => return self.request(id, action, now_us),
            ToHost::Undo => return self.undo(id, now_us),
            ToHost::Bye => self.left(id, now_us),
        })
    }

    /// Time passes: who is away or gone, prepares that waited long enough, boundaries, the end
    /// of the queue, the room holding or skipping by itself, requests growing old, an idle end.
    pub fn tick(&mut self, now_us: i64) -> Changes {
        if !self.ended {
            self.presence(now_us);
        }
        self.done(now_us)
    }

    /// End for everyone (the host's, confirmed): everyone is told, the host's own conductor
    /// too, and the room takes nothing more.
    pub fn end(&mut self, now_us: i64) -> Changes {
        if !self.ended {
            let why = format!("{} ended the room.", self.host_name);
            self.end_room(why);
        }
        let _ = now_us;
        self.take()
    }

    // --- The host's settings ------------------------------------------------------------

    /// Between songs: gapless, a cut, or a crossfade of 2, 4, 6 or 8 s (never within an album).
    pub fn set_transition(&mut self, t: Transition, now_us: i64) -> Result<Changes, String> {
        if let Transition::Crossfade { ms } = t
            && ![2_000, 4_000, 6_000, 8_000].contains(&ms)
        {
            return Err("A crossfade of 2, 4, 6 or 8 seconds.".into());
        }
        self.transition = t;
        self.quiet_since = Some(now_us);
        self.view_dirty = true;
        Ok(self.done(now_us))
    }

    /// The role guests have unless given one (`None`: control up to 4 people, add above).
    pub fn set_new_role(&mut self, role: Option<Role>, now_us: i64) -> Changes {
        self.new_role = role;
        self.quiet_since = Some(now_us);
        self.view_dirty = true;
        self.done(now_us)
    }

    /// One guest's role (`None`: the room's default again).
    pub fn set_role(&mut self, id: u32, role: Option<Role>, now_us: i64) -> Changes {
        if id != HOST
            && let Some(p) = self.person_mut(id)
        {
            p.role = role;
            self.view_dirty = true;
        }
        self.quiet_since = Some(now_us);
        self.done(now_us)
    }

    /// Remotes may change the host's speaker's volume (off by default).
    pub fn set_guests_volume(&mut self, on: bool, now_us: i64) -> Changes {
        self.guests_volume = on;
        self.quiet_since = Some(now_us);
        self.view_dirty = true;
        self.done(now_us)
    }

    /// The host's speaker's volume as its player has it now (0 to 100): in the host's view, and
    /// in remotes', while the host lets guests change it (B17).
    pub fn set_volume(&mut self, percent: Option<u8>, now_us: i64) -> Changes {
        let p = percent.map(|p| p.min(100));
        if self.volume != p {
            self.volume = p;
            self.view_dirty = true;
        }
        self.done(now_us)
    }

    /// Whether the room follows the host's player (one speaker, 4.13): every member but the host
    /// is a remote, or the host is alone (A19). Then the host's player plays free, as solo.
    pub fn host_leads(&self) -> bool {
        // Someone just let in counts once they have said how they listen (their mode, or how
        // their player is): a remote coming in never makes the host's player follow the clock
        // for a moment.
        self.people
            .iter()
            .all(|p| p.id == HOST || p.mode == ListenMode::OneSpeaker || p.sub_state.is_none())
    }

    /// Where the host's player is heard now (`RoomPlayStatus::heard`), each step. While the host
    /// leads, a heard place more than 20 ms from the anchor becomes the anchor: the room follows
    /// the host's sound card (remotes' progress moves; the host's player is untouched, its commit
    /// a no-op). When the host stops leading, a place 1 ms off is taken once, so whoever plays
    /// now follows the host's real place. Never while a prepare waits, nor within 3 s of the
    /// armed boundary (C6: the boundary stays).
    pub fn heard_host(&mut self, heard: Anchor, now_us: i64) -> Changes {
        let leads = self.host_leads();
        let limit = match (leads, self.led) {
            (true, _) => Some(REANCHOR_US),
            (false, true) => Some(REANCHOR_ONCE_US),
            (false, false) => None,
        };
        let Play::Playing(a) = self.play else {
            self.led = leads;
            return self.done(now_us);
        };
        let Some(limit) = limit else {
            return self.done(now_us);
        };
        let near_boundary = self
            .armed
            .is_some_and(|ar| now_us >= ar.next.boundary_host_us - FREEZE_US);
        if self.ended
            || self.prep.is_some()
            || near_boundary
            || heard.entry != a.entry
            || heard.host_us < a.host_us
        {
            return self.done(now_us);
        }
        self.led = leads;
        let drift = heard.content_us - a.content_at(heard.host_us);
        if drift.abs() > limit {
            let anchor = Anchor {
                entry: a.entry,
                content_us: heard.content_us.max(0),
                host_us: heard.host_us,
                playing: true,
                seq: self.new_seq(),
            };
            self.set_playing(anchor);
        }
        self.done(now_us)
    }

    pub fn set_let_in_without_asking(&mut self, on: bool) {
        self.let_in_without_asking = on;
    }

    pub fn lets_in_without_asking(&self) -> bool {
        self.let_in_without_asking
    }

    /// "Visible on this network" (D19): off, people nearby are refused at the door (members
    /// and those coming back still come in).
    pub fn set_visible(&mut self, on: bool) {
        self.visible = on;
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    // --- Kept and reopened (D8, the WP15 notes) -----------------------------------------

    /// What the room needs to come back, as it is at `now_us`: a playing room's place is
    /// where it is heard now (under the song's length).
    pub fn to_keep(&self, now_us: i64) -> RoomState {
        let place = match self.play {
            Play::Resting { after: None } => Place::Resting,
            Play::Resting { after: Some(e) } => Place::QueueEnded { after: e },
            Play::Playing(a) | Play::Held { anchor: a, .. } => {
                let mut c = if a.playing {
                    a.content_at(now_us)
                } else {
                    a.content_us
                };
                if let Some(len) = self.length(a.entry) {
                    c = c.min(len);
                }
                Place::At {
                    entry: a.entry,
                    content_us: c.max(0),
                }
            }
        };
        let kept = |p: &Person| KeptPerson {
            id: p.id,
            key: p.key.clone(),
            name: p.name.clone(),
            role: p.role,
            mode: p.mode,
        };
        let mut lengths: Vec<(EntryId, i64)> = self.lengths.iter().map(|(e, l)| (*e, *l)).collect();
        lengths.sort_unstable();
        let mut blocked: Vec<String> = self.blocked.iter().cloned().collect();
        blocked.sort_unstable();
        RoomState {
            host_name: self.host_name.clone(),
            secret: self.secret.clone(),
            queue: self.queue.clone(),
            lengths,
            place,
            people: self
                .people
                .iter()
                .filter(|p| p.id != HOST)
                .map(kept)
                .collect(),
            returning: self.returning.iter().cloned().collect(),
            blocked,
            transition: self.transition,
            new_role: self.new_role,
            guests_volume: self.guests_volume,
            let_in_without_asking: self.let_in_without_asking,
            visible: self.visible,
            next_member: self.next_member,
            next_entry: self.next_entry,
            next_prep: self.next_prep,
            seq: self.seq,
            next_request: self.next_request,
        }
    }

    /// A kept room back, at the host's time `now_us`: held at its place, its members away
    /// (kept 2 minutes from now for their reconnects, D17), every id and the sequence going on
    /// past anything given before. Nothing plays until a press starts it.
    pub fn reopen(s: RoomState, now_us: i64) -> Room {
        let margin = REOPEN_MARGIN;
        let mut r = Room::new(&s.host_name, &s.secret, Vec::new(), Opening::Resting);
        r.queue = s.queue.into_iter().take(MAX_QUEUE).collect();
        r.lengths = s.lengths.into_iter().collect();
        r.blocked = s.blocked.into_iter().collect();
        r.transition = s.transition;
        r.new_role = s.new_role;
        r.guests_volume = s.guests_volume;
        r.let_in_without_asking = s.let_in_without_asking;
        r.visible = s.visible;
        r.next_member = s.next_member.saturating_add(margin as u32);
        r.next_entry = s.next_entry.saturating_add(margin);
        r.next_prep = s.next_prep.saturating_add(margin);
        r.seq = s.seq.saturating_add(margin);
        r.next_request = s.next_request.saturating_add(margin as u32);
        for k in s.people.into_iter().take(MAX_PEOPLE - 1) {
            if k.id == HOST || r.people.iter().any(|p| p.id == k.id || p.key == k.key) {
                continue;
            }
            let mut p = Person::new(k.id, &k.key, &k.name, now_us);
            p.role = k.role;
            p.mode = k.mode;
            p.away = true;
            p.lost_at = Some(now_us);
            r.people.push(p);
        }
        r.returning = s
            .returning
            .into_iter()
            .filter(|k| !r.people.iter().any(|p| p.key == k.key))
            .collect();
        while r.returning.len() > RETURNING_KEPT {
            r.returning.pop_front();
        }
        r.play = match s.place {
            Place::At { entry, content_us } if r.index_of(entry).is_some() => {
                let seq = r.new_seq();
                Play::Held {
                    anchor: Anchor {
                        entry,
                        content_us: content_us.max(0),
                        host_us: now_us,
                        playing: false,
                        seq,
                    },
                    auto: false,
                }
            }
            Place::QueueEnded { after } if r.index_of(after).is_some() => {
                Play::Resting { after: Some(after) }
            }
            _ => Play::Resting { after: None },
        };
        r.quiet_since = Some(now_us);
        r.note(HOST, "reopened the room", false, now_us);
        r
    }

    // --- Reading ------------------------------------------------------------------------

    /// The room as one person sees it: the host also sees who's waiting by name and everyone's
    /// copy; a guest sees their own copy (D14).
    pub fn view_for(&self, id: u32) -> RoomView {
        let for_host = id == HOST;
        RoomView {
            host_name: self.host_name.clone(),
            members: self
                .people
                .iter()
                .map(|p| Member {
                    id: p.id,
                    name: p.name.clone(),
                    host: p.id == HOST,
                    role: self.role_of_person(p),
                    mode: p.mode,
                    sub_state: if p.away { None } else { p.sub_state },
                    away: p.away,
                    copy: if for_host || p.id == id { p.copy } else { None },
                    error_us: p.error_us,
                    bound_us: p.bound_us,
                })
                .collect(),
            waiting: self
                .waiting
                .iter()
                .map(|w| {
                    let name = if for_host {
                        w.name.clone()
                    } else {
                        String::new()
                    };
                    (w.id, name)
                })
                .collect(),
            queue: self.queue.clone(),
            phase: self.phase(),
            anchor: self.anchor(),
            next: self.armed.map(|a| a.next),
            starting: self.prep.as_ref().map(|p| {
                self.people
                    .iter()
                    .filter(|m| p.waiting.contains(&m.id))
                    .map(|m| m.name.clone())
                    .collect()
            }),
            activity: self.activity.iter().cloned().collect(),
            settings: Settings {
                transition: self.transition,
                new_role: self.default_role(),
                auto_roles: self.new_role.is_none(),
                guests_volume: self.guests_volume,
            },
            requests: self
                .requests
                .iter()
                .map(|r| RequestView {
                    id: r.id,
                    from: r.from,
                    action: r.action.clone(),
                    at_us: r.at_us,
                })
                .collect(),
            volume: if for_host
                || (self.guests_volume && self.mode_of(id) == Some(ListenMode::OneSpeaker))
            {
                self.volume
            } else {
                None
            },
        }
    }

    pub fn is_member(&self, id: u32) -> bool {
        self.people.iter().any(|p| p.id == id)
    }

    pub fn role_of(&self, id: u32) -> Option<Role> {
        self.person(id).map(|p| self.role_of_person(p))
    }

    pub fn mode_of(&self, id: u32) -> Option<ListenMode> {
        self.person(id).map(|p| p.mode)
    }

    /// The people in the room now (the host first), for sending to.
    pub fn member_ids(&self) -> Vec<u32> {
        self.people.iter().map(|p| p.id).collect()
    }

    /// What the room plays or holds now.
    pub fn anchor(&self) -> Option<Anchor> {
        match self.play {
            Play::Playing(a) | Play::Held { anchor: a, .. } => Some(a),
            Play::Resting { .. } => None,
        }
    }

    /// The armed next.
    pub fn next(&self) -> Option<Next> {
        self.armed.map(|a| a.next)
    }

    pub fn phase(&self) -> Phase {
        match self.play {
            Play::Resting { after: None } => Phase::Resting,
            Play::Resting { after: Some(_) } => Phase::QueueEnded,
            Play::Playing(_) => Phase::Playing,
            Play::Held { .. } => Phase::Held,
        }
    }

    /// The prepare under way: its id, entry, place and purpose.
    pub fn preparing(&self) -> Option<(PrepId, EntryId, i64, Purpose)> {
        self.prep
            .as_ref()
            .map(|p| (p.prep, p.entry, p.content_us, p.purpose))
    }

    pub fn queue(&self) -> &[Entry] {
        &self.queue
    }

    pub fn has_ended(&self) -> bool {
        self.ended
    }

    // --- Inside: people -----------------------------------------------------------------

    fn person(&self, id: u32) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }

    fn person_mut(&mut self, id: u32) -> Option<&mut Person> {
        self.people.iter_mut().find(|p| p.id == id)
    }

    fn name_of(&self, id: u32) -> String {
        self.person(id).map(|p| p.name.clone()).unwrap_or_default()
    }

    fn default_role(&self) -> Role {
        self.new_role
            .unwrap_or(if self.people.len() <= AUTO_CONTROL_UP_TO {
                Role::Control
            } else {
                Role::Add
            })
    }

    fn role_of_person(&self, p: &Person) -> Role {
        if p.id == HOST {
            Role::Control
        } else {
            p.role.unwrap_or_else(|| self.default_role())
        }
    }

    /// Whether a prepare of `entry` waits for this person: present, playing (not a remote),
    /// not suspended, not stepped out, able to play it. The host is a member like the others.
    fn waits_for(p: &Person, entry: EntryId) -> bool {
        p.plays() && !p.suspended() && !p.stepped_out() && !p.cannot(entry)
    }

    fn waits_for_id(&self, id: u32, entry: EntryId) -> bool {
        self.person(id).is_some_and(|p| Room::waits_for(p, entry))
    }

    fn not_waiting_for(&mut self, id: u32) {
        if let Some(p) = &mut self.prep
            && p.waiting.remove(&id)
        {
            self.view_dirty = true;
        }
    }

    /// Their welcome: the view (after everyone else's, which now shows them), then the
    /// prepare under way (they're waited for it too, A11), the armed next and the end said.
    fn welcome(&mut self, id: u32, now_us: i64) {
        self.view_dirty = true;
        self.flush_views();
        let room = self.view_for(id);
        self.push(id, ToGuest::Welcome { you: id, room });
        if let Some(p) = self.person_mut(id) {
            p.welcomed = true;
        }
        if let Some(pr) = self.prep.clone() {
            if self.waits_for_id(id, pr.entry)
                && let Some(p) = self.prep.as_mut()
            {
                p.waiting.insert(id);
                self.view_dirty = true;
            }
            self.push(
                id,
                ToGuest::Prepare {
                    prep: pr.prep,
                    entry: pr.entry,
                    content_us: pr.content_us,
                    purpose: pr.purpose,
                },
            );
        }
        if let Some(a) = self.armed {
            self.push(
                id,
                ToGuest::ArmNext {
                    prep: a.prep,
                    next: a.next,
                },
            );
        }
        if let Some(anchor) = self.ending {
            self.push(id, ToGuest::Hold { anchor });
        }
        let _ = now_us;
    }

    /// Out of the room (left, removed, gone): no longer waited for, their requests dropped.
    fn out_of_room(&mut self, id: u32) {
        self.people.retain(|p| p.id != id);
        self.waiting.retain(|p| p.id != id);
        self.requests.retain(|r| r.from != id);
        if let Some(p) = &mut self.prep {
            p.waiting.remove(&id);
        }
        self.out.gone.push(id);
        self.view_dirty = true;
    }

    /// Silent 5 s: away. Silent 15 s: lost. Lost 2 minutes: gone.
    fn presence(&mut self, now_us: i64) {
        let mut gone = Vec::new();
        let mut dirty = false;
        for p in self.people.iter_mut().filter(|p| p.id != HOST) {
            let silent = now_us - p.last_heard_us;
            if !p.away && silent >= AWAY_AFTER_US {
                p.away = true;
                dirty = true;
            }
            if p.away && p.lost_at.is_none() && silent >= SILENT_LOST_US {
                p.lost_at = Some(p.last_heard_us + SILENT_LOST_US);
            }
            if p.lost_at.is_some_and(|l| now_us - l >= KEEP_AWAY_US) {
                gone.push(p.id);
            }
        }
        if dirty {
            self.view_dirty = true;
        }
        let away: Vec<u32> = self
            .people
            .iter()
            .filter(|p| p.away)
            .map(|p| p.id)
            .collect();
        for id in away {
            self.not_waiting_for(id);
        }
        for id in gone {
            self.note(id, "lost connection", false, now_us);
            if let Some(p) = self.person(id) {
                // Known again if they come back (a rejoin after a restart, WP15).
                let kept = KeptPerson {
                    id: p.id,
                    key: p.key.clone(),
                    name: p.name.clone(),
                    role: p.role,
                    mode: p.mode,
                };
                self.returning.retain(|r| r.key != kept.key);
                self.returning.push_back(kept);
                while self.returning.len() > RETURNING_KEPT {
                    self.returning.pop_front();
                }
            }
            self.out_of_room(id);
        }
    }

    /// The worst p95 round trip among the guests who play, and whether they're all on the
    /// host's network.
    fn path_now(&self) -> (i64, bool) {
        let guests = || self.people.iter().filter(|p| p.id != HOST && p.plays());
        let p95 = guests()
            .filter_map(Person::rtt_p95)
            .max()
            .map_or(0, i64::from);
        (p95, guests().all(Person::same_network))
    }

    fn start_lead(&self) -> i64 {
        let (rtt, one) = self.path_now();
        tl::start_lead_us(rtt, one)
    }

    fn pause_lead(&self) -> i64 {
        let (rtt, one) = self.path_now();
        tl::pause_lead_us(rtt, one)
    }

    // --- Inside: the queue --------------------------------------------------------------

    fn new_entry_id(&mut self) -> EntryId {
        let id = EntryId(self.next_entry);
        self.next_entry += 1;
        id
    }

    fn new_prep_id(&mut self) -> PrepId {
        let id = PrepId(self.next_prep);
        self.next_prep += 1;
        id
    }

    fn new_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn index_of(&self, entry: EntryId) -> Option<usize> {
        self.queue.iter().position(|e| e.id == entry)
    }

    fn entry_after(&self, entry: EntryId) -> Option<EntryId> {
        let i = self.index_of(entry)?;
        self.queue.get(i + 1).map(|e| e.id)
    }

    /// The first entry after `after` (from the start with `None`).
    fn first_after(&self, after: Option<EntryId>) -> Option<EntryId> {
        match after {
            None => self.queue.first().map(|e| e.id),
            Some(e) => self.entry_after(e),
        }
    }

    fn song(&self, entry: EntryId) -> Option<&Song> {
        self.index_of(entry).map(|i| &self.queue[i].song)
    }

    fn title(&self, entry: EntryId) -> String {
        self.song(entry)
            .map(|s| short(&s.title))
            .unwrap_or_default()
    }

    /// The reference copy's length: the host's player's, else the song's stated length.
    fn length(&self, entry: EntryId) -> Option<i64> {
        self.lengths.get(&entry).copied().or_else(|| {
            self.song(entry)
                .and_then(|s| s.duration_ms)
                .map(|ms| ms as i64 * 1_000)
        })
    }

    fn transition_between(&self, a: EntryId, b: EntryId) -> Transition {
        match (self.transition, self.song(a), self.song(b)) {
            (Transition::Crossfade { .. }, Some(x), Some(y)) if same_album(x, y) => {
                Transition::Gapless
            }
            (t, _, _) => t,
        }
    }

    /// Where a press starts from: a prepare's target while one is under way, else the place
    /// playing (now) or held.
    fn base(&self, now_us: i64) -> Option<(EntryId, i64)> {
        if let Some(p) = &self.prep {
            return Some((p.entry, p.content_us));
        }
        match self.play {
            Play::Playing(a) => Some((a.entry, a.content_at(now_us))),
            Play::Held { anchor, .. } => Some((anchor.entry, anchor.content_us)),
            Play::Resting { .. } => None,
        }
    }

    fn purpose_for(&self, target: EntryId, jump: bool) -> Purpose {
        match self.play {
            Play::Resting { .. } => Purpose::Start,
            Play::Held { anchor, .. } if anchor.entry == target => Purpose::Resume,
            Play::Held { .. } => Purpose::Start,
            Play::Playing(a) if a.entry == target => Purpose::Seek,
            Play::Playing(_) if jump => Purpose::Jump,
            Play::Playing(_) => Purpose::Skip,
        }
    }

    /// The armed next within 3 s of its boundary, for the anchor it was armed for.
    fn frozen(&self, now_us: i64) -> Option<EntryId> {
        let a = self.armed?;
        let seq = self.anchor()?.seq;
        (a.anchor_seq == seq && now_us >= a.next.boundary_host_us - FREEZE_US)
            .then_some(a.next.entry)
    }

    /// After any change to the queue: the frozen armed next stays right after the playing
    /// entry (C6: the change applies to the one after), and everyone's view changes.
    fn queue_changed(&mut self, now_us: i64) {
        if let (Some(armed), Some(a)) = (self.frozen(now_us), self.anchor())
            && let (Some(ai), Some(ni)) = (self.index_of(a.entry), self.index_of(armed))
            && ni != ai + 1
        {
            let e = self.queue.remove(ni);
            let at = self.index_of(a.entry).map_or(self.queue.len(), |i| i + 1);
            self.queue.insert(at, e);
        }
        self.view_dirty = true;
    }

    fn keep_undo(&mut self) {
        self.undo = Some(Undo {
            queue: self.queue.clone(),
            anchor: self.anchor(),
        });
    }

    fn note(&mut self, who: u32, what: &str, undoable: bool, now_us: i64) {
        if undoable {
            for a in &mut self.activity {
                a.undoable = false;
            }
        }
        let what = match &self.said_yes {
            Some(by) => format!("{what} ({by} said yes)"),
            None => what.to_owned(),
        };
        let name = self.name_of(who);
        self.activity.push_back(Activity {
            who,
            name,
            what,
            at_us: now_us,
            undoable,
        });
        while self.activity.len() > ACTIVITY {
            self.activity.pop_front();
        }
        self.view_dirty = true;
    }

    // --- Inside: doing ------------------------------------------------------------------

    /// Do `action` as `who` (their role already checked).
    fn act(&mut self, who: u32, action: Action, now_us: i64) -> Result<(), String> {
        match action {
            Action::Play => {
                if self.prep.is_some() {
                    return Ok(());
                }
                match self.play {
                    Play::Playing(_) => {}
                    Play::Held { anchor, .. } => {
                        self.start_prep(anchor.entry, anchor.content_us, Purpose::Resume, now_us)
                    }
                    Play::Resting { after } => {
                        if let Some(e) = self.first_after(after) {
                            self.start_prep(e, 0, Purpose::Start, now_us);
                        }
                    }
                }
            }
            Action::Pause => self.pause(who, now_us),
            Action::Seek { content_us } => {
                let Some((entry, _)) = self.base(now_us) else {
                    return Ok(());
                };
                let c = match self.length(entry) {
                    Some(len) => content_us.clamp(0, len),
                    None => content_us.max(0),
                };
                if let Some(p) = &self.prep {
                    let purpose = self.purpose_for(p.entry, p.purpose == Purpose::Jump);
                    self.start_prep(entry, c, purpose, now_us);
                    return Ok(());
                }
                match self.play {
                    Play::Playing(a) => self.start_prep(a.entry, c, Purpose::Seek, now_us),
                    Play::Held { anchor, .. } => {
                        let moved = Anchor {
                            content_us: c,
                            host_us: now_us,
                            seq: self.new_seq(),
                            ..anchor
                        };
                        self.hold_at(moved, false, now_us);
                    }
                    Play::Resting { .. } => {}
                }
            }
            Action::Next => {
                let Some(next) = self.base(now_us).and_then(|b| self.entry_after(b.0)) else {
                    return Ok(());
                };
                self.keep_undo();
                self.note(who, "skipped", true, now_us);
                let purpose = self.purpose_for(next, false);
                self.start_prep(next, 0, purpose, now_us);
            }
            Action::Previous => {
                let Some((entry, at)) = self.base(now_us) else {
                    return Ok(());
                };
                let i = self.index_of(entry).unwrap_or(0);
                let to = if at > PREVIOUS_RESTARTS_US || i == 0 {
                    entry
                } else {
                    self.queue[i - 1].id
                };
                self.keep_undo();
                let what = if to == entry {
                    "went back to the start"
                } else {
                    "went back a song"
                };
                self.note(who, what, true, now_us);
                let purpose = self.purpose_for(to, false);
                self.start_prep(to, 0, purpose, now_us);
            }
            Action::Jump { entry } => {
                if self.index_of(entry).is_none() {
                    return Err("That song isn't in the queue any more.".into());
                }
                self.keep_undo();
                let what = format!("played “{}”", self.title(entry));
                self.note(who, &what, true, now_us);
                let purpose = self.purpose_for(entry, true);
                self.start_prep(entry, 0, purpose, now_us);
            }
            Action::Add { mut songs, next } => {
                songs.truncate(MAX_QUEUE.saturating_sub(self.queue.len()));
                if songs.is_empty() {
                    return Err("The room's queue is full.".into());
                }
                self.keep_undo();
                let what = if songs.len() == 1 {
                    format!("added “{}”", short(&songs[0].title))
                } else {
                    format!("added {} songs", songs.len())
                };
                let after = match (next, self.base(now_us), self.play) {
                    (true, Some((e, _)), _) => Some(e),
                    (true, None, Play::Resting { after }) => after,
                    _ => None,
                };
                let at = after
                    .and_then(|e| self.index_of(e))
                    .map_or(self.queue.len(), |i| i + 1);
                for (k, mut s) in songs.into_iter().enumerate() {
                    s.added_by = Some(who);
                    let id = self.new_entry_id();
                    self.queue.insert(at + k, Entry { id, song: s });
                }
                self.note(who, &what, true, now_us);
                self.queue_changed(now_us);
                // Added to an empty or resting room: it starts.
                if let Play::Resting { after } = self.play
                    && self.prep.is_none()
                    && let Some(e) = self.first_after(after)
                {
                    self.start_prep(e, 0, Purpose::Start, now_us);
                }
            }
            Action::Remove { entry } => {
                let Some(i) = self.index_of(entry) else {
                    return Err("That song isn't in the queue any more.".into());
                };
                if self.anchor().is_some_and(|a| a.entry == entry) {
                    return Err("The song playing can't be taken out; skip it instead.".into());
                }
                if self.prep.as_ref().is_some_and(|p| p.entry == entry)
                    || self.frozen(now_us) == Some(entry)
                {
                    return Err("That song is about to start; skip it instead.".into());
                }
                self.keep_undo();
                let what = format!("took out “{}”", self.title(entry));
                if let Play::Resting { after: Some(e) } = self.play
                    && e == entry
                {
                    self.play = Play::Resting {
                        after: i.checked_sub(1).map(|j| self.queue[j].id),
                    };
                }
                self.queue.remove(i);
                self.note(who, &what, true, now_us);
                self.queue_changed(now_us);
            }
            Action::Move { entry, to } => {
                let Some(from) = self.index_of(entry) else {
                    return Err("That song isn't in the queue any more.".into());
                };
                if self.frozen(now_us) == Some(entry) {
                    return Err("That song is about to start; it stays next.".into());
                }
                let to = to.min(self.queue.len() - 1);
                if from == to {
                    return Ok(());
                }
                self.keep_undo();
                let e = self.queue.remove(from);
                self.queue.insert(to, e);
                self.note(who, "moved a song", true, now_us);
                self.queue_changed(now_us);
            }
            Action::Volume { percent } => {
                let p = percent.min(100);
                self.out.volume = Some(p);
                if self.volume != Some(p) {
                    self.volume = Some(p);
                    self.view_dirty = true;
                }
            }
            Action::Accept { request } => {
                let Some(i) = self.requests.iter().position(|r| r.id == request) else {
                    return Err("That request has gone.".into());
                };
                let r = self.requests.remove(i);
                self.view_dirty = true;
                let moved_on = matches!(
                    r.action,
                    Action::Next | Action::Previous | Action::Seek { .. }
                ) && r.entry != self.base(now_us).map(|b| b.0);
                if moved_on {
                    return Err("That request is out of date.".into());
                }
                if !self.is_member(r.from) {
                    return Err("They've left the room.".into());
                }
                self.said_yes = Some(self.name_of(who));
                let done = self.act(r.from, r.action, now_us);
                self.said_yes = None;
                done?;
            }
            Action::Decline { request } => {
                self.requests.retain(|r| r.id != request);
                self.view_dirty = true;
            }
        }
        Ok(())
    }

    /// A pause for everyone, a pause lead ahead while music sounds, so everyone fades out from
    /// the same moment. During a prepare it lands where the prepare was going (a skip, then a
    /// pause: paused at the next song).
    fn pause(&mut self, who: u32, now_us: i64) {
        let sounding = matches!(self.play, Play::Playing(_));
        let host_us = if sounding {
            now_us + self.pause_lead()
        } else {
            now_us
        };
        let (entry, content_us) = match (self.prep.take(), self.play) {
            (Some(p), _) => (p.entry, p.content_us),
            (None, Play::Playing(a)) => (a.entry, a.content_at(host_us)),
            (None, _) => return,
        };
        let anchor = Anchor {
            entry,
            content_us,
            host_us,
            playing: false,
            seq: self.new_seq(),
        };
        self.note(who, "paused", false, now_us);
        self.hold_at(anchor, false, now_us);
    }

    fn hold_at(&mut self, anchor: Anchor, auto: bool, now_us: i64) {
        self.play = Play::Held { anchor, auto };
        self.ending = None;
        self.quiet_since = Some(now_us);
        self.view_dirty = true;
        self.send_all(ToGuest::Hold { anchor });
    }

    /// An entry asked about anew (a prepare or an arm): everyone answers again, so someone
    /// who couldn't play it before (a stream that failed) is asked, not left out.
    fn ask_again(&mut self, entry: EntryId) {
        for p in &mut self.people {
            p.unable.retain(|e| *e != entry);
        }
    }

    fn start_prep(&mut self, entry: EntryId, content_us: i64, purpose: Purpose, now_us: i64) {
        let prep = self.new_prep_id();
        self.ask_again(entry);
        let waiting = self
            .people
            .iter()
            .filter(|p| Room::waits_for(p, entry))
            .map(|p| p.id)
            .collect();
        self.prep = Some(Prep {
            prep,
            entry,
            content_us,
            purpose,
            waiting,
            since_us: now_us,
        });
        self.view_dirty = true;
        self.send_all(ToGuest::Prepare {
            prep,
            entry,
            content_us,
            purpose,
        });
    }

    fn set_playing(&mut self, anchor: Anchor) {
        self.play = Play::Playing(anchor);
        self.ending = None;
        self.unable_since = None;
        self.view_dirty = true;
        self.send_all(ToGuest::Commit { anchor });
    }

    // --- Inside: time -------------------------------------------------------------------

    /// Everything that follows from the state and the time, then what to send.
    fn done(&mut self, now_us: i64) -> Changes {
        self.settle(now_us);
        self.take()
    }

    fn settle(&mut self, now_us: i64) {
        if self.ended {
            return;
        }
        self.quiet_since.get_or_insert(now_us);
        // A very short song can end within one call: let each step happen in turn.
        for _ in 0..16 {
            let before = (self.seq, self.next_prep, self.prep.is_some());
            self.commit_if_ready(now_us);
            self.boundary(now_us);
            self.update_next(now_us);
            self.end_of_queue(now_us);
            if before == (self.seq, self.next_prep, self.prep.is_some()) {
                break;
            }
        }
        self.auto_hold(now_us);
        self.auto_skip(now_us);
        let before = self.requests.len();
        self.requests.retain(|r| now_us - r.at_us < REQUEST_KEEP_US);
        if self.requests.len() != before {
            self.view_dirty = true;
        }
        let idle = self.prep.is_none() && !matches!(self.play, Play::Playing(_));
        if idle && self.quiet_since.is_some_and(|q| now_us - q >= IDLE_END_US) {
            self.end_room("The room ended: nothing played for an hour.".into());
        }
    }

    /// Everyone ready, or 2.5 s: one anchor for all, a start lead ahead. Who was still waited
    /// for has missed it (three in a row tells the host).
    fn commit_if_ready(&mut self, now_us: i64) {
        let Some(p) = &self.prep else {
            return;
        };
        if !(p.waiting.is_empty() || now_us - p.since_us >= READY_WAIT_US) {
            return;
        }
        let p = self.prep.take().expect("a prepare");
        for id in &p.waiting {
            if let Some(m) = self.people.iter_mut().find(|m| m.id == *id) {
                m.missed += 1;
                if m.missed >= SLOW_AFTER && !m.told_slow {
                    m.told_slow = true;
                    self.out.notes.push(HostNote::Slow { id: *id });
                }
            }
        }
        let anchor = Anchor {
            entry: p.entry,
            content_us: p.content_us,
            host_us: now_us + self.start_lead(),
            playing: true,
            seq: self.new_seq(),
        };
        self.set_playing(anchor);
    }

    /// The armed next's boundary has come: each player switched by itself; the commit says so.
    fn boundary(&mut self, now_us: i64) {
        let (Play::Playing(_), Some(ar)) = (self.play, self.armed) else {
            return;
        };
        if now_us < ar.next.boundary_host_us {
            return;
        }
        self.armed = None;
        if self.index_of(ar.next.entry).is_none() {
            return;
        }
        let anchor = Anchor {
            entry: ar.next.entry,
            content_us: 0,
            host_us: ar.next.boundary_host_us,
            playing: true,
            seq: self.new_seq(),
        };
        self.set_playing(anchor);
    }

    /// The next to arm (20 s before its boundary), or none.
    fn wanted_next(&self, a: &Anchor, now_us: i64) -> Option<(EntryId, i64, Transition)> {
        let len = self.length(a.entry)?;
        let f = self.entry_after(a.entry)?;
        let t = self.transition_between(a.entry, f);
        let b = tl::boundary_us(a, len, t)?;
        (now_us >= b - tl::ARM_AHEAD_US).then_some((f, b, t))
    }

    /// Arm the next, arm another, or withdraw it, as the anchor, the queue and the lengths say;
    /// within 3 s of the boundary a change leaves the armed next for this boundary.
    fn update_next(&mut self, now_us: i64) {
        let Play::Playing(a) = self.play else {
            if self.armed.is_some() {
                self.disarm();
            }
            return;
        };
        if self.ending.is_some() {
            return;
        }
        let want = self.wanted_next(&a, now_us);
        if let Some(ar) = self.armed {
            let same = want.is_some_and(|(e, b, t)| {
                ar.next.entry == e && ar.next.boundary_host_us == b && ar.next.transition == t
            });
            if same || self.frozen(now_us).is_some() {
                return;
            }
        }
        match want {
            Some((entry, boundary_host_us, transition)) => {
                let prep = self.new_prep_id();
                self.ask_again(entry);
                let next = Next {
                    entry,
                    boundary_host_us,
                    transition,
                    seq: self.new_seq(),
                };
                self.armed = Some(Armed {
                    prep,
                    next,
                    anchor_seq: a.seq,
                });
                self.view_dirty = true;
                self.send_all(ToGuest::ArmNext { prep, next });
            }
            None if self.armed.is_some() => self.disarm(),
            None => {}
        }
    }

    fn disarm(&mut self) {
        self.armed = None;
        let seq = self.new_seq();
        self.view_dirty = true;
        self.send_all(ToGuest::Disarm { seq });
    }

    /// The last entry: its end said a pause lead ahead (a `Hold` at the end), then the room
    /// rests; anything added meanwhile starts from silence.
    fn end_of_queue(&mut self, now_us: i64) {
        let Play::Playing(a) = self.play else {
            return;
        };
        if self.armed.is_some() || (self.ending.is_none() && self.entry_after(a.entry).is_some()) {
            return;
        }
        let Some(end) = self.length(a.entry).and_then(|len| tl::end_us(&a, len)) else {
            return;
        };
        if self.ending.is_none() && now_us >= end - self.pause_lead() {
            let anchor = Anchor {
                entry: a.entry,
                content_us: end - a.host_us + a.content_us,
                host_us: end,
                playing: false,
                seq: self.new_seq(),
            };
            self.ending = Some(anchor);
            self.send_all(ToGuest::Hold { anchor });
        }
        if now_us >= end {
            self.play = Play::Resting {
                after: Some(a.entry),
            };
            self.ending = None;
            self.quiet_since = Some(now_us);
            self.view_dirty = true;
            if let Some(e) = self.entry_after(a.entry) {
                self.start_prep(e, 0, Purpose::Start, now_us);
            }
        }
    }

    /// Every playing member suspended for 3 s, the host included: the room holds itself
    /// (D11). Never for one person while others play.
    fn auto_hold(&mut self, now_us: i64) {
        let Play::Playing(a) = self.play else {
            return;
        };
        if self.prep.is_some() {
            return;
        }
        let set: Vec<&Person> = self
            .people
            .iter()
            .filter(|p| p.plays() && !p.stepped_out() && !p.cannot(a.entry))
            .collect();
        let since = set
            .iter()
            .map(|p| p.suspended_since)
            .collect::<Option<Vec<_>>>();
        let Some(latest) = since.and_then(|s| s.into_iter().max()) else {
            return;
        };
        if now_us - latest >= AUTO_HOLD_US {
            let anchor = Anchor {
                entry: a.entry,
                content_us: a.content_at(now_us),
                host_us: now_us,
                playing: false,
                seq: self.new_seq(),
            };
            self.hold_at(anchor, true, now_us);
        }
    }

    /// Nobody who plays can play the entry, for 5 s: skipped.
    fn auto_skip(&mut self, now_us: i64) {
        let Play::Playing(a) = self.play else {
            self.unable_since = None;
            return;
        };
        let players: Vec<&Person> = self.people.iter().filter(|p| p.plays()).collect();
        let nobody = !players.is_empty() && players.iter().all(|p| p.cannot(a.entry));
        if !nobody || self.prep.is_some() {
            self.unable_since = None;
            return;
        }
        let since = match self.unable_since {
            Some((e, t)) if e == a.entry => t,
            _ => {
                self.unable_since = Some((a.entry, now_us));
                now_us
            }
        };
        if now_us - since < AUTO_SKIP_US {
            return;
        }
        self.unable_since = None;
        let what = format!("skipped “{}”: no one could play it", self.title(a.entry));
        self.note(HOST, &what, false, now_us);
        match self.entry_after(a.entry) {
            Some(next) => self.start_prep(next, 0, Purpose::Skip, now_us),
            None => {
                let anchor = Anchor {
                    content_us: a.content_at(now_us),
                    host_us: now_us,
                    playing: false,
                    seq: self.new_seq(),
                    ..a
                };
                self.send_all(ToGuest::Hold { anchor });
                self.play = Play::Resting {
                    after: Some(a.entry),
                };
                self.quiet_since = Some(now_us);
                self.view_dirty = true;
            }
        }
    }

    fn end_room(&mut self, why: String) {
        self.send_all(ToGuest::Ended { why });
        self.ended = true;
        self.prep = None;
        self.armed = None;
        self.out.ended = true;
    }

    // --- Inside: what to send -----------------------------------------------------------

    fn push(&mut self, to: u32, msg: ToGuest) {
        self.out.out.push(Out { to, msg });
    }

    /// To every member welcomed (the host too), after the views if they changed.
    fn send_all(&mut self, msg: ToGuest) {
        self.flush_views();
        let ids: Vec<u32> = self
            .people
            .iter()
            .filter(|p| p.welcomed)
            .map(|p| p.id)
            .collect();
        for id in ids {
            self.push(id, msg.clone());
        }
    }

    fn flush_views(&mut self) {
        if !self.view_dirty || self.ended {
            return;
        }
        self.view_dirty = false;
        let ids: Vec<u32> = self
            .people
            .iter()
            .filter(|p| p.welcomed)
            .map(|p| p.id)
            .collect();
        for id in ids {
            let room = self.view_for(id);
            self.push(id, ToGuest::Room { room });
        }
    }

    fn take(&mut self) -> Changes {
        self.flush_views();
        std::mem::take(&mut self.out)
    }
}

/// Whether `role` allows what needs `need`.
fn allows(role: Role, need: Role) -> bool {
    match need {
        Role::Control => role == Role::Control,
        Role::Add => matches!(role, Role::Control | Role::Add),
        Role::Listen => true,
    }
}

#[cfg(test)]
mod tests;
