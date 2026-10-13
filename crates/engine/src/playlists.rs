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

use crate::library::{Library, Track};
use crate::plan::split_names;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "Playlists";
const MD5_TAG: &str = "#VENUTA-MD5:";
const SMART_TAG: &str = "#VENUTA-SMART";
/// The same lines as written before the app was named; still read.
const OLD_MD5_TAG: &str = "#LIBRARIAN-MD5:";
const OLD_SMART_TAG: &str = "#LIBRARIAN-SMART";
const WANT_TAG: &str = "#VENUTA-WANT:";
const SOURCE_TAG: &str = "#VENUTA-SOURCE:";

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
}

/// One line of a playlist to write: a song in the library, or one still wanted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub want: Option<WantEntry>,
}

impl Item {
    pub fn track(path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            want: None,
        }
    }
    pub fn wanted(w: WantEntry) -> Self {
        Self {
            path: None,
            want: Some(w),
        }
    }
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
}

impl PlaylistEntry {
    /// Back to a line to write (a missing song without details keeps its title).
    pub fn item(&self) -> Item {
        if !self.missing {
            return Item::track(self.path.clone());
        }
        Item::wanted(self.want.clone().unwrap_or_else(|| WantEntry {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            isrc: None,
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistSummary {
    pub name: String,
    pub file: String,
    pub smart: bool,
    pub tracks: usize,
    pub missing: usize,
    /// Where it came from ("Spotify", "Apple Music"…), if imported.
    pub source: Option<String>,
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

/// What the index knows, for resolving entries: path → track, and audio MD5 → path.
struct Resolver<'a> {
    by_path: HashMap<&'a str, &'a Track>,
    by_md5: HashMap<String, &'a Track>,
}

impl<'a> Resolver<'a> {
    fn new(lib: &'a Library) -> Self {
        let mut by_md5 = HashMap::new();
        for t in &lib.tracks {
            if let Some(m) = &t.audio_md5 {
                by_md5.insert(hex(m), t);
            }
        }
        Self {
            by_path: lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect(),
            by_md5,
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// Read a playlist; entries whose file moved are found again by audio checksum.
/// Returns the entries and whether any path was repaired.
pub fn read(
    library_root: &Path,
    lib: &Library,
    name: &str,
) -> Result<(Vec<PlaylistEntry>, bool), String> {
    let text = std::fs::read_to_string(file_for(library_root, name)).map_err(|e| e.to_string())?;
    let r = Resolver::new(lib);
    let mut out = Vec::new();
    let mut pending_md5: Option<String> = None;
    let mut repaired = false;
    for line in text.lines().map(str::trim) {
        if let Some(m) = line
            .strip_prefix(MD5_TAG)
            .or_else(|| line.strip_prefix(OLD_MD5_TAG))
        {
            pending_md5 = Some(m.trim().to_uppercase());
            continue;
        }
        if let Some(j) = line.strip_prefix(WANT_TAG) {
            if let Ok(w) = serde_json::from_str::<WantEntry>(j.trim()) {
                out.push(PlaylistEntry {
                    path: String::new(),
                    title: w.title.clone(),
                    artist: w.artist.clone(),
                    album: w.album.clone(),
                    duration_ms: w.duration_ms,
                    audio_md5: None,
                    missing: true,
                    want: Some(w),
                });
            }
            pending_md5 = None;
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.replace('\\', "/");
        let rel = line
            .strip_prefix(to_music(library_root))
            .unwrap_or_else(|| line.trim_start_matches("../"))
            .to_owned();
        let md5 = pending_md5.take();
        let track = r.by_path.get(rel.as_str()).copied().or_else(|| {
            let t = md5.as_ref().and_then(|m| r.by_md5.get(m).copied());
            repaired |= t.is_some();
            t
        });
        out.push(match track {
            Some(t) => entry(t),
            None => PlaylistEntry {
                path: String::new(),
                title: rel.rsplit('/').next().unwrap_or(&rel).to_owned(),
                artist: String::new(),
                album: String::new(),
                duration_ms: 0,
                audio_md5: md5,
                missing: true,
                want: None,
            },
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
    std::fs::read_to_string(file_for(library_root, name))
        .ok()
        .and_then(|t| source_in(&t))
}

/// Is there a playlist by this name?
pub fn exists(library_root: &Path, name: &str) -> bool {
    file_for(library_root, name).is_file()
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

/// Write a playlist from songs in the library and songs still wanted, in order. A path that
/// isn't in the library is dropped; a wanted song keeps its place as a comment line.
pub fn write_items(
    library_root: &Path,
    lib: &Library,
    name: &str,
    items: &[Item],
    smart: bool,
    source: Option<&str>,
) -> Result<(), String> {
    let by_path: HashMap<&str, &Track> = lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
    let mut out = String::from("#EXTM3U\n");
    out.push_str(&format!("#PLAYLIST:{name}\n"));
    if smart {
        out.push_str(SMART_TAG);
        out.push_str(" written from your rules; edit the rule, not this file\n");
    }
    if let Some(s) = source.filter(|s| !s.is_empty()) {
        out.push_str(&format!("{SOURCE_TAG}{s}\n"));
    }
    for item in items {
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
    let part = path.with_extension("m3u8.part");
    std::fs::write(&part, out).map_err(|e| e.to_string())?;
    std::fs::rename(&part, &path).map_err(|e| e.to_string())
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
            let text = std::fs::read_to_string(e.path()).ok()?;
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
                missing: entries.iter().filter(|x| x.missing).count(),
                source: source_in(&text),
                duration_ms: entries
                    .iter()
                    .filter(|x| !x.missing)
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

/// Deleted playlists are moved aside, never destroyed.
pub fn delete(library_root: &Path, name: &str) -> Result<(), String> {
    let src = file_for(library_root, name);
    let dir = crate::paths::beside(library_root, crate::paths::Helper::SetAside).join("playlists");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    std::fs::rename(
        &src,
        dir.join(format!("{stamp}-{}.m3u8", crate::plan::sanitize(name, 100))),
    )
    .map_err(|e| e.to_string())
}

pub fn rename(library_root: &Path, from: &str, to: &str) -> Result<(), String> {
    let dst = file_for(library_root, to);
    if dst.exists() {
        return Err("A playlist with that name already exists.".into());
    }
    let src = file_for(library_root, from);
    let text = std::fs::read_to_string(&src).map_err(|e| e.to_string())?;
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
    crate::audit::name_key(cut)
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
        }
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
    by_loose: HashMap<String, Vec<&'a Track>>,
    by_isrc: HashMap<String, &'a Track>,
}

impl<'a> Matcher<'a> {
    pub fn new(lib: &'a Library) -> Self {
        let mut by_title: HashMap<String, Vec<&Track>> = HashMap::new();
        let mut by_loose: HashMap<String, Vec<&Track>> = HashMap::new();
        let mut by_isrc: HashMap<String, &Track> = HashMap::new();
        for t in lib.tracks.iter() {
            if let Some(title) = t.first("TITLE") {
                by_title.entry(title_key(title)).or_default().push(t);
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
            by_loose,
            by_isrc,
        }
    }

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
            .any(|a| artists.contains(&crate::audit::name_key(a)))
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

    /// Find a song from elsewhere in the library, and say how sure the match is.
    pub fn grade(&self, f: &Foreign) -> Option<Found<'a>> {
        let near = |t: &Track| {
            f.duration_ms
                .is_none_or(|ms| t.duration_ms.abs_diff(ms) <= 3000)
        };
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
        let artists: HashSet<String> = split_names(&f.artist)
            .iter()
            .map(|a| crate::audit::name_key(a))
            .filter(|a| !a.is_empty())
            .collect();
        // 2. The same title by the same artist: the album or the length settles twins.
        if let Some(cands) = self.by_title.get(&title_key(&f.title)) {
            let mine: Vec<&Track> = cands
                .iter()
                .copied()
                .filter(|t| Self::shares_artist(t, &artists))
                .collect();
            if !mine.is_empty() {
                let album = title_key(&f.album);
                if let Some(t) = mine.iter().find(|t| {
                    !album.is_empty() && t.first("ALBUM").is_some_and(|a| title_key(a) == album)
                }) {
                    return Some(Found {
                        track: t,
                        grade: Grade::Exact,
                        why: "Same song, artist and album".into(),
                    });
                }
                let close: Vec<&Track> = mine.iter().copied().filter(|t| near(t)).collect();
                if close.len() == 1 || (mine.len() == 1 && close.len() <= 1) {
                    let t = close.first().copied().unwrap_or(mine[0]);
                    let exact = near(t);
                    return Some(Found {
                        track: t,
                        grade: if exact { Grade::Exact } else { Grade::Close },
                        why: if exact {
                            "Same song and artist".into()
                        } else {
                            "Same song and artist, a different length".into()
                        },
                    });
                }
                let t = Self::nearest(&mine, f.duration_ms);
                return Some(Found {
                    track: t,
                    grade: Grade::Close,
                    why: format!(
                        "You have {} versions; this is the nearest in length",
                        mine.len()
                    ),
                });
            }
            // The title alone: only one song, and the same length, is worth offering.
            if cands.len() == 1 && f.duration_ms.is_some() && near(cands[0]) {
                return Some(Found {
                    track: cands[0],
                    grade: Grade::Close,
                    why: "Same song and length, credited to someone else".into(),
                });
            }
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

#[derive(Debug, Clone, Serialize)]
pub struct ImportedPlaylist {
    pub name: String,
    pub total: usize,
    pub matched: usize,
}

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

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub playlists: Vec<ImportedPlaylist>,
    pub wanted: Vec<WantedAlbum>,
    pub tracks_total: usize,
    pub tracks_matched: usize,
}

pub fn import_apple(
    library_root: &Path,
    lib: &Library,
    apple: &crate::apple::AppleLibrary,
    prefix: &str,
    write_files: bool,
) -> Result<ImportReport, String> {
    let matcher = Matcher::new(lib);
    let mut report = ImportReport {
        playlists: Vec::new(),
        wanted: Vec::new(),
        tracks_total: 0,
        tracks_matched: 0,
    };
    // Unmatched tracks grouped by album, for the want-list.
    let mut missing: BTreeMap<(String, String), (HashSet<u64>, HashSet<String>)> = BTreeMap::new();
    let mut seen: HashSet<u64> = HashSet::new();
    for p in &apple.playlists {
        let mut paths = Vec::new();
        for id in &p.tracks {
            let Some(t) = apple.tracks.get(id) else {
                continue;
            };
            let first_time = seen.insert(*id);
            match matcher.find(&t.title, &t.artist, &t.album) {
                Some(local) => {
                    paths.push(local.path.clone());
                    if first_time {
                        report.tracks_matched += 1;
                    }
                }
                None => {
                    let artist = if t.album_artist.is_empty() {
                        &t.artist
                    } else {
                        &t.album_artist
                    };
                    let e = missing
                        .entry((t.album.clone(), artist.clone()))
                        .or_default();
                    e.0.insert(*id);
                    e.1.insert(p.name.clone());
                }
            }
            if first_time {
                report.tracks_total += 1;
            }
        }
        let name = format!("{prefix}{}", p.name);
        if write_files {
            write(library_root, lib, &name, &paths, false)?;
        }
        report.playlists.push(ImportedPlaylist {
            name,
            total: p.tracks.len(),
            matched: paths.len(),
        });
    }
    let groups: Vec<WantGroup> = missing
        .into_iter()
        .map(|((album, artist), (ids, lists))| WantGroup {
            genre: ids
                .iter()
                .filter_map(|i| apple.tracks.get(i))
                .map(|t| t.genre.clone())
                .next()
                .unwrap_or_default(),
            saved: ids.len(),
            playlists: lists.into_iter().collect(),
            album,
            artist,
        })
        .collect();
    report.wanted = rank_wanted(lib, groups);
    Ok(report)
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
    fn wanted_songs_keep_their_place_and_source() {
        let root = tempfile::tempdir().unwrap();
        let lib = library(vec![song(1, "A/01.flac", "One", "X", "Al", 200_000)]);
        let want = WantEntry {
            title: "Missing".into(),
            artist: "Y".into(),
            album: "Gone".into(),
            duration_ms: 180_000,
            isrc: None,
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
}
