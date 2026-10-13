//! Opening and decoding one song: any format symphonia reads, as interleaved samples.
//!
//! Samples come out as left-justified `i32` (a 16-bit sample `s` is `s << 16`), the form
//! symphonia uses, so a lossless source's exact values survive: shifting back recovers them.

use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

/// What a song is: its rate, channels and bit depth, and whether those are its true values.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SourceFormat {
    pub rate: u32,
    pub channels: u16,
    /// Bits per sample as stored (16, 24, 32). Lossy formats report 32: their samples are
    /// computed, not stored.
    pub bits: u16,
    /// Lossless (FLAC, ALAC, WAV, AIFF): the samples are the original ones.
    pub lossless: bool,
    /// "FLAC", "MP3", "AAC", "ALAC", "WAV", "AIFF", "Vorbis".
    pub codec: String,
}

/// About the file itself (not its sound): for the audio info.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FileInfo {
    pub size_bytes: u64,
    /// Average bitrate over the whole file, in kbit/s.
    pub bitrate_kbps: u32,
    /// The decoder that turns it into samples.
    pub decoder: String,
}

pub struct Source {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    pub format: SourceFormat,
    /// Length in frames, when the file says.
    pub frames: Option<u64>,
    pub info: FileInfo,
    /// Frames to drop from the next decoded output (to land exactly on a seek target).
    skip: u64,
}

fn codec_name(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("flac") => "FLAC",
        Some("mp3") => "MP3",
        Some("m4a" | "mp4" | "aac") => "AAC",
        Some("wav") => "WAV",
        Some("aif" | "aiff") => "AIFF",
        Some("ogg" | "oga") => "Vorbis",
        _ => "Audio",
    }
    .to_owned()
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(
            Box::new(file),
            MediaSourceStreamOptions {
                buffer_len: 1 << 20,
            },
        );
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let reader = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| format!("Not a format Venuta can play ({e})."))?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or("The file has no audio.")?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err("The file has no audio.".into());
        };
        let track_id = track.id;
        let frames = track.num_frames;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("Venuta can't decode this file ({e})."))?;
        let mut codec = codec_name(path);
        if codec == "AAC" && params.bits_per_sample.is_some() {
            // An MP4 with stored bit depth is Apple Lossless.
            codec = "ALAC".into();
        }
        let lossless = matches!(codec.as_str(), "FLAC" | "ALAC" | "WAV" | "AIFF");
        let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let rate_hz = u64::from(params.sample_rate.unwrap_or(44_100).max(1));
        let bitrate_kbps = frames
            .filter(|&f| f > 0)
            .map(|f| (size_bytes * 8 * rate_hz / f / 1000) as u32)
            .unwrap_or(0);
        let info = FileInfo {
            size_bytes,
            bitrate_kbps,
            decoder: format!("symphonia {} decoder", codec),
        };
        let format = SourceFormat {
            rate: params.sample_rate.unwrap_or(44_100),
            channels: params
                .channels
                .as_ref()
                .map(|c| c.count() as u16)
                .unwrap_or(2)
                .max(1),
            bits: if lossless {
                params.bits_per_sample.unwrap_or(16) as u16
            } else {
                32
            },
            lossless,
            codec,
        };
        Ok(Self {
            reader,
            decoder,
            track_id,
            format,
            frames,
            info,
            skip: 0,
        })
    }

    /// Length in milliseconds, when known.
    pub fn duration_ms(&self) -> Option<u64> {
        self.frames
            .map(|f| f * 1000 / u64::from(self.format.rate.max(1)))
    }

    /// Decode the next stretch into `out` (cleared first). `Ok(false)` at the end.
    pub fn next(&mut self, out: &mut Vec<i32>) -> Result<bool, String> {
        let channels = usize::from(self.format.channels);
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(e) => return Err(format!("The file is damaged here ({e}).")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                // One bad packet in a long file: skip it rather than stop the song.
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(format!("The file is damaged here ({e}).")),
            };
            out.clear();
            buf.copy_to_vec_interleaved::<i32>(out);
            if self.skip > 0 {
                let frames = (out.len() / channels) as u64;
                let drop = self.skip.min(frames);
                out.drain(..drop as usize * channels);
                self.skip -= drop;
                if out.is_empty() {
                    continue;
                }
            }
            return Ok(true);
        }
    }

    /// Go to `ms`, exactly: the container seeks to the nearest point before it, and the frames
    /// in between are decoded and dropped. Returns the frame now playing from.
    pub fn seek(&mut self, ms: u64) -> Result<u64, String> {
        let rate = u64::from(self.format.rate.max(1));
        let mut target = ms * rate / 1000;
        if let Some(total) = self.frames {
            target = target.min(total.saturating_sub(1));
        }
        let time = Time::try_new(
            (target / rate) as i64,
            ((target % rate) * 1_000_000_000 / rate) as u32,
        )
        .unwrap_or(Time::ZERO);
        let seeked = self
            .reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| format!("Could not move there ({e})."))?;
        self.decoder.reset();
        let actual = u64::try_from(seeked.actual_ts.get()).unwrap_or(0);
        let required = u64::try_from(seeked.required_ts.get()).unwrap_or(target);
        // Timestamps are in frames for every format Venuta plays (time base 1/rate).
        self.skip = required.saturating_sub(actual);
        Ok(required)
    }
}
