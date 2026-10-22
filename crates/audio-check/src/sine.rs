//! Detector 2: sine lock (after OboeTester's glitch analyzer). A known sine (1 kHz or 15 kHz at
//! -6 dBFS) goes through the whole path; the detector follows it and calls anything off its
//! line a click.
//!
//! For each block of one period, the sine's magnitude and phase are fitted over the 8 periods
//! around it (least squares on cos, sin, and both times t, so a rate change of up to the
//! resampler's 500 ppm and a slow change of level are part of the line). Outside the declared
//! spans, any sample off the fit by more than -60 dB of full scale is a click. Inside a
//! declared span (a fade, a pause), the line may bend: the envelope may change by at most
//! 1/240 of the sine's level per sample (a fade of 5 ms or longer), and no sample may be off a
//! fit over the two periods around it by more than -40 dB (a click inside the fade).

use crate::{Click, Declared, Flags};

/// Off the line by more than this (-60 dB of full scale) is a click.
const RESIDUAL: f64 = 0.001;
/// Inside a declared fade, off a short fit by more than this (-40 dB) is a click.
const RESIDUAL_IN_FADE: f64 = 0.01;
/// The fastest an envelope may move inside a declared span, per sample, as a share of the
/// sine's level.
const ENVELOPE_PER_SAMPLE: f64 = 1.0 / 240.0;
/// Periods each fit spans.
const FIT_PERIODS: f64 = 8.0;

/// A sine to follow.
pub struct SineLock {
    /// Radians per sample.
    w: f64,
    /// Samples per block (one period, rounded up).
    hop: usize,
    /// Samples per fit.
    span: usize,
    /// The sine's peak level, -1..1 (0.5 for -6 dBFS).
    amplitude: f64,
}

impl SineLock {
    pub fn new(freq: f64, rate: u32, amplitude: f64) -> SineLock {
        let period = f64::from(rate) / freq;
        SineLock {
            w: std::f64::consts::TAU / period,
            hop: period.ceil().max(1.0) as usize,
            span: (period * FIT_PERIODS).round().max(8.0) as usize,
            amplitude,
        }
    }

    /// The best (a, b, c, d) for x[n] = (a + c t) cos(w n) + (b + d t) sin(w n) over `from..to`,
    /// t = (n - mid) / half: the four normal equations, solved.
    fn fit(&self, x: &[f64], from: usize, to: usize) -> [f64; 4] {
        let mid = (from + to) as f64 / 2.0;
        let half = ((to - from) as f64 / 2.0).max(1.0);
        let mut m = [[0.0f64; 5]; 4];
        for (n, &v) in x.iter().enumerate().take(to).skip(from) {
            let t = (n as f64 - mid) / half;
            let (s, c) = (self.w * n as f64).sin_cos();
            let basis = [c, s, c * t, s * t];
            for i in 0..4 {
                for j in 0..4 {
                    m[i][j] += basis[i] * basis[j];
                }
                m[i][4] += basis[i] * v;
            }
        }
        solve(m)
    }

    /// The sine's level over the two periods around `centre` (short, so a fade's steepness
    /// shows): x[n] = a cos(w n) + b sin(w n), least squares.
    fn level_near(&self, x: &[f64], centre: usize) -> f64 {
        let half = (self.span / FIT_PERIODS as usize).max(2);
        let from = centre.saturating_sub(half);
        let to = (centre + half).min(x.len());
        let (mut cc, mut ss, mut cs, mut cx, mut sx) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (n, &v) in x.iter().enumerate().take(to).skip(from) {
            let (s, c) = (self.w * n as f64).sin_cos();
            cc += c * c;
            ss += s * s;
            cs += c * s;
            cx += c * v;
            sx += s * v;
        }
        let det = cc * ss - cs * cs;
        if det.abs() < 1e-12 {
            return 0.0;
        }
        let a = (cx * ss - sx * cs) / det;
        let b = (sx * cc - cx * cs) / det;
        (a * a + b * b).sqrt()
    }

    /// The first sample of `block` off the fit `p` (over `from..to`) by more than `limit`.
    fn first_off(
        &self,
        x: &[f64],
        block: std::ops::Range<usize>,
        p: &[f64; 4],
        from: usize,
        to: usize,
        limit: f64,
    ) -> Option<(usize, f64)> {
        x[block.clone()]
            .iter()
            .zip(block)
            .map(|(v, n)| (n, (v - self.value(p, n, from, to)).abs()))
            .find(|&(_, r)| r > limit)
    }

    fn value(&self, p: &[f64; 4], n: usize, from: usize, to: usize) -> f64 {
        let mid = (from + to) as f64 / 2.0;
        let half = ((to - from) as f64 / 2.0).max(1.0);
        let t = (n as f64 - mid) / half;
        let (s, c) = (self.w * n as f64).sin_cos();
        (p[0] + p[2] * t) * c + (p[1] + p[3] * t) * s
    }
}

/// Gaussian elimination with partial pivoting on a 4 x 5 augmented matrix.
fn solve(mut m: [[f64; 5]; 4]) -> [f64; 4] {
    for col in 0..4 {
        let piv = (col..4)
            .max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))
            .unwrap();
        m.swap(col, piv);
        if m[col][col].abs() < 1e-12 {
            continue;
        }
        for row in 0..4 {
            if row != col {
                let k = m[row][col] / m[col][col];
                let pivot = m[col];
                for (v, p) in m[row].iter_mut().zip(pivot).skip(col) {
                    *v -= k * p;
                }
            }
        }
    }
    let mut out = [0.0; 4];
    for (i, o) in out.iter_mut().enumerate() {
        if m[i][i].abs() >= 1e-12 {
            *o = m[i][4] / m[i][i];
        }
    }
    out
}

/// Every place `x` (one channel, -1..1) leaves the sine's line.
pub fn check(x: &[f64], lock: &SineLock, declared: &Declared) -> Vec<Click> {
    let mut flags = Flags::new(lock.span);
    let (hop, span) = (lock.hop, lock.span);
    if x.len() < span * 2 {
        return Vec::new();
    }
    // The first and last fit's worth are the start-up and the tail: not judged.
    let mut envelope: Option<f64> = None;
    let mut b = span;
    while b + hop + span <= x.len() {
        let centre = b + hop / 2;
        let from = centre - span / 2;
        let to = from + span;
        let p = lock.fit(x, from, to);
        let level = lock.level_near(x, centre);
        if declared.overlaps(from..to) {
            // Inside a fade: the envelope may move only slowly, and nothing may leave a short
            // fit (the fade bends its line a little, so by -40 dB, not -60).
            if let Some(before) = envelope {
                let per_sample = (level - before).abs() / hop as f64;
                let limit = lock.amplitude * ENVELOPE_PER_SAMPLE * 1.15;
                if per_sample > limit {
                    flags.add(
                        b,
                        "the level moved too fast inside a fade",
                        per_sample / limit,
                    );
                }
            }
            let short = (span / FIT_PERIODS as usize * 2).max(16);
            let (sf, st) = (
                centre.saturating_sub(short / 2),
                (centre + short / 2).min(x.len()),
            );
            let q = lock.fit(x, sf, st);
            if let Some((n, r)) = lock.first_off(x, b..b + hop, &q, sf, st, RESIDUAL_IN_FADE) {
                flags.add(n, "a click inside a fade", r / RESIDUAL_IN_FADE);
            }
        } else if let Some((n, r)) = lock.first_off(x, b..b + hop, &p, from, to, RESIDUAL) {
            flags.add(n, "the sine left its line", r / RESIDUAL);
        }
        envelope = Some(level);
        b += hop;
    }
    flags.done()
}
