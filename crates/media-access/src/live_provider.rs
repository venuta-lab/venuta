//! Sealed live remote-catalog provider.
//!
//! Enabled only with the `live-provider` feature. Product code should keep using
//! [`crate::FakeMediaAccess`] unless an explicit live factory is selected.

use crate::access::{AccessError, AccessErrorKind, MediaAccess};
use crate::types::{
    CancellationToken, EphemeralAudio, MediaHeader, Page, PageRequest, RemoteAlbum, RemoteArtist,
    RemoteTrack,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use innertube_rs::endpoints::player::resolve_stream_url_full;
use innertube_rs::models::music::{
    MusicAlbumView, MusicSearchFilter, MusicSearchResults, MusicTrackItem,
};
use innertube_rs::models::video::GetVideoInfoOptions;
use innertube_rs::{Innertube, endpoints::music::parse_music_search_response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

const PROVIDER: &str = "remote_catalog";
const MAX_EMPTY_SEARCH_PAGES: usize = 8;
const POT_ENDPOINT: &str = "http://127.0.0.1:4416/get_pot";

type ClientResult = Result<Arc<Innertube>, Arc<str>>;
type ApiJob = Box<dyn FnOnce(Arc<Runtime>, ClientResult) + Send + 'static>;

trait LiveBackend: Send + Sync {
    fn search(
        &self,
        query: &str,
        filter: MusicSearchFilter,
        continuation: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<MusicSearchResults, AccessError>;

    fn album(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<MusicAlbumView, AccessError>;

    fn resolve(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError>;
}

struct InnertubeBackend {
    jobs: SyncSender<ApiJob>,
}

impl InnertubeBackend {
    fn new() -> Result<Self, AccessError> {
        let runtime = Arc::new(
            Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|_| {
                    AccessError::new(
                        AccessErrorKind::Transport,
                        "could not start the live catalog runtime",
                    )
                })?,
        );
        let (jobs, receiver) = sync_channel::<ApiJob>(8);
        thread::Builder::new()
            .name("venuta-live-catalog".into())
            .spawn(move || run_api_actor(runtime, receiver))
            .map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "could not start the live catalog worker",
                )
            })?;
        Ok(Self { jobs })
    }

    fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(Arc<Runtime>, ClientResult) -> Result<T, AccessError> + Send + 'static,
    ) -> Result<T, AccessError> {
        let (reply, result) = sync_channel(1);
        self.jobs
            .send(Box::new(move |runtime, client| {
                let _ = reply.send(operation(runtime, client));
            }))
            .map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "the live catalog worker stopped",
                )
            })?;
        result.recv().map_err(|_| {
            AccessError::new(
                AccessErrorKind::Transport,
                "the live catalog worker stopped",
            )
        })?
    }
}

fn run_api_actor(runtime: Arc<Runtime>, receiver: Receiver<ApiJob>) {
    let client = runtime
        .block_on(Innertube::new())
        .map(Arc::new)
        .map_err(|_| Arc::<str>::from("initialization failed"));
    while let Ok(job) = receiver.recv() {
        job(runtime.clone(), client.clone());
    }
}

impl LiveBackend for InnertubeBackend {
    fn search(
        &self,
        query: &str,
        filter: MusicSearchFilter,
        continuation: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<MusicSearchResults, AccessError> {
        let query = query.to_owned();
        let cancel = cancel.clone();
        self.call(move |runtime, client| {
            let client = client.map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "could not initialize the live catalog client",
                )
            })?;
            runtime.block_on(cancellable(cancel, async move {
                let mut payload = json!({
                    "query": query.clone(),
                    "params": filter.to_param_str(),
                });
                if let Some(continuation) = continuation {
                    payload["continuation"] = json!(continuation);
                }
                let response = client
                    .session
                    .post_innertube_client("YTMUSIC", "/search", payload)
                    .await
                    .map_err(|_| {
                        AccessError::new(AccessErrorKind::Transport, "catalog search failed")
                    })?;
                if !response.status().is_success() {
                    return Err(AccessError::new(
                        AccessErrorKind::Transport,
                        "catalog search returned an unsuccessful response",
                    ));
                }
                let raw: Value = response.json().await.map_err(|_| {
                    AccessError::new(
                        AccessErrorKind::MalformedResponse,
                        "catalog returned invalid search data",
                    )
                })?;
                parse_music_search_response(&query, Some(filter), &raw).map_err(|_| {
                    AccessError::new(
                        AccessErrorKind::MalformedResponse,
                        "catalog search data could not be parsed",
                    )
                })
            }))
        })
    }

    fn album(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<MusicAlbumView, AccessError> {
        let album_id = album_id.to_owned();
        let cancel = cancel.clone();
        self.call(move |runtime, client| {
            let client = client.map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "could not initialize the live catalog client",
                )
            })?;
            runtime.block_on(cancellable(cancel, async move {
                client.get_music_album(&album_id).await.map_err(|_| {
                    AccessError::new(AccessErrorKind::Transport, "catalog album lookup failed")
                })
            }))
        })
    }

    fn resolve(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        if cancel.is_cancelled() {
            return Err(AccessError::new(
                AccessErrorKind::Cancelled,
                "request cancelled",
            ));
        }
        // Full-length path: external stream tool (easy to update independently).
        if let Ok(audio) = crate::stream_resolve::resolve_with_stream_tool(video_id) {
            if probe_media_status(&audio).is_some_and(|c| (200..300).contains(&c)) {
                return Ok(audio);
            }
            // Tool returned a URL that failed a tiny range probe — still prefer it over
            // the partial InnerTube window if the tool is present; player will retry ranges.
            return Ok(audio);
        }
        // Fallback: InnerTube multi-client (may only unlock a short media window).
        let video_id = video_id.to_owned();
        let cancel = cancel.clone();
        self.call(move |runtime, client| {
            let client = client.map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "could not initialize the live catalog client",
                )
            })?;
            runtime.block_on(cancellable(cancel, async move {
                resolve_playable_audio(&client, &video_id).await
            }))
        })
    }
}

async fn resolve_playable_audio(
    client: &Innertube,
    video_id: &str,
) -> Result<EphemeralAudio, AccessError> {
    // Prefer clients that currently yield direct HTTPS formats without a GVS token.
    // Fall back to WEB with a local access-proof token when needed.
    let visitor = client
        .session
        .context
        .client
        .visitor_data
        .clone()
        .unwrap_or_default();
    let pot_video = fetch_local_pot(video_id);
    let pot_visitor = if visitor.is_empty() {
        None
    } else {
        fetch_local_pot(&visitor)
    };

    // IOS/WEB_EMBEDDED tend to expose AAC; ANDROID_VR is a reliable no-token fallback.
    let attempts: Vec<(Option<&str>, Option<String>, &'static str, &'static str)> = vec![
        (Some("IOS"), None, "https://www.youtube.com", "https://www.youtube.com/"),
        (Some("WEB_EMBEDDED"), None, "https://www.youtube.com", "https://www.youtube.com/"),
        (Some("ANDROID_VR"), None, "https://www.youtube.com", "https://www.youtube.com/"),
        (Some("ANDROID"), None, "https://www.youtube.com", "https://www.youtube.com/"),
        (
            Some("WEB"),
            pot_visitor.clone().or_else(|| pot_video.clone()),
            "https://www.youtube.com",
            "https://www.youtube.com/",
        ),
        (
            Some("YTMUSIC"),
            pot_visitor.or(pot_video),
            "https://music.youtube.com",
            "https://music.youtube.com/",
        ),
        (None, None, "https://www.youtube.com", "https://www.youtube.com/"),
    ];

    let mut last_err: Option<AccessError> = None;
    for (client_name, pot, origin, referer) in attempts {
        match try_resolve_with_client(client, video_id, client_name, pot.as_deref(), origin, referer)
            .await
        {
            Ok(audio) => {
                // Prefer a candidate that the media host accepts when we can cheaply check.
                match probe_media_status(&audio) {
                    Some(code) if (200..300).contains(&code) => return Ok(audio),
                    Some(403) | Some(401) => {
                        last_err = Some(AccessError::new(
                            AccessErrorKind::Unsupported,
                            format!(
                                "catalog audio stream rejected by media host ({})",
                                client_name.unwrap_or("default")
                            ),
                        ));
                        continue;
                    }
                    _ => return Ok(audio),
                }
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        AccessError::new(
            AccessErrorKind::Unsupported,
            "catalog did not return a playable audio stream",
        )
    }))
}

async fn try_resolve_with_client(
    client: &Innertube,
    video_id: &str,
    client_name: Option<&str>,
    pot: Option<&str>,
    origin: &str,
    referer: &str,
) -> Result<EphemeralAudio, AccessError> {
    let options = GetVideoInfoOptions {
        client: client_name.map(str::to_owned),
        po_token: pot.map(str::to_owned),
        playback_context: None,
    };
    let info = client
        .get_basic_info(video_id, Some(&options))
        .await
        .map_err(|_| {
            AccessError::new(AccessErrorKind::Transport, "catalog stream lookup failed")
        })?;
    let stream_data = info.player_response.streaming_data.ok_or_else(|| {
        AccessError::new(
            AccessErrorKind::Unsupported,
            "catalog did not return playable stream data",
        )
    })?;

    let format = stream_data
        .adaptive_formats
        .iter()
        .chain(stream_data.formats.iter())
        .filter(|format| format.is_audio_only() && format.mime_type.starts_with("audio/mp4"))
        .max_by_key(|format| format.bitrate)
        .or_else(|| {
            stream_data
                .adaptive_formats
                .iter()
                .chain(stream_data.formats.iter())
                .filter(|format| format.is_audio_only())
                .max_by_key(|format| format.bitrate)
        })
        .ok_or_else(|| {
            AccessError::new(
                AccessErrorKind::Unsupported,
                "catalog did not return a supported audio stream",
            )
        })?;

    let url = resolve_stream_url_full(format, &client.player.decipherer, pot, None).map_err(|_| {
        AccessError::new(
            AccessErrorKind::Unsupported,
            "catalog audio stream could not be resolved",
        )
    })?;

    let mut headers = vec![
        MediaHeader {
            name: "Origin".into(),
            value: origin.into(),
        },
        MediaHeader {
            name: "Referer".into(),
            value: referer.into(),
        },
        MediaHeader {
            name: "Accept".into(),
            value: "*/*".into(),
        },
    ];
    if let Some(token) = pot {
        headers.push(MediaHeader {
            name: "X-Goog-Po-Token".into(),
            value: token.to_owned(),
        });
    }

    Ok(EphemeralAudio {
        track_id: video_id.to_owned(),
        provider: PROVIDER.into(),
        url,
        mime_type: Some(format.mime_type.clone()),
        content_length: format
            .content_length
            .as_deref()
            .and_then(|length| length.parse().ok()),
        duration_ms: format
            .approx_duration_ms
            .as_deref()
            .and_then(|duration| duration.parse().ok()),
        headers,
    })
}

fn probe_media_status(audio: &EphemeralAudio) -> Option<u16> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent.get(&audio.url);
    for h in &audio.headers {
        req = req.header(&h.name, &h.value);
    }
    // Adaptive media hosts often reject a bare full GET with 403 but accept ranges.
    req = req.header("Range", "bytes=0-2047");
    match req.call() {
        Ok(resp) => Some(resp.status().as_u16()),
        Err(_) => None,
    }
}

async fn cancellable<T>(
    cancel: CancellationToken,
    future: impl Future<Output = Result<T, AccessError>>,
) -> Result<T, AccessError> {
    tokio::pin!(future);
    loop {
        if cancel.is_cancelled() {
            return Err(AccessError::new(
                AccessErrorKind::Cancelled,
                "request cancelled",
            ));
        }
        tokio::select! {
            result = &mut future => return result,
            _ = tokio::time::sleep(Duration::from_millis(25)) => {}
        }
    }
}

/// Live catalog access implementing the public [`MediaAccess`] contract.
pub struct LiveCatalogAccess {
    backend: Arc<dyn LiveBackend>,
}

impl LiveCatalogAccess {
    pub fn new() -> Result<Self, AccessError> {
        Ok(Self {
            backend: Arc::new(InnertubeBackend::new()?),
        })
    }
}

impl MediaAccess for LiveCatalogAccess {
    fn search_tracks(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search query must not be empty",
            ));
        }
        let mut cursor = decode_cursor(page.cursor.as_deref(), CursorKind::Tracks)?;
        let mut skipped_empty_pages = 0;
        loop {
            let result = self.backend.search(
                query,
                MusicSearchFilter::Songs,
                cursor.continuation.clone(),
                cancel,
            )?;
            let continuation = result.continuation_token;
            let (tracks, skipped_tracks) = normalize_tracks(result.songs, None)?;

            if tracks.is_empty() {
                let continuation_advances = continuation
                    .as_deref()
                    .is_some_and(|next| Some(next) != cursor.continuation.as_deref());
                if continuation_advances {
                    if skipped_empty_pages >= MAX_EMPTY_SEARCH_PAGES {
                        return Err(AccessError::new(
                            AccessErrorKind::MalformedResponse,
                            "catalog search returned too many empty or unusable pages",
                        ));
                    }
                    cursor.continuation = continuation;
                    cursor.offset = 0;
                    skipped_empty_pages += 1;
                    continue;
                }
                if skipped_tracks > 0 {
                    return Err(AccessError::new(
                        AccessErrorKind::MalformedResponse,
                        "catalog search results contained no usable track IDs",
                    ));
                }
            }

            return page_batch(tracks, continuation, cursor, CursorKind::Tracks, page.limit);
        }
    }

    fn search_albums(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteAlbum>, AccessError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search query must not be empty",
            ));
        }
        let cursor = decode_cursor(page.cursor.as_deref(), CursorKind::Albums)?;
        let result = self.backend.search(
            query,
            MusicSearchFilter::Albums,
            cursor.continuation.clone(),
            cancel,
        )?;
        let albums = result
            .albums
            .into_iter()
            .filter_map(|album| {
                let id = album.browse_id.trim();
                if id.is_empty() {
                    return None;
                }
                Some(RemoteAlbum {
                    provider: PROVIDER.into(),
                    album_id: id.to_owned(),
                    title: album.title,
                    artists: album.artist.into_iter().collect(),
                    track_count: album.track_count,
                    release_date: album.year,
                    artwork_url: clean_artwork_url(album.thumbnail),
                })
            })
            .collect();
        page_batch(
            albums,
            result.continuation_token,
            cursor,
            CursorKind::Albums,
            page.limit,
        )
    }

    fn search_artists(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteArtist>, AccessError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search query must not be empty",
            ));
        }
        let cursor = decode_cursor(page.cursor.as_deref(), CursorKind::Artists)?;
        let result = self.backend.search(
            query,
            MusicSearchFilter::Artists,
            cursor.continuation.clone(),
            cancel,
        )?;
        let artists = result
            .artists
            .into_iter()
            .filter_map(|artist| {
                let id = artist.browse_id.trim();
                if id.is_empty() {
                    return None;
                }
                Some(RemoteArtist {
                    provider: PROVIDER.into(),
                    artist_id: id.to_owned(),
                    name: artist.name,
                    artwork_url: clean_artwork_url(artist.thumbnail),
                })
            })
            .collect();
        page_batch(
            artists,
            result.continuation_token,
            cursor,
            CursorKind::Artists,
            page.limit,
        )
    }

    fn album_tracks(
        &self,
        album_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        if album_id.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "album id must not be empty",
            ));
        }
        let cursor = decode_cursor(page.cursor.as_deref(), CursorKind::AlbumTracks)?;
        if cursor.continuation.is_some() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "album cursor is invalid",
            ));
        }
        let album_view = self.backend.album(album_id, cancel)?;
        let fallback_album = album_view.browse_id.clone();
        let (tracks, skipped_tracks) =
            normalize_tracks(album_view.tracks, Some(fallback_album.as_str()))?;
        if tracks.is_empty() && skipped_tracks > 0 {
            return Err(AccessError::new(
                AccessErrorKind::MalformedResponse,
                "catalog album tracks contained no usable track IDs",
            ));
        }
        page_batch(tracks, None, cursor, CursorKind::AlbumTracks, page.limit)
    }

    fn playlist_tracks(
        &self,
        playlist_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        if playlist_id.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "playlist id must not be empty",
            ));
        }
        self.album_tracks(playlist_id, page, cancel)
    }

    fn resolve_audio(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        if track_id.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "track id must not be empty",
            ));
        }
        self.backend.resolve(track_id.trim(), cancel)
    }
}

fn normalize_track(
    track: MusicTrackItem,
    fallback_album_id: Option<&str>,
) -> Result<Option<RemoteTrack>, AccessError> {
    let video_id = track.video_id.trim();
    if video_id.is_empty() {
        return Ok(None);
    }
    let album_id = track
        .album
        .as_ref()
        .and_then(|album| album.browse_id.as_ref())
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .or_else(|| fallback_album_id.map(str::to_owned));
    let artwork_url = clean_artwork_url(track.thumbnail).or_else(|| {
        // Always have a picture when the id is a YouTube video id.
        (video_id.len() == 11).then(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"))
    });
    Ok(Some(RemoteTrack {
        provider: PROVIDER.into(),
        track_id: video_id.to_owned(),
        title: track.title,
        artists: track
            .artists
            .into_iter()
            .map(|artist| artist.name)
            .collect(),
        album: track.album.as_ref().map(|album| album.title.clone()),
        album_id,
        duration_ms: track.duration_ms,
        artwork_url,
        isrc: None,
    }))
}

fn clean_artwork_url(raw: Option<String>) -> Option<String> {
    let s = raw?.trim().to_owned();
    if s.is_empty() {
        return None;
    }
    let url = if let Some(rest) = s.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = s.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        s
    };
    if url.starts_with("https://") {
        Some(url)
    } else {
        None
    }
}

fn normalize_tracks(
    tracks: Vec<MusicTrackItem>,
    fallback_album_id: Option<&str>,
) -> Result<(Vec<RemoteTrack>, usize), AccessError> {
    let mut normalized = Vec::with_capacity(tracks.len());
    let mut skipped = 0;
    for track in tracks {
        if let Some(track) = normalize_track(track, fallback_album_id)? {
            normalized.push(track);
        } else {
            skipped += 1;
        }
    }
    Ok((normalized, skipped))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum CursorKind {
    Tracks,
    Albums,
    Artists,
    AlbumTracks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cursor {
    version: u8,
    kind: CursorKind,
    continuation: Option<String>,
    offset: usize,
}

fn encode_cursor(cursor: Cursor) -> Result<String, AccessError> {
    let bytes = serde_json::to_vec(&cursor).map_err(|_| {
        AccessError::new(
            AccessErrorKind::MalformedResponse,
            "could not encode catalog cursor",
        )
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_cursor(raw: Option<&str>, kind: CursorKind) -> Result<Cursor, AccessError> {
    let Some(raw) = raw else {
        return Ok(Cursor {
            version: 1,
            kind,
            continuation: None,
            offset: 0,
        });
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw.as_bytes()).map_err(|_| {
        AccessError::new(AccessErrorKind::InvalidInput, "catalog cursor is invalid")
    })?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| {
        AccessError::new(AccessErrorKind::InvalidInput, "catalog cursor is invalid")
    })?;
    if cursor.version != 1 || cursor.kind != kind {
        return Err(AccessError::new(
            AccessErrorKind::InvalidInput,
            "catalog cursor is invalid",
        ));
    }
    Ok(cursor)
}

fn page_batch<T>(
    items: Vec<T>,
    next_continuation: Option<String>,
    cursor: Cursor,
    kind: CursorKind,
    requested_limit: usize,
) -> Result<Page<T>, AccessError> {
    if cursor.offset > items.len() {
        return Err(AccessError::new(
            AccessErrorKind::InvalidInput,
            "catalog cursor is outside the current result page",
        ));
    }
    let limit = requested_limit.clamp(1, 100);
    let start = cursor.offset;
    let total = items.len();
    let end = start.saturating_add(limit).min(total);
    let page_items = items.into_iter().skip(start).take(end - start).collect();
    let continuation = if end < total {
        Some(Cursor {
            version: 1,
            kind,
            continuation: cursor.continuation,
            offset: end,
        })
    } else {
        next_continuation
            .filter(|continuation| Some(continuation) != cursor.continuation.as_ref())
            .map(|continuation| Cursor {
                version: 1,
                kind,
                continuation: Some(continuation),
                offset: 0,
            })
    };
    Ok(Page {
        items: page_items,
        next_cursor: continuation.map(encode_cursor).transpose()?,
    })
}

fn fetch_local_pot(video_id: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    let body = json!({ "content_binding": video_id });
    let mut response = agent
        .post(POT_ENDPOINT)
        .header("Content-Type", "application/json")
        .send_json(&body)
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.body_mut().read_json().ok()?;
    value
        .get("poToken")
        .or_else(|| value.get("po_token"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|token| !token.trim().is_empty())
}
