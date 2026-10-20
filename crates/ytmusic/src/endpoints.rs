//! High-level endpoint facade over the transport. context/03, 08.

use serde::Serialize;

use crate::blocklist;
use crate::clients::YouTubeClient;
use crate::models::browse::{
    self, AlbumPage, ArtistPage, BrowseItem, HistoryGroup, HomePage, MoodSection,
    PlaylistContinuation, PlaylistPage, PlaylistSort, PlaylistSuggestions, RelatedShelf,
    SearchResults, SearchSuggestions, Section,
};
use crate::models::comment_write::WriteCommand;
use crate::models::comments::{self, CommentReplies, CommentsPage};
use crate::models::context::Context;
use crate::models::lyrics::{self, PlainLyrics, TimedLyricLine};
use crate::models::metadata::{
    self, AccountIdentity, AccountInfo, NextResult, Rating, SearchResult, SongItem, WatchEndpoint,
};
use crate::models::player::{
    ContentPlaybackContext, PlaybackContext, PlayerBody, PlayerResponse, ServiceIntegrityDimensions,
};
use crate::transport::{enum_like, Error, InnerTube};

/// Search filter params (opaque base64). context/08.
pub const FILTER_SONG: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D";
pub const FILTER_VIDEO: &str = "EgWKAQIQAWoKEAkQChAFEAMQBA%3D%3D";
pub const FILTER_ALBUM: &str = "EgWKAQIYAWoKEAkQChAFEAMQBA%3D%3D";
pub const FILTER_ARTIST: &str = "EgWKAQIgAWoKEAkQChAFEAMQBA%3D%3D";
pub const FILTER_COMMUNITY_PLAYLIST: &str = "EgeKAQQoAEABagoQAxAEEAoQCRAF";
/// Podcast shows (ytmusicapi's `JQ` filter, in the same parameter shape as the others).
pub const FILTER_PODCAST: &str = "EgWKAQJQAWoKEAkQChAFEAMQBA%3D%3D";

/// A comment's like/unlike/dislike/undislike and delete: `{context, actions: [<token>]}`.
pub const COMMENT_ACTION_PATH: &str = "comment/perform_comment_action";

impl InnerTube {
    /// `/player` for one client. context/03, context/06.
    ///
    /// `sts` — signature timestamp from the deciphering player.js (context/05); sent as
    /// `playbackContext.contentPlaybackContext.signatureTimestamp` so ciphered clients return
    /// usable formats. `po_token` — the session/streaming PoToken (context/04); sent as
    /// `serviceIntegrityDimensions.poToken` for web clients. Both `None` for the plain
    /// direct-URL clients that need neither.
    pub async fn player(
        &self,
        client: &YouTubeClient,
        video_id: &str,
        playlist_id: Option<&str>,
        sts: Option<i32>,
        po_token: Option<&str>,
    ) -> Result<PlayerResponse, Error> {
        let mut context = self.context_for(client);
        if let Some(tp) = context.third_party.as_mut() {
            tp.embed_url = format!("https://www.youtube.com/watch?v={video_id}");
        }
        let body = PlayerBody {
            context,
            video_id: video_id.to_owned(),
            playlist_id: playlist_id.map(str::to_owned),
            playback_context: sts.map(|signature_timestamp| PlaybackContext {
                content_playback_context: ContentPlaybackContext {
                    signature_timestamp,
                },
            }),
            service_integrity_dimensions: po_token.map(|t| ServiceIntegrityDimensions {
                po_token: t.to_owned(),
            }),
            content_check_ok: true,
            racy_check_ok: true,
        };
        let value = self
            .post("player", client, &body, /* set_login */ true)
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// Raw `search` POST. `params` = a filter (None = the mixed, unfiltered search). context/08.
    ///
    /// `record_history` decides whether the request carries the account at all. A signed-in
    /// `search` is written to the account's YouTube search history, and the typeahead fires one
    /// per debounce, so every half-typed prefix used to show up later in YouTube's and YTM's own
    /// search box (#203). Only the query the user actually submitted passes `true`; a preview goes
    /// out with no cookie and no `onBehalfOfUser`, which YouTube cannot attribute to anyone.
    /// Nothing is lost by that: a search response carries no per-account field, not even
    /// `likeStatus`, so the anonymous rows are the same rows minus personalised ranking.
    async fn search_raw(
        &self,
        client: &YouTubeClient,
        query: &str,
        params: Option<&str>,
        record_history: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct SearchBody {
            context: Context,
            query: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            params: Option<String>,
        }
        let body = SearchBody {
            context: if record_history {
                self.context_for(client)
            } else {
                self.context_anonymous(client)
            },
            query: query.to_owned(),
            params: params.map(str::to_owned),
        };
        self.post("search", client, &body, /* set_login */ record_history)
            .await
    }

    // --- "hide music videos" (user setting, off by default) ------------------------------------
    //
    // Gated at the fetch boundary rather than at each queue/search call site: every consumer
    // inherits it and there is no second policy to keep in sync. A row is a video when YouTube's
    // own `musicVideoType` says so (see `metadata::is_video_row`) — never by matching the title.

    fn drop_video_songs(&self, items: &mut Vec<SongItem>) {
        if self.hide_videos() {
            items.retain(|i| !i.is_video);
        }
    }

    fn drop_video_cards(&self, items: &mut Vec<BrowseItem>) {
        if self.hide_videos() {
            items.retain(|i| !i.is_video);
        }
    }

    // --- blocked artists (user list, empty by default) -----------------------------------------
    //
    // Same boundary and the same reason as "hide music videos", but a narrower set of surfaces:
    // only what YouTube generates. A playlist, an album, a search and the history are things the
    // user asked for by name, and silently dropping rows out of them is a bug, not a feature.
    // See plan 046.

    fn drop_blocked_songs(&self, items: &mut Vec<SongItem>, keep: Option<&str>) {
        blocklist::retain_songs(items, &self.blocked(), keep);
    }

    fn drop_blocked_cards(&self, items: &mut Vec<BrowseItem>) {
        blocklist::retain_cards(items, &self.blocked());
    }

    /// Search songs only (`FILTER_SONG`). context/08.
    pub async fn search_songs(
        &self,
        metadata_client: &YouTubeClient,
        query: &str,
        record_history: bool,
    ) -> Result<SearchResult, Error> {
        let value = self
            .search_raw(metadata_client, query, Some(FILTER_SONG), record_history)
            .await?;
        let mut r = metadata::parse_search(&value);
        self.drop_video_songs(&mut r.items);
        Ok(r)
    }

    /// Search video uploads only (`FILTER_VIDEO`): the covers, live sets and remixes that never
    /// got an official release, which `FILTER_SONG` cannot return by definition (#209, #266).
    /// context/08.
    pub async fn search_videos(
        &self,
        metadata_client: &YouTubeClient,
        query: &str,
    ) -> Result<SearchResult, Error> {
        // Not a filter on the rows, a refusal to ask: with music videos hidden there is no such
        // thing as a video search, and the caller's shelf disappears on an empty list.
        if self.hide_videos() {
            return Ok(SearchResult { items: Vec::new() });
        }
        // No history: the search page already recorded this query with its other two searches.
        let value = self
            .search_raw(metadata_client, query, Some(FILTER_VIDEO), false)
            .await?;
        Ok(metadata::parse_search(&value))
    }

    /// Unfiltered search → categorized sections (top / songs / albums / artists / playlists).
    pub async fn search_all(
        &self,
        client: &YouTubeClient,
        query: &str,
        record_history: bool,
    ) -> Result<SearchResults, Error> {
        let value = self.search_raw(client, query, None, record_history).await?;
        let mut r = browse::parse_search_all(&value);
        self.drop_video_cards(&mut r.top);
        self.drop_video_cards(&mut r.songs);
        Ok(r)
    }

    /// Typeahead for the search field and Ctrl+K: query completions plus a few matching songs,
    /// artists and albums. context/08.
    ///
    /// Sent with the account only when "Signed-in search" is on: this is the request YTM's own
    /// search box sends signed in on every keystroke, so it is not written to search history, but
    /// the account ranks the user's own listening first and returns their past searches, and
    /// every typed prefix reaches it. Off (the default), it goes like a preview `search` (#203).
    pub async fn search_suggestions(
        &self,
        client: &YouTubeClient,
        input: &str,
    ) -> Result<SearchSuggestions, Error> {
        #[derive(Serialize)]
        struct Body {
            context: Context,
            input: String,
        }
        let signed = self.signed_search();
        let body = Body {
            context: self.context_as(client, signed),
            input: input.to_owned(),
        };
        let value = self
            .post("music/get_search_suggestions", client, &body, signed)
            .await?;
        let mut r = browse::parse_search_suggestions(&value);
        self.drop_video_cards(&mut r.items);
        Ok(r)
    }

    /// Filtered card search for a "Show more" page. `category` ∈ albums / artists / playlists.
    pub async fn search_cards(
        &self,
        client: &YouTubeClient,
        query: &str,
        category: &str,
    ) -> Result<Vec<BrowseItem>, Error> {
        let filter = match category {
            "albums" => FILTER_ALBUM,
            "artists" => FILTER_ARTIST,
            "playlists" => FILTER_COMMUNITY_PLAYLIST,
            "podcasts" => FILTER_PODCAST,
            other => return Err(Error::Other(format!("unknown search category: {other}"))),
        };
        // No history: "Show more" repeats the query the search page already recorded.
        let value = self.search_raw(client, query, Some(filter), false).await?;
        Ok(browse::parse_search_cards(&value))
    }

    /// Up-next queue / radio for a video. context/08. Uses the metadata client.
    ///
    /// `video_id` is optional: an artist/mood radio is a playlist id with no seed track
    /// (`playlistId` alone), which is how YouTube itself opens one.
    pub async fn next(
        &self,
        metadata_client: &YouTubeClient,
        video_id: Option<&str>,
        playlist_id: Option<&str>,
    ) -> Result<NextResult, Error> {
        let endpoint = WatchEndpoint {
            video_id: video_id.map(str::to_owned),
            playlist_id: playlist_id.map(str::to_owned),
            ..Default::default()
        };
        self.next_watch(metadata_client, &endpoint, None).await
    }

    /// `next` for a watch endpoint, the way YouTube Music's web app asks: the endpoint a button,
    /// a chip or the automix marker carried, sent as it came (see [`watch_body`]), and with
    /// `continuation` the next page of that same queue.
    pub async fn next_watch(
        &self,
        metadata_client: &YouTubeClient,
        endpoint: &WatchEndpoint,
        continuation: Option<&str>,
    ) -> Result<NextResult, Error> {
        // As the account unless "Recommendations for me" is off: then YouTube's general radio.
        let signed = self.personal();
        let mut body = watch_body(endpoint, continuation);
        body.insert(
            "context".into(),
            serde_json::to_value(self.context_as(metadata_client, signed))?,
        );
        let value = self.post("next", metadata_client, &body, signed).await?;
        let mut next = metadata::parse_next(&value);
        let video_id = endpoint.video_id.as_deref();
        // The seed itself survives: "start radio from this video" must still open on that video,
        // and the track already playing is never yanked out from under the user.
        if self.hide_videos() {
            next.items
                .retain(|i| !i.is_video || Some(i.video_id.as_str()) == video_id);
        }
        self.drop_blocked_songs(&mut next.items, video_id);
        Ok(next)
    }

    /// The songs of a Related page (a `next` answer's `related_browse_id`): what YouTube Music
    /// relates to that song. Generated by YouTube, so music videos and blocked artists stay out
    /// as they do from a radio.
    pub async fn related_songs(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<Vec<SongItem>, Error> {
        // A recommendation like the radio: as the account only with "Recommendations for me".
        let value = self
            .browse_as(client, Some(browse_id), None, self.personal())
            .await?;
        let mut songs = metadata::parse_related_songs(&value);
        self.drop_video_songs(&mut songs);
        self.drop_blocked_songs(&mut songs, None);
        Ok(songs)
    }

    /// The whole of a Related page, shelf by shelf in YouTube's order: its songs, its playlists,
    /// artists and albums, and the words about the artist (what the Related tab shows). The same
    /// recommendation as [`Self::related_songs`], filtered the same way.
    pub async fn related_page(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<Vec<RelatedShelf>, Error> {
        let value = self
            .browse_as(client, Some(browse_id), None, self.personal())
            .await?;
        let mut shelves = browse::parse_related(&value);
        for s in &mut shelves {
            self.drop_video_songs(&mut s.songs);
            self.drop_blocked_songs(&mut s.songs, None);
            self.drop_video_cards(&mut s.cards);
            self.drop_blocked_cards(&mut s.cards);
        }
        // A shelf the filter emptied would read as a bare heading.
        shelves.retain(|s| !s.songs.is_empty() || !s.cards.is_empty() || s.description.is_some());
        Ok(shelves)
    }

    /// Mixed for you's own page (`FEmusic_mixed_for_you`, the "More" of Home's shelf): the
    /// account's mixes, each with its own play button. Asked for the way Home is, as the account
    /// only with "Recommendations for me".
    pub async fn mixed_for_you(&self, client: &YouTubeClient) -> Result<Vec<Section>, Error> {
        let value = self
            .browse_as(client, Some("FEmusic_mixed_for_you"), None, self.personal())
            .await?;
        let mut sections = browse::parse_mixed_for_you(&value);
        for s in &mut sections {
            self.drop_video_cards(&mut s.items);
            self.drop_blocked_cards(&mut s.items);
        }
        sections.retain(|s| !s.items.is_empty());
        Ok(sections)
    }

    // --- comments. Reads go through `next`: a comments token sent to `browse` is answered with
    // the Home feed. -----------------------------------------------------------------------

    /// First page of comments for a video (the Comments tab's token, then its page); no token is
    /// `CommentsState::Disabled`. Signed in, both requests go as the account without the healer;
    /// a 401/403 redoes the whole chain anonymously, so an account's token is never sent without it.
    pub async fn comments(
        &self,
        client: &YouTubeClient,
        video_id: &str,
    ) -> Result<CommentsPage, Error> {
        if self.can_read_as_account(client) {
            match self.comments_first_page(client, video_id, true).await {
                Ok(mut page) => {
                    page.read_as_account = true;
                    return Ok(page);
                }
                Err(e) if is_refusal(&e) => {
                    // Nothing about the request or the account is logged.
                    tracing::debug!("signed-in comments read refused, redoing it anonymously");
                }
                Err(e) => return Err(e),
            }
        }
        self.comments_first_page(client, video_id, false).await
    }

    /// The tab lookup and the first page, both sent the same way (`as_account`).
    async fn comments_first_page(
        &self,
        client: &YouTubeClient,
        video_id: &str,
        as_account: bool,
    ) -> Result<CommentsPage, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct NextBody {
            context: Context,
            video_id: String,
            is_audio_only: bool,
        }
        let tabs = self
            .comments_send(client, as_account, |context| NextBody {
                context,
                video_id: video_id.to_owned(),
                is_audio_only: true,
            })
            .await?;
        let Some(token) = comments::parse_comments_token(&tabs) else {
            return Ok(CommentsPage::disabled());
        };
        let value = self.next_continuation(client, &token, as_account).await?;
        Ok(comments::parse_comments_page(&value, true))
    }

    /// The next page of comments, or another sort (the header's sort entries carry tokens).
    /// `as_account` is how the page that issued `token` was read; a refusal is not retried
    /// anonymously.
    pub async fn comments_continuation(
        &self,
        client: &YouTubeClient,
        token: &str,
        as_account: bool,
    ) -> Result<CommentsPage, Error> {
        let value = self.next_continuation(client, token, as_account).await?;
        // A sort switch answers with a header and a body, a page with the body alone.
        let mut page = comments::parse_comments_page(&value, false);
        page.read_as_account = as_account;
        Ok(page)
    }

    /// One page of a thread's replies, via the token on the thread (or on the previous page's
    /// "Show more replies"). `as_account` and refusals as for [`Self::comments_continuation`].
    pub async fn comment_replies(
        &self,
        client: &YouTubeClient,
        token: &str,
        as_account: bool,
    ) -> Result<CommentReplies, Error> {
        let value = self.next_continuation(client, token, as_account).await?;
        Ok(comments::parse_comment_replies(&value))
    }

    /// `next` with a bare continuation token. Not `browse_continuation`, see above.
    async fn next_continuation(
        &self,
        client: &YouTubeClient,
        token: &str,
        as_account: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        struct ContinuationBody {
            context: Context,
            continuation: String,
        }
        self.comments_send(client, as_account, |context| ContinuationBody {
            context,
            continuation: token.to_owned(),
        })
        .await
    }

    /// Like, unlike, dislike or undislike: replays the comment's own toolbar token, through the
    /// ordinary path like `rate`. Needs `STATUS_SUCCEEDED` ([`answer_verdict`]).
    pub async fn comment_action(
        &self,
        client: &YouTubeClient,
        token: &str,
        kind: &'static str,
    ) -> Result<(), Error> {
        self.send_action(client, token, kind, false)
            .await
            .map(|_| ())
    }

    /// [`Self::comment_action`] and a delete. `missing_ok` accepts an answer with no status (a
    /// delete's). Returns the answer.
    async fn send_action(
        &self,
        client: &YouTubeClient,
        token: &str,
        kind: &'static str,
        missing_ok: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        struct ActionBody {
            context: Context,
            actions: Vec<String>,
        }
        let body = ActionBody {
            context: self.context_for(client),
            actions: vec![token.to_owned()],
        };
        let value = self.post(COMMENT_ACTION_PATH, client, &body, true).await?;
        accept_answer(kind, value, missing_ok)
    }

    /// Replays a write command from a signed-in read, with `text` in the command's text field.
    /// `replayable` writes (edit, delete) use `post`; the others use [`InnerTube::post_write`]. A
    /// 404 on a delete is success (`Null`). Returns the answer.
    pub async fn comment_write(
        &self,
        client: &YouTubeClient,
        command: &WriteCommand,
        text: Option<&str>,
        replayable: bool,
    ) -> Result<serde_json::Value, Error> {
        let kind = command.log_kind();
        let (path, body) = match command {
            WriteCommand::Action(token) => {
                return match self.send_action(client, token, kind, true).await {
                    // A token replay is a delete: a 404 means it is already gone.
                    Err(e) if e.is_not_found() => Ok(serde_json::Value::Null),
                    other => other,
                };
            }
            WriteCommand::Endpoint {
                path,
                payload,
                text_field,
                ..
            } => {
                if !crate::models::comment_write::is_plain_comment_path(path) {
                    tracing::debug!(kind, "comment write refused: not a plain comment path");
                    return Err(Error::Other("Not a comment path.".into()));
                }
                let mut body = serde_json::Map::new();
                body.insert(
                    "context".into(),
                    serde_json::to_value(self.context_for(client))?,
                );
                for (key, value) in payload {
                    // What the server issued never replaces the context.
                    body.entry(key.clone()).or_insert_with(|| value.clone());
                }
                if let Some(text) = text {
                    body.insert((*text_field).into(), text.into());
                }
                (path.as_str(), serde_json::Value::Object(body))
            }
        };
        let value = if replayable {
            self.post(path, client, &body, true).await?
        } else {
            self.post_write(path, client, &body, true).await?
        };
        accept_answer(kind, value, true)
    }

    /// Whether a comments request can go out as the account right now.
    fn can_read_as_account(&self, client: &YouTubeClient) -> bool {
        client.login_supported && self.is_logged_in()
    }

    /// One comments `next` request, as the account through [`InnerTube::post_no_heal`] or
    /// anonymously. An account request with nobody signed in is an error, never an anonymous one.
    async fn comments_send<B: Serialize>(
        &self,
        client: &YouTubeClient,
        as_account: bool,
        build: impl Fn(Context) -> B,
    ) -> Result<serde_json::Value, Error> {
        if as_account {
            if !self.can_read_as_account(client) {
                return Err(Error::AccountChanged);
            }
            self.post_no_heal("next", client, &build(self.context_for(client)), true)
                .await
        } else {
            self.post(
                "next",
                client,
                &build(self.context_anonymous(client)),
                false,
            )
            .await
        }
    }

    /// Logged-in account summary (`account/account_menu`, context/01). Requires a cookie. Also the
    /// source of `dataSyncId` (context/04A) and a login-bound visitorData (context/15).
    pub async fn account_menu(&self, client: &YouTubeClient) -> Result<AccountInfo, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct AccountMenuBody {
            context: Context,
        }
        let body = AccountMenuBody {
            context: self.context_for(client),
        };
        let value = self
            .post("account/account_menu", client, &body, true)
            .await?;
        Ok(metadata::parse_account_menu(&value))
    }

    /// Validate and refresh one delegated identity without mutating the transport's shared
    /// selection. This keeps unrelated in-flight requests on the previously committed channel.
    pub async fn account_menu_for_identity(
        &self,
        client: &YouTubeClient,
        data_sync_id: &str,
    ) -> Result<AccountInfo, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct AccountMenuBody {
            context: Context,
        }
        let body = AccountMenuBody {
            context: self.context_for_identity(client, data_sync_id),
        };
        let value = self
            .post("account/account_menu", client, &body, true)
            .await?;
        Ok(metadata::parse_account_menu(&value))
    }

    /// Every usable YouTube identity under the signed-in Google account. The official web client
    /// opens this sibling endpoint from the account menu; `account/account_menu` itself only
    /// carries the active header.
    pub async fn account_identities(
        &self,
        client: &YouTubeClient,
    ) -> Result<Vec<AccountIdentity>, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct AccountsListBody {
            context: Context,
        }
        let body = AccountsListBody {
            context: self.context_for(client),
        };
        let value = self
            .post("account/accounts_list", client, &body, true)
            .await?;
        Ok(metadata::parse_account_identities(&value))
    }

    /// Raw `browse` call (context/01, context/08). `browse_id`/`params` optional; response is the
    /// deeply-nested renderer tree the browse parsers walk.
    async fn browse(
        &self,
        client: &YouTubeClient,
        browse_id: Option<&str>,
        params: Option<&str>,
    ) -> Result<serde_json::Value, Error> {
        self.browse_as(client, browse_id, params, true).await
    }

    /// [`Self::browse`] as the account (`signed`), or as a signed-out visitor: what YouTube picks
    /// for you is asked for that way when "Recommendations for me" is off.
    async fn browse_as(
        &self,
        client: &YouTubeClient,
        browse_id: Option<&str>,
        params: Option<&str>,
        signed: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BrowseBody {
            context: Context,
            #[serde(skip_serializing_if = "Option::is_none")]
            browse_id: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            params: Option<String>,
        }
        let body = BrowseBody {
            context: self.context_as(client, signed),
            browse_id: browse_id.map(str::to_owned),
            params: params.map(str::to_owned),
        };
        let mut healed = false;
        loop {
            let value = self.post("browse", client, &body, signed).await?;

            // A stale cookie authenticates transport-wise but YouTube returns a logged-out "Sign
            // in" state for account-scoped browse. Same reasoning as the transport's 401: let the
            // healer have a go and retry once before telling the user their session expired.
            if signed && self.is_logged_in() && browse::is_signed_out(&value) {
                if !healed && !self.healing_suspended() {
                    healed = true;
                    tracing::warn!("InnerTube browse returned the signed-out state, healing");
                    self.wait_for_session_heal().await?;
                    tracing::info!("heal finished, retrying browse");
                    continue;
                }
                return Err(self.reject_session());
            }

            return Ok(value);
        }
    }

    /// POST a paging token. The ctoken is carried in the query, matching Metrolist's
    /// browse-continuation call; every paged surface (home, playlist tracks, library grids) uses
    /// this same carrier.
    async fn browse_continuation(
        &self,
        client: &YouTubeClient,
        token: &str,
    ) -> Result<serde_json::Value, Error> {
        self.browse_continuation_as(client, token, true).await
    }

    /// [`Self::browse_continuation`] as the account (`signed`) or as a signed-out visitor.
    async fn browse_continuation_as(
        &self,
        client: &YouTubeClient,
        token: &str,
        signed: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ContinuationBody {
            context: Context,
        }
        let body = ContinuationBody {
            context: self.context_as(client, signed),
        };
        let enc = urlencoding::encode(token);
        let path = format!("browse?ctoken={enc}&continuation={enc}&type=next");
        self.post(&path, client, &body, signed).await
    }

    /// A library grid, paged to the end. YouTube hands these out ~25 at a time, so a single browse
    /// silently truncated anyone's bigger library (issue #72).
    ///
    /// Library grids only. `browse_grid`'s "More" targets deliberately do NOT page: they are read
    /// as cards, and some of them (an album's `VL…` shelf) are really 100-track pages, so paging
    /// one would spend a dozen sequential requests building a card wall nobody asked for. The
    /// card grids behind those buttons come back whole anyway (a 91-album discography arrived in
    /// one response, no token).
    async fn library_grid(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<Vec<BrowseItem>, Error> {
        let value = self.browse(client, Some(browse_id), None).await?;
        // A card the grid hands out twice is fatal on the UI side: every library list is keyed by
        // browseId, and one repeat blanks the whole list (the sidebar, the Library page, the
        // add-to-playlist picker) with an each_key_duplicate. Only libraries past the first page
        // saw it, which is why removing a few playlists in YouTube Music "fixed" it. Issues #258,
        // #260.
        let mut seen = std::collections::HashSet::new();
        let mut dropped = 0usize;
        let mut keep = |items: &mut Vec<BrowseItem>| {
            items.retain(|i: &BrowseItem| {
                let fresh = seen.insert(i.id.clone());
                dropped += usize::from(!fresh);
                fresh
            })
        };
        let mut items = browse::parse_library(&value);
        keep(&mut items);
        let mut token = browse::continuation_token(&value);
        // ponytail: page cap, so a token that never resolves can't spin forever. Raise it if
        // anyone turns up with a library past ~500 entries.
        for _ in 0..20 {
            let Some(t) = token else { break };
            // A failed page must not throw away the ones that worked: a short library beats an
            // error where the grid should be.
            let Ok(value) = self.browse_continuation(client, &t).await.inspect_err(|e| {
                tracing::warn!(error = %e, browse_id, "grid continuation failed; keeping what loaded")
            }) else {
                break;
            };
            let mut page = browse::parse_library(&value);
            if page.is_empty() {
                // A spurious token: some grids carry one that resolves to nothing.
                break;
            }
            // Dedupe after the emptiness test, not before: a page that is entirely cards we
            // already have still carries the token for the page after it, and that one can hold
            // cards found nowhere else. A token that cycles is bounded by the self-reference
            // filter below and by the page cap above.
            keep(&mut page);
            items.extend(page);
            token = browse::continuation_token(&value).filter(|next| *next != t);
        }
        // Says so in a log the next reporter pastes: without it, a library that still looks wrong
        // can't be told apart from one that was never duplicating in the first place.
        if dropped > 0 {
            tracing::warn!(
                browse_id,
                dropped,
                "library grid repeated cards; kept the first of each"
            );
        }
        Ok(items)
    }

    /// Home feed (`FEmusic_home`). `params` is a mood/genre chip token from a previous home
    /// response — pass it to get that chip's filtered feed. context/08.
    pub async fn home(
        &self,
        client: &YouTubeClient,
        params: Option<&str>,
    ) -> Result<HomePage, Error> {
        let value = self
            .browse_as(client, Some("FEmusic_home"), params, self.personal())
            .await?;
        let mut page = browse::parse_home(&value);
        for s in &mut page.sections {
            self.drop_video_cards(&mut s.items);
            self.drop_blocked_cards(&mut s.items);
        }
        // A shelf the filter emptied (an all-videos row) would render as a bare heading.
        page.sections.retain(|s| !s.items.is_empty());
        Ok(page)
    }

    /// Next batch of home shelves via a continuation token. Same ctoken carrier as
    /// `playlist_continuation`; the response's shelves parse with `parse_home` (its find_all walk
    /// doesn't care whether shelves sit under `contents` or `continuationContents`).
    pub async fn home_continuation(
        &self,
        client: &YouTubeClient,
        token: &str,
    ) -> Result<HomePage, Error> {
        let value = self
            .browse_continuation_as(client, token, self.personal())
            .await?;
        let mut page = browse::parse_home(&value);
        for s in &mut page.sections {
            self.drop_video_cards(&mut s.items);
            self.drop_blocked_cards(&mut s.items);
        }
        page.sections.retain(|s| !s.items.is_empty());
        Ok(page)
    }

    /// Moods & Genres (`FEmusic_moods_and_genres`): the tiles the search page browses from. A tile's
    /// `params` opens its playlists via `browse_grid("FEmusic_moods_and_genres_category", ..)`.
    /// Works signed out. context/08.
    pub async fn moods(&self, client: &YouTubeClient) -> Result<Vec<MoodSection>, Error> {
        let value = self
            .browse(client, Some("FEmusic_moods_and_genres"), None)
            .await?;
        Ok(browse::parse_moods(&value))
    }

    /// Play history (`FEmusic_history`), in YouTube's own date buckets (Today, Yesterday, …).
    /// context/08. Needs login: signed out, YouTube has nothing to return.
    pub async fn history(&self, client: &YouTubeClient) -> Result<Vec<HistoryGroup>, Error> {
        let value = self.browse(client, Some("FEmusic_history"), None).await?;
        let mut groups = browse::parse_history(&value);
        for g in &mut groups {
            self.drop_video_songs(&mut g.items);
        }
        groups.retain(|g| !g.items.is_empty()); // a bucket the filter emptied is a bare heading
        Ok(groups)
    }

    /// Library playlists grid (`FEmusic_liked_playlists`). context/08. Needs login.
    pub async fn library_playlists(
        &self,
        client: &YouTubeClient,
    ) -> Result<Vec<BrowseItem>, Error> {
        self.library_grid(client, "FEmusic_liked_playlists").await
    }

    /// Saved albums grid (`FEmusic_liked_albums`). context/08. Needs login.
    pub async fn library_albums(&self, client: &YouTubeClient) -> Result<Vec<BrowseItem>, Error> {
        self.library_grid(client, "FEmusic_liked_albums").await
    }

    /// The albums the signed-in user uploaded themselves
    /// (`FEmusic_library_privately_owned_releases`). Cards come back as ordinary `MPREb_…` album
    /// browseIds, so they open on the album page like any other. context/08. Needs login.
    pub async fn upload_albums(&self, client: &YouTubeClient) -> Result<Vec<BrowseItem>, Error> {
        self.library_grid(client, "FEmusic_library_privately_owned_releases")
            .await
    }

    /// Library artists (`FEmusic_library_corpus_track_artists`) — the artists behind the songs and
    /// albums in your library, which is what YouTube Music's own Artists tab shows (subscriptions
    /// live under `FEmusic_library_corpus_artists`). context/08. Needs login.
    pub async fn library_artists(&self, client: &YouTubeClient) -> Result<Vec<BrowseItem>, Error> {
        self.library_grid(client, "FEmusic_library_corpus_track_artists")
            .await
    }

    /// The artists the signed-in user subscribes to (`FEmusic_library_corpus_artists`), YouTube
    /// Music's Library ▸ Artists ▸ Subscriptions. Same list rows as `library_artists`, but they
    /// link the channel itself rather than `MPLA` + it. Needs login.
    pub async fn library_subscriptions(
        &self,
        client: &YouTubeClient,
    ) -> Result<Vec<BrowseItem>, Error> {
        self.library_grid(client, "FEmusic_library_corpus_artists")
            .await
    }

    /// A playlist or album page by browseId (`VL…` / `MPRE…`). context/08.
    ///
    /// `sort` asks YouTube to order the tracks — see `PlaylistSort::params`. Passing `None` gets
    /// whatever order the account already has the list in, which is the one thing a fresh visit
    /// wants: it is what YouTube Music would show.
    pub async fn playlist(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
        sort: Option<(PlaylistSort, bool)>,
    ) -> Result<PlaylistPage, Error> {
        let params = sort.map(|(s, desc)| s.params(desc));
        let value = self.browse(client, Some(browse_id), params).await?;
        if let Some(show) = browse::podcast_show_id(browse_id, &value) {
            let value = self.browse(client, Some(&show), params).await?;
            return Ok(browse::parse_playlist(&value));
        }
        Ok(browse::parse_playlist(&value))
    }

    /// An album page by album browseId (`MPRE…`). context/08.
    ///
    /// Some album rows link the official music video (`musicVideoType` ≠ ATV), so playing them
    /// streams the MV's audio (intros, skits, crowd) instead of the album track. The album's
    /// `OLAK5uy_` *audio* playlist carries the album-audio uploads in track order, so for those
    /// rows we swap in its videoId (one extra fetch, only for affected albums).
    pub async fn album(&self, client: &YouTubeClient, browse_id: &str) -> Result<AlbumPage, Error> {
        let value = self.browse(client, Some(browse_id), None).await?;
        let mut page = browse::parse_album(&value);
        // Album track rows never carry the album's own browseId (live-checked 2026-09-02, every
        // release type), so without this every track played off a release page reaches the queue
        // with no `album_id` and the ⋮ menu offers no "Go to album". We are holding the id.
        for item in &mut page.items {
            item.album_id.get_or_insert_with(|| browse_id.to_owned());
        }
        let video = browse::album_video_flags(&value);
        if video.contains(&true) {
            if let Some(pl) = &page.playlist_id {
                match self.playlist(client, &format!("VL{pl}"), None).await {
                    // ponytail: positional match, guarded on equal track counts — the OLAK
                    // playlist is the same album in the same order. Mismatch → keep the MV ids.
                    Ok(audio) if audio.items.len() == page.items.len() => {
                        for ((item, is_video), audio_item) in
                            page.items.iter_mut().zip(&video).zip(audio.items)
                        {
                            if *is_video {
                                // The video stays reachable through the Song/Video switch.
                                item.counterpart = Some(std::mem::replace(
                                    &mut item.video_id,
                                    audio_item.video_id,
                                ));
                                item.duration = audio_item.duration.or(item.duration.take());
                                // The row linked the MV, but the id it now carries is the audio
                                // track: leaving the flag set would put the player view's video
                                // mode on a still album-art stream.
                                item.is_video = false;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "audio-playlist fetch failed; keeping MV ids")
                    }
                }
            }
        }
        for c in &mut page.sections {
            self.drop_video_cards(&mut c.items);
            self.drop_blocked_cards(&mut c.items);
            // YouTube lists the album you are already on under "Other versions". A card that
            // reopens the current page is noise, so drop it.
            c.items.retain(|i| i.id != browse_id);
        }
        page.sections.retain(|c| !c.items.is_empty());
        Ok(page)
    }

    /// An artist page by channel browseId (`UC…`). context/08.
    pub async fn artist(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<ArtistPage, Error> {
        let value = self.browse(client, Some(browse_id), None).await?;
        let mut page = browse::parse_artist(&value, browse_id);
        // `top_songs` deliberately keeps everything: filtering a blocked artist off their own
        // page is absurd, and the page is where you go to unblock them.
        self.drop_video_songs(&mut page.top_songs);
        for c in &mut page.sections {
            self.drop_video_cards(&mut c.items);
            self.drop_blocked_cards(&mut c.items);
        }
        page.sections.retain(|c| !c.items.is_empty()); // e.g. the artist's "Videos" carousel
        Ok(page)
    }

    /// An artist page's "Appears on" shelf (#456), read off `songs_id`, the page's
    /// `top_songs_id` playlist. See [`browse::appears_on`].
    pub async fn artist_appears_on(
        &self,
        client: &YouTubeClient,
        artist_id: &str,
        songs_id: &str,
    ) -> Result<Vec<BrowseItem>, Error> {
        let songs = self.playlist(client, songs_id, None).await?;
        let mut items = browse::appears_on(&songs.items, artist_id);
        self.drop_blocked_cards(&mut items);
        Ok(items)
    }

    /// A browse target that returns a grid of cards (e.g. an artist's "all albums" page reached
    /// via a carousel's "More" button). context/08.
    pub async fn browse_grid(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
        params: Option<&str>,
    ) -> Result<Vec<BrowseItem>, Error> {
        let value = self.browse(client, Some(browse_id), params).await?;
        let mut items = browse::parse_library(&value);
        // A mood category is several shelves flattened into one grid, and YouTube puts the same
        // playlist on more than one of them (Chill had 85 repeats). A repeat is fatal on the UI
        // side for the same reason as in `library_grid`: the grid is keyed, and one duplicate key
        // leaves the page on its skeleton forever. Issue #355.
        let mut seen = std::collections::HashSet::new();
        items.retain(|i| seen.insert(i.id.clone()));
        self.drop_video_cards(&mut items);
        self.drop_blocked_cards(&mut items);
        Ok(items)
    }

    /// Next page of playlist tracks via a continuation token. context/08.
    pub async fn playlist_continuation(
        &self,
        client: &YouTubeClient,
        token: &str,
    ) -> Result<PlaylistContinuation, Error> {
        let value = self.browse_continuation(client, token).await?;
        Ok(browse::parse_playlist_continuation(&value))
    }

    /// Suggestions for a playlist you own (#395). `token` is the page's `suggestions`, a previous
    /// batch's `refresh`, or `None` when the page offered no shelf.
    ///
    /// YouTube Music's own shelf first. It answers nothing at all for a playlist past 100 tracks
    /// (a Premium promo and no rows, checked live on a 122- and a 189-track one, 2026-10-11), so an
    /// empty shelf, or none, falls back to the playlist's radio. That barely repeats the playlist
    /// (5 of 49 rows on the 122-track one), and the caller drops those few. YouTube's picks, not
    /// rows the user asked for, so the blocklist and "hide music videos" apply to both.
    pub async fn playlist_suggestions(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        token: Option<&str>,
    ) -> Result<PlaylistSuggestions, Error> {
        let after = match token.map(|t| (t, t.strip_prefix(RADIO_MARK))) {
            Some((_, Some(video))) => Some(video).filter(|v| !v.is_empty()),
            Some((shelf, None)) => {
                let value = self.browse_continuation(client, shelf).await?;
                let mut batch = browse::parse_playlist_suggestions(&value);
                self.drop_video_songs(&mut batch.items);
                self.drop_blocked_songs(&mut batch.items, None);
                if !batch.items.is_empty() {
                    // A shelf with no Refresh of its own hands the next one to the radio.
                    batch.refresh.get_or_insert_with(|| RADIO_MARK.to_owned());
                    return Ok(batch);
                }
                None
            }
            None => None,
        };
        // The radio continues past a window the way autoplay extends one: ask again from its last
        // track (`extend_queue_radio`).
        let radio = format!(
            "RDAMPL{}",
            playlist_id.strip_prefix("VL").unwrap_or(playlist_id)
        );
        let next = self.next(client, after, Some(&radio)).await?;
        Ok(PlaylistSuggestions {
            refresh: next
                .items
                .last()
                .map(|s| format!("{RADIO_MARK}{}", s.video_id)),
            items: next.items,
        })
    }

    // --- lyrics (context/08 §lyrics; browseId comes from `next`) -----------------------------

    /// Line-synced lyrics. `client` must be a mobile identity (`LYRICS_TIMED_CLIENT`) — web
    /// clients never return `timedLyricsData`. Empty vec = track has no timed lyrics.
    pub async fn lyrics_timed(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<Vec<TimedLyricLine>, Error> {
        let value = self.browse(client, Some(browse_id), None).await?;
        Ok(lyrics::parse_lyrics_timed(&value))
    }

    /// Plain-text lyrics via WEB_REMIX (`musicDescriptionShelfRenderer`). `None` = none exist.
    pub async fn lyrics_plain(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<Option<PlainLyrics>, Error> {
        let value = self.browse(client, Some(browse_id), None).await?;
        Ok(lyrics::parse_lyrics_plain(&value))
    }

    // --- write actions (context/01 ✎, context/15 D7). All auth-gated (SAPISIDHASH). ---------

    /// Rate a video, or clear its rating. context/01. The three states are mutually exclusive on
    /// YouTube's side: disliking a liked track removes it from Liked Music in the same call.
    pub async fn rate(
        &self,
        client: &YouTubeClient,
        video_id: &str,
        rating: Rating,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct LikeBody {
            context: Context,
            target: Target,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Target {
            video_id: String,
        }
        let path = match rating {
            Rating::Like => "like/like",
            Rating::Dislike => "like/dislike",
            Rating::Indifferent => "like/removelike",
        };
        let body = LikeBody {
            context: self.context_for(client),
            target: Target {
                video_id: video_id.to_owned(),
            },
        };
        self.post(path, client, &body, true).await?;
        Ok(())
    }

    /// The raw `search` / `browse` responses, for callers that read the JSON with their own
    /// parsers (Venuta's catalog reader) and for the live smoke tests. A search sent this way is
    /// written to the account's search history only when the person typed it (`typed`) and
    /// "Signed-in search" is on. Venuta's own look-ups (a file's song, link matching, imports)
    /// pass `typed: false` and always go with no account at all, as everything does with the
    /// switch off (the default).
    pub async fn search_json(
        &self,
        client: &YouTubeClient,
        query: &str,
        params: Option<&str>,
        typed: bool,
    ) -> Result<serde_json::Value, Error> {
        self.search_raw(client, query, params, typed && self.signed_search())
            .await
    }

    /// The next page of a raw search, from the continuation token its last page gave. The query
    /// and filter go with it, as YouTube Music's own client sends them; as the account under the
    /// same rule as [`InnerTube::search_json`].
    pub async fn search_continuation_json(
        &self,
        client: &YouTubeClient,
        query: &str,
        params: Option<&str>,
        token: &str,
        typed: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ContinuationBody {
            context: Context,
            query: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            params: Option<String>,
        }
        let signed = typed && self.signed_search();
        let body = ContinuationBody {
            context: self.context_as(client, signed),
            query: query.to_owned(),
            params: params.map(str::to_owned),
        };
        let enc = urlencoding::encode(token);
        let path = format!("search?ctoken={enc}&continuation={enc}&type=next");
        self.post(&path, client, &body, signed).await
    }

    pub async fn browse_json(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<serde_json::Value, Error> {
        self.browse(client, Some(browse_id), None).await
    }

    /// The next page of a raw browse (a long playlist), from its continuation token.
    pub async fn browse_continuation_json(
        &self,
        client: &YouTubeClient,
        token: &str,
    ) -> Result<serde_json::Value, Error> {
        self.browse_continuation(client, token).await
    }

    /// Add a track to the library, or take it out: `youtubei/v1/feedback` with a token minted on
    /// the row itself ([`crate::models::LibraryToggle`]). Not the same thing as a like, which is
    /// what `rate` does: Library ▸ Songs and Liked Music are separate lists on the account.
    ///
    /// YouTube answers 200 with a per-token result rather than an HTTP error, so a token that has
    /// gone stale (the row was fetched long enough ago) surfaces here, not as a silent no-op.
    pub async fn feedback(&self, client: &YouTubeClient, token: &str) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FeedbackBody {
            context: Context,
            feedback_tokens: Vec<String>,
        }
        let body = FeedbackBody {
            context: self.context_for(client),
            feedback_tokens: vec![token.to_owned()],
        };
        let value = self.post("feedback", client, &body, true).await?;
        let processed = value
            .pointer("/feedbackResponses/0/isProcessed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if processed {
            Ok(())
        } else {
            Err(Error::Other(
                "YouTube turned down the library change — reopen the list and try again.".into(),
            ))
        }
    }

    /// Vote a song of a collaborative playlist up or down, or take the vote back: the token is the
    /// one the row's button sends in the state it is in ([`crate::PlaylistVote`]).
    pub async fn vote(&self, client: &YouTubeClient, token: &str) -> Result<(), Error> {
        self.row_feedback(client, token).await
    }

    /// Take one play out of the account's history, by the token its history row came with.
    /// ytmusicapi notes this doesn't work on a brand account: YouTube turns it down there.
    pub async fn remove_history_item(
        &self,
        client: &YouTubeClient,
        token: &str,
    ) -> Result<(), Error> {
        self.row_feedback(client, token).await
    }

    /// One row token to `youtubei/v1/feedback`, as the account and paced like the other account
    /// writes (a 429 waits as asked; see `InnerTube::post_account`). Going again is harmless: the
    /// token says what it does, so a second send leaves things as the first did. Taken only when
    /// the answer's per-token result says so; anything else is `Error::ActionRejected`.
    async fn row_feedback(&self, client: &YouTubeClient, token: &str) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FeedbackBody {
            context: Context,
            feedback_tokens: Vec<String>,
        }
        let body = FeedbackBody {
            context: self.context_for(client),
            feedback_tokens: vec![token.to_owned()],
        };
        let value = self.post_account("feedback", client, &body, true).await?;
        let processed = value
            .pointer("/feedbackResponses/0/isProcessed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if processed {
            Ok(())
        } else {
            Err(Error::ActionRejected)
        }
    }

    /// Save an album/playlist to the library, or remove it. Same `like` endpoint as a track, with
    /// a playlist target: for an album pass its `OLAK5uy_…` audio playlist id. Live-verified.
    pub async fn like_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        liked: bool,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct LikeBody {
            context: Context,
            target: Target,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Target {
            playlist_id: String,
        }
        let path = if liked {
            "like/like"
        } else {
            "like/removelike"
        };
        let body = LikeBody {
            context: self.context_for(client),
            target: Target {
                playlist_id: strip_vl(playlist_id).to_owned(),
            },
        };
        self.post(path, client, &body, true).await?;
        Ok(())
    }

    /// Add a video to a playlist. context/01 `browse/edit_playlist`.
    ///
    /// Returns `false` when the track is already in the playlist: YouTube refuses the add (see
    /// `edit_rejection`) rather than storing a second copy, unless `allow_duplicates` asks for one.
    pub async fn playlist_add(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_id: &str,
        allow_duplicates: bool,
    ) -> Result<bool, Error> {
        match self
            .edit_playlist_send(
                client,
                playlist_id,
                vec![add_video_action(video_id, allow_duplicates)],
                true,
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::AlreadyInPlaylist) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Add many videos in one `browse/edit_playlist` call, in order. The edit is all or nothing,
    /// and one track the playlist already holds fails the whole batch (`edit_rejection`), so the
    /// caller passes only videos it knows are new. Used by the Spotify import (#375).
    pub async fn playlist_add_many(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_ids: &[String],
    ) -> Result<(), Error> {
        let actions = video_ids
            .iter()
            .map(|v| add_video_action(v, false))
            .collect();
        self.edit_playlist_send(client, playlist_id, actions, true)
            .await
            .map(|_| ())
    }

    /// Remove a video from a playlist. Needs `set_video_id` (the item's playlistSetVideoId).
    pub async fn playlist_remove(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_id: &str,
        set_video_id: &str,
    ) -> Result<(), Error> {
        self.playlist_remove_many(
            client,
            playlist_id,
            &[(video_id.to_owned(), set_video_id.to_owned())],
        )
        .await
    }

    /// Remove several videos in one `edit_playlist` request: the endpoint takes an array of
    /// actions, so a bulk removal is one round trip rather than one per track.
    // ponytail: no chunking. YouTube has taken a few hundred actions in one body; split into
    // batches here if a very long selection ever comes back rejected.
    pub async fn playlist_remove_many(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        tracks: &[(String, String)],
    ) -> Result<(), Error> {
        let actions = tracks
            .iter()
            .map(|(video_id, set_video_id)| {
                serde_json::json!({
                    "action": "ACTION_REMOVE_VIDEO",
                    "setVideoId": set_video_id,
                    "removedVideoId": video_id,
                })
            })
            .collect();
        self.edit_playlist_actions(client, playlist_id, actions)
            .await
    }

    /// Store a sort order on a playlist you own, so every other client on the account shows the
    /// list the same way. context/01 `browse/edit_playlist`.
    ///
    /// Only meaningful where `SortMenu::editable` said so. A `browseEndpoint`-flavoured list has
    /// no equivalent write: Liked Music persists whatever page was last asked for, and someone
    /// else's playlist keeps nothing at all.
    pub async fn playlist_set_sort(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        sort: PlaylistSort,
    ) -> Result<(), Error> {
        self.edit_playlist(client, playlist_id, sort.edit_action())
            .await
    }

    /// Edit the details of a playlist you own. context/01 `browse/edit_playlist`.
    ///
    /// Every field is optional and only the ones given are sent, so an edit of the name cannot
    /// blank a description this parser failed to read back. `privacy` is YouTube's own vocabulary:
    /// `PUBLIC` / `PRIVATE` / `UNLISTED`.
    pub async fn playlist_edit_details(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        privacy: Option<&str>,
    ) -> Result<(), Error> {
        let mut actions = Vec::new();
        if let Some(name) = name {
            actions.push(
                serde_json::json!({ "action": "ACTION_SET_PLAYLIST_NAME", "playlistName": name }),
            );
        }
        if let Some(description) = description {
            actions.push(serde_json::json!({
                "action": "ACTION_SET_PLAYLIST_DESCRIPTION",
                "playlistDescription": description,
            }));
        }
        if let Some(privacy) = privacy {
            actions.push(
                serde_json::json!({ "action": "ACTION_SET_PLAYLIST_PRIVACY", "playlistPrivacy": privacy }),
            );
        }
        if actions.is_empty() {
            return Ok(());
        }
        self.edit_playlist_actions(client, playlist_id, actions)
            .await
    }

    /// Give a playlist you own a cover of your own, the way YouTube Music's web client does it:
    /// open a resumable ("Scotty") upload, send the whole image in one go, then attach the blob id
    /// that comes back. Two of the three calls are not InnerTube endpoints at all. context/01
    /// §custom playlist thumbnail.
    ///
    /// Signed in only, and YouTube treats the slot as square (`studio_square_thumbnail`): a photo
    /// that isn't gets cropped or refused at the far end, which the caller surfaces as-is.
    pub async fn playlist_set_cover(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        image: Vec<u8>,
    ) -> Result<(), Error> {
        // 1. Open the upload. The id comes back in a header; the body says nothing.
        let (headers, _) = self
            .post_upload(
                PLAYLIST_IMAGE_UPLOAD,
                client,
                &[
                    ("x-goog-upload-command", "start".to_owned()),
                    ("x-goog-upload-protocol", "resumable".to_owned()),
                    (
                        "x-goog-upload-header-content-length",
                        image.len().to_string(),
                    ),
                ],
                Vec::new(),
            )
            .await?;
        let upload_id = headers
            .get("x-guploader-uploadid")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Other("YouTube didn't open an upload for the artwork.".into()))?
            .to_owned();

        // 2. Send the bytes. "Resumable" in name only: one request, offset 0, finalized.
        let (_, body) = self
            .post_upload(
                &format!(
                    "{PLAYLIST_IMAGE_UPLOAD}?upload_id={}&upload_protocol=resumable",
                    urlencoding::encode(&upload_id)
                ),
                client,
                &[
                    ("x-goog-upload-command", "upload, finalize".to_owned()),
                    ("x-goog-upload-offset", "0".to_owned()),
                ],
                image,
            )
            .await?;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Uploaded {
            encrypted_blob_id: String,
        }
        let uploaded: Uploaded = serde_json::from_slice(&body)
            .map_err(|_| Error::Other("YouTube rejected the artwork upload.".into()))?;

        // 3. Attach the blob to the playlist. This one is an ordinary edit_playlist action.
        self.edit_playlist(
            client,
            playlist_id,
            serde_json::json!({
                "action": "ACTION_SET_CUSTOM_THUMBNAIL",
                "addedCustomThumbnail": {
                    "imageKey": custom_thumbnail_key(),
                    "playlistScottyEncryptedBlobId": uploaded.encrypted_blob_id,
                },
            }),
        )
        .await
        .map_err(cover_refusal)
    }

    /// Drop the custom cover again, so YouTube goes back to building one out of the tracks.
    /// Answers that rebuilt thumbnail: nothing here can guess the collage's URL.
    /// context/01 `browse/edit_playlist`.
    pub async fn playlist_clear_cover(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<Option<String>, Error> {
        let value = self
            .edit_playlist_value(
                client,
                playlist_id,
                vec![serde_json::json!({
                    "action": "ACTION_REMOVE_CUSTOM_THUMBNAIL",
                    "deletedCustomThumbnail": custom_thumbnail_key(),
                })],
            )
            .await
            .map_err(cover_refusal)?;
        Ok(edited_thumbnail(&value))
    }

    async fn edit_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        action: serde_json::Value,
    ) -> Result<(), Error> {
        self.edit_playlist_actions(client, playlist_id, vec![action])
            .await
    }

    async fn edit_playlist_actions(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        actions: Vec<serde_json::Value>,
    ) -> Result<(), Error> {
        self.edit_playlist_value(client, playlist_id, actions)
            .await
            .map(|_| ())
    }

    async fn edit_playlist_value(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        actions: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, Error> {
        self.edit_playlist_send(client, playlist_id, actions, false)
            .await
    }

    /// One `browse/edit_playlist`. `once`: it adds songs, so a lost answer is never sent again
    /// (it would fail as already there, or add twice); the rest (remove, move, rename) may go
    /// again. Either way a 429 waits as YouTube asks (see `InnerTube::post_account`).
    async fn edit_playlist_send(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        actions: Vec<serde_json::Value>,
        once: bool,
    ) -> Result<serde_json::Value, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct EditBody {
            context: Context,
            playlist_id: String,
            actions: Vec<serde_json::Value>,
        }
        let body = EditBody {
            context: self.context_for(client),
            playlist_id: strip_vl(playlist_id).to_owned(),
            actions,
        };
        let value = if once {
            self.post_write("browse/edit_playlist", client, &body, true)
                .await?
        } else {
            self.post_account("browse/edit_playlist", client, &body, true)
                .await?
        };
        match edit_rejection(&value) {
            Some(e) => Err(e),
            None => Ok(value),
        }
    }

    /// Create a private playlist; returns the new playlistId. context/01 `playlist/create`.
    pub async fn create_playlist(
        &self,
        client: &YouTubeClient,
        title: &str,
    ) -> Result<String, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CreateBody {
            context: Context,
            title: String,
            privacy_status: String,
        }
        let body = CreateBody {
            context: self.context_for(client),
            title: title.to_owned(),
            privacy_status: "PRIVATE".to_owned(),
        };
        // Never sent twice: a lost answer may still have made it (a second one would be a twin).
        let value = self
            .post_write("playlist/create", client, &body, true)
            .await?;
        metadata::find_first_str(&value, "playlistId")
            .ok_or_else(|| Error::Other("create_playlist: no playlistId in response".into()))
    }

    /// Delete a playlist you own. context/01 `playlist/delete`.
    pub async fn delete_playlist(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DeleteBody {
            context: Context,
            playlist_id: String,
        }
        let body = DeleteBody {
            context: self.context_for(client),
            playlist_id: strip_vl(playlist_id).to_owned(),
        };
        self.post_account("playlist/delete", client, &body, true)
            .await?;
        Ok(())
    }

    /// Subscribe / unsubscribe to a channel (artist). context/01 `subscription/*`.
    pub async fn subscribe(
        &self,
        client: &YouTubeClient,
        channel_id: &str,
        subscribed: bool,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct SubBody {
            context: Context,
            channel_ids: Vec<String>,
        }
        let path = if subscribed {
            "subscription/subscribe"
        } else {
            "subscription/unsubscribe"
        };
        let body = SubBody {
            context: self.context_for(client),
            channel_ids: vec![channel_id.to_owned()],
        };
        self.post(path, client, &body, true).await?;
        Ok(())
    }

    // --- The rest of the account: songs, uploads, podcasts, edits, collaboration -------------

    /// The songs saved to the library (`FEmusic_liked_videos`) or uploaded by the account
    /// (`FEmusic_library_privately_owned_tracks`): list rows, read page after page, each song once.
    /// Needs login.
    pub async fn library_songs(
        &self,
        client: &YouTubeClient,
        browse_id: &str,
    ) -> Result<Vec<SongItem>, Error> {
        let value = self.browse(client, Some(browse_id), None).await?;
        let mut seen = std::collections::HashSet::new();
        let mut songs = Vec::new();
        let mut keep = |rows: Vec<SongItem>, songs: &mut Vec<SongItem>| {
            for row in rows {
                if !row.video_id.is_empty() && seen.insert(row.video_id.clone()) {
                    songs.push(row);
                }
            }
        };
        keep(browse::song_rows(&value), &mut songs);
        let mut token = browse::continuation_token(&value);
        // ponytail: 25 a page, so 400 pages is 10,000 songs, YouTube's own cap on a library list.
        for _ in 0..400 {
            let Some(t) = token else { break };
            let Ok(page) = self.browse_continuation(client, &t).await.inspect_err(|e| {
                tracing::warn!(error = %e, browse_id, "song list continuation failed; keeping what loaded")
            }) else {
                break;
            };
            let rows = browse::song_rows(&page);
            if rows.is_empty() {
                break;
            }
            keep(rows, &mut songs);
            token = browse::continuation_token(&page).filter(|next| *next != t);
        }
        Ok(songs)
    }

    /// The podcasts the account follows (`FEmusic_library_non_music_audio_list`). Needs login.
    pub async fn library_podcasts(&self, client: &YouTubeClient) -> Result<Vec<BrowseItem>, Error> {
        self.library_grid(client, "FEmusic_library_non_music_audio_list")
            .await
    }

    /// Add videos in one edit and say where each landed: its `(videoId, setVideoId)`, the handle a
    /// later move or removal needs, from `playlistEditResults`. All or nothing, like
    /// `playlist_add_many`; a video the playlist holds already fails the batch.
    pub async fn playlist_add_placed(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_ids: &[String],
    ) -> Result<Vec<(String, String)>, Error> {
        let actions = video_ids
            .iter()
            .map(|v| add_video_action(v, false))
            .collect();
        let value = self
            .edit_playlist_send(client, playlist_id, actions, true)
            .await?;
        Ok(added_placements(&value))
    }

    /// Add many songs, each once, 50 to an edit with a pause between (a 2,000-song playlist is
    /// 40 edits, not 40 at once). Where each landed, as `playlist_add_placed` says. Stopped part
    /// way, what went before stays there and the error comes back; the caller reads again.
    pub async fn playlist_add_batches(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        video_ids: &[String],
    ) -> Result<Vec<(String, String)>, Error> {
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<String> = video_ids
            .iter()
            .filter(|v| seen.insert(*v))
            .cloned()
            .collect();
        let mut placed = Vec::new();
        for (n, batch) in unique.chunks(ADD_BATCH).enumerate() {
            if n > 0 {
                tokio::time::sleep(ADD_BATCH_PACE).await;
            }
            placed.extend(self.playlist_add_placed(client, playlist_id, batch).await?);
        }
        Ok(placed)
    }

    /// Move one item (by its `setVideoId`) to just before another, or to the end with `None`.
    /// context/01 `ACTION_MOVE_VIDEO_BEFORE`.
    pub async fn playlist_move(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        set_video_id: &str,
        before: Option<&str>,
    ) -> Result<(), Error> {
        self.edit_playlist(client, playlist_id, move_action(set_video_id, before))
            .await
    }

    /// Create a playlist with its words, privacy (`PRIVATE`, `UNLISTED`, `PUBLIC`) and first
    /// songs in one call; returns the new playlistId. context/01 `playlist/create`.
    pub async fn create_playlist_with(
        &self,
        client: &YouTubeClient,
        title: &str,
        description: Option<&str>,
        privacy: &str,
        video_ids: &[String],
    ) -> Result<String, Error> {
        let body = create_body(
            self.context_for(client),
            title,
            description,
            privacy,
            video_ids,
        );
        // Never sent twice: a lost answer may still have made it (a second one would be a twin).
        let value = self
            .post_write("playlist/create", client, &body, true)
            .await?;
        metadata::find_first_str(&value, "playlistId")
            .ok_or_else(|| Error::Other("create_playlist: no playlistId in response".into()))
    }

    /// Open a playlist you own to collaborators: the invite link anyone signed in can join with.
    pub async fn collaboration_invite(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
    ) -> Result<String, Error> {
        let value = self
            .edit_playlist_value(
                client,
                playlist_id,
                vec![serde_json::json!({ "action": "ACTION_CREATE_COLLABORATION_INVITE_LINK" })],
            )
            .await?;
        metadata::find_first_str(&value, "collaborationInviteLink")
            .ok_or_else(|| Error::Other("no invite link in the answer".into()))
    }

    /// Stop (or allow again) new collaborators joining a playlist you own.
    pub async fn collaboration_closed(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        closed: bool,
    ) -> Result<(), Error> {
        self.edit_playlist(
            client,
            playlist_id,
            serde_json::json!({ "action": "ACTION_SET_CLOSED_TO_CONTRIBUTIONS", "closedToContributions": closed }),
        )
        .await
    }

    /// Join someone's collaborative playlist with the `jct` token of its invite link.
    pub async fn collaboration_join(
        &self,
        client: &YouTubeClient,
        playlist_id: &str,
        token: &str,
    ) -> Result<(), Error> {
        self.edit_playlist(
            client,
            playlist_id,
            serde_json::json!({ "action": "ACTION_JOIN_COLLABORATION", "joinCollaborationToken": token }),
        )
        .await
    }

    /// Delete one of the account's uploads (a song or an album) by its entity id.
    pub async fn delete_upload(
        &self,
        client: &YouTubeClient,
        entity_id: &str,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DeleteBody {
            context: Context,
            entity_id: String,
        }
        let body = DeleteBody {
            context: self.context_for(client),
            entity_id: entity_id.to_owned(),
        };
        self.post("music/delete_privately_owned_entity", client, &body, true)
            .await?;
        Ok(())
    }

    /// Tell the account a song was played, as YouTube Music's own app does: the signed-in
    /// `/player` answer's `videostatsPlaybackUrl`, fetched once with a fresh playback nonce.
    /// That is what puts the song in the account's history and lets its recommendations learn.
    pub async fn report_play(&self, client: &YouTubeClient, video_id: &str) -> Result<(), Error> {
        let listen = self.listen_tracking(client, video_id).await?;
        self.report_tracked_play(client, &listen).await
    }

    /// One listen of a song, ready to be told: the signed-in `/player` answer's tracking
    /// addresses (the play's and the listening time's) with a fresh playback nonce, which every
    /// report of this listen shares, as YouTube's own player does. Asked once per listen; nothing
    /// is sent by asking. Signed out there is no listen to tell.
    pub async fn listen_tracking(
        &self,
        client: &YouTubeClient,
        video_id: &str,
    ) -> Result<ListenTracking, Error> {
        if !self.is_logged_in() {
            return Err(Error::Other("signed out: no listen to tell".into()));
        }
        let player = self.player(client, video_id, None, None, None).await?;
        let tracking = player.playback_tracking;
        let url = |pick: fn(
            &crate::models::player::PlaybackTracking,
        ) -> Option<&crate::models::player::BaseUrl>| {
            tracking
                .as_ref()
                .and_then(pick)
                .and_then(|u| u.base_url.clone())
                .filter(|u| !u.trim().is_empty())
        };
        Ok(ListenTracking {
            playback_url: url(|t| t.videostats_playback_url.as_ref()),
            watchtime_url: url(|t| t.videostats_watchtime_url.as_ref()),
            cpn: crate::transport::generate_cpn(),
        })
    }

    /// Tell the account the listen was a play (its history), through the listen's own address.
    pub async fn report_tracked_play(
        &self,
        client: &YouTubeClient,
        listen: &ListenTracking,
    ) -> Result<(), Error> {
        let url = listen
            .playback_url
            .as_deref()
            .ok_or_else(|| Error::Other("no playback address to report the play to".into()))?;
        self.register_playback(client, url, &listen.cpn, None).await
    }

    /// Tell the account how much of the listen was heard since the last report.
    pub async fn report_watchtime(
        &self,
        client: &YouTubeClient,
        listen: &ListenTracking,
        heard: &crate::transport::Watched,
    ) -> Result<(), Error> {
        let url = listen
            .watchtime_url
            .as_deref()
            .ok_or_else(|| Error::Other("no listening-time address to report to".into()))?;
        self.ping_watchtime(client, url, &listen.cpn, heard).await
    }
}

/// One listen of a song as YouTube's player tracks it ([`InnerTube::listen_tracking`]). The
/// addresses carry the session's own ids, so they never appear in a log line (`Debug` leaves
/// them out).
#[derive(Clone)]
pub struct ListenTracking {
    playback_url: Option<String>,
    watchtime_url: Option<String>,
    cpn: String,
}

impl ListenTracking {
    /// The answer gave a listening-time address.
    pub fn has_watchtime(&self) -> bool {
        self.watchtime_url.is_some()
    }
}

impl std::fmt::Debug for ListenTracking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenTracking")
            .field("playback", &self.playback_url.is_some())
            .field("watchtime", &self.watchtime_url.is_some())
            .finish_non_exhaustive()
    }
}

/// Starts a `PlaylistSuggestions::refresh` that continues the playlist's radio rather than YouTube's
/// own shelf, followed by the track the next window starts from (nothing: from the top). A shelf
/// token is base64, so it can never start with this.
const RADIO_MARK: &str = "radio:";

/// Where custom playlist artwork goes up. Not a `youtubei/v1` endpoint: it is Google's generic
/// resumable uploader, sitting on `music.youtube.com` under its own path. context/01.
const PLAYLIST_IMAGE_UPLOAD: &str = "playlist_image_upload/playlist_custom_thumbnail";

/// YouTube saying no to a custom playlist image, as opposed to the network saying nothing at all.
///
/// The gate is phone verification, and it is invisible: the account uploads the bytes fine (the
/// credential is clearly good, the uploader took it), and then the attach comes back 4xx with
/// nothing in the body naming a reason. A 403 in particular must not reach the user as our
/// "session expired, sign in again", which is what the transport makes of one everywhere else.
/// Timeouts and connect failures are left alone: those really are try-again.
fn cover_refusal(e: Error) -> Error {
    match &e {
        Error::SessionExpired | Error::Other(_) => Error::CoverRefused,
        Error::Http(h) if h.status().is_some() => Error::CoverRefused,
        _ => e,
    }
}

/// The playlist's thumbnail as the edit left it, off the header YouTube echoes back. Both cover
/// actions answer with one, and after a removal it is the only way to learn the collage YouTube
/// rebuilt out of the tracks. Scoped to `newHeader` so an unrelated avatar can't stand in for it.
fn edited_thumbnail(response: &serde_json::Value) -> Option<String> {
    metadata::find_all(response, "newHeader")
        .into_iter()
        .find_map(metadata::last_thumbnail)
}

/// The single image slot a playlist has. Square by name and by what YouTube does with it.
fn custom_thumbnail_key() -> serde_json::Value {
    serde_json::json!({
        "name": "studio_square_thumbnail",
        "type": "PLAYLIST_IMAGE_TYPE_CUSTOM_THUMBNAIL",
    })
}

/// What a comment action's or write's answer says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Succeeded,
    /// A status that is not `STATUS_SUCCEEDED`, or a top-level `error`.
    Rejected,
    /// A 200 with no status to read.
    Missing,
}

/// Read an answer to a comment action or write: `STATUS_SUCCEEDED` in `actionResults[0]` (a vote)
/// or `actionResult` (an edit) succeeds; another status or a top-level `error` is a rejection;
/// none is `Missing`, for the caller to decide. Also the status, as it may be logged.
fn answer_verdict(value: &serde_json::Value) -> (Verdict, &str) {
    let result = value
        .pointer("/actionResults/0")
        .or_else(|| value.get("actionResult"));
    let status = result
        .and_then(|r| r.get("status"))
        .and_then(serde_json::Value::as_str);
    let verdict = match status {
        _ if value.get("error").is_some() => Verdict::Rejected,
        Some("STATUS_SUCCEEDED") => Verdict::Succeeded,
        Some(_) => Verdict::Rejected,
        None => Verdict::Missing,
    };
    (
        verdict,
        status.map_or("missing", |s| enum_like(s).unwrap_or("unexpected")),
    )
}

/// The answer, unless it is a rejection. `missing_ok`: an answer with no status is accepted.
fn accept_answer(
    kind: &'static str,
    value: serde_json::Value,
    missing_ok: bool,
) -> Result<serde_json::Value, Error> {
    match answer_verdict(&value) {
        (Verdict::Succeeded, _) => Ok(value),
        (Verdict::Missing, _) if missing_ok => Ok(value),
        (_, status) => {
            tracing::debug!(kind, status, "comment action rejected");
            Err(Error::ActionRejected)
        }
    }
}

/// A 401/403: YouTube said no to this request.
fn is_refusal(e: &Error) -> bool {
    matches!(e, Error::Http(h) if h.status().is_some_and(|s| s == 401 || s == 403))
}

// Playlist edit/delete want the raw playlistId; browse gives it `VL`-prefixed. context/01.
fn strip_vl(id: &str) -> &str {
    id.strip_prefix("VL").unwrap_or(id)
}

/// The tuner setting YouTube Music's web app sends with every queue request.
const TUNER_SETTING: &str = "AUTOMIX_SETTING_NORMAL";

/// A `next` body without its context: the fields the web app sends for music
/// (`enablePersistentPlaylistPanel`, `isAudioOnly`, `tunerSettingValue`, as ytmusicapi's
/// `get_watch_playlist` does), then the endpoint's own fields as they came. A song asked for with
/// no params (one song played, not a radio or a shuffle) also carries the play settings ytmusicapi
/// sends for it. `continuation` asks for the page after the one that issued it, with the same
/// request around it (as Metrolist's `next(endpoint, continuation)` does).
pub(crate) fn watch_body(
    endpoint: &WatchEndpoint,
    continuation: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::Value;
    let mut body = serde_json::Map::new();
    body.insert("enablePersistentPlaylistPanel".into(), Value::Bool(true));
    body.insert("isAudioOnly".into(), Value::Bool(true));
    body.insert("tunerSettingValue".into(), TUNER_SETTING.into());
    if let Some(v) = &endpoint.video_id {
        body.insert("videoId".into(), v.as_str().into());
        if endpoint.params.is_none() {
            body.insert(
                "watchEndpointMusicSupportedConfigs".into(),
                serde_json::json!({ "watchEndpointMusicConfig": {
                    "hasPersistentPlaylistPanel": true,
                    "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
                } }),
            );
        }
    }
    if let Some(p) = &endpoint.playlist_id {
        body.insert("playlistId".into(), strip_vl(p).into());
    }
    if let Some(p) = &endpoint.params {
        body.insert("params".into(), p.as_str().into());
    }
    if let Some(i) = endpoint.index {
        body.insert("index".into(), i.into());
    }
    if let Some(s) = &endpoint.playlist_set_video_id {
        body.insert("playlistSetVideoId".into(), s.as_str().into());
    }
    if let Some(c) = continuation {
        body.insert("continuation".into(), c.into());
    }
    body
}

/// `DEDUPE_OPTION_SKIP` skips YouTube's duplicate check: it is what the refusal's own "Add anyway"
/// button sends (the capture in `duplicate_add_is_rejected`). Sent on an ordinary add, it stores a
/// second copy instead of refusing.
/// Songs to one add edit: one already there fails all of them, so batches stay small.
const ADD_BATCH: usize = 50;

/// The pause between two add edits of one change.
pub const ADD_BATCH_PACE: std::time::Duration = std::time::Duration::from_secs(1);

fn add_video_action(video_id: &str, allow_duplicates: bool) -> serde_json::Value {
    let mut action = serde_json::json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": video_id });
    if allow_duplicates {
        action["dedupeOption"] = "DEDUPE_OPTION_SKIP".into();
    }
    action
}

/// Move an item before another (`movedSetVideoIdSuccessor`), or to the end without one.
fn move_action(set_video_id: &str, before: Option<&str>) -> serde_json::Value {
    let mut action =
        serde_json::json!({ "action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": set_video_id });
    if let Some(b) = before {
        action["movedSetVideoIdSuccessor"] = b.into();
    }
    action
}

/// The `playlist/create` body: words, privacy, and the first songs when there are any.
fn create_body(
    context: Context,
    title: &str,
    description: Option<&str>,
    privacy: &str,
    video_ids: &[String],
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "context": context,
        "title": title,
        "privacyStatus": privacy,
    });
    if let Some(d) = description.filter(|d| !d.trim().is_empty()) {
        body["description"] = d.into();
    }
    if !video_ids.is_empty() {
        body["videoIds"] = video_ids.into();
    }
    body
}

/// Where each added video landed, `(videoId, setVideoId)`, from an edit's `playlistEditResults`.
fn added_placements(value: &serde_json::Value) -> Vec<(String, String)> {
    metadata::find_all(value, "playlistEditVideoAddedResultData")
        .into_iter()
        .filter_map(|r| {
            Some((
                r.get("videoId")?.as_str()?.to_owned(),
                r.get("setVideoId")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// The `jct` token in a collaboration invite link, the part `collaboration_join` takes.
pub fn invite_token(link: &str) -> Option<String> {
    let query = link.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "jct").then(|| {
            urlencoding::decode(v)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| v.to_owned())
        })
    })
}

/// The playlist id in a collaboration invite link (`list=`).
pub fn invite_playlist(link: &str) -> Option<String> {
    let query = link.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "list").then(|| v.to_owned())
    })
}

/// `browse/edit_playlist` answers HTTP 200 even when it applies nothing: the refusal is
/// `"status": "STATUS_FAILED"` in the body. Adding a track the playlist already holds is the
/// common one, and YouTube marks it by offering an "Add anyway" button whose endpoint repeats the
/// action with a `dedupeOption`. Left unread, the caller thinks the edit landed.
fn edit_rejection(v: &serde_json::Value) -> Option<Error> {
    if v.get("status").and_then(serde_json::Value::as_str) != Some("STATUS_FAILED") {
        return None;
    }
    Some(match metadata::find_first_str(v, "dedupeOption") {
        Some(_) => Error::AlreadyInPlaylist,
        None => Error::Other("YouTube refused the playlist edit.".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn add_video_action_json() {
        assert_eq!(
            add_video_action("dQw4w9WgXcQ", false),
            json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": "dQw4w9WgXcQ" })
        );
        assert_eq!(
            add_video_action("dQw4w9WgXcQ", true),
            json!({
                "action": "ACTION_ADD_VIDEO",
                "addedVideoId": "dQw4w9WgXcQ",
                "dedupeOption": "DEDUPE_OPTION_SKIP"
            })
        );
    }

    // What the web app (and ytmusicapi) sends to `next` for music, around the endpoint itself.
    #[test]
    fn next_asks_like_the_web_app() {
        // A song's radio: the radio's params, so no play-settings block.
        let body = watch_body(&WatchEndpoint::song_radio("vid00000001"), None);
        assert_eq!(
            serde_json::Value::Object(body),
            json!({
                "enablePersistentPlaylistPanel": true,
                "isAudioOnly": true,
                "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
                "videoId": "vid00000001",
                "playlistId": "RDAMVMvid00000001",
                "params": "wAEB"
            })
        );

        // One song played (no params): YouTube's play settings for it, as ytmusicapi sends them.
        let plain = WatchEndpoint {
            video_id: Some("vid00000001".into()),
            ..Default::default()
        };
        let body = serde_json::Value::Object(watch_body(&plain, None));
        assert_eq!(
            body["watchEndpointMusicSupportedConfigs"],
            json!({ "watchEndpointMusicConfig": {
                "hasPersistentPlaylistPanel": true,
                "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
            } })
        );
        assert!(body.get("playlistId").is_none(), "nothing built for it");

        // A server-sent endpoint goes out exactly (a `VL` browse id is a playlist id with a
        // prefix); the next page is the same request with its token.
        let chip = WatchEndpoint {
            video_id: Some("vid00000001".into()),
            playlist_id: Some("VLRDATiYvvid00000001".into()),
            params: Some("wAEB8gECKAE%3D".into()),
            index: Some(3),
            playlist_set_video_id: Some("SETID".into()),
        };
        let body = serde_json::Value::Object(watch_body(&chip, Some("TOKEN2")));
        assert_eq!(body["playlistId"], "RDATiYvvid00000001");
        assert_eq!(body["params"], "wAEB8gECKAE%3D");
        assert_eq!(body["index"], 3);
        assert_eq!(body["playlistSetVideoId"], "SETID");
        assert_eq!(body["continuation"], "TOKEN2");
        assert!(body.get("watchEndpointMusicSupportedConfigs").is_none());
    }

    #[test]
    fn strips_vl_prefix() {
        assert_eq!(strip_vl("VLPL123"), "PL123");
        assert_eq!(strip_vl("PL123"), "PL123");
    }

    /// The image bytes were accepted moments earlier, so the credential is good: a refusal on the
    /// attach is the account not being allowed a custom playlist image. Above all it must not come
    /// out as "your session expired", which is what a 403 means anywhere else in this crate.
    #[test]
    fn a_refused_cover_never_reads_as_a_dead_session() {
        assert!(matches!(
            cover_refusal(Error::SessionExpired),
            Error::CoverRefused
        ));
        // `edit_playlist`'s STATUS_FAILED rejection, which is how a 200 says no.
        assert!(matches!(
            cover_refusal(Error::Other("YouTube refused the playlist edit.".into())),
            Error::CoverRefused
        ));
        // Nothing reached YouTube at all: still worth a retry, so leave it be.
        assert!(matches!(
            cover_refusal(Error::VisitorDataNotFound),
            Error::VisitorDataNotFound
        ));
    }

    // Both bodies are trimmed captures of the live responses.
    #[test]
    fn duplicate_add_is_rejected() {
        let ok = json!({ "playlistEditResults": [{ "playlistEditVideoAddedResultData": {
            "setVideoId": "56B44F6D10557CC6", "videoId": "dQw4w9WgXcQ" } }],
            "status": "STATUS_SUCCEEDED" });
        assert!(edit_rejection(&ok).is_none());

        let dup = json!({ "actions": [{ "addToToastAction": { "item": {
            "notificationActionRenderer": { "actionButton": { "buttonRenderer": {
                "command": { "playlistEditEndpoint": { "actions": [{
                    "action": "ACTION_ADD_VIDEO",
                    "addedVideoId": "dQw4w9WgXcQ",
                    "dedupeOption": "DEDUPE_OPTION_SKIP" }] } },
                "text": { "runs": [{ "text": "Add anyway" }] } } },
            "responseText": { "runs": [{ "text": "This track is already in the playlist" }] } } } } }],
            "status": "STATUS_FAILED" });
        assert!(matches!(
            edit_rejection(&dup),
            Some(Error::AlreadyInPlaylist)
        ));

        let failed = json!({ "status": "STATUS_FAILED" });
        assert!(matches!(edit_rejection(&failed), Some(Error::Other(_))));
    }

    #[test]
    fn a_move_names_the_item_and_what_it_goes_before() {
        assert_eq!(
            move_action("set-b", Some("set-a")),
            json!({ "action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": "set-b", "movedSetVideoIdSuccessor": "set-a" })
        );
        // To the end: no successor at all.
        assert_eq!(
            move_action("set-b", None),
            json!({ "action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": "set-b" })
        );
    }

    #[test]
    fn a_new_playlist_carries_its_words_privacy_and_songs_only_when_given() {
        let ctx = InnerTube::new(Session::default(), None)
            .unwrap()
            .context_for(&web());
        let full = create_body(
            ctx.clone(),
            "Road",
            Some("For the car"),
            "UNLISTED",
            &["a".into(), "b".into()],
        );
        assert_eq!(full["title"], "Road");
        assert_eq!(full["privacyStatus"], "UNLISTED");
        assert_eq!(full["description"], "For the car");
        assert_eq!(full["videoIds"], json!(["a", "b"]));
        let bare = create_body(ctx, "Road", Some("  "), "PRIVATE", &[]);
        assert!(bare.get("description").is_none());
        assert!(bare.get("videoIds").is_none());
    }

    #[test]
    fn an_add_says_where_each_song_landed() {
        let resp = json!({ "status": "STATUS_SUCCEEDED", "playlistEditResults": [
            { "playlistEditVideoAddedResultData": { "videoId": "v1", "setVideoId": "s1" } },
            { "playlistEditVideoAddedResultData": { "videoId": "v2", "setVideoId": "s2" } }
        ] });
        assert_eq!(
            added_placements(&resp),
            vec![("v1".into(), "s1".into()), ("v2".into(), "s2".into())]
        );
    }

    #[test]
    fn an_invite_link_gives_its_playlist_and_token() {
        let link = "https://music.youtube.com/playlist?list=PLabc&jct=tok%3D123";
        assert_eq!(invite_playlist(link).as_deref(), Some("PLabc"));
        assert_eq!(invite_token(link).as_deref(), Some("tok=123"));
        assert_eq!(
            invite_token("https://music.youtube.com/playlist?list=PLabc"),
            None
        );
    }

    #[test]
    fn create_playlist_id_parsed() {
        let resp = json!({ "playlistId": "PLnew123", "status": "STATUS_SUCCEEDED" });
        assert_eq!(
            metadata::find_first_str(&resp, "playlistId").as_deref(),
            Some("PLnew123")
        );
    }

    // --- comments reads: as the account without the healer, never a token across the line ---

    use crate::clients::{Clients, METADATA_CLIENT};
    use crate::models::comment_write::{REPLY_EDIT_TEXT_FIELD, TEXT_FIELD};
    use crate::models::comments::CommentsState;
    use crate::test_server::{MockServer, Seen};
    use crate::transport::Session;
    use std::time::Duration;

    fn it_against(server: &MockServer, signed_in: bool) -> InnerTube {
        let session = Session {
            cookie: signed_in.then(|| "SAPISID=fixture-sapisid; PREF=x".to_owned()),
            data_sync_id: signed_in.then(|| "fixture-dsid".to_owned()),
            visitor_data: Some("fixture-visitor".into()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        it
    }

    fn web() -> YouTubeClient {
        Clients::bundled().get(METADATA_CLIENT).unwrap().clone()
    }

    fn as_account(req: &Seen) -> bool {
        req.header("cookie").is_some()
            || req.header("authorization").is_some()
            || req.body.contains("onBehalfOfUser")
    }

    fn is_tab_lookup(req: &Seen) -> bool {
        req.body.contains("\"videoId\"")
    }

    /// A tab-lookup answer whose comments token records who it was issued to.
    fn tab_answer(req: &Seen) -> String {
        let issued_to = if as_account(req) {
            "tok-account"
        } else {
            "tok-anon"
        };
        json!({ "tabs": [{ "tabRenderer": { "content": { "sectionListRenderer": {
            "continuations": [{ "reloadContinuationData": { "continuation": issued_to } }] } } } }] })
        .to_string()
    }

    /// A first page that has a header and no threads: parses as `Empty`.
    const PAGE: &str = r#"{"x":{"continuationItems":[{"commentsHeaderRenderer":{}}]}}"#;

    async fn no_heal(it: &InnerTube) {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), it.session_rejected().notified())
                .await
                .is_err(),
            "a comments request must never wake the healer"
        );
        assert!(it.is_logged_in(), "and the session is untouched");
    }

    /// A refusal comes back as the HTTP error it was, never as an expired session.
    fn is_plain_refusal(e: &Error) -> bool {
        is_refusal(e) && !matches!(e, Error::SessionExpired)
    }

    #[tokio::test]
    async fn a_signed_in_first_page_is_read_as_the_account_and_says_so() {
        let server = MockServer::start(|req| {
            (
                200,
                if is_tab_lookup(req) {
                    tab_answer(req)
                } else {
                    PAGE.to_owned()
                },
            )
        });
        let it = it_against(&server, true);
        let page = it.comments(&web(), "vid").await.unwrap();
        assert!(page.read_as_account);
        assert_eq!(page.state, CommentsState::Empty);
        let sent = server.requests();
        assert_eq!(sent.len(), 2, "tab lookup, first page");
        assert!(sent.iter().all(as_account));
        assert!(sent.iter().all(|r| r.path.starts_with("/youtubei/v1/next")));
        assert!(sent[0]
            .header("authorization")
            .is_some_and(|a| a.starts_with("SAPISIDHASH ")));
        assert!(
            sent[1].body.contains("tok-account"),
            "the token it was issued"
        );
        no_heal(&it).await;
    }

    /// The first page is refused: the whole chain restarts anonymously, with a FRESH tab lookup,
    /// and the account's token is never sent without the account.
    #[tokio::test]
    async fn a_refused_first_page_redoes_the_whole_chain_anonymously() {
        for status in [401, 403] {
            let server = MockServer::start(move |req| {
                if is_tab_lookup(req) {
                    (200, tab_answer(req))
                } else if as_account(req) {
                    (status, "{}".into())
                } else if req.body.contains("tok-anon") {
                    (200, PAGE.to_owned())
                } else {
                    (400, "an account's token in an anonymous request".into())
                }
            });
            let it = it_against(&server, true);
            let page = it.comments(&web(), "vid").await.unwrap();
            assert!(!page.read_as_account);
            assert_eq!(page.state, CommentsState::Empty);

            let sent = server.requests();
            let shape: Vec<(bool, bool)> = sent
                .iter()
                .map(|r| (is_tab_lookup(r), as_account(r)))
                .collect();
            assert_eq!(
                shape,
                [(true, true), (false, true), (true, false), (false, false)],
                "account tab, refused page, then a fresh anonymous tab and page"
            );
            assert!(
                sent.iter()
                    .filter(|r| !as_account(r))
                    .all(|r| !r.body.contains("tok-account")),
                "an account's token was replayed anonymously"
            );
            no_heal(&it).await;
        }
    }

    /// The tab lookup itself is refused: the chain restarts anonymously from the same place.
    #[tokio::test]
    async fn a_refused_tab_lookup_falls_back_anonymously() {
        let server = MockServer::start(|req| {
            if is_tab_lookup(req) && as_account(req) {
                (403, "{}".into())
            } else if is_tab_lookup(req) {
                (200, tab_answer(req))
            } else {
                (200, PAGE.to_owned())
            }
        });
        let it = it_against(&server, true);
        let page = it.comments(&web(), "vid").await.unwrap();
        assert!(!page.read_as_account);
        let sent = server.requests();
        assert_eq!(sent.len(), 3, "refused tab, anonymous tab, anonymous page");
        assert!(!as_account(&sent[1]) && !as_account(&sent[2]));
        no_heal(&it).await;
    }

    /// Only a refusal restarts the chain: a 500 would just repeat itself.
    #[tokio::test]
    async fn other_failures_do_not_restart_the_chain() {
        let server = MockServer::start(|_| (500, "{}".into()));
        let it = it_against(&server, true);
        assert!(matches!(
            it.comments(&web(), "vid").await,
            Err(Error::Http(_))
        ));
        assert_eq!(server.requests().len(), 1);
        no_heal(&it).await;
    }

    #[tokio::test]
    async fn a_refusal_on_both_sides_is_a_plain_error_not_a_session_one() {
        let server = MockServer::start(|_| (403, "{}".into()));
        let it = it_against(&server, true);
        let err = it.comments(&web(), "vid").await.unwrap_err();
        assert!(is_plain_refusal(&err), "{err:?}");
        assert_eq!(
            server.requests().len(),
            2,
            "one account attempt, one anonymous"
        );
        no_heal(&it).await;
    }

    #[tokio::test]
    async fn signed_out_is_one_anonymous_chain() {
        let server = MockServer::start(|req| {
            (
                200,
                if is_tab_lookup(req) {
                    tab_answer(req)
                } else {
                    PAGE.to_owned()
                },
            )
        });
        let it = it_against(&server, false);
        let page = it.comments(&web(), "vid").await.unwrap();
        assert!(!page.read_as_account);
        let sent = server.requests();
        assert_eq!(sent.len(), 2);
        assert!(!sent.iter().any(as_account));
    }

    /// A sort switch, a page and a replies page carry tokens issued earlier: refused, they come
    /// back as a plain error with exactly one request, never as an anonymous retry.
    #[tokio::test]
    async fn a_refused_continuation_or_replies_page_is_not_retried_anonymously() {
        for status in [401, 403] {
            let server = MockServer::start(move |_| (status, "{}".into()));
            let it = it_against(&server, true);
            let page = it.comments_continuation(&web(), "tok-account", true).await;
            let replies = it.comment_replies(&web(), "tok-account", true).await;
            for err in [page.unwrap_err(), replies.unwrap_err()] {
                assert!(is_plain_refusal(&err), "{err:?}");
            }
            assert_eq!(server.requests().len(), 2, "one request each, no retry");
            assert!(server.requests().iter().all(as_account));
            no_heal(&it).await;
        }
    }

    /// A token goes back the way it came: issued to an anonymous read, it is sent anonymously
    /// even though the app is signed in now.
    #[tokio::test]
    async fn a_token_goes_back_the_way_it_came() {
        let server = MockServer::start(|_| (200, PAGE.to_owned()));
        let it = it_against(&server, true);
        let anon = it
            .comments_continuation(&web(), "tok-anon", false)
            .await
            .unwrap();
        assert!(!anon.read_as_account);
        let acct = it
            .comments_continuation(&web(), "tok-account", true)
            .await
            .unwrap();
        assert!(acct.read_as_account);
        it.comment_replies(&web(), "tok-account", true)
            .await
            .unwrap();
        let sent = server.requests();
        assert!(!as_account(&sent[0]) && as_account(&sent[1]) && as_account(&sent[2]));
    }

    /// The session went away between the first page and a later one: the account's token is not
    /// sent anonymously, nothing goes out, and the caller is told the account changed.
    #[tokio::test]
    async fn an_account_token_without_the_account_is_an_error_and_sends_nothing() {
        let server = MockServer::start(|_| (200, PAGE.to_owned()));
        let it = it_against(&server, false);
        let err = it
            .comments_continuation(&web(), "tok-account", true)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::AccountChanged), "{err:?}");
        assert!(server.requests().is_empty());
    }

    // --- comment writes ----------------------------------------------------------------------

    /// An `Endpoint` command as a read would build it, sending its text as `commentText`.
    fn endpoint(path: &str, kind: &'static str, payload: serde_json::Value) -> WriteCommand {
        WriteCommand::Endpoint {
            path: path.to_owned(),
            payload: payload.as_object().cloned().unwrap_or_default(),
            text_field: TEXT_FIELD,
            kind,
        }
    }

    fn create() -> WriteCommand {
        endpoint(
            "comment/create_comment",
            "create",
            json!({ "createCommentParams": "P" }),
        )
    }

    fn edit() -> WriteCommand {
        endpoint(
            "comment/update_comment",
            "edit",
            json!({ "updateCommentParams": "P" }),
        )
    }

    fn delete() -> WriteCommand {
        WriteCommand::Action("fixture-token".into())
    }

    /// The request as the server sees it: the server-issued fields and the text, the context ours
    /// (a payload can never replace it), the path the command named, and the account's headers.
    #[tokio::test]
    async fn a_write_replays_the_servers_command_with_the_text() {
        let server =
            MockServer::start(|_| (200, r#"{"actions":[{"createCommentAction":{}}]}"#.into()));
        let it = it_against(&server, true);
        let cmd = endpoint(
            "comment/create_comment",
            "create",
            json!({ "createCommentParams": "fixture-params", "context": "must-not-win" }),
        );
        it.comment_write(&web(), &cmd, Some("hello"), false)
            .await
            .unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 1);
        assert!(sent[0]
            .path
            .starts_with("/youtubei/v1/comment/create_comment?"));
        let body: serde_json::Value = serde_json::from_str(&sent[0].body).unwrap();
        assert_eq!(body["createCommentParams"], "fixture-params");
        assert_eq!(body["commentText"], "hello");
        assert!(body["context"].is_object(), "the context is ours: {body}");
        assert!(as_account(&sent[0]));
        assert!(sent[0]
            .header("authorization")
            .is_some_and(|a| a.starts_with("SAPISIDHASH ")));
    }

    /// A post for another track than the one the composer was read for is refused by the session,
    /// so the command (which sends only with a ticket) puts nothing on the wire.
    #[tokio::test]
    async fn a_post_for_another_track_sends_nothing() {
        let server =
            MockServer::start(|_| (200, r#"{"actions":[{"createCommentAction":{}}]}"#.into()));
        let it = it_against(&server, true);
        let identity = it.comments_identity().unwrap();
        let mut session = crate::CommentsSession::default();
        session.start("read-for");
        let page = crate::models::comments::parse_comments_page(
            &crate::models::comments::tests::write_page(),
            true,
        );
        session.remember_page(&crate::Provenance::Account(identity.clone()), &page);
        match session.begin_create(&identity, "another-track") {
            Err(e) => assert_eq!(e, crate::ActionError::Unavailable),
            Ok(ticket) => {
                let _ = it
                    .comment_write(&web(), ticket.command(), Some("hello"), false)
                    .await;
                panic!("a post for another track was approved");
            }
        }
        assert!(server.requests().is_empty(), "nothing reached the server");
    }

    /// The answers: a clean 200 is accepted; an `error`, a failed result, or a non-200 is not; and
    /// none of them carries the params or the text in what comes back.
    #[tokio::test]
    async fn write_answers_are_checked_and_errors_never_hold_the_params_or_text() {
        for (status, reply, ok) in [
            (200, r#"{"actions":[{"createCommentAction":{}}]}"#, true),
            (200, r#"{}"#, true),
            (200, r#"{"error":{"code":400}}"#, false),
            (
                200,
                r#"{"actionResults":[{"status":"STATUS_FAILED"}]}"#,
                false,
            ),
            (
                200,
                r#"{"actionResults":[{"status":"STATUS_SUCCEEDED"}]}"#,
                true,
            ),
            (400, r#"{}"#, false),
            (500, r#"{}"#, false),
        ] {
            let server = MockServer::start(move |_: &Seen| (status, reply.to_owned()));
            let it = it_against(&server, true);
            let cmd = endpoint("comment/x", "reply", json!({ "p": "secret-params" }));
            let got = it
                .comment_write(&web(), &cmd, Some("secret text"), false)
                .await;
            assert_eq!(got.is_ok(), ok, "{status} {reply}");
            if let Err(e) = &got {
                for shown in [e.to_string(), format!("{e:?}")] {
                    assert!(!shown.contains("secret"), "{shown}");
                }
            }
            assert_eq!(server.requests().len(), 1);
        }
    }

    /// Never sent twice: a lost answer to a post is `WriteUncertain` after exactly one request.
    #[tokio::test]
    async fn a_lost_answer_to_a_post_is_uncertain_and_never_resent() {
        let server = MockServer::start(|_: &Seen| (0, String::new())); // hang up
        let it = it_against(&server, true);
        let got = it
            .comment_write(&web(), &create(), Some("hello"), false)
            .await;
        assert!(matches!(got, Err(Error::WriteUncertain)), "{got:?}");
        assert_eq!(server.requests().len(), 1, "exactly one request");
    }

    /// A path that is not a plain `comment/…` one never reaches the network, even in a command
    /// that was built by hand: the host, scheme and port are the fixed base's alone.
    #[tokio::test]
    async fn a_hostile_command_path_sends_nothing() {
        let server = MockServer::start(|_: &Seen| (200, "{}".into()));
        let it = it_against(&server, true);
        for path in [
            "../browse",
            "comment/../browse",
            "comment//x",
            "comment/x?y=1",
            "comment/x#frag",
            "https://evil.example/comment/x",
            "//evil.example/comment/x",
            "evil.example/comment/x",
            "comment/x@evil.example",
            "comment/%2e%2e/x",
            "browse",
            "",
        ] {
            let got = it
                .comment_write(&web(), &endpoint(path, "edit", json!({})), Some("t"), false)
                .await;
            assert!(matches!(got, Err(Error::Other(_))), "{path}: {got:?}");
        }
        assert!(server.requests().is_empty(), "something was sent");
    }

    /// A delete that is a perform-action token is the same replay as a vote.
    #[tokio::test]
    async fn an_action_token_write_goes_to_the_perform_action_path() {
        let server = MockServer::start(|_: &Seen| {
            (
                200,
                r#"{"actionResults":[{"status":"STATUS_SUCCEEDED"}]}"#.into(),
            )
        });
        let it = it_against(&server, true);
        it.comment_write(&web(), &delete(), None, true)
            .await
            .unwrap();
        let sent = server.requests();
        assert!(sent[0]
            .path
            .starts_with(&format!("/youtubei/v1/{COMMENT_ACTION_PATH}")));
        let body: serde_json::Value = serde_json::from_str(&sent[0].body).unwrap();
        assert_eq!(body["actions"], json!(["fixture-token"]));
    }

    // --- comment actions ---------------------------------------------------------------------

    #[test]
    fn success_is_a_succeeded_status_in_either_answer_shape() {
        let verdict = |v: &serde_json::Value| answer_verdict(v).0;
        // `actionResults: [..]` (a like) and `actionResult: {..}` (an edit).
        let like = json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED", "feedback": "FEEDBACK_LIKE" }] });
        let edit = json!({ "actionResult": { "status": "STATUS_SUCCEEDED", "feedbackText": { "runs": [] } } });
        for ok in [&like, &edit] {
            assert_eq!(
                answer_verdict(ok),
                (Verdict::Succeeded, "STATUS_SUCCEEDED"),
                "{ok}"
            );
        }

        // A different status is a rejection in either shape, and so is a top-level error, even
        // beside a success.
        for rejected in [
            json!({ "actionResults": [{ "status": "STATUS_FAILED", "feedback": "FEEDBACK_LIKE" }] }),
            json!({ "actionResult": { "status": "STATUS_FAILED" } }),
            json!({ "actionResults": [{ "status": "nope" }] }),
            json!({ "error": { "code": 400 }, "actionResult": { "status": "STATUS_SUCCEEDED" } }),
        ] {
            assert_eq!(verdict(&rejected), Verdict::Rejected, "{rejected}");
        }
        // The plural array wins when both are there, and only its first entry is the answer.
        let both = json!({ "actionResults": [{ "status": "STATUS_FAILED" }], "actionResult": { "status": "STATUS_SUCCEEDED" } });
        assert_eq!(verdict(&both), Verdict::Rejected);
        let late = json!({ "actionResults": [{ "status": "STATUS_FAILED" }, { "status": "STATUS_SUCCEEDED" }] });
        assert_eq!(verdict(&late), Verdict::Rejected);

        // No status to read: missing, for the caller to decide.
        for missing in [
            json!({}),
            json!(null),
            json!([]),
            json!({ "actionResults": [] }),
            json!({ "actionResults": [{ "feedback": "FEEDBACK_LIKE" }] }),
            json!({ "actionResults": "STATUS_SUCCEEDED" }),
            json!({ "actionResult": {} }),
            json!({ "actionResult": { "feedbackText": "x" } }),
            json!({ "responseContext": {} }),
        ] {
            assert_eq!(
                answer_verdict(&missing),
                (Verdict::Missing, "missing"),
                "{missing}"
            );
        }
        // Only enum-like words are echoed for the log.
        let odd = json!({ "actionResult": { "status": "STATUS_secret value" } });
        assert_eq!(answer_verdict(&odd), (Verdict::Rejected, "unexpected"));
    }

    /// Both answer shapes through a real request, for a vote, a write and a delete; and what a
    /// missing status means for each.
    #[tokio::test]
    async fn every_action_and_write_accepts_either_shape_and_handles_a_missing_status() {
        const PLURAL: &str = r#"{"actionResults":[{"status":"STATUS_SUCCEEDED"}]}"#;
        const SINGULAR: &str =
            r#"{"actionResult":{"status":"STATUS_SUCCEEDED","feedbackText":{}}}"#;
        const FAILED: &str = r#"{"actionResult":{"status":"STATUS_FAILED"}}"#;
        const NONE: &str = r#"{"responseContext":{},"trackingParams":"x"}"#;

        for (reply, vote, write, del) in [
            (PLURAL, "ok", "ok", "ok"),
            (SINGULAR, "ok", "ok", "ok"),
            (FAILED, "rejected", "rejected", "rejected"),
            // No status: a vote is a rejection; a write or a delete is accepted.
            (NONE, "rejected", "ok", "ok"),
        ] {
            let server = MockServer::start(move |_: &Seen| (200, reply.to_owned()));
            let it = it_against(&server, true);
            let label = |r: Result<_, Error>| match r {
                Ok(_) => "ok",
                Err(Error::ActionRejected) => "rejected",
                Err(Error::WriteUncertain) => "uncertain",
                Err(_) => "other",
            };
            let got_vote = label(it.comment_action(&web(), "T", "like").await);
            let got_write = label(
                it.comment_write(&web(), &edit(), Some("t"), false)
                    .await
                    .map(|_| ()),
            );
            let got_delete = label(
                it.comment_write(&web(), &delete(), None, true)
                    .await
                    .map(|_| ()),
            );
            assert_eq!(
                (got_vote, got_write, got_delete),
                (vote, write, del),
                "{reply}"
            );
        }
    }

    /// An edit is idempotent, so it takes the ordinary path: the transport retries a dropped
    /// connection, and a final failure is a plain failure, never `uncertain` (a post or a reply
    /// still is, and is still sent once).
    #[tokio::test]
    async fn an_edit_is_retried_and_never_uncertain_while_a_post_is_sent_once() {
        let server = MockServer::start(|_: &Seen| (0, String::new())); // hang up
        let it = it_against(&server, true);
        let got = it.comment_write(&web(), &edit(), Some("t"), true).await;
        assert!(matches!(&got, Err(Error::Http(_))), "{got:?}");
        assert!(server.requests().len() > 1, "the edit was not retried");

        let before = server.requests().len();
        let got = it.comment_write(&web(), &create(), Some("t"), false).await;
        assert!(matches!(got, Err(Error::WriteUncertain)), "{got:?}");
        assert_eq!(
            server.requests().len() - before,
            1,
            "a post is sent exactly once"
        );
    }

    /// A reply's edit goes out as `{context, replyText, updateReplyParams}` to its path, by the
    /// ordinary (retrying) path; a 404 or a 400 there is an ordinary error, never success.
    #[tokio::test]
    async fn a_reply_edit_sends_update_reply_params_and_a_refusal_is_an_error() {
        for status in [404, 400] {
            let server = MockServer::start(move |_: &Seen| {
                (
                    status,
                    r#"{"error":{"code":400,"message":"m","status":"INVALID_ARGUMENT"}}"#.into(),
                )
            });
            let it = it_against(&server, true);
            let edit = WriteCommand::Endpoint {
                path: crate::models::comment_write::COMMENT_UPDATE_REPLY_PATH.into(),
                payload: json!({ "updateReplyParams": "P" })
                    .as_object()
                    .cloned()
                    .unwrap(),
                text_field: REPLY_EDIT_TEXT_FIELD,
                kind: "edit_reply",
            };
            let got = it
                .comment_write(&web(), &edit, Some("new text"), true)
                .await;
            assert!(
                matches!(&got, Err(Error::Http(e)) if e.status().map(|s| s.as_u16()) == Some(status)),
                "{got:?}"
            );
            let sent = server.requests();
            assert_eq!(sent.len(), 1, "a refusal is not retried");
            assert!(sent[0]
                .path
                .starts_with("/youtubei/v1/comment/update_comment_reply?"));
            let body: serde_json::Value = serde_json::from_str(&sent[0].body).unwrap();
            let mut keys: Vec<&str> = body
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, ["context", "replyText", "updateReplyParams"]);
            assert_eq!(body[REPLY_EDIT_TEXT_FIELD], "new text");
        }
    }

    /// A delete's answer has no status (it carries a `removeCommentAction` and the web client's
    /// toast): accepted, as is a bare 200; an `error` is not. A vote with the same answer still
    /// needs a status.
    #[tokio::test]
    async fn a_delete_answer_without_a_status_is_accepted() {
        let answer = r#"{"responseContext":{},"actions":[
            {"clickTrackingParams":"x","removeCommentAction":{"actionResult":{"status":"STATUS_SUCCEEDED"}}},
            {"clickTrackingParams":"y","openPopupAction":{"popup":{}}}]}"#;
        let popup_only = r#"{"responseContext":{},"actions":[{"clickTrackingParams":"y","openPopupAction":{}}]}"#;
        let bare = r#"{"responseContext":{}}"#;
        let failed = r#"{"actions":[{"removeCommentAction":{}}],"error":{"code":400}}"#;
        for (reply, ok) in [
            (answer, true),
            (popup_only, true),
            (bare, true),
            (failed, false),
        ] {
            let server = MockServer::start(move |_: &Seen| (200, reply.to_owned()));
            let it = it_against(&server, true);
            let got = it.comment_write(&web(), &delete(), None, true).await;
            assert_eq!(got.is_ok(), ok, "{reply}");
        }
        let server = MockServer::start(move |_: &Seen| (200, answer.to_owned()));
        let it = it_against(&server, true);
        assert!(matches!(
            it.comment_action(&web(), "T", "like").await,
            Err(Error::ActionRejected)
        ));
    }

    /// Delete is idempotent: a 404 is success, a failure leaves nothing unknown (it can simply
    /// be pressed again), a rejection is a rejection, and none of them is `uncertain`.
    #[tokio::test]
    async fn a_delete_succeeds_on_a_404_and_is_never_uncertain() {
        // 404: already gone, so done.
        let server = MockServer::start(|_: &Seen| (404, "{}".into()));
        let it = it_against(&server, true);
        let got = it.comment_write(&web(), &delete(), None, true).await;
        assert!(matches!(got, Ok(serde_json::Value::Null)), "{got:?}");
        assert_eq!(server.requests().len(), 1);

        // The same 404 on an edit is an error (the comment is gone, the edit did not happen).
        let it = it_against(&server, true);
        let got = it.comment_write(&web(), &edit(), Some("t"), true).await;
        assert!(matches!(&got, Err(e) if e.is_not_found()), "{got:?}");

        // A 500 (what a dead connection looks like after the retries) is a plain failure.
        let server = MockServer::start(|_: &Seen| (500, "{}".into()));
        let it = it_against(&server, true);
        let got = it.comment_write(&web(), &delete(), None, true).await;
        assert!(matches!(&got, Err(Error::Http(_))), "{got:?}");

        // A rejected status is a rejection, in both shapes, and so is an `error` with no status.
        for reply in [
            r#"{"actionResult":{"status":"STATUS_FAILED"}}"#,
            r#"{"actionResults":[{"status":"STATUS_FAILED"}]}"#,
            r#"{"error":{"code":400}}"#,
        ] {
            let server = MockServer::start(move |_: &Seen| (200, reply.to_owned()));
            let it = it_against(&server, true);
            let got = it.comment_write(&web(), &delete(), None, true).await;
            assert!(matches!(got, Err(Error::ActionRejected)), "{reply}");
        }
    }

    /// The request as the server sees it, and what each answer turns into. The token is in the
    /// body and nowhere else, and never in what comes back to the caller.
    #[tokio::test]
    async fn a_comment_action_posts_the_token_and_checks_the_answer() {
        const TOKEN: &str = "fixture-secret-token";
        for (status, reply, ok) in [
            (
                200,
                r#"{"actionResults":[{"status":"STATUS_SUCCEEDED","feedback":"FEEDBACK_LIKE"}]}"#,
                true,
            ),
            (
                200,
                r#"{"actionResults":[{"status":"STATUS_FAILED"}]}"#,
                false,
            ),
            (200, r#"{}"#, false),
            (400, r#"{}"#, false),
            (500, r#"{}"#, false),
        ] {
            let server = MockServer::start(move |_: &Seen| (status, reply.to_owned()));
            let it = it_against(&server, true);
            let got = it.comment_action(&web(), TOKEN, "like").await;
            assert_eq!(got.is_ok(), ok, "{status} {reply}");
            if let Err(e) = &got {
                assert!(
                    !e.to_string().contains(TOKEN),
                    "the token is in an error: {e}"
                );
                assert!(!format!("{e:?}").contains(TOKEN));
            }
            let sent = server.requests();
            assert_eq!(sent.len(), 1);
            assert!(sent[0]
                .path
                .starts_with(&format!("/youtubei/v1/{COMMENT_ACTION_PATH}")));
            assert!(sent[0].path.contains("prettyPrint=false"));
            let body: serde_json::Value = serde_json::from_str(&sent[0].body).unwrap();
            assert_eq!(
                body["actions"],
                json!([TOKEN]),
                "{{context, actions: [token]}}"
            );
            assert!(body.get("context").is_some());
            assert!(as_account(&sent[0]), "an action is the account's own");
        }
    }
}

#[cfg(test)]
mod account_reads {
    use super::*;
    use crate::clients::{Clients, METADATA_CLIENT};
    use crate::test_server::MockServer;
    use crate::transport::Session;
    use serde_json::json;

    fn row(id: &str, title: &str) -> serde_json::Value {
        json!({ "musicResponsiveListItemRenderer": {
            "playlistItemData": { "videoId": id },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": title }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "An Artist" }, { "text": " \u{2022} " }, { "text": "3:12" }
                ] } } }
            ]
        } })
    }

    // Library songs come 25 at a time: every page is read, each song kept once, and a repeated
    // song on a later page doesn't come twice.
    #[tokio::test]
    async fn library_songs_are_read_page_after_page_each_once() {
        let server = MockServer::start(|req| {
            let body = if req.path.contains("ctoken=page2") {
                json!({ "continuationContents": { "musicShelfContinuation": {
                    "contents": [row("bbbbbbbbbbb", "Second"), row("ccccccccccc", "Third")]
                } } })
            } else {
                json!({ "contents": { "sectionListRenderer": { "contents": [{ "musicShelfRenderer": {
                    "contents": [row("aaaaaaaaaaa", "First"), row("bbbbbbbbbbb", "Second")],
                    "continuations": [{ "nextContinuationData": { "continuation": "page2" } }]
                } }] } } })
            };
            (200, body.to_string())
        });
        let session = Session {
            cookie: Some("SAPISID=fixture; PREF=x".into()),
            visitor_data: Some("v".into()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        let songs = it
            .library_songs(&client, "FEmusic_liked_videos")
            .await
            .unwrap();
        let ids: Vec<&str> = songs.iter().map(|s| s.video_id.as_str()).collect();
        assert_eq!(ids, ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"]);
        let first = &server.requests()[0];
        assert!(
            first.body.contains("FEmusic_liked_videos"),
            "{}",
            first.body
        );
        assert!(first.header("cookie").is_some(), "read as the account");
    }
}

#[cfg(test)]
mod upload {
    use crate::clients::{Clients, METADATA_CLIENT};
    use crate::test_server::MockServer;
    use crate::transport::{InnerTube, Session};

    // Two steps, as Google's uploader wants them: ask for a place with the name and size, then send
    // the whole file to the place it gave, signed in both times.
    #[tokio::test]
    async fn a_song_goes_up_in_two_steps_to_the_place_it_was_given() {
        let server = MockServer::start_with_headers(|req| {
            if req.header("x-goog-upload-command") == Some("start") {
                let place = format!(
                    "http://{}/place/123",
                    req.header("host").unwrap_or_default()
                );
                (
                    200,
                    vec![("x-goog-upload-url".into(), place)],
                    String::new(),
                )
            } else {
                (200, Vec::new(), String::new())
            }
        });
        let session = Session {
            cookie: Some("SAPISID=fixture; PREF=x".into()),
            visitor_data: Some("v".into()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        it.upload_song(&client, "My Song.flac", b"fLaC-bytes".to_vec())
            .await
            .unwrap();
        let seen = server.requests();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0].path.contains("upload/usermusic/http"),
            "{}",
            seen[0].path
        );
        assert_eq!(seen[0].body, "filename=My%20Song.flac");
        assert_eq!(
            seen[0].header("x-goog-upload-header-content-length"),
            Some("10")
        );
        assert_eq!(seen[0].header("x-goog-upload-protocol"), Some("resumable"));
        assert_eq!(seen[1].path, "/place/123");
        assert_eq!(
            seen[1].header("x-goog-upload-command"),
            Some("upload, finalize")
        );
        assert_eq!(seen[1].header("x-goog-upload-offset"), Some("0"));
        assert_eq!(seen[1].body, "fLaC-bytes");
        assert!(
            seen.iter().all(|r| r.header("cookie").is_some()),
            "as the account both times"
        );
    }

    #[tokio::test]
    async fn signed_out_nothing_goes_up() {
        let it = InnerTube::new(Session::default(), None).unwrap();
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        assert!(it.upload_song(&client, "a.flac", vec![1]).await.is_err());
    }
}

#[cfg(test)]
mod report_play {
    use crate::clients::{Clients, METADATA_CLIENT};
    use crate::test_server::MockServer;
    use crate::transport::{InnerTube, Session};
    use serde_json::json;

    // A play is told the way YouTube Music's app tells it: the signed-in player answer's playback
    // address, fetched once with a playback nonce, as the account.
    #[tokio::test]
    async fn a_play_is_reported_to_the_address_the_player_gives() {
        let server = MockServer::start(|req| {
            if req.path.contains("/youtubei/v1/player") {
                let host = req.header("host").unwrap_or_default().to_owned();
                (200, json!({ "playabilityStatus": { "status": "OK" }, "playbackTracking": { "videostatsPlaybackUrl": {
                    "baseUrl": format!("http://{host}/api/stats/playback?docid=vid&ns=yt")
                } } }).to_string())
            } else {
                (204, String::new())
            }
        });
        let session = Session {
            cookie: Some("SAPISID=fixture; PREF=x".into()),
            visitor_data: Some("v".into()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        it.report_play(&client, "vid").await.unwrap();
        let seen = server.requests();
        assert_eq!(seen.len(), 2, "{seen:?}");
        let ping = &seen[1];
        assert!(
            ping.path.starts_with("/api/stats/playback?docid=vid"),
            "{}",
            ping.path
        );
        assert!(
            ping.path.contains("c=WEB_REMIX")
                && ping.path.contains("ver=2")
                && ping.path.contains("cpn=")
        );
        assert!(ping.header("cookie").is_some(), "as the account");
    }

    fn tracked_server() -> MockServer {
        MockServer::start(|req| {
            if req.path.contains("/youtubei/v1/player") {
                let host = req.header("host").unwrap_or_default().to_owned();
                (200, json!({ "playabilityStatus": { "status": "OK" }, "playbackTracking": {
                    "videostatsPlaybackUrl": {
                        "baseUrl": format!("http://{host}/api/stats/playback?docid=vid&ns=yt")
                    },
                    "videostatsWatchtimeUrl": {
                        "baseUrl": format!("http://{host}/api/stats/watchtime?docid=vid&ns=yt&len=200")
                    }
                } }).to_string())
            } else {
                (204, String::new())
            }
        })
    }

    fn signed_in(server: &MockServer) -> InnerTube {
        let session = Session {
            cookie: Some("SAPISID=fixture; PREF=x".into()),
            visitor_data: Some("v".into()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        it
    }

    fn param<'a>(path: &'a str, key: &str) -> Option<&'a str> {
        path.split_once('?')?
            .1
            .split('&')
            .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
    }

    // One listen, one player answer: the play and every listening-time report of it go to the
    // addresses that answer gave, with one playback nonce, as the account.
    #[tokio::test]
    async fn a_listen_is_told_through_one_player_answer_with_one_nonce() {
        let server = tracked_server();
        let it = signed_in(&server);
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        let listen = it.listen_tracking(&client, "vid").await.unwrap();
        assert!(
            !format!("{listen:?}").contains("http"),
            "no address in a log line"
        );
        let heard = |segments: Vec<(u64, u64)>, at_ms| crate::transport::Watched {
            segments,
            at_ms,
            length_ms: 200_000,
            playing: true,
        };
        it.report_watchtime(&client, &listen, &heard(vec![(0, 10_000)], 10_000))
            .await
            .unwrap();
        it.report_tracked_play(&client, &listen).await.unwrap();
        it.report_watchtime(&client, &listen, &heard(vec![(10_000, 50_000)], 50_000))
            .await
            .unwrap();
        let seen = server.requests();
        let paths: Vec<&str> = seen.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            seen.len(),
            4,
            "one player answer for the whole listen: {paths:?}"
        );
        assert!(paths[0].contains("/youtubei/v1/player"));
        assert!(paths[1].starts_with("/api/stats/watchtime?docid=vid"));
        assert!(paths[2].starts_with("/api/stats/playback?docid=vid"));
        assert!(paths[3].starts_with("/api/stats/watchtime?docid=vid"));
        let cpn = param(paths[1], "cpn").unwrap();
        assert_eq!(cpn.len(), 16);
        assert!(paths[1..].iter().all(|p| param(p, "cpn") == Some(cpn)));
        for (p, st, et) in [(paths[1], "0", "10"), (paths[3], "10", "50")] {
            assert_eq!(param(p, "st"), Some(st), "{p}");
            assert_eq!(param(p, "et"), Some(et), "{p}");
            assert_eq!(param(p, "cmt"), Some(et), "{p}");
            assert_eq!(param(p, "len"), Some("200"), "{p}");
            assert_eq!(param(p, "state"), Some("playing"), "{p}");
            assert_eq!(param(p, "ver"), Some("2"), "{p}");
            assert_eq!(param(p, "c"), Some("WEB_REMIX"), "{p}");
        }
        assert!(
            seen.iter().all(|r| r.header("cookie").is_some()),
            "as the account"
        );
    }

    // A player answer without a listening-time address: the listen still has its play, and a
    // report has nowhere to go (an error, nothing sent).
    #[tokio::test]
    async fn no_watchtime_address_no_report() {
        let server = MockServer::start(|req| {
            if req.path.contains("/youtubei/v1/player") {
                let host = req.header("host").unwrap_or_default().to_owned();
                (200, json!({ "playabilityStatus": { "status": "OK" }, "playbackTracking": {
                    "videostatsPlaybackUrl": { "baseUrl": format!("http://{host}/api/stats/playback?docid=vid") }
                } }).to_string())
            } else {
                (204, String::new())
            }
        });
        let it = signed_in(&server);
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        let listen = it.listen_tracking(&client, "vid").await.unwrap();
        let heard = crate::transport::Watched {
            segments: vec![(0, 10_000)],
            at_ms: 10_000,
            length_ms: 0,
            playing: true,
        };
        assert!(it.report_watchtime(&client, &listen, &heard).await.is_err());
        it.report_tracked_play(&client, &listen).await.unwrap();
        assert_eq!(server.requests().len(), 2);
    }

    // Signed out there is no listen to tell: the player is never asked for one.
    #[tokio::test]
    async fn signed_out_no_listen() {
        let server = tracked_server();
        let mut it = InnerTube::new(Session::default(), None).unwrap();
        it.set_base_url(&server.base_url);
        let client = Clients::bundled().get(METADATA_CLIENT).unwrap().clone();
        assert!(it.listen_tracking(&client, "vid").await.is_err());
        assert!(server.requests().is_empty());
    }
}

// The account's choices: recommendations as the account, or YouTube's general ones; searches
// anonymous, or as the account. The library and every change stay the account's either way.
#[cfg(test)]
mod choices {
    use crate::clients::{Clients, YouTubeClient, METADATA_CLIENT};
    use crate::models::metadata::Rating;
    use crate::test_server::{MockServer, Seen};
    use crate::transport::{InnerTube, Session};

    fn signed_in(server: &MockServer) -> InnerTube {
        let session = Session {
            cookie: Some("SAPISID=fixture-sapisid; PREF=x".to_owned()),
            data_sync_id: Some("fixture-dsid".to_owned()),
            visitor_data: Some("fixture-visitor".into()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        it
    }

    fn web() -> YouTubeClient {
        Clients::bundled().get(METADATA_CLIENT).unwrap().clone()
    }

    fn as_account(req: &Seen) -> bool {
        req.header("cookie").is_some()
            || req.header("authorization").is_some()
            || req.body.contains("onBehalfOfUser")
    }

    #[tokio::test]
    async fn recommendations_go_as_the_account_until_switched_off() {
        let server = MockServer::start(|_| (200, "{}".to_owned()));
        let it = signed_in(&server);
        it.home(&web(), None).await.unwrap();
        it.home_continuation(&web(), "more").await.unwrap();
        it.next(&web(), Some("aaaaaaaaaaa"), None).await.unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 3);
        assert!(sent.iter().all(as_account), "personal by default");

        it.set_personal(false);
        it.home(&web(), None).await.unwrap();
        it.home_continuation(&web(), "more").await.unwrap();
        it.next(&web(), Some("aaaaaaaaaaa"), Some("RDAMVMaaaaaaaaaaa"))
            .await
            .unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 6);
        assert!(
            sent[3..].iter().all(|r| !as_account(r)),
            "YouTube's general ones, as signed out"
        );
        assert!(it.is_logged_in(), "the sign-in itself stays");

        // The library and every change are still the account's.
        it.library_playlists(&web()).await.unwrap();
        it.rate(&web(), "aaaaaaaaaaa", Rating::Like).await.unwrap();
        let sent = server.requests();
        assert!(sent[6..].iter().all(as_account));
    }

    #[tokio::test]
    async fn searches_stay_anonymous_until_signed_in_search_is_on() {
        let server = MockServer::start(|_| (200, "{}".to_owned()));
        let it = signed_in(&server);
        it.search_json(&web(), "road", None, true).await.unwrap();
        it.search_continuation_json(&web(), "road", None, "more", true)
            .await
            .unwrap();
        it.search_suggestions(&web(), "roa").await.unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 3);
        assert!(sent.iter().all(|r| !as_account(r)), "anonymous by default");

        it.set_signed_search(true);
        it.search_json(&web(), "road", None, true).await.unwrap();
        it.search_continuation_json(&web(), "road", None, "more", true)
            .await
            .unwrap();
        it.search_suggestions(&web(), "roa").await.unwrap();
        let sent = server.requests();
        assert!(sent[3..].iter().all(as_account));
    }

    // Venuta's own look-ups (a file's song for its radio, link matching, More by, find online,
    // imports) are searches nobody typed: they never go as the account, Signed-in search or not.
    #[tokio::test]
    async fn venutas_own_look_ups_never_go_as_the_account() {
        let server = MockServer::start(|_| (200, "{}".to_owned()));
        let it = signed_in(&server);
        it.set_signed_search(true);
        it.search_json(&web(), "road", None, false).await.unwrap();
        it.search_json(&web(), "road", Some("EgWKAQIIAWoMEA4QChADEAQQCRAF"), false)
            .await
            .unwrap();
        it.search_continuation_json(&web(), "road", None, "more", false)
            .await
            .unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 3);
        assert!(sent.iter().all(|r| !as_account(r)), "{sent:?}");
    }

    #[tokio::test]
    async fn signed_out_nothing_goes_as_an_account_whatever_the_choices() {
        let server = MockServer::start(|_| (200, "{}".to_owned()));
        let mut it = InnerTube::new(Session::default(), None).unwrap();
        it.set_base_url(&server.base_url);
        it.set_signed_search(true);
        it.home(&web(), None).await.unwrap();
        it.search_json(&web(), "road", None, true).await.unwrap();
        it.search_suggestions(&web(), "roa").await.unwrap();
        assert!(server.requests().iter().all(|r| !as_account(r)));
    }

    // Mixed for you's own page and a song's Related page are YouTube's picks, like Home: as the
    // account only with "Recommendations for me".
    #[tokio::test]
    async fn mixes_and_related_follow_recommendations_for_me() {
        let server = MockServer::start(|_| (200, "{}".to_owned()));
        let it = signed_in(&server);
        assert!(it.mixed_for_you(&web()).await.unwrap().is_empty());
        assert!(it
            .related_page(&web(), "MPTRt_seed")
            .await
            .unwrap()
            .is_empty());
        let sent = server.requests();
        assert_eq!(sent.len(), 2);
        assert!(sent.iter().all(as_account), "personal by default");
        assert!(sent[0]
            .body
            .contains("\"browseId\":\"FEmusic_mixed_for_you\""));
        assert!(sent[1].body.contains("\"browseId\":\"MPTRt_seed\""));

        it.set_personal(false);
        it.mixed_for_you(&web()).await.unwrap();
        it.related_page(&web(), "MPTRt_seed").await.unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 4);
        assert!(sent[2..].iter().all(|r| !as_account(r)));
    }
}

/// Playlist writes: sent once, and paced.
#[cfg(test)]
mod playlist_writes {
    use crate::clients::{Clients, YouTubeClient, METADATA_CLIENT};
    use crate::endpoints::ADD_BATCH_PACE;
    use crate::test_server::MockServer;
    use crate::transport::{Error, InnerTube, Session};

    fn it_against(server: &MockServer) -> InnerTube {
        let session = Session {
            cookie: Some("SAPISID=fixture-sapisid; PREF=x".to_owned()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        it
    }

    fn web() -> YouTubeClient {
        Clients::bundled().get(METADATA_CLIENT).unwrap().clone()
    }

    // A new playlist or songs added whose answer was lost are never sent again: that could make
    // a second playlist, or fail as "already in the playlist". The caller reads again instead.
    #[tokio::test]
    async fn a_new_playlist_and_added_songs_are_sent_once() {
        let server = MockServer::start(|_| (0, String::new())); // hang up
        let it = it_against(&server);
        let made = it
            .create_playlist_with(&web(), "Road", None, "PRIVATE", &[])
            .await;
        assert!(matches!(made, Err(Error::WriteUncertain)), "{made:?}");
        assert_eq!(server.requests().len(), 1);
        let added = it.playlist_add_placed(&web(), "PL1", &["a".into()]).await;
        assert!(matches!(added, Err(Error::WriteUncertain)), "{added:?}");
        assert_eq!(server.requests().len(), 2);
    }

    // Many songs go 50 at a time, with a pause between, each song once.
    #[tokio::test]
    async fn many_songs_go_in_batches_of_fifty_with_a_pause_between() {
        let at = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = at.clone();
        let server = MockServer::start(move |_| {
            seen.lock().unwrap().push(std::time::Instant::now());
            (200, "{}".into())
        });
        let it = it_against(&server);
        let mut ids: Vec<String> = (0..120).map(|i| format!("v{i}")).collect();
        ids.push("v0".into());
        it.playlist_add_batches(&web(), "PL1", &ids).await.unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0].body.matches("ACTION_ADD_VIDEO").count(), 50);
        assert_eq!(sent[2].body.matches("ACTION_ADD_VIDEO").count(), 20);
        let at = at.lock().unwrap();
        for w in at.windows(2) {
            assert!(w[1] - w[0] >= ADD_BATCH_PACE, "{:?}", w[1] - w[0]);
        }
    }
}

/// The feedback writes a person presses on a row: a vote on a collaborative playlist, a play taken
/// out of the history. Each sends the one token the row came with, as the account, paced.
#[cfg(test)]
mod row_feedback {
    use crate::clients::{Clients, YouTubeClient, METADATA_CLIENT};
    use crate::test_server::{MockServer, Seen};
    use crate::transport::{Error, InnerTube, Session};

    fn it_against(server: &MockServer) -> InnerTube {
        let session = Session {
            cookie: Some("SAPISID=fixture-sapisid; PREF=x".to_owned()),
            ..Default::default()
        };
        let mut it = InnerTube::new(session, None).unwrap();
        it.set_base_url(&server.base_url);
        it
    }

    fn web() -> YouTubeClient {
        Clients::bundled().get(METADATA_CLIENT).unwrap().clone()
    }

    const TAKEN: &str = r#"{"feedbackResponses":[{"isProcessed":true}]}"#;
    const REFUSED: &str = r#"{"feedbackResponses":[{"isProcessed":false}]}"#;

    fn tokens_sent(req: &Seen) -> serde_json::Value {
        let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        body["feedbackTokens"].clone()
    }

    #[tokio::test]
    async fn a_vote_sends_its_one_token_to_feedback_as_the_account() {
        let server = MockServer::start(|_| (200, TAKEN.to_owned()));
        let it = it_against(&server);
        it.vote(&web(), "FIXTURE-UPVOTE").await.unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].path.contains("/feedback?"), "{}", sent[0].path);
        assert_eq!(tokens_sent(&sent[0]), serde_json::json!(["FIXTURE-UPVOTE"]));
        assert!(sent[0].header("cookie").is_some(), "sent as the account");
    }

    #[tokio::test]
    async fn a_play_taken_out_of_the_history_sends_its_token_to_feedback() {
        let server = MockServer::start(|_| (200, TAKEN.to_owned()));
        let it = it_against(&server);
        it.remove_history_item(&web(), "FIXTURE-REMOVE-FROM-HISTORY")
            .await
            .unwrap();
        let sent = server.requests();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].path.contains("/feedback?"), "{}", sent[0].path);
        assert_eq!(
            tokens_sent(&sent[0]),
            serde_json::json!(["FIXTURE-REMOVE-FROM-HISTORY"])
        );
    }

    // YouTube answers 200 either way; only the per-token result says it was taken. A refusal is
    // said as one, never read as done (a brand channel's history is refused like this).
    #[tokio::test]
    async fn a_token_youtube_turns_down_is_a_refusal_not_a_success() {
        let server = MockServer::start(|_| (200, REFUSED.to_owned()));
        let it = it_against(&server);
        let voted = it.vote(&web(), "T").await;
        assert!(matches!(voted, Err(Error::ActionRejected)), "{voted:?}");
        let removed = it.remove_history_item(&web(), "T").await;
        assert!(matches!(removed, Err(Error::ActionRejected)), "{removed:?}");
        let server = MockServer::start(|_| (200, "{}".to_owned()));
        let it = it_against(&server);
        let silent = it.vote(&web(), "T").await;
        assert!(matches!(silent, Err(Error::ActionRejected)), "{silent:?}");
    }

    // Paced like the other account writes: asked to slow down, it waits and goes again.
    #[tokio::test]
    async fn asked_to_slow_down_a_vote_waits_and_goes_again() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n = calls.clone();
        let server = MockServer::start_with_headers(move |_| {
            if n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                (429, vec![("retry-after".into(), "0".into())], "{}".into())
            } else {
                (200, Vec::new(), TAKEN.into())
            }
        });
        let it = it_against(&server);
        it.vote(&web(), "T").await.unwrap();
        assert_eq!(server.requests().len(), 2);
    }
}
