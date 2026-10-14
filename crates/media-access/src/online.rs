//! Sealed real-catalog provider implementation lives here.
//!
//! Product layers must depend only on [`crate::MediaAccess`]. Do not call this module from UI code.

use crate::access::{AccessError, AccessErrorKind, MediaAccess};
use crate::types::{
    CancellationToken, EphemeralAudio, Page, PageRequest, RemoteAlbum, RemoteArtist, RemoteTrack,
};

/// Placeholder until the sealed provider is fully wired and verified.
///
/// Returns [`AccessErrorKind::Unsupported`] for resolve so product code can develop against
/// [`crate::FakeMediaAccess`] without accidental live network use.
pub struct UnconfiguredOnlineAccess;

impl MediaAccess for UnconfiguredOnlineAccess {
    fn search_tracks(
        &self,
        _query: &str,
        _page: &PageRequest,
        _cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "online catalog provider is not configured",
        ))
    }

    fn search_albums(
        &self,
        _query: &str,
        _page: &PageRequest,
        _cancel: &CancellationToken,
    ) -> Result<Page<RemoteAlbum>, AccessError> {
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "online catalog provider is not configured",
        ))
    }

    fn search_artists(
        &self,
        _query: &str,
        _page: &PageRequest,
        _cancel: &CancellationToken,
    ) -> Result<Page<RemoteArtist>, AccessError> {
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "online catalog provider is not configured",
        ))
    }

    fn album_tracks(
        &self,
        _album_id: &str,
        _page: &PageRequest,
        _cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "online catalog provider is not configured",
        ))
    }

    fn playlist_tracks(
        &self,
        _playlist_id: &str,
        _page: &PageRequest,
        _cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "online catalog provider is not configured",
        ))
    }

    fn resolve_audio(
        &self,
        _track_id: &str,
        _cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        Err(AccessError::new(
            AccessErrorKind::Unsupported,
            "online catalog provider is not configured",
        ))
    }
}
