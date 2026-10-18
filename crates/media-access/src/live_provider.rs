//! Sealed live remote-catalog provider.
//!
//! Enabled only with the `live-provider` feature. Product code should keep using
//! [`crate::FakeMediaAccess`] unless an explicit live factory is selected.
//!
//! Requests go out side by side through one shared YouTube Music client (the `ytmusic` crate):
//! a search never waits behind a playlist import or an artist page, and nothing has to start up
//! before the first search. Songs resolve inside the app first, with the stream tool behind that.

use crate::access::{AccessError, AccessErrorKind, MediaAccess};
use crate::catalog_json;
use crate::types::{
    CancellationToken, EphemeralAudio, MediaHeader, Page, PageRequest, RemoteAlbum, RemoteArtist,
    RemoteArtistPage, RemotePlaylist, RemoteTrack,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use innertube_rs::endpoints::player::resolve_stream_url_full;
use innertube_rs::models::music::{MusicSearchFilter, MusicSearchResults, MusicTrackItem};
use innertube_rs::models::video::GetVideoInfoOptions;
use innertube_rs::{Innertube, endpoints::music::parse_music_search_response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

const PROVIDER: &str = "remote_catalog";

/// Search filters as the catalog reads them today (checked against live responses on
/// 2026-10-15, and the same as ytmusicapi's). innertube-rs 0.9's own filter codes are ignored
/// by the server: they return the unfiltered mix (uploads, videos, podcast episodes) under a
/// Songs / Albums / Artists label, so these are kept here.
const SONGS_PARAMS: &str = "EgWKAQIIAWoMEA4QChADEAQQCRAF";
const ALBUMS_PARAMS: &str = "EgWKAQIYAWoMEA4QChADEAQQCRAF";
const ARTISTS_PARAMS: &str = "EgWKAQIgAWoMEA4QChADEAQQCRAF";

fn search_params(filter: MusicSearchFilter) -> &'static str {
    match filter {
        MusicSearchFilter::Songs => SONGS_PARAMS,
        MusicSearchFilter::Albums => ALBUMS_PARAMS,
        MusicSearchFilter::Artists => ARTISTS_PARAMS,
        other => other.to_param_str(),
    }
}

/// A search response: as innertube-rs parsed it, and the raw JSON for what it doesn't parse
/// (kind of upload, plays, top result).
struct SearchPage {
    parsed: MusicSearchResults,
    raw: Value,
}
const MAX_EMPTY_SEARCH_PAGES: usize = 8;
const POT_ENDPOINT: &str = "http://127.0.0.1:4416/get_pot";

/// The catalog client used for search and catalog pages.
const PAGES_CLIENT: &str = "WEB_REMIX";
/// The client that hands out plain stream addresses: nothing to decipher, no token to make.
const STREAM_CLIENT: &str = "VISIONOS";
/// A browser session id kept this long before a fresh one is asked for.
const VISITOR_KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

trait LiveBackend: Send + Sync {
    fn search(
        &self,
        query: &str,
        filter: MusicSearchFilter,
        continuation: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<SearchPage, AccessError>;

    /// The first page of Songs and the unfiltered search, asked for at the same time. The
    /// unfiltered one may fail on its own; the songs still come back.
    fn search_songs_with_mix(
        &self,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<(SearchPage, Result<Value, AccessError>), AccessError>;

    /// An album's songs, in order.
    fn album(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteTrack>, AccessError>;

    /// A catalog page as it comes (an artist, an artist's song list, a playlist).
    fn browse(&self, browse_id: &str, cancel: &CancellationToken) -> Result<Value, AccessError>;

    /// The next page of a catalog page (a long playlist's next songs), from its continuation.
    fn browse_continuation(
        &self,
        token: &str,
        cancel: &CancellationToken,
    ) -> Result<Value, AccessError>;

    fn resolve(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError>;
}

/// The live catalog over the shared YouTube Music client.
struct YtmBackend {
    runtime: Background,
    it: ytmusic::InnerTube,
    clients: ytmusic::Clients,
    /// The browser session id, asked for once (or read from disk) and reused.
    visitor: tokio::sync::OnceCell<String>,
    /// The older resolver, started only the first time both faster ways fail.
    legacy: OnceLock<Result<LegacyResolver, AccessError>>,
}

impl YtmBackend {
    fn new() -> Result<Self, AccessError> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("venuta-catalog")
            .enable_all()
            .build()
            .map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "could not start the live catalog runtime",
                )
            })?;
        let runtime = Background(Some(runtime));
        let region = crate::region::catalog_region();
        let session = ytmusic::Session {
            locale: ytmusic::Locale {
                gl: region.country,
                hl: region.language,
            },
            ..Default::default()
        };
        let it = ytmusic::InnerTube::new(session, None).map_err(|_| {
            AccessError::new(
                AccessErrorKind::Transport,
                "could not start the live catalog client",
            )
        })?;
        let backend = Self {
            runtime,
            it,
            clients: ytmusic::Clients::bundled(),
            visitor: tokio::sync::OnceCell::new(),
            legacy: OnceLock::new(),
        };
        Ok(backend)
    }

    /// Run a request to the end on the caller's thread. From inside an async runtime (where
    /// blocking would stall it) the request runs on a helper thread instead.
    fn run<T: Send>(&self, future: impl Future<Output = T> + Send) -> T {
        if tokio::runtime::Handle::try_current().is_ok() {
            thread::scope(|s| {
                s.spawn(|| self.runtime.get().block_on(future))
                    .join()
                    .expect("catalog request thread")
            })
        } else {
            self.runtime.get().block_on(future)
        }
    }

    fn client(&self, key: &str) -> Result<&ytmusic::YouTubeClient, AccessError> {
        self.clients.get(key).ok_or_else(|| {
            AccessError::new(
                AccessErrorKind::Unsupported,
                format!("no {key} catalog identity"),
            )
        })
    }

    /// The browser session id, from disk when a recent one is kept, else asked for and kept.
    /// A failure is not remembered: the next song asks again.
    async fn ensure_visitor(&self) -> Option<String> {
        let id = self
            .visitor
            .get_or_try_init(|| async {
                if let Some(kept) = read_kept_visitor() {
                    return Ok(kept);
                }
                let fresh = self.it.fetch_visitor_data().await.map_err(|_| ())?;
                keep_visitor(&fresh);
                Ok::<_, ()>(fresh)
            })
            .await
            .ok()?
            .clone();
        if self.it.visitor_data().as_deref() != Some(id.as_str()) {
            self.it.set_visitor_data(Some(id.clone()));
        }
        Some(id)
    }

    async fn search_raw(
        &self,
        query: &str,
        params: Option<&str>,
        continuation: Option<&str>,
    ) -> Result<Value, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let result = match continuation {
            Some(token) => {
                self.it
                    .search_continuation_json(client, query, params, token)
                    .await
            }
            None => self.it.search_json(client, query, params).await,
        };
        result.map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog search failed"))
    }

    /// The song played inside the app: the catalog's stream identity with a plain address,
    /// checked to play to its end. Fails (to the next way) when that identity is refused.
    async fn resolve_direct(&self, video_id: &str) -> Result<EphemeralAudio, AccessError> {
        let client = self.client(STREAM_CLIENT)?;
        self.ensure_visitor().await;
        let response = self
            .it
            .player(client, video_id, None, None, None)
            .await
            .map_err(|_| {
                AccessError::new(AccessErrorKind::Transport, "catalog stream lookup failed")
            })?;
        if !response.playability_status.is_ok() {
            return Err(AccessError::new(
                AccessErrorKind::Unsupported,
                format!(
                    "the catalog would not play this song ({})",
                    response.playability_status.status
                ),
            ));
        }
        let data = response.streaming_data.as_ref().ok_or_else(|| {
            AccessError::new(
                AccessErrorKind::Unsupported,
                "catalog returned no stream data",
            )
        })?;
        let format = crate::stream_pick::pick_audio(data, crate::stream_pick::PLAYS_OPUS)
            .ok_or_else(|| {
                AccessError::new(
                    AccessErrorKind::Unsupported,
                    "no playable audio stream offered",
                )
            })?;
        let url = format.direct_url().unwrap_or_default().to_owned();
        let content_length = format
            .content_length
            .as_deref()
            .and_then(|n| n.parse().ok());
        let headers = vec![MediaHeader {
            name: "User-Agent".into(),
            value: client.user_agent.clone(),
        }];
        let (check_url, check_headers) = (url.clone(), headers.clone());
        let plays = tokio::task::spawn_blocking(move || {
            crate::media_http::serves_to_the_end(&check_url, &check_headers, content_length)
        })
        .await
        .unwrap_or(false);
        if !plays {
            return Err(AccessError::new(
                AccessErrorKind::Unsupported,
                "the stream address would stop partway",
            ));
        }
        Ok(EphemeralAudio {
            track_id: video_id.to_owned(),
            provider: PROVIDER.into(),
            url,
            mime_type: Some(format.mime_type.clone()),
            content_length,
            duration_ms: format
                .approx_duration_ms
                .as_deref()
                .and_then(|d| d.parse().ok()),
            headers,
            via: "catalog",
        })
    }

    fn legacy(&self) -> Result<&LegacyResolver, AccessError> {
        self.legacy
            .get_or_init(LegacyResolver::new)
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// A runtime that shuts down without waiting when dropped, so the catalog can be dropped from
/// anywhere, an async context included (where a plain runtime's drop would panic).
struct Background(Option<Runtime>);

impl Background {
    fn get(&self) -> &Runtime {
        self.0.as_ref().expect("catalog runtime")
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

fn parse_search(
    query: &str,
    filter: MusicSearchFilter,
    raw: Value,
) -> Result<SearchPage, AccessError> {
    let parsed = parse_music_search_response(query, Some(filter), &raw).map_err(|_| {
        AccessError::new(
            AccessErrorKind::MalformedResponse,
            "catalog search data could not be parsed",
        )
    })?;
    Ok(SearchPage { parsed, raw })
}

impl LiveBackend for YtmBackend {
    fn search(
        &self,
        query: &str,
        filter: MusicSearchFilter,
        continuation: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<SearchPage, AccessError> {
        let raw = self.run(cancellable(
            cancel.clone(),
            self.search_raw(query, Some(search_params(filter)), continuation.as_deref()),
        ))?;
        parse_search(query, filter, raw)
    }

    fn search_songs_with_mix(
        &self,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<(SearchPage, Result<Value, AccessError>), AccessError> {
        let (songs, mix) = self.run(cancellable(cancel.clone(), async {
            let (songs, mix) = tokio::join!(
                self.search_raw(query, Some(SONGS_PARAMS), None),
                self.search_raw(query, None, None),
            );
            Ok((songs?, mix))
        }))?;
        Ok((parse_search(query, MusicSearchFilter::Songs, songs)?, mix))
    }

    fn album(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteTrack>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let page = self.run(cancellable(cancel.clone(), async {
            self.it.album(client, album_id).await.map_err(|_| {
                AccessError::new(AccessErrorKind::Transport, "catalog album lookup failed")
            })
        }))?;
        Ok(album_tracks_of(page, album_id))
    }

    fn browse(&self, browse_id: &str, cancel: &CancellationToken) -> Result<Value, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.it
                .browse_json(client, browse_id)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog page failed"))
        }))
    }

    fn browse_continuation(
        &self,
        token: &str,
        cancel: &CancellationToken,
    ) -> Result<Value, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.it
                .browse_continuation_json(client, token)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog page failed"))
        }))
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
        // 1) Inside the app: one request and one check, no extra program.
        match self.run(cancellable(cancel.clone(), self.resolve_direct(video_id))) {
            Ok(audio) => return Ok(audio),
            Err(e) if e.kind == AccessErrorKind::Cancelled => return Err(e),
            Err(_) => {}
        }
        // 2) The stream tool, kept up to date on its own.
        if let Ok(audio) = crate::stream_resolve::resolve_with_stream_tool(video_id) {
            return Ok(audio);
        }
        // 3) The older multi-identity resolver, started only now.
        self.legacy()?.resolve(video_id, cancel)
    }
}

/// An album page's songs as catalog tracks: the album's own title, id and cover on each.
fn album_tracks_of(page: ytmusic::AlbumPage, album_id: &str) -> Vec<RemoteTrack> {
    let cover = clean_artwork_url(page.thumbnail.clone());
    let album_title = page.title.clone();
    page.items
        .into_iter()
        .filter(|s| !s.video_id.trim().is_empty())
        .map(|s| {
            let named: Vec<String> = s
                .artist_runs
                .iter()
                .map(|r| r.text.trim().to_owned())
                .filter(|t| !t.is_empty() && !is_name_joiner(t))
                .collect();
            let artists = if named.is_empty() {
                page.artist.clone().into_iter().collect()
            } else {
                named
            };
            RemoteTrack {
                provider: PROVIDER.into(),
                track_id: s.video_id.trim().to_owned(),
                title: s.title,
                artists,
                album: album_title.clone(),
                album_id: Some(s.album_id.unwrap_or_else(|| album_id.to_owned())),
                duration_ms: s.duration.as_deref().and_then(catalog_json::parse_duration),
                artwork_url: clean_artwork_url(s.thumbnail).or_else(|| cover.clone()),
                plays: s.play_count.as_deref().and_then(catalog_json::parse_count),
                ..Default::default()
            }
        })
        .collect()
}

/// The words and marks between names in a credit line ("A, B & C").
fn is_name_joiner(text: &str) -> bool {
    matches!(text, "&" | "," | "and" | "•" | "x" | "X" | "feat." | "ft.")
}

fn visitor_file() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)?;
    Some(base.join("Venuta").join("catalog-session.txt"))
}

fn read_kept_visitor() -> Option<String> {
    let path = visitor_file()?;
    let age = std::fs::metadata(&path)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    if age > VISITOR_KEEP {
        return None;
    }
    let id = std::fs::read_to_string(path).ok()?.trim().to_owned();
    (!id.is_empty() && id.len() < 512).then_some(id)
}

fn keep_visitor(id: &str) {
    if let Some(path) = visitor_file() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, id);
    }
}

type ClientResult = Result<Arc<Innertube>, Arc<str>>;
type ApiJob = Box<dyn FnOnce(Arc<Runtime>, ClientResult) + Send + 'static>;

/// The resolver Venuta used before the in-app one: several older catalog identities tried in
/// turn. Kept as the last way to play a song, and started only when it is needed, because
/// starting it downloads and compiles the catalog's player script.
struct LegacyResolver {
    jobs: SyncSender<ApiJob>,
}

impl LegacyResolver {
    fn new() -> Result<Self, AccessError> {
        let runtime = Arc::new(
            Builder::new_multi_thread()
                .worker_threads(1)
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
            .name("venuta-legacy-resolver".into())
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

    fn resolve(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
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

fn run_api_actor(runtime: Arc<Runtime>, receiver: Receiver<ApiJob>) {
    let client = runtime
        .block_on(Innertube::new())
        .map(Arc::new)
        .map_err(|_| Arc::<str>::from("initialization failed"));
    while let Ok(job) = receiver.recv() {
        job(runtime.clone(), client.clone());
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
        (
            Some("IOS"),
            None,
            "https://www.youtube.com",
            "https://www.youtube.com/",
        ),
        (
            Some("WEB_EMBEDDED"),
            None,
            "https://www.youtube.com",
            "https://www.youtube.com/",
        ),
        (
            Some("ANDROID_VR"),
            None,
            "https://www.youtube.com",
            "https://www.youtube.com/",
        ),
        (
            Some("ANDROID"),
            None,
            "https://www.youtube.com",
            "https://www.youtube.com/",
        ),
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
        (
            None,
            None,
            "https://www.youtube.com",
            "https://www.youtube.com/",
        ),
    ];

    let mut last_err: Option<AccessError> = None;
    for (client_name, pot, origin, referer) in attempts {
        match try_resolve_with_client(
            client,
            video_id,
            client_name,
            pot.as_deref(),
            origin,
            referer,
        )
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

    let url =
        resolve_stream_url_full(format, &client.player.decipherer, pot, None).map_err(|_| {
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
        via: "fallback",
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
            backend: Arc::new(YtmBackend::new()?),
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
            // The first page also asks the unfiltered search, for the catalog's top result and
            // each song's place there. If that one fails, the songs alone are still the answer.
            let (result, mix) = if cursor.continuation.is_none() {
                let (songs, mix) = self.backend.search_songs_with_mix(query, cancel)?;
                (songs, mix.ok())
            } else {
                let songs = self.backend.search(
                    query,
                    MusicSearchFilter::Songs,
                    cursor.continuation.clone(),
                    cancel,
                )?;
                (songs, None)
            };
            // innertube-rs reads no continuation from a Songs shelf: it's read from the response.
            let continuation = result
                .parsed
                .continuation_token
                .or_else(|| catalog_json::read_continuation(&result.raw));
            let (mut tracks, skipped_tracks) = normalize_tracks(result.parsed.songs, None)?;
            let song_hits = catalog_json::read_hits(&result.raw);
            // A page innertube-rs couldn't read (a continuation page): its rows, read here.
            if tracks.is_empty() && !song_hits.is_empty() {
                tracks = catalog_json::tracks_from_hits(&song_hits, PROVIDER);
            }
            let mix_hits = mix
                .as_ref()
                .map(|raw| (catalog_json::read_hits(raw), catalog_json::read_top(raw)));
            catalog_json::enrich(
                &mut tracks,
                &song_hits,
                mix_hits
                    .as_ref()
                    .map(|(hits, top)| (hits.as_slice(), top.as_ref())),
                PROVIDER,
            );
            for t in &mut tracks {
                t.artwork_url = clean_artwork_url(t.artwork_url.take());
            }
            if cursor.offset == 0 {
                catalog_json::keep_top_in_page(&mut tracks, page.limit);
            }

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
        let page_result = self.backend.search(
            query,
            MusicSearchFilter::Albums,
            cursor.continuation.clone(),
            cancel,
        )?;
        // innertube-rs reads no album rows of a real Albums shelf (rows without a video id),
        // so they're read from the response itself; its own parse stays a fallback.
        let raw_albums = catalog_json::read_albums(&page_result.raw);
        let result = page_result.parsed;
        let albums: Vec<RemoteAlbum> = if raw_albums.is_empty() {
            result
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
                        ..Default::default()
                    })
                })
                .collect()
        } else {
            raw_albums
                .into_iter()
                .map(|a| RemoteAlbum {
                    provider: PROVIDER.into(),
                    album_id: a.browse_id,
                    title: a.title,
                    artists: a.artists,
                    track_count: None,
                    release_date: a.year,
                    artwork_url: clean_artwork_url(a.thumbnail),
                    kind: a.kind,
                })
                .collect()
        };
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
        let page_result = self.backend.search(
            query,
            MusicSearchFilter::Artists,
            cursor.continuation.clone(),
            cancel,
        )?;
        // As for albums: the response's own artist rows, innertube-rs's parse as a fallback.
        let raw_artists = catalog_json::read_artists(&page_result.raw);
        let result = page_result.parsed;
        let artists: Vec<RemoteArtist> = if raw_artists.is_empty() {
            result
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
                        ..Default::default()
                    })
                })
                .collect()
        } else {
            raw_artists
                .into_iter()
                .map(|a| RemoteArtist {
                    provider: PROVIDER.into(),
                    artist_id: a.browse_id,
                    name: a.name,
                    artwork_url: clean_artwork_url(a.thumbnail),
                    audience: a.audience,
                })
                .collect()
        };
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
        let tracks = self.backend.album(album_id.trim(), cancel)?;
        if tracks.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::MalformedResponse,
                "catalog album tracks contained no usable track IDs",
            ));
        }
        page_batch(tracks, None, cursor, CursorKind::AlbumTracks, page.limit)
    }

    fn artist_page(
        &self,
        artist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RemoteArtistPage, AccessError> {
        let id = artist_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "artist id must not be empty",
            ));
        }
        let raw = self.backend.browse(id, cancel)?;
        let page = catalog_json::read_artist_page(&raw);
        if page.name.trim().is_empty() && page.top_songs.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::NotFound,
                "that artist isn't in the online catalog",
            ));
        }
        let top_songs: Vec<RemoteTrack> = catalog_json::tracks_from_hits(&page.top_songs, PROVIDER)
            .into_iter()
            .map(|mut t| {
                t.artwork_url = clean_artwork_url(t.artwork_url.take());
                t
            })
            .collect();
        // More of their songs, from their full song list (a second page; skipped if it fails).
        let songs = page
            .songs_list_id
            .as_deref()
            .and_then(|list| self.backend.browse(list, cancel).ok())
            .map(|raw| catalog_json::tracks_from_hits(&catalog_json::read_hits(&raw), PROVIDER))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !top_songs.iter().any(|top| top.track_id == t.track_id))
            .take(100)
            .map(|mut t| {
                t.artwork_url = clean_artwork_url(t.artwork_url.take());
                t
            })
            .collect();
        let album = |c: catalog_json::RawCard| RemoteAlbum {
            provider: PROVIDER.into(),
            release_date: c.year(),
            kind: c.kind(),
            album_id: c.browse_id,
            title: c.title,
            artists: vec![page.name.clone()],
            track_count: None,
            artwork_url: clean_artwork_url(c.thumbnail),
        };
        Ok(RemoteArtistPage {
            provider: PROVIDER.into(),
            artist_id: id.to_owned(),
            name: page.name.clone(),
            artwork_url: clean_artwork_url(page.thumbnail.clone()),
            audience: page.audience,
            top_songs,
            songs,
            albums: page.albums.clone().into_iter().map(album).collect(),
            singles: page.singles.clone().into_iter().map(album).collect(),
            similar: page
                .similar
                .clone()
                .into_iter()
                .map(|c| RemoteArtist {
                    provider: PROVIDER.into(),
                    audience: c.audience(),
                    artist_id: c.browse_id,
                    name: c.title,
                    artwork_url: clean_artwork_url(c.thumbnail),
                })
                .collect(),
        })
    }

    fn playlist_tracks(
        &self,
        playlist_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        let cursor = decode_cursor(page.cursor.as_deref(), CursorKind::PlaylistTracks)?;
        // The first page by the playlist's id; the next ones by the continuation it gave.
        let raw = match cursor.continuation.as_deref() {
            Some(token) => self.backend.browse_continuation(token, cancel)?,
            None => self
                .backend
                .browse(&playlist_browse_id(playlist_id)?, cancel)?,
        };
        let tracks = catalog_json::tracks_from_hits(&catalog_json::read_hits(&raw), PROVIDER);
        let next = catalog_json::read_playlist_continuation(&raw);
        page_batch(tracks, next, cursor, CursorKind::PlaylistTracks, page.limit)
    }

    fn playlist(
        &self,
        playlist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RemotePlaylist, AccessError> {
        let raw = self
            .backend
            .browse(&playlist_browse_id(playlist_id)?, cancel)?;
        let title = catalog_json::read_playlist_title(&raw);
        let mut tracks = catalog_json::tracks_from_hits(&catalog_json::read_hits(&raw), PROVIDER);
        let mut next = catalog_json::read_playlist_continuation(&raw);
        // At most 60 more pages (6,000 songs, more than YouTube keeps in one playlist).
        for _ in 0..60 {
            let Some(token) = next.take() else { break };
            let raw = self.backend.browse_continuation(&token, cancel)?;
            tracks.extend(catalog_json::tracks_from_hits(
                &catalog_json::read_hits(&raw),
                PROVIDER,
            ));
            next = catalog_json::read_playlist_continuation(&raw).filter(|t| *t != token);
        }
        Ok(RemotePlaylist { title, tracks })
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
        ..Default::default()
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
    PlaylistTracks,
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

/// A playlist's page id: "VL" and its list id (as a share link has it, or already with VL).
fn playlist_browse_id(playlist_id: &str) -> Result<String, AccessError> {
    let id = playlist_id.trim();
    if id.is_empty() {
        return Err(AccessError::new(
            AccessErrorKind::InvalidInput,
            "playlist id must not be empty",
        ));
    }
    Ok(if id.starts_with("VL") {
        id.to_owned()
    } else {
        format!("VL{id}")
    })
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
