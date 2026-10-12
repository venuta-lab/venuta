//! Deep check: decode every track once and learn what the bytes can't tell us.
//!
//! One pass per album gives:
//! * **integrity**: decoded audio vs the STREAMINFO MD5 (what `flac -t` does);
//! * **lossy-source suspicion**: where the spectrum falls off a cliff (MP3/AAC cut ~16–20 kHz);
//! * **upsampled "hi-res"**: no real content above the original sample rate's Nyquist;
//! * **padded 24-bit**: 24-bit files whose low 8 bits are always zero;
//! * **loudness** (EBU R128) for ReplayGain 2.0, per track and per album.
//!
//! These are *suspicions with evidence*, never verdicts; old analogue masters can be
//! band-limited and still be genuine.

use ebur128::{EbuR128, Mode};
use realfft::RealFftPlanner;
use serde::Serialize;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::codecs::FlacDecoder;
use symphonia::default::formats::FlacReader;

/// ReplayGain 2.0 reference loudness.
pub const RG_REFERENCE_LUFS: f64 = -18.0;

const FFT_LEN: usize = 8192;
/// Analyse one FFT block in this many, spread evenly over the track.
const FFT_EVERY: usize = 12;

#[derive(Debug, Clone, Default, Serialize)]
pub struct TrackQuality {
    /// `Some(true)` decoded audio matches the MD5; `Some(false)` it does not (damage);
    /// `None` the file has no MD5 or could not be fully decoded.
    pub verify_ok: Option<bool>,
    pub decode_error: Option<String>,
    /// Bits actually used: 16 for a "24-bit" file padded from 16-bit audio.
    pub effective_bits: Option<u8>,
    /// Highest frequency with meaningful energy (Hz).
    pub cutoff_hz: Option<u32>,
    /// For >48 kHz files: mean level 23–40 kHz relative to 1–4 kHz (dB). Below ~-100 means
    /// nothing is there, i.e. upsampled.
    pub hires_db: Option<f32>,
    pub lufs: Option<f64>,
    pub peak: Option<f64>,
    pub album_lufs: Option<f64>,
    pub album_peak: Option<f64>,
}

struct TrackRun {
    quality: TrackQuality,
    meter: Option<EbuR128>,
}

/// Analyse one album (all tracks share album loudness). Tracks are `(id, absolute path)`.
pub fn analyze_album(
    tracks: &[(i64, std::path::PathBuf)],
    cancel: &AtomicBool,
) -> Vec<(i64, TrackQuality)> {
    let mut planner = RealFftPlanner::<f32>::new();
    let mut runs: Vec<(i64, TrackRun)> = Vec::with_capacity(tracks.len());
    for (id, path) in tracks {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        runs.push((*id, analyze_track(path, &mut planner)));
    }
    let meters: Vec<&EbuR128> = runs.iter().filter_map(|(_, r)| r.meter.as_ref()).collect();
    let album_lufs = (!meters.is_empty())
        .then(|| EbuR128::loudness_global_multiple(meters.iter().copied()).ok())
        .flatten()
        .filter(|l| l.is_finite());
    let album_peak = runs
        .iter()
        .filter_map(|(_, r)| r.quality.peak)
        .fold(None, |m: Option<f64>, p| Some(m.map_or(p, |m| m.max(p))));
    runs.into_iter()
        .map(|(id, mut r)| {
            r.quality.album_lufs = album_lufs;
            r.quality.album_peak = album_peak;
            (id, r.quality)
        })
        .collect()
}

/// An opened FLAC stream, ready to decode packet by packet.
pub(crate) struct FlacStream {
    reader: FlacReader<'static>,
    decoder: FlacDecoder,
    track_id: u32,
    pub rate: u32,
    pub channels: usize,
    pub bits: u32,
    pub frames: Option<u64>,
}

impl FlacStream {
    pub(crate) fn open(path: &Path, verify: bool) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("open: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let reader =
            FlacReader::try_new(mss, FormatOptions::default()).map_err(|e| format!("read: {e}"))?;
        let track = reader.tracks().first().ok_or("no audio track")?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err("not audio".into());
        };
        let track_id = track.id;
        let frames = track.num_frames;
        let decoder = FlacDecoder::try_new(&params, &AudioDecoderOptions::default().verify(verify))
            .map_err(|e| format!("decoder: {e}"))?;
        Ok(Self {
            reader,
            decoder,
            track_id,
            rate: params.sample_rate.unwrap_or(44_100),
            channels: params
                .channels
                .as_ref()
                .map(|c| c.count())
                .unwrap_or(2)
                .max(1),
            bits: params.bits_per_sample.unwrap_or(16),
            frames,
        })
    }

    /// Decode the next packet into `out` (interleaved, left-justified i32).
    /// `Ok(false)` at the end of the stream.
    pub(crate) fn next(&mut self, out: &mut Vec<i32>) -> Result<bool, String> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(e) => return Err(format!("demux: {e}")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = self
                .decoder
                .decode(&packet)
                .map_err(|e| format!("decode: {e}"))?;
            out.clear();
            buf.copy_to_vec_interleaved::<i32>(out);
            return Ok(true);
        }
    }

    pub(crate) fn verify_ok(&mut self) -> Option<bool> {
        self.decoder.finalize().verify_ok
    }
}

fn analyze_track(path: &Path, planner: &mut RealFftPlanner<f32>) -> TrackRun {
    let mut q = TrackQuality::default();
    let mut stream = match FlacStream::open(path, true) {
        Ok(s) => s,
        Err(e) => {
            q.decode_error = Some(e);
            return TrackRun {
                quality: q,
                meter: None,
            };
        }
    };
    let (rate, channels, bits) = (stream.rate, stream.channels, stream.bits);

    let mut meter = EbuR128::new(channels as u32, rate, Mode::I | Mode::SAMPLE_PEAK).ok();
    let fft = planner.plan_fft_forward(FFT_LEN);
    let mut fft_in = fft.make_input_vec();
    let mut fft_out = fft.make_output_vec();
    let window: Vec<f32> = (0..FFT_LEN)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_LEN as f32).cos())
        .collect();
    let mut power = vec![0f64; FFT_LEN / 2 + 1];
    let mut blocks_used = 0usize;
    let mut mono: Vec<f32> = Vec::with_capacity(FFT_LEN * 2);
    let mut block_index = 0usize;

    let mut samples: Vec<i32> = Vec::new();
    let mut floats: Vec<f32> = Vec::new();
    let mut or_bits: u32 = 0;

    loop {
        match stream.next(&mut samples) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                q.decode_error = Some(e);
                break;
            }
        }
        // Samples are left-justified to 32 bits; OR-ing them reveals which low bits are ever used.
        for s in &samples {
            or_bits |= *s as u32;
        }
        floats.clear();
        floats.extend(samples.iter().map(|&s| s as f32 / 2_147_483_648.0));
        if let Some(m) = meter.as_mut() {
            let _ = m.add_frames_f32(&floats);
        }
        // Mono mix into FFT blocks; analyse every FFT_EVERY-th block.
        for frame in floats.chunks_exact(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
        }
        while mono.len() >= FFT_LEN {
            if block_index % FFT_EVERY == FFT_EVERY / 2 {
                for (dst, (s, w)) in fft_in.iter_mut().zip(mono.iter().zip(&window)) {
                    *dst = s * w;
                }
                if fft.process(&mut fft_in, &mut fft_out).is_ok() {
                    for (p, c) in power.iter_mut().zip(&fft_out) {
                        *p += c.norm_sqr() as f64;
                    }
                    blocks_used += 1;
                }
            }
            block_index += 1;
            mono.drain(..FFT_LEN);
        }
    }

    if q.decode_error.is_none() {
        q.verify_ok = stream.verify_ok();
    }
    if or_bits != 0 {
        let used = 32 - or_bits.trailing_zeros();
        q.effective_bits = Some(used.min(bits) as u8);
    }
    if blocks_used > 0 {
        let (cutoff, hires) = spectrum_summary(&power, rate);
        q.cutoff_hz = cutoff;
        q.hires_db = hires;
    }
    if let Some(m) = meter.as_ref() {
        q.lufs = m.loudness_global().ok().filter(|l| l.is_finite());
        q.peak = (0..channels as u32)
            .filter_map(|c| m.sample_peak(c).ok())
            .fold(None, |acc: Option<f64>, p| {
                Some(acc.map_or(p, |a| a.max(p)))
            });
    }
    TrackRun { quality: q, meter }
}

/// From averaged power, find the cutoff frequency and (for >48 kHz) the ultrasonic level.
fn spectrum_summary(power: &[f64], rate: u32) -> (Option<u32>, Option<f32>) {
    let bin_hz = rate as f64 / FFT_LEN as f64;
    let db: Vec<f64> = power.iter().map(|p| 10.0 * (p + 1e-20).log10()).collect();
    let band = |lo: f64, hi: f64| {
        let (a, b) = (
            (lo / bin_hz) as usize,
            ((hi / bin_hz) as usize).min(db.len() - 1),
        );
        if b <= a {
            return None;
        }
        Some(db[a..b].iter().sum::<f64>() / (b - a) as f64)
    };
    let Some(reference) = band(1000.0, 4000.0) else {
        return (None, None);
    };
    // Smooth over ~500 Hz, then take the highest frequency within 70 dB of the reference.
    let k = ((500.0 / bin_hz) as usize).max(1);
    let mut cutoff = None;
    for i in (k..db.len().saturating_sub(k)).rev() {
        let avg = db[i - k / 2..i + k / 2 + 1].iter().sum::<f64>() / (k + 1) as f64;
        if avg > reference - 70.0 {
            cutoff = Some((i as f64 * bin_hz) as u32);
            break;
        }
    }
    let hires = (rate > 48_000)
        .then(|| band(23_000.0, 40_000.0).map(|h| (h - reference) as f32))
        .flatten();
    (cutoff, hires)
}

/// Progress shared with the caller (UI) while a deep check runs.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeepCheckReport {
    pub albums: usize,
    pub tracks: usize,
    pub cancelled: bool,
    pub elapsed_ms: u128,
}

/// Analyse every album that has new or changed tracks. Albums run in parallel; results are
/// saved in batches so a cancelled or interrupted check keeps what it finished.
pub fn run(
    index: &mut crate::Index,
    root: &Path,
    progress: &Progress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> crate::Result<DeepCheckReport> {
    use rayon::prelude::*;
    let started = std::time::Instant::now();
    let todo = index.quality_todo()?;
    let total: usize = todo.iter().map(|(_, t)| t.len()).sum();
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    on_progress(0, total);

    let mut report = DeepCheckReport {
        albums: todo.len(),
        tracks: total,
        ..Default::default()
    };
    for batch in todo.chunks(24) {
        if progress.cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let results: Vec<(i64, TrackQuality)> = batch
            .par_iter()
            .flat_map_iter(|(_, tracks)| {
                let paths: Vec<(i64, std::path::PathBuf)> = tracks
                    .iter()
                    .map(|(id, rel)| (*id, root.join(rel)))
                    .collect();
                let out = analyze_album(&paths, &progress.cancel);
                let done = progress.done.fetch_add(out.len(), Ordering::Relaxed) + out.len();
                on_progress(done, total);
                out
            })
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        // Only complete albums are saved, so album loudness is never computed from a partial set.
        let complete: std::collections::HashSet<i64> = batch
            .iter()
            .filter(|(_, t)| t.iter().all(|(id, _)| results.iter().any(|(r, _)| r == id)))
            .flat_map(|(_, t)| t.iter().map(|(id, _)| *id))
            .collect();
        let keep: Vec<(i64, TrackQuality)> = results
            .into_iter()
            .filter(|(id, _)| complete.contains(id))
            .collect();
        index.save_quality(&keep, now)?;
    }
    report.cancelled |= progress.cancel.load(Ordering::Relaxed);
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

/// ReplayGain 2.0 gain in dB for a measured loudness.
pub fn replaygain_db(lufs: f64) -> f64 {
    RG_REFERENCE_LUFS - lufs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_detects_lowpass() {
        // Flat spectrum to 16 kHz, then silence: cutoff should land near 16 kHz.
        let rate = 44_100;
        let bin_hz = rate as f64 / FFT_LEN as f64;
        let power: Vec<f64> = (0..=FFT_LEN / 2)
            .map(|i| {
                if (i as f64) * bin_hz < 16_000.0 {
                    1.0
                } else {
                    1e-12
                }
            })
            .collect();
        let (cutoff, hires) = spectrum_summary(&power, rate);
        let c = cutoff.unwrap();
        assert!((15_500..=16_500).contains(&c), "cutoff {c}");
        assert!(hires.is_none());
    }

    #[test]
    fn upsampled_hires_has_no_ultrasonics() {
        let rate = 96_000;
        let bin_hz = rate as f64 / FFT_LEN as f64;
        let power: Vec<f64> = (0..=FFT_LEN / 2)
            .map(|i| {
                if (i as f64) * bin_hz < 22_000.0 {
                    1.0
                } else {
                    1e-15
                }
            })
            .collect();
        let (_, hires) = spectrum_summary(&power, rate);
        assert!(hires.unwrap() < -100.0);
    }

    #[test]
    fn replaygain_reference() {
        assert!((replaygain_db(-10.0) - -8.0).abs() < 1e-9);
    }
}
