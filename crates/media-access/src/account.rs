//! The signed-in YouTube Music account: its library, the changes you make to it, and what you play
//! told back to it. A part of the online catalog of its own, reached through
//! `MediaAccess::account_access`, so a catalog without an account (Local only, the fake one)
//! simply has none.
//!
//! Every write here is one the user made: nothing in this module runs on its own.

use crate::{AccessError, CancellationToken, RemoteSuggestion, RemoteTrack};
use serde::{Deserialize, Serialize};

/// A part of the account's library that comes as cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryShelf {
    /// Playlists made or saved.
    Playlists,
    /// Albums saved to the library.
    Albums,
    /// The artists behind the songs and albums in the library.
    Artists,
    /// The artists subscribed to.
    Subscriptions,
    /// The podcasts followed.
    Podcasts,
    /// Albums the account uploaded itself.
    UploadAlbums,
    /// The podcast channels followed.
    PodcastChannels,
    /// The artists of the account's uploads, each opening on their uploaded songs.
    UploadArtists,
}

/// One song as the account's lists hold it: the song, and where it sits in the list (the handle
/// a move or a removal needs, which only the list itself hands out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountTrack {
    #[serde(flatten)]
    pub track: RemoteTrack,
    #[serde(default)]
    pub set_video_id: Option<String>,
    /// The account uploaded it: only it can play the song.
    #[serde(default)]
    pub upload: bool,
    /// Who put it in the playlist, on a collaborative one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<Contributor>,
    /// Its votes, on a collaborative playlist that has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub votes: Option<TrackVotes>,
    /// One of the account's uploads: the id that deletes it ([`AccountAccess::delete_upload`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_id: Option<String>,
}

/// Which of a song menu's presses a token is, for the words when YouTube turns it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuPress {
    /// Save to library, or remove from it.
    Library,
    /// Pin to Listen again, or unpin.
    Pin,
    /// Not interested, and the other "tell me less of this" items.
    Feedback,
}

impl MenuPress {
    /// What it is, in a sentence: "YouTube Music didn't take {what}."
    pub fn what(self) -> &'static str {
        match self {
            MenuPress::Library => "the library change",
            MenuPress::Pin => "the pin",
            MenuPress::Feedback => "that",
        }
    }
}

/// The person who added a song to a collaborative playlist, as the row shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contributor {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    /// Their channel, to open.
    #[serde(default)]
    pub channel_id: Option<String>,
}

/// A song's votes in a collaborative playlist, and what each button sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackVotes {
    /// How the account voted, as the list came.
    pub mine: Vote,
    /// The net count, as the list came.
    pub votes: i64,
    /// The count as YouTube writes it with no vote of yours, with yours up, with yours down.
    #[serde(default)]
    pub count: Option<String>,
    #[serde(default)]
    pub count_up: Option<String>,
    #[serde(default)]
    pub count_down: Option<String>,
    pub up: VoteButton,
    pub down: VoteButton,
}

/// One vote button: what it sends to vote, and what it sends pressed again to take it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteButton {
    pub token: String,
    #[serde(default)]
    pub undo_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vote {
    Up,
    Down,
    Neutral,
}

/// One of the account's playlists, whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountPlaylist {
    pub playlist_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `PRIVATE`, `UNLISTED` or `PUBLIC`, when YouTube says.
    #[serde(default)]
    pub privacy: Option<String>,
    /// The account made it, so it can change it.
    pub owned: bool,
    pub collaborative: bool,
    /// A collaborative playlist's people (their pictures), and the words beside them.
    #[serde(default)]
    pub people: Vec<String>,
    #[serde(default)]
    pub people_text: Option<String>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    pub tracks: Vec<AccountTrack>,
    /// Every page came: false when a later page failed, so a sync must not read a song missing
    /// from this list as one taken out.
    pub complete: bool,
    /// Songs the playlist holds that YouTube didn't list (the header counts more): unavailable
    /// ones it hides. A song missing from `tracks` may be one of these, not one taken out.
    #[serde(default)]
    pub hidden: usize,
    /// Its page's radio (the header's Start radio), as YouTube put it there.
    #[serde(default)]
    pub radio: Option<crate::types::RadioEndpoint>,
}

/// A change to a playlist the account owns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PlaylistEdit {
    Rename {
        title: String,
    },
    Describe {
        description: String,
    },
    Privacy {
        privacy: String,
    },
    /// Songs added at the end, in order.
    Add {
        video_ids: Vec<String>,
    },
    /// Songs taken out, each by its song and its place.
    Remove {
        items: Vec<(String, String)>,
    },
    /// One song moved to just before another, or to the end.
    Move {
        set_video_id: String,
        before: Option<String>,
    },
    /// A whole playlist (or an album's list) copied in, on YouTube Music's side, in one edit.
    AddPlaylist {
        playlist_id: String,
    },
    /// New songs go to the top (`on`) or to the end.
    AddToTop {
        on: bool,
    },
}

/// One artist the taste builder offers ("Choose artists you like"), and the two values its
/// answer is made of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasteArtist {
    pub name: String,
    pub selection_value: String,
    pub impression_value: String,
    #[serde(default)]
    pub picture: Option<String>,
    /// Chosen already, when YouTube Music said so.
    #[serde(default)]
    pub selected: bool,
}

/// Where each added song landed: `(videoId, setVideoId)`.
pub type Placements = Vec<(String, String)>;

/// One day of the account's history, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryDay {
    pub title: String,
    pub tracks: Vec<HistoryPlay>,
}

/// One play in the history: the song, and the token that takes this play out of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryPlay {
    #[serde(flatten)]
    pub track: RemoteTrack,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_token: Option<String>,
}

/// An identity the signed-in Google login can act as (itself, or a brand account).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountIdentityInfo {
    /// Stable id to switch to it by.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    /// The one in use now.
    pub current: bool,
    /// The login's own channel (not a brand account): acted as with no identity chosen.
    #[serde(default)]
    pub own: bool,
}

/// Whether the account has an album or a playlist saved, and the id to save it by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedState {
    pub playlist_id: String,
    pub saved: bool,
}

/// Whether the account follows an artist, and the channel to follow it by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FollowState {
    pub channel_id: String,
    pub subscribed: bool,
}

/// One listen of an online song, as the account is told about it: the catalog's own tracking for
/// that listen (one signed-in player answer, one playback nonce), shared by the play and every
/// listening-time report of it. Only ever held in memory; its addresses never reach a log line.
#[derive(Clone)]
pub struct Listen {
    pub video_id: String,
    /// Where its play counts.
    pub counts: PlayCounts,
    #[cfg(feature = "live-provider")]
    pub(crate) tracking: ytmusic::ListenTracking,
}

impl std::fmt::Debug for Listen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listen")
            .field("video_id", &self.video_id)
            .field("counts", &self.counts)
            .finish_non_exhaustive()
    }
}

/// Where a listen's play counts, as the playback log says it. The reasons are words safe to
/// log: never an id, a token or an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayCounts {
    /// On YouTube Music's History page, as its own web player tells it. `no_token`: told
    /// without YouTube's security check (PO token), and why there was none.
    YouTubeMusic { no_token: Option<String> },
    /// In the YouTube watch history: told through the YouTube web client, because YouTube
    /// Music's own player gave nowhere to tell it (why).
    YouTube { why: String },
}

/// How much of a listen was heard since its last report: each stretch from where it started to
/// where it stopped (a seek starts a new one), where the song is now, its length (0: not known),
/// and whether it is playing. Milliseconds of the song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard {
    pub segments: Vec<(u64, u64)>,
    pub at_ms: u64,
    pub length_ms: u64,
    pub playing: bool,
}

pub trait AccountAccess: Send + Sync {
    /// Signed in at all.
    fn signed_in(&self) -> bool;

    /// YouTube stopped taking the sign-in (it ran out, or was ended elsewhere): the account's part
    /// rests until the person signs in again.
    fn session_expired(&self) -> bool;

    /// A part of the library as cards.
    fn library(
        &self,
        shelf: LibraryShelf,
        cancel: &CancellationToken,
    ) -> Result<Vec<RemoteSuggestion>, AccessError>;

    /// The songs saved to the library, or the ones uploaded (`uploads`).
    fn library_songs(
        &self,
        uploads: bool,
        cancel: &CancellationToken,
    ) -> Result<Vec<AccountTrack>, AccessError>;

    /// A playlist with every song and its place.
    fn playlist(
        &self,
        playlist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<AccountPlaylist, AccessError>;

    /// Make a playlist; its new id.
    fn create_playlist(
        &self,
        title: &str,
        description: Option<&str>,
        privacy: &str,
        video_ids: &[String],
        cancel: &CancellationToken,
    ) -> Result<String, AccessError>;

    /// Change a playlist the account owns, the edits in order. Where the added songs landed.
    fn edit_playlist(
        &self,
        playlist_id: &str,
        edits: &[PlaylistEdit],
        cancel: &CancellationToken,
    ) -> Result<Placements, AccessError>;

    fn delete_playlist(
        &self,
        playlist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    /// Whether an album (`MPRE…`) is saved, and the id to save it by.
    fn album_saved(
        &self,
        album_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<SavedState>, AccessError>;

    /// Save (or un-save) an album or a playlist by its playlist id.
    fn save(
        &self,
        playlist_id: &str,
        saved: bool,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    /// Whether the account follows an artist (by its page id), and its channel.
    fn artist_followed(
        &self,
        artist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<FollowState>, AccessError>;

    fn subscribe(
        &self,
        channel_id: &str,
        subscribed: bool,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    /// The account's history, as YouTube keeps it.
    fn history(&self, cancel: &CancellationToken) -> Result<Vec<HistoryDay>, AccessError>;

    /// Take one play out of the history, by its [`HistoryPlay::remove_token`].
    fn remove_from_history(
        &self,
        token: &str,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    /// Vote on a song of a collaborative playlist, or take the vote back: the token is the one
    /// its [`VoteButton`] sends in the state it is in.
    fn vote(&self, token: &str, cancel: &CancellationToken) -> Result<(), AccessError>;

    /// One press on a song's own menu (save to library, pin, not interested): the token the
    /// song's [`crate::TrackMenu`] holds for that way.
    fn menu_press(
        &self,
        token: &str,
        press: MenuPress,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let _ = (token, press, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "song menu",
        ))
    }

    /// One song's own menu as the account sees it, for a song listed without one (a search sent
    /// signed out): only the song's id is sent. None when YouTube Music gave none.
    fn song_menu(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<crate::TrackMenu>, AccessError> {
        let _ = (video_id, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "song menu",
        ))
    }

    /// Take a past search out of the account's search history, by its
    /// [`crate::PastSearch::remove_token`].
    fn remove_past_search(
        &self,
        token: &str,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let _ = (token, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "search history",
        ))
    }

    /// Give a playlist the account owns its own cover picture (a square JPEG or PNG works best).
    fn set_playlist_cover(
        &self,
        playlist_id: &str,
        image: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let _ = (playlist_id, image, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "playlist covers",
        ))
    }

    /// Take a playlist's own cover away, so YouTube Music makes one from its songs again: the
    /// address of that one, when it said.
    fn clear_playlist_cover(
        &self,
        playlist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, AccessError> {
        let _ = (playlist_id, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "playlist covers",
        ))
    }

    /// The artists YouTube Music's taste builder offers.
    fn taste_profile(&self, cancel: &CancellationToken) -> Result<Vec<TasteArtist>, AccessError> {
        let _ = cancel;
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "choosing artists",
        ))
    }

    /// Tell YouTube Music the artists chosen: every one shown (`impressions`), the chosen ones
    /// (`selected`), by their [`TasteArtist`] values.
    fn set_taste_profile(
        &self,
        impressions: &[String],
        selected: &[String],
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let _ = (impressions, selected, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "choosing artists",
        ))
    }

    /// Delete one of the account's uploads, a song by its [`AccountTrack::delete_id`] or an
    /// album by its page's id. Asked first, in the app.
    fn delete_upload(
        &self,
        entity_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let _ = (entity_id, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "deleting uploads",
        ))
    }

    /// Tell the account a song was played (history, and what its recommendations learn from).
    fn report_play(&self, video_id: &str, cancel: &CancellationToken) -> Result<(), AccessError>;

    /// Begin telling one listen of a song (How much I listened): the catalog is asked, as the
    /// account, how its own player would track it. Nothing is told by asking.
    fn start_listen(
        &self,
        video_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Listen, AccessError> {
        let _ = (video_id, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "listening time",
        ))
    }

    /// Tell the account the listen was a play, as [`AccountAccess::report_play`] does, through
    /// the listen's own tracking (so the play and its listening time are one listen there).
    fn report_listen_play(
        &self,
        listen: &Listen,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        self.report_play(&listen.video_id, cancel)
    }

    /// Tell the account how much of the listen was heard since the last report.
    fn report_heard(
        &self,
        listen: &Listen,
        heard: &Heard,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError> {
        let _ = (listen, heard, cancel);
        Err(AccessError::new(
            crate::AccessErrorKind::Unsupported,
            "listening time",
        ))
    }

    /// Send a file to the account's uploads.
    fn upload(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    /// The identities the login can act as, the one in use marked.
    fn identities(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<AccountIdentityInfo>, AccessError>;

    /// Act as this identity from now on (`None`: the login's own).
    fn use_identity(&self, id: Option<&str>);

    /// Open a playlist the account owns to collaborators; the invite link.
    fn invite(&self, playlist_id: &str, cancel: &CancellationToken) -> Result<String, AccessError>;

    /// Stop new collaborators joining (or let them again).
    fn close_to_collaborators(
        &self,
        playlist_id: &str,
        closed: bool,
        cancel: &CancellationToken,
    ) -> Result<(), AccessError>;

    /// Join a collaborative playlist from its invite link; its playlist id.
    fn join(&self, invite_link: &str, cancel: &CancellationToken) -> Result<String, AccessError>;
}
