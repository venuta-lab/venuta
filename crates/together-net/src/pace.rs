//! Pacing of the host's files (design 4.1): each range stream may send a first burst worth
//! 10 s of the song's average bitrate, then twice that bitrate. A guest still downloads the
//! whole song ahead of where it plays, only more evenly, so the host's upload is never filled
//! by one person's file and everyone's room messages and clock pings go through.

use std::time::{Duration, Instant};

/// The first burst, in seconds of the song.
pub const BURST_S: f64 = 10.0;
/// After it, how many times the song's average bitrate.
pub const RATE_TIMES: f64 = 2.0;

/// A token bucket in bytes.
#[derive(Debug, Clone)]
pub struct Pacer {
    /// Bytes a second.
    rate: f64,
    /// The most it holds (the burst).
    cap: f64,
    tokens: f64,
    last: Instant,
}

impl Pacer {
    /// For a file of `size` bytes holding `duration_ms` of music; `None` when its length isn't
    /// known (then it goes as fast as the connection takes it).
    pub fn for_song(size: u64, duration_ms: Option<u64>, now: Instant) -> Option<Pacer> {
        let secs = duration_ms.filter(|&d| d > 0)? as f64 / 1_000.0;
        let average = size as f64 / secs;
        if average <= 0.0 {
            return None;
        }
        let cap = (average * BURST_S).max(64.0 * 1024.0);
        Some(Pacer {
            rate: average * RATE_TIMES,
            cap,
            tokens: cap,
            last: now,
        })
    }

    /// How much to send at a time: about 20 ms of the rate (4 KiB to 64 KiB), so a paced file
    /// never puts more than a moment's worth on the link at once.
    pub fn chunk(&self) -> usize {
        ((self.rate * 0.02) as usize).clamp(4 * 1024, 64 * 1024)
    }

    /// How long to wait before sending `n` bytes at `now` (they are taken from the bucket).
    pub fn take(&mut self, n: usize, now: Instant) -> Duration {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * self.rate).min(self.cap);
        self.tokens -= n as f64;
        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-self.tokens / self.rate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_ten_seconds_then_twice_the_bitrate() {
        let t0 = Instant::now();
        // 100 kB a second: a burst of 1 MB, then 200 kB a second.
        let mut p = Pacer::for_song(3_000_000, Some(30_000), t0).unwrap();
        for _ in 0..10 {
            assert_eq!(p.take(100_000, t0), Duration::ZERO);
        }
        assert_eq!(p.take(100_000, t0), Duration::from_millis(500));
        // Waited as told, the next 100 kB needs another half second.
        let t1 = t0 + Duration::from_millis(500);
        assert_eq!(p.take(100_000, t1), Duration::from_millis(500));
        // Over a long run it averages twice the bitrate.
        let mut t = t1 + Duration::from_millis(500);
        let mut sent = 0u64;
        while sent < 2_000_000 {
            t += p.take(50_000, t);
            sent += 50_000;
        }
        let secs = (t - t1 - Duration::from_millis(500)).as_secs_f64();
        assert!((secs - 10.0).abs() < 0.3, "{secs}");
    }

    #[test]
    fn idle_time_refills_only_up_to_the_burst() {
        let t0 = Instant::now();
        let mut p = Pacer::for_song(3_000_000, Some(30_000), t0).unwrap();
        assert_eq!(p.take(1_000_000, t0), Duration::ZERO);
        let later = t0 + Duration::from_secs(60);
        assert_eq!(p.take(1_000_000, later), Duration::ZERO);
        assert!(p.take(100_000, later) > Duration::ZERO);
    }

    #[test]
    fn no_length_no_pacing() {
        let now = Instant::now();
        assert!(Pacer::for_song(3_000_000, None, now).is_none());
        assert!(Pacer::for_song(3_000_000, Some(0), now).is_none());
        assert!(Pacer::for_song(0, Some(1_000), now).is_none());
    }

    #[test]
    fn a_tiny_file_still_gets_a_burst_worth_a_chunk() {
        let now = Instant::now();
        let mut p = Pacer::for_song(10_000, Some(100_000), now).unwrap();
        assert_eq!(p.take(64 * 1024, now), Duration::ZERO);
    }
}
