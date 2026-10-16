# 14: Signing in to every service, and sending playlists out

Researched 2026-10-14,
after [13-playlist-import](13-playlist-import.md). Asked: "we can login to account for any
accounts and fetch any playlists from account apple music yt music and all. also other all options
that this kind of tools usually provide", "give all easy options for import export so user can
choose whatever user like", and "keep this light weight and fast, so we can use external browsers
for logins". "(unconfirmed)" marks what wasn't confirmed at a source.

## The short answer

| Service | Sign in | Read playlists | Write playlists | Without an account |
|---|---|---|---|---|
| **Spotify** | your browser, your own free developer app (needs Premium) | own and collaborative only | yes | Exportify CSV, data download JSON |
| **Apple Music** | Apple's page in your browser (MusicKit JS), needs a project developer key or your own | all, ISRC via catalog | **create and append only**: no remove, reorder or delete | Windows library file (read today), privacy data copy, Mac XML, iTunes XML |
| **YouTube Music** | no official way; your signed-in session (unofficial) | all, Liked Music, saved | yes, full (create, add, remove, reorder, delete) | Google Takeout CSV |
| **TIDAL** | your browser, PKCE, project client ID | yes | yes (throttled) | transfer tool CSV |
| **Deezer** | none (no new apps since 2025) | public links, ISRC | no | link, transfer tool CSV |
| **ListenBrainz** | a token from its settings | yes (JSPF, MBIDs) | yes | public by username |
| **Last.fm** | username (key only) | loved tracks | needs a secret: later | |
| **Navidrome / Subsonic** | server, user, password (salted token) | yes | yes | its M3U export |
| **Jellyfin** | Quick Connect code, or user and password | yes | yes | |
| **Plex** | PIN at plex.tv/link (your browser) | yes | yes | |
| **Amazon, Qobuz, Pandora, JioSaavn, Gaana, Anghami, Boomplay** | no public API | via files | via files | Soundiiz / TuneMyMusic / FreeYourMusic file, pasted list |
| **SoundCloud** | needs a client secret (bring your own) | yes | yes | later |
| **Napster** (closed 2026), **Wynk** (closed 2024) | | | | said plainly |

Files are the way in and out for every service, so file import and export plus strong matching
are the core; accounts are a layer on top.

## Apple Music

- **The supported way** is the Apple Music API. Every call carries a developer token (an ES256 JWT
  signed with a MusicKit key from a paid $99/yr Apple Developer account, 6 months at most, can be
  limited to one web origin) and a Music User Token the user grants by signing in on Apple's own
  page through MusicKit JS `authorize()`. FreeYourMusic's Windows app works this way
  ([FYM](https://freeyourmusic.com/help/music-services/apple-music)). The developer token is public
  by design; only the private key is secret and never goes in the repo (DPLA §2.8).
- **For a desktop app**: a small page on the project's site loads Apple's hosted MusicKit JS (it
  may not be re-hosted) in the **system browser**, calls `authorize()`, and hands the user token
  back to Venuta on a `127.0.0.1` callback. Inside WebView2 it's unconfirmed (popups).
- **Read**: `GET /v1/me/library/playlists`, `/{id}/tracks`; ISRC through `include=catalog`
  (uploads and unmatched songs have none).
- **Write**: `POST /v1/me/library/playlists` (create), `POST …/{id}/tracks` (append). **No
  remove, reorder or delete** ([forum](https://developer.apple.com/forums/thread/805461)), so a
  playlist sent to Apple Music can grow but not be kept in step. Rate limits unpublished; pace it.
- **Terms**: DPLA §3.3.6(D) allows MusicKit for playlist management, disclosed; no charging for
  access. Needs an active subscription.
- **Bring your own key**: possible (Team ID, Key ID, .p8), rarely practical; an advanced setting.
- **Without an account**: Venuta already reads the Windows `Library.musicdb` with the key the user
  enters (persistent ids not read yet). privacy.apple.com's "Apple Media Services" copy has
  `Apple Music Library Playlists.json` and `…Tracks.json` (fields unconfirmed; up to 7 days).
  A Mac's Music app exports XML; iTunes for Windows still exports XML where installed.
- **Out without an account**: M3U8 / XML for a Mac's Music app (local files only); Apple's own
  Transfer (music.apple.com, SongShift-powered) takes playlists from other services, not files.
- **Not by default**: taking the web player's tokens or cookies (Cider forks, Ciderfy, gamdl). Free
  but against Apple's terms and breaks often.

## YouTube Music

- **No official sign-in for apps.** The OAuth device flow ytmusicapi used is dead for YouTube
  Music since Google's 2024 change (ytmusicapi #813: "I don't see any path forward"). The Data
  Portability API is read-only, EU/UK only, and needs Google's app verification.
- **What works** is the signed-in session (cookies) with a `SAPISIDHASH` header, against
  `music.youtube.com/youtubei/v1` with the `WEB_REMIX` client
  ([ytmusicapi browser auth](https://ytmusicapi.readthedocs.io/en/latest/setup/browser.html)).
  - Read: `FEmusic_liked_playlists` (own and saved), `VLLM` (Liked Music), liked albums,
    artists, history; playlist pages with continuations.
  - Write: `playlist/create`, `browse/edit_playlist` (add with `DEDUPE_OPTION_SKIP`, remove by
    `setVideoId`, reorder, rename), `playlist/delete`. Full sync is possible.
  - Lifetime: "about 2 years" per the docs, but field reports of 1 to 2 days, and cookies rotate
    while a browser tab is open; long write runs need pacing (1 to 3 s).
- **Getting the session**: the system browser can't hand its cookies to an app. The ways:
  1. A sign-in window of Venuta's own (a second WebView2 window with its own profile and a full
     Chrome user agent, `accounts.google.com/ServiceLogin?continue=https://music.youtube.com`),
     cookies read with Tauri's `cookies_for_url` (2.4+). Whether Google lets WebView2 sign in in
     2026 is **unconfirmed**; Google blocks OAuth in webviews and blocks some sign-ins by
     fingerprint.
  2. Paste the Cookie header from a signed-in music.youtube.com request (ytmusicapi's way).
  3. A "Connect to Venuta" bookmarklet run on music.youtube.com in the user's own browser, making
     the calls there and posting results to Venuta's `127.0.0.1` listener (idea; CSP and Private
     Network Access may block it: **to test**).
  4. Google Takeout CSVs (read today).
  Not: decrypting the browser's cookie store (Chrome's App-Bound Encryption; looks like malware).
- **Risk, said plainly**: unofficial, against YouTube's terms, small risk of account limits (a
  second Google account is safer), may need signing in again.
- **innertube-rs 0.9.0** (already a dependency) has cookie auth (`SessionOptions.cookie`,
  `apply_auth_headers`), playlist writes (`Actions::create_playlist`, `add_to_playlist`,
  `remove_from_playlist`, `move_playlist_video`, `delete_playlist`) and raw `Actions::execute`.
  Gaps: every call goes to `www.youtube.com` with that origin (YouTube Music needs
  `music.youtube.com`), no library-playlists or Liked Music calls, the music playlist parser drops
  `musicResponsiveListItemRenderer` rows and `playlistSetVideoId`, `create_playlist` lacks privacy
  and description, the cookie is fixed per session. A thin layer in `crates/media-access` covers
  them. Venuta's `playlist_tracks` today reads one page only.
- **Kept**: Venuta never downloads or extracts audio from YouTube ([11](11-youtube-music-in-venuta.md)).

## The others

- **Spotify** (Feb 2026 development mode): owner needs Premium, 5 users per app, tracks only for
  playlists you own or collaborate on, `/playlists/{id}/items`, `POST /me/playlists`, loopback
  `http://127.0.0.1:<port>` only, search limit 10. Extended quota needs 250k+ monthly users. So
  "your own client ID" with a guided setup stays the only sign-in; free accounts use Exportify or
  the data download ([migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide)).
- **TIDAL**: Authorization Code + PKCE, scopes `r_usr w_usr`; user collections and playlists read
  and write; ISRC filter on tracks. Generate the client from the live OpenAPI spec (paths changed
  in Sept 2026; changelog not found). Whether a public client needs no secret: **to test**.
- **Deezer**: keyless public playlist reads with ISRC (Venuta reads the link today but not the
  ISRC); no writes.
- **ListenBrainz**: token from settings; playlists as JSPF with recording MBIDs; create, edit,
  add, move, delete. **Last.fm**: loved tracks by username (key only). **Navidrome/Subsonic**:
  `getPlaylists`, `createPlaylist`, `updatePlaylist`, `deletePlaylist`. **Jellyfin**: Quick
  Connect. **Plex**: PIN flow at plex.tv/link.
- **SoundCloud**: OAuth 2.1 + PKCE but still a client secret: bring your own, later.

## What the transfer tools offer (and what Venuta should)

Soundiiz, TuneMyMusic (behind Spotify's own "Import your music"), FreeYourMusic, SongShift (behind
Apple's Transfer), MusConv, Playlisty, Spotlistr, Exportify:

- **Table stakes**: source → playlists (select all) → destination; Liked songs as a source; file,
  link and pasted text in; CSV/TXT out; a missing-songs list you can save; the source never changed;
  progress per playlist; long playlists split for destinations with a cap.
- **Delighters**: Rematch from a list of candidates; review grouped Matched / To check / Not found
  before writing (SongShift); one-way sync that keeps edits made at the destination
  (FreeYourMusic: hand-added stay, hand-removed aren't re-added, "Download list of songs" when a
  service can't remove); merge, split, dedupe, sort, shuffle, compare, replace, append; a free
  whole-library backup; a scratchpad for pasted text.
- **Wording**: "X of Y added", "Show only missing", "Not matched", "Deleted at target",
  "Manually added".
- **Nobody does well**: matching streaming playlists to **your own files**, and writing playlists
  with **device-correct paths**. Venuta does both.

## Export formats

- **M3U8** (UTF-8, `#EXTM3U`, `#EXTINF:<sec>,Artist - Title`; transfer tools match on that text)
  with **path styles**: PC absolute; relative to the playlist; Android/Poweramp (forward slashes,
  playlist in the Music root); FiiO (same storage, `/storage/emulated/0/` or
  `/storage/external_sd1/`); Sony Walkman (`MUSIC/`, relative); Rockbox (relative or `<microSD1>`);
  a server's view (prefix rewrite for Navidrome, Roon). `.m3u` with a BOM for picky devices.
- **CSV**: Soundiiz's header `title,artist,album,isrc,platform,trackId,duration,addedDate,addedBy,url`
  plus `playlist`; importers also auto-detect TuneMyMusic, Exportify and Takeout layouts.
- **XSPF** (Soundiiz, ListenBrainz), **JSPF** (ListenBrainz; MBIDs; Venuta's own lossless
  backup), **WPL** (Windows Media Player, Sonos), **PLS** (Sonos), **iTunes XML** (Mac Music app's
  Import Playlist; files must exist), **plain text** `Artist - Title`.

| Destination | Takes |
|---|---|
| Soundiiz | CSV, TXT, M3U, XSPF, WPL, PLS, JSON, XML |
| TuneMyMusic (and Spotify's import) | M3U, M3U8, PLS, XSPF, WPL, XML, CSV, TXT |
| Apple Music on a Mac | TXT, XML, M3U, M3U8 |
| Navidrome | M3U |
| Poweramp, FiiO, Walkman, Rockbox | M3U, M3U8 |
| ListenBrainz | JSPF |

Sources for the tools and formats: [Soundiiz](https://soundiiz.com/pricing),
[Soundiiz CSV](https://soundiiz.com/data/fileExamples/playlistExport.csv),
[TuneMyMusic](https://www.tunemymusic.com/help), [FreeYourMusic sync](https://freeyourmusic.com/help/transfers/auto-sync),
[SongShift](https://www.songshift.com/faq), [Exportify](https://github.com/pavelkomarov/exportify),
[Navidrome pls](https://www.navidrome.org/docs/usage/admin/cli/pls/),
[Poweramp paths](https://forum.powerampapp.com/topic/7220-are-relative-paths-supported-in-playlists/),
[FiiO](https://fiio.com/newsinfo/168517.html), [ListenBrainz](https://listenbrainz.readthedocs.io/en/latest/users/api/playlist.html),
[TIDAL API](https://tidal-music.github.io/tidal-api-reference/), [Apple tokens](https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens),
[ytmusicapi #813](https://github.com/sigma67/ytmusicapi/issues/813), [Tauri cookies](https://github.com/tauri-apps/tauri/pull/12665).
