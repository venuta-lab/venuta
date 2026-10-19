//! Pure-Rust YouTube Music (InnerTube) transport, client identities, models and endpoints.
//!
//! The boundary rule (context/11): this crate knows nothing about Tauri, webviews, mpv, or the
//! OS. It is unit-testable against JSON fixtures with no network. Cipher/PoToken/WEB_REMIX
//! streaming are Phase 2 and deliberately absent here.

pub mod blocklist;
pub mod clients;
pub mod comments_session;
pub mod endpoints;
pub mod models;
#[cfg(test)]
mod test_server;
pub mod transport;

pub use blocklist::BlockList;
pub use clients::{
    Clients, YouTubeClient, LYRICS_TIMED_CLIENT, MAIN_CLIENT, METADATA_CLIENT,
    STREAM_FALLBACK_ORDER, UPLOAD_FALLBACK_ORDER,
};
pub use comments_session::{ActionError, CommentsSession, Provenance};
pub use endpoints::{invite_playlist, invite_token, ListenTracking};
pub use models::browse::{
    AlbumPage, ArtistCarousel, ArtistPage, BrowseItem, HistoryGroup, HomePage, Mood, MoodSection,
    PlaylistContinuation, PlaylistPage, PlaylistSort, PlaylistSuggestions, QuerySuggestion,
    SearchResults, SearchSuggestions, Section, SortMenu,
};
pub use models::comment_write::{CommentWrite, WriteCommand};
pub use models::comments::{
    parse_written_comment, Comment, CommentAction, CommentReplies, CommentSortKey, CommentThread,
    CommentsPage, CommentsState, VoteState,
};
pub use models::context::Locale;
pub use models::lyrics::{PlainLyrics, TimedLyricLine};
pub use models::metadata::{
    AccountIdentity, AccountInfo, ArtistRun, NextResult, QueueChip, Rating, SearchResult, SongItem,
    WatchEndpoint, RADIO_PARAMS,
};
pub use models::player::{
    find_format, find_video_format, AudioQuality, Format, PlaybackTracking, PlayerResponse,
    StreamingData,
};
pub use transport::{
    cookie_sapisid, generate_cpn, without_healing, Error, Healing, InnerTube, Session, Watched,
};
pub use transport::{UPLOAD_EXTENSIONS, UPLOAD_MAX_BYTES};
