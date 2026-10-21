//! Detector 3: linear-prediction residual on real music (after Essentia's click detector, from
//! Vaseghi and Rayner). Music is predictable from its last few samples; a click is not. For each
//! 512-sample frame, a predictor of order 32 is fitted on the 512 samples before it
//! (autocorrelation over a Hann window, Levinson-Durbin; learned on the frame itself, it would
//! whiten a fault inside it), the frame is filtered through its inverse and then through the matched
//! filter (which gathers a disturbance the prediction smeared back into one peak), and a peak of
//! either 30 dB over its frame's robust floor (the median) is a click.
//!
//! Music differs in how predictable it is, so the threshold is calibrated on the same music
//! played alone: a room's recording may flag nothing the solo one doesn't. Frames quieter than
//! -60 dBFS are skipped, as are the declared spans (and the samples whose prediction reaches
//! into one).

use crate::{Click, Declared, Flags};

const ORDER: usize = 32;
const FRAME: usize = 512;
/// The least threshold: 30 dB over the floor.
const LEAST_DB: f64 = 30.0;
/// Frames quieter than this (-60 dBFS RMS) aren't judged.
const QUIET: f64 = 0.001;
/// The residual is never thought quieter than this (24-bit quantization), so a nearly perfect
/// prediction doesn't make every rounding a click.
const FLOOR_LEAST: f64 = 1e-7;

pub struct Lpc {
    /// A residual this many times the frame's floor is a click.
    threshold: f64,
}

impl Lpc {
    /// Calibrated on `solo` (the same music played alone, one channel): 30 dB, or 6 dB over the
    /// worst peak solo has, whichever is higher.
    pub fn calibrate(solo: &[f64], _rate: u32) -> Lpc {
        let least = 10f64.powf(LEAST_DB / 20.0);
        let mut worst = 0.0f64;
        residuals(solo, |_, ratio| worst = worst.max(ratio));
        Lpc {
            threshold: least.max(worst * 2.0),
        }
    }

    /// Every click in `x` (one channel, -1..1).
    pub fn check(&self, x: &[f64], declared: &Declared) -> Vec<Click> {
        let mut flags = Flags::new(FRAME / 4);
        residuals(x, |n, ratio| {
            if ratio > self.threshold && !declared.overlaps(n.saturating_sub(ORDER)..n + ORDER + 1)
            {
                flags.add(n, "a click in the music", ratio / self.threshold);
            }
        });
        flags.done()
    }
}

/// For every sample judged: its residual over its frame's floor.
fn residuals(x: &[f64], mut each: impl FnMut(usize, f64)) {
    // The residual runs a predictor's reach past the frame, for the matched filter.
    let mut e = vec![0.0; FRAME + ORDER];
    let mut m = vec![0.0; FRAME];
    let mut sorted = vec![0.0; FRAME];
    // Each frame is predicted by what was learned on the frame before it: a predictor learned
    // on the fault itself would whiten it (the first frame is only the start-up).
    let mut f = FRAME;
    while f + FRAME <= x.len() {
        let frame = &x[f..f + FRAME];
        let rms = (frame.iter().map(|v| v * v).sum::<f64>() / FRAME as f64).sqrt();
        if rms >= QUIET {
            let a = predictor(&x[f - FRAME..f]);
            let reach = (x.len() - f).min(FRAME + ORDER);
            for (k, out) in e.iter_mut().enumerate() {
                let n = f + k;
                *out = if k < reach {
                    x[n] - (1..=ORDER).map(|j| a[j] * x[n - j]).sum::<f64>()
                } else {
                    0.0
                };
            }
            // The matched filter: a disturbance at n shows in the residual as the inverse
            // filter's own response from n on; correlating with it gathers that back into one
            // peak (it sharpens what the music's own prediction smeared).
            let norm = (1.0 + (1..=ORDER).map(|j| a[j] * a[j]).sum::<f64>()).sqrt();
            for (k, out) in m.iter_mut().enumerate() {
                let s = e[k] - (1..=ORDER).map(|j| a[j] * e[k + j]).sum::<f64>();
                *out = s / norm;
            }
            let floor_of = |v: &[f64], sorted: &mut Vec<f64>| {
                sorted.clear();
                sorted.extend(v.iter().map(|x| x.abs()));
                sorted.sort_by(f64::total_cmp);
                sorted[FRAME / 2].max(FLOOR_LEAST)
            };
            let fe = floor_of(&e[..FRAME], &mut sorted);
            let fm = floor_of(&m, &mut sorted);
            for k in 0..FRAME {
                each(f + k, (e[k].abs() / fe).max(m[k].abs() / fm));
            }
        }
        f += FRAME;
    }
}

/// The predictor's coefficients a[1..=ORDER] (x[n] ~ sum a[j] x[n - j]) for one frame.
fn predictor(frame: &[f64]) -> [f64; ORDER + 1] {
    let n = frame.len();
    let w: Vec<f64> = frame
        .iter()
        .enumerate()
        .map(|(i, v)| v * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos()))
        .collect();
    let mut r = [0.0f64; ORDER + 1];
    for (lag, rl) in r.iter_mut().enumerate() {
        *rl = (lag..n).map(|i| w[i] * w[i - lag]).sum();
    }
    // A touch of white noise keeps the recursion stable on pure tones.
    r[0] *= 1.0 + 1e-9;
    r[0] += 1e-18;
    // Levinson-Durbin.
    let mut a = [0.0f64; ORDER + 1];
    let mut err = r[0];
    for i in 1..=ORDER {
        let mut acc = r[i];
        for j in 1..i {
            acc -= a[j] * r[i - j];
        }
        let k = acc / err;
        let prev = a;
        a[i] = k;
        for j in 1..i {
            a[j] = prev[j] - k * prev[i - j];
        }
        err *= 1.0 - k * k;
        if err <= 0.0 {
            break;
        }
    }
    a
}
