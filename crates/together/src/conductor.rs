//! The conductor (design 4.3, its WP9 notes in 9.2): the part of each person's Venuta, the
//! host's too, that turns what the room says into calls on the person's player
//! (`RoomHandle`), and answers the room: ready for a prepare, unable to play an entry, how the
//! player is (`State`), how the person listens (`Mode`).
//!
//! It holds what the room said (the queue, the newest anchor and armed next by sequence
//! number) and what it told the player, finds each entry's copy through `Copies`, catches up
//! when it comes in partway (a joiner, a rejoin after missing the song's start), and lets go of
//! the player when the room ends for this person. Pure: the session gives it each message and
//! steps it with this computer's time; the player answers through `RoomHandle::status`.

use crate::align::Alignment;
use crate::anchor::{Anchor, EntryId, Next, PrepId};
use crate::clock_map::ClockMapHandle;
use crate::proto::Song;
use crate::proto::v3::{Advice, CopyReport, Entry, RoomView, ToGuest, ToHost, UnableWhy};
use crate::room_player::{
    ListenMode, RoomHandle, RoomPlayStatus, RoomPlayer, SubState, Suspension,
};
use crate::timeline::v3 as tl;
use crate::view::{self, CopyKind};
use std::collections::HashMap;
use std::sync::Arc;

/// A catch-up expects to be ready this long after it is asked (plus `tl::CATCH_UP_MARGIN_US`).
pub const CATCH_UP_READY_US: i64 = 700_000;
/// A player that can't play the entry playing is tried again this often, ns.
pub const RETRY_NS: u64 = 5_000_000_000;
/// `State` goes at most this often when the sub-state, entry or copy changed, ns...
pub const STATE_EVERY_NS: u64 = 500_000_000;
/// ... and this often when only the error or the bound moved.
pub const STATE_FIGURES_EVERY_NS: u64 = 2_000_000_000;
/// A mode the person chose is held against the room's views this long, for the room to hear
/// it (a view sent before it arrived still says the old one), ns.
pub const MODE_HOLD_NS: u64 = 3_000_000_000;
/// Copies whose length differs from the song's stated one by more than this are another
/// version (E10), ms.
const OTHER_VERSION_MS: i64 = 2_000;
/// Local prepares (catch-ups) are numbered from here, apart from the room's.
const LOCAL_PREP: u64 = 1 << 62;

/// One copy of a song, to play.
#[derive(Debug, Clone)]
pub struct Copy<I> {
    pub item: I,
    /// Where it is against the room's reference copy.
    pub align: Alignment,
    pub kind: CopyKind,
    /// Its own length, when known (another version shows by it).
    pub length_ms: Option<u64>,
}

/// What looking for a copy came to.
#[derive(Debug, Clone)]
pub enum Lookup<I> {
    Found(Copy<I>),
    /// Still looking (a catalog search on its way): asked again at the next step.
    Looking,
    /// No copy anywhere.
    NoCopy,
}

/// Where this person's copies come from (the app: own files, the catalog, the host's files).
/// Never blocks: a slow look-up says `Looking` and runs on a thread of its own.
pub trait Copies<I>: Send {
    fn find(&mut self, song: &Song) -> Lookup<I>;
    /// From now on, the lightest copy that will do (the host's advice, G7).
    fn lighter(&mut self) {}
    /// This song from the host's own file, streamed (E10: "use Asha's copy").
    fn prefer_host(&mut self, _key: &str) {}
}

/// A prepare the conductor has to answer or has told the player.
#[derive(Debug, Clone, Copy)]
struct Prep {
    id: PrepId,
    entry: EntryId,
    content_us: i64,
    /// A catch-up of the conductor's own: nobody waits for its readiness.
    local: bool,
    told: bool,
    answered: bool,
}

/// An entry to come in on, partway (a catch-up), or held silent (a joiner into a held room).
#[derive(Debug, Clone, Copy)]
struct Want {
    anchor: Anchor,
}

#[derive(Debug, Clone, Copy)]
struct Arm {
    prep: PrepId,
    next: Next,
    /// Asked by the room (its prep answers); else a view's (nobody waits).
    asked: bool,
    told: bool,
    answered: bool,
}

/// The last `State` sent: what it said, the line it made, and when.
#[derive(Debug, Clone)]
struct Said {
    key: (Option<EntryId>, SubState, Option<CopyReport>),
    line: String,
    at_ns: u64,
}

/// One person's conductor.
pub struct Conductor<I> {
    player: Arc<dyn RoomPlayer<I>>,
    clock: ClockMapHandle,
    copies: Box<dyn Copies<I>>,
    handle: Option<Box<dyn RoomHandle<I>>>,
    mode: ListenMode,
    extra_us: i32,
    me: Option<u32>,
    view: Option<RoomView>,
    anchor: Option<Anchor>,
    anchor_seq: u64,
    next_seq: u64,
    prep: Option<Prep>,
    want: Option<Want>,
    arm: Option<Arm>,
    /// The entry the player was last given to play (committed, or made in place).
    given: Option<EntryId>,
    /// Entries with no copy (said to the room; the next one is tried).
    cannot: Option<EntryId>,
    /// What each entry's copy is, as found.
    kinds: HashMap<EntryId, (CopyKind, Option<i32>, Alignment)>,
    next_local: u64,
    stepped_out: bool,
    away: Option<Suspension>,
    start_with_next: bool,
    finishing: bool,
    ended: bool,
    failing_since: Option<u64>,
    mode_asked: Option<(ListenMode, u64)>,
    said: Option<Said>,
    out: Vec<ToHost>,
    /// The length the player says for the entry heard, until the session takes it.
    length: Option<(EntryId, i64)>,
    length_told: Option<(EntryId, i64)>,
    status: Option<RoomPlayStatus>,
    /// This computer's time at the last message or step, ns.
    now_ns: u64,
}

fn song_of(view: Option<&RoomView>, entry: EntryId) -> Option<Song> {
    view?
        .queue
        .iter()
        .find(|e| e.id == entry)
        .map(|e| e.song.clone())
}

impl<I: Clone + Send + 'static> Conductor<I> {
    /// A conductor for a person listening as `mode`, its player `player`, the clock map the
    /// player reads `clock` (the session publishes into it), copies from `copies`. It takes the
    /// player when the room lets the person in.
    pub fn new(
        player: Arc<dyn RoomPlayer<I>>,
        clock: ClockMapHandle,
        copies: Box<dyn Copies<I>>,
        mode: ListenMode,
    ) -> Self {
        Conductor {
            player,
            clock,
            copies,
            handle: None,
            mode,
            extra_us: 0,
            me: None,
            view: None,
            anchor: None,
            anchor_seq: 0,
            next_seq: 0,
            prep: None,
            want: None,
            arm: None,
            given: None,
            cannot: None,
            kinds: HashMap::new(),
            next_local: LOCAL_PREP,
            stepped_out: false,
            away: None,
            start_with_next: false,
            finishing: false,
            ended: false,
            failing_since: None,
            mode_asked: None,
            said: None,
            out: Vec::new(),
            length: None,
            length_told: None,
            status: None,
            now_ns: 0,
        }
    }

    /// The host's: its player taken already (made in place from its own music, A1, or
    /// attached), `playing` the entry it plays on as.
    pub fn with_handle(mut self, handle: Box<dyn RoomHandle<I>>, playing: Option<EntryId>) -> Self {
        if let Some(e) = playing {
            self.kinds
                .insert(e, (CopyKind::OwnFile, None, Alignment::EXACT));
        }
        self.handle = Some(handle);
        self.given = playing;
        self
    }

    /// The clock map the player reads.
    pub fn clock(&self) -> &ClockMapHandle {
        &self.clock
    }

    pub fn me(&self) -> Option<u32> {
        self.me
    }

    pub fn mode(&self) -> ListenMode {
        self.mode
    }

    /// The room as last seen.
    pub fn view(&self) -> Option<&RoomView> {
        self.view.as_ref()
    }

    /// The newest anchor the room gave.
    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor
    }

    pub fn stepped_out(&self) -> bool {
        self.stepped_out
    }

    pub fn starting_with_next(&self) -> bool {
        self.start_with_next
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The player's own word, as last read.
    pub fn status(&self) -> Option<RoomPlayStatus> {
        self.status
    }

    /// The copy heard now, as reported.
    pub fn copy(&self) -> Option<CopyReport> {
        self.report_copy(self.heard_entry())
    }

    /// What to send the room, in order.
    pub fn take_out(&mut self) -> Vec<ToHost> {
        std::mem::take(&mut self.out)
    }

    /// The length the player gives for the entry it plays (once per change): the host's is the
    /// room's reference.
    pub fn take_length(&mut self) -> Option<(EntryId, i64)> {
        let l = self.length?;
        if self.length_told == Some(l) {
            return None;
        }
        self.length_told = Some(l);
        Some(l)
    }

    // --- What the room says ------------------------------------------------------------

    /// A message from the room (checked by the session), at this computer's `now_ns`.
    pub fn message(&mut self, m: &ToGuest, now_ns: u64) {
        self.now_ns = now_ns;
        if self.ended {
            return;
        }
        match m {
            ToGuest::Waiting => {}
            ToGuest::Welcome { you, room } => {
                self.me = Some(*you);
                self.take_player();
                self.room_view(room.clone(), now_ns);
            }
            ToGuest::Room { room } => self.room_view(room.clone(), now_ns),
            ToGuest::Prepare {
                prep,
                entry,
                content_us,
                ..
            } => {
                if self.finishing {
                    return;
                }
                self.want = None;
                self.prep = Some(Prep {
                    id: *prep,
                    entry: *entry,
                    content_us: *content_us,
                    local: false,
                    told: false,
                    answered: false,
                });
                self.try_prepare();
            }
            ToGuest::Commit { anchor } | ToGuest::Hold { anchor } => {
                if anchor.seq > self.anchor_seq {
                    self.take_anchor(*anchor, now_ns);
                }
            }
            ToGuest::ArmNext { prep, next } => {
                if next.seq > self.next_seq && next.seq > self.anchor_seq {
                    self.take_next(*prep, *next, true);
                }
            }
            ToGuest::Disarm { seq } => {
                if *seq > self.next_seq {
                    self.next_seq = *seq;
                    self.arm = None;
                    if let Some(h) = &self.handle {
                        h.disarm();
                    }
                }
            }
            ToGuest::Advise { advice } => match advice {
                Advice::LighterCopy => self.copies.lighter(),
                Advice::StartWithNext => self.start_with_next(),
            },
            ToGuest::Refused { .. } | ToGuest::Ended { .. } => self.end(),
            ToGuest::HandOver { .. } => {}
        }
    }

    /// The room as it is now: the queue, my mode as the room has it, and an anchor or next
    /// newer than what is held (a joiner, a reconnect).
    fn room_view(&mut self, v: RoomView, now_ns: u64) {
        let mine = self
            .me
            .and_then(|me| v.members.iter().find(|m| m.id == me))
            .map(|m| m.mode);
        let anchor = v.anchor;
        let next = v.next;
        self.view = Some(v);
        if let Some(m) = mine {
            match self.mode_asked {
                Some((asked, at)) if asked != m && now_ns < at + MODE_HOLD_NS => {}
                _ => {
                    self.mode_asked = None;
                    if m != self.mode {
                        self.apply_mode(m, now_ns);
                    }
                }
            }
        }
        if let Some(a) = anchor
            && a.seq > self.anchor_seq
        {
            self.take_anchor(a, now_ns);
        }
        match next {
            Some(n) if n.seq > self.next_seq && n.seq > self.anchor_seq => {
                let prep = self.local_prep();
                self.take_next(prep, n, false);
            }
            None if self.arm.is_some_and(|a| a.next.seq < self.anchor_seq) => {
                // The room withdrew it while this person wasn't listening.
                self.arm = None;
                if let Some(h) = &self.handle {
                    h.disarm();
                }
            }
            _ => {}
        }
    }

    fn local_prep(&mut self) -> PrepId {
        self.next_local += 1;
        PrepId(self.next_local)
    }

    /// A new anchor (a commit or a hold).
    fn take_anchor(&mut self, a: Anchor, now_ns: u64) {
        self.anchor_seq = a.seq;
        self.anchor = Some(a);
        if self.cannot.is_some_and(|e| e != a.entry) {
            self.cannot = None;
        }
        self.failing_since = None;
        let Some(h) = self.handle.as_ref() else {
            return;
        };
        if !a.playing {
            let has = self.has(a.entry);
            if !has && self.given.is_none() && self.prep.is_none() {
                // A joiner into a held room: the held place ready, silent (A12).
                self.want = Some(Want { anchor: a });
                self.try_want(now_ns);
            } else {
                h.hold(a);
            }
            return;
        }
        if self.has(a.entry) || self.start_with_next {
            h.commit(a);
            if self.has(a.entry) {
                self.given = Some(a.entry);
            }
            return;
        }
        // Partway into an entry the player hasn't got: caught up where the room will be.
        self.want = Some(Want { anchor: a });
        self.try_want(now_ns);
    }

    /// Whether the player has this entry: prepared for it, armed with it, or playing it.
    fn has(&self, entry: EntryId) -> bool {
        self.prep.is_some_and(|p| p.told && p.entry == entry)
            || self.arm.is_some_and(|a| a.told && a.next.entry == entry)
            || self.given == Some(entry)
            || self.status.and_then(|s| s.entry) == Some(entry)
    }

    fn take_next(&mut self, prep: PrepId, next: Next, asked: bool) {
        self.next_seq = next.seq;
        if self.finishing {
            return;
        }
        self.arm = Some(Arm {
            prep,
            next,
            asked,
            told: false,
            answered: false,
        });
        self.try_arm();
    }

    fn find(&mut self, entry: EntryId) -> Lookup<I> {
        let Some(song) = song_of(self.view.as_ref(), entry) else {
            return Lookup::NoCopy;
        };
        let l = self.copies.find(&song);
        if let Lookup::Found(c) = &l {
            let diff = match (c.length_ms, song.duration_ms) {
                (Some(mine), Some(stated)) => {
                    let d = mine as i64 - stated as i64;
                    (d.abs() > OTHER_VERSION_MS).then(|| ((d as f64) / 1000.0).round() as i32)
                }
                _ => None,
            };
            self.kinds.insert(entry, (c.kind, diff, c.align));
        }
        l
    }

    fn try_prepare(&mut self) {
        let Some(p) = self.prep.filter(|p| !p.told && !p.answered) else {
            return;
        };
        if self.handle.is_none() {
            return;
        }
        match self.find(p.entry) {
            Lookup::Found(c) => {
                if let Some(h) = &self.handle {
                    h.prepare(p.id, p.entry, c.item, p.content_us, c.align);
                }
                if let Some(q) = self.prep.as_mut() {
                    q.told = true;
                }
            }
            Lookup::Looking => {}
            Lookup::NoCopy => {
                if !p.local {
                    self.out.push(ToHost::Unable {
                        prep: p.id,
                        entry: p.entry,
                        why: UnableWhy::NoCopy,
                    });
                }
                self.cannot = Some(p.entry);
                if let Some(q) = self.prep.as_mut() {
                    q.answered = true;
                }
            }
        }
    }

    fn try_arm(&mut self) {
        let Some(a) = self.arm.filter(|a| !a.told && !a.answered) else {
            return;
        };
        if self.handle.is_none() {
            return;
        }
        match self.find(a.next.entry) {
            Lookup::Found(c) => {
                if let Some(h) = &self.handle {
                    h.arm_next(
                        a.next.entry,
                        c.item,
                        a.next.boundary_host_us,
                        a.next.transition,
                        c.align,
                    );
                }
                if let Some(x) = self.arm.as_mut() {
                    x.told = true;
                }
            }
            Lookup::Looking => {}
            Lookup::NoCopy => {
                if a.asked {
                    self.out.push(ToHost::Unable {
                        prep: a.prep,
                        entry: a.next.entry,
                        why: UnableWhy::NoCopy,
                    });
                }
                if let Some(x) = self.arm.as_mut() {
                    x.answered = true;
                }
            }
        }
    }

    /// A catch-up (or a held place for a joiner), once the copy and the clock are there.
    fn try_want(&mut self, now_ns: u64) {
        let Some(w) = self.want else {
            return;
        };
        let a = w.anchor;
        if self.handle.is_none() {
            return;
        }
        let content = if a.playing {
            let Some(map) = **self.clock.load() else {
                return;
            };
            let host_now = map.host_of(now_ns);
            tl::catch_up(&a, host_now, CATCH_UP_READY_US).0
        } else {
            a.content_us
        };
        match self.find(a.entry) {
            Lookup::Found(c) => {
                let id = self.local_prep();
                if let Some(h) = &self.handle {
                    h.prepare(id, a.entry, c.item, content, c.align);
                    if a.playing {
                        h.commit(a);
                    } else {
                        h.hold(a);
                    }
                }
                self.prep = Some(Prep {
                    id,
                    entry: a.entry,
                    content_us: content,
                    local: true,
                    told: true,
                    answered: true,
                });
                if a.playing {
                    self.given = Some(a.entry);
                }
                self.want = None;
            }
            Lookup::Looking => {}
            Lookup::NoCopy => {
                self.cannot = Some(a.entry);
                if let Some(h) = &self.handle {
                    // The anchor is kept: the armed next starts by itself at its boundary.
                    if a.playing {
                        h.commit(a);
                    } else {
                        h.hold(a);
                    }
                }
                self.want = None;
            }
        }
    }

    // --- Time ----------------------------------------------------------------------------

    /// Read the player, answer the room, try again what was waiting (a copy, the clock).
    pub fn step(&mut self, now_ns: u64) {
        self.now_ns = now_ns;
        if self.ended {
            return;
        }
        self.status = self.handle.as_ref().map(|h| h.status());
        self.try_prepare();
        self.try_arm();
        self.try_want(now_ns);
        let Some(st) = self.status else {
            self.report(now_ns);
            return;
        };
        if let (Some(e), Some(l)) = (st.entry, st.length_us) {
            self.length = Some((e, l));
        }
        if let Some(e) = st.entry
            && self.given.is_some_and(|g| g != e)
            && self.arm.is_some_and(|a| a.next.entry == e)
        {
            // The armed next took over at its boundary.
            self.given = Some(e);
            self.arm = None;
        }
        // Readiness.
        if let Some(p) = self.prep.filter(|p| p.told && !p.answered) {
            if st.ready == Some(p.id) {
                self.out.push(ToHost::Ready {
                    prep: p.id,
                    entry: p.entry,
                });
                if let Some(q) = self.prep.as_mut() {
                    q.answered = true;
                }
            } else if st.sub_state == SubState::CannotPlay {
                self.out.push(ToHost::Unable {
                    prep: p.id,
                    entry: p.entry,
                    why: UnableWhy::Failed,
                });
                if let Some(q) = self.prep.as_mut() {
                    q.answered = true;
                }
            }
        }
        if let Some(a) = self.arm.filter(|a| a.told && !a.answered)
            && st.next_ready == Some(a.next.entry)
        {
            if a.asked {
                self.out.push(ToHost::Ready {
                    prep: a.prep,
                    entry: a.next.entry,
                });
            }
            if let Some(x) = self.arm.as_mut() {
                x.answered = true;
            }
        }
        self.retry(st, now_ns);
        self.finish_step(st, now_ns);
        self.report(now_ns);
    }

    /// A player that can't play the entry playing: looked for again every 5 s.
    fn retry(&mut self, st: RoomPlayStatus, now_ns: u64) {
        let Some(a) = self.anchor.filter(|a| a.playing) else {
            self.failing_since = None;
            return;
        };
        let failing = st.sub_state == SubState::CannotPlay || self.cannot == Some(a.entry);
        if !failing || self.stepped_out || self.away.is_some() || self.start_with_next {
            self.failing_since = None;
            return;
        }
        let since = *self.failing_since.get_or_insert(now_ns);
        if now_ns >= since + RETRY_NS {
            self.failing_since = Some(now_ns);
            self.cannot = None;
            self.prep = None;
            self.given = None;
            self.want = Some(Want { anchor: a });
            self.try_want(now_ns);
        }
    }

    /// After the 2 minutes: once the song playing is over, let go.
    fn finish_step(&mut self, st: RoomPlayStatus, now_ns: u64) {
        if !self.finishing {
            return;
        }
        let over = match self.anchor {
            Some(a) if a.playing => {
                let map = **self.clock.load();
                let length = song_of(self.view.as_ref(), a.entry)
                    .and_then(|s| s.duration_ms)
                    .map(|ms| ms as i64 * 1_000);
                let length = st.length_us.or(length);
                let past = match (map, length) {
                    (Some(m), Some(len)) => a.content_at(m.host_of(now_ns)) >= len,
                    _ => false,
                };
                st.entry.is_none() || past
            }
            _ => true,
        };
        if over {
            self.end();
        }
    }

    fn heard_entry(&self) -> Option<EntryId> {
        self.status
            .and_then(|s| s.entry)
            .or(self.anchor.map(|a| a.entry))
    }

    fn report_copy(&self, entry: Option<EntryId>) -> Option<CopyReport> {
        let e = entry?;
        let (kind, diff, align) = self.kinds.get(&e)?;
        let class = self
            .status
            .filter(|s| s.entry == Some(e))
            .and_then(|s| s.copy)
            .unwrap_or(align.class);
        Some(CopyReport {
            kind: *kind,
            class,
            length_diff_s: *diff,
        })
    }

    /// My sub-state, as the room should hear it.
    fn sub_state(&self) -> Option<SubState> {
        if self.mode == ListenMode::OneSpeaker {
            return Some(SubState::Remote);
        }
        let st = self.status?;
        if self
            .cannot
            .is_some_and(|e| Some(e) == self.anchor.map(|a| a.entry))
            && st.entry.is_none()
            && !self.stepped_out
            && self.away.is_none()
        {
            return Some(SubState::CannotPlay);
        }
        Some(st.sub_state)
    }

    /// `State` on a change, as often as the rules let it (see the constants).
    fn report(&mut self, now_ns: u64) {
        let Some(sub) = self.sub_state() else {
            return;
        };
        if self.me.is_none() {
            return;
        }
        let entry = self.heard_entry();
        let copy = if sub == SubState::Remote {
            None
        } else {
            self.report_copy(entry)
        };
        let (error_us, bound_us) = match self.status {
            Some(s) if sub != SubState::Remote => (s.error_us, s.bound_us),
            _ => (None, None),
        };
        let line = view::line(
            Some(sub),
            self.mode,
            false,
            error_us.map(|e| e as f32 / 1000.0),
            bound_us.map(|b| b as f32 / 1000.0),
            "",
        );
        let key = (entry, sub, copy);
        let due = match &self.said {
            None => true,
            Some(s) if s.key != key => now_ns >= s.at_ns + STATE_EVERY_NS,
            Some(s) => s.line != line && now_ns >= s.at_ns + STATE_FIGURES_EVERY_NS,
        };
        if !due {
            return;
        }
        self.out.push(ToHost::State {
            entry,
            sub_state: sub,
            error_us,
            bound_us,
            copy,
        });
        self.said = Some(Said {
            key,
            line,
            at_ns: now_ns,
        });
    }

    // --- The person's own choices ------------------------------------------------------

    /// How this person listens (their choice: the room hears it, and may say otherwise).
    pub fn set_mode(&mut self, mode: ListenMode) {
        let now_ns = self.now_ns;
        self.out.push(ToHost::Mode { mode });
        self.mode_asked = Some((mode, now_ns));
        if mode != self.mode {
            self.apply_mode(mode, now_ns);
        }
    }

    fn apply_mode(&mut self, mode: ListenMode, now_ns: u64) {
        let was = self.mode;
        self.mode = mode;
        match mode.sync() {
            None => {
                // A remote plays nothing: the player is let go (a 1 s fade of the audio only).
                if let Some(h) = self.handle.take() {
                    h.detach();
                }
                self.prep = None;
                self.arm = None;
                self.want = None;
                self.given = None;
            }
            Some(sync) => {
                if was == ListenMode::OneSpeaker || self.handle.is_none() {
                    self.take_player();
                    // Into the room's music: a late join.
                    if let Some(a) = self.anchor {
                        self.anchor_seq = a.seq.saturating_sub(1);
                        self.take_anchor(a, now_ns);
                    }
                } else if let Some(h) = &self.handle {
                    h.set_mode(sync);
                }
            }
        }
    }

    /// The room takes the player (not for a remote).
    fn take_player(&mut self) {
        if self.handle.is_some() {
            return;
        }
        if let Some(sync) = self.mode.sync() {
            let h = self.player.attach(self.clock.clone(), sync);
            if self.extra_us != 0 {
                h.set_extra_latency(self.extra_us);
            }
            self.handle = Some(h);
        }
    }

    /// Pause just for me (B16).
    pub fn step_out(&mut self) {
        self.stepped_out = true;
        if let Some(h) = &self.handle {
            h.step_out();
        }
    }

    /// Back in at the room's place.
    pub fn rejoin(&mut self) {
        self.stepped_out = false;
        self.start_with_next = false;
        let Some(h) = &self.handle else {
            return;
        };
        h.rejoin();
        // The room moved to an entry the player hasn't got meanwhile: caught up on it.
        if let Some(a) = self.anchor.filter(|a| a.playing && !self.has(a.entry)) {
            self.want = Some(Want { anchor: a });
        }
    }

    /// Don't catch up: start cleanly with the next song (A17, or the host's advice).
    pub fn start_with_next(&mut self) {
        self.start_with_next = true;
        self.want = None;
    }

    /// This output's extra delay (Bluetooth, a receiver), µs.
    pub fn set_extra_latency(&mut self, us: i32) {
        self.extra_us = us;
        if let Some(h) = &self.handle {
            h.set_extra_latency(us);
        }
    }

    /// Faded out for the platform's reason (a sleep, audio focus).
    pub fn suspend(&mut self, why: Suspension) {
        self.away = Some(why);
        if let Some(h) = &self.handle {
            h.suspend(why);
        }
    }

    /// The reason is over: back in at the room's place.
    pub fn resume(&mut self) {
        if self.away.take().is_none() {
            return;
        }
        if let Some(h) = &self.handle {
            h.resume();
        }
    }

    pub fn suspended(&self) -> Option<Suspension> {
        self.away
    }

    /// Use the host's copy of the song playing (E10).
    pub fn use_host_copy(&mut self) {
        let Some(a) = self.anchor.filter(|a| a.playing) else {
            return;
        };
        if let Some(s) = song_of(self.view.as_ref(), a.entry) {
            self.copies.prefer_host(&s.key);
            // Into the same song at the room's place again, on the host's copy: a crossfade.
            self.prep = None;
            self.given = None;
            self.want = Some(Want { anchor: a });
        }
    }

    /// Play a lighter copy from now on.
    pub fn lighter(&mut self) {
        self.copies.lighter();
    }

    /// Lost for good (the 2 minutes): the song playing finishes, nothing new starts, then the
    /// player is let go.
    pub fn finish_at_song_end(&mut self) {
        if self.finishing || self.ended {
            return;
        }
        self.finishing = true;
        self.arm = None;
        self.prep = None;
        self.want = None;
        if let Some(h) = &self.handle {
            h.disarm();
        }
    }

    pub fn finishing(&self) -> bool {
        self.finishing
    }

    /// The room is over for this person: the player let go now (a 1 s fade, your own music
    /// back paused).
    pub fn end(&mut self) {
        if let Some(h) = self.handle.take() {
            h.detach();
        }
        self.ended = true;
    }

    /// The queue as last seen.
    pub fn queue(&self) -> &[Entry] {
        self.view.as_ref().map_or(&[], |v| &v.queue)
    }
}
