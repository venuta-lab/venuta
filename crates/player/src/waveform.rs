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
    shape(Source::open(path)?, buckets)
}

/// An online song's shape, read from its stream as it downloads (the smallest stream will do:
/// loudness per slice needs no detail).
pub fn waveform_remote(
    audio: media_access::EphemeralAudio,
    buckets: usize,
) -> Result<Vec<u8>, String> {
    shape(Source::open_remote(audio)?, buckets)
}

fn shape(mut source: Source, buckets: usize) -> Result<Vec<u8>, String> {
    let buckets = buckets.clamp(16, 4096);
    // Mean square per slice (samples are left-justified i32, so full scale is i32::MAX).
    let mut slices: Vec<f64> = Vec::new();
    let mut buf = Vec::new();
    let full = f64::from(i32::MAX);
    let mut sum = 0.0f64;
    let mut count: u64 = 0;
    // Read once samples come: a stream knows its format and length only then.
    let mut shape: Option<(usize, u64)> = None;
    // A stream that stops sending for this long has failed.
    let mut waited = std::time::Duration::ZERO;
    const GIVE_UP: std::time::Duration = std::time::Duration::from_secs(60);
    loop {
        match source.next(&mut buf)? {
            DecodeState::End => break,
            DecodeState::Pending => {
                if waited >= GIVE_UP {
                    return Err("the stream stopped sending".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
                waited += std::time::Duration::from_millis(5);
                continue;
            }
            DecodeState::Samples => waited = std::time::Duration::ZERO,
        }
        let (channels, per_slice) = *shape.get_or_insert_with(|| {
            let channels = usize::from(source.format.channels.max(1));
            let total = source.frames.unwrap_or(0);
            // Without a known length, measure each tenth of a second and fold at the end.
            let per_slice = if total > 0 {
                total.div_ceil(buckets as u64).max(1)
            } else {
                u64::from(source.format.rate.max(10)) / 10
            };
            (channels, per_slice)
        });
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
    let channels = shape.map_or(1, |(c, _)| c);
    if count > 0 {
        slices.push(sum / (count as f64 * channels as f64));
    }
    if slices.is_empty() {
        return Err("no sound came".into());
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
