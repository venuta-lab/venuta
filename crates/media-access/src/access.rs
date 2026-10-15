use crate::types::{
    CancellationToken, EphemeralAudio, Page, PageRequest, RemoteAlbum, RemoteArtist,
    RemoteArtistPage, RemoteTrack,
};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessErrorKind {
    InvalidInput,
    NotFound,
    Transport,
    MalformedResponse,
    Unsupported,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessError {
    pub kind: AccessErrorKind,
    pub message: String,
}

impl AccessError {
    pub fn new(kind: AccessErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for AccessError {}

/// Provider-neutral remote catalog + session audio resolution.
pub trait MediaAccess: Send + Sync {
    fn search_tracks(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError>;

    fn search_albums(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteAlbum>, AccessError>;

    fn search_artists(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteArtist>, AccessError>;

    fn album_tracks(
        &self,
        album_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError>;

    fn playlist_tracks(
        &self,
        playlist_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError>;

    fn resolve_audio(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError>;

    /// An artist's page (top songs, more songs, albums, singles, similar artists). A catalog
    /// without artist pages answers Unsupported, which is what this default does.
    fn artist_page(
        &self,
        artist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RemoteArtistPage, AccessError> {
        let _ = (artist_id, cancel);
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "artist pages are not available from this catalog",
        ))
    }
}

/// Default factory used by the app and release builds.
///
/// - With `live-provider` (desktop app): **live catalog is on by default** so search and play
///   work in normal runs and shipped builds. Opt out with `VENUTA_FAKE_CATALOG=1` or
///   `VENUTA_LIVE_CATALOG=0` (tests / offline fixtures only).
/// - Without the feature: always [`crate::FakeMediaAccess`].
pub fn default_media_access() -> Arc<dyn MediaAccess> {
    let inner: Arc<dyn MediaAccess> = {
        #[cfg(feature = "live-provider")]
        {
            if force_fake_catalog() {
                Arc::new(crate::FakeMediaAccess::with_in_memory_fixture())
            } else {
                match crate::LiveCatalogAccess::new() {
                    Ok(live) => Arc::new(live),
                    // Init failure: fixture so the app still opens; Settings shows tool/catalog health.
                    Err(_) => Arc::new(crate::FakeMediaAccess::with_in_memory_fixture()),
                }
            }
        }
        #[cfg(not(feature = "live-provider"))]
        {
            Arc::new(crate::FakeMediaAccess::with_in_memory_fixture())
        }
    };
    // Memory-only session cache: replaying the same id skips a full resolve for a few minutes.
    // Never written to disk (URLs stay ephemeral).
    Arc::new(CachedAccess::new(inner, Duration::from_secs(12 * 60)))
}

/// Wraps a catalog so `resolve_audio` reuses a recent session result in process memory only.
pub struct CachedAccess {
    inner: Arc<dyn MediaAccess>,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, EphemeralAudio)>>,
}

impl CachedAccess {
    pub fn new(inner: Arc<dyn MediaAccess>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Prefetch resolve for a track (next-up warmup). Errors are ignored.
    pub fn warm(&self, track_id: &str) {
        let cancel = CancellationToken::default();
        let _ = self.resolve_audio(track_id, &cancel);
    }
}

impl MediaAccess for CachedAccess {
    fn search_tracks(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        self.inner.search_tracks(query, page, cancel)
    }

    fn search_albums(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteAlbum>, AccessError> {
        self.inner.search_albums(query, page, cancel)
    }

    fn search_artists(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteArtist>, AccessError> {
        self.inner.search_artists(query, page, cancel)
    }

    fn album_tracks(
        &self,
        album_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        self.inner.album_tracks(album_id, page, cancel)
    }

    fn artist_page(
        &self,
        artist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RemoteArtistPage, AccessError> {
        self.inner.artist_page(artist_id, cancel)
    }

    fn playlist_tracks(
        &self,
        playlist_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        self.inner.playlist_tracks(playlist_id, page, cancel)
    }

    fn resolve_audio(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        let id = track_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "track id must not be empty",
            ));
        }
        if let Ok(cache) = self.cache.lock() {
            if let Some((at, audio)) = cache.get(id) {
                if at.elapsed() < self.ttl {
                    return Ok(audio.clone());
                }
            }
        }
        let audio = self.inner.resolve_audio(id, cancel)?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|_, (at, _)| at.elapsed() < self.ttl);
            if cache.len() > 64 {
                // Drop oldest-ish: clear half when large.
                let keys: Vec<_> = cache.keys().take(cache.len() / 2).cloned().collect();
                for k in keys {
                    cache.remove(&k);
                }
            }
            cache.insert(id.to_owned(), (Instant::now(), audio.clone()));
        }
        Ok(audio)
    }
}

fn env_truthy(name: &str) -> Option<bool> {
    let v = std::env::var(name).ok()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    if matches!(
        v.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "live"
    ) {
        return Some(true);
    }
    if matches!(
        v.to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off" | "fake" | "fixture"
    ) {
        return Some(false);
    }
    None
}

/// Force the in-process fixture catalog (no network). For tests and local Fake runs.
#[cfg(feature = "live-provider")]
fn force_fake_catalog() -> bool {
    if env_truthy("VENUTA_FAKE_CATALOG") == Some(true) {
        return true;
    }
    // Legacy: VENUTA_LIVE_CATALOG=0|false|off|fake turns live off.
    match env_truthy("VENUTA_LIVE_CATALOG") {
        Some(false) => true,
        Some(true) | None => false,
    }
}

#[cfg(feature = "live-provider")]
fn live_catalog_enabled() -> bool {
    !force_fake_catalog()
}

/// Whether this process will use the live catalog (feature on and not forced to Fake).
pub fn live_catalog_env_enabled() -> bool {
    #[cfg(feature = "live-provider")]
    {
        live_catalog_enabled()
    }
    #[cfg(not(feature = "live-provider"))]
    {
        false
    }
}

/// Human-facing summary of catalog + stream tool health (no URLs).
#[derive(Debug, Clone)]
pub struct CatalogHealth {
    pub live_env: bool,
    pub live_provider_built: bool,
    pub stream_tool_available: bool,
    pub stream_tool_kind: String,
    pub stream_tool_detail: String,
    pub summary: String,
}

pub fn catalog_health() -> CatalogHealth {
    let live_env = live_catalog_env_enabled();
    let live_provider_built = cfg!(feature = "live-provider");

    let (stream_tool_available, stream_tool_kind, stream_tool_detail) = {
        #[cfg(feature = "live-provider")]
        {
            let tool = crate::stream_tool_status();
            (tool.available, tool.kind, tool.detail)
        }
        #[cfg(not(feature = "live-provider"))]
        {
            (false, String::new(), String::new())
        }
    };

    let summary = if !live_provider_built {
        "This build has no live catalog. Online search uses fixtures only.".into()
    } else if !live_env {
        "Live catalog is off for this session (VENUTA_FAKE_CATALOG or VENUTA_LIVE_CATALOG=0). Online search shows test songs only."
            .into()
    } else if !stream_tool_available {
        "Live catalog is on, but the stream tool is missing. Install yt-dlp (python -m pip install -U yt-dlp) for full-length play."
            .into()
    } else {
        format!(
            "Live catalog is on. Stream tool: {} ({}).",
            stream_tool_kind, stream_tool_detail
        )
    };

    CatalogHealth {
        live_env,
        live_provider_built,
        stream_tool_available,
        stream_tool_kind,
        stream_tool_detail,
        summary,
    }
}
