//! A song's shape for the seek bar: the loudest moment in each of `buckets` equal slices,
//! from 0 (silence) to 255 (full scale).

use crate::decode::Source;
use std::path::Path;

pub fn waveform(path: &Path, buckets: usize) -> Result<Vec<u8>, String> {
    let buckets = buckets.clamp(16, 4096);
    let mut source = Source::open(path)?;
    let channels = usize::from(source.format.channels.max(1));
    let total = source.frames.unwrap_or(0);
    let mut peaks: Vec<u32> = Vec::with_capacity(buckets);
    let mut frames: Vec<u32> = Vec::new();
    let mut buf = Vec::new();
    // Without a known length, collect per-second peaks and fold them at the end.
    let per_bucket = if total > 0 {
        total.div_ceil(buckets as u64).max(1)
    } else {
        u64::from(source.format.rate.max(1))
    };
    let mut cur: u32 = 0;
    let mut in_bucket: u64 = 0;
    while source.next(&mut buf)? {
        for frame in buf.chunks_exact(channels) {
            let loud = frame.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            cur = cur.max(loud);
            in_bucket += 1;
            if in_bucket == per_bucket {
                frames.push(cur);
                cur = 0;
                in_bucket = 0;
            }
        }
    }
    if in_bucket > 0 {
        frames.push(cur);
    }
    // Fold (or stretch) to exactly `buckets`.
    let n = frames.len().max(1);
    for b in 0..buckets {
        let lo = b * n / buckets;
        let hi = ((b + 1) * n / buckets).max(lo + 1).min(n);
        peaks.push(
            frames
                .get(lo..hi)
                .and_then(|s| s.iter().max().copied())
                .unwrap_or(0),
        );
    }
    // Shown on a gentle curve, so quiet passages are still visible.
    Ok(peaks
        .iter()
        .map(|&p| {
            let x = f64::from(p) / f64::from(u32::MAX >> 1);
            ((x.min(1.0).sqrt()) * 255.0).round() as u8
        })
        .collect())
}
