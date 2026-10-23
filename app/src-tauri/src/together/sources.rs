//! Everyone hears their best copy of the room's song:
//! 1. **Their own file**: the same recording code, MusicBrainz id, or title, artist and length
//!    (the engine's playlist matcher, which already finds your files for imported playlists).
//! 2. **The online catalog's**: the song's own id.
//! 3. **The host's file, streamed from the host**: lossless, the exact copy.
//! 4. **A close match online**: the catalog's nearest by title, artist and length.
//!
//! Listening in step with the room's speakers (every device), the host's file comes first: only
//! an exact copy lines up within a millisecond from the first frame (decision D5).
//!
//! The host's files are known to the room by a hash (`Song::host_key`), never by path; their
//! bytes go only from the host, on request, to people in the room. A guest's player reads the
//! stream like any online song, through a small server on this computer (127.0.0.1 only, with a
//! random token), which forwards each request to the host.

use crate::playback::QueueItem;
use engine::library::{Library, Track};
use media_access::{
    AccessError, AccessErrorKind, CancellationToken, EphemeralAudio, MediaAccess, Page,
    PageRequest, RemoteAlbum, RemoteArtist, RemoteTrack,
};
use std::path::PathBuf;
use std::sync::Arc;
use together::proto::Song;
use together::view::CopyKind;

/// The catalog's provider name (the songs the catalog plays).
pub const CATALOG: &str = "remote_catalog";

/// A copy to play: the queue item, its own way to the stream (the host's), and what it is.
pub struct Copy {
    pub item: QueueItem,
    pub own: Option<Arc<dyn MediaAccess>>,
    pub kind: CopyKind,
}

/// A library song as the player takes it.
pub fn item_of_track(t: &Track) -> QueueItem {
    let artists: Vec<String> = t
        .tags
        .get("ARTIST")
        .map(|v| {
            v.iter()
                .flat_map(|a| engine::plan::split_names(a))
                .collect()
        })
        .unwrap_or_default();
    let ext = t.path.rsplit('.').next().unwrap_or("").to_ascii_uppercase();
    serde_json::from_value(serde_json::json!({
        "path": t.path,
        "dir": t.dir,
        "title": t.first("TITLE").map(str::to_owned).unwrap_or_else(|| t.path.rsplit('/').next().unwrap_or(&t.path).to_owned()),
        "artists": artists,
        "album": t.first("ALBUM").unwrap_or_default(),
        "album_artist": t.first("ALBUMARTIST").or(t.first("ARTIST")).unwrap_or_default(),
        "duration_ms": t.duration_ms,
        "format": ext,
        "bit_depth": t.bit_depth,
        "sample_rate": t.sample_rate,
    }))
    .expect("a queue item from a track")
}

/// An online song as the player takes it.
pub fn item_of_online(song: &Song, track_id: &str) -> QueueItem {
    serde_json::from_value(serde_json::json!({
        "title": song.title,
        "artists": song.artists,
        "album": song.album.clone().unwrap_or_default(),
        "duration_ms": song.duration_ms.unwrap_or(0),
        "online": { "kind": "online", "provider": CATALOG, "track_id": track_id },
        "artwork_url": song.artwork_url,
    }))
    .expect("a queue item for an online song")
}

/// The room's form of a song the host has: from the host's queue item (and its library, for
/// the recording code and MusicBrainz id).
pub fn song_of_item(it: &QueueItem, lib: Option<&Library>) -> Option<Song> {
    let online_id = match &it.online {
        Some(media_access::PlayableId::Online { provider, track_id })
            if provider == CATALOG && together::proto::is_online_id(track_id) =>
        {
            Some(track_id.clone())
        }
        _ => None,
    };
    let local = !it.path.is_empty();
    let track = local
        .then(|| lib.and_then(|l| l.tracks.iter().find(|t| t.path == it.path)))
        .flatten();
    let key = if local {
        Song::host_key(&it.path)
    } else {
        format!("online:{}", online_id.clone()?)
    };
    let tag = |k: &str| {
        track
            .and_then(|t| t.first(k))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let song = Song {
        key,
        title: clip(&it.title),
        artists: it.artists.iter().take(20).map(|a| clip(a)).collect(),
        album: Some(clip(&it.album)).filter(|a| !a.is_empty()),
        duration_ms: Some(it.duration_ms).filter(|d| *d > 0),
        isrc: tag("ISRC").filter(|i| i.len() <= 15 && i.bytes().all(|b| b.is_ascii_alphanumeric())),
        mbid: tag("MUSICBRAINZ_TRACKID").filter(|m| m.len() == 36),
        online_id,
        host_file: local,
        format: local
            .then(|| {
                it.path
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
            })
            .filter(|f| !f.is_empty() && f.len() <= 8),
        artwork_url: it.artwork_url.clone().filter(|u| u.starts_with("https://")),
        added_by: None,
    };
    song.check().ok().map(|()| song)
}

fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(300).collect()
}

/// Your own file of this song, when you have it: the same recording, or the same title,
/// artist and length (within 3 seconds).
pub fn own_file<'a>(song: &Song, lib: &'a Library) -> Option<&'a Track> {
    let f = engine::playlists::Foreign {
        title: song.title.clone(),
        artist: song.artists.join("; "),
        album: song.album.clone().unwrap_or_default(),
        duration_ms: song.duration_ms,
        isrc: song.isrc.clone(),
        mbid: song.mbid.clone(),
        ..Default::default()
    };
    let found = engine::playlists::Matcher::new(lib).grade(&f)?;
    let close_in_length = match song.duration_ms {
        Some(d) => (found.track.duration_ms as i64 - d as i64).abs() <= 3_000,
        None => true,
    };
    (found.grade == engine::playlists::Grade::Exact || close_in_length).then_some(found.track)
}

/// The catalog's nearest to this song by title, artist and length (a close match).
pub fn close_online(song: &Song, catalog: &Arc<dyn MediaAccess>) -> Option<String> {
    let q = format!(
        "{} {}",
        song.title,
        song.artists.first().cloned().unwrap_or_default()
    );
    let page = catalog
        .search_tracks(
            &q,
            // A room's own look-up, never a search the person typed.
            &PageRequest::new(10, None),
            &CancellationToken::default(),
        )
        .ok()?;
    let want = song.duration_ms.map(|d| d as i64);
    let title = song.title.to_lowercase();
    page.items
        .into_iter()
        .filter(|t| t.provider == CATALOG && together::proto::is_online_id(&t.track_id))
        .filter(|t| match (want, t.duration_ms) {
            (Some(w), Some(d)) => (d as i64 - w).abs() <= 4_000,
            _ => true,
        })
        .find(|t| {
            t.title.to_lowercase().contains(&title) || title.contains(&t.title.to_lowercase())
        })
        .map(|t| t.track_id)
}

/// How a person's copies are ordered: the module's order, with the host's file first (the
/// person asked for the host's copy of a song), or last (a lighter copy: the host's advice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    Usual,
    HostFirst,
    Lighter,
}

/// The best copy for a guest (see the module's order), in this `order`. `host_stream`: how to
/// reach the host's files (none for the host itself). `every_device`: speakers heard together,
/// where only an exact copy lines up within a millisecond from the first frame, so the host's
/// file streamed comes first (D5), unless the host advised a lighter copy.
pub fn best_copy_in(
    song: &Song,
    lib: Option<&Library>,
    catalog: &Arc<dyn MediaAccess>,
    local_only: bool,
    host_stream: Option<Arc<HostStream>>,
    order: Order,
    every_device: bool,
) -> Option<Copy> {
    let from_host = |hs: Option<Arc<HostStream>>| -> Option<Copy> {
        let hs = hs.filter(|_| song.host_file)?;
        let mut item = item_of_online(song, &song.key);
        item.online = Some(media_access::PlayableId::Online {
            provider: HOST_PROVIDER.into(),
            track_id: song.key.clone(),
        });
        hs.know(song);
        Some(Copy {
            item,
            own: Some(hs),
            kind: CopyKind::HostStream,
        })
    };
    if (order == Order::HostFirst || (every_device && order != Order::Lighter))
        && let Some(c) = from_host(host_stream.clone())
    {
        return Some(c);
    }
    if let Some(t) = lib.and_then(|l| own_file(song, l)) {
        return Some(Copy {
            item: item_of_track(t),
            own: None,
            kind: CopyKind::OwnFile,
        });
    }
    if !local_only && let Some(id) = &song.online_id {
        return Some(Copy {
            item: item_of_online(song, id),
            own: None,
            kind: CopyKind::Catalog,
        });
    }
    if order != Order::Lighter
        && let Some(c) = from_host(host_stream.clone())
    {
        return Some(c);
    }
    if !local_only && let Some(id) = close_online(song, catalog) {
        return Some(Copy {
            item: item_of_online(song, &id),
            own: None,
            kind: CopyKind::CloseMatch,
        });
    }
    // A lighter copy only if there is one: else the host's after all.
    from_host(host_stream)
}

/// The provider name of songs streamed from the room's host.
pub const HOST_PROVIDER: &str = "venuta_room";

/// A guest's way to the host's files: the local server's address and token. As a catalog, it
/// answers only "where is this song's stream" (on this computer), and only for the room's
/// host files.
pub struct HostStream {
    pub port: u16,
    pub token: String,
    formats: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl HostStream {
    pub fn new(port: u16, token: String) -> Arc<HostStream> {
        Arc::new(HostStream {
            port,
            token,
            formats: Default::default(),
        })
    }

    /// Remember a host song's format, for its stream's type.
    pub fn know(&self, song: &Song) {
        if let (Ok(mut m), Some(f)) = (self.formats.lock(), &song.format) {
            m.insert(song.key.clone(), f.clone());
        }
    }
}

pub fn mime_of(format: &str) -> &'static str {
    match format {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" | "alac" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "aif" | "aiff" => "audio/aiff",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

fn not_here(what: &str) -> AccessError {
    AccessError::new(
        AccessErrorKind::Unsupported,
        format!("{what} isn't in a room's stream"),
    )
}

impl MediaAccess for HostStream {
    fn search_tracks(
        &self,
        _: &str,
        _: &PageRequest,
        _: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        Err(not_here("search"))
    }
    fn search_albums(
        &self,
        _: &str,
        _: &PageRequest,
        _: &CancellationToken,
    ) -> Result<Page<RemoteAlbum>, AccessError> {
        Err(not_here("search"))
    }
    fn search_artists(
        &self,
        _: &str,
        _: &PageRequest,
        _: &CancellationToken,
    ) -> Result<Page<RemoteArtist>, AccessError> {
        Err(not_here("search"))
    }
    fn album_tracks(
        &self,
        _: &str,
        _: &PageRequest,
        _: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        Err(not_here("an album"))
    }
    fn playlist_tracks(
        &self,
        _: &str,
        _: &PageRequest,
        _: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        Err(not_here("a playlist"))
    }
    fn resolve_audio(
        &self,
        track_id: &str,
        _: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        if !track_id.starts_with("host:") {
            return Err(not_here("that song"));
        }
        let format = self
            .formats
            .lock()
            .ok()
            .and_then(|m| m.get(track_id).cloned())
            .unwrap_or_default();
        Ok(EphemeralAudio {
            track_id: track_id.to_owned(),
            provider: HOST_PROVIDER.into(),
            url: format!(
                "http://127.0.0.1:{}/s/{}?t={}",
                self.port, track_id, self.token
            ),
            mime_type: Some(mime_of(&format).into()),
            content_length: None,
            duration_ms: None,
            headers: Vec::new(),
            via: "room host",
            loudness_db: None,
        })
    }
}

/// The host's files the room knows, by key: where each is, and its format.
#[derive(Default)]
pub struct HostFiles {
    files: std::sync::Mutex<std::collections::HashMap<String, (PathBuf, String)>>,
}

impl HostFiles {
    pub fn add(&self, key: &str, file: PathBuf) {
        let format = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if let Ok(mut m) = self.files.lock() {
            m.insert(key.to_owned(), (file, format));
        }
    }
    pub fn get(&self, key: &str) -> Option<(PathBuf, String)> {
        self.files.lock().ok()?.get(key).cloned()
    }
}

/// A room song as a playlist line: the catalog's song by its id, else your own file of it;
/// a song only the host has can't be one.
pub fn playlist_item(song: &Song, lib: &Library) -> Option<engine::playlists::Item> {
    if let Some(id) = &song.online_id {
        return Some(engine::playlists::Item::online(
            engine::playlists::OnlineEntry {
                provider: CATALOG.into(),
                track_id: id.clone(),
                title: song.title.clone(),
                artists: song.artists.clone(),
                album: song.album.clone(),
                duration_ms: song.duration_ms,
                isrc: song.isrc.clone(),
                artwork_url: song.artwork_url.clone(),
            },
        ));
    }
    own_file(song, lib).map(|t| engine::playlists::Item::track(t.path.clone()))
}
// --- A person's copies, for the conductor ---------------------------------------------------

/// Binds an item to where it plays from under a library folder (`Playback::resolve`).
pub type Resolver = Arc<dyn Fn(&str, QueueItem) -> QueueItem + Send + Sync>;

/// What a look-up came to (on its own thread), kept by song and order.
type Found = Arc<std::sync::Mutex<Option<Option<(QueueItem, CopyKind)>>>>;

/// How the app finds a person's copies for the room (`together::conductor::Copies`): the host's
/// own songs from the host's own items (exact); everyone else's by the module's order, looked up
/// on a thread of its own (a catalog search can take seconds; the session never waits for it).
pub struct AppCopies {
    pub root: String,
    pub lib: Option<Arc<Library>>,
    pub catalog: Arc<dyn MediaAccess>,
    pub local_only: bool,
    /// A guest's way to the host's files.
    pub host_stream: Option<Arc<HostStream>>,
    /// The host's: its own songs in the room, as its queue had them, by key.
    pub own: Arc<std::sync::Mutex<std::collections::HashMap<String, QueueItem>>>,
    /// Binds an item to where it plays from (Playback::resolve).
    pub resolve: Resolver,
    pub order: Order,
    pub host_first: std::collections::HashSet<String>,
    /// How this person listens: every device puts the host's file first (D5).
    pub mode: together::room_player::ListenMode,
    found: std::collections::HashMap<(String, bool, bool, bool), Found>,
}

impl AppCopies {
    pub fn new(
        root: String,
        lib: Option<Arc<Library>>,
        catalog: Arc<dyn MediaAccess>,
        local_only: bool,
        host_stream: Option<Arc<HostStream>>,
        resolve: Resolver,
    ) -> AppCopies {
        AppCopies {
            root,
            lib,
            catalog,
            local_only,
            host_stream,
            own: Arc::default(),
            resolve,
            order: Order::Usual,
            host_first: Default::default(),
            mode: together::room_player::ListenMode::Apart,
            found: Default::default(),
        }
    }

    fn copy_of(&self, item: QueueItem, kind: CopyKind) -> together::conductor::Copy<QueueItem> {
        use together::align::Alignment;
        let length_ms = Some(item.duration_ms).filter(|d| *d > 0);
        together::conductor::Copy {
            item,
            // The host's own bytes are exact; anything else is measured against the host's
            // reference by the room's measure (`together::measure::Measured`, WP11).
            align: if kind == CopyKind::HostStream {
                Alignment::EXACT
            } else {
                Alignment::APPROXIMATE
            },
            kind,
            length_ms,
        }
    }
}

impl together::conductor::Copies<QueueItem> for AppCopies {
    fn find(&mut self, song: &Song) -> together::conductor::Lookup<QueueItem> {
        use together::conductor::Lookup;
        if let Some(it) = self.own.lock().ok().and_then(|m| m.get(&song.key).cloned()) {
            let mut c = self.copy_of(it, CopyKind::OwnFile);
            // The host's own file is the room's reference: exact by definition.
            c.align = together::align::Alignment::EXACT;
            return Lookup::Found(c);
        }
        let host_first = self.host_first.contains(&song.key);
        let order = if host_first {
            Order::HostFirst
        } else {
            self.order
        };
        let every = self.mode == together::room_player::ListenMode::EveryDevice;
        let k = (song.key.clone(), host_first, order == Order::Lighter, every);
        let slot = self.found.entry(k.clone()).or_insert_with(|| {
            let slot: Found = Arc::default();
            let (s2, song) = (slot.clone(), song.clone());
            let (lib, catalog, local_only, hs) = (
                self.lib.clone(),
                self.catalog.clone(),
                self.local_only,
                self.host_stream.clone(),
            );
            let (root, resolve) = (self.root.clone(), self.resolve.clone());
            std::thread::Builder::new()
                .name("venuta-room-copy".into())
                .spawn(move || {
                    let c = best_copy_in(
                        &song,
                        lib.as_deref(),
                        &catalog,
                        local_only,
                        hs,
                        order,
                        every,
                    )
                    .map(|c| {
                        let mut item = resolve(&root, c.item);
                        if let Some(own) = c.own {
                            item.media = crate::playback::MediaRef::new(own);
                        }
                        (item, c.kind)
                    });
                    if let Ok(mut s) = s2.lock() {
                        *s = Some(c);
                    }
                })
                .ok();
            slot
        });
        let done = slot.lock().ok().and_then(|s| s.clone());
        match done {
            None => Lookup::Looking,
            Some(None) => {
                // Looked for afresh when asked again (the conductor tries every 5 s while the
                // song plays: the catalog may answer then).
                self.found.remove(&k);
                Lookup::NoCopy
            }
            Some(Some((item, kind))) => Lookup::Found(self.copy_of(item, kind)),
        }
    }

    fn lighter(&mut self) {
        self.order = Order::Lighter;
    }

    fn prefer_host(&mut self, key: &str) {
        self.host_first.insert(key.to_owned());
    }

    fn set_mode(&mut self, mode: together::room_player::ListenMode) {
        self.mode = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        Song {
            key: "online:abcdefghijk".into(),
            title: "Kesariya".into(),
            artists: vec!["Arijit Singh".into()],
            album: Some("Brahmastra".into()),
            duration_ms: Some(268_000),
            isrc: None,
            mbid: None,
            online_id: Some("abcdefghijk".into()),
            host_file: false,
            format: None,
            artwork_url: None,
            added_by: None,
        }
    }

    #[test]
    fn an_online_song_plays_online_without_a_file_of_its_own() {
        let it = item_of_online(&song(), "abcdefghijk");
        assert!(it.path.is_empty());
        assert!(
            matches!(&it.online, Some(media_access::PlayableId::Online { provider, track_id }) if provider == CATALOG && track_id == "abcdefghijk")
        );
        assert_eq!(it.title, "Kesariya");
    }

    #[test]
    fn a_host_song_streams_from_this_computer_only() {
        let hs = HostStream::new(4567, "tok".into());
        let s = Song {
            key: "host:0123456789abcdef0123456789abcdef".into(),
            host_file: true,
            format: Some("flac".into()),
            online_id: None,
            ..song()
        };
        hs.know(&s);
        let a = hs
            .resolve_audio(&s.key, &CancellationToken::default())
            .unwrap();
        assert!(a.url.starts_with("http://127.0.0.1:4567/s/host:"));
        assert!(a.url.ends_with("?t=tok"));
        assert_eq!(a.mime_type.as_deref(), Some("audio/flac"));
        assert!(
            hs.resolve_audio("online:abcdefghijk", &CancellationToken::default())
                .is_err()
        );
    }

    #[test]
    fn every_device_puts_the_hosts_file_first_and_apart_its_own_order() {
        let hs = HostStream::new(4567, "tok".into());
        let s = Song {
            key: "host:0123456789abcdef0123456789abcdef".into(),
            host_file: true,
            format: Some("flac".into()),
            ..song()
        };
        let catalog: Arc<dyn MediaAccess> = Arc::new(media_access::UnconfiguredOnlineAccess);
        let kind = |order, every| {
            best_copy_in(&s, None, &catalog, false, Some(hs.clone()), order, every).map(|c| c.kind)
        };
        assert_eq!(kind(Order::Usual, false), Some(CopyKind::Catalog));
        assert_eq!(kind(Order::Usual, true), Some(CopyKind::HostStream), "D5");
        assert_eq!(kind(Order::HostFirst, false), Some(CopyKind::HostStream));
        assert_eq!(
            kind(Order::Lighter, true),
            Some(CopyKind::Catalog),
            "the host's advice wins"
        );
    }

    #[test]
    fn a_host_item_becomes_a_room_song_without_its_path() {
        let it: QueueItem = serde_json::from_value(serde_json::json!({
            "path": "Arijit Singh/Brahmastra/01 Kesariya.flac", "dir": "Arijit Singh/Brahmastra",
            "title": "Kesariya", "artists": ["Arijit Singh"], "album": "Brahmastra", "duration_ms": 268000
        }))
        .unwrap();
        let s = song_of_item(&it, None).unwrap();
        assert!(s.host_file);
        assert_eq!(s.format.as_deref(), Some("flac"));
        assert!(
            !serde_json::to_string(&s).unwrap().contains("Brahmastra/01"),
            "no path leaves the host"
        );
        assert!(s.check().is_ok());
    }
}
