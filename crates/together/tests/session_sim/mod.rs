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
use together::align::Alignment;
use together::anchor::{Anchor, EntryId, PrepId};
use together::clock_map;
use together::conductor::{Conductor, Copies, Copy, Lookup};
use together::fakes::{Call, FakeLog, FakeRoomPlayer, Script};
use together::proto::Song;
use together::proto::v3::Action;
use together::room::v2::{HOST, Opening, Room};
use together::room_player::{ListenMode, RoomPlayer, SyncMode};
use together::session::{GuestSession, HostConfig, HostSession, Stage};
use together::sim_net::{SimNet, SimTransport, Way};
use together::time::{MonoClock, SimClock, SkewedClock, Timebase};
use together::transport::PeerId;
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
        let host = r.host_copy.contains(&song.key);
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
        let conductor = Conductor::new(
            player,
            map,
            Box::new(TestCopies(rules.clone())),
            ListenMode::Apart,
        )
        .with_handle(handle, playing);
        let open = w.invite_open.clone();
        let host = HostSession::new(
            Arc::new(t.clone()),
            hc.clone(),
            room,
            conductor,
            HostConfig {
                invite_open: Arc::new(move || open.load(std::sync::atomic::Ordering::SeqCst)),
            },
        );
        w.people.push(Person {
            name: "Asha".into(),
            t,
            clock: hc,
            log,
            rules,
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

    /// A guest connects and knocks (as `mode`), not waited for.
    pub fn arrive(&mut self, name: &str, up: Way, down: Way, mode: ListenMode) -> usize {
        let i = self.people.len();
        let gc = computer(&self.base, i);
        let t = self.net.peer(gc.clone());
        let (player, log) = FakeRoomPlayer::<String>::new(self.base.clone(), self.script.clone());
        let rules = Arc::new(Mutex::new(Rules::default()));
        let conductor = Conductor::new(
            Arc::new(player),
            clock_map::handle(),
            Box::new(TestCopies(rules.clone())),
            mode,
        );
        let host = self.people[0].t.id();
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
            },
        );
        self.net.connect(&t, &self.people[0].t, up, down);
        self.people.push(Person {
            name: name.into(),
            t,
            clock: gc,
            log,
            rules,
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

    /// The member id of the person of this name, as the host's room has them.
    pub fn member_id(&mut self, name: &str) -> Option<u32> {
        let v = self.room().view_for(HOST);
        v.members.iter().find(|m| m.name == name).map(|m| m.id)
    }

    /// Seconds since 1970 for the records, from the simulation's clock.
    pub fn unix(&self) -> u64 {
        1_800_000_000 + self.clock.now_ns() / 1_000_000_000
    }

    /// The host's app goes (a quit, a crash or a kill: nobody is told): every guest's link
    /// carries nothing more (each side told lost after the idle timeout), the session and its
    /// player gone. The guests play on.
    pub fn host_goes(&mut self) {
        for g in self.guest_ids() {
            self.cut(g);
        }
        self.people[0].side = None;
    }

    /// The host's app started again and the room reopened from `state` (`Room::reopen`): a new
    /// player (the app's own, fresh), the same transport (the same endpoint key), the links
    /// carrying again (the guests' reconnects reaching it).
    pub fn host_reopens(&mut self, state: together::room::v2::RoomState) {
        let hc = self.people[0].clock.clone();
        let (player, log) = FakeRoomPlayer::<String>::new(self.base.clone(), self.script.clone());
        let player: Arc<dyn RoomPlayer<String>> = Arc::new(player);
        let map = clock_map::handle();
        map.store(Arc::new(Some(together::clock_map::ClockMap::identity())));
        let now_us = (hc.now_ns() / 1_000) as i64;
        let room = Room::reopen(state, now_us);
        let handle = player.attach(map.clone(), SyncMode::Apart);
        let rules = Arc::new(Mutex::new(Rules::default()));
        let conductor = Conductor::new(
            player,
            map,
            Box::new(TestCopies(rules.clone())),
            ListenMode::Apart,
        )
        .with_handle(handle, None);
        let open = self.invite_open.clone();
        let t = self.people[0].t.clone();
        let host = HostSession::new(
            Arc::new(t),
            hc,
            room,
            conductor,
            HostConfig {
                invite_open: Arc::new(move || open.load(std::sync::atomic::Ordering::SeqCst)),
            },
        );
        let p = &mut self.people[0];
        p.log = log;
        p.rules = rules;
        p.side = Some(Side::Host(Box::new(host)));
        for g in self.guest_ids() {
            self.restore(g);
        }
    }

    /// A guest's app goes (a crash: no goodbye): its link carries nothing more, its session
    /// and player gone.
    pub fn guest_goes(&mut self, i: usize) {
        self.cut(i);
        self.people[i].side = None;
    }

    /// A guest's app started again and joins the same room with the same key (the same
    /// computer's key in this room): a new player, the same transport, the link back.
    pub fn guest_comes_back(&mut self, i: usize) {
        let gc = self.people[i].clock.clone();
        let (player, log) = FakeRoomPlayer::<String>::new(self.base.clone(), self.script.clone());
        let rules = Arc::new(Mutex::new(Rules::default()));
        let conductor = Conductor::new(
            Arc::new(player),
            clock_map::handle(),
            Box::new(TestCopies(rules.clone())),
            ListenMode::Apart,
        );
        let t = self.people[i].t.clone();
        let key = format!("peer{}", t.id().0);
        let g = GuestSession::new(
            Arc::new(t),
            self.people[0].t.id(),
            gc,
            conductor,
            together::session::GuestConfig {
                name: self.people[i].name.clone(),
                proof: together::invite::proof(SECRET, &key),
                app: "Venuta test".into(),
                host_name: "Asha".into(),
            },
        );
        let p = &mut self.people[i];
        p.log = log;
        p.rules = rules;
        p.side = Some(Side::Guest(Box::new(g)));
        self.restore(i);
    }

    /// A person nearby (no invite: the beacon's door code as the secret) connects and knocks.
    pub fn arrive_nearby(&mut self, name: &str) -> usize {
        let i = self.people.len();
        let gc = computer(&self.base, i);
        let t = self.net.peer(gc.clone());
        let (player, log) = FakeRoomPlayer::<String>::new(self.base.clone(), self.script.clone());
        let rules = Arc::new(Mutex::new(Rules::default()));
        let conductor = Conductor::new(
            Arc::new(player),
            clock_map::handle(),
            Box::new(TestCopies(rules.clone())),
            ListenMode::Apart,
        );
        let host = self.people[0].t.id();
        let key = format!("peer{}", t.id().0);
        let door = together::invite::nearby_secret(SECRET);
        let g = GuestSession::new(
            Arc::new(t.clone()),
            host,
            gc.clone(),
            conductor,
            together::session::GuestConfig {
                name: name.into(),
                proof: together::invite::proof(&door, &key),
                app: "Venuta test".into(),
                host_name: "Asha".into(),
            },
        );
        self.net.connect(&t, &self.people[0].t, Way::LAN, Way::LAN);
        self.people.push(Person {
            name: name.into(),
            t,
            clock: gc,
            log,
            rules,
            side: Some(Side::Guest(Box::new(g))),
            ways: (Way::LAN, Way::LAN),
        });
        i
    }
}
