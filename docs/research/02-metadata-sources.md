<!-- Research report produced 2026-10-12 by a research agent; claims tagged VERIFIED/UNVERIFIED with sources. -->

# Music metadata sources for a free desktop tagger (Indian film music focus)
Research date 2026-10-12. **VERIFIED** means I confirmed it in official docs or by a live, unauthenticated test call today. Test files: Rockstar OST, UPC 8902894697408, ISRC INS181103501. **UNVERIFIED** means a third-party source or my own inference.

## 1. iTunes Search / Lookup API (Apple)
- **Auth:** none. **Rate limit:** "approximately 20 calls per minute." Apple says large sites should cache results. VERIFIED: https://performance-partners.apple.com/search-api
- **Lookup:** by iTunes ID, UPC/EAN or AMG ID. The `country` parameter takes a two-letter code. VERIFIED (same page).
  - `lookup?upc=8902894697408&entity=song&country=in` returned the album plus 14 tracks. VERIFIED, live test.
  - `lookup?isrc=INS181103501` returned 0 results, so there is no ISRC lookup. VERIFIED, live test.
- **Fields:** artistName (track artists, here the singers), collectionName (canonical title, e.g. "Rockstar (Original Motion Picture Soundtrack)"), releaseDate, primaryGenreName ("Bollywood"), copyright (℗ line, which gives the label), disc and track numbers. There is **no composer or lyricist field** in the result keys. VERIFIED, live test.
- **Artwork:** change `100x100bb` to `3000x3000bb` in the URL and Apple returns the largest stored image without upscaling.
  - Rockstar (2011): 1400×1400. Animal (2023): 3000×3000. VERIFIED, live test.
- **ToS:** Promotional content (art, previews) may only be used to promote store content, and previews can't be cached. https://performance-partners.apple.com/search-api. Embedding art in personal files is a grey area (UNVERIFIED, my inference).
- **Apple Music API (MusicKit):** needs a signed developer token, which requires Apple Developer Program membership. VERIFIED: https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens. Fee of $99/yr is UNVERIFIED: https://developer.apple.com/forums/thread/661313. Apple says tokens scraped from music.apple.com are unsupported and may be blocked: https://developer.apple.com/forums/thread/702228. **Not usable.**

## 2. Deezer public API
- **Auth:** none for catalog reads. VERIFIED, live calls.
- **Rate limit:** a 60-request burst got 5 errors of "Quota limit exceeded" (code 4). That fits the commonly cited 50 requests per 5 seconds. VERIFIED, live test. Deezer's terms let it restrict access at any time: https://developers.deezer.com/termsofuse
- **Lookup:** `track/isrc:{ISRC}` and `album/upc:{UPC}` both work. VERIFIED.
  - **Caution:** `track/isrc:INS181103501` returned a 2024 compilation copy ("The Rk Magic"), not the 2011 album. Look up by UPC first, then match tracks by ISRC inside that album. VERIFIED, live test.
- **Fields:** album title, upc, label ("Super Cassettes Industries Private Limited"), release_date (2011-10-01), record_type, genres (Films/Games, Film Scores, Bollywood). VERIFIED.
  - **Contributors only have role "Main"** (A.R. Rahman, Javed Ali, Mohit Chauhan), so there are no composer or lyricist roles. VERIFIED.
- **Artwork:** cover_xl is 1000px. Editing the URL gives 1400 at "1400x1400", 1200 at 1500–1800, and HTTP 403 at 2000 and above. VERIFIED, live test.
- **ToS:** non-commercial only, no income from the API, Deezer's trademark guidelines apply, and the terms can change without notice. VERIFIED: https://developers.deezer.com/termsofuse

## 3. MusicBrainz, Cover Art Archive and Harmony
- **Rate limit:** about 1 request per second per IP. A meaningful User-Agent with contact details is required. Excess requests get 503. VERIFIED: https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting
  - Today I got intermittent 503s even at one request every 2 seconds. VERIFIED, live; the cause may be server load.
- **License:** core data is CC0, supplementary data is CC BY-NC-SA 3.0. VERIFIED: https://musicbrainz.org/doc/About/Data_License
- **Lookup and credits:** `/isrc/INS181103501` gave the recording, its work, and first-release-date 2011-09-30. The work carries "composer: A. R. Rahman, lyricist: Irshad Kamil". `release?query=barcode:` found both the IN CD and the worldwide digital release, with label T-Series, catalog number, and type Album + Soundtrack. VERIFIED, live. **This is the best structured source for original date, composer, lyricist and release type.**
- **Indian coverage:** the search index reports 56,777 releases with country IN. VERIFIED, live (`release?query=country:IN`).
  - Found: Rockstar, Sairat, Lootera, Queen, Premam, Vikram, RRR (11 releases), Jailer, Crash Landing on You, Witcher 3.
  - "Kaavaalaa" returned 0, and Natsamrat matched an unexpected artist. Coverage is good for big albums but patchy for singles and regional titles. VERIFIED, live spot checks.
  - Work-level lyricist links are often missing: Wikidata has only 2,619 Hindi works with a lyricist (see §9). There is long-standing confusion over how to credit Bollywood film albums: https://www.hifivision.com/threads/style-guideline-for-tagging-film-music.55328 (UNVERIFIED, 2014).
- **Genres:** the official genre list includes filmi, ghazal, qawwali, carnatic classical, hindustani classical, indian pop, bhangra, j-pop, k-pop, c-pop, mandopop, opm. VERIFIED: https://musicbrainz.org/ws/2/genre/all?fmt=txt
- **Cover Art Archive:** no stated rate limit, though 503 is listed for over-limit. Thumbnails at 250/500/1200 plus the original image. VERIFIED: https://musicbrainz.org/doc/Cover_Art_Archive/API
  - Today CAA returned 500/502 from archive.org for the Rockstar release. VERIFIED, live; reliability risk.
- **Harmony:** MIT-licensed, self-hostable with Deno. Looks up by GTIN, URL or ID. Some providers need your own client credentials. VERIFIED: https://github.com/kellnerd/harmony
  - Providers include Deezer, iTunes, Spotify, Tidal, Discogs, Qobuz, Bandcamp, Beatport, Melon, Bugs, Mora, Ototoy. VERIFIED: https://github.com/kellnerd/harmony/tree/main/providers
  - **There is no JSON API.** The server routes are only HTML pages (`index`, `release`, `release/actions`, `settings`). VERIFIED: https://github.com/kellnerd/harmony/tree/main/server/routes
  - Use it as a design reference, or port its merge logic. Don't call the public instance.

## 4. Spotify Web API (effectively unusable)
- Development mode: the app owner needs Premium, there's a 5-user cap, and each user must be allowlisted. VERIFIED: https://developer.spotify.com/documentation/web-api/concepts/quota-modes
- Since 2025-05-15, extended access is open only to organizations with 250k or more monthly active users. VERIFIED (same page), and https://developer.spotify.com/blog/2025-04-15-updating-the-criteria-for-web-api-extended-access
- February 2026 dev-mode changes: batch endpoints were removed; album `label` and `popularity` fields were removed; `external_ids` is marked "reverted"; search limit dropped to 10. VERIFIED: https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide
- No songwriter credits endpoint exists. UNVERIFIED (community posts): https://community.spotify.com/t5/Spotify-for-Developers/Request-Spotify-API-Credits-Endpoint/td-p/7503978

## 5. TIDAL API v2
- The official OpenAPI spec (v1.10.165) lets **Client Credentials** call `/tracks?filter[isrc]`, `/albums?filter[barcodeId]`, `/credits/{id}` (name, role) and `/lyrics/{id}` (`lrcText`, `direction`). VERIFIED: https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json
- The Developer Terms allow "non-commercial applications" only. VERIFIED (via search snippet): https://developer.tidal.com/documentation/guidelines-developer-terms-1_0
- The `@tidal-music/auth` package says client credentials are for TIDAL-internal apps only, which conflicts with the spec. UNVERIFIED: https://npmjs.com/package/@tidal-music/auth
- beets ships a public demo client ID and logs users in through a browser. VERIFIED: https://beets.readthedocs.io/en/v2.12.0/plugins/tidal.html
- Shipping a client secret in open-source code is a ToS and revocation risk (UNVERIFIED, my inference).
- **Potentially the richest source for credits and synced lyrics, matching the user's Tidal-sourced files. Treat it as an optional plugin.**

## 6. Qobuz
- Partner-only API, credentials by email request. The app_id was revoked for third parties in 2019. UNVERIFIED: https://moodeaudio.org/forum/showthread.php?tid=2208 and https://github.com/taschenb/python-qobuz. **Not usable.**

## 7. Discogs
- Unauthenticated calls work, including `database/search?barcode=`. The response header says `x-discogs-ratelimit: 25`. VERIFIED, live.
- 60/min when authenticated, 25/min without. UNVERIFIED (copy of the docs): https://repo.rock-n-code.com/rock-n-code/discogs-service/blame/tag/v0.6.0/Sources/DiscogsService/openapi.yaml
- The Rockstar IN CD has release-level "Music By: A.R. Rahman" and "Lyrics By: Irshad Kamil", track-level musicians, genres Pop / Stage & Screen, styles Bollywood / Soundtrack. VERIFIED, live (release 21326812).
- **Images are scans of 600px or less.** VERIFIED, live.
- Strong for physical releases. Weak for digital-only singles (UNVERIFIED).

## 8. JioSaavn, Gaana, Wynk, Hungama
- **None has an official public API** (UNVERIFIED). Gaana's 2014 developer program is not confirmed alive: https://www.medianama.com/2014/02/223-gaana-developer-api/
- JioSaavn's internal web endpoint (`jiosaavn.com/api.php?__call=search.getResults`) returns `artistMap.artists` with roles **music, lyricist, singer, starring**, plus label and year. VERIFIED, live (Rockstar: music A.R. Rahman; lyricist Irshad Kamil; singers Javed Ali, Mohit Chauhan).
  - Artwork maxes at 500×500; 1000×1000 returns 404. VERIFIED, live.
  - **The best credits data for Indian film music, but:**
- JioSaavn's terms ban "automated or unofficial means (including use of scripts or web crawlers or programming interfaces)". VERIFIED: https://www.jiosaavn.com/corporate/terms
  - Unofficial wrappers exist: https://github.com/anxkhn/jiosaavn-api
  - **High ToS risk.** At most an opt-in, off-by-default plugin.

## 9. Wikidata / Wikipedia
- Wikidata main-namespace data is CC0. VERIFIED: https://www.wikidata.org/wiki/Wikidata:Licensing
- SPARQL, no key: 6,906 Hindi-language Indian films, 4,444 of them with a composer (P86). Only 2,619 Hindi works have a lyricist (P676). VERIFIED, live on query.wikidata.org.
- Useful for film release year and music director as a fallback. Year-only dates are common (Rockstar = "2011-01-01"). Title matching is ambiguous (two films named "Rockstar").
- Wikipedia infobox text is CC BY-SA. UNVERIFIED: https://en.wikipedia.org/wiki/Wikipedia:Copyrights

## 10. Lyrics
- **LRCLIB:** no key, "generous rate limiting". A User-Agent is required, and clients must honor 429/503 with `Retry-After`. `/api/get` matches duration within ±2 seconds. Optional app registration via the `Lrclib-App-Id` header is not enforced. Database dumps exist (`/db-dumps`). VERIFIED: https://lrclib.net/docs
  - The lyrics content license is not stated (UNVERIFIED). Code is MIT: https://github.com/tranxuanthang/lrclib
  - Coverage: Kun Faya Kun had about 20 records, mostly synced, in both Devanagari and romanized. Pasoori (Shahmukhi script) and Zingaat were found. VERIFIED, live.
  - Quality issues: one Zingaat result was an English translation, and another contained "म्हारीcountry". Many entries were 377–385 s against the 470 s album cut. VERIFIED, live.
  - I saw a 503 ServerOverloaded once. VERIFIED, live.
- **Musixmatch:** free tier reportedly gives a ~30% lyric preview, and full or synced lyrics need a paid license. UNVERIFIED: https://freeapihub.com/apis/musixmatch. Not viable.
- **NetEase / QQ:** only unofficial wrappers; the main NetEase wrapper is archived. UNVERIFIED: https://best-of-web.builder.io/library/Binaryify/NeteaseCloudMusicApi
- **Legal:** the EFF argued that personal, non-commercial lyric annotation is likely fair use (opinion, not a ruling): https://www.eff.org/deeplinks/2005/12/lyrics-browsers-itunes-and-copyright-law. The NMPA has pursued unlicensed lyric *publishers*: https://www.nmpa.org/?p=1133. The app should fetch on the user's machine and never bundle or redistribute lyrics (inference).

## 11. Genres
- **Last.fm:** needs an API key and shared secret. VERIFIED: https://www.last.fm/api/authentication
  - Terms: non-commercial, attribution required, cache per the HTTP headers, and a 100 MB stored-data cap. VERIFIED: https://www.last.fm/api/tos
  - Whether one app key can ship in an open-source binary is unaddressed (UNVERIFIED).
- **Better genre sources for this library:** MusicBrainz genres (filmi, ghazal, qawwali), Deezer album genres (Bollywood, Film Scores), and Discogs styles (Bollywood, Soundtrack). VERIFIED (above).

## 12. AcoustID
- One application key for lookups; per-user keys are needed only for submissions. Limit 3 requests per second. "free for non-commercial use only". Can return MusicBrainz recording and release-group data. VERIFIED: https://acoustid.org/webservice
- 76.5M AcoustIDs; 22.2M linked recordings; only 30.48% of AcoustIDs link to a MusicBrainz recording. VERIFIED: https://acoustid.org/stats
- Useful only for untagged files. Not needed for this library, where 99% of files have an ISRC.

## Comparison
| Source | Auth | Rate limit | ISRC / UPC lookup | Composer / lyricist | Max art | Indian film coverage | ToS risk |
|---|---|---|---|---|---|---|---|
| iTunes | none | ~20/min | no / yes | no | 3000 (often 1400) | high | low–med (promo clause) |
| Apple Music API | paid program | – | yes / yes | composerName | 3000+ | high | needs paid program |
| Deezer | none | ~50/5 s | yes* / yes | no (Main only) | 1400 | high | low (non-commercial) |
| MusicBrainz + CAA | UA only | 1/s | yes / yes | yes (work rels) | original / 1200 | medium, patchy | very low (CC0) |
| Harmony | – | – | GTIN only | no | – | – | no API |
| Spotify | Premium owner, 5 users | quota | limited | no | 640 (UNVERIFIED) | high | blocked |
| TIDAL v2 | client creds | ? | yes / yes | yes (credits) | ? | high (your source) | med–high |
| Qobuz | partner only | – | – | – | – | – | blocked |
| Discogs | none / token | 25 / 60 per min | no / yes | yes (release) | 600 | medium (physical) | low |
| JioSaavn | none (unofficial) | ? | no / no | **yes, best** | 500 | **highest** | **high** |
| Wikidata | none | WDQS limits | no | composer (film) | – | medium | very low |
| LRCLIB | none | generous | duration match | – | – | good (both scripts) | low–med |
| AcoustID | 1 app key | 3/s | fingerprint | – | – | low | low (non-commercial) |

*Deezer's ISRC lookup can return a compilation copy of the track; see §2.

## Recommended lookup strategy
1. **Anchor on the UPC.** `Deezer album/upc:` and `iTunes lookup?upc=&country=in`, with `country=us` as a second try. Match tracks inside that album by ISRC, never by ISRC alone. Use `MB release?query=barcode:` to get the MBID.
2. **Album title, release type, label:**
   - Title: iTunes collectionName, which keeps "(Original Motion Picture Soundtrack)".
   - Label: Deezer `label`, or the iTunes ℗ line.
   - Release type: Deezer record_type or MB primary/secondary type.
3. **Original year vs digital date:** MB recording `first-release-date`, then MB release-group date, then Wikidata film P577, then Deezer/iTunes date. Flag cases where Deezer's date is much later than MB's (a re-release).
4. **Album artist = composer:**
   - MB release artist-credit.
   - iTunes collectionArtistName/artistName.
   - Discogs "Music By".
   - Wikidata P86.
5. **Track artists (singers):** iTunes artistName, then Deezer contributors.
6. **Composer / lyricist per track:**
   - MB work relations (by ISRC).
   - Discogs release extraartists (album-level "Lyrics By").
   - Optional plugins: TIDAL credits (user supplies own client ID) and JioSaavn (off by default, ToS warning).
   - Otherwise leave the field empty for the user to fill.
7. **Cover art:**
   - iTunes, requesting `3000x3000bb` and accepting the native size.
   - Deezer 1400.
   - CAA original/1200.
   - Reject anything under 1400, with a manual override.
8. **Genre:**
   - Deezer album genres.
   - MB genres.
   - Discogs styles.
   - Map all of these to a small user-editable vocabulary (Filmi/Bollywood, Film Score, Ghazal, Qawwali, K-OST, Game OST, Rock).
9. **Synced lyrics:**
   - LRCLIB `/api/get` with the exact duration.
   - Then `/api/search`, filtered to within ±2 s and synced lyrics only.
   - Prefer the user's chosen script (Devanagari or romanized) using a Unicode-range check.
   - Write a sidecar `.lrc` and/or the tag. Never bundle lyrics.
10. **Caching and politeness:**
    - Keep a local SQLite cache keyed by UPC, ISRC and MBID, with a long TTL for catalog data.
    - One request queue per host (iTunes 1 per 3 s, MB 1/s, Deezer under 10/s, Discogs under 25/min).
    - Exponential backoff that honors Retry-After.
    - A descriptive User-Agent.
    - Batch runs should be resumable, because iTunes alone takes about 3 s per album.

## Risks
- **iTunes:** 20/min makes large-library scans slow. Artwork-URL tricks are undocumented and could break (UNVERIFIED).
- **ISRC reuse across compilations** produces wrong album matches (VERIFIED, §2).
- **MusicBrainz/CAA:** 503s and archive.org outages (VERIFIED today). Indian credits are incomplete.
- **Non-commercial clauses** in Deezer, TIDAL, Last.fm and AcoustID. Fine for a free app, but they block any future paid tier.
- **JioSaavn and TIDAL credentials:** scraping and secret-distribution risk; ban or revocation is possible.
- **Spotify, Apple Music API and Qobuz** are effectively closed to a free app without paid membership or organization status.
- **LRCLIB:** single-maintainer crowdsourced service with variable quality (translations, mixed scripts) and an unstated lyrics license.
- **Lyrics copyright:** keep fetching user-initiated and local only.
