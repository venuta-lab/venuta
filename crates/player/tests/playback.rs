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
        .as_chunks::<4>().0.iter()
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
    // 250 ms at 48 kHz is frame 12,000.
    assert_eq!(
        &v[..want.len() - 24_000],
        &want[24_000..],
        "starts exactly at 250 ms"
    );
}

fn tempdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "venuta-player-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}
