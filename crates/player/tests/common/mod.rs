//! What the room checks share: music-shaped test songs, and an ear for clicks (both in
//! `audio-check`, shared with the rooms' and the app's tests), and reading what a memory device
//! heard.

#![allow(dead_code)]

pub use audio_check::music::{Click, Piece, clicks};
use player::output::Captured;
use std::path::PathBuf;

/// A test song in a folder of its own: `secs` long, cut from the piece at `from_s` seconds.
pub fn song(name: &str, piece: &Piece, from_s: f64, secs: f64, rise: bool, fall: bool) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "venuta-clicks-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    let rate = f64::from(piece.rate);
    piece.write(
        &f,
        (from_s * rate) as usize,
        (secs * rate) as usize,
        rise,
        fall,
    );
    f
}

/// What a memory device heard, per channel (24-bit values, from the 32-bit container).
pub fn channels(c: &Captured, ch: usize) -> Vec<Vec<i32>> {
    let all: Vec<i32> = c
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 8)
        .collect();
    (0..ch)
        .map(|k| all.iter().skip(k).step_by(ch).copied().collect())
        .collect()
}

/// Clicks in every channel heard, by channel.
pub fn all_clicks(c: &Captured, ch: usize, s1: f64, s3: f64) -> Vec<Click> {
    channels(c, ch)
        .iter()
        .flat_map(|x| clicks(x, s1, s3))
        .collect()
}
