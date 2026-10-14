//! Songs made smaller on their way to a card: hi-res FLAC as 16-bit FLAC at 44.1 or 48 kHz
//! (still lossless for any player), or lossless songs as AAC for phones and cars. The library's
//! files are only ever read.
//!
//! The audio flows through in chunks (decode → resample → dither → encode), so a long hi-res
//! song never has to fit in memory at once.

use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::codecs::FlacDecoder;
use symphonia::default::formats::FlacReader;

/// Frames the resampler takes at a time.
const CHUNK: usize = 4096;

/// A FLAC file's audio as interleaved f32 chunks at `rate`, at most two channels.
pub(crate) struct PcmStream {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// The file's own sample rate.
    pub in_rate: u32,
    /// The rate the chunks come out at.
    pub rate: u32,
    pub channels: usize,
    /// Channels in the file (only the first two are kept).
    file_channels: usize,
    resampler: Option<rubato::Fft<f32>>,
    /// Planar input waiting for the resampler.
    pending: Vec<Vec<f32>>,
    /// Frames still to drop from the resampler's output (its startup delay).
    skip: usize,
    /// Frames in and out so far, so the output ends at exactly the source's length.
    frames_in: u64,
    frames_out: u64,
    ended: bool,
}

impl PcmStream {
    pub fn open(path: &Path, rate: u32) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("open: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let reader: Box<dyn FormatReader> = Box::new(
            FlacReader::try_new(mss, FormatOptions::default()).map_err(|e| format!("read: {e}"))?,
        );
        let track = reader.tracks().first().ok_or("no audio track")?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err("not audio".into());
        };
        let track_id = track.id;
        let decoder: Box<dyn AudioDecoder> = Box::new(
            FlacDecoder::try_new(&params, &AudioDecoderOptions::default())
                .map_err(|e| format!("decoder: {e}"))?,
        );
        let in_rate = params.sample_rate.ok_or("no sample rate")?;
        let file_channels = params
            .channels
            .as_ref()
            .map(|c| c.count())
            .unwrap_or(2)
            .max(1);
        let channels = file_channels.min(2);
        let resampler = if in_rate == rate {
            None
        } else {
            Some(
                rubato::Fft::<f32>::new(
                    in_rate as usize,
                    rate as usize,
                    CHUNK,
                    channels,
                    rubato::FixedSync::Input,
                )
                .map_err(|e| format!("resampler: {e}"))?,
            )
        };
        let skip = resampler
            .as_ref()
            .map(rubato::Resampler::output_delay)
            .unwrap_or(0);
        Ok(Self {
            reader,
            decoder,
            track_id,
            in_rate,
            rate,
            channels,
            file_channels,
            resampler,
            pending: vec![Vec::new(); channels],
            skip,
            frames_in: 0,
            frames_out: 0,
            ended: false,
        })
    }

    /// The file's next frames, interleaved, first two channels only; None at the end.
    fn decode_next(&mut self) -> Result<Option<Vec<f32>>, String> {
        let mut buf = Vec::new();
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                // How the reader says the stream has ended.
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(format!("read: {e}")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|e| format!("decode: {e}"))?;
            decoded.copy_to_vec_interleaved::<f32>(&mut buf);
            if self.file_channels == self.channels {
                return Ok(Some(buf));
            }
            let (fc, ch) = (self.file_channels, self.channels);
            return Ok(Some(
                buf.chunks(fc)
                    .flat_map(|f| f[..ch.min(f.len())].iter().copied())
                    .collect(),
            ));
        }
    }

    /// The next chunk of interleaved output; None at the end.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<f32>>, String> {
        let Some(mut rs) = self.resampler.take() else {
            return self.decode_next();
        };
        let out = self.resample_next(&mut rs);
        self.resampler = Some(rs);
        out
    }

    fn resample_next(&mut self, rs: &mut rubato::Fft<f32>) -> Result<Option<Vec<f32>>, String> {
        use rubato::Resampler;
        use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
        let ch = self.channels;
        loop {
            let need = rs.input_frames_next();
            while !self.ended && self.pending[0].len() < need {
                match self.decode_next()? {
                    Some(frames) => {
                        for frame in frames.chunks(ch) {
                            for (c, p) in self.pending.iter_mut().enumerate() {
                                p.push(frame.get(c).copied().unwrap_or(0.0));
                            }
                        }
                        self.frames_in += (frames.len() / ch) as u64;
                    }
                    None => self.ended = true,
                }
            }
            // The source's length at the new rate: the output ends there exactly.
            let total_out = (self.frames_in * u64::from(self.rate) + u64::from(self.in_rate) / 2)
                / u64::from(self.in_rate);
            if self.ended && self.frames_out >= total_out {
                return Ok(None);
            }
            let have = self.pending[0].len().min(need);
            // The last chunk is padded with silence, which also pushes the delay through.
            let input: Vec<Vec<f32>> = self
                .pending
                .iter()
                .map(|p| {
                    let mut v = p[..have].to_vec();
                    v.resize(need, 0.0);
                    v
                })
                .collect();
            for p in &mut self.pending {
                p.drain(..have);
            }
            let adapter = SequentialSliceOfVecs::new(&input, ch, need)
                .map_err(|e| format!("resampler: {e:?}"))?;
            let mut data = rs
                .process(&adapter, None)
                .map_err(|e| format!("resample: {e}"))?
                .take_data();
            let drop = self.skip.min(data.len() / ch);
            self.skip -= drop;
            data.drain(..drop * ch);
            let left = total_out.saturating_sub(self.frames_out) as usize;
            data.truncate(left.min(data.len() / ch) * ch);
            self.frames_out += (data.len() / ch) as u64;
            if !data.is_empty() {
                return Ok(Some(data));
            }
        }
    }
}

/// Floating-point samples to 16-bit, with triangular dither (what makes a 24-bit master sound
/// right at 16 bits instead of gritty in its quietest parts).
pub(crate) struct Dither {
    state: u64,
}

impl Dither {
    pub fn new() -> Self {
        Self {
            state: 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next(&mut self) -> f32 {
        // xorshift64*: plenty for dither noise.
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let v = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn sample16(&mut self, x: f32) -> i32 {
        let v = x * 32768.0;
        let tpdf = self.next() - self.next();
        // Already an exact 16-bit value (16-bit audio stored as 24-bit): kept exactly, with no
        // noise added. Only real detail below 16 bits is dithered.
        let out = if v.fract() == 0.0 {
            v
        } else {
            (v + tpdf).round()
        };
        out.clamp(-32768.0, 32767.0) as i32
    }
}

// --- 16-bit FLAC ---------------------------------------------------------------------------

struct FlacSource {
    pcm: PcmStream,
    dither: Dither,
    queue: Vec<i32>,
    error: Option<String>,
}

impl flacenc::source::Source for FlacSource {
    fn channels(&self) -> usize {
        self.pcm.channels
    }
    fn bits_per_sample(&self) -> usize {
        16
    }
    fn sample_rate(&self) -> usize {
        self.pcm.rate as usize
    }
    fn read_samples<F: flacenc::source::Fill>(
        &mut self,
        block_size: usize,
        dest: &mut F,
    ) -> Result<usize, flacenc::error::SourceError> {
        let want = block_size * self.pcm.channels;
        while self.queue.len() < want && self.error.is_none() {
            match self.pcm.next_chunk() {
                Ok(Some(chunk)) => {
                    let d = &mut self.dither;
                    self.queue.extend(chunk.iter().map(|&x| d.sample16(x)));
                }
                Ok(None) => break,
                Err(e) => self.error = Some(e),
            }
        }
        if self.error.is_some() {
            return Err(flacenc::error::SourceError::from_unknown());
        }
        let n = want.min(self.queue.len());
        let take: Vec<i32> = self.queue.drain(..n).collect();
        dest.fill_interleaved(&take)?;
        Ok(n / self.pcm.channels)
    }
}

/// A 16-bit FLAC stream (bytes, starting "fLaC") of a library FLAC, at `rate`.
pub fn flac16(src: &Path, rate: u32) -> Result<Vec<u8>, String> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let mut source = FlacSource {
        pcm: PcmStream::open(src, rate)?,
        dither: Dither::new(),
        queue: Vec::new(),
        error: None,
    };
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| format!("encoder: {e:?}"))?;
    let stream = flacenc::encode_with_fixed_block_size(&config, &mut source, config.block_size)
        .map_err(|e| match &source.error {
            Some(why) => why.clone(),
            None => format!("encode: {e:?}"),
        })?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| format!("encode: {e:?}"))?;
    let mut out = sink.as_slice().to_vec();
    fix_min_block(&mut out);
    Ok(out)
}

/// flacenc writes the last (short) block's size as the stream's minimum block size; the FLAC
/// format counts every block but the last, and readers (symphonia among them, and likely some
/// players) refuse a stream that says otherwise. Every block here is the same size but the
/// last, so the minimum is the maximum.
pub(crate) fn fix_min_block(stream: &mut [u8]) {
    if stream.len() > 12 && stream.starts_with(b"fLaC") && stream[4] & 0x7F == 0 {
        stream[8] = stream[10];
        stream[9] = stream[11];
    }
}

// --- AAC -------------------------------------------------------------------------------------

/// A library FLAC as AAC in an .m4a file at `dst`, at `kbps` (96 to 192), by Windows' own
/// encoder (Media Foundation): nothing to download. 44.1 and 48 kHz only, so other rates are
/// resampled to the nearer one.
#[cfg(windows)]
pub fn aac(src: &Path, dst: &Path, kbps: u32) -> Result<(), String> {
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::core::HSTRING;

    // The source's own rate decides 44.1 or 48 kHz (whichever family it belongs to).
    let in_rate = PcmStream::open(src, 44_100)?.in_rate;
    let rate = if in_rate.is_multiple_of(44_100) {
        44_100
    } else {
        48_000
    };
    let mut pcm = PcmStream::open(src, rate)?;
    let ch = pcm.channels as u32;
    let mut dither = Dither::new();

    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| -> Result<(), String> {
            let e = |what: &str| {
                let what = what.to_owned();
                move |err: windows::core::Error| format!("{what}: {err}")
            };
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(e("Media Foundation"))?;
            let inner = (|| -> Result<(), String> {
                // The file is written as ".part" first: say it's MPEG-4, not by its name.
                let mut attrs: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut attrs, 2).map_err(e("attributes"))?;
                let attrs = attrs.ok_or("attributes")?;
                attrs
                    .SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)
                    .map_err(e("container"))?;
                let writer =
                    MFCreateSinkWriterFromURL(&HSTRING::from(dst.as_os_str()), None, &attrs)
                        .map_err(e("writer"))?;

                let out = MFCreateMediaType().map_err(e("type"))?;
                out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                    .map_err(e("type"))?;
                out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
                    .map_err(e("type"))?;
                out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
                    .map_err(e("type"))?;
                out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)
                    .map_err(e("type"))?;
                out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)
                    .map_err(e("type"))?;
                out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, kbps * 1000 / 8)
                    .map_err(e("type"))?;
                let stream = writer.AddStream(&out).map_err(e("stream"))?;

                let input = MFCreateMediaType().map_err(e("type"))?;
                input
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                    .map_err(e("type"))?;
                input
                    .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
                    .map_err(e("type"))?;
                input
                    .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
                    .map_err(e("type"))?;
                input
                    .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)
                    .map_err(e("type"))?;
                input
                    .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)
                    .map_err(e("type"))?;
                input
                    .SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, ch * 2)
                    .map_err(e("type"))?;
                input
                    .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * ch * 2)
                    .map_err(e("type"))?;
                writer
                    .SetInputMediaType(stream, &input, None)
                    .map_err(e("input"))?;
                writer.BeginWriting().map_err(e("begin"))?;

                let mut frames_done: u64 = 0;
                while let Some(chunk) = pcm.next_chunk()? {
                    let bytes: Vec<u8> = chunk
                        .iter()
                        .flat_map(|&x| (dither.sample16(x) as i16).to_le_bytes())
                        .collect();
                    let frames = (chunk.len() / ch as usize) as u64;
                    let buffer = MFCreateMemoryBuffer(bytes.len() as u32).map_err(e("buffer"))?;
                    let mut ptr = std::ptr::null_mut();
                    buffer.Lock(&mut ptr, None, None).map_err(e("buffer"))?;
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                    buffer.Unlock().map_err(e("buffer"))?;
                    buffer
                        .SetCurrentLength(bytes.len() as u32)
                        .map_err(e("buffer"))?;
                    let sample = MFCreateSample().map_err(e("sample"))?;
                    sample.AddBuffer(&buffer).map_err(e("sample"))?;
                    // Times in 100-nanosecond units.
                    sample
                        .SetSampleTime((frames_done * 10_000_000 / u64::from(rate)) as i64)
                        .map_err(e("sample"))?;
                    sample
                        .SetSampleDuration((frames * 10_000_000 / u64::from(rate)) as i64)
                        .map_err(e("sample"))?;
                    writer.WriteSample(stream, &sample).map_err(e("write"))?;
                    frames_done += frames;
                }
                writer.Finalize().map_err(e("finish"))?;
                Ok(())
            })();
            let _ = MFShutdown();
            inner
        })();
        if com.is_ok() {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
pub fn aac(_src: &Path, _dst: &Path, _kbps: u32) -> Result<(), String> {
    Err("Converting to AAC uses Windows' own encoder; it isn't available here.".into())
}

/// The library FLAC's tags and front cover onto an .m4a (cover re-saved at most `cover_px`).
pub(crate) fn tag_m4a(src: &Path, dst: &Path, cover_px: u32) -> Result<(), String> {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::TaggedFileExt;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::probe::Probe;
    use lofty::tag::{Tag, TagExt, TagType};
    let tagged = Probe::open(src)
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new())
        .read()
        .map_err(|e| e.to_string())?;
    let Some(from) = tagged.primary_tag().or(tagged.first_tag()) else {
        return Ok(());
    };
    let mut tag = Tag::new(TagType::Mp4Ilst);
    for item in from.items() {
        tag.insert(item.clone());
    }
    let front = from
        .pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or(from.pictures().first());
    if let Some(p) = front {
        let data = small_cover(p.data(), cover_px);
        let mime = if data.starts_with(&[0xFF, 0xD8]) {
            MimeType::Jpeg
        } else {
            p.mime_type().cloned().unwrap_or(MimeType::Jpeg)
        };
        tag.push_picture(
            Picture::unchecked(data)
                .pic_type(PictureType::CoverFront)
                .mime_type(mime)
                .build(),
        );
    }
    tag.save_to_path(dst, WriteOptions::default())
        .map_err(|e| format!("tags: {e}"))
}

/// A cover as a baseline JPEG of at most `max_px`, unless it already is a small one.
fn small_cover(bytes: &[u8], max_px: u32) -> Vec<u8> {
    let fine = crate::media::picture::inspect(bytes).is_some_and(|i| {
        i.width.max(i.height) <= max_px && !i.progressive && bytes.len() <= 1_350_000
    });
    if fine {
        return bytes.to_vec();
    }
    crate::apply::baseline_jpeg(bytes, max_px).unwrap_or_else(|_| bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dither_keeps_silence_quiet_and_full_scale_in_range() {
        let mut d = Dither::new();
        for _ in 0..10_000 {
            assert!(d.sample16(0.0).abs() <= 1);
            assert!((-32768..=32767).contains(&d.sample16(1.5)));
        }
        assert_eq!(d.sample16(-2.0), -32768);
    }

    #[test]
    fn sixteen_bit_audio_in_a_24_bit_file_comes_out_exactly() {
        let mut d = Dither::new();
        for s in [-32768i32, -1, 0, 1, 12345, 32767] {
            // A 24-bit sample whose last 8 bits are zero, as the decoder gives it.
            let x = (s * 256) as f32 / 8_388_608.0;
            assert_eq!(d.sample16(x), s);
        }
    }
}
