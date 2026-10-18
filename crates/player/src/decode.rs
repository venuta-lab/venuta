//! Opening and decoding one song: any format symphonia reads, as interleaved samples.
//!
//! Samples come out as left-justified `i32` (a 16-bit sample `s` is `s << 16`), the form
//! symphonia uses, so a lossless source's exact values survive: shifting back recovers them.

use media_access::EphemeralAudio;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{
    MediaSource as SymphoniaMediaSource, MediaSourceStream, MediaSourceStreamOptions,
};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

/// Default bytes before the first open attempt (overridden per mime below).
const OPEN_AFTER_DEFAULT: usize = 256 * 1024;

/// How many bytes we want before probing a container (smaller → faster start; MP4 needs more).
fn open_after_bytes(mime_type: Option<&str>, url: &str) -> usize {
    match media_extension(mime_type, url).as_deref() {
        Some("wav" | "aiff") => 24 * 1024,
        Some("mp3" | "ogg" | "oga") => 64 * 1024,
        // Enough for the header and the first fragment of a streamed MP4. A probe that runs
        // short retries with more as it arrives, so a small window only ever costs a retry.
        Some("m4a" | "mp4" | "aac") => 128 * 1024,
        Some("flac") => 128 * 1024,
        _ => OPEN_AFTER_DEFAULT,
    }
}

fn is_starved(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock || err.raw_os_error() == Some(35) // EAGAIN on some platforms
}

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
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct FileInfo {
    pub size_bytes: u64,
    /// Average bitrate over the whole file, in kbit/s.
    pub bitrate_kbps: u32,
    /// The decoder that turns it into samples.
    pub decoder: String,
}

pub struct Source {
    reader: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn AudioDecoder>>,
    track_id: u32,
    pub format: SourceFormat,
    /// Length in frames, when the file says.
    pub frames: Option<u64>,
    pub info: FileInfo,
    /// Frames to drop from the next decoded output (to land exactly on a seek target).
    skip: u64,
    duration_hint_ms: Option<u64>,
    remote: Option<RemoteState>,
    /// Progressive download body (cancel + wake blocked readers on Drop).
    download_body: Option<Arc<GrowBody>>,
    /// True once at least one decoded packet produced samples (UI may leave Opening…).
    primed: bool,
}

struct RemoteState {
    receiver: Receiver<RemoteEvent>,
    cancelled: Arc<AtomicBool>,
    finished: bool,
    /// Seek requested while the body was still downloading; applied on open.
    pending_seek_ms: Option<u64>,
    /// Open attempt failed for want of bytes; retry when the body grows past this length.
    retry_after: Option<usize>,
    /// Last OpenReady payload kept for retry after a starved probe.
    pending_open: Option<PendingOpen>,
}

struct PendingOpen {
    body: Arc<GrowBody>,
    mime_type: Option<String>,
    url_hint: String,
    duration_hint_ms: Option<u64>,
    size_hint: Option<u64>,
}

/// Worker downloads into a shared grow buffer; main opens once `OPEN_AFTER` bytes arrive.
enum RemoteEvent {
    OpenReady {
        body: Arc<GrowBody>,
        mime_type: Option<String>,
        url_hint: String,
        duration_hint_ms: Option<u64>,
        size_hint: Option<u64>,
    },
    Error(String),
}

/// Shared download buffer: the decoder reads while the worker appends.
struct GrowBody {
    data: Mutex<Vec<u8>>,
    more: Condvar,
    finished: AtomicBool,
    /// Same flag the Source Drop trips (wakes blocked readers).
    cancelled: Arc<AtomicBool>,
    /// Known Content-Length when the host gave one; 0 if unknown.
    total: AtomicU64,
}

impl GrowBody {
    fn new(cancelled: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(Vec::with_capacity(OPEN_AFTER_DEFAULT)),
            more: Condvar::new(),
            finished: AtomicBool::new(false),
            cancelled,
            total: AtomicU64::new(0),
        })
    }

    fn len(&self) -> usize {
        self.data.lock().map(|g| g.len()).unwrap_or(0)
    }

    fn append(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if let Ok(mut g) = self.data.lock() {
            g.extend_from_slice(chunk);
            self.more.notify_all();
        }
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::Release);
        self.more.notify_all();
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.finished.store(true, Ordering::Release);
        self.more.notify_all();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn set_total(&self, total: u64) {
        if total > 0 {
            self.total.store(total, Ordering::Release);
        }
    }
}

/// Reads from [`GrowBody`] without blocking the player thread.
///
/// When the decoder outruns the download, returns [`io::ErrorKind::WouldBlock`] so decode can
/// yield [`DecodeState::Pending`] instead of freezing audio.
struct GrowReader {
    body: Arc<GrowBody>,
    pos: u64,
}

impl Read for GrowReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let g = self
            .body
            .data
            .lock()
            .map_err(|_| io::Error::other("download buffer lock"))?;
        if self.body.cancelled.load(Ordering::Acquire) {
            return Ok(0);
        }
        let len = g.len() as u64;
        if self.pos < len {
            let start = self.pos as usize;
            let n = ((len - self.pos) as usize).min(buf.len());
            buf[..n].copy_from_slice(&g[start..start + n]);
            self.pos += n as u64;
            return Ok(n);
        }
        if self.body.finished.load(Ordering::Acquire) {
            return Ok(0);
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "waiting for more of the stream",
        ))
    }
}

impl Seek for GrowReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let g = self
            .body
            .data
            .lock()
            .map_err(|_| io::Error::other("download buffer lock"))?;
        let finished = self.body.finished.load(Ordering::Acquire);
        let cancelled = self.body.cancelled.load(Ordering::Acquire);
        let len = g.len() as u64;
        // End-relative seeks need the finished length; until then, don't block — caller retries.
        if matches!(pos, SeekFrom::End(_)) && !finished && !cancelled {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "waiting for the full stream to seek from the end",
            ));
        }
        let mut target = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(p) => {
                if p >= 0 {
                    self.pos.saturating_add(p as u64)
                } else {
                    self.pos.saturating_sub((-p) as u64)
                }
            }
            SeekFrom::End(p) => {
                if p >= 0 {
                    len.saturating_add(p as u64)
                } else {
                    len.saturating_sub((-p) as u64)
                }
            }
        };
        if target > len && !finished && !cancelled {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "waiting for more of the stream to seek",
            ));
        }
        if target > len {
            target = len;
        }
        self.pos = target;
        Ok(self.pos)
    }
}

impl SymphoniaMediaSource for GrowReader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        let total = self.body.total.load(Ordering::Acquire);
        if total > 0 {
            return Some(total);
        }
        if self.body.finished.load(Ordering::Acquire) {
            return self.body.data.lock().ok().map(|g| g.len() as u64);
        }
        None
    }
}

/// Result of asking a source for its next decoded chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeState {
    Samples,
    /// A remote decoder has no chunk ready yet; polling again is safe and nonblocking.
    Pending,
    End,
}

enum LocalDecode {
    Samples,
    Pending,
    End,
}

fn codec_name(extension: Option<&str>) -> String {
    match extension.map(str::to_ascii_lowercase).as_deref() {
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
        let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        Self::from_media_source(
            Box::new(file),
            path.extension().and_then(|e| e.to_str()),
            None,
            size_bytes,
            None,
        )
    }

    /// Open resolved direct audio without performing network I/O on the caller's thread.
    pub fn open_remote(audio: EphemeralAudio) -> Result<Self, String> {
        let duration_hint_ms = audio.duration_ms;
        let (sender, receiver) = sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        std::thread::Builder::new()
            .name("venuta-stream-decoder".into())
            .spawn(move || decode_remote(audio, sender, worker_cancelled, None))
            .map_err(|_| "Could not start the remote stream decoder.")?;
        Ok(Self::remote_placeholder(
            receiver,
            cancelled,
            duration_hint_ms,
        ))
    }

    /// Resolve through MediaAccess on a worker thread, then download — never on the audio thread.
    pub fn open_remote_deferred(
        access: Arc<dyn media_access::MediaAccess>,
        track_id: impl Into<String>,
        duration_hint_ms: Option<u64>,
    ) -> Result<Self, String> {
        let track_id = track_id.into();
        let (sender, receiver) = sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        std::thread::Builder::new()
            .name("venuta-stream-resolve".into())
            .spawn(move || {
                if worker_cancelled.load(Ordering::Acquire) {
                    return;
                }
                let cancel = media_access::CancellationToken::default();
                // If the player dropped us, cooperative cancel still helps long resolves.
                let audio = match access.resolve_audio(&track_id, &cancel) {
                    Ok(a) => a,
                    Err(e) => {
                        let _ = sender.send(RemoteEvent::Error(e.to_string()));
                        return;
                    }
                };
                if worker_cancelled.load(Ordering::Acquire) {
                    return;
                }
                // Partway failures ask the catalog again, past its memory of the old address.
                let refresh: Refresh = Box::new(move || {
                    access.forget(&track_id);
                    access
                        .resolve_audio(&track_id, &media_access::CancellationToken::default())
                        .ok()
                });
                decode_remote(audio, sender, worker_cancelled, Some(refresh));
            })
            .map_err(|_| "Could not start the remote stream resolver.")?;
        Ok(Self::remote_placeholder(
            receiver,
            cancelled,
            duration_hint_ms,
        ))
    }

    fn remote_placeholder(
        receiver: Receiver<RemoteEvent>,
        cancelled: Arc<AtomicBool>,
        duration_hint_ms: Option<u64>,
    ) -> Self {
        Self {
            reader: None,
            decoder: None,
            track_id: 0,
            format: SourceFormat {
                rate: 44_100,
                channels: 2,
                bits: 32,
                lossless: false,
                codec: "Remote".into(),
            },
            frames: None,
            info: FileInfo::default(),
            skip: 0,
            duration_hint_ms,
            remote: Some(RemoteState {
                receiver,
                cancelled,
                finished: false,
                pending_seek_ms: None,
                retry_after: None,
                pending_open: None,
            }),
            download_body: None,
            primed: false,
        }
    }

    fn from_media_source(
        source: Box<dyn SymphoniaMediaSource>,
        extension: Option<&str>,
        mime_type: Option<&str>,
        size_bytes: u64,
        duration_hint_ms: Option<u64>,
    ) -> Result<Self, String> {
        let mss = MediaSourceStream::new(
            source,
            MediaSourceStreamOptions {
                buffer_len: 1 << 20,
            },
        );
        let mut hint = Hint::new();
        if let Some(ext) = extension {
            hint.with_extension(ext);
        }
        if let Some(mime) = mime_type {
            hint.mime_type(mime);
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
        let mut codec = codec_name(extension);
        if codec == "AAC" && params.bits_per_sample.is_some() {
            // An MP4 with stored bit depth is Apple Lossless.
            codec = "ALAC".into();
        }
        let lossless = matches!(codec.as_str(), "FLAC" | "ALAC" | "WAV" | "AIFF");
        let rate_hz = u64::from(params.sample_rate.unwrap_or(44_100).max(1));
        let bitrate_kbps = frames
            .filter(|&f| f > 0)
            .map(|f| (size_bytes * 8 * rate_hz / f / 1000) as u32)
            .or_else(|| {
                duration_hint_ms
                    .filter(|&duration| duration > 0)
                    .map(|duration| (size_bytes.saturating_mul(8) / duration) as u32)
            })
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
            reader: Some(reader),
            decoder: Some(decoder),
            track_id,
            format,
            frames,
            info,
            skip: 0,
            duration_hint_ms,
            remote: None,
            download_body: None,
            // Local files are ready immediately; progressive sets this after first samples.
            primed: true,
        })
    }

    /// True when the UI may leave Opening…: local always; remote only after first samples.
    pub fn is_ready(&self) -> bool {
        if self.remote.is_some() {
            return false;
        }
        if self.download_body.is_some() {
            return self.primed;
        }
        true
    }

    /// Length in milliseconds, when known.
    pub fn duration_ms(&self) -> Option<u64> {
        self.frames
            .map(|f| f * 1000 / u64::from(self.format.rate.max(1)))
            .or(self.duration_hint_ms)
    }

    /// Decode the next stretch into `out` (cleared first), or report that a remote chunk is
    /// not ready yet. This never waits for network I/O.
    pub fn next(&mut self, out: &mut Vec<i32>) -> Result<DecodeState, String> {
        if self.remote.is_some() {
            return self.next_remote(out);
        }
        match self.next_local(out)? {
            LocalDecode::Samples => {
                self.primed = true;
                Ok(DecodeState::Samples)
            }
            LocalDecode::End => Ok(DecodeState::End),
            LocalDecode::Pending => Ok(DecodeState::Pending),
        }
    }

    fn next_remote(&mut self, out: &mut Vec<i32>) -> Result<DecodeState, String> {
        out.clear();
        // Retry a previous open that starved for bytes.
        if let Some(state) = self.try_pending_open(out)? {
            return Ok(state);
        }
        let event = {
            let remote = self.remote.as_ref().expect("remote source");
            if remote.finished {
                return Ok(DecodeState::End);
            }
            remote.receiver.try_recv()
        };
        match event {
            Ok(RemoteEvent::OpenReady {
                body,
                mime_type,
                url_hint,
                duration_hint_ms,
                size_hint,
            }) => self.finish_open(
                PendingOpen {
                    body,
                    mime_type,
                    url_hint,
                    duration_hint_ms,
                    size_hint,
                },
                out,
            ),
            Ok(RemoteEvent::Error(error)) => {
                let remote = self.remote.as_mut().expect("remote source");
                remote.finished = true;
                Err(error)
            }
            Err(TryRecvError::Empty) => Ok(DecodeState::Pending),
            Err(TryRecvError::Disconnected) => {
                let remote = self.remote.as_mut().expect("remote source");
                if remote.pending_open.is_some() {
                    return self
                        .try_pending_open(out)?
                        .map(Ok)
                        .unwrap_or(Ok(DecodeState::Pending));
                }
                remote.finished = true;
                Err("The remote stream ended before audio could be decoded.".into())
            }
        }
    }

    fn try_pending_open(&mut self, out: &mut Vec<i32>) -> Result<Option<DecodeState>, String> {
        let Some(remote) = self.remote.as_ref() else {
            return Ok(None);
        };
        let Some(pending) = remote.pending_open.as_ref() else {
            return Ok(None);
        };
        let len = pending.body.len();
        if let Some(need) = remote.retry_after
            && len < need
            && !pending.body.finished.load(Ordering::Acquire)
        {
            return Ok(Some(DecodeState::Pending));
        }
        let pending = self
            .remote
            .as_mut()
            .and_then(|r| r.pending_open.take())
            .expect("pending_open");
        if let Some(r) = self.remote.as_mut() {
            r.retry_after = None;
        }
        self.finish_open(pending, out).map(Some)
    }

    fn finish_open(
        &mut self,
        pending: PendingOpen,
        out: &mut Vec<i32>,
    ) -> Result<DecodeState, String> {
        let pending_seek = self.remote.as_ref().and_then(|r| r.pending_seek_ms);
        let size_bytes = pending
            .size_hint
            .filter(|n| *n > 0)
            .unwrap_or_else(|| pending.body.len() as u64);
        let open = Self::from_media_source(
            Box::new(GrowReader {
                body: Arc::clone(&pending.body),
                pos: 0,
            }),
            media_extension(pending.mime_type.as_deref(), &pending.url_hint).as_deref(),
            pending.mime_type.as_deref(),
            size_bytes,
            pending.duration_hint_ms.or(self.duration_hint_ms),
        );
        match open {
            Ok(mut opened) => {
                opened.download_body = Some(pending.body);
                opened.primed = false;
                opened.duration_hint_ms = opened.duration_hint_ms.or(self.duration_hint_ms);
                // Drop remote handle without cancelling the shared body.
                if let Some(r) = self.remote.as_ref() {
                    // Keep cancelled flag shared via body.
                    let _ = r;
                }
                let _ = self.remote.take();
                *self = opened;
                if let Some(ms) = pending_seek {
                    match self.seek(ms) {
                        Ok(_) => {}
                        Err(e) if e.contains("waiting") => {}
                        Err(e) => return Err(e),
                    }
                }
                match self.next_local(out)? {
                    LocalDecode::Samples => {
                        self.primed = true;
                        Ok(DecodeState::Samples)
                    }
                    LocalDecode::Pending => Ok(DecodeState::Pending),
                    LocalDecode::End => Ok(DecodeState::End),
                }
            }
            Err(e) => {
                // Incomplete body / starved probe: keep downloading and retry. Finished body = real error.
                if pending.body.finished.load(Ordering::Acquire) {
                    return Err(e);
                }
                let retry = pending.body.len().saturating_add(128 * 1024);
                if let Some(remote) = self.remote.as_mut() {
                    remote.retry_after = Some(retry);
                    remote.pending_open = Some(pending);
                    return Ok(DecodeState::Pending);
                }
                Err(e)
            }
        }
    }

    fn next_local(&mut self, out: &mut Vec<i32>) -> Result<LocalDecode, String> {
        let channels = usize::from(self.format.channels);
        loop {
            let packet = match self.reader.as_mut().expect("local reader").next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(LocalDecode::End),
                Err(symphonia::core::errors::Error::IoError(ref e)) if is_starved(e) => {
                    return Ok(LocalDecode::Pending);
                }
                Err(e) => return Err(format!("The file is damaged here ({e}).")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self
                .decoder
                .as_mut()
                .expect("local decoder")
                .decode(&packet)
            {
                Ok(b) => b,
                Err(symphonia::core::errors::Error::IoError(ref e)) if is_starved(e) => {
                    return Ok(LocalDecode::Pending);
                }
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
            return Ok(LocalDecode::Samples);
        }
    }

    /// Go to `ms`, exactly: the container seeks to the nearest point before it, and the frames
    /// in between are decoded and dropped. Returns the frame now playing from.
    ///
    /// Remote songs open after the first download window; seek waits for bytes if needed.
    /// A seek asked before open is remembered and applied as soon as decode starts.
    pub fn seek(&mut self, ms: u64) -> Result<u64, String> {
        if let Some(remote) = self.remote.as_mut() {
            if remote.finished {
                return Err("This song has already finished.".into());
            }
            remote.pending_seek_ms = Some(ms);
            let rate = u64::from(self.format.rate.max(1));
            return Ok(ms.saturating_mul(rate) / 1000);
        }
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
            .as_mut()
            .expect("local reader")
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| match e {
                symphonia::core::errors::Error::IoError(ref io) if is_starved(io) => {
                    "waiting for more of the stream to seek".to_owned()
                }
                other => format!("Could not move there ({other})."),
            })?;
        self.decoder.as_mut().expect("local decoder").reset();
        let actual = u64::try_from(seeked.actual_ts.get()).unwrap_or(0);
        let required = u64::try_from(seeked.required_ts.get()).unwrap_or(target);
        // Timestamps are in frames for every format Venuta plays (time base 1/rate).
        self.skip = required.saturating_sub(actual);
        Ok(required)
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if let Some(body) = self.download_body.take() {
            body.cancel();
        }
        if let Some(remote) = self.remote.as_ref() {
            remote.cancelled.store(true, Ordering::Release);
        }
    }
}

/// Asks for a fresh stream address for the same song, after the old one stopped working partway.
type Refresh = Box<dyn FnMut() -> Option<EphemeralAudio> + Send>;

fn decode_remote(
    audio: EphemeralAudio,
    sender: SyncSender<RemoteEvent>,
    cancelled: Arc<AtomicBool>,
    refresh: Option<Refresh>,
) {
    let body = GrowBody::new(cancelled.clone());
    let result = (|| {
        if cancelled.load(Ordering::Acquire) {
            body.cancel();
            return Ok(());
        }
        fill_grow_body(audio, &body, &sender, refresh)?;
        Ok(())
    })();
    if let Err(error) = result
        && !cancelled.load(Ordering::Acquire)
    {
        body.cancel();
        let _ = sender.send(RemoteEvent::Error(error));
    }
}

/// Bytes asked for per request. Media hosts serve a bounded range at full speed, where an
/// open-ended one may be paced to the song's own rate.
const RANGE_BYTES: u64 = 2 * 1_048_576;
/// Fresh addresses asked for in one song before giving up on it.
const MAX_REFRESHES: u32 = 2;

/// Download into `body`, signalling [`RemoteEvent::OpenReady`] once enough bytes (or the whole
/// short body) are present so decode can start while the rest arrives.
///
/// Bytes are added as they arrive, not a whole request at a time, so the song opens as soon as
/// its first part is in. Every song shares one connection pool (see `media_access::media_agent`),
/// so the connection the resolver's check opened is the one this download uses.
///
/// When the host stops serving partway (the address expired, or was only good for its
/// opening), a fresh address is asked for and the download carries on from the same byte.
fn fill_grow_body(
    mut audio: EphemeralAudio,
    body: &Arc<GrowBody>,
    sender: &SyncSender<RemoteEvent>,
    mut refresh: Option<Refresh>,
) -> Result<(), String> {
    let agent = media_access::media_agent();
    let started = std::time::Instant::now();
    let url_hint = audio.url.clone();
    let duration_hint_ms = audio.duration_ms;

    let mut opened = false;
    let mut signal_open = |mime: &Option<String>, size_hint: Option<u64>| {
        if opened {
            return;
        }
        let need = open_after_bytes(mime.as_deref(), &url_hint);
        // Open once we have enough bytes, or the download already finished (short tracks).
        if body.len() < need && !body.finished.load(Ordering::Acquire) {
            return;
        }
        opened = true;
        media_access::note_timing(&format!(
            "online song ready to open after {} ms ({} KB in)",
            media_access::ms_since(started),
            body.len() / 1024
        ));
        let _ = sender.send(RemoteEvent::OpenReady {
            body: Arc::clone(body),
            mime_type: mime.clone(),
            url_hint: url_hint.clone(),
            duration_hint_ms,
            size_hint,
        });
    };

    let sync_cancel = || {
        if body.is_cancelled() {
            body.cancel();
            true
        } else {
            false
        }
    };
    let mut buf = vec![0_u8; 64 * 1024];

    // 1) Plain GET — works for ordinary CDN URLs and fixtures.
    // Skip for googlevideo hosts: they commonly 403 bare full GETs.
    let host_is_gvs = audio
        .url
        .split('/')
        .nth(2)
        .is_some_and(|host| host.contains("googlevideo.com"));
    if !host_is_gvs {
        let mut request = agent.get(&audio.url);
        for header in &audio.headers {
            request = request.header(&header.name, &header.value);
        }
        if let Ok(response) = request.call() {
            let status = response.status().as_u16();
            if (200..300).contains(&status) && status != 206 {
                let mime_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned());
                let len_hdr = response
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                if let Some(t) = len_hdr.or(audio.content_length) {
                    body.set_total(t);
                }
                let mut reader = response.into_body().into_reader();
                loop {
                    if sync_cancel() {
                        return Err("Remote stream cancelled.".into());
                    }
                    let n = reader
                        .read(&mut buf)
                        .map_err(|_| "Could not read the remote stream.".to_owned())?;
                    if n == 0 {
                        break;
                    }
                    body.append(&buf[..n]);
                    signal_open(&mime_type, len_hdr.or(audio.content_length));
                    if body.len() > 128 * 1024 * 1024 {
                        return Err("Remote stream is larger than Venuta will buffer.".into());
                    }
                }
                if body.len() == 0 {
                    // Fall through to ranged path.
                } else {
                    body.finish();
                    // After finish, even a short body (fixture WAV) may open.
                    signal_open(&mime_type, Some(body.len() as u64).or(len_hdr));
                    return Ok(());
                }
            }
        }
    }

    // 2) Ranged GETs (the media host's own path, and the fallback for any other).
    let mut start = 0u64;
    let mut mime_type = audio
        .mime_type
        .as_ref()
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned());
    let mut known_total = audio.content_length.filter(|n| *n > 0);
    if let Some(t) = known_total {
        body.set_total(t);
    }
    let mut refreshes = 0u32;
    let mut retried_here = false;

    // A request or a read failed partway. Try once more on the same address, then on a fresh
    // one; `Err` when neither is left.
    let mut recover = |audio: &mut EphemeralAudio, retried_here: &mut bool| -> bool {
        if !*retried_here {
            *retried_here = true;
            std::thread::sleep(Duration::from_millis(250));
            return true;
        }
        if refreshes >= MAX_REFRESHES {
            return false;
        }
        let Some(fresh) = refresh.as_mut().and_then(|ask| ask()) else {
            return false;
        };
        refreshes += 1;
        *retried_here = false;
        media_access::note_timing("online song asked for a fresh address partway");
        *audio = fresh;
        true
    };

    loop {
        if sync_cancel() {
            return Err("Remote stream cancelled.".into());
        }
        let end = match known_total {
            Some(total) if start >= total => break,
            Some(total) => (start + RANGE_BYTES - 1).min(total - 1),
            None => start + RANGE_BYTES - 1,
        };
        let mut request = agent.get(&audio.url);
        for header in &audio.headers {
            request = request.header(&header.name, &header.value);
        }
        request = request.header("Range", format!("bytes={start}-{end}"));
        let response = match request.call() {
            Ok(r) => r,
            Err(_) if body.len() > 0 && recover(&mut audio, &mut retried_here) => continue,
            Err(_) => return Err("Could not retrieve the remote stream.".to_owned()),
        };
        let status = response.status().as_u16();
        if status == 416 && body.len() > 0 {
            break;
        }
        if !(200..300).contains(&status) {
            if body.len() > 0 {
                // Refused partway: a fresh address carries on from here.
                if recover(&mut audio, &mut retried_here) {
                    continue;
                }
                break;
            }
            return Err(format!("The remote stream returned HTTP {status}."));
        }
        if mime_type.is_none() {
            mime_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned());
        }
        let total_from_header = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit_once('/').map(|(_, total)| total))
            .and_then(|total| total.parse::<u64>().ok());
        if let Some(t) = total_from_header {
            known_total = Some(t);
            body.set_total(t);
        }
        let mut reader = response.into_body().into_reader();
        let mut got = 0u64;
        let mut broke = false;
        loop {
            if sync_cancel() {
                return Err("Remote stream cancelled.".into());
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    body.append(&buf[..n]);
                    got += n as u64;
                    start += n as u64;
                    signal_open(&mime_type, known_total);
                    if body.len() > 128 * 1024 * 1024 {
                        return Err("Remote stream is larger than Venuta will buffer.".into());
                    }
                }
                Err(_) => {
                    broke = true;
                    break;
                }
            }
        }
        if broke {
            // The connection dropped mid-range: carry on from the last byte that arrived.
            if recover(&mut audio, &mut retried_here) {
                continue;
            }
            if body.len() == 0 {
                return Err("Could not read the remote stream.".into());
            }
            break;
        }
        retried_here = false;
        if got == 0 {
            break;
        }
        if let Some(total) = known_total
            && start >= total
        {
            break;
        }
        if known_total.is_none() && total_from_header.is_none() && got < RANGE_BYTES {
            break;
        }
    }

    if body.len() == 0 {
        return Err("The remote stream returned no audio data.".into());
    }
    body.finish();
    signal_open(&mime_type, known_total.or(Some(body.len() as u64)));
    Ok(())
}

fn media_extension(mime_type: Option<&str>, url: &str) -> Option<String> {
    let mime = mime_type.unwrap_or_default().to_ascii_lowercase();
    let extension = match mime.split(';').next().unwrap_or("").trim() {
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/mpeg" => Some("mp3"),
        "audio/aac" => Some("aac"),
        "audio/mp4" => Some("m4a"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        "audio/aiff" | "audio/x-aiff" => Some("aiff"),
        "audio/ogg" | "audio/vorbis" => Some("ogg"),
        _ => None,
    };
    extension.map(str::to_owned).or_else(|| {
        url.split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit_once('.').map(|(_, extension)| extension))
            .filter(|extension| !extension.is_empty())
            .map(str::to_ascii_lowercase)
    })
}
