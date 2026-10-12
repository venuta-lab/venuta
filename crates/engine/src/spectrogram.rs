//! Spectrogram images: the evidence behind "high frequencies cut off" and "hi-res in name
//! only". Time runs left to right, frequency bottom (0 Hz) to top (Nyquist), loudness as colour.

use crate::quality::FlacStream;
use image::{Rgb, RgbImage};
use realfft::RealFftPlanner;
use std::path::{Path, PathBuf};

const FFT_LEN: usize = 4096;
pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 360;
/// Dynamic range shown, in dB below the loudest bin.
const RANGE_DB: f32 = 120.0;

pub fn cache_dir() -> PathBuf {
    crate::paths::app_data_dir().join("spectrograms")
}

/// Render (or reuse) a spectrogram PNG for a FLAC file; `key` identifies the audio (e.g. its MD5).
pub fn render_cached(path: &Path, key: &str) -> Result<PathBuf, String> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(format!("{key}.png"));
    if out.exists() {
        return Ok(out);
    }
    let img = render(path)?;
    let tmp = out.with_extension("png.part");
    img.save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn render(path: &Path) -> Result<RgbImage, String> {
    let mut stream = FlacStream::open(path, false)?;
    let channels = stream.channels;
    let total = stream.frames.ok_or("unknown length")? as usize;
    if total < FFT_LEN {
        return Err("track too short".into());
    }
    // One FFT per image column, at evenly spaced positions through the track.
    let cols = WIDTH as usize;
    let starts: Vec<usize> = (0..cols)
        .map(|c| c * (total - FFT_LEN) / (cols - 1))
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_LEN);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let window: Vec<f32> = (0..FFT_LEN)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_LEN as f32).cos())
        .collect();
    let bins = FFT_LEN / 2;
    let mut columns: Vec<Vec<f32>> = Vec::with_capacity(cols);

    // Stream the decoded audio, keeping only the mono samples we still need.
    let mut mono: Vec<f32> = Vec::new();
    let mut mono_start = 0usize; // frame index of mono[0]
    let mut next_col = 0usize;
    let mut samples = Vec::new();
    while next_col < cols && stream.next(&mut samples)? {
        for frame in samples.chunks_exact(channels) {
            mono.push(
                frame
                    .iter()
                    .map(|&s| s as f32 / 2_147_483_648.0)
                    .sum::<f32>()
                    / channels as f32,
            );
        }
        while next_col < cols && mono_start + mono.len() >= starts[next_col] + FFT_LEN {
            let at = starts[next_col] - mono_start;
            for ((dst, s), w) in input.iter_mut().zip(&mono[at..at + FFT_LEN]).zip(&window) {
                *dst = s * w;
            }
            fft.process(&mut input, &mut spectrum)
                .map_err(|e| e.to_string())?;
            columns.push(
                spectrum[..bins]
                    .iter()
                    .map(|c| 10.0 * (c.norm_sqr() + 1e-20).log10())
                    .collect(),
            );
            next_col += 1;
        }
        // Drop samples no future column needs.
        if next_col < cols {
            let keep_from = starts[next_col].saturating_sub(mono_start).min(mono.len());
            mono.drain(..keep_from);
            mono_start += keep_from;
        }
    }
    if columns.is_empty() {
        return Err("no audio decoded".into());
    }

    let max = columns
        .iter()
        .flat_map(|c| c.iter())
        .fold(f32::MIN, |m, v| m.max(*v));
    let mut img = RgbImage::new(columns.len() as u32, HEIGHT);
    for (x, col) in columns.iter().enumerate() {
        for y in 0..HEIGHT {
            // Row 0 is the top (Nyquist); average the bins that fall into this pixel row.
            let hi = bins - (y as usize * bins / HEIGHT as usize);
            let lo = bins - ((y as usize + 1) * bins / HEIGHT as usize);
            let v = col[lo..hi.max(lo + 1)]
                .iter()
                .copied()
                .fold(f32::MIN, f32::max);
            let t = ((v - (max - RANGE_DB)) / RANGE_DB).clamp(0.0, 1.0);
            img.put_pixel(x as u32, y, colour(t));
        }
    }
    Ok(img)
}

/// Dark violet → magenta → amber → pale yellow, perceptually ordered (inferno-like).
fn colour(t: f32) -> Rgb<u8> {
    const STOPS: [(f32, [f32; 3]); 6] = [
        (0.0, [0.0, 0.0, 4.0]),
        (0.25, [66.0, 10.0, 104.0]),
        (0.5, [147.0, 38.0, 103.0]),
        (0.7, [221.0, 81.0, 58.0]),
        (0.85, [252.0, 165.0, 10.0]),
        (1.0, [252.0, 255.0, 164.0]),
    ];
    let i = STOPS
        .iter()
        .position(|(s, _)| *s >= t)
        .unwrap_or(STOPS.len() - 1)
        .max(1);
    let (t0, c0) = STOPS[i - 1];
    let (t1, c1) = STOPS[i];
    let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    Rgb([0, 1, 2].map(|k| (c0[k] + (c1[k] - c0[k]) * f) as u8))
}
