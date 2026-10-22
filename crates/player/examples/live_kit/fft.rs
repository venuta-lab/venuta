//! A small radix-2 FFT and the correlations the kit needs (no new dependency).

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    pub fn mul(self, o: C) -> C {
        C::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    pub fn conj(self) -> C {
        C::new(self.re, -self.im)
    }
    pub fn norm(self) -> f64 {
        (self.re * self.re + self.im * self.im).sqrt()
    }
}

/// In place; `buf.len()` must be a power of two. The inverse is scaled by 1/n.
pub fn fft(buf: &mut [C], inverse: bool) {
    let n = buf.len();
    assert!(n.is_power_of_two(), "fft length {n} is not a power of two");
    if n < 2 {
        return;
    }
    // Bit reversal.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            buf.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let tw: Vec<C> = (0..n / 2)
        .map(|k| {
            let a = sign * 2.0 * PI * k as f64 / n as f64;
            C::new(a.cos(), a.sin())
        })
        .collect();
    let mut len = 2;
    while len <= n {
        let step = n / len;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let w = tw[k * step];
                let a = buf[start + k];
                let b = buf[start + k + len / 2].mul(w);
                buf[start + k] = C::new(a.re + b.re, a.im + b.im);
                buf[start + k + len / 2] = C::new(a.re - b.re, a.im - b.im);
            }
        }
        len <<= 1;
    }
    if inverse {
        let s = 1.0 / n as f64;
        for v in buf.iter_mut() {
            v.re *= s;
            v.im *= s;
        }
    }
}

/// The spectrum of `x`, zero-padded to `n`.
pub fn spectrum(x: &[f64], n: usize) -> Vec<C> {
    let mut b = vec![C::default(); n];
    for (o, &v) in b.iter_mut().zip(x) {
        o.re = v;
    }
    fft(&mut b, false);
    b
}

/// A peak's place between samples, from it and its two neighbours (a parabola through them).
pub fn parabolic(ym: f64, y0: f64, yp: f64) -> f64 {
    let d = ym - 2.0 * y0 + yp;
    if d.abs() < 1e-300 {
        0.0
    } else {
        (0.5 * (ym - yp) / d).clamp(-0.5, 0.5)
    }
}

/// How much later `b` is than `a` (in samples, fractional), by the generalised
/// cross-correlation with phase transform (GCC-PHAT), searched within `max_lag` samples and
/// using only the band `lo_hz..hi_hz`. Also returns the confidence: the peak over the highest
/// value further than `exclude` samples from it (1 means no clear peak).
pub fn gcc_phat(
    a: &[f64],
    b: &[f64],
    rate: f64,
    max_lag: usize,
    lo_hz: f64,
    hi_hz: f64,
) -> (f64, f64) {
    let len = a.len().max(b.len());
    let n = (len + max_lag + 1).next_power_of_two() * 2;
    let fa = spectrum(a, n);
    let fb = spectrum(b, n);
    let lo = (lo_hz / rate * n as f64).floor() as usize;
    let hi = ((hi_hz / rate * n as f64).ceil() as usize).min(n / 2);
    let mut r = vec![C::default(); n];
    let cross: Vec<C> = (0..n).map(|k| fa[k].conj().mul(fb[k])).collect();
    // Bins far under the band's typical level are mostly leakage and noise: the phase
    // transform is softened by a tenth of the mean magnitude so they weigh little.
    let band = lo.max(1)..hi;
    let mean = cross[band.clone()].iter().map(|p| p.norm()).sum::<f64>() / band.len().max(1) as f64;
    for k in band {
        // b later than a by d: B = A e^{-iwd}; conj(A) B peaks at +d.
        let p = cross[k];
        let m = p.norm() + 0.1 * mean;
        if m > 1e-300 {
            let v = C::new(p.re / m, p.im / m);
            r[k] = v;
            r[n - k] = v.conj();
        }
    }
    fft(&mut r, true);
    let at = |lag: i64| -> f64 {
        let i = if lag >= 0 {
            lag as usize
        } else {
            n - (-lag) as usize
        };
        r[i].re
    };
    let ml = max_lag as i64;
    let (mut best, mut best_v) = (0i64, f64::MIN);
    for lag in -ml..=ml {
        let v = at(lag);
        if v > best_v {
            best_v = v;
            best = lag;
        }
    }
    let frac = if best > -ml && best < ml {
        parabolic(at(best - 1), best_v, at(best + 1))
    } else {
        0.0
    };
    // The next best peak, away from this one (a millisecond and more).
    let exclude = (rate / 1000.0).max(2.0) as i64;
    let mut second = f64::MIN;
    for lag in -ml..=ml {
        if (lag - best).abs() > exclude {
            second = second.max(at(lag));
        }
    }
    let conf = if second > 0.0 {
        best_v / second
    } else {
        f64::INFINITY
    };
    (best as f64 + frac, conf)
}

/// Normalised cross-correlation of `needle` against every place of `hay`: value `k` is the
/// correlation of `needle` with `hay[k..k + needle.len()]` (1 for the same shape).
pub fn find_in(needle: &[f64], hay: &[f64]) -> Vec<f64> {
    let m = needle.len();
    if m == 0 || hay.len() < m {
        return Vec::new();
    }
    let n = (hay.len() + m).next_power_of_two();
    let fh = spectrum(hay, n);
    let mut rev: Vec<f64> = needle.to_vec();
    rev.reverse();
    let fnd = spectrum(&rev, n);
    let mut prod: Vec<C> = fh.iter().zip(&fnd).map(|(a, b)| a.mul(*b)).collect();
    fft(&mut prod, true);
    let ne = needle.iter().map(|v| v * v).sum::<f64>().sqrt();
    let mut pre = vec![0.0f64; hay.len() + 1];
    for (i, v) in hay.iter().enumerate() {
        pre[i + 1] = pre[i] + v * v;
    }
    (0..=hay.len() - m)
        .map(|k| {
            let he = (pre[k + m] - pre[k]).max(0.0).sqrt();
            if ne * he < 1e-300 {
                0.0
            } else {
                prod[k + m - 1].re / (ne * he)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_round_trips() {
        let x: Vec<f64> = (0..64)
            .map(|i| (i as f64 * 0.37).sin() + 0.1 * i as f64)
            .collect();
        let mut b = spectrum(&x, 64);
        fft(&mut b, true);
        for (a, b) in x.iter().zip(&b) {
            assert!((a - b.re).abs() < 1e-9);
        }
    }

    #[test]
    fn gcc_phat_finds_a_fractional_delay() {
        let rate = 48_000.0;
        // Broadband, as music is: 200 partials at unrelated frequencies.
        let parts: Vec<(f64, f64)> = (0..200)
            .map(|k| {
                let u = ((k * 7919 + 13) % 1000) as f64 / 1000.0;
                (80.0 * 75f64.powf(u), k as f64 * 2.39)
            })
            .collect();
        let f = |t: f64| {
            parts
                .iter()
                .map(|&(fr, ph)| (2.0 * PI * fr * t + ph).sin() / (fr / 80.0).sqrt())
                .sum::<f64>()
        };
        let d = 37.25; // samples
        let a: Vec<f64> = (0..48_000).map(|i| f(i as f64 / rate)).collect();
        let b: Vec<f64> = (0..48_000).map(|i| f((i as f64 - d) / rate)).collect();
        let (lag, _) = gcc_phat(&a, &b, rate, 2000, 100.0, 8000.0);
        assert!((lag - d).abs() < 0.2, "lag {lag}");
    }

    #[test]
    fn find_in_finds_the_place() {
        let hay: Vec<f64> = (0..5000).map(|i| ((i * i) as f64 * 0.001).sin()).collect();
        let needle = hay[1234..1534].to_vec();
        let c = find_in(&needle, &hay);
        let best = c
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        assert_eq!(best.0, 1234);
        assert!((best.1 - 1.0).abs() < 1e-6);
    }
}
