//! Playing in step with a room, on a device that takes sound at real-time pace: the song waits
//! silent until the room's start, then plays from the room's place; joining late lands where the
//! room is now. Each test song's samples are their own frame numbers, so the device's bytes say
//! exactly what was heard when.

mod engines;
use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::SyncPlan;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;

/// A mono 24-bit WAV whose sample i is i + 1 (never zero, so silence is told apart).
fn counting_wav(path: &Path, frames: usize) {
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
        v.extend_from_slice(&((i + 1) as i32).to_le_bytes()[..3]);
    }
    std::fs::write(path, v).unwrap();
}

/// The device's frames back as the song's frame numbers (0: silence).
fn heard(c: &Captured) -> Vec<i32> {
    c.bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 8)
        .collect()
}

fn song(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("venuta-sync-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("song.wav");
    counting_wav(&f, RATE as usize * 6);
    f
}

/// Play to a paced memory device for `for_ms` after sending the plan `make(now)`.
fn run(file: PathBuf, for_ms: u64, make: impl FnOnce(Instant) -> SyncPlan) -> (Captured, Instant) {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let player = Player::spawn(
        OutputChoice::PacedMemory(captured.clone()),
        |_s: Status<PathBuf>| {},
    );
    player.send(Cmd::Set {
        items: vec![file],
        index: 0,
        start_ms: 0,
        play: false,
        from: "room".into(),
    });
    std::thread::sleep(Duration::from_millis(100));
    let now = Instant::now();
    player.send(Cmd::Sync(Some(make(now))));
    std::thread::sleep(Duration::from_millis(for_ms));
    drop(player);
    std::thread::sleep(Duration::from_millis(50));
    let c = std::mem::take(&mut *captured.lock().unwrap());
    (c, now)
}

/// Partway into a song the music rises from silence over its first 15 ms (720 frames): the
/// song's frame at `first` (the first heard), read from where the rise is over.
fn frame_at(h: &[i32], first: usize) -> i32 {
    const RISE: usize = 720;
    assert!(
        (0..RISE)
            .all(|k| h[first + k].unsigned_abs()
                <= (h[first + RISE] - (RISE - k) as i32).unsigned_abs()),
        "a fade in, never louder than the song"
    );
    h[first + RISE] - RISE as i32
}

#[test]
fn a_room_start_waits_silent_then_plays_from_the_rooms_place() {
    let _turn = engines::real_time();
    let (c, now) = run(song("start"), 1_200, |now| SyncPlan {
        anchor: Some(now + Duration::from_millis(400)),
        position_ms: 1_000,
        playing: true,
        same_room: false,
        extra_latency_ms: 0,
    });
    let h = heard(&c);
    let first = h.iter().position(|&v| v != 0).expect("it played");
    // Frame k is heard k/rate after the device's first: the first sound should be heard at the start.
    let started = c.started_at.expect("a paced device");
    let heard_at = started + Duration::from_secs_f64(first as f64 / f64::from(RATE));
    let start = now + Duration::from_millis(400);
    let off_ms = if heard_at >= start {
        heard_at.duration_since(start).as_secs_f64()
    } else {
        -start.duration_since(heard_at).as_secs_f64()
    } * 1_000.0;
    assert!(
        off_ms.abs() < 5.0,
        "started {off_ms:.1} ms from the room's start"
    );
    let from = frame_at(&h, first) - 1;
    assert_eq!(from, 48_000, "from the room's position, to the frame");
    // And it plays on in order from there, with nothing skipped.
    assert!(
        h[first + 720..]
            .windows(2)
            .take(20_000)
            .all(|w| w[1] == w[0] + 1)
    );
}

#[test]
fn joining_late_lands_where_the_room_is_now() {
    let _turn = engines::real_time();
    // The room started this song 2 s ago.
    let (c, now) = run(song("late"), 800, |now| SyncPlan {
        anchor: Some(now - Duration::from_millis(2_000)),
        position_ms: 0,
        playing: true,
        same_room: false,
        extra_latency_ms: 0,
    });
    let h = heard(&c);
    let first = h.iter().position(|&v| v != 0).expect("it played");
    // Where the room is at the moment this frame is heard.
    let heard_at = c.started_at.expect("a paced device")
        + Duration::from_secs_f64(first as f64 / f64::from(RATE));
    let since_ms = heard_at.duration_since(now).as_secs_f64() * 1_000.0;
    let want = (2_000.0 + since_ms) * f64::from(RATE) / 1_000.0;
    let off_ms = (f64::from(frame_at(&h, first) - 1) - want) * 1_000.0 / f64::from(RATE);
    assert!(off_ms.abs() < 40.0, "{off_ms:.1} ms from the room");
}

#[test]
fn a_paused_room_holds_its_place() {
    let _turn = engines::real_time();
    let (c, _) = run(song("paused"), 500, |now| SyncPlan {
        anchor: Some(now),
        position_ms: 3_000,
        playing: false,
        same_room: false,
        extra_latency_ms: 0,
    });
    assert!(
        heard(&c).iter().all(|&v| v == 0),
        "nothing plays while the room is paused"
    );
}

#[test]
fn same_room_closes_a_small_gap_by_single_frames() {
    let _turn = engines::real_time();
    let captured = Arc::new(Mutex::new(Captured::default()));
    let errors = Arc::new(Mutex::new(Vec::<(Instant, f32)>::new()));
    let seen = errors.clone();
    let player = Player::spawn(
        OutputChoice::PacedMemory(captured.clone()),
        move |s: Status<PathBuf>| {
            if let Some(e) = s.sync.and_then(|y| y.error_ms) {
                seen.lock().unwrap().push((Instant::now(), e));
            }
        },
    );
    player.send(Cmd::Set {
        items: vec![song("room")],
        index: 0,
        start_ms: 0,
        play: false,
        from: "room".into(),
    });
    std::thread::sleep(Duration::from_millis(100));
    let start = Instant::now() + Duration::from_millis(300);
    let plan = SyncPlan {
        anchor: Some(start),
        position_ms: 0,
        playing: true,
        same_room: true,
        extra_latency_ms: 0,
    };
    player.send(Cmd::Sync(Some(plan)));
    std::thread::sleep(Duration::from_millis(1_500));
    // The room is found to be 4 ms further on (as a clock estimate improves): within the
    // lining-up tolerance, so it's taken out gently, not by a seek.
    player.send(Cmd::Sync(Some(SyncPlan {
        anchor: Some(start - Duration::from_millis(4)),
        ..plan
    })));
    std::thread::sleep(Duration::from_millis(4_500));
    drop(player);
    let e = errors.lock().unwrap().clone();
    let after = |ms: u64| {
        e.iter()
            .filter(|(t, _)| *t >= start + Duration::from_millis(ms))
            .map(|(_, v)| *v)
            .next()
    };
    let early = after(2_200).expect("errors reported");
    let late = after(5_600).expect("errors reported");
    assert!(early < -1.0, "behind the room at first: {early}");
    assert!(
        late.abs() < early.abs() - 0.8,
        "closing in: {early} then {late}"
    );
    // Never a jump: the heard frames run on with at most single frames taken out.
    let h = heard(&std::mem::take(&mut *captured.lock().unwrap()));
    let first = h.iter().position(|&v| v != 0).unwrap();
    let jumps = h[first..]
        .windows(2)
        .filter(|w| w[1] != w[0] + 1 && w[1] != 0)
        .filter(|w| (w[1] - w[0]).abs() > 2)
        .count();
    assert_eq!(jumps, 0, "no seek, only single frames");
}
