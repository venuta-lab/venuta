# Bringing playlists in: what each service allows, and how the best importers feel

Researched 2026-10-14 for [PLAN-PLAYLIST-IMPORT](../plans/PLAN-PLAYLIST-IMPORT.md). Sources are
linked; "(inference)" marks what wasn't confirmed at a source.

## What each service lets a free desktop app do (2026)

| Service | Ways in for Venuta | Song IDs | Notes |
|---|---|---|---|
| **Spotify** | sign-in with **your own** Client ID; Exportify CSV; "Download your data" JSON | ISRC (API, Exportify) | Development mode since Feb 2026: the app's owner needs Premium, 5 users per app, songs only for playlists you **own or collaborate on** (followed ones: name only). Extended access only for organisations with 250k+ monthly users. [migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide), [quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes). Its policy allows moving "the metadata of the user's playlists". |
| **Apple Music** | the library file on a Mac (XML export); privacy.apple.com data copy; a transfer service's CSV | none | The Apple Music API needs a paid developer membership and a private key that can't ship in an app. [developer tokens](https://developer.apple.com/tutorials/data/documentation/applemusicapi/generating-developer-tokens.md). The Windows app has no export. |
| **YouTube Music** | Google Takeout CSV; a transfer service's CSV | none | The YouTube API forbids credentials in open-source projects and stored data past 30 days. [policies](https://developers.google.com/youtube/terms/developer-policies). Takeout's playlist CSV has video IDs only; the library CSV has title, album, artist. [schema](https://developers.google.com/data-portability/schema-reference/youtube) |
| **Deezer** | a public playlist link (keyless API) | ISRC | New API apps can't be created since 2025 ([Deezer community](https://en.deezercommunity.com/features-feedback-44/api-auth-impossible-80857)); public links still read, with ISRC (checked live). |
| **TIDAL** | sign-in with your own Client ID (to verify); transfer CSV | ISRC | Open registration, OAuth with PKCE; its API changed paths in Sept 2026. |
| **JioSaavn, Gaana** | a transfer service's file from a **public** link (Soundiiz "From Web URL", FreeYourMusic); pasting the song list | sometimes | No public API; JioSaavn's API policy forbids retrieving its data without its authorisation. Wynk shut down in 2024. |
| **Amazon, Qobuz, SoundCloud, Pandora, Boomplay** | a transfer service's CSV | | Closed, partner-only or paid APIs. Napster closed in 2026. |
| **ListenBrainz** | a username or playlist link (JSPF) | MusicBrainz IDs | Open API, no key for public playlists. |
| **Navidrome / Subsonic, Jellyfin, Plex** | your own server: address and sign-in | ISRC, MBID, file paths | Your server, your data. |
| **Last.fm** | loved tracks by username | some MBIDs | Free key, non-commercial. |

The Apple ⇄ YouTube Music transfer (Data Transfer Initiative, 2024) runs server to server between
partners; a desktop app can't be a destination. Its music model (`MusicPlaylist`,
`MusicRecording` with ISRC, duration, release, artists) is a good shape for Venuta's own record.
[DTP models](https://github.com/dtinit/data-transfer-project/tree/master/portability-types-common/src/main/java/org/datatransferproject/types/common/models/music)

## Files a universal importer should read

M3U/M3U8 (no formal spec; `#EXTINF`, `#PLAYLIST`, `#EXTALB`…), PLS, XSPF and JSPF (can carry
MBIDs), WPL/ZPL (Windows Media Player, Zune), ASX, iTunes/Music XML, CSV from Exportify (with
ISRC), Soundiiz (`title,artist,album,isrc,…`), TuneMyMusic and Takeout, FreeYourMusic's XLSX, and
plain `Artist - Title` text. Not: foobar2000 `.fpl` and MusicBee `.mbp` (proprietary; both export
M3U8). Phones' M3U8 files carry the phone's paths: relink by file name and folder, then tags.

## How the best importers feel

- **Apple Music** (SongShift inside): choose a service, sign in, choose, Add. Two endings:
  "Transfer Complete" or "Some Music Needs Review" → Review Now. [Apple](https://support.apple.com/118249)
- **YouTube Music**: runs in the background; close the app. No sync afterwards.
  [Google](https://support.google.com/youtubemusic/answer/14729358)
- **Spotify, Deezer, TIDAL** hand you to TuneMyMusic; **Qobuz** to Soundiiz.
- **TuneMyMusic**'s first screen: *Free text*, *Upload file*, *URL*. No service to choose first.
- **Soundiiz**: one number ("97% transferred") and a per-song **Fix**.
- **FreeYourMusic**: a reason and confidence per match, filters (Missing, Low confidence),
  "Use this match" / "Nothing fits this song, skip it"; never merges into a playlist just because
  the name matches; one-way sync that keeps what you added and never re-adds what you removed.
  [matching](https://freeyourmusic.com/help/transfers/how-matching-works), [sync](https://freeyourmusic.com/help/transfers/auto-sync)
- **Roon, Navidrome, Poweramp** pick up playlist files from a watched folder.
- Sign-in belongs in the system browser, never an embedded one ([RFC 8252](https://datatracker.ietf.org/doc/html/rfc8252)).

Everyone else drops a song that isn't on the destination. Venuta keeps it in its place until you
have it. That is the best thing about Venuta's import, and the flow should lead with it.
