//! A song's shape for the seek bar: how loud each of `buckets` equal slices is, from 0 (quiet)
//! to 255 (full scale).
//!
//! Loudness, not peaks: modern masters touch full scale almost everywhere, so a peak shape is a
//! flat slab. The RMS level of each slice, on a decibel scale from -42 dBFS to 0, shows verses,
//! choruses and quiet intros as they sound.

use crate::decode::{DecodeState, Source};
use std::path::Path;

/// The bottom of the scale: anything this quiet or quieter draws at the minimum.
const FLOOR_DB: f64 = -42.0;

pub fn waveform(path: &Path, buckets: usize) -> Result<Vec<u8>, String> {
    let buckets = buckets.clamp(16, 4096);
    let mut source = Source::open(path)?;
    let channels = usize::from(source.format.channels.max(1));
    let total = source.frames.unwrap_or(0);
    // Mean square per slice (samples are left-justified i32, so full scale is i32::MAX).
    let mut slices: Vec<f64> = Vec::new();
    let mut buf = Vec::new();
    // Without a known length, measure each second and fold at the end.
    let per_slice = if total > 0 {
        total.div_ceil(buckets as u64).max(1)
    } else {
        u64::from(source.format.rate.max(1))
    };
    let full = f64::from(i32::MAX);
    let mut sum = 0.0f64;
    let mut count: u64 = 0;
    while source.next(&mut buf)? == DecodeState::Samples {
        for frame in buf.chunks_exact(channels) {
            for &s in frame {
                let x = f64::from(s) / full;
                sum += x * x;
            }
            count += 1;
            if count == per_slice {
                slices.push(sum / (count as f64 * channels as f64));
                sum = 0.0;
                count = 0;
            }
        }
    }
    if count > 0 {
        slices.push(sum / (count as f64 * channels as f64));
    }
    // Fold (or stretch) to exactly `buckets`, averaging energy.
    let n = slices.len().max(1);
    Ok((0..buckets)
        .map(|b| {
            let lo = b * n / buckets;
            let hi = ((b + 1) * n / buckets).max(lo + 1).min(n);
            let part = slices.get(lo..hi).unwrap_or(&[]);
            let ms = if part.is_empty() {
                0.0
            } else {
                part.iter().sum::<f64>() / part.len() as f64
            };
            let db = if ms > 0.0 {
                10.0 * ms.log10()
            } else {
                FLOOR_DB
            };
            (((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect())
}
