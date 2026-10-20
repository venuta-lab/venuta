use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Playlist comment prefix for stable remote-catalog entries (JSON follows the colon).
/// Never store stream URLs in this tag.
pub const ONLINE_PLAYLIST_TAG: &str = "#VENUTA-ONLINE:";
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Stable identity for a queue/playlist entry. Stream URLs never appear here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayableId {
    Local {
        /// Library-relative path, same convention as today's queue items.
        path: String,
    },
    /// A remote-catalog track identified only by stable provider + id.
    Online { provider: String, track_id: String },
}

/// UI/availability tag derived at play time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Local,
    Online,
    /// Local file exists and is preferred over a bound online id.
    LocalPreferred,
}

/// Display + identity payload shared by local and online queue rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playable {
    pub id: PlayableId,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// Filled by the app when known; not required for persistence of online ids.
    #[serde(default)]
    pub availability: Option<Availability>,
    /// Absolute file path when local and resolved against the open library root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTrack {
    pub provider: String,
    pub track_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub isrc: Option<String>,
    // --- Ranking hints (search only; all optional, never needed to play) ---
    /// What kind of upload the catalog says it is: "ATV" (the artist's own audio track),
    /// "OMV" (official music video), "UGC" (someone's upload), "PODCAST_EPISODE", or another
    /// catalog word as given. None when unknown.
    #[serde(default)]
    pub video_type: Option<String>,
    /// Plays (songs) or views (videos), as a number, when the catalog shows one.
    #[serde(default)]
    pub plays: Option<u64>,
    /// The catalog's own top result for this search.
    #[serde(default)]
    pub is_top: bool,
    /// Where it sat among the unfiltered search's songs and videos (0 = first), when it did.
    #[serde(default)]
    pub shelf_rank: Option<u16>,
    /// What the signed-in account can do with this song from its own menu where the catalog
    /// listed it: save it to the library, pin it, tell it "Not interested". Only rows read as
    /// the account carry it; each press sends one of its tokens and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<TrackMenu>,
    /// The lead artist's channel (`UC…`), when the row links one: what Follow in its menu follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
}

/// A song's own account menu, as the catalog listed it with the song ([`RemoteTrack::menu`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackMenu {
    /// In the account's library (Library, Songs) or not, and the token for each way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<MenuToggle>,
    /// Pinned to Listen again or not, and the token for each way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<MenuToggle>,
    /// "Tell me less of this" presses ("Not interested"), in the catalog's own words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<MenuFeedback>,
    /// When it was listed (milliseconds since 1970): a press made later stands over it, and a
    /// listing made later than a press stands over the press.
    #[serde(default)]
    pub listed_at: u64,
}

impl TrackMenu {
    /// Nothing to offer (a row read signed out).
    pub fn is_empty(&self) -> bool {
        self.library.is_none() && self.pin.is_none() && self.feedback.is_empty()
    }
}

/// A two-way menu press: how it stands now, and what each way sends. A way the row didn't carry
/// can't be pressed until the song is listed again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuToggle {
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_token: Option<String>,
}

/// A one-way menu press ("Not interested"): the catalog's icon name and words, and its token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuFeedback {
    pub icon: String,
    pub label: String,
    pub token: String,
}

impl RemoteTrack {
    pub fn playable_id(&self) -> PlayableId {
        PlayableId::Online {
            provider: self.provider.clone(),
            track_id: self.track_id.clone(),
        }
    }

    pub fn into_playable(self) -> Playable {
        Playable {
            id: self.playable_id(),
            title: self.title,
            artists: self.artists,
            album: self.album,
            duration_ms: self.duration_ms,
            artwork_url: self.artwork_url,
            availability: Some(Availability::Online),
            local_file: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAlbum {
    pub provider: String,
    pub album_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub track_count: Option<u32>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// "Album", "Single", "EP"… as the catalog labels it (a ranking hint).
    #[serde(default)]
    pub kind: Option<String>,
    /// Its card's "tell me less of this" presses (Home's "Not interested"), signed in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<MenuFeedback>,
}

/// A whole playlist from the online catalog: its name and every song, in order (ids and what to
/// show only, never a stream address).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePlaylist {
    pub title: Option<String>,
    pub tracks: Vec<RemoteTrack>,
    /// Its page's radio (the header's Start radio), as the catalog put it there.
    #[serde(default)]
    pub radio: Option<RadioEndpoint>,
}

/// An artist's page in the online catalog: who they are, their songs, their records, and
/// artists like them. Ids and what to show only, never a stream address.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteArtistPage {
    pub provider: String,
    pub artist_id: String,
    pub name: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// Monthly listeners, when the catalog shows it.
    #[serde(default)]
    pub audience: Option<u64>,
    /// The catalog's top songs for them (a handful).
    #[serde(default)]
    pub top_songs: Vec<RemoteTrack>,
    /// More of their songs (their song list's first page), the top ones left out.
    #[serde(default)]
    pub songs: Vec<RemoteTrack>,
    #[serde(default)]
    pub albums: Vec<RemoteAlbum>,
    /// Singles and EPs.
    #[serde(default)]
    pub singles: Vec<RemoteAlbum>,
    /// Artists their listeners also play.
    #[serde(default)]
    pub similar: Vec<RemoteArtist>,
    /// Who they are, in a paragraph or two, when the catalog has it.
    #[serde(default)]
    pub description: Option<String>,
    /// Subscribers as the catalog words it ("6.52M subscribers").
    #[serde(default)]
    pub subscribers: Option<String>,
    /// The wide picture across the top of their page (the square one is `artwork_url`).
    #[serde(default)]
    pub banner_url: Option<String>,
    /// Their radio: a never-ending mix of them and artists like them.
    #[serde(default)]
    pub radio_id: Option<String>,
    /// Their page's radio button, as the catalog put it there (`radio_id` and its params).
    #[serde(default)]
    pub radio: Option<RadioEndpoint>,
    /// Their page's Shuffle button, as the catalog put it there.
    #[serde(default)]
    pub shuffle: Option<RadioEndpoint>,
    /// Every row of their page in the catalog's order: Albums, Singles & EPs, Videos, Live
    /// performances, Featured on, Playlists by them, Fans might also like, Appears on.
    #[serde(default)]
    pub shelves: Vec<RemoteShelf>,
}

/// An album's page in the online catalog: what it is, who made it (each linked to their page
/// when the catalog links them), its songs, and the rows under them (other versions, more by the
/// artist, releases like it). Ids and what to show only, never a stream address.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAlbumPage {
    pub album: RemoteAlbum,
    /// Who made it, in credit order.
    #[serde(default)]
    pub artists: Vec<RemoteArtistLink>,
    /// What it is and when, as the catalog words it ("Album • 2026", "EP • 2025").
    #[serde(default)]
    pub subtitle: Option<String>,
    /// How long, as the catalog words it ("18 songs • 1 hour, 8 minutes").
    #[serde(default)]
    pub length: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub explicit: bool,
    pub tracks: Vec<RemoteTrack>,
    #[serde(default)]
    pub shelves: Vec<RemoteShelf>,
    /// The album's own list in the catalog (`OLAK5uy_…`): what its queue is, and what Autoplay
    /// asks for when it ends.
    #[serde(default)]
    pub playlist_id: Option<String>,
    /// Its page's radio (the header's Start radio), as the catalog put it there.
    #[serde(default)]
    pub radio: Option<RadioEndpoint>,
}

/// A name in a credit line, and their artist page when the catalog links one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteArtistLink {
    pub name: String,
    #[serde(default)]
    pub artist_id: Option<String>,
}

/// How a catalog song is rated on the signed-in account: liked, disliked, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRating {
    Like,
    Dislike,
    None,
}

/// Who is signed in to the catalog (YouTube Music), as its account menu says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAccount {
    pub name: Option<String>,
    pub email: Option<String>,
    pub handle: Option<String>,
    pub picture: Option<String>,
}

/// The catalog's home feed: its mood chips (each filters the feed), its rows, and where more
/// rows start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHome {
    pub chips: Vec<RemoteChip>,
    /// Rows, each with `kind` when it is one the app knows ("quick_picks", "listen_again"…).
    pub shelves: Vec<RemoteHomeShelf>,
    #[serde(default)]
    pub continuation: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteChip {
    pub title: String,
    /// Opaque: asks for the feed filtered to this chip.
    pub params: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHomeShelf {
    #[serde(flatten)]
    pub shelf: RemoteShelf,
    #[serde(default)]
    pub kind: Option<String>,
}

/// A song's lyrics as the catalog has them: timed lines when it has them, else the words as
/// text, and who provided them ("Source: Musixmatch").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogLyrics {
    /// `(start ms, line)`, in order; empty when only text is known.
    pub lines: Vec<(u64, String)>,
    pub text: String,
    #[serde(default)]
    pub credit: Option<String>,
}

/// Where a queue of the catalog's starts: the endpoint the catalog put on a button, a tuning chip
/// or the end of a list, kept as it came and sent back as it is (its `params` are opaque). Built
/// here only as a fallback, for a song with no button (`RDAMVM` and its id).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioEndpoint {
    #[serde(default)]
    pub video_id: Option<String>,
    #[serde(default)]
    pub playlist_id: Option<String>,
    #[serde(default)]
    pub params: Option<String>,
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub playlist_set_video_id: Option<String>,
}

/// A tuning chip over a radio's queue ("All", "Familiar", "Deep cuts", a mood…).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadioChip {
    pub text: String,
    /// The one in effect, as the catalog marks it.
    #[serde(default)]
    pub selected: bool,
    /// What choosing it asks for: its songs replace what is coming up.
    pub endpoint: RadioEndpoint,
}

/// A radio, or a page of one: the catalog's queue as it sent it (its order, a song coming round
/// again and all), and everything needed to keep that same session going: the page after this
/// one, the chips that re-tune it, and what it goes on with when it ends. Ids and what to show
/// only, never a stream address.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRadio {
    /// The radio's own list id.
    #[serde(default)]
    pub playlist_id: Option<String>,
    /// Its songs in the catalog's order. On a radio of a song, that song leading the answer is
    /// left out (it is the one playing); nothing else is.
    pub tracks: Vec<RemoteTrack>,
    /// What the catalog's player calls it ("Kitni Baatein Radio").
    #[serde(default)]
    pub title: Option<String>,
    /// What was asked for (after a dead radio was swapped for the mix it named): sent again with
    /// `continuation` for the next page.
    #[serde(default)]
    pub endpoint: Option<RadioEndpoint>,
    /// The next page of the same radio; None on its last.
    #[serde(default)]
    pub continuation: Option<String>,
    /// The tuning chips (a radio's first page only).
    #[serde(default)]
    pub chips: Vec<RadioChip>,
    /// What Autoplay goes on with when this runs out (the list's automix marker).
    #[serde(default)]
    pub continue_with: Option<RadioEndpoint>,
    /// The catalog's Related page for the song it is about (`MPTRt…`).
    #[serde(default)]
    pub related_id: Option<String>,
}

/// A titled row of cards on a catalog page, and where its "See all" leads when it has one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteShelf {
    pub title: String,
    /// Songs and videos to play; albums, artists and playlists to open.
    pub items: Vec<RemoteSuggestion>,
    #[serde(default)]
    pub more: Option<RemoteMore>,
}

/// One shelf of a song's Related page, in the catalog's order: songs to play ("You might also
/// like"), albums, artists and playlists to open ("Similar artists", "Recommended playlists"),
/// or words about the artist. Ids and what to show only, never a stream address.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRelatedShelf {
    pub title: String,
    #[serde(default)]
    pub tracks: Vec<RemoteTrack>,
    #[serde(default)]
    pub items: Vec<RemoteSuggestion>,
    /// "About the artist": the words, as the catalog has them.
    #[serde(default)]
    pub about: Option<String>,
}

/// The catalog page behind a row's "See all".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteMore {
    pub browse_id: String,
    #[serde(default)]
    pub params: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteArtist {
    pub provider: String,
    pub artist_id: String,
    pub name: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// Monthly listeners (or subscribers) as a number, when the catalog shows it (a ranking hint).
    #[serde(default)]
    pub audience: Option<u64>,
}

/// A playlist or podcast card from the online catalog: what it is called, its owner line
/// ("YouTube Music • 50 songs", "T-Series • 550K views") and its cover. Opening it lists its songs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCollection {
    pub provider: String,
    /// The catalog page id (a playlist's `VL…`, a podcast's `MPSP…`).
    pub collection_id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// "playlist" or "podcast".
    pub kind: String,
    /// What the card's own play button plays (a mix's endpoint, `params` and all): played
    /// as it came, rather than a radio rebuilt from the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play: Option<RadioEndpoint>,
    /// Its card's "tell me less of this" presses (Home's "Not interested"), signed in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<MenuFeedback>,
}

/// One thing the catalog suggests while words are typed: a song to play, or an album, artist
/// or playlist to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteSuggestion {
    Song {
        track: RemoteTrack,
    },
    Album {
        album: RemoteAlbum,
        subtitle: Option<String>,
    },
    Artist {
        artist: RemoteArtist,
        subtitle: Option<String>,
    },
    Playlist {
        collection: RemoteCollection,
    },
}

/// What the catalog suggests for half-typed words: completions of the words (spelling fixed),
/// and a few matching songs, albums, artists and playlists, in the catalog's order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSuggestions {
    pub queries: Vec<String>,
    pub items: Vec<RemoteSuggestion>,
    /// The ones of `queries` that are the account's own past searches (signed-in search only),
    /// with the token that takes each out of its search history when YouTube sent one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past: Vec<PastSearch>,
}

/// One of the account's past searches, as a suggestion carried it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PastSearch {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_token: Option<String>,
}

/// A browse tile: a mood ("Chill", "Workout") or a genre, and the color the catalog gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoodTile {
    pub title: String,
    /// Opaque token that opens this mood's page.
    pub params: String,
    /// `#rrggbb`.
    pub color: String,
}

/// A titled group of tiles ("Moods & moments", "Genres"), in the catalog's order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoodSection {
    pub title: String,
    pub tiles: Vec<MoodTile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    pub limit: usize,
    pub cursor: Option<String>,
    /// A search the person typed (the Search box). Only such a search may go as the account,
    /// and only with Signed-in search on; Venuta's own look-ups (a file's song for its radio,
    /// link matching, More by, find online, imports) leave it false and stay anonymous.
    pub typed: bool,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: 25,
            cursor: None,
            typed: false,
        }
    }
}

impl PageRequest {
    pub fn new(limit: usize, cursor: Option<String>) -> Self {
        Self {
            limit: limit.clamp(1, 100),
            cursor,
            typed: false,
        }
    }

    /// The same request, for a search the person typed.
    pub fn typed(self) -> Self {
        Self {
            typed: true,
            ..self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// HTTP header required only for the lifetime of resolved session audio.
#[derive(Clone, PartialEq, Eq)]
pub struct MediaHeader {
    pub name: String,
    pub value: String,
}

impl fmt::Debug for MediaHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaHeader")
            .field("name", &self.name)
            .field("value", &"[redacted]")
            .finish()
    }
}

/// Direct HTTP audio for the current playback session only.
///
/// Not serializable on purpose: playlists and restored queues must store stable ids, not URLs.
#[derive(Clone, PartialEq)]
pub struct EphemeralAudio {
    pub track_id: String,
    pub provider: String,
    pub url: String,
    pub mime_type: Option<String>,
    pub content_length: Option<u64>,
    pub duration_ms: Option<u64>,
    pub headers: Vec<MediaHeader>,
    /// Which way found the address ("catalog", "stream tool", "fallback", "fixture"), for the
    /// playback log's timings.
    pub via: &'static str,
    /// How much louder than the catalog's reference the song is, in dB, when it says (its
    /// `loudnessDb`): levelling turns an online song down (or up) by it.
    pub loudness_db: Option<f64>,
}

impl fmt::Debug for EphemeralAudio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EphemeralAudio")
            .field("track_id", &self.track_id)
            .field("provider", &self.provider)
            .field("url", &"[redacted]")
            .field("mime_type", &self.mime_type)
            .field("content_length", &self.content_length)
            .field("duration_ms", &self.duration_ms)
            .field("headers", &self.headers)
            .finish()
    }
}

/// Cooperative cancellation for catalog and resolve calls.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
