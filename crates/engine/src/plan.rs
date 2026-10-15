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
    /// The album's identity in the plan: its source folder, or "folder#target" when a folder of
    /// loose singles becomes several albums.
    pub id: String,
    /// The folder the tracks come from.
    pub source_dir: String,
    /// Key for the cover thumbnail: the source folder, or "track:<path>" for one track's art.
    pub cover: String,
    /// Decisions this album waits for; it is held back from a build until they are answered.
    pub waiting_on: Vec<String>,
    /// (folder year, date-tag year) when they disagree and nothing else settles it.
    #[serde(skip)]
    pub year_conflict: Option<(String, String)>,
    /// The year settled automatically for a conflict, with its proof.
    #[serde(skip)]
    pub year_auto: Option<(String, String)>,
    pub title: String,
    pub album_artist: String,
    pub target_dir: String,
    /// The id this album had when ids named its destination folder; answers saved under it
    /// are still honoured.
    #[serde(skip)]
    pub legacy_id: Option<String>,
    pub action: AlbumAction,
    pub confidence: Confidence,
    pub notes: Vec<String>,
    pub changes: Vec<FieldChange>,
    pub art: ArtPlan,
    /// Source image to copy as `cover.jpg` (the largest available), if any.
    pub cover_from: Option<String>,
    /// A sharper cover to download (Apple up to 3000 px, else Deezer 1400 px).
    pub cover_url: Option<String>,
    pub tracks: Vec<TrackPlan>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub albums: Vec<AlbumPlan>,
    /// What only the owner can settle (see [`crate::review`]).
    pub decisions: Vec<crate::review::Decision>,
    /// Likely duplicates not yet compared by decoding; compare them and plan again.
    #[serde(skip)]
    pub pending_pairs: Vec<(i64, i64)>,
}

/// What the Review screen needs about an album: everything except per-track tag lists,
/// which can run to megabytes (lyrics) and stay in the engine.
#[derive(Debug, Clone, Serialize)]
pub struct AlbumView {
    pub id: String,
    pub source_dir: String,
    pub cover: String,
    pub waiting_on: Vec<String>,
    pub title: String,
    pub album_artist: String,
    pub target_dir: String,
    pub action: AlbumAction,
    pub confidence: Confidence,
    pub notes: Vec<String>,
    pub changes: Vec<FieldChange>,
    pub art: ArtPlan,
    /// A sharper cover was found online and replaces the embedded one.
    pub cover_upgrade: bool,
    pub tracks: usize,
    /// Track file names in the new library, in order.
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub albums: Vec<AlbumView>,
    pub decisions: Vec<crate::review::Decision>,
}

impl Plan {
    pub fn view(&self) -> PlanView {
        PlanView {
            albums: self
                .albums
                .iter()
                .map(|a| AlbumView {
                    id: a.id.clone(),
                    source_dir: a.source_dir.clone(),
                    cover: a.cover.clone(),
                    waiting_on: a.waiting_on.clone(),
                    title: a.title.clone(),
                    album_artist: a.album_artist.clone(),
                    target_dir: a.target_dir.clone(),
                    action: a.action.clone(),
                    confidence: a.confidence,
                    notes: a.notes.clone(),
                    changes: a.changes.clone(),
                    art: a.art.clone(),
                    cover_upgrade: a.cover_url.is_some(),
                    tracks: a.tracks.len(),
                    files: a
                        .tracks
                        .iter()
                        .map(|t| t.target.rsplit('/').next().unwrap_or(&t.target).to_owned())
                        .collect(),
                })
                .collect(),
            decisions: self.decisions.clone(),
        }
    }
}

/// Embedded art larger than this is downscaled for players.
pub const EMBED_MAX_PX: u32 = 1000;

fn re(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

pub fn build(lib: &Library, rules: &Rules) -> Plan {
    build_with(lib, rules, &crate::review::Answers::new())
}

/// Plan with the owner's answers to earlier decisions applied.
pub fn build_with(lib: &Library, rules: &Rules, answers: &crate::review::Answers) -> Plan {
    use crate::review;
    // Files with no tags take their names from the file name, or stay out if so answered.
    let leave_out_untagged = answers.get(review::UNTAGGED).map(String::as_str) == Some("leave_out");
    let named: HashMap<i64, Track> = lib
        .tracks
        .iter()
        .filter(|t| review::is_untagged(t))
        .map(|t| {
            (
                t.id,
                review::tag_from_file_name(t, lib.identities.get(&t.id)),
            )
        })
        .collect();
    let mut albums: BTreeMap<&str, Vec<&Track>> = BTreeMap::new();
    for t in &lib.tracks {
        let t = match named.get(&t.id) {
            Some(_) if leave_out_untagged => continue,
            Some(n) => n,
            None => t,
        };
        albums.entry(t.dir.as_str()).or_default().push(t);
    }
    // One spelling per name: "AUR" and "aur" are the same name written two ways.
    let mut answered = rules.clone();
    for (id, v) in answers {
        if let (true, Some(s)) = (id.starts_with("spell:"), v.strip_prefix("spell:")) {
            answered.add_spelling(s);
        }
    }
    let (rules, spellings) = &unify_spellings(lib, &answered);
    let quarantined = find_duplicate_albums(&albums);
    let mut plan = Plan::default();
    for (dir, tracks) in &albums {
        let year = answers
            .get(&review::year_id(dir))
            .and_then(|y| y.strip_prefix("y:"));
        let mut planned = plan_album(dir, tracks, lib, rules, quarantined.get(*dir), year);
        // In your library, what Venuta can't place (no category tag: files it can't tag, or
        // music put there by hand) stays exactly where it is.
        if lib.in_library && rule_from_tags(tracks, rules).is_none() {
            planned.iter_mut().for_each(stay_put);
        }
        plan.albums.extend(planned);
    }
    let mut decisions = review::duplicate_songs(&mut plan, lib, answers);
    decisions.extend(review::spellings(&plan, spellings, answers));
    // Files without tags already in your library were kept on purpose; no need to ask again.
    if !lib.in_library {
        decisions.extend(review::untagged(lib, answers, &plan));
    }
    decisions.extend(review::same_album(&mut plan, lib, answers));
    resolve_collisions(&mut plan);
    decisions.extend(review::years(&plan, lib, answers));
    decisions.extend(review::same_artist(&plan, lib, rules));
    review::hold(&mut plan, &decisions);
    plan.decisions = decisions;
    plan
}

/// Leave an album of your library as it is: same folder, same files, same tags.
fn stay_put(a: &mut AlbumPlan) {
    a.target_dir = a.source_dir.clone();
    for t in &mut a.tracks {
        t.target = t.source.clone();
        t.tags_after = t.tags_before.clone();
        t.embed_lyrics_from = None;
        t.write_lrc = false;
        t.copy_lrc_from = None;
    }
    a.changes.clear();
}

/// Where an album of your library belongs, from the tags its build wrote. Whether its shelf
/// holds soundtracks (composer as album artist) is your category rules' call, as at the build.
fn rule_from_tags(tracks: &[&Track], rules: &Rules) -> Option<crate::rules::CategoryRule> {
    let pick = |k: &str| crate::library::most_common(tracks.iter().filter_map(|t| t.first(k)));
    let category = pick("CATEGORY")?;
    let subfolder = pick("SUBCATEGORY");
    let soundtrack = rules.categories.rules.iter().any(|r| {
        r.soundtrack && r.category == category && r.subfolder.as_deref() == subfolder.as_deref()
    });
    Some(crate::rules::CategoryRule {
        pattern: String::new(),
        category,
        language: pick("LANGUAGE"),
        genre: None,
        soundtrack,
        subfolder,
    })
}

fn plan_album(
    dir: &str,
    tracks: &[&Track],
    lib: &Library,
    rules: &Rules,
    duplicate_of: Option<&String>,
    year_answer: Option<&str>,
) -> Vec<AlbumPlan> {
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

    // Online lookups (if "Look up online" has run).
    let meta = lib.meta.get(dir);
    // MusicBrainz first; Discogs (with a token) where MusicBrainz doesn't know the release.
    let mb = meta.and_then(|m| m.mb.as_ref().or(m.discogs.as_ref()));
    let mb_name = mb.map(|r| r.source.label()).unwrap_or("MusicBrainz");

    // In your library an album is where its tags say (as built); elsewhere your category
    // rules decide; failing those, the catalogue genre suggests one.
    let from_tags = lib
        .in_library
        .then(|| rule_from_tags(tracks, rules))
        .flatten();
    let (rule_owned, rule_inferred) = match from_tags.or_else(|| rules.category_for(dir).cloned()) {
        Some(r) => (Some(r), false),
        None => {
            let i = infer_category(meta);
            let inferred = i.is_some();
            (i, inferred)
        }
    };
    let rule = rule_owned.as_ref();
    if rule.is_none() {
        notes.push("No category rule matches this folder and the catalogues didn't say; it is filed under Unsorted.".into());
    } else if rule_inferred {
        notes.push(format!(
            "Filed under {} from the catalogue genre. Add a category rule to decide this yourself.",
            rule.map(|r| r.category.as_str()).unwrap_or_default()
        ));
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
    // Composer as credited by the catalogues, for soundtracks without a composer folder.
    let catalogue_artist = mb.and_then(|r| r.album_artist.clone()).or_else(|| {
        meta.and_then(|m| m.apple.as_ref())
            .and_then(|r| r.album_artist.clone())
    });
    let soundtrack_artist: Option<(String, Confidence)> = if soundtrack && rule_inferred {
        catalogue_artist.map(|a| match rules.canonical(&a) {
            Some(c) => (c.to_owned(), Confidence::High),
            None => (tidy_name(&a), Confidence::High),
        })
    } else {
        (soundtrack && !folder_artist.is_empty()).then(|| match rules.canonical(&folder_artist) {
            Some(c) => (c.to_owned(), Confidence::High),
            None if name_key(&folder_artist) == "variousartists" => {
                ("Various Artists".to_owned(), Confidence::High)
            }
            None => (folder_artist.clone(), Confidence::High),
        })
    };
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
                None => (tidy_name(&n), Confidence::High),
            },
            None => ("Unknown Artist".to_owned(), Confidence::Low),
        }
    };
    let mut unknown_names: BTreeSet<String> = BTreeSet::new();
    // Everyone credited anywhere on the album, so leftover album-artist names already known
    // as a singer, composer or lyricist on another track aren't kept twice.
    let album_known: HashSet<String> = tracks
        .iter()
        .flat_map(|t| {
            ["ARTIST", "COMPOSER", "LYRICIST"]
                .iter()
                .flat_map(|k| t.tags.get(*k).cloned().unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .flat_map(|v| split_names(&v))
        .map(|n| name_key(&canon(rules, &n)))
        .collect();

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
    let mut year_conflict: Option<(String, String)> = None;
    let mut year_auto: Option<(String, String)> = None;
    // The year the album's recording codes were issued: strong evidence of the original year.
    let isrc_year = tracks
        .iter()
        .filter_map(|t| t.first("ISRC").and_then(isrc_year))
        .min();
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

        // This track on MusicBrainz: by ISRC, else by position, else by title.
        let online = mb.and_then(|r| match_track(t, r, tracks.len()));
        let online_composers: Vec<String> = online
            .map(|o| dedupe(o.composers.iter().map(|n| canon(rules, n)).collect()))
            .unwrap_or_default();
        let online_lyricists: Vec<String> = online
            .map(|o| dedupe(o.lyricists.iter().map(|n| canon(rules, n)).collect()))
            .unwrap_or_default();

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
        // Names now known as composer, lyricist or singer leave the unsorted list.
        let known: HashSet<String> = online_composers
            .iter()
            .chain(&online_lyricists)
            .cloned()
            .chain(
                t.first("ARTIST")
                    .map(split_names)
                    .unwrap_or_default()
                    .iter()
                    .map(|n| canon(rules, n)),
            )
            .map(|n| name_key(&n))
            .collect();
        let extra_credits: Vec<String> = extra_credits
            .into_iter()
            .filter(|n| !known.contains(&name_key(n)) && !album_known.contains(&name_key(n)))
            .collect();
        if !extra_credits.is_empty() && !tags.has("CREDITS") {
            tags.set("CREDITS", extra_credits.clone());
            reason(
                "CREDITS",
                "Other names from the old album artist (cast, director, co-writers), kept so nothing is lost.",
                Confidence::High,
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
        if !online_composers.is_empty() {
            tags.set("COMPOSER", online_composers.clone());
            reason(
                "COMPOSER",
                "Credited on MusicBrainz for this recording.",
                Confidence::High,
            );
        } else if composers.is_empty() {
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

        // Singers: one ARTIST entry per singer (the Vorbis standard). Card copies join them for
        // players that read only one value.
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
            tags.set("ARTIST", singers);
            reason(
                "ARTIST",
                "Each singer as a separate entry, spelled one way.",
                Confidence::High,
            );
        }
        if !online_lyricists.is_empty() && !t.has("LYRICIST") {
            tags.set("LYRICIST", online_lyricists.clone());
            reason(
                "LYRICIST",
                "Credited on MusicBrainz for this recording.",
                Confidence::High,
            );
        } else if let Some(l) = t.tags.get("LYRICIST") {
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
        let date_year = t.first("DATE").and_then(year_of).map(str::to_owned);
        let existing_orig = t
            .first("ORIGINALYEAR")
            .or(t.first("ORIGINALDATE"))
            .and_then(year_of);
        let mb_first = mb
            .and_then(|r| r.first_release.as_deref())
            .and_then(year_of);
        let original = match (existing_orig, &folder_year, &date_year) {
            (Some(o), _, _) => Some(o.to_owned()),
            (None, _, _) if year_answer.is_some() => {
                let y = year_answer.unwrap_or_default().to_owned();
                reason("ORIGINALYEAR", &format!("You chose {y}."), Confidence::High);
                Some(y)
            }
            (None, _, _) if mb_first.is_some() => {
                let y = mb_first.unwrap_or_default().to_owned();
                let mut why = format!("{mb_name}: first released in {y}.");
                if folder_year.as_deref().is_some_and(|f| f != y) {
                    why.push_str(&format!(
                        " Your folder says {}.",
                        folder_year.as_deref().unwrap_or("")
                    ));
                }
                reason("ORIGINALYEAR", &why, Confidence::High);
                Some(y)
            }
            (None, Some(f), Some(d)) if f != d => {
                year_conflict = Some((f.clone(), d.clone()));
                match isrc_year.as_ref().filter(|y| *y == f || *y == d) {
                    Some(y) => {
                        let other = if y == f { d } else { f };
                        let proof = format!(
                            "Proof: the recording codes (ISRC) were issued in {y}, which matches {}; {other} is likely a re-release or a misdated folder.",
                            if y == f {
                                "your folder name"
                            } else {
                                "the date tag"
                            }
                        );
                        reason("ORIGINALYEAR", &proof, Confidence::High);
                        year_auto = Some((y.clone(), proof));
                        Some(y.clone())
                    }
                    None => {
                        reason(
                            "ORIGINALYEAR",
                            &format!(
                                "Your folder says {f}; the date tag says {d}. Waiting for your answer."
                            ),
                            Confidence::Medium,
                        );
                        Some(f.clone())
                    }
                }
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
        // The sub-folder too, so the library alone knows where each album belongs.
        if subfolder.is_empty() {
            tags.remove("SUBCATEGORY");
        } else {
            tags.set("SUBCATEGORY", vec![subfolder.to_owned()]);
        }
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

        // Label and release type from the lookups, when the file has none.
        let online_label = mb.and_then(|r| r.label.clone()).or_else(|| {
            meta.and_then(|m| m.deezer.as_ref())
                .and_then(|r| r.label.clone())
        });
        if !t.has("LABEL")
            && let Some(l) = online_label
        {
            tags.set("LABEL", vec![l]);
            reason(
                "LABEL",
                "Record label from MusicBrainz or Deezer.",
                Confidence::High,
            );
        }
        if let Some(r) = mb.filter(|r| !r.release_types.is_empty())
            && !t.has("RELEASETYPE")
        {
            tags.set("RELEASETYPE", r.release_types.clone());
            reason(
                "RELEASETYPE",
                "Release type from MusicBrainz.",
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
        } else if lyrics_tag.is_none()
            && !t.has("UNSYNCEDLYRICS")
            && let Some(found) = lib.fetched_lyrics.get(&t.id)
        {
            tags.set("LYRICS", vec![found.synced.clone()]);
            write_lrc = true;
            reason(
                "LYRICS",
                "Synced lyrics from LRCLIB, checked for timing, length and script.",
                Confidence::High,
            );
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

    if unknown_names.contains("Unknown Artist") {
        notes.push("Some tracks have no artist in their tags.".to_owned());
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

    let best_embedded = tracks
        .iter()
        .filter_map(|t| t.front_cover())
        .filter_map(|p| Some(p.width?.min(p.height?)))
        .max()
        .unwrap_or(0);
    let cover_url = (best_embedded < crate::enrich::GOOD_ART_PX)
        .then(|| {
            meta.and_then(|m| m.apple.as_ref().or(m.deezer.as_ref()))
                .and_then(|r| r.cover_url.clone())
        })
        .flatten();
    if cover_url.is_some() {
        needs_art_fix = true;
    }
    let art = if tracks.iter().all(|t| t.front_cover().is_none()) && cover_url.is_none() {
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

    let mut confidence = Confidence::High;
    if rule.is_none() {
        confidence = Confidence::Low;
    } else if rule_inferred {
        confidence = Confidence::Medium;
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

    // One album per destination: a folder of loose singles becomes one album per single.
    let mut groups: Vec<(String, Vec<TrackPlan>)> = Vec::new();
    for tp in track_plans {
        let d = crate::library::parent_dir(&tp.target).to_owned();
        match groups.iter_mut().find(|(g, _)| g.eq_ignore_ascii_case(&d)) {
            Some((_, v)) => v.push(tp),
            None => groups.push((d, vec![tp])),
        }
    }
    let split = groups.len() > 1;
    let by_id: HashMap<i64, &Track> = tracks.iter().map(|t| (t.id, *t)).collect();
    groups
        .into_iter()
        .map(|(target_dir, tps)| {
            let changes = summarize_changes(&tps, &reasons);
            let confidence = changes
                .iter()
                .map(|c| c.confidence)
                .min()
                .unwrap_or(Confidence::High)
                .min(confidence);
            let album_artist = crate::library::most_common(
                tps.iter()
                    .filter_map(|tp| {
                        TagSet(tp.tags_after.clone())
                            .first("ALBUMARTIST")
                            .map(str::to_owned)
                    })
                    .collect::<Vec<_>>()
                    .iter()
                    .map(String::as_str),
            )
            .unwrap_or_else(|| album_artist.clone());
            let title = tps
                .first()
                .and_then(|t| {
                    TagSet(t.tags_after.clone())
                        .first("ALBUM")
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| raw_album.clone());
            // A single's cover is its own embedded art; the folder's image and the looked-up
            // cover belong to the folder's main album only.
            let main = !split
                || meta
                    .and_then(|m| m.apple.as_ref().or(m.deezer.as_ref()))
                    .is_some_and(|r| title_key_eq(&r.title, &title));
            let cover = if split {
                tps.iter()
                    .find(|tp| {
                        by_id
                            .get(&tp.file_id)
                            .is_some_and(|t| t.front_cover().is_some())
                    })
                    .map(|tp| format!("track:{}", tp.source))
                    .unwrap_or_else(|| dir.to_owned())
            } else {
                dir.to_owned()
            };
            // One album of several in a folder is known by what its files say (album and
            // artist, as found), never by where your rules put it, so renaming folders can't
            // undo an answer.
            let identity = tps
                .first()
                .map(|tp| {
                    let found = TagSet(tp.tags_before.clone());
                    format!(
                        "{}~{}",
                        name_key(found.first("ALBUM").unwrap_or_default()),
                        name_key(
                            found
                                .first("ALBUMARTIST")
                                .or(found.first("ARTIST"))
                                .unwrap_or_default()
                        )
                    )
                })
                .unwrap_or_default();
            AlbumPlan {
                id: if split {
                    format!("{dir}#{identity}")
                } else {
                    dir.to_owned()
                },
                legacy_id: split.then(|| format!("{dir}#{target_dir}")),
                source_dir: dir.to_owned(),
                cover,
                waiting_on: vec![],
                year_conflict: year_conflict.clone(),
                year_auto: year_auto.clone(),
                title,
                album_artist,
                target_dir,
                action: action.clone(),
                confidence,
                notes: notes.clone(),
                changes,
                art: art.clone(),
                cover_from: if main { cover_from.clone() } else { None },
                cover_url: if main { cover_url.clone() } else { None },
                tracks: tps,
            }
        })
        .collect()
}

/// Whether two album titles are the same once catalogue suffixes are ignored.
fn title_key_eq(a: &str, b: &str) -> bool {
    let k = |s: &str| {
        let s = s.to_lowercase();
        let s = s
            .split(" (")
            .next()
            .unwrap_or(&s)
            .split(" - ")
            .next()
            .unwrap_or(&s)
            .to_owned();
        name_key(&s)
    };
    k(a) == k(b)
}

/// A category from the catalogues' genres, used when no rule of yours matches. Apple files
/// Hindi film music as "Bollywood" and has per-language Indian genres; MusicBrainz marks
/// soundtracks.
pub fn infer_category(
    meta: Option<&crate::enrich::AlbumMeta>,
) -> Option<crate::rules::CategoryRule> {
    let m = meta?;
    let genres: Vec<String> = [&m.apple, &m.deezer, &m.mb]
        .iter()
        .filter_map(|r| r.as_ref())
        .flat_map(|r| r.genres.iter().map(|g| g.to_lowercase()))
        .collect();
    let has = |words: &[&str]| genres.iter().any(|g| words.iter().any(|w| g.contains(w)));
    let rule = |category: &str,
                language: Option<&str>,
                genre: Option<&str>,
                soundtrack: bool,
                sub: Option<&str>| {
        Some(crate::rules::CategoryRule {
            pattern: String::new(),
            category: category.to_owned(),
            language: language.map(str::to_owned),
            genre: genre.map(str::to_owned),
            soundtrack,
            subfolder: sub.map(str::to_owned),
        })
    };
    for (word, lang) in [
        ("bollywood", "hin"),
        ("filmi", "hin"),
        ("tamil", "tam"),
        ("telugu", "tel"),
        ("malayalam", "mal"),
        ("kannada", "kan"),
        ("marathi", "mar"),
        ("bengali", "ben"),
    ] {
        if has(&[word]) {
            return rule("Film & Series", Some(lang), Some("Filmi"), true, None);
        }
    }
    if has(&["punjabi"]) {
        return rule("Independent", Some("pan"), Some("Punjabi Pop"), false, None);
    }
    if has(&["ghazal", "sufi", "qawwali"]) {
        return rule("Independent", Some("urd"), Some("Ghazal"), false, None);
    }
    if has(&["indian pop", "indie pop", "devotional"]) {
        return rule("Independent", Some("hin"), Some("Indie Pop"), false, None);
    }
    if has(&["video game", "game"]) {
        return rule("Scores", None, Some("Game Score"), false, Some("Games"));
    }
    if has(&["anime"]) {
        return rule("International", Some("jpn"), Some("Anime"), false, None);
    }
    if has(&["j-pop"]) {
        return rule("International", Some("jpn"), Some("J-Pop"), false, None);
    }
    if has(&["k-pop"]) {
        return rule("International", Some("kor"), Some("K-Pop"), false, None);
    }
    if has(&["c-pop", "mandopop", "cantopop"]) {
        return rule("International", Some("zho"), Some("C-Pop"), false, None);
    }
    if has(&["soundtrack", "film score"]) {
        return rule("Scores", None, Some("Film Score"), false, Some("Film"));
    }
    if has(&[
        "rock",
        "pop",
        "alternative",
        "electronic",
        "jazz",
        "hip-hop",
        "r&b",
        "classical",
        "folk",
    ]) {
        return rule("International", Some("eng"), None, false, None);
    }
    None
}

/// Find a track's entry in a looked-up release: ISRC first, then disc/track position when the
/// track counts agree, then an exact title match.
fn match_track<'a>(
    t: &Track,
    r: &'a crate::providers::ReleaseInfo,
    album_tracks: usize,
) -> Option<&'a crate::providers::TrackInfo> {
    if let Some(isrc) = t.first("ISRC")
        && let Some(hit) = r
            .tracks
            .iter()
            .find(|o| o.isrc.iter().any(|i| i.eq_ignore_ascii_case(isrc)))
    {
        return Some(hit);
    }
    let num = |k: &str| t.first(k).and_then(parse_num);
    if r.tracks.len() == album_tracks
        && let Some(n) = num("TRACKNUMBER")
    {
        let d = num("DISCNUMBER").unwrap_or(1);
        if let Some(hit) = r.tracks.iter().find(|o| o.number == n && o.disc == d) {
            return Some(hit);
        }
    }
    let key = name_key(t.first("TITLE")?);
    r.tracks.iter().find(|o| name_key(&o.title) == key)
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

pub(crate) fn keep_score(dir: &str) -> i32 {
    let mut s = 0;
    if !dir.contains("Various_Artists") && !dir.contains("06_Collections") {
        s += 2;
    }
    if !dir.contains(' ') {
        s += 1; // the owner's own underscore naming
    }
    s
}

/// A date's year: its first four characters, when they are four digits ("1999-05-12" is
/// 1999; "05/12/1999" has no year this way, rather than "05/1").
fn year_of(date: &str) -> Option<&str> {
    date.get(..4)
        .filter(|y| y.bytes().all(|b| b.is_ascii_digit()))
}

/// Two source tracks must never land on the same target: number the later ones, until the
/// name is free (a numbered name can itself be taken).
fn resolve_collisions(plan: &mut Plan) {
    let mut seen: HashSet<String> = HashSet::new();
    for album in plan
        .albums
        .iter_mut()
        .filter(|a| matches!(a.action, AlbumAction::Migrate))
    {
        for t in &mut album.tracks {
            if seen.insert(t.target.to_lowercase()) {
                continue;
            }
            {
                let (stem, ext) = t
                    .target
                    .rsplit_once('.')
                    .map(|(s, e)| (s.to_owned(), e.to_owned()))
                    .unwrap_or_else(|| (t.target.clone(), String::new()));
                let mut n = 2;
                while !seen.insert(format!("{stem} ({n}).{ext}").to_lowercase()) {
                    n += 1;
                }
                t.target = format!("{stem} ({n}).{ext}");
                album.notes.push(format!(
                    "Another album already uses “{stem}”; numbered to keep both."
                ));
                album.confidence = album.confidence.min(Confidence::Low);
            }
        }
    }
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
    // A missing year must not leave " - Album" or "Album ()".
    let year_album = |s: &str| {
        s.trim_start_matches(" - ")
            .replace(" ()", "")
            .replace("()", "")
            .trim()
            .to_owned()
    };
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
    // Only a year that is a year: anything else ("05/1") would make folders of its own.
    out = out.replace(
        "{year}",
        if f.year.len() == 4 && f.year.bytes().all(|b| b.is_ascii_digit()) {
            f.year
        } else {
            ""
        },
    );
    // Empty segments collapse.
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
    // Uploads often name the album "Title | Artist Official": the part after "|" is not
    // the album's name.
    let mut a = album.split(" | ").next().unwrap_or(album).trim().to_owned();
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
pub(crate) fn same_lyrics(a: &str, b: &str) -> bool {
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

/// The year an ISRC was issued ("IN-O13-13-01169" → 2013).
pub(crate) fn isrc_year(isrc: &str) -> Option<String> {
    let code: String = isrc.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if code.len() != 12 {
        return None;
    }
    let yy: u32 = code.get(5..7)?.parse().ok()?;
    let this_year = 1970
        + (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            / 31_556_952) as u32;
    let full = if 2000 + yy <= this_year {
        2000 + yy
    } else {
        1900 + yy
    };
    Some(full.to_string())
}

fn parse_num(s: &str) -> Option<u32> {
    s.split('/').next()?.trim().parse().ok()
}

/// Split "A, B & C" into names, keeping duo names like "Vishal-Shekhar" whole.
pub fn split_names(s: &str) -> Vec<String> {
    s.split([',', ';'])
        // "A/ B" and "A / B" are two names; a short "AC/DC" is one.
        .flat_map(|p| {
            let parts: Vec<&str> = p.split('/').collect();
            if parts.len() > 1 && parts.iter().all(|x| x.trim().chars().count() > 2) {
                parts
            } else {
                vec![p]
            }
        })
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

/// Names written more than one way (capitals, spacing, punctuation) and not covered by an
/// alias get one spelling: the most common, preferring one that isn't all lower case.
/// Returns the rules with those spellings added, and each name's spellings (chosen first).
fn unify_spellings(lib: &Library, rules: &Rules) -> (Rules, Vec<Vec<String>>) {
    let mut seen: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for t in &lib.tracks {
        for key in ["ALBUMARTIST", "ARTIST"] {
            for v in t.tags.get(key).into_iter().flatten() {
                for n in split_names(v) {
                    *seen.entry(name_key(&n)).or_default().entry(n).or_default() += 1;
                }
            }
        }
    }
    let mut out = rules.clone();
    let mut variants = Vec::new();
    for (key, spellings) in seen {
        if spellings.len() < 2 || rules.canonical(&key).is_some() || key.is_empty() {
            continue;
        }
        let mut list: Vec<(String, usize)> = spellings.into_iter().collect();
        list.sort_by_key(|(s, n)| {
            (
                std::cmp::Reverse(s.chars().any(char::is_uppercase)),
                std::cmp::Reverse(*n),
                s.clone(),
            )
        });
        out.add_spelling(&list[0].0);
        variants.push(list.into_iter().map(|(s, _)| s).collect());
    }
    variants.sort();
    (out, variants)
}

/// Plan a library with the owner's answers, comparing likely duplicates by decoding them
/// first when they haven't been compared yet. Returns the library the plan was built from.
pub fn build_checked(
    index: &crate::Index,
    root: &std::path::Path,
    rules: &Rules,
) -> crate::Result<(Library, Plan)> {
    let mut answers = crate::settings::load_answers(root);
    let mut lib = Library::load(index)?;
    lib.in_library = crate::paths::home_of(root).is_some();
    // Answers saved under ids that named a destination folder move to the new ids, once, so
    // a later change of folder names keeps them.
    let first = build_with(&lib, rules, &answers);
    let mut upgraded = false;
    for a in &first.albums {
        if let Some(old) = &a.legacy_id
            && let Some(v) = answers.get(&format!("dup:{old}")).cloned()
            && !answers.contains_key(&format!("dup:{}", a.id))
        {
            answers.insert(format!("dup:{}", a.id), v);
            upgraded = true;
        }
    }
    if upgraded {
        let _ = crate::settings::save_answers(root, &answers);
    }
    let mut plan = build_with(&lib, rules, &answers);
    if !plan.pending_pairs.is_empty() {
        crate::compare::run(index, root, &plan.pending_pairs)?;
        lib.pairs = crate::compare::load(index)?;
        plan = build_with(&lib, rules, &answers);
    }
    Ok((lib, plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app shows a typed playlist name as it will be saved (app/src/lib/playlistName.ts);
    /// this table, which its test reads too, keeps the two rules the same.
    #[test]
    fn playlist_names_are_saved_as_the_app_says() {
        let table = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../app/tests/fixtures/playlist-names.json"),
        )
        .expect("the shared table");
        let v: serde_json::Value = serde_json::from_str(&table).unwrap();
        for case in v["cases"].as_array().unwrap() {
            let typed = case[0].as_str().unwrap();
            let saved = case[1].as_str().unwrap();
            assert_eq!(sanitize(typed, 100), saved, "{typed:?}");
        }
    }

    fn fields<'a>(year: &'a str, album: &'a str) -> PathFields<'a> {
        PathFields {
            category: "Film & Series",
            subfolder: "",
            language: "Hindi",
            albumartist: "A.R. Rahman",
            year,
            album,
            disc: String::new(),
            track: "01".into(),
            title: "Beat Of Passion",
            ext: "flac",
        }
    }

    #[test]
    fn album_folders_are_named_then_dated() {
        let rules = Rules::default();
        assert_eq!(
            render_path(&rules, &fields("1999", "Taal")),
            "Film & Series/Hindi/A.R. Rahman/Taal (1999)/01 - Beat Of Passion.flac"
        );
        // No year: no empty brackets.
        assert_eq!(
            render_path(&rules, &fields("", "Taal")),
            "Film & Series/Hindi/A.R. Rahman/Taal/01 - Beat Of Passion.flac"
        );
        // The older year-first template still works for anyone who keeps it.
        let mut old = Rules::default();
        old.layout.template =
            "{category}/{albumartist}/{year} - {album}/{disc}{track} - {title}".into();
        assert_eq!(
            render_path(&old, &fields("", "Taal")),
            "Film & Series/A.R. Rahman/Taal/01 - Beat Of Passion.flac"
        );
    }

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
        assert_eq!(
            split_names("Yashal Shahid/ Atif Ali"),
            vec!["Yashal Shahid", "Atif Ali"]
        );
        assert_eq!(split_names("AC/DC").len(), 1); // not split
    }

    #[test]
    fn isrc_years() {
        assert_eq!(isrc_year("INO131301169").as_deref(), Some("2013"));
        assert_eq!(isrc_year("QZ-Z7W-25-53774").as_deref(), Some("2025"));
        assert_eq!(isrc_year("USRC17607839").as_deref(), Some("1976"));
        assert_eq!(isrc_year("bad"), None);
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
            clean_album("Love Songs | The Local Music Train Official", &rules),
            "Love Songs"
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
    fn categories_from_catalogue_genres() {
        use crate::enrich::AlbumMeta;
        use crate::providers::{ReleaseInfo, Source};
        let with = |genres: &[&str]| AlbumMeta {
            apple: Some(ReleaseInfo {
                source: Source::Apple,
                id: String::new(),
                title: String::new(),
                album_artist: None,
                first_release: None,
                release_date: None,
                label: None,
                genres: genres.iter().map(|g| (*g).to_owned()).collect(),
                release_types: vec![],
                cover_url: None,
                cover_px: None,
                tracks: vec![],
            }),
            ..Default::default()
        };
        let c = infer_category(Some(&with(&["Bollywood"]))).unwrap();
        assert_eq!(
            (c.category.as_str(), c.language.as_deref(), c.soundtrack),
            ("Film & Series", Some("hin"), true)
        );
        assert_eq!(
            infer_category(Some(&with(&["Tamil"])))
                .unwrap()
                .language
                .as_deref(),
            Some("tam")
        );
        assert_eq!(
            infer_category(Some(&with(&["Soundtrack"])))
                .unwrap()
                .subfolder
                .as_deref(),
            Some("Film")
        );
        assert!(infer_category(Some(&with(&["Spoken Word"]))).is_none());
        assert!(infer_category(None).is_none());
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
