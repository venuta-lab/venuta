//! The planner: from the index and the owner's rules, work out what every album *should*
//! look like (tags, artwork, lyrics, path). Read-only and fast; nothing is written here.
//!
//! Every change carries the reason it was proposed and how confident the planner is, so the
//! Review screen can show it and the owner decides.

use crate::audit::name_key;
use crate::library::{Library, Track};
use crate::quality::replaygain_db;
use crate::rules::{Rules, language_name};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Something only the owner can answer.
    Low,
    /// Likely right; worth a glance.
    Medium,
    /// Mechanical cleanup or confirmed by the rules.
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub field: String,
    /// Distinct values across the album's tracks (capped).
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub reason: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ArtPlan {
    /// Embedded art is fine as it is.
    Keep,
    /// Re-encode the embedded front cover as a baseline JPEG no larger than `max_px`.
    Reencode { max_px: u32 },
    /// No art anywhere.
    Missing,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackPlan {
    pub file_id: i64,
    pub source: String,
    pub target: String,
    pub tags_before: Vec<(String, String)>,
    pub tags_after: Vec<(String, String)>,
    /// Lyrics to embed, read from this sidecar at apply time.
    pub embed_lyrics_from: Option<String>,
    /// Write a `.lrc` next to the target from the (synced) LYRICS tag.
    pub write_lrc: bool,
    /// Copy this existing `.lrc` next to the target.
    pub copy_lrc_from: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AlbumAction {
    /// Copy into the new library with the planned changes.
    Migrate,
    /// Exact duplicate (or subset) of another album: set aside, never deleted.
    Quarantine { duplicate_of: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumPlan {
    /// Source album folder; the album's identity in the plan.
    pub id: String,
    pub title: String,
    pub album_artist: String,
    pub target_dir: String,
    pub action: AlbumAction,
    pub confidence: Confidence,
    pub notes: Vec<String>,
    pub changes: Vec<FieldChange>,
    pub art: ArtPlan,
    /// Source image to copy as `cover.jpg` (the largest available), if any.
    pub cover_from: Option<String>,
    pub tracks: Vec<TrackPlan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub a: String,
    pub b: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub albums: Vec<AlbumPlan>,
    pub questions: Vec<Question>,
}

/// What the Review screen needs about an album: everything except per-track tag lists,
/// which can run to megabytes (lyrics) and stay in the engine.
#[derive(Debug, Clone, Serialize)]
pub struct AlbumView {
    pub id: String,
    pub title: String,
    pub album_artist: String,
    pub target_dir: String,
    pub action: AlbumAction,
    pub confidence: Confidence,
    pub notes: Vec<String>,
    pub changes: Vec<FieldChange>,
    pub art: ArtPlan,
    pub tracks: usize,
    /// Track file names in the new library, in order.
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub albums: Vec<AlbumView>,
    pub questions: Vec<Question>,
}

impl Plan {
    pub fn view(&self) -> PlanView {
        PlanView {
            albums: self
                .albums
                .iter()
                .map(|a| AlbumView {
                    id: a.id.clone(),
                    title: a.title.clone(),
                    album_artist: a.album_artist.clone(),
                    target_dir: a.target_dir.clone(),
                    action: a.action.clone(),
                    confidence: a.confidence,
                    notes: a.notes.clone(),
                    changes: a.changes.clone(),
                    art: a.art.clone(),
                    tracks: a.tracks.len(),
                    files: a
                        .tracks
                        .iter()
                        .map(|t| t.target.rsplit('/').next().unwrap_or(&t.target).to_owned())
                        .collect(),
                })
                .collect(),
            questions: self.questions.clone(),
        }
    }
}

/// Embedded art larger than this is downscaled for players.
pub const EMBED_MAX_PX: u32 = 1000;

fn re(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

pub fn build(lib: &Library, rules: &Rules) -> Plan {
    let albums = lib.albums();
    let quarantined = find_duplicate_albums(&albums);
    let mut plan = Plan::default();
    for (dir, tracks) in &albums {
        plan.albums
            .push(plan_album(dir, tracks, lib, rules, quarantined.get(*dir)));
    }
    resolve_collisions(&mut plan);
    plan.questions = name_questions(&plan, rules);
    plan
}

fn plan_album(
    dir: &str,
    tracks: &[&Track],
    lib: &Library,
    rules: &Rules,
    duplicate_of: Option<&String>,
) -> AlbumPlan {
    let mut notes: Vec<String> = Vec::new();
    let mut reasons: BTreeMap<String, (String, Confidence)> = BTreeMap::new();
    let mut reason = |field: &str, why: &str, c: Confidence| {
        let e = reasons
            .entry(field.to_owned())
            .or_insert_with(|| (why.to_owned(), c));
        if c < e.1 {
            *e = (why.to_owned(), c);
        }
    };

    let rule = rules.category_for(dir);
    if rule.is_none() {
        notes.push("No category rule matches this folder; it is filed under Unsorted.".into());
    }
    let category = rule.map(|r| r.category.as_str()).unwrap_or("Unsorted");
    let subfolder = rule.and_then(|r| r.subfolder.as_deref()).unwrap_or("");
    let language = rule.and_then(|r| r.language.as_deref());
    let soundtrack = rule.is_some_and(|r| r.soundtrack);

    // --- Album artist -------------------------------------------------------------------
    let folder_artist = crate::library::parent_dir(dir)
        .rsplit('/')
        .next()
        .unwrap_or("")
        .replace('_', " ");
    // Soundtracks: the composer folder decides, for the whole album.
    let soundtrack_artist: Option<(String, Confidence)> = (soundtrack && !folder_artist.is_empty())
        .then(|| match rules.canonical(&folder_artist) {
            Some(c) => (c.to_owned(), Confidence::High),
            None if name_key(&folder_artist) == "variousartists" => {
                ("Various Artists".to_owned(), Confidence::High)
            }
            None => (folder_artist.clone(), Confidence::Medium),
        });
    // Everything else: each track's own album artist, because a folder of loose singles holds
    // many artists and albums.
    let artist_for = |t: &Track| -> (String, Confidence) {
        if let Some(sa) = &soundtrack_artist {
            return sa.clone();
        }
        let first = t
            .first("ALBUMARTIST")
            .and_then(|a| split_names(a).into_iter().next())
            .or_else(|| {
                t.first("ARTIST")
                    .and_then(|a| split_names(a).into_iter().next())
            });
        match first {
            Some(n) => match rules.canonical(&n) {
                Some(c) => (c.to_owned(), Confidence::High),
                None => (tidy_name(&n), Confidence::Medium),
            },
            None => ("Unknown Artist".to_owned(), Confidence::Low),
        }
    };
    let mut unknown_names: BTreeSet<String> = BTreeSet::new();

    // --- Album title, years --------------------------------------------------------------
    let folder_album = crate::library::most_common(tracks.iter().filter_map(|t| t.first("ALBUM")))
        .unwrap_or_else(|| dir.rsplit('/').next().unwrap_or(dir).replace('_', " "));
    let folder_year = folder_year(dir);

    // --- Duplicates ---------------------------------------------------------------------
    if let Some(of) = duplicate_of {
        notes.push(format!(
            "Every track here is also in “{of}”. This copy is set aside, not deleted."
        ));
    }

    // --- Per track ----------------------------------------------------------------------
    let mut track_plans: Vec<TrackPlan> = Vec::new();
    let mut needs_art_fix = false;
    let disc_total = tracks
        .iter()
        .filter_map(|t| t.first("DISCNUMBER").and_then(parse_num))
        .max()
        .unwrap_or(1)
        .max(
            tracks
                .iter()
                .filter_map(|t| {
                    t.first("DISCTOTAL")
                        .or(t.first("TOTALDISCS"))
                        .and_then(parse_num)
                })
                .max()
                .unwrap_or(1),
        );

    for (i, t) in tracks.iter().enumerate() {
        let before: Vec<(String, String)> = t
            .raw_keys
            .iter()
            .zip(ordered_values(t))
            .map(|(k, v)| (k.clone(), v))
            .collect();
        let mut tags = TagSet(before.clone());

        // Junk: listed keys, empty values, watermark values.
        for k in &rules.strip.keys {
            if tags.remove(k) {
                reason(k, "Converter or downloader leftover.", Confidence::High);
            }
        }
        let watermarked: Vec<String> = tags
            .0
            .iter()
            .filter(|(k, v)| {
                !matches!(
                    k.to_ascii_uppercase().as_str(),
                    "LYRICS" | "UNSYNCEDLYRICS" | "URL"
                ) && rules
                    .strip
                    .watermarks
                    .iter()
                    .any(|w| v.to_lowercase().contains(w))
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in watermarked {
            tags.0.retain(|(kk, v)| {
                !(kk == &k
                    && rules
                        .strip
                        .watermarks
                        .iter()
                        .any(|w| v.to_lowercase().contains(w)))
            });
            reason(&k, "Download-site watermark.", Confidence::High);
        }
        let empties: Vec<String> = tags
            .0
            .iter()
            .filter(|(_, v)| v.trim().is_empty())
            .map(|(k, _)| k.clone())
            .collect();
        for k in empties {
            tags.0.retain(|(kk, v)| !(kk == &k && v.trim().is_empty()));
            reason(&k, "Empty value.", Confidence::High);
        }

        // Album artist, credits.
        let (album_artist, aa_conf) = artist_for(t);
        if aa_conf < Confidence::High {
            unknown_names.insert(album_artist.clone());
        }
        let aa_key = name_key(&album_artist);
        let extra_credits: Vec<String> = t
            .first("ALBUMARTIST")
            .map(split_names)
            .unwrap_or_default()
            .iter()
            .filter(|n| name_key(&canon(rules, n)) != aa_key && name_key(n) != aa_key)
            .map(|n| canon(rules, n))
            .collect();
        tags.set("ALBUMARTIST", vec![album_artist.clone()]);
        reason(
            "ALBUMARTIST",
            if soundtrack {
                "The composer, from your folder; spelling from your aliases."
            } else {
                "One canonical name per artist."
            },
            aa_conf,
        );
        if !extra_credits.is_empty() && !tags.has("CREDITS") {
            tags.set("CREDITS", extra_credits.clone());
            reason(
                "CREDITS",
                "Other names from the old album artist, kept until their roles (lyricist, composer…) are known.",
                Confidence::Medium,
            );
        }

        // Composer: canonical spellings; fill from the album artist for soundtracks.
        let composers: Vec<String> = t
            .tags
            .get("COMPOSER")
            .map(|v| {
                v.iter()
                    .flat_map(|c| split_names(c))
                    .map(|n| canon(rules, &n))
                    .collect()
            })
            .unwrap_or_default();
        if composers.is_empty() {
            if soundtrack && album_artist != "Various Artists" {
                tags.set("COMPOSER", vec![album_artist.clone()]);
                reason(
                    "COMPOSER",
                    "Music director of this soundtrack (your folder).",
                    aa_conf,
                );
            }
        } else {
            tags.set("COMPOSER", dedupe(composers));
            reason(
                "COMPOSER",
                "One canonical spelling per name.",
                Confidence::High,
            );
        }

        // Singers: one display string for every player, plus ARTISTS for those that split names.
        let singers: Vec<String> = t
            .tags
            .get("ARTIST")
            .map(|v| {
                dedupe(
                    v.iter()
                        .flat_map(|a| split_names(a))
                        .map(|n| canon(rules, &n))
                        .collect(),
                )
            })
            .unwrap_or_default();
        if !singers.is_empty() {
            tags.set("ARTIST", vec![singers.join(", ")]);
            tags.set("ARTISTS", singers);
            reason(
                "ARTIST",
                "Readable singer list, with each name spelled one way.",
                Confidence::High,
            );
            reason(
                "ARTISTS",
                "Each singer as a separate value, for players that support it.",
                Confidence::High,
            );
        }
        if let Some(l) = t.tags.get("LYRICIST") {
            let l: Vec<String> = dedupe(
                l.iter()
                    .flat_map(|x| split_names(x))
                    .map(|n| canon(rules, &n))
                    .collect(),
            );
            tags.set("LYRICIST", l);
        }

        // Album title and track title.
        let raw_album = t
            .first("ALBUM")
            .map(str::to_owned)
            .unwrap_or_else(|| folder_album.clone());
        let album = clean_album(&raw_album, rules);
        tags.set("ALBUM", vec![album.clone()]);
        if album != raw_album {
            reason(
                "ALBUM",
                "Soundtrack suffix removed; genre and release type already say it.",
                Confidence::High,
            );
        }
        if let Some(title) = t.first("TITLE") {
            let cleaned = clean_title(title, &album);
            if cleaned != title {
                tags.set("TITLE", vec![cleaned]);
                reason(
                    "TITLE",
                    "“From <this album>” removed: it repeats the album.",
                    Confidence::High,
                );
            }
        }

        // Years.
        let date_year = t.first("DATE").and_then(|d| d.get(..4)).map(str::to_owned);
        let existing_orig = t
            .first("ORIGINALYEAR")
            .or(t.first("ORIGINALDATE"))
            .and_then(|d| d.get(..4));
        let original = match (existing_orig, &folder_year, &date_year) {
            (Some(o), _, _) => Some(o.to_owned()),
            (None, Some(f), Some(d)) if f != d => {
                reason(
                    "ORIGINALYEAR",
                    &format!(
                        "Your folder says {f}; the date tag says {d} (likely a re-release). Check this one."
                    ),
                    Confidence::Medium,
                );
                Some(f.clone())
            }
            (None, _, Some(d)) => Some(d.clone()),
            (None, Some(f), None) => Some(f.clone()),
            _ => None,
        };
        if let Some(y) = &original {
            tags.set("ORIGINALYEAR", vec![y.clone()]);
            if !tags.has("ORIGINALDATE") {
                tags.set("ORIGINALDATE", vec![y.clone()]);
            }
            reason(
                "ORIGINALYEAR",
                "Year of first release, used for folder names.",
                Confidence::High,
            );
            reason(
                "ORIGINALDATE",
                "Year of first release, for players that read this name.",
                Confidence::High,
            );
        }

        // Genre, language, category.
        let category_genre = rule.and_then(|r| r.genre.clone());
        let genres: Vec<String> = t
            .tags
            .get("GENRE")
            .map(|v| {
                v.iter()
                    .flat_map(|g| g.split(['&', ';', '/']))
                    .filter_map(|g| rules.map_genre(g))
                    .collect()
            })
            .unwrap_or_default();
        let genres = if soundtrack {
            category_genre.clone().map(|g| vec![g]).unwrap_or(genres)
        } else if genres.is_empty() {
            category_genre.clone().map(|g| vec![g]).unwrap_or_default()
        } else {
            genres
        };
        if !genres.is_empty() {
            tags.set("GENRE", dedupe(genres));
            reason(
                "GENRE",
                "From your category rules and genre map.",
                Confidence::High,
            );
        }
        if let Some(l) = language {
            tags.set("LANGUAGE", vec![l.to_owned()]);
            reason("LANGUAGE", "From your category rules.", Confidence::High);
        }
        tags.set("CATEGORY", vec![category.to_owned()]);
        reason(
            "CATEGORY",
            "From your category rules; decides the top folder.",
            Confidence::High,
        );
        if soundtrack && !tags.has("RELEASETYPE") {
            tags.set("RELEASETYPE", vec!["album".into(), "soundtrack".into()]);
            reason(
                "RELEASETYPE",
                "Film and series albums are soundtracks.",
                Confidence::High,
            );
        }

        // Identifiers and numbering, in the spellings every player reads.
        if let Some(upc) = t.first("UPC").map(str::to_owned) {
            tags.remove("UPC");
            if !tags.has("BARCODE") {
                tags.set("BARCODE", vec![upc]);
            }
            reason(
                "BARCODE",
                "UPC moved to BARCODE, the standard name.",
                Confidence::High,
            );
            reason("UPC", "Moved to BARCODE.", Confidence::High);
        }
        mirror(&mut tags, "TRACKTOTAL", "TOTALTRACKS");
        mirror(&mut tags, "DISCTOTAL", "TOTALDISCS");
        for k in ["TRACKTOTAL", "TOTALTRACKS", "DISCTOTAL", "TOTALDISCS"] {
            reason(
                k,
                "Both common spellings, so every player finds the totals.",
                Confidence::High,
            );
        }

        // Loudness from the deep check.
        if let Some(q) = &t.quality
            && let (Some(l), Some(p)) = (q.lufs, q.peak)
        {
            tags.set(
                "REPLAYGAIN_TRACK_GAIN",
                vec![format!("{:.2} dB", replaygain_db(l))],
            );
            tags.set("REPLAYGAIN_TRACK_PEAK", vec![format!("{p:.6}")]);
            if let (Some(al), Some(ap)) = (q.album_lufs, q.album_peak) {
                tags.set(
                    "REPLAYGAIN_ALBUM_GAIN",
                    vec![format!("{:.2} dB", replaygain_db(al))],
                );
                tags.set("REPLAYGAIN_ALBUM_PEAK", vec![format!("{ap:.6}")]);
            }
            for k in [
                "REPLAYGAIN_TRACK_GAIN",
                "REPLAYGAIN_TRACK_PEAK",
                "REPLAYGAIN_ALBUM_GAIN",
                "REPLAYGAIN_ALBUM_PEAK",
            ] {
                reason(
                    k,
                    "Measured by the deep check (ReplayGain 2.0, −18 LUFS): steady volume between tracks.",
                    Confidence::High,
                );
            }
        }

        // Lyrics: keep one synced LYRICS tag and a matching .lrc file.
        let sidecar = format!(
            "{}.lrc",
            t.path.rsplit_once('.').map(|x| x.0).unwrap_or(&t.path)
        );
        let has_sidecar = lib.lyrics_files.contains(&sidecar);
        let lyrics_tag = t.first("LYRICS").filter(|l| !l.trim().is_empty());
        let synced = lyrics_tag.is_some_and(is_synced);
        let mut embed_lyrics_from = None;
        let mut write_lrc = false;
        let mut copy_lrc_from = None;
        if has_sidecar {
            copy_lrc_from = Some(sidecar.clone());
            if lyrics_tag.is_none() {
                embed_lyrics_from = Some(sidecar);
                reason(
                    "LYRICS",
                    "Lyrics from the .lrc file, embedded too.",
                    Confidence::High,
                );
            }
        } else if synced {
            write_lrc = true;
        }
        // UNSYNCEDLYRICS goes only when it is the same text as LYRICS; it may hold another script.
        if let (Some(l), Some(u)) = (lyrics_tag, t.first("UNSYNCEDLYRICS"))
            && same_lyrics(l, u)
            && tags.remove("UNSYNCEDLYRICS")
        {
            reason(
                "UNSYNCEDLYRICS",
                "Same words as the LYRICS tag.",
                Confidence::High,
            );
        }

        if t.front_cover().is_some_and(|p| {
            p.progressive || p.width.unwrap_or(0).max(p.height.unwrap_or(0)) > EMBED_MAX_PX
        }) || t.pictures.len() > 1
        {
            needs_art_fix = true;
        }

        // Target path.
        let track_no = t
            .first("TRACKNUMBER")
            .and_then(parse_num)
            .unwrap_or(i as u32 + 1);
        let disc_no = t.first("DISCNUMBER").and_then(parse_num).unwrap_or(1);
        let stem = t.path.rsplit('/').next().unwrap_or(&t.path);
        let stem = stem.rsplit_once('.').map(|x| x.0).unwrap_or(stem);
        let title = tags
            .first("TITLE")
            .map(str::to_owned)
            .unwrap_or_else(|| stem.to_owned());
        let year = original.clone().unwrap_or_default();
        let ext = t
            .path
            .rsplit('.')
            .next()
            .unwrap_or("flac")
            .to_ascii_lowercase();
        let target = render_path(
            rules,
            &PathFields {
                category,
                subfolder,
                language: language.map(language_name).unwrap_or(""),
                albumartist: &album_artist,
                year: &year,
                album: &album,
                disc: if disc_total > 1 {
                    format!("{disc_no}-")
                } else {
                    String::new()
                },
                track: format!("{track_no:02}"),
                title: &title,
                ext: &ext,
            },
        );

        track_plans.push(TrackPlan {
            file_id: t.id,
            source: t.path.clone(),
            target,
            tags_before: before,
            tags_after: tags.0,
            embed_lyrics_from,
            write_lrc,
            copy_lrc_from,
        });
    }

    for n in &unknown_names {
        notes.push(if n == "Unknown Artist" {
            "Some tracks have no artist in their tags.".to_owned()
        } else {
            format!("“{n}” isn't in your aliases yet; check the spelling.")
        });
    }
    let album_artist = crate::library::most_common(
        track_plans
            .iter()
            .filter_map(|tp| {
                TagSet(tp.tags_after.clone())
                    .first("ALBUMARTIST")
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str),
    )
    .unwrap_or_default();
    let raw_album = folder_album;

    let art = if tracks.iter().all(|t| t.front_cover().is_none()) {
        ArtPlan::Missing
    } else if needs_art_fix {
        ArtPlan::Reencode {
            max_px: EMBED_MAX_PX,
        }
    } else {
        ArtPlan::Keep
    };
    let cover_from = lib
        .images
        .get(dir)
        .and_then(|imgs| imgs.iter().max_by_key(|(_, s)| *s))
        .map(|(p, _)| p.clone());

    let changes = summarize_changes(&track_plans, &reasons);
    let target_dir = track_plans
        .first()
        .map(|t| crate::library::parent_dir(&t.target).to_owned())
        .unwrap_or_default();
    let mut confidence = changes
        .iter()
        .map(|c| c.confidence)
        .min()
        .unwrap_or(Confidence::High);
    if rule.is_none() {
        confidence = Confidence::Low;
    }
    let action = match duplicate_of {
        Some(of) => {
            confidence = Confidence::Low;
            AlbumAction::Quarantine {
                duplicate_of: of.clone(),
            }
        }
        None => AlbumAction::Migrate,
    };
    let title = track_plans
        .first()
        .and_then(|t| {
            TagSet(t.tags_after.clone())
                .first("ALBUM")
                .map(str::to_owned)
        })
        .unwrap_or(raw_album);
    AlbumPlan {
        id: dir.to_owned(),
        title,
        album_artist,
        target_dir,
        action,
        confidence,
        notes,
        changes,
        art,
        cover_from,
        tracks: track_plans,
    }
}

/// Album-level view of the per-track tag changes: distinct before/after values per field.
fn summarize_changes(
    tracks: &[TrackPlan],
    reasons: &BTreeMap<String, (String, Confidence)>,
) -> Vec<FieldChange> {
    let mut fields: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for t in tracks {
        let before = group(&t.tags_before);
        let after = group(&t.tags_after);
        let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        for k in keys {
            let b = before.get(k).cloned().unwrap_or_default();
            let a = after.get(k).cloned().unwrap_or_default();
            if a != b {
                let e = fields.entry(k.clone()).or_default();
                e.0.insert(b.join(" | "));
                e.1.insert(a.join(" | "));
            }
        }
    }
    fields
        .into_iter()
        .map(|(field, (before, after))| {
            let (why, conf) = reasons
                .get(&field)
                .cloned()
                .or_else(|| {
                    reasons
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(&field))
                        .map(|(_, v)| v.clone())
                })
                .unwrap_or_else(|| ("Cleanup.".into(), Confidence::High));
            let cap =
                |s: BTreeSet<String>| s.into_iter().filter(|v| !v.is_empty()).take(6).collect();
            FieldChange {
                field,
                before: cap(before),
                after: cap(after),
                reason: why,
                confidence: conf,
            }
        })
        .collect()
}

fn group(tags: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in tags {
        let v = if v.chars().count() > 80 {
            format!("{}…", v.chars().take(80).collect::<String>())
        } else {
            v.clone()
        };
        m.entry(k.to_ascii_uppercase()).or_default().push(v);
    }
    m
}

/// Albums that are exact duplicates (or subsets) of another album, mapped to the album kept.
fn find_duplicate_albums(albums: &BTreeMap<&str, Vec<&Track>>) -> HashMap<String, String> {
    let sets: Vec<(&str, HashSet<&[u8]>, usize)> = albums
        .iter()
        .map(|(d, ts)| {
            let md5: HashSet<&[u8]> = ts.iter().filter_map(|t| t.audio_md5.as_deref()).collect();
            (*d, md5, ts.len())
        })
        .collect();
    let mut out = HashMap::new();
    for (i, (a, sa, na)) in sets.iter().enumerate() {
        if sa.is_empty() || sa.len() < *na {
            continue; // some tracks lack MD5: can't prove duplication
        }
        for (j, (b, sb, _)) in sets.iter().enumerate() {
            if i == j || sb.len() < sa.len() || !sa.is_subset(sb) || out.contains_key(*b) {
                continue;
            }
            // Equal sets: keep the copy with the better home (a composer folder over a collection).
            if sa.len() == sb.len() && keep_score(a) > keep_score(b) {
                continue;
            }
            out.insert((*a).to_owned(), (*b).to_owned());
            break;
        }
    }
    out
}

fn keep_score(dir: &str) -> i32 {
    let mut s = 0;
    if !dir.contains("Various_Artists") && !dir.contains("06_Collections") {
        s += 2;
    }
    if !dir.contains(' ') {
        s += 1; // the owner's own underscore naming
    }
    s
}

/// Two source tracks must never land on the same target: number the later ones.
fn resolve_collisions(plan: &mut Plan) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for album in plan
        .albums
        .iter_mut()
        .filter(|a| matches!(a.action, AlbumAction::Migrate))
    {
        for t in &mut album.tracks {
            let key = t.target.to_lowercase();
            let n = seen.entry(key).or_insert(0);
            *n += 1;
            if *n > 1 {
                let (stem, ext) = t
                    .target
                    .rsplit_once('.')
                    .map(|(s, e)| (s.to_owned(), e.to_owned()))
                    .unwrap_or_else(|| (t.target.clone(), String::new()));
                t.target = format!("{stem} ({n}).{ext}");
                album.notes.push(format!(
                    "Another album already uses “{stem}”; numbered to keep both."
                ));
                album.confidence = album.confidence.min(Confidence::Low);
            }
        }
    }
}

/// "Is X the same person as Y?" when one album artist's name contains another's.
fn name_questions(plan: &Plan, rules: &Rules) -> Vec<Question> {
    let names: BTreeSet<&str> = plan
        .albums
        .iter()
        .map(|a| a.album_artist.as_str())
        .collect();
    let tokens = |n: &str| -> BTreeSet<String> {
        n.split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let mut out = Vec::new();
    for a in &names {
        for b in &names {
            if a >= b || rules.is_distinct(a, b) {
                continue;
            }
            let (ta, tb) = (tokens(a), tokens(b));
            if ta.is_empty() || tb.is_empty() {
                continue;
            }
            if ta.is_subset(&tb) || tb.is_subset(&ta) {
                out.push(Question {
                    id: format!("same:{}|{}", name_key(a), name_key(b)),
                    text: format!("Are “{a}” and “{b}” the same artist?"),
                    a: (*a).to_owned(),
                    b: (*b).to_owned(),
                });
            }
        }
    }
    out
}

struct PathFields<'a> {
    category: &'a str,
    subfolder: &'a str,
    language: &'a str,
    albumartist: &'a str,
    year: &'a str,
    album: &'a str,
    disc: String,
    track: String,
    title: &'a str,
    ext: &'a str,
}

fn render_path(rules: &Rules, f: &PathFields<'_>) -> String {
    let max = rules.layout.max_component;
    let year_album = |s: &str| s.trim_start_matches(" - ").to_owned();
    let mut out = rules.layout.template.clone();
    for (key, value) in [
        ("{category}", sanitize(f.category, max)),
        ("{subfolder}", sanitize(f.subfolder, max)),
        ("{language}", sanitize(f.language, max)),
        ("{albumartist}", sanitize(f.albumartist, max)),
        ("{album}", sanitize(f.album, max)),
        ("{disc}", f.disc.clone()),
        ("{track}", f.track.clone()),
    ] {
        out = out.replace(key, &value);
    }
    out = out.replace("{year}", f.year);
    // Empty segments collapse; a missing year must not leave " - Album".
    let segments: Vec<String> = out
        .split('/')
        .map(year_album)
        .filter(|s| !s.trim().is_empty())
        .collect();
    let mut path = segments.join("/");
    // The title goes last and absorbs any length limit.
    let room = rules
        .layout
        .max_path
        .saturating_sub(path.chars().count() + f.ext.len() + 1);
    let title = sanitize(f.title, room.clamp(20, max));
    path = path.replace("{title}", &title);
    format!("{path}.{}", f.ext)
}

/// Safe on Windows, exFAT and Android: no reserved characters, no trailing dot or space,
/// NFC, length-limited on a character boundary.
pub fn sanitize(s: &str, max: usize) -> String {
    use unicode_normalization::UnicodeNormalization;
    let cleaned: String = s
        .nfc()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let mut out = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > max {
        out = out
            .chars()
            .take(max)
            .collect::<String>()
            .trim_end()
            .to_owned();
    }
    let out = out.trim_end_matches(['.', ' ']).to_owned();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "LPT1", "LPT2", "LPT3",
    ];
    if RESERVED.contains(&out.to_ascii_uppercase().as_str()) {
        format!("{out}_")
    } else {
        out
    }
}

/// Remove soundtrack labels wherever they appear: "Interstellar (Original Motion Picture
/// Soundtrack) [Expanded Edition]" becomes "Interstellar [Expanded Edition]".
fn clean_album(album: &str, rules: &Rules) -> String {
    let mut a = album.trim().to_owned();
    for suffix in &rules.titles.album_suffixes {
        let needle = suffix.trim().to_lowercase();
        if needle.is_empty() {
            continue;
        }
        while let Some(at) = a.to_lowercase().find(&needle) {
            // Lower-casing never changes byte length for these ASCII labels.
            a.replace_range(at..at + needle.len(), "");
        }
        a = a
            .trim_end_matches([' ', '-'])
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
    }
    a
}

fn clean_title(title: &str, album: &str) -> String {
    static FROM: OnceLock<Regex> = OnceLock::new();
    let rx = re(
        r#"(?i)\s*[\(\[]\s*from\s+["“”']?([^"“”'\)\]]+?)["“”']?\s*[\)\]]"#,
        &FROM,
    );
    static SPACE_IN: OnceLock<Regex> = OnceLock::new();
    static SPACE_OUT: OnceLock<Regex> = OnceLock::new();
    let album_key = name_key(album);
    // "(From Tamasha )" -> "(From Tamasha)": stray spaces just inside brackets.
    let tidy = re(r"\s+([\)\]])", &SPACE_IN).replace_all(title, "$1");
    let tidy = re(r"([\(\[])\s+", &SPACE_OUT)
        .replace_all(&tidy, "$1")
        .into_owned();
    let title = tidy.as_str();
    let mut out = title.to_owned();
    if let Some(m) = rx.captures(title)
        && name_key(&m[1]) == album_key
    {
        out = rx.replace(title, "").trim().to_owned();
    }
    out
}

/// True when two lyrics texts have the same words, ignoring LRC timestamps and spacing.
fn same_lyrics(a: &str, b: &str) -> bool {
    static TS: OnceLock<Regex> = OnceLock::new();
    let ts = re(r"\[[^\]]*\]", &TS);
    let norm = |s: &str| -> String {
        ts.replace_all(s, " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn is_synced(lyrics: &str) -> bool {
    static TS: OnceLock<Regex> = OnceLock::new();
    re(r"\[\d{1,2}:\d{2}(\.\d{1,3})?\]", &TS).is_match(lyrics)
}

fn folder_year(dir: &str) -> Option<String> {
    let name = dir.rsplit('/').next()?;
    let y: String = name.chars().take(4).collect();
    (y.len() == 4
        && y.chars().all(|c| c.is_ascii_digit())
        && (1900..2100).contains(&y.parse::<u32>().ok()?))
    .then_some(y)
}

fn parse_num(s: &str) -> Option<u32> {
    s.split('/').next()?.trim().parse().ok()
}

/// Split "A, B & C" into names, keeping duo names like "Vishal-Shekhar" whole.
pub fn split_names(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .flat_map(|p| p.split(" & "))
        .flat_map(|p| p.split(" and "))
        .map(tidy_name)
        .filter(|p| !p.is_empty())
        .collect()
}

/// Collapse spacing and non-breaking spaces; turn SHOUTED names into Title Case.
fn tidy_name(s: &str) -> String {
    let collapsed = s
        .replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let letters: Vec<char> = collapsed.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 3 && letters.iter().all(|c| c.is_uppercase()) {
        collapsed
            .split(' ')
            .map(|w| {
                let mut cs = w.chars();
                cs.next()
                    .map(|f| {
                        f.to_uppercase()
                            .chain(cs.flat_map(char::to_lowercase))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect::<Vec<String>>()
            .join(" ")
    } else {
        collapsed
    }
}

fn canon(rules: &Rules, name: &str) -> String {
    rules
        .canonical(name)
        .map(str::to_owned)
        .unwrap_or_else(|| tidy_name(name))
}

fn dedupe(v: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    v.into_iter().filter(|x| seen.insert(name_key(x))).collect()
}

/// Values of a track's tags in file order (Library keeps keys and values separately).
fn ordered_values(t: &Track) -> Vec<String> {
    let mut cursor: HashMap<String, usize> = HashMap::new();
    t.raw_keys
        .iter()
        .map(|k| {
            let up = k.to_ascii_uppercase();
            let i = cursor.entry(up.clone()).or_insert(0);
            let v = t
                .tags
                .get(&up)
                .and_then(|vs| vs.get(*i))
                .cloned()
                .unwrap_or_default();
            *i += 1;
            v
        })
        .collect()
}

fn mirror(tags: &mut TagSet, a: &str, b: &str) {
    match (
        tags.first(a).map(str::to_owned),
        tags.first(b).map(str::to_owned),
    ) {
        (Some(v), None) => tags.set(b, vec![v]),
        (None, Some(v)) => tags.set(a, vec![v]),
        _ => {}
    }
}

/// Ordered tag list with Vorbis semantics (case-insensitive keys, repeated keys allowed).
#[derive(Debug, Clone)]
pub struct TagSet(pub Vec<(String, String)>);

impl TagSet {
    pub fn first(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, key: &str) -> bool {
        self.0
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case(key) && !v.trim().is_empty())
    }

    /// Replace every value of `key` with `values`, at the position of the first old value.
    pub fn set(&mut self, key: &str, values: Vec<String>) {
        let at = self.0.iter().position(|(k, _)| k.eq_ignore_ascii_case(key));
        let current: Vec<&String> = self
            .0
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
            .collect();
        if current.len() == values.len() && current.iter().zip(&values).all(|(a, b)| *a == b) {
            return; // unchanged: keep the original key spelling and position
        }
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
        let at = at.unwrap_or(self.0.len()).min(self.0.len());
        for (i, v) in values.into_iter().enumerate() {
            self.0.insert(at + i, (key.to_owned(), v));
        }
    }

    /// Remove every value of `key`; true if anything was removed.
    pub fn remove(&mut self, key: &str) -> bool {
        let n = self.0.len();
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
        self.0.len() != n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_and_tidy() {
        assert_eq!(
            split_names("A.R. Rahman, IRSHAD KAMIL & Javed Ali"),
            vec!["A.R. Rahman", "Irshad Kamil", "Javed Ali"]
        );
        assert_eq!(split_names("Vishal-Shekhar"), vec!["Vishal-Shekhar"]);
        assert_eq!(tidy_name("Ram Sampath\u{a0}"), "Ram Sampath");
        assert_eq!(tidy_name("ADNAN SAMI"), "Adnan Sami");
        assert_eq!(tidy_name("KK"), "KK"); // short acronyms stay
    }

    #[test]
    fn titles_and_albums() {
        let rules = Rules::default();
        assert_eq!(
            clean_album("Swades (Original Motion Picture Soundtrack)", &rules),
            "Swades"
        );
        assert_eq!(
            clean_album("Interstellar (Expanded Edition)", &rules),
            "Interstellar (Expanded Edition)"
        );
        assert_eq!(
            clean_title("Raanjhanaa (From \"Raanjhanaa\")", "Raanjhanaa"),
            "Raanjhanaa"
        );
        assert_eq!(
            clean_title("Tum Hi Ho (From \"Aashiqui 2\")", "Raanjhanaa"),
            "Tum Hi Ho (From \"Aashiqui 2\")"
        );
        assert_eq!(
            clean_title("Agar Tum Saath Ho (From Tamasha )", "Arijit's Aura"),
            "Agar Tum Saath Ho (From Tamasha)"
        );
        assert_eq!(
            clean_album(
                "Interstellar (Original Motion Picture Soundtrack) [Expanded Edition]",
                &rules
            ),
            "Interstellar [Expanded Edition]"
        );
    }

    #[test]
    fn lyrics_comparison_ignores_timestamps_only() {
        let synced = "[00:39.06] Dil hai chhota sa\n[00:43.93] Masti bhare man ki";
        assert!(same_lyrics(
            synced,
            "Dil hai chhota sa\nMasti bhare  man ki"
        ));
        // Same song in another script is NOT a duplicate: it must be kept.
        assert!(!same_lyrics(synced, "दिल है छोटा सा\nमस्ती भरे मन की"));
        assert!(is_synced(synced));
        assert!(!is_synced("Dil hai chhota sa"));
    }

    #[test]
    fn sanitize_is_safe() {
        assert_eq!(
            sanitize("Jaane Tu... Ya Jaane Na", 120),
            "Jaane Tu... Ya Jaane Na"
        );
        assert_eq!(sanitize("What? A: B/C*", 120), "What A B C");
        assert_eq!(sanitize("Ends with dot.", 120), "Ends with dot");
        assert_eq!(sanitize("CON", 120), "CON_");
        assert!(sanitize(&"x".repeat(300), 120).chars().count() <= 120);
    }

    #[test]
    fn tagset_set_keeps_position_and_skips_noop() {
        let mut t = TagSet(vec![
            ("title".into(), "A".into()),
            ("ARTIST".into(), "B".into()),
        ]);
        t.set("TITLE", vec!["A".into()]);
        assert_eq!(t.0[0].0, "title"); // unchanged value: original key spelling kept
        t.set("ARTIST", vec!["X".into(), "Y".into()]);
        assert_eq!(
            t.0,
            vec![
                ("title".into(), "A".into()),
                ("ARTIST".into(), "X".into()),
                ("ARTIST".into(), "Y".into())
            ]
        );
        assert!(t.remove("artist"));
        assert_eq!(t.0.len(), 1);
    }

    #[test]
    fn folder_years() {
        assert_eq!(folder_year("a/b/2004_Swades").as_deref(), Some("2004"));
        assert_eq!(folder_year("a/b/Dune (OST)"), None);
        assert_eq!(folder_year("a/b/12th_Fail"), None);
    }
}
