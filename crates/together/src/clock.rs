//! The room's shared clock: the host's, as each guest estimates it from pings.
//!
//! A guest sends a ping at its time `t1`; the host stamps when it got it (`t2`) and when it
//! answered (`t3`); the guest stamps the answer's arrival (`t4`). Then (the NTP method):
//! - offset = ((t2 - t1) + (t3 - t4)) / 2, the host's clock minus the guest's;
//! - delay = (t4 - t1) - (t3 - t2), the round trip without the host's own time.
//!
//! Networks have a long tail of slow answers (Wi-Fi, the internet), and a slow answer's offset
//! is off by up to half its delay; so the estimate trusts the samples with the least delay, as
//! Jellyfin's SyncPlay does, among the last few. All times are microseconds on each side's
//! monotonic clock, never the wall clock (Windows adjusting the time can't break a session).

/// One ping's four timestamps, in microseconds (t1 and t4 the guest's, t2 and t3 the host's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub t1: i64,
    pub t2: i64,
    pub t3: i64,
    pub t4: i64,
}

impl Sample {
    /// The host's clock minus the guest's.
    pub fn offset(&self) -> i64 {
        ((self.t2 - self.t1) + (self.t3 - self.t4)) / 2
    }

    /// The round trip, the host's own time taken out.
    pub fn delay(&self) -> i64 {
        ((self.t4 - self.t1) - (self.t3 - self.t2)).max(0)
    }

    /// Whether the timestamps make sense (an answer can't come before its ping).
    fn sane(&self) -> bool {
        self.t4 >= self.t1 && self.t3 >= self.t2
    }
}

/// How many recent samples are kept (a minute or so of pings: long enough to see the two
/// clocks' rates differ).
const WINDOW: usize = 64;
/// Of those, how many of the quickest are averaged while there are too few to fit a line.
const BEST: usize = 3;
/// A sample's delay may be over the quickest by at least this (microseconds) and still count.
const SLACK_US: i64 = 1_000;
/// A fitted line (offset and rate) needs this many samples over this long (microseconds).
const FIT_SAMPLES: usize = 8;
const FIT_SPAN_US: i64 = 30_000_000;
/// The clock the players read moves toward the estimate this fast at most (microseconds per
/// second): a better estimate is taken up without a step anyone hears.
const SLEW_US_PER_S: i64 = 50;
/// A gap this big (microseconds) that lasts `STEP_AFTER_US` is a real change (the path to the
/// host changed): stepped, not slewed.
const STEP_US: i64 = 20_000;
const STEP_AFTER_US: i64 = 10_000_000;
/// Slewing only once this many answers are in (the burst at joining steps freely).
const SETTLED: usize = 16;

/// A guest's estimate of the host's clock.
///
/// The quickest answers are kept (within a little of the least delay); through them a line is
/// fitted, offset and rate (the two clocks tick at slightly different rates, tens of parts per
/// million), once there are enough over a long enough time; before that, the mean of the three
/// quickest. What the players read is that estimate slewed: moved toward it by at most 50 µs a
/// second, so each better estimate is taken up without a step, except a change of over 20 ms
/// that lasts 10 s (the path to the host changed), which is stepped.
#[derive(Debug, Clone, Default)]
pub struct Clock {
    samples: Vec<Sample>,
    /// What the players read: the offset at a local time (microseconds each).
    shown: Option<(i64, i64)>,
    /// The fitted rate: the host's clock gains this many microseconds per local microsecond.
    skew: f64,
    /// Since when (local time) the estimate has been a step away from what's shown.
    far_since: Option<i64>,
}

impl Clock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take one ping's answer (a nonsense one is left out).
    pub fn add(&mut self, s: Sample) {
        if !s.sane() {
            return;
        }
        self.samples.push(s);
        if self.samples.len() > WINDOW {
            self.samples.remove(0);
        }
        let Some((a, b, t_ref)) = self.fit() else {
            return;
        };
        self.skew = b;
        let now = s.t4;
        let target = a + (b * (now - t_ref) as f64) as i64;
        let Some((t0, off0)) = self.shown.filter(|_| self.samples.len() >= SETTLED) else {
            self.shown = Some((now, target));
            self.far_since = None;
            return;
        };
        let dt = (now - t0).max(0);
        // Where what's shown is by now, the clocks' rates taken into account.
        let cur = off0 + (b * dt as f64) as i64;
        let gap = target - cur;
        if gap.abs() > STEP_US {
            let since = *self.far_since.get_or_insert(now);
            if now - since >= STEP_AFTER_US {
                self.shown = Some((now, target));
                self.far_since = None;
                return;
            }
        } else {
            self.far_since = None;
        }
        let most = SLEW_US_PER_S * dt / 1_000_000;
        self.shown = Some((now, cur + gap.clamp(-most, most)));
    }

    /// How many answers it has.
    pub fn samples(&self) -> usize {
        self.samples.len()
    }

    /// Enough answers to trust it (a burst at joining gives them in a second).
    pub fn ready(&self) -> bool {
        self.samples.len() >= 4
    }

    /// The host's clock minus this one's, in microseconds, as the players read it (slewed).
    pub fn offset(&self) -> Option<i64> {
        self.shown.map(|(_, o)| o)
    }

    /// How much faster the host's clock runs than this one, in parts per million (0 until a
    /// line can be fitted).
    pub fn skew_ppm(&self) -> f64 {
        self.skew * 1e6
    }

    /// The round trip of the quickest answer, in microseconds (how good the estimate can be:
    /// its error is at most half of it).
    pub fn delay(&self) -> Option<i64> {
        self.samples.iter().map(Sample::delay).min()
    }

    /// The host's time now, from this clock's time now.
    pub fn host_time(&self, local_us: i64) -> Option<i64> {
        self.offset().map(|o| local_us + o)
    }

    /// This clock's time for a moment on the host's clock.
    pub fn local_time(&self, host_us: i64) -> Option<i64> {
        self.offset().map(|o| host_us - o)
    }

    /// The samples worth trusting: within a little of the least delay (half the way to the
    /// middle delay, or a millisecond, whichever is more).
    fn selected(&self) -> Vec<Sample> {
        let mut delays: Vec<i64> = self.samples.iter().map(Sample::delay).collect();
        if delays.is_empty() {
            return Vec::new();
        }
        delays.sort_unstable();
        let least = delays[0];
        let middle = delays[delays.len() / 2];
        let line = least + ((middle - least) / 2).max(SLACK_US);
        self.samples
            .iter()
            .filter(|s| s.delay() <= line)
            .copied()
            .collect()
    }

    /// The estimate: (offset at `t_ref`, rate, `t_ref`), a line through the trusted samples
    /// (weighted toward the quickest) when there are enough over long enough; else the mean of
    /// the quickest few, flat.
    fn fit(&self) -> Option<(i64, f64, i64)> {
        let chosen = self.selected();
        let first = chosen.first()?.t1;
        let last = chosen.last()?.t1;
        if chosen.len() >= FIT_SAMPLES && last - first >= FIT_SPAN_US {
            let t_ref = (first + last) / 2;
            let (mut sw, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for s in &chosen {
                let w = 1.0 / (s.delay() + SLACK_US) as f64;
                let x = (s.t1 - t_ref) as f64;
                let y = s.offset() as f64;
                sw += w;
                sx += w * x;
                sy += w * y;
                sxx += w * x * x;
                sxy += w * x * y;
            }
            let var = sxx - sx * sx / sw;
            if var > 0.0 {
                let b = ((sxy - sx * sy / sw) / var).clamp(-500e-6, 500e-6);
                let a = (sy - b * sx) / sw;
                return Some((a.round() as i64, b, t_ref));
            }
        }
        let mut best = chosen;
        best.sort_by_key(Sample::delay);
        best.truncate(BEST);
        let a = best.iter().map(Sample::offset).sum::<i64>() / best.len() as i64;
        Some((a, 0.0, best.last()?.t4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ping that took `up` microseconds to reach the host and `down` to come back, sent at the
    /// guest's `at`, with the host's clock `skew` ahead and 50 microseconds spent answering.
    fn ping(at: i64, skew: i64, up: i64, down: i64) -> Sample {
        let t1 = at;
        let t2 = at + up + skew;
        let t3 = t2 + 50;
        let t4 = t1 + up + 50 + down;
        Sample { t1, t2, t3, t4 }
    }

    #[test]
    fn an_even_path_gives_the_exact_offset() {
        let s = ping(1_000, 250_000, 4_000, 4_000);
        assert_eq!(s.offset(), 250_000);
        assert_eq!(s.delay(), 8_000);
    }

    #[test]
    fn slow_answers_are_outvoted_by_the_quick_ones() {
        let mut c = Clock::new();
        // Wi-Fi: mostly 3 ms each way, with a few 200 ms one-way spikes that skew their offset.
        for i in 0..12 {
            let at = i * 1_000_000;
            let s = if i % 3 == 0 {
                ping(at, -90_000, 200_000, 3_000)
            } else {
                ping(at, -90_000, 3_000, 3_000)
            };
            c.add(s);
        }
        let off = c.offset().unwrap();
        assert!(
            (off + 90_000).abs() <= 1,
            "offset {off} trusts the quick pings"
        );
        assert_eq!(c.delay(), Some(6_000));
        assert!(c.ready());
    }

    #[test]
    fn times_convert_both_ways() {
        let mut c = Clock::new();
        for i in 0..4 {
            c.add(ping(i * 10, 5_000, 1_000, 1_000));
        }
        assert_eq!(c.host_time(100), Some(5_100));
        assert_eq!(c.local_time(5_100), Some(100));
    }

    #[test]
    fn nonsense_and_old_answers_are_left_out() {
        let mut c = Clock::new();
        assert_eq!(c.offset(), None);
        c.add(Sample {
            t1: 10,
            t2: 5,
            t3: 6,
            t4: 9,
        });
        assert_eq!(c.samples(), 0, "an answer before its ping");
        for i in 0..(WINDOW as i64 + 20) {
            c.add(ping(i, 0, 10, 10));
        }
        assert_eq!(c.samples(), WINDOW, "the oldest are let go");
    }

    /// A minute of pings, one a second, the host's clock `skew_ppm` faster than ours, paths
    /// `up` and `down` microseconds (`change` can alter them per ping).
    fn minutes(
        c: &mut Clock,
        secs: i64,
        from_s: i64,
        skew_ppm: f64,
        path: impl Fn(i64) -> (i64, i64),
    ) {
        for i in from_s..from_s + secs {
            let at = i * 1_000_000;
            let skew = 250_000 + (at as f64 * skew_ppm / 1e6) as i64;
            let (up, down) = path(i);
            c.add(ping(at, skew, up, down));
        }
    }

    #[test]
    fn the_hosts_faster_clock_is_learned_and_followed() {
        let mut c = Clock::new();
        minutes(&mut c, 150, 0, 100.0, |_| (3_000, 3_000));
        assert!(
            (c.skew_ppm() - 100.0).abs() < 5.0,
            "100 ppm learned: {:.1}",
            c.skew_ppm()
        );
        // Where the host is now, as the players read it: within a fraction of a millisecond.
        let now = 149 * 1_000_000;
        let true_offset = 250_000 + now / 10_000;
        let off = c.offset().unwrap();
        assert!(
            (off - true_offset).abs() < 300,
            "offset {off} against {true_offset}"
        );
    }

    #[test]
    fn a_small_change_of_path_is_slewed_never_stepped() {
        let mut c = Clock::new();
        minutes(&mut c, 40, 0, 0.0, |_| (5_000, 5_000));
        let before = c.offset().unwrap();
        // The way there gets 20 ms longer, the way back not: the offset the pings show moves
        // by 10 ms at once (the clock can't tell), but what the players read moves by at
        // most 50 µs a second.
        minutes(&mut c, 5, 40, 0.0, |_| (25_000, 5_000));
        let after = c.offset().unwrap();
        assert!(
            (after - before).abs() <= 5 * SLEW_US_PER_S + 1,
            "moved {} µs in 5 s",
            after - before
        );
    }

    #[test]
    fn a_big_lasting_change_of_path_is_stepped_after_a_while() {
        let mut c = Clock::new();
        minutes(&mut c, 40, 0, 0.0, |_| (80_000, 80_000));
        let before = c.offset().unwrap();
        // Off the relay onto a direct path, 60 ms shorter one way than the other.
        minutes(&mut c, 8, 40, 0.0, |_| (20_000, 80_000));
        let soon = c.offset().unwrap();
        assert!(
            (soon - before).abs() <= 8 * SLEW_US_PER_S + 1,
            "slewing first ({} µs in 8 s)",
            soon - before
        );
        minutes(&mut c, 4, 48, 0.0, |_| (20_000, 80_000));
        let later = c.offset().unwrap();
        assert!(
            (later - before + 30_000).abs() < 1_000,
            "stepped to the new estimate after 10 s: moved {} µs",
            later - before
        );
    }
}
