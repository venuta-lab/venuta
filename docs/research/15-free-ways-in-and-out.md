# 15: Completely free ways in and out

Researched 2026-10-14, after [14](14-accounts-and-export.md), for
[PLAN-PLAYLISTS-IN-AND-OUT](../plans/PLAN-PLAYLISTS-IN-AND-OUT.md). Asked: "we don't need anything,
we need to keep this completely free. Think of some better ways. I see some websites that offer
these kinds of services, explore some options as well. But completely free is important."
Endpoints were tried live on 2026-10-14 without signing in or any key; "(unconfirmed)" marks what
wasn't. Free means: no paid account, no developer app, no key, for the project or the person.

## What changed from 14

- **Out**: the Apple Developer account ($99/yr) and MusicKit sign-in; TIDAL's API (needs a
  registered client); Spotify's API for anything new (Feb 2026: owner Premium, 5 users, own
  playlists only, search limit 10; the anonymous web token now returns 403).
- **Gone**: Odesli/song.link's API (`401 PUBLIC_API_ACCESS_DEPRECATED`, retired 2026-07-31, no
  new keys). The iTunes Search API has no ISRC lookup. Apple Music Shortcuts that built playlists
  stopped working with iOS 18.
- **In**: reading public share pages, copying from the apps, a "Copy for Venuta" bookmark, free
  ID bridges, and each service's **own free importer** reached through YouTube Music or Spotify.

## Reading a playlist from its share link (no key)

| Service | What the page gives | Limit | Notes |
|---|---|---|---|
| **Spotify** | `open.spotify.com/embed/playlist/<id>`: `__NEXT_DATA__` → `props.pageProps.state.data.entity`: `name`, `trackList[]` (`uri`, `title`, `subtitle` = artists, `duration` ms) | **100 songs** | No album, no ISRC. Liked Songs has no link. |
| **Apple Music** | `music.apple.com/<cc>/playlist/<slug>/pl.<id>`: `serialized-server-data` → `data[0].data.sections`, `trackLockup` items: `title`, `artistName`, `duration`, album (`tertiaryLinks[0].title`), song id | 300 seen (cap or size: unconfirmed) | Library playlists (`pl.u-`) once shared (unconfirmed). |
| **YouTube Music** | InnerTube, as Venuta does now | 5,000 (continuations) | Liked Music can't be read without a session. |
| **Deezer** | `api.deezer.com/playlist/<id>`: `title`, `isrc`, `duration`, artist, album; paging; public profile favourites `/user/<id>/tracks` | none | Official, keyless. Best behaved. |
| **TIDAL** | `embed.tidal.com/playlists/<uuid>`: `<list-item product-id>` with title, artist, duration | 49 of 49 seen (cap unconfirmed) | HTML; fragile. |
| **JioSaavn** | `jiosaavn.com/api.php?__call=webapi.get&token=<token>&type=playlist&p=<page>&n=50…`: title, album, duration, artists | pages | The site's own JSON; undocumented. |
| **Gaana** | `preloaded-state` JSON: title, artists, album, duration, **ISRC**, **YouTube id** | first 50 | Rest needs its private API. |
| **SoundCloud** | `__sc_hydration` set: first few songs full, the rest ids | | Filling the rest needs its web key: no. Track pages one by one (slow). |
| **Amazon Music** | server-rendered only for crawlers | | Pretending to be Google: no. A page you open yourself: yes (bookmark, below). |
| **Anghami, Boomplay** | bot walls | | Bookmark only. |

Risk: reading public pages is unofficial (each service's terms frown on automated access); at one
person's own, on-request volume the risk is low; page shapes change, so each reader is small,
tested against a saved page, and fails with a plain message.

## Copying from the apps and pages (no key, any length, liked songs too)

- **Spotify desktop**: Ctrl+A, Ctrl+C in any playlist **or Liked Songs** puts one
  `https://open.spotify.com/track/<id>` per line on the clipboard (2026 build: unconfirmed). Each
  link is named from `open.spotify.com/embed/track/<id>` (title, artists, length, release date) or
  the track page's meta tags (`og:description` "Artist · Album · Song · Year", `music:duration`).
  One request per song: throttled and cached.
- **iTunes for Windows, Music on a Mac**: Export Playlist (TXT, XML, M3U); copying rows gives
  tab-separated text (Windows Apple Music app: unconfirmed).
- **"Copy for Venuta" bookmark** (idea; to test per service): a saved bookmark run on any web
  player page you're already signed in to in your own browser (Spotify, Apple Music, TIDAL,
  Amazon, Anghami, Boomplay, YouTube Music, JioSaavn, Gaana) scrolls the list to load it all,
  reads title, artist, album, length from the page and **copies it to the clipboard**; you paste
  into Venuta. Covers liked songs and private playlists, no network from the page, nothing
  installed, works for any service whose page lists songs. Risk: each page's layout changes; some
  pages may block bookmarklets (CSP) (to test).

## Data downloads (free, no risk, take days)

Spotify "Download your data" (playlists, Liked Songs, URIs; no ISRC), Google Takeout (playlist
CSVs with video ids), Apple privacy.apple.com (contents disputed: unconfirmed), Deezer "My
personal data" (unconfirmed), Amazon "Request your data" (unconfirmed).

## Free bridges between services (finding the same song elsewhere)

- **Deezer**: `api.deezer.com/track/isrc:<ISRC>` and search, keyless (tested). The best ISRC bridge.
- **MusicBrainz**: `/ws/2/isrc/<ISRC>`, 1 request per second, honest User-Agent (Venuta's is
  `Venuta/<version> (+https://github.com/venuta-lab/venuta)`); URL relations to streaming links
  are sparse.
- **ListenBrainz Labs** (experimental, keyless, tested): `spotify-id-from-metadata`,
  `spotify-id-from-mbid`, `apple-music-id-from-metadata`.
- **iTunes Search API** (keyless, ~20/min): Apple song ids by text.
- **Venuta's own YouTube Music search** (free, already built): YouTube ids.

## Sending a playlist out, free

| To | Free way | Limit / effort |
|---|---|---|
| **YouTube Music** | signed-in session (14: free, unofficial): create and edit in full | paced |
| | no sign-in: `youtube.com/watch_videos?video_ids=…` makes an "Untitled List" (tested); open on music.youtube.com and **Save** | **50 songs per link**; several links for longer |
| **Spotify** | Venuta copies `open.spotify.com/track/…` links (ids from ListenBrainz Labs); paste with Ctrl+V into a playlist in Spotify's desktop app | any length (Free accounts: unconfirmed) |
| | Spotify's own **Import your music** (TuneMyMusic inside Spotify) from YouTube Music | reported unlimited inside Spotify |
| **Apple Music** | Apple's own **Transfer Music** (music.apple.com, iPhone, Android) from YouTube Music or Spotify | free with the subscription; playlists you made; 30 days to review |
| | Google Takeout → Apple (YouTube Music playlists you made) | |
| | a page of Apple Music links (iTunes Search) to add by hand | |
| **Deezer** | its own **Transfer your favourites** from YouTube Music or Spotify | "completely free", unlimited |
| **TIDAL, Amazon, SoundCloud** | TuneMyMusic from YouTube Music or a Venuta CSV | **500 songs free** |
| **Any of 40+ services** | Soundiiz with a Venuta file (CSV, TXT, M3U, XSPF, WPL, PLS, JSON) | **200 songs per playlist**, one at a time, free |
| **JioSaavn, Gaana** | FreeYourMusic | **600 songs, lifetime** |
| | a page of links to add by hand | |

So **YouTube Music is the free hub**: Venuta can write it in full; from there every big service's
own free importer takes it the rest of the way.

## Free tools, and where Venuta hands off

| Tool | Free | Takes |
|---|---|---|
| TuneMyMusic | 500 songs; unlimited inside Spotify and Deezer | text, CSV/TXT (M3U unconfirmed), links |
| Soundiiz | 200 songs per playlist, one playlist at a time | text, CSV, TXT, M3U, XSPF, WPL, PLS, JSON, XML |
| FreeYourMusic | 300, or 600 after email sign-up, lifetime | service to service |
| Exportify | free Spotify → CSV (still working for everyone after Feb 2026: unconfirmed) | |
| Apple Transfer, Spotify Import, Deezer Transfer | free | service to service |

Not free enough: Spotlistr (credits now), Playlisty (20-song cap free), SongShift files (Pro),
spotify_to_ytmusic (needs developer apps).

Sources: [Spotify Feb 2026](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide),
[100-song embed](https://community.spotify.com/t5/Spotify-for-Developers/Spotify-Playlists-Limited-To-100-Tracks-When-Embedded/m-p/5250720),
[Spotify paste](https://community.spotify.com/t5/Your-Library/how-to-paste-a-list-of-URL-s-into-a-playlist/m-p/5353749),
[Spotify import](https://www.spotify.com/us/import-music/), [Spotify data](https://support.spotify.com/us/article/understanding-my-data/),
[Odesli notice](https://linktree.notion.site/API-d0ebe08a5e304a55928405eb682f6741),
[YouTube save trick](https://dev.to/roshan_jaiswal_b3fb04eb75/i-discovered-a-way-to-create-a-saveable-youtube-music-playlist-from-any-list-of-video-ids-no-40nn),
[Apple transfer](https://support.apple.com/118249), [Google → Apple](https://support.apple.com/107776),
[Deezer transfer](https://support.deezer.com/hc/en-gb/articles/360020762638-Transfer-Music-To-Deezer-From-Another-Service),
[TuneMyMusic](https://www.tunemymusic.com/plans), [Soundiiz](https://soundiiz.com/pricing),
[FreeYourMusic](https://freeyourmusic.com/help/transfers/free-limit-and-what-counts-as-a-song),
[ListenBrainz Labs](https://labs.api.listenbrainz.org/), [MusicBrainz](https://musicbrainz.org/development/mmd),
[iTunes export](https://support.apple.com/guide/itunes/itns2998/windows),
[Shortcut ended](https://github.com/NumNumV/TXT-to-AppleMusic-Shortcut).
