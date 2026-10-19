//! Listening together must sound like listening alone: nothing the room does may be heard as a
//! skip, a repeat, a gap or a click. The songs here are counting WAVs (every frame says which song
//! frame it is), played on a device that takes sound at real-time pace, so the bytes that reach
//! it show every discontinuity exactly; one test uses a sine, to hear clicks.

use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::SyncPlan;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;

/// A mono 24-bit WAV of `frames` samples made by `sample`.
fn wav(path: &Path, frames: usize, sample: impl Fn(usize) -> i32) {
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
        v.extend_from_slice(&sample(i).to_le_bytes()[..3]);
    }
    std::fs::write(path, v).unwrap();
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("venuta-room-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A counting song: frame i is base + i + 1 (never zero, so silence is told apart).
fn counting(name: &str, secs: f64, base: i32) -> PathBuf {
    let f = dir(name).join("song.wav");
    wav(&f, (f64::from(RATE) * secs) as usize, |i| {
        base + i as i32 + 1
    });
    f
}

fn heard(c: &Captured) -> Vec<i32> {
    c.bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 8)
        .collect()
}

/// What broke in the music: (frames dropped or repeated, bigger jumps, silences inside it).
/// `seams` are the expected steps from one song's last frame to the next one's first.
fn breaks(h: &[i32], seams: &[(i32, i32)]) -> (usize, usize, usize) {
    let (mut small, mut jumps, mut gaps) = (0, 0, 0);
    let Some(first) = h.iter().position(|&x| x != 0) else {
        return (0, 0, 0);
    };
    let last_sound = h.iter().rposition(|&x| x != 0).unwrap_or(first);
    let (mut last, mut in_gap) = (None::<i32>, false);
    for &x in &h[first..=last_sound] {
        if x == 0 {
            gaps += usize::from(!in_gap);
            in_gap = true;
            continue;
        }
        in_gap = false;
        if let Some(l) = last
            && !seams.contains(&(l, x))
        {
            match x as i64 - l as i64 {
                1 => {}
                0 | 2 => small += 1,
                _ => jumps += 1,
            }
        }
        last = Some(x);
    }
    (small, jumps, gaps)
}

fn plan(anchor: Instant, position_ms: u64, same_room: bool) -> SyncPlan {
    SyncPlan {
        anchor: Some(anchor),
        position_ms,
        playing: true,
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
    moved: std::sync::mpsc::Receiver<Option<usize>>,
}

fn start(file: PathBuf) -> Run {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, moved) = channel::<Option<usize>>();
    let player = Player::spawn(
        OutputChoice::PacedMemory(captured.clone()),
        move |s: Status<PathBuf>| {
            let _ = tx.send(s.index);
        },
    );
    player.send(Cmd::Set {
        items: vec![file],
        index: 0,
        start_ms: 0,
        play: false,
        from: "room".into(),
    });
    std::thread::sleep(Duration::from_millis(100));
    Run {
        player,
        captured,
        moved,
    }
}

fn finish(r: Run) -> Vec<i32> {
    drop(r.player);
    std::thread::sleep(Duration::from_millis(50));
    heard(&std::mem::take(&mut *r.captured.lock().unwrap()))
}

/// The room's next song, readied ahead as a room does, and the follower's plan for it sent
/// `react_ms` after the player moves on (as the follower does after hearing it moved).
fn song_change(react_ms: u64, same_room: bool) -> (Vec<i32>, (i32, i32)) {
    let a = counting(&format!("a-{react_ms}-{same_room}"), 2.0, 0);
    let b = counting(&format!("b-{react_ms}-{same_room}"), 3.0, 4_000_000);
    let r = start(a);
    let at = Instant::now() + Duration::from_millis(300);
    r.player.send(Cmd::Sync(Some(plan(at, 0, same_room))));
    std::thread::sleep(Duration::from_millis(200));
    r.player.send(Cmd::Insert {
        items: vec![b],
        next: true,
    });
    let t0 = Instant::now();
    let mut sent = false;
    while t0.elapsed() < Duration::from_millis(4_500) {
        if let Ok(Some(1)) = r.moved.recv_timeout(Duration::from_millis(5))
            && !sent
        {
            sent = true;
            std::thread::sleep(Duration::from_millis(react_ms));
            // The next song starts exactly as the last ends, 2 s after its start.
            r.player.send(Cmd::Sync(Some(plan(
                at + Duration::from_millis(2_000),
                0,
                same_room,
            ))));
        }
    }
    assert!(sent, "the player moved on to the next song");
    (finish(r), (96_000, 4_000_001))
}

#[test]
fn a_song_change_in_a_room_plays_straight_on() {
    for (react, same_room) in [(50, false), (150, false), (150, true)] {
        let (h, seam) = song_change(react, same_room);
        assert_eq!(
            breaks(&h, &[seam]),
            (0, 0, 0),
            "react {react} ms, same room {same_room}"
        );
        assert!(h.contains(&4_000_001), "the next song from its first frame");
    }
}

#[test]
fn apart_a_moving_clock_estimate_is_never_heard() {
    let f = counting("apart-noise", 9.0, 0);
    let r = start(f);
    let at = Instant::now() + Duration::from_millis(300);
    r.player.send(Cmd::Sync(Some(plan(at, 0, false))));
    let mut seed = 7;
    // Wobbling by up to 15 ms, then one step of 120 ms (a new path to the host, a burst of
    // late pings): none of it worth a sound anyone hears.
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(500));
        let off = noise(&mut seed) * 15.0;
        r.player
            .send(Cmd::Sync(Some(plan(shifted(at, off), 0, false))));
    }
    r.player
        .send(Cmd::Sync(Some(plan(shifted(at, 120.0), 0, false))));
    std::thread::sleep(Duration::from_millis(2_500));
    assert_eq!(breaks(&finish(r), &[]), (0, 0, 0));
}

#[test]
fn apart_far_off_lines_up_once_and_without_a_click() {
    // A sine at 1 kHz: a cut from one place to another shows as a spike in its curvature.
    let f = dir("apart-far").join("song.wav");
    let amp = 4_000_000.0;
    wav(&f, RATE as usize * 10, |i| {
        (amp * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(RATE)).sin()) as i32
    });
    let r = start(f);
    let at = Instant::now() + Duration::from_millis(300);
    r.player.send(Cmd::Sync(Some(plan(at, 0, false))));
    std::thread::sleep(Duration::from_millis(1_500));
    // The room is suddenly 900 ms further on than this player (as after a long stall).
    r.player
        .send(Cmd::Sync(Some(plan(shifted(at, -900.0), 0, false))));
    std::thread::sleep(Duration::from_millis(5_500));
    let h = finish(r);
    let first = h.iter().position(|&x| x != 0).unwrap();
    let last = h.iter().rposition(|&x| x != 0).unwrap();
    // The sine's own curvature peaks at amp * w^2; a hard cut is thousands of times that.
    let w = 2.0 * std::f64::consts::PI * 1000.0 / f64::from(RATE);
    let own = amp * w * w;
    let worst = h[first + 1..last]
        .windows(3)
        .map(|s| (f64::from(s[2]) - 2.0 * f64::from(s[1]) + f64::from(s[0])).abs())
        .fold(0.0, f64::max);
    assert!(
        worst < own * 4.0,
        "no click: curvature {worst:.0} against the sine's own {own:.0}"
    );
}

#[test]
fn same_room_ignores_clock_noise_under_a_few_ms() {
    let f = counting("same-noise", 9.0, 0);
    let r = start(f);
    let at = Instant::now() + Duration::from_millis(300);
    r.player.send(Cmd::Sync(Some(plan(at, 0, true))));
    let mut seed = 11;
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(900));
        let off = noise(&mut seed) * 1.5;
        r.player
            .send(Cmd::Sync(Some(plan(shifted(at, off), 0, true))));
    }
    std::thread::sleep(Duration::from_millis(500));
    let (small, jumps, gaps) = breaks(&finish(r), &[]);
    assert_eq!((jumps, gaps), (0, 0));
    assert!(
        small <= 3,
        "{small} frames dropped or repeated for noise alone"
    );
}
