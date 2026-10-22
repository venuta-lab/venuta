//! The click detectors, the one place the kit names them.
//!
//! - The music-shaped detector (steps, third-difference breaks, hard-edged gaps) is the one in
//!   `crates/player/tests/common`, taken in by path, unchanged. It knows the test music's
//!   limits, so the kit runs it only on the test music it plays itself (`live_check tone`).
//! - Detectors 3 (LPC residual) and 4 (dip and gap) of section 7.1 run on everything.
//!
//! **At merge with work package 0:** `crates/player/tests/common` moves to
//! `crates/audio-check`, which carries detectors 2 to 4 too. Then this file becomes re-exports
//! from audio-check (`player` takes it as a dev-dependency, as its tests will), `detect34.rs`
//! is deleted, and nothing else in the kit changes: the tools only use the names below.

// The music-shaped detector and its test music now live in audio-check.
use audio_check::music as common;

pub use super::detect34::{DipParams, Flag, LpcParams, Quiet};

/// The test music the kit plays (a chord, a little treble, soft noise; never silent inside).
pub use common::Piece;

/// Full scale of the music-shaped detector's 24-bit units.
pub const FULL: f64 = common::FULL;

/// Detector 3 on one channel.
pub fn lpc_residual(x: &[f32], p: &LpcParams) -> Vec<Flag> {
    super::detect34::lpc_residual(x, p)
}

/// Detector 4 on interleaved frames: the dips, and the long quiet stretches set aside.
pub fn dip_gap(data: &[f32], channels: u16, rate: u32, p: &DipParams) -> (Vec<Flag>, Vec<Quiet>) {
    super::detect34::dips(data, channels, rate, p)
}

/// The music-shaped detector on one channel in 24-bit units, for music whose steepest step is
/// `s1` and whose third difference is at most `s3` (`Piece::limits`, scaled to the level).
pub fn music_shaped(x: &[i32], s1: f64, s3: f64) -> Vec<Flag> {
    common::clicks(x, s1, s3)
        .into_iter()
        .map(|c| Flag {
            at: c.at,
            len: 1,
            detector: "music-shaped",
            db: 20.0 * c.over.max(1e-9).log10(),
        })
        .collect()
}
