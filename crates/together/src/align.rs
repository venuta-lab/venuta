//! How one person's copy of a song lines up with the room's reference copy (contract 5.8 of the
//! Listen together design).
//!
//! The host makes an envelope of its reference copy and sends it (the `env:` stream); each
//! guest makes one of its own copy and correlates the two. This file declares the shapes every
//! package builds against; making envelopes and aligning them is the alignment package's
//! (`player::analysis`: `envelope(decoded, rate, ch) -> Envelope` and
//! `align(reference, mine) -> Alignment`).

use serde::{Deserialize, Serialize};

/// A song's onset envelope: one byte per hop (onset strength from the rise of log energy in
/// four bands), in windows (the first 30 s, then 10 s every 60 s), and the hash of the first
/// 10 s of decoded PCM, which tells an exact copy at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub hop_ms: u8,
    /// (where the window starts in the song, in ms; one byte per hop)
    pub windows: Vec<(u32, Vec<u8>)>,
    pub pcm_hash: Option<[u8; 16]>,
}

/// Where my copy is against the reference: `my content time = room content time + offset_us`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    pub offset_us: i64,
    pub class: CopyClass,
    /// How sure the measure is (the correlation peak over the noise, in standard deviations);
    /// 0 for an exact or approximate copy.
    pub confidence: f32,
}

impl Alignment {
    /// The same bytes as the reference (the host's own file): offset 0, nothing to measure.
    pub const EXACT: Alignment = Alignment {
        offset_us: 0,
        class: CopyClass::Exact,
        confidence: 0.0,
    };
    /// Nothing known (no reference envelope, or no confident peak): offset 0, best effort.
    pub const APPROXIMATE: Alignment = Alignment {
        offset_us: 0,
        class: CopyClass::Approximate,
        confidence: 0.0,
    };
}

/// How well a copy is known to match the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyClass {
    /// The host's own bytes, or a lossless copy whose decoded PCM hashes the same.
    Exact,
    /// Correlated with enough confidence; refined through the song.
    Measured,
    /// No reference, low confidence, or another edit.
    Approximate,
}
