//! After a room, the player is the player again: whatever the room was doing when it ended (a
//! song getting ready, frames being taken up in one room, a seek fading out, the song after
//! taking over, paused), your own music then plays sample for sample as it does alone, with no
//! plan, no correction and no silence. And a queue of your own never plays under an old room's
//! plan, even if the room's goodbye never reached the player.
//!
//! Songs are counting WAVs (each frame says which frame of which song it is) on a device that
//! takes sound at real-time pace, so the bytes that reach it show every frame dropped, repeated
//! or moved.

mod engines;
use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::{SyncNote, SyncPlan};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
/// Your own song's frames count from here (the room's from 0 and `NEXT`).
const OWN: i32 = -8_000_000;
const NEXT: i32 = -6_000_000;

fn wav(path: &Path, frames: usize, base: i32) {
    let data_len = frames * 3;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 3).to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    v.extend_from_slice(&24u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        v.extend_from_slice(&(base + i as i32 + 1).to_le_bytes()[..3]);
    }
    std::fs::write(path, v).unwrap();
}

/// A counting song of `secs` seconds whose frame i is `base + i + 1`.
fn counting(name: &str, secs: u32, base: i32) -> PathBuf {
    let d =
        player::test_support::temp_dir().join(format!("venuta-end-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    wav(&f, (RATE * secs) as usize, base);
    f
}

fn samples(bytes: &[u8]) -> Vec<i32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 8)
        .collect()
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

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

struct Run {
    player: Player<PathBuf>,
    captured: Arc<Mutex<Captured>>,
    last: Arc<Mutex<Option<Status<PathBuf>>>>,
    /// Everything the player said it did to stay with the room, as it said it.
    notes: Arc<Mutex<Vec<(Instant, SyncNote)>>>,
    moved: std::sync::mpsc::Receiver<Option<usize>>,
}

impl Run {
    fn new(room_song: &Path) -> Self {
        let captured = Arc::new(Mutex::new(Captured::default()));
        let last = Arc::new(Mutex::new(None));
        let seen = last.clone();
        let notes = Arc::new(Mutex::new(Vec::new()));
        let noted = notes.clone();
        let (tx, moved) = std::sync::mpsc::channel();
        let player = Player::spawn(
            OutputChoice::PacedMemory(captured.clone()),
            move |s: Status<PathBuf>| {
                let _ = tx.send(s.index);
                let now = Instant::now();
                noted
                    .lock()
                    .unwrap()
                    .extend(s.sync_notes.iter().map(|n| (now, n.clone())));
                *seen.lock().unwrap() = Some(s);
            },
        );
        // The room's song, as a follower loads it: alone, ready at its start.
        player.send(Cmd::Set {
            items: vec![room_song.to_path_buf()],
            index: 0,
            start_ms: 0,
            play: false,
            from: String::new(),
        });
        player.send(Cmd::Sync(Some(plan(None, 0, true, false))));
        sleep(100);
        Run {
            player,
            captured,
            last,
            notes,
            moved,
        }
    }

    fn send(&self, c: Cmd<PathBuf>) {
        self.player.send(c);
    }

    fn written(&self) -> usize {
        self.captured.lock().unwrap().bytes.len()
    }

    /// What came out from byte `from` on.
    fn heard_since(&self, from: usize) -> Vec<i32> {
        samples(&self.captured.lock().unwrap().bytes[from..])
    }

    /// What the player did for the room since `at`.
    fn notes_since(&self, at: Instant) -> Vec<SyncNote> {
        self.notes
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| *t >= at)
            .map(|(_, n)| n.clone())
            .collect()
    }

    fn status(&self) -> Status<PathBuf> {
        self.last.lock().unwrap().clone().expect("a status")
    }
}

/// How a room was when it ended.
#[derive(Debug, Clone, Copy)]
enum When {
    /// Its song readied silent, waiting for the room's start.
    WaitingForStart,
    /// Playing apart, in step.
    PlayingApart,
    /// Playing in one room, a little off, frames being taken up.
    TakingUpFrames,
    /// Far off in one room: a seek fading out.
    LiningUp,
    /// The song after just took over, the room's plan for it not here yet.
    NextSongTookOver,
    /// Paused by the room.
    Paused,
}

/// Run a room until it is `when`, end it as every way of ending does (the follower says the
/// player is on its own again, then gives your queue back, paused), then press play: what is
/// heard from the press on.
fn end_room(when: When) -> (Vec<i32>, Status<PathBuf>, Vec<SyncNote>) {
    let name = format!("{when:?}");
    let a = counting(&format!("{name}-a"), 4, 0);
    let own = counting(&format!("{name}-own"), 30, OWN);
    let r = Run::new(&a);
    let start = Instant::now() + Duration::from_millis(300);
    match when {
        When::WaitingForStart => sleep(500),
        When::PlayingApart => {
            r.send(Cmd::Sync(Some(plan(Some(start), 0, true, false))));
            sleep(1_500);
        }
        When::TakingUpFrames => {
            r.send(Cmd::Sync(Some(plan(Some(start), 0, true, true))));
            sleep(1_200);
            // The room is 15 ms on from here (a better clock estimate): frames go.
            r.send(Cmd::Sync(Some(plan(
                Some(start - Duration::from_millis(15)),
                0,
                true,
                true,
            ))));
            sleep(1_000);
        }
        When::LiningUp => {
            r.send(Cmd::Sync(Some(plan(Some(start), 0, true, true))));
            sleep(1_200);
            r.send(Cmd::Sync(Some(plan(
                Some(start - Duration::from_millis(60)),
                0,
                true,
                true,
            ))));
            // A second past the seek line: lined up, fading, or just after.
            sleep(1_010);
        }
        When::NextSongTookOver => {
            let b = counting(&format!("{name}-b"), 4, NEXT);
            r.send(Cmd::Sync(Some(plan(Some(start), 2_000, true, false))));
            sleep(200);
            r.send(Cmd::Insert {
                items: vec![b],
                next: true,
            });
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(4) {
                if let Ok(Some(1)) = r.moved.recv_timeout(Duration::from_millis(5)) {
                    break;
                }
            }
            sleep(30);
        }
        When::Paused => {
            r.send(Cmd::Sync(Some(plan(Some(start), 0, true, false))));
            sleep(1_000);
            r.send(Cmd::Sync(Some(plan(
                Some(Instant::now()),
                700,
                false,
                false,
            ))));
            sleep(300);
        }
    }
    // The room is over: on its own again, and your queue back where you were, paused.
    r.send(Cmd::Sync(None));
    r.send(Cmd::Set {
        items: vec![own],
        index: 0,
        start_ms: 1_000,
        play: false,
        from: "your own".into(),
    });
    sleep(200);
    let from = r.written();
    let pressed = Instant::now();
    r.send(Cmd::Play);
    sleep(2_500);
    let heard = r.heard_since(from);
    let status = r.status();
    let notes = r.notes_since(pressed);
    drop(r.player);
    (heard, status, notes)
}

/// Your own song, heard from its frame at `from_ms`: (whether every frame follows the one before
/// with nothing between, how many frames were heard).
fn straight_from(h: &[i32], from_ms: u64) -> (bool, usize) {
    let expect = OWN + (from_ms * u64::from(RATE) / 1_000) as i32 + 1;
    // Partway into a song it rises from silence over 15 ms (720 frames, each the song's own
    // frame scaled, never louder); from there, every frame is exactly the next. (What the
    // device held of the song before is played out first, as when playing anything.)
    let fade = if from_ms > 0 { 720 } else { 0 };
    let Some(exact) = h.iter().position(|&x| x == expect + fade as i32) else {
        return (false, 0);
    };
    let Some(first) = exact.checked_sub(fade) else {
        return (false, 0);
    };
    let faded = h[first..exact]
        .iter()
        .enumerate()
        .all(|(k, &x)| x.unsigned_abs() <= (expect + k as i32).unsigned_abs());
    let last = h.iter().rposition(|&x| x != 0).unwrap();
    let rest = &h[exact..=last];
    let straight = faded && rest.windows(2).all(|w| w[1] == w[0] + 1);
    (straight, last + 1 - first)
}

#[test]
fn after_a_room_ends_your_music_plays_sample_for_sample() {
    let _turn = engines::real_time();
    for when in [
        When::WaitingForStart,
        When::PlayingApart,
        When::TakingUpFrames,
        When::LiningUp,
        When::NextSongTookOver,
        When::Paused,
    ] {
        let (h, status, notes) = end_room(when);
        assert_eq!(notes, [], "{when:?}: nothing done to your music for a room");
        let (straight, frames) = straight_from(&h, 1_000);
        assert!(
            straight,
            "{when:?}: your song from 0:01, every frame after the one before"
        );
        // 2.5 s of playing, less the device's start: no stall, no silence put in.
        assert!(
            frames as f64 >= f64::from(RATE) * 2.2,
            "{when:?}: it plays on ({frames} frames)"
        );
        assert!(status.sync.is_none(), "{when:?}: no room's plan left");
        assert!(status.playing, "{when:?}: playing");
    }
}

/// A room's plan left in the player (its goodbye never came): your own queue played then.
fn own_queue_under_a_left_plan(left: SyncPlan) -> (Vec<i32>, Status<PathBuf>) {
    let a = counting(&format!("left-{}-a", left.position_ms), 160, 0);
    let own = counting(&format!("left-{}-own", left.position_ms), 30, OWN);
    let r = Run::new(&a);
    r.send(Cmd::Sync(Some(left)));
    sleep(600);
    let from = r.written();
    r.send(Cmd::Set {
        items: vec![own],
        index: 0,
        start_ms: 0,
        play: true,
        from: "your own".into(),
    });
    // Past the 3 s an apart room waits before lining a song up again.
    sleep(4_000);
    let heard = r.heard_since(from);
    let status = r.status();
    drop(r.player);
    (heard, status)
}

#[test]
fn your_own_queue_never_plays_under_a_rooms_plan() {
    let _turn = engines::real_time();
    let now = Instant::now();
    for left in [
        // Readied for a start that never comes: it would play silence, "playing", forever.
        plan(None, 0, true, false),
        // Playing minutes into the room's song: your song would be moved there.
        plan(Some(now), 120_000, true, false),
        plan(Some(now), 120_000, true, true),
        // Paused at the room's place: your song would be held there.
        plan(Some(now), 30_000, false, false),
    ] {
        let (h, status) = own_queue_under_a_left_plan(left);
        let (straight, frames) = straight_from(&h, 0);
        assert!(
            straight,
            "{left:?}: your song from its start, every frame in turn"
        );
        assert!(
            frames as f64 >= f64::from(RATE) * 3.6,
            "{left:?}: it plays on ({frames} frames)"
        );
        assert!(status.sync.is_none(), "{left:?}: the plan is gone");
    }
}

#[test]
fn every_correction_in_a_room_is_said() {
    let _turn = engines::real_time();
    // Apart, the room suddenly 900 ms on (a long stall somewhere): one seek, faded.
    let a = counting("said-apart", 12, 0);
    let r = Run::new(&a);
    let start = Instant::now() + Duration::from_millis(300);
    r.send(Cmd::Sync(Some(plan(Some(start), 0, true, false))));
    sleep(1_500);
    let t0 = Instant::now();
    r.send(Cmd::Sync(Some(plan(
        Some(start - Duration::from_millis(900)),
        0,
        true,
        false,
    ))));
    sleep(4_500);
    let seeks: Vec<_> = r
        .notes_since(t0)
        .into_iter()
        .filter(|n| matches!(n, SyncNote::Seek { .. }))
        .collect();
    assert!(
        matches!(seeks[..], [SyncNote::Seek { faded: true, from_ms, to_ms }] if to_ms > from_ms + 800),
        "one faded seek on: {seeks:?}"
    );
    drop(r.player);

    // One room, 4 ms off: the rate goes up a little, then back to the song's own.
    let b = counting("said-same", 12, 0);
    let r = Run::new(&b);
    r.send(Cmd::Sync(Some(plan(Some(start), 0, true, true))));
    sleep(1_200);
    let t0 = Instant::now();
    r.send(Cmd::Sync(Some(plan(
        Some(start - Duration::from_millis(4)),
        0,
        true,
        true,
    ))));
    sleep(10_000);
    let rates: Vec<i32> = r
        .notes_since(t0)
        .into_iter()
        .filter_map(|n| match n {
            SyncNote::Rate { ppm } => Some(ppm),
            _ => None,
        })
        .collect();
    assert!(
        rates.iter().any(|&p| p > 0) && rates.iter().all(|&p| p.abs() <= 500),
        "faster to catch up, within 500 ppm: {rates:?}"
    );
    assert_eq!(rates.last(), Some(&0), "back to its own rate: {rates:?}");
    drop(r.player);
}

#[test]
fn waiting_silent_for_the_rooms_start_is_not_a_stall() {
    let _turn = engines::real_time();
    let a = counting("not-a-stall", 4, 0);
    let r = Run::new(&a);
    // Readied and silent, the start a while off (others still getting ready).
    sleep(800);
    let s = r.status();
    assert!(s.playing, "the device runs on in silence");
    assert!(
        !s.stalled && !s.buffering,
        "ready, not waiting for a stream"
    );
    assert!(s.sync.is_some_and(|y| y.ready), "and says it's ready");
    drop(r.player);
}
