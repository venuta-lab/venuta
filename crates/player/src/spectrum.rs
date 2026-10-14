//! Band levels for a spectrum display: how loud the sound being played is in a few
//! frequency bands, from 0 (below -60 dB) to 255 (full scale).
//!
//! Each band is measured with the Goertzel algorithm at its centre frequency over a
//! Hann-windowed block of the most recent output, so it costs a few thousand multiplies per
//! update and needs no FFT library. It reads what is played (after the sound tools, before
//! volume); it never touches the audio.

/// Centre frequencies, log-spaced from bass to air.
pub const BANDS: [f32; 16] = [
    40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1000.0, 1600.0, 2500.0, 4000.0, 6300.0, 8000.0,
    10000.0, 12500.0, 16000.0,
];
/// How much recent sound each update looks at.
pub const WINDOW: usize = 2048;
const FLOOR_DB: f32 = -60.0;

/// The levels of `samples` (mono, -1..1, oldest first) at `rate`.
pub fn levels(samples: &[f32], rate: u32) -> Vec<u8> {
    let n = samples.len();
    if n < 64 || rate == 0 {
        return vec![0; BANDS.len()];
    }
    // Hann window, and its gain, so a full-scale sine reads near 0 dB.
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos())
        .collect();
    let gain: f32 = window.iter().sum::<f32>() / 2.0;
    BANDS
        .iter()
        .map(|&f| {
            if f >= rate as f32 / 2.0 {
                return 0;
            }
            let k = (f * n as f32 / rate as f32).round();
            let w = 2.0 * std::f32::consts::PI * k / n as f32;
            let coeff = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for (x, h) in samples.iter().zip(&window) {
                let s = x * h + coeff * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
            let amp = power.max(0.0).sqrt() / gain;
            let db = if amp > 0.0 {
                20.0 * amp.log10()
            } else {
                FLOOR_DB
            };
            (((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_lights_its_own_band_most() {
        let rate = 48_000;
        let tone = 1000.0;
        let samples: Vec<f32> = (0..WINDOW)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * tone * i as f32 / rate as f32).sin())
            .collect();
        let l = levels(&samples, rate);
        let at = BANDS.iter().position(|&f| f == 1000.0).unwrap();
        let loudest = l.iter().enumerate().max_by_key(|(_, v)| **v).unwrap().0;
        assert_eq!(loudest, at, "{l:?}");
        // -6 dB sine: near the top of a 60 dB scale.
        assert!(l[at] > 200, "{l:?}");
        // Far bands stay low.
        assert!(l[0] < 120 && l[15] < 120, "{l:?}");
    }

    #[test]
    fn silence_is_zero() {
        assert!(levels(&vec![0.0; WINDOW], 44_100).iter().all(|&v| v == 0));
    }
}
