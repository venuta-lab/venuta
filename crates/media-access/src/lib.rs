//! Provider-neutral remote catalog access and ephemeral session audio.
//!
//! Real catalog providers implement [`MediaAccess`]. Product code and tests should prefer
//! [`FakeMediaAccess`] until a concrete provider is wired through [`default_media_access`].
//!
//! Ephemeral audio descriptors are intentionally not serializable so stream addresses cannot be
//! saved into playlists or restored queues.

mod access;
pub mod account;
mod art_url;
#[cfg(feature = "live-provider")]
mod catalog_cards;
mod catalog_json;
mod fake;
#[cfg(feature = "live-provider")]
mod live_provider;
mod media_http;
mod online;
mod region;
#[cfg(feature = "live-provider")]
mod stream_pick;
#[cfg(feature = "live-provider")]
mod stream_resolve;
mod timing;
mod types;

pub use access::{
    AccessError, AccessErrorKind, CachedAccess, CatalogHealth, MediaAccess, catalog_health,
    default_media_access, live_catalog_env_enabled,
};
pub use account::{
    AccountAccess, AccountIdentityInfo, AccountPlaylist, AccountTrack, Contributor, FollowState,
    Heard, HistoryDay, HistoryPlay, LibraryShelf, Listen, MenuPress, Placements, PlayCounts,
    PlaylistEdit, SavedState, TrackVotes, Vote, VoteButton,
};
pub use art_url::sized_art;
pub use fake::FakeMediaAccess;
#[cfg(feature = "live-provider")]
pub use live_provider::LiveCatalogAccess;
pub use media_http::{media_agent, serves_to_the_end};
pub use online::UnconfiguredOnlineAccess;
pub use region::{CatalogRegion, catalog_region};
#[cfg(feature = "live-provider")]
pub use stream_resolve::{StreamToolStatus, ensure_stream_tool, stream_tool_status};
pub use timing::{ms_since, note_timing, set_timing_log};
pub use types::{
    Availability, CancellationToken, CatalogLyrics, EphemeralAudio, MediaHeader, MenuFeedback,
    MenuToggle, MoodSection, MoodTile, ONLINE_PLAYLIST_TAG, Page, PageRequest, PastSearch,
    Playable, PlayableId, RadioChip, RadioEndpoint, RemoteAccount, RemoteAlbum, RemoteAlbumPage,
    RemoteArtist, RemoteArtistLink, RemoteArtistPage, RemoteChip, RemoteCollection, RemoteHome,
    RemoteHomeShelf, RemoteMore, RemotePlaylist, RemoteRadio, RemoteRating, RemoteRelatedShelf,
    RemoteShelf, RemoteSuggestion, RemoteTrack, SearchSuggestions, TrackMenu,
};
/// YouTube's security check (PO tokens): where it runs is installed by the app.
#[cfg(feature = "live-provider")]
pub use ytmusic::potoken;
