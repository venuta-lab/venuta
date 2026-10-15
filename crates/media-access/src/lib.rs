//! Provider-neutral remote catalog access and ephemeral session audio.
//!
//! Real catalog providers implement [`MediaAccess`]. Product code and tests should prefer
//! [`FakeMediaAccess`] until a concrete provider is wired through [`default_media_access`].
//!
//! Ephemeral audio descriptors are intentionally not serializable so stream addresses cannot be
//! saved into playlists or restored queues.

mod access;
mod fake;
#[cfg(feature = "live-provider")]
mod live_provider;
mod online;
#[cfg(feature = "live-provider")]
mod stream_resolve;
mod types;

pub use access::{
    AccessError, AccessErrorKind, CachedAccess, CatalogHealth, MediaAccess, catalog_health,
    default_media_access, live_catalog_env_enabled,
};
pub use fake::FakeMediaAccess;
#[cfg(feature = "live-provider")]
pub use live_provider::LiveCatalogAccess;
pub use online::UnconfiguredOnlineAccess;
#[cfg(feature = "live-provider")]
pub use stream_resolve::{StreamToolStatus, ensure_stream_tool, stream_tool_status};
pub use types::{
    Availability, CancellationToken, EphemeralAudio, MediaHeader, ONLINE_PLAYLIST_TAG, Page,
    PageRequest, Playable, PlayableId, RemoteAlbum, RemoteArtist, RemoteTrack,
};
