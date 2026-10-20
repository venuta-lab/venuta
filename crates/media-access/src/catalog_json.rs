//! Reading what the catalog's search JSON says beyond what `innertube-rs` parses: the kind of
//! upload (official audio, music video, someone's upload), play and view counts, the album a
//! song is on, and the catalog's own top result. Pure functions over `serde_json::Value`, so
//! they're tested without the network or the live provider.
//!
//! Every field is a ranking hint only: anything unreadable is left out, never an error.

#![cfg_attr(not(feature = "live-provider"), allow(dead_code))]

use crate::types::RemoteTrack;
use serde_json::Value;
use std::collections::HashMap;

/// One song or video row of a search response, as far as it could be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawHit {
    pub video_id: String,
    /// "ATV", "OMV", "UGC", "PODCAST_EPISODE"… (the catalog's word without its prefix).
    pub video_type: Option<String>,
    pub plays: Option<u64>,
    pub album_id: Option<String>,
    pub album: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: Option<u64>,
    pub thumbnail: Option<String>,
    /// Its place among the response's song and video rows (0 = first).
    pub rank: u16,
}

/// "MUSIC_VIDEO_TYPE_ATV" → "ATV".
pub(crate) fn short_video_type(raw: &str) -> String {
    raw.strip_prefix("MUSIC_VIDEO_TYPE_")
        .unwrap_or(raw)
        .to_owned()
}

/// "966M plays", "1.7B plays", "308K views", "1,234 views" → a number. None for anything else.
pub(crate) fn parse_count(text: &str) -> Option<u64> {
    count_before(text, &["plays", "play", "views", "view"])
}

/// "540M monthly audience", "12M subscribers" → a number.
pub(crate) fn parse_audience(text: &str) -> Option<u64> {
    count_before(
        text,
        &[
            "monthly audience",
            "monthly listeners",
            "subscribers",
            "subscriber",
        ],
    )
}

fn count_before(text: &str, words: &[&str]) -> Option<u64> {
    let t = text.trim();
    let number = words.iter().find_map(|w| t.strip_suffix(w))?.trim();
    let (digits, times) = match number.chars().last()? {
        'K' | 'k' => (&number[..number.len() - 1], 1e3),
        'M' | 'm' => (&number[..number.len() - 1], 1e6),
        'B' | 'b' => (&number[..number.len() - 1], 1e9),
        _ => (number, 1.0),
    };
    let digits = digits.trim().replace(',', "");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let n: f64 = digits.parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| (n * times).round() as u64)
}

/// "4:07" → 247 000 ms; "1:02:03" too. None for anything else.
pub(crate) fn parse_duration(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let secs = parts
        .iter()
        .try_fold(0_u64, |acc, p| Some(acc * 60 + p.parse::<u64>().ok()?))?;
    Some(secs * 1000)
}

/// Every object in `v`, depth first, in document order.
fn walk<'a>(v: &'a Value, out: &mut dyn FnMut(&'a serde_json::Map<String, Value>)) {
    match v {
        Value::Object(map) => {
            out(map);
            for child in map.values() {
                walk(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, out);
            }
        }
        _ => {}
    }
}

/// The first value under `key` anywhere inside `v`.
fn first_string<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    let mut found = None;
    walk(v, &mut |m| {
        if found.is_none() {
            found = m.get(key).and_then(Value::as_str);
        }
    });
    found
}

/// What page a run links to ("MUSIC_PAGE_TYPE_ARTIST"…), and its browse id. A playlist row's
/// album column can link the album by its id (`MPRE…`) alone, with no page type beside it.
fn run_link(run: &Value) -> Option<(&str, &str)> {
    let browse = run.pointer("/navigationEndpoint/browseEndpoint")?;
    let id = browse.get("browseId").and_then(Value::as_str)?;
    let page = browse
        .pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str)
        .or_else(|| id.starts_with("MPRE").then_some("MUSIC_PAGE_TYPE_ALBUM"))?;
    Some((page, id))
}

fn runs_of(text: Option<&Value>) -> Vec<&Value> {
    text.and_then(|t| t.get("runs"))
        .and_then(Value::as_array)
        .map(|r| r.iter().collect())
        .unwrap_or_default()
}

fn joined(runs: &[&Value]) -> String {
    runs.iter()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .collect()
}

fn last_thumbnail(v: &Value) -> Option<String> {
    let mut url = None;
    walk(v, &mut |m| {
        if url.is_none()
            && let Some(list) = m.get("thumbnails").and_then(Value::as_array)
        {
            url = list
                .last()
                .and_then(|t| t.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    });
    url
}

/// Fill a hit's details from the text runs that describe it (artists, album, counts, length).
fn read_runs(hit: &mut RawHit, runs: &[&Value]) {
    for run in runs {
        let text = run.get("text").and_then(Value::as_str).unwrap_or_default();
        match run_link(run) {
            Some(("MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL", _)) => {
                hit.artists.push(text.to_owned())
            }
            Some(("MUSIC_PAGE_TYPE_ALBUM", id)) if hit.album_id.is_none() => {
                hit.album_id = Some(id.to_owned());
                hit.album = Some(text.to_owned());
            }
            _ => {
                if hit.plays.is_none() {
                    hit.plays = parse_count(text);
                }
                if hit.duration_ms.is_none() {
                    hit.duration_ms = parse_duration(text);
                }
            }
        }
    }
}

/// The song and video rows of a search response, in order (rows without a video id, such as
/// albums and artists, are skipped and don't take a place).
pub(crate) fn read_hits(raw: &Value) -> Vec<RawHit> {
    let mut rows = Vec::new();
    walk(raw, &mut |m| {
        if let Some(r) = m.get("musicResponsiveListItemRenderer") {
            rows.push(r);
        }
    });
    let mut hits = Vec::new();
    for r in rows {
        let video_id = r
            .pointer("/playlistItemData/videoId")
            .and_then(Value::as_str)
            .or_else(|| r.get("overlay").and_then(|o| first_string(o, "videoId")))
            .or_else(|| {
                r.get("flexColumns")
                    .and_then(|f| first_string(f, "videoId"))
            })
            .unwrap_or_default()
            .trim();
        if video_id.is_empty() {
            continue;
        }
        let columns: Vec<Vec<&Value>> = r
            .get("flexColumns")
            .and_then(Value::as_array)
            .map(|cols| {
                cols.iter()
                    .map(|c| runs_of(c.pointer("/musicResponsiveListItemFlexColumnRenderer/text")))
                    .collect()
            })
            .unwrap_or_default();
        let mut hit = RawHit {
            video_id: video_id.to_owned(),
            video_type: first_string(r, "musicVideoType").map(short_video_type),
            title: columns.first().map(|c| joined(c)).unwrap_or_default(),
            thumbnail: r.get("thumbnail").and_then(last_thumbnail),
            rank: u16::try_from(hits.len()).unwrap_or(u16::MAX),
            ..Default::default()
        };
        for col in columns.iter().skip(1) {
            read_runs(&mut hit, col);
        }
        // A song row's length sits in its fixed column ("4:18").
        let fixed: Vec<&Value> = r
            .get("fixedColumns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|c| runs_of(c.pointer("/musicResponsiveListItemFixedColumnRenderer/text")))
            .collect();
        read_runs(&mut hit, &fixed);
        hits.push(hit);
    }
    hits
}

/// The catalog's top result card, when it is a song or a video (an artist, an album or a
/// podcast episode is not a song to play).
pub(crate) fn read_top(raw: &Value) -> Option<RawHit> {
    let mut card = None;
    walk(raw, &mut |m| {
        if card.is_none() {
            card = m.get("musicCardShelfRenderer");
        }
    });
    let card = card?;
    let title_runs = runs_of(card.get("title"));
    let watch = title_runs
        .first()
        .and_then(|r| r.pointer("/navigationEndpoint/watchEndpoint"))
        .or_else(|| {
            card.pointer("/thumbnailOverlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint")
        })?;
    let video_id = watch.get("videoId").and_then(Value::as_str)?.trim();
    if video_id.is_empty() {
        return None;
    }
    let video_type = first_string(watch, "musicVideoType").map(short_video_type);
    let subtitle = runs_of(card.get("subtitle"));
    let kind = subtitle
        .first()
        .and_then(|r| r.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let songlike = matches!(kind, "Song" | "Video")
        || matches!(video_type.as_deref(), Some("ATV" | "OMV" | "UGC"));
    if !songlike || video_type.as_deref() == Some("PODCAST_EPISODE") || kind == "Episode" {
        return None;
    }
    let mut hit = RawHit {
        video_id: video_id.to_owned(),
        video_type,
        title: joined(&title_runs),
        thumbnail: card.get("thumbnail").and_then(last_thumbnail),
        ..Default::default()
    };
    read_runs(&mut hit, &subtitle);
    Some(hit)
}

/// An album row of a search response ("Kesariya" / "Single • Arijit Singh • 2022").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawAlbum {
    pub browse_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub year: Option<String>,
    /// "Album", "Single", "EP"…
    pub kind: Option<String>,
    pub thumbnail: Option<String>,
}

/// An artist row ("Pritam" / "Artist • 540M monthly audience").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawArtist {
    pub browse_id: String,
    pub name: String,
    pub audience: Option<u64>,
    pub thumbnail: Option<String>,
}

/// The rows of a response that open a page of this type, with their title and other runs.
fn page_rows<'a>(raw: &'a Value, page: &str) -> Vec<(&'a Value, &'a str, String, Vec<&'a Value>)> {
    let mut rows = Vec::new();
    walk(raw, &mut |m| {
        let Some(r) = m.get("musicResponsiveListItemRenderer") else {
            return;
        };
        let Some((kind, id)) = run_link(r) else {
            return;
        };
        if kind != page || id.trim().is_empty() {
            return;
        }
        let mut columns = r
            .get("flexColumns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| runs_of(c.pointer("/musicResponsiveListItemFlexColumnRenderer/text")));
        let title = columns.next().map(|c| joined(&c)).unwrap_or_default();
        rows.push((r, id.trim(), title, columns.flatten().collect()));
    });
    rows
}

/// The album rows of a search response, in order.
pub(crate) fn read_albums(raw: &Value) -> Vec<RawAlbum> {
    page_rows(raw, "MUSIC_PAGE_TYPE_ALBUM")
        .into_iter()
        .map(|(r, id, title, runs)| {
            let texts: Vec<&str> = runs
                .iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .map(str::trim)
                .filter(|t| !t.is_empty() && *t != "•")
                .collect();
            RawAlbum {
                browse_id: id.to_owned(),
                title,
                artists: runs
                    .iter()
                    .filter(|r| matches!(run_link(r), Some(("MUSIC_PAGE_TYPE_ARTIST", _))))
                    .filter_map(|r| r.get("text").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect(),
                year: texts
                    .iter()
                    .find(|t| t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()))
                    .map(|t| (*t).to_owned()),
                kind: texts
                    .first()
                    .filter(|t| {
                        matches!(
                            **t,
                            "Album" | "Single" | "EP" | "Live" | "Compilation" | "Soundtrack"
                        )
                    })
                    .map(|t| (*t).to_owned()),
                thumbnail: r.get("thumbnail").and_then(last_thumbnail),
            }
        })
        .collect()
}

/// A card in a carousel ("Albums", "Singles & EPs", "Fans might also like"): an album or an
/// artist, with its title, the page it opens, its picture and the line under it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawCard {
    pub browse_id: String,
    pub page_type: String,
    pub title: String,
    /// The words under it ("Single • 2026", "288M monthly audience").
    pub subtitle: Vec<String>,
    pub thumbnail: Option<String>,
}

impl RawCard {
    /// A four-digit year in the line under it.
    pub fn year(&self) -> Option<String> {
        self.subtitle
            .iter()
            .map(|t| t.trim())
            .find(|t| t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_owned)
    }
    /// "Album", "Single", "EP"… when the line under it starts with one.
    pub fn kind(&self) -> Option<String> {
        self.subtitle
            .first()
            .map(|t| t.trim())
            .filter(|t| {
                matches!(
                    *t,
                    "Album" | "Single" | "EP" | "Live" | "Compilation" | "Soundtrack"
                )
            })
            .map(str::to_owned)
    }
    pub fn audience(&self) -> Option<u64> {
        self.subtitle.iter().find_map(|t| parse_audience(t))
    }
}

/// What an artist's page says, as far as it could be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawArtistPage {
    pub name: String,
    pub thumbnail: Option<String>,
    pub audience: Option<u64>,
    pub top_songs: Vec<RawHit>,
    /// The page that lists all their songs ("Top songs" → See all), when given.
    pub songs_list_id: Option<String>,
    pub albums: Vec<RawCard>,
    pub singles: Vec<RawCard>,
    pub similar: Vec<RawCard>,
}

fn text_of(v: Option<&Value>) -> String {
    let runs = runs_of(v);
    if runs.is_empty() {
        v.and_then(|t| t.get("simpleText"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    } else {
        joined(&runs)
    }
}

fn read_card(item: &Value) -> Option<RawCard> {
    let r = item.get("musicTwoRowItemRenderer")?;
    let browse = r.pointer("/navigationEndpoint/browseEndpoint")?;
    let browse_id = browse.get("browseId").and_then(Value::as_str)?.trim();
    if browse_id.is_empty() {
        return None;
    }
    Some(RawCard {
        browse_id: browse_id.to_owned(),
        page_type: browse
            .pointer(
                "/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
            )
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        title: text_of(r.get("title")),
        subtitle: runs_of(r.get("subtitle"))
            .iter()
            .filter_map(|x| x.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|t| !t.is_empty() && *t != "•")
            .map(str::to_owned)
            .collect(),
        thumbnail: r.get("thumbnailRenderer").and_then(last_thumbnail),
    })
}

/// An artist's page: the header (name, picture, audience), the top songs shelf (and where all
/// their songs are listed), and the carousels of albums, singles and similar artists. Shelves
/// are told apart by what their cards open (albums, artists), and singles by their title.
pub(crate) fn read_artist_page(raw: &Value) -> RawArtistPage {
    let mut page = RawArtistPage::default();
    if let Some(header) = raw
        .get("header")
        .and_then(Value::as_object)
        .and_then(|h| h.values().next())
    {
        page.name = text_of(header.get("title"));
        page.thumbnail = header
            .get("foregroundThumbnail")
            .or_else(|| header.get("thumbnail"))
            .and_then(last_thumbnail);
        page.audience =
            parse_audience(&text_of(header.get("monthlyListenerCount"))).or_else(|| {
                header
                    .pointer("/subscriptionButton/subscribeButtonRenderer/subscriberCountText")
                    .map(|t| text_of(Some(t)))
                    .and_then(|t| parse_audience(&format!("{t} subscribers")))
            });
    }
    walk(raw, &mut |m| {
        if let Some(shelf) = m.get("musicShelfRenderer")
            && page.top_songs.is_empty()
        {
            page.top_songs = read_hits(shelf);
            page.songs_list_id = shelf
                .pointer("/bottomEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if let Some(carousel) = m.get("musicCarouselShelfRenderer") {
            let title =
                text_of(carousel.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"));
            let cards: Vec<RawCard> = carousel
                .get("contents")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(read_card)
                .collect();
            let albums = cards
                .iter()
                .filter(|c| c.page_type == "MUSIC_PAGE_TYPE_ALBUM")
                .count();
            let artists = cards
                .iter()
                .filter(|c| c.page_type == "MUSIC_PAGE_TYPE_ARTIST")
                .count();
            if albums > 0 && albums * 2 >= cards.len() {
                let singles = title.contains("Single") || title.contains("EP");
                let into = if singles {
                    &mut page.singles
                } else {
                    &mut page.albums
                };
                if into.is_empty() {
                    *into = cards
                        .into_iter()
                        .filter(|c| c.page_type == "MUSIC_PAGE_TYPE_ALBUM")
                        .collect();
                }
            } else if artists > 0 && artists * 2 >= cards.len() && page.similar.is_empty() {
                page.similar = cards
                    .into_iter()
                    .filter(|c| c.page_type == "MUSIC_PAGE_TYPE_ARTIST")
                    .collect();
            }
        }
    });
    page
}

/// The artist rows of a search response, in order.
pub(crate) fn read_artists(raw: &Value) -> Vec<RawArtist> {
    page_rows(raw, "MUSIC_PAGE_TYPE_ARTIST")
        .into_iter()
        .map(|(r, id, name, runs)| RawArtist {
            browse_id: id.to_owned(),
            name,
            audience: runs
                .iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .find_map(parse_audience),
            thumbnail: r.get("thumbnail").and_then(last_thumbnail),
        })
        .collect()
}

/// Add what the raw responses say to the songs list, which stays in the catalog's order.
///
/// - From the Songs response itself: kind of upload, plays, album id when the parser missed it.
/// - From the unfiltered response (`mix`): each song's place there (`shelf_rank`), and the top
///   result marked `is_top`. A top result missing from the songs is added at the end (never
///   in front: the catalog's top result is sometimes a cover or a music video of one), and
///   only when it is the artist's own audio or an official music video.
pub(crate) fn enrich(
    tracks: &mut Vec<RemoteTrack>,
    songs: &[RawHit],
    mix: Option<(&[RawHit], Option<&RawHit>)>,
    provider: &str,
) {
    let by_id: HashMap<&str, &RawHit> = songs.iter().map(|h| (h.video_id.as_str(), h)).collect();
    let mixed: HashMap<&str, &RawHit> = mix
        .map(|(hits, _)| hits.iter().map(|h| (h.video_id.as_str(), h)).collect())
        .unwrap_or_default();
    let top = mix.and_then(|(_, top)| top);
    for t in tracks.iter_mut() {
        for hit in [
            by_id.get(t.track_id.as_str()),
            mixed.get(t.track_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            t.video_type = t.video_type.take().or_else(|| hit.video_type.clone());
            t.plays = t.plays.or(hit.plays);
            if t.artists.iter().all(|a| a.trim().is_empty()) && !hit.artists.is_empty() {
                t.artists = hit.artists.clone();
            }
            if t.album_id.is_none() {
                t.album_id = hit.album_id.clone();
                if t.album.is_none() {
                    t.album = hit.album.clone();
                }
            }
        }
        t.shelf_rank = mixed.get(t.track_id.as_str()).map(|h| h.rank);
        t.is_top = top.is_some_and(|h| h.video_id == t.track_id);
    }
    if let Some(top) = top
        && !tracks.iter().any(|t| t.track_id == top.video_id)
        && matches!(top.video_type.as_deref(), Some("ATV" | "OMV"))
        && !top.title.trim().is_empty()
    {
        tracks.push(RemoteTrack {
            provider: provider.to_owned(),
            track_id: top.video_id.clone(),
            title: top.title.clone(),
            artists: top.artists.clone(),
            album: top.album.clone(),
            album_id: top.album_id.clone(),
            duration_ms: top.duration_ms,
            artwork_url: top.thumbnail.clone(),
            video_type: top.video_type.clone(),
            plays: top.plays,
            is_top: true,
            shelf_rank: mixed.get(top.video_id.as_str()).map(|h| h.rank),
            ..Default::default()
        });
    }
}

/// A top result added at the end stays inside the first page of `limit` rows: it takes the
/// page's last place, and the catalog row that was there gives way (the page stays one page,
/// so the next one starts cleanly).
pub(crate) fn keep_top_in_page(tracks: &mut Vec<RemoteTrack>, limit: usize) {
    let limit = limit.max(1);
    if tracks.len() > limit
        && tracks.last().is_some_and(|t| t.is_top)
        && !tracks[..limit].iter().any(|t| t.is_top)
    {
        let top = tracks.pop().expect("checked");
        tracks.remove(limit - 1);
        tracks.insert(limit - 1, top);
    }
}

/// Where the next page of a search starts (the shelf's continuation), when the response has one.
pub(crate) fn read_continuation(raw: &Value) -> Option<String> {
    let mut found = None;
    walk(raw, &mut |m| {
        if found.is_none()
            && let Some(c) = m
                .get("nextContinuationData")
                .and_then(|n| n.get("continuation"))
                .and_then(Value::as_str)
                .filter(|c| !c.trim().is_empty())
        {
            found = Some(c.to_owned());
        }
    });
    found
}

/// The next page of a playlist: its song shelf's own continuation command. (The sections below a
/// playlist carry tokens of their own, which aren't its songs, so they're never followed.)
pub(crate) fn read_playlist_continuation(raw: &Value) -> Option<String> {
    let mut found = None;
    walk(raw, &mut |m| {
        if found.is_none()
            && let Some(t) = m
                .get("continuationItemRenderer")
                .and_then(|c| c.pointer("/continuationEndpoint/continuationCommand/token"))
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
        {
            found = Some(t.to_owned());
        }
    });
    found
}

/// A playlist page's name, from its header.
pub(crate) fn read_playlist_title(raw: &Value) -> Option<String> {
    let mut found = None;
    walk(raw, &mut |m| {
        if found.is_none()
            && let Some(h) = m
                .get("musicResponsiveHeaderRenderer")
                .or_else(|| m.get("musicDetailHeaderRenderer"))
                .or_else(|| m.get("musicEditablePlaylistDetailHeaderRenderer"))
        {
            let title = joined(&runs_of(h.get("title")));
            if !title.trim().is_empty() {
                found = Some(title.trim().to_owned());
            }
        }
    });
    found
}

/// Songs read straight from a response's rows (for a page innertube-rs doesn't parse, such as
/// a continuation page). Rows without a title are left out.
pub(crate) fn tracks_from_hits(hits: &[RawHit], provider: &str) -> Vec<RemoteTrack> {
    hits.iter()
        .filter(|h| !h.title.trim().is_empty())
        .map(|h| RemoteTrack {
            provider: provider.to_owned(),
            track_id: h.video_id.clone(),
            title: h.title.clone(),
            artists: h.artists.clone(),
            album: h.album.clone(),
            album_id: h.album_id.clone(),
            duration_ms: h.duration_ms,
            artwork_url: h.thumbnail.clone(),
            video_type: h.video_type.clone(),
            plays: h.plays,
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn link(text: &str, page: &str, id: &str) -> Value {
        json!({"text": text, "navigationEndpoint": {"browseEndpoint": {"browseId": id,
            "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": page}}}}})
    }

    fn row(id: &str, kind: &str, title: &str, second: Vec<Value>, third: Option<&str>) -> Value {
        let mut cols = vec![
            json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title,
                "navigationEndpoint": {"watchEndpoint": {"videoId": id,
                "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": kind}}}}}]}}}),
            json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": second}}}),
        ];
        if let Some(t) = third {
            cols.push(json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": t}]}}}));
        }
        json!({"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": id},
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://a/s.jpg"}, {"url": "https://a/l.jpg"}]}}},
            "flexColumns": cols}})
    }

    fn songs_response() -> Value {
        json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
            {"musicShelfRenderer": {"contents": [
                row("ATV1", "MUSIC_VIDEO_TYPE_ATV", "Kesariya", vec![link("Arijit Singh", "MUSIC_PAGE_TYPE_ARTIST", "UC1"), json!({"text": " • "}), link("Brahmastra", "MUSIC_PAGE_TYPE_ALBUM", "MPREb_1")], Some("966M plays")),
                row("ATV2", "MUSIC_VIDEO_TYPE_ATV", "Kesariya (Remix)", vec![link("Someone", "MUSIC_PAGE_TYPE_ARTIST", "UC2")], Some("2.5K plays")),
            ]}}
        ]}}}}]}}})
    }

    fn mixed_response(top: Value) -> Value {
        json!({"contents": {"sectionListRenderer": {"contents": [
            {"musicCardShelfRenderer": top},
            {"musicShelfRenderer": {"contents": [
                row("UGC1", "MUSIC_VIDEO_TYPE_UGC", "Kesariya cover", vec![json!({"text": "Video"}), json!({"text": " • "}), link("A Fan", "MUSIC_PAGE_TYPE_USER_CHANNEL", "UC9"), json!({"text": " • "}), json!({"text": "88M views"}), json!({"text": " • "}), json!({"text": "4:47"})], None),
                row("ATV1", "MUSIC_VIDEO_TYPE_ATV", "Kesariya", vec![json!({"text": "Song"}), json!({"text": " • "}), link("Arijit Singh", "MUSIC_PAGE_TYPE_ARTIST", "UC1")], None),
                link("Arijit Singh", "MUSIC_PAGE_TYPE_ARTIST", "UC1"),
            ]}}
        ]}}})
    }

    fn card(id: &str, kind: &str, label: &str, title: &str) -> Value {
        json!({
            "title": {"runs": [{"text": title, "navigationEndpoint": {"watchEndpoint": {"videoId": id,
                "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": kind}}}}}]},
            "subtitle": {"runs": [{"text": label}, {"text": " • "}, link("Oasis", "MUSIC_PAGE_TYPE_ARTIST", "UC5"), {"text": " • "}, {"text": "9.7M views"}, {"text": " • "}, {"text": "4:07"}]},
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://a/top.jpg"}]}}}
        })
    }

    fn track(id: &str) -> RemoteTrack {
        RemoteTrack {
            provider: "remote_catalog".into(),
            track_id: id.into(),
            title: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn counts_and_lengths_read_as_numbers() {
        assert_eq!(parse_count("966M plays"), Some(966_000_000));
        assert_eq!(parse_count("1.7B plays"), Some(1_700_000_000));
        assert_eq!(parse_count("2.5K plays"), Some(2_500));
        assert_eq!(parse_count("308K views"), Some(308_000));
        assert_eq!(parse_count("1,234 views"), Some(1_234));
        assert_eq!(parse_count("1 view"), Some(1));
        assert_eq!(parse_count("Arijit Singh"), None);
        assert_eq!(parse_count("No views"), None);
        assert_eq!(parse_count("plays"), None);
        assert_eq!(parse_duration("4:07"), Some(247_000));
        assert_eq!(parse_duration("1:02:03"), Some(3_723_000));
        assert_eq!(parse_duration("2022"), None);
        assert_eq!(parse_duration("4:0x"), None);
        assert_eq!(short_video_type("MUSIC_VIDEO_TYPE_ATV"), "ATV");
    }

    #[test]
    fn rows_give_kind_plays_album_and_place() {
        let hits = read_hits(&songs_response());
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].video_id, "ATV1");
        assert_eq!(hits[0].video_type.as_deref(), Some("ATV"));
        assert_eq!(hits[0].plays, Some(966_000_000));
        assert_eq!(hits[0].album_id.as_deref(), Some("MPREb_1"));
        assert_eq!(hits[0].artists, ["Arijit Singh"]);
        assert_eq!(hits[0].thumbnail.as_deref(), Some("https://a/l.jpg"));
        assert_eq!((hits[1].rank, hits[1].plays), (1, Some(2_500)));

        // The unfiltered mix: a video row with views and a length; an artist row takes no place.
        let mixed = read_hits(&mixed_response(json!({})));
        assert_eq!(mixed.len(), 2);
        assert_eq!(mixed[0].video_type.as_deref(), Some("UGC"));
        assert_eq!(
            (mixed[0].plays, mixed[0].duration_ms),
            (Some(88_000_000), Some(287_000))
        );
        assert_eq!(mixed[0].artists, ["A Fan"]);
        assert_eq!(mixed[1].rank, 1);
    }

    #[test]
    fn the_top_card_counts_only_when_it_is_a_song_or_video() {
        let video = read_top(&mixed_response(card(
            "OMV1",
            "MUSIC_VIDEO_TYPE_OMV",
            "Video",
            "Wonderwall",
        )))
        .unwrap();
        assert_eq!(video.video_id, "OMV1");
        assert_eq!(video.video_type.as_deref(), Some("OMV"));
        assert_eq!(
            (video.plays, video.duration_ms),
            (Some(9_700_000), Some(247_000))
        );
        assert_eq!(video.artists, ["Oasis"]);
        // A podcast episode, or an artist card (no song to play), is not a top song.
        assert!(
            read_top(&mixed_response(card(
                "EP1",
                "MUSIC_VIDEO_TYPE_PODCAST_EPISODE",
                "Episode",
                "Talk"
            )))
            .is_none()
        );
        let artist = json!({"title": {"runs": [link("Arijit Singh", "MUSIC_PAGE_TYPE_ARTIST", "UC1")]},
            "subtitle": {"runs": [{"text": "Artist"}]}});
        assert!(read_top(&mixed_response(artist)).is_none());
        assert!(read_top(&songs_response()).is_none());
    }

    #[test]
    fn songs_keep_their_order_and_gain_hints() {
        let songs = read_hits(&songs_response());
        let raw = mixed_response(card("ATV1", "MUSIC_VIDEO_TYPE_ATV", "Song", "Kesariya"));
        let (mix, top) = (read_hits(&raw), read_top(&raw));
        let mut tracks = vec![track("ATV1"), track("ATV2"), track("NEW")];
        enrich(
            &mut tracks,
            &songs,
            Some((&mix, top.as_ref())),
            "remote_catalog",
        );
        assert_eq!(
            tracks
                .iter()
                .map(|t| t.track_id.as_str())
                .collect::<Vec<_>>(),
            ["ATV1", "ATV2", "NEW"]
        );
        let a = &tracks[0];
        assert_eq!(
            (a.video_type.as_deref(), a.plays, a.album_id.as_deref()),
            (Some("ATV"), Some(966_000_000), Some("MPREb_1"))
        );
        assert_eq!((a.is_top, a.shelf_rank), (true, Some(1)));
        assert_eq!((tracks[1].is_top, tracks[1].shelf_rank), (false, None));
        // Nothing known about it: no hints, and no harm.
        assert_eq!(tracks[2], track("NEW"));
    }

    #[test]
    fn a_missing_top_song_is_added_last_and_a_cover_or_episode_never() {
        let songs = read_hits(&songs_response());
        // An official music video the songs list lacks: added at the end, marked top.
        let raw = mixed_response(card("OMV1", "MUSIC_VIDEO_TYPE_OMV", "Video", "Wonderwall"));
        let (mix, top) = (read_hits(&raw), read_top(&raw));
        let mut tracks = vec![track("ATV1")];
        enrich(
            &mut tracks,
            &songs,
            Some((&mix, top.as_ref())),
            "remote_catalog",
        );
        assert_eq!(tracks.len(), 2);
        let added = &tracks[1];
        assert_eq!(
            (added.track_id.as_str(), added.is_top, added.title.as_str()),
            ("OMV1", true, "Wonderwall")
        );
        assert_eq!(
            (added.artists.clone(), added.duration_ms),
            (vec!["Oasis".to_owned()], Some(247_000))
        );
        assert_eq!(added.artwork_url.as_deref(), Some("https://a/top.jpg"));

        // Someone's upload as the top result: marked if present, never added.
        let raw = mixed_response(card(
            "UGC7",
            "MUSIC_VIDEO_TYPE_UGC",
            "Video",
            "Wonderwall (cover)",
        ));
        let (mix, top) = (read_hits(&raw), read_top(&raw));
        let mut tracks = vec![track("ATV1")];
        enrich(
            &mut tracks,
            &songs,
            Some((&mix, top.as_ref())),
            "remote_catalog",
        );
        assert_eq!(tracks.len(), 1);
        assert!(!tracks[0].is_top);

        // No unfiltered response at all (it failed): the songs still get their own hints.
        let mut tracks = vec![track("ATV1")];
        enrich(&mut tracks, &songs, None, "remote_catalog");
        assert_eq!(
            (tracks[0].plays, tracks[0].shelf_rank, tracks[0].is_top),
            (Some(966_000_000), None, false)
        );
    }

    fn page_row(page: &str, id: &str, title: &str, second: Vec<Value>) -> Value {
        json!({"musicResponsiveListItemRenderer": {
        "navigationEndpoint": {"browseEndpoint": {"browseId": id,
            "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": page}}}},
        "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://a/s.jpg"}, {"url": "https://a/l.jpg"}]}}},
        "flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title}]}}},
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": second}}}
        ]}})
    }

    #[test]
    fn songs_without_artists_take_them_from_the_row_and_a_late_top_stays_in_page() {
        let songs = read_hits(&songs_response());
        let mut tracks = vec![RemoteTrack {
            artists: vec![" ".into()],
            ..track("ATV1")
        }];
        enrich(&mut tracks, &songs, None, "remote_catalog");
        assert_eq!(tracks[0].artists, ["Arijit Singh"]);

        let mut five: Vec<RemoteTrack> = ["a", "b", "c", "d", "top"]
            .iter()
            .map(|id| track(id))
            .collect();
        five[4].is_top = true;
        keep_top_in_page(&mut five, 3);
        // The top takes the page's last place; the row that was there gives way.
        assert_eq!(
            five.iter().map(|t| t.track_id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "top", "d"]
        );
        // Already inside the page, or no top: nothing moves.
        let before = five.clone();
        keep_top_in_page(&mut five, 3);
        assert_eq!(five, before);
        let mut plain: Vec<RemoteTrack> = ["a", "b"].iter().map(|id| track(id)).collect();
        keep_top_in_page(&mut plain, 1);
        assert_eq!(plain[0].track_id, "a");
    }

    fn saved(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ytm")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn a_long_playlist_reads_page_after_page() {
        // A 307-song public playlist, signed out (trimmed): 100 rows and a next page.
        let first = saved("playlist-first.json");
        assert_eq!(
            read_playlist_title(&first).as_deref(),
            Some("Carvaan Classics Radio Show")
        );
        assert_eq!(read_hits(&first).len(), 3);
        let token = read_playlist_continuation(&first).expect("a next page");
        assert!(token.starts_with("4qmF"));
        // The next page: its rows and a token for the one after.
        let next = saved("playlist-next.json");
        assert_eq!(read_hits(&next).len(), 2);
        assert!(read_playlist_continuation(&next).is_some_and(|t| t != token));
        // The last page has none.
        assert!(read_playlist_continuation(&saved("playlist-last.json")).is_none());
    }

    // A playlist row's album column links the album by its browse id (`MPREb_…`), with or
    // without the page type beside it: either way the song keeps its album, so Go to album
    // works for songs from a YouTube Music playlist.
    #[test]
    fn a_playlist_row_keeps_the_album_its_album_column_links() {
        let tracks = tracks_from_hits(
            &read_hits(&saved("playlist-album-column.json")),
            "remote_catalog",
        );
        let albums: Vec<_> = tracks
            .iter()
            .map(|t| {
                (
                    t.track_id.as_str(),
                    t.album.as_deref(),
                    t.album_id.as_deref(),
                    t.duration_ms,
                )
            })
            .collect();
        assert_eq!(
            albums,
            [
                (
                    "maandVid001",
                    Some("Suno"),
                    Some("MPREb_SANITIZEDSUNO"),
                    Some(225_000)
                ),
                (
                    "kasoorVid01",
                    Some("The Way That Lovers Do"),
                    Some("MPREb_SANITIZEDLOVERS"),
                    Some(171_000)
                ),
            ]
        );
        // The album's name is not taken for an artist.
        assert_eq!(tracks[0].artists, ["Bayaan"]);
    }

    #[test]
    fn continuation_pages_read_their_token_and_their_rows() {
        let raw = json!({"contents": {"sectionListRenderer": {"contents": [{"musicShelfRenderer": {
            "contents": [],
            "continuations": [{"nextContinuationData": {"continuation": "NEXT123"}}]
        }}]}}});
        assert_eq!(read_continuation(&raw).as_deref(), Some("NEXT123"));
        assert_eq!(read_continuation(&songs_response()), None);
        // A continuation page's rows (under continuationContents), length from the fixed column.
        let mut r = row(
            "ATV9",
            "MUSIC_VIDEO_TYPE_ATV",
            "Wonderwall",
            vec![link("Oasis", "MUSIC_PAGE_TYPE_ARTIST", "UC5")],
            Some("1.7B plays"),
        );
        r["musicResponsiveListItemRenderer"]["fixedColumns"] = json!([{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": "4:18"}]}}}]);
        let page = json!({"continuationContents": {"musicShelfContinuation": {"contents": [r]}}});
        let tracks = tracks_from_hits(&read_hits(&page), "remote_catalog");
        assert_eq!(tracks.len(), 1);
        let t = &tracks[0];
        assert_eq!(
            (
                t.track_id.as_str(),
                t.title.as_str(),
                t.duration_ms,
                t.plays
            ),
            ("ATV9", "Wonderwall", Some(258_000), Some(1_700_000_000))
        );
        assert_eq!(
            (t.artists.clone(), t.video_type.as_deref()),
            (vec!["Oasis".to_owned()], Some("ATV"))
        );
    }

    fn two_row(page: &str, id: &str, title: &str, subtitle: &[&str]) -> Value {
        json!({"musicTwoRowItemRenderer": {
            "title": {"runs": [{"text": title}]},
            "subtitle": {"runs": subtitle.iter().map(|t| json!({"text": t})).collect::<Vec<_>>()},
            "thumbnailRenderer": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://a/c.jpg"}]}}},
            "navigationEndpoint": {"browseEndpoint": {"browseId": id,
                "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": page}}}}
        }})
    }

    fn carousel(title: &str, cards: Vec<Value>) -> Value {
        json!({"musicCarouselShelfRenderer": {
            "header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": title}]}}},
            "contents": cards
        }})
    }

    #[test]
    fn an_artist_page_reads_its_header_songs_records_and_similar_artists() {
        const ALBUM: &str = "MUSIC_PAGE_TYPE_ALBUM";
        const ARTIST: &str = "MUSIC_PAGE_TYPE_ARTIST";
        let raw = json!({
            "header": {"musicImmersiveHeaderRenderer": {
                "title": {"runs": [{"text": "Arijit Singh"}]},
                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://a/s.jpg"}, {"url": "https://a/big.jpg"}]}}},
                "monthlyListenerCount": {"runs": [{"text": "642M monthly audience"}]}
            }},
            "contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
                {"musicShelfRenderer": {
                    "contents": [row("ATV1", "MUSIC_VIDEO_TYPE_ATV", "Kesariya", vec![link("Arijit Singh", ARTIST, "UC1")], Some("966M plays"))],
                    "bottomEndpoint": {"browseEndpoint": {"browseId": "VLOLAK5uy_songs"}}
                }},
                carousel("Albums", vec![two_row(ALBUM, "MPREb_1", "Heartbeat Diaries", &["2026"])]),
                carousel("Singles & EPs", vec![two_row(ALBUM, "MPREb_2", "Ramayana", &["Single", " • ", "2026"])]),
                carousel("Videos", vec![two_row("MUSIC_PAGE_TYPE_NON_MUSIC_AUDIO_TRACK_PAGE", "MPED1", "Live", &["3 min"])]),
                carousel("Fans might also like", vec![two_row(ARTIST, "UC9", "Arko", &["288M monthly audience"])]),
            ]}}}}]}}
        });
        let page = read_artist_page(&raw);
        assert_eq!(
            (page.name.as_str(), page.audience),
            ("Arijit Singh", Some(642_000_000))
        );
        assert_eq!(page.thumbnail.as_deref(), Some("https://a/big.jpg"));
        assert_eq!(page.top_songs.len(), 1);
        assert_eq!(page.top_songs[0].plays, Some(966_000_000));
        assert_eq!(page.songs_list_id.as_deref(), Some("VLOLAK5uy_songs"));
        assert_eq!(page.albums.len(), 1);
        assert_eq!(
            (
                page.albums[0].title.as_str(),
                page.albums[0].year().as_deref()
            ),
            ("Heartbeat Diaries", Some("2026"))
        );
        assert_eq!(
            (
                page.singles[0].kind().as_deref(),
                page.singles[0].year().as_deref()
            ),
            (Some("Single"), Some("2026"))
        );
        assert_eq!(
            (page.similar[0].title.as_str(), page.similar[0].audience()),
            ("Arko", Some(288_000_000))
        );
        // Nothing readable: an empty page, not an error.
        assert_eq!(read_artist_page(&json!({})), RawArtistPage::default());
    }

    #[test]
    fn album_and_artist_rows_read_with_their_details() {
        let raw = json!({"contents": [
            page_row("MUSIC_PAGE_TYPE_ALBUM", "MPREb_9", "Brahmastra", vec![json!({"text": "Album"}), json!({"text": " • "}), link("Pritam", "MUSIC_PAGE_TYPE_ARTIST", "UC7"), json!({"text": " • "}), json!({"text": "2022"})]),
            page_row("MUSIC_PAGE_TYPE_ALBUM", "MPREb_8", "Kesariya", vec![json!({"text": "Single"}), json!({"text": " • "}), link("Arijit Singh", "MUSIC_PAGE_TYPE_ARTIST", "UC1")]),
            page_row("MUSIC_PAGE_TYPE_ARTIST", "UC7", "Pritam", vec![json!({"text": "Artist"}), json!({"text": " • "}), json!({"text": "540M monthly audience"})]),
            row("ATV1", "MUSIC_VIDEO_TYPE_ATV", "Kesariya", vec![], None),
        ]});
        let albums = read_albums(&raw);
        assert_eq!(albums.len(), 2);
        assert_eq!(
            (
                albums[0].browse_id.as_str(),
                albums[0].title.as_str(),
                albums[0].year.as_deref(),
                albums[0].kind.as_deref()
            ),
            ("MPREb_9", "Brahmastra", Some("2022"), Some("Album"))
        );
        assert_eq!(albums[0].artists, ["Pritam"]);
        assert_eq!(albums[0].thumbnail.as_deref(), Some("https://a/l.jpg"));
        assert_eq!(
            (albums[1].kind.as_deref(), albums[1].year.as_deref()),
            (Some("Single"), None)
        );
        let artists = read_artists(&raw);
        assert_eq!(artists.len(), 1);
        assert_eq!(
            (artists[0].name.as_str(), artists[0].audience),
            ("Pritam", Some(540_000_000))
        );
        assert_eq!(parse_audience("12M subscribers"), Some(12_000_000));
        assert_eq!(parse_audience("540M plays"), None);
    }
}
