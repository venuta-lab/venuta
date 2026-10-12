//! Metadata sources, each normalised into the same shape. Lookups are by identifiers the
//! files already carry (barcode first, then ISRC within the release), never by guessing.
//!
//! * MusicBrainz — original release year, label, release type, per-track composer and
//!   lyricist (through work relations). CC0 data.
//! * Apple (iTunes Search) — canonical album title, artwork up to 3000 px.
//! * Deezer — label, genres, release date, artwork up to 1400 px.
//! * LRCLIB — synced lyrics, validated before use.

use crate::net::{Http, NetError, enc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    MusicBrainz,
    Apple,
    Deezer,
    Lrclib,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::MusicBrainz => "MusicBrainz",
            Source::Apple => "Apple Music catalogue",
            Source::Deezer => "Deezer",
            Source::Lrclib => "LRCLIB",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackInfo {
    pub disc: u32,
    pub number: u32,
    pub title: String,
    pub isrc: Vec<String>,
    pub artists: Vec<String>,
    pub composers: Vec<String>,
    pub lyricists: Vec<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub album_artist: Option<String>,
    /// Earliest release of this album anywhere (MusicBrainz release group).
    pub first_release: Option<String>,
    pub release_date: Option<String>,
    pub label: Option<String>,
    pub genres: Vec<String>,
    pub release_types: Vec<String>,
    pub cover_url: Option<String>,
    pub cover_px: Option<u32>,
    pub tracks: Vec<TrackInfo>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
}

fn artist_credit(v: &Value) -> Option<String> {
    let parts = v.get("artist-credit")?.as_array()?;
    let mut out = String::new();
    for p in parts {
        out.push_str(&s(p, "name").or_else(|| p.get("artist").and_then(|a| s(a, "name")))?);
        out.push_str(p.get("joinphrase").and_then(Value::as_str).unwrap_or(""));
    }
    Some(out.trim().to_owned()).filter(|x| !x.is_empty())
}

// --- MusicBrainz ---------------------------------------------------------------------------

pub fn musicbrainz_by_barcode(http: &Http, barcode: &str) -> Result<Option<ReleaseInfo>, NetError> {
    let search = http.get_json(&format!(
        "https://musicbrainz.org/ws/2/release/?query=barcode:{}&fmt=json&limit=5",
        enc(barcode)
    ))?;
    let Some(id) = search
        .get("releases")
        .and_then(Value::as_array)
        .and_then(|rs| {
            // Exact barcode, preferring the Indian release, then worldwide, then anything.
            let exact: Vec<&Value> = rs
                .iter()
                .filter(|r| s(r, "barcode").as_deref() == Some(barcode))
                .collect();
            let pick = |c: &str| {
                exact
                    .iter()
                    .find(|r| s(r, "country").as_deref() == Some(c))
                    .copied()
            };
            pick("IN")
                .or_else(|| pick("XW"))
                .or_else(|| exact.first().copied())
        })
        .and_then(|r| s(r, "id"))
    else {
        return Ok(None);
    };
    musicbrainz_release(http, &id).map(Some)
}

/// For albums without a barcode: search by title and artist, and accept only a near-certain
/// match (search score ≥ 95 and the same number of tracks). No match beats a wrong one.
pub fn musicbrainz_by_title(
    http: &Http,
    title: &str,
    artist: &str,
    track_count: usize,
) -> Result<Option<ReleaseInfo>, NetError> {
    let q = format!(
        "release:\"{}\" AND artist:\"{}\"",
        title.replace('"', ""),
        artist.replace('"', "")
    );
    let search = http.get_json(&format!(
        "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit=5",
        enc(&q)
    ))?;
    let id = search
        .get("releases")
        .and_then(Value::as_array)
        .and_then(|rs| {
            rs.iter()
                .filter(|r| r.get("score").and_then(Value::as_u64).unwrap_or(0) >= 95)
                .find(|r| r.get("track-count").and_then(Value::as_u64) == Some(track_count as u64))
                .and_then(|r| s(r, "id"))
        });
    match id {
        Some(id) => musicbrainz_release(http, &id).map(Some),
        None => Ok(None),
    }
}

pub fn musicbrainz_release(http: &Http, id: &str) -> Result<ReleaseInfo, NetError> {
    let r = http.get_json(&format!(
        "https://musicbrainz.org/ws/2/release/{id}?inc=recordings+isrcs+artist-credits+release-groups+labels+genres+recording-level-rels+work-rels+work-level-rels+artist-rels&fmt=json"
    ))?;
    let rg = r.get("release-group").cloned().unwrap_or(Value::Null);
    let mut types: Vec<String> = Vec::new();
    if let Some(p) = s(&rg, "primary-type") {
        types.push(p.to_lowercase());
    }
    for t in rg
        .get("secondary-types")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(t) = t.as_str() {
            types.push(t.to_lowercase());
        }
    }
    let label = r
        .get("label-info")
        .and_then(Value::as_array)
        .and_then(|l| {
            l.iter()
                .find_map(|li| li.get("label").and_then(|x| s(x, "name")))
        })
        .map(|l| l.replace('\u{2010}', "-"));
    let genres = rg
        .get("genres")
        .or_else(|| r.get("genres"))
        .and_then(Value::as_array)
        .map(|g| g.iter().filter_map(|x| s(x, "name")).collect())
        .unwrap_or_default();
    let mut tracks = Vec::new();
    for (mi, medium) in r
        .get("media")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let disc = medium
            .get("position")
            .and_then(Value::as_u64)
            .unwrap_or(mi as u64 + 1) as u32;
        for t in medium
            .get("tracks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let rec = t.get("recording").cloned().unwrap_or(Value::Null);
            let mut composers = Vec::new();
            let mut lyricists = Vec::new();
            for rel in rec
                .get("relations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(work) = rel.get("work") else {
                    continue;
                };
                for wr in work
                    .get("relations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = wr.get("artist").and_then(|a| s(a, "name")) else {
                        continue;
                    };
                    match wr.get("type").and_then(Value::as_str) {
                        Some("composer") | Some("writer") => composers.push(name),
                        Some("lyricist") => lyricists.push(name),
                        _ => {}
                    }
                }
            }
            composers.dedup();
            lyricists.dedup();
            tracks.push(TrackInfo {
                disc,
                number: t.get("position").and_then(Value::as_u64).unwrap_or(0) as u32,
                title: s(t, "title")
                    .or_else(|| s(&rec, "title"))
                    .unwrap_or_default(),
                isrc: rec
                    .get("isrcs")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .filter_map(|x| x.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                artists: artist_credit(t)
                    .or_else(|| artist_credit(&rec))
                    .into_iter()
                    .collect(),
                composers,
                lyricists,
                duration_ms: t.get("length").and_then(Value::as_u64),
            });
        }
    }
    Ok(ReleaseInfo {
        source: Source::MusicBrainz,
        id: id.to_owned(),
        title: s(&r, "title").unwrap_or_default(),
        album_artist: artist_credit(&r),
        first_release: s(&rg, "first-release-date"),
        release_date: s(&r, "date"),
        label,
        genres,
        release_types: types,
        cover_url: None,
        cover_px: None,
        tracks,
    })
}

// --- Apple (iTunes Search API) ------------------------------------------------------------

pub fn apple_by_upc(http: &Http, upc: &str) -> Result<Option<ReleaseInfo>, NetError> {
    for country in ["in", "us"] {
        let v = http.get_json(&format!(
            "https://itunes.apple.com/lookup?upc={}&entity=song&country={country}",
            enc(upc)
        ))?;
        let results = v
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some(album) = results
            .iter()
            .find(|r| s(r, "wrapperType").as_deref() == Some("collection"))
        else {
            continue;
        };
        let art = s(album, "artworkUrl100").map(|u| u.replace("100x100bb", "3000x3000bb"));
        let label = s(album, "copyright").map(|c| {
            // "℗ 2011 Super Cassettes Industries Private Limited" -> the company.
            let c = c.trim_start_matches(['℗', '©', ' ']);
            let c = c
                .split_once(' ')
                .filter(|(y, _)| y.len() == 4 && y.chars().all(|x| x.is_ascii_digit()))
                .map(|x| x.1)
                .unwrap_or(c);
            c.trim().to_owned()
        });
        let tracks = results
            .iter()
            .filter(|r| s(r, "wrapperType").as_deref() == Some("track"))
            .map(|t| TrackInfo {
                disc: t.get("discNumber").and_then(Value::as_u64).unwrap_or(1) as u32,
                number: t.get("trackNumber").and_then(Value::as_u64).unwrap_or(0) as u32,
                title: s(t, "trackName").unwrap_or_default(),
                artists: s(t, "artistName").into_iter().collect(),
                duration_ms: t.get("trackTimeMillis").and_then(Value::as_u64),
                ..Default::default()
            })
            .collect();
        return Ok(Some(ReleaseInfo {
            source: Source::Apple,
            id: album
                .get("collectionId")
                .map(|x| x.to_string())
                .unwrap_or_default(),
            title: s(album, "collectionName").unwrap_or_default(),
            album_artist: s(album, "artistName"),
            first_release: None,
            release_date: s(album, "releaseDate").map(|d| d.chars().take(10).collect()),
            label,
            genres: s(album, "primaryGenreName").into_iter().collect(),
            release_types: Vec::new(),
            cover_url: art,
            cover_px: Some(3000), // Apple serves the largest it has, up to this
            tracks,
        }));
    }
    Ok(None)
}

// --- Deezer ------------------------------------------------------------------------------

pub fn deezer_by_upc(http: &Http, upc: &str) -> Result<Option<ReleaseInfo>, NetError> {
    let v = match http.get_json(&format!("https://api.deezer.com/album/upc:{}", enc(upc))) {
        Ok(v) => v,
        Err(NetError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    if v.get("error").is_some() || v.get("id").is_none() {
        return Ok(None);
    }
    let tracks = v
        .get("tracks")
        .and_then(|t| t.get("data"))
        .and_then(Value::as_array)
        .map(|ts| {
            ts.iter()
                .enumerate()
                .map(|(i, t)| TrackInfo {
                    disc: t.get("disk_number").and_then(Value::as_u64).unwrap_or(1) as u32,
                    number: t
                        .get("track_position")
                        .and_then(Value::as_u64)
                        .unwrap_or(i as u64 + 1) as u32,
                    title: s(t, "title").unwrap_or_default(),
                    artists: t
                        .get("artist")
                        .and_then(|a| s(a, "name"))
                        .into_iter()
                        .collect(),
                    duration_ms: t.get("duration").and_then(Value::as_u64).map(|d| d * 1000),
                    ..Default::default()
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(ReleaseInfo {
        source: Source::Deezer,
        id: v.get("id").map(|x| x.to_string()).unwrap_or_default(),
        title: s(&v, "title").unwrap_or_default(),
        album_artist: v.get("artist").and_then(|a| s(a, "name")),
        first_release: None,
        release_date: s(&v, "release_date"),
        label: s(&v, "label"),
        genres: v
            .get("genres")
            .and_then(|g| g.get("data"))
            .and_then(Value::as_array)
            .map(|g| g.iter().filter_map(|x| s(x, "name")).collect())
            .unwrap_or_default(),
        release_types: s(&v, "record_type").into_iter().collect(),
        // Deezer's "xl" is 1000 px; 1400 is the largest it serves.
        cover_url: s(&v, "cover_xl").map(|u| u.replace("1000x1000", "1400x1400")),
        cover_px: Some(1400),
        tracks,
    }))
}

// --- LRCLIB ------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricsFound {
    pub id: i64,
    pub synced: String,
    pub script: Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Script {
    Latin,
    Devanagari,
    Arabic,
    Tamil,
    Malayalam,
    Other,
}

pub fn script_of(text: &str) -> Script {
    let (mut lat, mut dev, mut ara, mut tam, mut mal) = (0, 0, 0, 0, 0);
    for c in text.chars() {
        match c as u32 {
            0x41..=0x5A | 0x61..=0x7A => lat += 1,
            0x0900..=0x097F => dev += 1,
            0x0600..=0x06FF => ara += 1,
            0x0B80..=0x0BFF => tam += 1,
            0x0D00..=0x0D7F => mal += 1,
            _ => {}
        }
    }
    let max = *[lat, dev, ara, tam, mal].iter().max().unwrap_or(&0);
    if max == 0 {
        Script::Other
    } else if max == dev {
        Script::Devanagari
    } else if max == ara {
        Script::Arabic
    } else if max == tam {
        Script::Tamil
    } else if max == mal {
        Script::Malayalam
    } else {
        Script::Latin
    }
}

/// The script preferred for a language (decision D5: native script, romanized as fallback).
pub fn preferred_script(language: Option<&str>) -> Option<Script> {
    match language? {
        "hin" | "mar" => Some(Script::Devanagari),
        "urd" => Some(Script::Arabic),
        "tam" => Some(Script::Tamil),
        "mal" => Some(Script::Malayalam),
        "eng" => Some(Script::Latin),
        _ => None,
    }
}

/// Synced lyrics that pass validation, preferring the given script. `duration_s` must be the
/// real track length: LRCLIB matches within ±2 s and so do we.
pub fn lrclib_find(
    http: &Http,
    title: &str,
    artist: &str,
    album: &str,
    duration_s: u64,
    prefer: Option<Script>,
) -> Result<Option<LyricsFound>, NetError> {
    let mut candidates: Vec<Value> = Vec::new();
    let get = http.get_json(&format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={duration_s}",
        enc(title),
        enc(artist),
        enc(album)
    ));
    match get {
        Ok(v) => candidates.push(v),
        Err(NetError::NotFound) => {}
        Err(e) => return Err(e),
    }
    let search = http.get_json(&format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        enc(title),
        enc(artist)
    ));
    if let Ok(Value::Array(v)) = search {
        candidates.extend(v);
    }
    let valid: Vec<LyricsFound> = candidates
        .iter()
        .filter(|c| {
            c.get("duration")
                .and_then(Value::as_f64)
                .is_some_and(|d| (d - duration_s as f64).abs() <= 2.0)
        })
        .filter_map(|c| {
            let synced = s(c, "syncedLyrics")?;
            validate_lrc(&synced, duration_s).then(|| LyricsFound {
                id: c.get("id").and_then(Value::as_i64).unwrap_or(0),
                script: script_of(&synced),
                synced,
            })
        })
        .collect();
    let best = prefer
        .and_then(|p| valid.iter().find(|l| l.script == p))
        .or_else(|| valid.iter().find(|l| l.script == Script::Latin))
        .or_else(|| valid.first())
        .cloned();
    Ok(best)
}

/// Synced, in time order, ending within the track, enough lines, and not a translation.
pub fn validate_lrc(text: &str, duration_s: u64) -> bool {
    let mut last = -1.0f64;
    let mut lines = 0;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((ts, words)) = rest.split_once(']') else {
            continue;
        };
        let Some((m, sec)) = ts.split_once(':') else {
            continue;
        };
        let (Ok(m), Ok(sec)) = (m.trim().parse::<f64>(), sec.trim().parse::<f64>()) else {
            continue;
        };
        let t = m * 60.0 + sec;
        if t + 0.01 < last {
            return false; // out of order
        }
        last = t;
        if !words.trim().is_empty() {
            lines += 1;
        }
    }
    let lower = text.to_lowercase();
    lines >= 4
        && last <= duration_s as f64 + 5.0
        && !lower.contains("translation")
        && !lower.contains("[ar:") // metadata-only junk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_validation() {
        let good = "[00:10.00] a\n[00:12.00] b\n[00:15.00] c\n[00:20.00] d\n";
        assert!(validate_lrc(good, 30));
        assert!(!validate_lrc(good, 10)); // runs past the end
        assert!(!validate_lrc(
            "[00:20.00] a\n[00:10.00] b\n[00:21.00] c\n[00:22.00] d\n",
            30
        )); // out of order
        assert!(!validate_lrc("[00:10.00] a\n[00:11.00] b\n", 30)); // too short
        assert!(!validate_lrc(
            "[00:10.00] English translation\n[00:11.00] b\n[00:12.00] c\n[00:13.00] d\n",
            30
        ));
    }

    #[test]
    fn scripts() {
        assert_eq!(script_of("Phir se ud chala"), Script::Latin);
        assert_eq!(script_of("फिर से उड़ चला"), Script::Devanagari);
        assert_eq!(script_of("پسوڑی"), Script::Arabic);
        assert_eq!(preferred_script(Some("mar")), Some(Script::Devanagari));
    }
}
