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
    CancellationToken, CatalogLyrics, EphemeralAudio, MediaHeader, MoodSection, MoodTile, Page,
    PageRequest, RadioEndpoint, RemoteAccount, RemoteAlbum, RemoteAlbumPage, RemoteArtist,
    RemoteArtistLink, RemoteArtistPage, RemoteCollection, RemoteHome, RemoteMore, RemotePlaylist,
    RemoteRadio, RemoteRating, RemoteSuggestion, RemoteTrack, SearchSuggestions,
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

    /// An album's page: its header, its songs in order, and the rows under them.
    fn album(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<ytmusic::AlbumPage, AccessError>;

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

    /// The song's smallest audio stream, inside the app only (no stream tool): for reading its
    /// shape, never for playing.
    fn resolve_small(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError>;

    fn suggest(
        &self,
        input: &str,
        cancel: &CancellationToken,
    ) -> Result<SearchSuggestions, AccessError>;

    fn search_cards(
        &self,
        query: &str,
        kind: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError>;

    fn search_videos(
        &self,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteTrack>, AccessError>;

    fn moods(&self, cancel: &CancellationToken) -> Result<Vec<MoodSection>, AccessError>;

    fn mood(
        &self,
        params: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError>;

    /// The catalog's queue for an endpoint, as it answers (`continuation`: the page after the one
    /// that issued it). One request, nothing followed.
    fn watch(
        &self,
        endpoint: &ytmusic::WatchEndpoint,
        continuation: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<ytmusic::NextResult, AccessError>;

    /// The songs of a Related page (`MPTRt…`).
    fn related(
        &self,
        browse_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<ytmusic::SongItem>, AccessError>;

    fn set_blocked(&self, blocked: &[(Option<String>, String)]);

    fn set_cookie(&self, cookie: Option<&str>);

    fn cookie(&self) -> Option<String>;

    fn account(&self, cancel: &CancellationToken) -> Result<Option<RemoteAccount>, AccessError>;

    fn library_playlists(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError>;

    fn rate(
        &self,
        video_id: &str,
        rating: RemoteRating,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    fn logged_in(&self) -> bool;

    /// The account's part of the catalog (see `crate::account`).
    fn account_api(self: Arc<Self>) -> Option<Arc<dyn crate::account::AccountAccess>> {
        None
    }

    /// The home feed (see `MediaAccess::home`).
    fn home(
        &self,
        params: Option<&str>,
        continuation: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<ytmusic::HomePage, AccessError>;

    /// A song's lyrics from the catalog (see `MediaAccess::lyrics`).
    fn lyrics(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<CatalogLyrics>, AccessError>;

    fn set_hide_videos(&self, on: bool);

    /// See `MediaAccess::set_personal`.
    fn set_personal(&self, on: bool);

    /// See `MediaAccess::set_signed_search`.
    fn set_signed_search(&self, on: bool);

    fn set_region(&self, country: Option<&str>);

    fn set_data_saver(&self, on: bool);

    /// The cards of a page that is one grid (a row's "See all").
    fn grid(
        &self,
        browse_id: &str,
        params: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteSuggestion>, AccessError>;
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
    /// Data saver: the smallest stream instead of the best.
    saver: std::sync::atomic::AtomicBool,
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
            saver: std::sync::atomic::AtomicBool::new(false),
        };
        // No cookie can be minted again from here: the heal is declared over at once, so a
        // request waiting on it is sent once more now instead of after the transport's long
        // timeout. Only a request refused again after that marks the session expired (the
        // transport keeps that), so one refusal never asks the person to sign in again.
        {
            let it = backend.it.clone();
            let rejected = it.session_rejected();
            backend.runtime.get().spawn(async move {
                loop {
                    rejected.notified().await;
                    drop(it.begin_heal());
                }
            });
        }
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
        let saver = self.saver.load(std::sync::atomic::Ordering::Relaxed);
        self.resolve_direct_as(video_id, saver).await
    }

    /// As `resolve_direct`, the smallest stream when `smallest` (data saver, or a song's shape).
    async fn resolve_direct_as(
        &self,
        video_id: &str,
        smallest: bool,
    ) -> Result<EphemeralAudio, AccessError> {
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
        let format =
            crate::stream_pick::pick_audio_for(data, crate::stream_pick::PLAYS_OPUS, smallest)
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
            loudness_db: format.loudness_db.or_else(|| {
                response
                    .player_config
                    .as_ref()
                    .and_then(|c| c.audio_config.as_ref())
                    .and_then(|a| a.loudness_db)
            }),
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
    fn account_api(self: Arc<Self>) -> Option<Arc<dyn crate::account::AccountAccess>> {
        Some(self)
    }

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
    ) -> Result<ytmusic::AlbumPage, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.it.album(client, album_id).await.map_err(|_| {
                AccessError::new(AccessErrorKind::Transport, "catalog album lookup failed")
            })
        }))
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

    fn resolve_small(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        self.run(cancellable(
            cancel.clone(),
            self.resolve_direct_as(video_id, true),
        ))
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

    fn suggest(
        &self,
        input: &str,
        cancel: &CancellationToken,
    ) -> Result<SearchSuggestions, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let found = self.run(cancellable(cancel.clone(), async {
            self.it
                .search_suggestions(client, input)
                .await
                .map_err(|_| {
                    AccessError::new(AccessErrorKind::Transport, "catalog suggestions failed")
                })
        }))?;
        Ok(crate::catalog_cards::suggestions_of(&found))
    }

    fn search_cards(
        &self,
        query: &str,
        kind: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let cards = self.run(cancellable(cancel.clone(), async {
            self.it
                .search_cards(client, query, kind)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog search failed"))
        }))?;
        let kind = if kind == "podcasts" {
            "podcast"
        } else {
            "playlist"
        };
        Ok(cards
            .iter()
            .filter(|c| c.kind != "song")
            .filter_map(|c| crate::catalog_cards::collection_of(c, kind))
            .collect())
    }

    fn search_videos(
        &self,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteTrack>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let found = self.run(cancellable(cancel.clone(), async {
            self.it
                .search_videos(client, query)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog search failed"))
        }))?;
        Ok(found
            .items
            .iter()
            .filter_map(crate::catalog_cards::track_of_row)
            .collect())
    }

    fn moods(&self, cancel: &CancellationToken) -> Result<Vec<MoodSection>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let sections = self.run(cancellable(cancel.clone(), async {
            self.it
                .moods(client)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog moods failed"))
        }))?;
        Ok(sections
            .into_iter()
            .map(|s| MoodSection {
                title: s.title,
                tiles: s
                    .items
                    .into_iter()
                    .map(|m| MoodTile {
                        title: m.title,
                        params: m.params,
                        color: m.color,
                    })
                    .collect(),
            })
            .collect())
    }

    fn mood(
        &self,
        params: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let cards = self.run(cancellable(cancel.clone(), async {
            self.it
                .browse_grid(client, "FEmusic_moods_and_genres_category", Some(params))
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog page failed"))
        }))?;
        Ok(cards
            .iter()
            .filter(|c| c.kind == "playlist" || c.kind == "album")
            .filter_map(|c| crate::catalog_cards::collection_of(c, "playlist"))
            .collect())
    }

    fn watch(
        &self,
        endpoint: &ytmusic::WatchEndpoint,
        continuation: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<ytmusic::NextResult, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.it
                .next_watch(client, endpoint, continuation)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog radio failed"))
        }))
    }

    fn related(
        &self,
        browse_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<ytmusic::SongItem>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.it.related_songs(client, browse_id).await.map_err(|_| {
                AccessError::new(AccessErrorKind::Transport, "catalog related songs failed")
            })
        }))
    }

    fn set_blocked(&self, blocked: &[(Option<String>, String)]) {
        self.it
            .set_blocked(ytmusic::BlockList::new(blocked.iter().cloned()));
    }

    fn home(
        &self,
        params: Option<&str>,
        continuation: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<ytmusic::HomePage, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.ensure_visitor().await;
            let page = match continuation {
                Some(token) => self.it.home_continuation(client, token).await,
                None => self.it.home(client, params).await,
            };
            page.map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog home failed"))
        }))
    }

    fn lyrics(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<CatalogLyrics>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let failed = |_| AccessError::new(AccessErrorKind::Transport, "catalog lyrics failed");
        self.run(cancellable(cancel.clone(), async {
            self.ensure_visitor().await;
            // The lyrics are a page of their own, named by the song's "next" answer.
            let next = self
                .it
                .next(client, Some(video_id), None)
                .await
                .map_err(failed)?;
            let Some(page) = next.lyrics_browse_id else {
                return Ok(None);
            };
            // Timed lines come only to the mobile identity; the words alone to the web one.
            if let Some(mobile) = self.clients.get(ytmusic::LYRICS_TIMED_CLIENT)
                && let Ok(timed) = self.it.lyrics_timed(mobile, &page).await
                && !timed.is_empty()
            {
                return Ok(Some(CatalogLyrics {
                    lines: timed.into_iter().map(|l| (l.time_ms, l.text)).collect(),
                    text: String::new(),
                    credit: None,
                }));
            }
            Ok(self
                .it
                .lyrics_plain(client, &page)
                .await
                .map_err(failed)?
                .filter(|p| !p.text.trim().is_empty())
                .map(|p| CatalogLyrics {
                    lines: Vec::new(),
                    text: p.text,
                    credit: p.footer,
                }))
        }))
    }

    fn set_hide_videos(&self, on: bool) {
        self.it.set_hide_videos(on);
    }

    fn set_personal(&self, on: bool) {
        self.it.set_personal(on);
    }

    fn set_signed_search(&self, on: bool) {
        self.it.set_signed_search(on);
    }

    fn set_region(&self, country: Option<&str>) {
        let gl = country
            .and_then(crate::region::country_code)
            .unwrap_or_else(|| crate::region::catalog_region().country);
        self.it.set_country(&gl);
    }

    fn set_data_saver(&self, on: bool) {
        self.saver.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    fn set_cookie(&self, cookie: Option<&str>) {
        // Each sign-in (or sign-out) acts as itself: a brand channel chosen under the last one is
        // chosen again under this one (the app does that at start, from what it kept).
        self.it.set_data_sync_id(None);
        self.it.set_cookie(
            cookie
                .map(str::to_owned)
                .filter(|c| ytmusic::cookie_sapisid(c).is_some()),
        );
    }

    fn cookie(&self) -> Option<String> {
        self.it.cookie()
    }

    fn account(&self, cancel: &CancellationToken) -> Result<Option<RemoteAccount>, AccessError> {
        if !self.it.is_logged_in() {
            return Ok(None);
        }
        let client = self.client(PAGES_CLIENT)?;
        let info = self.run(cancellable(cancel.clone(), async {
            self.it.account_menu(client).await.map_err(|_| {
                AccessError::new(AccessErrorKind::Transport, "the account didn't answer")
            })
        }))?;
        // The account answered as itself: whatever was refused before, the sign-in works.
        self.it.session_accepted();
        Ok(Some(RemoteAccount {
            name: info.name,
            email: info.email,
            handle: info.handle,
            picture: info.thumbnail,
        }))
    }

    fn library_playlists(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError> {
        if !self.it.is_logged_in() {
            return Ok(Vec::new());
        }
        let client = self.client(PAGES_CLIENT)?;
        let cards = self.run(cancellable(cancel.clone(), async {
            self.it.library_playlists(client).await.map_err(|_| {
                AccessError::new(
                    AccessErrorKind::Transport,
                    "the account's playlists didn't come",
                )
            })
        }))?;
        Ok(cards
            .iter()
            .filter(|c| c.kind == "playlist")
            .filter_map(|c| crate::catalog_cards::collection_of(c, "playlist"))
            .collect())
    }

    fn rate(
        &self,
        video_id: &str,
        rating: RemoteRating,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        if !self.it.is_logged_in() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "sign in to YouTube Music first",
            ));
        }
        let rating = match rating {
            RemoteRating::Like => ytmusic::Rating::Like,
            RemoteRating::Dislike => ytmusic::Rating::Dislike,
            RemoteRating::None => ytmusic::Rating::Indifferent,
        };
        let client = self.client(PAGES_CLIENT)?;
        self.run(cancellable(cancel.clone(), async {
            self.it.rate(client, video_id, rating).await.map_err(|_| {
                AccessError::new(AccessErrorKind::Transport, "YouTube Music didn't take it")
            })
        }))
    }

    fn logged_in(&self) -> bool {
        self.it.is_logged_in()
    }

    fn grid(
        &self,
        browse_id: &str,
        params: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteSuggestion>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let cards = self.run(cancellable(cancel.clone(), async {
            self.it
                .browse_grid(client, browse_id, params)
                .await
                .map_err(|_| AccessError::new(AccessErrorKind::Transport, "catalog page failed"))
        }))?;
        Ok(cards
            .iter()
            .filter_map(crate::catalog_cards::suggestion_of)
            .collect())
    }
}

/// An album page's songs as catalog tracks: the album's own title, id and cover on each.
fn album_tracks_of(page: &ytmusic::AlbumPage, album_id: &str) -> Vec<RemoteTrack> {
    let cover = clean_artwork_url(page.thumbnail.clone());
    let album_title = page.title.clone();
    page.items
        .iter()
        .filter(|s| !s.video_id.trim().is_empty())
        .cloned()
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
                plays: s
                    .play_count
                    .as_deref()
                    .and_then(crate::catalog_cards::plays),
                ..Default::default()
            }
        })
        .collect()
}

/// An album's whole page: what it is, who made it, its songs, and the rows under them.
fn album_page_of(page: &ytmusic::AlbumPage, album_id: &str) -> RemoteAlbumPage {
    let text = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let mut artists: Vec<RemoteArtistLink> = page
        .artist_runs
        .iter()
        .filter(|r| !r.text.trim().is_empty() && !is_name_joiner(r.text.trim()))
        .map(|r| RemoteArtistLink {
            name: r.text.trim().to_owned(),
            artist_id: r.id.clone().filter(|id| !id.trim().is_empty()),
        })
        .collect();
    if artists.is_empty()
        && let Some(name) = text(&page.artist)
    {
        artists.push(RemoteArtistLink {
            name,
            artist_id: page.artist_id.clone(),
        });
    }
    let subtitle = text(&page.subtitle);
    let words: Vec<&str> = subtitle
        .as_deref()
        .map(|s| s.split('•').map(str::trim).collect())
        .unwrap_or_default();
    let tracks = album_tracks_of(page, album_id);
    RemoteAlbumPage {
        album: RemoteAlbum {
            provider: PROVIDER.into(),
            album_id: album_id.to_owned(),
            title: text(&page.title).unwrap_or_default(),
            artists: artists.iter().map(|a| a.name.clone()).collect(),
            track_count: u32::try_from(tracks.len()).ok(),
            release_date: words
                .iter()
                .rfind(|w| w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit()))
                .map(|w| (*w).to_owned()),
            artwork_url: clean_artwork_url(page.thumbnail.clone()),
            kind: words
                .first()
                .map(|w| (*w).to_owned())
                .filter(|w| !w.is_empty()),
        },
        artists,
        subtitle,
        length: text(&page.second_subtitle),
        description: text(&page.description),
        explicit: page.explicit,
        tracks,
        shelves: page
            .sections
            .iter()
            .filter_map(crate::catalog_cards::shelf_of)
            .collect(),
        playlist_id: text(&page.playlist_id),
        radio: page.radio.as_ref().map(crate::catalog_cards::endpoint_of),
    }
}

/// A playlist page's radio, as its header's Start radio names it (None when it names none).
fn playlist_radio(raw: &Value) -> Option<RadioEndpoint> {
    ytmusic::models::browse::header_radio(raw)
        .as_ref()
        .map(crate::catalog_cards::endpoint_of)
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
        loudness_db: None,
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

// --- The signed-in account (crate::account) --------------------------------------------------

/// What a failed account request tells the person.
fn account_error(e: ytmusic::Error, what: &str) -> AccessError {
    match e {
        ytmusic::Error::SessionExpired => AccessError::new(
            AccessErrorKind::Transport,
            "YouTube Music needs you to sign in again.",
        ),
        ytmusic::Error::AlreadyInPlaylist => {
            AccessError::new(AccessErrorKind::InvalidInput, "Already in the playlist.")
        }
        ytmusic::Error::WriteUncertain => AccessError::new(
            AccessErrorKind::Transport,
            format!(
                "YouTube Music didn't say whether {what} went through. Look there before trying again."
            ),
        ),
        ytmusic::Error::SlowDown => AccessError::new(
            AccessErrorKind::Transport,
            "YouTube Music asked Venuta to slow down. Try again in a few minutes.",
        ),
        other => AccessError::new(
            AccessErrorKind::Transport,
            format!("YouTube Music didn't answer about {what}: {other}"),
        ),
    }
}

fn signed_out() -> AccessError {
    AccessError::new(
        AccessErrorKind::InvalidInput,
        "Sign in to YouTube Music first.",
    )
}

/// A song row as the account's lists hold it: the song, its place, whether it's an upload.
fn account_track_of(row: &ytmusic::SongItem) -> Option<crate::account::AccountTrack> {
    Some(crate::account::AccountTrack {
        track: crate::catalog_cards::track_of_row(row)?,
        set_video_id: row.set_video_id.clone(),
        upload: row.is_upload,
    })
}

/// Whether a reading of a playlist got every song: no page failed or was left unread, every row
/// is a song Venuta can name, and there are at least as many as the header says. Unsure reads as
/// not whole, so a linked playlist waits instead of taking the songs it missed as taken out
/// there (an unavailable song with no id would read just like that).
fn read_whole(said: Option<u32>, rows: &[ytmusic::SongItem], failed: bool, left: bool) -> bool {
    let named = rows
        .iter()
        .filter(|r| !r.video_id.trim().is_empty())
        .count();
    !failed && !left && named == rows.len() && said.is_none_or(|n| named >= n as usize)
}

impl YtmBackend {
    /// One account request, signed in, cancellable, its failure in words.
    fn account_call<T: Send>(
        &self,
        cancel: &CancellationToken,
        what: &str,
        call: impl Future<Output = Result<T, ytmusic::Error>> + Send,
    ) -> Result<T, AccessError> {
        if !self.it.is_logged_in() {
            return Err(signed_out());
        }
        let it = self.it.clone();
        self.run(cancellable(cancel.clone(), async move {
            let got = call.await;
            // Answered as the account (its library, its playlists): the sign-in works again, even
            // if something was refused before. A refusal after the retry is the transport's to mark.
            if got.is_ok() {
                it.session_accepted();
            }
            got.map_err(|e| account_error(e, what))
        }))
    }
}

impl crate::account::AccountAccess for YtmBackend {
    fn signed_in(&self) -> bool {
        self.it.is_logged_in()
    }

    fn session_expired(&self) -> bool {
        self.it.session_refused()
    }

    fn library(
        &self,
        shelf: crate::account::LibraryShelf,
        cancel: &CancellationToken,
    ) -> Result<Vec<crate::RemoteSuggestion>, AccessError> {
        use crate::account::LibraryShelf as S;
        let client = self.client(PAGES_CLIENT)?;
        let cards = self.account_call(cancel, "your library", async {
            match shelf {
                S::Playlists => self.it.library_playlists(client).await,
                S::Albums => self.it.library_albums(client).await,
                S::Artists => self.it.library_artists(client).await,
                S::Subscriptions => self.it.library_subscriptions(client).await,
                S::Podcasts => self.it.library_podcasts(client).await,
                S::UploadAlbums => self.it.upload_albums(client).await,
            }
        })?;
        Ok(cards
            .iter()
            .filter_map(|c| match (shelf, c.kind) {
                // Podcasts come as playlist-like cards: opened as podcasts.
                (S::Podcasts, _) => crate::catalog_cards::collection_of(c, "podcast")
                    .map(|collection| crate::RemoteSuggestion::Playlist { collection }),
                // Library artists and subscriptions come as list rows the card reader calls songs.
                (S::Artists | S::Subscriptions, _) => {
                    (!c.id.trim().is_empty()).then(|| crate::RemoteSuggestion::Artist {
                        artist: crate::RemoteArtist {
                            provider: PROVIDER.into(),
                            artist_id: c.id.trim().to_owned(),
                            name: c.title.clone(),
                            artwork_url: c.thumbnail.clone(),
                            audience: None,
                        },
                        subtitle: c.subtitle.clone(),
                    })
                }
                _ => crate::catalog_cards::suggestion_of(c),
            })
            .collect())
    }

    fn library_songs(
        &self,
        uploads: bool,
        cancel: &CancellationToken,
    ) -> Result<Vec<crate::account::AccountTrack>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let id = if uploads {
            "FEmusic_library_privately_owned_tracks"
        } else {
            "FEmusic_liked_videos"
        };
        let rows = self.account_call(cancel, "your songs", self.it.library_songs(client, id))?;
        Ok(rows.iter().filter_map(account_track_of).collect())
    }

    fn playlist(
        &self,
        playlist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<crate::account::AccountPlaylist, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let browse_id = playlist_browse_id(playlist_id)?;
        let (page, rest, failed, left) = self.account_call(cancel, "the playlist", async {
            let page = self.it.playlist(client, &browse_id, None).await?;
            let mut rest = Vec::new();
            let mut token = page.continuation.clone();
            let mut failed = false;
            // ponytail: 100 songs a page, YouTube's cap is 5,000 a playlist.
            for _ in 0..60 {
                let Some(t) = token.clone() else { break };
                match self.it.playlist_continuation(client, &t).await {
                    Ok(next) => {
                        rest.extend(next.items);
                        token = next.continuation.filter(|n| *n != t);
                    }
                    // A page that failed leaves the list incomplete, and says so.
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            Ok((page, rest, failed, token.is_some()))
        })?;
        let all: Vec<ytmusic::SongItem> = page.items.iter().chain(rest.iter()).cloned().collect();
        let complete = read_whole(page.song_count, &all, failed, left);
        let tracks = all.iter().filter_map(account_track_of).collect();
        Ok(crate::account::AccountPlaylist {
            playlist_id: playlist_id.trim_start_matches("VL").to_owned(),
            title: page.title.unwrap_or_default(),
            description: page.description,
            privacy: page.privacy,
            owned: page.owned,
            collaborative: page.collaborative,
            people: page.people,
            people_text: page.people_text,
            artwork_url: page.thumbnail,
            tracks,
            complete,
            radio: page.radio.as_ref().map(crate::catalog_cards::endpoint_of),
        })
    }

    fn create_playlist(
        &self,
        title: &str,
        description: Option<&str>,
        privacy: &str,
        video_ids: &[String],
        cancel: &CancellationToken,
    ) -> Result<String, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "the new playlist",
            self.it
                .create_playlist_with(client, title, description, privacy, video_ids),
        )
    }

    fn edit_playlist(
        &self,
        playlist_id: &str,
        edits: &[crate::account::PlaylistEdit],
        cancel: &CancellationToken,
    ) -> Result<crate::account::Placements, AccessError> {
        use crate::account::PlaylistEdit as E;
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(cancel, "the playlist change", async {
            let mut placed = Vec::new();
            let mut words: (Option<String>, Option<String>, Option<String>) = (None, None, None);
            for edit in edits {
                match edit {
                    E::Rename { title } => words.0 = Some(title.clone()),
                    E::Describe { description } => words.1 = Some(description.clone()),
                    E::Privacy { privacy } => words.2 = Some(privacy.clone()),
                    // Each song once (one duplicate fails a batch), 50 at a time with a pause.
                    E::Add { video_ids } => {
                        placed.extend(
                            self.it
                                .playlist_add_batches(client, playlist_id, video_ids)
                                .await?,
                        );
                    }
                    E::Remove { items } => {
                        if !items.is_empty() {
                            self.it
                                .playlist_remove_many(client, playlist_id, items)
                                .await?;
                        }
                    }
                    E::Move {
                        set_video_id,
                        before,
                    } => {
                        self.it
                            .playlist_move(client, playlist_id, set_video_id, before.as_deref())
                            .await?;
                    }
                }
            }
            if words != (None, None, None) {
                self.it
                    .playlist_edit_details(
                        client,
                        playlist_id,
                        words.0.as_deref(),
                        words.1.as_deref(),
                        words.2.as_deref(),
                    )
                    .await?;
            }
            Ok(placed)
        })
    }

    fn delete_playlist(
        &self,
        playlist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "deleting the playlist",
            self.it.delete_playlist(client, playlist_id),
        )
    }

    fn album_saved(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<crate::account::SavedState>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let page = self.account_call(cancel, "the album", self.it.album(client, album_id))?;
        Ok(page
            .playlist_id
            .map(|playlist_id| crate::account::SavedState {
                playlist_id,
                saved: page.in_library,
            }))
    }

    fn save(
        &self,
        playlist_id: &str,
        saved: bool,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let id = playlist_id.trim_start_matches("VL");
        self.account_call(
            cancel,
            "saving to your library",
            self.it.like_playlist(client, id, saved),
        )
    }

    fn artist_followed(
        &self,
        artist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<crate::account::FollowState>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let page = self.account_call(cancel, "the artist", self.it.artist(client, artist_id))?;
        Ok(
            (!page.channel_id.is_empty()).then(|| crate::account::FollowState {
                channel_id: page.channel_id.clone(),
                subscribed: page.subscribed,
            }),
        )
    }

    fn subscribe(
        &self,
        channel_id: &str,
        subscribed: bool,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "following the artist",
            self.it.subscribe(client, channel_id, subscribed),
        )
    }

    fn history(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<crate::account::HistoryDay>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let groups = self.account_call(cancel, "your history", self.it.history(client))?;
        Ok(groups
            .iter()
            .map(|g| crate::account::HistoryDay {
                title: g.title.clone(),
                tracks: g
                    .items
                    .iter()
                    .filter_map(crate::catalog_cards::track_of_row)
                    .collect(),
            })
            .filter(|d| !d.tracks.is_empty())
            .collect())
    }

    fn report_play(&self, video_id: &str, cancel: &CancellationToken) -> Result<(), AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(cancel, "the play", self.it.report_play(client, video_id))
    }

    fn upload(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let ext = file_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !ytmusic::UPLOAD_EXTENSIONS.contains(&ext.as_str()) {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "YouTube Music takes FLAC, M4A, MP3, OGG and WMA uploads.",
            ));
        }
        if bytes.len() as u64 >= ytmusic::UPLOAD_MAX_BYTES {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "YouTube Music takes uploads up to 300 MB.",
            ));
        }
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "the upload",
            self.it.upload_song(client, file_name, bytes),
        )
    }

    fn identities(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<crate::account::AccountIdentityInfo>, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        let all = self.account_call(cancel, "your accounts", self.it.account_identities(client))?;
        let using = self.it.data_sync_id();
        Ok(all
            .into_iter()
            .map(|a| crate::account::AccountIdentityInfo {
                current: using
                    .as_deref()
                    .map_or(a.is_selected, |u| u == a.data_sync_id),
                id: a.data_sync_id,
                name: a.name,
                handle: a.handle,
                email: a.email,
                picture: a.thumbnail,
            })
            .collect())
    }

    fn use_identity(&self, id: Option<&str>) {
        self.it.set_data_sync_id(id.map(str::to_owned));
    }

    fn invite(&self, playlist_id: &str, cancel: &CancellationToken) -> Result<String, AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "the invite link",
            self.it.collaboration_invite(client, playlist_id),
        )
    }

    fn close_to_collaborators(
        &self,
        playlist_id: &str,
        closed: bool,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "collaborators",
            self.it.collaboration_closed(client, playlist_id, closed),
        )
    }

    fn join(&self, invite_link: &str, cancel: &CancellationToken) -> Result<String, AccessError> {
        let (Some(playlist_id), Some(token)) = (
            ytmusic::invite_playlist(invite_link),
            ytmusic::invite_token(invite_link),
        ) else {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "That isn't a YouTube Music invite link.",
            ));
        };
        let client = self.client(PAGES_CLIENT)?;
        self.account_call(
            cancel,
            "joining the playlist",
            self.it.collaboration_join(client, &playlist_id, &token),
        )?;
        Ok(playlist_id)
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

    /// `next` for an endpoint, and the endpoint that answered. One request, except a first
    /// answer holding nothing but the song it opens on (a dead `RDAMVM` radio): that one names
    /// the live mix in its automix marker, which is asked instead, opening on the same song.
    fn answer(
        &self,
        endpoint: &ytmusic::WatchEndpoint,
        continuation: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(ytmusic::WatchEndpoint, ytmusic::NextResult), AccessError> {
        let answer = self.backend.watch(endpoint, continuation, cancel)?;
        if continuation.is_none()
            && answer.items.len() <= 1
            && let Some(mut mix) = answer.automix.clone()
        {
            if mix.video_id.is_none() {
                mix.video_id = endpoint.video_id.clone();
            }
            let live = self.backend.watch(&mix, None, cancel)?;
            return Ok((mix, live));
        }
        Ok((endpoint.clone(), answer))
    }

    /// A radio page as Venuta keeps it (see `catalog_cards::radio_page_of`). `seed`: the song
    /// playing, left out when the answer opens with it.
    fn radio_page(
        &self,
        endpoint: &ytmusic::WatchEndpoint,
        continuation: Option<&str>,
        seed: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<RemoteRadio, AccessError> {
        let started = std::time::Instant::now();
        let (asked, answer) = self.answer(endpoint, continuation, cancel)?;
        let mut radio = crate::catalog_cards::radio_page_of(&answer, &asked, seed);
        for t in &mut radio.tracks {
            t.artwork_url = clean_artwork_url(t.artwork_url.take());
        }
        crate::timing::note_timing(&format!(
            "online radio answered in {} ms ({} songs)",
            started.elapsed().as_millis(),
            radio.tracks.len()
        ));
        Ok(radio)
    }
}

impl MediaAccess for LiveCatalogAccess {
    fn account_access(&self) -> Option<Arc<dyn crate::account::AccountAccess>> {
        self.backend.clone().account_api()
    }

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
        let tracks = album_tracks_of(
            &self.backend.album(album_id.trim(), cancel)?,
            album_id.trim(),
        );
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
        let songs_raw = page
            .songs_list_id
            .as_deref()
            .and_then(|list| self.backend.browse(list, cancel).ok());
        let extras = crate::catalog_cards::artist_extras(&raw, id, songs_raw.as_ref());
        let songs = songs_raw
            .as_ref()
            .map(|raw| catalog_json::tracks_from_hits(&catalog_json::read_hits(raw), PROVIDER))
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
            description: extras.description,
            subscribers: extras.subscribers,
            banner_url: extras.banner_url,
            radio_id: extras.radio_id,
            radio: extras.radio,
            shuffle: extras.shuffle,
            shelves: extras.shelves,
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
        let radio = playlist_radio(&raw);
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
        Ok(RemotePlaylist {
            title,
            tracks,
            radio,
        })
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

    fn suggest(
        &self,
        input: &str,
        cancel: &CancellationToken,
    ) -> Result<SearchSuggestions, AccessError> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(SearchSuggestions::default());
        }
        self.backend.suggest(input, cancel)
    }

    fn search_collections(
        &self,
        query: &str,
        kind: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteCollection>, AccessError> {
        let query = query.trim();
        if query.is_empty() || !matches!(kind, "playlists" | "podcasts") {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search playlists or podcasts for some words",
            ));
        }
        let mut items = self.backend.search_cards(query, kind, cancel)?;
        items.truncate(page.limit.clamp(1, 100));
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    fn search_videos(
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
        let mut items = self.backend.search_videos(query, cancel)?;
        items.truncate(page.limit.clamp(1, 100));
        Ok(Page {
            items,
            next_cursor: None,
        })
    }

    fn moods(&self, cancel: &CancellationToken) -> Result<Vec<MoodSection>, AccessError> {
        self.backend.moods(cancel)
    }

    fn mood_collections(
        &self,
        params: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError> {
        if params.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which mood?",
            ));
        }
        self.backend.mood(params.trim(), cancel)
    }

    fn album_page(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RemoteAlbumPage, AccessError> {
        let id = album_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "album id must not be empty",
            ));
        }
        let page = album_page_of(&self.backend.album(id, cancel)?, id);
        if page.tracks.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::MalformedResponse,
                "catalog album tracks contained no usable track IDs",
            ));
        }
        Ok(page)
    }

    fn radio(
        &self,
        seed: Option<&str>,
        playlist_id: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<RemoteRadio, AccessError> {
        let seed = seed.map(str::trim).filter(|s| !s.is_empty());
        let list = playlist_id.map(str::trim).filter(|s| !s.is_empty());
        if seed.is_none() && list.is_none() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "a radio needs a song or a radio to start from",
            ));
        }
        // Built only here, for a song or a list with no button to replay: YouTube's own "Start
        // radio" shape (`RDAMVM` + the song, the radio's params).
        let endpoint = match (seed, list) {
            (Some(seed), None) => ytmusic::WatchEndpoint::song_radio(seed),
            (seed, list) => ytmusic::WatchEndpoint {
                video_id: seed.map(str::to_owned),
                playlist_id: list.map(str::to_owned),
                params: Some(ytmusic::RADIO_PARAMS.to_owned()),
                ..Default::default()
            },
        };
        self.radio_page(&endpoint, None, seed, cancel)
    }

    fn radio_at(
        &self,
        endpoint: &RadioEndpoint,
        playing: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<RemoteRadio, AccessError> {
        let endpoint = crate::catalog_cards::watch_of(endpoint);
        if endpoint.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "a radio needs a song or a radio to start from",
            ));
        }
        self.radio_page(&endpoint, None, playing, cancel)
    }

    fn radio_more(
        &self,
        endpoint: &RadioEndpoint,
        continuation: &str,
        cancel: &CancellationToken,
    ) -> Result<RemoteRadio, AccessError> {
        let token = continuation.trim();
        if token.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which page of the radio?",
            ));
        }
        let endpoint = crate::catalog_cards::watch_of(endpoint);
        self.radio_page(&endpoint, Some(token), None, cancel)
    }

    fn related_songs(
        &self,
        track_id: &str,
        related_id: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteTrack>, AccessError> {
        let id = track_id.trim();
        let page = match related_id.map(str::trim).filter(|r| !r.is_empty()) {
            Some(page) => Some(page.to_owned()),
            // The song's own answer names its Related page.
            None if !id.is_empty() => {
                let plain = ytmusic::WatchEndpoint {
                    video_id: Some(id.to_owned()),
                    ..Default::default()
                };
                self.backend.watch(&plain, None, cancel)?.related_browse_id
            }
            None => None,
        };
        let Some(page) = page else {
            return Ok(Vec::new());
        };
        Ok(self
            .backend
            .related(&page, cancel)?
            .iter()
            .filter_map(crate::catalog_cards::track_of_row)
            .map(|mut t| {
                t.artwork_url = clean_artwork_url(t.artwork_url.take());
                t
            })
            .collect())
    }

    fn set_blocked_artists(&self, blocked: &[(Option<String>, String)]) {
        self.backend.set_blocked(blocked);
    }

    fn track(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<RemoteTrack>, AccessError> {
        let id = track_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which song?",
            ));
        }
        // The song's own "next" answer starts with the song itself.
        let (_, answer) = self.answer(&ytmusic::WatchEndpoint::song_radio(id), None, cancel)?;
        Ok(answer
            .items
            .iter()
            .find(|s| s.video_id == id)
            .and_then(crate::catalog_cards::track_of_row)
            .map(|mut t| {
                t.artwork_url = clean_artwork_url(t.artwork_url.take());
                t
            }))
    }

    fn home(
        &self,
        params: Option<&str>,
        continuation: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<RemoteHome, AccessError> {
        let started = std::time::Instant::now();
        let page = self.backend.home(
            params.map(str::trim).filter(|p| !p.is_empty()),
            continuation.map(str::trim).filter(|c| !c.is_empty()),
            cancel,
        )?;
        crate::timing::note_timing(&format!(
            "online home answered in {} ms ({} rows)",
            started.elapsed().as_millis(),
            page.sections.len()
        ));
        Ok(crate::catalog_cards::home_of(&page))
    }

    fn lyrics(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<CatalogLyrics>, AccessError> {
        let id = track_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which song?",
            ));
        }
        self.backend.lyrics(id, cancel)
    }

    fn set_hide_videos(&self, on: bool) {
        self.backend.set_hide_videos(on);
    }

    fn set_personal(&self, on: bool) {
        self.backend.set_personal(on);
    }

    fn set_signed_search(&self, on: bool) {
        self.backend.set_signed_search(on);
    }

    fn set_region(&self, country: Option<&str>) {
        self.backend.set_region(country);
    }

    fn set_data_saver(&self, on: bool) {
        self.backend.set_data_saver(on);
    }

    fn set_account_cookie(&self, cookie: Option<&str>) {
        self.backend.set_cookie(cookie);
    }

    fn account_cookie(&self) -> Option<String> {
        self.backend.cookie()
    }

    fn account(&self, cancel: &CancellationToken) -> Result<Option<RemoteAccount>, AccessError> {
        self.backend.account(cancel)
    }

    fn account_playlists(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteCollection>, AccessError> {
        self.backend.library_playlists(cancel)
    }

    fn rate_on_account(
        &self,
        track_id: &str,
        rating: RemoteRating,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let id = track_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which song?",
            ));
        }
        self.backend.rate(id, rating, cancel)
    }

    fn resolve_audio_small(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        let id = track_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which song?",
            ));
        }
        self.backend.resolve_small(id, cancel)
    }

    fn account_liked(&self, cancel: &CancellationToken) -> Result<Vec<RemoteTrack>, AccessError> {
        if !self.backend.logged_in() {
            return Ok(Vec::new());
        }
        // Liked Music is the account's own playlist "LM".
        Ok(self.playlist("LM", cancel)?.tracks)
    }

    fn shelf_more(
        &self,
        more: &RemoteMore,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteSuggestion>, AccessError> {
        let id = more.browse_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "which row?",
            ));
        }
        self.backend.grid(id, more.params.as_deref(), cancel)
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
    // Kept at a size that looks sharp in Now Playing and the Windows media overlay; each place
    // in the window asks for its own size from this.
    url.starts_with("https://")
        .then(|| crate::art_url::sized_art(&url, 544))
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
/// A podcast's page id ("MPSP" and its list id) opens its episode list the same way.
fn playlist_browse_id(playlist_id: &str) -> Result<String, AccessError> {
    let id = playlist_id.trim();
    if id.is_empty() {
        return Err(AccessError::new(
            AccessErrorKind::InvalidInput,
            "playlist id must not be empty",
        ));
    }
    let id = id
        .strip_prefix("MPSP")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(id);
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

#[cfg(test)]
mod tests {
    use super::{album_page_of, playlist_browse_id, playlist_radio, read_whole};
    use crate::types::RadioEndpoint;

    fn saved(name: &str) -> serde_json::Value {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ytm")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(file).expect("fixture")).expect("json")
    }

    // Start radio on an album or a playlist replays the radio its page header names, as it came.
    #[test]
    fn album_and_playlist_pages_carry_their_radio() {
        let album = album_page_of(
            &ytmusic::models::browse::parse_album(&saved("album-heartbeat.json")),
            "MPREb_DbkpS1rORXk",
        );
        assert_eq!(
            album.radio,
            Some(RadioEndpoint {
                playlist_id: Some("RDAMPLOLAK5uy_nzlLa_sS90GkezY_47EhlFbbf2z21PO_4".into()),
                params: Some("wAEB".into()),
                ..Default::default()
            })
        );
        assert_eq!(
            playlist_radio(&saved("playlist-first.json")),
            Some(RadioEndpoint {
                playlist_id: Some("RDAMPLPLXCoHsJ9oLec7aYGojOIj3Acu7wMKjADm".into()),
                params: Some("wAEB".into()),
                ..Default::default()
            })
        );
        assert_eq!(playlist_radio(&serde_json::json!({})), None);
    }

    #[test]
    fn an_album_page_says_what_it_is_who_made_it_and_what_is_under_it() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ytm/album-heartbeat.json");
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).expect("fixture")).expect("json");
        let page = album_page_of(
            &ytmusic::models::browse::parse_album(&raw),
            "MPREb_DbkpS1rORXk",
        );
        assert_eq!(page.album.title, "Heartbeat Diaries");
        assert_eq!(page.album.kind.as_deref(), Some("Album"));
        assert_eq!(page.album.release_date.as_deref(), Some("2026"));
        assert!(
            page.subtitle
                .as_deref()
                .is_some_and(|s| s.starts_with("Album"))
        );
        assert!(page.length.as_deref().is_some_and(|s| s.contains("songs")));
        assert!(!page.artists.is_empty());
        assert!(!page.tracks.is_empty());
        assert!(
            page.tracks
                .iter()
                .all(|t| t.album_id.as_deref() == Some("MPREb_DbkpS1rORXk"))
        );
        assert!(
            page.tracks.iter().any(|t| t.plays.is_some()),
            "album rows carry their plays"
        );
        assert_eq!(page.album.track_count, Some(page.tracks.len() as u32));
    }

    // The transport tells the healer about the first refusal, before its one retry. That alone
    // must not say "Sign in again": only a request that is refused after the retry does.
    #[test]
    fn one_refusal_told_to_the_healer_is_not_an_expired_session() {
        use super::LiveBackend;
        use crate::account::AccountAccess;
        let b = super::YtmBackend::new().unwrap();
        b.set_cookie(Some("SAPISID=fixture; PREF=x"));
        b.it.session_rejected().notify_one();
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(b.signed_in());
        assert!(!b.session_expired());
    }

    // A brand channel chosen under one sign-in must not ride along on the next one (its
    // `onBehalfOfUser` with another account's cookie is refused), nor after signing out.
    #[test]
    fn a_new_sign_in_or_a_sign_out_acts_as_itself_again() {
        use super::LiveBackend;
        use crate::account::AccountAccess;
        let b = super::YtmBackend::new().unwrap();
        b.set_cookie(Some("SAPISID=first; PREF=x"));
        b.use_identity(Some("brand-x"));
        assert_eq!(b.it.data_sync_id().as_deref(), Some("brand-x"));
        b.set_cookie(Some("SAPISID=second; PREF=x"));
        assert_eq!(b.it.data_sync_id(), None, "another sign-in");
        b.use_identity(Some("brand-y"));
        b.set_cookie(None);
        assert_eq!(b.it.data_sync_id(), None, "signed out");
    }

    fn rows(ids: &[&str]) -> Vec<ytmusic::SongItem> {
        ids.iter()
            .map(|id| ytmusic::SongItem {
                video_id: (*id).into(),
                title: id.to_uppercase(),
                ..Default::default()
            })
            .collect()
    }

    // A reading that came back short must not count as whole: a linked playlist's sync would
    // take every song it missed as taken out on YouTube Music.
    #[test]
    fn a_playlist_reading_is_whole_only_when_nothing_is_missing_from_it() {
        assert!(read_whole(Some(3), &rows(&["a", "b", "c"]), false, false));
        assert!(
            read_whole(None, &rows(&["a", "b"]), false, false),
            "no count to go by"
        );
        assert!(
            !read_whole(Some(5), &rows(&["a", "b", "c"]), false, false),
            "short of the header"
        );
        assert!(
            !read_whole(Some(3), &rows(&["a", "b", "c"]), true, false),
            "a page failed"
        );
        assert!(
            !read_whole(Some(3), &rows(&["a", "b", "c"]), false, true),
            "pages left unread"
        );
        // A row with no song to name (an unavailable one) can't be told from a song taken out.
        assert!(!read_whole(Some(3), &rows(&["a", " ", "c"]), false, false));
    }

    #[test]
    fn playlists_and_podcasts_open_by_their_list() {
        assert_eq!(playlist_browse_id("PL123").unwrap(), "VLPL123");
        assert_eq!(playlist_browse_id("VLPL123").unwrap(), "VLPL123");
        // A podcast's page is its episode list.
        assert_eq!(playlist_browse_id("MPSPPL123").unwrap(), "VLPL123");
        assert!(playlist_browse_id("  ").is_err());
    }
}
