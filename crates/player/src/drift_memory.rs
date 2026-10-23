//! The drift each output was measured at, remembered (design 4.11 point 4, the WP15 notes in
//! 9.2): a room measures its output's sound card against the monotonic clock (the
//! feed-forward's part that belongs to the output); when it lets the output go, what it
//! measured is kept by the output's key, and the next room on that output starts from it
//! while its own measure is still short. Read and written only by room programs: solo playback
//! never sees it.
//!
//! Small on purpose: a per-device store keyed by the endpoint id is coming (fix/devices); this
//! is the part of it a room needs (the key, the two numbers, `recall` and `remember`), to be
//! merged into it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A measure shorter than this is not remembered, s.
pub const LEAST_SPAN_S: f64 = 8.0;
/// A rate further off than this is not a sound card's, ppm.
pub const MOST_PPM: f64 = 1_000.0;

/// What was measured of one output.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    /// How fast its sound card runs against the monotonic clock (positive: fast).
    pub ppm: f64,
    /// Over how long it was measured, s.
    pub span_s: f64,
}

/// The rates learned, by output key (the endpoint id, else `name:` and the output's name).
#[derive(Debug, Default)]
pub struct DriftMemory {
    path: Option<PathBuf>,
    by_output: Mutex<HashMap<String, Learned>>,
}

impl DriftMemory {
    /// Kept in memory only.
    pub fn in_memory() -> DriftMemory {
        DriftMemory::default()
    }

    /// Kept in the file at `path` too: read now (a missing or broken file is an empty
    /// memory), written by `save`.
    pub fn open(path: &Path) -> DriftMemory {
        let by_output = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, Learned>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, l)| l.ppm.is_finite() && l.ppm.abs() <= MOST_PPM)
            .collect();
        DriftMemory {
            path: Some(path.to_owned()),
            by_output: Mutex::new(by_output),
        }
    }

    /// The rate last learned for this output.
    pub fn recall(&self, output: &str) -> Option<f64> {
        self.lock().get(output).map(|l| l.ppm)
    }

    /// What a room measured of this output (`span_s` of presentation pairs): kept when long
    /// enough and plausible.
    pub fn remember(&self, output: &str, ppm: f64, span_s: f64) {
        if span_s >= LEAST_SPAN_S && ppm.is_finite() && ppm.abs() <= MOST_PPM {
            self.lock()
                .insert(output.to_owned(), Learned { ppm, span_s });
        }
    }

    /// Write the file (when there is one): whole, a temporary file renamed over it.
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(&*self.lock()).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Learned>> {
        self.by_output.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_is_remembered_only_when_measured_long_enough_and_plausible() {
        let m = DriftMemory::in_memory();
        m.remember("a", 41.5, 3.0);
        assert_eq!(m.recall("a"), None, "too short");
        m.remember("a", 41.5, 9.5);
        assert_eq!(m.recall("a"), Some(41.5));
        m.remember("a", 5_000.0, 10.0);
        m.remember("a", f64::NAN, 10.0);
        assert_eq!(m.recall("a"), Some(41.5), "no card is that far off");
        m.remember("a", -12.0, 10.0);
        assert_eq!(m.recall("a"), Some(-12.0), "the newest");
        assert_eq!(m.recall("b"), None);
    }

    #[test]
    fn the_memory_goes_through_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("together-drift.json");
        let m = DriftMemory::open(&path);
        assert_eq!(m.recall("x"), None, "no file yet");
        m.remember("{0.0.0.00000000}.{abc}", 77.25, 10.0);
        m.save().unwrap();
        let again = DriftMemory::open(&path);
        assert_eq!(again.recall("{0.0.0.00000000}.{abc}"), Some(77.25));
        std::fs::write(&path, b"[broken").unwrap();
        assert_eq!(
            DriftMemory::open(&path).recall("{0.0.0.00000000}.{abc}"),
            None
        );
    }
}
