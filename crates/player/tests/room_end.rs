//! After a room, the player is the player again: whatever the room was doing when it ended (a
//! song getting ready, frames being taken up in one room, a seek fading out, the song after
//! taking over, paused), your own music then plays sample for sample as it does alone, with no
//! plan, no correction and no silence. And a queue of your own never plays under an old room's
//! plan, even if the room's goodbye never reached the player.
//!
//! Songs are counting WAVs (each frame says which frame of which song it is) on a device that
//! takes sound at real-time pace, so the bytes that reach it show every frame dropped, repeated
//! or moved.

use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::SyncPlan;
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
    let d = std::env::temp_dir().join(format!("venuta-end-{}-{name}", std::process::id()));
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
    moved: std::sync::mpsc::Receiver<Option<usize>>,
}

impl Run {
    fn new(room_song: &Path) -> Self {
        let captured = Arc::new(Mutex::new(Captured::default()));
        let last = Arc::new(Mutex::new(None));
        let seen = last.clone();
        let (tx, moved) = std::sync::mpsc::channel();
        let player = Player::spawn(
            OutputChoice::PacedMemory(captured.clone()),
            move |s: Status<PathBuf>| {
                let _ = tx.send(s.index);
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
fn end_room(when: When) -> (Vec<i32>, Status<PathBuf>) {
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
    r.send(Cmd::Play);
    sleep(2_500);
    let heard = r.heard_since(from);
    let status = r.status();
    drop(r.player);
    (heard, status)
}

/// Your own song, heard from its frame at `from_ms`: (whether every frame follows the one before
/// with nothing between, how many frames were heard).
fn straight_from(h: &[i32], from_ms: u64) -> (bool, usize) {
    // What the device held of the song before is played out first, as when playing anything.
    let own = |x: i32| (OWN + 1..OWN + 30 * RATE as i32 + 1).contains(&x);
    let Some(first) = h.iter().position(|&x| own(x)) else {
        return (false, 0);
    };
    let last = h.iter().rposition(|&x| x != 0).unwrap();
    let music = &h[first..=last];
    let expect = OWN + (from_ms * u64::from(RATE) / 1_000) as i32 + 1;
    let straight = music[0] == expect && music.windows(2).all(|w| w[1] == w[0] + 1);
    (straight, music.len())
}

#[test]
fn after_a_room_ends_your_music_plays_sample_for_sample() {
    for when in [
        When::WaitingForStart,
        When::PlayingApart,
        When::TakingUpFrames,
        When::LiningUp,
        When::NextSongTookOver,
        When::Paused,
    ] {
        let (h, status) = end_room(when);
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
