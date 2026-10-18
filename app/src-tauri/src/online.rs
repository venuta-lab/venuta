//! The online catalog for the window: search it, and an album's songs. Only through
//! `MediaAccess` (the catalog playback uses); results are ids and what to show, never a
//! stream address — that is resolved by the player when a song opens.

use crate::playback::Playback;
use media_access::{
    AccessError, AccessErrorKind, CancellationToken, MoodSection, PageRequest, RemoteAlbum,
    RemoteAlbumPage, RemoteArtist, RemoteCollection, RemoteMore, RemotePlaylist, RemoteRadio,
    RemoteSuggestion, RemoteTrack, SearchSuggestions,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

/// What a search that a newer one (from the same search box) replaced answers with: an error
/// the window recognises and ignores, never an empty result that reads as "nothing online".
pub const SEARCH_CANCELLED: &str = "online-search-cancelled";

/// Identical searches within this long reuse the answer (typing back, switching boxes).
const CACHE_FOR: Duration = Duration::from_secs(90);
const CACHE_SIZE: usize = 32;

/// The search running in each search box (Music, Ctrl+K, Add songs, the online picker,
/// "more by"…): a newer search stops only the one before it in the same box. Plus a short
/// memory of recent answers; only ids and what to show are kept, never stream addresses.
#[derive(Default)]
pub struct OnlineSearch {
    running: Mutex<HashMap<String, CancellationToken>>,
    recent: Mutex<HashMap<String, (Instant, OnlineResults)>>,
    /// The catalog's moods and genres, asked once a session (they change rarely).
    moods: Mutex<Option<Vec<MoodSection>>>,
}

impl OnlineSearch {
    fn fresh(&self, search_box: &str) -> CancellationToken {
        let token = CancellationToken::default();
        if let Ok(mut running) = self.running.lock()
            && let Some(last) = running.insert(search_box.to_owned(), token.clone())
        {
            last.cancel();
        }
        token
    }

    fn cached(&self, query: &str) -> Option<OnlineResults> {
        let recent = self.recent.lock().ok()?;
        recent
            .get(query)
            .filter(|(at, _)| at.elapsed() < CACHE_FOR)
            .map(|(_, r)| r.clone())
    }

    fn remember(&self, query: String, results: &OnlineResults) {
        if let Ok(mut recent) = self.recent.lock() {
            recent.retain(|_, (at, _)| at.elapsed() < CACHE_FOR);
            if recent.len() >= CACHE_SIZE
                && let Some(oldest) = recent
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(k, _)| k.clone())
            {
                recent.remove(&oldest);
            }
            recent.insert(query, (Instant::now(), results.clone()));
        }
    }
}

/// Which search box asked: a short name from the window, "music" when none is given.
fn box_name(search_box: Option<String>) -> String {
    search_box
        .map(|b| b.trim().to_owned())
        .filter(|b| !b.is_empty() && b.len() <= 32)
        .unwrap_or_else(|| "music".into())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OnlineResults {
    pub tracks: Vec<RemoteTrack>,
    pub albums: Vec<RemoteAlbum>,
    pub artists: Vec<RemoteArtist>,
    /// Where more songs for these words start (`online_search_tracks`); None when that's all.
    /// A catalog paging token, never a stream address.
    pub tracks_cursor: Option<String>,
}

/// More songs for the same words: the next page, and where the one after it starts.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OnlineTrackPage {
    pub tracks: Vec<RemoteTrack>,
    pub next_cursor: Option<String>,
}

/// Songs asked for per page (the first answer, and each "More").
const TRACKS_PER_PAGE: usize = 20;

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

/// What a part of a search answers when its thread stopped without answering.
fn search_stopped() -> AccessError {
    AccessError::new(AccessErrorKind::Transport, "search stopped")
}

/// Songs, albums and artists for these words. One kind failing doesn't cost the others.
///
/// `search_box` names the box that asked ("music", "palette", "add-songs", "pick",
/// "more-by"…; "music" when left out): a newer search from the same box stops this one, which
/// then fails with [`SEARCH_CANCELLED`] for the window to ignore. Other boxes are untouched.
#[tauri::command]
pub async fn online_search(
    query: String,
    search_box: Option<String>,
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<OnlineResults, String> {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return Ok(OnlineResults::default());
    }
    let key = query.to_lowercase();
    if let Some(hit) = running.cached(&key) {
        return Ok(hit);
    }
    let access = pb.catalog();
    let cancel = running.fresh(&box_name(search_box));
    let started = Instant::now();
    let results = tauri::async_runtime::spawn_blocking(move || {
        // All three asked at once: the answer takes as long as the slowest, not the sum.
        let (tracks, albums, artists) = std::thread::scope(|s| {
            let albums =
                s.spawn(|| access.search_albums(&query, &PageRequest::new(12, None), &cancel));
            let artists =
                s.spawn(|| access.search_artists(&query, &PageRequest::new(12, None), &cancel));
            let tracks =
                access.search_tracks(&query, &PageRequest::new(TRACKS_PER_PAGE, None), &cancel);
            (
                tracks,
                albums.join().unwrap_or_else(|_| Err(search_stopped())),
                artists.join().unwrap_or_else(|_| Err(search_stopped())),
            )
        });
        if cancel.is_cancelled() {
            return Err(SEARCH_CANCELLED.to_owned());
        }
        if let (Err(e), Err(_), Err(_)) = (&tracks, &albums, &artists) {
            return Err(words(e.clone()));
        }
        // Remembered only when every part answered (a part that failed is asked again).
        let whole = tracks.is_ok() && albums.is_ok() && artists.is_ok();
        let (tracks, tracks_cursor) = tracks.map(|p| (p.items, p.next_cursor)).unwrap_or_default();
        Ok((
            OnlineResults {
                tracks,
                albums: albums.map(|p| p.items).unwrap_or_default(),
                artists: artists.map(|p| p.items).unwrap_or_default(),
                tracks_cursor,
            },
            whole,
        ))
    })
    .await
    .map_err(|e| e.to_string())??;
    media_access::note_timing(&format!(
        "online search answered in {} ms",
        media_access::ms_since(started)
    ));
    if results.1 {
        running.remember(key, &results.0);
    }
    Ok(results.0)
}

/// What the catalog suggests for half-typed words: completions, and a few songs, albums,
/// artists and playlists. A newer call from the same box stops this one (answered with
/// [`SEARCH_CANCELLED`]); offline or unsupported, it answers nothing rather than an error.
#[tauri::command]
pub async fn online_suggest(
    input: String,
    search_box: Option<String>,
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<SearchSuggestions, String> {
    let input = input.trim().to_owned();
    if input.is_empty() {
        return Ok(SearchSuggestions::default());
    }
    let access = pb.catalog();
    let cancel = running.fresh(&format!("suggest:{}", box_name(search_box)));
    let started = Instant::now();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let found = access.suggest(&input, &cancel);
        if cancel.is_cancelled() {
            return Err(SEARCH_CANCELLED.to_owned());
        }
        Ok(found.unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())??;
    media_access::note_timing(&format!(
        "online suggestions answered in {} ms",
        media_access::ms_since(started)
    ));
    Ok(found)
}

/// Playlists (`kind` "playlists") or podcasts (`kind` "podcasts") for these words.
#[tauri::command]
pub async fn online_collections(
    query: String,
    kind: String,
    search_box: Option<String>,
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<Vec<RemoteCollection>, String> {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let access = pb.catalog();
    let cancel = running.fresh(&format!("{kind}:{}", box_name(search_box)));
    tauri::async_runtime::spawn_blocking(move || {
        let page = access.search_collections(&query, &kind, &PageRequest::new(30, None), &cancel);
        if cancel.is_cancelled() {
            return Err(SEARCH_CANCELLED.to_owned());
        }
        Ok(page.map(|p| p.items).unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Videos for these words: live sets, covers and uploads that never had a release.
#[tauri::command]
pub async fn online_videos(
    query: String,
    search_box: Option<String>,
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<Vec<RemoteTrack>, String> {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let access = pb.catalog();
    let cancel = running.fresh(&format!("videos:{}", box_name(search_box)));
    tauri::async_runtime::spawn_blocking(move || {
        let page = access.search_videos(&query, &PageRequest::new(30, None), &cancel);
        if cancel.is_cancelled() {
            return Err(SEARCH_CANCELLED.to_owned());
        }
        Ok(page.map(|p| p.items).unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The catalog's moods and genres to browse (asked once a session; empty when offline).
#[tauri::command]
pub async fn online_moods(
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<Vec<MoodSection>, String> {
    if let Some(kept) = running.moods.lock().ok().and_then(|m| m.clone()) {
        return Ok(kept);
    }
    let access = pb.catalog();
    let found =
        tauri::async_runtime::spawn_blocking(move || access.moods(&CancellationToken::default()))
            .await
            .map_err(|e| e.to_string())?;
    match found {
        Ok(sections) => {
            if !sections.is_empty()
                && let Ok(mut kept) = running.moods.lock()
            {
                *kept = Some(sections.clone());
            }
            Ok(sections)
        }
        Err(_) => Ok(Vec::new()),
    }
}

/// One mood's or genre's playlists (`params` from its tile).
#[tauri::command]
pub async fn online_mood(
    params: String,
    pb: State<'_, Playback>,
) -> Result<Vec<RemoteCollection>, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        access
            .mood_collections(&params, &CancellationToken::default())
            .map_err(words)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The catalog's home feed (filtered by a chip's `params`), or its next rows.
#[tauri::command]
pub async fn online_home(
    params: Option<String>,
    continuation: Option<String>,
    pb: State<'_, Playback>,
) -> Result<media_access::RemoteHome, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        access
            .home(
                params.as_deref(),
                continuation.as_deref(),
                &CancellationToken::default(),
            )
            .map_err(words)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A radio: songs like `seed` (a song id), or more of the radio `playlist_id` after the song
/// `seed`. Songs you disliked are left out.
#[tauri::command]
pub async fn online_radio(
    seed: Option<String>,
    playlist_id: Option<String>,
    pb: State<'_, Playback>,
) -> Result<RemoteRadio, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        let mut radio = access
            .radio(
                seed.as_deref(),
                playlist_id.as_deref(),
                &CancellationToken::default(),
            )
            .map_err(words)?;
        let disliked = engine::settings::load().listening.disliked;
        if !disliked.is_empty() {
            radio
                .tracks
                .retain(|t| !disliked.contains(&format!("{}:{}", t.provider, t.track_id)));
        }
        Ok(radio)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// An online album's whole page: what it is, who made it, its songs and the rows under them.
#[tauri::command]
pub async fn online_album_page(
    album_id: String,
    pb: State<'_, Playback>,
) -> Result<RemoteAlbumPage, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        access
            .album_page(&album_id, &CancellationToken::default())
            .map_err(words)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Everything in a row of a catalog page (its "See all": all their albums, all their singles).
#[tauri::command]
pub async fn online_shelf_more(
    more: RemoteMore,
    pb: State<'_, Playback>,
) -> Result<Vec<RemoteSuggestion>, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        access
            .shelf_more(&more, &CancellationToken::default())
            .map_err(words)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A catalog playlist or podcast, opened: its name and its songs or episodes, in order.
#[tauri::command]
pub async fn online_collection(
    collection_id: String,
    pb: State<'_, Playback>,
) -> Result<RemotePlaylist, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        access
            .playlist(&collection_id, &CancellationToken::default())
            .map_err(words)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// More songs for the same words, from `cursor` (an answer's `tracks_cursor`, or a page's
/// `next_cursor`). Same search boxes and cancelling as [`online_search`]; not remembered.
#[tauri::command]
pub async fn online_search_tracks(
    query: String,
    cursor: String,
    search_box: Option<String>,
    pb: State<'_, Playback>,
    running: State<'_, OnlineSearch>,
) -> Result<OnlineTrackPage, String> {
    let query = query.trim().to_owned();
    if query.is_empty() || cursor.trim().is_empty() {
        return Ok(OnlineTrackPage::default());
    }
    let access = pb.catalog();
    let cancel = running.fresh(&box_name(search_box));
    tauri::async_runtime::spawn_blocking(move || {
        let page = access.search_tracks(
            &query,
            &PageRequest::new(TRACKS_PER_PAGE, Some(cursor)),
            &cancel,
        );
        if cancel.is_cancelled() {
            return Err(SEARCH_CANCELLED.to_owned());
        }
        let page = page.map_err(words)?;
        Ok(OnlineTrackPage {
            tracks: page.items,
            next_cursor: page.next_cursor,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// An online artist's page: top songs, more songs, albums, singles and similar artists.
/// Ids and what to show only; songs play through the catalog like any online song.
#[tauri::command]
pub async fn online_artist(
    artist_id: String,
    pb: State<'_, Playback>,
) -> Result<media_access::RemoteArtistPage, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        access
            .artist_page(&artist_id, &CancellationToken::default())
            .map_err(words)
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
