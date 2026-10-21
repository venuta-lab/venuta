//! Detector 4: dips and gaps in the level. The level in 2.5 ms windows (every 0.5 ms, so a gap of
//! 5 ms always holds a whole window) is compared with the median of the 200 ms around it; a
//! window more than 20 dB under it is a dip. Silence
//! before the first sound and after the last, the music's own rise and fall at its edges (the
//! first and last 100 ms), and the declared spans (pauses, boundaries) are not judged, nor are
//! places whose surroundings are near silence themselves (under -60 dBFS).

use crate::{Click, Declared, Flags};

/// A window this far under the median around it is a dip (20 dB).
const UNDER: f64 = 0.1;
/// Surroundings quieter than this (-60 dBFS) aren't judged.
const QUIET: f64 = 0.001;

pub struct Dip {
    window: usize,
    hop: usize,
    around: usize,
    edge: usize,
}

impl Dip {
    pub fn new(rate: u32) -> Dip {
        let tenths = |t: u32| (rate * t / 10_000) as usize;
        Dip {
            window: tenths(25),
            hop: tenths(5).max(1),
            around: tenths(1_000),
            edge: tenths(1_000),
        }
    }

    /// Every dip in `x` (one channel, -1..1).
    pub fn check(&self, x: &[f64], declared: &Declared) -> Vec<Click> {
        let mut flags = Flags::new(self.window * 4);
        let Some(first) = x.iter().position(|v| v.abs() > QUIET * 0.1) else {
            return Vec::new();
        };
        let last = x.iter().rposition(|v| v.abs() > QUIET * 0.1).unwrap();
        if last - first < 2 * self.edge + self.window {
            return Vec::new();
        }
        // The level of every window between the edges.
        let starts: Vec<usize> = (first..=last + 1 - self.window).step_by(self.hop).collect();
        let level: Vec<f64> = starts
            .iter()
            .map(|&s| {
                let w = &x[s..s + self.window];
                (w.iter().map(|v| v * v).sum::<f64>() / w.len() as f64).sqrt()
            })
            .collect();
        let reach = self.around / self.hop;
        let mut near = Vec::with_capacity(2 * reach + 1);
        for (k, &s) in starts.iter().enumerate() {
            if s < first + self.edge
                || s + self.window > last + 1 - self.edge
                || declared.overlaps(s..s + self.window)
            {
                continue;
            }
            near.clear();
            // Every fifth window around is enough for the median, and keeps an hour quick.
            near.extend(
                level[k.saturating_sub(reach)..(k + reach + 1).min(level.len())]
                    .iter()
                    .step_by(5),
            );
            near.sort_by(f64::total_cmp);
            let median = near[near.len() / 2];
            if median > QUIET && level[k] < median * UNDER {
                flags.add(s, "the level dropped out", median / level[k].max(1e-12));
            }
        }
        flags.done()
    }
}
