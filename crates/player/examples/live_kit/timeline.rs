//! When each captured frame was heard: kept beside a capture as `<capture>.json`, so a flag can
//! be matched to the playback log (whose lines start with Unix milliseconds) and to another
//! capture's clock.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Timeline {
    pub rate: u32,
    pub channels: u16,
    /// What was recorded ("loopback of Speakers (Realtek(R) Audio)").
    pub source: String,
    /// Unix time in ms at `start_qpc`.
    pub start_unix_ms: f64,
    /// The performance counter (100 ns units) when the capture began.
    pub start_qpc: u64,
    /// (frame in the file, performance counter of that frame), about once a second.
    pub anchors: Vec<(u64, u64)>,
    /// Stretches where the output played nothing and the loopback gave no data, filled with
    /// silence in the file: (frame, frames).
    pub idle: Vec<(u64, u64)>,
    /// Frames where the capture itself lost data (it was late): flags near these are not the
    /// player's.
    pub capture_glitches: Vec<u64>,
}

impl Timeline {
    pub fn path_for(wav: &Path) -> PathBuf {
        wav.with_extension("json")
    }

    pub fn load(wav: &Path) -> Option<Self> {
        let s = std::fs::read_to_string(Self::path_for(wav)).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn save(&self, wav: &Path) -> std::io::Result<()> {
        std::fs::write(
            Self::path_for(wav),
            serde_json::to_string_pretty(self).unwrap_or_default(),
        )
    }

    /// The performance counter (100 ns units) at `frame`, from the nearest anchors.
    pub fn qpc_at(&self, frame: f64) -> Option<f64> {
        let a = &self.anchors;
        if a.is_empty() || self.rate == 0 {
            return None;
        }
        let i = a.partition_point(|&(f, _)| (f as f64) <= frame);
        let (f0, q0) = a[i.saturating_sub(1).min(a.len() - 1)];
        Some(q0 as f64 + (frame - f0 as f64) / f64::from(self.rate) * 1e7)
    }

    /// Unix ms at `frame` (comparable with the playback log's first column).
    pub fn unix_ms_at(&self, frame: f64) -> Option<f64> {
        let q = self.qpc_at(frame)?;
        Some(self.start_unix_ms + (q - self.start_qpc as f64) / 1e4)
    }

    /// Whether `frame` is within `near` frames of a capture glitch.
    pub fn near_glitch(&self, frame: usize, near: usize) -> bool {
        self.capture_glitches
            .iter()
            .any(|&g| (g as usize).abs_diff(frame) <= near)
    }
}

/// A Unix ms time as the local clock reads, for people (hh:mm:ss.mmm, UTC offset not known
/// here, so it is printed as UTC with the raw milliseconds beside it).
pub fn clock(unix_ms: f64) -> String {
    let ms = unix_ms.max(0.0) as u64;
    let s = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03} UTC ({ms})",
        (s / 3600) % 24,
        (s / 60) % 60,
        s % 60,
        ms % 1000
    )
}
