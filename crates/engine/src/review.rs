//! Decisions: the few things only the owner can settle, each with the evidence to settle it
//! quickly and a suggested answer.
//!
//! The planner decides everything it can prove (identical audio is the same song, a canonical
//! spelling from the aliases, a year from MusicBrainz). What it can't prove becomes a
//! [`Decision`]: two artists that might be one, an album that exists twice, two candidate
//! years, files with no tags. Albums waiting on an unanswered decision are held back from a
//! build; everything else builds.

use crate::library::{Library, Track};
use crate::plan::{AlbumAction, AlbumPlan, Plan};
use crate::rules::Rules;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Answers to decisions, by decision id (kept per library in `answers.json`).
pub type Answers = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    DuplicateSongs,
    SameArtist,
    SameAlbum,
    Year,
    Untagged,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub id: String,
    pub kind: DecisionKind,
    /// Handled automatically because the evidence proves it; already applied, shown so the
    /// owner can approve it or change it.
    pub auto: bool,
    /// The exact proof behind an automatic decision.
    pub proof: String,
    /// The owner's answer, when there is one.
    pub answer: Option<String>,
    /// The question, short: "Same artist?"
    pub question: String,
    /// What it is about: "The Local Train and The Local Music Train".
    pub subject: String,
    /// Evidence, one panel per candidate.
    pub sides: Vec<Side>,
    pub choices: Vec<Choice>,
    /// The choice the app would make, and why.
    pub suggested: Option<String>,
    pub why: String,
    /// Albums held back until this is answered.
    pub albums: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Side {
    pub title: String,
    pub subtitle: String,
    /// Cover keys (see `AlbumPlan::cover`).
    pub covers: Vec<String>,
    pub facts: Vec<Fact>,
    /// Songs to listen to, by library-relative path.
    pub songs: Vec<Song>,
    /// The choice that picks this side, if one does.
    pub choice: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Song {
    pub title: String,
    pub path: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub hint: String,
}

fn fact(label: &str, value: impl Into<String>) -> Fact {
    Fact {
        label: label.into(),
        value: value.into(),
    }
}

fn choice(id: impl Into<String>, label: impl Into<String>, hint: impl Into<String>) -> Choice {
    Choice {
        id: id.into(),
        label: label.into(),
        hint: hint.into(),
    }
}

fn song(t: &Track) -> Song {
    Song {
        title: t
            .first("TITLE")
            .map(str::to_owned)
            .unwrap_or_else(|| stem(&t.path).to_owned()),
        path: t.path.clone(),
        duration_ms: t.duration_ms,
    }
}

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|x| x.0).unwrap_or(name)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// --- Files with no tags ------------------------------------------------------------------

/// A track with no title, artist or album: usually a video-site download.
pub fn is_untagged(t: &Track) -> bool {
    ["TITLE", "ARTIST", "ALBUM", "ALBUMARTIST"]
        .iter()
        .all(|k| t.first(k).is_none_or(|v| v.trim().is_empty()))
}

/// Artist and title from a file name like "01 - Artist - Title - Artist.mp3".
pub fn names_from_file(path: &str) -> (Option<String>, String) {
    let s = stem(path).trim();
    let s = s
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['.', ' ', '-', '_'])
        .trim();
    match s.split_once(" - ") {
        Some((artist, title)) => {
            let artist = artist.trim().to_owned();
            // Video titles often repeat the channel or artist at the end.
            let mut title = title.trim().to_owned();
            if let Some(rest) = title.strip_suffix(&format!(" - {artist}")) {
                title = rest.trim().to_owned();
            }
            (Some(artist), title)
        }
        None => (None, s.to_owned()),
    }
}

/// A copy of an untagged track with artist, title and album (the title, as a single) taken
/// from its file name.
pub fn tag_from_file_name(t: &Track, identity: Option<&crate::providers::Identified>) -> Track {
    let mut c = t.clone();
    let (artist, title) = match identity {
        Some(i) => (Some(i.artist.clone()), i.title.clone()),
        None => names_from_file(&t.path),
    };
    let mut set = |k: &str, v: &str| {
        c.tags.insert(k.into(), vec![v.into()]);
        c.raw_keys.push(k.into());
    };
    set("TITLE", &title);
    set("ALBUM", &title);
    if let Some(a) = &artist {
        set("ARTIST", a);
        set("ALBUMARTIST", a);
    }
    c
}

pub fn untagged(lib: &Library, answers: &Answers, plan: &Plan) -> Option<Decision> {
    let files: Vec<&Track> = lib.tracks.iter().filter(|t| is_untagged(t)).collect();
    if files.is_empty() || answers.contains_key(UNTAGGED) {
        return None;
    }
    let ids: HashSet<i64> = files.iter().map(|t| t.id).collect();
    let albums: Vec<String> = plan
        .albums
        .iter()
        .filter(|a| a.tracks.iter().any(|t| ids.contains(&t.file_id)))
        .map(|a| a.id.clone())
        .collect();
    let lossy = files.iter().filter(|t| t.kind != "flac").count();
    let recognised = files
        .iter()
        .filter(|t| lib.identities.contains_key(&t.id))
        .count();
    let named: Vec<String> = files
        .iter()
        .take(2)
        .map(|t| match lib.identities.get(&t.id) {
            Some(i) => format!("{} – {}", i.artist, i.title),
            None => match names_from_file(&t.path) {
                (Some(a), title) => format!("{a} – {title}"),
                (None, title) => title,
            },
        })
        .collect();
    Some(Decision {
        id: UNTAGGED.into(),
        auto: false,
        proof: String::new(),
        answer: None,
        kind: DecisionKind::Untagged,
        question: "Keep these downloads?".into(),
        subject: format!(
            "{} with no tags{}",
            plural(files.len(), "file", "files"),
            if lossy == files.len() {
                ", all MP3"
            } else {
                ""
            }
        ),
        sides: vec![Side {
            title: plural(files.len(), "file", "files"),
            subtitle: "No title, artist or album inside the file. Most look like video-site downloads."
                .into(),
            covers: vec![],
            facts: vec![
                fact(
                    "Format",
                    if lossy > 0 {
                        format!("{lossy} lossy (MP3), {} lossless", files.len() - lossy)
                    } else {
                        "lossless".into()
                    },
                ),
                fact(
                    "Recognised by sound",
                    if recognised > 0 {
                        format!("{recognised} of {} (AcoustID)", files.len())
                    } else {
                        "none yet (add an AcoustID key in Settings)".into()
                    },
                ),
                fact(
                    "Folders",
                    files
                        .iter()
                        .map(|t| t.dir.rsplit('/').next().unwrap_or(&t.dir).replace('_', " "))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            ],
            songs: files
                .iter()
                .map(|t| {
                    let (artist, title) = match lib.identities.get(&t.id) {
                        Some(i) => (Some(i.artist.clone()), i.title.clone()),
                        None => names_from_file(&t.path),
                    };
                    Song {
                        title: match artist {
                            Some(a) => format!("{a} – {title}"),
                            None => title,
                        },
                        path: t.path.clone(),
                        duration_ms: t.duration_ms,
                    }
                })
                .collect(),
            choice: None,
        }],
        choices: vec![
            choice(
                "keep",
                "Keep them as singles",
                format!("Named from the file names, e.g. {}.", named.join("; ")),
            ),
            choice(
                "leave_out",
                "Leave them out",
                "They stay in your current library, untouched.",
            ),
        ],
        suggested: Some("keep".into()),
        why: "They are low-quality copies, but music you chose to keep. The want-list is the place to find proper releases.".into(),
        albums,
    })
}

pub const UNTAGGED: &str = "untagged";

// --- The same album twice ----------------------------------------------------------------

/// Albums from different folders that would land in the same place are the same release
/// held twice (e.g. one folder per spelling of the composer). Keep one, or both.
pub fn same_album(plan: &mut Plan, lib: &Library, answers: &Answers) -> Vec<Decision> {
    let mut by_target: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, a) in plan.albums.iter().enumerate() {
        if matches!(a.action, AlbumAction::Migrate) && !a.tracks.is_empty() {
            by_target
                .entry(a.target_dir.to_lowercase())
                .or_default()
                .push(i);
        }
    }
    let by_id: HashMap<i64, &Track> = lib.tracks.iter().map(|t| (t.id, t)).collect();
    let mut out = Vec::new();
    for (key, idx) in by_target {
        let sources: BTreeSet<&str> = idx
            .iter()
            .map(|&i| plan.albums[i].source_dir.as_str())
            .collect();
        if idx.len() < 2 || sources.len() < 2 {
            continue;
        }
        let id = format!("album:{key}");
        let copies: Vec<(usize, Copy)> = idx
            .iter()
            .map(|&i| (i, Copy::of(&plan.albums[i], &by_id)))
            .collect();
        match answers.get(&id).map(String::as_str) {
            Some("both") => {
                for (n, (i, _)) in copies.iter().enumerate().skip(1) {
                    let a = &mut plan.albums[*i];
                    let old = a.target_dir.clone();
                    a.target_dir = format!("{old} ({})", n + 1);
                    for t in &mut a.tracks {
                        if let Some(rest) = t.target.strip_prefix(&old) {
                            t.target = format!("{}{rest}", a.target_dir);
                        }
                    }
                }
            }
            Some(keep) if keep.starts_with("keep:") => {
                let kept = &keep["keep:".len()..];
                for (i, _) in &copies {
                    if plan.albums[*i].id != kept {
                        plan.albums[*i].action = AlbumAction::Quarantine {
                            duplicate_of: kept.to_owned(),
                        };
                    }
                }
            }
            _ => {
                let best = copies
                    .iter()
                    .max_by_key(|(_, c)| c.score())
                    .map(|(i, _)| plan.albums[*i].id.clone());
                let first = &plan.albums[copies[0].0];
                let letters = ["A", "B", "C", "D", "E"];
                let sides: Vec<Side> = copies
                    .iter()
                    .enumerate()
                    .map(|(n, (i, c))| {
                        c.side(&plan.albums[*i], letters.get(n).copied().unwrap_or("?"))
                    })
                    .collect();
                let mut choices: Vec<Choice> = copies
                    .iter()
                    .enumerate()
                    .map(|(n, (i, _))| {
                        choice(
                            format!("keep:{}", plan.albums[*i].id),
                            format!("Keep copy {}", letters.get(n).copied().unwrap_or("?")),
                            "The other copy is set aside, not deleted.",
                        )
                    })
                    .collect();
                choices.push(choice(
                    "both",
                    "Keep both",
                    "As two albums, the second numbered (2). For different editions.",
                ));
                let why = best
                    .as_ref()
                    .and_then(|b| copies.iter().find(|(i, _)| &plan.albums[*i].id == b))
                    .map(|(_, c)| c.why_best(&copies.iter().map(|(_, c)| c).collect::<Vec<_>>()))
                    .unwrap_or_default();
                out.push(Decision {
                    id,
                    auto: false,
                    proof: String::new(),
                    answer: None,
                    kind: DecisionKind::SameAlbum,
                    question: "Same album twice?".into(),
                    subject: format!("{} by {}", first.title, first.album_artist),
                    sides,
                    choices,
                    suggested: best.map(|b| format!("keep:{b}")),
                    why,
                    albums: copies
                        .iter()
                        .map(|(i, _)| plan.albums[*i].id.clone())
                        .collect(),
                });
            }
        }
    }
    out
}

/// What makes one copy of an album better than another.
struct Copy {
    tracks: usize,
    lyrics: usize,
    barcode: bool,
    cover_px: u32,
    format: String,
    verified: usize,
    bytes: u64,
    songs: Vec<Song>,
}

impl Copy {
    fn of(a: &AlbumPlan, by_id: &HashMap<i64, &Track>) -> Self {
        let ts: Vec<&Track> = a
            .tracks
            .iter()
            .filter_map(|t| by_id.get(&t.file_id).copied())
            .collect();
        let lyrics = a
            .tracks
            .iter()
            .filter(|t| {
                t.write_lrc
                    || t.copy_lrc_from.is_some()
                    || t.embed_lyrics_from.is_some()
                    || t.tags_after
                        .iter()
                        .any(|(k, _)| k.eq_ignore_ascii_case("LYRICS"))
            })
            .count();
        let formats: BTreeSet<String> = ts
            .iter()
            .map(|t| {
                if t.kind == "flac" {
                    format!(
                        "FLAC {}-bit / {:.1} kHz",
                        t.bit_depth,
                        f64::from(t.sample_rate) / 1000.0
                    )
                } else {
                    t.path.rsplit('.').next().unwrap_or("").to_uppercase()
                }
            })
            .collect();
        Copy {
            tracks: ts.len(),
            lyrics,
            barcode: ts
                .iter()
                .any(|t| t.first("BARCODE").or(t.first("UPC")).is_some())
                || a.tracks
                    .iter()
                    .any(|t| t.tags_after.iter().any(|(k, _)| k == "BARCODE")),
            cover_px: ts
                .iter()
                .filter_map(|t| t.front_cover())
                .filter_map(|p| Some(p.width?.min(p.height?)))
                .max()
                .unwrap_or(0),
            format: formats.into_iter().collect::<Vec<_>>().join(", "),
            verified: ts
                .iter()
                .filter(|t| t.quality.as_ref().and_then(|q| q.verify_ok) == Some(true))
                .count(),
            bytes: ts.iter().map(|t| t.size).sum(),
            songs: ts.iter().take(3).map(|t| song(t)).collect(),
        }
    }

    fn score(&self) -> (usize, usize, bool, u32) {
        (self.tracks, self.lyrics, self.barcode, self.cover_px)
    }

    fn why_best(&self, all: &[&Copy]) -> String {
        let mut good = Vec::new();
        if all.iter().all(|c| c.tracks <= self.tracks) && all.iter().any(|c| c.tracks < self.tracks)
        {
            good.push("more songs".to_owned());
        }
        if all.iter().any(|c| c.lyrics < self.lyrics) {
            good.push("lyrics".to_owned());
        }
        if self.barcode && all.iter().any(|c| !c.barcode) {
            good.push("its barcode".to_owned());
        }
        if all.iter().any(|c| c.cover_px < self.cover_px) {
            good.push("a sharper cover".to_owned());
        }
        if good.is_empty() {
            "Both copies are equally complete; either is fine.".into()
        } else {
            format!(
                "It has {}. The other copy is set aside, not deleted.",
                good.join(", ")
            )
        }
    }

    fn side(&self, a: &AlbumPlan, letter: &str) -> Side {
        Side {
            title: format!("Copy {letter}"),
            subtitle: format!("In {}", a.source_dir.replace('_', " ")),
            covers: vec![a.cover.clone()],
            facts: vec![
                fact("Songs", self.tracks.to_string()),
                fact("Format", self.format.clone()),
                fact("Lyrics", format!("{} of {}", self.lyrics, self.tracks)),
                fact("Barcode", if self.barcode { "yes" } else { "no" }),
                fact(
                    "Cover",
                    if self.cover_px > 0 {
                        format!("{} px", self.cover_px)
                    } else {
                        "none".into()
                    },
                ),
                fact(
                    "Checked",
                    format!("{} of {} verified", self.verified, self.tracks),
                ),
                fact("Size", format!("{:.0} MB", self.bytes as f64 / 1_048_576.0)),
            ],
            songs: self.songs.clone(),
            choice: Some(format!("keep:{}", a.id)),
        }
    }
}

// --- Two candidate years -----------------------------------------------------------------

pub fn year_id(album_id: &str) -> String {
    format!("year:{album_id}")
}

pub fn years(plan: &Plan, lib: &Library, answers: &Answers) -> Vec<Decision> {
    let mut out = Vec::new();
    for a in &plan.albums {
        let Some((folder, tag)) = &a.year_conflict else {
            continue;
        };
        if !matches!(a.action, AlbumAction::Migrate) {
            continue;
        }
        let meta = lib.meta.get(&a.source_dir);
        let catalogue: Vec<(String, String)> = meta
            .map(|m| {
                [("Apple Music", &m.apple), ("Deezer", &m.deezer)]
                    .into_iter()
                    .filter_map(|(n, r)| {
                        let d = r.as_ref()?.release_date.clone()?;
                        Some((n.to_owned(), d))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
        sources
            .entry(folder.clone())
            .or_default()
            .push("your folder name".into());
        sources
            .entry(tag.clone())
            .or_default()
            .push("the files' date tag".into());
        for (name, date) in &catalogue {
            if let Some(y) = date.get(..4) {
                sources
                    .entry(y.to_owned())
                    .or_default()
                    .push(format!("{name}, {date} (may be a re-release)"));
            }
        }
        let id = year_id(&a.id);
        let (auto, proof, suggested, why) = match &a.year_auto {
            Some((y, proof)) => (
                true,
                proof.clone(),
                y.clone(),
                "The recording codes settle it.".to_owned(),
            ),
            None => (
                false,
                String::new(),
                sources.keys().next().cloned().unwrap_or_default(),
                "Re-releases come later, so the earliest year is usually the original.".to_owned(),
            ),
        };
        out.push(Decision {
            id: id.clone(),
            kind: DecisionKind::Year,
            auto,
            proof,
            answer: answers.get(&id).cloned(),
            question: "Which year?".into(),
            subject: format!("{} by {}", a.title, a.album_artist),
            sides: sources
                .iter()
                .map(|(y, from)| Side {
                    title: y.clone(),
                    subtitle: format!("From {}", from.join("; ")),
                    covers: vec![a.cover.clone()],
                    facts: vec![],
                    songs: vec![],
                    choice: Some(format!("y:{y}")),
                })
                .collect(),
            choices: sources
                .keys()
                .map(|y| {
                    choice(
                        format!("y:{y}"),
                        y.clone(),
                        "Used for the folder name and the original-year tag.",
                    )
                })
                .collect(),
            suggested: Some(format!("y:{suggested}")),
            why,
            albums: if auto { vec![] } else { vec![a.id.clone()] },
        });
    }
    out
}

// --- Two names, one artist? --------------------------------------------------------------

/// Titles of respect that don't make a different person ("Late Madan Mohan").
const HONORIFICS: &[&str] = &["late", "ustad", "pandit", "pt", "dr", "shri", "smt", "lata"];

/// "Is X the same artist as Y?" when one name's words are contained in the other's.
/// Names that share words are often different people (Sami Khan, Azaan Sami Khan), so the
/// recommendation is "different" unless something shows they are one.
pub fn same_artist(plan: &Plan, lib: &Library, rules: &Rules) -> Vec<Decision> {
    let mut by_artist: BTreeMap<&str, Vec<&AlbumPlan>> = BTreeMap::new();
    for a in &plan.albums {
        if matches!(a.action, AlbumAction::Migrate) && !a.album_artist.is_empty() {
            by_artist
                .entry(a.album_artist.as_str())
                .or_default()
                .push(a);
        }
    }
    let tokens = |n: &str| -> BTreeSet<String> {
        n.split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let by_id: HashMap<i64, &Track> = lib.tracks.iter().map(|t| (t.id, t)).collect();
    let names: Vec<&str> = by_artist.keys().copied().collect();
    let mut out = Vec::new();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            if rules.is_distinct(a, b) || *a == "Various Artists" || *b == "Various Artists" {
                continue;
            }
            let (ta, tb) = (tokens(a), tokens(b));
            if ta.is_empty() || tb.is_empty() || !(ta.is_subset(&tb) || tb.is_subset(&ta)) {
                continue;
            }
            let pa = Profile::of(&by_artist[a], &by_id, lib);
            let pb = Profile::of(&by_artist[b], &by_id, lib);
            // Signs of one artist: the longer name only adds a title of respect.
            let extra: BTreeSet<String> = ta.symmetric_difference(&tb).cloned().collect();
            let honorific =
                !extra.is_empty() && extra.iter().all(|t| HONORIFICS.contains(&t.as_str()));
            let keep = if pa.albums >= pb.albums { a } else { b };
            let differences = pa.differences(&pb);
            let (suggested, why) = if honorific {
                (
                    format!("same:{keep}"),
                    format!(
                        "The names differ only by a title of respect ({}).",
                        extra.into_iter().collect::<Vec<_>>().join(", ")
                    ),
                )
            } else if !differences.is_empty() {
                (
                    "different".to_owned(),
                    format!("Probably two artists: {}.", differences.join(", ")),
                )
            } else {
                (
                    "different".to_owned(),
                    "Nothing shows they are one artist, and names that share words are often different people. Listen to both before merging.".to_owned(),
                )
            };
            let mut albums: Vec<String> = by_artist[a]
                .iter()
                .chain(&by_artist[b])
                .map(|x| x.id.clone())
                .collect();
            albums.sort();
            out.push(Decision {
                id: format!(
                    "same:{}|{}",
                    crate::audit::name_key(a),
                    crate::audit::name_key(b)
                ),
                kind: DecisionKind::SameArtist,
                auto: false,
                proof: String::new(),
                answer: None,
                question: "Same artist?".into(),
                subject: format!("{a} and {b}"),
                sides: vec![pa.side(a), pb.side(b)],
                choices: vec![
                    choice(
                        format!("same:{a}"),
                        "Same artist",
                        format!("Call them “{a}”."),
                    ),
                    choice(
                        format!("same:{b}"),
                        "Same artist",
                        format!("Call them “{b}”."),
                    ),
                    choice("different", "Different artists", "Kept apart from now on."),
                ],
                suggested: Some(suggested),
                why,
                albums,
            });
        }
    }
    out
}

/// What one artist name's albums look like.
struct Profile {
    albums: usize,
    songs: usize,
    /// Release years: the catalogues' dates where known, else the planned original year.
    years: BTreeSet<u32>,
    languages: BTreeSet<String>,
    /// Each album's main genre (Apple first, then Deezer).
    genres: BTreeSet<String>,
    countries: BTreeSet<String>,
    covers: Vec<String>,
    titles: Vec<String>,
    samples: Vec<Song>,
}

impl Profile {
    fn of(albums: &[&AlbumPlan], by_id: &HashMap<i64, &Track>, lib: &Library) -> Self {
        let mut p = Profile {
            albums: albums.len(),
            songs: 0,
            years: BTreeSet::new(),
            languages: BTreeSet::new(),
            genres: BTreeSet::new(),
            countries: BTreeSet::new(),
            covers: vec![],
            titles: vec![],
            samples: vec![],
        };
        for a in albums {
            p.songs += a.tracks.len();
            p.covers.push(a.cover.clone());
            p.titles.push(a.title.clone());
            let meta = lib.meta.get(&a.source_dir);
            let catalogue = meta.and_then(|m| m.apple.as_ref().or(m.deezer.as_ref()));
            if let Some(g) = catalogue.and_then(|r| r.genres.first()) {
                p.genres.insert(g.to_lowercase());
            }
            let catalogue_year = catalogue
                .and_then(|r| r.release_date.as_deref())
                .and_then(|d| d.get(..4)?.parse::<u32>().ok());
            for (k, t) in a.tracks.iter().enumerate() {
                let after = |key: &str| {
                    t.tags_after
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(key))
                        .map(|(_, v)| v.clone())
                };
                if k == 0 {
                    if let Some(y) = catalogue_year
                        .or_else(|| after("ORIGINALYEAR").and_then(|y| y.get(..4)?.parse().ok()))
                    {
                        p.years.insert(y);
                    }
                    if p.samples.len() < 3
                        && let Some(src) = by_id.get(&t.file_id)
                    {
                        p.samples.push(song(src));
                    }
                }
                if let Some(l) = after("LANGUAGE") {
                    p.languages.insert(l);
                }
                if let Some(isrc) = after("ISRC").filter(|i| i.len() >= 2) {
                    p.countries.insert(country(&isrc[..2]));
                }
            }
        }
        p
    }

    /// Plain-language reasons these look like two different acts, strongest first.
    fn differences(&self, o: &Profile) -> Vec<String> {
        let mut d = Vec::new();
        let disjoint = |a: &BTreeSet<String>, b: &BTreeSet<String>| {
            !a.is_empty() && !b.is_empty() && a.is_disjoint(b)
        };
        if let (Some(a0), Some(a1), Some(b0), Some(b1)) = (
            self.years.first(),
            self.years.last(),
            o.years.first(),
            o.years.last(),
        ) && (a1 + 3 <= *b0 || b1 + 3 <= *a0)
        {
            let span = |x: u32, y: u32| {
                if x == y {
                    x.to_string()
                } else {
                    format!("{x}–{y}")
                }
            };
            d.push(format!(
                "they released music years apart ({} and {})",
                span(*a0, *a1),
                span(*b0, *b1)
            ));
        }
        if disjoint(&self.languages, &o.languages) {
            d.push("they sing in different languages".to_owned());
        }
        if disjoint(&self.genres, &o.genres) {
            d.push(format!(
                "different genres ({} and {})",
                self.genres.iter().cloned().collect::<Vec<_>>().join(", "),
                o.genres.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if disjoint(&self.countries, &o.countries) {
            d.push("their labels registered the recordings in different countries".to_owned());
        }
        d
    }

    fn side(&self, name: &str) -> Side {
        let span = match (self.years.first(), self.years.last()) {
            (Some(a), Some(b)) if a != b => format!("{a}–{b}"),
            (Some(a), _) => a.to_string(),
            _ => "year unknown".into(),
        };
        let list = |s: &BTreeSet<String>| {
            if s.is_empty() {
                "unknown".to_owned()
            } else {
                s.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        };
        Side {
            title: name.to_owned(),
            subtitle: format!(
                "{}, {}, {span}",
                plural(self.albums, "album", "albums"),
                plural(self.songs, "song", "songs")
            ),
            covers: self.covers.iter().take(4).cloned().collect(),
            facts: vec![
                fact(
                    "Albums",
                    self.titles
                        .iter()
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                fact(
                    "Language",
                    self.languages
                        .iter()
                        .map(|l| crate::rules::language_name(l).to_owned())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                fact("Genre", list(&self.genres)),
                fact("Recording codes from", list(&self.countries)),
            ]
            .into_iter()
            .filter(|f| !f.value.is_empty())
            .collect(),
            songs: self.samples.clone(),
            choice: Some(format!("same:{name}")),
        }
    }
}

/// The country of an ISRC registrant prefix. QM/QZ/QT are US codes handed to
/// self-distribution services.
fn country(code: &str) -> String {
    match code.to_ascii_uppercase().as_str() {
        "IN" => "India",
        "US" => "United States",
        "QM" | "QZ" | "QT" => "United States (self-released)",
        "GB" | "UK" => "United Kingdom",
        "PK" => "Pakistan",
        "AE" => "UAE",
        "CA" => "Canada",
        "AU" => "Australia",
        "DE" => "Germany",
        "FR" => "France",
        "JP" => "Japan",
        "KR" => "South Korea",
        "SE" => "Sweden",
        "NL" => "Netherlands",
        other => return other.to_owned(),
    }
    .to_owned()
}

/// Hold back every album that waits on an unanswered question. Automatic decisions are
/// already applied and hold nothing back.
pub fn hold(plan: &mut Plan, decisions: &[Decision]) {
    let mut waiting: HashMap<&str, Vec<String>> = HashMap::new();
    for d in decisions.iter().filter(|d| !d.auto && d.answer.is_none()) {
        for a in &d.albums {
            waiting.entry(a.as_str()).or_default().push(d.id.clone());
        }
    }
    for a in &mut plan.albums {
        a.waiting_on = waiting.remove(a.id.as_str()).unwrap_or_default();
    }
}

// --- The same song twice -----------------------------------------------------------------

/// How two copies of a song were proven the same.
#[derive(Clone, Copy)]
enum Proof {
    Checksum,
    Decoded(crate::compare::Comparison),
}

fn find_root(p: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while p[r] != r {
        r = p[r];
    }
    let mut i = i;
    while p[i] != r {
        let n = p[i];
        p[i] = r;
        i = n;
    }
    r
}

/// Identical audio, proven by the audio checksum or by decoding both files, is the same
/// song. The copy on the fuller, better album is kept; the others are set aside, and each
/// group of set-aside songs becomes an automatic decision that states its proof.
///
/// Likely duplicates that haven't been compared yet are listed in `plan.pending_pairs`.
pub fn duplicate_songs(plan: &mut Plan, lib: &Library, answers: &Answers) -> Vec<Decision> {
    let by_id: HashMap<i64, &Track> = lib.tracks.iter().map(|t| (t.id, t)).collect();
    // Every migrating track: (album index, track index, track).
    let mut items: Vec<(usize, usize, &Track)> = Vec::new();
    for (ai, a) in plan.albums.iter().enumerate() {
        if !matches!(a.action, AlbumAction::Migrate) {
            continue;
        }
        for (ti, t) in a.tracks.iter().enumerate() {
            if let Some(src) = by_id.get(&t.file_id) {
                items.push((ai, ti, src));
            }
        }
    }
    // Union-find over items, remembering how each item was linked.
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let mut proof_of: HashMap<usize, Proof> = HashMap::new();
    let mut link = |parent: &mut Vec<usize>, x: usize, y: usize, why: Proof| {
        let (rx, ry) = (find_root(parent, x), find_root(parent, y));
        if rx != ry {
            parent[rx] = ry;
        }
        proof_of.entry(x).or_insert(why);
        proof_of.entry(y).or_insert(why);
    };
    let mut by_md5: HashMap<&[u8], usize> = HashMap::new();
    let mut by_shape: HashMap<(u64, String), Vec<usize>> = HashMap::new();
    for (i, (_, _, t)) in items.iter().enumerate() {
        if let Some(m) = t.audio_md5.as_deref().filter(|m| m.iter().any(|b| *b != 0)) {
            match by_md5.get(m) {
                Some(&j) => link(&mut parent, i, j, Proof::Checksum),
                None => {
                    by_md5.insert(m, i);
                }
            }
        }
        if t.kind == "flac" && t.duration_ms > 0 {
            let title = t
                .first("TITLE")
                .map(crate::playlists::title_key)
                .unwrap_or_default();
            if !title.is_empty() {
                by_shape.entry((t.duration_ms, title)).or_default().push(i);
            }
        }
    }
    // Same length to the millisecond and the same title: compare the decoded sound.
    for group in by_shape.values().filter(|g| g.len() > 1) {
        for (n, &i) in group.iter().enumerate() {
            for &j in &group[n + 1..] {
                let (ti, tj) = (items[i].2, items[j].2);
                if ti.audio_md5.is_some() && ti.audio_md5 == tj.audio_md5 {
                    continue; // already linked by checksum
                }
                match lib.pairs.get(&crate::compare::key(ti.id, tj.id)) {
                    Some(c) if c.same() => link(&mut parent, i, j, Proof::Decoded(*c)),
                    Some(_) => {}
                    None => plan.pending_pairs.push((ti.id, tj.id)),
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..items.len() {
        let r = find_root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    // A compilation (MusicBrainz says so, or its songs turn up on three or more other albums)
    // never keeps a song that also has an original album.
    let mut partners: HashMap<usize, BTreeSet<usize>> = HashMap::new();
    for members in groups.values().filter(|g| g.len() > 1) {
        let albums: BTreeSet<usize> = members.iter().map(|&i| items[i].0).collect();
        for &x in &albums {
            partners
                .entry(x)
                .or_default()
                .extend(albums.iter().filter(|&&y| y != x));
        }
    }
    let compilation = |ai: usize| {
        let a = &plan.albums[ai];
        let mb_says = lib
            .meta
            .get(&a.source_dir)
            .and_then(|m| m.mb.as_ref())
            .is_some_and(|r| {
                r.release_types
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case("compilation"))
            });
        // Partners that are albums in their own right (three songs or more), so an album whose
        // songs were also released as singles is not mistaken for a compilation.
        let album_partners = partners
            .get(&ai)
            .map(|p| {
                p.iter()
                    .filter(|&&x| plan.albums[x].tracks.len() >= 3)
                    .count()
            })
            .unwrap_or(0);
        mb_says || album_partners >= 3
    };
    // Which album keeps the song: an original album over a compilation, an album over a
    // single, then the fuller and better-tagged copy.
    type Rank = (
        bool,
        bool,
        usize,
        usize,
        bool,
        u32,
        std::cmp::Reverse<usize>,
    );
    let ranks: HashMap<usize, Rank> = items
        .iter()
        .map(|(ai, _, _)| *ai)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|ai| {
            let a = &plan.albums[ai];
            let c = Copy::of(a, &by_id);
            (
                ai,
                (
                    !compilation(ai),
                    a.tracks.len() > 1,
                    a.tracks.len(),
                    c.lyrics,
                    c.barcode,
                    c.cover_px,
                    std::cmp::Reverse(ai),
                ),
            )
        })
        .collect();
    // dropped album → [(dropped track index, kept album, kept track index, proof)]
    let mut moves: BTreeMap<usize, Vec<(usize, usize, usize, Proof)>> = BTreeMap::new();
    for members in groups.values().filter(|g| g.len() > 1) {
        let keep = *members
            .iter()
            .max_by_key(|&&i| ranks.get(&items[i].0).copied())
            .expect("non-empty");
        for &i in members {
            if i == keep || items[i].0 == items[keep].0 {
                continue; // a song twice on one album is left alone
            }
            let why = proof_of.get(&i).copied().unwrap_or(Proof::Checksum);
            moves.entry(items[i].0).or_default().push((
                items[i].1,
                items[keep].0,
                items[keep].1,
                why,
            ));
        }
    }
    let mut out = Vec::new();
    let mut drop: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (da, list) in &moves {
        let dropped = &plan.albums[*da];
        let kept_albums: Vec<usize> = list
            .iter()
            .map(|(_, ka, _, _)| *ka)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let kept = &plan.albums[kept_albums[0]];
        let id = format!("dup:{}", dropped.id);
        let answer = answers.get(&id).cloned();
        let keep_both = answer.as_deref() == Some("keep_both");
        let whole = list.len() == dropped.tracks.len();
        let song_of = |a: &AlbumPlan, ti: usize| by_id.get(&a.tracks[ti].file_id).map(|t| song(t));
        let kept_names: Vec<String> = kept_albums
            .iter()
            .map(|&k| plan.albums[k].title.clone())
            .collect();
        let where_kept = if kept_names.len() == 1 {
            kept.title.clone()
        } else if kept_names.len() <= 3 {
            kept_names.join(", ")
        } else {
            format!(
                "{}, {} and {} more",
                kept_names[0],
                kept_names[1],
                kept_names.len() - 2
            )
        };
        out.push(Decision {
            id,
            kind: DecisionKind::DuplicateSongs,
            auto: true,
            proof: describe_proof(list.iter().map(|(_, _, _, p)| *p)),
            answer,
            question: "Same songs".into(),
            subject: if whole && kept_albums.len() == 1 && dropped.title == kept.title {
                format!(
                    "{} in {} is a copy of the one in {}",
                    kept.title,
                    folder_name(&dropped.source_dir),
                    folder_name(&kept.source_dir)
                )
            } else if whole && kept_albums.len() == 1 {
                format!("{} is a copy of {}", dropped.title, kept.title)
            } else {
                format!(
                    "{} from {} already on {}",
                    plural(list.len(), "song", "songs"),
                    dropped.title,
                    if kept_albums.len() == 1 { kept.title.clone() } else { "their original albums".into() }
                )
            },
            sides: vec![
                Side {
                    title: "Set aside".into(),
                    subtitle: format!("{} · {}", dropped.title, folder_name(&dropped.source_dir)),
                    covers: vec![dropped.cover.clone()],
                    facts: vec![],
                    songs: list.iter().filter_map(|(ti, _, _, _)| song_of(dropped, *ti)).collect(),
                    choice: Some("keep_both".into()),
                },
                Side {
                    title: "Kept".into(),
                    subtitle: where_kept.clone(),
                    covers: kept_albums.iter().take(4).map(|&k| plan.albums[k].cover.clone()).collect(),
                    facts: vec![],
                    songs: list
                        .iter()
                        .filter_map(|(_, ka, ki, _)| song_of(&plan.albums[*ka], *ki))
                        .collect(),
                    choice: Some("set_aside".into()),
                },
            ],
            choices: vec![
                choice(
                    "set_aside",
                    "Set the copies aside",
                    format!("Keep the songs on {where_kept} only. The copies stay in your current library."),
                ),
                choice("keep_both", "Keep both", "Both copies go into the new library."),
            ],
            suggested: Some("set_aside".into()),
            why: if kept_albums.len() > 1 {
                format!("{} is a compilation; each song stays with its original album.", dropped.title)
            } else if whole {
                format!("{} is the fuller, better-tagged copy.", kept.title)
            } else {
                format!("The songs stay with their album, {}.", kept.title)
            },
            albums: vec![],
        });
        if !keep_both {
            drop.entry(*da)
                .or_default()
                .extend(list.iter().map(|(ti, _, _, _)| *ti));
        }
    }
    for (ai, tis) in drop {
        let a = &mut plan.albums[ai];
        if tis.len() == a.tracks.len() {
            a.action = AlbumAction::Quarantine {
                duplicate_of: "the same songs on another album".into(),
            };
        } else {
            let mut i = 0;
            a.tracks.retain(|_| {
                let keep = !tis.contains(&i);
                i += 1;
                keep
            });
        }
    }
    out
}

fn describe_proof(proofs: impl Iterator<Item = Proof>) -> String {
    use crate::compare::Verdict;
    let (mut checksum, mut exact, mut nearly, mut worst, mut rounding) =
        (0usize, 0usize, 0usize, 0u64, 0usize);
    for p in proofs {
        match p {
            Proof::Checksum => checksum += 1,
            Proof::Decoded(c) if c.verdict == Verdict::Identical => exact += 1,
            Proof::Decoded(c) if c.verdict == Verdict::LastBit => rounding += 1,
            Proof::Decoded(c) => {
                nearly += 1;
                worst = worst.max(c.differing);
            }
        }
    }
    let mut parts = Vec::new();
    if checksum > 0 {
        parts.push(format!(
            "{} the same audio checksum",
            plural(checksum, "song has", "songs have")
        ));
    }
    if exact > 0 {
        parts.push(format!(
            "{} identical when decoded, sample for sample",
            plural(exact, "song is", "songs are")
        ));
    }
    if nearly > 0 {
        parts.push(format!(
            "{} identical when decoded except at most {worst} samples",
            plural(nearly, "song is", "songs are")
        ));
    }
    if rounding > 0 {
        parts.push(format!(
            "{} the same master when decoded, differing only by rounding in the last bit (inaudible)",
            plural(rounding, "song is", "songs are")
        ));
    }
    format!("Proof: {}.", parts.join("; "))
}

/// The last part of a folder path, as a person would say it.
fn folder_name(dir: &str) -> String {
    dir.rsplit('/').next().unwrap_or(dir).replace('_', " ")
}

// --- One name, several spellings ---------------------------------------------------------

/// Names written differently only in capitals, spacing or punctuation are one name. They are
/// unified automatically; this shows each one that affects an album artist.
pub fn spellings(plan: &Plan, variants: &[Vec<String>], answers: &Answers) -> Vec<Decision> {
    let album_artists: BTreeSet<String> = plan
        .albums
        .iter()
        .map(|a| crate::audit::name_key(&a.album_artist))
        .collect();
    variants
        .iter()
        .filter(|v| album_artists.contains(&crate::audit::name_key(&v[0])))
        .map(|v| {
            let id = format!("spell:{}", crate::audit::name_key(&v[0]));
            let others: Vec<String> = v[1..].iter().map(|s| format!("“{s}”")).collect();
            Decision {
                id: id.clone(),
                kind: DecisionKind::SameArtist,
                auto: true,
                proof: format!(
                    "Proof: {} and “{}” differ only in capitals, spacing or punctuation.",
                    others.join(", "),
                    v[0]
                ),
                answer: answers.get(&id).cloned(),
                question: "One name".into(),
                subject: format!("Written as “{}” everywhere", v[0]),
                sides: vec![],
                choices: v
                    .iter()
                    .map(|s| {
                        choice(
                            format!("spell:{s}"),
                            format!("Write “{s}”"),
                            "Used on every album and song.",
                        )
                    })
                    .collect(),
                suggested: Some(format!("spell:{}", v[0])),
                why: "The most common spelling, with its capitals.".into(),
                albums: vec![],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_download_file_names() {
        assert_eq!(
            names_from_file(
                "x/Gajendra Verma - Tune Mere Jaana (Emptiness) - Live 2024 - Gajendra Verma.mp3"
            ),
            (
                Some("Gajendra Verma".into()),
                "Tune Mere Jaana (Emptiness) - Live 2024".into()
            )
        );
        assert_eq!(
            names_from_file("x/01 - Your_favourite_songs.mp3"),
            (None, "Your_favourite_songs".into())
        );
        assert_eq!(country("QZ"), "United States (self-released)");
    }
}
