//! Whole rooms with real players (WP9's gates 7.3, 7.4 and 7.6, and zero clicks): the host's
//! and the guests' sessions and conductors on the simulated network, each person's player a
//! real engine 2 on its own simulated output, all on one simulated clock the test moves. The
//! songs carry the test music (left, judged by the detectors) and a counting ramp (right: what
//! each simulated DAC played, at its exact time, says which place of which song was heard,
//! against the room's own anchors).
//!
//! Every computer here shares the simulation's clock (a real player reads the clock its
//! threads wait on), so the hosts' and guests' clocks differ by nothing: what the guests' clock
//! estimates have to get right is the network's delays and their asymmetry, which is what
//! decides the error (WP6 and WP5 cover clocks that drift against each other). The sound cards
//! drift (each simulated output's skew).
//!
//! These also carry the protocol 2 room check's scenarios (`app/.../room_check.rs`, in real
//! time until WP9): songs heard all the way one after another, long and short; three people
//! together on one Wi-Fi and over the internet; every way a room ends giving each player back.

mod room_kit;

use player::engine::{Cmd, Player};
use player::sim::SimConfig;
use player::voice_api::RenderNote;
use room_kit::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use together::align::Alignment;
use together::anchor::EntryId;
use together::clock_map::{self, ClockMap};
use together::conductor::{Conductor, Copies, Copy, Lookup};
use together::proto::Song;
use together::proto::v3::Action;
use together::room::v2::{HOST, Opening, Room};
use together::room_player::{ListenMode, RoomHandle, RoomPlayer, SyncMode};
use together::session::{GuestConfig, GuestSession, HostConfig, HostSession, Stage};
use together::sim_net::{SimNet, SimTransport, Way};
use together::time::{MonoClock, SimClock, Timebase};
use together::view::CopyKind;

/// A test's engine 2, lent to the conductor (taken back to quit it).
type Lent = Arc<Mutex<Option<Player<PathBuf>>>>;

/// The player a room takes: the test's engine 2, lent to the conductor.
struct Seat(Lent);

impl RoomPlayer<PathBuf> for Seat {
    fn attach(
        &self,
        clock: clock_map::ClockMapHandle,
        mode: SyncMode,
    ) -> Box<dyn RoomHandle<PathBuf>> {
        let p = self.0.lock().unwrap();
        RoomPlayer::attach(p.as_ref().expect("a player"), clock, mode)
    }
    fn adopt(
        &self,
        clock: clock_map::ClockMapHandle,
        mode: SyncMode,
        entry: EntryId,
    ) -> Box<dyn RoomHandle<PathBuf>> {
        let p = self.0.lock().unwrap();
        RoomPlayer::adopt(p.as_ref().expect("a player"), clock, mode, entry)
    }
}

/// Everyone has every song's file: exact copies (the same bytes), or approximate (as if
/// another copy) for listening apart.
struct Files {
    files: HashMap<String, PathBuf>,
    exact: bool,
}

impl Copies<PathBuf> for Files {
    fn find(&mut self, song: &Song) -> Lookup<PathBuf> {
        match self.files.get(&song.key) {
            Some(f) => Lookup::Found(Copy {
                item: f.clone(),
                align: if self.exact {
                    Alignment::EXACT
                } else {
                    Alignment::APPROXIMATE
                },
                kind: CopyKind::OwnFile,
                length_ms: None,
            }),
            None => Lookup::NoCopy,
        }
    }
}

/// A room song with its file and where its ramp counts from.
#[derive(Clone)]
struct Track {
    song: Song,
    file: PathBuf,
    base: i32,
    secs: f64,
}

/// `n` songs of `secs`, the music running on across them (a gapless album).
fn album(n: usize, secs: f64) -> Vec<Track> {
    tracks(n, secs, true)
}

/// `n` songs of `secs`, each rising from silence and falling to it (singles: a skip into one
/// starts at its own quiet start, and nothing follows its own quiet end).
fn singles(n: usize, secs: f64) -> Vec<Track> {
    tracks(n, secs, false)
}

fn tracks(n: usize, secs: f64, gapless: bool) -> Vec<Track> {
    (0..n)
        .map(|k| {
            let base = -8_000_000 + (k as i32) * 3_500_000;
            let file = song(
                k as f64 * secs,
                secs,
                base,
                !gapless || k == 0,
                !gapless || k + 1 == n,
            );
            Track {
                song: Song {
                    key: format!("online:song{:07}", k + 1),
                    title: format!("Song {}", k + 1),
                    artists: vec!["Tester".into()],
                    album: Some("Album".into()),
                    duration_ms: Some((secs * 1000.0) as u64),
                    isrc: None,
                    mbid: None,
                    online_id: Some(format!("song{:07}", k + 1)),
                    host_file: false,
                    format: None,
                    artwork_url: None,
                    added_by: None,
                },
                file,
                base,
                secs,
            }
        })
        .collect()
}

enum Side {
    Host(Box<HostSession<PathBuf>>),
    Guest(Box<GuestSession<PathBuf>>),
}

struct One {
    name: String,
    t: SimTransport,
    p: Person<PathBuf>,
    seat: Lent,
    side: Option<Side>,
    asleep: bool,
}

impl One {
    fn guest(&mut self) -> &mut GuestSession<PathBuf> {
        match self.side.as_mut() {
            Some(Side::Guest(g)) => g,
            _ => panic!("{} is no guest", self.name),
        }
    }
    fn host(&mut self) -> &mut HostSession<PathBuf> {
        match self.side.as_mut() {
            Some(Side::Host(h)) => h,
            _ => panic!("{} is no host", self.name),
        }
    }
    /// The player's status now.
    fn status(&mut self) -> player::engine::Status<PathBuf> {
        self.p.status().clone()
    }
    fn send(&self, c: Cmd<PathBuf>) {
        if let Some(p) = self.seat.lock().unwrap().as_ref() {
            p.send(c);
        }
    }
    /// Quit its player and what it played.
    fn finish(&mut self) -> Heard {
        self.side = None;
        self.p.player = self.seat.lock().unwrap().take();
        self.p.finish()
    }
}

struct Whole {
    clock: SimClock,
    net: Arc<SimNet>,
    people: Vec<One>,
    tracks: Vec<Track>,
    exact: bool,
    /// The room's anchors as they were, for the truth.
    tl: Timeline,
    last_seq: Option<u64>,
}

impl Whole {
    /// The host Asha with `tracks`, the room resting; every person's copies exact or not.
    fn new(tracks: Vec<Track>, exact: bool) -> Whole {
        let clock = SimClock::new(1_000_000_000);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb, 31);
        let mut w = Whole {
            clock,
            net,
            people: Vec::new(),
            tracks,
            exact,
            tl: Timeline::default(),
            last_seq: None,
        };
        let mono: Arc<dyn MonoClock> = Arc::new(w.clock.clone());
        let t = w.net.peer(mono.clone());
        let (p, seat) = w.person(0.0);
        let map = clock_map::handle();
        map.store(Arc::new(Some(ClockMap::identity())));
        let player: Arc<dyn RoomPlayer<PathBuf>> = Arc::new(Seat(seat.clone()));
        let handle = player.attach(map.clone(), SyncMode::Apart);
        let conductor =
            Conductor::new(player, map, w.copies(), ListenMode::Apart).with_handle(handle, None);
        let mut room = Room::new(
            "Asha",
            "0123456789abcdef0123456789abcdef",
            w.tracks.iter().map(|t| t.song.clone()).collect(),
            Opening::Resting,
        );
        room.set_let_in_without_asking(true);
        let host = HostSession::new(
            Arc::new(t.clone()),
            mono,
            room,
            conductor,
            HostConfig {
                invite_open: Arc::new(|| true),
            },
        );
        w.people.push(One {
            name: "Asha".into(),
            t,
            p,
            seat,
            side: Some(Side::Host(Box::new(host))),
            asleep: false,
        });
        w.pass_ms(20);
        w
    }

    fn copies(&self) -> Box<dyn Copies<PathBuf>> {
        Box::new(Files {
            files: self
                .tracks
                .iter()
                .map(|t| (t.song.key.clone(), t.file.clone()))
                .collect(),
            exact: self.exact,
        })
    }

    /// A player on its own simulated output, its sound card `ppm` off.
    fn person(&self, ppm: f64) -> (Person<PathBuf>, Lent) {
        let mut p = Person::new(
            &self.clock,
            SimConfig {
                skew_ppm: ppm,
                ..SimConfig::default()
            },
        );
        let seat = Arc::new(Mutex::new(p.player.take()));
        (p, seat)
    }

    /// A guest joins over these ways, as `mode`; runs until in.
    fn join(&mut self, name: &str, up: Way, down: Way, mode: ListenMode, ppm: f64) -> usize {
        let i = self.arrive(name, up, down, mode, ppm);
        assert!(
            self.run_until(|w| w.people[i].guest().stage() == Stage::In, 10_000),
            "{name} in"
        );
        if mode != ListenMode::Apart {
            self.people[i].guest().conductor().set_mode(mode);
        }
        i
    }

    fn arrive(&mut self, name: &str, up: Way, down: Way, mode: ListenMode, ppm: f64) -> usize {
        let mono: Arc<dyn MonoClock> = Arc::new(self.clock.clone());
        let t = self.net.peer(mono.clone());
        let (p, seat) = self.person(ppm);
        let conductor = Conductor::new(
            Arc::new(Seat(seat.clone())),
            clock_map::handle(),
            self.copies(),
            ListenMode::Apart,
        );
        let key = format!("peer{}", t.id().0);
        let g = GuestSession::new(
            Arc::new(t.clone()),
            self.people[0].t.id(),
            mono,
            conductor,
            GuestConfig {
                name: name.into(),
                proof: together::invite::proof("0123456789abcdef0123456789abcdef", &key),
                app: "Venuta test".into(),
                host_name: "Asha".into(),
            },
        );
        let _ = mode;
        self.net.connect(&t, &self.people[0].t, up, down);
        self.people.push(One {
            name: name.into(),
            t,
            p,
            seat,
            side: Some(Side::Guest(Box::new(g))),
            asleep: false,
        });
        self.people.len() - 1
    }

    fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    /// The host's time now (µs): the simulation's.
    fn host_now(&self) -> i64 {
        (self.clock.now_ns() / 1_000) as i64
    }

    fn base_of(&self, entry: EntryId) -> i32 {
        let i = (entry.0 - 1) as usize;
        self.tracks[i].base
    }

    /// Five ms of the world: the players run each millisecond, the sessions step.
    fn step(&mut self) {
        pass(&self.clock, 5, &mut []);
        for p in &mut self.people {
            if p.asleep {
                continue;
            }
            match p.side.as_mut() {
                Some(Side::Host(h)) => h.step(),
                Some(Side::Guest(g)) => g.step(),
                None => {}
            }
            p.p.collect();
        }
        // The truth: the room's anchors as the host made them.
        let a = match self.people[0].side.as_ref() {
            Some(Side::Host(h)) => h.room().anchor(),
            _ => None,
        };
        if let Some(a) = a
            && Some(a.seq) != self.last_seq
        {
            self.last_seq = Some(a.seq);
            let base = self.base_of(a.entry);
            let from = a.host_us.min(self.host_now());
            self.tl.set(from, a, base);
        }
    }

    fn pass_ms(&mut self, ms: u64) {
        let until = self.now_ns() + ms * 1_000_000;
        while self.now_ns() < until {
            self.step();
        }
    }

    fn run_until(&mut self, done: impl Fn(&mut Whole) -> bool, max_ms: u64) -> bool {
        let until = self.now_ns() + max_ms * 1_000_000;
        while self.now_ns() < until {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    fn host_ask(&mut self, a: Action) {
        self.people[0].host().ask(a).expect("the host may");
    }

    /// The room starts from the first song.
    fn start(&mut self) {
        self.host_ask(Action::Play);
        assert!(self.run_until(
            |w| {
                w.people[0]
                    .host()
                    .room()
                    .anchor()
                    .is_some_and(|a| a.playing)
            },
            4_000
        ));
    }

    fn cut(&mut self, i: usize) {
        let (a, b) = (self.people[i].t.clone(), self.people[0].t.clone());
        self.net.cut(&a, &b);
    }

    fn restore(&mut self, i: usize) {
        let (a, b) = (self.people[i].t.clone(), self.people[0].t.clone());
        self.net.restore(&a, &b);
    }

    /// Everyone's players quit; what each played.
    fn finish(&mut self) -> Vec<Heard> {
        // The fades of whatever still sounds.
        for p in &mut self.people {
            p.side = None;
        }
        self.pass_ms(1_500);
        let mut out = Vec::new();
        for p in &mut self.people {
            out.push(p.finish());
        }
        out
    }
}

fn internet() -> (Way, Way) {
    (Way::internet(20.0, 10.0), Way::internet(40.0, 10.0))
}

fn relay() -> (Way, Way) {
    let w = Way {
        relay: true,
        ..Way::internet(40.0, 10.0)
    };
    (w, w)
}

/// The first tick at or after `from_ns` that plays something of the songs (a ramp value).
fn first_sound(h: &Heard, from_ns: f64) -> Option<f64> {
    let k = h.tick_at(from_ns);
    (k..h.right.len())
        .find(|&i| h.count(i) != 0)
        .map(|i| h.at_ns[i])
}

/// The first tick at or after `from_ns` playing the song whose ramp counts from `base`.
fn first_of(h: &Heard, base: i32, from_ns: f64) -> Option<f64> {
    let k = h.tick_at(from_ns);
    (k..h.right.len())
        .find(|&i| {
            let v = h.count(i) - base - 1;
            (0..48_000 * 200).contains(&v)
        })
        .map(|i| h.at_ns[i])
}

/// How many frames of each song were heard, and in how many straight runs.
fn songs_heard(h: &Heard, tracks: &[Track]) -> Vec<(usize, usize)> {
    let r = runs(h);
    tracks
        .iter()
        .map(|t| {
            let lo = t.base + 1;
            let hi = t.base + (t.secs * f64::from(R)) as i32;
            let mine: Vec<&(i32, usize)> = r.iter().filter(|(v, _)| *v >= lo && *v <= hi).collect();
            (mine.iter().map(|(_, n)| *n).sum(), mine.len())
        })
        .collect()
}

// --- Songs one after another (the room check's chains) -----------------------------------------

/// The host alone, `n` songs of `secs` through: each heard all the way in one straight run (a
/// gapless album, exact copies: the joins are sample-exact).
fn chain(n: usize, secs: f64) {
    let mut w = Whole::new(album(n, secs), true);
    w.start();
    w.pass_ms((n as f64 * secs * 1000.0) as u64 + 2_000);
    let tracks = w.tracks.clone();
    let h = w.finish().remove(0);
    let heard = songs_heard(&h, &tracks);
    for (k, (got, runs)) in heard.iter().enumerate() {
        let frames = (tracks[k].secs * f64::from(R)) as usize;
        assert!(
            *got as f64 >= frames as f64 * 0.99,
            "song {k} heard all the way: {got} of {frames} ({heard:?})"
        );
        assert!(*runs <= 1 || k > 0, "song {k} in one run: {heard:?}");
    }
    assert_music_clean("a chain of songs", &h);
}

#[test]
fn a_room_plays_long_songs_through_one_after_another() {
    // Longer than the 20 s the next one is armed ahead.
    chain(3, 24.0);
}

#[test]
fn a_room_plays_short_songs_through_one_after_another() {
    // Shorter than that: each armed as soon as it starts.
    chain(4, 2.5);
}

// --- Together: one Wi-Fi, the internet ------------------------------------------------------------

/// Host and two guests over `ways` in `mode`, three songs: every song heard by everyone, the
/// guests' errors against the room (ms, p95 and worst after the first 5 s of each), clean.
fn three(ways: (Way, Way), mode: ListenMode, exact: bool) -> (f64, f64) {
    let mut w = Whole::new(album(3, 12.0), exact);
    if mode == ListenMode::EveryDevice {
        w.people[0].host().conductor().set_mode(mode);
    }
    w.join("Ravi", ways.0, ways.1, mode, 60.0);
    w.join("Meera", ways.1, ways.0, mode, -90.0);
    w.pass_ms(1_000);
    let t0 = w.now_ns() as f64;
    w.start();
    w.pass_ms(38_000);
    let (tl, tracks) = (w.tl.clone(), w.tracks.clone());
    let heard = w.finish();
    let mut all = Vec::new();
    for (who, h) in heard.iter().enumerate() {
        for (k, (got, _)) in songs_heard(h, &tracks).iter().enumerate() {
            let frames = tracks[k].secs * f64::from(R);
            assert!(
                *got as f64 >= frames * 0.95,
                "{who}: song {k} heard: {got} of {frames}"
            );
        }
        explain(&format!("person {who}"), h);
        assert_music_clean(&format!("person {who}"), h);
        if who > 0 {
            all.extend(errors(h, &tl, &HostTime::same(), t0 + 5e9, t0 + 36e9));
        }
    }
    assert!(!all.is_empty());
    (quantile(&all, 0.95), worst(&all))
}

#[test]
fn on_one_wifi_everyone_hears_each_song_together() {
    let (p95, max) = three((Way::LAN, Way::LAN), ListenMode::EveryDevice, true);
    eprintln!("one Wi-Fi, every device: p95 {p95:.2} ms, worst {max:.2} ms");
    assert!(
        p95 <= 1.0 && max <= 3.0,
        "7.2: p95 {p95:.2}, worst {max:.2}"
    );
}

#[test]
fn over_the_internet_everyone_hears_each_song_together() {
    let (p95, max) = three(internet(), ListenMode::Apart, false);
    eprintln!("internet, apart: p95 {p95:.2} ms, worst {max:.2} ms");
    // The clock's floor is half the asymmetry (20 ms up, 40 down: 10 ms), plus 5 ms.
    assert!(p95 <= 15.0, "7.2: p95 {p95:.2} ms");
}

// --- 7.3: time to sound -------------------------------------------------------------------------

#[test]
fn joining_and_presses_are_heard_within_their_gates() {
    for (what, ways, join_max, press_max) in [
        ("one Wi-Fi", (Way::LAN, Way::LAN), 3.0, 1.0),
        ("internet, direct", internet(), 5.0, 1.5),
        ("relay", relay(), 8.0, 2.0),
    ] {
        let mut w = Whole::new(singles(3, 40.0), false);
        w.start();
        w.pass_ms(5_000);
        let joined = w.now_ns() as f64;
        let i = w.arrive("Meera", ways.0, ways.1, ListenMode::Apart, 30.0);
        w.pass_ms(9_000);
        let pressed = w.now_ns() as f64;
        w.host_ask(Action::Next);
        w.pass_ms(4_000);
        let second = w.tracks[1].base;
        let heard = w.finish();
        let g = &heard[i];
        explain(what, g);
        let first = first_sound(g, joined).expect("Meera heard the room");
        let next = first_of(g, second, pressed).expect("and the skip");
        let host_next = first_of(&heard[0], second, pressed).expect("the host the skip");
        let (j, p, hp) = (
            (first - joined) / 1e9,
            (next - pressed) / 1e9,
            (host_next - pressed) / 1e9,
        );
        eprintln!(
            "{what}: first sound {j:.2} s after joining; a skip heard in {p:.2} s (the host {hp:.2} s)"
        );
        assert!(j <= join_max, "{what}: joined to first sound in {j:.2} s");
        assert!(
            p <= press_max && hp <= press_max,
            "{what}: a press heard in {p:.2} s"
        );
        assert_music_clean(&format!("{what}: Meera"), g);
        assert_music_clean(&format!("{what}: the host"), &heard[0]);
    }
}

// --- 7.4: recovery ------------------------------------------------------------------------------

#[test]
fn a_cut_under_two_minutes_is_heard_as_nothing_and_in_step_when_back() {
    let mut w = Whole::new(album(2, 90.0), false);
    let r = w.join("Ravi", Way::LAN, Way::LAN, ListenMode::Apart, 80.0);
    w.start();
    w.pass_ms(10_000);
    w.cut(r);
    w.pass_ms(40_000);
    assert_eq!(w.people[r].guest().stage(), Stage::Reconnecting);
    let back = w.now_ns() as f64;
    w.restore(r);
    w.pass_ms(12_000);
    assert_eq!(w.people[r].guest().stage(), Stage::In);
    let tl = w.tl.clone();
    let heard = w.finish();
    let g = &heard[r];
    assert_music_clean("cut and back", g);
    let during = errors(g, &tl, &HostTime::same(), back - 38e9, back);
    let after = errors(g, &tl, &HostTime::same(), back + 2e9, back + 10e9);
    eprintln!(
        "a 40 s cut: worst {:.2} ms while cut, {:.2} ms from 2 s after",
        worst(&during),
        worst(&after)
    );
    assert!(worst(&during) < 10.0, "played on, on its clock line");
    assert!(
        quantile(&after, 0.95) < 5.0,
        "in step within 2 s of the connection back"
    );
}

#[test]
fn a_change_of_path_from_relay_to_direct_is_heard_as_nothing() {
    let (up, down) = relay();
    let mut w = Whole::new(album(2, 90.0), false);
    let r = w.join("Ravi", up, down, ListenMode::Apart, -50.0);
    w.start();
    w.pass_ms(15_000);
    let changed = w.now_ns() as f64;
    let (a, b) = (w.people[r].t.clone(), w.people[0].t.clone());
    w.net.set_way(&a, &b, Way::LAN, Way::LAN);
    w.pass_ms(32_000);
    let tl = w.tl.clone();
    let heard = w.finish();
    assert_music_clean("relay to direct", &heard[r]);
    let after = errors(
        &heard[r],
        &tl,
        &HostTime::same(),
        changed + 30e9,
        changed + 32e9,
    );
    eprintln!("relay to direct: worst {:.2} ms 30 s after", worst(&after));
    // Apart's band is 10 ms in, 3 out.
    assert!(
        !after.is_empty() && worst(&after) < 10.0,
        "inside the band within 30 s"
    );
}

#[test]
fn the_host_gone_for_good_lets_each_guest_finish_the_song_and_stop_cleanly() {
    let mut w = Whole::new(singles(3, 60.0), false);
    let r = w.join("Ravi", Way::LAN, Way::LAN, ListenMode::Apart, 0.0);
    w.start();
    w.pass_ms(10_000);
    // The host's computer is gone: nothing of it runs, the link is dead.
    w.cut(r);
    w.people[0].asleep = true;
    w.pass_ms(150_000);
    assert_eq!(w.people[r].guest().stage(), Stage::Ended);
    assert!(w.people[r].status().sync.is_none(), "own program back");
    let tracks = w.tracks.clone();
    let heard = w.finish();
    let g = &heard[r];
    explain("the host gone", g);
    assert_music_clean("the host gone", g);
    let h = songs_heard(g, &tracks);
    // The song playing, through to its very end (nothing was armed after it before the host
    // went); nothing after it.
    let frames = (60.0 * f64::from(R)) as usize;
    assert!(h[0].0 >= frames * 97 / 100, "{h:?}");
    let last = tracks[0].base + frames as i32;
    assert!(
        (0..g.right.len()).any(|k| g.count(k) >= last - 48),
        "heard to its last frames"
    );
    assert_eq!((h[1].0, h[2].0), (0, 0), "nothing after: {h:?}");
}

// --- 7.6: after a room ----------------------------------------------------------------------------

/// Your own song (a counting ramp of its own), set in everyone's player before the room.
fn own() -> PathBuf {
    song(200.0, 20.0, 7_000_000, true, true)
}

/// After an ending: within 100 ms the player is on its own again, paused, nothing of the room
/// left; then it plays your own song bit-identical to a player that never joined.
fn gives_back(w: &mut Whole, i: usize, alone: &mut One, what: &str) {
    let t = w.now_ns();
    assert!(
        w.run_until(
            |w| {
                let s = w.people[i].status();
                s.sync.is_none() && !s.playing
            },
            100
        ),
        "{what}: own program back within 100 ms"
    );
    let _ = t;
    w.pass_ms(1_500);
    w.people[i].send(Cmd::Play);
    alone.send(Cmd::Play);
}

fn assert_same_as_alone(what: &str, joined: &Heard, alone: &Heard) {
    let last = |h: &Heard| {
        h.notes
            .iter()
            .rev()
            .find_map(|n| match *n {
                RenderNote::Started { at, content, .. } => Some((at as usize, content)),
                _ => None,
            })
            .unwrap()
    };
    let ((sj, cj), (sa, ca)) = (last(joined), last(alone));
    assert_eq!(cj, ca, "{what}: from the same place");
    let n = ms(1_500) as usize;
    assert!(
        joined.left[sj..sj + n] == alone.left[sa..sa + n]
            && joined.right[sj..sj + n] == alone.right[sa..sa + n],
        "{what}: bit-identical to a player that never joined"
    );
}

#[test]
fn every_way_a_room_ends_gives_each_player_back_bit_identical() {
    let mut w = Whole::new(album(2, 60.0), false);
    let mine = own();
    let set = |o: &One| {
        o.send(Cmd::Set {
            items: vec![mine.clone()],
            index: 0,
            start_ms: 3_000,
            play: false,
            from: String::new(),
        })
    };
    let r = w.join("Ravi", Way::LAN, Way::LAN, ListenMode::Apart, 40.0);
    let m = w.join("Meera", Way::LAN, Way::LAN, ListenMode::Apart, -40.0);
    let k = w.join("Kabir", Way::LAN, Way::LAN, ListenMode::Apart, 10.0);
    // (Set before each joined in a real app; here the room took them with nothing playing.)
    for i in 0..4 {
        set(&w.people[i]);
    }
    let (p, seat) = w.person(0.0);
    let mut alone = One {
        name: "alone".into(),
        t: w.people[0].t.clone(),
        p,
        seat,
        side: None,
        asleep: false,
    };
    set(&alone);
    w.start();
    w.pass_ms(5_000);
    // I1: Ravi leaves.
    w.people[r].guest().leave();
    gives_back(&mut w, r, &mut alone, "left");
    // I2: Meera is taken out.
    let id = w.people[m].guest().me().unwrap();
    w.people[0].host().remove(id, false);
    w.pass_ms(5);
    gives_back(&mut w, m, &mut alone, "removed");
    // I3: the host ends it for everyone: Kabir and the host.
    w.people[0].host().end();
    w.pass_ms(5);
    gives_back(&mut w, k, &mut alone, "ended (Kabir)");
    gives_back(&mut w, 0, &mut alone, "ended (the host)");
    w.pass_ms(2_000);
    let heard = w.finish();
    let a = alone.finish();
    for (i, what) in [(r, "left"), (m, "removed"), (k, "ended"), (0, "the host")] {
        assert_same_as_alone(what, &heard[i], &a);
    }
    let _ = HOST;
}

/// While looking into a click: the notes and the ramp around each music-shaped flag.
#[allow(dead_code)]
fn explain(what: &str, h: &Heard) {
    let piece = audio_check::music::Piece {
        rate: R,
        channels: 1,
    };
    let (s1, s3) = piece.limits();
    let x24: Vec<i32> = h.left.iter().map(|v| v >> 8).collect();
    for c in audio_check::music::clicks(&x24, s1, s3) {
        eprintln!(
            "{what}: {c:?}; ramp {:?}",
            (c.at.saturating_sub(3)..c.at + 3)
                .map(|k| h.count(k))
                .collect::<Vec<_>>()
        );
        for n in &h.notes {
            let at = match *n {
                RenderNote::Started { at, .. }
                | RenderNote::Joined { at, .. }
                | RenderNote::Ended { at, .. }
                | RenderNote::Silent { at, .. }
                | RenderNote::Starved { at, .. }
                | RenderNote::Late { at, .. } => at,
                RenderNote::Faded { from, .. } | RenderNote::Gain { from, .. } => from,
                RenderNote::Resampling { at, .. } => at,
                _ => continue,
            };
            if (at as i64 - c.at as i64).abs() < 96_000 {
                eprintln!("    {n:?}");
            }
        }
    }
}
