//! Playlists as plain `.m3u8` files in `<library>/Playlists/`, readable by every player.
//!
//! Entries use paths relative to the playlist (`../Film & Series/…`), so the same file works
//! on the PC and on a card (where the music sits under `Music/`). Each entry also carries the
//! track's audio checksum in a comment line players ignore (`#LIBRARIAN-MD5:`), so when files
//! move (e.g. the migration) a playlist repairs itself instead of breaking.
//!
//! Smart playlists are rules in `<rules>/playlists/*.toml`, written out as `.m3u8` too.

use crate::library::{Library, Track};
use crate::plan::split_names;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "Playlists";
const MD5_TAG: &str = "#LIBRARIAN-MD5:";
const SMART_TAG: &str = "#LIBRARIAN-SMART";

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistEntry {
    /// Library-relative path, or empty when the track is missing.
    pub path: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub audio_md5: Option<String>,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistSummary {
    pub name: String,
    pub file: String,
    pub smart: bool,
    pub tracks: usize,
    pub missing: usize,
}

pub fn folder(library_root: &Path) -> PathBuf {
    library_root.join(FOLDER)
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
        if let Some(m) = line.strip_prefix(MD5_TAG) {
            pending_md5 = Some(m.trim().to_uppercase());
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rel = line.trim_start_matches("../").replace('\\', "/");
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
                duration_ms: 0,
                audio_md5: md5,
                missing: true,
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
        duration_ms: t.duration_ms,
        audio_md5: t.audio_md5.as_deref().map(hex),
        missing: false,
    }
}

/// Write a static playlist from library-relative paths (missing tracks are kept as comments).
pub fn write(
    library_root: &Path,
    lib: &Library,
    name: &str,
    paths: &[String],
    smart: bool,
) -> Result<(), String> {
    let by_path: HashMap<&str, &Track> = lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
    let mut out = String::from("#EXTM3U\n");
    out.push_str(&format!("#PLAYLIST:{name}\n"));
    if smart {
        out.push_str(SMART_TAG);
        out.push_str(" written from your rules; edit the rule, not this file\n");
    }
    for p in paths {
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
        out.push_str(&format!("../{p}\n"));
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
            Some(PlaylistSummary {
                file: e.path().to_string_lossy().into_owned(),
                smart: text.contains(SMART_TAG),
                tracks: entries.len(),
                missing: entries.iter().filter(|x| x.missing).count(),
                name,
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
            let paths: Vec<String> = entries
                .into_iter()
                .filter(|e| !e.missing)
                .map(|e| e.path)
                .collect();
            if write(library_root, lib, &p.name, &paths, false).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// Deleted playlists are moved aside, never destroyed.
pub fn delete(library_root: &Path, name: &str) -> Result<(), String> {
    let src = file_for(library_root, name);
    let dir = library_root.join("_Quarantine").join("playlists");
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

pub struct Matcher<'a> {
    by_title: HashMap<String, Vec<&'a Track>>,
}

impl<'a> Matcher<'a> {
    pub fn new(lib: &'a Library) -> Self {
        let mut by_title: HashMap<String, Vec<&Track>> = HashMap::new();
        for t in lib.tracks.iter() {
            if let Some(title) = t.first("TITLE") {
                by_title.entry(title_key(title)).or_default().push(t);
            }
        }
        Self { by_title }
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
    for ((album, artist), (ids, lists)) in missing {
        let saved = ids.len();
        let genre = ids
            .iter()
            .filter_map(|i| apple.tracks.get(i))
            .map(|t| t.genre.to_lowercase())
            .next()
            .unwrap_or_default();
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
        report.wanted.push(WantedAlbum {
            album,
            artist,
            tier,
            why,
            saved_tracks: saved,
            playlists: lists.into_iter().collect(),
        });
    }
    report.wanted.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then(b.saved_tracks.cmp(&a.saved_tracks))
    });
    Ok(report)
}

/// The want-list as CSV, for shopping or ripping CDs.
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
        }
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
