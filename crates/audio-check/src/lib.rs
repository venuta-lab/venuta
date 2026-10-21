//! Ears for tests: what a player played is judged by detectors, never by ear (section 7.1 of
//! the Listen together design). Every detector proves itself first on injected faults
//! (`faults`, and `tests/self_test.rs`): one dropped frame, one repeated frame, a 1-sample step
//! of -40 dBFS at a peak, a 5 ms gap, a hard cut, a gain step.
//!
//! - [`music`]: the music-shaped test songs and the fast first check (steps, breaks in the
//!   line, hard-edged gaps), moved here from the player's tests.
//! - [`exact`]: detector 1, bit-exact with declared edits.
//! - [`sine`]: detector 2, sine lock (after OboeTester's glitch analyzer).
//! - [`lpc`]: detector 3, linear-prediction residual on real music (after Essentia's click
//!   detector), calibrated on the same music played alone.
//! - [`dip`]: detector 4, dips and gaps in the level.
//!
//! Signals are one channel at a time: 24-bit values for `music` and `exact` (what the player's
//! counting songs and memory devices hold), -1..1 for the others. No dependency on the player,
//! so the player's, the rooms' and the app's tests all use it.

use std::ops::Range;

pub mod dip;
pub mod exact;
pub mod faults;
pub mod lpc;
pub mod music;
pub mod sine;

pub use music::Click;

/// The spans (in frames of what was heard) where the renderer said it changed the music on
/// purpose: fades, crossfades, resampled spans, pauses, song boundaries. A detector judges them
/// by their own rule, or not at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    spans: Vec<Range<usize>>,
}

impl Declared {
    pub fn none() -> Declared {
        Declared::default()
    }

    pub fn new(mut spans: Vec<Range<usize>>) -> Declared {
        spans.sort_by_key(|r| r.start);
        Declared { spans }
    }

    /// One declared span.
    pub fn span(r: Range<usize>) -> Declared {
        Declared { spans: vec![r] }
    }

    /// Whether frame `i` is inside a declared span.
    pub fn covers(&self, i: usize) -> bool {
        self.overlaps(i..i + 1)
    }

    /// Whether any frame of `r` is inside a declared span.
    pub fn overlaps(&self, r: Range<usize>) -> bool {
        let k = self.spans.partition_point(|s| s.end <= r.start);
        self.spans.get(k).is_some_and(|s| s.start < r.end)
    }

    pub fn spans(&self) -> &[Range<usize>] {
        &self.spans
    }
}

/// Collects flags, one per event however many frames it touches (flags closer than `apart`
/// frames to the one before are the same event).
pub(crate) struct Flags {
    found: Vec<Click>,
    apart: usize,
}

impl Flags {
    pub(crate) fn new(apart: usize) -> Flags {
        Flags {
            found: Vec::new(),
            apart,
        }
    }

    pub(crate) fn add(&mut self, at: usize, what: &'static str, over: f64) {
        if self
            .found
            .last()
            .is_some_and(|c| at < c.at + self.apart && at >= c.at)
        {
            return;
        }
        self.found.push(Click { at, what, over });
    }

    pub(crate) fn done(self) -> Vec<Click> {
        self.found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_spans_cover_what_they_say() {
        let d = Declared::new(vec![50..60, 10..20]);
        assert!(!d.covers(9) && d.covers(10) && d.covers(19) && !d.covers(20));
        assert!(d.overlaps(0..11) && !d.overlaps(20..50) && d.overlaps(59..70));
        assert!(!Declared::none().covers(0));
    }
}
