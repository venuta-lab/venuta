//! The engine end to end, into memory: exact samples, gapless joins, format changes, seeking.

use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A PCM WAV file whose sample `i` of channel `c` is `value(i, c)` (within the bit depth).
fn wav(
    path: &Path,
    rate: u32,
    bits: u16,
    channels: u16,
    frames: usize,
    value: impl Fn(usize, u16) -> i32,
) {
    let bytes_per = usize::from(bits / 8);
    let data_len = frames * usize::from(channels) * bytes_per;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
    v.extend_from_slice(&(channels * bits / 8).to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        for c in 0..channels {
            v.extend_from_slice(&value(i, c).to_le_bytes()[..bytes_per]);
        }
    }
    std::fs::write(path, v).unwrap();
}

/// A recognisable, never-silent pattern for each song.
fn pattern(seed: i32, bits: u16) -> impl Fn(usize, u16) -> i32 {
    let max = (1i32 << (bits - 1)) - 1;
    move |i, c| {
        ((i as i32)
            .wrapping_mul(7919)
            .wrapping_add(seed * 31 + i32::from(c) * 1013))
            % max
    }
}

/// Play `items` to memory until the queue ends; returns what the device received.
fn play_all(items: Vec<PathBuf>, start_ms: u64) -> Captured {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, rx) = channel::<Status<PathBuf>>();
    let player = Player::spawn(OutputChoice::Memory(captured.clone()), move |s| {
        let _ = tx.send(s);
    });
    player.send(Cmd::Set {
        items,
        index: 0,
        start_ms,
        play: true,
        from: "test".into(),
    });
    let started = Instant::now();
    let mut was_playing = false;
    loop {
        let s = rx.recv_timeout(Duration::from_secs(10)).expect("status");
        assert!(s.error.is_empty(), "error: {}", s.error);
        was_playing |= s.playing;
        if was_playing && !s.playing {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "never finished"
        );
    }
    drop(player);
    Arc::try_unwrap(captured)
        .ok()
        .unwrap()
        .into_inner()
        .unwrap()
}

/// Device samples back as the source's own values (32-bit containers, left-justified).
fn values(bytes: &[u8], bits: u16) -> Vec<i32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) >> (32 - bits))
        .collect()
}

fn expected(frames: usize, channels: u16, f: impl Fn(usize, u16) -> i32) -> Vec<i32> {
    (0..frames)
        .flat_map(|i| (0..channels).map(move |c| (i, c)))
        .map(|(i, c)| f(i, c))
        .collect()
}

#[test]
fn two_songs_join_sample_for_sample_and_every_value_is_exact() {
    let dir = tempdir();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    // Odd lengths, so the join falls in the middle of a device period.
    wav(&a, 44_100, 16, 2, 12_345, pattern(1, 16));
    wav(&b, 44_100, 16, 2, 6_789, pattern(2, 16));
    let got = play_all(vec![a, b], 0);
    assert_eq!(
        got.formats.len(),
        1,
        "same format: the device was never reopened"
    );
    let v = values(&got.bytes, 16);
    let mut want = expected(12_345, 2, pattern(1, 16));
    want.extend(expected(6_789, 2, pattern(2, 16)));
    assert_eq!(&v[..want.len()], &want[..], "gapless and bit-perfect");
    assert!(
        v[want.len()..].iter().all(|&x| x == 0),
        "only silence after the end"
    );
}

#[test]
fn a_new_format_reopens_the_device_and_stays_exact() {
    let dir = tempdir();
    let (a, b) = (dir.join("cd.wav"), dir.join("hires.wav"));
    wav(&a, 44_100, 16, 2, 5_000, pattern(3, 16));
    wav(&b, 96_000, 24, 2, 7_000, pattern(4, 24));
    let got = play_all(vec![a, b], 0);
    assert_eq!(got.formats.len(), 2);
    assert_eq!(
        (got.formats[0].rate, got.formats[0].valid_bits),
        (44_100, 16)
    );
    assert_eq!(
        (got.formats[1].rate, got.formats[1].valid_bits),
        (96_000, 24)
    );
    let first_bytes = got.frames[0] * 2 * 4;
    let first = values(&got.bytes[..first_bytes], 16);
    let second = values(&got.bytes[first_bytes..], 24);
    let want_a = expected(5_000, 2, pattern(3, 16));
    let want_b = expected(7_000, 2, pattern(4, 24));
    assert_eq!(&first[..want_a.len()], &want_a[..]);
    assert_eq!(
        &second[..want_b.len()],
        &want_b[..],
        "the 24/96 song arrives exact"
    );
}

#[test]
fn starting_part_way_lands_on_the_exact_sample() {
    let dir = tempdir();
    let a = dir.join("seek.wav");
    wav(&a, 48_000, 24, 2, 48_000, pattern(5, 24));
    let got = play_all(vec![a], 250);
    let v = values(&got.bytes, 24);
    let want = expected(48_000, 2, pattern(5, 24));
    // 250 ms at 48 kHz is frame 12,000. It rises from silence over its first 15 ms (720
    // frames, both channels), then every sample is the song's own.
    let fade = 720 * 2;
    assert_eq!(
        &v[fade..want.len() - 24_000],
        &want[24_000 + fade..],
        "starts exactly at 250 ms"
    );
    assert!(
        v[..fade]
            .iter()
            .zip(&want[24_000..])
            .all(|(got, own)| got.unsigned_abs() <= own.unsigned_abs()),
        "faded in from it, never louder"
    );
}

/// A folder of its own for each test. The tests run at once in one process and several write
/// files of the same name (a.wav): the clock alone can give two of them the same folder, and one
/// would play the other's song. A count makes each folder its own.
fn tempdir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "venuta-player-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Play to memory with commands sent first; returns what came out and the last status.
fn play_with(items: Vec<PathBuf>, before: Vec<Cmd<PathBuf>>) -> (Captured, Status<PathBuf>) {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, rx) = channel::<Status<PathBuf>>();
    let player = Player::spawn(OutputChoice::Memory(captured.clone()), move |s| {
        let _ = tx.send(s);
    });
    for c in before {
        player.send(c);
    }
    player.send(Cmd::Set {
        items,
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    let mut was = false;
    let last = loop {
        let s = rx.recv_timeout(Duration::from_secs(10)).expect("status");
        was |= s.playing;
        if was && !s.playing {
            break s;
        }
    };
    drop(player);
    (
        Arc::try_unwrap(captured)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap(),
        last,
    )
}

#[test]
fn stop_after_this_song_stops_with_the_next_one_ready() {
    let dir = tempdir();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    wav(&a, 44_100, 16, 2, 9_000, pattern(6, 16));
    wav(&b, 44_100, 16, 2, 9_000, pattern(7, 16));
    let (got, last) = play_with(vec![a, b], vec![Cmd::StopAfterCurrent(true)]);
    let v = values(&got.bytes, 16);
    let want = expected(9_000, 2, pattern(6, 16));
    assert_eq!(&v[..want.len()], &want[..]);
    assert!(
        v[want.len()..].iter().all(|&x| x == 0),
        "nothing of the second song"
    );
    assert_eq!(last.index, Some(1), "the next song is ready");
    assert!(!last.stop_after_current, "and the switch resets");
}

#[test]
fn a_sound_tool_changes_the_output_and_says_so() {
    use player::dsp::Sound;
    let dir = tempdir();
    let a = dir.join("eq.wav");
    wav(&a, 48_000, 24, 2, 12_000, pattern(8, 24));
    let sound = Sound {
        bass_db: 6.0,
        ..Sound::default()
    };
    let (got, last) = play_with(vec![a], vec![Cmd::Sound(sound)]);
    let v = values(&got.bytes, 24);
    let want = expected(12_000, 2, pattern(8, 24));
    assert_ne!(
        &v[..want.len()],
        &want[..],
        "the bass boost changed the samples"
    );
    assert_eq!(last.sound.bass_db, 6.0);
}

#[test]
fn crossfade_overlaps_the_songs_by_exactly_the_fade() {
    use player::dsp::Sound;
    let dir = tempdir();
    let (a, b) = (dir.join("x.wav"), dir.join("y.wav"));
    wav(&a, 44_100, 16, 1, 20_000, |_, _| 10_000);
    wav(&b, 44_100, 16, 1, 20_000, |_, _| 20_000);
    let sound = Sound {
        crossfade_s: 0.1,
        limiter: false,
        ..Sound::default()
    };
    let (got, _) = play_with(vec![a, b], vec![Cmd::Sound(sound)]);
    let v = values(&got.bytes, 16);
    let fade = 4_410;
    // Untouched until the fade starts.
    assert!(v[..20_000 - fade].iter().all(|&x| x == 10_000));
    // Then the second song, at full level, for the rest of it.
    let after = 20_000 + (20_000 - fade);
    assert!(
        v[20_000..after].iter().all(|&x| x == 20_000),
        "second song continues where the blend left it"
    );
    assert!(
        v[after..].iter().all(|&x| x == 0),
        "overlap shortened the total by the fade"
    );
    // In between, it rises from one level to the other.
    let mid = v[20_000 - fade / 2];
    assert!(mid > 10_000 && mid < 30_000, "{mid}");
}

/// Reports that arrive within `window`.
fn drain(
    rx: &std::sync::mpsc::Receiver<Status<PathBuf>>,
    window: Duration,
) -> Vec<Status<PathBuf>> {
    let until = Instant::now() + window;
    let mut out = Vec::new();
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(s) => out.push(s),
            Err(_) => break,
        }
    }
    out
}

#[test]
fn a_paused_player_says_nothing_until_something_changes() {
    let dir = tempdir();
    let song = dir.join("a.wav");
    wav(&song, 44_100, 16, 2, 44_100 * 3, pattern(1, 16));
    let (tx, rx) = channel::<Status<PathBuf>>();
    let player = Player::spawn(
        OutputChoice::Memory(Arc::new(Mutex::new(Captured::default()))),
        move |s| {
            let _ = tx.send(s);
        },
    );
    player.send(Cmd::Set {
        items: vec![song],
        index: 0,
        start_ms: 0,
        play: false,
        from: String::new(),
    });
    // Loading is reported; then the player rests.
    assert!(
        !drain(&rx, Duration::from_millis(400)).is_empty(),
        "loading is reported"
    );
    let quiet = drain(&rx, Duration::from_millis(1500));
    assert!(
        quiet.is_empty(),
        "{} reports while nothing changed",
        quiet.len()
    );

    // Anything asked of it is answered at once, even when nothing changes (a window that has
    // just opened asks for the state this way).
    player.send(Cmd::Refresh);
    let answer = drain(&rx, Duration::from_millis(300));
    assert_eq!(answer.len(), 1, "one report for one request");
    assert!(answer[0].queue.is_some(), "with the queue");

    // Playing is reported as it moves, and so is the end (memory plays faster than real time);
    // then the player rests again.
    player.send(Cmd::Play);
    let playing = drain(&rx, Duration::from_millis(1000));
    let seen: Vec<(bool, u64)> = playing.iter().map(|s| (s.playing, s.position_ms)).collect();
    assert!(
        seen.first().is_some_and(|s| s.0),
        "playing is reported: {seen:?}"
    );
    assert!(
        seen.iter().any(|s| s.0 && s.1 > 0),
        "the position moves: {seen:?}"
    );
    assert!(
        seen.last().is_some_and(|s| !s.0),
        "the end is reported: {seen:?}"
    );
    let after = drain(&rx, Duration::from_millis(1000));
    assert!(after.is_empty(), "{} reports after the end", after.len());
    drop(player);
}
