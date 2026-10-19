//! Playing in step with others (listening together): the room says where the song should be
//! at each moment, and the engine keeps what comes out of the speaker there.
//!
//! Two ways to stay together:
//! - **Apart** (friends in different places, the default): nobody can hear the others' speakers,
//!   so smooth sound comes first. Nothing touches the music mid-song: each song lines up from
//!   silence as it starts, and only a drift past 300 ms that lasts is lined up again, faded.
//!   Bit-perfect throughout.
//! - **Same room** (speakers that hear each other, on one network): within a few milliseconds.
//!   Drift is taken out by blending single frames away or in, at most 500 per million (under a
//!   cent of pitch), Snapcast's way, on an error smoothed over a second so clock noise isn't
//!   chased. Not bit-perfect while it corrects.

use std::time::{Duration, Instant};

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
    /// Move to where the room is (a hard resync, faded).
    Seek,
    /// Take out (behind) or add (ahead) single frames at this many per million.
    Trim { ppm: f64, ahead: bool },
}

/// A new plan this close to the old one is the same plan, its start known better (same room).
pub const NUDGE_MS: f64 = 20.0;
/// Apart, a start known better by up to this much is still the same plan: the music plays on.
pub const APART_NUDGE_MS: f64 = 300.0;

/// The same plan with its start known a little better (the host's clock estimated better), not
/// a change of plan: the player keeps on, its usual corrections taking up the difference.
pub fn is_nudge(old: &SyncPlan, new: &SyncPlan, now: Instant) -> bool {
    old.playing
        && new.playing
        && old.same_room == new.same_room
        && old.extra_latency_ms == new.extra_latency_ms
        && match (
            old.anchor,
            new.anchor,
            old.expected_ms(now),
            new.expected_ms(now),
        ) {
            (Some(_), Some(_), Some(a), Some(b)) => {
                (a - b).abs()
                    <= if new.same_room {
                        NUDGE_MS
                    } else {
                        APART_NUDGE_MS
                    }
            }
            _ => false,
        }
}

/// Apart: further than this for a while (`far_for`), lined up again (smaller drifts aren't heard
/// by anyone, in another home).
pub const APART_SEEK_MS: f64 = 300.0;
/// Same room: under this (after smoothing), nothing to do; clock and position noise is a
/// millisecond or two.
pub const SAME_ROOM_DEAD_MS: f64 = 2.0;
/// Same room: further than this, a seek.
pub const SAME_ROOM_SEEK_MS: f64 = 50.0;
/// The most frames per million taken out or added.
pub const MAX_PPM: f64 = 500.0;
/// How long the error is smoothed over before it's acted on.
pub const SMOOTHING: Duration = Duration::from_secs(1);
/// How long a fade takes, either side of a seek that would otherwise be heard as a cut.
pub const FADE: Duration = Duration::from_millis(12);

/// How long an error past the seek line has to last before it is lined up: a stall or a burst
/// of late clock readings passes; a real drift doesn't.
pub fn far_for(same_room: bool) -> Duration {
    Duration::from_secs(if same_room { 1 } else { 3 })
}

/// The correction for an error (smoothed). `settling` is true just after a start or a seek,
/// while the device fills (no correction then).
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

/// Takes out or adds single frames in interleaved audio, spread evenly: a running fraction
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

/// Interleaved audio (`ch` channels) stretched or squeezed to `to` frames by
/// linear interpolation, its first and last frames kept exactly: a tiny local resample, so a
/// frame more or fewer is spread over the whole stretch instead of showing as a step.
fn resample_into(src: &[i32], ch: usize, to: usize, out: &mut Vec<i32>) {
    let n = src.len() / ch;
    if n < 2 || to < 2 {
        out.extend_from_slice(src);
        return;
    }
    let scale = (n - 1) as f64 / (to - 1) as f64;
    for j in 0..to {
        let pos = j as f64 * scale;
        let i = (pos.floor() as usize).min(n - 2);
        let t = pos - i as f64;
        for c in 0..ch {
            let x = f64::from(src[i * ch + c]);
            let y = f64::from(src[(i + 1) * ch + c]);
            out.push((x + (y - x) * t).round() as i32);
        }
    }
}

/// Change `samples` (interleaved, `ch` channels) by `n` frames, longer (`longer`) or shorter,
/// in `n` even stretches each a frame longer or shorter than it was.
fn respace(samples: &mut Vec<i32>, ch: usize, n: usize, longer: bool) {
    let ch = ch.max(1);
    let frames = samples.len() / ch;
    // Each stretch at least a few dozen frames, so the change is a fraction of a sample a frame.
    if n == 0 || frames < 32 * n {
        return;
    }
    let len = frames / n;
    let mut out = Vec::with_capacity(samples.len() + n * ch);
    for k in 0..n {
        let from = k * len;
        let to = if k + 1 == n { frames } else { from + len };
        let piece = &samples[from * ch..to * ch];
        let count = to - from;
        resample_into(
            piece,
            ch,
            if longer { count + 1 } else { count - 1 },
            &mut out,
        );
    }
    *samples = out;
}

/// Add `n` frames to `samples` (interleaved, `ch` channels), spread through it: the audio gets
/// `n` frames longer (we were ahead). Stretched, not repeated: a repeated sample is heard as grit
/// on clean music.
pub fn repeat_frames(samples: &mut Vec<i32>, ch: usize, n: usize) {
    respace(samples, ch, n, true);
}

/// Take `n` frames out of `samples` (interleaved, `ch` channels), spread through it: the audio
/// gets `n` frames shorter (we were behind). Squeezed, not cut.
pub fn drop_frames(samples: &mut Vec<i32>, ch: usize, n: usize) {
    respace(samples, ch, n, false);
}

/// How loud each frame of a fade is: up from silence (`rising`) or down to it, `done` frames of
/// `len` already past.
pub fn fade_gain(done: usize, len: usize, rising: bool) -> f32 {
    let t = (done as f32 / len.max(1) as f32).clamp(0.0, 1.0);
    // An equal-power curve: no dip or bump in loudness on the way.
    let g = (t * std::f32::consts::FRAC_PI_2).sin();
    if rising { g } else { 1.0 - g }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_known_a_little_better_is_a_nudge_not_a_new_plan() {
        let t0 = Instant::now();
        let p = SyncPlan {
            anchor: Some(t0),
            position_ms: 10_000,
            playing: true,
            same_room: true,
            extra_latency_ms: 0,
        };
        let later = |ms| SyncPlan {
            anchor: Some(t0 + Duration::from_millis(ms)),
            ..p
        };
        assert!(is_nudge(&p, &later(3), t0), "a clock estimate moving 3 ms");
        assert!(!is_nudge(&p, &later(40), t0), "in one room, 40 ms is heard");
        assert!(!is_nudge(&p, &later(500), t0), "a seek in the room");
        let apart = SyncPlan {
            same_room: false,
            ..p
        };
        let apart_later = |ms| SyncPlan {
            anchor: Some(t0 + Duration::from_millis(ms)),
            ..apart
        };
        assert!(
            is_nudge(&apart, &apart_later(150), t0),
            "apart, 150 ms isn't heard"
        );
        assert!(!is_nudge(&apart, &apart_later(500), t0));
        assert!(
            !is_nudge(
                &p,
                &SyncPlan {
                    playing: false,
                    ..p
                },
                t0
            ),
            "a pause"
        );
        assert!(
            !is_nudge(
                &p,
                &SyncPlan {
                    same_room: false,
                    ..p
                },
                t0
            ),
            "the way changed"
        );
        assert!(
            !is_nudge(&p, &SyncPlan { anchor: None, ..p }, t0),
            "a new song getting ready"
        );
    }

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
        assert_eq!(correction(-250.0, false, false), Correction::None);
        assert_eq!(correction(301.0, false, false), Correction::Seek);
        assert_eq!(
            correction(900.0, false, true),
            Correction::None,
            "not while settling"
        );
        assert!(far_for(false) > far_for(true), "apart waits longer");
    }

    #[test]
    fn same_room_trims_gently_and_seeks_when_far() {
        assert_eq!(correction(0.3, true, false), Correction::None);
        assert_eq!(correction(1.9, true, false), Correction::None, "noise");
        assert_eq!(
            correction(3.0, true, false),
            Correction::Trim {
                ppm: 300.0,
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
    fn frames_go_in_and_out_without_a_step_in_the_wave() {
        // A 2 kHz sine at 48 kHz: what it changes from one sample to the next is its own.
        let amp = 1_000_000.0;
        let sine: Vec<i32> = (0..480)
            .map(|i| {
                (amp * (2.0 * std::f64::consts::PI * 2000.0 * i as f64 / 48_000.0).sin()) as i32
            })
            .collect();
        let own = sine.windows(2).map(|w| (w[1] - w[0]).abs()).max().unwrap();
        let changes: [fn(&mut Vec<i32>, usize, usize); 2] = [repeat_frames, drop_frames];
        for change in changes {
            let mut s = sine.clone();
            change(&mut s, 1, 6);
            let worst = s.windows(2).map(|w| (w[1] - w[0]).abs()).max().unwrap();
            // A frame spread over 80: a step at most a couple of percent past the wave's own (a
            // repeated or dropped sample doubles it, or worse).
            assert!(
                f64::from(worst) <= f64::from(own) * 1.03,
                "steps no bigger than the wave's own: {worst} > {own}"
            );
        }
        // Too short to spread a frame over: left as it is.
        let mut short = sine[..40].to_vec();
        repeat_frames(&mut short, 1, 2);
        assert_eq!(short, sine[..40]);
    }

    #[test]
    fn frames_are_added_or_taken_out_whole() {
        // Stereo: frame i is [i, -i].
        let base: Vec<i32> = (0..200).flat_map(|i| [i * 2, -i * 2]).collect();
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

    #[test]
    fn a_fade_goes_all_the_way_smoothly() {
        assert_eq!(fade_gain(0, 100, true), 0.0);
        assert!((fade_gain(100, 100, true) - 1.0).abs() < 1e-6);
        assert!((fade_gain(0, 100, false) - 1.0).abs() < 1e-6);
        assert!(fade_gain(100, 100, false).abs() < 1e-6);
        let rising: Vec<f32> = (0..=100).map(|i| fade_gain(i, 100, true)).collect();
        assert!(rising.windows(2).all(|w| w[1] >= w[0]), "only up");
    }
}
