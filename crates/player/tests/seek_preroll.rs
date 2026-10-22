//! Lossy seeks pre-roll (design 4.6 and the WP2 notes in 9.2): a seek to any frame gives what a
//! straight decode gives from that frame, bit for bit: MP3 (at 128 and 32 kbps), AAC (without
//! noise substitution, whose noise is drawn afresh), Vorbis, and Opus in Ogg and in WebM (the
//! catalog's stream, timed in milliseconds).
//!
//! The fixtures are six seconds of WP10's synthesized music (`align-music.m4a`), made stereo
//! (the right channel 7 ms behind) and encoded by ffmpeg; see `make_fixtures`. No real recording.

use player::decode::{DecodeState, Source};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Every frame of a straight decode, interleaved.
fn straight(path: &Path) -> (usize, Vec<i32>) {
    let mut source = Source::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let ch = usize::from(source.format.channels);
    let (mut all, mut out) = (Vec::new(), Vec::new());
    loop {
        match source.next(&mut out).expect("decodes") {
            DecodeState::Samples => all.extend_from_slice(&out),
            DecodeState::End => break,
            DecodeState::Pending => unreachable!("a local file never waits"),
        }
    }
    (ch, all)
}

/// A fresh open, a seek to `target`, then everything to the end: where it says it landed, where
/// its first frame says it is, and the frames.
fn from(path: &Path, target: u64) -> (u64, u64, Vec<i32>) {
    let mut source = Source::open(path).unwrap();
    let landed = source.seek_frame(target).expect("seeks");
    let at = source.position();
    let (mut all, mut out) = (Vec::new(), Vec::new());
    while let DecodeState::Samples = source.next(&mut out).expect("decodes") {
        all.extend_from_slice(&out);
    }
    (landed, at, all)
}

/// Seek targets over the song: the very start, inside the first pre-roll, on and beside packet
/// boundaries of every codec here (960, 1024, 1152), a spread of odd places, and near the end.
fn targets(frames: u64) -> Vec<u64> {
    let mut t = vec![
        1, 7, 500, 959, 960, 1023, 1024, 1151, 1152, 2304, 3839, 3840, 8191, 8192,
    ];
    for k in [5u64, 17, 40, 77, 101] {
        t.extend([960 * k, 1024 * k, 1152 * k, 1152 * k + 1, 1024 * k - 1]);
    }
    for i in 1..30u64 {
        t.push(i * 7_919 % (frames - 10_000));
    }
    t.push(frames - 5_000);
    t.retain(|&x| x < frames - 2_000);
    t
}

/// The largest difference between a seek's frames and the straight decode's, as a share of full
/// scale; and checks the seek landed where it says.
fn worst(name: &str) -> f64 {
    let path = fixture(name);
    let (ch, all) = straight(&path);
    let frames = (all.len() / ch) as u64;
    let mut worst = 0f64;
    for target in targets(frames) {
        let (landed, at, got) = from(&path, target);
        assert_eq!(
            (landed, at),
            (target, target),
            "{name}: landed for {target}"
        );
        let want = &all[target as usize * ch..];
        assert_eq!(
            got.len(),
            want.len(),
            "{name}: from {target}, as many frames as a straight decode has from there"
        );
        for (g, w) in got.iter().zip(want) {
            let d = (f64::from(*g) - f64::from(*w)).abs() / 2f64.powi(31);
            worst = worst.max(d);
        }
    }
    worst
}

#[test]
fn an_mp3_seek_equals_the_straight_decode_bit_for_bit() {
    assert_eq!(worst("seek-music.mp3"), 0.0);
}

/// 32 kbps: frames of about 104 bytes, so a frame's bit reservoir (up to 511 bytes) reaches as
/// far as seven frames back, past the four symphonia rewinds by itself (two frames of pre-roll
/// failed here, by up to 0.16 of full scale).
#[test]
fn a_low_bitrate_mp3_seek_equals_the_straight_decode_bit_for_bit() {
    assert_eq!(worst("seek-music-32k.mp3"), 0.0);
}

#[test]
fn an_aac_seek_equals_the_straight_decode_bit_for_bit() {
    assert_eq!(worst("seek-music.m4a"), 0.0);
}

#[test]
fn a_vorbis_seek_equals_the_straight_decode_bit_for_bit() {
    assert_eq!(worst("seek-music.ogg"), 0.0);
}

/// Opus predicts each frame's band energies from the frame before (a factor of 0.5 to 0.9 a
/// frame): after RFC 7845's 80 ms a seek still differed by up to 4e-2 of full scale for half a
/// second, after 400 ms by 6e-7; after a second, not at all.
#[test]
fn an_opus_seek_equals_the_straight_decode_bit_for_bit() {
    assert_eq!(worst("seek-music.opus"), 0.0);
}

/// WebM counts in milliseconds; the place comes from the packet grid (whole 2.5 ms).
#[test]
fn an_opus_seek_in_webm_equals_the_straight_decode_bit_for_bit() {
    assert_eq!(worst("seek-music.webm"), 0.0);
}

/// Writes the fixtures from WP10's synthesized song. Run by hand:
/// `FFMPEG=path/to/ffmpeg cargo test -p player --test seek_preroll make_fixtures -- --ignored`.
#[test]
#[ignore = "writes the fixtures; needs ffmpeg"]
fn make_fixtures() {
    let ffmpeg = std::env::var("FFMPEG").expect("FFMPEG names ffmpeg");
    let stereo = "pan=stereo|c0=c0|c1=c0,adelay=0|7";
    for (name, args) in [
        ("seek-music.opus", &["-c:a", "libopus", "-b:a", "64k"][..]),
        (
            "seek-music.m4a",
            &[
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-aac_pns",
                "0",
                "-movflags",
                "+faststart",
            ][..],
        ),
        (
            "seek-music.mp3",
            &["-c:a", "libmp3lame", "-b:a", "128k"][..],
        ),
        (
            "seek-music-32k.mp3",
            &["-c:a", "libmp3lame", "-b:a", "32k"][..],
        ),
        ("seek-music.ogg", &["-c:a", "libvorbis", "-q:a", "3"][..]),
    ] {
        let status = std::process::Command::new(&ffmpeg)
            .args(["-y", "-loglevel", "error", "-ss", "2", "-t", "6", "-i"])
            .arg(fixture("align-music.m4a"))
            .args(["-map_metadata", "-1", "-af", stereo])
            .args(args)
            .arg(fixture(name))
            .status()
            .unwrap();
        assert!(status.success(), "{name}");
    }
    // The catalog's WebM: its index before the music, timed from 0 (remuxed from the cut, as
    // a cut encoded straight to WebM keeps the cut's offset in its times).
    let cut = std::env::temp_dir().join("venuta-seek-music-cut.webm");
    for (input, args, out) in [
        (
            fixture("align-music.m4a"),
            vec!["-ss", "2", "-t", "6"],
            cut.clone(),
        ),
        (cut.clone(), vec![], fixture("seek-music.webm")),
    ] {
        let first = out == cut;
        let mut c = std::process::Command::new(&ffmpeg);
        c.args(["-y", "-loglevel", "error"])
            .args(&args)
            .arg("-i")
            .arg(&input);
        c.args(["-map_metadata", "-1"]);
        if first {
            c.args(["-af", stereo, "-c:a", "libopus", "-b:a", "64k"]);
        } else {
            c.args(["-c", "copy", "-dash", "1"]);
        }
        assert!(c.arg(&out).status().unwrap().success());
    }
}
