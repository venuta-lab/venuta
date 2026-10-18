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

/// How many recent samples are kept.
const WINDOW: usize = 16;
/// Of those, how many of the quickest are averaged.
const BEST: usize = 3;

/// A guest's estimate of the host's clock.
#[derive(Debug, Clone, Default)]
pub struct Clock {
    samples: Vec<Sample>,
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
    }

    /// How many answers it has.
    pub fn samples(&self) -> usize {
        self.samples.len()
    }

    /// Enough answers to trust it (a burst at joining gives them in a second).
    pub fn ready(&self) -> bool {
        self.samples.len() >= 4
    }

    /// The host's clock minus this one's, in microseconds: the mean of the quickest samples'.
    pub fn offset(&self) -> Option<i64> {
        let best = self.quickest();
        if best.is_empty() {
            return None;
        }
        Some(best.iter().map(Sample::offset).sum::<i64>() / best.len() as i64)
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

    fn quickest(&self) -> Vec<Sample> {
        let mut s = self.samples.clone();
        s.sort_by_key(Sample::delay);
        s.truncate(BEST);
        s
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
        for i in 0..40 {
            c.add(ping(i, 0, 10, 10));
        }
        assert_eq!(c.samples(), WINDOW);
    }
}
