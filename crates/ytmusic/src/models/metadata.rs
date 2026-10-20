//! Search + next(queue) parsing. context/08.
//!
//! YouTube's response is a deeply-nested "renderer" tree. Rather than port Metrolist's ~40
//! renderer classes, we walk the raw JSON for the two node types we need
//! (`musicResponsiveListItemRenderer` for search, `playlistPanelVideoRenderer` for next) and
//! pull only the handful of fields the playback path uses. Targeted and robust to the tree
//! moving around (fixture reality wins over spec — see plan risks).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A song item — the minimum the playback path (context/06) needs. context/08.
/// Round-trips through the UI (serialized into search results, deserialized back into `play`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SongItem {
    pub video_id: String,
    pub title: String,
    pub artists: String,
    /// The primary artist's channel browseId (`UC…`), when the row links one — lets the UI make
    /// the artist name navigate to its artist page. context/08.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
    /// The artist line split into its original runs, each tagged with its own channel id when it
    /// links one — a collab ("Future & Metro Boomin") links each name separately. Empty when the
    /// row links no artist at all; the UI then falls back to plain `artists`. context/08.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artist_runs: Vec<ArtistRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// The album's browseId (`MPRE…`), when the row links one — lets the UI navigate to the album.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    /// The row's play count as YouTube abbreviates it ("53M"), from an album page's plays column.
    /// Absent on rows that don't carry one (playlists, search, queue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_count: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// `playlistSetVideoId` — the item's id *within a playlist*, needed to remove it (context/01
    /// edit_playlist). Only present when the item came from a playlist page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_video_id: Option<String>,
    /// Who put this track in the playlist, on a collaborative one: their names and one avatar per
    /// person off the row's `contributorsAvatars`. Usually one, several on a Taste Match row that
    /// more than one member's taste picked (#102). Empty everywhere else, since an ordinary
    /// playlist has one contributor and YouTube doesn't send the stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_by_avatars: Vec<String>,
    /// The channel (`UC…`) a tap on that stack opens: the person who added it. One per stack,
    /// so a stack of several people names none of theirs apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by_channel: Option<String>,
    /// The row's votes, on a collaborative playlist that has them ([`PlaylistVote`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vote: Option<PlaylistVote>,
    /// "Remove from history" off a history row's own menu: the token that takes this one play
    /// out. Only `FEmusic_history` rows carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_from_history: Option<String>,
    /// The signed-in user's rating of this track, from the row's `likeStatus`. `None` when the
    /// response didn't carry one, which the UI treats the same as [`Rating::Indifferent`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<Rating>,
    /// Listen Together: username of the guest who added this queue item (`None` for the user's own
    /// tracks). Never parsed from YouTube — pure queue metadata, carried for attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_by: Option<String>,
    /// "Play next" (or a guest's session add): marks the "up next" block so successive adds stack
    /// FIFO right after the current song. Pure queue metadata, never parsed.
    #[serde(default)]
    pub queued: bool,
    /// "Add to queue": appended at the tail, after everything the user picked. Its own block in the
    /// queue panel — without this it would read as part of the playlist that's playing. Pure queue
    /// metadata, never parsed.
    #[serde(default)]
    pub queued_end: bool,
    /// What either block was added from ("Nightcore Bangers"), when it came from an album/playlist.
    /// The panel heads the block with it instead of the playing playlist's name. `None` for
    /// single-song adds. Pure queue metadata, never parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_from: Option<String>,
    /// Appended by autoplay radio continuation (vs. chosen by the user). Drives the queue's
    /// "Autoplay" divider + player-bar badge. Pure queue metadata, never parsed.
    #[serde(default)]
    pub autoplay: bool,
    /// This row links a music video, not the audio track ([`is_video_row`]). Drives the
    /// "hide music videos" setting; computed once here, never re-derived downstream.
    #[serde(default)]
    pub is_video: bool,
    /// The same song's other version: the music video for an audio track, or the audio track for
    /// a video. What the player's Song/Video switch swaps to (#442). Known only where YouTube
    /// pairs them: an album row we swapped to its audio id, and a signed-in radio's wrapper rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterpart: Option<String>,
    /// One of the signed-in user's own uploads ([`is_upload_row`]). Only an authenticated client
    /// can stream these, so the orchestrator swaps in a login-only fallback chain. Issue #71.
    #[serde(default)]
    pub is_upload: bool,
    /// YouTube marks this track explicit ([`is_explicit`]). Only browse/search rows carry the
    /// badge: `/next` panel rows and `/player` don't, so a radio- or autoplay-appended track
    /// reads false even when the same song shows the badge on an album page.
    #[serde(default)]
    pub explicit: bool,
    /// "Add to library" / "Remove from library" off this row's own menu ([`library_toggle`]).
    /// `None` on rows YouTube sent no such menu for, and on the ones this app builds itself (local
    /// files, On Repeat), where there is nothing to send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryToggle>,
    /// "Pin to Listen again" / "Unpin from Listen again" off the row's own menu ([`pin_toggle`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<PinToggle>,
    /// The row menu's "tell me less of this" items ("Not interested"), in its order
    /// ([`menu_feedback`]). Empty on rows that carry none, which is most of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<MenuFeedback>,
    /// One of the account's uploads: the id its Delete item sends (`musicDeletePrivatelyOwnedEntityCommand`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_upload: Option<String>,
}

/// A song's "Pin to Listen again", as its menu states it: pinned or not, and the token for each
/// way. Like [`LibraryToggle`], a feedback toggle minted with the row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinToggle {
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpin_token: Option<String>,
}

/// One "tell me less of this" item off a row's menu ("Not interested", "Don't recommend …"): its
/// icon, YouTube's own words for it (localised, so they are shown as they come), and the token
/// that sends it. One way only: YouTube's own menus offer no way back from Venuta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MenuFeedback {
    pub icon: String,
    pub label: String,
    pub token: String,
}

/// Everything a song row's own menu lets the account do with it, read once ([`row_menu`]):
/// what a card or a row Venuta reads with its own parser carries along.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RowMenu {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryToggle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<PinToggle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<MenuFeedback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_upload: Option<String>,
}

impl SongItem {
    /// What this row's own menu offers the account, as a card carries it.
    pub fn menu(&self) -> RowMenu {
        RowMenu {
            library: self.library.clone(),
            pin: self.pin.clone(),
            feedback: self.feedback.clone(),
            delete_upload: self.delete_upload.clone(),
        }
    }
}

impl RowMenu {
    /// Nothing to offer: a signed-out row, or one Venuta built itself.
    pub fn is_empty(&self) -> bool {
        self.library.is_none()
            && self.pin.is_none()
            && self.feedback.is_empty()
            && self.delete_upload.is_none()
    }
}

/// A track's library membership as its menu states it, with the tokens for both directions.
///
/// Library ▸ Songs is not Liked Music: adding a song to the library is a `feedbackEndpoint`, a
/// separate write from the rating, and the only handle on it is an opaque token minted with the
/// row. That is why this rides along on every `SongItem` instead of being asked for later, and why
/// a row we built ourselves can't offer the action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryToggle {
    /// Whether the song is in the library right now, per the menu YouTube sent with the row.
    pub in_library: bool,
    /// One token per direction. Both are present when the row's menu is a *toggle*; a row that
    /// offers the action one way only (a plain menu item, which is how some surfaces send it)
    /// carries just the one it offers, and the way back has to come from a re-fetched row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_token: Option<String>,
}

/// A row's votes on a collaborative playlist, off its `engagementBar`'s `votingViewModel`.
///
/// Like the library toggle, each way is an opaque `feedbackEndpoint` token minted with the row:
/// a button sends `token` while it is up, and its toggled state's `undo_token` once pressed, which
/// takes the vote back. The counts are YouTube's own words for each way your vote can stand
/// (`compactVotes`, `…Upvoted`, `…Downvoted`), so pressing needs no arithmetic of ours.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaylistVote {
    /// How the signed-in account voted, as the row came.
    pub mine: VoteChoice,
    /// The net count YouTube gave with the row.
    pub votes: i64,
    /// The count to show with no vote of yours, with yours up, with yours down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_up: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_down: Option<String>,
    pub up: VoteButton,
    pub down: VoteButton,
}

/// One vote button's two taps: the vote, and taking it back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoteButton {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_token: Option<String>,
}

/// Which way the signed-in account voted on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoteChoice {
    Up,
    Down,
    /// No vote (`VOTE_STATUS_UNSPECIFIED`).
    Neutral,
}

/// How the signed-in user rated a track. One type for both directions: it's what a row's
/// `likeStatus` parses into, and what the write action ([`crate::InnerTube::rate`]) takes. The three
/// values are mutually exclusive on YouTube's side, so rating a liked track "dislike" un-likes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    Like,
    Dislike,
    /// No rating: what `like/removelike` leaves behind, and what an unrated row reports.
    Indifferent,
}

/// `MUSIC_VIDEO_TYPE_ATV` — the audio track YouTube Music generates for a release. Anything else
/// (`_OMV`, `_UGC`) is a video upload, except [`UPLOADED_TRACK_TYPE`].
pub(crate) const AUDIO_TRACK_TYPE: &str = "MUSIC_VIDEO_TYPE_ATV";

/// A track the signed-in user uploaded to their own library. Audio, not a music video: counting
/// it as one hid every upload under the "hide music videos" setting, and put the player view into
/// video mode on a track that has no video at all. Issue #71.
pub(crate) const UPLOADED_TRACK_TYPE: &str = "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK";

/// True for a `musicVideoType` that means "a music video", i.e. neither of the two audio kinds.
pub(crate) fn is_video_type(t: &str) -> bool {
    t != AUDIO_TRACK_TYPE && t != UPLOADED_TRACK_TYPE
}

/// The `musicVideoType` a watch endpoint carries, if any.
fn endpoint_video_type(endpoint: &Value) -> Option<&str> {
    endpoint
        .get("watchEndpoint")
        .or_else(|| endpoint.get("watchPlaylistEndpoint"))?
        .get("watchEndpointMusicSupportedConfigs")?
        .get("watchEndpointMusicConfig")?
        .get("musicVideoType")?
        .as_str()
}

/// True when a watch endpoint points at a music video rather than the audio track.
pub(crate) fn is_video_endpoint(endpoint: &Value) -> bool {
    matches!(endpoint_video_type(endpoint), Some(t) if is_video_type(t))
}

/// True when a watch endpoint points at one of the user's own uploads.
pub(crate) fn is_upload_endpoint(endpoint: &Value) -> bool {
    endpoint_video_type(endpoint) == Some(UPLOADED_TRACK_TYPE)
}

/// True when a renderer row (list row, two-row card, or queue panel row) links a music video.
///
/// The authoritative tag sits on the thumbnail overlay's play button (`overlay` on list rows,
/// `thumbnailOverlay` on cards); queue-panel rows have no overlay, so the row's own navigation
/// endpoint is the fallback. Absent ⇒ we can't tell ⇒ audio, so a parse that misses the tag
/// degrades to "keep everything" rather than hiding the library.
pub(crate) fn is_video_row(node: &Value) -> bool {
    matches!(row_video_type(node), Some(t) if is_video_type(t))
}

/// True when a renderer row links one of the user's own uploads. Same lookup as [`is_video_row`].
pub(crate) fn is_upload_row(node: &Value) -> bool {
    row_video_type(node) == Some(UPLOADED_TRACK_TYPE)
}

/// The `musicVideoType` a row claims, from the overlay play button when it carries one.
fn row_video_type(node: &Value) -> Option<&str> {
    let overlay = node
        .get("overlay")
        .or_else(|| node.get("thumbnailOverlay"))
        .and_then(|o| o.get("musicItemThumbnailOverlayRenderer"))
        .and_then(|o| o.get("content"))
        .and_then(|c| c.get("musicPlayButtonRenderer"))
        .and_then(|p| p.get("playNavigationEndpoint"))
        .filter(|ep| endpoint_video_type(ep).is_some());
    match overlay {
        Some(ep) => endpoint_video_type(ep),
        None => node.get("navigationEndpoint").and_then(endpoint_video_type),
    }
}

/// True when a renderer node wears YouTube's explicit badge. The badge is the same
/// `musicInlineBadgeRenderer` everywhere, under one of three keys depending on the node: `badges`
/// on a list row, `subtitleBadges` on a two-row card, `subtitleBadge` (singular) on a
/// playlist/album header. Scoped to those keys rather than swept from the whole node, so a card's
/// badge can't leak onto the row that contains it. Live-verified 2026-08.
pub(crate) fn is_explicit(node: &Value) -> bool {
    ["badges", "subtitleBadges", "subtitleBadge"]
        .into_iter()
        .filter_map(|key| node.get(key))
        .any(|b| {
            find_all(b, "iconType")
                .into_iter()
                .any(|t| t.as_str() == Some("MUSIC_EXPLICIT_BADGE"))
        })
}

/// One run of an artist line: the literal text plus its channel browseId when it links one
/// (separators like " & " carry no id).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtistRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub items: Vec<SongItem>,
}

/// A watch endpoint as YouTube puts it on a button, a tuning chip or the automix marker: what
/// `next` is asked with to open that queue (`InnerTube::next_watch`). Replayed as it came, never
/// rebuilt: its `params` are opaque, and they are what makes a radio a radio, a shuffle a shuffle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEndpoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist_set_video_id: Option<String>,
}

/// `params` of YouTube's own "Start radio" endpoints (ytmusicapi's `radio=True`).
pub const RADIO_PARAMS: &str = "wAEB";

impl WatchEndpoint {
    /// A song's radio when no button names one: `RDAMVM` + its id, started the way YouTube's own
    /// "Start radio" starts it. The fallback only; a server-sent endpoint always wins.
    pub fn song_radio(video_id: &str) -> Self {
        WatchEndpoint {
            video_id: Some(video_id.to_owned()),
            playlist_id: Some(format!("RDAMVM{video_id}")),
            params: Some(RADIO_PARAMS.to_owned()),
            ..Default::default()
        }
    }

    /// True when it names nothing to play.
    pub fn is_empty(&self) -> bool {
        self.video_id.is_none() && self.playlist_id.is_none()
    }

    /// The fields of one `watchEndpoint` / `watchPlaylistEndpoint` object.
    pub(crate) fn read(node: &Value) -> Option<Self> {
        let s = |k: &str| {
            node.get(k)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let out = WatchEndpoint {
            video_id: s("videoId"),
            playlist_id: s("playlistId"),
            params: s("params"),
            index: node
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|i| u32::try_from(i).ok()),
            playlist_set_video_id: s("playlistSetVideoId"),
        };
        (!out.is_empty()).then_some(out)
    }

    /// The first watch endpoint inside `node` (either kind), as above.
    pub(crate) fn within(node: &Value) -> Option<Self> {
        find_all(node, "watchEndpoint")
            .into_iter()
            .chain(find_all(node, "watchPlaylistEndpoint"))
            .find_map(Self::read)
    }
}

/// One of the tuning chips above Up next ("All", "Familiar", "Deep cuts", a mood…): choosing it
/// asks `next` with its endpoint and the answer replaces what is coming up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueChip {
    pub text: String,
    /// The chip YouTube marks as the one in effect.
    pub selected: bool,
    pub endpoint: WatchEndpoint,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NextResult {
    pub items: Vec<SongItem>,
    /// The next page of this queue (`playlistPanelRenderer.continuations`, a radio's
    /// `nextRadioContinuationData` or a list's `nextContinuationData`); `None` on its last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
    /// The queue's own list id, as the panel states it (a radio's `RDAMVM…`, a chip's `RDAT…`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_id: Option<String>,
    /// What YouTube's player names the queue ("Kitni Baatein Radio"): the queue header's subtitle,
    /// else the panel's own title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The tuning chips above the queue (radios only, first page only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chips: Vec<QueueChip>,
    /// The endpoint the panel's automix marker names: what Autoplay goes on with when this list
    /// ends. [`Self::automix_playlist_id`] is its list id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automix: Option<WatchEndpoint>,
    /// The Related tab's browseId (`MPTRt…`, `MUSIC_PAGE_TYPE_TRACK_RELATED`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_browse_id: Option<String>,
    /// The lyrics tab's browseId (`MPLYt…`) — feed it to a lyrics `browse` (models::lyrics).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_browse_id: Option<String>,
    /// The mix the panel says it continues into (`automixPreviewVideoRenderer`, context/08): a
    /// radio playlist id to re-request `next` with. Present on a bare `next(videoId)`, which is
    /// otherwise just the seed song — that's how a dead `RDAMVM` radio finds a live one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automix_playlist_id: Option<String>,
    /// The signed-in user's rating of the **requested** video, off `playerOverlays`. The panel
    /// rows carry no `likeStatus` of their own (0 of 50, live-checked 2026-08-24), so this is the
    /// only place a `next` response states one, and it is what YouTube Music's own player bar
    /// draws its thumbs-up from. `None` when signed out or when YouTube omits the overlay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<Rating>,
}

/// Logged-in account summary from `account/account_menu`. context/01, context/04A, context/15.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountInfo {
    pub name: Option<String>,
    pub handle: Option<String>,
    pub email: Option<String>,
    pub thumbnail: Option<String>,
    /// The channel browse id (`UC…`) when the response links one. Never used for delegation.
    pub channel_id: Option<String>,
    /// `onBehalfOfUser` id, `||`-split (context/04A). None when absent / single-account.
    pub data_sync_id: Option<String>,
    /// A login-bound visitorData, if the response carried one (context/15).
    pub visitor_data: Option<String>,
}

/// One usable YouTube identity returned by `account/accounts_list`.
///
/// `data_sync_id` is server-issued identity material from `datasyncIdToken` or `pageIdToken`, not
/// something inferred from the channel browse id. Rows without such a token are intentionally not
/// returned: displaying an identity that requests cannot actually select is worse than omitting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub name: String,
    pub handle: Option<String>,
    pub email: Option<String>,
    pub thumbnail: Option<String>,
    pub channel_id: Option<String>,
    pub data_sync_id: String,
    /// YouTube's current/default marker. The app's persisted choice may deliberately differ.
    pub is_selected: bool,
}

/// Parse a `search` response into song items. context/08.
pub fn parse_search(root: &Value) -> SearchResult {
    let mut items = Vec::new();
    for node in find_all(root, "musicResponsiveListItemRenderer") {
        if let Some(item) = parse_list_item(node) {
            items.push(item);
        }
    }
    SearchResult { items }
}

/// Parse a `next` response into the up-next queue + continuation token. context/08.
pub fn parse_next(root: &Value) -> NextResult {
    let mut items = Vec::new();
    let mut rows = Vec::new();
    panel_rows(root, &mut rows);
    for (node, counterpart) in rows {
        if let Some(mut item) = parse_panel_video(node) {
            item.counterpart = counterpart;
            items.push(item);
        }
    }
    // The queue panel: `playlistPanelRenderer` on a first answer, `playlistPanelContinuation` on
    // a page fetched with its continuation.
    let panel = find_all(root, "playlistPanelRenderer")
        .into_iter()
        .chain(find_all(root, "playlistPanelContinuation"))
        .next();
    let automix = find_all(root, "automixPreviewVideoRenderer")
        .into_iter()
        .find_map(WatchEndpoint::within);
    let automix_playlist_id = automix.as_ref().and_then(|e| e.playlist_id.clone());
    NextResult {
        items,
        continuation: panel.and_then(panel_continuation),
        playlist_id: panel
            .and_then(|p| p.get("playlistId"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        title: queue_title(root, panel),
        chips: queue_chips(root),
        automix,
        related_browse_id: tab_browse_id(root, "MUSIC_PAGE_TYPE_TRACK_RELATED"),
        lyrics_browse_id: lyrics_browse_id(root),
        automix_playlist_id,
        // Scoped to the overlay on purpose: a bare `find_first_str` over the whole response would
        // read whatever a future panel row starts carrying instead of the requested video's own.
        rating: root.get("playerOverlays").and_then(like_status),
    }
}

/// Every panel row in order, ignoring the `counterpart` arm of a
/// `playlistPanelVideoWrapperRenderer`. A signed-in radio wraps each track's audio row together
/// with its music-video twin (what YouTube's own Song/Video toggle switches between), so a plain
/// [`find_all`] takes both and the queue gets every song twice. Issue #170.
///
/// The twin's id rides along with its primary row instead, for our own Song/Video switch (#442).
fn panel_rows<'a>(node: &'a Value, out: &mut Vec<(&'a Value, Option<String>)>) {
    match node {
        Value::Object(map) => {
            let start = out.len();
            for (k, v) in map {
                if k == "counterpart" {
                    continue;
                }
                if k == "playlistPanelVideoRenderer" {
                    out.push((v, None));
                }
                panel_rows(v, out);
            }
            if let Some(twin) = map
                .get("counterpart")
                .and_then(|c| find_first_str(c, "videoId"))
            {
                // A wrapper holds exactly one primary row; anything else is a shape we don't know.
                if let [(_, slot)] = &mut out[start..] {
                    *slot = Some(twin);
                }
            }
        }
        Value::Array(arr) => arr.iter().for_each(|e| panel_rows(e, out)),
        _ => {}
    }
}

/// The queue panel's own next page: a radio's `nextRadioContinuationData`, a list's
/// `nextContinuationData`. Read off the panel only, never the first token anywhere in the answer.
fn panel_continuation(panel: &Value) -> Option<String> {
    panel
        .get("continuations")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|c| {
            c.get("nextRadioContinuationData")
                .or_else(|| c.get("nextContinuationData"))
                .and_then(|d| d.get("continuation"))
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        })
}

/// What the queue is called: the queue header's subtitle, else the panel's title (a plain string
/// on most answers, runs on some).
fn queue_title(root: &Value, panel: Option<&Value>) -> Option<String> {
    find_all(root, "musicQueueHeaderRenderer")
        .into_iter()
        .find_map(|h| runs_text(h.get("subtitle")))
        .or_else(|| {
            let t = panel?.get("title")?;
            t.as_str().map(str::to_owned).or_else(|| runs_text(Some(t)))
        })
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// The tuning chips (`musicQueueRenderer.subHeaderChipCloud`), each with the endpoint its
/// `queueUpdateCommand` fetches. A chip with no endpoint can't be chosen, so it isn't offered.
fn queue_chips(root: &Value) -> Vec<QueueChip> {
    let Some(cloud) = find_all(root, "subHeaderChipCloud").into_iter().next() else {
        return Vec::new();
    };
    find_all(cloud, "chipCloudChipRenderer")
        .into_iter()
        .filter_map(|chip| {
            let text = runs_text(chip.get("text")).or_else(|| {
                chip.get("text")
                    .and_then(|t| t.get("simpleText"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })?;
            let nav = chip.get("navigationEndpoint")?;
            let endpoint = nav
                .get("queueUpdateCommand")
                .and_then(|q| q.get("fetchContentsCommand"))
                .and_then(WatchEndpoint::within)
                .or_else(|| WatchEndpoint::within(nav))?;
            Some(QueueChip {
                text,
                selected: chip
                    .get("isSelected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                endpoint,
            })
        })
        .collect()
}

/// The songs on a Related page (`browse` of [`NextResult::related_browse_id`]): its list rows,
/// in order. The page's playlists, artists and words are not songs and stay out.
pub fn parse_related_songs(root: &Value) -> Vec<SongItem> {
    find_all_shallow(root, "musicResponsiveListItemRenderer")
        .into_iter()
        .filter_map(parse_list_item)
        .collect()
}

/// The lyrics tab's browseId from a `next` response: the browseEndpoint whose pageType is
/// `MUSIC_PAGE_TYPE_TRACK_LYRICS`. context/08 §lyrics.
fn lyrics_browse_id(root: &Value) -> Option<String> {
    tab_browse_id(root, "MUSIC_PAGE_TYPE_TRACK_LYRICS")
}

/// The browseId of the tab whose page type is `page_type` (Lyrics, Related).
fn tab_browse_id(root: &Value, page_type: &str) -> Option<String> {
    find_all(root, "browseEndpoint").into_iter().find_map(|be| {
        (find_first_str(be, "pageType").as_deref() == Some(page_type))
            .then(|| {
                be.get("browseId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten()
    })
}

/// Parse an `account/account_menu` response into an account summary. context/01, context/15.
pub fn parse_account_menu(root: &Value) -> AccountInfo {
    let header = find_all(root, "activeAccountHeaderRenderer")
        .into_iter()
        .next();
    let name = header.and_then(|h| account_text(h.get("accountName")));
    let handle = header.and_then(|h| account_text(h.get("channelHandle")));
    let email = header.and_then(|h| account_text(h.get("email")));
    let thumbnail = header.and_then(last_thumbnail);
    let channel_id = header.and_then(channel_browse_id);

    let rc = root.get("responseContext");
    // dataSyncId lives in the response context, not the menu header. context/04A.
    let data_sync_id = response_data_sync_id(root);
    let visitor_data = rc
        .and_then(|r| r.get("visitorData"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    AccountInfo {
        name,
        handle,
        email,
        thumbnail,
        channel_id,
        data_sync_id,
        visitor_data,
    }
}

/// Parse every selectable channel from `account/accounts_list`.
///
/// The action envelope differs between YouTube clients, so this deliberately anchors on
/// `accountSectionListRenderer` / `accountItem` rather than one absolute JSON path. Section headers
/// provide the Google-account email; each row provides its channel metadata and supported identity
/// tokens.
pub fn parse_account_identities(root: &Value) -> Vec<AccountIdentity> {
    let response_id = response_data_sync_id(root);
    let mut identities = Vec::new();
    let sections = find_all(root, "accountSectionListRenderer");

    if sections.is_empty() {
        for item in find_all(root, "accountItem") {
            push_account_identity(&mut identities, item, None, response_id.as_deref());
        }
        return identities;
    }

    for section in sections {
        // Only the Google-account header. A section *title* is a group label ("Brand accounts"),
        // not an address, and putting one in `email` shows it under a channel name in the picker.
        let email = find_all(section, "googleAccountHeaderRenderer")
            .into_iter()
            .find_map(|h| account_text(h.get("email")));
        for item in find_all(section, "accountItem") {
            push_account_identity(
                &mut identities,
                item,
                email.as_deref(),
                response_id.as_deref(),
            );
        }
    }
    identities
}

fn push_account_identity(
    identities: &mut Vec<AccountIdentity>,
    item: &Value,
    email: Option<&str>,
    response_id: Option<&str>,
) {
    let Some(identity) = parse_account_identity(item, email, response_id) else {
        return;
    };
    if identities
        .iter()
        .all(|i| i.data_sync_id != identity.data_sync_id)
    {
        identities.push(identity);
    }
}

fn parse_account_identity(
    item: &Value,
    email: Option<&str>,
    response_id: Option<&str>,
) -> Option<AccountIdentity> {
    if item.get("isDisabled").and_then(Value::as_bool) == Some(true)
        || item.get("hasChannel").and_then(Value::as_bool) == Some(false)
    {
        return None;
    }
    let name = account_text(item.get("accountName"))?;
    let endpoint = item
        .get("serviceEndpoint")?
        .get("selectActiveIdentityEndpoint")?;
    let tokens = endpoint
        .get("supportedTokens")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    // Prefer the purpose-built data-sync token. Some WEB responses only provide pageIdToken;
    // that is still an explicit server-issued identity token and is the value YouTube's web
    // switcher uses for onBehalfOfUser. A `UC…` browse id is never substituted here.
    let token_data_sync_id = tokens.iter().find_map(|token| {
        let value = token
            .get("datasyncIdToken")
            .or_else(|| token.get("dataSyncIdToken"))?;
        value
            .get("datasyncIdToken")
            .or_else(|| value.get("datasyncId"))
            .or_else(|| value.get("dataSyncId"))
            .and_then(Value::as_str)
            .and_then(normalize_data_sync_id)
    });
    let page_id = tokens.iter().find_map(|token| {
        token
            .get("pageIdToken")
            .and_then(|value| value.get("pageId"))
            .and_then(Value::as_str)
            .and_then(nonempty_string)
    });
    let is_selected = item
        .get("isSelected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let selected_response_id = is_selected
        .then_some(response_id)
        .flatten()
        .map(str::to_owned);
    let data_sync_id = token_data_sync_id.or(page_id).or(selected_response_id)?;

    Some(AccountIdentity {
        name,
        handle: account_text(item.get("channelHandle")),
        email: email.and_then(nonempty_string),
        thumbnail: last_thumbnail(item),
        channel_id: channel_browse_id(item),
        data_sync_id,
        is_selected,
    })
}

fn account_text(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| {
        runs_text_opt(v)
            .or_else(|| {
                v.get("simpleText")
                    .and_then(Value::as_str)
                    .and_then(nonempty_string)
            })
            .or_else(|| v.as_str().and_then(nonempty_string))
    })
}

fn channel_browse_id(node: &Value) -> Option<String> {
    find_all(node, "browseEndpoint")
        .into_iter()
        .find_map(|endpoint| {
            endpoint
                .get("browseId")
                .and_then(Value::as_str)
                .filter(|id| id.starts_with("UC"))
                .map(str::to_owned)
        })
}

fn response_data_sync_id(root: &Value) -> Option<String> {
    root.get("responseContext")
        .and_then(|r| r.get("mainAppWebResponseContext"))
        .and_then(|m| m.get("datasyncId").or_else(|| m.get("dataSyncId")))
        .and_then(Value::as_str)
        .and_then(normalize_data_sync_id)
}

fn normalize_data_sync_id(raw: &str) -> Option<String> {
    nonempty_string(&split_datasync_id(raw))
}

fn nonempty_string(raw: &str) -> Option<String> {
    (!raw.trim().is_empty()).then(|| raw.trim().to_owned())
}

/// Split a `dataSyncId` (`"<id>||<other>"`): prefer the part before `||`, else after. context/04A.
fn split_datasync_id(raw: &str) -> String {
    match raw.split_once("||") {
        Some((before, _)) if !before.is_empty() => before.to_owned(),
        Some((_, after)) => after.to_owned(),
        None => raw.to_owned(),
    }
}

// --- node parsers -------------------------------------------------------------------------

pub(crate) fn parse_list_item(node: &Value) -> Option<SongItem> {
    let video_id = list_item_video_id(node)?;
    let flex = node.get("flexColumns").and_then(Value::as_array);
    let title = flex
        .and_then(|c| c.first())
        .and_then(flex_text)
        .unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    // Second flex column holds subtitle runs: "Artist • Album • duration" (• separated).
    let subtitle_runs = flex_runs(node, 1);
    let (artists, album, duration) = split_subtitle(subtitle_runs);
    // Playlist/album rows keep the length in a fixed column instead of the subtitle. context/08.
    // Some rows carry it in the third *flex* column instead — the slot the album name and the
    // play count share. `play_count` rejects the duration shape (a count never carries a colon),
    // so read it the other way: a duration-shaped, unlinked third column is the length.
    let duration = duration
        .or_else(|| fixed_column_text(node))
        .or_else(|| flex_column_duration(node));
    // A video row's subtitle has no album field: its middle is the view count
    // ("Artist • 21M views • 4:51"). Gated on `musicVideoType`, not the text, since a
    // release can open with a digit ("1989 (Deluxe)").
    let is_video = is_video_row(node);
    // The album can also sit in its own column. A linked name goes first (#451, #377);
    // plain text is only the fallback for playlist and library rows.
    let album = album
        .filter(|_| !is_video)
        .or_else(|| {
            (3..flex.map_or(0, Vec::len))
                .find_map(|i| flex_runs(node, i).and_then(|r| album_from_runs(r)))
        })
        .or_else(|| album_column(node));
    let artist_id = subtitle_runs.and_then(|r| first_artist_id(r));
    // Only when this row's own menu offers the remove action. Every playlist row carries a
    // playlistSetVideoId, including rows on a playlist you can't edit (live-checked 2026-08-23),
    // so the id alone is not permission. The menu is: a collaborative playlist gives the remove
    // action on the rows you added and withholds it on everyone else's, which is exactly what
    // YouTube Music itself shows.
    let set_video_id = removable(node)
        .then(|| {
            node.get("playlistItemData")
                .and_then(|d| d.get("playlistSetVideoId"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .flatten();
    let (added_by, added_by_avatars) = contributors(node);
    Some(SongItem {
        video_id,
        title,
        artists,
        artist_id,
        artist_runs: subtitle_runs.map(|r| artist_runs(r)).unwrap_or_default(),
        album,
        album_id: album_id(node),
        duration,
        play_count: play_count(node),
        thumbnail: last_thumbnail(node),
        set_video_id,
        added_by_channel: contributor_channel(node),
        added_by,
        added_by_avatars,
        vote: playlist_vote(node),
        remove_from_history: history_removal(node),
        rating: like_status(node),
        queued_by: None,
        queued: false,
        queued_end: false,
        queued_from: None,
        autoplay: false,
        is_video,
        counterpart: None,
        is_upload: is_upload_row(node),
        explicit: is_explicit(node),
        library: library_toggle(node),
        pin: pin_toggle(node),
        feedback: menu_feedback(node),
        delete_upload: upload_entity(node),
    })
}

/// A podcast episode row (`musicMultiRowListItemRenderer`), the only row a show page (`MPSP…`),
/// Saved Episodes and the home Podcasts feed carry. Only the home rows name their show
/// (`secondTitle`); on a show page `artists` is left for the page to fill. Issue #286.
pub(crate) fn parse_episode_item(node: &Value) -> Option<SongItem> {
    let video_id = find_first_str(
        node.get("onTap").or_else(|| node.get("overlay"))?,
        "videoId",
    )?;
    let title = runs_text(node.get("title"))?;
    let duration = node
        .pointer("/playbackProgress/musicPlaybackProgressRenderer/durationText")
        .and_then(runs_text_opt)
        .and_then(|d| episode_duration(&d));
    Some(SongItem {
        video_id,
        title,
        artists: runs_text(node.get("secondTitle")).unwrap_or_default(),
        duration,
        thumbnail: last_thumbnail(node.get("thumbnail")?),
        // An episode's type is MUSIC_VIDEO_TYPE_PODCAST_EPISODE, which "hide music videos" would
        // otherwise drop. Video mode still reads the player response, so video podcasts keep it.
        is_video: false,
        library: library_toggle(node),
        pin: pin_toggle(node),
        feedback: menu_feedback(node),
        delete_upload: upload_entity(node),
        ..Default::default()
    })
}

/// " • 1 hr 4 min" → "1:04:00", " • 55 min" → "55:00": the colon form every other row uses.
/// ponytail: numbers only, so it survives a localized unit word; a lone number is read as minutes,
/// which makes an exact "2 hr" episode "2:00". The player's own duration corrects it on play.
fn episode_duration(text: &str) -> Option<String> {
    let n: Vec<u32> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok())
        .collect();
    match n[..] {
        [m] => Some(format!("{m}:00")),
        [h, m] => Some(format!("{h}:{m:02}:00")),
        _ => None,
    }
}

/// The play count from an album row's third flex column ("53M plays" → "53M"). Playlist and
/// library rows put the album name in that column instead, so the two have to be told apart.
///
/// The discriminator used to be the trailing word "plays", which only held while the locale was
/// pinned to en. The UI language now goes out as `hl` (#274), so it is structural instead: the
/// album column links its album page and a play count links nothing, and a count opens with a
/// digit in whatever numerals the locale writes ("53M plays", "9845만회 재생"). Live-verified
/// 2026-09-20 in en and ko across an album page and five playlists; 773 search rows in en kept
/// every play count and every album exactly as the old word match had them.
///
/// A leading digit alone let a bare duration through on rows where the length sits in the third
/// flex column rather than a fixed one: "3:42" opens with a digit, links nothing, and was read
/// as a play count — the album page then printed the track's length where the plays go. A count
/// never carries a colon, so the duration shape is excluded the same way `is_duration` admits
/// it.
pub(crate) fn play_count(node: &Value) -> Option<String> {
    let text = flex_column_text(node, 2)?;
    let text = text.trim();
    if flex_column_links(node, 2) || !text.starts_with(char::is_numeric) || text.contains(':') {
        return None;
    }
    // The unit word is noise; the number is the value. No space to cut at (Japanese writes
    // "9845万回再生") leaves the whole string, which still reads as a count.
    Some(
        text.rsplit_once(' ')
            .map_or(text, |(count, _)| count)
            .to_owned(),
    )
}

/// True when any run of the `i`th flex column navigates somewhere: what separates a linked album
/// from the play count that shares that column. See [`play_count`].
fn flex_column_links(node: &Value, i: usize) -> bool {
    flex_runs(node, i)
        .is_some_and(|runs| runs.iter().any(|r| r.get("navigationEndpoint").is_some()))
}

/// The album name from that same third flex column — the other thing it can hold. A playlist row's
/// subtitle is the artist alone ("Artist"), not the search/next form ("Artist • Album • 3:02"), so
/// without this every playlist track comes back album-less. Same discriminator as [`play_count`],
/// read the other way round. context/08.
fn album_column(node: &Value) -> Option<String> {
    let text = flex_column_text(node, 2)?;
    let text = text.trim();
    // Not a duration: the same column carries the length on rows with no fixed column, and
    // once `play_count` stopped claiming it the length would land in the album field.
    (!text.is_empty() && !is_duration(text) && play_count(node).is_none()).then(|| text.to_owned())
}

/// The album's browseId (`MPRE…`): either the linked album run or the row menu's "Go to album"
/// entry — whichever the renderer carries. Tolerant: first `MPRE…` browseId in the node. context/08.
pub(crate) fn album_id(node: &Value) -> Option<String> {
    find_all(node, "browseId")
        .into_iter()
        .filter_map(Value::as_str)
        .find(|id| id.starts_with("MPRE"))
        .map(str::to_owned)
}

/// The artist field of a run list, kept run by run so each linked artist of a collab navigates to
/// its own page. Empty when nothing links a channel. Cut at the "•" separators and dropping a
/// leading type label exactly like `split_subtitle`, so these runs describe the same field as the
/// `artists` string beside them: a search row reads "Song • Delara • 3:02", and taking everything
/// before the first "•" would hand back the unlinked word "Song". context/08.
pub(crate) fn artist_runs(runs: &[Value]) -> Vec<ArtistRun> {
    let mut fields: Vec<Vec<ArtistRun>> = vec![Vec::new()];
    for run in runs {
        let text = run.get("text").and_then(Value::as_str).unwrap_or_default();
        if text.trim() == "•" {
            fields.push(Vec::new());
        } else {
            fields.last_mut().expect("never empty").push(ArtistRun {
                text: text.to_owned(),
                id: first_artist_id(std::slice::from_ref(run)),
            });
        }
    }
    let linked = |f: &Vec<ArtistRun>| f.iter().any(|r| r.id.is_some());
    if fields.len() > 1 && !linked(&fields[0]) {
        let label: String = fields[0].iter().map(|r| r.text.as_str()).collect();
        if is_type_label(label.trim()) || fields[1..].iter().any(linked) {
            fields.remove(0);
        }
    }
    let out = fields.swap_remove(0);
    if !linked(&out) {
        return Vec::new();
    }
    out
}

/// First run that links an artist channel (`browseEndpoint.browseId` starting with `UC`). context/08.
pub(crate) fn first_artist_id(runs: &[Value]) -> Option<String> {
    runs.iter().find_map(|r| {
        let id = r
            .get("navigationEndpoint")?
            .get("browseEndpoint")?
            .get("browseId")?
            .as_str()?;
        id.starts_with("UC").then(|| id.to_owned())
    })
}

/// The album's name from a byline: the run that links an `MPRE…` album ("Delara • Sjelen • 2026").
/// Structural, so a music video's "Artist • 50M views" yields nothing rather than a view count,
/// and it reads the same in every locale. context/08.
pub(crate) fn album_from_runs(runs: &[Value]) -> Option<String> {
    runs.iter().find_map(|r| {
        let id = r
            .get("navigationEndpoint")?
            .get("browseEndpoint")?
            .get("browseId")?
            .as_str()?;
        let text = r.get("text")?.as_str()?.trim();
        (id.starts_with("MPRE") && !text.is_empty()).then(|| text.to_owned())
    })
}

/// The track's rating from its menu's `likeStatus` (`LIKE` / `INDIFFERENT` / `DISLIKE`).
/// Tolerant: grabs the first `likeStatus` anywhere in the node, and reads anything it doesn't
/// recognise as unrated rather than dropping the row. context/08.
/// Who added this row, on a collaborative playlist: their names for the tooltip and one avatar URL
/// per person, off a stack renderer YouTube only sends there. A Taste Match row several members
/// share stacks all of them, and a name is never required: the multi-person rows used to vanish
/// whole because the first avatar carried none (#102). Each avatar's own `accessibilityText`
/// first, else the stack's label.
fn contributors(node: &Value) -> (Option<String>, Vec<String>) {
    let Some(stack) = find_all(node, "contributorsAvatars").into_iter().next() else {
        return (None, Vec::new());
    };
    let avatars = find_all(stack, "avatarViewModel");
    let urls: Vec<String> = avatars
        .iter()
        .filter_map(|a| a.pointer("/image/sources/0/url").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    if urls.is_empty() {
        return (None, urls);
    }
    let names: Vec<&str> = avatars
        .iter()
        .filter_map(|a| a.get("accessibilityText").and_then(Value::as_str))
        .collect();
    let label = if names.is_empty() {
        find_all(stack, "label")
            .into_iter()
            .chain(find_all(stack, "content"))
            .find_map(Value::as_str)
            .map(str::to_owned)
    } else {
        Some(names.join(", "))
    };
    (label, urls)
}

/// The channel the contributor stack opens (`MUSIC_PAGE_TYPE_USER_CHANNEL`): who added the row.
fn contributor_channel(node: &Value) -> Option<String> {
    let stack = node.get("contributorsAvatars")?;
    find_all(stack, "browseEndpoint")
        .into_iter()
        .find_map(|b| b.get("browseId")?.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
}

/// The row's votes, when it is on a collaborative playlist with voting. Both buttons must carry
/// the token they send, or there is nothing to press: no votes then, rather than half of them.
/// Which way you voted is each button's `isToggled`, else the state YouTube names.
fn playlist_vote(node: &Value) -> Option<PlaylistVote> {
    let voting = find_all(node.get("engagementBar")?, "votingViewModel")
        .into_iter()
        .next()?;
    let tap = |state: Option<&Value>| {
        find_all(state?, "feedbackEndpoint")
            .into_iter()
            .find_map(|f| f.get("feedbackToken")?.as_str())
            .map(str::to_owned)
    };
    let button = |key: &str| {
        let toggle = voting.get(key)?.get("toggleButtonViewModel")?;
        let pressed = toggle
            .get("isToggled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let button = VoteButton {
            token: tap(toggle.get("defaultButtonViewModel"))?,
            undo_token: tap(toggle.get("toggledButtonViewModel")),
        };
        Some((button, pressed))
    };
    let (up, up_pressed) = button("upvoteButton")?;
    let (down, down_pressed) = button("downvoteButton")?;
    let state = voting.get("initialState");
    let text = |key: &str| {
        state
            .and_then(|s| s.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let status = state.and_then(|s| s.get("status")).and_then(Value::as_str);
    let mine = if up_pressed || status == Some("VOTE_STATUS_UPVOTED") {
        VoteChoice::Up
    } else if down_pressed || status == Some("VOTE_STATUS_DOWNVOTED") {
        VoteChoice::Down
    } else {
        VoteChoice::Neutral
    };
    let votes = state.and_then(|s| s.get("votes")).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    });
    Some(PlaylistVote {
        mine,
        votes: votes.unwrap_or(0),
        count: text("compactVotes"),
        count_up: text("compactVotesUpvoted"),
        count_down: text("compactVotesDownvoted"),
        up,
        down,
    })
}

/// "Remove from history" off the row's menu, by its icon: the words are localised.
fn history_removal(node: &Value) -> Option<String> {
    find_all(node.get("menu")?, "menuServiceItemRenderer")
        .into_iter()
        .find(|item| {
            item.get("icon")
                .and_then(|i| find_first_str(i, "iconType"))
                .as_deref()
                == Some("REMOVE_FROM_HISTORY")
        })
        .and_then(|item| {
            find_first_str(
                item.get("serviceEndpoint")?.get("feedbackEndpoint")?,
                "feedbackToken",
            )
        })
}

/// Does this row's menu carry "Remove from playlist"? The menu item is a `playlistEditEndpoint`
/// whose action is `ACTION_REMOVE_VIDEO`; YouTube only sends it where the account may remove that
/// item, so it is the per-row edit permission (owner, or collaborator on their own additions).
fn removable(node: &Value) -> bool {
    find_all(node, "action")
        .iter()
        .any(|a| a.as_str() == Some("ACTION_REMOVE_VIDEO"))
}

/// The row's "Save to library" action, in either shape YouTube sends it.
///
/// Usually a `toggleMenuServiceItemRenderer` holding both directions, one `feedbackEndpoint` each
/// ([`feedback_toggle`]). Some surfaces send a plain `menuServiceItemRenderer` with one direction
/// only; that is the fallback below.
///
/// Icons, not the label, because the label is localised. A track menu carries other feedback
/// toggles that must not match: "Add to liked songs" (FAVORITE/UNFAVORITE, a rating and a
/// different list) and "Pin to Listen again" (KEEP/KEEP_OFF, [`pin_toggle`]). Live-checked
/// 2026-08-28: search rows send BOOKMARK_BORDER/BOOKMARK; the LIBRARY_* spellings are kept as
/// YouTube has used them too.
fn library_toggle(node: &Value) -> Option<LibraryToggle> {
    /// The bookmark is empty when the song is not in the library — that direction adds it.
    const ADDS: [&str; 2] = ["BOOKMARK_BORDER", "LIBRARY_ADD"];
    /// Filled: it is in there, and this direction takes it out.
    const REMOVES: [&str; 3] = ["BOOKMARK", "LIBRARY_REMOVE", "LIBRARY_SAVED"];

    if let Some(t) = feedback_toggle(node, &ADDS, &REMOVES) {
        return Some(LibraryToggle {
            in_library: t.on,
            add_token: Some(t.on_token),
            remove_token: Some(t.off_token),
        });
    }
    find_all(row_menu_node(node), "menuServiceItemRenderer")
        .into_iter()
        .find_map(|item| {
            let token = feedback_token(item.get("serviceEndpoint"))?;
            let icon = find_first_str(item.get("icon")?, "iconType")?;
            if ADDS.contains(&icon.as_str()) {
                Some(LibraryToggle {
                    in_library: false,
                    add_token: Some(token),
                    remove_token: None,
                })
            } else if REMOVES.contains(&icon.as_str()) {
                Some(LibraryToggle {
                    in_library: true,
                    add_token: None,
                    remove_token: Some(token),
                })
            } else {
                None
            }
        })
}

/// The row's "Pin to Listen again" / "Unpin from Listen again": a feedback toggle like the
/// library's, told apart by its pin icons (KEEP pins, KEEP_OFF unpins).
fn pin_toggle(node: &Value) -> Option<PinToggle> {
    feedback_toggle(node, &["KEEP"], &["KEEP_OFF"]).map(|t| PinToggle {
        pinned: t.on,
        pin_token: Some(t.on_token),
        unpin_token: Some(t.off_token),
    })
}

/// A two-way feedback toggle, read: whether it stands on now, and the token for each way.
struct FeedbackToggle {
    on: bool,
    on_token: String,
    off_token: String,
}

/// The row menu's scope: `find_all` walks everything, and a row can carry other people's
/// endpoints (an album's, an artist's) elsewhere in its tree.
fn row_menu_node(node: &Value) -> &Value {
    node.get("menu").unwrap_or(node)
}

fn feedback_token(endpoint: Option<&Value>) -> Option<String> {
    find_first_str(endpoint?.get("feedbackEndpoint")?, "feedbackToken")
}

/// The `toggleMenuServiceItemRenderer` whose two icons are one of `turns_on` and one of
/// `turns_off`, each way sending a `feedbackEndpoint` token. Which way it stands is the side the
/// menu shows now: its toggled side when `isToggled` is true (a Library ▸ Songs row: "Remove from
/// library" over a "Save to library" default), else its default side. With no flag at all the
/// default side is the one on offer, which is how search rows send it (the icons swapped instead).
/// Signed out, a side asks you to sign in (`modalEndpoint`) and there is nothing to send: none.
fn feedback_toggle(node: &Value, turns_on: &[&str], turns_off: &[&str]) -> Option<FeedbackToggle> {
    find_all(row_menu_node(node), "toggleMenuServiceItemRenderer")
        .into_iter()
        .find_map(|toggle| {
            let icon = |key: &str| find_first_str(toggle.get(key)?, "iconType");
            let default = feedback_token(toggle.get("defaultServiceEndpoint"))?;
            let toggled = feedback_token(toggle.get("toggledServiceEndpoint"))?;
            let (default_icon, toggled_icon) = (icon("defaultIcon")?, icon("toggledIcon")?);
            let (default_icon, toggled_icon) = (default_icon.as_str(), toggled_icon.as_str());
            let default_turns_on =
                if turns_on.contains(&default_icon) && turns_off.contains(&toggled_icon) {
                    true
                } else if turns_off.contains(&default_icon) && turns_on.contains(&toggled_icon) {
                    false
                } else {
                    return None;
                };
            let shows_toggled = toggle
                .get("isToggled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            // The side on offer turns it on: then it is off now.
            let offered_turns_on = default_turns_on != shows_toggled;
            let (on_token, off_token) = if default_turns_on {
                (default, toggled)
            } else {
                (toggled, default)
            };
            Some(FeedbackToggle {
                on: !offered_turns_on,
                on_token,
                off_token,
            })
        })
}

/// The row menu's "tell me less of this" items, by their icons (the words are localised): a
/// plain `menuServiceItemRenderer` whose endpoint is a `feedbackEndpoint`. NOT_INTERESTED and
/// DISMISS are "Not interested"; REMOVE / REMOVE_CIRCLE are YouTube's "Don't recommend …".
/// Needs a live check: no live song menu has carried one in front of Venuta yet.
pub(crate) fn menu_feedback(node: &Value) -> Vec<MenuFeedback> {
    const LESS: [&str; 4] = ["NOT_INTERESTED", "DISMISS", "REMOVE", "REMOVE_CIRCLE"];
    let Some(menu) = node.get("menu") else {
        return Vec::new();
    };
    find_all(menu, "menuServiceItemRenderer")
        .into_iter()
        .filter_map(|item| {
            let icon = find_first_str(item.get("icon")?, "iconType")?;
            if !LESS.contains(&icon.as_str()) {
                return None;
            }
            let token = feedback_token(item.get("serviceEndpoint"))?;
            let label = runs_text(item.get("text"))?.trim().to_owned();
            (!label.is_empty()).then_some(MenuFeedback { icon, label, token })
        })
        .collect()
}

/// An upload's Delete item: the entity id its confirm dialog's button sends
/// (`musicDeletePrivatelyOwnedEntityCommand`), as ytmusicapi reads it for `delete_upload_entity`.
fn upload_entity(node: &Value) -> Option<String> {
    find_all(node.get("menu")?, "musicDeletePrivatelyOwnedEntityCommand")
        .into_iter()
        .find_map(|c| c.get("entityId")?.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// Everything a song row's own menu offers the account, for rows read by another parser (a
/// search page, a playlist page read raw): the same reading `SongItem` gets.
pub fn row_menu(node: &Value) -> RowMenu {
    RowMenu {
        library: library_toggle(node),
        pin: pin_toggle(node),
        feedback: menu_feedback(node),
        delete_upload: upload_entity(node),
    }
}

fn like_status(node: &Value) -> Option<Rating> {
    find_first_str(node, "likeStatus").map(|s| match s.as_str() {
        "LIKE" => Rating::Like,
        "DISLIKE" => Rating::Dislike,
        _ => Rating::Indifferent,
    })
}

fn parse_panel_video(node: &Value) -> Option<SongItem> {
    let video_id = node.get("videoId").and_then(Value::as_str)?.to_owned();
    let title = runs_text(node.get("title"))?;
    let byline = node
        .get("longBylineText")
        .or_else(|| node.get("shortBylineText"));
    let byline_runs = byline.and_then(|b| b.get("runs")).and_then(Value::as_array);
    // The byline is a full descriptor ("Delara • Sjelen • 2026"), not a name: take its artist
    // field only, or the queue (and the scrobbler behind it) gets the whole string as the artist.
    let artists = artists_from_runs(byline_runs).unwrap_or_default();
    let artist_id = byline_runs.and_then(|r| first_artist_id(r));
    let duration = node.get("lengthText").and_then(runs_text_opt);
    Some(SongItem {
        video_id,
        title,
        artists,
        artist_id,
        artist_runs: byline_runs.map(|r| artist_runs(r)).unwrap_or_default(),
        album: byline_runs.and_then(|r| album_from_runs(r)),
        album_id: album_id(node),
        duration,
        play_count: None,
        thumbnail: last_thumbnail(node),
        set_video_id: None,
        added_by: None,
        added_by_avatars: Vec::new(),
        added_by_channel: None,
        vote: None,
        remove_from_history: None,
        rating: like_status(node),
        queued_by: None,
        queued: false,
        queued_end: false,
        queued_from: None,
        autoplay: false,
        is_video: is_video_row(node),
        counterpart: None,
        is_upload: is_upload_row(node),
        // Queue-panel rows carry no badges today; asking anyway costs a map lookup and picks the
        // flag up for free if YouTube ever adds one.
        explicit: is_explicit(node),
        library: library_toggle(node),
        pin: pin_toggle(node),
        feedback: menu_feedback(node),
        delete_upload: upload_entity(node),
    })
}

/// Joined text of a `musicResponsiveListItemRenderer` flex column (0 = title, 1 = subtitle). Used
/// by the search-section parser to build cards from list rows. context/08.
/// The raw runs of a list row's `i`th flex column (the text with its per-run links intact).
pub(crate) fn flex_runs(node: &Value, i: usize) -> Option<&Vec<Value>> {
    node.get("flexColumns")
        .and_then(Value::as_array)
        .and_then(|c| c.get(i))
        .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
        .and_then(|r| r.get("text"))
        .and_then(|t| t.get("runs"))
        .and_then(Value::as_array)
}

pub(crate) fn flex_column_text(node: &Value, i: usize) -> Option<String> {
    node.get("flexColumns")
        .and_then(Value::as_array)
        .and_then(|c| c.get(i))
        .and_then(flex_text)
}

/// videoId from any of the three known locations. context/08 / AlbumPage.kt.
pub(crate) fn list_item_video_id(node: &Value) -> Option<String> {
    let direct = node
        .get("playlistItemData")
        .and_then(|d| d.get("videoId"))
        .and_then(Value::as_str)
        .or_else(|| {
            node.get("navigationEndpoint")
                .and_then(|n| n.get("watchEndpoint"))
                .and_then(|w| w.get("videoId"))
                .and_then(Value::as_str)
        });
    match direct {
        Some(id) => Some(id.to_owned()),
        // Last resort: the play-button overlay's watchEndpoint videoId.
        None => node
            .get("overlay")
            .and_then(|o| find_first_str(o, "videoId")),
    }
}

// --- small helpers ------------------------------------------------------------------------

/// The row's `fixedColumns` duration ("3:47"). Playlist and album track rows carry it here.
fn fixed_column_text(node: &Value) -> Option<String> {
    node.get("fixedColumns")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|c| {
            c.get("musicResponsiveListItemFixedColumnRenderer")
                .and_then(|r| r.get("text"))
                .and_then(runs_text_opt)
        })
        .filter(|s| is_duration(s))
}

/// The row's third *flex* column, when it holds a length rather than an album or a play count:
/// the same slot those two share, and the same duration shape `fixed_column_text` filters for.
/// Some playlist rows put the track's length here instead of in a fixed column, and without this
/// the row parsed duration-less once `play_count` stopped claiming the string for itself.
fn flex_column_duration(node: &Value) -> Option<String> {
    flex_column_text(node, 2).filter(|s| is_duration(s))
}

fn flex_text(col: &Value) -> Option<String> {
    col.get("musicResponsiveListItemFlexColumnRenderer")
        .and_then(|r| r.get("text"))
        .and_then(runs_text_opt)
}

pub(crate) fn runs_text(v: Option<&Value>) -> Option<String> {
    v.and_then(runs_text_opt)
}

/// Join all `runs[].text` in a `{ runs: [...] }` object.
pub(crate) fn runs_text_opt(v: &Value) -> Option<String> {
    let runs = v.get("runs").and_then(Value::as_array)?;
    let s: String = runs
        .iter()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .collect();
    (!s.is_empty()).then_some(s)
}

/// One "•"-separated field of a subtitle, plus whether it links an artist channel (`UC…`).
struct Group {
    text: String,
    artist_link: bool,
}

/// Cut a subtitle run list at its "•" separators, keeping each field's artist link.
fn subtitle_groups(runs: &[Value]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut cur = Group {
        text: String::new(),
        artist_link: false,
    };
    for run in runs {
        let t = run.get("text").and_then(Value::as_str).unwrap_or("");
        if t.trim() == "•" {
            groups.push(std::mem::replace(
                &mut cur,
                Group {
                    text: String::new(),
                    artist_link: false,
                },
            ));
        } else {
            cur.text.push_str(t);
            cur.artist_link |= first_artist_id(std::slice::from_ref(run)).is_some();
        }
    }
    groups.push(cur);
    for g in &mut groups {
        g.text = g.text.trim().to_string();
    }
    groups
}

/// Result rows on an unfiltered search lead with the result type: "Song • Delara • 3:02". Nothing
/// downstream wants that word, and taken as an artist it lands in the user's Last.fm scrobbles.
///
/// English only, and deliberately: YouTube localizes these words now that `hl` follows the UI
/// language (#274). [`split_subtitle`] carries two structural tests for the rest.
fn is_type_label(s: &str) -> bool {
    matches!(
        s,
        "Song"
            | "Video"
            | "Album"
            | "Single"
            | "EP"
            | "Playlist"
            | "Artist"
            | "Episode"
            | "Podcast"
    )
}

/// "3:02" / "1:04:11", and nothing else. A colon alone doesn't make a duration: an artist or album
/// can carry one ("Jorge Rivera-Herrans & Cast of EPIC: The Musical"), and taking that as the
/// length prints the whole title where the time goes and squeezes the rest of the row out.
fn is_duration(s: &str) -> bool {
    let s = s.trim();
    s.contains(':') && s.chars().all(|c| c.is_ascii_digit() || c == ':')
}

/// Split a "• "-separated subtitle run list into (artists, album, duration). context/08.
fn split_subtitle(runs: Option<&Vec<Value>>) -> (String, Option<String>, Option<String>) {
    let Some(runs) = runs else {
        return (String::new(), None, None);
    };
    let mut groups = subtitle_groups(runs);
    // Drop a leading type label so artist/album don't both shift one field to the right. A later
    // field linking an artist channel proves the first one isn't the artist; the word list covers
    // the rows where nothing is linked at all.
    //
    // Third test, for the rows where neither of those fires: `hl` follows the UI language now
    // (#274), so the word list is blind to "노래 • 2:43". Two fields whose second is the length
    // means there is no artist field at all (YouTube drops it when the query *is* the artist,
    // #216), so the first one is the label. Checked against 773 live search rows in en: the only
    // value this shape ever carried was "Song". A misread unlinked artist comes back from
    // `/player` (`backfill_metadata`) on play, where a stray "노래" would have stuck for good, in
    // the row and in the user's scrobbles.
    if groups.len() > 1
        && !groups[0].artist_link
        && (is_type_label(&groups[0].text)
            || groups[1..].iter().any(|g| g.artist_link)
            || (groups.len() == 2 && is_duration(&groups[1].text)))
    {
        groups.remove(0);
    }
    let groups: Vec<String> = groups.into_iter().map(|g| g.text).collect();
    // A row can carry no artist field at all: YouTube drops it when the query is the artist, so
    // every song row of a "boygenius" search reads "Song • 3:55". The length is not a name, and
    // taken as one it reaches the player bar and the user's scrobbles (#216). Leave it empty and
    // let `backfill_metadata` fill it from the player's author when the row is played.
    let artists = groups
        .first()
        .filter(|g| !is_duration(g))
        .cloned()
        .unwrap_or_default();
    // Last group that is a duration is the duration; the middle is album.
    let duration = groups.iter().rev().find(|g| is_duration(g)).cloned();
    let album = groups
        .get(1)
        .filter(|g| Some(*g) != duration.as_ref())
        .cloned();
    (artists, album, duration)
}

/// Just the artist field of a subtitle run list, for the surfaces that keep a flat string.
pub(crate) fn artists_from_runs(runs: Option<&Vec<Value>>) -> Option<String> {
    Some(split_subtitle(runs).0).filter(|s| !s.is_empty())
}

/// Just the duration field ("Song • Delara • 3:02" → "3:02"). Song cards carry it in the same
/// subtitle they take their artist from, and dropping it leaves the row blank where the time goes
/// once the card is queued.
pub(crate) fn duration_from_runs(runs: Option<&Vec<Value>>) -> Option<String> {
    split_subtitle(runs).2
}

/// Deepest/last thumbnail URL under this node (highest resolution).
pub(crate) fn last_thumbnail(node: &Value) -> Option<String> {
    // Find any `thumbnails: [ { url }, ... ]` array and take the last url.
    fn walk(v: &Value) -> Option<String> {
        match v {
            Value::Object(map) => {
                if let Some(arr) = map.get("thumbnails").and_then(Value::as_array) {
                    if let Some(url) = arr
                        .last()
                        .and_then(|t| t.get("url"))
                        .and_then(Value::as_str)
                    {
                        return Some(url.to_owned());
                    }
                }
                map.values().find_map(walk)
            }
            Value::Array(arr) => arr.iter().find_map(walk),
            _ => None,
        }
    }
    walk(node)
}

/// Recursively collect every object that is the value of a key named `key`.
pub(crate) fn find_all<'a>(root: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
        match v {
            Value::Object(map) => {
                for (k, val) in map {
                    if k == key {
                        out.push(val);
                    }
                    walk(val, key, out);
                }
            }
            Value::Array(arr) => arr.iter().for_each(|e| walk(e, key, out)),
            _ => {}
        }
    }
    walk(root, key, &mut out);
    out
}

/// Like [`find_all`], but does not descend into a node once it matches `key`. Use when collecting
/// "top-level" renderers (e.g. playlist track rows): an *editable* playlist item embeds a nested
/// copy of its own `musicResponsiveListItemRenderer` inside an add-suggestion edit command, so a
/// deep search counts every track twice. Stopping at the first match avoids that double-count.
pub(crate) fn find_all_shallow<'a>(root: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
        match v {
            Value::Object(map) => {
                for (k, val) in map {
                    if k == key {
                        out.push(val); // matched — do NOT recurse into it
                    } else {
                        walk(val, key, out);
                    }
                }
            }
            Value::Array(arr) => arr.iter().for_each(|e| walk(e, key, out)),
            _ => {}
        }
    }
    walk(root, key, &mut out);
    out
}

/// First string value under any key named `key`.
pub(crate) fn find_first_str(root: &Value, key: &str) -> Option<String> {
    match root {
        Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    if let Some(s) = v.as_str() {
                        return Some(s.to_owned());
                    }
                }
                if let Some(s) = find_first_str(v, key) {
                    return Some(s);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|e| find_first_str(e, key)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // The whole "hide music videos" feature is this predicate, and the way it fails is silent: a
    // wrong JSON path reads `None` everywhere, the filter quietly becomes a no-op, and the setting
    // just looks broken. So: every shape a row arrives in, plus the fail-open case.
    #[test]
    fn video_rows_are_recognised_in_every_renderer_shape() {
        let cfg = |t: &str| {
            json!({ "watchEndpoint": { "videoId": "v",
                "watchEndpointMusicSupportedConfigs": {
                    "watchEndpointMusicConfig": { "musicVideoType": t } } } })
        };
        let overlay = |t: &str| {
            json!({ "musicItemThumbnailOverlayRenderer": { "content": {
                "musicPlayButtonRenderer": { "playNavigationEndpoint": cfg(t) } } } })
        };

        // Queue-panel row (`/next`): no overlay, the tag sits on the row's own endpoint.
        assert!(is_video_row(
            &json!({ "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_OMV") })
        ));
        assert!(is_video_row(
            &json!({ "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_UGC") })
        ));
        assert!(!is_video_row(
            &json!({ "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_ATV") })
        ));

        // List row: `overlay` wins over the row endpoint. Card: same, under `thumbnailOverlay`.
        assert!(is_video_row(&json!({
            "overlay": overlay("MUSIC_VIDEO_TYPE_OMV"),
            "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_ATV"),
        })));
        assert!(!is_video_row(&json!({
            "thumbnailOverlay": overlay("MUSIC_VIDEO_TYPE_ATV"),
            "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_OMV"),
        })));

        // Fail open: no tag (or an overlay carrying none) means audio, never hide.
        assert!(!is_video_row(
            &json!({ "navigationEndpoint": { "watchEndpoint": { "videoId": "v" } } })
        ));
        assert!(!is_video_row(&json!({})));

        // A personal upload is audio, so it must not read as a music video. Issue #71: with
        // "hide music videos" on, treating it as one dropped every upload out of the library.
        let up = "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK";
        assert!(!is_video_row(&json!({ "navigationEndpoint": cfg(up) })));
        assert!(!is_video_row(&json!({ "overlay": overlay(up) })));
    }

    // The upload flag picks the orchestrator's login-only client chain (issue #71), so it has to
    // come off the same lookup `is_video_row` uses: overlay first, row endpoint as the fallback.
    #[test]
    fn upload_rows_are_recognised_in_every_renderer_shape() {
        let cfg = |t: &str| {
            json!({ "watchEndpoint": { "videoId": "v",
                "watchEndpointMusicSupportedConfigs": {
                    "watchEndpointMusicConfig": { "musicVideoType": t } } } })
        };
        let overlay = |t: &str| {
            json!({ "musicItemThumbnailOverlayRenderer": { "content": {
                "musicPlayButtonRenderer": { "playNavigationEndpoint": cfg(t) } } } })
        };
        let up = "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK";

        assert!(is_upload_row(&json!({ "navigationEndpoint": cfg(up) })));
        assert!(is_upload_row(&json!({ "overlay": overlay(up) })));
        assert!(is_upload_row(&json!({ "thumbnailOverlay": overlay(up) })));
        // The overlay wins over the row endpoint, the same way round as for videos.
        assert!(is_upload_row(&json!({
            "overlay": overlay(up),
            "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_ATV"),
        })));
        assert!(!is_upload_row(&json!({
            "overlay": overlay("MUSIC_VIDEO_TYPE_ATV"),
            "navigationEndpoint": cfg(up),
        })));

        // Fail closed: no tag means an ordinary track, so the normal client chain still runs.
        assert!(!is_upload_row(
            &json!({ "navigationEndpoint": cfg("MUSIC_VIDEO_TYPE_OMV") })
        ));
        assert!(!is_upload_row(&json!({})));
    }

    // The library toggle is opaque tokens and no state field: which one is live is told by the
    // icons alone, so a mixed-up pair would send "add" for a song already in the library (a no-op
    // the UI would report as success). Both orientations, the one-way shape, and the two feedback
    // toggles that sit right next to it in a real track menu and must not be mistaken for it.
    #[test]
    fn reads_the_library_action_in_every_shape() {
        let toggle = |default_icon: &str, toggled_icon: &str| {
            json!({ "menu": { "menuRenderer": { "items": [
                { "toggleMenuServiceItemRenderer": {
                    "defaultIcon": { "iconType": default_icon },
                    "defaultServiceEndpoint": { "feedbackEndpoint": { "feedbackToken": "TOK_DEFAULT" } },
                    "toggledIcon": { "iconType": toggled_icon },
                    "toggledServiceEndpoint": { "feedbackEndpoint": { "feedbackToken": "TOK_TOGGLED" } }
                } }
            ] } } })
        };

        // Not in the library: the empty bookmark is the action on offer, so its token is default.
        let out = library_toggle(&toggle("BOOKMARK_BORDER", "BOOKMARK")).unwrap();
        assert!(!out.in_library);
        assert_eq!(out.add_token.as_deref(), Some("TOK_DEFAULT"));
        assert_eq!(out.remove_token.as_deref(), Some("TOK_TOGGLED"));

        // Already in it: the same renderer arrives the other way round.
        let out = library_toggle(&toggle("BOOKMARK", "BOOKMARK_BORDER")).unwrap();
        assert!(out.in_library);
        assert_eq!(out.add_token.as_deref(), Some("TOK_TOGGLED"));
        assert_eq!(out.remove_token.as_deref(), Some("TOK_DEFAULT"));

        // The older spelling, still accepted.
        assert!(
            !library_toggle(&toggle("LIBRARY_ADD", "LIBRARY_REMOVE"))
                .unwrap()
                .in_library
        );

        // One direction only, as a plain menu item: still worth offering, with no way back until
        // the row is fetched again.
        let one_way = json!({ "menu": { "menuRenderer": { "items": [
            { "menuServiceItemRenderer": {
                "icon": { "iconType": "BOOKMARK_BORDER" },
                "serviceEndpoint": { "feedbackEndpoint": { "feedbackToken": "TOK_ONE_WAY" } }
            } }
        ] } } });
        let out = library_toggle(&one_way).unwrap();
        assert!(!out.in_library);
        assert_eq!(out.add_token.as_deref(), Some("TOK_ONE_WAY"));
        assert_eq!(out.remove_token, None);

        // Its neighbours in a real menu: "Add to liked songs" is a rating on a different list, and
        // "Pin to Listen again" is a feedback toggle too. Neither is this.
        assert_eq!(library_toggle(&toggle("FAVORITE", "UNFAVORITE")), None);
        assert_eq!(library_toggle(&toggle("KEEP", "KEEP_OFF")), None);
        assert_eq!(library_toggle(&json!({})), None);
    }

    fn row_fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    // A Library ▸ Songs row: every song there is in the library, and its toggle says so with
    // `isToggled`, not by swapping its icons. Reading the icons alone called each one "not in the
    // library" and offered "Save to library" on a song already saved.
    #[test]
    fn a_library_song_row_reads_its_library_and_pin_toggles() {
        let root = row_fixture("library_song_row_menu.json");
        let row = parse_list_item(&root["musicResponsiveListItemRenderer"]).unwrap();
        assert_eq!(
            row.library,
            Some(LibraryToggle {
                in_library: true,
                add_token: Some("FIXTURE-LIBRARY-ADD".into()),
                remove_token: Some("FIXTURE-LIBRARY-REMOVE".into()),
            })
        );
        assert_eq!(
            row.pin,
            Some(PinToggle {
                pinned: false,
                pin_token: Some("FIXTURE-PIN".into()),
                unpin_token: Some("FIXTURE-UNPIN".into()),
            })
        );
        // Nothing on this menu is a "tell me less of this".
        assert!(row.feedback.is_empty(), "{:?}", row.feedback);
        assert_eq!(row.delete_upload, None);
    }

    // `isToggled` both ways on both toggles: it is which of the two the menu shows now.
    #[test]
    fn is_toggled_says_which_way_a_toggle_stands() {
        let menu = |default_icon: &str, toggled_icon: &str, toggled: Option<bool>| {
            let mut t = json!({
                "defaultIcon": { "iconType": default_icon },
                "defaultServiceEndpoint": { "feedbackEndpoint": { "feedbackToken": "DEFAULT" } },
                "toggledIcon": { "iconType": toggled_icon },
                "toggledServiceEndpoint": { "feedbackEndpoint": { "feedbackToken": "TOGGLED" } }
            });
            if let Some(on) = toggled {
                t["isToggled"] = json!(on);
            }
            json!({ "menu": { "menuRenderer": { "items": [ { "toggleMenuServiceItemRenderer": t } ] } } })
        };
        let lib = |d, t, on| library_toggle(&menu(d, t, on)).unwrap().in_library;
        assert!(!lib("BOOKMARK_BORDER", "BOOKMARK", Some(false)));
        assert!(lib("BOOKMARK_BORDER", "BOOKMARK", Some(true)));
        // No flag: the icons alone, as search rows send it.
        assert!(!lib("BOOKMARK_BORDER", "BOOKMARK", None));
        assert!(lib("BOOKMARK", "BOOKMARK_BORDER", None));
        assert!(!lib("BOOKMARK", "BOOKMARK_BORDER", Some(true)));

        let pin = pin_toggle(&menu("KEEP", "KEEP_OFF", Some(true))).unwrap();
        assert!(pin.pinned);
        assert_eq!(pin.pin_token.as_deref(), Some("DEFAULT"));
        assert_eq!(pin.unpin_token.as_deref(), Some("TOGGLED"));
        assert!(!pin_toggle(&menu("KEEP", "KEEP_OFF", None)).unwrap().pinned);
        assert!(pin_toggle(&menu("KEEP_OFF", "KEEP", None)).unwrap().pinned);
        // The library toggle is not a pin, and the other way round.
        assert_eq!(pin_toggle(&menu("BOOKMARK_BORDER", "BOOKMARK", None)), None);
        assert_eq!(library_toggle(&menu("KEEP", "KEEP_OFF", None)), None);
        // Signed out the menu asks you to sign in instead: nothing to send, so no toggle.
        let signed_out = json!({ "menu": { "menuRenderer": { "items": [ { "toggleMenuServiceItemRenderer": {
            "defaultIcon": { "iconType": "KEEP" },
            "defaultServiceEndpoint": { "modalEndpoint": {} },
            "toggledIcon": { "iconType": "KEEP_OFF" },
            "toggledServiceEndpoint": { "feedbackEndpoint": { "feedbackToken": "T" } }
        } } ] } } });
        assert_eq!(pin_toggle(&signed_out), None);
    }

    // An upload's row carries the id that deletes it, inside its Delete item's confirm dialog
    // (ytmusicapi parse_uploaded_items). Other rows carry none.
    #[test]
    fn an_upload_row_carries_the_id_that_deletes_it() {
        let root = row_fixture("upload_song_row_menu.json");
        let row = parse_list_item(&root["musicResponsiveListItemRenderer"]).unwrap();
        assert!(row.is_upload);
        assert_eq!(row.delete_upload.as_deref(), Some("t_po_FIXTUREENTITY"));
        let liked = row_fixture("library_song_row_menu.json");
        let row = parse_list_item(&liked["musicResponsiveListItemRenderer"]).unwrap();
        assert_eq!(row.delete_upload, None);
    }

    // "Not interested" and its kind: a feedback item named by its icon, with YouTube's own words.
    // No live row has shown one yet, so the shape is the one YouTube uses on its other surfaces
    // (a menuServiceItemRenderer with a feedbackEndpoint) and is read by icon alone. Items with
    // their own place elsewhere (history, library, pins) and items that send no feedback stay out.
    #[test]
    fn tell_me_less_items_are_read_by_icon_with_their_own_words() {
        let item = |icon: &str, text: &str, endpoint: Value| {
            json!({ "menuServiceItemRenderer": {
                "text": { "runs": [ { "text": text } ] },
                "icon": { "iconType": icon },
                "serviceEndpoint": endpoint
            } })
        };
        let fb = |t: &str| json!({ "feedbackEndpoint": { "feedbackToken": t } });
        let node = json!({ "menu": { "menuRenderer": { "items": [
            item("NOT_INTERESTED", "Not interested", fb("NI")),
            item("REMOVE", "Don't recommend this artist", fb("DR")),
            item("REMOVE_FROM_HISTORY", "Remove from history", fb("H")),
            item("BOOKMARK_BORDER", "Save to library", fb("L")),
            item("DISMISS", "Not interested", json!({ "queueAddEndpoint": {} })),
        ] } } });
        assert_eq!(
            menu_feedback(&node),
            vec![
                MenuFeedback {
                    icon: "NOT_INTERESTED".into(),
                    label: "Not interested".into(),
                    token: "NI".into(),
                },
                MenuFeedback {
                    icon: "REMOVE".into(),
                    label: "Don't recommend this artist".into(),
                    token: "DR".into(),
                },
            ]
        );
        assert!(menu_feedback(&json!({})).is_empty());
    }

    // The rating drives both thumbs on every row, and the third state is new: `DISLIKE` used to
    // collapse into "not liked". An unknown value must read as unrated rather than as a dislike.
    #[test]
    fn reads_all_three_rating_states() {
        let row = |status: &str| {
            json!({ "menu": { "menuRenderer": { "topLevelButtons": [
                { "likeButtonRenderer": { "likeStatus": status } }
            ] } } })
        };
        assert_eq!(like_status(&row("LIKE")), Some(Rating::Like));
        assert_eq!(like_status(&row("DISLIKE")), Some(Rating::Dislike));
        assert_eq!(like_status(&row("INDIFFERENT")), Some(Rating::Indifferent));
        assert_eq!(
            like_status(&row("SOMETHING_NEW")),
            Some(Rating::Indifferent)
        );
        assert_eq!(like_status(&json!({})), None);
    }

    // A dead `RDAMVM` radio answers with the seed song plus this marker, which names the mix the
    // song really belongs to. It's the escalation the "start radio did nothing" case runs on.
    #[test]
    fn parses_the_automix_the_panel_continues_into() {
        let root = json!({
            "contents": { "playlistPanelRenderer": { "contents": [
                { "playlistPanelVideoRenderer": {
                    "videoId": "seed1",
                    "title": { "runs": [{ "text": "Only Song" }] },
                    "longBylineText": { "runs": [{ "text": "An Artist" }] },
                    "lengthText": { "runs": [{ "text": "3:00" }] },
                    "thumbnail": { "thumbnails": [{ "url": "https://t/1" }] }
                } },
                { "automixPreviewVideoRenderer": { "content": { "automixPlaylistVideoRenderer": {
                    "navigationEndpoint": { "watchPlaylistEndpoint": { "playlistId": "RDAMVMseed1" } }
                } } } }
            ] } }
        });
        let out = parse_next(&root);
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.automix_playlist_id.as_deref(), Some("RDAMVMseed1"));
    }

    // Signed in, YouTube pairs a radio track's audio row with its music-video twin under one
    // wrapper. Only the primary arm is the queue row; taking the counterpart too put every song
    // in the radio twice, with two different durations. Issue #170.
    #[test]
    fn a_wrapped_row_contributes_only_its_primary() {
        let renderer = |id: &str, len: &str| {
            json!({ "playlistPanelVideoRenderer": {
                "videoId": id,
                "title": { "runs": [{ "text": "Sunflower" }] },
                "longBylineText": { "runs": [{ "text": "Post Malone" }] },
                "lengthText": { "runs": [{ "text": len }] },
                "thumbnail": { "thumbnails": [{ "url": "https://t/1" }] }
            } })
        };
        let root = json!({
            "contents": { "playlistPanelRenderer": { "contents": [
                { "playlistPanelVideoWrapperRenderer": {
                    "primaryRenderer": renderer("audio1", "2:39"),
                    "counterpart": [{ "counterpartRenderer": renderer("video1", "2:42") }]
                } },
                renderer("plain1", "3:00")
            ] } }
        });
        let out = parse_next(&root);
        let ids: Vec<&str> = out.items.iter().map(|i| i.video_id.as_str()).collect();
        assert_eq!(ids, ["audio1", "plain1"]);
        // The twin is kept on its primary for the Song/Video switch (#442), and only there.
        assert_eq!(out.items[0].counterpart.as_deref(), Some("video1"));
        assert_eq!(out.items[1].counterpart, None);
    }

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/next")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json")
    }

    // A radio's first answer is the whole session YouTube's own player keeps: the rows in their
    // order (repeats and all), the radio's own id and name, the page after this one, the tuning
    // chips, and the Related tab. Venuta plays exactly that, so every part has to come through.
    #[test]
    fn a_radio_answer_reads_as_youtube_sent_it() {
        let out = parse_next(&fixture("radio_first.json"));
        let ids: Vec<&str> = out.items.iter().map(|i| i.video_id.as_str()).collect();
        // One row per song (the wrapper's twin rides on its primary), and the repeat stays.
        assert_eq!(
            ids,
            [
                "seedVid0001",
                "radioVid0002",
                "radioVid0003",
                "radioVid0002"
            ]
        );
        assert_eq!(out.items[0].counterpart.as_deref(), Some("seedVideoOMV"));
        assert_eq!(out.playlist_id.as_deref(), Some("RDAMVMseedVid0001"));
        assert_eq!(out.title.as_deref(), Some("Kitni Baatein Radio"));
        assert_eq!(out.continuation.as_deref(), Some("CBQSRadioPage2Token"));
        assert_eq!(out.related_browse_id.as_deref(), Some("MPTRt_seedRelated"));
        assert_eq!(out.lyrics_browse_id.as_deref(), Some("MPLYt_seedLyrics"));
        assert_eq!(out.automix, None);

        let chips: Vec<(&str, bool)> = out
            .chips
            .iter()
            .map(|c| (c.text.as_str(), c.selected))
            .collect();
        assert_eq!(
            chips,
            [("All", true), ("Familiar", false), ("Deep cuts", false)]
        );
        // Each chip's endpoint as YouTube put it on the chip, params and all.
        assert_eq!(
            out.chips[0].endpoint,
            WatchEndpoint {
                playlist_id: Some("RDAMVMseedVid0001".into()),
                params: Some("wAEB8gECGAE%3D".into()),
                index: Some(0),
                ..Default::default()
            }
        );
        assert_eq!(
            out.chips[1].endpoint,
            WatchEndpoint {
                video_id: Some("seedVid0001".into()),
                playlist_id: Some("RDATiYvseedVid0001".into()),
                params: Some("wAEB8gECKAE%3D".into()),
                ..Default::default()
            }
        );
        assert_eq!(
            out.chips[2].endpoint.playlist_id.as_deref(),
            Some("RDATpXvseedVid0001")
        );
    }

    // The next page of the same radio comes back under `continuationContents`, with no header and
    // no chips: its rows (a song from earlier coming round again included) and the page after it.
    #[test]
    fn a_radio_continuation_page_reads_its_rows_and_the_next_token() {
        let out = parse_next(&fixture("radio_continuation.json"));
        let ids: Vec<&str> = out.items.iter().map(|i| i.video_id.as_str()).collect();
        assert_eq!(ids, ["radioVid0004", "seedVid0001", "radioVid0005"]);
        assert_eq!(out.continuation.as_deref(), Some("CBQSRadioPage3Token"));
        assert_eq!(out.playlist_id.as_deref(), Some("RDAMVMseedVid0001"));
        assert!(out.chips.is_empty());
        assert_eq!(out.title, None);
    }

    // A list (an album, a playlist, one song) has no continuation of its own: it ends with the
    // marker naming what YouTube's Autoplay goes on with, an endpoint with its own params.
    #[test]
    fn a_list_answer_names_what_it_continues_with() {
        let out = parse_next(&fixture("song_automix.json"));
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.continuation, None);
        assert_eq!(
            out.automix,
            Some(WatchEndpoint {
                playlist_id: Some("RDAMPLOLAK5uy_fixtureAlbum".into()),
                params: Some("OAHyAQIIAQ%3D%3D".into()),
                ..Default::default()
            })
        );
        assert_eq!(
            out.automix_playlist_id.as_deref(),
            Some("RDAMPLOLAK5uy_fixtureAlbum")
        );
        assert_eq!(out.playlist_id.as_deref(), Some("OLAK5uy_fixtureAlbum"));
        assert_eq!(out.title.as_deref(), Some("Fixture Album"));
        assert_eq!(out.related_browse_id.as_deref(), Some("MPTRt_lastRelated"));
    }

    // Only the queue's own continuation: a token anywhere else in the answer (comments, a tab)
    // is not the next page of the radio.
    #[test]
    fn the_continuation_is_the_queue_panels_own() {
        let root = json!({
            "engagementPanels": [{ "continuations": [{ "nextContinuationData": { "continuation": "NOT_THE_QUEUE" } }] }],
            "contents": { "playlistPanelRenderer": {
                "contents": [],
                "continuations": [{ "nextContinuationData": { "continuation": "QUEUE_NEXT" } }]
            } }
        });
        assert_eq!(
            parse_next(&root).continuation.as_deref(),
            Some("QUEUE_NEXT")
        );
        let none = json!({
            "engagementPanels": [{ "continuations": [{ "nextContinuationData": { "continuation": "NOT_THE_QUEUE" } }] }],
            "contents": { "playlistPanelRenderer": { "contents": [] } }
        });
        assert_eq!(parse_next(&none).continuation, None);
    }

    // The Related tab's songs ("You might also like"): the fallback when a radio has nothing more.
    #[test]
    fn related_songs_are_the_list_rows_of_the_related_page() {
        let songs = parse_related_songs(&fixture("related.json"));
        let ids: Vec<&str> = songs.iter().map(|s| s.video_id.as_str()).collect();
        assert_eq!(ids, ["relatedVid01", "relatedVid02"]);
        assert_eq!(songs[0].title, "Related One");
        assert_eq!(songs[0].artists, "Artist One");
        assert_eq!(songs[0].duration.as_deref(), Some("3:33"));
    }

    // A real radio page has no automix marker — nothing to escalate to, and nothing to mistake a
    // regular playlist id for.
    #[test]
    fn no_automix_on_a_live_radio_page() {
        let root = json!({
            "contents": { "playlistPanelRenderer": {
                "playlistId": "RDAMVMseed1",
                "contents": []
            } }
        });
        assert_eq!(parse_next(&root).automix_playlist_id, None);
    }

    #[test]
    fn parses_search_item() {
        let root = json!({
            "a": { "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": "abc123" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song Title" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                        { "text": "The Artist", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCartist1" } } },
                        { "text": " & " },
                        { "text": "Guest", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCartist2" } } },
                        { "text": " • " },
                        { "text": "The Album", "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREalbum1" } } },
                        { "text": " • " }, { "text": "3:21" }
                    ] } } }
                ],
                "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                    { "url": "small.jpg" }, { "url": "big.jpg" }
                ] } } }
            }}
        });
        let r = parse_search(&root);
        assert_eq!(r.items.len(), 1);
        let s = &r.items[0];
        assert_eq!(s.video_id, "abc123");
        assert_eq!(s.title, "Song Title");
        assert_eq!(s.artists, "The Artist & Guest");
        assert_eq!(s.artist_id.as_deref(), Some("UCartist1"));
        // Each artist keeps its own link; the run list stops at the first "•" (album/duration).
        assert_eq!(
            s.artist_runs
                .iter()
                .map(|r| (r.text.as_str(), r.id.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("The Artist", Some("UCartist1")),
                (" & ", None),
                ("Guest", Some("UCartist2"))
            ]
        );
        assert_eq!(s.album.as_deref(), Some("The Album"));
        assert_eq!(s.album_id.as_deref(), Some("MPREalbum1"));
        assert_eq!(s.duration.as_deref(), Some("3:21"));
        assert_eq!(s.thumbnail.as_deref(), Some("big.jpg"));
    }

    // Album rows carry "53M plays" in the third flex column; playlist rows put the album name
    // there. Confusing the two would print an album title where the play count goes.
    #[test]
    fn play_count_comes_only_from_a_plays_column() {
        let row = |third: Value| {
            json!({ "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": "abc123" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song Title" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "The Artist" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": third }] } } }
                ]
            }})
        };
        let plays = |third: Value| parse_list_item(&row(third)["musicResponsiveListItemRenderer"]);
        assert_eq!(
            plays(json!("53M plays")).unwrap().play_count.as_deref(),
            Some("53M")
        );
        assert_eq!(
            plays(json!("1,234 plays")).unwrap().play_count.as_deref(),
            Some("1,234")
        );
        assert_eq!(plays(json!("The Album")).unwrap().play_count, None);
        assert_eq!(plays(json!("")).unwrap().play_count, None);
        // The other half of the same discriminator: what isn't a play count is the album, which is
        // the only place a playlist row carries one (its subtitle is the artist alone). Without it
        // every playlist track parses album-less and grouping a playlist by album does nothing.
        assert_eq!(
            plays(json!("The Album")).unwrap().album.as_deref(),
            Some("The Album")
        );
        assert_eq!(plays(json!("53M plays")).unwrap().album, None);
        assert_eq!(plays(json!("")).unwrap().album, None);
        // A bare length in the third column (no fixed column on these rows) starts with a digit
        // and links nothing — every discriminator a count passes. A count never carries a colon.
        assert_eq!(plays(json!("3:42")).unwrap().play_count, None);
        assert_eq!(plays(json!("1:04:11")).unwrap().play_count, None);
        // The other side of the same fix: what the count gave up, the duration claims, so the row
        // still shows its length instead of going blank there.
        assert_eq!(
            plays(json!("3:42")).unwrap().duration.as_deref(),
            Some("3:42")
        );
        assert_eq!(
            plays(json!("1:04:11")).unwrap().duration.as_deref(),
            Some("1:04:11")
        );
        // An album name in that slot is neither.
        assert_eq!(plays(json!("The Album")).unwrap().duration, None);
        // And a duration is not the album either, which `album_column` would otherwise take once
        // `play_count` stopped claiming the string for itself.
        assert_eq!(plays(json!("3:42")).unwrap().album, None);
        assert_eq!(plays(json!("1:04:11")).unwrap().album, None);
    }

    // #274: `hl` follows the UI language, so the word "plays" is gone in ten of the eleven
    // locales. The column that opens with a digit and links nothing is the count; the one that
    // links its album page is the album, whatever either of them says.
    #[test]
    fn a_localized_play_count_is_still_a_play_count() {
        let row = |third: Value, link: bool| {
            let mut run = json!({ "text": third });
            if link {
                run["navigationEndpoint"] =
                    json!({ "browseEndpoint": { "browseId": "MPREalbum1" } });
            }
            json!({
                "playlistItemData": { "videoId": "abc123" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song Title" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "The Artist" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [run] } } }
                ]
            })
        };
        let ko = parse_list_item(&row(json!("9845만회 재생"), false)).unwrap();
        assert_eq!(ko.play_count.as_deref(), Some("9845만회"));
        assert_eq!(ko.album, None);
        // No space to cut at: the whole string is the count rather than nothing at all.
        let ja = parse_list_item(&row(json!("9845万回再生"), false)).unwrap();
        assert_eq!(ja.play_count.as_deref(), Some("9845万回再生"));
        // A linked column is the album even when the album is named after a year.
        let album = parse_list_item(&row(json!("1989"), true)).unwrap();
        assert_eq!(album.play_count, None);
        assert_eq!(album.album.as_deref(), Some("1989"));
    }

    // #377: an artist page's top songs read "Title | Artist | 1.2B plays | Album". The count takes
    // the third column, so the album is the fourth, and without it the whole Top songs queue
    // played and scrobbled album-less.
    #[test]
    fn artist_top_song_album_is_the_fourth_column() {
        let row = json!({
            "playlistItemData": { "videoId": "abc123" },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song Title" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "The Artist" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "1.2B plays" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{
                    "text": "The Album",
                    "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREalbum1" } }
                }] } } }
            ]
        });
        let song = parse_list_item(&row).unwrap();
        assert_eq!(song.play_count.as_deref(), Some("1.2B"));
        assert_eq!(song.album.as_deref(), Some("The Album"));
        assert_eq!(song.album_id.as_deref(), Some("MPREalbum1"));
    }

    // An artist page's Top songs row reads "Title | Artist | plays | Album", and the album in the
    // fourth column links its own page. `album_column` reads the third one on a guess — anything
    // that is not a length and not a count — and that guess only holds while the count opens with
    // a digit. It does not: `hl` follows the UI language (#274), and Portuguese leads with the
    // verb ("Tocou 93 mi vezes"), so `play_count` declines it, `album_column` swallows it, and the
    // count ships as the album to Discord, MPRIS and the scrobbler while the real album sits in
    // the next column, linked and unread. Reading the link before the guess makes the locale stop
    // mattering. Lifted from a live artist page, hl=pt.
    #[test]
    fn a_linked_album_beats_a_localized_play_count() {
        let row = |second: &str| {
            json!({
                "playlistItemData": { "videoId": "abc123" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "The Summoning" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                        { "text": "Sleep Token",
                          "navigationEndpoint": { "browseEndpoint": { "browseId": "UCsleeptoken" } } }
                    ] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": second }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{
                        "text": "Take Me Back To Eden",
                        "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREb_VVttKm9imi6" } }
                    }] } } }
                ]
            })
        };
        // Whatever the locale orders the count in, the linked album is the one that wins.
        for count in ["Tocou 93\u{a0}mi vezes", "93M plays", "再生回数 9385万 回"] {
            let song = parse_list_item(&row(count)).unwrap();
            assert_eq!(
                song.album.as_deref(),
                Some("Take Me Back To Eden"),
                "count {count:?} won"
            );
            assert_eq!(song.album_id.as_deref(), Some("MPREb_VVttKm9imi6"));
        }
    }

    // Number-like album names ("1989", "24 Songs") still read as albums.
    // They come from the subtitle text, so column order does not change them.
    #[test]
    fn an_album_named_after_a_number_is_still_an_album() {
        let row = |second: &str| {
            json!({ "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": "abc123" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song Title" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                        { "text": "The Artist" },
                        { "text": " • " },
                        { "text": second },
                        { "text": " • " },
                        { "text": "3:41" }
                    ] } } }
                ]
            }})
        };
        for album in ["1989 (Deluxe)", "24 Songs", "5SOS5", "2nd Collection"] {
            let row = row(album);
            assert_eq!(
                parse_list_item(&row["musicResponsiveListItemRenderer"])
                    .unwrap()
                    .album
                    .as_deref(),
                Some(album),
                "{album:?} is a release, not a count"
            );
        }
    }

    // A music video row has no third column to put an album in, so the count in its subtitle lands in
    // `album` and rides out to Discord, MPRIS and the scrobble as if it named a release. The shape
    // is lifted verbatim from a live `FILTER_VIDEO` row.
    #[test]
    fn a_video_row_view_count_is_not_the_album() {
        let row = json!({
            "playlistItemData": { "videoId": "4iSvoQNfrrk" },
            "overlay": { "musicItemThumbnailOverlayRenderer": { "content": {
                "musicPlayButtonRenderer": { "playNavigationEndpoint": { "watchEndpoint": {
                    "videoId": "4iSvoQNfrrk",
                    "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": {
                        "musicVideoType": "MUSIC_VIDEO_TYPE_OMV"
                    } }
                } } }
            } } },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Caramel" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Sleep Token",
                      "navigationEndpoint": { "browseEndpoint": { "browseId": "UCsleeptoken" } } },
                    { "text": " • " },
                    { "text": "21M views" },
                    { "text": " • " },
                    { "text": "4:51" }
                ] } } }
            ]
        });
        let song = parse_list_item(&row).unwrap();
        assert!(song.is_video);
        assert_eq!(song.artists, "Sleep Token");
        assert_eq!(song.duration.as_deref(), Some("4:51"));
        assert_eq!(song.album, None, "the view count is not a release");
        assert_eq!(
            song.play_count, None,
            "the count is in the subtitle, not a column"
        );
    }

    // #274 again, the other half: with `hl=ko` a row YouTube stripped the artist from reads
    // "노래 • 2:43", and the word list cannot see that "노래" is the type label. Two fields whose
    // second is the length have no artist field, so the first one goes.
    #[test]
    fn a_localized_type_label_is_not_an_artist() {
        let row = json!({
            "playlistItemData": { "videoId": "abc123" },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Himmelen brenner" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "노래" }, { "text": " \u{2022} " }, { "text": "2:43" }
                ] } } }
            ]
        });
        let s = parse_list_item(&row).unwrap();
        assert_eq!(s.artists, "");
        assert_eq!(s.duration.as_deref(), Some("2:43"));
        assert_eq!(s.album, None);
    }

    // #216: search for an artist by name and YouTube drops the artist from the song rows it
    // returns, leaving "Song • 3:55". The length is not a name; taking it as one put a time in the
    // artist field of the row, the card, the player bar and the scrobble.
    #[test]
    fn a_duration_only_subtitle_has_no_artist() {
        let row = json!({
            "playlistItemData": { "videoId": "abc123" },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Not Strong Enough" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Song" }, { "text": " \u{2022} " }, { "text": "3:55" }
                ] } } }
            ]
        });
        let s = parse_list_item(&row).unwrap();
        assert_eq!(s.artists, "");
        assert!(s.artist_runs.is_empty());
        assert_eq!(s.duration.as_deref(), Some("3:55"));
        assert_eq!(s.album, None);
    }

    // An artist or album with a colon in its name ("Cast of EPIC: The Musical") used to read as the
    // duration, so the row printed that whole string where the time goes and lost the real length.
    #[test]
    fn a_colon_in_a_name_is_not_a_duration() {
        let row = json!({
            "playlistItemData": { "videoId": "abc123" },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Monster" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Jorge Rivera-Herrans & Cast of EPIC: The Musical",
                      "navigationEndpoint": { "browseEndpoint": { "browseId": "UCjorge" } } },
                    { "text": " • " }, { "text": "EPIC: The Musical" }
                ] } } }
            ],
            "fixedColumns": [
                { "musicResponsiveListItemFixedColumnRenderer": { "text": { "runs": [{ "text": "4:32" }] } } }
            ]
        });
        let s = parse_list_item(&row).unwrap();
        assert_eq!(
            s.artists,
            "Jorge Rivera-Herrans & Cast of EPIC: The Musical"
        );
        assert_eq!(s.album.as_deref(), Some("EPIC: The Musical"));
        assert_eq!(s.duration.as_deref(), Some("4:32"));
    }

    #[test]
    fn parses_next_panel_video() {
        let root = json!({
            "contents": { "playlistPanelRenderer": { "contents": [
                { "playlistPanelVideoRenderer": {
                    "videoId": "vid9",
                    "title": { "runs": [{ "text": "Next Song" }] },
                    "longBylineText": { "runs": [{ "text": "Artist A" }, { "text": " & " }, { "text": "Artist B" }] },
                    "lengthText": { "runs": [{ "text": "4:05" }] },
                    "thumbnail": { "thumbnails": [{ "url": "t.jpg" }] }
                }}
            ], "continuations": [{ "nextContinuationData": { "continuation": "CONT_TOKEN" } }] } },
            "tabs": [{ "tabRenderer": { "title": "Lyrics", "endpoint": { "browseEndpoint": {
                "browseId": "MPLYt_abc123",
                "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": {
                    "pageType": "MUSIC_PAGE_TYPE_TRACK_LYRICS" } }
            } } } }]
        });
        let r = parse_next(&root);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].video_id, "vid9");
        assert_eq!(r.items[0].title, "Next Song");
        assert_eq!(r.items[0].artists, "Artist A & Artist B");
        assert_eq!(r.items[0].duration.as_deref(), Some("4:05"));
        assert_eq!(r.continuation.as_deref(), Some("CONT_TOKEN"));
        assert_eq!(r.lyrics_browse_id.as_deref(), Some("MPLYt_abc123"));
        // No overlay in this response, and the panel row carries no `likeStatus` either: the
        // rating has to read as unknown, not as "not liked".
        assert_eq!(r.rating, None);
    }

    /// The one place a `next` response states the requested video's rating (issue #93). It has to
    /// come from the player overlay, not from a panel row, and not from the *seed* row's absence.
    #[test]
    fn next_reads_the_requested_videos_rating_from_the_player_overlay() {
        let root = json!({
            "contents": { "playlistPanelRenderer": { "contents": [
                { "playlistPanelVideoRenderer": {
                    "videoId": "vid9",
                    "title": { "runs": [{ "text": "Next Song" }] }
                }}
            ] } },
            "playerOverlays": { "playerOverlayRenderer": { "actions": [
                { "likeButtonRenderer": { "likeStatus": "LIKE" } }
            ] } }
        });
        let r = parse_next(&root);
        assert_eq!(r.rating, Some(Rating::Like));
        // The panel row keeps its own (absent) rating: the overlay speaks for the seed alone.
        assert_eq!(r.items[0].rating, None);
    }

    /// An unfiltered search row leads with the result type ("Song • Delara • 3:02"). It must not
    /// end up in `artists` — that string is what gets scrobbled.
    #[test]
    fn drops_the_result_type_from_a_search_row() {
        let root = json!({
            "a": { "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": "abc123" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Hele uka" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                        { "text": "Song" }, { "text": " • " },
                        { "text": "Delara", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCdelara" } } },
                        { "text": " • " }, { "text": "3:02" }
                    ] } } }
                ]
            }}
        });
        let s = &parse_search(&root).items[0];
        assert_eq!(s.artists, "Delara");
        assert_eq!(s.album, None);
        assert_eq!(s.duration.as_deref(), Some("3:02"));
        // The links describe the same field as `artists` — never the "Song" label in front of it.
        assert_eq!(
            s.artist_runs
                .iter()
                .map(|r| (r.text.as_str(), r.id.as_deref()))
                .collect::<Vec<_>>(),
            [("Delara", Some("UCdelara"))]
        );
    }

    /// A queue row's byline is a whole descriptor; only its artist field is the artist.
    #[test]
    fn panel_byline_keeps_only_the_artist() {
        let root = json!({
            "contents": { "playlistPanelRenderer": { "contents": [
                { "playlistPanelVideoRenderer": {
                    "videoId": "vid9",
                    "title": { "runs": [{ "text": "Hele uka" }] },
                    "longBylineText": { "runs": [
                        { "text": "Delara", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCdelara" } } },
                        { "text": " • " }, { "text": "Sjelen" }, { "text": " • " }, { "text": "2026" }
                    ] }
                }}
            ] } }
        });
        assert_eq!(parse_next(&root).items[0].artists, "Delara");
    }

    /// Radio and autoplay rows scrobbled album-less because the panel parser never read the
    /// byline's album run (issue #309). A music video's byline links no album and must stay empty.
    #[test]
    fn panel_album_is_the_linked_byline_run() {
        let row = |id: &str, byline: Value| {
            json!({ "playlistPanelVideoRenderer": {
                "videoId": id,
                "title": { "runs": [{ "text": "T" }] },
                "longBylineText": { "runs": byline }
            }})
        };
        let artist = json!({ "text": "Delara", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCdelara" } } });
        let song = row(
            "song",
            json!([
                artist, { "text": " • " },
                { "text": "Sjelen", "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREb_sjelen" } } },
                { "text": " • " }, { "text": "2026" }
            ]),
        );
        let video = row(
            "video",
            json!([artist, { "text": " • " }, { "text": "50M views" }]),
        );
        let root =
            json!({ "contents": { "playlistPanelRenderer": { "contents": [song, video] } } });
        let items = parse_next(&root).items;
        assert_eq!(items[0].album.as_deref(), Some("Sjelen"));
        assert_eq!(items[1].album, None);
    }

    #[test]
    fn splits_datasync_id() {
        assert_eq!(split_datasync_id("realid||other"), "realid");
        assert_eq!(split_datasync_id("||fallback"), "fallback");
        assert_eq!(split_datasync_id("plain"), "plain");
    }

    #[test]
    fn parses_account_menu() {
        let root: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/account_menu_single.json"
        ))
        .unwrap();
        let a = parse_account_menu(&root);
        assert_eq!(a.name.as_deref(), Some("Personal channel"));
        assert_eq!(a.handle.as_deref(), Some("@personal"));
        assert_eq!(a.email.as_deref(), Some("listener@example.invalid"));
        assert_eq!(
            a.thumbnail.as_deref(),
            Some("https://example.invalid/avatar-large.jpg")
        );
        assert_eq!(a.channel_id.as_deref(), Some("UCSANITIZEDPERSONAL"));
        assert_eq!(a.data_sync_id.as_deref(), Some("personal-sync"));
        assert_eq!(a.visitor_data.as_deref(), Some("CgtSANITIZEDVISITOR"));
    }

    #[test]
    fn parses_one_selectable_identity() {
        let root: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/accounts_list_one.json"))
                .unwrap();
        let identities = parse_account_identities(&root);
        assert_eq!(identities.len(), 1);
        let identity = &identities[0];
        assert_eq!(identity.name, "Personal channel");
        assert_eq!(identity.handle.as_deref(), Some("@personal"));
        assert_eq!(identity.email.as_deref(), Some("listener@example.invalid"));
        assert_eq!(identity.channel_id.as_deref(), Some("UCSANITIZEDPERSONAL"));
        assert_eq!(identity.data_sync_id, "personal-sync");
        assert!(identity.is_selected);
    }

    #[test]
    fn parses_multiple_identities_and_keeps_a_persisted_non_default_choice() {
        let root: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/accounts_list_multiple.json"
        ))
        .unwrap();
        let identities = parse_account_identities(&root);
        assert_eq!(identities.len(), 2);
        assert!(identities[0].is_selected);

        // The app restores by server-issued id, not YouTube's current/default marker.
        let persisted = identities
            .iter()
            .find(|i| i.data_sync_id == "brand-page-id")
            .unwrap();
        assert_eq!(persisted.name, "Brand channel");
        assert_eq!(persisted.channel_id.as_deref(), Some("UCSANITIZEDBRAND"));
        assert!(!persisted.is_selected);
    }

    #[test]
    fn missing_identity_metadata_stays_optional() {
        let root: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/accounts_list_missing_optional.json"
        ))
        .unwrap();
        let identities = parse_account_identities(&root);
        assert_eq!(identities.len(), 1);
        let identity = &identities[0];
        assert_eq!(identity.name, "No optional metadata");
        assert_eq!(identity.handle, None);
        assert_eq!(identity.email, None);
        assert_eq!(identity.thumbnail, None);
        assert_eq!(identity.channel_id, None);
    }

    #[test]
    fn malformed_or_unusable_identity_tokens_are_not_selectable() {
        let root: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/accounts_list_malformed_ids.json"
        ))
        .unwrap();
        let identities = parse_account_identities(&root);
        assert_eq!(identities.len(), 2);
        assert_eq!(identities[0].name, "Missing token");
        assert_eq!(identities[0].data_sync_id, "active-response-sync");
        assert_eq!(identities[1].name, "Fallback token");
        assert_eq!(identities[1].data_sync_id, "fallback-sync");
    }

    // Three keys, one badge shape, and a badge array that can hold other badges alongside it.
    // Getting the key wrong reads false everywhere and the flag silently never shows.
    #[test]
    fn explicit_badge_is_read_from_every_key_it_arrives_under() {
        let badge = json!([{ "musicInlineBadgeRenderer": {
            "icon": { "iconType": "MUSIC_EXPLICIT_BADGE" },
            "accessibilityData": { "accessibilityData": { "label": "Explicit" } }
        } }]);
        // List row (search / playlist / album tracks), two-row card, playlist+album header.
        assert!(is_explicit(&json!({ "badges": badge })));
        assert!(is_explicit(&json!({ "subtitleBadges": badge })));
        assert!(is_explicit(&json!({ "subtitleBadge": badge })));
        // Alongside another badge, and with no badge at all.
        assert!(is_explicit(&json!({ "badges": [
            { "musicInlineBadgeRenderer": { "icon": { "iconType": "MUSIC_NEW_RELEASE_BADGE" } } },
            badge[0]
        ] })));
        assert!(!is_explicit(&json!({ "badges": [
            { "musicInlineBadgeRenderer": { "icon": { "iconType": "MUSIC_NEW_RELEASE_BADGE" } } }
        ] })));
        assert!(!is_explicit(&json!({})));
        // Never from a nested row: an album card inside a shelf must not badge the shelf's row.
        assert!(!is_explicit(
            &json!({ "contents": [{ "subtitleBadges": badge }] })
        ));
    }

    #[test]
    fn absent_or_empty_response_datasync_id_is_none() {
        assert_eq!(parse_account_menu(&json!({})).data_sync_id, None);
        assert_eq!(
            parse_account_menu(&json!({
                "responseContext": { "mainAppWebResponseContext": { "datasyncId": "||" } }
            }))
            .data_sync_id,
            None
        );
    }

    /// A row on a playlist you can't edit still carries a playlistSetVideoId, so the remove action
    /// in its own menu is what decides whether the row can be removed (issue #86: a collaborator
    /// can remove the tracks they added, and only those).
    #[test]
    fn set_video_id_only_comes_from_a_row_that_may_be_removed() {
        let row = |menu: serde_json::Value| {
            json!({
                "playlistItemData": { "playlistSetVideoId": "SVID", "videoId": "vid1" },
                "flexColumns": [{ "musicResponsiveListItemFlexColumnRenderer": {
                    "text": { "runs": [{ "text": "Alpha" }] } } }],
                "menu": menu
            })
        };
        let remove = json!({ "menuRenderer": { "items": [{ "menuServiceItemRenderer": {
            "serviceEndpoint": { "playlistEditEndpoint": {
                "actions": [{ "setVideoId": "SVID", "action": "ACTION_REMOVE_VIDEO" }] } } } }] } });
        let share = json!({ "menuRenderer": { "items": [{ "menuNavigationItemRenderer": {} }] } });
        assert_eq!(
            parse_list_item(&row(remove))
                .unwrap()
                .set_video_id
                .as_deref(),
            Some("SVID")
        );
        assert_eq!(parse_list_item(&row(share)).unwrap().set_video_id, None);
    }

    /// A collaborative playlist stamps each row with whoever added it.
    #[test]
    fn a_row_carries_the_contributor_who_added_it() {
        let node = json!({
            "playlistItemData": { "videoId": "vid1" },
            "flexColumns": [{ "musicResponsiveListItemFlexColumnRenderer": {
                "text": { "runs": [{ "text": "Alpha" }] } } }],
            "contributorsAvatars": { "avatarStackViewModel": { "avatars": [{ "avatarViewModel": {
                "image": { "sources": [{ "url": "https://yt3.ggpht.com/abc=s48" }] },
                "accessibilityText": "Rajat"
            } }] } }
        });
        let item = parse_list_item(&node).unwrap();
        assert_eq!(item.added_by.as_deref(), Some("Rajat"));
        assert_eq!(item.added_by_avatars, ["https://yt3.ggpht.com/abc=s48"]);
        // A Taste Match row two members share stacks both, and the names sit on the stack: the
        // row must keep both avatars rather than vanish for lack of a per-avatar name (#102).
        let shared = json!({
            "playlistItemData": { "videoId": "vid1" },
            "flexColumns": [{ "musicResponsiveListItemFlexColumnRenderer": {
                "text": { "runs": [{ "text": "Alpha" }] } } }],
            "contributorsAvatars": { "avatarStackViewModel": {
                "avatars": [
                    { "avatarViewModel": { "image": { "sources": [{ "url": "https://yt3.ggpht.com/a=s48" }] } } },
                    { "avatarViewModel": { "image": { "sources": [{ "url": "https://yt3.ggpht.com/b=s48" }] } } }
                ],
                "rendererContext": { "accessibilityContext": { "label": "Irine and Rajat" } }
            } }
        });
        let item = parse_list_item(&shared).unwrap();
        assert_eq!(
            item.added_by_avatars,
            ["https://yt3.ggpht.com/a=s48", "https://yt3.ggpht.com/b=s48"]
        );
        assert_eq!(item.added_by.as_deref(), Some("Irine and Rajat"));
        // An ordinary playlist row has no stack, so no name to draw.
        assert_eq!(
            parse_list_item(&json!({
                "playlistItemData": { "videoId": "vid1" },
                "flexColumns": [{ "musicResponsiveListItemFlexColumnRenderer": {
                    "text": { "runs": [{ "text": "Alpha" }] } } }]
            }))
            .unwrap()
            .added_by,
            None
        );
    }

    /// The collaborative row as a real account got it (tokens and ids replaced).
    fn collab_row() -> Value {
        let root: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/collab_playlist_row.json"
        ))
        .unwrap();
        root["musicResponsiveListItemRenderer"].clone()
    }

    /// Who added the song: the name and picture off the stack, and the channel a tap on it opens.
    #[test]
    fn a_collaborative_row_names_who_added_it_and_their_channel() {
        let item = parse_list_item(&collab_row()).unwrap();
        assert_eq!(item.added_by.as_deref(), Some("Akash Kore"));
        assert_eq!(
            item.added_by_avatars,
            ["https://yt3.ggpht.com/fixture-adder=s48-c-k-c0x00ffffff-no-rj"]
        );
        assert_eq!(
            item.added_by_channel.as_deref(),
            Some("UCbBecQPKuJnY1eKo8dVfI0g")
        );
        // An ordinary row names no one, and no channel either.
        let mut plain = collab_row();
        plain.as_object_mut().unwrap().remove("contributorsAvatars");
        let item = parse_list_item(&plain).unwrap();
        assert_eq!((item.added_by, item.added_by_channel), (None, None));
    }

    /// The row's votes: the count as YouTube writes it for each way, and the token each button
    /// sends now and when pressed again (its toggled state undoes it).
    #[test]
    fn a_collaborative_row_carries_its_votes_and_both_tokens_of_each_button() {
        let vote = parse_list_item(&collab_row()).unwrap().vote.unwrap();
        assert_eq!(
            vote,
            PlaylistVote {
                mine: VoteChoice::Neutral,
                votes: 0,
                count: Some("0".into()),
                count_up: Some("1".into()),
                count_down: Some("-1".into()),
                up: VoteButton {
                    token: "FIXTURE-UPVOTE".into(),
                    undo_token: Some("FIXTURE-UNDO-UPVOTE".into()),
                },
                down: VoteButton {
                    token: "FIXTURE-DOWNVOTE".into(),
                    undo_token: Some("FIXTURE-UNDO-DOWNVOTE".into()),
                },
            }
        );
        let voting = |row: &mut Value| -> Value {
            row["engagementBar"]["engagementBarViewModel"]["actions"][0]["votingViewModel"].take()
        };
        let with = |f: &dyn Fn(&mut Value)| {
            let mut row = collab_row();
            let mut v = voting(&mut row);
            f(&mut v);
            row["engagementBar"]["engagementBarViewModel"]["actions"][0]["votingViewModel"] = v;
            parse_list_item(&row).unwrap().vote
        };
        // A pressed button reads as your vote, whichever way; so does the state YouTube names.
        let up = with(&|v| v["upvoteButton"]["toggleButtonViewModel"]["isToggled"] = json!(true));
        assert_eq!(up.unwrap().mine, VoteChoice::Up);
        let down =
            with(&|v| v["downvoteButton"]["toggleButtonViewModel"]["isToggled"] = json!(true));
        assert_eq!(down.unwrap().mine, VoteChoice::Down);
        let said = with(&|v| v["initialState"]["status"] = json!("VOTE_STATUS_DOWNVOTED"));
        assert_eq!(said.unwrap().mine, VoteChoice::Down);
        // A button with no token to send can't vote: no votes rather than half a pair.
        assert_eq!(with(&|v| v["downvoteButton"] = json!({})), None);
        // A row with no voting (an ordinary playlist) has none to show.
        let mut plain = collab_row();
        plain.as_object_mut().unwrap().remove("engagementBar");
        assert_eq!(parse_list_item(&plain).unwrap().vote, None);
    }

    /// A history row's own menu holds "Remove from history": its token, found by the icon (the
    /// words are localised). Rows anywhere else have none.
    #[test]
    fn a_history_row_carries_its_remove_from_history_token() {
        let item: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/history_remove_item.json"
        ))
        .unwrap();
        let mut row = collab_row();
        row["menu"] = json!({ "menuRenderer": { "items": [
            { "menuServiceItemRenderer": item["menuServiceItemRenderer"].clone() }
        ] } });
        assert_eq!(
            parse_list_item(&row)
                .unwrap()
                .remove_from_history
                .as_deref(),
            Some("FIXTURE-REMOVE-FROM-HISTORY")
        );
        assert_eq!(
            parse_list_item(&collab_row()).unwrap().remove_from_history,
            None
        );
    }
}
