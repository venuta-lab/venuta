//! The player, as the window sees it: commands in, a stream of status out.
//!
//! The engine (`player` crate) runs on its own thread. Status arrives about ten times a second
//! and goes straight to the window over a channel; the queue travels only when it changes.

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
    /// Exclusive (bit-perfect) unless you choose shared; the device, or the default.
    output: Mutex<(Option<String>, bool)>,
    /// Catalog + session-audio access for remote items (the app factory until a real provider).
    catalog: Arc<dyn MediaAccess>,
    /// Local only (Settings, Listening): every catalog request is refused while it's on.
    local_only: Arc<std::sync::atomic::AtomicBool>,
    offline: Arc<dyn MediaAccess>,
    /// Listening together: the room's follower hears every status here.
    room_tap: Arc<Mutex<Option<std::sync::mpsc::Sender<RoomTick>>>>,
    /// Listening together: the room takes the player's buttons (the tray, media keys, every
    /// window), so no one plays on their own by accident.
    room_buttons: RoomButtons,
    /// Which room's follower has the player now (0: none), so one winding down never undoes
    /// what the next one set up.
    room_owner: std::sync::atomic::AtomicU64,
    /// Where you are in your own queue, kept current (a room sets it aside, and gives it back).
    here: Arc<Mutex<Here>>,
    /// How the player was last told to follow a room (`Some(None)`: on its own), for the log.
    room_mode: Mutex<Option<Option<bool>>>,
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

/// Your own queue while a room has the player (in the data folder, so a Venuta closed in a room
/// still gives it back when it opens again).
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

/// What the room's follower needs of each player status.
#[derive(Debug, Clone)]
pub struct RoomTick {
    pub index: Option<usize>,
    pub duration_ms: u64,
    pub sync: Option<player::engine::SyncStatus>,
    /// The player stopped on an error (the output refused to open, the song couldn't be read).
    pub error: bool,
    /// Not a status: time passing with none (the follower looks at the clock).
    pub idle: bool,
}

impl RoomTick {
    pub fn idle() -> Self {
        RoomTick {
            index: None,
            duration_ms: 0,
            sync: None,
            error: false,
            idle: true,
        }
    }
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
            // Bit-perfect unless you choose otherwise.
            output: Mutex::new((None, true)),
            catalog: media_access::default_media_access(),
            local_only: Arc::default(),
            offline: Arc::new(LocalOnlyAccess),
            room_tap: Arc::default(),
            room_buttons: Arc::default(),
            room_owner: std::sync::atomic::AtomicU64::new(0),
            here: Arc::default(),
            room_mode: Mutex::default(),
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

    fn with<R>(&self, f: impl FnOnce(&Player<QueueItem>) -> R) -> Result<R, String> {
        let mut slot = self.player.lock().map_err(|e| e.to_string())?;
        if slot.is_none() {
            let window = self.window.clone();
            let media = self.media.clone();
            let root = self.root.clone();
            let catalog = self.catalog.clone();
            let local_only = self.local_only.clone();
            let room_tap = self.room_tap.clone();
            let here = self.here.clone();
            // Last next-id we asked the cache to warm (avoid spamming resolve every status tick).
            let mut warmed: Option<String> = None;
            // Plays and where you were are written to disk on a thread of their own: never on
            // the player's, where a slow disk or a busy database would starve the sound.
            let (keep, kept) = std::sync::mpsc::channel::<Keep>();
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
                    if let Ok(tap) = room_tap.lock()
                        && let Some(tx) = tap.as_ref()
                    {
                        let _ = tx.send(RoomTick {
                            index: s.index,
                            duration_ms: s.duration_ms,
                            sync: s.sync,
                            error: !s.error.is_empty(),
                            idle: false,
                        });
                    }
                    let folder = root.lock().map(|r| r.clone()).unwrap_or_default();
                    let index = s.index.map(|i| i as i64).unwrap_or(-1);
                    if let Ok(mut h) = here.lock() {
                        if s.queue.is_some() {
                            h.items = queue.clone();
                            h.order = order.clone();
                        }
                        if h.from != s.from {
                            h.from = s.from.clone();
                        }
                        h.index = index;
                        h.position_ms = s.position_ms;
                        h.shuffle = s.shuffle;
                        h.repeat = s.repeat;
                    }
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
                    if (key.clone(), s.playing, tick) != last
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
                    if let Ok(mut w) = window.lock()
                        && !w.is_empty()
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
                        // A window that has closed stops listening.
                        w.retain(|_, ch| ch.send(status.clone()).is_ok());
                    }
                },
            ));
            if let Some(sound) = crate::listening::saved_sound()
                && let Some(p) = slot.as_ref()
            {
                p.send(Cmd::Sound(sound));
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

    /// Listening together: a room's follower takes the player (its id, to let go of it later).
    pub fn claim_room(&self) -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.room_owner
            .store(id, std::sync::atomic::Ordering::SeqCst);
        id
    }

    /// Listening together: the room takes the player, so your own queue as it is now is set
    /// aside (once: a room right after another keeps the queue from before the first).
    pub fn set_aside(&self) {
        let path = aside_path();
        if path.exists() {
            return;
        }
        let Ok(mut h) = self.here.lock().map(|h| h.clone()) else {
            return;
        };
        if h.items.is_empty() || h.index < 0 {
            return;
        }
        h.root = self.root.lock().map(|r| r.clone()).unwrap_or_default();
        if let Ok(json) = serde_json::to_vec(&h) {
            let _ = std::fs::create_dir_all(engine::paths::app_data_dir());
            let _ = std::fs::write(&path, json);
        }
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

    /// Listening together: that follower lets go of the player, unless another room's has it
    /// already. True when it was still its own.
    pub fn release_room(&self, id: u64) -> bool {
        self.room_owner
            .compare_exchange(
                id,
                0,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Listening together: the room takes the buttons (`None`: the player has them again).
    pub fn set_room_buttons(&self, f: Option<Arc<dyn Fn(Button) + Send + Sync>>) {
        if let Ok(mut b) = self.room_buttons.lock() {
            *b = f;
        }
    }
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
    fn resolve(&self, root: &str, items: Vec<QueueItem>) -> Vec<QueueItem> {
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

    /// Listening together: every player status goes to `tap` too (`None`: no more).
    pub fn set_room_tap(&self, tap: Option<std::sync::mpsc::Sender<RoomTick>>) {
        if let Ok(mut t) = self.room_tap.lock() {
            *t = tap;
        }
    }

    /// Listening together: this song alone in the player, ready at `start_ms` (the room's plan
    /// then says when it plays). A remote item may bring its own way to its stream.
    pub fn room_load(
        &self,
        root: &str,
        item: QueueItem,
        start_ms: u64,
        own: Option<Arc<dyn MediaAccess>>,
    ) -> Result<(), String> {
        if let Ok(mut r) = self.root.lock() {
            *r = root.to_owned();
        }
        let mut items = self.resolve(root, vec![item]);
        if let Some(access) = own {
            for it in &mut items {
                it.media = MediaRef::new(access.clone());
            }
        }
        self.send(Cmd::Set {
            items,
            index: 0,
            start_ms,
            play: false,
            // No source of its own: the room line says whose room it is.
            from: String::new(),
        })
    }

    /// Listening together: add a song after the last one readied (ready ahead, to follow it
    /// without a gap).
    pub fn room_queue_next(
        &self,
        root: &str,
        item: QueueItem,
        own: Option<Arc<dyn MediaAccess>>,
    ) -> Result<(), String> {
        let mut items = self.resolve(root, vec![item]);
        if let Some(access) = own {
            for it in &mut items {
                it.media = MediaRef::new(access.clone());
            }
        }
        self.send(Cmd::Insert { items, next: false })
    }

    /// Listening together: quiet (this person can't play the room's song, or stepped out).
    pub fn room_pause(&self) -> Result<(), String> {
        self.send(Cmd::Pause)
    }

    /// Listening together: on to the readied next song now (this copy ran longer than the room's).
    pub fn room_next(&self) -> Result<(), String> {
        self.send(Cmd::Next)
    }

    /// Listening together: the room's plan for the player (`None`: on its own again).
    pub fn room_sync(&self, plan: Option<player::sync::SyncPlan>) -> Result<(), String> {
        // In the log when the player starts or stops following a room, or changes how.
        let mode = plan.map(|p| p.same_room);
        if let Ok(mut was) = self.room_mode.lock()
            && *was != Some(mode)
        {
            *was = Some(mode);
            note_play(
                match mode {
                    Some(true) => "player: follows the room (one room: kept within a few ms)",
                    Some(false) => "player: follows the room (apart)",
                    None => "player: on its own again (no room plan, no correction)",
                },
                "listening together",
            );
        }
        self.send(Cmd::Sync(plan))
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

#[cfg(windows)]
use player::wasapi_out::{OutputDevice, list_devices};

#[cfg(not(windows))]
#[derive(Serialize)]
pub struct OutputDevice {
    id: String,
    name: String,
    default: bool,
}

#[cfg(not(windows))]
fn list_devices() -> Result<Vec<OutputDevice>, String> {
    Ok(Vec::new())
}

#[derive(Serialize)]
pub struct OutputChoices {
    devices: Vec<OutputDevice>,
    /// The chosen device (`None`: follow the Windows default).
    device: Option<String>,
    exclusive: bool,
}

/// Sound outputs, and which one plays.
#[tauri::command]
pub async fn player_devices(pb: State<'_, Playback>) -> Result<OutputChoices, String> {
    let (device, exclusive) = pb.output.lock().map_err(|e| e.to_string())?.clone();
    let devices = tauri::async_runtime::spawn_blocking(list_devices)
        .await
        .map_err(|e| e.to_string())??;
    Ok(OutputChoices {
        devices,
        device,
        exclusive,
    })
}

/// Play through another device, or switch between exclusive (bit-perfect) and shared.
#[tauri::command]
pub fn player_output(
    device: Option<String>,
    exclusive: bool,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    *pb.output.lock().map_err(|e| e.to_string())? = (device.clone(), exclusive);
    pb.send(Cmd::Output(OutputChoice::Device {
        id: device,
        exclusive,
    }))
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

/// Open the mini player (a small window that stays on top; the main window steps aside), or
/// close it and bring the main window back. Async: making a window from a synchronous command
/// deadlocks on Windows.
#[tauri::command]
pub async fn mini_player(open: bool, app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let main = app.get_webview_window("main");
    if open {
        if let Some(w) = app.get_webview_window("mini") {
            let _ = w.show();
            let _ = w.set_focus();
        } else {
            let (w, h) = (380.0, 132.0);
            // Bottom right of the screen the main window is on, clear of the taskbar.
            let place = main
                .as_ref()
                .and_then(|m| m.current_monitor().ok().flatten())
                .map(|mon| {
                    let area = mon.work_area();
                    let scale = mon.scale_factor();
                    let right = (area.position.x as f64 + area.size.width as f64) / scale;
                    let bottom = (area.position.y as f64 + area.size.height as f64) / scale;
                    (right - w - 24.0, bottom - h - 24.0)
                });
            let mut b = tauri::WebviewWindowBuilder::new(
                &app,
                "mini",
                tauri::WebviewUrl::App("index.html#mini".into()),
            )
            .title("Venuta")
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
            crate::app_icon::apply(&app);
        }
        if let Some(m) = main {
            let _ = m.minimize();
        }
    } else {
        if let Some(w) = app.get_webview_window("mini") {
            let _ = w.close();
        }
        if let Some(m) = main {
            let _ = m.unminimize();
            let _ = m.show();
            let _ = m.set_focus();
        }
    }
    Ok(())
}

/// The mini player button, Ctrl+M and the tray: open the mini player, or, while it is showing,
/// close it and bring the main window back.
#[tauri::command]
pub async fn mini_player_toggle(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let showing = app
        .get_webview_window("mini")
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    mini_player(!showing, app).await
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
