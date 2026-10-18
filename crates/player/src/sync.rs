//! Playing in step with others (listening together): the room says where the song should be
//! at each moment, and the engine keeps what comes out of the speaker there.
//!
//! Two ways to stay together:
//! - **Apart** (friends in different places, the default): nobody can hear the others' speakers,
//!   so within ~50 ms is perfect. The sound stays bit-perfect: when the song drifts too far
//!   (sound cards' clocks differ by up to ~100 ppm, about 24 ms over a song), one small seek
//!   brings it back.
//! - **Same room** (speakers that hear each other): within a few milliseconds. Drift is taken out
//!   by dropping or repeating single frames, at most 500 per million (inaudible, under a cent of
//!   pitch), Snapcast's way. Not bit-perfect while it corrects.

use std::time::Instant;

/// What the room asks of this player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncPlan {
    /// When the song is at `position_ms` (this computer's clock); `None` while it waits for the
    /// room to start: the song is made ready there, silent.
    pub anchor: Option<Instant>,
    pub position_ms: u64,
    pub playing: bool,
    /// Speakers in one room: correct by single frames.
    pub same_room: bool,
    /// How much later than the device says the sound is heard (Bluetooth), milliseconds.
    pub extra_latency_ms: i32,
}

impl SyncPlan {
    /// Where the song should be heard at `now` (milliseconds), once started.
    pub fn expected_ms(&self, now: Instant) -> Option<f64> {
        let anchor = self.anchor?;
        if !self.playing {
            return Some(self.position_ms as f64);
        }
        let elapsed = if now >= anchor {
            now.duration_since(anchor).as_secs_f64() * 1_000.0
        } else {
            -(anchor.duration_since(now).as_secs_f64() * 1_000.0)
        };
        Some(self.position_ms as f64 + elapsed)
    }
}

/// What to do about an error (heard minus expected, milliseconds; positive is ahead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Correction {
    /// Close enough.
    None,
    /// Move to where the room is (a hard resync).
    Seek,
    /// Take out (behind) or repeat (ahead) single frames at this many per million.
    Trim { ppm: f64, ahead: bool },
}

/// Apart: further than this, a seek (smaller drifts aren't heard by anyone).
pub const APART_SEEK_MS: f64 = 80.0;
/// Same room: under this, nothing to do.
pub const SAME_ROOM_DEAD_MS: f64 = 0.5;
/// Same room: further than this, a seek.
pub const SAME_ROOM_SEEK_MS: f64 = 50.0;
/// The most frames per million taken out or repeated.
pub const MAX_PPM: f64 = 500.0;

/// The correction for an error. `settling` is true just after a start or a seek, while the
/// device fills (no correction then).
pub fn correction(error_ms: f64, same_room: bool, settling: bool) -> Correction {
    if settling {
        return Correction::None;
    }
    let e = error_ms.abs();
    if !same_room {
        return if e > APART_SEEK_MS {
            Correction::Seek
        } else {
            Correction::None
        };
    }
    if e > SAME_ROOM_SEEK_MS {
        Correction::Seek
    } else if e > SAME_ROOM_DEAD_MS {
        // 100 ppm per millisecond off, up to 500: a few seconds to close a small gap, smoothly.
        Correction::Trim {
            ppm: (e * 100.0).min(MAX_PPM),
            ahead: error_ms > 0.0,
        }
    } else {
        Correction::None
    }
}

/// Takes out or repeats single frames in interleaved audio, spread evenly: a running fraction
/// carries over between device periods so the rate is exact over time.
#[derive(Debug, Default, Clone)]
pub struct Trimmer {
    owed: f64,
}

impl Trimmer {
    /// How many frames to change in a period of `frames` at `ppm`.
    pub fn due(&mut self, frames: usize, ppm: f64) -> usize {
        self.owed += frames as f64 * ppm / 1_000_000.0;
        // A hair over, so sums like 100 x 0.24 come to 24, not 23.
        let n = (self.owed + 1e-9).floor();
        self.owed -= n;
        n as usize
    }

    pub fn reset(&mut self) {
        self.owed = 0.0;
    }
}

/// Repeat `n` frames of `samples` (interleaved, `ch` channels), spread through it: the audio gets
/// `n` frames longer (we were ahead).
pub fn repeat_frames(samples: &mut Vec<i32>, ch: usize, n: usize) {
    let frames = samples.len() / ch.max(1);
    if n == 0 || frames == 0 {
        return;
    }
    let mut out = Vec::with_capacity(samples.len() + n * ch);
    let step = (frames / (n + 1)).max(1);
    let mut left = n;
    for (i, f) in samples.chunks_exact(ch).enumerate() {
        out.extend_from_slice(f);
        if left > 0 && (i + 1) % step == 0 {
            out.extend_from_slice(f);
            left -= 1;
        }
    }
    *samples = out;
}

/// Take out `n` frames of `samples` (interleaved, `ch` channels), spread through it: the audio gets
/// `n` frames shorter (we were behind).
pub fn drop_frames(samples: &mut Vec<i32>, ch: usize, n: usize) {
    let frames = samples.len() / ch.max(1);
    if n == 0 || frames <= n {
        return;
    }
    let step = (frames / (n + 1)).max(1);
    let mut out = Vec::with_capacity(samples.len() - n * ch);
    let mut left = n;
    for (i, f) in samples.chunks_exact(ch).enumerate() {
        if left > 0 && (i + 1) % step == 0 {
            left -= 1;
            continue;
        }
        out.extend_from_slice(f);
    }
    *samples = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_expected_place_moves_with_the_clock() {
        let t0 = Instant::now();
        let p = SyncPlan {
            anchor: Some(t0),
            position_ms: 10_000,
            playing: true,
            same_room: false,
            extra_latency_ms: 0,
        };
        let at = p.expected_ms(t0 + Duration::from_millis(1_500)).unwrap();
        assert!((at - 11_500.0).abs() < 0.01);
        assert!(p.expected_ms(t0).unwrap() >= 10_000.0 - 0.01);
        let waiting = SyncPlan { anchor: None, ..p };
        assert_eq!(waiting.expected_ms(t0), None, "not started");
        let paused = SyncPlan {
            playing: false,
            ..p
        };
        assert_eq!(
            paused.expected_ms(t0 + Duration::from_secs(9)),
            Some(10_000.0)
        );
    }

    #[test]
    fn apart_listeners_stay_bit_perfect_until_far_off() {
        assert_eq!(correction(40.0, false, false), Correction::None);
        assert_eq!(correction(-79.0, false, false), Correction::None);
        assert_eq!(correction(81.0, false, false), Correction::Seek);
        assert_eq!(
            correction(300.0, false, true),
            Correction::None,
            "not while settling"
        );
    }

    #[test]
    fn same_room_trims_gently_and_seeks_when_far() {
        assert_eq!(correction(0.3, true, false), Correction::None);
        assert_eq!(
            correction(2.0, true, false),
            Correction::Trim {
                ppm: 200.0,
                ahead: true
            }
        );
        assert_eq!(
            correction(-20.0, true, false),
            Correction::Trim {
                ppm: 500.0,
                ahead: false
            }
        );
        assert_eq!(correction(60.0, true, false), Correction::Seek);
    }

    #[test]
    fn trimming_is_exact_over_time() {
        let mut t = Trimmer::default();
        // 500 ppm over 48,000 frames a second, in 480-frame periods: 24 frames a second.
        let per_second: usize = (0..100).map(|_| t.due(480, 500.0)).sum();
        assert_eq!(per_second, 24);
    }

    #[test]
    fn frames_are_repeated_or_taken_out_whole() {
        // Stereo: frame i is [i, -i].
        let base: Vec<i32> = (0..10).flat_map(|i| [i, -i]).collect();
        let mut more = base.clone();
        repeat_frames(&mut more, 2, 2);
        assert_eq!(more.len(), base.len() + 4);
        assert!(
            more.as_chunks::<2>().0.iter().all(|f| f[0] == -f[1]),
            "channels stay together"
        );
        let mut fewer = base.clone();
        drop_frames(&mut fewer, 2, 2);
        assert_eq!(fewer.len(), base.len() - 4);
        assert!(fewer.as_chunks::<2>().0.iter().all(|f| f[0] == -f[1]));
        let mut none = base.clone();
        drop_frames(&mut none, 2, 0);
        assert_eq!(none, base);
    }
}
