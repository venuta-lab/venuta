//! WP18's gates with a real engine 2 host (design 4.13, 7.8 and the WP18 notes): a room on one
//! Wi-Fi in simulated time whose every guest is a remote. The host's player leads: its output
//! is bit-identical to the source frames wherever the renderer declares nothing, its trim is 0
//! at every tick, whatever the remotes press; the room re-anchors on where the host's sound
//! card really is, and each remote's progress line stays within 100 ms of the host's heard
//! place, for ten minutes too. The remotes attach no player (their players are fakes that
//! record nothing but would show any call).

mod room_kit;

use player::engine::{Cmd, Player};
use player::sim::SimConfig;
use player::voice_api::RenderNote;
use room_kit::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use together::align::Alignment;
use together::anchor::{Anchor, EntryId};
use together::clock_map::{self, ClockMap};
use together::conductor::{Conductor, Copies, Copy, Lookup};
use together::fakes::{FakeLog, FakeRoomPlayer, Script};
use together::proto::Song;
use together::proto::v3::{Action, Phase};
use together::room::v2::{Opening, Room};
use together::room_player::{ListenMode, RoomHandle, RoomPlayer, SyncMode};
use together::session::{GuestConfig, GuestSession, HostConfig, HostSession, Stage};
use together::sim_net::{SimNet, SimTransport, Way};
use together::time::{MonoClock, SimClock, Timebase};
use together::view::CopyKind;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

/// A test's engine 2, lent to the conductor.
struct Seat(Arc<Mutex<Option<Player<PathBuf>>>>);

impl RoomPlayer<PathBuf> for Seat {
    fn attach(
        &self,
        clock: clock_map::ClockMapHandle,
        mode: SyncMode,
    ) -> Box<dyn RoomHandle<PathBuf>> {
        let p = self.0.lock().unwrap();
        RoomPlayer::attach(p.as_ref().expect("a player"), clock, mode)
    }
}

/// The host has every song's file: its own, the room's reference.
struct Files(HashMap<String, PathBuf>);

impl Copies<PathBuf> for Files {
    fn find(&mut self, song: &Song) -> Lookup<PathBuf> {
        match self.0.get(&song.key) {
            Some(f) => Lookup::Found(Copy {
                item: f.clone(),
                align: Alignment::EXACT,
                kind: CopyKind::OwnFile,
                length_ms: None,
            }),
            None => Lookup::NoCopy,
        }
    }
}

fn room_song(k: usize, secs: f64) -> Song {
    Song {
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
    }
}

struct Remote {
    s: GuestSession<PathBuf>,
    log: FakeLog<PathBuf>,
}

/// The host Asha on a real player (its sound card `ppm` off), with these songs, and remotes.
struct Speaker {
    clock: SimClock,
    net: Arc<SimNet>,
    host_t: SimTransport,
    host: HostSession<PathBuf>,
    p: Person<PathBuf>,
    seat: Arc<Mutex<Option<Player<PathBuf>>>>,
    remotes: Vec<Remote>,
    /// The host's trim, at every step: never anything but 0.
    worst_ppm: f64,
}

impl Speaker {
    fn new(files: Vec<PathBuf>, secs: f64, cfg: SimConfig) -> Speaker {
        let clock = SimClock::new(1_000_000_000);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let net = SimNet::new(tb, 5);
        let mono: Arc<dyn MonoClock> = Arc::new(clock.clone());
        let host_t = net.peer(mono.clone());
        let mut p = Person::new(&clock, cfg);
        let seat = Arc::new(Mutex::new(p.player.take()));
        let map = clock_map::handle();
        map.store(Arc::new(Some(ClockMap::identity())));
        let player: Arc<dyn RoomPlayer<PathBuf>> = Arc::new(Seat(seat.clone()));
        let handle = player.attach(map.clone(), SyncMode::Apart);
        let songs: Vec<Song> = (0..files.len()).map(|k| room_song(k, secs)).collect();
        let copies = Files(
            songs
                .iter()
                .map(|s| s.key.clone())
                .zip(files.iter().cloned())
                .collect(),
        );
        let conductor = Conductor::new(player, map, Box::new(copies), ListenMode::Apart)
            .with_handle(handle, None);
        let mut room = Room::new("Asha", SECRET, songs, Opening::Resting);
        room.set_let_in_without_asking(true);
        let host = HostSession::new(
            Arc::new(host_t.clone()),
            mono,
            room,
            conductor,
            HostConfig {
                invite_open: Arc::new(|| true),
            },
        );
        let mut s = Speaker {
            clock,
            net,
            host_t,
            host,
            p,
            seat,
            remotes: Vec::new(),
            worst_ppm: 0.0,
        };
        s.pass_ms(20);
        s
    }

    /// A remote on the host's speaker joins over one Wi-Fi.
    fn remote(&mut self, name: &str) -> usize {
        let mono: Arc<dyn MonoClock> = Arc::new(self.clock.clone());
        let t = self.net.peer(mono.clone());
        let (fake, log) = FakeRoomPlayer::<PathBuf>::new(mono.clone(), Script::default());
        let conductor = Conductor::new(
            Arc::new(fake),
            clock_map::handle(),
            Box::new(Files(HashMap::new())),
            ListenMode::OneSpeaker,
        );
        let key = format!("peer{}", t.id().0);
        let s = GuestSession::new(
            Arc::new(t.clone()),
            self.host_t.id(),
            mono,
            conductor,
            GuestConfig {
                name: name.into(),
                proof: together::invite::proof(SECRET, &key),
                app: "Venuta test".into(),
                host_name: "Asha".into(),
                mode: Some(ListenMode::OneSpeaker),
            },
        );
        self.net.connect(&t, &self.host_t, Way::LAN, Way::LAN);
        self.remotes.push(Remote { s, log });
        let i = self.remotes.len() - 1;
        assert!(self.run_until(
            |w| w.remotes[i].s.stage() == Stage::In && w.remotes[i].s.clock_ready(),
            5_000
        ));
        i
    }

    fn step(&mut self, ms: u64) {
        pass(&self.clock, ms, &mut []);
        self.host.step();
        for r in &mut self.remotes {
            r.s.step();
        }
        // The app's part: a volume a remote set goes to the host's player (ramped).
        if let Some(v) = self.host.take_volume()
            && let Some(p) = self.seat.lock().unwrap().as_ref()
        {
            p.send(Cmd::Volume(f32::from(v) / 100.0));
        }
        if let Some(st) = self.host.conductor().status() {
            self.worst_ppm = self.worst_ppm.max(st.ppm.abs());
        }
        self.p.collect();
    }

    fn pass_ms(&mut self, ms: u64) {
        let until = self.clock.now_ns() + ms * 1_000_000;
        while self.clock.now_ns() < until {
            self.step(5);
        }
    }

    fn run_until(&mut self, done: impl Fn(&mut Speaker) -> bool, max_ms: u64) -> bool {
        let until = self.clock.now_ns() + max_ms * 1_000_000;
        while self.clock.now_ns() < until {
            if done(self) {
                return true;
            }
            self.step(5);
        }
        done(self)
    }

    /// Where the host's speaker is heard now (the room's content, host µs now), by its player.
    fn heard_now(&mut self) -> Option<(EntryId, i64)> {
        let h: Anchor = self.host.conductor().status()?.heard?;
        let now = (self.clock.now_ns() / 1_000) as i64;
        Some((h.entry, h.content_us + (now - h.host_us)))
    }

    /// A remote's progress line now: the anchor in its view at its estimate of the host's time.
    fn progress(&mut self, i: usize) -> Option<(EntryId, i64)> {
        let v = self.remotes[i].s.window(&Default::default());
        let a = v.room?.anchor?;
        Some((a.entry, a.content_at(v.host_now_us?)))
    }

    /// The remote's progress against the host's heard place, ms (while both say the same song).
    fn progress_off_ms(&mut self, i: usize) -> Option<f64> {
        let (e, heard) = self.heard_now()?;
        let (pe, p) = self.progress(i)?;
        (e == pe).then(|| (p - heard) as f64 / 1_000.0)
    }

    fn finish(&mut self) -> Heard {
        for r in &self.remotes {
            assert!(r.log.just_calls().is_empty(), "a remote attaches no player");
        }
        self.remotes.clear();
        self.host.end();
        self.pass_ms(1_500);
        self.p.player = self.seat.lock().unwrap().take();
        self.p.finish()
    }
}

#[test]
fn j7_a19_the_hosts_speaker_is_solo_playback_bit_for_bit_whatever_the_remotes_press() {
    // A gapless album of three on a sound card 300 ppm fast.
    let secs = 20.0;
    let files: Vec<PathBuf> = (0..3)
        .map(|k| song(k as f64 * secs, secs, BASES[k], k == 0, k == 2))
        .collect();
    let mut w = Speaker::new(
        files,
        secs,
        SimConfig {
            skew_ppm: 300.0,
            ..SimConfig::default()
        },
    );
    let r = w.remote("Ravi");
    w.remote("Meera");
    w.host.act(|room, now| room.set_guests_volume(true, now));
    let press = |w: &mut Speaker, a: Action| {
        w.remotes[r].s.ask(a);
        w.pass_ms(1_500);
    };
    press(&mut w, Action::Play);
    assert!(w.run_until(|w| w.host.room().phase() == Phase::Playing, 3_000));
    let mut off: Vec<f64> = Vec::new();
    let mut watch = |w: &mut Speaker, ms: u64| {
        let until = w.clock.now_ns() + ms * 1_000_000;
        while w.clock.now_ns() < until {
            w.pass_ms(100);
            if w.host.room().preparing().is_none()
                && let Some(x) = w.progress_off_ms(0)
            {
                off.push(x);
            }
        }
    };
    watch(&mut w, 5_000);
    press(
        &mut w,
        Action::Seek {
            content_us: 9_000_000,
        },
    );
    watch(&mut w, 4_000);
    press(&mut w, Action::Pause);
    press(&mut w, Action::Play);
    watch(&mut w, 3_000);
    press(&mut w, Action::Volume { percent: 40 });
    press(&mut w, Action::Volume { percent: 100 });
    // On through the gapless boundary into the second song, then a skip to the third.
    watch(&mut w, 8_000);
    press(&mut w, Action::Next);
    watch(&mut w, 22_000);
    let worst_ppm = w.worst_ppm;
    let heard = w.finish();
    let worst_off = off.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    eprintln!(
        "the host's trim at most {worst_ppm} ppm; a remote's progress at most {worst_off:.1} ms from the heard place ({} samples)",
        off.len()
    );
    assert_eq!(worst_ppm, 0.0, "never a rate change");
    assert!(
        !has(&heard.notes, |n| matches!(
            n,
            RenderNote::Resampling { on: true, .. }
        )),
        "never resampled"
    );
    assert!(worst_off < 100.0, "{worst_off} ms");
    let source = music_reference(0.0, 3.0 * secs);
    assert_exact_where_undeclared("the one-speaker host", &heard, &source);
}

#[test]
fn j7_a_remotes_progress_stays_with_the_hosts_real_speaker_for_ten_minutes() {
    // Three 205 s songs (a sine each: the place is what is judged here), the card 300 ppm
    // fast: free running, the host drifts 18 ms a minute from any clock; the tape is off (the
    // run judges as it goes).
    let secs = 205.0;
    let files: Vec<PathBuf> = [440.0, 550.0, 660.0]
        .into_iter()
        .map(|hz| sine_song(hz, secs))
        .collect();
    let mut w = Speaker::new(
        files.clone(),
        secs,
        SimConfig {
            skew_ppm: 300.0,
            keep_tape: false,
            ..SimConfig::default()
        },
    );
    w.remote("Ravi");
    w.host.ask(Action::Play).unwrap();
    assert!(w.run_until(|w| w.host.room().phase() == Phase::Playing, 3_000));
    w.pass_ms(1_000);
    let mut worst = 0.0f64;
    let mut seqs = std::collections::BTreeSet::new();
    for _ in 0..6_000 {
        for _ in 0..10 {
            w.step(10);
        }
        if let Some(x) = w.progress_off_ms(0) {
            worst = worst.max(x.abs());
        }
        if let Some(a) = w.host.room().anchor() {
            seqs.insert(a.seq);
        }
    }
    let worst_ppm = w.worst_ppm;
    eprintln!(
        "ten minutes: a remote's progress at most {worst:.1} ms from the host's heard place, {} anchors, the trim at most {worst_ppm} ppm",
        seqs.len()
    );
    assert!(worst < 100.0, "{worst} ms (and so within 1 s)");
    assert!(seqs.len() >= 10, "re-anchored as the card drifts: {seqs:?}");
    assert_eq!(worst_ppm, 0.0);
    w.finish();
    for f in files {
        let _ = std::fs::remove_file(f);
    }
}
