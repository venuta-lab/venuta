//! WP11's gates with the real analysis (design 4.5 and its WP11 notes): a guest's copy measured
//! against the host's reference in a whole room on the simulated network (the host's session
//! shelving its reference, the `env:` stream, the guest's session fetching it), with the
//! player's own envelope maker and `analysis::align_track` on WP10's real-shaped encodes (MP3
//! with and without its gapless header, AAC, Opus, all of the synthesizer's song); and a
//! refined offset on a real engine 2 player, taken up by rate with nothing heard.

mod align_kit;
mod room_kit;

use player::measure::{Analysis, EnvelopeMaker, Making};
use player::sim::SimConfig;
use player::voice_api::{FadeKind, RenderNote};
use room_kit::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use together::align::{Alignment, CopyClass};
use together::anchor::{EntryId, PrepId};
use together::clock_map::{self, ClockMap};
use together::conductor::{Conductor, Copies, Copy, Lookup};
use together::fakes::{Call, FakeRoomPlayer, Script};
use together::measure::{Measured, References, Shelf};
use together::proto::Song;
use together::proto::v3::Action;
use together::room::v2::{Opening, Room};
use together::room_player::{ListenMode, RoomPlayer, SyncMode};
use together::session::{GuestConfig, GuestSession, HostConfig, HostSession, Stage};
use together::sim_net::{SimNet, Way};
use together::time::{MonoClock, SimClock, Timebase};
use together::transport::{Get, MediaServer, PeerId};
use together::view::CopyKind;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

/// One person's one copy of the song: this file, as this kind.
struct OneCopy {
    file: PathBuf,
    kind: CopyKind,
}

impl Copies<PathBuf> for OneCopy {
    fn find(&mut self, _: &Song) -> Lookup<PathBuf> {
        Lookup::Found(Copy {
            item: self.file.clone(),
            align: Alignment::APPROXIMATE,
            kind: self.kind,
            length_ms: None,
        })
    }
}

/// The host serves no files here: only its references.
struct NoFiles;

impl MediaServer for NoFiles {
    fn open(&self, _: PeerId, _: &Get) -> Option<Arc<[u8]>> {
        None
    }
}

/// The room's song: the synthesizer's 20 s piece.
fn the_song() -> Song {
    Song {
        key: "online:alignmusic1".into(),
        title: "Align music".into(),
        artists: vec!["Synthesizer".into()],
        album: None,
        duration_ms: Some((align_kit::FIXTURE_SECS * 1000.0) as u64),
        isrc: None,
        mbid: None,
        online_id: Some("alignmusic1".into()),
        host_file: false,
        format: None,
        artwork_url: None,
        added_by: None,
    }
}

/// The host's reference: the synthesizer's song, lossless, at 44.1 kHz mono (the lossy
/// fixtures were encoded from the same).
fn reference_wav() -> PathBuf {
    let m = align_kit::Master {
        rate: 44_100,
        ch: 1,
        ..align_kit::REF
    };
    let pcm = align_kit::render(
        &align_kit::song(align_kit::FIXTURE_SEED, align_kit::FIXTURE_SECS),
        align_kit::FIXTURE_SECS,
        m,
    );
    let dir = std::env::temp_dir().join(format!("venuta-room-align-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("reference.wav");
    align_kit::write_wav(&f, &pcm, m.rate, m.ch);
    f
}

/// A room of two on one Wi-Fi in simulated time: the host Asha plays `reference`, the guest
/// Ravi has `copy` (of `kind`). Both measures are real (the player's envelope maker, inline,
/// and its analysis); the players are fakes. The alignment of Ravi's first prepare.
fn measured(reference: &Path, copy: &Path, kind: CopyKind) -> Alignment {
    let clock = SimClock::new(1_000_000_000);
    let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
    let net = SimNet::new(tb, 7);
    let mono: Arc<dyn MonoClock> = Arc::new(clock.clone());
    let ht = net.peer(mono.clone());
    let gt = net.peer(mono.clone());
    let (hp, _) = FakeRoomPlayer::<PathBuf>::new(mono.clone(), Script::default());
    let hp: Arc<dyn RoomPlayer<PathBuf>> = Arc::new(hp);
    let map = clock_map::handle();
    map.store(Arc::new(Some(ClockMap::identity())));
    let shelf = Shelf::new();
    let copies = Measured::host(
        Box::new(OneCopy {
            file: reference.to_path_buf(),
            kind: CopyKind::OwnFile,
        }),
        Box::new(EnvelopeMaker::<PathBuf>::new(None, Making::Inline)),
        shelf.clone(),
    );
    let handle = hp.attach(map.clone(), SyncMode::Apart);
    let conductor =
        Conductor::new(hp, map, Box::new(copies), ListenMode::Apart).with_handle(handle, None);
    let mut room = Room::new("Asha", SECRET, vec![the_song()], Opening::Resting);
    room.set_let_in_without_asking(true);
    let mut host = HostSession::new(
        Arc::new(ht.clone()),
        mono.clone(),
        room,
        conductor,
        HostConfig {
            invite_open: Arc::new(|| true),
        },
    )
    .with_envelopes(shelf);
    net.serve(&ht, host.media(Arc::new(NoFiles)));
    let (gp, glog) = FakeRoomPlayer::<PathBuf>::new(mono.clone(), Script::default());
    let copies = Measured::guest(
        Box::new(OneCopy {
            file: copy.to_path_buf(),
            kind,
        }),
        Box::new(EnvelopeMaker::<PathBuf>::new(None, Making::Inline)),
        References::new(Arc::new(gt.clone()), ht.id(), None),
        Arc::new(Analysis),
    );
    let conductor = Conductor::new(
        Arc::new(gp),
        clock_map::handle(),
        Box::new(copies),
        ListenMode::Apart,
    );
    let key = format!("peer{}", gt.id().0);
    let mut guest = GuestSession::new(
        Arc::new(gt.clone()),
        ht.id(),
        mono,
        conductor,
        GuestConfig {
            name: "Ravi".into(),
            proof: together::invite::proof(SECRET, &key),
            app: "Venuta test".into(),
            host_name: "Asha".into(),
            mode: Some(ListenMode::Apart),
        },
    );
    net.connect(&gt, &ht, Way::LAN, Way::LAN);
    let step = |host: &mut HostSession<PathBuf>, guest: &mut GuestSession<PathBuf>| {
        clock.run_until(clock.now_ns() + 5_000_000);
        host.step();
        guest.step();
    };
    for _ in 0..1_000 {
        if guest.stage() == Stage::In && guest.clock_ready() {
            break;
        }
        step(&mut host, &mut guest);
    }
    assert_eq!(guest.stage(), Stage::In);
    host.ask(Action::Play).unwrap();
    for _ in 0..1_000 {
        let prepared = glog.just_calls().into_iter().find_map(|c| match c {
            Call::Prepare { align, .. } => Some(align),
            _ => None,
        });
        if let Some(a) = prepared {
            return a;
        }
        step(&mut host, &mut guest);
    }
    panic!("Ravi never prepared");
}

#[test]
fn e3_e4_real_encodes_are_measured_within_10_ms_of_their_codec_delay() {
    let reference = reference_wav();
    for (name, kind) in [
        ("align-music.mp3", CopyKind::OwnFile),
        ("align-music-raw.mp3", CopyKind::OwnFile),
        ("align-music.m4a", CopyKind::Catalog),
        ("align-music.opus", CopyKind::Catalog),
    ] {
        let path = align_kit::fixture(name);
        let (rate, ch, pcm) = align_kit::decode_all(&path);
        let truth_s = align_kit::true_delay(&pcm, ch, rate) as f64 / f64::from(rate);
        let a = measured(&reference, &path, kind);
        let e = align_kit::err_ms(a.offset_us, truth_s);
        println!(
            "{name}: codec delay {:.2} ms, measured in the room {:.2} ms, off by {e:.3} ms, {:?}",
            truth_s * 1e3,
            a.offset_us as f64 / 1e3,
            a.class
        );
        assert_eq!(a.class, CopyClass::Measured, "{name}");
        assert!(e <= 10.0, "{name}: off by {e:.3} ms");
    }
}

#[test]
fn e2_the_same_decoded_audio_is_exact() {
    let reference = reference_wav();
    let copy = reference.with_file_name("my-copy.wav");
    std::fs::copy(&reference, &copy).unwrap();
    let a = measured(&reference, &copy, CopyKind::OwnFile);
    assert_eq!(a, Alignment::EXACT);
}

#[test]
fn a_refined_offset_is_taken_up_by_rate_and_never_heard() {
    let clock = SimClock::new(1_000_000_000);
    let mut p = Person::<PathBuf>::new(&clock, SimConfig::default());
    let host = HostTime::same();
    let h = RoomPlayer::attach(p.player(), host.exact_map(), SyncMode::EveryDevice);
    let f = room_kit::song(0.0, 50.0, BASES[0], true, true);
    let measured = Alignment {
        offset_us: 0,
        class: CopyClass::Measured,
        confidence: 15.0,
    };
    h.prepare(PrepId(1), EntryId(1), f, 0, measured);
    pass(&clock, 300, &mut []);
    let a = anchor(1, 0, host_now(&clock) + 300_000);
    let mut tl = Timeline::default();
    tl.set(a.host_us, a, BASES[0]);
    h.commit(a);
    for _ in 0..100 {
        pass(&clock, 100, &mut []);
        p.collect();
    }
    // A later window measured my copy 8 ms later than the start did.
    let t = clock.now_ns() as f64;
    h.refine(
        EntryId(1),
        Alignment {
            offset_us: 8_000,
            ..measured
        },
    );
    let mut most_ppm: f64 = 0.0;
    for _ in 0..300 {
        pass(&clock, 100, &mut []);
        p.collect();
        most_ppm = most_ppm.max(h.status().ppm.abs());
    }
    let st = h.status();
    h.detach();
    pass(&clock, 1_500, &mut []);
    let heard = p.finish();
    let before = errors(&heard, &tl, &host, t - 5e9, t);
    let after = errors(&heard, &tl, &host, t + 25e9, t + 30e9);
    let mean = |e: &[(f64, f64)]| e.iter().map(|x| x.1).sum::<f64>() / e.len() as f64;
    println!(
        "before {:.3} ms, 25 s after {:.3} ms, the trim at most {most_ppm:.0} ppm, now {:.1} ppm",
        mean(&before),
        mean(&after),
        st.ppm
    );
    assert!(mean(&before).abs() < 0.5);
    assert!(
        (mean(&after) - 8.0).abs() < 1.0,
        "the 8 ms taken up by rate: {:.3} ms",
        mean(&after)
    );
    assert!(most_ppm <= 500.0);
    assert!(
        st.ppm.abs() < 30.0,
        "nothing learned from the move: {} ppm",
        st.ppm
    );
    let crossfades = count(&heard.notes, |n| {
        matches!(
            n,
            RenderNote::Faded {
                kind: FadeKind::CrossIn,
                ..
            }
        )
    });
    assert_eq!(crossfades, 0, "never a realign for alignment alone");
    assert_music_clean("a refined offset", &heard);
}
