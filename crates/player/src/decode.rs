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
        Some("mp3" | "ogg" | "oga" | "opus") => 64 * 1024,
        // Enough for the header and the first fragment of a streamed MP4. A probe that runs
        // short retries with more as it arrives, so a small window only ever costs a retry.
        Some("m4a" | "mp4" | "aac" | "webm") => 128 * 1024,
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
    /// The track's clock (seconds per timestamp tick), when it is not one tick per frame: a WebM
    /// stream counts in milliseconds. Seek answers come in these ticks.
    time_base: Option<(u64, u64)>,
    duration_hint_ms: Option<u64>,
    remote: Option<RemoteState>,
    /// Progressive download body (cancel + wake blocked readers on Drop).
    download_body: Option<Arc<GrowBody>>,
    /// True once at least one decoded packet produced samples (UI may leave Opening…).
    primed: bool,
    /// The last decode found nothing more downloaded yet (the stream is behind playback).
    starved: bool,
    /// An online song's loudness as the catalog says it (see `EphemeralAudio::loudness_db`).
    pub loudness_db: Option<f64>,
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
        loudness_db: Option<f64>,
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
    /// How far the decoder has read: what is downloaded past it is what playback has in hand.
    read_at: AtomicU64,
    /// Why the download stopped short, when it did: said once what arrived has played.
    failed: Mutex<Option<String>>,
}

impl GrowBody {
    fn new(cancelled: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(Vec::with_capacity(OPEN_AFTER_DEFAULT)),
            more: Condvar::new(),
            finished: AtomicBool::new(false),
            cancelled,
            total: AtomicU64::new(0),
            read_at: AtomicU64::new(0),
            failed: Mutex::new(None),
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

    /// The download stopped short for good: what has arrived still plays, then `why` is said.
    fn fail(&self, why: &str) {
        if let Ok(mut f) = self.failed.lock() {
            f.get_or_insert_with(|| why.to_owned());
        }
        self.finish();
    }

    fn failure(&self) -> Option<String> {
        self.failed.lock().ok().and_then(|f| f.clone())
    }

    /// Bytes downloaded past where the decoder has read.
    fn ahead(&self) -> u64 {
        (self.len() as u64).saturating_sub(self.read_at.load(Ordering::Acquire))
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
            self.body.read_at.store(self.pos, Ordering::Release);
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
        self.body.read_at.store(self.pos, Ordering::Release);
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

/// A timestamp in a track's ticks (`numer/denom` seconds each) as frames at `rate`.
fn ticks_to_frames(ticks: u64, numer: u64, denom: u64, rate: u64) -> u64 {
    (u128::from(ticks) * u128::from(numer) * u128::from(rate) / u128::from(denom.max(1))) as u64
}

/// Every decoder symphonia brings, and Opus (libopus) beside them.
fn codecs() -> &'static symphonia::core::codecs::registry::CodecRegistry {
    static CODECS: std::sync::OnceLock<symphonia::core::codecs::registry::CodecRegistry> =
        std::sync::OnceLock::new();
    CODECS.get_or_init(|| {
        let mut registry = symphonia::core::codecs::registry::CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        registry
    })
}

fn codec_name(extension: Option<&str>) -> String {
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("flac") => "FLAC",
        Some("mp3") => "MP3",
        Some("m4a" | "mp4" | "aac") => "AAC",
        Some("wav") => "WAV",
        Some("aif" | "aiff") => "AIFF",
        Some("ogg" | "oga") => "Vorbis",
        Some("opus" | "webm") => "Opus",
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
        Self::open_remote_watched(audio, None, WATCH, playback_log())
    }

    /// `open_remote`, with how the download is watched and where it says what happened.
    pub(crate) fn open_remote_watched(
        audio: EphemeralAudio,
        refresh: Option<Refresh>,
        watch: Watch,
        note: Note,
    ) -> Result<Self, String> {
        let duration_hint_ms = audio.duration_ms;
        let (sender, receiver) = sync_channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        std::thread::Builder::new()
            .name("venuta-stream-decoder".into())
            .spawn(move || decode_remote(audio, sender, worker_cancelled, refresh, watch, note))
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
                decode_remote(
                    audio,
                    sender,
                    worker_cancelled,
                    Some(refresh),
                    WATCH,
                    playback_log(),
                );
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
            time_base: None,
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
            starved: false,
            loudness_db: None,
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
        let rate_of_track = u64::from(params.sample_rate.unwrap_or(44_100).max(1));
        let time_base = track
            .time_base
            .map(|tb| (u64::from(tb.numer.get()), u64::from(tb.denom.get())))
            // One tick per frame needs no converting.
            .filter(|&(numer, denom)| !(numer == 1 && denom == rate_of_track));
        let frames = track.num_frames.or_else(|| {
            let (numer, denom) = time_base?;
            track
                .duration
                .map(|d| ticks_to_frames(d.get(), numer, denom, rate_of_track))
        });
        let decoder = codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("Venuta can't decode this file ({e})."))?;
        let mut codec = if params.codec == symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS
        {
            "Opus".to_owned()
        } else {
            codec_name(extension)
        };
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
            time_base,
            duration_hint_ms,
            remote: None,
            download_body: None,
            // Local files are ready immediately; progressive sets this after first samples.
            primed: true,
            starved: false,
            loudness_db: None,
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
        let state = if self.remote.is_some() {
            self.next_remote(out)
        } else {
            match self.next_local(out) {
                Ok(LocalDecode::Samples) => {
                    self.primed = true;
                    Ok(DecodeState::Samples)
                }
                Ok(LocalDecode::End) => Ok(DecodeState::End),
                Ok(LocalDecode::Pending) => Ok(DecodeState::Pending),
                Err(e) => Err(e),
            }
        };
        self.starved = matches!(state, Ok(DecodeState::Pending));
        // A stream that stopped short: once what arrived has played, say why (not the end of
        // the song, and not "damaged" for bytes that never came).
        if !matches!(state, Ok(DecodeState::Samples | DecodeState::Pending))
            && let Some(why) = self.download_body.as_ref().and_then(|b| b.failure())
        {
            return Err(why);
        }
        state
    }

    /// An opened stream waiting for more of its bytes: all that has arrived is decoded.
    pub fn is_waiting(&self) -> bool {
        self.starved
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
                loudness_db,
            }) => {
                self.loudness_db = loudness_db;
                self.finish_open(
                    PendingOpen {
                        body,
                        mime_type,
                        url_hint,
                        duration_hint_ms,
                        size_hint,
                    },
                    out,
                )
            }
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
                    return Err(pending.body.failure().unwrap_or(e));
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
        let to_frames = |ts: u64| match self.time_base {
            Some((numer, denom)) => ticks_to_frames(ts, numer, denom, rate),
            None => ts,
        };
        let actual = u64::try_from(seeked.actual_ts.get())
            .map(to_frames)
            .unwrap_or(0);
        let required = u64::try_from(seeked.required_ts.get())
            .map(to_frames)
            .unwrap_or(target);
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
pub(crate) type Refresh = Box<dyn FnMut() -> Option<EphemeralAudio> + Send>;

/// Where a download says what happened to it (the playback log). Never an address.
pub(crate) type Note = Arc<dyn Fn(&str) + Send + Sync>;

fn playback_log() -> Note {
    Arc::new(media_access::note_timing)
}

fn decode_remote(
    audio: EphemeralAudio,
    sender: SyncSender<RemoteEvent>,
    cancelled: Arc<AtomicBool>,
    refresh: Option<Refresh>,
    watch: Watch,
    note: Note,
) {
    let body = GrowBody::new(cancelled.clone());
    let result: Result<(), String> = (|| {
        if cancelled.load(Ordering::Acquire) {
            body.cancel();
            return Ok(());
        }
        fill_grow_body(audio, &body, &sender, refresh, &watch, &*note)?;
        Ok(())
    })();
    if let Err(error) = result
        && !cancelled.load(Ordering::Acquire)
    {
        // What did arrive still plays; then the song says why it stopped (an opened song no
        // longer listens for the event, so the body carries it too).
        body.fail(&error);
        let _ = sender.send(RemoteEvent::Error(error));
    }
}

/// Bytes asked for per request. Media hosts serve a bounded range at full speed, where an
/// open-ended one may be paced to the song's own rate.
const RANGE_BYTES: u64 = 2 * 1_048_576;
/// Fresh addresses asked for in one song before giving up on it.
const MAX_REFRESHES: u32 = 2;
/// What a song says when its stream stopped partway and could not be picked up again.
pub(crate) const STREAM_LOST: &str = "The stream stopped partway and couldn't be picked up again.";

/// How a ranged download notices a request that has stopped, or slowed to a trickle, and gives
/// it up for a new one from the last byte that arrived (instead of waiting out a timeout).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Watch {
    /// Nothing at all (no answer, no bytes) for this long: given up.
    pub stall: Duration,
    /// Speed is judged over spans this long.
    pub window: Duration,
    /// Slower than this share of the song's own rate over a span, while less than `low_water`
    /// of it is downloaded ahead of playback: a trickle that will run dry, given up. Anything at
    /// the song's rate or faster is never given up, however slow.
    pub trickle_share: f64,
    pub low_water: Duration,
    /// A request that brought this much before it failed was working: the next try starts
    /// again with a quick retry on the same address.
    pub progress_bytes: u64,
    /// The pause before a quick retry on the same address.
    pub retry_pause: Duration,
}

const WATCH: Watch = Watch {
    stall: Duration::from_secs(6),
    window: Duration::from_secs(8),
    trickle_share: 0.5,
    low_water: Duration::from_secs(10),
    progress_bytes: 256 * 1024,
    retry_pause: Duration::from_millis(250),
};

/// The longest a range request may take to answer, or to bring its whole range, before ureq
/// itself gives it up: only a backstop, so a request the watch gave up on ends some time. The
/// watch notices a stall long before; a slow but steady range that reaches it carries on from
/// its last byte. (The agent's own 30 s for the whole call would cut a steady range short.)
const RANGE_ANSWER_LIMIT: Duration = Duration::from_secs(30);
const RANGE_BODY_LIMIT: Duration = Duration::from_secs(300);
/// How often the download looks at a request in flight (and at being cancelled).
const WATCH_TICK: Duration = Duration::from_millis(100);

/// What a range request's reader hands back, in order.
enum Fetched {
    /// The host answered: its status, content type, and the whole length its range says.
    Answer {
        status: u16,
        mime: Option<String>,
        total: Option<u64>,
    },
    Bytes(Vec<u8>),
    /// The whole range is in.
    Done,
    /// No answer, or the connection broke while reading: why, in words without the address.
    Failed(String),
}

/// One range request, read on a thread of its own so the download can watch it, and give it up
/// without waiting for it. Stops as soon as nobody listens.
fn fetch_range(agent: ureq::Agent, audio: &EphemeralAudio, range: String) -> Receiver<Fetched> {
    let (tx, rx) = sync_channel(16);
    let url = audio.url.clone();
    let headers = audio.headers.clone();
    let spawned = std::thread::Builder::new()
        .name("venuta-stream-range".into())
        .spawn(move || {
            let mut request = agent
                .get(&url)
                .config()
                .timeout_global(None)
                .timeout_recv_response(Some(RANGE_ANSWER_LIMIT))
                .timeout_recv_body(Some(RANGE_BODY_LIMIT))
                .build();
            for header in &headers {
                request = request.header(&header.name, &header.value);
            }
            let response = match request.header("Range", range).call() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Fetched::Failed(failed_why(&e)));
                    return;
                }
            };
            let status = response.status().as_u16();
            let header = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let mime = header("content-type")
                .map(|value| value.split(';').next().unwrap_or(&value).trim().to_owned());
            let total = header("content-range")
                .and_then(|value| value.rsplit_once('/').map(|(_, t)| t.to_owned()))
                .and_then(|total| total.parse::<u64>().ok());
            if tx
                .send(Fetched::Answer {
                    status,
                    mime,
                    total,
                })
                .is_err()
                || !(200..300).contains(&status)
            {
                return;
            }
            let mut reader = response.into_body().into_reader();
            let mut buf = vec![0_u8; 64 * 1024];
            loop {
                let sent = match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Fetched::Done);
                        return;
                    }
                    Ok(n) => tx.send(Fetched::Bytes(buf[..n].to_vec())),
                    Err(e) => {
                        let _ = tx.send(Fetched::Failed(format!("{:?}", e.kind())));
                        return;
                    }
                };
                if sent.is_err() {
                    return;
                }
            }
        });
    // Without a reader thread the receiver has no sender, and the request counts as broken.
    drop(spawned);
    rx
}

/// Why a request failed, in a word or two: never ureq's own text, which can carry the address.
fn failed_why(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(t) => format!("timed out ({t:?})"),
        ureq::Error::Io(io) => format!("{:?}", io.kind()),
        ureq::Error::ConnectionFailed => "could not connect".into(),
        ureq::Error::HostNotFound => "host not found".into(),
        _ => "request failed".into(),
    }
}

/// How one range request ended.
enum Ended {
    /// Its whole range came.
    Done,
    /// The host says the song ends before this range (asked past the end).
    PastEnd,
    Refused(u16),
    /// No answer came (could not connect, or the request failed before any reply).
    NoAnswer(String),
    /// The connection broke partway through the range.
    Broke(String),
    /// Nothing for `Watch::stall`.
    Stalled,
    /// Bytes per second, far under the song's rate while playback is close to running dry.
    Trickle(f64),
}

fn kb(bytes: f64) -> String {
    format!("{:.1} KB", bytes / 1024.0)
}

fn kb_per_s(bytes: u64, took: Duration) -> String {
    format!(
        "{:.1} KB/s",
        bytes as f64 / 1024.0 / took.as_secs_f64().max(0.001)
    )
}

/// Download into `body`, signalling [`RemoteEvent::OpenReady`] once enough bytes (or the whole
/// short body) are present so decode can start while the rest arrives.
///
/// Bytes are added as they arrive, not a whole request at a time, so the song opens as soon as
/// its first part is in. Every song shares one connection pool (see `media_access::media_agent`),
/// so the connection the resolver's check opened is the one this download uses.
///
/// When the host stops serving partway (the address expired, or was only good for its
/// opening), a fresh address is asked for and the download carries on from the same byte.
/// A request that stalls, or trickles so slowly that playback will run dry, is given up within
/// seconds (see [`Watch`]) and asked again from its last byte, through the same steps: once
/// more on the same address, then on a fresh one. Each step goes to `note`, with the bytes and
/// speeds that led to it. When none is left, the download fails with words saying so.
fn fill_grow_body(
    mut audio: EphemeralAudio,
    body: &Arc<GrowBody>,
    sender: &SyncSender<RemoteEvent>,
    mut refresh: Option<Refresh>,
    watch: &Watch,
    note: &dyn Fn(&str),
) -> Result<(), String> {
    let agent = media_access::media_agent();
    let started = std::time::Instant::now();
    let url_hint = audio.url.clone();
    let duration_hint_ms = audio.duration_ms;
    let loudness_db = audio.loudness_db;

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
        note(&format!(
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
            loudness_db,
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
    // The song's own rate, bytes a second, when its length and duration are both known: a
    // request slower than it is falling behind playback.
    let song_rate = |total: Option<u64>| {
        total
            .zip(duration_hint_ms.filter(|d| *d > 0))
            .map(|(t, d)| t as f64 * 1000.0 / d as f64)
    };
    // After a failure, what was tried: the next request says how it went.
    let mut trying: Option<String> = None;

    // A request failed or was given up partway. Try once more on the same address, then on a
    // fresh one; `false` when neither is left.
    let mut recover = |audio: &mut EphemeralAudio,
                       retried_here: &mut bool,
                       at: u64,
                       total: Option<u64>|
     -> bool {
        let gave_up = |refreshes: u32| {
            note(&format!(
                "online stream gave up at byte {at} of {} after {refreshes} fresh addresses",
                total.map_or_else(|| "?".to_owned(), |t| t.to_string()),
            ));
            false
        };
        if !*retried_here {
            *retried_here = true;
            std::thread::sleep(watch.retry_pause);
            note(&format!(
                "online stream asking again on the same address from byte {at}"
            ));
            return true;
        }
        if refreshes >= MAX_REFRESHES {
            return gave_up(refreshes);
        }
        let Some(ask) = refresh.as_mut() else {
            return gave_up(refreshes);
        };
        let asked = std::time::Instant::now();
        let Some(fresh) = ask() else {
            note(&format!(
                "online song asked for a fresh address partway ({} of {MAX_REFRESHES}) at byte {at}: none came back after {} ms",
                refreshes + 1,
                media_access::ms_since(asked)
            ));
            return gave_up(refreshes);
        };
        refreshes += 1;
        *retried_here = false;
        note(&format!(
            "online song asked for a fresh address partway ({refreshes} of {MAX_REFRESHES}) at byte {at}: one came in {} ms",
            media_access::ms_since(asked)
        ));
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
        let from = start;
        let replies = fetch_range(agent.clone(), &audio, format!("bytes={start}-{end}"));
        let began = std::time::Instant::now();
        // When the request last showed life (its answer, or bytes).
        let mut heard = began;
        let mut answered = false;
        let mut got = 0u64;
        let mut total_from_header = None;
        // The span speed is judged over: since when, and what had come by then.
        let (mut span_at, mut span_got) = (began, 0u64);
        let ended = loop {
            if sync_cancel() {
                return Err("Remote stream cancelled.".into());
            }
            match replies.recv_timeout(WATCH_TICK) {
                Ok(Fetched::Answer {
                    status,
                    mime,
                    total,
                }) => {
                    answered = true;
                    heard = std::time::Instant::now();
                    if status == 416 && body.len() > 0 {
                        break Ended::PastEnd;
                    }
                    if !(200..300).contains(&status) {
                        break Ended::Refused(status);
                    }
                    if mime_type.is_none() {
                        mime_type = mime;
                    }
                    if let Some(t) = total {
                        total_from_header = Some(t);
                        known_total = Some(t);
                        body.set_total(t);
                    }
                }
                Ok(Fetched::Bytes(bytes)) => {
                    heard = std::time::Instant::now();
                    body.append(&bytes);
                    got += bytes.len() as u64;
                    start += bytes.len() as u64;
                    signal_open(&mime_type, known_total);
                    if body.len() > 128 * 1024 * 1024 {
                        return Err("Remote stream is larger than Venuta will buffer.".into());
                    }
                    if got >= watch.progress_bytes
                        && let Some(what) = trying.take()
                    {
                        note(&format!(
                            "online stream picked up again (after: {what}): {} from byte {from} in {} ms ({})",
                            kb(got as f64),
                            media_access::ms_since(began),
                            kb_per_s(got, began.elapsed())
                        ));
                    }
                }
                Ok(Fetched::Done) => break Ended::Done,
                Ok(Fetched::Failed(why)) if answered => break Ended::Broke(why),
                Ok(Fetched::Failed(why)) => break Ended::NoAnswer(why),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break Ended::Broke("the reader stopped".into());
                }
            }
            let now = std::time::Instant::now();
            if now.duration_since(heard) >= watch.stall {
                break Ended::Stalled;
            }
            if now.duration_since(span_at) >= watch.window {
                let speed = (got - span_got) as f64 / now.duration_since(span_at).as_secs_f64();
                if let Some(rate) = song_rate(known_total)
                    && speed < rate * watch.trickle_share
                    && (body.ahead() as f64) < rate * watch.low_water.as_secs_f64()
                {
                    break Ended::Trickle(speed);
                }
                (span_at, span_got) = (now, got);
            }
        };
        // Dropping `replies` lets a reader still waiting on the network go, whenever it wakes.
        drop(replies);
        let took = began.elapsed();
        let what = match ended {
            Ended::Done => {
                if let Some(what) = trying.take() {
                    note(&format!(
                        "online stream picked up again (after: {what}): {} from byte {from} in {} ms ({})",
                        kb(got as f64),
                        took.as_millis(),
                        kb_per_s(got, took)
                    ));
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
                continue;
            }
            Ended::PastEnd => break,
            // Refused, or no answer at all, before anything came: the address is no good.
            Ended::Refused(status) if body.len() == 0 => {
                return Err(format!("The remote stream returned HTTP {status}."));
            }
            Ended::NoAnswer(_) if body.len() == 0 => {
                return Err("Could not retrieve the remote stream.".to_owned());
            }
            Ended::Refused(status) => format!("refused with HTTP {status}"),
            Ended::NoAnswer(why) => format!("no answer ({why})"),
            Ended::Broke(why) => format!("connection broke ({why})"),
            Ended::Stalled => format!("stalled, nothing for {:.1} s", watch.stall.as_secs_f64()),
            Ended::Trickle(speed) => format!(
                "trickling at {}/s, the song needs {}/s, {:.1} s ahead of playback",
                kb(speed),
                kb(song_rate(known_total).unwrap_or(0.0)),
                song_rate(known_total)
                    .map(|rate| body.ahead() as f64 / rate)
                    .unwrap_or(0.0)
            ),
        };
        note(&format!(
            "online stream {what} at byte {start} of {}: this request brought {} from byte {from} in {} ms ({})",
            known_total.map_or_else(|| "?".to_owned(), |t| t.to_string()),
            kb(got as f64),
            took.as_millis(),
            kb_per_s(got, took)
        ));
        if got >= watch.progress_bytes {
            // It was working until then: start the steps over, with a quick retry.
            retried_here = false;
        }
        if recover(&mut audio, &mut retried_here, start, known_total) {
            trying = Some(what);
            continue;
        }
        if body.len() == 0 {
            return Err("Could not read the remote stream.".into());
        }
        return Err(STREAM_LOST.into());
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
        "audio/webm" => Some("webm"),
        "audio/opus" => Some("opus"),
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

/// A media host on this computer, for checks of the download: never the network.
#[cfg(test)]
pub(crate) mod mock_host {
    use super::{Note, Watch};
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// What the host does with one ranged request.
    #[derive(Clone, Copy)]
    pub enum Serve {
        /// The whole range, at once.
        Full,
        /// The first `n` bytes of the range, then nothing (the connection stays open).
        ThenStall(usize),
        /// The first `n` bytes of the range, then the connection closes.
        ThenDrop(usize),
        /// Refused with this status.
        Refuse(u16),
        /// The first `first` bytes at once, then the rest in pieces of `piece` bytes, one
        /// every `every_ms`.
        Paced {
            first: usize,
            piece: usize,
            every_ms: u64,
        },
    }

    pub struct Host {
        pub url: String,
        /// Where each ranged request started.
        asked: Arc<Mutex<Vec<u64>>>,
    }

    impl Host {
        pub fn asked(&self) -> Vec<u64> {
            self.asked.lock().unwrap().clone()
        }
    }

    /// Serves `content` in ranges, each request as `plan(n, start)` says (`n` counts the ranged
    /// requests so far). A request without a range is refused, as the media host refuses a bare
    /// GET, so the download takes the ranged path.
    pub fn host(
        content: Arc<Vec<u8>>,
        plan: impl Fn(usize, u64) -> Serve + Send + Sync + 'static,
    ) -> Host {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let plan = Arc::new(plan);
        let seen = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (content, plan, seen) = (content.clone(), plan.clone(), seen.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut range = None;
                    let mut line = String::new();
                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        let l = line.trim().to_ascii_lowercase();
                        if l.is_empty() {
                            break;
                        }
                        if let Some(r) = l.strip_prefix("range: bytes=")
                            && let Some((a, b)) = r.split_once('-')
                        {
                            range = a.parse::<u64>().ok().zip(b.parse::<u64>().ok());
                        }
                        line.clear();
                    }
                    let mut s = stream;
                    let Some((start, end)) = range else {
                        let _ = s.write_all(
                            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        return;
                    };
                    let n = {
                        let mut seen = seen.lock().unwrap();
                        seen.push(start);
                        seen.len() - 1
                    };
                    let total = content.len() as u64;
                    let end = end.min(total - 1);
                    let part = &content[start as usize..=end as usize];
                    let serve = plan(n, start);
                    if let Serve::Refuse(code) = serve {
                        let _ = s.write_all(
                            format!(
                                "HTTP/1.1 {code} No\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        );
                        return;
                    }
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/webm\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        part.len()
                    );
                    if s.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    match serve {
                        Serve::Full | Serve::Refuse(_) => {
                            let _ = s.write_all(part);
                        }
                        Serve::ThenStall(k) => {
                            let _ = s.write_all(&part[..k.min(part.len())]);
                            let _ = s.flush();
                            std::thread::sleep(Duration::from_secs(120));
                        }
                        Serve::ThenDrop(k) => {
                            let _ = s.write_all(&part[..k.min(part.len())]);
                            let _ = s.flush();
                            let _ = s.shutdown(std::net::Shutdown::Both);
                        }
                        Serve::Paced {
                            first,
                            piece,
                            every_ms,
                        } => {
                            let (burst, rest) = part.split_at(first.min(part.len()));
                            if s.write_all(burst).is_err() {
                                return;
                            }
                            for chunk in rest.chunks(piece) {
                                if s.write_all(chunk).is_err() {
                                    return;
                                }
                                let _ = s.flush();
                                std::thread::sleep(Duration::from_millis(every_ms));
                            }
                        }
                    }
                });
            }
        });
        Host {
            url: format!("http://{addr}/song.webm"),
            asked,
        }
    }

    /// The watch on a much shorter clock, so a check takes moments.
    pub fn quick_watch(stall_ms: u64) -> Watch {
        Watch {
            stall: Duration::from_millis(stall_ms),
            window: Duration::from_millis(600),
            trickle_share: 0.5,
            low_water: Duration::from_secs(2),
            progress_bytes: 64 * 1024,
            retry_pause: Duration::from_millis(10),
        }
    }

    /// A playback log to read back.
    pub fn log() -> (Note, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let keep = lines.clone();
        (
            Arc::new(move |line: &str| keep.lock().unwrap().push(line.to_owned())),
            lines,
        )
    }

    /// No line of the log names where the stream is.
    pub fn assert_private(lines: &[String]) {
        for line in lines {
            assert!(
                !line.contains("127.0.0.1")
                    && !line.contains("http")
                    && !line.contains("song.webm"),
                "the log names the address: {line}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock_host::{Serve, assert_private, host, log, quick_watch};
    use super::*;
    use std::time::Instant;

    /// Bytes that differ from place to place, so a piece fetched from the wrong byte shows.
    fn content(len: usize) -> Arc<Vec<u8>> {
        Arc::new((0..len).map(|i| (i % 251) as u8).collect())
    }

    fn audio(url: &str, len: usize, duration_ms: u64) -> EphemeralAudio {
        EphemeralAudio {
            track_id: "t".into(),
            provider: "test".into(),
            url: url.into(),
            mime_type: Some("audio/webm".into()),
            content_length: Some(len as u64),
            duration_ms: Some(duration_ms),
            headers: Vec::new(),
            via: "test",
            loudness_db: None,
        }
    }

    fn body() -> Arc<GrowBody> {
        GrowBody::new(Arc::new(AtomicBool::new(false)))
    }

    /// Plays as the bytes come: the decoder always at the edge of what has arrived, so nothing
    /// is ever ahead of it (what decides whether a slow stream will run dry).
    fn listen_at_the_edge(body: &Arc<GrowBody>) {
        let body = Arc::clone(body);
        std::thread::spawn(move || {
            while !body.finished.load(Ordering::Acquire) {
                body.read_at.store(body.len() as u64, Ordering::Release);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }

    fn has(lines: &[String], words: &str) -> bool {
        lines.iter().any(|l| l.contains(words))
    }

    #[test]
    fn a_stalled_range_is_asked_again_from_the_last_byte_within_seconds() {
        let song = content(600 * 1024);
        let host = host(song.clone(), |n, _| {
            if n == 0 {
                Serve::ThenStall(128 * 1024)
            } else {
                Serve::Full
            }
        });
        let body = body();
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        let started = Instant::now();
        fill_grow_body(
            audio(&host.url, song.len(), 36_000),
            &body,
            &tx,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "noticed the stall quickly, took {:?}",
            started.elapsed()
        );
        assert_eq!(
            host.asked(),
            vec![0, 128 * 1024],
            "asked again from the last byte"
        );
        assert!(
            *body.data.lock().unwrap() == *song,
            "the song arrived whole"
        );
        let lines = lines.lock().unwrap();
        assert!(
            has(
                &lines,
                "online stream stalled, nothing for 0.4 s at byte 131072 of 614400"
            ),
            "{lines:?}"
        );
        assert!(has(
            &lines,
            "asking again on the same address from byte 131072"
        ));
        assert!(has(&lines, "picked up again (after: stalled"), "{lines:?}");
        assert_private(&lines);
    }

    #[test]
    fn the_production_watch_gives_a_stall_up_in_seconds_not_the_agent_timeout() {
        assert!(WATCH.stall <= Duration::from_secs(8));
        assert!(WATCH.stall < RANGE_ANSWER_LIMIT && WATCH.window < RANGE_BODY_LIMIT);
    }

    #[test]
    fn a_dropped_connection_carries_on_from_the_last_byte() {
        let song = content(500 * 1024);
        let host = host(song.clone(), |n, _| {
            if n == 0 {
                Serve::ThenDrop(100 * 1024)
            } else {
                Serve::Full
            }
        });
        let body = body();
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 30_000),
            &body,
            &tx,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(host.asked(), vec![0, 100 * 1024]);
        assert!(*body.data.lock().unwrap() == *song);
        let lines = lines.lock().unwrap();
        assert!(has(&lines, "connection broke"), "{lines:?}");
        assert_private(&lines);
    }

    #[test]
    fn a_refusal_partway_is_taken_up_on_a_fresh_address() {
        let song = content(500 * 1024);
        // The first address breaks partway, then refuses; the fresh one serves.
        let old = host(song.clone(), |n, _| {
            if n == 0 {
                Serve::ThenDrop(200 * 1024)
            } else {
                Serve::Refuse(403)
            }
        });
        let fresh = Arc::new(host(song.clone(), |_, _| Serve::Full));
        let fresh_url = fresh.url.clone();
        let len = song.len();
        let refresh: Refresh = Box::new(move || Some(audio(&fresh_url, len, 30_000)));
        let body = body();
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        fill_grow_body(
            audio(&old.url, song.len(), 30_000),
            &body,
            &tx,
            Some(refresh),
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(old.asked(), vec![0, 200 * 1024], "a quick retry first");
        assert_eq!(
            fresh.asked(),
            vec![200 * 1024],
            "then a fresh address, same byte"
        );
        assert!(*body.data.lock().unwrap() == *song);
        let lines = lines.lock().unwrap();
        assert!(
            has(&lines, "refused with HTTP 403 at byte 204800"),
            "{lines:?}"
        );
        assert!(has(
            &lines,
            "fresh address partway (1 of 2) at byte 204800: one came"
        ));
        assert_private(&lines);
    }

    #[test]
    fn a_stream_that_never_comes_back_says_so_instead_of_ending_quietly() {
        let song = content(500 * 1024);
        let host = Arc::new(host(song.clone(), |n, _| {
            Serve::ThenStall(if n == 0 { 100 * 1024 } else { 0 })
        }));
        let url = host.url.clone();
        let len = song.len();
        let asked_fresh = Arc::new(AtomicU64::new(0));
        let counted = asked_fresh.clone();
        let refresh: Refresh = Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Some(audio(&url, len, 30_000))
        });
        let body = body();
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        let started = Instant::now();
        let result = fill_grow_body(
            audio(&host.url, song.len(), 30_000),
            &body,
            &tx,
            Some(refresh),
            &quick_watch(300),
            &*note,
        );
        assert_eq!(result, Err(STREAM_LOST.to_owned()));
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(asked_fresh.load(Ordering::SeqCst), u64::from(MAX_REFRESHES));
        // Retry, fresh, retry, fresh, retry: every one from the byte the first stopped at.
        assert_eq!(
            host.asked(),
            vec![0, 102_400, 102_400, 102_400, 102_400, 102_400]
        );
        let lines = lines.lock().unwrap();
        assert!(has(
            &lines,
            "gave up at byte 102400 of 512000 after 2 fresh addresses"
        ));
        assert_private(&lines);
    }

    #[test]
    fn a_failed_download_plays_what_arrived_then_says_why() {
        let body = body();
        body.append(&[1, 2, 3]);
        body.fail(STREAM_LOST);
        let mut reader = GrowReader {
            body: Arc::clone(&body),
            pos: 0,
        };
        let mut buf = [0_u8; 8];
        assert_eq!(
            reader.read(&mut buf).unwrap(),
            3,
            "what arrived is still read"
        );
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert_eq!(body.failure().as_deref(), Some(STREAM_LOST));
    }

    #[test]
    fn a_slow_but_steady_stream_is_never_given_up() {
        // The song needs 100 KB/s; the host sends 150 KB/s in pieces, and playback eats every
        // byte as it comes, so nothing is ever ahead.
        let song = content(300 * 1024);
        let host = host(song.clone(), |_, _| Serve::Paced {
            first: 0,
            piece: 15 * 1024,
            every_ms: 100,
        });
        let body = body();
        listen_at_the_edge(&body);
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 3_000),
            &body,
            &tx,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(host.asked(), vec![0], "one request, never given up");
        assert!(*body.data.lock().unwrap() == *song);
        let lines = lines.lock().unwrap();
        assert!(
            !has(&lines, "trickling") && !has(&lines, "stalled"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_trickle_that_will_run_dry_is_given_up_and_asked_again() {
        // 20 KB/s for a song that needs 100 KB/s, with nothing ahead of playback.
        let song = content(300 * 1024);
        let host = host(song.clone(), |n, _| {
            if n == 0 {
                Serve::Paced {
                    first: 0,
                    piece: 2 * 1024,
                    every_ms: 100,
                }
            } else {
                Serve::Full
            }
        });
        let body = body();
        listen_at_the_edge(&body);
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 3_000),
            &body,
            &tx,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        let asked = host.asked();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert!(asked[1] > 0, "carried on from where the trickle got to");
        assert!(*body.data.lock().unwrap() == *song);
        let lines = lines.lock().unwrap();
        assert!(has(&lines, "trickling at"), "{lines:?}");
        assert!(has(&lines, "the song needs 100.0 KB/s"), "{lines:?}");
        assert_private(&lines);
    }

    #[test]
    fn a_slow_stream_well_ahead_of_playback_is_left_alone() {
        // As slow as a trickle (2.5 KB/s for a song needing 10 KB/s), but six seconds came at
        // once and playback is behind them: it can't run dry yet.
        let song = content(70 * 1024);
        let host = host(song.clone(), |_, _| Serve::Paced {
            first: 60 * 1024,
            piece: 256,
            every_ms: 100,
        });
        let body = body();
        let (tx, _rx) = sync_channel(8);
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 7_000),
            &body,
            &tx,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(host.asked(), vec![0]);
        assert!(*body.data.lock().unwrap() == *song);
        assert!(!has(&lines.lock().unwrap(), "trickling"));
    }
}
