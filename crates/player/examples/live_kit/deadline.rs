//! The render deadline histogram (sections 4.7 and 7.5): for each period, the share of the
//! period left between the buffer being handed back to the device and the moment it would
//! have been late. 100% is a renderer that took no time; 0% or less is a period that missed.
//!
//! The same shape is read from two places:
//! - `deadline_probe probe` measures it here, with its own render loop on the app's WASAPI
//!   output code;
//! - the playback log, where the new engine's status thread writes it once a minute and when
//!   the output stops, as one line (the format is in `to_log_line`, and in
//!   docs/qa/listen-together-live.md).

/// Buckets of 1% of the period, from 0% (bucket 0) to 100%; periods with no margin at all
/// (late) are counted apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    pub buckets: [u64; 100],
    pub late: u64,
    /// Times the device ran dry (it played silence it was not given).
    pub ran_dry: u64,
    /// Seconds the histogram covers.
    pub seconds: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            buckets: [0; 100],
            late: 0,
            ran_dry: 0,
            seconds: 0,
        }
    }
}

pub const LOG_TAG: &str = "render: deadline margin";

impl Histogram {
    /// One period with `margin` (a share of the period: 0.7 is 70% left).
    pub fn record(&mut self, margin: f64) {
        if margin.is_nan() || margin <= 0.0 {
            self.late += 1;
        } else {
            let b = ((margin * 100.0).floor() as usize).min(99);
            self.buckets[b] += 1;
        }
    }

    pub fn periods(&self) -> u64 {
        self.late + self.buckets.iter().sum::<u64>()
    }

    pub fn add(&mut self, o: &Histogram) {
        for (a, b) in self.buckets.iter_mut().zip(o.buckets.iter()) {
            *a += b;
        }
        self.late += o.late;
        self.ran_dry += o.ran_dry;
        self.seconds += o.seconds;
    }

    /// The margin that all but a share `q` of the periods had at least (the lower edge of its
    /// bucket, so never better than the truth): `low(0.0001)` is the p99.99 of section 7.5.
    /// `None` when a share `q` or more of the periods were late, or there are none.
    pub fn low(&self, q: f64) -> Option<f64> {
        let n = self.periods();
        if n == 0 {
            return None;
        }
        // The worst periods allowed below the answer.
        let allowed = (q * n as f64).floor() as u64;
        if self.late > allowed {
            return None;
        }
        let mut below = self.late;
        for (b, &c) in self.buckets.iter().enumerate() {
            below += c;
            if below > allowed {
                return Some(b as f64 / 100.0);
            }
        }
        Some(0.99)
    }

    /// The worst period's margin (lower bucket edge); `None` if any was late.
    pub fn min(&self) -> Option<f64> {
        if self.late > 0 {
            return None;
        }
        self.buckets
            .iter()
            .position(|&c| c > 0)
            .map(|b| b as f64 / 100.0)
    }

    /// One line for the playback log.
    pub fn to_log_line(&self) -> String {
        let cells: Vec<String> = self.buckets.iter().map(u64::to_string).collect();
        format!(
            "{LOG_TAG} over {} s: {} periods, ran dry {}, late {}; per 1% of the period from 0%: {}",
            self.seconds,
            self.periods(),
            self.ran_dry,
            self.late,
            cells.join(" ")
        )
    }

    /// The histogram in a playback log line, if the line has one.
    pub fn from_log_line(line: &str) -> Option<Histogram> {
        let at = line.find(LOG_TAG)?;
        let rest = &line[at + LOG_TAG.len()..];
        let num_after = |key: &str| -> Option<u64> {
            let i = rest.find(key)? + key.len();
            rest[i..]
                .trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        };
        let seconds = num_after("over")?;
        let ran_dry = num_after("ran dry")?;
        let late = num_after("late")?;
        let cells_at = rest.find("from 0%:")? + "from 0%:".len();
        let cells: Vec<u64> = rest[cells_at..]
            .split_whitespace()
            .take_while(|w| w.chars().all(|c| c.is_ascii_digit()))
            .filter_map(|w| w.parse().ok())
            .collect();
        if cells.len() != 100 {
            return None;
        }
        let mut h = Histogram {
            seconds,
            ran_dry,
            late,
            ..Histogram::default()
        };
        h.buckets.copy_from_slice(&cells);
        Some(h)
    }

    /// A readout for people: the percentiles of section 7.5 and a bar chart in 10% rows.
    pub fn readout(&self) -> String {
        let pct = |v: Option<f64>| match v {
            Some(v) => format!("{:.0}%", v * 100.0),
            None => "late".into(),
        };
        let mut s = format!(
            "{} periods over {} s; ran dry {}, late {}\n  margin p50 {}, p99 {}, p99.9 {}, p99.99 {}, worst {}\n",
            self.periods(),
            self.seconds,
            self.ran_dry,
            self.late,
            pct(self.low(0.5)),
            pct(self.low(0.01)),
            pct(self.low(0.001)),
            pct(self.low(0.0001)),
            pct(self.min()),
        );
        let n = self.periods().max(1) as f64;
        if self.late > 0 {
            s += &format!("  late      {:>9}\n", self.late);
        }
        for row in 0..10 {
            let c: u64 = self.buckets[row * 10..row * 10 + 10].iter().sum();
            let bar = "#".repeat(((c as f64 / n) * 50.0).ceil() as usize);
            s += &format!("  {:>3}-{:<3}% {:>9} {bar}\n", row * 10, row * 10 + 10, c);
        }
        s
    }

    /// Section 7.5: p99.99 of the margin over 30% and no period dry.
    pub fn passes(&self) -> bool {
        self.ran_dry == 0 && self.late == 0 && self.low(0.0001).is_some_and(|m| m >= 0.30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_line_reads_back() {
        let mut h = Histogram {
            seconds: 60,
            ran_dry: 1,
            ..Histogram::default()
        };
        for i in 0..6000 {
            h.record(0.4 + (i % 50) as f64 / 100.0);
        }
        h.record(-0.1);
        let line = format!("1761000000000 {} from the player", h.to_log_line());
        assert_eq!(Histogram::from_log_line(&line), Some(h));
        assert_eq!(
            Histogram::from_log_line("1761000000000 play from the tray"),
            None
        );
    }

    #[test]
    fn percentiles_from_the_low_side() {
        let mut h = Histogram::default();
        for _ in 0..9_999 {
            h.record(0.8);
        }
        h.record(0.25);
        // One period in 10 000 at 25%: p99.99 still 80% (one is allowed under), the worst 25%.
        assert_eq!(h.low(0.0001), Some(0.8));
        assert_eq!(h.min(), Some(0.25));
        h.record(0.25);
        assert_eq!(h.low(0.0001), Some(0.25));
        assert!(!h.passes());
        h.record(-1.0);
        assert_eq!(h.min(), None);
    }
}
