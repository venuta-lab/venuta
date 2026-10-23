//! Whole rooms of sessions on the simulated network, in simulated time: the host's
//! `HostSession` (Room v2 behind its door, its own conductor) and each guest's `GuestSession`
//! (knock, clock, conductor, reconnects), every one on its own `SimTransport` with a clock of
//! its own computer (a `SkewedClock` over the simulation's), and each person's player a
//! `FakeRoomPlayer` that records every call and answers readiness as scripted. Items are song
//! keys; each person's copies answer from rules a test can change (found, none, still looking).
//!
//! The harness only moves time and steps the sessions, as the app's runner does: everything
//! the room does is the sessions' own.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use together::align::{Alignment, CopyClass, Envelope, Track, Verdict, WindowCheck};
use together::anchor::{Anchor, EntryId, PrepId};
use together::clock_map;
use together::conductor::{Conductor, Copies, Copy, Lookup};
use together::fakes::{Call, FakeLog, FakeRoomPlayer, Script};
use together::measure::{Aligner, Envelopes, Made, Measured, References, Shelf};
use together::proto::Song;
use together::proto::v3::Action;
use together::room::v2::{HOST, Opening, Room};
use together::room_player::{ListenMode, RoomPlayer, SyncMode};
use together::session::{GuestSession, HostConfig, HostSession, Stage};
use together::sim_net::{SimNet, SimTransport, Way};
use together::time::{MonoClock, SimClock, SkewedClock, Timebase};
use together::transport::{Get, MediaServer, PeerId};
use together::view::CopyKind;

pub const SECRET: &str = "0123456789abcdef0123456789abcdef";
pub const MS: i64 = 1_000;
pub const S: i64 = 1_000_000;
pub const NS_MS: u64 = 1_000_000;

/// A song of the room's queue: `n` names it, `secs` long.
pub fn song(n: u32, secs: u64) -> Song {
    Song {
        key: format!("online:song{n:07}"),
        title: format!("Song {n}"),
        artists: vec!["Someone".into()],
        album: None,
        duration_ms: Some(secs * 1_000),
        isrc: None,
        mbid: None,
        online_id: Some(format!("song{n:07}")),
        host_file: false,
        format: None,
        artwork_url: None,
        added_by: None,
    }
}

/// `n` songs of 200 s, named 1 to n.
pub fn songs(n: u32) -> Vec<Song> {
    (1..=n).map(|i| song(i, 200)).collect()
}

/// How one person's copies answer, by song key.
#[derive(Default)]
pub struct Rules {
    /// No copy of these.
    pub none: HashSet<String>,
    /// Still looking for these (until taken out).
    pub looking: HashSet<String>,
    /// These are another version, this many ms long.
    pub length_ms: HashMap<String, u64>,
    /// Asked for a lighter copy / the host's copy.
    pub lighter: bool,
    pub host_copy: HashSet<String>,
    /// Every song asked for, in order.
    pub asked: Vec<String>,
    /// How they listen: every device puts the host's file first (D5, as `sources.rs`).
    pub mode: Option<ListenMode>,
}

/// A person's copies: found by the rules (the key is the item; approximate, the catalog's).
pub struct TestCopies(pub Arc<Mutex<Rules>>);

impl Copies<String> for TestCopies {
    fn find(&mut self, song: &Song) -> Lookup<String> {
        let mut r = self.0.lock().unwrap();
        r.asked.push(song.key.clone());
        if r.none.contains(&song.key) {
            return Lookup::NoCopy;
        }
        if r.looking.contains(&song.key) {
            return Lookup::Looking;
        }
        let host = r.host_copy.contains(&song.key)
            || (song.host_file && r.mode == Some(ListenMode::EveryDevice));
        Lookup::Found(Copy {
            item: song.key.clone(),
            align: if host {
                Alignment::EXACT
            } else {
                Alignment::APPROXIMATE
            },
            kind: if host {
                CopyKind::HostStream
            } else {
                CopyKind::Catalog
            },
            length_ms: r.length_ms.get(&song.key).copied(),
        })
    }
    fn lighter(&mut self) {
        self.0.lock().unwrap().lighter = true;
    }
    fn prefer_host(&mut self, key: &str) {
        self.0.lock().unwrap().host_copy.insert(key.to_owned());
    }
    fn set_mode(&mut self, mode: ListenMode) {
        self.0.lock().unwrap().mode = Some(mode);
    }
}

pub enum Side {
    Host(Box<HostSession<String>>),
    Guest(Box<GuestSession<String>>),
}

/// One person.
pub struct Person {
    pub name: String,
    pub t: SimTransport,
    pub clock: Arc<SkewedClock>,
    pub log: FakeLog<String>,
    pub rules: Arc<Mutex<Rules>>,
    /// What each of their copies truly is (measured worlds).
    pub truths: Truths,
    pub side: Option<Side>,
    pub ways: (Way, Way),
}

impl Person {
    pub fn guest(&mut self) -> &mut GuestSession<String> {
        match self.side.as_mut() {
            Some(Side::Guest(g)) => g,
            _ => panic!("{} is not a guest in a room", self.name),
        }
    }
    pub fn host(&mut self) -> &mut HostSession<String> {
        match self.side.as_mut() {
            Some(Side::Host(h)) => h,
            _ => panic!("{} is not the host", self.name),
        }
    }
    pub fn calls(&self) -> Vec<Call<String>> {
        self.log.just_calls()
    }
    /// The calls made at or after `t_ns` (the fake's clock is the simulation's).
    pub fn calls_since(&self, t_ns: u64) -> Vec<Call<String>> {
        self.log
            .calls()
            .into_iter()
            .filter(|(at, _)| *at >= t_ns)
            .map(|(_, c)| c)
            .collect()
    }
    pub fn commits(&self) -> Vec<Anchor> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Commit(a) => Some(a),
                _ => None,
            })
            .collect()
    }
    pub fn holds(&self) -> Vec<Anchor> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Hold(a) => Some(a),
                _ => None,
            })
            .collect()
    }
    /// Prepares: (prep, entry, room content µs).
    pub fn prepares(&self) -> Vec<(PrepId, EntryId, i64)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Prepare {
                    prep,
                    entry,
                    room_content_us,
                    ..
                } => Some((prep, entry, room_content_us)),
                _ => None,
            })
            .collect()
    }
    pub fn arms(&self) -> Vec<(EntryId, i64)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::ArmNext {
                    entry,
                    boundary_host_us,
                    ..
                } => Some((entry, boundary_host_us)),
                _ => None,
            })
            .collect()
    }
    pub fn detached(&self) -> bool {
        self.calls().last() == Some(&Call::Detach)
    }
}

/// A whole room.
pub struct World {
    pub clock: SimClock,
    pub net: Arc<SimNet>,
    pub people: Vec<Person>,
    base: Arc<dyn MonoClock>,
    step_ns: u64,
    pub script: Script,
    pub invite_open: Arc<std::sync::atomic::AtomicBool>,
    /// People whose computers sleep: their sessions aren't stepped.
    asleep: HashSet<usize>,
    /// Copies measured (WP11).
    pub measured: bool,
}

/// Every person's computer clock: booted at its own moment, a little fast or slow.
fn computer(base: &Arc<dyn MonoClock>, i: usize) -> Arc<SkewedClock> {
    Arc::new(SkewedClock::new(
        base.clone(),
        (500_000 + 977 * i as u64) * 1_000_000_000,
        30.0 - 13.0 * i as f64,
    ))
}

impl World {
    /// The host Asha alone, its room opened as `opening` with `songs` (let in without asking).
    pub fn open(songs: Vec<Song>, opening: Opening) -> World {
        Self::open_with(songs, opening, Script::default())
    }

    pub fn open_with(songs: Vec<Song>, opening: Opening, script: Script) -> World {
        Self::build(songs, opening, script, false)
    }

    /// As `open`, everyone's copies measured (WP11): the host shelves its references, each
    /// guest fetches them and aligns its own (fake envelopes that carry each copy's `Truth`).
    pub fn open_measured(songs: Vec<Song>, opening: Opening) -> World {
        Self::build(songs, opening, Script::default(), true)
    }

    fn build(songs: Vec<Song>, opening: Opening, script: Script, measured: bool) -> World {
        let clock = SimClock::new(1_000_000_000);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb, 23);
        let base: Arc<dyn MonoClock> = Arc::new(clock.clone());
        let mut w = World {
            clock,
            net,
            people: Vec::new(),
            base,
            step_ns: 5 * NS_MS,
            script,
            invite_open: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            asleep: HashSet::new(),
            measured,
        };
        let hc = computer(&w.base, 0);
        let t = w.net.peer(hc.clone());
        let (player, log) = FakeRoomPlayer::<String>::new(w.base.clone(), w.script.clone());
        let player: Arc<dyn RoomPlayer<String>> = Arc::new(player);
        let rules = Arc::new(Mutex::new(Rules::default()));
        let map = clock_map::handle();
        map.store(Arc::new(Some(together::clock_map::ClockMap {
            epoch: 0,
            local_ref_ns: 0,
            host_ref_us: 0,
            rate: 1.0,
            bound_us: 0,
        })));
        let now_us = (hc.now_ns() / 1_000) as i64;
        let opening = match opening {
            Opening::Playing {
                index, content_us, ..
            } => Opening::Playing {
                index,
                content_us,
                host_us: now_us,
            },
            o => o,
        };
        let mut room = Room::new("Asha", SECRET, songs, opening);
        room.set_let_in_without_asking(true);
        let playing = match opening {
            Opening::Playing { index, .. } => Some(room.queue()[index].id),
            _ => None,
        };
        let handle = match playing {
            Some(e) => player.adopt(map.clone(), SyncMode::Apart, e),
            None => player.attach(map.clone(), SyncMode::Apart),
        };
        let truths = Truths::default();
        let shelf = Shelf::new();
        let copies: Box<dyn Copies<String>> = if measured {
            Box::new(Measured::host(
                Box::new(TestCopies(rules.clone())),
                Box::new(FakeEnvelopes::new(truths.clone(), w.base.clone())),
                shelf.clone(),
            ))
        } else {
            Box::new(TestCopies(rules.clone()))
        };
        let conductor =
            Conductor::new(player, map, copies, ListenMode::Apart).with_handle(handle, playing);
        let open = w.invite_open.clone();
        let mut host = HostSession::new(
            Arc::new(t.clone()),
            hc.clone(),
            room,
            conductor,
            HostConfig {
                invite_open: Arc::new(move || open.load(std::sync::atomic::Ordering::SeqCst)),
            },
        );
        if measured {
            host = host.with_envelopes(shelf);
            let gate = host.media(Arc::new(NoFiles));
            w.net.serve(&t, gate);
        }
        w.people.push(Person {
            name: "Asha".into(),
            t,
            clock: hc,
            log,
            rules,
            truths,
            side: Some(Side::Host(Box::new(host))),
            ways: (Way::LAN, Way::LAN),
        });
        w.run_ms(10);
        w
    }

    /// A room on one Wi-Fi with these guests in it, playing from the first song.
    pub fn lan(names: &[&str]) -> World {
        let mut w = World::open(songs(5), Opening::Resting);
        for n in names {
            w.join(n, Way::LAN, Way::LAN);
        }
        w
    }

    /// A guest with these ways (to the host, from it) joins now, protocol 3; runs until in.
    pub fn join(&mut self, name: &str, up: Way, down: Way) -> usize {
        let i = self.arrive(name, up, down, ListenMode::Apart);
        assert!(
            self.run_until(|w| w.people[i].guest().stage() == Stage::In, 5_000),
            "{name} in"
        );
        i
    }

    /// A guest who chose `mode` joins now over these ways; runs until in.
    pub fn join_as(&mut self, name: &str, up: Way, down: Way, mode: ListenMode) -> usize {
        let i = self.arrive(name, up, down, mode);
        assert!(
            self.run_until(|w| w.people[i].guest().stage() == Stage::In, 5_000),
            "{name} in"
        );
        i
    }

    /// A guest connects and knocks (as `mode`), not waited for.
    pub fn arrive(&mut self, name: &str, up: Way, down: Way, mode: ListenMode) -> usize {
        self.arrive_with(name, up, down, mode, Some(mode))
    }

    /// A guest who chose nothing before joining connects and knocks: the network decides (A16).
    pub fn arrive_by_network(&mut self, name: &str, up: Way, down: Way) -> usize {
        self.arrive_with(name, up, down, ListenMode::Apart, None)
    }

    fn arrive_with(
        &mut self,
        name: &str,
        up: Way,
        down: Way,
        mode: ListenMode,
        chosen: Option<ListenMode>,
    ) -> usize {
        let i = self.people.len();
        let gc = computer(&self.base, i);
        let t = self.net.peer(gc.clone());
        let (player, log) = FakeRoomPlayer::<String>::new(self.base.clone(), self.script.clone());
        let rules = Arc::new(Mutex::new(Rules::default()));
        let host = self.people[0].t.id();
        let truths = Truths::default();
        let copies: Box<dyn Copies<String>> = if self.measured {
            Box::new(Measured::guest(
                Box::new(TestCopies(rules.clone())),
                Box::new(FakeEnvelopes::new(truths.clone(), self.base.clone())),
                References::new(Arc::new(t.clone()), host, None),
                Arc::new(FakeAligner),
            ))
        } else {
            Box::new(TestCopies(rules.clone()))
        };
        let conductor = Conductor::new(Arc::new(player), clock_map::handle(), copies, mode);
        let key = format!("peer{}", t.id().0);
        let g = GuestSession::new(
            Arc::new(t.clone()),
            host,
            gc.clone(),
            conductor,
            together::session::GuestConfig {
                name: name.into(),
                proof: together::invite::proof(SECRET, &key),
                app: "Venuta test".into(),
                host_name: "Asha".into(),
                mode: chosen,
            },
        );
        self.net.connect(&t, &self.people[0].t, up, down);
        self.people.push(Person {
            name: name.into(),
            t,
            clock: gc,
            log,
            rules,
            truths,
            side: Some(Side::Guest(Box::new(g))),
            ways: (up, down),
        });
        i
    }

    pub fn host_t(&self) -> SimTransport {
        self.people[0].t.clone()
    }

    /// The host's time now (µs).
    pub fn now(&self) -> i64 {
        (self.people[0].clock.now_ns() / 1_000) as i64
    }

    /// The simulation's time (ns): the fakes' clock.
    pub fn sim_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    pub fn set_step_ms(&mut self, ms: u64) {
        self.step_ns = ms * NS_MS;
    }

    pub fn step(&mut self) {
        let next = self.clock.now_ns() + self.step_ns;
        self.clock.run_until(next);
        for (i, p) in self.people.iter_mut().enumerate() {
            if self.asleep.contains(&i) {
                continue;
            }
            match p.side.as_mut() {
                Some(Side::Host(h)) => h.step(),
                Some(Side::Guest(g)) => g.step(),
                None => {}
            }
        }
    }

    pub fn run_ms(&mut self, ms: u64) {
        let until = self.clock.now_ns() + ms * NS_MS;
        while self.clock.now_ns() < until {
            self.step();
        }
    }

    pub fn run_until(&mut self, done: impl Fn(&mut World) -> bool, max_ms: u64) -> bool {
        let until = self.clock.now_ns() + max_ms * NS_MS;
        while self.clock.now_ns() < until {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    /// Person `i`'s computer sleeps (`true`) or wakes.
    pub fn asleep(&mut self, i: usize, on: bool) {
        if on {
            self.asleep.insert(i);
        } else {
            self.asleep.remove(&i);
        }
    }

    /// Time passes without the sessions being stepped (the computers asleep), `ms`.
    pub fn sleep_all_ms(&mut self, ms: u64) {
        let until = self.clock.now_ns() + ms * NS_MS;
        self.clock.run_until(until);
    }

    pub fn room(&mut self) -> &Room {
        self.people[0].host().room()
    }

    /// The entry id at a place in the queue.
    pub fn entry(&mut self, i: usize) -> EntryId {
        self.room().queue()[i].id
    }

    /// The host presses (its own asks).
    pub fn host_ask(&mut self, a: Action) {
        self.people[0].host().ask(a).expect("the host may");
    }

    /// The room starts from the first song: everyone ready and committed.
    pub fn start(&mut self) -> Anchor {
        self.host_ask(Action::Play);
        assert!(
            self.run_until(
                |w| w.room().anchor().is_some_and(|a| a.playing)
                    && w.people.iter().all(|p| !p.commits().is_empty()),
                4_000
            ),
            "started"
        );
        self.room().anchor().unwrap()
    }

    /// The room starts from the first song with remotes in it (who play nothing): the room and
    /// the host's player.
    pub fn start_with_remotes(&mut self) -> Anchor {
        self.host_ask(Action::Play);
        assert!(
            self.run_until(
                |w| w.room().phase() == together::proto::v3::Phase::Playing
                    && !w.people[0].commits().is_empty(),
                4_000
            ),
            "started"
        );
        self.room().anchor().unwrap()
    }

    pub fn guest_ids(&self) -> Vec<usize> {
        (1..self.people.len()).collect()
    }

    /// Cut a guest's link (nothing either way; both told lost after the idle timeout).
    pub fn cut(&mut self, i: usize) {
        let (a, b) = (self.people[i].t.clone(), self.host_t());
        self.net.cut(&a, &b);
    }

    pub fn restore(&mut self, i: usize) {
        let (a, b) = (self.people[i].t.clone(), self.host_t());
        self.net.restore(&a, &b);
    }

    pub fn set_ways(&mut self, i: usize, up: Way, down: Way) {
        self.people[i].ways = (up, down);
        let (a, b) = (self.people[i].t.clone(), self.host_t());
        self.net.set_way(&a, &b, up, down);
    }

    pub fn peer(&self, i: usize) -> PeerId {
        self.people[i].t.id()
    }

    /// Whether the room still holds a member of this name.
    pub fn has_member(&mut self, name: &str) -> bool {
        let v = self.room().view_for(HOST);
        v.members.iter().any(|m| m.name == name)
    }
}

// --- Measured copies (WP11) ---------------------------------------------------------------

/// What a copy truly is, for the fake envelopes (they carry it, and the fake aligner reads it
/// back): its offset from the reference at the start and at each later window (60 s apart),
/// where another edit begins, whether its decoded audio is the reference's.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Truth {
    pub offsets: Vec<i64>,
    pub edit_ms: Option<u32>,
    pub same_pcm: bool,
    pub fails: bool,
    /// The start's envelope is made this long after first asked for; the whole song's this
    /// long, ms.
    pub start_ms: u64,
    pub whole_ms: u64,
}

impl Default for Truth {
    /// The reference's own audio: the host's copy, or a copy that decodes the same.
    fn default() -> Truth {
        Truth {
            offsets: vec![0; 4],
            edit_ms: None,
            same_pcm: true,
            fails: false,
            start_ms: 0,
            whole_ms: 0,
        }
    }
}

impl Truth {
    /// Another recording of the same song, `offset_us` later than the reference throughout.
    pub fn at(offset_us: i64) -> Truth {
        Truth {
            offsets: vec![offset_us; 4],
            same_pcm: false,
            ..Truth::default()
        }
    }
}

/// Each person's copies' truths, by song key (missing: the reference's own audio).
pub type Truths = Arc<Mutex<HashMap<String, Truth>>>;

/// Envelopes made from the truths: the start's once `start_ms` has passed, the whole song's
/// once `whole_ms` has.
pub struct FakeEnvelopes {
    truths: Truths,
    clock: Arc<dyn MonoClock>,
    since: HashMap<String, u64>,
}

impl FakeEnvelopes {
    pub fn new(truths: Truths, clock: Arc<dyn MonoClock>) -> FakeEnvelopes {
        FakeEnvelopes {
            truths,
            clock,
            since: HashMap::new(),
        }
    }
}

impl Envelopes<String> for FakeEnvelopes {
    fn of(&mut self, key: &str, _item: &String) -> Made {
        let t = self
            .truths
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default();
        let now = self.clock.now_ns();
        let ran_ms = (now - *self.since.entry(key.to_owned()).or_insert(now)) / NS_MS;
        if t.fails {
            return Made::Failed;
        }
        if ran_ms < t.start_ms {
            return Made::Making;
        }
        let pcm_hash = Some(if t.same_pcm { [1; 16] } else { [2; 16] });
        let mut windows = vec![(0, serde_json::to_vec(&t).unwrap())];
        if ran_ms < t.whole_ms {
            return Made::Start(Envelope {
                hop_ms: 10,
                windows,
                pcm_hash,
            });
        }
        for k in 1..t.offsets.len() {
            windows.push((k as u32 * 60_000, vec![k as u8]));
        }
        Made::Whole(Envelope {
            hop_ms: 10,
            windows,
            pcm_hash,
        })
    }
}

/// Aligns the truths the fake envelopes carry, by the real analysis's rules: exact for the same
/// audio with no edit, measured otherwise; later windows agree at each window's offset, or
/// show the edit (moved 5 s) from where it begins.
pub struct FakeAligner;

impl Aligner for FakeAligner {
    fn track(&self, r: &Envelope, m: &Envelope) -> Track {
        let read = |e: &Envelope| -> Truth { serde_json::from_slice(&e.windows[0].1).unwrap() };
        let (rt, mt) = (read(r), read(m));
        let at = |v: &Vec<i64>, k: usize| v.get(k).or(v.first()).copied().unwrap_or(0);
        let off = |k: usize| at(&mt.offsets, k) - at(&rt.offsets, k);
        let mut edit_at = mt.edit_ms.filter(|e| *e < 30_000);
        let mut windows = Vec::new();
        for k in 1..r.windows.len().min(m.windows.len()) {
            let start_ms = k as u32 * 60_000;
            let verdict = if mt.edit_ms.is_some_and(|e| e <= start_ms) {
                edit_at.get_or_insert(start_ms);
                Verdict::Moved {
                    offset_us: off(k) + 5_000_000,
                }
            } else {
                Verdict::Agrees { offset_us: off(k) }
            };
            windows.push(WindowCheck { start_ms, verdict });
        }
        let exact = r.pcm_hash.is_some() && r.pcm_hash == m.pcm_hash;
        let alignment = match (exact, edit_at) {
            (true, None) => Alignment::EXACT,
            (_, Some(_)) => Alignment {
                offset_us: off(0),
                ..Alignment::APPROXIMATE
            },
            (false, None) => Alignment {
                offset_us: off(0),
                class: CopyClass::Measured,
                confidence: 15.0,
            },
        };
        Track {
            alignment,
            windows,
            edit_at_ms: edit_at,
        }
    }
}

/// The host's files: none (a measured world serves only references).
pub struct NoFiles;

impl MediaServer for NoFiles {
    fn open(&self, _: PeerId, _: &Get) -> Option<Arc<[u8]>> {
        None
    }
}

impl Person {
    /// Their copies' alignments as the player was told them: prepares' and arms' (entry,
    /// alignment), and refinements.
    pub fn aligns(&self) -> Vec<(EntryId, Alignment)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Prepare { entry, align, .. }
                | Call::ArmNext { entry, align, .. }
                | Call::Refine { entry, align } => Some((entry, align)),
                _ => None,
            })
            .collect()
    }
    pub fn refines(&self) -> Vec<(u64, EntryId, Alignment)> {
        self.log
            .calls()
            .into_iter()
            .filter_map(|(at, c)| match c {
                Call::Refine { entry, align } => Some((at, entry, align)),
                _ => None,
            })
            .collect()
    }
    /// Set what their copy of `key` truly is.
    pub fn truth(&self, key: &str, t: Truth) {
        self.truths.lock().unwrap().insert(key.to_owned(), t);
    }
}
