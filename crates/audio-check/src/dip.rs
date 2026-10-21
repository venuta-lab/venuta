//! Detector 4: dips and gaps in the level. The level in 5 ms windows, one starting at every
//! sample (so a gap of exactly 5 ms fills one window), is compared with the median of the 200 ms
//! around it (the levels every 1 ms); a window more than 20 dB under it is a dip. Silence before
//! the first sound and after the last, the music's own rise and fall at its edges (the first and
//! last 100 ms), and the declared spans (pauses, boundaries) are not judged, nor are places whose
//! surroundings are near silence themselves (under -60 dBFS).

use crate::{Click, Declared, Flags};

/// A window this far under the median around it is a dip (20 dB).
const UNDER: f64 = 0.1;
/// Surroundings quieter than this (-60 dBFS) aren't judged.
const QUIET: f64 = 0.001;

pub struct Dip {
    window: usize,
    /// The median's grid: one level every this many samples.
    grid: usize,
    around: usize,
    edge: usize,
}

impl Dip {
    pub fn new(rate: u32) -> Dip {
        let ms = |m: u32| (rate * m / 1000) as usize;
        Dip {
            window: ms(5).max(1),
            grid: ms(1).max(1),
            around: ms(100),
            edge: ms(100),
        }
    }

    /// Every dip in `x` (one channel, -1..1).
    pub fn check(&self, x: &[f64], declared: &Declared) -> Vec<Click> {
        let mut flags = Flags::new(self.window * 2);
        let Some(first) = x.iter().position(|v| v.abs() > QUIET * 0.1) else {
            return Vec::new();
        };
        let last = x.iter().rposition(|v| v.abs() > QUIET * 0.1).unwrap();
        if last - first < 2 * self.edge + self.window {
            return Vec::new();
        }
        // Running sums of squares: the level of the window at any start in one step.
        let mut sum = Vec::with_capacity(x.len() + 1);
        sum.push(0.0f64);
        for v in x {
            sum.push(sum.last().unwrap() + v * v);
        }
        let w = self.window;
        let level = |s: usize| ((sum[s + w] - sum[s]).max(0.0) / w as f64).sqrt();
        // The levels on the grid, for the medians.
        let grid: Vec<f64> = (first..=last + 1 - w)
            .step_by(self.grid)
            .map(level)
            .collect();
        let reach = self.around / self.grid;
        let mut near = Vec::with_capacity(2 * reach + 1);
        let mut median_at = None;
        for s in first + self.edge..=last + 1 - self.edge - w {
            if declared.overlaps(s..s + w) {
                continue;
            }
            let g = (s - first) / self.grid;
            if median_at.is_none_or(|(at, _)| at != g) {
                near.clear();
                near.extend_from_slice(
                    &grid[g.saturating_sub(reach)..(g + reach + 1).min(grid.len())],
                );
                near.sort_by(f64::total_cmp);
                median_at = Some((g, near[near.len() / 2]));
            }
            let median = median_at.map_or(0.0, |m| m.1);
            let l = level(s);
            if median > QUIET && l < median * UNDER {
                flags.add(s, "the level dropped out", median / l.max(1e-12));
            }
        }
        flags.done()
    }
}
