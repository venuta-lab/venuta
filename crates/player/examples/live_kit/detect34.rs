//! Detectors 3 and 4 of docs/design/listen-together.md section 7.1, for real music.
//!
//! Interim: work package 0 is building `crates/audio-check` with all four detectors. Until it
//! merges, the kit carries these, written to the same section. At merge, `checks.rs` re-exports
//! audio-check's instead and this file goes.
//!
//! - **Detector 3, LPC residual** (after Essentia's ClickDetector, Vaseghi and Rayner): each
//!   512-sample frame is whitened by its own linear predictor (order 16, in the design's 12 to
//!   32); a sample whose prediction error has more than 30 dB over the frame's robust
//!   excitation power (the mean error power, each sample's clipped at 10 times the median) is
//!   a click. A dropped or repeated frame, a cut and a step are all sudden; music, even loud
//!   transients, is mostly predictable from the 16 samples before it.
//! - **Detector 4, dip and gap**: 5 ms RMS windows (at every sample) more than 20 dB under the
//!   level of the 200 ms around them. A quiet stretch of 250 ms or more is a pause, a stop or
//!   a song boundary with silence, not a dip; a short one inside music is flagged.

/// What a detector found.
#[derive(Debug, Clone, PartialEq)]
pub struct Flag {
    /// The frame it starts at.
    pub at: usize,
    /// Its length in frames (1 for a click).
    pub len: usize,
    /// "lpc residual" or "dip".
    pub detector: &'static str,
    /// How far over the threshold's reference: dB over the excitation floor (3), dB under the
    /// surrounding level (4).
    pub db: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct LpcParams {
    pub order: usize,
    pub frame: usize,
    pub hop: usize,
    pub threshold_db: f64,
}

impl Default for LpcParams {
    fn default() -> Self {
        Self {
            order: 16,
            frame: 512,
            hop: 256,
            threshold_db: 30.0,
        }
    }
}

/// Below this a sample is silence (about -100 dBFS): no floor is learned from it.
const ACTIVE: f64 = 1e-5;
/// The excitation floor never goes under -120 dB (power), so digital silence and pure tones
/// do not make every rounding error a click.
const MIN_FLOOR: f64 = 1e-12;
/// Flags this close (frames) are one event.
const MERGE: usize = 480;

/// Prediction-error filter `a` (a[0] = 1) of order `p` for frame `x` (Hann windowed,
/// autocorrelation method, Levinson-Durbin).
fn lpc(x: &[f64], p: usize) -> Vec<f64> {
    let n = x.len();
    let w: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let h = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
            v * h
        })
        .collect();
    let mut r = vec![0.0f64; p + 1];
    for (k, rk) in r.iter_mut().enumerate() {
        *rk = (k..n).map(|i| w[i] * w[i - k]).sum();
    }
    // A touch of white noise keeps the solution stable on pure tones.
    r[0] *= 1.0 + 1e-9;
    let mut a = vec![0.0f64; p + 1];
    a[0] = 1.0;
    if r[0] <= 0.0 {
        return a;
    }
    let mut err = r[0];
    let mut prev = a.clone();
    for i in 1..=p {
        let acc: f64 = r[i] + (1..i).map(|j| a[j] * r[i - j]).sum::<f64>();
        let k = -acc / err;
        prev.copy_from_slice(&a);
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        err *= 1.0 - k * k;
        if err <= 0.0 {
            break;
        }
    }
    a
}

/// Detector 3's measure on one channel: for every sample, its prediction error's power over
/// its frame's excitation floor, in dB (`f64::NEG_INFINITY` where the frame is silent).
pub fn lpc_ratio_db(x: &[f32], p: &LpcParams) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NEG_INFINITY; n];
    if n < p.frame {
        return out;
    }
    let xs: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let mut e = vec![0.0f64; p.frame];
    let mut pow: Vec<f64> = Vec::with_capacity(p.frame);
    let mut sorted: Vec<f64> = Vec::with_capacity(p.frame);
    // Each hop is predicted by the filter learned on the frame just before it, so a fault is
    // never part of the model that has to find it (learned on it, a model whitens the fault
    // too and the whole frame's error rises with it). The floor is the error's robust power
    // over the frame ending with the hop.
    let mut r0 = p.frame;
    while r0 < n {
        let r1 = (r0 + p.hop).min(n);
        let past = &xs[r0 - p.frame..r0];
        let peak = xs[r0 - p.frame..r1]
            .iter()
            .fold(0.0f64, |m, v| m.max(v.abs()));
        if peak > ACTIVE {
            let a = lpc(past, p.order);
            let f0 = (r1 - p.frame).max(p.order);
            e.resize(r1 - f0, 0.0);
            for (i, ei) in e.iter_mut().enumerate() {
                let g = f0 + i;
                let mut v = xs[g];
                for (k, ak) in a.iter().enumerate().skip(1) {
                    v += ak * xs[g - k];
                }
                *ei = v;
            }
            pow.clear();
            pow.extend(
                e.iter()
                    .zip(&xs[f0..r1])
                    .filter(|(_, x)| x.abs() > ACTIVE)
                    .map(|(v, _)| v * v),
            );
            if !pow.is_empty() {
                sorted.clear();
                sorted.extend_from_slice(&pow);
                let mid = sorted.len() / 2;
                let med = *sorted.select_nth_unstable_by(mid, f64::total_cmp).1;
                let clip = 10.0 * med;
                let floor = (pow.iter().map(|v| v.min(clip)).sum::<f64>() / pow.len() as f64)
                    .max(MIN_FLOOR);
                for g in r0..r1 {
                    let v = e[g - f0];
                    out[g] = 10.0 * (v * v / floor).max(1e-30).log10();
                }
            }
        }
        r0 = r1;
    }
    out
}

/// Detector 3 on one channel.
pub fn lpc_residual(x: &[f32], p: &LpcParams) -> Vec<Flag> {
    let mut flags: Vec<Flag> = Vec::new();
    for (at, db) in lpc_ratio_db(x, p).into_iter().enumerate() {
        if db > p.threshold_db {
            match flags.last_mut() {
                // One event, however many samples it touches.
                Some(f) if at < f.at + f.len + MERGE => {
                    f.len = at + 1 - f.at;
                    f.db = f.db.max(db);
                }
                _ => flags.push(Flag {
                    at,
                    len: 1,
                    detector: "lpc residual",
                    db,
                }),
            }
        }
    }
    flags
}

#[derive(Debug, Clone, Copy)]
pub struct DipParams {
    pub window_ms: f64,
    pub context_ms: f64,
    pub depth_db: f64,
    /// The surroundings must be louder than this (dBFS RMS) for a dip to count.
    pub music_dbfs: f64,
    /// Quieter than this (dBFS RMS) is silence, whatever the surroundings.
    pub silence_dbfs: f64,
    /// A quiet stretch this long or longer is a pause or boundary.
    pub pause_ms: f64,
}

impl Default for DipParams {
    fn default() -> Self {
        Self {
            window_ms: 5.0,
            context_ms: 200.0,
            depth_db: 20.0,
            music_dbfs: -50.0,
            silence_dbfs: -70.0,
            pause_ms: 250.0,
        }
    }
}

/// A quiet stretch long enough to be a pause or a boundary: (start frame, frames).
pub type Quiet = (usize, usize);

/// Detector 4 on interleaved frames: the dips, and the long quiet stretches it set aside.
pub fn dips(data: &[f32], channels: u16, rate: u32, p: &DipParams) -> (Vec<Flag>, Vec<Quiet>) {
    let ch = usize::from(channels.max(1));
    let frames = data.len() / ch;
    let w = ((p.window_ms / 1000.0) * f64::from(rate)).round().max(1.0) as usize;
    // The context is measured on a 1 ms grid of windows.
    let grid = (f64::from(rate) / 1000.0).round().max(1.0) as usize;
    if frames < w * 4 {
        return (Vec::new(), Vec::new());
    }
    let power = |i: usize| -> f64 {
        data[i * ch..i * ch + ch]
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>()
            / ch as f64
    };
    let starts = frames - w + 1; // window starts 0..starts
    // Every window's RMS, at every sample, by a running sum (recomputed now and then so it
    // never drifts).
    let mut rms = Vec::with_capacity(starts);
    let mut sum: f64 = (0..w).map(power).sum();
    for j in 0..starts {
        if j > 0 {
            if j % 65_536 == 0 {
                sum = (j..j + w).map(power).sum();
            } else {
                sum += power(j + w - 1) - power(j - 1);
            }
        }
        rms.push((sum.max(0.0) / w as f64).sqrt() as f32);
    }
    // The level of the surroundings: a median over the context, every 10 ms.
    let half = ((p.context_ms / 2.0) * f64::from(rate) / 1000.0) as usize;
    let step = 10 * grid;
    let mut meds = Vec::with_capacity(starts / step + 1);
    let mut buf: Vec<f32> = Vec::with_capacity(2 * half / grid + 2);
    for c in (0..starts).step_by(step) {
        buf.clear();
        buf.extend(
            (c.saturating_sub(half)..(c + half + 1).min(starts))
                .step_by(grid)
                .map(|j| rms[j]),
        );
        let mid = buf.len() / 2;
        meds.push(f64::from(
            *buf.select_nth_unstable_by(mid, f32::total_cmp).1,
        ));
    }
    let depth = 10f64.powf(-p.depth_db / 20.0);
    let silence = 10f64.powf(p.silence_dbfs / 20.0);
    let music = 10f64.powf(p.music_dbfs / 20.0);
    let quiet = |j: usize| f64::from(rms[j]) < (meds[j / step] * depth).max(silence);
    let pause = ((p.pause_ms / 1000.0) * f64::from(rate)) as usize;
    let (mut flags, mut long) = (Vec::new(), Vec::new());
    let mut j = 0;
    while j < starts {
        if !quiet(j) {
            j += 1;
            continue;
        }
        let a = j;
        while j < starts && quiet(j) {
            j += 1;
        }
        let b = j; // windows a..b are quiet
        let len = b - a - 1 + w;
        // At the very start or end of the capture: the music had not begun or had ended.
        if a == 0 || b == starts {
            continue;
        }
        if len >= pause {
            long.push((a, len));
            continue;
        }
        // The level around it, outside the stretch.
        buf.clear();
        buf.extend(
            (a.saturating_sub(half)..a.saturating_sub(w))
                .step_by(grid)
                .map(|k| rms[k]),
        );
        buf.extend(
            ((b + w).min(starts)..(b + w + half).min(starts))
                .step_by(grid)
                .map(|k| rms[k]),
        );
        if buf.is_empty() {
            continue;
        }
        let mid = buf.len() / 2;
        let around = f64::from(*buf.select_nth_unstable_by(mid, f32::total_cmp).1);
        let low = rms[a..b].iter().fold(f32::MAX, |m, &v| m.min(v));
        if around > music && f64::from(low) < around * depth {
            flags.push(Flag {
                at: a,
                len,
                detector: "dip",
                db: (20.0 * (around / f64::from(low).max(1e-9)).log10()).min(180.0),
            });
        }
    }
    (flags, long)
}
