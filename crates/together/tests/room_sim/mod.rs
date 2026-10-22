//! Whole rooms on the simulated network, in simulated time: the host's Room v2 behind a thin
//! host loop, and each person (the host too) a scripted player that answers prepares and arms
//! as told. Every message goes over `SimNet` as protocol 3 lines, checked on arrival as a real
//! peer checks them, so these tests run the protocol as well as the room.
//!
//! The host loop here is test scaffolding, not the session (that is WP9's): it reads hellos at
//! the door, hands each message to `Room::handle`, each ping to `Room::heard`, each loss to
//! `Room::lost`, ticks the room every step, and sends what the room says to whom it says.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use together::anchor::{Anchor, EntryId, Next, PrepId, Purpose};
use together::invite::proof;
use together::proto::Song;
use together::proto::v3::{self, Action, RoomView, ToGuest, ToHost, UnableWhy};
use together::room::v2::{Changes, HOST, HostNote, Knock, Opening, Report, Room};
use together::room_player::{ListenMode, SubState};
use together::sim_net::{SimNet, SimTransport, Way};
use together::time::{MonoClock, SimClock, SkewedClock};
use together::transport::{ClockDatagram, ControlMsg, PathInfo, PeerId, Transport, TransportEvent};

pub const SECRET: &str = "0123456789abcdef0123456789abcdef";
pub const MS: i64 = 1_000;
pub const S: i64 = 1_000_000;
const NS: u64 = 1_000;

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

/// How a person's player answers.
#[derive(Debug, Clone)]
pub struct Player {
    /// Ready this long after a prepare arrives (`None`: never).
    pub ready_ms: Option<i64>,
    /// Ready this long after an armed next arrives (`None`: never).
    pub next_ready_ms: Option<i64>,
    /// Songs (by key) it has no copy of: `Unable` for them.
    pub unable: HashSet<String>,
    /// Clock pings once a second (the heartbeat).
    pub pings: bool,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            ready_ms: Some(200),
            next_ready_ms: Some(500),
            unable: HashSet::new(),
            pings: true,
        }
    }
}

/// One person: the host (index 0, no transport: its conductor is local) or a guest.
pub struct Person {
    pub name: String,
    t: Option<SimTransport>,
    rx: Option<Receiver<TransportEvent>>,
    /// Their member id, once welcomed.
    pub id: Option<u32>,
    pub player: Player,
    /// Every message that reached them: (the host's time as it arrived, the message).
    pub got: Vec<(i64, ToGuest)>,
    /// Their latest view.
    pub view: Option<RoomView>,
    /// Answers to send at a host time.
    due: Vec<(i64, ToHost)>,
    next_ping: i64,
    /// What they hear now, as their scripted player has it (from the latest commit or hold).
    pub ways: (Way, Way),
}

/// A whole room.
pub struct Sim {
    pub clock: SimClock,
    pub net: Arc<SimNet>,
    host_t: SimTransport,
    host_rx: Receiver<TransportEvent>,
    pub room: Room,
    pub people: Vec<Person>,
    /// The host's side: which peer is which member, and back.
    peer_of: HashMap<u32, PeerId>,
    member_of: HashMap<PeerId, u32>,
    pub notes: Vec<HostNote>,
    pub gone: Vec<u32>,
    pub volume: Option<u8>,
    pub ended: bool,
    /// What the room refused, with whom and why.
    pub refused: Vec<(u32, String)>,
    /// The host's time each step ends at.
    step_us: i64,
    next_path: i64,
}

impl Sim {
    /// A room with these songs, opened as `opening`, and these guests on these ways (guest to
    /// host, host to guest), let in without asking; runs until everyone is in.
    pub fn new(songs: Vec<Song>, opening: Opening, guests: &[(&str, Way, Way)]) -> Sim {
        let clock = SimClock::new(0);
        let tb: Arc<dyn together::time::Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb, 11);
        let base: Arc<dyn MonoClock> = Arc::new(clock.clone());
        // The host's clock: booted long before, a little fast.
        let host_t = net.peer(Arc::new(SkewedClock::new(
            base.clone(),
            500_000 * 1_000_000_000,
            30.0,
        )));
        let host_rx = host_t.events();
        let mut room = Room::new("Asha", SECRET, songs, opening);
        room.set_let_in_without_asking(true);
        let mut people = vec![Person {
            name: "Asha".into(),
            t: None,
            rx: None,
            id: Some(HOST),
            player: Player::default(),
            got: Vec::new(),
            view: None,
            due: Vec::new(),
            next_ping: 0,
            ways: (Way::LAN, Way::LAN),
        }];
        for (i, (name, up, down)) in guests.iter().enumerate() {
            let t = net.peer(Arc::new(SkewedClock::new(
                base.clone(),
                (1_000 + 77 * i as u64) * 1_000_000_000,
                -20.0 + 9.0 * i as f64,
            )));
            let rx = t.events();
            people.push(Person {
                name: (*name).into(),
                t: Some(t),
                rx: Some(rx),
                id: None,
                player: Player::default(),
                got: Vec::new(),
                view: None,
                due: Vec::new(),
                next_ping: 0,
                ways: (*up, *down),
            });
        }
        let mut sim = Sim {
            clock,
            net,
            host_t,
            host_rx,
            room,
            people,
            peer_of: HashMap::new(),
            member_of: HashMap::new(),
            notes: Vec::new(),
            gone: Vec::new(),
            volume: None,
            ended: false,
            refused: Vec::new(),
            step_us: 5 * MS,
            next_path: 0,
        };
        for i in 1..sim.people.len() {
            sim.connect(i);
        }
        let all_in = sim.run_until(|s| s.people.iter().all(|p| p.id.is_some()), 5_000);
        assert!(all_in, "everyone in");
        // A second for the host's transport to say everyone's path.
        sim.run_ms(1_100);
        sim
    }

    /// Run in steps of `ms` (longer runs; 5 ms by default).
    pub fn set_step_ms(&mut self, ms: i64) {
        self.step_us = ms * MS;
    }

    /// A guest that joins now, on these ways, with this player (let in without asking); runs
    /// until they're in.
    pub fn join(&mut self, name: &str, up: Way, down: Way, player: Player) -> usize {
        let base: Arc<dyn MonoClock> = Arc::new(self.clock.clone());
        let t = self.net.peer(Arc::new(SkewedClock::new(
            base,
            9_000 * 1_000_000_000,
            12.0,
        )));
        let rx = t.events();
        self.people.push(Person {
            name: name.into(),
            t: Some(t),
            rx: Some(rx),
            id: None,
            player,
            got: Vec::new(),
            view: None,
            due: Vec::new(),
            next_ping: 0,
            ways: (up, down),
        });
        let i = self.people.len() - 1;
        self.connect(i);
        assert!(
            self.run_until(|s| s.people[i].id.is_some(), 5_000),
            "{name} in"
        );
        i
    }

    /// Change person `i`'s path (a path change on the network, and what the host's transport
    /// says of it from now on).
    pub fn set_ways(&mut self, i: usize, up: Way, down: Way) {
        self.people[i].ways = (up, down);
        let t = self.guest_t(i);
        self.net.set_way(&t, &self.host_t, up, down);
    }

    /// One Wi-Fi for everyone.
    pub fn lan(songs: Vec<Song>, opening: Opening, names: &[&str]) -> Sim {
        let g: Vec<_> = names.iter().map(|n| (*n, Way::LAN, Way::LAN)).collect();
        Sim::new(songs, opening, &g)
    }

    fn connect(&mut self, i: usize) {
        let (up, down) = self.people[i].ways;
        let t = self.people[i].t.clone().unwrap();
        self.net.connect(&t, &self.host_t, up, down);
    }

    /// The host's time now (µs).
    pub fn now(&self) -> i64 {
        (self.host_t.now_ns() / NS) as i64
    }

    /// Run for `ms`.
    pub fn run_ms(&mut self, ms: i64) {
        let until = self.now() + ms * MS;
        while self.now() < until {
            self.step();
        }
    }

    /// Run until `done` (at most `max_ms`); whether it came.
    pub fn run_until(&mut self, done: impl Fn(&Sim) -> bool, max_ms: i64) -> bool {
        let until = self.now() + max_ms * MS;
        while self.now() < until {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    /// Run to the host time `t` (µs).
    pub fn run_to(&mut self, t: i64) {
        while self.now() < t {
            self.step();
        }
    }

    fn step(&mut self) {
        let next_ns = self.clock.now_ns() + (self.step_us as u64) * NS;
        self.clock.run_until(next_ns);
        let now = self.now();
        // The host's side of the network.
        while let Ok(e) = self.host_rx.try_recv() {
            match e {
                TransportEvent::Message { from, msg, .. } => self.at_host(from, &msg, now),
                TransportEvent::Datagram { from, bytes, .. } => {
                    if let (Some(ClockDatagram::Ping { .. }), Some(&id)) =
                        (ClockDatagram::decode(&bytes), self.member_of.get(&from))
                    {
                        let c = self.room.heard(id, now);
                        self.apply(c);
                    }
                }
                TransportEvent::Lost { peer } => {
                    if let Some(&id) = self.member_of.get(&peer) {
                        let c = self.room.lost(id, now);
                        self.apply(c);
                    }
                }
                TransportEvent::Joined { .. } | TransportEvent::PathChanged { .. } => {}
            }
        }
        // The host's own conductor answers locally.
        let due = take_due(&mut self.people[0].due, now);
        for m in due {
            self.host_conductor_says(m, now);
        }
        // The guests.
        for i in 1..self.people.len() {
            self.guest_step(i, now);
        }
        // The host's transport says each guest's path once a second.
        if now >= self.next_path {
            self.next_path = now + S;
            let ids: Vec<(u32, usize)> = self
                .people
                .iter()
                .enumerate()
                .skip(1)
                .filter_map(|(i, p)| p.id.map(|id| (id, i)))
                .collect();
            for (id, i) in ids {
                let (up, down) = self.people[i].ways;
                let path = PathInfo {
                    direct: !(up.relay || down.relay),
                    relay: up.relay || down.relay,
                    rtt_us: Some(((up.delay_ms + down.delay_ms) * 1_000.0) as u32),
                };
                let c = self.room.path(id, path, now);
                self.apply(c);
            }
        }
        let c = self.room.tick(now);
        self.apply(c);
    }

    fn at_host(&mut self, from: PeerId, msg: &ControlMsg, now: i64) {
        let line = msg.0.strip_suffix(b"\n").unwrap_or(&msg.0);
        let Some(&id) = self.member_of.get(&from) else {
            // At the door.
            match v3::door(line) {
                Ok(h) => {
                    let key = format!("peer{}", from.0);
                    let (k, c) = self.room.knock(&key, &h.name, &h.proof, now);
                    match k {
                        Knock::Refused(why) => {
                            let _ = self
                                .host_t
                                .send(from, ControlMsg::of(&ToGuest::Refused { why }));
                        }
                        Knock::In(id) | Knock::Waiting(id) => {
                            self.peer_of.insert(id, from);
                            self.member_of.insert(from, id);
                        }
                    }
                    self.apply(c);
                }
                Err(why) => {
                    if let Some(answer) = why.answer() {
                        let _ = self.host_t.send(from, ControlMsg::of(&answer));
                    }
                }
            }
            return;
        };
        let m: ToHost = msg.read().expect("a guest's line reads");
        v3::check_to_host(&m).expect("a guest's message checks");
        if let ToHost::Hello { .. } = m {
            // Back on the same peer (a reconnect): through the door again.
            self.member_of.remove(&from);
            self.at_host(from, msg, now);
            return;
        }
        let c = self.room.heard(id, now);
        self.apply(c);
        match self.room.handle(id, m, now) {
            Ok(c) => self.apply(c),
            Err(why) => self.refused.push((id, why)),
        }
    }

    fn host_conductor_says(&mut self, m: ToHost, now: i64) {
        match self.room.handle(HOST, m, now) {
            Ok(c) => self.apply(c),
            Err(why) => self.refused.push((HOST, why)),
        }
    }

    fn guest_step(&mut self, i: usize, now: i64) {
        let events: Vec<TransportEvent> = {
            let rx = self.people[i].rx.as_ref().unwrap();
            std::iter::from_fn(|| rx.try_recv().ok()).collect()
        };
        for e in events {
            match e {
                TransportEvent::Joined { peer } => {
                    let key = format!("peer{}", self.people[i].t.as_ref().unwrap().id().0);
                    let hello = ToHost::Hello {
                        protocol: v3::PROTOCOL,
                        name: self.people[i].name.clone(),
                        proof: proof(SECRET, &key),
                        app: "Venuta test".into(),
                    };
                    let _ = self.people[i]
                        .t
                        .as_ref()
                        .unwrap()
                        .send(peer, ControlMsg::of(&hello));
                }
                TransportEvent::Message { msg, .. } => {
                    let m: ToGuest = msg.read().expect("the host's line reads");
                    v3::check_to_guest(&m).expect("the host's message checks");
                    self.arrive(i, m, now);
                }
                _ => {}
            }
        }
        let due = take_due(&mut self.people[i].due, now);
        for m in due {
            self.send_from(i, &m);
        }
        let p = &mut self.people[i];
        if p.player.pings && now >= p.next_ping {
            p.next_ping = now + S;
            let t = p.t.as_ref().unwrap();
            let _ = t.send_datagram(
                self.host_t.id(),
                ClockDatagram::Ping { id: 1, t1: 0 }.encode(),
            );
        }
    }

    /// A message reached person `i` (the host's local ones too): their scripted player answers.
    fn arrive(&mut self, i: usize, m: ToGuest, now: i64) {
        let p = &mut self.people[i];
        match &m {
            ToGuest::Welcome { you, room } => {
                p.id = Some(*you);
                p.view = Some(room.clone());
            }
            ToGuest::Room { room } => p.view = Some(room.clone()),
            ToGuest::Prepare { prep, entry, .. } => {
                let key = key_of(p.view.as_ref(), *entry);
                if key.is_some_and(|k| p.player.unable.contains(&k)) {
                    p.due.push((
                        now,
                        ToHost::Unable {
                            prep: *prep,
                            entry: *entry,
                            why: UnableWhy::NoCopy,
                        },
                    ));
                } else if let Some(ms) = p.player.ready_ms {
                    p.due.push((
                        now + ms * MS,
                        ToHost::Ready {
                            prep: *prep,
                            entry: *entry,
                        },
                    ));
                }
            }
            ToGuest::ArmNext { prep, next } => {
                let key = key_of(p.view.as_ref(), next.entry);
                if key.is_some_and(|k| p.player.unable.contains(&k)) {
                    p.due.push((
                        now,
                        ToHost::Unable {
                            prep: *prep,
                            entry: next.entry,
                            why: UnableWhy::NoCopy,
                        },
                    ));
                } else if let Some(ms) = p.player.next_ready_ms {
                    p.due.push((
                        now + ms * MS,
                        ToHost::Ready {
                            prep: *prep,
                            entry: next.entry,
                        },
                    ));
                }
            }
            _ => {}
        }
        p.got.push((now, m));
    }

    fn send_from(&mut self, i: usize, m: &ToHost) {
        if i == 0 {
            let now = self.now();
            self.host_conductor_says(m.clone(), now);
        } else {
            let t = self.people[i].t.as_ref().unwrap();
            let _ = t.send(self.host_t.id(), ControlMsg::of(m));
        }
    }

    /// Carry out what the room said.
    pub fn apply(&mut self, c: Changes) {
        let now = self.now();
        for o in c.out {
            if o.to == HOST {
                self.arrive(0, o.msg, now);
            } else if let Some(&peer) = self.peer_of.get(&o.to) {
                let _ = self.host_t.send(peer, ControlMsg::of(&o.msg));
            }
        }
        self.notes.extend(c.notes);
        self.volume = c.volume.or(self.volume);
        for id in &c.gone {
            if let Some(peer) = self.peer_of.remove(id) {
                self.member_of.remove(&peer);
            }
        }
        self.gone.extend(c.gone);
        self.ended |= c.ended;
    }

    /// Person `i` presses: the host asks locally, a guest over the network.
    pub fn press(&mut self, i: usize, action: Action) {
        let m = ToHost::Ask { action };
        if i == 0 {
            let now = self.now();
            self.host_conductor_says(m, now);
        } else {
            self.send_from(i, &m);
        }
    }

    /// Person `i` sends something else.
    pub fn say(&mut self, i: usize, m: ToHost) {
        if i == 0 {
            let now = self.now();
            self.host_conductor_says(m, now);
        } else {
            self.send_from(i, &m);
        }
    }

    /// Person `i`'s player says how it is.
    pub fn report(&mut self, i: usize, sub_state: SubState) {
        let entry = self.room.anchor().map(|a| a.entry);
        self.say(
            i,
            ToHost::State {
                entry,
                sub_state,
                error_us: None,
                bound_us: None,
                copy: None,
            },
        );
    }

    /// The room's own report for the host, straight in (no network).
    pub fn host_report(&mut self, r: Report) {
        let now = self.now();
        let c = self.room.state(HOST, r, now);
        self.apply(c);
    }

    pub fn id(&self, i: usize) -> u32 {
        self.people[i].id.expect("in the room")
    }

    pub fn guest_t(&self, i: usize) -> SimTransport {
        self.people[i].t.clone().unwrap()
    }

    pub fn host_t(&self) -> SimTransport {
        self.host_t.clone()
    }

    /// Cut person `i`'s link (both ways); `restore` brings it back.
    pub fn cut(&mut self, i: usize) {
        let t = self.guest_t(i);
        self.net.cut(&t, &self.host_t);
    }

    pub fn restore(&mut self, i: usize) {
        let t = self.guest_t(i);
        self.net.restore(&t, &self.host_t);
    }

    /// The entry id of the song at queue place `i` now.
    pub fn entry(&self, i: usize) -> EntryId {
        self.room.queue()[i].id
    }

    /// The entry id of song `n` (by its title "Song n").
    pub fn entry_of(&self, n: u32) -> EntryId {
        let t = format!("Song {n}");
        self.room
            .queue()
            .iter()
            .find(|e| e.song.title == t)
            .map(|e| e.id)
            .expect("in the queue")
    }

    // --- What people got -----------------------------------------------------------------

    pub fn prepares(&self, i: usize) -> Vec<(i64, PrepId, EntryId, i64, Purpose)> {
        self.people[i]
            .got
            .iter()
            .filter_map(|(t, m)| match m {
                ToGuest::Prepare {
                    prep,
                    entry,
                    content_us,
                    purpose,
                } => Some((*t, *prep, *entry, *content_us, *purpose)),
                _ => None,
            })
            .collect()
    }

    pub fn commits(&self, i: usize) -> Vec<(i64, Anchor)> {
        self.people[i]
            .got
            .iter()
            .filter_map(|(t, m)| match m {
                ToGuest::Commit { anchor } => Some((*t, *anchor)),
                _ => None,
            })
            .collect()
    }

    pub fn holds(&self, i: usize) -> Vec<(i64, Anchor)> {
        self.people[i]
            .got
            .iter()
            .filter_map(|(t, m)| match m {
                ToGuest::Hold { anchor } => Some((*t, *anchor)),
                _ => None,
            })
            .collect()
    }

    pub fn arms(&self, i: usize) -> Vec<(i64, PrepId, Next)> {
        self.people[i]
            .got
            .iter()
            .filter_map(|(t, m)| match m {
                ToGuest::ArmNext { prep, next } => Some((*t, *prep, *next)),
                _ => None,
            })
            .collect()
    }

    pub fn disarms(&self, i: usize) -> usize {
        self.people[i]
            .got
            .iter()
            .filter(|(_, m)| matches!(m, ToGuest::Disarm { .. }))
            .count()
    }

    /// Everything person `i` got from host time `t` on.
    pub fn since(&self, i: usize, t: i64) -> Vec<&ToGuest> {
        self.people[i]
            .got
            .iter()
            .filter(|(at, _)| *at >= t)
            .map(|(_, m)| m)
            .collect()
    }

    /// Room-changing messages (prepares, commits, holds, arms, disarms) person `i` got from `t`.
    pub fn playback_since(&self, i: usize, t: i64) -> Vec<&ToGuest> {
        self.since(i, t)
            .into_iter()
            .filter(|m| {
                matches!(
                    m,
                    ToGuest::Prepare { .. }
                        | ToGuest::Commit { .. }
                        | ToGuest::Hold { .. }
                        | ToGuest::ArmNext { .. }
                        | ToGuest::Disarm { .. }
                )
            })
            .collect()
    }

    /// Start the room (the host presses play) and run until it plays; the committed anchor.
    pub fn start(&mut self) -> Anchor {
        self.press(0, Action::Play);
        let n = self.commits(0).len();
        assert!(
            self.run_until(|s| s.commits(0).len() > n, 5_000),
            "the room started"
        );
        self.commits(0).last().unwrap().1
    }

    /// Activity lines, as everyone sees them.
    pub fn activity(&self) -> Vec<String> {
        self.room
            .view_for(HOST)
            .activity
            .iter()
            .map(|a| format!("{} {}", a.name, a.what))
            .collect()
    }

    pub fn member(&self, id: u32) -> v3::Member {
        self.room
            .view_for(HOST)
            .members
            .into_iter()
            .find(|m| m.id == id)
            .expect("a member")
    }

    pub fn set_mode(&mut self, i: usize, mode: ListenMode) {
        self.say(i, ToHost::Mode { mode });
    }
}

fn take_due(due: &mut Vec<(i64, ToHost)>, now: i64) -> Vec<ToHost> {
    let mut out = Vec::new();
    due.retain(|(t, m)| {
        if *t <= now {
            out.push(m.clone());
            false
        } else {
            true
        }
    });
    out
}

fn key_of(view: Option<&RoomView>, entry: EntryId) -> Option<String> {
    view?
        .queue
        .iter()
        .find(|e| e.id == entry)
        .map(|e| e.song.key.clone())
}
