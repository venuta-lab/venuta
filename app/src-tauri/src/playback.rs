//! The player, as the window sees it: commands in, a stream of status out.
//!
//! The engine (`player` crate) runs on its own thread. Status arrives about ten times a second
//! and goes straight to the window over a channel; the queue travels only when it changes.

use crate::one_window::{self, Host, WindowKind};
use media_access::{Availability, MediaAccess, PlayableId};
use player::decode::{Source, Streams};
use player::engine::{Cmd, EngineKind, Item, OutputChoice, Player, Signal, Status};
use player::queue::Repeat;
use player::remote::RemoteItem;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri::ipc::Channel;

/// A [`MediaAccess`] handle carried on a queue item so a remote track can resolve its stream at
/// open time. Skipped in (de)serialization — a snapshot is id + metadata only — and kept out of
/// `Debug` so the handle never prints.
#[derive(Clone, Default)]
pub struct MediaRef(Option<Arc<dyn MediaAccess>>);

impl MediaRef {
    /// A remote item's own way to its stream (listening together: the host's file, streamed).
    pub fn new(access: Arc<dyn MediaAccess>) -> Self {
        MediaRef(Some(access))
    }
}

impl std::fmt::Debug for MediaRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "MediaRef(configured)"
        } else {
            "MediaRef(none)"
        })
    }
}

/// A song in the queue, as the window sent it, plus where it is.
///
/// Hybrid: a **local** item carries a library-relative `path` (resolved to `file`); a **remote**
/// item carries `online` (a stable provider id) and metadata only — never a stream URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub dir: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_artist: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub bit_depth: u16,
    #[serde(default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub lyrics: bool,
    /// Your rating, 0–5.
    #[serde(default)]
    pub stars: u8,
    /// ReplayGain from the tags.
    #[serde(default)]
    pub rg: player::dsp::Gain,
    /// Stable remote-catalog identity (no stream URL). Present for remote items.
    #[serde(default)]
    pub online: Option<PlayableId>,
    /// Cover art address for remote items (local items use their on-disk cover).
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// The online album a remote item is from (`MPRE…`), when the catalog named one: what the
    /// queue's Go to album opens. Local items have their folder instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_id: Option<String>,
    /// UI-facing availability, filled in at play time (`Local` | `Online`).
    #[serde(default)]
    pub availability: Option<Availability>,
    /// The file on disk (the open folder joined with `path`); empty for remote items.
    #[serde(skip)]
    pub file: PathBuf,
    /// Catalog handle for resolving a remote stream at open time (see [`MediaRef`]).
    #[serde(skip)]
    pub media: MediaRef,
}

impl QueueItem {
    /// A remote-catalog item: a stable online id and no local path to play from.
    fn is_remote(&self) -> bool {
        self.path.is_empty() && matches!(&self.online, Some(PlayableId::Online { .. }))
    }

    /// The on-disk file, for local items only (remote items have none).
    fn local_file(&self) -> Option<PathBuf> {
        (!self.is_remote()).then(|| self.file.clone())
    }

    /// A stable key for the media overlay: the file path locally, the provider id for remote.
    fn media_key(&self) -> String {
        match &self.online {
            Some(PlayableId::Online { provider, track_id }) if self.is_remote() => {
                format!("online:{provider}:{track_id}")
            }
            _ => self.file.to_string_lossy().into_owned(),
        }
    }
}

impl QueueItem {
    /// An online song as the player's remote item (resolved when it opens, never stored).
    fn remote(&self) -> Option<Result<RemoteItem, String>> {
        match &self.online {
            Some(PlayableId::Online { provider, track_id }) if self.is_remote() => {
                let access = self.media.0.clone();
                let album = (!self.album.is_empty()).then(|| self.album.clone());
                Some(
                    access
                        .ok_or_else(|| "No catalog access for remote playback.".to_owned())
                        .map(|access| {
                            RemoteItem::deferred_with_duration(
                                access,
                                provider.clone(),
                                track_id.clone(),
                                &self.title,
                                album,
                                Some(self.duration_ms).filter(|d| *d > 0),
                            )
                        }),
                )
            }
            _ => None,
        }
    }
}

impl Item for QueueItem {
    fn path(&self) -> Option<&Path> {
        (!self.is_remote()).then_some(self.file.as_path())
    }
    fn open(&self) -> Result<Source, String> {
        match self.remote() {
            // Resolve the ephemeral stream now, right before opening: never stored.
            Some(remote) => remote?.open(),
            None => Source::open(&self.file),
        }
    }
    fn stream_key(&self) -> Option<String> {
        self.remote()?.ok()?.stream_key()
    }
    fn open_on(&self, streams: &Streams) -> Result<Source, String> {
        match self.remote() {
            Some(remote) => remote?.open_on(streams),
            None => Source::open(&self.file),
        }
    }
    fn gain(&self) -> player::dsp::Gain {
        self.rg
    }
    fn album(&self) -> Option<String> {
        (!self.is_remote()).then(|| self.dir.clone())
    }
    fn reopen_when_stopped(&self) -> bool {
        !self.is_remote()
    }
}

/// What each window receives (see `app/src/lib/player.ts`).
#[derive(Serialize, Clone)]
pub struct WindowStatus {
    queue: Option<Vec<QueueItem>>,
    /// With `queue`, while shuffled: where each song of the order before shuffling is (so a
    /// window can put this queue back exactly, see `player_restore`).
    unshuffled: Option<Vec<usize>>,
    /// The folder the queue's songs are in (the mini player needs it for covers and lyrics).
    root: String,
    index: i64,
    playing: bool,
    /// Current song waiting for its stream: still opening (remote body downloading), or partway.
    buffering: bool,
    /// Buffering partway through the song (all that arrived played), not opening.
    stalled: bool,
    position_ms: u64,
    duration_ms: u64,
    shuffle: bool,
    repeat: Repeat,
    volume: f32,
    signal: Option<Signal>,
    from: String,
    error: String,
    sound: player::dsp::Sound,
    stop_after_current: bool,
    sleep_in_s: Option<u64>,
    bands: Option<Vec<u8>>,
}

/// The output the music is on: its endpoint (the key of its delay), its name and kind.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayingOn {
    pub endpoint: String,
    pub name: String,
    pub kind: Option<player::devices::Kind>,
}

/// What the Windows media overlay, media keys and the tray show. A loaded song has a non-empty
/// `title`; `file` is the local cover source (None for remote items, which use `artwork_url`).
#[derive(Clone, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub file: Option<PathBuf>,
    pub artwork_url: Option<String>,
    pub duration_ms: u64,
    pub playing: bool,
    pub position_ms: u64,
}

pub struct Playback {
    player: Arc<Mutex<Option<Player<QueueItem>>>>,
    /// Whoever follows what's playing outside the window (the media overlay, the tray).
    media: Arc<Mutex<Vec<std::sync::mpsc::Sender<NowPlaying>>>>,
    /// Every window listening (the main one, the mini player), by label.
    window: Arc<Mutex<std::collections::HashMap<String, Channel<WindowStatus>>>>,
    /// Windows showing a spectrum display (the engine measures while any is).
    spectrum: Mutex<std::collections::HashSet<String>>,
    /// The folder the queue's songs are in (for plays and picking up where you were).
    root: Arc<Mutex<String>>,
    /// Exclusive (bit-perfect) unless you choose shared; the device, or the default (saved in
    /// Settings, `player.output` and `player.exclusive`).
    output: Mutex<(Option<String>, bool)>,
    /// The endpoint the music is on now (the chosen one, its stereo twin, the default or a
    /// fallback), as the player last said.
    endpoint: Arc<Mutex<Option<PlayingOn>>>,
    /// Told when the music moves to another endpoint (a room takes that output's delay).
    #[allow(clippy::type_complexity)]
    on_output: Arc<Mutex<Option<Box<dyn Fn() + Send>>>>,
    /// Catalog + session-audio access for remote items (the app factory until a real provider).
    catalog: Arc<dyn MediaAccess>,
    /// Local only (Settings, Listening): every catalog request is refused while it's on.
    local_only: Arc<std::sync::atomic::AtomicBool>,
    offline: Arc<dyn MediaAccess>,
    /// Listening together: the room takes the player's buttons (the tray, media keys, every
    /// window), so no one plays on their own by accident.
    room_buttons: RoomButtons,
    /// Listening together: what the room plays now, shown in place of your own program (which
    /// waits paused meanwhile) by every window, the media overlay and the tray.
    room_now: Arc<Mutex<Option<RoomNow>>>,
    /// Your own program's last status (what a room's now playing is laid over).
    last: Arc<Mutex<Option<WindowStatus>>>,
    /// The listening thread's queue (plays, where you were, the playback log).
    keep: Arc<Mutex<Option<std::sync::mpsc::Sender<Keep>>>>,
}

/// What a room plays now, for the player's own bar while you are in one.
#[derive(Debug, Clone, Default)]
pub struct RoomNow {
    /// The room's queue, as the windows show songs.
    pub items: Vec<QueueItem>,
    pub index: i64,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// The room is getting a song ready for you, or waits for its stream.
    pub buffering: bool,
    /// Whose room (the source line).
    pub from: String,
}

/// Your place in the queue: the songs, the one playing and where, and how it plays.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Here {
    root: String,
    items: Vec<QueueItem>,
    index: i64,
    position_ms: u64,
    from: String,
    shuffle: bool,
    repeat: Repeat,
    order: Option<Vec<usize>>,
}

/// Your own queue set aside by a room of Venuta 0.3 (in the data folder): given back once, so a
/// Venuta closed in such a room still gets your own queue back after the update. Rooms now keep
/// your own program in the player itself.
fn aside_path() -> PathBuf {
    engine::paths::app_data_dir().join("together-yours.json")
}

/// A press of one of the player's buttons, from wherever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Toggle,
    Play,
    Pause,
    Next,
    Previous,
    Seek(u64),
}

type RoomButtons = Arc<Mutex<Option<Arc<dyn Fn(Button) + Send + Sync>>>>;

/// The room has the press when you're in one (said by `true`).
fn room_takes(buttons: &RoomButtons, b: Button) -> bool {
    // Out of the lock first: the room's answer may come back to the player.
    let f = buttons.lock().ok().and_then(|f| f.clone());
    f.map(|f| f(b)).is_some()
}

/// The catalog while Local only is on: it answers every request with "Local only is on", so
/// nothing reaches the network, whichever screen asks.
struct LocalOnlyAccess;

fn local_only_refusal() -> media_access::AccessError {
    media_access::AccessError::new(
        media_access::AccessErrorKind::Unsupported,
        "Local only is on: Venuta plays your own music and stays offline",
    )
}

impl MediaAccess for LocalOnlyAccess {
    fn search_tracks(
        &self,
        _: &str,
        _: &media_access::PageRequest,
        _: &media_access::CancellationToken,
    ) -> Result<media_access::Page<media_access::RemoteTrack>, media_access::AccessError> {
        Err(local_only_refusal())
    }
    fn search_albums(
        &self,
        _: &str,
        _: &media_access::PageRequest,
        _: &media_access::CancellationToken,
    ) -> Result<media_access::Page<media_access::RemoteAlbum>, media_access::AccessError> {
        Err(local_only_refusal())
    }
    fn search_artists(
        &self,
        _: &str,
        _: &media_access::PageRequest,
        _: &media_access::CancellationToken,
    ) -> Result<media_access::Page<media_access::RemoteArtist>, media_access::AccessError> {
        Err(local_only_refusal())
    }
    fn album_tracks(
        &self,
        _: &str,
        _: &media_access::PageRequest,
        _: &media_access::CancellationToken,
    ) -> Result<media_access::Page<media_access::RemoteTrack>, media_access::AccessError> {
        Err(local_only_refusal())
    }
    fn playlist_tracks(
        &self,
        _: &str,
        _: &media_access::PageRequest,
        _: &media_access::CancellationToken,
    ) -> Result<media_access::Page<media_access::RemoteTrack>, media_access::AccessError> {
        Err(local_only_refusal())
    }
    fn resolve_audio(
        &self,
        _: &str,
        _: &media_access::CancellationToken,
    ) -> Result<media_access::EphemeralAudio, media_access::AccessError> {
        Err(local_only_refusal())
    }
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            player: Arc::default(),
            media: Arc::default(),
            window: Arc::default(),
            spectrum: Mutex::default(),
            root: Arc::default(),
            // As chosen before: Windows' default, followed, and bit-perfect unless you choose
            // otherwise.
            output: Mutex::new({
                let p = engine::settings::load().player;
                (p.output, p.exclusive)
            }),
            endpoint: Arc::default(),
            on_output: Arc::default(),
            catalog: media_access::default_media_access(),
            local_only: Arc::default(),
            offline: Arc::new(LocalOnlyAccess),
            room_buttons: Arc::default(),
            room_now: Arc::default(),
            last: Arc::default(),
            keep: Arc::default(),
        }
    }
}

impl Playback {
    /// Follow what's playing (song changes, play/pause, and every few seconds while playing).
    pub fn follow(&self) -> std::sync::mpsc::Receiver<NowPlaying> {
        let (tx, rx) = std::sync::mpsc::channel();
        if let Ok(mut m) = self.media.lock() {
            m.push(tx);
        }
        rx
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&Player<QueueItem>) -> R) -> Result<R, String> {
        let mut slot = self.player.lock().map_err(|e| e.to_string())?;
        if slot.is_none() {
            let window = self.window.clone();
            let media = self.media.clone();
            let root = self.root.clone();
            let catalog = self.catalog.clone();
            let local_only = self.local_only.clone();
            let room_now = self.room_now.clone();
            let last_status = self.last.clone();
            let endpoint = self.endpoint.clone();
            let on_output = self.on_output.clone();
            // Last next-id we asked the cache to warm (avoid spamming resolve every status tick).
            let mut warmed: Option<String> = None;
            // Plays and where you were are written to disk on a thread of their own: never on
            // the player's, where a slow disk or a busy database would starve the sound.
            let (keep, kept) = std::sync::mpsc::channel::<Keep>();
            if let Ok(mut k) = self.keep.lock() {
                *k = Some(keep.clone());
            }
            let (reporting, offline) = (catalog.clone(), local_only.clone());
            std::thread::Builder::new()
                .name("venuta-listening".into())
                .spawn(move || {
                    let mut tracker = crate::listening::Tracker::default();
                    // Plays (Count plays) and how much was heard (How much I listened) told to
                    // the YouTube Music account, as the account in use's switches allow; off the
                    // listening thread, so a slow answer never holds the next write.
                    tracker.reporter = Some(crate::ytm_signals::reporter(reporting, offline));
                    for k in kept {
                        k.write(&mut tracker);
                    }
                })
                .map_err(|e| e.to_string())?;
            let mut saved: (i64, bool, u64, u32, bool, Repeat) =
                (-2, false, 0, 0, false, Repeat::Off);
            // The order before shuffling, while shuffled (saved with the queue).
            let mut order: Option<Vec<usize>> = None;
            let (id, exclusive) = self.output.lock().map_err(|e| e.to_string())?.clone();
            // The current song and play state, to tell the media overlay only when they change.
            let mut last: (Option<String>, bool, u64) = (None, false, 0);
            // The problem last written to the playback log: once each, not with every status
            // while it stays on screen (that would push out what led up to it).
            let mut logged_error = String::new();
            // Since when the song has waited for its stream partway, to log when it plays on.
            let mut stalled_since: Option<std::time::Instant> = None;
            // The device ran dry this many times so far (said in the log as it grows).
            let mut dry_said: u64 = 0;
            let mut queue: Vec<QueueItem> = Vec::new();
            // Engine 2 while it is built beside engine 1, when the setting `player.engine` says 2.
            let kind = EngineKind::from_setting(engine::settings::load().player.engine);
            *slot = Some(Player::spawn_with(
                kind,
                OutputChoice::Device { id, exclusive },
                move |s: Status<QueueItem>| {
                    if let Some(q) = &s.queue {
                        queue = q.clone();
                        order = s.unshuffled.clone();
                    }
                    // The output the music is on now (the key of its delay): a room is told
                    // when it moves to another one.
                    if let Some(g) = s.signal.as_ref()
                        && let Some(e) = g.endpoint.clone()
                    {
                        let now = PlayingOn {
                            endpoint: e,
                            name: g.device.clone(),
                            kind: g.kind,
                        };
                        let moved = endpoint
                            .lock()
                            .map(|mut cur| {
                                let moved =
                                    cur.as_ref().map(|c| &c.endpoint) != Some(&now.endpoint);
                                *cur = Some(now);
                                moved
                            })
                            .unwrap_or(false);
                        if moved {
                            // In the playback log, for the guided test (docs/qa/devices-live.md).
                            note_play(
                                &format!(
                                    "output: on {} ({:?}, {:?}, {} Hz){}",
                                    g.device,
                                    g.kind,
                                    g.mode,
                                    g.output.rate,
                                    g.note
                                        .as_ref()
                                        .map(|n| format!(": {n}"))
                                        .unwrap_or_default()
                                ),
                                "the player",
                            );
                            if let Ok(hook) = on_output.lock()
                                && let Some(h) = hook.as_ref()
                            {
                                h();
                            }
                        }
                    }
                    // In a room, the windows, the overlay and plays follow the room (`room_tick`);
                    // where you were is still your own program's.
                    let in_room = room_now.lock().is_ok_and(|r| r.is_some());
                    let folder = root.lock().map(|r| r.clone()).unwrap_or_default();
                    let index = s.index.map(|i| i as i64).unwrap_or(-1);
                    {
                        let song = s.index.and_then(|i| queue.get(i));
                        // An online song is counted by its id, with what it was.
                        let online =
                            song.filter(|q| q.is_remote())
                                .map(|q| crate::listening::OnlineSong {
                                    file: q.media_key(),
                                    title: q.title.clone(),
                                    artists: q.artists.clone(),
                                    album: q.album.clone(),
                                    art: q.artwork_url.clone(),
                                });
                        if !in_room {
                            let _ = keep.send(Keep::Heard {
                                folder: folder.clone(),
                                song: song.map(|q| {
                                    if q.is_remote() {
                                        (q.media_key(), String::new())
                                    } else {
                                        (q.path.clone(), q.dir.clone())
                                    }
                                }),
                                online,
                                playing: s.playing,
                                position_ms: s.position_ms,
                                duration_ms: s.duration_ms,
                                from: s.from.clone(),
                            });
                        }
                        // Where you were: when the queue, song or play state changes, and every
                        // ten seconds of playing.
                        let mark = (
                            index,
                            s.playing,
                            s.position_ms / 10_000,
                            (s.volume * 100.0) as u32,
                            s.shuffle,
                            s.repeat,
                        );
                        if !folder.is_empty() && (s.queue.is_some() || mark != saved) {
                            let _ = keep.send(Keep::Place {
                                folder: folder.clone(),
                                queue: s.queue.clone(),
                                index,
                                position_ms: s.position_ms,
                                from: s.from.clone(),
                                volume: s.volume,
                                shuffle: s.shuffle,
                                repeat: s.repeat,
                                order: order.clone(),
                            });
                            saved = mark;
                        }
                    }
                    let item = s.index.and_then(|i| queue.get(i));
                    let key = item.map(|i| i.media_key());
                    let tick = s.position_ms / 5000;
                    if !in_room
                        && (key.clone(), s.playing, tick) != last
                        && let Ok(mut m) = media.lock()
                    {
                        let now = match item {
                            Some(item) => NowPlaying {
                                title: item.title.clone(),
                                artist: item.artists.join(", "),
                                album: item.album.clone(),
                                // Remote items have no on-disk cover; the overlay uses artwork_url.
                                file: item.local_file(),
                                artwork_url: item.artwork_url.clone(),
                                duration_ms: s.duration_ms,
                                playing: s.playing,
                                position_ms: s.position_ms,
                            },
                            None => NowPlaying::default(),
                        };
                        // A follower that has gone stops being told.
                        m.retain(|tx| tx.send(now.clone()).is_ok());
                    }
                    last = (key, s.playing, tick);
                    // A song that stops for its stream partway: when, and when it plays on, so a
                    // freeze in the log can be told from one the download explains.
                    // What the player did for a room, and the device running dry: in the log,
                    // written on the listening thread (never this one, which feeds the sound).
                    for n in &s.sync_notes {
                        let _ = keep.send(Keep::Note(describe_sync(n)));
                    }
                    if s.ran_dry > dry_said {
                        let _ = keep.send(Keep::Note(format!(
                            "the output ran dry {} more time(s), {} in all (a gap: this thread was late)",
                            s.ran_dry - dry_said,
                            s.ran_dry
                        )));
                        dry_said = s.ran_dry;
                    }
                    match (s.stalled, stalled_since) {
                        (true, None) => {
                            stalled_since = Some(std::time::Instant::now());
                            let _ = keep.send(Keep::Note(format!(
                                "waiting for the stream pos_ms={}",
                                s.position_ms
                            )));
                        }
                        (false, Some(since)) => {
                            stalled_since = None;
                            let _ = keep.send(Keep::Note(format!(
                                "done waiting for the stream pos_ms={} after {} ms playing={} error={}",
                                s.position_ms,
                                since.elapsed().as_millis(),
                                s.playing,
                                !s.error.is_empty()
                            )));
                        }
                        _ => {}
                    }
                    if s.error.is_empty() {
                        logged_error.clear();
                    } else if s.error != logged_error {
                        logged_error = s.error.clone();
                        note_play(
                            &format!(
                                "error playing={} buffering={} pos_ms={}",
                                s.playing, s.buffering, s.position_ms
                            ),
                            &s.error,
                        );
                    }
                    // While this song plays, warm the next online id (in-memory resolve cache).
                    if s.playing
                        && !s.buffering
                        && let Some(next) = s.index.and_then(|i| queue.get(i + 1))
                        && next.is_remote()
                        && !local_only.load(std::sync::atomic::Ordering::Relaxed)
                        && let Some(PlayableId::Online { track_id, .. }) = &next.online
                        && warmed.as_deref() != Some(track_id.as_str())
                    {
                        warmed = Some(track_id.clone());
                        let cat = catalog.clone();
                        let id = track_id.clone();
                        std::thread::Builder::new()
                            .name("venuta-warm-next".into())
                            .spawn(move || {
                                let cancel = media_access::CancellationToken::default();
                                let _ = cat.resolve_audio(&id, &cancel);
                            })
                            .ok();
                    }
                    {
                        let status = WindowStatus {
                            queue: s.queue,
                            unshuffled: s.unshuffled,
                            root: folder,
                            index: s.index.map(|i| i as i64).unwrap_or(-1),
                            playing: s.playing,
                            buffering: s.buffering,
                            stalled: s.stalled,
                            position_ms: s.position_ms,
                            duration_ms: s.duration_ms,
                            shuffle: s.shuffle,
                            repeat: s.repeat,
                            volume: s.volume,
                            signal: s.signal,
                            from: s.from,
                            error: s.error,
                            sound: s.sound,
                            stop_after_current: s.stop_after_current,
                            sleep_in_s: s.sleep_in_s,
                            bands: s.bands,
                        };
                        if let Ok(mut l) = last_status.lock() {
                            // The queue is kept: a room's bar is laid over all of it.
                            let queue = status
                                .queue
                                .clone()
                                .or_else(|| l.as_ref().and_then(|x| x.queue.clone()));
                            *l = Some(WindowStatus {
                                queue,
                                ..status.clone()
                            });
                        }
                        if !in_room
                            && let Ok(mut w) = window.lock()
                            && !w.is_empty()
                        {
                            // A window that has closed stops listening.
                            w.retain(|_, ch| ch.send(status.clone()).is_ok());
                        }
                    }
                },
            ));
            if let Some(sound) = crate::listening::saved_sound()
                && let Some(p) = slot.as_ref()
            {
                p.send(Cmd::Sound(sound));
            }
            // The extra delay set for each output (what you hear is that much later).
            if let Some(p) = slot.as_ref() {
                let s = engine::settings::load();
                p.send(Cmd::OutputDelays(s.output_delays()));
                p.send(Cmd::PauseWhenHeadphonesGo(
                    s.player.pause_when_headphones_go,
                ));
            }
        }
        Ok(f(slot.as_ref().expect("just made")))
    }

    fn send(&self, cmd: Cmd<QueueItem>) -> Result<(), String> {
        // A player whose thread has died (a panic on a damaged file) is started again, back
        // where you were and paused, so the controls never go quietly dead.
        let died = {
            let mut slot = self.player.lock().map_err(|e| e.to_string())?;
            let dead = slot.as_ref().is_some_and(|p| !p.is_alive());
            if dead {
                *slot = None;
            }
            dead
        };
        if died {
            let root = self.root.lock().map(|r| r.clone()).unwrap_or_default();
            if !root.is_empty() {
                let _ = self.resume(&root);
            }
        }
        self.with(|p| p.send(cmd))
    }

    /// For the tray: play or pause, next, previous.
    pub fn toggle(&self) {
        note_play("play or pause", "the tray");
        let _ = self.press(Button::Toggle);
    }
    pub fn next(&self) {
        let _ = self.press(Button::Next);
    }
    pub fn previous(&self) {
        let _ = self.press(Button::Previous);
    }

    /// A button pressed: the room's when you're in one, else the player's.
    pub fn press(&self, b: Button) -> Result<(), String> {
        if room_takes(&self.room_buttons, b) {
            return Ok(());
        }
        self.send(match b {
            Button::Toggle => Cmd::Toggle,
            Button::Play => Cmd::Play,
            Button::Pause => Cmd::Pause,
            Button::Next => Cmd::Next,
            Button::Previous => Cmd::Previous,
            Button::Seek(ms) => Cmd::Seek(ms),
        })
    }

    /// Your own queue back where it was set aside, paused. True when there was one.
    pub fn give_back(&self) -> bool {
        let path = aside_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let _ = std::fs::remove_file(&path);
        let Ok(h) = serde_json::from_slice::<Here>(&bytes) else {
            return false;
        };
        if h.items.is_empty() || h.index < 0 {
            return false;
        }
        if let Ok(mut r) = self.root.lock() {
            *r = h.root.clone();
        }
        let items = crate::fresh_gains(&h.root, h.items);
        let items = self.resolve(&h.root, items);
        let _ = self.send(Cmd::Set {
            items,
            index: h.index as usize,
            start_ms: h.position_ms,
            play: false,
            from: h.from,
        });
        if let (true, Some(order)) = (h.shuffle, h.order) {
            let _ = self.send(Cmd::RestoreOrder(order));
        }
        let _ = self.send(Cmd::Options {
            shuffle: h.shuffle,
            repeat: h.repeat,
        });
        true
    }

    /// Listening together: the room takes the buttons (`None`: the player has them again).
    pub fn set_room_buttons(&self, f: Option<Arc<dyn Fn(Button) + Send + Sync>>) {
        if let Ok(mut b) = self.room_buttons.lock() {
            *b = f;
        }
    }
}

/// The library folder the player plays from now (empty before one is opened).
pub fn current_root(app: &tauri::AppHandle) -> String {
    use tauri::Manager;
    app.state::<Playback>()
        .root
        .lock()
        .map(|r| r.clone())
        .unwrap_or_default()
}

/// What asked to play, kept in `<data>/logs/playback.log` (newest last, trimmed as it grows), so a
/// song that starts by itself can be traced to whatever started it.
pub fn note_play(what: &str, from: &str) {
    use std::io::Write;
    let dir = engine::paths::app_data_dir().join("logs");
    let path = dir.join("playback.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::rename(&path, dir.join("playback.old.log"));
    }
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let _ = writeln!(f, "{at} {what} from {from}");
    }
}

/// Media keys and the Windows media overlay: a thread of their own, for the window `hwnd`.
#[cfg(windows)]
pub fn start_media_controls(pb: &Playback, hwnd: isize) {
    use souvlaki::{
        MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition,
        PlatformConfig,
    };
    let rx = pb.follow();
    let player = pb.player.clone();
    let buttons = pb.room_buttons.clone();
    // Windows hears about Venuta only once you've played something this time. A restored
    // queue sits paused, and Windows sends Play to a paused session when a headset reconnects
    // or the flyout is touched: that must never start the music by itself.
    let live = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let live_events = live.clone();
    std::thread::Builder::new()
        .name("venuta-media-keys".into())
        .spawn(move || {
            let Ok(mut controls) = MediaControls::new(PlatformConfig {
                display_name: "Venuta",
                dbus_name: "venuta",
                hwnd: Some(hwnd as *mut std::ffi::c_void),
            }) else {
                return;
            };
            let send = move |cmd: Cmd<QueueItem>| {
                let b = match &cmd {
                    Cmd::Toggle => Some(Button::Toggle),
                    Cmd::Play => Some(Button::Play),
                    Cmd::Pause => Some(Button::Pause),
                    Cmd::Next => Some(Button::Next),
                    Cmd::Previous => Some(Button::Previous),
                    Cmd::Seek(ms) => Some(Button::Seek(*ms)),
                    _ => None,
                };
                if b.is_some_and(|b| room_takes(&buttons, b)) {
                    return;
                }
                if let Ok(p) = player.lock()
                    && let Some(p) = p.as_ref()
                {
                    p.send(cmd);
                }
            };
            let _ = controls.attach(move |e| match e {
                MediaControlEvent::Play | MediaControlEvent::Toggle
                    if !live_events.load(std::sync::atomic::Ordering::Relaxed) =>
                {
                    note_play("ignored (nothing played yet)", "Windows media controls")
                }
                _ if !live_events.load(std::sync::atomic::Ordering::Relaxed) => {}
                MediaControlEvent::Play => {
                    note_play("play", "Windows media controls");
                    send(Cmd::Play)
                }
                MediaControlEvent::Pause => send(Cmd::Pause),
                MediaControlEvent::Toggle => {
                    note_play("play or pause", "Windows media controls");
                    send(Cmd::Toggle)
                }
                MediaControlEvent::Next => send(Cmd::Next),
                MediaControlEvent::Previous => send(Cmd::Previous),
                MediaControlEvent::Stop => send(Cmd::Pause),
                MediaControlEvent::SetPosition(MediaPosition(d)) => {
                    send(Cmd::Seek(d.as_millis() as u64))
                }
                _ => {}
            });
            // The cover file for the song showing (looked up once per song).
            let mut cover: (Option<PathBuf>, Option<String>) = (None, None);
            for now in rx {
                if now.playing {
                    live.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if !live.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = controls.set_playback(MediaPlayback::Stopped);
                    continue;
                }
                // Nothing loaded (stopped at the end of the queue): no title to show.
                if now.title.is_empty() {
                    let _ = controls.set_playback(MediaPlayback::Stopped);
                    cover = (None, None);
                    continue;
                }
                // Cover: a local song's on-disk art; a remote song shows title/artist and, if
                // present, its artwork URL — no filesystem path is required.
                let cover_url = match now.file.clone() {
                    Some(file) => {
                        if cover.0.as_ref() != Some(&file) {
                            // souvlaki strips "file://" and opens the rest as a Windows path,
                            // which must use backslashes only (library roots are kept with "/").
                            let url = engine::art::cover_file_for(&file).map(|c| {
                                format!("file://{}", c.to_string_lossy().replace('/', "\\"))
                            });
                            cover = (Some(file), url);
                        }
                        cover.1.clone()
                    }
                    None => {
                        cover = (None, None);
                        // Asked for at a size that looks sharp in the overlay.
                        now.artwork_url
                            .as_deref()
                            .map(|u| media_access::sized_art(u, 544))
                    }
                };
                let meta = |cover_url| MediaMetadata {
                    title: Some(&now.title),
                    artist: Some(&now.artist),
                    album: Some(&now.album),
                    cover_url,
                    duration: Some(std::time::Duration::from_millis(now.duration_ms)),
                };
                // A cover Windows can't open must not cost the title: try again without it.
                if controls.set_metadata(meta(cover_url.as_deref())).is_err() {
                    cover.1 = None;
                    let _ = controls.set_metadata(meta(None));
                }
                let progress = Some(MediaPosition(std::time::Duration::from_millis(
                    now.position_ms,
                )));
                let _ = controls.set_playback(if now.playing {
                    MediaPlayback::Playing { progress }
                } else {
                    MediaPlayback::Paused { progress }
                });
            }
        })
        .ok();
}

impl Playback {
    /// Bind each item to what it needs to play: a local item to its file under `root`; a remote
    /// item to the catalog handle that resolves its stream at open time. Also fills the
    /// UI-facing availability (`Local` | `Online`, or `LocalPreferred` for a playlist's online
    /// song playing from your file). No stream URL is ever stored here.
    pub(crate) fn resolve(&self, root: &str, items: Vec<QueueItem>) -> Vec<QueueItem> {
        let root = PathBuf::from(root);
        items
            .into_iter()
            .map(|mut it| {
                if it.is_remote() {
                    it.file = PathBuf::new();
                    it.media = MediaRef(Some(self.catalog()));
                    it.availability = Some(Availability::Online);
                } else {
                    it.file = root.join(&it.path);
                    if it.availability != Some(Availability::LocalPreferred) {
                        it.availability = Some(Availability::Local);
                    }
                }
                it
            })
            .collect()
    }

    /// The catalog remote songs play from, for searching it (the same one, so what you find
    /// is what plays).
    pub fn catalog(&self) -> Arc<dyn MediaAccess> {
        if self.local_only.load(std::sync::atomic::Ordering::Relaxed) {
            self.offline.clone()
        } else {
            self.catalog.clone()
        }
    }

    /// The catalog itself, Local only or not: for its settings (region, the sign-in), which
    /// must hold when online comes back. Never for a request.
    pub fn online_catalog(&self) -> Arc<dyn MediaAccess> {
        self.catalog.clone()
    }

    /// The output chosen (its id), or `None` for the Windows default.
    pub fn output_id(&self) -> Option<String> {
        self.output.lock().ok().and_then(|o| o.0.clone())
    }

    /// The output the music is on (as the player last said), else the one the choice would
    /// play on now.
    pub fn current_output(&self) -> Option<PlayingOn> {
        if let Some(p) = self.endpoint.lock().ok().and_then(|e| e.clone()) {
            return Some(p);
        }
        let chosen = self.output_id();
        #[cfg(windows)]
        if let Ok((eps, default)) = player::wasapi_devices::list_endpoints() {
            return player::devices::resolve(chosen.as_deref(), &eps, default.as_deref(), &[]).map(
                |r| PlayingOn {
                    endpoint: r.id,
                    name: r.entry.name,
                    kind: Some(r.entry.kind),
                },
            );
        }
        chosen.map(|endpoint| PlayingOn {
            name: endpoint.clone(),
            endpoint,
            kind: None,
        })
    }

    /// The endpoint the music is on, or would be on: the key of the output's delay.
    pub fn current_endpoint(&self) -> Option<String> {
        self.current_output().map(|p| p.endpoint)
    }

    /// Call `f` whenever the music moves to another endpoint.
    pub fn on_output_change(&self, f: impl Fn() + Send + 'static) {
        if let Ok(mut h) = self.on_output.lock() {
            *h = Some(Box::new(f));
        }
    }

    /// Set (or clear) an output's extra delay: kept in Settings, told to the player (the place
    /// it reports, and so lyrics, follow it at once), and to a room.
    pub fn set_output_delay(&self, endpoint: &str, ms: Option<i32>) -> Result<(), String> {
        let mut s = engine::settings::load();
        s.set_output_delay(endpoint, ms);
        engine::settings::save(&s)?;
        let delays = s.output_delays();
        if let Ok(slot) = self.player.lock()
            && let Some(p) = slot.as_ref()
        {
            p.send(Cmd::OutputDelays(delays));
        }
        if let Ok(hook) = self.on_output.lock()
            && let Some(h) = hook.as_ref()
        {
            h();
        }
        Ok(())
    }

    /// Listening together: what the room plays now (`None`: your own program shows again),
    /// told to every window at once.
    pub fn set_room_now(&self, now: Option<RoomNow>) {
        let gone = now.is_none();
        if let Ok(mut r) = self.room_now.lock() {
            *r = now;
        }
        if gone {
            // Your own program's last status, the queue with it.
            let last = self.last.lock().ok().and_then(|l| l.clone());
            if let (Some(st), Ok(mut w)) = (last, self.window.lock()) {
                w.retain(|_, ch| ch.send(st.clone()).is_ok());
            }
            if let Ok(mut m) = self.media.lock() {
                m.retain(|tx| tx.send(NowPlaying::default()).is_ok());
            }
        } else {
            self.room_tick(true);
        }
    }

    /// Listening together: the room's now playing to every window, the media overlay and the
    /// plays (ten times a second while in a room; `queue`: with the room's queue).
    pub fn room_tick(&self, queue: bool) {
        let Some(now) = self.room_now.lock().ok().and_then(|r| r.clone()) else {
            return;
        };
        let base = self.last.lock().ok().and_then(|l| l.clone());
        let item = usize::try_from(now.index)
            .ok()
            .and_then(|i| now.items.get(i));
        let st = WindowStatus {
            queue: queue.then(|| now.items.clone()),
            unshuffled: None,
            root: base.as_ref().map(|b| b.root.clone()).unwrap_or_default(),
            index: now.index,
            playing: now.playing,
            buffering: now.buffering,
            stalled: false,
            position_ms: now.position_ms,
            duration_ms: now.duration_ms,
            shuffle: false,
            repeat: Repeat::Off,
            volume: base.as_ref().map_or(1.0, |b| b.volume),
            signal: base.as_ref().and_then(|b| b.signal.clone()),
            from: now.from.clone(),
            error: String::new(),
            sound: base.as_ref().map(|b| b.sound.clone()).unwrap_or_default(),
            stop_after_current: false,
            sleep_in_s: base.as_ref().and_then(|b| b.sleep_in_s),
            bands: base.as_ref().and_then(|b| b.bands.clone()),
        };
        if let Ok(mut w) = self.window.lock() {
            w.retain(|_, ch| ch.send(st.clone()).is_ok());
        }
        if let Some(item) = item {
            if let Ok(mut m) = self.media.lock() {
                let np = NowPlaying {
                    title: item.title.clone(),
                    artist: item.artists.join(", "),
                    album: item.album.clone(),
                    file: None,
                    artwork_url: item.artwork_url.clone(),
                    duration_ms: now.duration_ms,
                    playing: now.playing,
                    position_ms: now.position_ms,
                };
                m.retain(|tx| tx.send(np.clone()).is_ok());
            }
            // Room songs count as heard (J4), marked as from the room.
            if let Ok(k) = self.keep.lock()
                && let Some(k) = k.as_ref()
            {
                let folder = self.root.lock().map(|r| r.clone()).unwrap_or_default();
                let catalog = matches!(&item.online, Some(PlayableId::Online { provider, .. })
                    if provider == crate::together::CATALOG_PROVIDER);
                // An online song by its id; one of your own files by its path; a song only the
                // host has isn't yours to count.
                let (song, online) = if catalog {
                    let key = item.media_key();
                    (
                        Some((key.clone(), String::new())),
                        Some(crate::listening::OnlineSong {
                            file: key,
                            title: item.title.clone(),
                            artists: item.artists.clone(),
                            album: item.album.clone(),
                            art: item.artwork_url.clone(),
                        }),
                    )
                } else if !item.path.is_empty() {
                    (Some((item.path.clone(), item.dir.clone())), None)
                } else {
                    (None, None)
                };
                let _ = k.send(Keep::Heard {
                    folder,
                    song,
                    online,
                    playing: now.playing,
                    position_ms: now.position_ms,
                    duration_ms: now.duration_ms,
                    from: now.from.clone(),
                });
            }
        }
    }

    /// What the room plays now, set without telling the windows (`room_tick` does).
    pub fn set_room_now_quiet(&self, now: Option<RoomNow>) {
        if let Ok(mut r) = self.room_now.lock() {
            *r = now;
        }
    }

    /// A room's now playing is shown (you're in a room).
    pub fn in_room(&self) -> bool {
        self.room_now.lock().is_ok_and(|r| r.is_some())
    }

    /// Your volume, 0 to 100 (the host's speaker's, in a room).
    pub fn volume_percent(&self) -> Option<u8> {
        let l = self.last.lock().ok()?;
        l.as_ref()
            .map(|s| (s.volume.clamp(0.0, 1.0) * 100.0).round() as u8)
    }

    /// Whether the player plays with engine 2 (rooms play only through it).
    pub fn engine_two(&self) -> bool {
        self.with(|p| p.engine() == EngineKind::Two)
            .unwrap_or(false)
    }

    /// Local only on or off (Settings, Listening).
    pub fn set_local_only(&self, on: bool) {
        self.local_only
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The sound tools: equalizer, bass and treble, preamp, levelling, limiter. Kept with your
/// library, so they're the same next time.
#[tauri::command]
pub fn player_sound(sound: player::dsp::Sound, pb: State<'_, Playback>) -> Result<(), String> {
    crate::listening::save_sound(&sound);
    pb.send(Cmd::Sound(sound))
}

#[tauri::command]
pub fn player_stop_after(on: bool, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::StopAfterCurrent(on))
}

/// Pause after `minutes` (fading out the last ten seconds); `None` cancels.
#[tauri::command]
pub fn player_sleep(minutes: Option<f64>, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::SleepIn(minutes.map(|m| {
        std::time::Duration::from_secs_f64((m * 60.0).max(0.0))
    })))
}

/// Pick up where you left off in this folder: the saved queue, on the song and at the place
/// you stopped, paused.
#[tauri::command]
pub fn player_resume(root: String, pb: State<'_, Playback>) -> Result<bool, String> {
    pb.resume(&root)
}

impl Playback {
    fn resume(&self, root: &str) -> Result<bool, String> {
        // Closed while in a room: your own queue, not the room's song.
        if self.give_back() {
            return Ok(true);
        }
        let pb = self;
        let root = root.to_owned();
        let Some((queue, place)) = crate::listening::saved_place() else {
            return Ok(false);
        };
        let same = place.get("root").and_then(|r| r.as_str()).is_some_and(|r| {
            engine::paths::canonical_root_string(Path::new(r))
                == engine::paths::canonical_root_string(Path::new(&root))
        });
        let items: Vec<QueueItem> = serde_json::from_value(queue).unwrap_or_default();
        let index = place.get("index").and_then(|i| i.as_i64()).unwrap_or(-1);
        if !same || items.is_empty() || index < 0 {
            return Ok(false);
        }
        let items = crate::fresh_gains(&root, items);
        *pb.root.lock().map_err(|e| e.to_string())? = root.clone();
        let items = pb.resolve(&root, items);
        pb.send(Cmd::Set {
            items,
            index: index as usize,
            start_ms: place
                .get("position_ms")
                .and_then(|p| p.as_u64())
                .unwrap_or(0),
            play: false,
            from: place
                .get("from")
                .and_then(|f| f.as_str())
                .unwrap_or_default()
                .to_owned(),
        })?;
        // The volume you left it at, too.
        if let Some(v) = place.get("volume").and_then(|v| v.as_f64()) {
            pb.send(Cmd::Volume(v as f32))?;
        }
        // And shuffle and repeat, the shuffled order as it was (unshuffling still goes back).
        let order: Option<Vec<usize>> = place
            .get("order")
            .and_then(|o| serde_json::from_value(o.clone()).ok());
        let shuffle = place
            .get("shuffle")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);
        if let (true, Some(order)) = (shuffle, order) {
            pb.send(Cmd::RestoreOrder(order))?;
        }
        let repeat: Repeat = place
            .get("repeat")
            .and_then(|r| serde_json::from_value(r.clone()).ok())
            .unwrap_or_default();
        pb.send(Cmd::Options { shuffle, repeat })?;
        Ok(true)
    }
}

/// A line for the playback log about what the player did to stay with a room.
fn describe_sync(n: &player::sync::SyncNote) -> String {
    use player::sync::SyncNote;
    match n {
        SyncNote::Started { position_ms } => {
            format!("sync: the room's song started at {position_ms} ms, on its frame")
        }
        SyncNote::Seek {
            from_ms,
            to_ms,
            faded,
        } => format!(
            "sync: lined up from {from_ms} ms to {to_ms} ms ({})",
            if *faded {
                "faded out and in"
            } else {
                "from silence"
            }
        ),
        SyncNote::Fine { frames } if *frames >= 0 => {
            format!("sync: closed up {frames} frames just after a seek")
        }
        SyncNote::Fine { frames } => {
            format!(
                "sync: {} frames of silence first, just after a seek",
                -frames
            )
        }
        SyncNote::Rate { ppm: 0 } => "sync: back to the song's own rate (bit-perfect)".into(),
        SyncNote::Rate { ppm } => format!("sync: rate {ppm:+} ppm (one room, keeping in step)"),
    }
}

/// What the listening thread writes: a moment heard (plays are counted from these), where
/// you were and how it plays, or a line for the playback log.
enum Keep {
    Heard {
        folder: String,
        song: Option<(String, String)>,
        online: Option<crate::listening::OnlineSong>,
        playing: bool,
        position_ms: u64,
        duration_ms: u64,
        from: String,
    },
    Note(String),
    Place {
        folder: String,
        queue: Option<Vec<QueueItem>>,
        index: i64,
        position_ms: u64,
        from: String,
        volume: f32,
        shuffle: bool,
        repeat: Repeat,
        order: Option<Vec<usize>>,
    },
}

impl Keep {
    fn write(self, tracker: &mut crate::listening::Tracker) {
        match self {
            Keep::Note(what) => note_play(&what, "the player"),
            Keep::Heard {
                folder,
                song,
                online,
                playing,
                position_ms,
                duration_ms,
                from,
            } => tracker.update(
                &folder,
                song.as_ref().map(|(p, d)| (p.as_str(), d.as_str())),
                online.as_ref(),
                playing,
                position_ms,
                duration_ms,
                &from,
            ),
            Keep::Place {
                folder,
                queue,
                index,
                position_ms,
                from,
                volume,
                shuffle,
                repeat,
                order,
            } => {
                let q = queue.as_ref().and_then(|q| serde_json::to_string(q).ok());
                tracker.save_place(
                    &folder,
                    q.as_deref(),
                    index,
                    position_ms,
                    &from,
                    &crate::listening::HowItPlays {
                        volume,
                        shuffle,
                        repeat,
                        order: order.as_deref(),
                    },
                );
            }
        }
    }
}

/// Start receiving the player's status (the window calls this once).
#[tauri::command]
pub fn player_subscribe(
    on_state: Channel<WindowStatus>,
    webview: tauri::Webview,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    pb.window
        .lock()
        .map_err(|e| e.to_string())?
        .insert(webview.label().to_string(), on_state);
    // Send the current state at once (and the queue with it).
    pb.send(Cmd::Refresh)
}

#[tauri::command]
pub fn player_play(
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    from: String,
    pb: State<'_, Playback>,
    window: tauri::Window,
) -> Result<(), String> {
    note_play("play a list", &format!("the {} window", window.label()));
    *pb.root.lock().map_err(|e| e.to_string())? = root.clone();
    let items = pb.resolve(&root, items);
    pb.send(Cmd::Set {
        items,
        index,
        start_ms: 0,
        play: true,
        from,
    })
}

/// Put a queue back as it was (Undo after playing something else replaced it): the same songs
/// in the same order, on the same song at the same moment, playing or paused as it was, with
/// shuffle and repeat, and the order before shuffling so turning shuffle off still goes back.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn player_restore(
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    position_ms: u64,
    play: bool,
    from: String,
    shuffle: bool,
    repeat: Repeat,
    order: Option<Vec<usize>>,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    *pb.root.lock().map_err(|e| e.to_string())? = root.clone();
    let items = pb.resolve(&root, items);
    pb.send(Cmd::Set {
        items,
        index,
        start_ms: position_ms,
        play,
        from,
    })?;
    if let (true, Some(order)) = (shuffle, order) {
        pb.send(Cmd::RestoreOrder(order))?;
    }
    pb.send(Cmd::Options { shuffle, repeat })
}

/// The source the queue came from has a new list (a playlist edited while it plays): the song
/// playing now (at `index` in `items`) plays on, and the rest follows the new list.
#[tauri::command]
pub fn player_follow(
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    let items = pb.resolve(&root, items);
    pb.send(Cmd::Follow { items, index })
}

#[tauri::command]
pub fn player_toggle(pb: State<'_, Playback>, window: tauri::Window) -> Result<(), String> {
    note_play("play or pause", &format!("the {} window", window.label()));
    pb.press(Button::Toggle)
}

#[tauri::command]
pub fn player_next(pb: State<'_, Playback>) -> Result<(), String> {
    pb.press(Button::Next)
}

#[tauri::command]
pub fn player_previous(pb: State<'_, Playback>) -> Result<(), String> {
    pb.press(Button::Previous)
}

#[tauri::command]
pub fn player_seek(ms: u64, pb: State<'_, Playback>) -> Result<(), String> {
    pb.press(Button::Seek(ms))
}

#[tauri::command]
pub fn player_volume(volume: f32, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Volume(volume))
}

#[tauri::command]
pub fn player_options(
    shuffle: bool,
    repeat: Repeat,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    pb.send(Cmd::Options { shuffle, repeat })
}

#[tauri::command]
pub fn player_enqueue(
    root: String,
    items: Vec<QueueItem>,
    next: bool,
    pb: State<'_, Playback>,
    window: tauri::Window,
) -> Result<(), String> {
    note_play(
        "add to the queue",
        &format!("the {} window", window.label()),
    );
    if let Ok(mut r) = pb.root.lock()
        && r.is_empty()
    {
        *r = root.clone();
    }
    let items = pb.resolve(&root, items);
    pb.send(Cmd::Insert { items, next })
}

#[tauri::command]
pub fn player_remove(index: usize, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Remove(index))
}

#[tauri::command]
pub fn player_move(from: usize, to: usize, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Move(from, to))
}

#[tauri::command]
pub fn player_jump(
    index: usize,
    pb: State<'_, Playback>,
    window: tauri::Window,
) -> Result<(), String> {
    note_play("jump to a song", &format!("the {} window", window.label()));
    pb.send(Cmd::Jump(index))
}

/// One device as the picker lists it (`docs/design/devices.md`): what it is, and its delay and
/// the exclusive formats it takes, once known.
#[derive(Serialize)]
pub struct DeviceView {
    #[serde(flatten)]
    entry: player::devices::OutputEntry,
    /// The extra delay set for it, ms (`None`: not set).
    delay_ms: Option<i32>,
    /// A starting value to offer while not set (Bluetooth).
    suggested_delay_ms: Option<i32>,
    /// Exclusive mode suits it (never Bluetooth or a virtual device).
    exclusive_suits: bool,
    /// The rates it takes in exclusive mode, and its deepest bits, once it has been asked.
    exclusive_rates: Vec<u32>,
    exclusive_bits: u16,
}

#[derive(Serialize)]
pub struct OutputChoices {
    /// Every device connected, grouped by kind (and the chosen one when it is away).
    devices: Vec<DeviceView>,
    /// The chosen device's endpoint (`None`: follow the Windows default).
    device: Option<String>,
    exclusive: bool,
    /// The endpoint the music is on now (the chosen one, its stereo twin, the default or a
    /// fallback), once something has played.
    playing: Option<String>,
}

/// The devices listed for the picker, with the chosen one (away or not) and each one's delay.
fn device_views(chosen: Option<&str>) -> Result<Vec<DeviceView>, String> {
    #[cfg(windows)]
    let (eps, default) = player::wasapi_devices::list_endpoints()?;
    #[cfg(not(windows))]
    let (eps, default): (Vec<player::devices::Endpoint>, Option<String>) = (Vec::new(), None);
    let delays = engine::settings::load().output_delays();
    let probes = player::devices::ProbeCache::global();
    Ok(player::devices::entries(&eps, default.as_deref(), chosen)
        .into_iter()
        .map(|entry| {
            let (rates, bits) = probes.summary(&entry.id).unwrap_or_default();
            DeviceView {
                delay_ms: delays.get(&entry.id).copied(),
                suggested_delay_ms: player::devices::suggested_delay_ms(
                    entry.kind,
                    entry.hands_free_only,
                ),
                exclusive_suits: player::devices::plays_exclusive(entry.kind),
                exclusive_rates: rates,
                exclusive_bits: bits,
                entry,
            }
        })
        .collect())
}

/// Sound outputs, and which one plays.
#[tauri::command]
pub async fn player_devices(pb: State<'_, Playback>) -> Result<OutputChoices, String> {
    let (device, exclusive) = pb.output.lock().map_err(|e| e.to_string())?.clone();
    let playing = pb
        .endpoint
        .lock()
        .ok()
        .and_then(|e| e.as_ref().map(|p| p.endpoint.clone()));
    let chosen = device.clone();
    let devices = tauri::async_runtime::spawn_blocking(move || device_views(chosen.as_deref()))
        .await
        .map_err(|e| e.to_string())??;
    Ok(OutputChoices {
        devices,
        device,
        exclusive,
        playing,
    })
}

/// Play through another device (`None`: follow Windows' default), or switch between exclusive
/// (bit-perfect) and shared. Remembered, with the device's name for when it is away.
#[tauri::command]
pub fn player_output(
    device: Option<String>,
    name: Option<String>,
    exclusive: bool,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    *pb.output.lock().map_err(|e| e.to_string())? = (device.clone(), exclusive);
    let mut s = engine::settings::load();
    if s.player.output != device {
        s.player.output_name = name.unwrap_or_default();
    }
    s.player.output = device.clone();
    s.player.exclusive = exclusive;
    engine::settings::save(&s)?;
    pb.send(Cmd::Output(OutputChoice::Device {
        id: device,
        exclusive,
    }))
}

/// Set (or, `None`, clear) an output's extra delay, -500 to +500 ms: how much later than the
/// device says you hear it (Bluetooth, a receiver). Lyrics and rooms follow it.
#[tauri::command]
pub fn player_set_output_delay(
    endpoint: String,
    ms: Option<i32>,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    pb.set_output_delay(&endpoint, ms)
}

/// A song's words: timed lines (from a synced .lrc or LYRICS tag), or plain text.
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Lyrics {
    /// The lines in order, each with its words' times and its translation when the file has
    /// them (the lyrics model, `engine::lyrics::LyricLine`). Empty when the lyrics have no times.
    pub lines: Vec<engine::lyrics::LyricLine>,
    /// Lyrics without times, as written.
    pub text: String,
}

/// A song's lyrics text (its .lrc, or its tag) as lyrics to show: word times and translations
/// kept all the way to the screen.
fn lyrics_of(text: String) -> Lyrics {
    let mut lines = engine::lyrics::read_lrc(&text).lines;
    engine::lyrics::mark_source(&mut lines, "file");
    engine::lyrics::add_pronunciation_lines(&mut lines, None);
    if lines.is_empty() {
        Lyrics { lines, text }
    } else {
        Lyrics {
            lines,
            text: String::new(),
        }
    }
}

/// Timed lines `[ms, line]`, read the way Venuta reads lyrics everywhere (`engine::lyrics`):
/// word stamps are timing, not words; headers and the offset are understood.
fn parse_lrc(text: &str) -> Vec<(u64, String)> {
    engine::lyrics::read_lrc(text)
        .lines
        .into_iter()
        .map(|l| (l.start_ms, l.text))
        .collect()
}

/// The words of a song in the open folder.
#[tauri::command]
pub async fn song_lyrics(
    app: tauri::AppHandle,
    root: String,
    path: String,
) -> Result<Lyrics, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = PathBuf::from(&root).join(&path);
        let lrc = file.with_extension("lrc");
        let text = engine::text::read_text(&lrc).ok().or_else(|| {
            let kind = engine::media::FileKind::from_path(&file);
            engine::media::read_facts(&file, kind).ok().and_then(|f| {
                let pick = |k: &str| {
                    f.tags
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(k))
                        .map(|(_, v)| v.clone())
                };
                pick("LYRICS").or_else(|| pick("UNSYNCEDLYRICS"))
            })
        });
        let Some(text) = text else {
            return Ok(Lyrics::default());
        };
        let mut lyrics = lyrics_of(text);
        crate::japanese_dictionary::fetch_if_waiting(&app, &lyrics.lines, None);
        // Remembered for Translation (the views' key for a file), with any made here.
        engine::translation::Translations::global()
            .serve(&format!("file:{path}"), &mut lyrics.lines);
        Ok(lyrics)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One song's words for searching: its lines without times, each once.
#[derive(Serialize)]
pub struct SongWords {
    pub path: String,
    pub lines: Vec<String>,
}

/// Lines to search in a song's lyrics text (timed or not), each line once, in order.
fn words_of(text: &str) -> Vec<String> {
    let timed = parse_lrc(text);
    let lines: Vec<String> = if timed.is_empty() {
        text.lines().map(|l| l.trim().to_owned()).collect()
    } else {
        timed.into_iter().map(|(_, l)| l).collect()
    };
    let mut seen = std::collections::HashSet::new();
    lines
        .into_iter()
        .filter(|l| !l.is_empty() && seen.insert(l.to_lowercase()))
        .collect()
}

/// The words of every song in the open folder that has any (a .lrc beside it, or lyrics in
/// its tags), for searching lyrics.
#[tauri::command]
pub async fn all_lyrics(root: String) -> Result<Vec<SongWords>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let rootp = PathBuf::from(&root);
        let index = crate::open_index(&rootp)?;
        let lib = crate::libcache::indexed(&index, &rootp)?;
        let mut out = Vec::new();
        for t in &lib.tracks {
            let lrc = std::path::Path::new(&t.path).with_extension("lrc");
            let lrc = lrc.to_string_lossy().replace('\\', "/");
            let text = if lib.lyrics_files.contains(&lrc) {
                engine::text::read_text(&rootp.join(&lrc)).ok()
            } else {
                None
            }
            .or_else(|| {
                t.first("LYRICS")
                    .or_else(|| t.first("UNSYNCEDLYRICS"))
                    .map(str::to_owned)
            });
            if let Some(text) = text {
                let lines = words_of(&text);
                if !lines.is_empty() {
                    out.push(SongWords {
                        path: t.path.clone(),
                        lines,
                    });
                }
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{QueueItem, lyrics_of, parse_lrc, words_of};

    // An online song's album rides with it through the queue (the window sends it, the queue
    // sends it back), so the queue's Go to album can open it. Older queues without one still read.
    #[test]
    fn a_queued_online_song_keeps_its_album() {
        let item: QueueItem = serde_json::from_value(serde_json::json!({
            "title": "A",
            "album": "Album A",
            "album_id": "MPREalbumA",
            "online": { "kind": "online", "provider": "remote_catalog", "track_id": "aaaaaaaaaaa" },
        }))
        .unwrap();
        assert_eq!(item.album_id.as_deref(), Some("MPREalbumA"));
        let back = serde_json::to_value(&item).unwrap();
        assert_eq!(back["album_id"], "MPREalbumA");

        let old: QueueItem = serde_json::from_value(serde_json::json!({ "title": "B" })).unwrap();
        assert_eq!(old.album_id, None);
        assert!(
            serde_json::to_value(&old)
                .unwrap()
                .get("album_id")
                .is_none()
        );
    }

    #[test]
    fn words_to_search_have_no_times_and_no_repeats() {
        let w = words_of(
            "[ar:X]\n[00:01.00]Kesariya tera\n[00:05.00]ishq hai piya\n[00:09.00]Kesariya Tera\n[00:12.00]",
        );
        assert_eq!(w, vec!["Kesariya tera", "ishq hai piya"]);
        assert_eq!(words_of("one\n\ntwo\none"), vec!["one", "two"]);
    }

    #[test]
    fn your_word_timed_lrc_shows_words_not_stamps() {
        let l = parse_lrc("[00:05.00]<00:05.00>Word <00:05.40>by <00:05.90>word");
        assert_eq!(l, vec![(5000, "Word by word".into())]);
    }

    #[test]
    fn your_word_timed_lrc_keeps_its_word_times_and_translations() {
        let l = lyrics_of(
            "[00:05.00]<00:05.00>Word <00:05.40>by <00:05.90>word
[00:05.00]Palabra
[00:09.00]Next"
                .into(),
        );
        assert_eq!(l.text, "");
        assert_eq!(l.lines.len(), 2);
        let words: Vec<(u64, u64, &str)> = l.lines[0]
            .words
            .iter()
            .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
            .collect();
        assert_eq!(
            words,
            [
                (5000, 5400, "Word "),
                (5400, 5900, "by "),
                (5900, 8900, "word")
            ]
        );
        let t = l.lines[0].translation.as_ref().unwrap();
        assert_eq!(t.text, "Palabra");
        assert_eq!(
            t.source,
            engine::lyrics::SecondSource::Lyrics("file".into())
        );
        // Plain words stay as written.
        assert_eq!(
            lyrics_of(
                "One
Two"
                .into()
            )
            .text,
            "One
Two"
        );
    }

    #[test]
    fn timed_lines_and_headers() {
        let l =
            parse_lrc("[ar:Someone]\n[00:01.25]First\n[00:10.00][01:00.50]Again\n\n[00:05.5]Mid");
        assert_eq!(
            l,
            vec![
                (1250, "First".into()),
                (5500, "Mid".into()),
                (10_000, "Again".into()),
                (60_500, "Again".into())
            ]
        );
    }

    #[test]
    fn every_room_correction_reads_as_a_line_in_the_log() {
        use player::sync::SyncNote;
        let lines: Vec<String> = [
            SyncNote::Started {
                position_ms: 12_000,
            },
            SyncNote::Seek {
                from_ms: 1_000,
                to_ms: 1_900,
                faded: true,
            },
            SyncNote::Fine { frames: 37 },
            SyncNote::Fine { frames: -12 },
            SyncNote::Rate { ppm: 300 },
            SyncNote::Rate { ppm: 0 },
        ]
        .iter()
        .map(super::describe_sync)
        .collect();
        assert_eq!(
            lines,
            [
                "sync: the room's song started at 12000 ms, on its frame",
                "sync: lined up from 1000 ms to 1900 ms (faded out and in)",
                "sync: closed up 37 frames just after a seek",
                "sync: 12 frames of silence first, just after a seek",
                "sync: rate +300 ppm (one room, keeping in step)",
                "sync: back to the song's own rate (bit-perfect)",
            ]
        );
    }
}

/// Prove a FLAC plays bit-perfect: through the player into memory, against the file's own
/// fingerprint of its samples.
#[tauri::command]
pub async fn player_verify(
    root: String,
    path: String,
) -> Result<player::verify::Verification, String> {
    let file = PathBuf::from(root).join(path);
    tauri::async_runtime::spawn_blocking(move || player::verify::verify_bit_perfect(&file))
        .await
        .map_err(|e| e.to_string())?
}

/// One online song's shape is read at a time, so reading never competes with playing.
static READING_ONLINE_SHAPE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// An online song's shape for the seek bar (600 loudness levels, 0–255), read once from its
/// smallest stream and kept by its id in `.venuta/waves-online`. Not with data saver on: it
/// would fetch the song a second time.
#[tauri::command]
pub async fn online_waveform(pb: State<'_, Playback>, track_id: String) -> Result<Vec<u8>, String> {
    let catalog = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        let id = track_id.trim().to_owned();
        if id.is_empty() {
            return Err("which song?".to_owned());
        }
        let key = blake3::hash(format!("remote_catalog|{id}|600").as_bytes()).to_hex();
        let dir = engine::paths::app_data_dir().join("waves-online");
        let cached = dir.join(format!("{}.bin", &key[..24]));
        if let Ok(b) = std::fs::read(&cached)
            && b.len() == 600
        {
            return Ok(b);
        }
        if engine::settings::load().listening.stream_quality == "low" {
            return Err("data saver is on".to_owned());
        }
        let _one = READING_ONLINE_SHAPE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Read while waiting its turn by another call for the same song.
        if let Ok(b) = std::fs::read(&cached)
            && b.len() == 600
        {
            return Ok(b);
        }
        let audio = catalog
            .resolve_audio_small(&id, &media_access::CancellationToken::default())
            .map_err(|e| e.to_string())?;
        let w = player::waveform::waveform_remote(audio, 600)?;
        // A shape of all silence is a read that went wrong: asked again next time, not kept.
        if w.iter().any(|&v| v > 0) {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&cached, &w);
        }
        Ok(w)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A song's shape for the seek bar (600 loudness levels, 0–255), made once and kept in `.venuta/waves-rms`.
#[tauri::command]
pub async fn song_waveform(root: String, path: String) -> Result<Vec<u8>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = PathBuf::from(root).join(path);
        let meta = std::fs::metadata(&file).map_err(|e| e.to_string())?;
        let stamp = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let key = blake3::hash(
            format!("{}|{}|{stamp}|600", file.to_string_lossy(), meta.len()).as_bytes(),
        )
        .to_hex();
        // "waves-rms": loudness per slice (the older "waves" held peaks).
        let dir = engine::paths::app_data_dir().join("waves-rms");
        let cached = dir.join(format!("{}.bin", &key[..24]));
        if let Ok(b) = std::fs::read(&cached)
            && b.len() == 600
        {
            return Ok(b);
        }
        let w = player::waveform::waveform(&file, 600)?;
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&cached, &w);
        Ok(w)
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- The mini player ---------------------------------------------------------------------------
//
// One window, whatever asks for it (its button, Ctrl+M in either window, the tray, the command
// palette) and however quickly, kept as the lyrics windows are (one_window): each opening and
// closing waits its turn, each window has a label of its own ("mini-3"), and anything of it still
// about is cleared away before another is made. The main window steps aside in the same turn as
// the mini player shows, and comes back in the same turn as it goes, so presses in quick
// succession can never leave the two the wrong way round.

/// The mini player's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Mini;

impl WindowKind for Mini {
    const ALL: &'static [Mini] = &[Mini];
    const WHOSE: &'static str = "the mini player";
    fn label(self) -> &'static str {
        "mini"
    }
    fn title(self) -> &'static str {
        "Venuta"
    }
}

/// The mini player's window: one at a time, whoever asks and however quickly.
pub(crate) type MiniShelf = one_window::Shelf<Mini>;

/// The mini player as Tauri has it.
struct MiniHost<'a>(&'a tauri::AppHandle);

impl MiniHost<'_> {
    /// The main window steps aside while the mini player shows.
    fn step_aside(&self) {
        use tauri::Manager;
        if let Some(m) = self.0.get_webview_window("main") {
            let _ = m.minimize();
        }
    }
}

impl Host<Mini> for MiniHost<'_> {
    type Place = ();

    fn has(&self, label: &str) -> bool {
        one_window::has(self.0, label)
    }

    fn labels(&self) -> Vec<String> {
        one_window::labels(self.0)
    }

    async fn make(&self, kind: Mini, label: &str, _place: &()) -> Result<(), String> {
        use tauri::Manager;
        let app = self.0;
        let (w, h) = (380.0, 132.0);
        // Bottom right of the screen the main window is on, clear of the taskbar.
        let place = app
            .get_webview_window("main")
            .and_then(|m| m.current_monitor().ok().flatten())
            .map(|mon| {
                let area = mon.work_area();
                let scale = mon.scale_factor();
                let right = (area.position.x as f64 + area.size.width as f64) / scale;
                let bottom = (area.position.y as f64 + area.size.height as f64) / scale;
                (right - w - 24.0, bottom - h - 24.0)
            });
        let mut b = tauri::WebviewWindowBuilder::new(
            app,
            label,
            tauri::WebviewUrl::App("index.html#mini".into()),
        )
        .title(kind.title())
        .inner_size(w, h)
        .min_inner_size(220.0, 56.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .resizable(true)
        .effects(tauri::utils::config::WindowEffectsConfig {
            effects: vec![tauri::window::Effect::Acrylic],
            ..Default::default()
        });
        if let Some((x, y)) = place {
            b = b.position(x, y);
        }
        b.build().map_err(|e| e.to_string())?;
        crate::app_icon::apply(app);
        self.step_aside();
        Ok(())
    }

    fn bring(&self, _kind: Mini, label: &str, _place: &()) {
        use tauri::Manager;
        if let Some(w) = self.0.get_webview_window(label) {
            let _ = w.show();
            let _ = w.set_focus();
        }
        self.step_aside();
    }

    fn end(&self, label: &str) {
        one_window::end(self.0, label);
    }

    async fn end_lost(&self, kind: Mini) -> usize {
        one_window::end_lost(self.0, kind.title()).await
    }

    /// The mini player gone (or asked to go when it wasn't there): the main window comes back.
    fn closed(&self, _kind: Mini) {
        use tauri::Manager;
        if let Some(m) = self.0.get_webview_window("main") {
            let _ = m.unminimize();
            let _ = m.show();
            let _ = m.set_focus();
        }
    }
}

/// A window gone, however: when it was the mini player's window now, it is forgotten (an older
/// one's end changes nothing).
pub fn mini_closed(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;
    app.state::<MiniShelf>().gone(label);
}

/// Open the mini player (a small window that stays on top; the main window steps aside), or
/// close it and bring the main window back. Async: making a window from a synchronous command
/// deadlocks on Windows.
#[tauri::command]
pub async fn mini_player(open: bool, app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let shelf = app.state::<MiniShelf>();
    if open {
        shelf.open(&MiniHost(&app), Mini, &()).await
    } else {
        shelf.close(&MiniHost(&app), Mini).await;
        Ok(())
    }
}

/// The mini player button, Ctrl+M and the tray: open the mini player, or, while it is showing,
/// close it and bring the main window back, in one step (asking first and acting after could act
/// on what was already changing).
#[tauri::command]
pub async fn mini_player_toggle(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    app.state::<MiniShelf>()
        .toggle(&MiniHost(&app), Mini, &())
        .await
        .map(|_| ())
}

/// A window starts or stops showing a spectrum display; the engine measures band levels while
/// any window shows one.
#[tauri::command]
pub fn player_spectrum(
    on: bool,
    webview: tauri::Webview,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    let any = {
        let mut set = pb.spectrum.lock().map_err(|e| e.to_string())?;
        if on {
            set.insert(webview.label().to_string());
        } else {
            set.remove(webview.label());
        }
        !set.is_empty()
    };
    pb.send(Cmd::Spectrum(any))
}

#[cfg(test)]
mod mini_one_window {
    //! The mini player made as the lyrics windows were before 2026-10-21: asked whether it was
    //! showing, then opened or closed, under the one label "mini". Ctrl+M or the button pressed
    //! again while it was being made made a second window of that label, and the end of either
    //! left the other unknown: alive on screen, deaf to the app, and the main window left
    //! minimized or brought back out of turn. These hold the host every one-of-a-kind window is
    //! tested against (one_window::fake).
    use super::{Mini, MiniShelf};
    use crate::one_window::fake::Fake;
    use crate::one_window::{Host, WindowKind};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    #[test]
    fn its_windows_are_known_by_their_labels() {
        for mine in ["mini", "mini-1", "mini-42"] {
            assert_eq!(Mini::of_label(mine), Some(Mini), "{mine}");
        }
        for other in ["main", "minimal", "mini-", "mini-x", "overlay-3", "stage-1"] {
            assert_eq!(Mini::of_label(other), None, "{other}");
        }
    }

    #[tokio::test]
    async fn two_asks_at_once_make_one_mini_player() {
        let shelf = Arc::new(MiniShelf::default());
        let host = Fake::with(shelf.clone());
        let (a, b) = tokio::join!(shelf.open(&host, Mini, &()), shelf.open(&host, Mini, &()));
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(
            host.made.load(Ordering::SeqCst),
            1,
            "the second brings it forward"
        );
        assert_eq!(host.most.load(Ordering::SeqCst), 1);
        assert_eq!(host.shown(Mini), Some(true));
    }

    #[tokio::test]
    async fn presses_in_quick_succession_never_make_two_and_leave_the_main_window_right() {
        let shelf = Arc::new(MiniShelf::default());
        let host = Fake::with(shelf.clone());
        let (a, b, c) = tokio::join!(
            shelf.toggle(&host, Mini, &()),
            shelf.toggle(&host, Mini, &()),
            shelf.toggle(&host, Mini, &())
        );
        assert_eq!((a, b, c), (Ok(true), Ok(false), Ok(true)));
        assert_eq!(host.most.load(Ordering::SeqCst), 1);
        assert_eq!(host.of(Mini), 1);
        // The one there is the one the app knows, and the main window stepped aside for it last.
        let live = shelf.live(&host, Mini).expect("known");
        assert!(host.has(&live) && live.starts_with("mini-"), "{live}");
        assert_eq!(host.shown(Mini), Some(true));
        // One more press closes it: nothing left behind, and the main window back.
        assert_eq!(shelf.toggle(&host, Mini, &()).await, Ok(false));
        assert_eq!(host.of(Mini), 0);
        assert!(shelf.live(&host, Mini).is_none());
        assert_eq!(host.shown(Mini), Some(false));
    }

    #[tokio::test]
    async fn back_to_venuta_brings_the_main_window_back_even_with_no_mini_player() {
        let shelf = Arc::new(MiniShelf::default());
        let host = Fake::with(shelf.clone());
        shelf.close(&host, Mini).await;
        assert_eq!(host.shown(Mini), Some(false));
    }

    #[tokio::test]
    async fn an_old_or_lost_mini_player_goes_before_another_is_made() {
        let shelf = Arc::new(MiniShelf::default());
        let host = Fake::with(shelf.clone());
        // One under the old fixed label, one under a numbered label the app no longer counts as
        // its own, and one only the system knows.
        host.windows.lock().unwrap().push("mini".into());
        host.windows.lock().unwrap().push("mini-90".into());
        host.windows.lock().unwrap().push("main".into());
        host.lost.store(1, Ordering::SeqCst);
        shelf.open(&host, Mini, &()).await.unwrap();
        assert_eq!(host.of(Mini), 1, "only the new one");
        let live = shelf.live(&host, Mini).unwrap();
        assert!(live != "mini" && live != "mini-90", "{live}");
        assert!(host.has("main"), "the main window is never taken for one");
    }
}

/// Pause when headphones, a headset or Bluetooth go away (rather than carry on through the
/// speakers): kept in Settings and told to the player.
#[tauri::command]
pub fn player_set_pause_when_headphones_go(
    on: bool,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    let mut s = engine::settings::load();
    s.player.pause_when_headphones_go = on;
    engine::settings::save(&s)?;
    pb.send(Cmd::PauseWhenHeadphonesGo(on))
}

/// Whether headphones or Bluetooth going away pause the music.
#[tauri::command]
pub fn player_pause_when_headphones_go() -> bool {
    engine::settings::load().player.pause_when_headphones_go
}
