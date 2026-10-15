//! Playlists as plain `.m3u8` files, readable by every player: in `Playlists/` beside the
//! library's `Music/` folder (inside the music folder for older libraries).
//!
//! Entries use paths relative to the playlist (`../Music/Film & Series/…`), so the same file
//! works on the PC and on a card, which has the same `Music/` and `Playlists/` folders. Each entry also carries the
//! track's audio checksum in a comment line players ignore (`#VENUTA-MD5:`), so when files
//! move (e.g. the migration) a playlist repairs itself instead of breaking.
//!
//! Smart playlists are rules in `<rules>/playlists/*.toml`, written out as `.m3u8` too.
//!
//! A song that isn't in the library (yet) keeps its place as a comment line players ignore
//! (`#VENUTA-WANT:{json}`), so an imported playlist loses nothing and can be completed later;
//! `#VENUTA-SOURCE:` remembers where a playlist came from.
//!
//! A song from the online catalog is a comment line too (`#VENUTA-ONLINE:{json}`): its stable
//! provider id and what to show for it, never a stream address. Your own file wins: when the
//! library has the song, the entry plays from it; otherwise it plays online (see `playables`).

use crate::library::{Library, Track};
use crate::plan::split_names;
use media_access::{Availability, ONLINE_PLAYLIST_TAG, Playable, PlayableId, RemoteTrack};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "Playlists";
const MD5_TAG: &str = "#VENUTA-MD5:";
const SMART_TAG: &str = "#VENUTA-SMART";
/// The same lines as written before the app was named; still read.
const OLD_MD5_TAG: &str = "#LIBRARIAN-MD5:";
const OLD_SMART_TAG: &str = "#LIBRARIAN-SMART";
pub(crate) const WANT_TAG: &str = "#VENUTA-WANT:";
const SOURCE_TAG: &str = "#VENUTA-SOURCE:";
/// Which playlist at the source it is ("spotify:37i9…", "apple:Hindi"), to update it later.
const SOURCE_ID_TAG: &str = "#VENUTA-SOURCE-ID:";
/// The songs the source had when it was last brought in (short keys), so an update can tell a
/// song you took out from one the source added, and a song the source dropped from one you added.
const BASE_TAG: &str = "#VENUTA-BASE:";

/// A song a playlist wants that isn't in the library: enough to find it again later.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WantEntry {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// The audio checksum of a file the playlist had, which went away: when the same audio is
    /// back (anywhere in the library), it is this song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    /// Library files you said are not this song (never offered or filled in again).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not: Vec<String>,
}

impl WantEntry {
    /// As a song from elsewhere, for matching.
    pub fn foreign(&self) -> Foreign {
        Foreign {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_ms: Some(self.duration_ms).filter(|d| *d > 0),
            isrc: self.isrc.clone(),
            ..Default::default()
        }
    }
}

/// A song in the online catalog, held by its stable id with what to show for it. No address
/// to stream it from: that is resolved each time it plays, and never written down. A cover
/// picture URL is allowed (display only); stream hosts are rejected on write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineEntry {
    pub provider: String,
    pub track_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artists: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// The recording code, to find the song in the library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// Cover picture only (https). Never a stream URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
}

impl OnlineEntry {
    /// A catalog track as a playlist keeps it (album id stays behind; cover picture may stay).
    pub fn from_remote(t: &RemoteTrack) -> Self {
        Self {
            provider: t.provider.clone(),
            track_id: t.track_id.clone(),
            title: t.title.clone(),
            artists: t.artists.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
            isrc: t.isrc.clone(),
            artwork_url: clean_artwork_url(t.artwork_url.as_deref()),
        }
    }

    pub fn id(&self) -> PlayableId {
        PlayableId::Online {
            provider: self.provider.clone(),
            track_id: self.track_id.clone(),
        }
    }

    /// A real id: something there, and not an address.
    pub fn is_stable(&self) -> bool {
        let ok = |s: &str| !s.trim().is_empty() && !s.contains("://");
        ok(&self.provider) && ok(&self.track_id)
    }

    /// As a song from elsewhere, for finding it in the library.
    pub fn foreign(&self) -> Foreign {
        Foreign {
            title: self.title.clone(),
            artist: self.artists.join("; "),
            album: self.album.clone().unwrap_or_default(),
            duration_ms: self.duration_ms,
            isrc: self.isrc.clone(),
            ..Default::default()
        }
    }

    /// To play from the catalog.
    pub fn playable(&self) -> Playable {
        Playable {
            id: self.id(),
            title: self.title.clone(),
            artists: self.artists.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            artwork_url: self.artwork_url.clone(),
            availability: Some(Availability::Online),
            local_file: None,
        }
    }
}

/// Keep only https picture URLs; drop stream hosts and anything that is not a picture address.
fn clean_artwork_url(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    let url = if let Some(rest) = s.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = s.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        s.to_owned()
    };
    if !url.starts_with("https://") {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if lower.contains("googlevideo.com") || lower.contains(".m3u8") || lower.contains("mime=audio")
    {
        return None;
    }
    Some(url)
}

/// The playlist line for an online song (`#VENUTA-ONLINE:{json}`, no newline).
pub fn encode_online(e: &OnlineEntry) -> String {
    format!(
        "{ONLINE_PLAYLIST_TAG}{}",
        serde_json::to_string(e).expect("plain data serializes")
    )
}

/// An online song from its playlist line; None for any other line, or one without a real id.
/// Fields it doesn't know (an address someone typed in) are dropped, so never written back.
pub fn decode_online(line: &str) -> Option<OnlineEntry> {
    let json = line.trim().strip_prefix(ONLINE_PLAYLIST_TAG)?;
    let mut entry = serde_json::from_str::<OnlineEntry>(json.trim()).ok()?;
    if !entry.is_stable() {
        return None;
    }
    entry.artwork_url = clean_artwork_url(entry.artwork_url.as_deref());
    Some(entry)
}

/// A library song that may be one a playlist is waiting for, offered for you to confirm.
#[derive(Debug, Clone, Serialize)]
pub struct Maybe {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    /// Why, in words ("Another version: …").
    pub why: String,
}

/// One line of a playlist to write: a song in the library, one still wanted, or one online.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Item {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub want: Option<WantEntry>,
    /// For a song matched to a file on import: what the source called it (so an update later
    /// knows this file answers that song, even when the names differ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<WantEntry>,
    /// A song from the online catalog. Written as its id even when the library has it too: the
    /// file is found again on every read, so it wins while it's here and the song stays if not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online: Option<OnlineEntry>,
}

impl Item {
    pub fn track(path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            want: None,
            source: None,
            online: None,
        }
    }
    pub fn wanted(w: WantEntry) -> Self {
        Self {
            path: None,
            want: Some(w),
            source: None,
            online: None,
        }
    }
    pub fn online(o: OnlineEntry) -> Self {
        Self {
            path: None,
            want: None,
            source: None,
            online: Some(o),
        }
    }
}

/// A playlist file's own lines about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    /// Where it came from, in words, with the day ("Spotify 2026-10-13").
    pub source: Option<String>,
    pub source_id: Option<String>,
    /// The source's songs when last brought in: each song's keys (see `song_keys`).
    pub base: Option<Vec<Vec<String>>>,
}

fn header_in(text: &str) -> Header {
    let mut h = Header::default();
    for l in text.lines().map(str::trim) {
        if let Some(s) = l.strip_prefix(SOURCE_ID_TAG) {
            h.source_id = Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        } else if let Some(s) = l.strip_prefix(SOURCE_TAG) {
            h.source = Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        } else if let Some(s) = l.strip_prefix(BASE_TAG) {
            h.base = Some(
                s.split_whitespace()
                    .map(|song| song.split('.').map(str::to_owned).collect())
                    .collect(),
            );
        }
    }
    h
}

/// What a playlist's file says about itself (nothing, for a playlist that isn't there).
pub fn header_of(library_root: &Path, name: &str) -> Header {
    crate::text::read_text(&existing_file(library_root, name))
        .map(|t| header_in(&t))
        .unwrap_or_default()
}

fn short_key(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex()[..10].to_owned()
}

/// What tells one song from another across playlists, sources and files: the file, the
/// recording code (ISRC), and the title with each of its artists (film and feature notes
/// ignored, see `title_key`). Two entries that share a key are the same song.
pub fn song_keys(
    path: Option<&str>,
    title: &str,
    artists: &str,
    isrc: Option<&str>,
) -> Vec<String> {
    let mut k = Vec::new();
    if let Some(p) = path.filter(|p| !p.is_empty()) {
        k.push(short_key(&format!("p:{}", p.to_lowercase())));
    }
    if let Some(i) = isrc
        .map(|i| i.trim().to_uppercase())
        .filter(|i| i.len() >= 12)
    {
        k.push(short_key(&format!("i:{i}")));
    }
    let t = title_key(title);
    if !t.is_empty() {
        let names: Vec<String> = split_names(artists)
            .iter()
            .map(|a| crate::audit::name_key(a))
            .filter(|a| !a.is_empty())
            .collect();
        if names.is_empty() {
            k.push(short_key(&format!("s:{t}|")));
        }
        for a in names {
            k.push(short_key(&format!("s:{t}|{a}")));
        }
    }
    k
}

/// A library song's keys (its artists and album artists both count).
pub fn track_keys(t: &Track) -> Vec<String> {
    let artists = [t.first("ARTIST"), t.first("ALBUMARTIST")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");
    song_keys(
        Some(&t.path),
        t.first("TITLE").unwrap_or_default(),
        &artists,
        t.first("ISRC"),
    )
}

/// A wanted song's keys.
pub fn want_keys(w: &WantEntry) -> Vec<String> {
    song_keys(None, &w.title, &w.artist, w.isrc.as_deref())
}

/// An online song's keys (title/artists/ISRC, plus its stable provider id).
pub fn online_keys(o: &OnlineEntry) -> Vec<String> {
    let mut k = song_keys(None, &o.title, &o.artists.join("; "), o.isrc.as_deref());
    if o.is_stable() {
        k.push(short_key(&format!("o:{}:{}", o.provider, o.track_id)));
    }
    k
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistEntry {
    /// Library-relative path, or empty when the track is missing.
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub audio_md5: Option<String>,
    pub missing: bool,
    /// For a missing song: what's known about it (from an import), to find it later.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub want: Option<WantEntry>,
    /// For a missing song: one in your library that may be it (see `suggest`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maybe: Option<Maybe>,
    /// A song from the online catalog. With `missing` it plays online; without, the library
    /// has it (`path`) and the file plays instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<OnlineEntry>,
}

impl PlaylistEntry {
    /// Back to a line to write (a missing song without details keeps its title).
    pub fn item(&self) -> Item {
        if let Some(o) = &self.online {
            return Item::online(o.clone());
        }
        if !self.missing {
            return Item::track(self.path.clone());
        }
        Item::wanted(self.want.clone().unwrap_or_else(|| WantEntry {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            isrc: None,
            md5: self.audio_md5.clone(),
            not: Vec::new(),
        }))
    }

    /// What to play for it, your own file first: the library's file when it's on disk, else
    /// the online song. None for a song still to get (it stays on the want-list).
    pub fn playable(&self, library_root: &Path) -> Option<Playable> {
        let file = (!self.missing && !self.path.is_empty())
            .then(|| library_root.join(&self.path))
            .filter(|f| f.is_file());
        match (file, &self.online) {
            (Some(file), online) => Some(Playable {
                id: PlayableId::Local {
                    path: self.path.clone(),
                },
                title: self.title.clone(),
                artists: split_names(&self.artist),
                album: Some(self.album.clone()).filter(|a| !a.is_empty()),
                duration_ms: Some(self.duration_ms).filter(|d| *d > 0),
                artwork_url: None,
                availability: Some(if online.is_some() {
                    Availability::LocalPreferred
                } else {
                    Availability::Local
                }),
                local_file: Some(file),
            }),
            (None, Some(o)) => Some(o.playable()),
            (None, None) => None,
        }
    }
}

/// A playlist as the player gets it: every song that can play, in order, each from your file
/// when you have it and online when not. Songs still to get are left out, so playing on goes
/// from one playable song to the next whether it's a file or online.
pub fn playables(library_root: &Path, entries: &[PlaylistEntry]) -> Vec<Playable> {
    entries
        .iter()
        .filter_map(|e| e.playable(library_root))
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistSummary {
    pub name: String,
    pub file: String,
    pub smart: bool,
    pub tracks: usize,
    /// Songs still to get: not in the library, and not online either.
    pub missing: usize,
    /// Songs not in the library that play online.
    pub online: usize,
    /// Where it came from ("Spotify", "Apple Music"…), if imported.
    pub source: Option<String>,
    /// Which playlist at the source, when known (to update it from there).
    pub source_id: Option<String>,
    /// The first few songs' album folders, for its mosaic cover.
    pub dirs: Vec<String>,
    pub duration_ms: u64,
    /// When its file last changed (seconds since 1970), for "recent first".
    pub modified: u64,
}

pub fn folder(library_root: &Path) -> PathBuf {
    crate::paths::beside(library_root, crate::paths::Helper::Playlists)
}

/// How an entry reaches the music from the playlist folder: `../Music/` beside a library's
/// Music folder, `../` from a Playlists folder inside the music.
fn to_music(library_root: &Path) -> &'static str {
    if crate::paths::home_of(library_root).is_some() {
        "../Music/"
    } else {
        "../"
    }
}

fn file_for(library_root: &Path, name: &str) -> PathBuf {
    folder(library_root).join(format!("{}.m3u8", crate::plan::sanitize(name, 100)))
}

/// The file a playlist is in: its `.m3u8`, or an `.m3u` another player left there.
fn existing_file(library_root: &Path, name: &str) -> PathBuf {
    let m3u8 = file_for(library_root, name);
    if m3u8.is_file() {
        return m3u8;
    }
    let m3u = m3u8.with_extension("m3u");
    if m3u.is_file() { m3u } else { m3u8 }
}

/// "03 - Tum Hi Ho.flac" → "Tum Hi Ho".
pub(crate) fn title_from_file(p: &str) -> String {
    let name = p.rsplit(['/', '\\']).next().unwrap_or(p);
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let re = regex::Regex::new(r"^\s*\d{1,3}\s*[-._ ]\s*").expect("valid");
    re.replace(stem, "").trim().to_owned()
}

/// What a playlist line says about a song whose file isn't here: its `#EXTINF` ("Artist -
/// Title" and the length), else the file and folder names.
fn want_from_line(location: &str, extinf: Option<(u64, String)>, md5: Option<String>) -> WantEntry {
    let norm = location.replace('\\', "/");
    let mut parts = norm.rsplit('/');
    let file = parts.next().unwrap_or_default();
    let folder = parts.next().unwrap_or_default();
    let year = regex::Regex::new(r"\s*[(\[]\d{4}[)\]]\s*$").expect("valid");
    let album = if folder.is_empty() || folder == ".." || folder.ends_with(':') {
        String::new()
    } else {
        year.replace(folder, "").trim().to_owned()
    };
    let (ms, label) = extinf.unwrap_or_default();
    let (artist, title) = match label.trim().split_once(" - ") {
        Some((a, t)) if !t.trim().is_empty() => (a.trim().to_owned(), t.trim().to_owned()),
        _ if !label.trim().is_empty() => (String::new(), label.trim().to_owned()),
        _ => (String::new(), title_from_file(file)),
    };
    WantEntry {
        title,
        artist,
        album,
        duration_ms: ms,
        isrc: None,
        md5,
        not: Vec::new(),
    }
}

/// What the index knows, for resolving entries: path → track, and audio MD5 → path.
struct Resolver<'a> {
    lib: &'a Library,
    look: &'a crate::library::Lookup,
}

impl<'a> Resolver<'a> {
    fn new(lib: &'a Library) -> Self {
        Self {
            lib,
            look: lib.lookup(),
        }
    }

    fn path(&self, p: &str) -> Option<&'a Track> {
        self.look.by_path.get(p).map(|&i| &self.lib.tracks[i])
    }

    /// `m` in upper-case hex.
    fn md5(&self, m: &str) -> Option<&'a Track> {
        self.look.by_md5.get(m).map(|&i| &self.lib.tracks[i])
    }
}

fn hex(b: &[u8]) -> String {
    crate::library::hex_upper(b)
}

/// Read a playlist; entries whose file moved are found again by audio checksum.
/// Returns the entries and whether any path was repaired.
pub fn read(
    library_root: &Path,
    lib: &Library,
    name: &str,
) -> Result<(Vec<PlaylistEntry>, bool), String> {
    let text =
        crate::text::read_text(&existing_file(library_root, name)).map_err(|e| e.to_string())?;
    let r = Resolver::new(lib);
    let mut out = Vec::new();
    let mut pending_md5: Option<String> = None;
    let mut pending_info: Option<(u64, String)> = None;
    let mut repaired = false;
    // Made only for a playlist with online songs, to find them in the library first.
    let mut matcher: Option<Matcher> = None;
    // Paths written in full (another player's list) still count when they're in this library.
    let root_prefix = format!(
        "{}/",
        library_root
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    );
    for line in text.lines().map(str::trim) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (secs, label) = info.split_once(',').unwrap_or((info, ""));
            let secs = secs.trim().parse::<i64>().ok().filter(|s| *s > 0);
            pending_info = Some((secs.unwrap_or(0) as u64 * 1000, label.trim().to_owned()));
            continue;
        }
        if let Some(m) = line
            .strip_prefix(MD5_TAG)
            .or_else(|| line.strip_prefix(OLD_MD5_TAG))
        {
            pending_md5 = Some(m.trim().to_uppercase());
            continue;
        }
        if let Some(j) = line.strip_prefix(WANT_TAG) {
            pending_info = None;
            if let Ok(w) = serde_json::from_str::<WantEntry>(j.trim()) {
                // The file it was waiting for is back.
                if let Some(t) = w.md5.as_ref().and_then(|m| r.md5(&m.to_uppercase())) {
                    repaired = true;
                    out.push(entry(t));
                    pending_md5 = None;
                    continue;
                }
                out.push(PlaylistEntry {
                    path: String::new(),
                    title: w.title.clone(),
                    artist: w.artist.clone(),
                    album: w.album.clone(),
                    duration_ms: w.duration_ms,
                    audio_md5: None,
                    missing: true,
                    want: Some(w),
                    maybe: None,
                    online: None,
                });
            }
            pending_md5 = None;
            continue;
        }
        if line.starts_with(ONLINE_PLAYLIST_TAG) {
            pending_info = None;
            pending_md5 = None;
            if let Some(o) = decode_online(line) {
                let m = matcher.get_or_insert_with(|| Matcher::new(lib));
                out.push(online_entry(o, m));
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let location = line;
        let line = line.replace('\\', "/");
        let rel = if line.to_lowercase().starts_with(&root_prefix) {
            line[root_prefix.len()..].to_owned()
        } else {
            line.strip_prefix(to_music(library_root))
                .unwrap_or_else(|| line.trim_start_matches("../"))
                .to_owned()
        };
        let md5 = pending_md5.take();
        let info = pending_info.take();
        let track = r.path(&rel).or_else(|| {
            let t = md5.as_ref().and_then(|m| r.md5(m));
            repaired |= t.is_some();
            t
        });
        out.push(match track {
            Some(t) => entry(t),
            None => {
                // Not here: kept as a song to get, with what the line says about it.
                let w = want_from_line(location, info, md5.clone());
                PlaylistEntry {
                    path: String::new(),
                    title: w.title.clone(),
                    artist: w.artist.clone(),
                    album: w.album.clone(),
                    duration_ms: w.duration_ms,
                    audio_md5: md5,
                    missing: true,
                    want: Some(w),
                    maybe: None,
                    online: None,
                }
            }
        });
    }
    Ok((out, repaired))
}

fn entry(t: &Track) -> PlaylistEntry {
    PlaylistEntry {
        path: t.path.clone(),
        title: t.first("TITLE").unwrap_or_default().to_owned(),
        artist: t.first("ARTIST").unwrap_or_default().to_owned(),
        album: t.first("ALBUM").unwrap_or_default().to_owned(),
        duration_ms: t.duration_ms,
        audio_md5: t.audio_md5.as_deref().map(hex),
        missing: false,
        want: None,
        maybe: None,
        online: None,
    }
}

/// An online song, as the library's own file when it has the very same song (a sure match
/// only), else as one that plays online.
fn online_entry(o: OnlineEntry, m: &Matcher) -> PlaylistEntry {
    if let Some(h) = m.grade(&o.foreign()).filter(|h| h.grade == Grade::Exact) {
        return PlaylistEntry {
            online: Some(o),
            ..entry(h.track)
        };
    }
    PlaylistEntry {
        path: String::new(),
        title: o.title.clone(),
        artist: o.artists.join(", "),
        album: o.album.clone().unwrap_or_default(),
        duration_ms: o.duration_ms.unwrap_or(0),
        audio_md5: None,
        missing: true,
        // Not wanted: it plays, so it isn't on the list of songs to get.
        want: None,
        maybe: None,
        online: Some(o),
    }
}

/// For each missing song with details, a library song that may be it (not one you turned
/// down), close matches included: a different version, length or credit.
pub fn suggest(lib: &Library, entries: &mut [PlaylistEntry]) {
    let m = Matcher::new(lib);
    for e in entries.iter_mut().filter(|e| e.missing) {
        let Some(w) = &e.want else { continue };
        e.maybe = m
            .grade(&w.foreign())
            .filter(|h| !w.not.iter().any(|p| p == &h.track.path))
            .map(|h| Maybe {
                path: h.track.path.clone(),
                title: h.track.first("TITLE").unwrap_or_default().to_owned(),
                artist: h.track.first("ARTIST").unwrap_or_default().to_owned(),
                album: h.track.first("ALBUM").unwrap_or_default().to_owned(),
                duration_ms: h.track.duration_ms,
                why: h.why,
            });
    }
}

/// Where a playlist came from, from its file's text.
fn source_in(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix(SOURCE_TAG))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// A playlist's source line, read from its file.
pub fn source_of(library_root: &Path, name: &str) -> Option<String> {
    crate::text::read_text(&existing_file(library_root, name))
        .ok()
        .and_then(|t| source_in(&t))
}

/// Is there a playlist by this name?
pub fn exists(library_root: &Path, name: &str) -> bool {
    existing_file(library_root, name).is_file()
}

/// Make a new, empty playlist (a draft given its name or its first song); never over one that
/// exists, under that name or one that is saved as the same file.
pub fn create(library_root: &Path, lib: &Library, name: &str) -> Result<(), String> {
    if crate::plan::sanitize(name, 100).is_empty() {
        return Err("A name needs at least one letter or number.".into());
    }
    if exists(library_root, name) {
        return Err(format!("A playlist called “{name}” already exists."));
    }
    write_items(library_root, lib, name, &[], false, None)
}

/// Add songs to the end of a playlist (making it if it's new), skipping ones already in it:
/// the same file, or the same online song (by its id, never an address). Returns how many
/// were added.
pub fn add(
    library_root: &Path,
    lib: &Library,
    name: &str,
    items: &[Item],
) -> Result<usize, String> {
    add_with(library_root, lib, name, items, false)
}

/// `add`, and with `again` the songs already in it are added once more (asked for, after
/// "already in this playlist").
pub fn add_with(
    library_root: &Path,
    lib: &Library,
    name: &str,
    items: &[Item],
    again: bool,
) -> Result<usize, String> {
    let entries = if exists(library_root, name) {
        read(library_root, lib, name)?.0
    } else {
        vec![]
    };
    let mut paths: HashSet<String> = entries
        .iter()
        .filter(|e| !e.missing)
        .map(|e| e.path.clone())
        .collect();
    let mut ids: HashSet<PlayableId> = entries
        .iter()
        .filter_map(|e| e.online.as_ref().map(OnlineEntry::id))
        .collect();
    let added: Vec<Item> = items
        .iter()
        .filter(|i| match (&i.online, &i.path) {
            (Some(o), _) => o.is_stable() && (ids.insert(o.id()) || again),
            (None, Some(p)) => !p.is_empty() && (paths.insert(p.clone()) || again),
            (None, None) => i.want.is_some(),
        })
        .cloned()
        .collect();
    let mut all: Vec<Item> = entries.iter().map(PlaylistEntry::item).collect();
    all.extend(added.iter().cloned());
    write_items(
        library_root,
        lib,
        name,
        &all,
        false,
        source_of(library_root, name).as_deref(),
    )?;
    Ok(added.len())
}

/// Write a static playlist from library-relative paths (keeping the source it came from).
pub fn write(
    library_root: &Path,
    lib: &Library,
    name: &str,
    paths: &[String],
    smart: bool,
) -> Result<(), String> {
    let items: Vec<Item> = paths.iter().map(|p| Item::track(p.clone())).collect();
    let source = if smart {
        None
    } else {
        source_of(library_root, name)
    };
    write_items(library_root, lib, name, &items, smart, source.as_deref())
}

/// Write a playlist from songs in the library and songs still wanted, in order. A wanted song
/// keeps its place as a comment line; so does a path the library doesn't have (never dropped).
pub fn write_items(
    library_root: &Path,
    lib: &Library,
    name: &str,
    items: &[Item],
    smart: bool,
    source: Option<&str>,
) -> Result<(), String> {
    // Where it came from stays with it through every edit.
    let mut header = if smart {
        Header::default()
    } else {
        header_of(library_root, name)
    };
    header.source = source.map(str::to_owned);
    write_with(library_root, lib, name, items, smart, &header)
}

/// Write a playlist with these lines about itself (see `Header`).
pub fn write_with(
    library_root: &Path,
    lib: &Library,
    name: &str,
    items: &[Item],
    smart: bool,
    header: &Header,
) -> Result<(), String> {
    let by_path: HashMap<&str, &Track> = lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
    let mut out = String::from("#EXTM3U\n");
    out.push_str(&format!("#PLAYLIST:{name}\n"));
    if smart {
        out.push_str(SMART_TAG);
        out.push_str(" written from your rules; edit the rule, not this file\n");
    }
    if let Some(s) = header.source.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("{SOURCE_TAG}{s}\n"));
    }
    if let Some(s) = header.source_id.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("{SOURCE_ID_TAG}{s}\n"));
    }
    if let Some(base) = header.base.as_ref().filter(|b| !b.is_empty()) {
        let songs: Vec<String> = base
            .iter()
            .filter(|k| !k.is_empty())
            .map(|k| k.join("."))
            .collect();
        out.push_str(&format!("{BASE_TAG}{}\n", songs.join(" ")));
    }
    for item in items {
        if let Some(o) = &item.online {
            if o.is_stable() {
                out.push_str(&encode_online(o));
                out.push('\n');
            } else {
                // No real id (an empty one, or an address): kept as a song to get instead.
                let w = WantEntry {
                    title: o.title.clone(),
                    artist: o.artists.join(", "),
                    album: o.album.clone().unwrap_or_default(),
                    duration_ms: o.duration_ms.unwrap_or(0),
                    isrc: o.isrc.clone(),
                    ..Default::default()
                };
                out.push_str(&format!(
                    "{WANT_TAG}{}\n",
                    serde_json::to_string(&w).map_err(|e| e.to_string())?
                ));
            }
            continue;
        }
        if let Some(w) = &item.want {
            out.push_str(&format!(
                "{WANT_TAG}{}\n",
                serde_json::to_string(w).map_err(|e| e.to_string())?
            ));
            continue;
        }
        let Some(p) = &item.path else {
            continue;
        };
        let Some(t) = by_path.get(p.as_str()) else {
            let w = want_from_line(p, None, None);
            out.push_str(&format!(
                "{WANT_TAG}{}\n",
                serde_json::to_string(&w).map_err(|e| e.to_string())?
            ));
            continue;
        };
        let secs = (t.duration_ms + 500) / 1000;
        out.push_str(&format!(
            "#EXTINF:{secs},{} - {}\n",
            t.first("ARTIST").unwrap_or_default(),
            t.first("TITLE").unwrap_or_default()
        ));
        if let Some(m) = &t.audio_md5 {
            out.push_str(&format!("{MD5_TAG}{}\n", hex(m)));
        }
        out.push_str(&format!("{}{p}\n", to_music(library_root)));
    }
    let path = file_for(library_root, name);
    std::fs::create_dir_all(folder(library_root)).map_err(|e| e.to_string())?;
    // Smart playlists are written again on every visit to Playlists: one that says the same is
    // left alone, so it isn't a change (to rescan, to sync, for other players to reload).
    if !std::fs::read(&path).is_ok_and(|now| now == out.as_bytes()) {
        let part = path.with_extension("m3u8.part");
        std::fs::write(&part, out).map_err(|e| e.to_string())?;
        std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    }
    // An .m3u it was read from is now an .m3u8: the old file goes aside, not left as a twin.
    let old = path.with_extension("m3u");
    if old.is_file() {
        set_aside(library_root, &old, name)?;
    }
    Ok(())
}

pub fn list(library_root: &Path, lib: &Library) -> Vec<PlaylistSummary> {
    let mut out: Vec<PlaylistSummary> = std::fs::read_dir(folder(library_root))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("m3u8") || x.eq_ignore_ascii_case("m3u"))
        })
        .filter_map(|e| {
            let name = e.path().file_stem()?.to_string_lossy().into_owned();
            let text = crate::text::read_text(&e.path()).ok()?;
            let (entries, _) = read(library_root, lib, &name).ok()?;
            // Up to four different albums from the top, for the mosaic cover.
            let mut dirs: Vec<String> = Vec::new();
            for en in entries.iter().filter(|x| !x.missing) {
                let dir = en
                    .path
                    .rsplit_once('/')
                    .map(|(d, _)| d.to_owned())
                    .unwrap_or_default();
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
                if dirs.len() == 4 {
                    break;
                }
            }
            Some(PlaylistSummary {
                file: e.path().to_string_lossy().into_owned(),
                smart: text.contains(SMART_TAG) || text.contains(OLD_SMART_TAG),
                tracks: entries.len(),
                missing: entries
                    .iter()
                    .filter(|x| x.missing && x.online.is_none())
                    .count(),
                online: entries
                    .iter()
                    .filter(|x| x.missing && x.online.is_some())
                    .count(),
                source: source_in(&text),
                source_id: header_in(&text).source_id,
                duration_ms: entries
                    .iter()
                    .filter(|x| !x.missing || x.online.is_some())
                    .map(|x| x.duration_ms)
                    .sum(),
                dirs,
                name,
                modified: e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    // "Road.m3u" beside "Road.m3u8" is one playlist (the .m3u8 is the one read).
    out.dedup_by(|b, a| {
        let same = a.name.eq_ignore_ascii_case(&b.name);
        if same && b.file.to_lowercase().ends_with(".m3u8") {
            std::mem::swap(a, b);
        }
        same
    });
    out
}

/// Re-write playlists whose entries were found again by checksum, so other players see the
/// right paths too. Returns how many were repaired.
pub fn repair_all(library_root: &Path, lib: &Library) -> usize {
    let mut n = 0;
    for p in list(library_root, lib).into_iter().filter(|p| !p.smart) {
        if let Ok((entries, true)) = read(library_root, lib, &p.name) {
            // Wanted songs and the source stay; only the moved paths change.
            let items: Vec<Item> = entries.iter().map(PlaylistEntry::item).collect();
            if write_items(
                library_root,
                lib,
                &p.name,
                &items,
                false,
                p.source.as_deref(),
            )
            .is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

/// Where a deleted playlist went, and where it was: what Undo needs to put it back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Deleted {
    pub aside: PathBuf,
    pub was: PathBuf,
}

/// Deleted playlists are moved aside, never destroyed.
pub fn delete(library_root: &Path, name: &str) -> Result<Deleted, String> {
    let was = existing_file(library_root, name);
    let aside = set_aside(library_root, &was, name)?;
    Ok(Deleted { aside, was })
}

/// Undo a delete: the file moves back from Set aside to where it was, the same bytes under the
/// same name. Never over a playlist made since, and only between Set aside and the playlists.
pub fn undelete(library_root: &Path, name: &str, aside: &Path, was: &Path) -> Result<(), String> {
    let same_dir = |p: &Path, dir: PathBuf| {
        p.parent()
            .and_then(|d| d.canonicalize().ok())
            .zip(dir.canonicalize().ok())
            .is_some_and(|(d, want)| d == want)
    };
    let set_aside_dir =
        crate::paths::beside(library_root, crate::paths::Helper::SetAside).join("playlists");
    if !aside.is_file() || !same_dir(aside, set_aside_dir) {
        return Err(format!("The deleted “{name}” isn't in Set aside any more."));
    }
    if !same_dir(was, folder(library_root)) {
        return Err("That isn't a place for a playlist.".into());
    }
    if was.exists() || exists(library_root, name) {
        return Err(format!(
            "A playlist called “{name}” has been made since. Rename it, then undo again."
        ));
    }
    std::fs::rename(aside, was).map_err(|e| e.to_string())
}

/// Where a playlist's file goes in Set aside, under a name that is never taken.
fn aside_path(library_root: &Path, src: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = crate::paths::beside(library_root, crate::paths::Helper::SetAside).join("playlists");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "m3u8".into());
    let base = format!("{stamp}-{}", crate::plan::sanitize(name, 100));
    let mut to = dir.join(format!("{base}.{ext}"));
    for n in 2.. {
        if !to.exists() {
            break;
        }
        to = dir.join(format!("{base}-{n}.{ext}"));
    }
    Ok(to)
}

/// Move a playlist file to Set aside; returns where it went.
fn set_aside(library_root: &Path, src: &Path, name: &str) -> Result<PathBuf, String> {
    let to = aside_path(library_root, src, name)?;
    std::fs::rename(src, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

/// Keep a copy of a playlist as it is now in Set aside, before a change; returns where.
pub fn keep_copy(library_root: &Path, name: &str) -> Result<PathBuf, String> {
    let src = existing_file(library_root, name);
    let to = aside_path(library_root, &src, name)?;
    std::fs::copy(&src, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

/// Put back a copy kept by `keep_copy` (undo); the playlist as it is now goes aside first.
pub fn restore_copy(library_root: &Path, name: &str, copy: &Path) -> Result<(), String> {
    let aside =
        crate::paths::beside(library_root, crate::paths::Helper::SetAside).join("playlists");
    let inside = copy
        .parent()
        .zip(aside.canonicalize().ok())
        .and_then(|(p, a)| p.canonicalize().ok().map(|p| p == a))
        .unwrap_or(false);
    if !inside || !copy.is_file() {
        return Err("That earlier version isn't in Set aside any more.".into());
    }
    let now = existing_file(library_root, name);
    if now.is_file() {
        set_aside(library_root, &now, name)?;
    }
    std::fs::copy(copy, file_for(library_root, name)).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn rename(library_root: &Path, from: &str, to: &str) -> Result<(), String> {
    let dst = file_for(library_root, to);
    if dst.exists() {
        return Err("A playlist with that name already exists.".into());
    }
    let src = existing_file(library_root, from);
    let text = crate::text::read_text(&src).map_err(|e| e.to_string())?;
    // Other players show the #PLAYLIST name, so it changes with the file name.
    let renamed: String = text
        .lines()
        .map(|l| {
            if l.starts_with("#PLAYLIST:") {
                format!("#PLAYLIST:{to}\n")
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    std::fs::write(&dst, renamed).map_err(|e| e.to_string())?;
    std::fs::remove_file(&src).map_err(|e| e.to_string())
}

/// A name not yet taken: "Road trip", then "Road trip 2", "Road trip 3"…
pub fn free_name(library_root: &Path, wanted: &str) -> String {
    let base = wanted.trim();
    if !exists(library_root, base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !exists(library_root, n))
        .expect("some number is free")
}

/// A copy of a playlist under a new name (a smart one becomes an ordinary playlist).
pub fn duplicate(
    library_root: &Path,
    lib: &Library,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let (entries, _) = read(library_root, lib, from)?;
    let items: Vec<Item> = entries.iter().map(PlaylistEntry::item).collect();
    let name = free_name(library_root, to);
    write_items(
        library_root,
        lib,
        &name,
        &items,
        false,
        source_of(library_root, from).as_deref(),
    )?;
    Ok(name)
}

/// Save a playlist for use outside the library (another app, a phone): full paths to the
/// files, and only songs that are here. Returns how many songs it holds.
pub fn export(library_root: &Path, lib: &Library, name: &str, to: &Path) -> Result<usize, String> {
    let (entries, _) = read(library_root, lib, name)?;
    let mut out = format!("#EXTM3U\n#PLAYLIST:{name}\n");
    let mut n = 0;
    for e in entries.iter().filter(|e| !e.missing) {
        out.push_str(&format!(
            "#EXTINF:{},{} - {}\n{}\n",
            (e.duration_ms + 500) / 1000,
            e.artist,
            e.title,
            library_root.join(&e.path).to_string_lossy()
        ));
        n += 1;
    }
    std::fs::write(to, out).map_err(|e| e.to_string())?;
    Ok(n)
}

// --- Matching streaming tracks to local files ----------------------------------------------

/// Normalise a title for matching: no "(From …)", "(feat …)", remaster notes, punctuation.
pub fn title_key(s: &str) -> String {
    crate::audit::name_key(&title_cut(s))
}

/// [`title_key`] in the loose spelling shared with the search box ("Kesariyaa" = "Kesariya"):
/// a second way to find a song when its title is spelled differently. Never stored.
pub fn title_fold_key(s: &str) -> String {
    crate::songid::fold_key(&title_cut(s))
}

/// The title, lower case, with "(From …)", "(feat …)" and remaster notes cut off.
fn title_cut(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut cut = lower.as_str();
    for marker in [
        " (from",
        " [from",
        " (feat",
        " [feat",
        " (ft.",
        " - from",
        " (remaster",
        " - remaster",
    ] {
        if let Some(i) = cut.find(marker) {
            cut = &cut[..i];
        }
    }
    cut.to_owned()
}

/// A title with every version note taken off ("(Remix)", "- Live", "[Acoustic]"…), to find
/// another version of the same song. Only ever offered as a "close" match, for you to confirm.
pub fn loose_key(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut depth = 0;
    for c in lower.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.split(" - ").next().unwrap_or(&out).to_owned();
    let words: Vec<&str> = out
        .split_whitespace()
        .filter(|w| {
            !matches!(
                *w,
                "remix"
                    | "reprise"
                    | "unplugged"
                    | "acoustic"
                    | "live"
                    | "version"
                    | "edit"
                    | "mix"
                    | "remastered"
                    | "remaster"
                    | "radio"
            )
        })
        .collect();
    crate::audit::name_key(&words.join(" "))
}

/// A song from elsewhere (a streaming playlist, another player's file).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Foreign {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub isrc: Option<String>,
    /// The file another player's playlist pointed at, when it exists on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// A YouTube video still waiting for its title (Google Takeout lists only IDs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub youtube: Option<String>,
    /// The MusicBrainz recording (ListenBrainz and XSPF lists carry it): as sure as an ISRC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
}

impl Foreign {
    /// What to keep in the playlist while it isn't in the library.
    pub fn want(&self) -> WantEntry {
        WantEntry {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms.unwrap_or(0),
            isrc: self.isrc.clone(),
            md5: None,
            not: Vec::new(),
        }
    }

    /// A stable online id this import already carries (e.g. a Takeout video id). Never a stream URL.
    /// Provider is the hybrid catalog name so resolve uses the same path as search.
    pub fn online(&self) -> Option<OnlineEntry> {
        let id = self.youtube.as_deref()?.trim();
        if id.is_empty() || id.contains("://") {
            return None;
        }
        // YouTube video ids are 11 chars of [A-Za-z0-9_-]; reject junk and playlist ids.
        if id.len() != 11
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        let title = self.title.trim();
        let title = if title.is_empty() {
            format!("Video {id}")
        } else {
            title.to_owned()
        };
        let artists: Vec<String> = split_names(&self.artist)
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty())
            .collect();
        Some(OnlineEntry {
            provider: "remote_catalog".into(),
            track_id: id.to_owned(),
            title,
            artists,
            album: Some(self.album.clone()).filter(|a| !a.trim().is_empty()),
            duration_ms: self.duration_ms.filter(|d| *d > 0),
            isrc: self.isrc.clone(),
            // Cover picture only; googlevideo stream hosts are rejected on write.
            artwork_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
        })
    }
}

/// How sure a match is: exact ones are used as they are; close ones are offered for you to
/// confirm (another version, a different artist credit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Exact,
    Close,
}

#[derive(Debug, Clone)]
pub struct Found<'a> {
    pub track: &'a Track,
    pub grade: Grade,
    /// Why it was matched, in words ("Same recording code", "Another version: …").
    pub why: String,
}

pub struct Matcher<'a> {
    by_title: HashMap<String, Vec<&'a Track>>,
    /// The same, by the loose spelling: titles spelled differently ("Kesariyaa").
    by_fold: HashMap<String, Vec<&'a Track>>,
    by_loose: HashMap<String, Vec<&'a Track>>,
    by_isrc: HashMap<String, &'a Track>,
    /// MusicBrainz recording ids (the `MUSICBRAINZ_TRACKID` tag Picard writes).
    by_mbid: HashMap<String, &'a Track>,
}

/// An artist name as it's compared: as written, and in the loose spelling ("Arijeet" =
/// "Arijit"), marked apart so the two never mix.
fn artist_keys(name: &str) -> [String; 2] {
    [
        crate::audit::name_key(name),
        format!("~{}", crate::songid::fold_key(name)),
    ]
}

impl<'a> Matcher<'a> {
    pub fn new(lib: &'a Library) -> Self {
        let mut by_title: HashMap<String, Vec<&Track>> = HashMap::new();
        let mut by_fold: HashMap<String, Vec<&Track>> = HashMap::new();
        let mut by_loose: HashMap<String, Vec<&Track>> = HashMap::new();
        let mut by_isrc: HashMap<String, &Track> = HashMap::new();
        let mut by_mbid: HashMap<String, &Track> = HashMap::new();
        for t in lib.tracks.iter() {
            if let Some(m) = t
                .first("MUSICBRAINZ_TRACKID")
                .map(|m| m.trim().to_lowercase())
                .filter(|m| m.len() == 36)
            {
                by_mbid.entry(m).or_insert(t);
            }
            if let Some(title) = t.first("TITLE") {
                by_title.entry(title_key(title)).or_default().push(t);
                by_fold.entry(title_fold_key(title)).or_default().push(t);
                by_loose.entry(loose_key(title)).or_default().push(t);
            }
            if let Some(i) = t
                .first("ISRC")
                .map(|i| i.trim().to_uppercase())
                .filter(|i| i.len() >= 12)
            {
                by_isrc.entry(i).or_insert(t);
            }
        }
        Self {
            by_title,
            by_fold,
            by_loose,
            by_isrc,
            by_mbid,
        }
    }

    /// `artists` holds [`artist_keys`] of every name the song from elsewhere credits.
    fn shares_artist(t: &Track, artists: &HashSet<String>) -> bool {
        t.first("ARTIST")
            .map(split_names)
            .unwrap_or_default()
            .iter()
            .chain(
                t.first("ALBUMARTIST")
                    .map(split_names)
                    .unwrap_or_default()
                    .iter(),
            )
            .any(|a| artist_keys(a).iter().any(|k| artists.contains(k)))
    }

    /// The one nearest in length, if lengths are known (else the first).
    fn nearest<'b>(pool: &[&'b Track], ms: Option<u64>) -> &'b Track {
        match ms {
            Some(ms) => pool
                .iter()
                .min_by_key(|t| t.duration_ms.abs_diff(ms))
                .copied()
                .expect("not empty"),
            None => pool[0],
        }
    }

    /// Of the songs with the title asked for, which one, and how sure. `spelled`: the title
    /// matched only in the loose spelling, so without a length to agree it's never "found".
    fn same_title(
        &self,
        cands: &[&'a Track],
        f: &Foreign,
        artists: &HashSet<String>,
        spelled: bool,
    ) -> Option<Found<'a>> {
        let near = |t: &Track| {
            f.duration_ms
                .is_none_or(|ms| t.duration_ms.abs_diff(ms) <= 3000)
        };
        let sure = |t: &Track| near(t) && (!spelled || f.duration_ms.is_some());
        let spelled_note = if spelled { ", spelled differently" } else { "" };
        let mine: Vec<&Track> = cands
            .iter()
            .copied()
            .filter(|t| Self::shares_artist(t, artists))
            .collect();
        if mine.is_empty() {
            // The title alone: only one song, and the same length, is worth offering.
            return (!spelled && cands.len() == 1 && f.duration_ms.is_some() && near(cands[0]))
                .then(|| Found {
                    track: cands[0],
                    grade: Grade::Close,
                    why: "Same song and length, credited to someone else".into(),
                });
        }
        // The album agrees: when it has the title more than once (an edit and the full song, two
        // remixes), the length says which.
        let album = title_key(&f.album);
        let on_album: Vec<&Track> = mine
            .iter()
            .copied()
            .filter(|t| {
                !album.is_empty() && t.first("ALBUM").is_some_and(|a| title_key(a) == album)
            })
            .collect();
        if !on_album.is_empty() {
            let t = Self::nearest(&on_album, f.duration_ms);
            let exact = sure(t) && (on_album.len() == 1 || f.duration_ms.is_some());
            let why = if exact {
                format!("Same song, artist and album{spelled_note}")
            } else if on_album.len() > 1 {
                format!(
                    "The album has it {} times; this is the nearest in length",
                    on_album.len()
                )
            } else {
                format!("Same song, artist and album{spelled_note}, a different length")
            };
            return Some(Found {
                track: t,
                grade: if exact { Grade::Exact } else { Grade::Close },
                why,
            });
        }
        let close: Vec<&Track> = mine.iter().copied().filter(|t| near(t)).collect();
        if close.len() == 1 || (mine.len() == 1 && close.len() <= 1) {
            let t = close.first().copied().unwrap_or(mine[0]);
            let exact = sure(t);
            return Some(Found {
                track: t,
                grade: if exact { Grade::Exact } else { Grade::Close },
                why: if near(t) {
                    format!("Same song and artist{spelled_note}")
                } else {
                    format!("Same song and artist{spelled_note}, a different length")
                },
            });
        }
        let t = Self::nearest(&mine, f.duration_ms);
        Some(Found {
            track: t,
            grade: Grade::Close,
            why: format!(
                "You have {} versions; this is the nearest in length",
                mine.len()
            ),
        })
    }

    /// Find a song from elsewhere in the library, and say how sure the match is.
    pub fn grade(&self, f: &Foreign) -> Option<Found<'a>> {
        // 1. The same recording code is the same recording.
        if let Some(t) = f
            .isrc
            .as_ref()
            .and_then(|i| self.by_isrc.get(&i.trim().to_uppercase()))
        {
            return Some(Found {
                track: t,
                grade: Grade::Exact,
                why: "Same recording code (ISRC)".into(),
            });
        }
        if let Some(t) = f
            .mbid
            .as_ref()
            .and_then(|m| self.by_mbid.get(&m.trim().to_lowercase()))
        {
            return Some(Found {
                track: t,
                grade: Grade::Exact,
                why: "Same MusicBrainz recording".into(),
            });
        }
        let artists: HashSet<String> = split_names(&f.artist)
            .iter()
            .filter(|a| !crate::audit::name_key(a).is_empty())
            .flat_map(|a| artist_keys(a))
            .collect();
        // 2. The same title by the same artist: the album or the length settles twins.
        if let Some(cands) = self.by_title.get(&title_key(&f.title))
            && let Some(found) = self.same_title(cands, f, &artists, false)
        {
            return Some(found);
        }
        // 2b. The same, spelled differently ("Kesariyaa", "Arijeet"): sure only when the length
        // agrees too.
        let fold = title_fold_key(&f.title);
        if !fold.is_empty()
            && let Some(cands) = self.by_fold.get(&fold)
            && let Some(found) = self.same_title(cands, f, &artists, true)
        {
            return Some(found);
        }
        // 3. Another version of the song by the same artist.
        let loose = loose_key(&f.title);
        if !loose.is_empty()
            && let Some(cands) = self.by_loose.get(&loose)
        {
            let mine: Vec<&Track> = cands
                .iter()
                .copied()
                .filter(|t| Self::shares_artist(t, &artists))
                .collect();
            if !mine.is_empty() {
                let t = Self::nearest(&mine, f.duration_ms);
                return Some(Found {
                    track: t,
                    grade: Grade::Close,
                    why: format!("Another version: {}", t.first("TITLE").unwrap_or_default()),
                });
            }
        }
        None
    }

    /// Same title, then a shared artist name, then the album as a tiebreak. Only one clear
    /// candidate counts as a match.
    pub fn find(&self, title: &str, artist: &str, album: &str) -> Option<&'a Track> {
        let cands = self.by_title.get(&title_key(title))?;
        let artists: HashSet<String> = split_names(artist)
            .iter()
            .map(|a| crate::audit::name_key(a))
            .collect();
        let shares_artist = |t: &&Track| {
            t.first("ARTIST")
                .map(split_names)
                .unwrap_or_default()
                .iter()
                .any(|a| artists.contains(&crate::audit::name_key(a)))
        };
        let with_artist: Vec<&Track> = cands.iter().copied().filter(shares_artist).collect();
        let pool = if with_artist.is_empty() {
            cands.clone()
        } else {
            with_artist
        };
        let album_key = title_key(album);
        if let Some(t) = pool
            .iter()
            .find(|t| t.first("ALBUM").is_some_and(|a| title_key(a) == album_key))
        {
            return Some(t);
        }
        (pool.len() == 1).then(|| pool[0])
    }
}

// --- Importing Apple playlists and the want-list ---------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WantedAlbum {
    pub album: String,
    pub artist: String,
    pub tier: u8,
    pub why: String,
    pub saved_tracks: usize,
    pub playlists: Vec<String>,
    /// On the list by itself (you added it, or an older import), not because a playlist
    /// still wants its songs: only these can be taken off by hand.
    #[serde(default)]
    pub kept: bool,
}

/// Songs from one album that playlists want, for the want-list.
#[derive(Debug, Clone, Default)]
pub struct WantGroup {
    pub album: String,
    pub artist: String,
    pub genre: String,
    pub saved: usize,
    pub playlists: Vec<String>,
}

/// Albums your playlists want, ranked: whole albums worth getting first (several saved songs,
/// an artist you already collect, a score), then ones to explore, then filler to skip.
pub fn rank_wanted(lib: &Library, groups: Vec<WantGroup>) -> Vec<WantedAlbum> {
    // Composers you already collect deeply count as reasons to get the whole album.
    let mut composer_albums: HashMap<String, HashSet<&str>> = HashMap::new();
    for t in &lib.tracks {
        if let Some(aa) = t.first("ALBUMARTIST") {
            composer_albums
                .entry(crate::audit::name_key(aa))
                .or_default()
                .insert(t.dir.as_str());
        }
    }
    let filler = regex::Regex::new(r"(?i)karaoke|sleep|lullab|cover|relax|meditat|tribute|for study|piano version|lo-?fi beats|8d audio").expect("valid");
    let mut wanted = Vec::new();
    for g in groups {
        let WantGroup {
            album,
            artist,
            genre,
            saved,
            playlists,
        } = g;
        let genre = genre.to_lowercase();
        let core = composer_albums
            .get(&crate::audit::name_key(&artist))
            .is_some_and(|a| a.len() >= 3);
        let (tier, why) = if filler.is_match(&album) || filler.is_match(&artist) {
            (
                3,
                "Covers, karaoke or sleep music: look for the original instead.".to_owned(),
            )
        } else if saved >= 3 {
            (1, format!("You saved {saved} songs from it."))
        } else if core {
            (1, format!("{artist} is already one of your core artists."))
        } else if genre.contains("soundtrack")
            || genre.contains("score")
            || genre.contains("instrumental")
        {
            (1, "A score: worth having whole.".to_owned())
        } else {
            (
                2,
                format!(
                    "You saved {saved} {}: explore the rest.",
                    if saved == 1 { "song" } else { "songs" }
                ),
            )
        };
        wanted.push(WantedAlbum {
            album,
            artist,
            tier,
            why,
            saved_tracks: saved,
            playlists,
            kept: false,
        });
    }
    wanted.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then(b.saved_tracks.cmp(&a.saved_tracks))
    });
    wanted
}

/// The want-list as CSV, for shopping or ripping CDs.
/// The albums your playlists want right now, read from their wanted songs and ranked. Found,
/// removed or replaced songs drop out by themselves, since nothing is kept apart from the files.
pub fn wanted_now(library_root: &Path, lib: &Library) -> Vec<WantedAlbum> {
    let mut groups: BTreeMap<(String, String), (HashSet<String>, Vec<String>)> = BTreeMap::new();
    let files = std::fs::read_dir(folder(library_root))
        .into_iter()
        .flatten()
        .flatten();
    for e in files {
        let p = e.path();
        if !p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("m3u8") || x.eq_ignore_ascii_case("m3u"))
        {
            continue;
        }
        let Some(name) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let Ok((entries, _)) = read(library_root, lib, &name) else {
            continue;
        };
        for w in entries.iter().filter_map(|x| x.want.as_ref()) {
            if w.album.trim().is_empty() {
                continue;
            }
            let g = groups
                .entry((w.album.clone(), w.artist.clone()))
                .or_default();
            g.0.insert(format!(
                "{}|{}",
                w.title.to_lowercase(),
                w.artist.to_lowercase()
            ));
            if !g.1.contains(&name) {
                g.1.push(name.clone());
            }
        }
    }
    rank_wanted(
        lib,
        groups
            .into_iter()
            .map(|((album, artist), (songs, playlists))| WantGroup {
                album,
                artist,
                genre: String::new(),
                saved: songs.len(),
                playlists,
            })
            .collect(),
    )
}

pub fn wanted_csv(wanted: &[WantedAlbum]) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out = String::from("tier,album,artist,saved_tracks,why,playlists\n");
    for w in wanted {
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            w.tier,
            q(&w.album),
            q(&w.artist),
            w.saved_tracks,
            q(&w.why),
            q(&w.playlists.join("; "))
        ));
    }
    out
}

// --- Smart playlists -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRule {
    pub name: String,
    /// "all" (every condition) or "any".
    #[serde(default = "all")]
    pub r#match: String,
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub limit: Option<usize>,
}

fn all() -> String {
    "all".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// rating, genre, language, category, composer, lyricist, album_artist, year, added_days,
    /// unrated, album_has_rating
    pub field: String,
    /// is, contains, at_least, at_most
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub value: String,
}

pub fn smart_dir(rules_dir: &Path) -> PathBuf {
    rules_dir.join("playlists")
}

/// Built-in smart playlists, written once so they can be edited like any other rule.
pub fn ensure_default_smart(rules_dir: &Path) -> Result<(), String> {
    let dir = smart_dir(rules_dir);
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c = |field: &str, op: &str, value: &str| Condition {
        field: field.into(),
        op: op.into(),
        value: value.into(),
    };
    let defaults = [
        SmartRule {
            name: "Top rated".into(),
            r#match: all(),
            conditions: vec![c("rating", "at_least", "4")],
            limit: None,
        },
        SmartRule {
            name: "Explore".into(),
            r#match: all(),
            conditions: vec![c("unrated", "", ""), c("album_has_rating", "at_least", "4")],
            limit: Some(200),
        },
        SmartRule {
            name: "Recently added".into(),
            r#match: all(),
            conditions: vec![c("added_days", "at_most", "30")],
            limit: Some(300),
        },
    ];
    for r in defaults {
        let text = toml::to_string_pretty(&r).map_err(|e| e.to_string())?;
        std::fs::write(
            dir.join(format!("{}.toml", crate::plan::sanitize(&r.name, 80))),
            text,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn load_smart(rules_dir: &Path) -> Vec<SmartRule> {
    std::fs::read_dir(smart_dir(rules_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .filter_map(|e| toml::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect()
}

pub fn save_smart(rules_dir: &Path, rule: &SmartRule) -> Result<(), String> {
    std::fs::create_dir_all(smart_dir(rules_dir)).map_err(|e| e.to_string())?;
    let text = toml::to_string_pretty(rule).map_err(|e| e.to_string())?;
    std::fs::write(
        smart_dir(rules_dir).join(format!("{}.toml", crate::plan::sanitize(&rule.name, 80))),
        text,
    )
    .map_err(|e| e.to_string())
}

fn stars(t: &Track) -> u8 {
    t.first("RATING")
        .map(crate::edit::rating_to_stars)
        .unwrap_or(0)
}

/// Tracks a smart rule selects, in album order.
pub fn evaluate(lib: &Library, rule: &SmartRule, mtimes: &HashMap<i64, i64>) -> Vec<String> {
    let mut album_best: HashMap<&str, u8> = HashMap::new();
    for t in &lib.tracks {
        let e = album_best.entry(t.dir.as_str()).or_default();
        *e = (*e).max(stars(t));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let test = |t: &Track, c: &Condition| -> bool {
        let num = c.value.trim().parse::<f64>().unwrap_or(0.0);
        let cmp = |v: f64| match c.op.as_str() {
            "at_most" => v <= num,
            "is" => (v - num).abs() < f64::EPSILON,
            _ => v >= num,
        };
        let text = |key: &str| {
            let vals: Vec<String> = t.tags.get(key).cloned().unwrap_or_default();
            let want = c.value.to_lowercase();
            match c.op.as_str() {
                "is" => vals.iter().any(|v| v.to_lowercase() == want),
                _ => vals.iter().any(|v| v.to_lowercase().contains(&want)),
            }
        };
        match c.field.as_str() {
            "rating" => cmp(f64::from(stars(t))),
            "unrated" => stars(t) == 0,
            "album_has_rating" => cmp(f64::from(
                album_best.get(t.dir.as_str()).copied().unwrap_or(0),
            )),
            "genre" => text("GENRE"),
            "language" => text("LANGUAGE"),
            "category" => text("CATEGORY"),
            "composer" => text("COMPOSER"),
            "lyricist" => text("LYRICIST"),
            "album_artist" => text("ALBUMARTIST"),
            "year" => t
                .first("ORIGINALYEAR")
                .or(t.first("DATE"))
                .and_then(|d| d.get(..4)?.parse::<f64>().ok())
                .is_some_and(cmp),
            "added_days" => mtimes
                .get(&t.id)
                .is_some_and(|m| cmp(((now - m / 1_000_000_000) / 86_400) as f64)),
            _ => false,
        }
    };
    let any = rule.r#match == "any";
    let mut out: Vec<&Track> = lib
        .tracks
        .iter()
        .filter(|t| t.kind == "flac" || t.kind == "other_audio")
        .filter(|t| {
            if any {
                rule.conditions.iter().any(|c| test(t, c))
            } else {
                rule.conditions.iter().all(|c| test(t, c))
            }
        })
        .collect();
    if rule.conditions.iter().any(|c| c.field == "added_days") {
        out.sort_by_key(|t| std::cmp::Reverse(mtimes.get(&t.id).copied().unwrap_or(0)));
    }
    if let Some(n) = rule.limit {
        out.truncate(n);
    }
    out.into_iter().map(|t| t.path.clone()).collect()
}

/// Each smart rule with the tracks it selects right now. Reads only.
pub fn evaluate_all(
    rules_dir: &Path,
    lib: &Library,
    index: &crate::Index,
) -> Result<Vec<(SmartRule, Vec<String>)>, String> {
    let mut mtimes = HashMap::new();
    {
        let mut stmt = index
            .conn()
            .prepare("SELECT id, mtime_ns FROM files")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            mtimes.insert(row.0, row.1);
        }
    }
    Ok(load_smart(rules_dir)
        .into_iter()
        .map(|r| {
            let paths = evaluate(lib, &r, &mtimes);
            (r, paths)
        })
        .collect())
}

/// Write every smart playlist from its rule.
pub fn refresh_smart(
    library_root: &Path,
    rules_dir: &Path,
    lib: &Library,
    index: &crate::Index,
) -> Result<usize, String> {
    let all = evaluate_all(rules_dir, lib, index)?;
    for (r, paths) in &all {
        write(library_root, lib, &r.name, paths, true)?;
    }
    Ok(all.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_keys_ignore_film_and_feature_notes() {
        assert_eq!(
            title_key("Tum Hi Ho (From \"Aashiqui 2\")"),
            title_key("Tum Hi Ho")
        );
        assert_eq!(title_key("Pasoori (feat. Ali Sethi)"), title_key("Pasoori"));
        assert_eq!(
            title_key("Wish You Were Here - Remastered 2011"),
            title_key("Wish You Were Here - Remastered 2011")
        ); // only known markers are cut
        assert_ne!(title_key("Roja"), title_key("Roja (Instrumental)"));
    }

    fn track(id: i64, path: &str, title: &str, md5: u8) -> Track {
        Track {
            id,
            path: path.into(),
            dir: path.rsplit_once('/').map(|p| p.0).unwrap_or("").into(),
            kind: "flac".into(),
            size: 1,
            duration_ms: 200_000,
            sample_rate: 44_100,
            bit_depth: 16,
            audio_md5: Some(vec![md5; 16]),
            vendor: None,
            tags: HashMap::from([("TITLE".into(), vec![title.into()])]),
            raw_keys: vec![],
            pictures: vec![],
            quality: None,
        }
    }

    fn library(tracks: Vec<Track>) -> Library {
        Library {
            tracks,
            lyrics_files: Default::default(),
            images: Default::default(),
            meta: Default::default(),
            fetched_lyrics: Default::default(),
            pairs: Default::default(),
            identities: Default::default(),
            in_library: false,
            lookup: Default::default(),
        }
    }

    #[test]
    fn a_library_keeps_playlists_beside_its_music() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("Lib");
        crate::paths::make_home(&home).unwrap();
        let music = crate::paths::music_dir(&home);
        let lib = library(vec![track(1, "Film/Taal (1999)/01 - One.flac", "One", 1)]);
        write(
            &music,
            &lib,
            "Road",
            &["Film/Taal (1999)/01 - One.flac".into()],
            false,
        )
        .unwrap();
        let file = home.join("Playlists").join("Road.m3u8");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains(
                "
../Music/Film/Taal (1999)/01 - One.flac
"
            ),
            "{text}"
        );
        let (entries, repaired) = read(&music, &lib, "Road").unwrap();
        assert!(!repaired);
        assert_eq!(entries[0].path, "Film/Taal (1999)/01 - One.flac");
    }

    fn song(id: i64, path: &str, title: &str, artist: &str, album: &str, ms: u64) -> Track {
        let mut t = track(id, path, title, id as u8);
        t.duration_ms = ms;
        t.tags.insert("ARTIST".into(), vec![artist.into()]);
        t.tags.insert("ALBUM".into(), vec![album.into()]);
        t
    }

    fn foreign(title: &str, artist: &str, album: &str, ms: Option<u64>) -> Foreign {
        Foreign {
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration_ms: ms,
            ..Default::default()
        }
    }

    #[test]
    fn matches_are_graded_exact_close_or_none() {
        let mut isrc = song(
            4,
            "B/04.flac",
            "Kesariya",
            "Arijit Singh",
            "Brahmastra",
            268_000,
        );
        isrc.tags.insert("ISRC".into(), vec!["INZ032201234".into()]);
        let lib = library(vec![
            song(
                1,
                "A/01.flac",
                "Tum Hi Ho",
                "Arijit Singh",
                "Aashiqui 2",
                262_000,
            ),
            song(
                2,
                "A/02.flac",
                "Tum Hi Ho (Remix)",
                "Arijit Singh",
                "Aashiqui 2",
                240_000,
            ),
            song(
                3,
                "C/03.flac",
                "Kabira",
                "Tochi Raina, Rekha Bhardwaj",
                "YJHD",
                223_000,
            ),
            isrc,
        ]);
        let m = Matcher::new(&lib);
        let g = |f: Foreign| m.grade(&f).map(|h| (h.track.path.clone(), h.grade));
        // Title and artist, the "(From …)" note ignored.
        assert_eq!(
            g(foreign(
                "Tum Hi Ho (From \"Aashiqui 2\")",
                "Arijit Singh",
                "",
                Some(262_500)
            )),
            Some(("A/01.flac".into(), Grade::Exact))
        );
        // The recording code alone is enough.
        assert_eq!(
            g(Foreign {
                isrc: Some("inz032201234".into()),
                ..foreign("Something else", "", "", None)
            }),
            Some(("B/04.flac".into(), Grade::Exact))
        );
        // One artist of several is enough to say it's theirs.
        assert_eq!(
            g(foreign("Kabira", "Rekha Bhardwaj", "", None)),
            Some(("C/03.flac".into(), Grade::Exact))
        );
        // A version you don't have: the nearest you do, for you to confirm.
        assert_eq!(
            g(foreign(
                "Tum Hi Ho - Unplugged",
                "Arijit Singh",
                "",
                Some(250_000)
            ))
            .map(|x| x.1),
            Some(Grade::Close)
        );
        // The same title by someone else, a different length: not offered.
        assert_eq!(
            g(foreign("Kabira", "Someone Else", "", Some(100_000))),
            None
        );
        // Nothing like it.
        assert_eq!(g(foreign("Kun Faya Kun", "A.R. Rahman", "", None)), None);
    }

    #[test]
    fn an_album_with_two_versions_of_a_title_is_settled_by_length() {
        // Special 26 has "Mujh Mein Tu" twice: 3:19 and 4:23.
        let lib = library(vec![
            song(
                1,
                "S/04.flac",
                "Mujh Mein Tu",
                "M.M. Kareem",
                "Special 26",
                263_000,
            ),
            song(
                2,
                "S/08.flac",
                "Mujh Mein Tu",
                "M.M. Kareem",
                "Special 26",
                199_000,
            ),
        ]);
        let m = Matcher::new(&lib);
        let g = |f: Foreign| m.grade(&f).map(|h| (h.track.path.clone(), h.grade));
        assert_eq!(
            g(foreign(
                "Mujh Mein Tu",
                "M.M. Kareem",
                "Special 26",
                Some(199_400)
            )),
            Some(("S/08.flac".into(), Grade::Exact))
        );
        assert_eq!(
            g(foreign(
                "Mujh Mein Tu",
                "M.M. Kareem",
                "Special 26",
                Some(262_000)
            )),
            Some(("S/04.flac".into(), Grade::Exact))
        );
        // No length to tell them apart: one of them, for you to check.
        assert_eq!(
            g(foreign("Mujh Mein Tu", "M.M. Kareem", "Special 26", None)).map(|x| x.1),
            Some(Grade::Close)
        );
        // The album agrees but neither length does: for you to check.
        assert_eq!(
            g(foreign(
                "Mujh Mein Tu",
                "M.M. Kareem",
                "Special 26",
                Some(230_000)
            ))
            .map(|x| x.1),
            Some(Grade::Close)
        );
    }

    #[test]
    fn titles_and_names_spelled_differently_still_match() {
        let lib = library(vec![
            song(
                1,
                "B/04.flac",
                "Kesariya",
                "Arijit Singh",
                "Brahmastra",
                268_000,
            ),
            song(2, "K/01.flac", "Khwaab", "Someone", "Dreams", 200_000),
        ]);
        let m = Matcher::new(&lib);
        let g = |f: Foreign| m.grade(&f).map(|h| (h.track.path.clone(), h.grade));
        // Spelled differently, same artist and length: found.
        assert_eq!(
            g(foreign("Kesariyaa", "Arijit Singh", "", Some(268_200))),
            Some(("B/04.flac".into(), Grade::Exact))
        );
        // The artist spelled differently too.
        assert_eq!(
            g(foreign(
                "Kesariyaa",
                "Arijeet Singh",
                "Brahmastra",
                Some(268_200)
            )),
            Some(("B/04.flac".into(), Grade::Exact))
        );
        // A pasted list (no length): offered, for you to check.
        assert_eq!(
            g(foreign("Kesariyaa", "Arijit Singh", "", None)),
            Some(("B/04.flac".into(), Grade::Close))
        );
        // A different artist and length: not offered.
        assert_eq!(g(foreign("Kwab", "Another", "", Some(100_000))), None);
    }

    #[test]
    fn wanted_songs_keep_their_place_and_source() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01.flac", "One", "X", "Al", 200_000)]);
        let want = WantEntry {
            title: "Missing".into(),
            artist: "Y".into(),
            album: "Gone".into(),
            duration_ms: 180_000,
            isrc: None,
            md5: None,
            not: Vec::new(),
        };
        let items = vec![Item::wanted(want.clone()), Item::track("A/01.flac")];
        write_items(
            root.path(),
            &lib,
            "Mixed",
            &items,
            false,
            Some("Spotify 2026-10-13"),
        )
        .unwrap();
        let (entries, _) = read(root.path(), &lib, "Mixed").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].missing);
        assert_eq!(entries[0].want.as_ref(), Some(&want));
        assert_eq!(entries[1].path, "A/01.flac");
        assert_eq!(
            source_of(root.path(), "Mixed").as_deref(),
            Some("Spotify 2026-10-13")
        );
        // A plain save (reorder in the app) keeps the source line.
        write(root.path(), &lib, "Mixed", &["A/01.flac".into()], false).unwrap();
        assert_eq!(
            source_of(root.path(), "Mixed").as_deref(),
            Some("Spotify 2026-10-13")
        );
        // Names are never overwritten by a copy.
        assert_eq!(
            duplicate(root.path(), &lib, "Mixed", "Mixed").unwrap(),
            "Mixed 2"
        );
    }

    #[test]
    fn to_get_follows_the_playlists() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01.flac", "One", "X", "Al", 200_000)]);
        let want = |t: &str, album: &str| {
            Item::wanted(WantEntry {
                title: t.into(),
                artist: "Y".into(),
                album: album.into(),
                duration_ms: 0,
                isrc: None,
                md5: None,
                not: Vec::new(),
            })
        };
        write_items(
            root.path(),
            &lib,
            "Road",
            &[
                want("A", "Gone"),
                want("B", "Gone"),
                Item::track("A/01.flac"),
            ],
            false,
            None,
        )
        .unwrap();
        write_items(
            root.path(),
            &lib,
            "Night",
            &[want("A", "Gone"), want("C", "Other")],
            false,
            None,
        )
        .unwrap();
        let now = wanted_now(root.path(), &lib);
        let gone = now.iter().find(|w| w.album == "Gone").unwrap();
        assert_eq!(gone.saved_tracks, 2);
        assert_eq!(gone.playlists.len(), 2);
        // Replace the wanted songs in Night with a library song: "Other" is no longer wanted.
        write_items(
            root.path(),
            &lib,
            "Night",
            &[Item::track("A/01.flac")],
            false,
            None,
        )
        .unwrap();
        let now = wanted_now(root.path(), &lib);
        assert!(now.iter().all(|w| w.album != "Other"));
        assert_eq!(
            now.iter().find(|w| w.album == "Gone").unwrap().playlists,
            vec!["Road".to_string()]
        );
    }

    #[test]
    fn moved_tracks_are_found_again_and_rename_updates_the_header() {
        let root = tempfile::tempdir().unwrap();
        let before = library(vec![
            track(1, "A/01 One.flac", "One", 1),
            track(2, "A/02 Two.flac", "Two", 2),
        ]);
        write(
            root.path(),
            &before,
            "Road",
            &["A/02 Two.flac".into(), "A/01 One.flac".into()],
            false,
        )
        .unwrap();

        // "Two" moves to another folder; its checksum finds it.
        let after = library(vec![
            track(1, "A/01 One.flac", "One", 1),
            track(2, "B/02 Two.flac", "Two", 2),
        ]);
        let (entries, repaired) = read(root.path(), &after, "Road").unwrap();
        assert!(repaired);
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["B/02 Two.flac", "A/01 One.flac"]
        );
        assert_eq!(repair_all(root.path(), &after), 1);
        assert!(
            !read(root.path(), &after, "Road").unwrap().1,
            "rewritten with the new path"
        );

        rename(root.path(), "Road", "Night drive").unwrap();
        let text = std::fs::read_to_string(folder(root.path()).join("Night drive.m3u8")).unwrap();
        assert!(text.contains("#PLAYLIST:Night drive\n"));
        assert!(!folder(root.path()).join("Road.m3u8").exists());
        assert!(rename(root.path(), "Night drive", "Night drive").is_err());
    }
    #[test]
    fn a_song_already_there_is_added_again_only_when_asked() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01 - One.flac", "One", "X", "Al", 200_000)]);
        let one = Item {
            path: Some("A/01 - One.flac".into()),
            ..Default::default()
        };
        assert_eq!(
            add(root.path(), &lib, "Mix", std::slice::from_ref(&one)).unwrap(),
            1
        );
        assert_eq!(
            add(root.path(), &lib, "Mix", std::slice::from_ref(&one)).unwrap(),
            0,
            "already in it"
        );
        assert_eq!(
            add_with(root.path(), &lib, "Mix", std::slice::from_ref(&one), true).unwrap(),
            1,
            "again, asked"
        );
        assert_eq!(read(root.path(), &lib, "Mix").unwrap().0.len(), 2);
    }

    #[test]
    fn a_new_playlist_never_takes_the_place_of_one_there() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![]);
        create(root.path(), &lib, "Fresh").unwrap();
        assert!(exists(root.path(), "Fresh"));
        std::fs::write(folder(root.path()).join("Kept.m3u8"), "#EXTM3U\n# mine\n").unwrap();
        assert!(create(root.path(), &lib, "Kept").is_err());
        assert!(
            create(root.path(), &lib, "Kept?").is_err(),
            "saved as the same file"
        );
        assert_eq!(
            std::fs::read_to_string(folder(root.path()).join("Kept.m3u8")).unwrap(),
            "#EXTM3U\n# mine\n"
        );
        assert!(create(root.path(), &lib, "***").is_err());
    }

    #[test]
    fn a_deleted_playlist_comes_back_byte_for_byte() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(folder(root.path())).unwrap();
        // An .m3u another player wrote, with bytes Venuta wouldn't write itself (CRLF).
        let was = folder(root.path()).join("Road trip.m3u");
        let bytes = b"#EXTM3U\r\n#EXTINF:200,X - One\r\n../A/01 - One.flac\r\n".to_vec();
        std::fs::write(&was, &bytes).unwrap();
        let gone = delete(root.path(), "Road trip").unwrap();
        assert!(!was.exists(), "moved aside");
        assert!(gone.aside.is_file(), "into Set aside");
        assert_eq!(gone.was, was);
        undelete(root.path(), "Road trip", &gone.aside, &gone.was).unwrap();
        assert_eq!(
            std::fs::read(&was).unwrap(),
            bytes,
            "the same file, the same bytes"
        );
        assert!(!gone.aside.exists(), "and no longer in Set aside");
    }

    #[test]
    fn undo_never_overwrites_a_playlist_made_since() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(folder(root.path())).unwrap();
        let was = folder(root.path()).join("Mix.m3u8");
        std::fs::write(&was, "#EXTM3U\n").unwrap();
        let gone = delete(root.path(), "Mix").unwrap();
        std::fs::write(&was, "#EXTM3U\n# a new one\n").unwrap();
        assert!(undelete(root.path(), "Mix", &gone.aside, &gone.was).is_err());
        assert_eq!(
            std::fs::read_to_string(&was).unwrap(),
            "#EXTM3U\n# a new one\n"
        );
        assert!(gone.aside.is_file(), "the deleted one stays in Set aside");
    }

    #[test]
    fn undo_only_moves_files_between_set_aside_and_the_playlists() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(folder(root.path())).unwrap();
        let elsewhere = root.path().join("elsewhere.m3u8");
        std::fs::write(&elsewhere, "#EXTM3U\n").unwrap();
        let was = folder(root.path()).join("X.m3u8");
        assert!(
            undelete(root.path(), "X", &elsewhere, &was).is_err(),
            "not from outside Set aside"
        );
        std::fs::write(&was, "#EXTM3U\n").unwrap();
        let gone = delete(root.path(), "X").unwrap();
        assert!(
            undelete(root.path(), "X", &gone.aside, &root.path().join("out.m3u8")).is_err(),
            "not to outside the playlists"
        );
    }

    #[test]
    fn no_song_of_any_playlist_is_ever_dropped() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01 - One.flac", "One", "X", "Al", 200_000)]);
        // A playlist another player wrote, dropped into the folder: a song here, one whose file
        // is gone (with its EXTINF), one from another computer, and one Venuta was waiting for.
        std::fs::create_dir_all(folder(root.path())).unwrap();
        std::fs::write(
            folder(root.path()).join("Mixed.m3u8"),
            "#EXTM3U\n#EXTINF:200,X - One\n../A/01 - One.flac\n#EXTINF:190,Artist B - Song B\n../Gone/02 - Song B.flac\nD:\\Elsewhere\\Album C (2001)\\03 - Song C.flac\n#VENUTA-WANT:{\"title\":\"Song D\",\"artist\":\"Z\"}\n",
        )
        .unwrap();
        let (entries, _) = read(root.path(), &lib, "Mixed").unwrap();
        assert_eq!(entries.len(), 4);
        assert!(!entries[0].missing);
        let b = entries[1].want.as_ref().expect("the gone song is kept");
        assert_eq!(
            (b.title.as_str(), b.artist.as_str(), b.duration_ms),
            ("Song B", "Artist B", 190_000)
        );
        let c = entries[2]
            .want
            .as_ref()
            .expect("the other computer's song is kept");
        assert_eq!((c.title.as_str(), c.album.as_str()), ("Song C", "Album C"));
        assert_eq!(entries[3].title, "Song D");

        // Saving it back (any edit in the app) keeps all four.
        let items: Vec<Item> = entries.iter().map(PlaylistEntry::item).collect();
        write_items(root.path(), &lib, "Mixed", &items, false, None).unwrap();
        let (again, _) = read(root.path(), &lib, "Mixed").unwrap();
        assert_eq!(
            again.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
            ["One", "Song B", "Song C", "Song D"]
        );

        // A path the library doesn't have is never silently left out of a save.
        write_items(
            root.path(),
            &lib,
            "Paths",
            &[
                Item::track("A/01 - One.flac"),
                Item::track("Lost/07 - Seven.flac"),
            ],
            false,
            None,
        )
        .unwrap();
        let (p, _) = read(root.path(), &lib, "Paths").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].title, "Seven");
        assert!(p[1].missing);
    }

    #[test]
    fn a_song_whose_file_went_away_comes_back_by_its_checksum() {
        let root = tempfile::tempdir().unwrap();
        let had = library(vec![track(1, "A/01 One.flac", "One", 7)]);
        write(root.path(), &had, "Road", &["A/01 One.flac".into()], false).unwrap();
        // The file is gone for a while, and the playlist is saved meanwhile.
        let gone = library(vec![]);
        let (e, _) = read(root.path(), &gone, "Road").unwrap();
        assert!(e[0].missing);
        let items: Vec<Item> = e.iter().map(PlaylistEntry::item).collect();
        write_items(root.path(), &gone, "Road", &items, false, None).unwrap();
        // It comes back somewhere else: the same audio is the same song.
        let back = library(vec![track(1, "B/01 One.flac", "One", 7)]);
        let (e, repaired) = read(root.path(), &back, "Road").unwrap();
        assert!(repaired);
        assert_eq!(e[0].path, "B/01 One.flac");
        assert!(!e[0].missing);
    }

    #[test]
    fn saving_the_same_playlist_again_leaves_the_file_alone() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![track(1, "A/01 One.flac", "One", 7)]);
        write(root.path(), &lib, "Road", &["A/01 One.flac".into()], true).unwrap();
        let file = file_for(root.path(), "Road");
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
        write(root.path(), &lib, "Road", &["A/01 One.flac".into()], true).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), old);
        // A real change is still written.
        let more = library(vec![
            track(1, "A/01 One.flac", "One", 7),
            track(2, "A/02 Two.flac", "Two", 8),
        ]);
        let both = ["A/01 One.flac".into(), "A/02 Two.flac".into()];
        write(root.path(), &more, "Road", &both, true).unwrap();
        assert_ne!(std::fs::metadata(&file).unwrap().modified().unwrap(), old);
    }

    #[test]
    fn m3u_files_in_any_encoding_are_playlists_too() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(
            1,
            "A/01 - Café.flac",
            "Café",
            "X",
            "Al",
            200_000,
        )]);
        std::fs::create_dir_all(folder(root.path())).unwrap();
        // Windows-1252, as older players write .m3u.
        let mut bytes = b"#EXTM3U\n../A/01 - Caf".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b".flac\n");
        std::fs::write(folder(root.path()).join("Old.m3u"), bytes).unwrap();
        let listed = list(root.path(), &lib);
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].name.as_str(), listed[0].tracks, listed[0].missing),
            ("Old", 1, 0)
        );
        let (e, _) = read(root.path(), &lib, "Old").unwrap();
        assert_eq!(e[0].path, "A/01 - Café.flac");
        // Saved once, it becomes an .m3u8; the old file is set aside, not left as a twin.
        let items: Vec<Item> = e.iter().map(PlaylistEntry::item).collect();
        write_items(root.path(), &lib, "Old", &items, false, None).unwrap();
        assert!(folder(root.path()).join("Old.m3u8").is_file());
        assert!(!folder(root.path()).join("Old.m3u").exists());
        assert_eq!(list(root.path(), &lib).len(), 1);
    }
    #[test]
    fn a_close_match_is_offered_once_and_a_no_is_remembered() {
        let root = tempfile::tempdir().unwrap();
        // You have another version (a different length) of a song the playlist waits for.
        let lib = library(vec![song(
            1,
            "A/01 - Roja.flac",
            "Roja",
            "A. R. Rahman",
            "Roja",
            300_000,
        )]);
        let want = WantEntry {
            title: "Roja".into(),
            artist: "A. R. Rahman".into(),
            duration_ms: 240_000,
            ..Default::default()
        };
        write_items(
            root.path(),
            &lib,
            "Old",
            &[Item::wanted(want.clone())],
            false,
            None,
        )
        .unwrap();
        let (mut e, _) = read(root.path(), &lib, "Old").unwrap();
        suggest(&lib, &mut e);
        let maybe = e[0].maybe.as_ref().expect("offered");
        assert_eq!(maybe.path, "A/01 - Roja.flac");
        // Opening Playlists fills in only sure matches: this one waits for you.
        assert_eq!(crate::imports::refind_all(root.path(), &lib), 0);

        // "Not this one" is kept in the playlist, so it isn't offered again.
        let no = WantEntry {
            not: vec!["A/01 - Roja.flac".into()],
            ..want
        };
        write_items(root.path(), &lib, "Old", &[Item::wanted(no)], false, None).unwrap();
        let (mut e, _) = read(root.path(), &lib, "Old").unwrap();
        suggest(&lib, &mut e);
        assert!(e[0].maybe.is_none());
        assert!(e[0].missing);
    }

    fn online(id: &str, title: &str, artist: &str) -> OnlineEntry {
        OnlineEntry {
            provider: "remote_catalog".into(),
            track_id: id.into(),
            title: title.into(),
            artists: vec![artist.into()],
            album: Some("Elsewhere".into()),
            duration_ms: Some(201_000),
            isrc: None,
            artwork_url: Some("https://i.ytimg.com/vi/example/hqdefault.jpg".into()),
        }
    }

    #[test]
    fn online_lines_encode_and_decode() {
        let o = online("t-1", "Kesariya", "Arijit Singh");
        let line = encode_online(&o);
        assert!(line.starts_with("#VENUTA-ONLINE:{"), "{line}");
        assert_eq!(decode_online(&line), Some(o.clone()));
        assert_eq!(decode_online(&format!("  {line}  ")), Some(o));
        // The contract's own example.
        let doc = r#"#VENUTA-ONLINE:{"provider":"remote_catalog","track_id":"x9","title":"T","artists":["A"],"album":"Al","duration_ms":123}"#;
        assert_eq!(decode_online(doc).unwrap().duration_ms, Some(123));
        // Not an online line, broken, or without a real id.
        assert_eq!(decode_online("#VENUTA-WANT:{\"title\":\"x\"}"), None);
        assert_eq!(decode_online("#VENUTA-ONLINE:{not json"), None);
        assert_eq!(
            decode_online(
                r#"#VENUTA-ONLINE:{"provider":"remote_catalog","track_id":" ","title":"T"}"#
            ),
            None
        );
        assert_eq!(
            decode_online(
                r#"#VENUTA-ONLINE:{"provider":"remote_catalog","track_id":"https://cdn.example/a.mp3","title":"T"}"#
            ),
            None
        );
        // A stream-shaped field someone added is dropped; a cover picture may stay.
        let o = decode_online(r#"#VENUTA-ONLINE:{"provider":"p","track_id":"t","title":"T","url":"https://cdn.example/a.mp3"}"#).unwrap();
        assert!(!encode_online(&o).contains("cdn.example"));
        let art = decode_online(r#"#VENUTA-ONLINE:{"provider":"p","track_id":"t","title":"T","artwork_url":"//i.ytimg.com/vi/x/hqdefault.jpg"}"#).unwrap();
        assert_eq!(
            art.artwork_url.as_deref(),
            Some("https://i.ytimg.com/vi/x/hqdefault.jpg")
        );
        let bad_art = decode_online(r#"#VENUTA-ONLINE:{"provider":"p","track_id":"t","title":"T","artwork_url":"https://googlevideo.com/a.mp3"}"#).unwrap();
        assert_eq!(bad_art.artwork_url, None);
    }

    #[test]
    fn online_songs_keep_their_place_and_are_never_addresses() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01.flac", "One", "X", "Al", 200_000)]);
        let want = WantEntry {
            title: "Missing".into(),
            artist: "Y".into(),
            album: "Gone".into(),
            ..Default::default()
        };
        let o = online("t-1", "Kesariya", "Arijit Singh");
        let items = vec![
            Item::track("A/01.flac"),
            Item::online(o.clone()),
            Item::wanted(want.clone()),
        ];
        write_items(root.path(), &lib, "Mixed", &items, false, None).unwrap();
        let file = file_for(root.path(), "Mixed");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(&format!("{}\n", encode_online(&o))), "{text}");
        assert!(
            !text.contains("googlevideo.com") && !text.contains("mime=audio"),
            "no stream addresses in a playlist: {text}"
        );
        assert!(
            text.contains("artwork_url") && text.contains("i.ytimg.com"),
            "cover picture may stay: {text}"
        );

        let (e, _) = read(root.path(), &lib, "Mixed").unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!((e[0].path.as_str(), e[0].missing), ("A/01.flac", false));
        assert!(e[1].missing && e[1].want.is_none());
        assert_eq!(e[1].online.as_ref(), Some(&o));
        assert_eq!(
            (e[1].title.as_str(), e[1].artist.as_str()),
            ("Kesariya", "Arijit Singh")
        );
        assert_eq!(e[2].want.as_ref(), Some(&want));

        // Saved back as it was read (any edit in the app): the very same file.
        let again: Vec<Item> = e.iter().map(PlaylistEntry::item).collect();
        write_items(root.path(), &lib, "Mixed", &again, false, None).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);

        // It plays, so it isn't a song to get: the want-list has only the unbound one.
        let listed = list(root.path(), &lib);
        assert_eq!((listed[0].missing, listed[0].online), (1, 1));
        assert_eq!(listed[0].duration_ms, 200_000 + 201_000);
        let now = wanted_now(root.path(), &lib);
        assert_eq!(
            now.iter().map(|w| w.album.as_str()).collect::<Vec<_>>(),
            ["Gone"]
        );

        // An id that is really an address is never written: the song stays, as one to get.
        let bad = OnlineEntry {
            track_id: "https://cdn.example/a.mp3".into(),
            ..o
        };
        write_items(root.path(), &lib, "Bad", &[Item::online(bad)], false, None).unwrap();
        let text = std::fs::read_to_string(file_for(root.path(), "Bad")).unwrap();
        assert!(
            !text.contains("://") && !text.contains(ONLINE_PLAYLIST_TAG),
            "{text}"
        );
        let (e, _) = read(root.path(), &lib, "Bad").unwrap();
        assert_eq!(e[0].want.as_ref().unwrap().title, "Kesariya");
    }

    #[test]
    fn your_own_file_plays_first_and_online_fills_in() {
        let root = tempfile::tempdir().unwrap();
        let path = "A/01 - Kesariya.flac";
        std::fs::create_dir_all(root.path().join("A")).unwrap();
        std::fs::write(root.path().join(path), b"flac").unwrap();
        let mut have = song(1, path, "Kesariya", "Arijit Singh", "Brahmastra", 268_000);
        have.tags.insert("ISRC".into(), vec!["INZ032201234".into()]);
        let lib = library(vec![have]);
        let mine = OnlineEntry {
            isrc: Some("INZ032201234".into()),
            ..online("t-1", "Kesariya (From \"Brahmastra\")", "Arijit Singh")
        };
        let other = online("t-2", "Not Here", "Someone");
        let items = vec![
            Item::online(mine.clone()),
            Item::wanted(WantEntry {
                title: "To get".into(),
                ..Default::default()
            }),
            Item::online(other.clone()),
        ];
        write_items(root.path(), &lib, "Hybrid", &items, false, None).unwrap();
        let (e, _) = read(root.path(), &lib, "Hybrid").unwrap();
        // The library has the first one: it is that file, and keeps its online id.
        assert_eq!((e[0].path.as_str(), e[0].missing), (path, false));
        assert_eq!(e[0].online.as_ref(), Some(&mine));

        let p = playables(root.path(), &e);
        assert_eq!(p.len(), 2, "the song to get is left out; the rest play on");
        assert_eq!(p[0].id, PlayableId::Local { path: path.into() });
        assert_eq!(p[0].availability, Some(Availability::LocalPreferred));
        assert_eq!(
            p[0].local_file.as_deref(),
            Some(root.path().join(path).as_path())
        );
        assert_eq!(p[1].id, other.id());
        assert_eq!(p[1].availability, Some(Availability::Online));
        assert_eq!(p[1].local_file, None);

        // The file goes away (still in the index): it plays online again, nothing breaks.
        std::fs::remove_file(root.path().join(path)).unwrap();
        let p = playables(root.path(), &e);
        assert_eq!(p[0].id, mine.id());
        assert_eq!(p[0].availability, Some(Availability::Online));

        // Saved, it stays an online id (the file is found again on each read).
        let again: Vec<Item> = e.iter().map(PlaylistEntry::item).collect();
        assert_eq!(again[0].online.as_ref(), Some(&mine));
        assert_eq!(again[0].path, None);
    }

    #[test]
    fn a_catalog_track_round_trips_and_resolves_only_when_played() {
        use media_access::{CancellationToken, FakeMediaAccess, MediaAccess, PageRequest};
        let access = FakeMediaAccess::with_in_memory_fixture();
        let cancel = CancellationToken::default();
        let found = access
            .search_tracks("Night", &PageRequest::default(), &cancel)
            .unwrap()
            .items
            .remove(0);
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![]);
        let items = [Item::online(OnlineEntry::from_remote(&found))];
        write_items(root.path(), &lib, "Online", &items, false, None).unwrap();
        let text = std::fs::read_to_string(file_for(root.path(), "Online")).unwrap();
        assert!(!text.contains(access.fixture_url()), "{text}");
        assert!(!text.contains("://"), "{text}");

        let (e, _) = read(root.path(), &lib, "Online").unwrap();
        let p = playables(root.path(), &e);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].id, found.playable_id());
        assert_eq!(p[0].title, found.title);
        // The stream is resolved from the stable id at play time, and only then.
        let PlayableId::Online { track_id, .. } = &p[0].id else {
            panic!("online")
        };
        let audio = access.resolve_audio(track_id, &cancel).unwrap();
        assert_eq!(audio.url, access.fixture_url());
    }

    #[test]
    fn adding_skips_songs_already_there_by_file_or_online_id() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01.flac", "One", "X", "Al", 200_000)]);
        let o = online("t-1", "Kesariya", "Arijit Singh");
        // A new playlist, mixed, in the order given.
        let n = add(
            root.path(),
            &lib,
            "Mix",
            &[Item::online(o.clone()), Item::track("A/01.flac")],
        )
        .unwrap();
        assert_eq!(n, 2);
        // Again, with the same song online under fresh metadata, a duplicate in the same call,
        // an id that is an address, and one new online song.
        let renamed = OnlineEntry {
            title: "Kesariya (Live)".into(),
            ..o.clone()
        };
        let bad = online("https://cdn.example/a.mp3", "Bad", "Y");
        let new = online("t-2", "Two", "Y");
        let n = add(
            root.path(),
            &lib,
            "Mix",
            &[
                Item::online(renamed),
                Item::track("A/01.flac"),
                Item::online(bad),
                Item::online(new.clone()),
                Item::online(new.clone()),
            ],
        )
        .unwrap();
        assert_eq!(n, 1);
        let (e, _) = read(root.path(), &lib, "Mix").unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].online.as_ref(), Some(&o));
        assert_eq!(e[1].path, "A/01.flac");
        assert_eq!(e[2].online.as_ref(), Some(&new));
        let text = std::fs::read_to_string(file_for(root.path(), "Mix")).unwrap();
        // Cover pictures may stay; stream hosts and bare media addresses must not.
        assert!(
            !text.contains("cdn.example")
                && !text.contains("googlevideo.com")
                && !text.contains("mime=audio"),
            "{text}"
        );
        assert!(
            !text.contains("\"track_id\":\"https://"),
            "track ids must never be addresses: {text}"
        );
    }
}
