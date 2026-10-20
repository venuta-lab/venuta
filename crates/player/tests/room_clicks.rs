//! Not one click, in or out of a room: music-shaped songs (see `common`) played through a device
//! that takes sound at real-time pace, and every frame that reaches it listened to by an ear that
//! knows how steep and how bent the music can be. Normal playing; a room in step, apart and in
//! one room; a sound card running fast or slow against the computer (as all do, a little), both
//! ways; the room's clock estimate wobbling and arriving late; joining partway into a song;
//! another output mid-song; pause, resume, seek and skip as a room does them; one song into the
//! next at the very frame; the volume moved; songs at 44.1, 48 and 96 kHz; and the room ending,
//! then your own music, frame for frame as it plays alone. The device must never run dry.

mod common;

use common::{Piece, all_clicks, channels, song};
use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::{SyncNote, SyncPlan};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn plan(anchor: Option<Instant>, position_ms: u64, playing: bool, same_room: bool) -> SyncPlan {
    SyncPlan {
        anchor,
        position_ms,
        playing,
        same_room,
        extra_latency_ms: 0,
    }
}

fn shifted(a: Instant, ms: f64) -> Instant {
    if ms >= 0.0 {
        a + Duration::from_secs_f64(ms / 1000.0)
    } else {
        a - Duration::from_secs_f64(-ms / 1000.0)
    }
}

/// A small steady pseudo-random number in -1..1.
fn noise(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0
}

struct Run {
    player: Player<PathBuf>,
    captured: Arc<Mutex<Captured>>,
    notes: Arc<Mutex<Vec<SyncNote>>>,
    last: Arc<Mutex<Option<Status<PathBuf>>>>,
    moved: std::sync::mpsc::Receiver<Option<usize>>,
}

impl Run {
    /// A player on a paced device whose clock runs `drift_ppm` fast (negative: slow).
    fn new(drift_ppm: f64) -> Self {
        let captured = Arc::new(Mutex::new(Captured {
            drift_ppm,
            ..Captured::default()
        }));
        let notes = Arc::new(Mutex::new(Vec::new()));
        let last = Arc::new(Mutex::new(None));
        let (noted, seen) = (notes.clone(), last.clone());
        let (tx, moved) = std::sync::mpsc::channel();
        let player = Player::spawn(
            OutputChoice::PacedMemory(captured.clone()),
            move |s: Status<PathBuf>| {
                noted.lock().unwrap().extend(s.sync_notes.iter().cloned());
                let _ = tx.send(s.index);
                *seen.lock().unwrap() = Some(s);
            },
        );
        Run {
            player,
            captured,
            notes,
            last,
            moved,
        }
    }

    fn send(&self, c: Cmd<PathBuf>) {
        self.player.send(c);
    }

    /// A song alone in the player, ready at `ms` (as a room's follower loads it).
    fn load(&self, f: &Path, ms: u64, play: bool) {
        self.send(Cmd::Set {
            items: vec![f.to_path_buf()],
            index: 0,
            start_ms: ms,
            play,
            from: String::new(),
        });
    }

    fn status(&self) -> Status<PathBuf> {
        self.last.lock().unwrap().clone().expect("a status")
    }

    /// Wait until the player is on queue entry `i` (or a while passes).
    fn wait_for_index(&self, i: usize, within: Duration) -> bool {
        let t0 = Instant::now();
        while t0.elapsed() < within {
            if let Ok(Some(k)) = self.moved.recv_timeout(Duration::from_millis(5))
                && k == i
            {
                return true;
            }
        }
        false
    }

    /// Stop, and what was heard: (the capture, what the player did for a room, how many times the
    /// device ran dry while music played).
    fn finish(self) -> (Captured, Vec<SyncNote>, u64) {
        let dry = self.last.lock().unwrap().as_ref().map_or(0, |s| s.ran_dry);
        drop(self.player);
        sleep(30);
        let c = std::mem::take(&mut *self.captured.lock().unwrap());
        let n = self.notes.lock().unwrap().clone();
        (c, n, dry)
    }
}

/// The checks every run ends with: no click, the device never dry, and music was heard.
fn clean(name: &str, piece: &Piece, c: &Captured, dry: u64, at_least_s: f64) {
    let (s1, s2) = piece.limits();
    let ch = usize::from(piece.channels);
    let found = all_clicks(c, ch, s1, s2);
    let around = found.first().map(|k| {
        let x = &channels(c, ch)[0];
        x[k.at.saturating_sub(6)..(k.at + 6).min(x.len())].to_vec()
    });
    assert!(
        found.is_empty(),
        "{name}: {} clicks: {:?}, around the first: {around:?}",
        found.len(),
        &found[..found.len().min(5)]
    );
    assert_eq!(dry, 0, "{name}: the device ran dry");
    let heard = channels(c, ch)[0].iter().filter(|&&v| v != 0).count();
    assert!(
        heard as f64 >= f64::from(piece.rate) * at_least_s,
        "{name}: music heard ({heard} frames)"
    );
}

fn stereo(rate: u32) -> Piece {
    Piece { rate, channels: 2 }
}

#[test]
fn the_ear_hears_every_kind_of_click() {
    // Clean music first: nothing.
    let piece = stereo(48_000);
    let (s1, s2) = piece.limits();
    let clean_x: Vec<i32> = (0..96_000)
        .map(|i| piece.sample(i, 0, 0, 96_000, true, true))
        .collect();
    assert_eq!(common::clicks(&clean_x, s1, s2), []);
    // Then each fault, at a dozen places: every one heard.
    type Fault = fn(&mut Vec<i32>, usize);
    let faults: [(&str, Fault); 6] = [
        ("a frame dropped", |x, i| {
            x.remove(i);
        }),
        ("a frame repeated", |x, i| x.insert(i, x[i])),
        ("5 ms of silence cut in", |x, i| {
            x[i..i + 240].iter_mut().for_each(|v| *v = 0)
        }),
        ("a hard cut 300 ms on", |x, i| {
            x.drain(i..i + 14_400);
        }),
        ("the volume stepped down", |x, i| {
            x[i..].iter_mut().for_each(|v| *v /= 2)
        }),
        ("one sample out of line", |x, i| {
            x[i] = x[i].saturating_add(400_000)
        }),
    ];
    for (what, fault) in faults {
        let mut missed = 0;
        for k in 0..12 {
            let mut x = clean_x.clone();
            // Spread over the song, at moments of every steepness.
            fault(&mut x, 5_000 + k * 2_731);
            if common::clicks(&x, s1, s2).is_empty() {
                missed += 1;
            }
        }
        // A frame dropped where the wave is nearly flat changes almost nothing (and is heard as
        // almost nothing): those may pass; every other fault is always heard.
        let allowed = if what.starts_with("a frame") { 3 } else { 0 };
        assert!(missed <= allowed, "{what}: missed {missed} of 12");
    }
}

#[test]
fn normal_play_is_clean_at_every_rate_and_the_volume_glides() {
    for rate in [44_100, 48_000, 96_000] {
        let piece = stereo(rate);
        let a = song(&format!("normal-{rate}"), &piece, 0.0, 4.0, true, true);
        let r = Run::new(0.0);
        r.load(&a, 0, true);
        sleep(1_200);
        // The volume moved, twice, and back up.
        r.send(Cmd::Volume(0.4));
        sleep(500);
        r.send(Cmd::Volume(0.9));
        sleep(500);
        r.send(Cmd::Volume(1.0));
        sleep(1_200);
        let (c, notes, dry) = r.finish();
        clean(&format!("{rate} Hz"), &piece, &c, dry, 3.0);
        assert_eq!(notes, [], "nothing done for a room");
    }
}

/// A room's song played from its start at `start`, with `plans` (time from the start, ms off the
/// true start) re-sent as a follower does when the host's clock estimate moves, on a device
/// drifting `drift_ppm`: what was heard.
fn room(
    name: &str,
    piece: &Piece,
    same_room: bool,
    drift_ppm: f64,
    secs: f64,
    plans: &[(u64, f64)],
) -> (Captured, Vec<SyncNote>, u64, Status<PathBuf>) {
    let a = song(name, piece, 0.0, secs + 2.0, true, true);
    let r = Run::new(drift_ppm);
    r.load(&a, 0, false);
    r.send(Cmd::Sync(Some(plan(None, 0, true, same_room))));
    sleep(300);
    let start = Instant::now() + Duration::from_millis(400);
    r.send(Cmd::Sync(Some(plan(Some(start), 0, true, same_room))));
    let t0 = Instant::now();
    for &(at, off) in plans {
        let wait = Duration::from_millis(at).saturating_sub(t0.elapsed());
        std::thread::sleep(wait);
        r.send(Cmd::Sync(Some(plan(
            Some(shifted(start, off)),
            0,
            true,
            same_room,
        ))));
    }
    let left = Duration::from_secs_f64(secs).saturating_sub(t0.elapsed());
    std::thread::sleep(left);
    let status = r.status();
    let (c, notes, dry) = r.finish();
    (c, notes, dry, status)
}

#[test]
fn a_room_in_step_is_clean_apart_and_in_one_room() {
    let piece = stereo(48_000);
    for same_room in [false, true] {
        let (c, notes, dry, _) = room(
            &format!("step-{same_room}"),
            &piece,
            same_room,
            0.0,
            4.0,
            &[],
        );
        clean(
            &format!("in step, same room {same_room}"),
            &piece,
            &c,
            dry,
            3.5,
        );
        assert!(
            !notes.iter().any(|n| matches!(n, SyncNote::Seek { .. })),
            "nothing to line up: {notes:?}"
        );
    }
}

#[test]
fn a_wobbling_or_late_clock_estimate_is_never_heard() {
    let piece = stereo(48_000);
    let mut seed = 5;
    // Apart: a wobble of up to 40 ms every 400 ms, then a step of 150 ms (a new path to the
    // host), late: none of it worth touching the music for.
    let mut plans: Vec<(u64, f64)> = (1..=10)
        .map(|k| (k * 400, noise(&mut seed) * 40.0))
        .collect();
    plans.push((4_400, 150.0));
    let (c, notes, dry, _) = room("wobble-apart", &piece, false, 0.0, 6.0, &plans);
    clean("apart, wobbling", &piece, &c, dry, 5.0);
    assert!(
        !notes.iter().any(|n| matches!(n, SyncNote::Seek { .. })),
        "apart, nothing moved: {notes:?}"
    );
    // One room: a wobble of 1.5 ms every 300 ms (the best a network gives), and one of 8 ms.
    let mut plans: Vec<(u64, f64)> = (1..=15)
        .map(|k| (k * 300, noise(&mut seed) * 1.5))
        .collect();
    plans.push((4_800, 8.0));
    let (c, _, dry, _) = room("wobble-same", &piece, true, 0.0, 7.0, &plans);
    clean("one room, wobbling", &piece, &c, dry, 6.0);
}

#[test]
fn a_sound_card_running_fast_or_slow_is_followed_without_a_sound() {
    // One room, a sound card 300 ppm fast and one 300 ppm slow (worse than most): the rate is
    // eased the other way, within 500 ppm, and the music stays with the room.
    let piece = stereo(48_000);
    for drift in [300.0, -300.0] {
        let (c, notes, dry, status) =
            room(&format!("drift-{drift}"), &piece, true, drift, 12.0, &[]);
        clean(
            &format!("one room, card {drift} ppm"),
            &piece,
            &c,
            dry,
            11.0,
        );
        let rates: Vec<i32> = notes
            .iter()
            .filter_map(|n| match n {
                SyncNote::Rate { ppm } => Some(*ppm),
                _ => None,
            })
            .collect();
        assert!(
            rates.iter().all(|p| p.abs() <= 500),
            "within 500 ppm: {rates:?}"
        );
        // A fast card plays ahead: the music is slowed (and the other way round).
        assert!(
            rates.iter().any(|&p| p * drift.signum() as i32 <= -100),
            "{drift} ppm: eased the other way: {rates:?}"
        );
        assert!(
            !notes.iter().any(|n| matches!(n, SyncNote::Seek { .. })),
            "never a seek: {notes:?}"
        );
        let e = status.sync.and_then(|s| s.error_ms).expect("how far off");
        assert!(e.abs() < 3.0, "{drift} ppm: with the room ({e} ms)");
    }
    // Apart, a card far off (0.5%: 300 ms in a minute, compressed here into seconds by the room
    // being suddenly 900 ms on): lined up once, faded out and in, never a click.
    let (c, notes, dry, _) = room("far-apart", &piece, false, 0.0, 7.0, &[(1_500, -900.0)]);
    clean("apart, far off", &piece, &c, dry, 5.0);
    let seeks = notes
        .iter()
        .filter(|n| matches!(n, SyncNote::Seek { faded: true, .. }))
        .count();
    assert_eq!(seeks, 1, "once, faded: {notes:?}");
}

#[test]
fn joining_partway_and_another_output_fade_in() {
    let piece = stereo(48_000);
    let a = song("join", &piece, 0.0, 12.0, true, true);
    // The room started this song 3 s ago.
    let r = Run::new(0.0);
    r.load(&a, 3_000, false);
    r.send(Cmd::Sync(Some(plan(
        Some(Instant::now() - Duration::from_secs(3)),
        0,
        true,
        false,
    ))));
    sleep(2_500);
    // Headphones chosen mid-song: the new output takes over from silence.
    let next = Arc::new(Mutex::new(Captured::default()));
    r.send(Cmd::Output(OutputChoice::PacedMemory(next.clone())));
    sleep(2_500);
    let (c, _, dry) = r.finish();
    clean("joined 3 s in", &piece, &c, dry, 1.5);
    let c2 = std::mem::take(&mut *next.lock().unwrap());
    clean("another output", &piece, &c2, 0, 1.5);
}

#[test]
fn pause_resume_seek_and_skip_in_a_room_are_clean() {
    let piece = stereo(48_000);
    let a = song("flow-a", &piece, 0.0, 20.0, true, true);
    let b = song("flow-b", &piece, 40.0, 20.0, true, true);
    let r = Run::new(0.0);
    let ready = |f: &Path, ms: u64| {
        r.load(f, ms, false);
        r.send(Cmd::Sync(Some(plan(None, ms, true, false))));
        sleep(400);
    };
    let go = |ms: u64| {
        let at = Instant::now() + Duration::from_millis(500);
        r.send(Cmd::Sync(Some(plan(Some(at), ms, true, false))));
        sleep(500);
        at
    };
    ready(&a, 0);
    go(0);
    sleep(1_500);
    // Paused at 1.5 s, resumed there (the song already in the player: readied in place).
    r.send(Cmd::Sync(Some(plan(
        Some(Instant::now()),
        1_500,
        false,
        false,
    ))));
    sleep(600);
    r.send(Cmd::Sync(Some(plan(None, 1_500, true, false))));
    sleep(400);
    go(1_500);
    sleep(1_500);
    // A seek to 10 s.
    r.send(Cmd::Sync(Some(plan(None, 10_000, true, false))));
    sleep(400);
    go(10_000);
    sleep(1_500);
    // Skipped to the next song.
    ready(&b, 0);
    go(0);
    sleep(1_500);
    let (c, _, dry) = r.finish();
    clean("pause, resume, seek, skip", &piece, &c, dry, 5.0);
}

#[test]
fn one_song_into_the_next_at_the_very_frame_is_clean() {
    // Two songs cut one after the other from the same music (a live album): any slip at the
    // seam would show. Apart and in one room (where a correction is under way as it nears).
    let piece = stereo(48_000);
    for same_room in [false, true] {
        let a = song(
            &format!("seam-a-{same_room}"),
            &piece,
            0.0,
            3.0,
            true,
            false,
        );
        let b = song(
            &format!("seam-b-{same_room}"),
            &piece,
            3.0,
            3.0,
            false,
            true,
        );
        let r = Run::new(if same_room { 400.0 } else { 0.0 });
        r.load(&a, 0, false);
        r.send(Cmd::Sync(Some(plan(None, 0, true, same_room))));
        sleep(300);
        let start = Instant::now() + Duration::from_millis(400);
        r.send(Cmd::Sync(Some(plan(Some(start), 0, true, same_room))));
        sleep(200);
        r.send(Cmd::Insert {
            items: vec![b],
            next: true,
        });
        assert!(r.wait_for_index(1, Duration::from_secs(6)), "moved on");
        // The follower's plan for the next song, a moment later.
        sleep(100);
        r.send(Cmd::Sync(Some(plan(
            Some(start + Duration::from_secs(3)),
            0,
            true,
            same_room,
        ))));
        sleep(1_500);
        let (c, _, dry) = r.finish();
        clean(
            &format!("the seam, same room {same_room}"),
            &piece,
            &c,
            dry,
            4.0,
        );
    }
}

#[test]
fn after_a_room_your_music_is_frame_for_frame_as_alone() {
    // Your own song played alone, as the reference.
    let piece = stereo(48_000);
    let own = song("own", &piece, 100.0, 6.0, true, true);
    let alone = {
        let r = Run::new(0.0);
        r.load(&own, 1_000, false);
        sleep(200);
        r.send(Cmd::Play);
        sleep(2_000);
        r.finish().0
    };
    let reference = channels(&alone, 2);
    let first = reference[0].iter().position(|&v| v != 0).unwrap();
    let n = 48_000;
    let expect: Vec<Vec<i32>> = reference
        .iter()
        .map(|x| x[first..first + n].to_vec())
        .collect();
    // The room each way it can be when it ends, then your song.
    let a = song("room-song", &piece, 0.0, 30.0, true, true);
    for (name, same_room, drift) in [("apart", false, 0.0), ("one room, correcting", true, 400.0)] {
        let r = Run::new(drift);
        r.load(&a, 0, false);
        r.send(Cmd::Sync(Some(plan(None, 0, true, same_room))));
        sleep(300);
        let start = Instant::now() + Duration::from_millis(300);
        r.send(Cmd::Sync(Some(plan(Some(start), 0, true, same_room))));
        sleep(4_000);
        // The room ends: on its own, your queue given back, paused, then play.
        r.send(Cmd::Sync(None));
        r.load(&own, 1_000, false);
        sleep(200);
        let from = r.captured.lock().unwrap().bytes.len();
        r.send(Cmd::Play);
        sleep(2_000);
        let (c, notes, dry) = r.finish();
        let after = Captured {
            bytes: c.bytes[from..].to_vec(),
            ..Captured::default()
        };
        let heard = channels(&after, 2);
        let at = heard[0]
            .iter()
            .position(|&v| v == expect[0][0])
            .expect("your song");
        for k in 0..2 {
            assert_eq!(
                heard[k][at..at + n],
                expect[k][..],
                "{name}: channel {k} frame for frame as alone"
            );
        }
        assert_eq!(dry, 0, "{name}: never dry");
        let _ = notes;
    }
}

#[test]
fn a_start_known_later_just_after_it_never_cuts_silence_into_the_music() {
    // Just after a song starts, a guest's estimate of the host's clock gets better and moves the
    // start 40 ms later (apart) or 8 ms (one room): the music plays on, no silence cut in.
    let piece = stereo(48_000);
    for (same_room, later, at) in [(false, 40.0, 415), (true, 8.0, 403)] {
        let (c, _, dry, _) = room(
            &format!("later-{same_room}"),
            &piece,
            same_room,
            0.0,
            3.0,
            &[(at, later)],
        );
        clean(
            &format!("started later, same room {same_room}"),
            &piece,
            &c,
            dry,
            2.0,
        );
    }
}
