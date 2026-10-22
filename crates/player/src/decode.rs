//! Opening and decoding one song: any format symphonia reads, as interleaved samples.
//!
//! Samples come out as left-justified `i32` (a 16-bit sample `s` is `s << 16`), the form
//! symphonia uses, so a lossless source's exact values survive: shifting back recovers them.
//!
//! Places are content frames as a straight decode from the start counts them. A seek lands on
//! its frame exactly, and a lossy codec is decoded from a little before it (its pre-roll), so
//! what comes out is what a straight decode gives from there (design 4.6, the WP2 notes in 9.2).
//!
//! An online song downloads into a [`Download`] that every source of the song shares
//! ([`Streams`]). A source either never waits for bytes ([`ByteWait::Yield`]: the old engine,
//! which decodes on the sound's own thread) or waits inside its reader ([`ByteWait::Wait`]: a
//! voice's decode thread), so a packet is never cut where the download stopped.

use media_access::EphemeralAudio;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::mpsc::{Receiver, sync_channel};
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_MP1, CODEC_ID_MP2, CODEC_ID_MP3, CODEC_ID_OPUS, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{
    MediaSource as SymphoniaMediaSource, MediaSourceStream, MediaSourceStreamOptions,
};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::packet::Packet;
use symphonia::core::units::Timestamp;
use together::time::Sleeper;

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
    /// The codec as symphonia names it (what decides the pre-roll).
    codec_id: Option<AudioCodecId>,
    pub format: SourceFormat,
    /// Length in frames, when the file says.
    pub frames: Option<u64>,
    pub info: FileInfo,
    /// The track's clock (seconds per timestamp tick), when it is not one tick per frame: a WebM
    /// stream counts in milliseconds.
    time_base: Option<(u64, u64)>,
    /// The track's first timestamp and, when known, its last: a seek stays between them.
    ts_range: (i64, Option<i64>),
    duration_hint_ms: Option<u64>,
    remote: Option<RemoteState>,
    /// The song's download, for an online song (shared with its other sources).
    download: Option<Arc<Download>>,
    /// Where this source's reader is in the download's bytes.
    read_pos: Option<Arc<AtomicU64>>,
    /// Whether the reader waits for bytes or says it would have to.
    wait: ByteWait,
    /// True once at least one decoded packet produced samples (UI may leave Opening…).
    primed: bool,
    /// The last decode found nothing more downloaded yet (the stream is behind playback).
    starved: bool,
    /// An online song's loudness as the catalog says it (see `EphemeralAudio::loudness_db`).
    pub loudness_db: Option<f64>,
    /// The content frame after the last frame `next` gave (where the next one lies).
    at: u64,
    /// A container timestamp in frames, less this, is the place as the straight decode counts
    /// it. Learned from the first packet that gives sound.
    base: Option<i64>,
    /// After a seek: the next packet with sound says where it lies.
    anchor: bool,
    /// Frames before this place are dropped (a seek's pre-roll).
    drop_until: u64,
    /// A packet read ahead (to learn the base), decoded next.
    held: Option<Packet>,
    /// Frames the first packet with sound gave (where the straight decode's packet grid lies).
    first_sound: i64,
    /// How to read the song again from its first byte.
    reopen: Option<Reopen>,
}

/// Where a song's bytes come from, to read it again from the first (a seek near the start
/// decodes from there: exactly the straight decode).
#[derive(Clone)]
enum Origin {
    File(std::path::PathBuf),
    Body(Arc<GrowBody>),
}

#[derive(Clone)]
struct Reopen {
    origin: Origin,
    extension: Option<String>,
    mime_type: Option<String>,
    size_bytes: u64,
    duration_hint_ms: Option<u64>,
}

/// Why no packet came.
enum LocalError {
    Pending,
    Failed(String),
}

/// A seek asked of an online song before it has opened.
#[derive(Debug, Clone, Copy)]
enum PendingSeek {
    Ms(u64),
    Frame(u64),
}

struct RemoteState {
    download: Arc<Download>,
    /// It failed (said why once); nothing more comes.
    finished: bool,
    /// Seek requested while the body was still downloading; applied on open.
    pending_seek: Option<PendingSeek>,
    /// Open attempt failed for want of bytes; retry when the body grows past this length.
    retry_after: Option<u64>,
}

/// What the media host said about a song, kept for every source that opens on its download.
#[derive(Debug, Clone, Default)]
pub struct OpenInfo {
    pub mime_type: Option<String>,
    /// The address's path (for its extension); never logged.
    pub url_hint: String,
    pub duration_hint_ms: Option<u64>,
    pub size_hint: Option<u64>,
    pub loudness_db: Option<f64>,
}

/// Bytes per piece of a download: appended to, never moved.
pub const CHUNK: usize = 64 * 1024;

/// Shared download buffer: decoders read while the download appends.
struct GrowBody {
    /// Pieces of `CHUNK` bytes, every one full but the last.
    chunks: Mutex<Vec<Box<[u8]>>>,
    /// Bytes in, set under the `chunks` lock after the bytes are.
    len: AtomicU64,
    more: Condvar,
    finished: AtomicBool,
    /// Every holder let go: the download stops.
    cancelled: AtomicBool,
    /// Known Content-Length when the host gave one; 0 if unknown.
    total: AtomicU64,
    /// Where each reader is: what is downloaded past the furthest is what playback has in hand.
    readers: Mutex<Vec<Weak<AtomicU64>>>,
    /// Why the download stopped short, when it did: said once what arrived has played.
    failed: Mutex<Option<String>>,
    /// Enough is in to open the song, and what the host said about it.
    open: Mutex<Option<OpenInfo>>,
    /// The thread filling it has finished, whatever came of it.
    worker_done: AtomicBool,
}

impl GrowBody {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            chunks: Mutex::new(Vec::new()),
            len: AtomicU64::new(0),
            more: Condvar::new(),
            finished: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            total: AtomicU64::new(0),
            readers: Mutex::new(Vec::new()),
            failed: Mutex::new(None),
            open: Mutex::new(None),
            worker_done: AtomicBool::new(false),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Box<[u8]>>> {
        self.chunks.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn len(&self) -> usize {
        self.len.load(Ordering::Acquire) as usize
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Wake every reader waiting (under the lock, so none misses it).
    fn wake(&self) {
        let _g = self.lock();
        self.more.notify_all();
    }

    fn append(&self, mut bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut chunks = self.lock();
        let mut len = self.len.load(Ordering::Relaxed) as usize;
        while !bytes.is_empty() {
            let off = len % CHUNK;
            if off == 0 && len / CHUNK == chunks.len() {
                chunks.push(vec![0u8; CHUNK].into_boxed_slice());
            }
            let n = (CHUNK - off).min(bytes.len());
            chunks[len / CHUNK][off..off + n].copy_from_slice(&bytes[..n]);
            len += n;
            bytes = &bytes[n..];
        }
        self.len.store(len as u64, Ordering::Release);
        self.more.notify_all();
    }

    /// Copy from `pos` into `buf` (at most to the end of one chunk); what was copied.
    fn copy_at(&self, pos: u64, buf: &mut [u8]) -> usize {
        let chunks = self.lock();
        let len = self.len.load(Ordering::Acquire);
        if pos >= len {
            return 0;
        }
        let (i, off) = ((pos as usize) / CHUNK, (pos as usize) % CHUNK);
        let n = ((len - pos) as usize).min(CHUNK - off).min(buf.len());
        buf[..n].copy_from_slice(&chunks[i][off..off + n]);
        n
    }

    /// Every byte in, as one piece (for checks).
    #[cfg(test)]
    fn to_vec(&self) -> Vec<u8> {
        let chunks = self.lock();
        let len = self.len();
        let mut v: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        v.truncate(len);
        v
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::Release);
        self.wake();
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.finished.store(true, Ordering::Release);
        self.wake();
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

    /// Enough is in to open: say so, once, with what the host said.
    fn set_open(&self, info: OpenInfo) {
        if let Ok(mut o) = self.open.lock() {
            o.get_or_insert(info);
        }
        self.wake();
    }

    fn open_info(&self) -> Option<OpenInfo> {
        self.open.lock().ok().and_then(|o| o.clone())
    }

    fn done_working(&self) {
        self.worker_done.store(true, Ordering::Release);
        self.wake();
    }

    fn worker_done(&self) -> bool {
        self.worker_done.load(Ordering::Acquire)
    }

    /// A new reader's place, counted in `ahead` while it lives.
    fn slot(&self) -> Arc<AtomicU64> {
        let slot = Arc::new(AtomicU64::new(0));
        if let Ok(mut r) = self.readers.lock() {
            r.retain(|w| w.strong_count() > 0);
            r.push(Arc::downgrade(&slot));
        }
        slot
    }

    /// Bytes downloaded past the furthest reader (all of them, with no reader).
    fn ahead(&self) -> u64 {
        let furthest = self
            .readers
            .lock()
            .map(|r| {
                r.iter()
                    .filter_map(Weak::upgrade)
                    .map(|s| s.load(Ordering::Acquire))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        (self.len() as u64).saturating_sub(furthest)
    }

    /// Wait (as `w` waits) until `ready` holds. Before waiting `w` is told (true), and after
    /// (false). Ends early, with an error, when `w` is stopped.
    fn wait_until(&self, w: &Waiter, ready: impl Fn(&GrowBody) -> bool) -> io::Result<()> {
        if ready(self) {
            return Ok(());
        }
        (w.hook)(true);
        let r = loop {
            if w.stopped() {
                break Err(io::Error::other(STOPPED));
            }
            if ready(self) {
                break Ok(());
            }
            match &w.sleeper {
                None => {
                    let g = self.lock();
                    if !ready(self) && !w.stopped() {
                        drop(
                            self.more
                                .wait_timeout(g, Duration::from_millis(20))
                                .unwrap_or_else(|p| p.into_inner()),
                        );
                    }
                }
                Some(s) => s.sleep_until(s.now_ns() + 1_000_000),
            }
        };
        (w.hook)(false);
        r
    }
}

/// What a read or wait says when its voice was told to stop.
const STOPPED: &str = "stopped";

/// How a decode thread of its own waits for bytes that have not come yet.
pub struct Waiter {
    /// `None`: real time (a condition variable). Some: a clock's sleeper, polled each 1 ms.
    sleeper: Option<Sleeper>,
    stop: Arc<AtomicBool>,
    /// Told `true` before each wait for bytes, `false` when they came.
    hook: Box<dyn Fn(bool) + Send + Sync>,
}

impl Waiter {
    /// Waits in real time, or on `sleeper` (made on the thread that starts the waiting one).
    /// `stop` ends any wait; `hook` hears each wait begin (true) and end (false).
    pub fn new(
        sleeper: Option<Sleeper>,
        stop: Arc<AtomicBool>,
        hook: impl Fn(bool) + Send + Sync + 'static,
    ) -> Arc<Waiter> {
        Arc::new(Waiter {
            sleeper,
            stop,
            hook: Box::new(hook),
        })
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// Sleep `ms` on this waiter's clock (the decode thread's own polls).
    pub fn sleep_ms(&self, ms: u64) {
        match &self.sleeper {
            None => std::thread::sleep(Duration::from_millis(ms)),
            Some(s) => s.sleep_until(s.now_ns() + ms * 1_000_000),
        }
    }
}

/// What a stream's reader does when the bytes it needs have not come.
#[derive(Clone, Default)]
pub enum ByteWait {
    /// Says so at once (`WouldBlock`), and decode reports `Pending`: for decoding on a thread
    /// that must never wait (the old engine). A packet can be cut where the bytes stopped.
    #[default]
    Yield,
    /// Waits, wherever the read falls: decode never reports `Pending`, and no packet is cut.
    Wait(Arc<Waiter>),
}

/// One song's bytes as they download (or are fed). Shared by every source that plays the song:
/// the download stops when the last holder lets go.
pub struct Download {
    body: Arc<GrowBody>,
}

impl Drop for Download {
    fn drop(&mut self) {
        self.body.cancel();
    }
}

/// How much of a song's bytes are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamBytes {
    /// Bytes downloaded.
    pub have: u64,
    /// The whole length, when the host said.
    pub total: Option<u64>,
    /// Bytes downloaded past the furthest reader.
    pub ahead: u64,
    /// Every byte is in (the download finished and did not fail).
    pub complete: bool,
    pub failed: bool,
}

impl Download {
    fn new(body: Arc<GrowBody>) -> Arc<Download> {
        Arc::new(Download { body })
    }

    /// A download whose bytes the caller appends (a test, or a room's media stream): it opens
    /// once enough are in for its type (or all).
    pub fn fed(info: OpenInfo) -> (Arc<Download>, Feeder) {
        let need = open_after_bytes(info.mime_type.as_deref(), &info.url_hint);
        Download::fed_opening_after(info, need)
    }

    /// `fed`, opening once `open_after` bytes are in (or all).
    pub fn fed_opening_after(info: OpenInfo, open_after: usize) -> (Arc<Download>, Feeder) {
        let body = GrowBody::new();
        if let Some(total) = info.size_hint {
            body.set_total(total);
        }
        (
            Download::new(body.clone()),
            Feeder {
                body,
                info,
                open_after,
                opened: false,
            },
        )
    }

    pub fn bytes(&self) -> StreamBytes {
        let b = &self.body;
        let total = b.total.load(Ordering::Acquire);
        let failed = b.failure().is_some();
        StreamBytes {
            have: b.len() as u64,
            total: (total > 0).then_some(total),
            ahead: b.ahead(),
            complete: b.is_finished() && !failed && !b.is_cancelled(),
            failed,
        }
    }
}

/// A look at an online source's bytes from another thread: how much is in, and where its
/// reader is.
#[derive(Clone)]
pub struct StreamWatch {
    download: Arc<Download>,
    read: Arc<AtomicU64>,
}

impl StreamWatch {
    pub fn bytes(&self) -> StreamBytes {
        self.download.bytes()
    }

    /// Where the source's reader is in the bytes.
    pub fn read(&self) -> u64 {
        self.read.load(Ordering::Acquire)
    }
}

/// The writing end of a fed [`Download`].
pub struct Feeder {
    body: Arc<GrowBody>,
    info: OpenInfo,
    open_after: usize,
    opened: bool,
}

impl Feeder {
    pub fn append(&mut self, bytes: &[u8]) {
        self.body.append(bytes);
        self.offer();
    }

    /// Every byte is in.
    pub fn finish(&mut self) {
        self.body.finish();
        self.offer();
        self.body.done_working();
    }

    /// No more will come: what arrived plays, then `why` is said.
    pub fn fail(&mut self, why: &str) {
        self.body.fail(why);
        self.body.done_working();
    }

    /// Nobody holds the download any more.
    pub fn is_cancelled(&self) -> bool {
        self.body.is_cancelled()
    }

    fn offer(&mut self) {
        if !self.opened && (self.body.len() >= self.open_after || self.body.is_finished()) {
            self.opened = true;
            self.body.set_open(self.info.clone());
        }
    }
}

impl Drop for Feeder {
    fn drop(&mut self) {
        // A feeder gone without finishing: nothing more comes.
        if !self.body.is_finished() {
            self.body.fail(STREAM_LOST);
        }
        self.body.done_working();
    }
}

/// Every song's live download by its media key, so a song is downloaded once however many
/// sources play it (a seek, a realign, a resume: a new voice on the same bytes).
#[derive(Default)]
pub struct Streams {
    live: Mutex<HashMap<String, Weak<Download>>>,
}

impl Streams {
    pub fn new() -> Streams {
        Streams::default()
    }

    /// The song's live download, if any: holding it keeps it going with no source open.
    pub fn hold(&self, key: &str) -> Option<Arc<Download>> {
        self.live.lock().ok()?.get(key).and_then(Weak::upgrade)
    }

    /// Make `download` the song's (a fed one, say).
    pub fn insert(&self, key: &str, download: &Arc<Download>) {
        if let Ok(mut live) = self.live.lock() {
            live.retain(|_, w| w.strong_count() > 0);
            live.insert(key.to_owned(), Arc::downgrade(download));
        }
    }

    fn get_or_start(
        &self,
        key: &str,
        start: impl FnOnce() -> Result<Arc<Download>, String>,
    ) -> Result<Arc<Download>, String> {
        let mut live = self.live.lock().map_err(|_| "Streams lock".to_owned())?;
        if let Some(d) = live.get(key).and_then(Weak::upgrade) {
            return Ok(d);
        }
        live.retain(|_, w| w.strong_count() > 0);
        let d = start()?;
        live.insert(key.to_owned(), Arc::downgrade(&d));
        Ok(d)
    }

    /// A source of the song `key`: on its live download, or a new download of `audio`.
    pub fn open_remote(&self, key: &str, audio: EphemeralAudio) -> Result<Source, String> {
        let duration = audio.duration_ms;
        let d = self.get_or_start(key, || start_download(audio, None, WATCH, playback_log()))?;
        Ok(Source::on_download(d, duration))
    }

    /// A source of the song `key`: on its live download, or one resolved through `access` (on
    /// a worker thread) and downloaded.
    pub fn open_remote_deferred(
        &self,
        key: &str,
        access: Arc<dyn media_access::MediaAccess>,
        track_id: impl Into<String>,
        duration_hint_ms: Option<u64>,
    ) -> Result<Source, String> {
        let track_id = track_id.into();
        let d = self.get_or_start(key, || start_deferred(access, track_id))?;
        Ok(Source::on_download(d, duration_hint_ms))
    }
}

/// Reads from [`GrowBody`] at its own place.
///
/// When the decoder outruns the download it either says so ([`io::ErrorKind::WouldBlock`], so
/// decode can yield [`DecodeState::Pending`] instead of freezing audio) or waits, as its
/// [`ByteWait`] says.
struct GrowReader {
    body: Arc<GrowBody>,
    pos: u64,
    slot: Arc<AtomicU64>,
    wait: ByteWait,
    /// While a waiting reader is probed it says it can't seek, so the container is read forward
    /// only and the song opens on its first bytes even where its index points at the end (a
    /// WebM's tags, an Ogg's last page). Seeks work once it has opened.
    probing: Arc<AtomicBool>,
}

impl GrowReader {
    fn new(body: Arc<GrowBody>, wait: ByteWait) -> GrowReader {
        let slot = body.slot();
        let probing = Arc::new(AtomicBool::new(matches!(wait, ByteWait::Wait(_))));
        GrowReader {
            body,
            pos: 0,
            slot,
            wait,
            probing,
        }
    }

    /// Bytes are not there: say so, or wait until `ready`.
    fn short(&self, why: &'static str, ready: impl Fn(&GrowBody) -> bool) -> io::Result<()> {
        match &self.wait {
            ByteWait::Yield => Err(io::Error::new(io::ErrorKind::WouldBlock, why)),
            ByteWait::Wait(w) => self.body.wait_until(w, ready),
        }
    }

    fn moved(&mut self, pos: u64) {
        self.pos = pos;
        self.slot.store(pos, Ordering::Release);
    }
}

impl Read for GrowReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.body.is_cancelled() {
                return Ok(0);
            }
            let n = self.body.copy_at(self.pos, buf);
            if n > 0 {
                self.moved(self.pos + n as u64);
                return Ok(n);
            }
            if self.body.is_finished() {
                return Ok(0);
            }
            let pos = self.pos;
            self.short("waiting for more of the stream", |b| {
                b.len() as u64 > pos || b.is_finished()
            })?;
        }
    }
}

impl Seek for GrowReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        // End-relative seeks need the finished length.
        if matches!(pos, SeekFrom::End(_)) && !self.body.is_finished() {
            self.short(
                "waiting for the full stream to seek from the end",
                GrowBody::is_finished,
            )?;
        }
        let len = self.body.len() as u64;
        let target = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(p) => self.pos.saturating_add_signed(p),
            SeekFrom::End(p) => len.saturating_add_signed(p),
        };
        if target > len && !self.body.is_finished() {
            self.short("waiting for more of the stream to seek", |b| {
                b.len() as u64 >= target || b.is_finished()
            })?;
        }
        let target = target.min(self.body.len() as u64);
        self.moved(target);
        Ok(target)
    }
}

impl SymphoniaMediaSource for GrowReader {
    fn is_seekable(&self) -> bool {
        !self.probing.load(Ordering::Acquire)
    }

    fn byte_len(&self) -> Option<u64> {
        let total = self.body.total.load(Ordering::Acquire);
        if total > 0 {
            return Some(total);
        }
        self.body.is_finished().then(|| self.body.len() as u64)
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

/// Frames decoded before a seek's target and dropped, so a lossy decoder's state is what a
/// straight decode's is there (see the WP2 notes in the design, 9.2).
fn preroll_frames(codec: Option<AudioCodecId>, rate: u32) -> u64 {
    match codec {
        // RFC 7845 asks for at least 80 ms, which converges to the ear; for the bits, the
        // inter-frame energy prediction (a factor of 0.5 to 0.9 a frame) needs about a second.
        Some(c) if c == CODEC_ID_OPUS => u64::from(rate),
        // One packet: the overlap of the one before.
        Some(c) if c == CODEC_ID_AAC => 1024,
        // Eight MPEG-1 frames: the overlap of the one before, and seven to fill that one's bit
        // reservoir (up to 511 bytes back: seven frames at 32 kbps; MPEG-2's 255 bytes, at its
        // lowest rate, fit in as many of its half-length frames).
        Some(c) if c == CODEC_ID_MP3 || c == CODEC_ID_MP2 || c == CODEC_ID_MP1 => 9216,
        // The longest block: a whole packet before the target's (the first after a reset gives
        // no sound).
        Some(c) if c == CODEC_ID_VORBIS => 8192,
        _ => 0,
    }
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        let extension = path.extension().and_then(|e| e.to_str());
        let mut s = Self::from_media_source(Box::new(file), extension, None, size_bytes, None)?;
        s.reopen = Some(Reopen {
            origin: Origin::File(path.to_owned()),
            extension: extension.map(str::to_owned),
            mime_type: None,
            size_bytes,
            duration_hint_ms: None,
        });
        Ok(s)
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
        let d = start_download(audio, refresh, watch, note)?;
        Ok(Self::on_download(d, duration_hint_ms))
    }

    /// Resolve through MediaAccess on a worker thread, then download — never on the audio thread.
    pub fn open_remote_deferred(
        access: Arc<dyn media_access::MediaAccess>,
        track_id: impl Into<String>,
        duration_hint_ms: Option<u64>,
    ) -> Result<Self, String> {
        let d = start_deferred(access, track_id.into())?;
        Ok(Self::on_download(d, duration_hint_ms))
    }

    /// A source of the song `download` holds; it opens once enough of it is in.
    pub fn on_download(download: Arc<Download>, duration_hint_ms: Option<u64>) -> Self {
        Self {
            reader: None,
            decoder: None,
            track_id: 0,
            codec_id: None,
            format: SourceFormat {
                rate: 44_100,
                channels: 2,
                bits: 32,
                lossless: false,
                codec: "Remote".into(),
            },
            frames: None,
            info: FileInfo::default(),
            time_base: None,
            ts_range: (0, None),
            duration_hint_ms,
            remote: Some(RemoteState {
                download,
                finished: false,
                pending_seek: None,
                retry_after: None,
            }),
            download: None,
            read_pos: None,
            wait: ByteWait::Yield,
            primed: false,
            starved: false,
            loudness_db: None,
            at: 0,
            base: None,
            anchor: false,
            drop_until: 0,
            held: None,
            first_sound: 0,
            reopen: None,
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
        let start_ts = track.start_ts.get();
        // The duration is in ticks; a frame count only serves when a tick is a frame.
        let last_ts = track
            .duration
            .map(|d| d.get())
            .or(if time_base.is_none() {
                track.num_frames
            } else {
                None
            })
            .map(|n| start_ts.saturating_add(i64::try_from(n).unwrap_or(i64::MAX)));
        let decoder = codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("Venuta can't decode this file ({e})."))?;
        let mut codec = if params.codec == CODEC_ID_OPUS {
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
            codec_id: Some(params.codec),
            format,
            frames,
            info,
            time_base,
            ts_range: (start_ts, last_ts),
            duration_hint_ms,
            remote: None,
            download: None,
            read_pos: None,
            wait: ByteWait::Yield,
            // Local files are ready immediately; progressive sets this after first samples.
            primed: true,
            starved: false,
            loudness_db: None,
            at: 0,
            base: None,
            anchor: false,
            drop_until: 0,
            held: None,
            first_sound: 0,
            reopen: None,
        })
    }

    /// How this source's reader meets bytes that have not come (online songs; set it before
    /// the first decode). Files never wait for bytes.
    pub fn set_byte_wait(&mut self, wait: ByteWait) {
        self.wait = wait;
    }

    /// True when the UI may leave Opening…: local always; remote only after first samples.
    pub fn is_ready(&self) -> bool {
        if self.remote.is_some() {
            return false;
        }
        if self.download.is_some() {
            return self.primed;
        }
        true
    }

    /// Whether this is an online song (downloading, or downloaded).
    pub fn is_stream(&self) -> bool {
        self.remote.is_some() || self.download.is_some()
    }

    /// Whether it has opened (an online song opens once enough of it is in).
    pub fn is_open(&self) -> bool {
        self.reader.is_some()
    }

    /// For an online song, how much of its bytes are in.
    pub fn stream_bytes(&self) -> Option<StreamBytes> {
        let d = self
            .download
            .as_ref()
            .or_else(|| self.remote.as_ref().map(|r| &r.download))?;
        Some(d.bytes())
    }

    /// Where this source's reader is in its song's bytes.
    pub fn bytes_read(&self) -> Option<u64> {
        self.read_pos.as_ref().map(|p| p.load(Ordering::Acquire))
    }

    /// A look at this (opened) online song's bytes from another thread.
    pub fn stream_watch(&self) -> Option<StreamWatch> {
        Some(StreamWatch {
            download: Arc::clone(self.download.as_ref()?),
            read: Arc::clone(self.read_pos.as_ref()?),
        })
    }

    /// Length in milliseconds, when known.
    pub fn duration_ms(&self) -> Option<u64> {
        self.frames
            .map(|f| f * 1000 / u64::from(self.format.rate.max(1)))
            .or(self.duration_hint_ms)
    }

    /// The content frame after the last frame `next` gave: where the next one lies (after a
    /// seek, the frame it landed on).
    pub fn position(&self) -> u64 {
        self.at.max(self.drop_until)
    }

    /// The frames a seek decodes before its target and drops.
    pub fn preroll(&self) -> u64 {
        preroll_frames(self.codec_id, self.format.rate)
    }

    /// Decode the next stretch into `out` (cleared first), or report that a remote chunk is
    /// not ready yet. With [`ByteWait::Yield`] this never waits for network I/O; with
    /// [`ByteWait::Wait`] it waits for the bytes instead and never reports `Pending`.
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
            && let Some(why) = self.download.as_ref().and_then(|d| d.body.failure())
        {
            return Err(why);
        }
        state
    }

    /// An opened stream waiting for more of its bytes: all that has arrived is decoded.
    pub fn is_waiting(&self) -> bool {
        self.starved
    }

    /// Open an online song once enough of it is in. With [`ByteWait::Wait`] this waits for
    /// that (or for the download to fail); otherwise it says `Pending` (false) until then.
    pub fn open_now(&mut self) -> Result<bool, String> {
        while self.remote.is_some() {
            if !self.step_open()? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// One try at opening: `false` when it must wait (only with `ByteWait::Yield`).
    fn step_open(&mut self) -> Result<bool, String> {
        loop {
            let remote = self.remote.as_mut().expect("remote source");
            if remote.finished {
                return Err("This song has already finished.".into());
            }
            let body = Arc::clone(&remote.download.body);
            let retry = remote.retry_after;
            let info = body.open_info();
            let short = |b: &GrowBody| retry.is_some_and(|need| (b.len() as u64) < need);
            let has_info = info.is_some();
            if let Some(info) = info
                && !(short(&body) && !body.is_finished())
            {
                return self.finish_open(info);
            }
            if !has_info {
                if let Some(why) = body.failure() {
                    remote.finished = true;
                    return Err(why);
                }
                if body.worker_done() {
                    remote.finished = true;
                    return Err("The remote stream ended before audio could be decoded.".into());
                }
            }
            match &self.wait {
                ByteWait::Yield => return Ok(false),
                ByteWait::Wait(w) => body
                    .wait_until(w, |b| {
                        (b.open_info().is_some() && !(short(b) && !b.is_finished()))
                            || b.failure().is_some()
                            || b.worker_done()
                    })
                    .map_err(|e| e.to_string())?,
            }
        }
    }

    fn next_remote(&mut self, out: &mut Vec<i32>) -> Result<DecodeState, String> {
        out.clear();
        if self.remote.as_ref().is_some_and(|r| r.finished) {
            return Ok(DecodeState::End);
        }
        if !self.open_now()? {
            return Ok(DecodeState::Pending);
        }
        // Opened: decoding goes on as for any source.
        self.next(out)
    }

    /// Open on the download now that enough is in; `false` if the probe ran short of bytes
    /// (only with `ByteWait::Yield`: a waiting reader never runs short).
    fn finish_open(&mut self, info: OpenInfo) -> Result<bool, String> {
        let remote = self.remote.as_ref().expect("remote source");
        let download = Arc::clone(&remote.download);
        let pending_seek = remote.pending_seek;
        let body = Arc::clone(&download.body);
        let size_bytes = info
            .size_hint
            .filter(|n| *n > 0)
            .unwrap_or_else(|| body.len() as u64);
        let reader = GrowReader::new(Arc::clone(&body), self.wait.clone());
        let read_pos = Arc::clone(&reader.slot);
        let probing = Arc::clone(&reader.probing);
        let extension = media_extension(info.mime_type.as_deref(), &info.url_hint);
        let duration_hint_ms = info.duration_hint_ms.or(self.duration_hint_ms);
        let open = Self::from_media_source(
            Box::new(reader),
            extension.as_deref(),
            info.mime_type.as_deref(),
            size_bytes,
            duration_hint_ms,
        );
        probing.store(false, Ordering::Release);
        match open {
            Ok(mut opened) => {
                opened.download = Some(download);
                opened.read_pos = Some(read_pos);
                opened.wait = self.wait.clone();
                opened.primed = false;
                opened.duration_hint_ms = opened.duration_hint_ms.or(self.duration_hint_ms);
                opened.loudness_db = info.loudness_db;
                opened.reopen = Some(Reopen {
                    origin: Origin::Body(Arc::clone(&body)),
                    extension,
                    mime_type: info.mime_type.clone(),
                    size_bytes,
                    duration_hint_ms,
                });
                *self = opened;
                match pending_seek {
                    Some(PendingSeek::Ms(ms)) => self.seek_or_wait(|s| s.seek(ms))?,
                    Some(PendingSeek::Frame(f)) => self.seek_or_wait(|s| s.seek_frame(f))?,
                    None => {}
                }
                Ok(true)
            }
            Err(e) => {
                // A probe starved for bytes (only a reader that never waits): keep downloading
                // and retry. A finished body, or a reader that waited, is a real error.
                if body.is_finished() || matches!(self.wait, ByteWait::Wait(_)) {
                    return Err(body.failure().unwrap_or(e));
                }
                let retry = body.len() as u64 + 128 * 1024;
                if let Some(remote) = self.remote.as_mut() {
                    remote.retry_after = Some(retry);
                }
                Ok(false)
            }
        }
    }

    /// A seek asked before the song opened, made now. One that is short of bytes (only a
    /// reader that never waits) is let go: playback starts from the top instead.
    fn seek_or_wait(
        &mut self,
        seek: impl FnOnce(&mut Self) -> Result<u64, String>,
    ) -> Result<(), String> {
        match seek(self) {
            Ok(_) => Ok(()),
            Err(e) if e.contains("waiting") => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// A container timestamp (in the track's ticks) as frames.
    fn ts_frames(&self, ts: i128) -> i128 {
        let rate = i128::from(self.format.rate.max(1));
        match self.time_base {
            Some((numer, denom)) => {
                (ts * i128::from(numer) * rate).div_euclid(i128::from(denom.max(1)))
            }
            None => ts,
        }
    }

    /// The packet grid of an Opus track whose container counts coarser than a frame (WebM's
    /// milliseconds): every Opus packet is a whole number of 2.5 ms, and a tick is less than
    /// half of that, so the grid point nearest the container's place is the place.
    fn grid(&self) -> Option<i64> {
        let (numer, denom) = self.time_base?;
        let rate = u64::from(self.format.rate);
        let per_tick = numer * rate / denom.max(1);
        let grid = rate / 400;
        (self.codec_id == Some(CODEC_ID_OPUS) && per_tick > 1 && per_tick * 2 < grid)
            .then_some(grid as i64)
    }

    /// Frames as a container timestamp, rounded down (so a seek lands at or before them).
    fn frames_ts(&self, frames: i128) -> i128 {
        let rate = i128::from(self.format.rate.max(1));
        match self.time_base {
            Some((numer, denom)) => {
                (frames * i128::from(denom)).div_euclid(i128::from(numer.max(1)) * rate)
            }
            None => frames,
        }
    }

    /// The next packet of this source's track (one held back first).
    fn next_packet(&mut self) -> Result<Option<Packet>, LocalError> {
        if let Some(p) = self.held.take() {
            return Ok(Some(p));
        }
        loop {
            match self.reader.as_mut().expect("local reader").next_packet() {
                Ok(Some(p)) if p.track_id != self.track_id => continue,
                Ok(p) => return Ok(p),
                Err(symphonia::core::errors::Error::IoError(ref e)) if is_starved(e) => {
                    return Err(LocalError::Pending);
                }
                Err(symphonia::core::errors::Error::IoError(ref e)) if e.to_string() == STOPPED => {
                    return Err(LocalError::Failed(STOPPED.into()));
                }
                Err(e) => {
                    return Err(LocalError::Failed(format!(
                        "The file is damaged here ({e})."
                    )));
                }
            }
        }
    }

    fn next_local(&mut self, out: &mut Vec<i32>) -> Result<LocalDecode, String> {
        let channels = usize::from(self.format.channels);
        loop {
            let packet = match self.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(LocalDecode::End),
                Err(LocalError::Pending) => return Ok(LocalDecode::Pending),
                Err(LocalError::Failed(e)) => return Err(e),
            };
            // Where the packet's valid frames start, in frames of the container's count.
            let start = self
                .ts_frames(i128::from(packet.pts.get()) + i128::from(packet.trim_start.get()))
                as i64;
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
            let n = (out.len() / channels) as i64;
            if n == 0 {
                continue;
            }
            let first = if self.base.is_none() {
                // The first packet with sound is frame 0 of the straight decode. Its sound ends
                // where the next packet starts (a decoder that trims more than the container
                // says, as the libopus adapter does with the pre-skip once, trims the start).
                let end = match self.next_packet() {
                    Ok(Some(next)) => {
                        let end = self.ts_frames(i128::from(next.pts.get())) as i64;
                        self.held = Some(next);
                        end
                    }
                    _ => start + n,
                };
                self.base = Some(end - n);
                self.first_sound = n;
                self.at as i64
            } else if std::mem::take(&mut self.anchor) {
                // After a seek: where the container says, as the straight decode counts.
                let first = start - self.base.unwrap_or(0);
                match self.grid() {
                    // To a tick, in a container counting coarser than frames: Opus packets are
                    // whole 2.5 ms, so the place is the grid point within that tick.
                    Some(g) => {
                        let phase = self.first_sound;
                        phase + (first - phase + g / 2).div_euclid(g) * g
                    }
                    None => first,
                }
            } else {
                self.at as i64
            };
            self.at = (first + n).max(0) as u64;
            // Frames before the song's first (a packet the straight decode trimmed more of) or
            // before a seek's target are dropped.
            let keep_from = (self.drop_until as i64).max(0);
            if first < keep_from {
                let drop = (keep_from - first).min(n) as usize;
                out.drain(..drop * channels);
                if out.is_empty() {
                    continue;
                }
            }
            return Ok(LocalDecode::Samples);
        }
    }

    /// Learn the base (see `base`) from the first packet with sound, when nothing has been
    /// decoded yet. A stream short of bytes that can't wait counts from 0.
    fn learn_base(&mut self) -> Result<(), String> {
        let mut out = Vec::new();
        while self.base.is_none() {
            match self.next_local(&mut out)? {
                LocalDecode::Samples => {}
                LocalDecode::End | LocalDecode::Pending => self.base = Some(0),
            }
        }
        Ok(())
    }

    /// Read the song again from its first byte, with a fresh decoder: what comes next is the
    /// straight decode from its start. `false` when it can't be read again.
    fn rewind(&mut self) -> Result<bool, String> {
        let Some(r) = self.reopen.clone() else {
            return Ok(false);
        };
        let mut probing = None;
        let media: Box<dyn SymphoniaMediaSource> = match &r.origin {
            Origin::File(path) => Box::new(
                File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?,
            ),
            Origin::Body(body) => {
                let reader = GrowReader::new(Arc::clone(body), self.wait.clone());
                self.read_pos = Some(Arc::clone(&reader.slot));
                probing = Some(Arc::clone(&reader.probing));
                Box::new(reader)
            }
        };
        let fresh = Self::from_media_source(
            media,
            r.extension.as_deref(),
            r.mime_type.as_deref(),
            r.size_bytes,
            r.duration_hint_ms,
        );
        if let Some(p) = probing {
            p.store(false, Ordering::Release);
        }
        let fresh = fresh?;
        self.reader = fresh.reader;
        self.decoder = fresh.decoder;
        self.held = None;
        self.anchor = false;
        self.at = 0;
        Ok(true)
    }

    /// Go to `ms`, exactly: see [`Source::seek_frame`]. Returns the frame now playing from.
    ///
    /// Remote songs open after the first download window; seek waits for bytes if needed.
    /// A seek asked before open is remembered and applied as soon as decode starts.
    pub fn seek(&mut self, ms: u64) -> Result<u64, String> {
        if let Some(remote) = self.remote.as_mut() {
            if remote.finished {
                return Err("This song has already finished.".into());
            }
            remote.pending_seek = Some(PendingSeek::Ms(ms));
            let rate = u64::from(self.format.rate.max(1));
            return Ok(ms.saturating_mul(rate) / 1000);
        }
        let rate = u64::from(self.format.rate.max(1));
        self.seek_frame(ms.saturating_mul(rate) / 1000)
    }

    /// Go to content frame `frame`, exactly: the container seeks to the nearest point before
    /// it, less the codec's pre-roll, and the frames in between are decoded and dropped. So what
    /// comes next is what a straight decode gives from `frame`. Returns the frame it landed on
    /// (`frame`, or the last frame of a song that is shorter).
    pub fn seek_frame(&mut self, frame: u64) -> Result<u64, String> {
        if let Some(remote) = self.remote.as_mut() {
            if remote.finished {
                return Err("This song has already finished.".into());
            }
            remote.pending_seek = Some(PendingSeek::Frame(frame));
            return Ok(frame);
        }
        let mut target = frame;
        if let Some(total) = self.frames {
            target = target.min(total.saturating_sub(1));
        }
        if self.base.is_none() {
            self.learn_base()?;
        }
        let base = self.base.unwrap_or(0);
        let mut from = target.saturating_sub(self.preroll());
        // Within the pre-roll of the start: decode from the first byte (containers differ in
        // how they seek to their very start; this is the straight decode itself).
        if from == 0 && self.rewind()? {
            self.drop_until = target;
            return Ok(target);
        }
        let (first_ts, last_ts) = self.ts_range;
        // A container that can't seek to a place (symphonia's WebM, in the last cluster) is
        // sought earlier, and decoded forward from there.
        let mut back = u64::from(self.format.rate.max(1)) * 10;
        let mut at_first = false;
        loop {
            let mut ts = self.frames_ts(i128::from(from) + i128::from(base)) as i64;
            ts = ts.max(first_ts);
            if at_first {
                ts = first_ts;
            }
            if let Some(last) = last_ts {
                ts = ts.min(last);
            }
            let tried = self.reader.as_mut().expect("local reader").seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: Timestamp::new(ts),
                    track_id: self.track_id,
                },
            );
            match tried {
                Ok(_) => break,
                Err(symphonia::core::errors::Error::IoError(ref io)) if is_starved(io) => {
                    return Err("waiting for more of the stream to seek".to_owned());
                }
                Err(symphonia::core::errors::Error::SeekError(_)) if ts > first_ts => {
                    at_first = from == 0;
                    from = from.saturating_sub(back);
                    back = back.saturating_mul(2);
                    if from == 0 && self.rewind()? {
                        self.drop_until = target;
                        return Ok(target);
                    }
                }
                Err(other) => return Err(format!("Could not move there ({other}).")),
            }
        }
        self.held = None;
        self.decoder.as_mut().expect("local decoder").reset();
        self.anchor = true;
        self.drop_until = target;
        self.at = target;
        Ok(target)
    }
}

/// Asks for a fresh stream address for the same song, after the old one stopped working partway.
pub(crate) type Refresh = Box<dyn FnMut() -> Option<EphemeralAudio> + Send>;

/// Where a download says what happened to it (the playback log). Never an address.
pub(crate) type Note = Arc<dyn Fn(&str) + Send + Sync>;

fn playback_log() -> Note {
    Arc::new(media_access::note_timing)
}

/// Marks the body's worker finished however its thread ends (a panic too).
struct WorkerDone(Arc<GrowBody>);

impl Drop for WorkerDone {
    fn drop(&mut self) {
        self.0.done_working();
    }
}

/// Start downloading `audio` on a thread of its own.
fn start_download(
    audio: EphemeralAudio,
    refresh: Option<Refresh>,
    watch: Watch,
    note: Note,
) -> Result<Arc<Download>, String> {
    let body = GrowBody::new();
    let worker = Arc::clone(&body);
    std::thread::Builder::new()
        .name("venuta-stream-decoder".into())
        .spawn(move || {
            let _done = WorkerDone(Arc::clone(&worker));
            decode_remote(audio, &worker, refresh, watch, note);
        })
        .map_err(|_| "Could not start the remote stream decoder.")?;
    Ok(Download::new(body))
}

/// Resolve `track_id` through `access` on a thread of its own, then download it there.
fn start_deferred(
    access: Arc<dyn media_access::MediaAccess>,
    track_id: String,
) -> Result<Arc<Download>, String> {
    let body = GrowBody::new();
    let worker = Arc::clone(&body);
    std::thread::Builder::new()
        .name("venuta-stream-resolve".into())
        .spawn(move || {
            let _done = WorkerDone(Arc::clone(&worker));
            if worker.is_cancelled() {
                return;
            }
            let cancel = media_access::CancellationToken::default();
            // If the player dropped us, cooperative cancel still helps long resolves.
            let audio = match access.resolve_audio(&track_id, &cancel) {
                Ok(a) => a,
                Err(e) => {
                    worker.fail(&e.to_string());
                    return;
                }
            };
            if worker.is_cancelled() {
                return;
            }
            // Partway failures ask the catalog again, past its memory of the old address.
            let refresh: Refresh = Box::new(move || {
                access.forget(&track_id);
                access
                    .resolve_audio(&track_id, &media_access::CancellationToken::default())
                    .ok()
            });
            decode_remote(audio, &worker, Some(refresh), WATCH, playback_log());
        })
        .map_err(|_| "Could not start the remote stream resolver.")?;
    Ok(Download::new(body))
}

fn decode_remote(
    audio: EphemeralAudio,
    body: &Arc<GrowBody>,
    refresh: Option<Refresh>,
    watch: Watch,
    note: Note,
) {
    if body.is_cancelled() {
        return;
    }
    if let Err(error) = fill_grow_body(audio, body, refresh, &watch, &*note)
        && !body.is_cancelled()
    {
        // What did arrive still plays; then the song says why it stopped.
        body.fail(&error);
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

/// Download into `body`, saying it can open (`GrowBody::set_open`) once enough bytes (or the whole
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
        if body.len() < need && !body.is_finished() {
            return;
        }
        opened = true;
        note(&format!(
            "online song ready to open after {} ms ({} KB in)",
            media_access::ms_since(started),
            body.len() / 1024
        ));
        body.set_open(OpenInfo {
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
        GrowBody::new()
    }

    /// Plays as the bytes come: the decoder always at the edge of what has arrived, so nothing
    /// is ever ahead of it (what decides whether a slow stream will run dry).
    fn listen_at_the_edge(body: &Arc<GrowBody>) {
        let body = Arc::clone(body);
        let slot = body.slot();
        std::thread::spawn(move || {
            while !body.is_finished() {
                slot.store(body.len() as u64, Ordering::Release);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn straight(path: &Path) -> Vec<i32> {
        let mut s = Source::open(path).unwrap();
        let (mut all, mut out) = (Vec::new(), Vec::new());
        while let DecodeState::Samples = s.next(&mut out).unwrap() {
            all.extend_from_slice(&out);
        }
        all
    }

    fn info(mime: &str, ext: &str) -> OpenInfo {
        OpenInfo {
            mime_type: Some(mime.into()),
            url_hint: format!("song.{ext}"),
            ..OpenInfo::default()
        }
    }

    #[test]
    fn a_body_keeps_its_bytes_in_chunks_and_reads_them_from_anywhere() {
        let body = GrowBody::new();
        let bytes: Vec<u8> = (0..(3 * CHUNK + 777)).map(|i| (i % 253) as u8).collect();
        // Pieces that straddle the chunk boundaries.
        for piece in bytes.chunks(CHUNK / 3 + 5) {
            body.append(piece);
        }
        assert_eq!(body.len(), bytes.len());
        assert_eq!(
            body.lock().len(),
            4,
            "64 KiB chunks, never one growing buffer"
        );
        assert!(body.to_vec() == bytes);
        let mut r = GrowReader::new(Arc::clone(&body), ByteWait::Yield);
        for at in [
            0u64,
            1,
            CHUNK as u64 - 1,
            CHUNK as u64,
            2 * CHUNK as u64 + 3,
        ] {
            r.seek(SeekFrom::Start(at)).unwrap();
            let mut buf = vec![0u8; CHUNK + 10];
            let mut got = Vec::new();
            while got.len() < buf.len() {
                let n = r.read(&mut buf[got.len()..]).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[got.len()..got.len() + n]);
            }
            let end = (at as usize + buf.len()).min(bytes.len());
            assert!(got == bytes[at as usize..end], "from {at}");
        }
        // Past what has come: a reader that never waits says so.
        r.seek(SeekFrom::Start(bytes.len() as u64)).unwrap();
        let e = r.read(&mut [0u8; 4]).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn a_waiting_reader_waits_for_the_bytes_and_says_so() {
        let body = GrowBody::new();
        body.append(&[1, 2, 3]);
        let told = Arc::new(Mutex::new(Vec::new()));
        let t = Arc::clone(&told);
        let w = Waiter::new(None, Arc::new(AtomicBool::new(false)), move |waiting| {
            t.lock().unwrap().push(waiting)
        });
        let mut r = GrowReader::new(Arc::clone(&body), ByteWait::Wait(w));
        let mut buf = [0u8; 8];
        assert_eq!(r.read(&mut buf).unwrap(), 3);
        let feeder = Arc::clone(&body);
        let late = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            feeder.append(&[4, 5]);
        });
        let started = Instant::now();
        assert_eq!(
            r.read(&mut buf).unwrap(),
            2,
            "the read waited for the bytes"
        );
        assert!(started.elapsed() >= Duration::from_millis(90));
        assert_eq!(&buf[..2], &[4, 5]);
        assert_eq!(*told.lock().unwrap(), vec![true, false]);
        late.join().unwrap();
        body.finish();
        assert_eq!(r.read(&mut buf).unwrap(), 0, "the end once all is in");
    }

    #[test]
    fn a_stopped_waiter_ends_the_wait() {
        let body = GrowBody::new();
        let stop = Arc::new(AtomicBool::new(false));
        let w = Waiter::new(None, Arc::clone(&stop), |_| {});
        let mut r = GrowReader::new(Arc::clone(&body), ByteWait::Wait(w));
        let s = Arc::clone(&stop);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            s.store(true, Ordering::Release);
        });
        let e = r.read(&mut [0u8; 4]).unwrap_err();
        assert_eq!(e.to_string(), STOPPED);
        t.join().unwrap();
    }

    #[test]
    fn readers_at_their_own_places_and_the_bytes_ahead_of_the_furthest() {
        let body = GrowBody::new();
        body.append(&[0u8; 1000]);
        assert_eq!(body.ahead(), 1000, "nobody reading: all of it");
        let mut a = GrowReader::new(Arc::clone(&body), ByteWait::Yield);
        let mut b = GrowReader::new(Arc::clone(&body), ByteWait::Yield);
        a.seek(SeekFrom::Start(300)).unwrap();
        b.seek(SeekFrom::Start(700)).unwrap();
        assert_eq!(body.ahead(), 300);
        drop(b);
        assert_eq!(body.ahead(), 700, "a reader gone counts no more");
        let _ = a.read(&mut [0u8; 100]).unwrap();
        assert_eq!(body.ahead(), 600);
    }

    #[test]
    fn a_fed_download_opens_once_enough_is_in_and_is_shared_by_key() {
        let bytes = std::fs::read(fixture("seek-music.mp3")).unwrap();
        let (d, mut feeder) = Download::fed(info("audio/mpeg", "mp3"));
        let streams = Streams::new();
        streams.insert("k", &d);
        let mut s = Source::on_download(Arc::clone(&d), Some(6_000));
        let mut out = Vec::new();
        feeder.append(&bytes[..32 * 1024]);
        assert_eq!(
            s.next(&mut out).unwrap(),
            DecodeState::Pending,
            "an MP3 opens at 64 KiB"
        );
        feeder.append(&bytes[32 * 1024..]);
        feeder.finish();
        assert_eq!(s.next(&mut out).unwrap(), DecodeState::Samples);
        assert!(d.bytes().complete);
        let held = streams.hold("k").expect("live while held");
        drop((d, s));
        assert!(!feeder.is_cancelled(), "a hold keeps the download");
        drop(held);
        assert!(
            feeder.is_cancelled(),
            "the last holder gone: the download stops"
        );
        assert!(streams.hold("k").is_none());
    }

    /// The catalog's loudness reaches the opened song (it was lost when the placeholder was
    /// replaced by the opened source).
    #[test]
    fn an_online_song_keeps_the_catalog_s_loudness_once_open() {
        let bytes = std::fs::read(fixture("seek-music.mp3")).unwrap();
        let mut i = info("audio/mpeg", "mp3");
        i.loudness_db = Some(-7.5);
        let (d, mut feeder) = Download::fed(i);
        feeder.append(&bytes);
        feeder.finish();
        let mut s = Source::on_download(d, None);
        let mut out = Vec::new();
        assert_eq!(s.next(&mut out).unwrap(), DecodeState::Samples);
        assert_eq!(s.loudness_db, Some(-7.5));
    }

    /// What the gate in `voice` closes: a reader that never waits, fed in pieces that end inside
    /// packets, against the straight decode of the same file. A record of the old engine's reader
    /// (on 2026-10-22 it gave 290398 of an MP3's 528040 samples; M4A and WebM came through).
    #[test]
    #[ignore = "a record of the reader that never waits; run by hand with --nocapture"]
    fn a_reader_that_never_waits_on_pieces_ending_inside_packets() {
        for (name, mime, ext) in [
            ("seek-music.mp3", "audio/mpeg", "mp3"),
            ("seek-music.m4a", "audio/mp4", "m4a"),
            ("seek-music.webm", "audio/webm", "webm"),
        ] {
            let bytes = std::fs::read(fixture(name)).unwrap();
            let want = straight(&fixture(name));
            let (d, mut feeder) = Download::fed_opening_after(info(mime, ext), 16 * 1024);
            let mut s = Source::on_download(d, None);
            let (mut got, mut out) = (Vec::new(), Vec::new());
            let mut at = 0;
            let mut ended = false;
            while !ended {
                match s.next(&mut out) {
                    Ok(DecodeState::Samples) => got.extend_from_slice(&out),
                    Ok(DecodeState::Pending) if at < bytes.len() => {
                        let end = (at + 997).min(bytes.len());
                        feeder.append(&bytes[at..end]);
                        at = end;
                        if at == bytes.len() {
                            feeder.finish();
                        }
                    }
                    Ok(DecodeState::Pending) => panic!("{name}: waiting with every byte in"),
                    Ok(DecodeState::End) | Err(_) => ended = true,
                }
            }
            println!(
                "{name}: a reader that never waits gave {} of {} samples, {}",
                got.len(),
                want.len(),
                if got == want {
                    "the same"
                } else {
                    "not the same"
                }
            );
        }
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
        let (note, lines) = log();
        let started = Instant::now();
        fill_grow_body(
            audio(&host.url, song.len(), 36_000),
            &body,
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
        assert!(body.to_vec() == *song, "the song arrived whole");
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
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 30_000),
            &body,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(host.asked(), vec![0, 100 * 1024]);
        assert!(body.to_vec() == *song);
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
        let (note, lines) = log();
        fill_grow_body(
            audio(&old.url, song.len(), 30_000),
            &body,
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
        assert!(body.to_vec() == *song);
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
        let (note, lines) = log();
        let started = Instant::now();
        let result = fill_grow_body(
            audio(&host.url, song.len(), 30_000),
            &body,
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
        let mut reader = GrowReader::new(Arc::clone(&body), ByteWait::Yield);
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
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 3_000),
            &body,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(host.asked(), vec![0], "one request, never given up");
        assert!(body.to_vec() == *song);
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
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 3_000),
            &body,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        let asked = host.asked();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert!(asked[1] > 0, "carried on from where the trickle got to");
        assert!(body.to_vec() == *song);
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
        let (note, lines) = log();
        fill_grow_body(
            audio(&host.url, song.len(), 7_000),
            &body,
            None,
            &quick_watch(400),
            &*note,
        )
        .unwrap();
        assert_eq!(host.asked(), vec![0]);
        assert!(body.to_vec() == *song);
        assert!(!has(&lines.lock().unwrap(), "trickling"));
    }
}
