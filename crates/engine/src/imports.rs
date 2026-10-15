//! Playlists from elsewhere: other players and streaming services, read from the files they
//! export (or Deezer's public API), then matched to the library with a grade you can trust.
//!
//! Sources, all without an account key, and nothing is ever downloaded:
//! - Spotify account data (`Playlist1.json`, `YourLibrary.json`) and Exportify CSV;
//! - Apple Music / iTunes exported playlists (`.xml`, `.txt`) and the Apple Music library;
//! - Deezer public playlist links;
//! - YouTube Music via Google Takeout (video IDs; titles from YouTube's public oEmbed);
//! - TuneMyMusic / Soundiiz CSV (TIDAL, Amazon and the rest);
//! - `.m3u`, `.m3u8` and `.pls` from any player.

use crate::library::Library;
use crate::playlists::{self, Foreign, Grade, Item, Matcher, OnlineEntry, WantGroup, WantedAlbum};
use media_access::{CancellationToken, MediaAccess, PageRequest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// A playlist found in a file or at a link, before it's matched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignPlaylist {
    pub name: String,
    /// Where it came from, in words ("Spotify").
    pub source: String,
    /// Which playlist at the source, to read it again for an update: "spotify:<id>",
    /// "deezer:<id>", "apple:<name>", or "file:<path>|<name>".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub tracks: Vec<Foreign>,
}

/// The id of a playlist read from a file: the file and the playlist's name in it.
pub fn file_id(path: &Path, name: &str) -> String {
    format!("file:{}|{name}", path.to_string_lossy())
}

/// Read every playlist in a file another app made.
pub fn read_file(path: &Path) -> Result<Vec<ForeignPlaylist>, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported".into());
    let text = crate::text::read_text(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let found = match ext.as_str() {
        "json" => spotify_json(&text)?,
        "csv" => csv_playlists(&text, &stem)?,
        "xml" => itunes_xml(&text)?,
        "txt" => vec![itunes_txt(&text, &stem)?],
        "m3u" | "m3u8" => vec![m3u(&text, &stem, path.parent().unwrap_or(Path::new(".")))],
        "pls" => vec![pls(&text, &stem, path.parent().unwrap_or(Path::new(".")))],
        _ => {
            return Err(format!(
                "{} isn't a playlist file Venuta can read.",
                path.display()
            ));
        }
    };
    let found: Vec<ForeignPlaylist> = found
        .into_iter()
        .filter(|p| !p.tracks.is_empty())
        .map(|mut p| {
            p.id = Some(file_id(path, &p.name));
            p
        })
        .collect();
    if found.is_empty() {
        return Err(format!("No songs found in {}.", path.display()));
    }
    Ok(found)
}

// --- Spotify -------------------------------------------------------------------------------

/// Spotify's account data: `Playlist1.json` (your playlists) or `YourLibrary.json` (liked songs).
fn spotify_json(text: &str) -> Result<Vec<ForeignPlaylist>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("Not readable JSON: {e}"))?;
    let s = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let mut out = Vec::new();
    if let Some(lists) = v.get("playlists").and_then(|p| p.as_array()) {
        for p in lists {
            let tracks = p
                .get("items")
                .and_then(|i| i.as_array())
                .into_iter()
                .flatten()
                .filter_map(|i| i.get("track").filter(|t| !t.is_null()))
                .map(|t| Foreign {
                    title: s(t, "trackName"),
                    artist: s(t, "artistName"),
                    album: s(t, "albumName"),
                    ..Default::default()
                })
                .filter(|f| !f.title.is_empty())
                .collect();
            out.push(ForeignPlaylist {
                name: s(p, "name"),
                source: "Spotify".into(),
                id: None,
                tracks,
            });
        }
    }
    if let Some(liked) = v.get("tracks").and_then(|t| t.as_array()) {
        out.push(ForeignPlaylist {
            name: "Liked Songs".into(),
            source: "Spotify".into(),
            id: None,
            tracks: liked
                .iter()
                .map(|t| Foreign {
                    title: s(t, "track"),
                    artist: s(t, "artist"),
                    album: s(t, "album"),
                    ..Default::default()
                })
                .filter(|f| !f.title.is_empty())
                .collect(),
        });
    }
    if out.is_empty() {
        return Err(
            "This JSON isn't a Spotify playlist export (Playlist1.json or YourLibrary.json)."
                .into(),
        );
    }
    Ok(out)
}

// --- CSV (Exportify, TuneMyMusic, Soundiiz, Google Takeout) ---------------------------------

/// Rows of a CSV file (quoted fields, doubled quotes, line breaks inside quotes).
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    rows
}

/// "3:45", "1:02:10", "225" (seconds) or "225000" (milliseconds, when it says so) → ms.
fn duration_ms(s: &str, millis: bool) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains(':') {
        let parts: Vec<u64> = s.split(':').filter_map(|p| p.trim().parse().ok()).collect();
        return Some(parts.iter().fold(0, |acc, p| acc * 60 + p) * 1000);
    }
    let n: f64 = s.parse().ok()?;
    Some(if millis {
        n as u64
    } else {
        (n * 1000.0) as u64
    })
}

fn csv_playlists(text: &str, stem: &str) -> Result<Vec<ForeignPlaylist>, String> {
    let mut rows = csv_rows(text);
    // Google Takeout's older playlist files open with a block about the playlist itself.
    if let Some(at) = rows
        .iter()
        .position(|r| r.iter().any(|f| f.trim().eq_ignore_ascii_case("video id")))
    {
        rows.drain(..at);
    }
    let Some(head) = rows.first().cloned() else {
        return Ok(vec![]);
    };
    let col = |names: &[&str]| {
        head.iter()
            .position(|h| names.iter().any(|n| h.trim().eq_ignore_ascii_case(n)))
    };
    let title = col(&[
        "track name",
        "track",
        "title",
        "name",
        "song",
        "song name",
        "track title",
    ]);
    let artist = col(&[
        "artist name(s)",
        "artist name",
        "artist",
        "artists",
        "artist(s)",
        "artist names",
    ]);
    let album = col(&["album name", "album", "album title"]);
    let isrc = col(&["isrc"]);
    let ms = col(&["track duration (ms)", "duration (ms)", "duration_ms"]);
    let secs = col(&["duration", "time", "length"]);
    let list = col(&["playlist name", "playlist"]);
    let video = col(&["video id", "video_id", "videoid"]);
    let source = if video.is_some() {
        "YouTube Music"
    } else if head
        .iter()
        .any(|h| h.eq_ignore_ascii_case("track uri") || h.eq_ignore_ascii_case("spotify id"))
    {
        "Spotify"
    } else if head.iter().any(|h| h.eq_ignore_ascii_case("type")) && list.is_some() {
        "TuneMyMusic"
    } else {
        "CSV"
    };
    if title.is_none() && video.is_none() {
        return Err(
            "This CSV has no title column (looked for \"Track name\", \"Title\", \"Video Id\"…)."
                .into(),
        );
    }
    let get = |r: &Vec<String>, c: Option<usize>| {
        c.and_then(|i| r.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let base = stem
        .trim_end_matches("-videos")
        .trim_end_matches(" videos")
        .to_owned();
    let mut groups: BTreeMap<String, Vec<Foreign>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for r in rows.iter().skip(1) {
        let f = Foreign {
            title: get(r, title),
            artist: get(r, artist),
            album: get(r, album),
            isrc: Some(get(r, isrc)).filter(|s| s.len() >= 12),
            duration_ms: ms
                .and_then(|i| r.get(i))
                .and_then(|s| duration_ms(s, true))
                .or_else(|| {
                    secs.and_then(|i| r.get(i))
                        .and_then(|s| duration_ms(s, false))
                }),
            youtube: Some(get(r, video)).filter(|s| !s.is_empty()),
            file: None,
        };
        if f.title.is_empty() && f.youtube.is_none() {
            continue;
        }
        let name = Some(get(r, list))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| base.clone());
        if !groups.contains_key(&name) {
            order.push(name.clone());
        }
        groups.entry(name).or_default().push(f);
    }
    Ok(order
        .into_iter()
        .map(|name| ForeignPlaylist {
            tracks: groups.remove(&name).unwrap_or_default(),
            name,
            source: source.into(),
            id: None,
        })
        .collect())
}

// --- Apple Music / iTunes ------------------------------------------------------------------

/// A tiny property-list reader: enough for iTunes / Apple Music library and playlist XML.
#[derive(Debug, Clone)]
enum Plist {
    Dict(Vec<(String, Plist)>),
    Array(Vec<Plist>),
    Str(String),
    Int(i64),
    Other,
}

impl Plist {
    fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Plist::Str(s) => Some(s),
            _ => None,
        }
    }
    fn int(&self) -> Option<i64> {
        match self {
            Plist::Int(i) => Some(*i),
            Plist::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else { break };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16)
                .ok()
                .and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

struct PlistReader<'a> {
    s: &'a str,
    at: usize,
}

impl<'a> PlistReader<'a> {
    /// The next tag: its name, whether it closes or is self-closing.
    fn tag(&mut self) -> Option<(String, bool, bool)> {
        loop {
            let i = self.s[self.at..].find('<')? + self.at;
            let j = self.s[i..].find('>')? + i;
            self.at = j + 1;
            let inner = &self.s[i + 1..j];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue;
            }
            let closing = inner.starts_with('/');
            let selfclose = inner.ends_with('/');
            let name = inner
                .trim_start_matches('/')
                .trim_end_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_owned();
            return Some((name, closing, selfclose));
        }
    }
    fn text_until(&mut self, close: &str) -> String {
        let end = self.s[self.at..]
            .find(close)
            .map(|i| i + self.at)
            .unwrap_or(self.s.len());
        let t = unescape(&self.s[self.at..end]);
        self.at = (end + close.len()).min(self.s.len());
        t
    }
    fn value(&mut self) -> Option<Plist> {
        let (name, closing, selfclose) = self.tag()?;
        if closing {
            return None;
        }
        Some(match name.as_str() {
            "plist" => self.value()?,
            "dict" if !selfclose => {
                let mut d = Vec::new();
                loop {
                    let save = self.at;
                    match self.tag() {
                        Some((k, false, false)) if k == "key" => {
                            let key = self.text_until("</key>");
                            let v = self.value().unwrap_or(Plist::Other);
                            d.push((key, v));
                        }
                        Some((k, true, _)) if k == "dict" => break,
                        None => break,
                        _ => {
                            self.at = save.max(self.at);
                        }
                    }
                }
                Plist::Dict(d)
            }
            "array" if !selfclose => {
                let mut a = Vec::new();
                loop {
                    let save = self.at;
                    match self.tag() {
                        Some((k, true, _)) if k == "array" => break,
                        None => break,
                        Some(_) => {
                            self.at = save;
                            match self.value() {
                                Some(v) => a.push(v),
                                None => break,
                            }
                        }
                    }
                }
                Plist::Array(a)
            }
            "string" if !selfclose => Plist::Str(self.text_until("</string>")),
            "integer" => Plist::Int(self.text_until("</integer>").trim().parse().unwrap_or(0)),
            "dict" | "array" | "string" => Plist::Other,
            other => {
                if !selfclose {
                    let _ = self.text_until(&format!("</{other}>"));
                }
                Plist::Other
            }
        })
    }
}

/// An iTunes / Apple Music XML export: one playlist, or a whole library's playlists.
fn itunes_xml(text: &str) -> Result<Vec<ForeignPlaylist>, String> {
    let root = PlistReader { s: text, at: 0 }
        .value()
        .ok_or("This XML isn't an iTunes or Apple Music export.")?;
    let tracks = root
        .get("Tracks")
        .ok_or("This XML has no tracks (export a playlist from Apple Music or iTunes).")?;
    let song = |id: i64| -> Option<Foreign> {
        let t = tracks.get(&id.to_string())?;
        let s = |k: &str| t.get(k).and_then(Plist::str).unwrap_or_default().to_owned();
        Some(Foreign {
            title: s("Name"),
            artist: s("Artist"),
            album: s("Album"),
            duration_ms: t.get("Total Time").and_then(Plist::int).map(|v| v as u64),
            ..Default::default()
        })
    };
    let skip = [
        "Library",
        "Music",
        "Downloaded",
        "Music Videos",
        "Movies",
        "TV Shows",
        "Podcasts",
        "Audiobooks",
        "Genius",
    ];
    let mut out = Vec::new();
    if let Some(Plist::Array(lists)) = root.get("Playlists") {
        for p in lists {
            let name = p
                .get("Name")
                .and_then(Plist::str)
                .unwrap_or_default()
                .to_owned();
            if p.get("Master").is_some()
                || p.get("Distinguished Kind").is_some()
                || skip.contains(&name.as_str())
            {
                continue;
            }
            let items = match p.get("Playlist Items") {
                Some(Plist::Array(a)) => a.clone(),
                _ => vec![],
            };
            out.push(ForeignPlaylist {
                name,
                source: "Apple Music".into(),
                id: None,
                tracks: items
                    .iter()
                    .filter_map(|i| i.get("Track ID").and_then(Plist::int))
                    .filter_map(song)
                    .collect(),
            });
        }
    }
    Ok(out)
}

/// Apple Music / iTunes "Export Playlist" as plain text: tab-separated, headed by column names.
fn itunes_txt(text: &str, stem: &str) -> Result<ForeignPlaylist, String> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let head: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    let col = |n: &str| head.iter().position(|h| h.trim().eq_ignore_ascii_case(n));
    let (Some(name), artist, album, time) = (col("Name"), col("Artist"), col("Album"), col("Time"))
    else {
        return Err(
            "This text file isn't an Apple Music / iTunes playlist export (no \"Name\" column)."
                .into(),
        );
    };
    let tracks = lines
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter_map(|r| {
            let get = |c: Option<usize>| {
                c.and_then(|i| r.get(i))
                    .map(|s| s.trim().to_owned())
                    .unwrap_or_default()
            };
            let title = get(Some(name));
            (!title.is_empty()).then(|| Foreign {
                title,
                artist: get(artist),
                album: get(album),
                duration_ms: time
                    .and_then(|i| r.get(i))
                    .and_then(|s| duration_ms(s, false)),
                ..Default::default()
            })
        })
        .collect();
    Ok(ForeignPlaylist {
        name: stem.to_owned(),
        source: "Apple Music".into(),
        id: None,
        tracks,
    })
}

/// The Apple Music library's own playlists (read with your key), and what couldn't be read:
/// a playlist entry whose song the library file doesn't describe is counted, never hidden.
pub fn apple_library(apple: &crate::apple::AppleLibrary) -> (Vec<ForeignPlaylist>, Vec<String>) {
    let mut problems = Vec::new();
    let lists = apple
        .playlists
        .iter()
        .map(|p| {
            let tracks: Vec<Foreign> = p
                .tracks
                .iter()
                .filter_map(|id| apple.tracks.get(id))
                .map(|t| Foreign {
                    title: t.title.clone(),
                    artist: t.artist.clone(),
                    album: t.album.clone(),
                    duration_ms: t.duration_ms.filter(|d| *d > 0),
                    ..Default::default()
                })
                .collect();
            let unread = p.tracks.len() - tracks.len();
            if unread > 0 {
                problems.push(format!(
                    "Apple Music's library file has no details for {unread} of the {} songs in “{}”, so they can't be brought over.",
                    p.tracks.len(),
                    p.name
                ));
            }
            ForeignPlaylist {
                name: p.name.clone(),
                source: "Apple Music".into(),
                id: Some(format!("apple:{}", p.name)),
                tracks,
            }
        })
        .filter(|p| !p.tracks.is_empty())
        .collect();
    (lists, problems)
}

// --- Other players' playlist files ---------------------------------------------------------

/// What the file at `path` says about itself (any format lofty reads).
fn tags_of(path: &Path) -> Option<Foreign> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::Accessor;
    let f = lofty::read_from_path(path).ok()?;
    let tag = f.primary_tag().or_else(|| f.first_tag());
    Some(Foreign {
        title: tag
            .and_then(|t| t.title().map(|s| s.into_owned()))
            .unwrap_or_default(),
        artist: tag
            .and_then(|t| t.artist().map(|s| s.into_owned()))
            .unwrap_or_default(),
        album: tag
            .and_then(|t| t.album().map(|s| s.into_owned()))
            .unwrap_or_default(),
        duration_ms: Some(f.properties().duration().as_millis() as u64).filter(|d| *d > 0),
        file: Some(path.to_string_lossy().into_owned()),
        ..Default::default()
    })
}

/// One entry of another player's list: the file's own tags when it's here, else what the
/// playlist line says ("Artist - Title" or the file name).
fn entry(location: &str, extinf: Option<(Option<u64>, String)>, dir: &Path) -> Foreign {
    let loc = location.trim();
    let path = if Path::new(loc).is_absolute() {
        PathBuf::from(loc)
    } else {
        dir.join(loc.replace('\\', "/"))
    };
    if path.is_file()
        && let Some(f) = tags_of(&path).filter(|f| !f.title.is_empty())
    {
        return f;
    }
    let (dur, label) = extinf.unwrap_or((None, String::new()));
    let (artist, title) = match label.split_once(" - ") {
        Some((a, t)) if !label.is_empty() => (a.trim().to_owned(), t.trim().to_owned()),
        _ if !label.is_empty() => (String::new(), label.trim().to_owned()),
        _ => (String::new(), playlists::title_from_file(loc)),
    };
    Foreign {
        title,
        artist,
        duration_ms: dur,
        file: path.is_file().then(|| path.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

fn m3u(text: &str, stem: &str, dir: &Path) -> ForeignPlaylist {
    let mut name = stem.to_owned();
    let mut pending: Option<(Option<u64>, String)> = None;
    let mut tracks = Vec::new();
    let mut venuta = false;
    for line in text.lines().map(str::trim) {
        if let Some(n) = line.strip_prefix("#PLAYLIST:") {
            name = n.trim().to_owned();
        } else if line.starts_with("#VENUTA") {
            venuta = true;
        } else if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (secs, label) = info.split_once(',').unwrap_or((info, ""));
            pending = Some((
                secs.trim()
                    .parse::<i64>()
                    .ok()
                    .filter(|s| *s > 0)
                    .map(|s| s as u64 * 1000),
                label.to_owned(),
            ));
        } else if !line.is_empty() && !line.starts_with('#') {
            tracks.push(entry(line, pending.take(), dir));
        }
    }
    ForeignPlaylist {
        name,
        source: if venuta {
            "Venuta".into()
        } else {
            "Playlist file".into()
        },
        id: None,
        tracks,
    }
}

fn pls(text: &str, stem: &str, dir: &Path) -> ForeignPlaylist {
    let mut files: BTreeMap<u32, (String, Option<String>, Option<u64>)> = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.to_lowercase();
        let num = |p: &str| k.strip_prefix(p).and_then(|n| n.parse::<u32>().ok());
        if let Some(n) = num("file") {
            files.entry(n).or_default().0 = v.to_owned();
        } else if let Some(n) = num("title") {
            files.entry(n).or_default().1 = Some(v.to_owned());
        } else if let Some(n) = num("length") {
            files.entry(n).or_default().2 = v
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|s| *s > 0)
                .map(|s| s as u64 * 1000);
        }
    }
    ForeignPlaylist {
        name: stem.to_owned(),
        source: "Playlist file".into(),
        id: None,
        tracks: files
            .into_values()
            .filter(|(f, _, _)| !f.is_empty())
            .map(|(f, t, d)| entry(&f, t.map(|t| (d, t)), dir))
            .collect(),
    }
}

// --- Deezer and YouTube (public endpoints, no key) ------------------------------------------

/// The playlist number in a Deezer link ("deezer.com/en/playlist/123…" or "deezer.page.link/…"
/// already opened in a browser).
pub fn deezer_id(link: &str) -> Option<String> {
    let re = regex::Regex::new(r"deezer\.com/(?:[a-z]{2}/)?playlist/(\d+)").expect("valid");
    re.captures(link).map(|c| c[1].to_owned())
}

/// A public Deezer playlist, every page of it.
/// The songs on one page of a Deezer playlist (`tracks`, or a `next` page): title, artist,
/// album, length and the recording code (ISRC), which Deezer gives for every song.
fn deezer_tracks(page: &serde_json::Value) -> Vec<Foreign> {
    let s =
        |v: Option<&serde_json::Value>| v.and_then(|x| x.as_str()).unwrap_or_default().to_owned();
    page.get("data")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .map(|t| Foreign {
            title: s(t.get("title")),
            artist: s(t.get("artist").and_then(|a| a.get("name"))),
            album: s(t.get("album").and_then(|a| a.get("title"))),
            duration_ms: t.get("duration").and_then(|d| d.as_u64()).map(|d| d * 1000),
            isrc: t
                .get("isrc")
                .and_then(|i| i.as_str())
                .map(|i| i.trim().to_uppercase())
                .filter(|i| i.len() >= 12),
            ..Default::default()
        })
        .collect()
}

pub fn deezer(http: &crate::net::Http, id: &str) -> Result<ForeignPlaylist, String> {
    let v = http
        .get_json(&format!("https://api.deezer.com/playlist/{id}"))
        .map_err(|e| format!("Deezer: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!(
            "Deezer couldn't show that playlist ({}). Is it public?",
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("error")
        ));
    }
    let name = v
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Deezer playlist")
        .to_owned();
    let mut tracks = Vec::new();
    let mut page = v.get("tracks").cloned();
    let mut guard = 0;
    while let Some(p) = page.take() {
        tracks.extend(deezer_tracks(&p));
        guard += 1;
        if let Some(next) = p.get("next").and_then(|n| n.as_str())
            && guard < 100
        {
            page = Some(http.get_json(next).map_err(|e| format!("Deezer: {e}"))?);
        }
    }
    Ok(ForeignPlaylist {
        name,
        source: "Deezer".into(),
        id: Some(format!("deezer:{id}")),
        tracks,
    })
}

/// A YouTube title as a song: "Artist - Title (Official Video)" or "Title | Film | Artist"; the
/// channel ("Artist - Topic", "ArtistVEVO") fills in the artist when the title doesn't say.
pub fn youtube_song(title: &str, channel: &str) -> (String, String) {
    let noise = regex::Regex::new(r"(?i)\s*[\(\[][^\)\]]*(official|video|audio|lyric|lyrics|visualizer|hd|4k|full song|music video)[^\)\]]*[\)\]]").expect("valid");
    let clean = noise.replace_all(title, "").trim().to_owned();
    let channel = channel
        .trim_end_matches(" - Topic")
        .trim_end_matches("VEVO")
        .trim()
        .to_owned();
    if let Some((a, t)) = clean.split_once(" - ") {
        return (
            t.split('|').next().unwrap_or(t).trim().to_owned(),
            a.trim().to_owned(),
        );
    }
    let first = clean.split('|').next().unwrap_or(&clean).trim().to_owned();
    (first, channel)
}

/// Fill in titles for YouTube entries (one public oEmbed request each, cached).
pub fn youtube_titles(
    http: &crate::net::Http,
    lists: &mut [ForeignPlaylist],
    on_progress: &mut dyn FnMut(usize, usize),
) {
    let total: usize = lists
        .iter()
        .flat_map(|l| &l.tracks)
        .filter(|t| t.youtube.is_some() && t.title.is_empty())
        .count();
    let mut done = 0;
    for l in lists.iter_mut() {
        for t in l
            .tracks
            .iter_mut()
            .filter(|t| t.youtube.is_some() && t.title.is_empty())
        {
            let id = t.youtube.clone().unwrap_or_default();
            let url = format!(
                "https://www.youtube.com/oembed?format=json&url={}",
                crate::net::enc(&format!("https://www.youtube.com/watch?v={id}"))
            );
            if let Ok(v) = http.get_json(&url) {
                let title = v.get("title").and_then(|x| x.as_str()).unwrap_or_default();
                let channel = v
                    .get("author_name")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default();
                let (title, artist) = youtube_song(title, channel);
                t.title = title;
                t.artist = artist;
            }
            done += 1;
            on_progress(done, total);
        }
        // Videos that are gone (private, deleted) have no title to match by.
        l.tracks.retain(|t| !t.title.is_empty());
    }
}

// --- Matching and writing -------------------------------------------------------------------

/// A library song an imported one was matched to.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub foreign: Foreign,
    pub found: Option<Candidate>,
    pub grade: Option<Grade>,
    pub why: String,
    /// When not in the library: a stable online id (Takeout video id, or a catalog hit). Never a stream URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online: Option<OnlineEntry>,
}

/// One playlist as it would come in: every song with its match, ready for you to confirm.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewPlaylist {
    pub name: String,
    pub source: String,
    /// Which playlist at the source (to update it again later).
    pub source_id: Option<String>,
    /// A playlist by this name is already in the library.
    pub exists: bool,
    /// The playlist you have that this one would update (by where it came from, else by name).
    pub target: Option<Target>,
    pub items: Vec<PreviewItem>,
    pub exact: usize,
    pub close: usize,
    /// Not in the library and not seeded as online either (true To get).
    pub missing: usize,
    /// Not in the library, but will write as `#VENUTA-ONLINE:` (stable id).
    pub online: usize,
}

/// A playlist you have that an incoming one updates, and what the update would do to it.
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub name: String,
    /// Found by where it came from (it remembers), not only by its name.
    pub linked: bool,
    /// Songs in it now, and how many of them are in the library.
    pub songs: usize,
    pub here: usize,
    /// After the update, with every close match taken.
    pub after: usize,
    pub after_here: usize,
    /// Songs the source has that it doesn't yet.
    pub added: usize,
    /// Songs you added yourself, which stay.
    pub yours: usize,
    /// Songs the source dropped since it was last brought in (they go).
    pub removed: usize,
    /// Songs you took out, which stay out even though the source still has them.
    pub skipped: usize,
}

/// An incoming song for a merge: the line to write, and the keys that tell which song it is.
struct Incoming {
    item: Item,
    keys: Vec<String>,
}

fn incoming_of(lib: &Library, items: &[Item]) -> Vec<Incoming> {
    let by_path: std::collections::HashMap<&str, &crate::library::Track> =
        lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
    items
        .iter()
        .map(|i| {
            let mut keys = Vec::new();
            if let Some(t) = i.path.as_deref().and_then(|p| by_path.get(p)) {
                keys.extend(playlists::track_keys(t));
            }
            for w in i.want.iter().chain(i.source.iter()) {
                keys.extend(playlists::want_keys(w));
            }
            if let Some(o) = &i.online {
                keys.extend(playlists::online_keys(o));
            }
            Incoming {
                item: i.clone(),
                keys,
            }
        })
        .collect()
}

/// What merging an incoming playlist into one you have gives.
#[derive(Debug, Default)]
struct Merged {
    items: Vec<Item>,
    /// The source's songs now (for the next update).
    base: Vec<Vec<String>>,
    added: usize,
    yours: usize,
    removed: usize,
    skipped: usize,
}

/// Bring a playlist you have up to date with its source, losing nothing of yours:
/// - the source's order is the spine; a song you already have keeps the exact file it uses
///   (a song you picked by hand stays your pick), a song still to get becomes its file once
///   the library has it;
/// - songs you added stay, right after the song they followed;
/// - with what the source had last time (`base`): a song you took out stays out, and a song
///   the source dropped goes. Without it (an older import), nothing is taken out.
fn merge(
    lib: &Library,
    existing: &[playlists::PlaylistEntry],
    incoming: &[Incoming],
    base: Option<&[Vec<String>]>,
) -> Merged {
    let by_path: std::collections::HashMap<&str, &crate::library::Track> =
        lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
    let keys_of = |e: &playlists::PlaylistEntry| -> Vec<String> {
        let mut k = Vec::new();
        if let Some(t) = (!e.missing).then(|| by_path.get(e.path.as_str())).flatten() {
            k.extend(playlists::track_keys(t));
        }
        if let Some(o) = &e.online {
            k.extend(playlists::online_keys(o));
        }
        if let Some(w) = &e.want {
            k.extend(playlists::want_keys(w));
        } else if e.missing && e.online.is_none() {
            k.extend(playlists::song_keys(None, &e.title, &e.artist, None));
        }
        k
    };
    let mine: Vec<Vec<String>> = existing.iter().map(keys_of).collect();
    let mut where_mine: std::collections::HashMap<&str, Vec<usize>> = Default::default();
    for (i, ks) in mine.iter().enumerate() {
        for k in ks {
            where_mine.entry(k.as_str()).or_default().push(i);
        }
    }
    let in_base: HashSet<&str> = base
        .into_iter()
        .flatten()
        .flatten()
        .map(String::as_str)
        .collect();
    let was_in_base = |ks: &[String]| ks.iter().any(|k| in_base.contains(k.as_str()));

    let mut out = Merged::default();
    // Each written line, with the entry of yours it came from.
    let mut lines: Vec<(Item, Option<usize>)> = Vec::new();
    let mut used = vec![false; existing.len()];
    for inc in incoming {
        out.base.push(inc.keys.clone());
        let mut cands: Vec<usize> = inc
            .keys
            .iter()
            .filter_map(|k| where_mine.get(k.as_str()))
            .flatten()
            .copied()
            .filter(|&j| !used[j])
            .collect();
        cands.sort_unstable();
        match cands.first().copied() {
            Some(j) => {
                used[j] = true;
                let e = &existing[j];
                // Yours is kept as it is, unless it was waiting and the import found the file.
                let item = if e.missing && inc.item.path.is_some() {
                    inc.item.clone()
                } else {
                    e.item()
                };
                lines.push((item, Some(j)));
            }
            None if base.is_some() && was_in_base(&inc.keys) => out.skipped += 1,
            None => {
                out.added += 1;
                lines.push((inc.item.clone(), None));
            }
        }
    }
    // What's left of yours: dropped by the source, or added by you (kept beside its neighbour).
    for j in 0..existing.len() {
        if used[j] {
            continue;
        }
        if base.is_some() && was_in_base(&mine[j]) {
            out.removed += 1;
            continue;
        }
        out.yours += 1;
        // Right after the nearest earlier song of yours that is placed (the top if none).
        let at = (0..j)
            .rev()
            .find_map(|k| lines.iter().position(|(_, from)| *from == Some(k)))
            .map_or(0, |p| p + 1);
        lines.insert(at, (existing[j].item(), Some(j)));
        used[j] = true;
    }
    out.items = lines.into_iter().map(|(i, _)| i).collect();
    out
}

/// A playlist at a source that you don't have all of: an older import kept fewer songs, or it
/// was never brought over.
#[derive(Debug, Clone, Serialize)]
pub struct Behind {
    /// The source's name for it.
    pub name: String,
    /// Yours, when you have it.
    pub yours: Option<String>,
    pub have: usize,
    pub source_songs: usize,
}

/// The source's playlists you have fewer songs of than it has (counting songs still to get).
pub fn behind(library_root: &Path, lib: &Library, lists: &[ForeignPlaylist]) -> Vec<Behind> {
    let have = playlists::list(library_root, lib);
    lists
        .iter()
        .filter_map(|l| {
            let yours = target_of(library_root, &have, l).map(|(n, _)| n);
            let count = yours
                .as_ref()
                .and_then(|n| have.iter().find(|p| &p.name == n))
                .map_or(0, |p| p.tracks);
            (count < l.tracks.len()).then(|| Behind {
                name: l.name.clone(),
                yours,
                have: count,
                source_songs: l.tracks.len(),
            })
        })
        .collect()
}

/// The playlist you have that an incoming one is: the one that came from the same place,
/// else the one with its name (as a file name: "still loves..." is "still loves").
fn target_of(
    library_root: &Path,
    have: &[playlists::PlaylistSummary],
    l: &ForeignPlaylist,
) -> Option<(String, bool)> {
    if let Some(id) = &l.id
        && let Some(p) = have
            .iter()
            .find(|p| !p.smart && p.source_id.as_deref() == Some(id.as_str()))
    {
        return Some((p.name.clone(), true));
    }
    let file = crate::plan::sanitize(&l.name, 100).to_lowercase();
    have.iter()
        .find(|p| !p.smart && p.name.to_lowercase() == file)
        .filter(|_| playlists::exists(library_root, &l.name))
        .map(|p| (p.name.clone(), false))
}

/// Match every song of every playlist, without writing anything.
pub fn preview(
    library_root: &Path,
    lib: &Library,
    lists: &[ForeignPlaylist],
) -> Vec<PreviewPlaylist> {
    preview_with(library_root, lib, lists, None)
}

/// Like [`preview`], and for songs still missing after the library match, try a catalog hit
/// (stable id only). `catalog` is optional so dry imports stay offline.
pub fn preview_with(
    library_root: &Path,
    lib: &Library,
    lists: &[ForeignPlaylist],
    catalog: Option<&dyn MediaAccess>,
) -> Vec<PreviewPlaylist> {
    let matcher = Matcher::new(lib);
    let have = playlists::list(library_root, lib);
    // A file that's already in the library is itself, whatever its tags say.
    let root = library_root
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    let by_path: std::collections::HashMap<String, &crate::library::Track> = lib
        .tracks
        .iter()
        .map(|t| (t.path.to_lowercase(), t))
        .collect();
    let cancel = CancellationToken::default();
    lists
        .iter()
        .map(|l| {
            let items: Vec<PreviewItem> = l
                .tracks
                .iter()
                .map(|f| {
                    let in_library = f.file.as_ref().and_then(|p| {
                        let p = p.replace('\\', "/").to_lowercase();
                        p.strip_prefix(&root)
                            .map(|r| r.trim_start_matches('/').to_owned())
                    });
                    let hit = in_library
                        .and_then(|rel| by_path.get(&rel).copied())
                        .map(|t| playlists::Found {
                            track: t,
                            grade: Grade::Exact,
                            why: "The same file".into(),
                        })
                        .or_else(|| matcher.grade(f));
                    match hit {
                        Some(h) => PreviewItem {
                            foreign: f.clone(),
                            found: Some(Candidate {
                                path: h.track.path.clone(),
                                title: h.track.first("TITLE").unwrap_or_default().to_owned(),
                                artist: h.track.first("ARTIST").unwrap_or_default().to_owned(),
                                album: h.track.first("ALBUM").unwrap_or_default().to_owned(),
                                duration_ms: h.track.duration_ms,
                            }),
                            grade: Some(h.grade),
                            why: h.why,
                            online: None,
                        },
                        None => {
                            let online = f.online().or_else(|| {
                                catalog.and_then(|a| catalog_online_for(a, f, &cancel))
                            });
                            let why = if online.is_some() {
                                "Not in your library; plays from the online catalog".into()
                            } else {
                                "Not in your library".into()
                            };
                            PreviewItem {
                                foreign: f.clone(),
                                found: None,
                                grade: None,
                                why,
                                online,
                            }
                        }
                    }
                })
                .collect();
            let count = |g: Option<Grade>| items.iter().filter(|i| i.grade == g).count();
            let online_n = items
                .iter()
                .filter(|i| i.found.is_none() && i.online.is_some())
                .count();
            let missing = items
                .iter()
                .filter(|i| i.found.is_none() && i.online.is_none())
                .count();
            let target = target_of(library_root, &have, l).and_then(|(name, linked)| {
                let (entries, _) = playlists::read(library_root, lib, &name).ok()?;
                let header = playlists::header_of(library_root, &name);
                let lines: Vec<Item> = items.iter().map(PreviewItem::item).collect();
                let same_source = l.id.is_some() && header.source_id == l.id;
                let m = merge(
                    lib,
                    &entries,
                    &incoming_of(lib, &lines),
                    header.base.as_deref().filter(|_| same_source),
                );
                Some(Target {
                    name,
                    linked,
                    songs: entries.len(),
                    here: entries.iter().filter(|e| !e.missing).count(),
                    after: m.items.len(),
                    after_here: m.items.iter().filter(|i| i.path.is_some()).count(),
                    added: m.added,
                    yours: m.yours,
                    removed: m.removed,
                    skipped: m.skipped,
                })
            });
            PreviewPlaylist {
                name: l.name.clone(),
                source: l.source.clone(),
                source_id: l.id.clone(),
                exists: playlists::exists(library_root, &l.name),
                target,
                exact: count(Some(Grade::Exact)),
                close: count(Some(Grade::Close)),
                missing,
                online: online_n,
                items,
            }
        })
        .collect()
}

/// A catalog track that is sure enough to write as online for this foreign song (stable id only).
pub fn catalog_online_for(
    access: &dyn MediaAccess,
    f: &Foreign,
    cancel: &CancellationToken,
) -> Option<OnlineEntry> {
    if let Some(o) = f.online() {
        return Some(o);
    }
    let title = f.title.trim();
    if title.is_empty() {
        return None;
    }
    let query = if f.artist.trim().is_empty() {
        title.to_owned()
    } else {
        format!("{} {}", title, f.artist.trim())
    };
    let page = access
        .search_tracks(&query, &PageRequest::new(5, None), cancel)
        .ok()?;
    let want_title = playlists::title_key(title);
    if want_title.is_empty() {
        return None;
    }
    let want_artists: HashSet<String> = crate::plan::split_names(&f.artist)
        .iter()
        .map(|a| crate::audit::name_key(a))
        .filter(|a| !a.is_empty())
        .collect();
    page.items.into_iter().find_map(|t| {
        if playlists::title_key(&t.title) != want_title {
            return None;
        }
        if !want_artists.is_empty() {
            let hit = t
                .artists
                .iter()
                .any(|a| want_artists.contains(&crate::audit::name_key(a)));
            if !hit {
                return None;
            }
        }
        if let (Some(a), Some(b)) = (f.duration_ms, t.duration_ms)
            && a.abs_diff(b) > 5_000
        {
            return None;
        }
        let entry = OnlineEntry::from_remote(&t);
        entry.is_stable().then_some(entry).filter(|_| {
            // from_remote already drops stream-shaped artwork; track_id must not be an address.
            !t.track_id.contains("://") && !t.provider.contains("://")
        })
    })
}

impl PreviewItem {
    /// The line this song becomes when every close match is taken.
    /// Unmatched songs with a catalog/Takeout id become online; otherwise they stay wanted.
    pub fn item(&self) -> Item {
        match &self.found {
            Some(c) => Item {
                path: Some(c.path.clone()),
                want: None,
                source: Some(self.foreign.want()),
                online: None,
            },
            None => {
                if let Some(o) = self.online.clone().or_else(|| self.foreign.online()) {
                    Item::online(o)
                } else {
                    Item::wanted(self.foreign.want())
                }
            }
        }
    }
}

/// A playlist to bring in, as you confirmed it: each song a library path or still wanted.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Choice {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub source_id: Option<String>,
    pub items: Vec<Item>,
    /// Bring this playlist you have up to date instead (see `merge`): nothing of yours is lost.
    #[serde(default)]
    pub update: Option<String>,
    /// Replace the playlist of this name (the old one is set aside), else take a free name.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Written {
    /// The name it was saved under (a number is added when the name was taken).
    pub name: String,
    /// Songs matched to a library file.
    pub songs: usize,
    /// Songs still waiting on To get.
    pub wanted: usize,
    /// Songs written as online (stable id, playable without a file).
    #[serde(default)]
    pub online: usize,
    /// It updated a playlist you had: what changed, and the copy kept of how it was.
    pub updated: bool,
    pub added: usize,
    pub removed: usize,
    pub copy: Option<String>,
}

/// Write the playlists you confirmed, and the albums they still want.
pub fn apply(
    library_root: &Path,
    lib: &Library,
    choices: &[Choice],
    when: &str,
) -> Result<(Vec<Written>, Vec<WantedAlbum>), String> {
    let mut written = Vec::new();
    let mut groups: BTreeMap<(String, String), (HashSet<String>, HashSet<String>)> =
        BTreeMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    for c in choices {
        let source = format!("{} {when}", c.source).trim().to_owned();
        let target = c
            .update
            .as_deref()
            .filter(|t| playlists::exists(library_root, t) && !taken.contains(&t.to_lowercase()));
        let (name, items, done) = if let Some(t) = target {
            let (entries, _) = playlists::read(library_root, lib, t)?;
            let before = playlists::header_of(library_root, t);
            let same_source = c.source_id.is_some() && before.source_id == c.source_id;
            let m = merge(
                lib,
                &entries,
                &incoming_of(lib, &c.items),
                before.base.as_deref().filter(|_| same_source),
            );
            let copy = playlists::keep_copy(library_root, t)?;
            let header = playlists::Header {
                source: Some(source.clone()),
                source_id: c.source_id.clone().or(before.source_id),
                base: Some(m.base.clone()),
            };
            playlists::write_with(library_root, lib, t, &m.items, false, &header)?;
            let done = Written {
                name: t.to_owned(),
                songs: m.items.iter().filter(|i| i.path.is_some()).count(),
                wanted: m.items.iter().filter(|i| i.want.is_some()).count(),
                online: m
                    .items
                    .iter()
                    .filter(|i| i.online.is_some() && i.path.is_none())
                    .count(),
                updated: true,
                added: m.added,
                removed: m.removed,
                copy: Some(copy.to_string_lossy().into_owned()),
            };
            (t.to_owned(), m.items, done)
        } else {
            // "Replace" means the playlist there before this import, never one this import
            // just wrote.
            let replace = c.replace && !taken.contains(&c.name.trim().to_lowercase());
            let name = if replace && playlists::exists(library_root, &c.name) {
                playlists::delete(library_root, &c.name)?;
                c.name.trim().to_owned()
            } else {
                playlists::free_name(library_root, &c.name)
            };
            let header = playlists::Header {
                source: Some(source.clone()),
                source_id: c.source_id.clone(),
                base: Some(
                    incoming_of(lib, &c.items)
                        .into_iter()
                        .map(|i| i.keys)
                        .collect(),
                ),
            };
            playlists::write_with(library_root, lib, &name, &c.items, false, &header)?;
            let done = Written {
                name: name.clone(),
                songs: c.items.iter().filter(|i| i.path.is_some()).count(),
                wanted: c.items.iter().filter(|i| i.want.is_some()).count(),
                online: c
                    .items
                    .iter()
                    .filter(|i| i.online.is_some() && i.path.is_none())
                    .count(),
                updated: false,
                added: c.items.len(),
                removed: 0,
                copy: None,
            };
            (name, c.items.clone(), done)
        };
        taken.insert(name.to_lowercase());
        for w in items.iter().filter_map(|i| i.want.as_ref()) {
            let e = groups
                .entry((w.album.clone(), w.artist.clone()))
                .or_default();
            e.0.insert(format!(
                "{}|{}",
                w.title.to_lowercase(),
                w.artist.to_lowercase()
            ));
            e.1.insert(name.clone());
        }
        written.push(done);
    }
    let wanted = playlists::rank_wanted(
        lib,
        groups
            .into_iter()
            .filter(|((album, _), _)| !album.trim().is_empty())
            .map(|((album, artist), (songs, lists))| WantGroup {
                album,
                artist,
                genre: String::new(),
                saved: songs.len(),
                playlists: lists.into_iter().collect(),
            })
            .collect(),
    );
    Ok((written, wanted))
}

/// Match a playlist's missing songs again (after new music came in): the ones now found are
/// put in place. Returns how many were found.
pub fn refind(library_root: &Path, lib: &Library, name: &str) -> Result<usize, String> {
    refind_with(library_root, lib, &Matcher::new(lib), name)
}

/// Every playlist's missing songs matched again (exact matches only), for when new music came
/// in. Returns how many songs were put in place.
pub fn refind_all(library_root: &Path, lib: &Library) -> usize {
    let matcher = Matcher::new(lib);
    playlists::list(library_root, lib)
        .into_iter()
        .filter(|p| !p.smart && p.missing > 0)
        .map(|p| refind_with(library_root, lib, &matcher, &p.name).unwrap_or(0))
        .sum()
}

fn refind_with(
    library_root: &Path,
    lib: &Library,
    matcher: &Matcher,
    name: &str,
) -> Result<usize, String> {
    let (entries, _) = playlists::read(library_root, lib, name)?;
    let mut found = 0;
    let items: Vec<Item> = entries
        .iter()
        .map(|e| {
            if let Some(w) = e.want.as_ref().filter(|_| e.missing)
                && let Some(h) = matcher
                    .grade(&w.foreign())
                    .filter(|h| h.grade == Grade::Exact && !w.not.contains(&h.track.path))
            {
                found += 1;
                return Item::track(h.track.path.clone());
            }
            e.item()
        })
        .collect();
    if found > 0 {
        playlists::write_items(
            library_root,
            lib,
            name,
            &items,
            false,
            playlists::source_of(library_root, name).as_deref(),
        )?;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_handles_quotes_and_line_breaks() {
        let rows = csv_rows("a,\"b, c\",\"say \"\"hi\"\"\"\n\"x\ny\",2,3\n");
        assert_eq!(
            rows,
            vec![vec!["a", "b, c", "say \"hi\""], vec!["x\ny", "2", "3"]]
        );
    }

    #[test]
    fn exportify_csv_is_spotify_with_isrc_and_length() {
        let text = "Track URI,Track Name,Artist Name(s),Album Name,Track Duration (ms),ISRC\nspotify:track:1,Tum Hi Ho,Arijit Singh,Aashiqui 2,262000,INT101300123\n";
        let lists = csv_playlists(text, "Road trip").unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].name, "Road trip");
        assert_eq!(lists[0].source, "Spotify");
        let t = &lists[0].tracks[0];
        assert_eq!(
            (t.title.as_str(), t.artist.as_str(), t.album.as_str()),
            ("Tum Hi Ho", "Arijit Singh", "Aashiqui 2")
        );
        assert_eq!(t.duration_ms, Some(262_000));
        assert_eq!(t.isrc.as_deref(), Some("INT101300123"));
    }

    #[test]
    fn tunemymusic_csv_groups_by_playlist() {
        let text = "Track name,Artist name,Album,Playlist name,Type,ISRC\nA,X,AA,One,Playlist,\nB,Y,BB,Two,Playlist,\nC,Z,CC,One,Playlist,\n";
        let lists = csv_playlists(text, "export").unwrap();
        assert_eq!(
            lists
                .iter()
                .map(|l| (l.name.as_str(), l.tracks.len()))
                .collect::<Vec<_>>(),
            vec![("One", 2), ("Two", 1)]
        );
        assert_eq!(lists[0].source, "TuneMyMusic");
    }

    #[test]
    fn takeout_csv_skips_its_header_block() {
        let text = "Playlist Id,Channel Id,Time Created\nPL1,UC1,2024\n\nVideo Id,Time Added\ndQw4w9WgXcQ,2024\njNQXAC9IVRw,2024\n";
        let lists = csv_playlists(text, "Chill-videos").unwrap();
        assert_eq!(lists[0].name, "Chill");
        assert_eq!(lists[0].source, "YouTube Music");
        assert_eq!(
            lists[0]
                .tracks
                .iter()
                .map(|t| t.youtube.clone().unwrap())
                .collect::<Vec<_>>(),
            vec!["dQw4w9WgXcQ", "jNQXAC9IVRw"]
        );
    }

    #[test]
    fn unmatched_takeout_becomes_online_not_want() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            tracks: vec![],
            lyrics_files: Default::default(),
            images: Default::default(),
            meta: Default::default(),
            fetched_lyrics: Default::default(),
            pairs: Default::default(),
            identities: Default::default(),
            in_library: false,
            lookup: Default::default(),
        };
        let lists = vec![ForeignPlaylist {
            name: "Chill".into(),
            source: "YouTube Music".into(),
            id: None,
            tracks: vec![
                Foreign {
                    title: "Tum Hi Ho".into(),
                    artist: "Arijit Singh".into(),
                    youtube: Some("dQw4w9WgXcQ".into()),
                    ..Default::default()
                },
                Foreign {
                    title: "No Id Song".into(),
                    artist: "Someone".into(),
                    album: "Gone".into(),
                    ..Default::default()
                },
                Foreign {
                    // Address-shaped ids must never become online.
                    title: "Bad".into(),
                    youtube: Some("https://cdn.example/a.mp3".into()),
                    ..Default::default()
                },
            ],
        }];
        let preview = preview(root.path(), &lib, &lists);
        assert_eq!(preview[0].online, 1);
        assert_eq!(preview[0].missing, 2);
        let items: Vec<_> = preview[0].items.iter().map(PreviewItem::item).collect();
        assert!(items[0].online.is_some());
        assert_eq!(items[0].online.as_ref().unwrap().track_id, "dQw4w9WgXcQ");
        assert_eq!(items[0].online.as_ref().unwrap().provider, "remote_catalog");
        assert!(items[0].want.is_none());
        assert!(items[1].want.is_some() && items[1].online.is_none());
        assert!(items[2].want.is_some() && items[2].online.is_none());

        let choice = Choice {
            name: "Chill".into(),
            source: "YouTube Music".into(),
            items,
            ..Default::default()
        };
        let (written, wanted) = apply(root.path(), &lib, &[choice], "2026-10-14").unwrap();
        assert_eq!(written[0].online, 1);
        assert_eq!(written[0].wanted, 2);
        assert_eq!(written[0].songs, 0);
        let text =
            std::fs::read_to_string(playlists::folder(root.path()).join("Chill.m3u8")).unwrap();
        assert!(text.contains("#VENUTA-ONLINE:"), "{text}");
        assert!(text.contains("dQw4w9WgXcQ"), "{text}");
        assert!(!text.contains("://cdn.example"), "{text}");
        assert!(!text.contains("googlevideo"), "{text}");
        // Cover picture URLs are allowed; stream hosts are not.
        assert!(text.contains("i.ytimg.com"), "{text}");
        let (e, _) = playlists::read(root.path(), &lib, "Chill").unwrap();
        assert_eq!(
            e[0].online.as_ref().map(|o| o.track_id.as_str()),
            Some("dQw4w9WgXcQ")
        );
        assert!(e[1].want.is_some());
        assert_eq!(wanted.len(), 1);
        assert_eq!(wanted[0].album, "Gone");
    }

    #[test]
    fn catalog_online_for_uses_youtube_id_or_strict_search_hit() {
        use media_access::{
            AccessError, AccessErrorKind, EphemeralAudio, FakeMediaAccess, MediaAccess, Page,
            RemoteAlbum, RemoteArtist, RemoteTrack,
        };

        // Known Takeout id: no search needed.
        let access = FakeMediaAccess::with_in_memory_fixture();
        let from_id = catalog_online_for(
            &access,
            &Foreign {
                title: "Anything".into(),
                youtube: Some("dQw4w9WgXcQ".into()),
                ..Default::default()
            },
            &CancellationToken::default(),
        )
        .expect("youtube id");
        assert_eq!(from_id.track_id, "dQw4w9WgXcQ");
        assert_eq!(from_id.provider, "remote_catalog");

        struct ExactCatalog;
        impl MediaAccess for ExactCatalog {
            fn search_tracks(
                &self,
                query: &str,
                page: &PageRequest,
                cancel: &CancellationToken,
            ) -> Result<Page<RemoteTrack>, AccessError> {
                if cancel.is_cancelled() {
                    return Err(AccessError::new(AccessErrorKind::Cancelled, "cancelled"));
                }
                let track = RemoteTrack {
                    provider: "remote_catalog".into(),
                    track_id: "hit-1".into(),
                    title: if query.contains("Kesariya") {
                        "Kesariya".into()
                    } else {
                        format!("{query} other")
                    },
                    artists: vec!["Arijit Singh".into()],
                    album: Some("Brahmastra".into()),
                    album_id: None,
                    duration_ms: Some(268_000),
                    artwork_url: None,
                    isrc: None,
                };
                Ok(Page {
                    items: vec![track].into_iter().take(page.limit).collect(),
                    next_cursor: None,
                })
            }
            fn search_albums(
                &self,
                _: &str,
                _: &PageRequest,
                _: &CancellationToken,
            ) -> Result<Page<RemoteAlbum>, AccessError> {
                Ok(Page {
                    items: vec![],
                    next_cursor: None,
                })
            }
            fn search_artists(
                &self,
                _: &str,
                _: &PageRequest,
                _: &CancellationToken,
            ) -> Result<Page<RemoteArtist>, AccessError> {
                Ok(Page {
                    items: vec![],
                    next_cursor: None,
                })
            }
            fn album_tracks(
                &self,
                _: &str,
                _: &PageRequest,
                _: &CancellationToken,
            ) -> Result<Page<RemoteTrack>, AccessError> {
                Ok(Page {
                    items: vec![],
                    next_cursor: None,
                })
            }
            fn playlist_tracks(
                &self,
                _: &str,
                _: &PageRequest,
                _: &CancellationToken,
            ) -> Result<Page<RemoteTrack>, AccessError> {
                Ok(Page {
                    items: vec![],
                    next_cursor: None,
                })
            }
            fn resolve_audio(
                &self,
                _: &str,
                _: &CancellationToken,
            ) -> Result<EphemeralAudio, AccessError> {
                Err(AccessError::new(AccessErrorKind::Unsupported, "n/a"))
            }
        }

        let cat = ExactCatalog;
        let hit = catalog_online_for(
            &cat,
            &Foreign {
                title: "Kesariya".into(),
                artist: "Arijit Singh".into(),
                duration_ms: Some(268_000),
                ..Default::default()
            },
            &CancellationToken::default(),
        )
        .expect("strict title+artist hit");
        assert_eq!(hit.track_id, "hit-1");
        assert!(!format!("{hit:?}").contains("://"));

        assert!(
            catalog_online_for(
                &cat,
                &Foreign {
                    title: "Kesariya".into(),
                    artist: "Someone Else".into(),
                    ..Default::default()
                },
                &CancellationToken::default(),
            )
            .is_none(),
            "wrong artist stays WANT"
        );
    }

    #[test]
    fn spotify_account_data_reads_playlists_and_liked_songs() {
        let text = r#"{"playlists":[{"name":"Drive","items":[{"track":{"trackName":"Kun Faya Kun","artistName":"A.R. Rahman","albumName":"Rockstar","trackUri":"x"},"episode":null}]}],"tracks":[{"artist":"Pritam","album":"Barfi!","track":"Ala Barfi","uri":"y"}]}"#;
        let lists = spotify_json(text).unwrap();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].tracks[0].title, "Kun Faya Kun");
        assert_eq!(lists[1].name, "Liked Songs");
        assert_eq!(lists[1].tracks[0].artist, "Pritam");
    }

    #[test]
    fn itunes_xml_reads_playlists_by_track_id() {
        let text = r#"<?xml version="1.0"?><plist version="1.0"><dict>
            <key>Tracks</key><dict>
              <key>10</key><dict><key>Track ID</key><integer>10</integer><key>Name</key><string>Ae Ajnabi</string><key>Artist</key><string>A.R. Rahman &amp; Udit</string><key>Album</key><string>Dil Se</string><key>Total Time</key><integer>350000</integer></dict>
            </dict>
            <key>Playlists</key><array>
              <dict><key>Name</key><string>Library</string><key>Master</key><true/><key>Playlist Items</key><array><dict><key>Track ID</key><integer>10</integer></dict></array></dict>
              <dict><key>Name</key><string>Night</string><key>Playlist Items</key><array><dict><key>Track ID</key><integer>10</integer></dict></array></dict>
            </array></dict></plist>"#;
        let lists = itunes_xml(text).unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].name, "Night");
        assert_eq!(lists[0].tracks[0].artist, "A.R. Rahman & Udit");
        assert_eq!(lists[0].tracks[0].duration_ms, Some(350_000));
    }

    #[test]
    fn itunes_text_export_reads_columns() {
        let text =
            "Name\tArtist\tComposer\tAlbum\tTime\nSaans\tShreya Ghoshal\t\tJab Tak Hai Jaan\t264\n";
        let p = itunes_txt(text, "Evening").unwrap();
        assert_eq!(p.name, "Evening");
        assert_eq!(p.tracks[0].album, "Jab Tak Hai Jaan");
        assert_eq!(p.tracks[0].duration_ms, Some(264_000));
    }

    #[test]
    fn m3u_without_files_uses_extinf_or_file_names() {
        let text = "#EXTM3U\n#EXTINF:215,Lucky Ali - O Sanam\nC:/gone/x.mp3\nD:/music/07 - Kya Mujhe Pyar Hai.flac\n";
        let p = m3u(text, "Old", Path::new("."));
        assert_eq!(p.tracks[0].artist, "Lucky Ali");
        assert_eq!(p.tracks[0].title, "O Sanam");
        assert_eq!(p.tracks[0].duration_ms, Some(215_000));
        assert_eq!(p.tracks[1].title, "Kya Mujhe Pyar Hai");
    }

    #[test]
    fn a_deezer_page_gives_every_song_with_its_recording_code() {
        let page: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/pages/deezer-playlist.json"))
                .unwrap();
        let songs = deezer_tracks(&page["tracks"]);
        assert_eq!(songs.len(), 100);
        let first = &songs[0];
        assert_eq!(first.title, "Boston");
        assert_eq!(first.isrc.as_deref(), Some("QZJ842603468"));
        assert!(first.duration_ms.is_some_and(|ms| ms > 60_000));
        assert!(!first.artist.is_empty() && !first.album.is_empty());
        assert!(songs.iter().all(|s| s.isrc.is_some()));
    }

    #[test]
    fn deezer_links_and_youtube_titles() {
        assert_eq!(
            deezer_id("https://www.deezer.com/en/playlist/908622995?utm=x").as_deref(),
            Some("908622995")
        );
        assert_eq!(deezer_id("https://example.com/playlist/1"), None);
        assert_eq!(
            youtube_song("Arijit Singh - Tum Hi Ho (Official Video)", "T-Series"),
            ("Tum Hi Ho".into(), "Arijit Singh".into())
        );
        assert_eq!(
            youtube_song(
                "Kesariya | Brahmastra | Arijit Singh",
                "Arijit Singh - Topic"
            ),
            ("Kesariya".into(), "Arijit Singh".into())
        );
    }

    #[test]
    fn two_imports_with_one_name_both_survive() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            tracks: vec![],
            lyrics_files: Default::default(),
            images: Default::default(),
            meta: Default::default(),
            fetched_lyrics: Default::default(),
            pairs: Default::default(),
            identities: Default::default(),
            in_library: false,
            lookup: Default::default(),
        };
        let want = |t: &str| {
            Item::wanted(playlists::WantEntry {
                title: t.into(),
                artist: "X".into(),
                album: "Al".into(),
                duration_ms: 0,
                isrc: None,
                md5: None,
                not: Vec::new(),
            })
        };
        playlists::write_items(
            root.path(),
            &lib,
            "Liked Songs",
            &[want("Old")],
            false,
            None,
        )
        .unwrap();
        let choice = |t: &str| Choice {
            name: "Liked Songs".into(),
            source: "Spotify".into(),
            items: vec![want(t)],
            replace: true,
            ..Default::default()
        };
        let (written, wanted) =
            apply(root.path(), &lib, &[choice("A"), choice("B")], "2026-10-13").unwrap();
        // The first replaces the old one; the second must not replace the first.
        assert_eq!(
            written.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
            vec!["Liked Songs", "Liked Songs 2"]
        );
        let (first, _) = playlists::read(root.path(), &lib, "Liked Songs").unwrap();
        assert_eq!(first[0].title, "A");
        assert_eq!(wanted.len(), 1);
        assert_eq!(wanted[0].saved_tracks, 2);
    }

    #[test]
    fn durations_parse_every_way() {
        assert_eq!(duration_ms("3:45", false), Some(225_000));
        assert_eq!(duration_ms("1:02:10", false), Some(3_730_000));
        assert_eq!(duration_ms("225", false), Some(225_000));
        assert_eq!(duration_ms("225000", true), Some(225_000));
    }
    fn tune(id: i64, path: &str, title: &str, artist: &str) -> crate::library::Track {
        crate::library::Track {
            id,
            path: path.into(),
            dir: path.rsplit_once('/').map(|p| p.0).unwrap_or("").into(),
            kind: "flac".into(),
            size: 1,
            duration_ms: 200_000,
            sample_rate: 44_100,
            bit_depth: 16,
            audio_md5: Some(vec![id as u8; 16]),
            vendor: None,
            tags: std::collections::HashMap::from([
                ("TITLE".into(), vec![title.into()]),
                ("ARTIST".into(), vec![artist.into()]),
            ]),
            raw_keys: vec![],
            pictures: vec![],
            quality: None,
        }
    }

    fn shelf(tracks: Vec<crate::library::Track>) -> Library {
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

    fn wish(title: &str, artist: &str) -> playlists::WantEntry {
        playlists::WantEntry {
            title: title.into(),
            artist: artist.into(),
            ..Default::default()
        }
    }

    /// What a source lists, as the review step sends it: a file when it was matched (with
    /// what the source said), else the song to get.
    fn from_source(lib: &Library, songs: &[(&str, &str)]) -> Vec<Item> {
        let m = Matcher::new(lib);
        songs
            .iter()
            .map(|(t, a)| {
                let f = Foreign {
                    title: (*t).into(),
                    artist: (*a).into(),
                    ..Default::default()
                };
                match m.grade(&f) {
                    Some(h) => Item {
                        path: Some(h.track.path.clone()),
                        want: None,
                        source: Some(f.want()),
                        online: None,
                    },
                    None => Item::wanted(f.want()),
                }
            })
            .collect()
    }

    fn titles(root: &Path, lib: &Library, name: &str) -> Vec<String> {
        playlists::read(root, lib, name)
            .unwrap()
            .0
            .into_iter()
            .map(|e| e.title)
            .collect()
    }

    #[test]
    fn importing_again_fills_a_playlist_in_and_keeps_what_you_added() {
        let root = tempfile::tempdir().unwrap();
        let lib = shelf(vec![
            tune(1, "A/01 - Alpha.flac", "Alpha", "X"),
            tune(2, "A/02 - Yours.flac", "Yours", "Me"),
            tune(3, "A/03 - Gamma.flac", "Gamma", "X"),
        ]);
        // Made by an older import that kept only what it matched, then you added "Yours".
        playlists::write(
            root.path(),
            &lib,
            "Hindi",
            &["A/01 - Alpha.flac".into(), "A/02 - Yours.flac".into()],
            false,
        )
        .unwrap();
        let source = ForeignPlaylist {
            name: "Hindi".into(),
            source: "Apple Music".into(),
            id: Some("apple:Hindi".into()),
            tracks: vec![
                Foreign {
                    title: "Alpha".into(),
                    artist: "X".into(),
                    ..Default::default()
                },
                Foreign {
                    title: "Beta".into(),
                    artist: "X".into(),
                    ..Default::default()
                },
                Foreign {
                    title: "Gamma".into(),
                    artist: "X".into(),
                    ..Default::default()
                },
            ],
        };
        let p = &preview(root.path(), &lib, std::slice::from_ref(&source))[0];
        let t = p.target.as_ref().expect("it updates the playlist you have");
        assert_eq!(
            (t.name.as_str(), t.songs, t.after, t.added, t.yours),
            ("Hindi", 2, 4, 2, 1)
        );

        let items = from_source(&lib, &[("Alpha", "X"), ("Beta", "X"), ("Gamma", "X")]);
        let choice = Choice {
            name: "Hindi".into(),
            source: "Apple Music".into(),
            source_id: Some("apple:Hindi".into()),
            update: Some("Hindi".into()),
            items,
            ..Default::default()
        };
        let (written, _) = apply(root.path(), &lib, &[choice], "2026-10-13").unwrap();
        assert!(written[0].updated);
        assert_eq!(written[0].added, 2);
        // In the source's order, yours right after the song it followed, the gap kept.
        assert_eq!(
            titles(root.path(), &lib, "Hindi"),
            ["Alpha", "Yours", "Beta", "Gamma"]
        );
        let copy = written[0].copy.as_ref().expect("the old one is kept");
        assert!(Path::new(copy).is_file());
        // Undo puts the old one back.
        playlists::restore_copy(root.path(), "Hindi", Path::new(copy)).unwrap();
        assert_eq!(titles(root.path(), &lib, "Hindi"), ["Alpha", "Yours"]);
    }

    #[test]
    fn an_update_knows_what_you_removed_and_what_the_source_removed() {
        let root = tempfile::tempdir().unwrap();
        let lib = shelf(vec![
            tune(1, "A/01 - Alpha.flac", "Alpha", "X"),
            tune(2, "A/02 - Beta.flac", "Beta", "X"),
            tune(3, "A/03 - Mine.flac", "Mine", "Me"),
            tune(4, "A/04 - Echo.flac", "Echo", "X"),
        ]);
        let first = Choice {
            name: "Road".into(),
            source: "Spotify".into(),
            source_id: Some("spotify:1".into()),
            items: from_source(&lib, &[("Alpha", "X"), ("Beta", "X"), ("Echo", "X")]),
            ..Default::default()
        };
        apply(root.path(), &lib, &[first], "2026-10-01").unwrap();
        // You take Beta out and add one of yours.
        playlists::write(
            root.path(),
            &lib,
            "Road",
            &[
                "A/01 - Alpha.flac".into(),
                "A/03 - Mine.flac".into(),
                "A/04 - Echo.flac".into(),
            ],
            false,
        )
        .unwrap();
        // Spotify since: Echo gone, Foxtrot new. Found by where it came from, even renamed.
        playlists::rename(root.path(), "Road", "Road trip").unwrap();
        let now = ForeignPlaylist {
            name: "Road".into(),
            source: "Spotify".into(),
            id: Some("spotify:1".into()),
            tracks: [("Alpha", "X"), ("Beta", "X"), ("Foxtrot", "X")]
                .iter()
                .map(|(t, a)| Foreign {
                    title: (*t).into(),
                    artist: (*a).into(),
                    ..Default::default()
                })
                .collect(),
        };
        let p = &preview(root.path(), &lib, &[now])[0];
        let t = p.target.as_ref().unwrap();
        assert!(t.linked);
        assert_eq!(t.name, "Road trip");
        assert_eq!((t.added, t.skipped, t.removed, t.yours), (1, 1, 1, 1));
        let update = Choice {
            name: "Road trip".into(),
            source: "Spotify".into(),
            source_id: Some("spotify:1".into()),
            update: Some("Road trip".into()),
            items: from_source(&lib, &[("Alpha", "X"), ("Beta", "X"), ("Foxtrot", "X")]),
            ..Default::default()
        };
        apply(root.path(), &lib, &[update], "2026-10-13").unwrap();
        assert_eq!(
            titles(root.path(), &lib, "Road trip"),
            ["Alpha", "Mine", "Foxtrot"]
        );
    }

    #[test]
    fn a_song_you_found_by_hand_stays_the_one_you_chose() {
        let root = tempfile::tempdir().unwrap();
        // The source says "Kesariya (From Brahmastra)"; you picked your "Kesariya" for it.
        let lib = shelf(vec![tune(
            1,
            "B/01 - Kesariya.flac",
            "Kesariya",
            "Arijit Singh",
        )]);
        playlists::write(
            root.path(),
            &lib,
            "still loves",
            &["B/01 - Kesariya.flac".into()],
            false,
        )
        .unwrap();
        let source = ForeignPlaylist {
            name: "still loves...".into(),
            source: "Apple Music".into(),
            id: Some("apple:still loves...".into()),
            tracks: vec![
                Foreign {
                    title: "Kesariya (From \"Brahmastra\")".into(),
                    artist: "Pritam, Arijit Singh".into(),
                    ..Default::default()
                },
                Foreign {
                    title: "Other".into(),
                    artist: "Y".into(),
                    ..Default::default()
                },
            ],
        };
        let p = &preview(root.path(), &lib, &[source])[0];
        let t = p
            .target
            .as_ref()
            .expect("a name made safe for a file is still its name");
        assert_eq!(t.name, "still loves");
        assert_eq!((t.after, t.added, t.yours), (2, 1, 0));
        let items = vec![
            Item::wanted(wish(
                "Kesariya (From \"Brahmastra\")",
                "Pritam, Arijit Singh",
            )),
            Item::wanted(wish("Other", "Y")),
        ];
        let choice = Choice {
            name: "still loves...".into(),
            source: "Apple Music".into(),
            update: Some("still loves".into()),
            items,
            ..Default::default()
        };
        apply(root.path(), &lib, &[choice], "2026-10-13").unwrap();
        let (e, _) = playlists::read(root.path(), &lib, "still loves").unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, "B/01 - Kesariya.flac");
        assert!(e[1].missing);
    }
    #[test]
    fn playlists_short_of_their_source_are_found() {
        let root = tempfile::tempdir().unwrap();
        let lib = shelf(vec![tune(1, "A/01 - Alpha.flac", "Alpha", "X")]);
        // Kept only what matched (1 of 3), and one with every song.
        playlists::write(
            root.path(),
            &lib,
            "Hindi",
            &["A/01 - Alpha.flac".into()],
            false,
        )
        .unwrap();
        playlists::write_items(
            root.path(),
            &lib,
            "Whole",
            &[
                Item::track("A/01 - Alpha.flac"),
                Item::wanted(wish("Beta", "X")),
            ],
            false,
            None,
        )
        .unwrap();
        let list = |name: &str, n: usize| ForeignPlaylist {
            name: name.into(),
            source: "Apple Music".into(),
            id: Some(format!("apple:{name}")),
            tracks: (0..n)
                .map(|i| Foreign {
                    title: format!("Song {i}"),
                    artist: "X".into(),
                    ..Default::default()
                })
                .collect(),
        };
        let b = behind(
            root.path(),
            &lib,
            &[list("Hindi", 3), list("Whole", 2), list("Games", 4)],
        );
        let got: Vec<(&str, Option<&str>, usize, usize)> = b
            .iter()
            .map(|x| (x.name.as_str(), x.yours.as_deref(), x.have, x.source_songs))
            .collect();
        assert_eq!(got, [("Hindi", Some("Hindi"), 1, 3), ("Games", None, 0, 4)]);
    }
}
