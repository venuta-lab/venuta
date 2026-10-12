//! Library health: read-only checks over the index. Every finding names the albums it
//! affects so the UI can show them, and explains in plain words why it matters.

use crate::library::{AlbumSummary, Library, Track, summarize};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Worth knowing; nothing is wrong yet.
    Note,
    /// Makes the library inconsistent or harder to use on some devices.
    Fix,
    /// Risk of data loss, duplicates or misleading quality.
    Problem,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub id: &'static str,
    pub title: String,
    /// Why it matters, in one or two plain sentences.
    pub why: String,
    pub severity: Severity,
    /// Tracks affected.
    pub tracks: usize,
    /// Albums affected (folder paths), most affected first.
    pub albums: Vec<String>,
    /// Short concrete examples, e.g. the spellings found.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatCount {
    pub bit_depth: u8,
    pub sample_rate: u32,
    pub tracks: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub tracks: usize,
    pub albums: usize,
    pub hours: f64,
    pub bytes: u64,
    pub formats: Vec<FormatCount>,
    /// Tracks with no Fix/Problem issue.
    pub healthy_tracks: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub summary: Summary,
    pub issues: Vec<Issue>,
    pub albums: Vec<AlbumSummary>,
}

/// Tag keys left behind by converters and downloaders; never meaningful to a listener.
const JUNK_KEYS: &[&str] = &[
    "MAJOR_BRAND",
    "MINOR_VERSION",
    "COMPATIBLE_BRANDS",
    "LENGTH",
    "TIDAL_DATA",
    "ENCODER",
];
/// Watermarks stamped into tags by download sites.
const WATERMARKS: &[&str] = &["t.me/", "telegram", "samperals", "desihires", "raag.fm"];
const LONG_PATH: usize = 180;
const SMALL_ART: u32 = 1000;

struct Builder<'a> {
    id: &'static str,
    title: String,
    why: &'static str,
    severity: Severity,
    hits: Vec<&'a Track>,
    examples: Vec<String>,
}

impl<'a> Builder<'a> {
    fn new(
        id: &'static str,
        title: impl Into<String>,
        why: &'static str,
        severity: Severity,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            why,
            severity,
            hits: Vec::new(),
            examples: Vec::new(),
        }
    }

    fn finish(self) -> Option<Issue> {
        if self.hits.is_empty() {
            return None;
        }
        let mut per_album: BTreeMap<&str, usize> = BTreeMap::new();
        for t in &self.hits {
            *per_album.entry(t.dir.as_str()).or_default() += 1;
        }
        let mut albums: Vec<(&str, usize)> = per_album.into_iter().collect();
        albums.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        Some(Issue {
            id: self.id,
            title: self.title,
            why: self.why.to_owned(),
            severity: self.severity,
            tracks: self.hits.len(),
            albums: albums.into_iter().map(|(d, _)| d.to_owned()).collect(),
            examples: self.examples,
        })
    }
}

pub fn run(lib: &Library) -> HealthReport {
    let flac: Vec<&Track> = lib.tracks.iter().filter(|t| t.kind == "flac").collect();
    let albums = lib.albums();
    let mut issues: Vec<Issue> = Vec::new();
    let mut flagged: HashSet<i64> = HashSet::new();

    // --- Credits and tags --------------------------------------------------------------
    let mut b = Builder::new(
        "missing_composer",
        "Composer is missing",
        "Players and DAPs browse by tags, so the composer you organise by is invisible without it.",
        Severity::Fix,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| !t.has("COMPOSER"))
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "missing_genre",
        "Genre is missing",
        "Without a genre, smart playlists and genre browsing on devices leave these tracks out.",
        Severity::Fix,
    );
    b.hits = flac.iter().copied().filter(|t| !t.has("GENRE")).collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "missing_album_artist",
        "Album artist is missing",
        "Players then group each track under its singers and the album falls apart.",
        Severity::Fix,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| !t.has("ALBUMARTIST"))
        .collect();
    add(&mut issues, &mut flagged, b);

    // Same person spelled differently: compare names with punctuation and spacing removed.
    let mut spellings: HashMap<String, BTreeMap<String, usize>> = HashMap::new();
    for t in &flac {
        if let Some(aa) = t.first("ALBUMARTIST") {
            let first_name = aa.split(',').next().unwrap_or(aa);
            *spellings
                .entry(name_key(first_name))
                .or_default()
                .entry(first_name.to_owned())
                .or_default() += 1;
        }
    }
    let mut variant_names: HashSet<String> = HashSet::new();
    let mut b = Builder::new(
        "name_variants",
        "The same name is spelled several ways",
        "Each spelling shows up as a separate artist, splitting their albums apart in every player.",
        Severity::Fix,
    );
    let mut groups: Vec<&BTreeMap<String, usize>> =
        spellings.values().filter(|v| v.len() > 1).collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.values().sum::<usize>()));
    for g in groups {
        let dominant = g
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(s, _)| s.clone())
            .unwrap_or_default();
        b.examples.push(
            g.keys()
                .map(|s| format!("“{s}”"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        variant_names.extend(g.keys().filter(|s| **s != dominant).cloned());
    }
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| {
            t.first("ALBUMARTIST")
                .map(|aa| variant_names.contains(aa.split(',').next().unwrap_or(aa)))
                .unwrap_or(false)
        })
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "crowded_album_artist",
        "Album artist lists several people",
        "Lyricists and singers mixed into the album artist create a new “artist” for every combination.",
        Severity::Fix,
    );
    let mut crowded: BTreeSet<String> = BTreeSet::new();
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| {
            t.first("ALBUMARTIST").is_some_and(|aa| {
                let many = aa.contains(", ") || aa.matches(" & ").count() > 0;
                if many {
                    crowded.insert(aa.to_owned());
                }
                many
            })
        })
        .collect();
    b.examples = crowded
        .into_iter()
        .take(5)
        .map(|s| format!("“{s}”"))
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "mixed_album_tags",
        "One folder holds tracks from different albums",
        "Players split the folder into several albums, or merge tracks that don't belong together.",
        Severity::Fix,
    );
    for tracks in albums.values() {
        let titles: HashSet<_> = tracks.iter().filter_map(|t| t.first("ALBUM")).collect();
        if titles.len() > 1 {
            b.hits
                .extend(tracks.iter().copied().filter(|t| t.kind == "flac"));
        }
    }
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "rerelease_dates",
        "Dates are re-release dates",
        "The year shown is when a streaming service re-published the album, not when the film or album came out.",
        Severity::Fix,
    );
    for (dir, tracks) in &albums {
        let folder = dir.rsplit('/').next().unwrap_or(dir);
        let folder_year: String = folder.chars().take(4).collect();
        if folder_year.len() == 4 && folder_year.chars().all(|c| c.is_ascii_digit()) {
            let tag_year = tracks
                .iter()
                .find_map(|t| t.first("DATE"))
                .map(|d| d.chars().take(4).collect::<String>());
            if tag_year.is_some_and(|y| y != folder_year) {
                b.hits.extend(tracks.iter().copied());
            }
        }
    }
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "junk_tags",
        "Leftover junk and watermarks in tags",
        "Converter leftovers and download-site watermarks clutter every tag editor and player info screen.",
        Severity::Fix,
    );
    let mut seen_junk: BTreeSet<String> = BTreeSet::new();
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| {
            let mut junk = false;
            for k in &t.raw_keys {
                let up = k.to_ascii_uppercase();
                if JUNK_KEYS.contains(&up.as_str()) {
                    seen_junk.insert(k.clone());
                    junk = true;
                }
            }
            for (k, vals) in &t.tags {
                if k == "LYRICS" || k == "UNSYNCEDLYRICS" || k == "URL" {
                    continue;
                }
                for v in vals {
                    let lv = v.to_lowercase();
                    if let Some(w) = WATERMARKS.iter().find(|w| lv.contains(**w)) {
                        seen_junk.insert(format!("{w} in {}", k.to_lowercase()));
                        junk = true;
                    }
                }
            }
            junk
        })
        .collect();
    b.examples = seen_junk.into_iter().take(8).collect();
    add(&mut issues, &mut flagged, b);

    // --- Artwork -------------------------------------------------------------------------
    let mut b = Builder::new(
        "art_missing",
        "No cover art in the file",
        "Players and DAPs show a blank square for these tracks.",
        Severity::Problem,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| t.front_cover().is_none())
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "art_progressive",
        "Cover art some players can't show",
        "The art is saved as a progressive JPEG. Rockbox and many budget players show a blank cover instead.",
        Severity::Fix,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| t.front_cover().is_some_and(|p| p.progressive))
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "art_small",
        "Low-resolution cover art",
        "Covers under 1000 pixels look soft on large screens and DAP displays.",
        Severity::Note,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| {
            t.front_cover().is_some_and(|p| match (p.width, p.height) {
                (Some(w), Some(h)) => w.min(h) < SMALL_ART,
                _ => false,
            })
        })
        .collect();
    add(&mut issues, &mut flagged, b);

    // --- Integrity and duplicates ----------------------------------------------------------
    let mut b = Builder::new(
        "no_checksum",
        "No audio checksum",
        "These files were saved without the checksum FLAC uses to prove the audio is intact, so damage can't be detected.",
        Severity::Note,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| t.audio_md5.is_none())
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut by_md5: HashMap<&[u8], Vec<&Track>> = HashMap::new();
    for t in &flac {
        if let Some(m) = &t.audio_md5 {
            by_md5.entry(m.as_slice()).or_default().push(t);
        }
    }
    let mut b = Builder::new(
        "duplicate_audio",
        "Exact duplicate recordings",
        "The same audio is stored more than once, so it plays twice in shuffle and wastes space on every card.",
        Severity::Problem,
    );
    let mut dup_ids: HashSet<i64> = HashSet::new();
    for group in by_md5.values().filter(|g| g.len() > 1) {
        b.hits.extend(group.iter().copied());
        dup_ids.extend(group.iter().map(|t| t.id));
    }
    add(&mut issues, &mut flagged, b);

    let mut by_isrc: HashMap<&str, Vec<&Track>> = HashMap::new();
    for t in &flac {
        if let Some(i) = t.first("ISRC").filter(|i| !i.trim().is_empty()) {
            by_isrc.entry(i).or_default().push(t);
        }
    }
    let mut b = Builder::new(
        "duplicate_recording",
        "Same recording in more than one album",
        "A compilation or a second copy repeats songs you already have from the original album.",
        Severity::Note,
    );
    for group in by_isrc.values() {
        let dirs: HashSet<_> = group.iter().map(|t| t.dir.as_str()).collect();
        if dirs.len() > 1 && !group.iter().all(|t| dup_ids.contains(&t.id)) {
            b.hits.extend(group.iter().copied());
        }
    }
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "lossy_files",
        "Lossy files in a lossless library",
        "MP3 and AAC files can't be restored to full quality; they belong on the upgrade list.",
        Severity::Problem,
    );
    b.hits = lib
        .tracks
        .iter()
        .filter(|t| t.kind == "other_audio")
        .collect();
    add(&mut issues, &mut flagged, b);

    // --- Listening comfort ------------------------------------------------------------------
    let mut b = Builder::new(
        "no_replaygain",
        "No volume levelling",
        "Without ReplayGain, loud modern masters and quiet older albums jump in volume between tracks.",
        Severity::Fix,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| !t.has("REPLAYGAIN_TRACK_GAIN"))
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "no_lyrics",
        "No lyrics",
        "No lyrics in the file and no .lrc file next to it.",
        Severity::Note,
    );
    b.hits = flac
        .iter()
        .copied()
        .filter(|t| {
            let sidecar = format!(
                "{}.lrc",
                t.path.rsplit_once('.').map(|x| x.0).unwrap_or(&t.path)
            );
            !t.has("LYRICS") && !t.has("UNSYNCEDLYRICS") && !lib.lyrics_files.contains(&sidecar)
        })
        .collect();
    add(&mut issues, &mut flagged, b);

    // --- Files and folders -----------------------------------------------------------------
    let mut b = Builder::new(
        "long_paths",
        "File paths are too long",
        "Paths over 180 characters break on some SD cards, Android players and older Windows tools.",
        Severity::Fix,
    );
    b.hits = lib
        .tracks
        .iter()
        .filter(|t| t.path.chars().count() > LONG_PATH)
        .collect();
    add(&mut issues, &mut flagged, b);

    let mut b = Builder::new(
        "incomplete_albums",
        "Albums with missing tracks",
        "The tags say the album has more tracks than the folder holds.",
        Severity::Note,
    );
    for tracks in albums.values() {
        let total = tracks
            .iter()
            .find_map(|t| t.first("TRACKTOTAL").or_else(|| t.first("TOTALTRACKS")))
            .and_then(|s| s.trim().parse::<usize>().ok());
        let discs: HashSet<_> = tracks
            .iter()
            .filter_map(|t| t.first("DISCNUMBER"))
            .collect();
        if let Some(total) = total
            && discs.len() <= 1
            && tracks.len() < total
        {
            b.hits.extend(tracks.iter().copied());
        }
    }
    add(&mut issues, &mut flagged, b);

    issues.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.tracks.cmp(&a.tracks)));

    let mut formats: BTreeMap<(u8, u32), usize> = BTreeMap::new();
    for t in &flac {
        *formats.entry((t.bit_depth, t.sample_rate)).or_default() += 1;
    }
    let mut formats: Vec<FormatCount> = formats
        .into_iter()
        .map(|((bit_depth, sample_rate), tracks)| FormatCount {
            bit_depth,
            sample_rate,
            tracks,
        })
        .collect();
    formats.sort_by_key(|f| std::cmp::Reverse(f.tracks));

    HealthReport {
        summary: Summary {
            tracks: lib.tracks.len(),
            albums: albums.len(),
            hours: lib.tracks.iter().map(|t| t.duration_ms).sum::<u64>() as f64 / 3_600_000.0,
            bytes: lib.tracks.iter().map(|t| t.size).sum(),
            formats,
            healthy_tracks: lib.tracks.len() - flagged.len(),
        },
        issues,
        albums: albums.iter().map(|(d, t)| summarize(d, t)).collect(),
    }
}

/// Record a finished check. Tracks hit by a Fix- or Problem-level check stop counting as healthy.
fn add(issues: &mut Vec<Issue>, flagged: &mut HashSet<i64>, b: Builder<'_>) {
    if b.severity >= Severity::Fix {
        flagged.extend(b.hits.iter().map(|t| t.id));
    }
    issues.extend(b.finish());
}

/// Name comparison key: lower-case letters and digits only, so "A. R.  Rahman" == "A.R. Rahman".
pub fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::name_key;

    #[test]
    fn name_key_ignores_punctuation_and_spacing() {
        assert_eq!(name_key("A.R. Rahman"), name_key("A.  R.  Rahman"));
        assert_eq!(name_key("Vishal-Shekhar"), name_key("Vishal–Shekhar"));
        assert_eq!(name_key("Ram Sampath\u{a0}"), name_key("Ram Sampath"));
        assert_ne!(name_key("Pritam"), name_key("Pritam Chakraborty"));
    }
}
