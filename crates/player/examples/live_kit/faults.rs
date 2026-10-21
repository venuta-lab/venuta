//! The test music, the faults section 7.1 says every detector must catch, and the shaped
//! edits (section 4.7's splice table) it must let through.

use super::checks::{self, Flag, Piece};

/// The test music: `secs` of the piece, interleaved stereo float at `gain` (1 is the piece's
/// own level, about -5 dBFS at its peaks), rising over its first and falling over its last
/// 15 ms so it starts and ends as a shaped event.
pub fn music(rate: u32, secs: f64, gain: f64) -> Vec<f32> {
    let piece = Piece { rate, channels: 2 };
    let n = (secs * f64::from(rate)) as usize;
    let edge = (0.015 * f64::from(rate)) as usize;
    let mut v = Vec::with_capacity(n * 2);
    for i in 0..n {
        let g = gain * fade(i.min(n - 1 - i), edge);
        for c in 0..2 {
            v.push((piece.at(i as f64, c) / checks::FULL * g) as f32);
        }
    }
    v
}

/// Denser, never-repeating music for the alignment checks (the test piece is nearly periodic):
/// 48 partials between 60 Hz and 6 kHz, each swelling and fading on its own slow cycle, with
/// low-passed noise under them. Interleaved stereo float, peaks under about -6 dBFS.
pub fn dense(rate: u32, secs: f64, seed: u64) -> Vec<f32> {
    let mut s = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut rnd = move || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((s >> 11) as f64) / ((1u64 << 53) as f64)
    };
    let tau = 2.0 * std::f64::consts::PI;
    let partials: Vec<(f64, f64, f64, f64)> = (0..48)
        .map(|_| {
            let f = 60.0 * (100.0f64).powf(rnd());
            (f, rnd() * tau, 0.05 + 0.4 * rnd(), rnd() * tau)
        })
        .collect();
    let n = (secs * f64::from(rate)) as usize;
    let mut v = Vec::with_capacity(n * 2);
    let mut lp = [0.0f64; 2];
    for i in 0..n {
        let t = i as f64 / f64::from(rate);
        let mut x = 0.0;
        for &(f, ph, am, aph) in &partials {
            let env = 0.5 + 0.5 * (tau * am * t + aph).sin();
            x += env * (tau * f * t + ph).sin() / (f / 60.0).sqrt();
        }
        for (c, l) in lp.iter_mut().enumerate() {
            *l += 0.2 * ((rnd() - 0.5) - *l);
            v.push(((x * 0.05 + *l * 0.05) * (1.0 + 0.1 * c as f64)) as f32);
        }
    }
    v
}

/// Raised cosine from 0 at `i = 0` to 1 at `i = len`.
fn fade(i: usize, len: usize) -> f64 {
    if i >= len {
        1.0
    } else {
        0.5 - 0.5 * (std::f64::consts::PI * i as f64 / len as f64).cos()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    DroppedFrame,
    RepeatedFrame,
    /// A 1-sample step of -40 dB (of full scale at gain 1) at a peak.
    Step,
    Gap5ms,
    HardCut,
    /// The level stepping down by 1 dB from one sample to the next.
    GainStep,
}

impl Fault {
    pub const ALL: [Fault; 6] = [
        Fault::DroppedFrame,
        Fault::RepeatedFrame,
        Fault::Step,
        Fault::Gap5ms,
        Fault::HardCut,
        Fault::GainStep,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Fault::DroppedFrame => "one dropped frame",
            Fault::RepeatedFrame => "one repeated frame",
            Fault::Step => "a 1-sample step of -40 dB at a peak",
            Fault::Gap5ms => "a 5 ms gap",
            Fault::HardCut => "a hard cut",
            Fault::GainStep => "a gain step",
        }
    }

    pub fn parse(s: &str) -> Option<Fault> {
        Some(match s {
            "drop" => Fault::DroppedFrame,
            "repeat" => Fault::RepeatedFrame,
            "step" => Fault::Step,
            "gap" => Fault::Gap5ms,
            "cut" => Fault::HardCut,
            "gain" => Fault::GainStep,
            _ => return None,
        })
    }
}

/// `x` (interleaved stereo) with `fault` at about frame `at`; returns where it really is.
pub fn inject(x: &mut Vec<f32>, rate: u32, fault: Fault, at: usize, gain: f64) -> usize {
    let ch = 2;
    match fault {
        Fault::DroppedFrame => {
            x.drain(at * ch..at * ch + ch);
            at
        }
        Fault::RepeatedFrame => {
            let f: Vec<f32> = x[at * ch..at * ch + ch].to_vec();
            for (k, v) in f.into_iter().enumerate() {
                x.insert(at * ch + ch + k, v);
            }
            at
        }
        Fault::Step => {
            // The next peak of the left channel: where the music is flattest, so the step is
            // hardest to tell from it.
            let mut p = at;
            while p + 2 < x.len() / ch
                && !(x[p * ch] > x[(p - 1) * ch] && x[p * ch] >= x[(p + 1) * ch])
            {
                p += 1;
            }
            let s = (0.01 * gain) as f32;
            for v in &mut x[p * ch..] {
                *v += s;
            }
            p
        }
        Fault::Gap5ms => {
            let n = (0.005 * f64::from(rate)) as usize;
            for v in &mut x[at * ch..(at + n) * ch] {
                *v = 0.0;
            }
            at
        }
        Fault::HardCut => {
            // Out of the music at one place, into it at another 0.165 s on.
            let skip = (0.165 * f64::from(rate)) as usize + 7;
            x.drain(at * ch..(at + skip) * ch);
            at
        }
        Fault::GainStep => {
            let g = 10f32.powf(-1.0 / 20.0);
            for v in &mut x[at * ch..] {
                *v *= g;
            }
            at
        }
    }
}

/// The edits a renderer is allowed to make (section 4.7), each on the test music, by name.
pub fn shaped(rate: u32, gain: f64) -> Vec<(&'static str, Vec<f32>)> {
    let piece = Piece { rate, channels: 2 };
    let r = f64::from(rate);
    let ms = |m: f64| (m / 1000.0 * r) as usize;
    let s = |i: f64, c: u16| piece.at(i, c) / checks::FULL * gain;
    let total = ms(3000.0);
    let edge = ms(15.0);
    let mut out = Vec::new();
    let build = |f: &dyn Fn(usize, u16) -> f64| -> Vec<f32> {
        let mut v = Vec::with_capacity(total * 2);
        for i in 0..total {
            let g = fade(i.min(total - 1 - i), edge);
            for c in 0..2 {
                v.push((f(i, c) * g) as f32);
            }
        }
        v
    };
    // Pause and resume: 30 ms out, 400 ms of silence, 15 ms in, at the same place.
    let (p0, p1) = (ms(1000.0), ms(1430.0));
    out.push((
        "pause and resume",
        build(&|i, c| {
            if i < p0 {
                s(i as f64, c) * (1.0 - fade(i.saturating_sub(p0 - ms(30.0)), ms(30.0)))
            } else if i < p1 {
                0.0
            } else {
                s((i - (p1 - p0)) as f64, c) * fade(i - p1, ms(15.0))
            }
        }),
    ));
    // A seek: a 40 ms equal-power crossfade to a place 7.3 s on.
    let (x0, xl) = (ms(1200.0), ms(40.0));
    let jump = 7.3 * r;
    out.push((
        "seek (40 ms equal-power crossfade)",
        build(&|i, c| {
            if i < x0 {
                s(i as f64, c)
            } else if i >= x0 + xl {
                s(i as f64 + jump, c)
            } else {
                let t = (i - x0) as f64 / xl as f64 * std::f64::consts::FRAC_PI_2;
                s(i as f64, c) * t.cos() + s(i as f64 + jump, c) * t.sin()
            }
        }),
    ));
    // Kept in step: the rate gliding to +500 ppm at 1000 ppm/s and back.
    out.push((
        "rate change of 500 ppm",
        build(&|i, c| {
            let t = i as f64 / r;
            // Integral of the rate offset (ppm, gliding up from 0.5 s, down from 2 s).
            let ppm = |t: f64| -> f64 {
                let up = ((t - 0.5).clamp(0.0, 0.5)) * 1000.0;
                let down = ((t - 2.0).clamp(0.0, 0.5)) * 1000.0;
                up - down
            };
            let mut place = 0.0;
            let steps = 64;
            for k in 0..steps {
                place += ppm(t * (k as f64 + 0.5) / steps as f64) * 1e-6 * t / steps as f64;
            }
            s((t + place) * r, c)
        }),
    ));
    // A late join: 300 ms in from silence, partway into the song.
    let j0 = ms(500.0);
    out.push((
        "join (300 ms fade-in)",
        build(&|i, c| {
            if i < j0 {
                0.0
            } else {
                s((i + ms(20_000.0)) as f64, c) * fade(i - j0, ms(300.0))
            }
        }),
    ));
    // The volume moved: a 20 ms ramp to half.
    let v0 = ms(1500.0);
    out.push((
        "volume ramp (20 ms)",
        build(&|i, c| {
            let g = if i < v0 {
                1.0
            } else {
                1.0 - 0.5 * fade(i - v0, ms(20.0))
            };
            s(i as f64, c) * g
        }),
    ));
    // Leaving the room: a 1 s fade-out to silence.
    let l0 = ms(1500.0);
    out.push((
        "leave (1 s fade-out)",
        build(&|i, c| {
            if i >= l0 + ms(1000.0) {
                0.0
            } else {
                s(i as f64, c) * (1.0 - fade(i.saturating_sub(l0), ms(1000.0)))
            }
        }),
    ));
    out
}

/// Detectors 3 and 4 on interleaved stereo: every flag, left and right merged by place.
pub fn detect(x: &[f32], rate: u32) -> Vec<Flag> {
    let lpc = checks::LpcParams::default();
    let mut flags = Vec::new();
    for c in 0..2u16 {
        let ch: Vec<f32> = x.iter().skip(usize::from(c)).step_by(2).copied().collect();
        flags.extend(checks::lpc_residual(&ch, &lpc));
    }
    let (dips, _) = checks::dip_gap(x, 2, rate, &checks::DipParams::default());
    flags.extend(dips);
    merge(flags, (0.005 * f64::from(rate)) as usize)
}

/// Flags of the same detector within `near` frames of each other are one.
pub fn merge(mut flags: Vec<Flag>, near: usize) -> Vec<Flag> {
    flags.sort_by_key(|f| (f.detector, f.at));
    let mut out: Vec<Flag> = Vec::new();
    for f in flags {
        if let Some(l) = out.last_mut()
            && l.detector == f.detector
            && f.at <= l.at + l.len + near
        {
            l.len = l.len.max(f.at + f.len - l.at);
            l.db = l.db.max(f.db);
            continue;
        }
        out.push(f);
    }
    out.sort_by_key(|f| f.at);
    out
}

/// The music-shaped detector on interleaved stereo test music played at `gain` (the samples
/// are scaled back to the piece's own level first, so its limits hold).
pub fn detect_music_shaped(x: &[f32], rate: u32, gain: f64) -> Vec<Flag> {
    let (s1, s3) = Piece { rate, channels: 2 }.limits();
    let mut flags = Vec::new();
    for c in 0..2usize {
        let ch: Vec<i32> = x
            .iter()
            .skip(c)
            .step_by(2)
            .map(|&v| (f64::from(v) / gain.max(1e-9) * checks::FULL).round() as i32)
            .collect();
        flags.extend(checks::music_shaped(&ch, s1, s3));
    }
    merge(flags, 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn by(flags: &[Flag], detector: &str) -> Vec<Flag> {
        flags
            .iter()
            .filter(|f| f.detector == detector)
            .cloned()
            .collect()
    }

    #[test]
    fn detector_3_catches_every_fault() {
        for gain in [1.0, 0.03] {
            for fault in Fault::ALL {
                let mut x = music(RATE, 2.0, gain);
                let at = inject(&mut x, RATE, fault, 51_234, gain);
                let flags = by(&detect(&x, RATE), "lpc residual");
                assert!(
                    flags.iter().any(|f| f.at.abs_diff(at) < 400),
                    "{} at gain {gain}: not caught at {at} (flags {flags:?})",
                    fault.name()
                );
                assert!(
                    flags.iter().all(|f| f.at.abs_diff(at) < 1200),
                    "{} at gain {gain}: flags elsewhere {flags:?}",
                    fault.name()
                );
            }
        }
    }

    #[test]
    fn detector_4_catches_a_gap() {
        for gain in [1.0, 0.03] {
            let mut x = music(RATE, 2.0, gain);
            let at = inject(&mut x, RATE, Fault::Gap5ms, 61_000, gain);
            let dips = by(&detect(&x, RATE), "dip");
            assert_eq!(dips.len(), 1, "gain {gain}: {dips:?}");
            assert!(dips[0].at.abs_diff(at) < 480, "{dips:?}");
        }
    }

    #[test]
    fn clean_music_and_every_shaped_edit_pass() {
        for gain in [1.0, 0.03] {
            let clean = music(RATE, 3.0, gain);
            assert_eq!(detect(&clean, RATE), vec![], "clean at gain {gain}");
            assert_eq!(detect_music_shaped(&clean, RATE, gain), vec![]);
            for (name, x) in shaped(RATE, gain) {
                assert_eq!(detect(&x, RATE), vec![], "{name} at gain {gain}");
            }
        }
    }

    #[test]
    fn on_dense_music_detector_4_still_finds_a_gap_and_nothing_else_is_flagged() {
        let clean = dense(RATE, 4.0, 5);
        assert_eq!(detect(&clean, RATE), vec![]);
        let mut x = clean.clone();
        let at = inject(&mut x, RATE, Fault::Gap5ms, 100_000, 1.0);
        let dips = by(&detect(&x, RATE), "dip");
        assert_eq!(dips.len(), 1, "{dips:?}");
        assert!(dips[0].at.abs_diff(at) < 480);
    }

    #[test]
    fn the_music_shaped_detector_catches_a_dropped_frame() {
        let gain = 0.03;
        let mut x = music(RATE, 2.0, gain);
        let at = inject(&mut x, RATE, Fault::DroppedFrame, 40_321, gain);
        let f = detect_music_shaped(&x, RATE, gain);
        assert!(f.iter().any(|f| f.at.abs_diff(at) < 64), "{f:?}");
    }
}
