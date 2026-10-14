//! The online catalog for the window: search it, and an album's songs. Only through
//! `MediaAccess` (the catalog playback uses); results are ids and what to show, never a
//! stream address — that is resolved by the player when a song opens.

use crate::playback::Playback;
use media_access::{
    AccessError, AccessErrorKind, CancellationToken, PageRequest, RemoteAlbum, RemoteArtist,
    RemoteTrack,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

/// The search running now: a newer one stops it (you typed on).
#[derive(Default)]
pub struct OnlineSearch(Mutex<CancellationToken>);

impl OnlineSearch {
    fn fresh(&self) -> CancellationToken {
        let token = CancellationToken::default();
        if let Ok(mut last) = self.0.lock() {
            last.cancel();
            *last = token.clone();
        }
        token
    }
}

#[derive(Debug, Default, Serialize)]
pub struct OnlineResults {
    pub tracks: Vec<RemoteTrack>,
    pub albums: Vec<RemoteAlbum>,
    pub artists: Vec<RemoteArtist>,
}

fn words(e: AccessError) -> String {
    match e.kind {
        AccessErrorKind::Transport => "The online catalog can't be reached right now.".into(),
        AccessErrorKind::Unsupported => {
            if e.message.contains("stream resolver") {
                "Full-length play needs the stream tool. Install yt-dlp (python -m pip install -U yt-dlp), then restart."
                    .into()
            } else {
                "Online search isn't available in this session.".into()
            }
        }
        AccessErrorKind::NotFound => "That song isn't in the online catalog.".into(),
        AccessErrorKind::Cancelled => "Search was cancelled.".into(),
        AccessErrorKind::InvalidInput => e.message,
        AccessErrorKind::MalformedResponse => {
            "The online catalog returned something Venuta couldn't read.".into()
        }
    }
}

/// Playback catalog health for Settings (no stream URLs).
#[derive(Debug, Serialize)]
pub struct OnlineCatalogStatus {
    pub live_env: bool,
    pub live_provider_built: bool,
    pub stream_tool_available: bool,
    pub stream_tool_kind: String,
    pub stream_tool_detail: String,
    pub summary: String,
    pub mode: String,
}

/// Install or refresh the standalone stream tool into the user data folder (no Python needed).
#[tauri::command]
pub async fn online_ensure_stream_tool() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        media_access::ensure_stream_tool()
            .map(|p| p.display().to_string())
            .map_err(|e| e.message)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn online_catalog_status() -> OnlineCatalogStatus {
    let h = media_access::catalog_health();
    let mode = if !h.live_provider_built {
        "fixture_only".into()
    } else if !h.live_env {
        "off".into()
    } else if h.stream_tool_available {
        "live".into()
    } else {
        "live_no_tool".into()
    };
    OnlineCatalogStatus {
        live_env: h.live_env,
        live_provider_built: h.live_provider_built,
        stream_tool_available: h.stream_tool_available,
        stream_tool_kind: h.stream_tool_kind,
        stream_tool_detail: h.stream_tool_detail,
        summary: h.summary,
        mode,
    }
}

/// Songs, albums and artists for these words. One kind failing doesn't cost the others;
/// a search overtaken by a newer one comes back empty.
#[tauri::command]
pub async fn online_search(
    query: String,
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<OnlineResults, String> {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return Ok(OnlineResults::default());
    }
    let access = pb.catalog();
    let cancel = running.fresh();
    tauri::async_runtime::spawn_blocking(move || {
        let page = PageRequest::new(20, None);
        let tracks = access.search_tracks(&query, &page, &cancel);
        let albums = access.search_albums(&query, &PageRequest::new(12, None), &cancel);
        let artists = access.search_artists(&query, &PageRequest::new(12, None), &cancel);
        if cancel.is_cancelled() {
            return Ok(OnlineResults::default());
        }
        if let (Err(e), Err(_), Err(_)) = (&tracks, &albums, &artists) {
            return Err(words(e.clone()));
        }
        Ok(OnlineResults {
            tracks: tracks.map(|p| p.items).unwrap_or_default(),
            albums: albums.map(|p| p.items).unwrap_or_default(),
            artists: artists.map(|p| p.items).unwrap_or_default(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// An online album's songs, in order (to play it, or add it to a playlist).
#[tauri::command]
pub async fn online_album_tracks(
    album_id: String,
    pb: State<'_, Playback>,
) -> Result<Vec<RemoteTrack>, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = CancellationToken::default();
        let mut out = Vec::new();
        let mut cursor = None;
        // A few pages at most: an album, not a catalog.
        for _ in 0..5 {
            let page = access
                .album_tracks(&album_id, &PageRequest::new(100, cursor), &cancel)
                .map_err(words)?;
            out.extend(page.items);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Synced lyrics for an online song by title/artist (LRCLIB). Display only — nothing is written
/// to the library. Duration helps match; 0 skips the tight duration filter via search only.
#[tauri::command]
pub async fn online_lyrics(
    title: String,
    artist: String,
    album: String,
    duration_ms: u64,
) -> Result<OnlineLyrics, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let title = title.trim();
        let artist = artist.trim();
        if title.is_empty() {
            return Ok(OnlineLyrics::default());
        }
        let http = engine::net::Http::open().map_err(|e| e.to_string())?;
        let duration_s = ((duration_ms + 500) / 1000).max(1);
        let found = engine::providers::lrclib_find(
            &http,
            title,
            if artist.is_empty() { " " } else { artist },
            album.trim(),
            duration_s,
            None,
        )
        .map_err(|e| e.to_string())?;
        let Some(found) = found else {
            return Ok(OnlineLyrics::default());
        };
        Ok(parse_lrc(&found.synced))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Default, Serialize)]
pub struct OnlineLyrics {
    /// Timed lines [ms, text].
    pub lines: Vec<(u64, String)>,
    pub text: String,
}

fn parse_lrc(synced: &str) -> OnlineLyrics {
    let mut lines = Vec::new();
    let mut plain = Vec::new();
    for line in synced.lines() {
        let Some(rest) = line.strip_prefix('[') else {
            if !line.trim().is_empty() {
                plain.push(line.to_owned());
            }
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
        let ms = ((m * 60.0 + sec) * 1000.0).round() as u64;
        let words = words.trim().to_owned();
        if !words.is_empty() {
            plain.push(words.clone());
        }
        lines.push((ms, words));
    }
    OnlineLyrics {
        lines,
        text: plain.join("\n"),
    }
}
