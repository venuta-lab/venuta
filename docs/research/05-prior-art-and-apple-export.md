<!-- Research report produced 2026-10-12 by a research agent; claims tagged VERIFIED/UNVERIFIED with sources. -->

# Prior-art report: open-source "music librarian" (as of 2026-10-12)

**How each claim is labelled**
- **[V]**: I read the primary page myself.
- **[S]**: the source is primary (official docs or the repo), but I only saw it as a search-engine extract.
- **[U]**: the source is secondary, anecdotal or an aggregator.

**Limits on the research**
- reddit.com and old.reddit.com refused fetches, and a Reddit mirror (redlib) returned a 502 error. So I have **no direct r/musichoarder or r/DataHoarder evidence**.
- The Hydrogenaudio wiki returned 403. Head-Fi coverage is thin.
- The "what users complain about" findings rely on beets/Jellyfin/MusicBee/Lidarr forums, Lemmy, Trustpilot and the projects' own docs. They need a manual Reddit pass before you treat them as community consensus.

---

## Part A: Existing tools

| Tool | Best at | Biggest complaints | Copy / avoid |
|---|---|---|---|
| **beets** | Very actively maintained: v2.9 through v2.14.1 in 2026, Python 3.14 support, uv, a database backup before migrations, library paths stored relative to the root [V] https://github.com/beetbox/beets/releases. Plugin set includes fetchart, lyrics, replaygain, smartplaylist, missing, duplicates, edit, convert, chroma and fromfilename; the only GUI is an "experimental" web plugin [V] https://beets.readthedocs.io/en/stable/plugins/index.html. beets-alternatives (MIT) keeps transcoded or symlinked copies in sync with a query [V] https://github.com/geigerzaehler/beets-alternatives | Its own guide says the autotagger is "fussy" and asks many questions [S] https://docs.beets.io/en/v1.4.8/guides/tagger.html. Users are overwhelmed by match percentages, singles match badly, and Various Artists gets assigned wrongly [U] https://discourse.beets.io/t/import-problems/2069. On Windows, path bytes-vs-text handling needs `\\?\` boilerplate and causes many bugs [S] https://beets.io/blog/paths; console UnicodeEncodeError [U] https://discourse.beets.io/t/unicodeencodeerror/215. For non-Latin names it relies on MusicBrainz aliases; aliases don't reach titles, and `asciify_paths` produced garbled Hebrew folder names [U] https://discourse.beets.io/t/romanized-artist-name/863 | **Copy:** the query language, smartplaylist-to-m3u, "alternatives" for device copies, backup before migrations. **Avoid:** CLI-only interrogation; make every decision a reviewable queue item with a diff |
| **MusicBrainz Picard** | Album-oriented matching, AcoustID fingerprinting, file-naming script (everything before the last `/` is the folder) [S] https://picard-docs.musicbrainz.org/en/config/options_filerenaming_editor.html. A locale-ordered alias option translates artist, album and track names [S] https://picard-docs.musicbrainz.org/en/latest/config/options_metadata.html | Slow or appears stalled on large batches; the docs advise about 200 files per batch [S] https://picard-docs.musicbrainz.org/en/troubleshooting/stopped_working.html. 3.0 (Qt6, new plugin API, synced lyrics) reportedly shipped Oct 2026; the official changelog only shows 3.0.0b7 (2026-07-16) [U] https://alternativeto.net/news/2026/10/musicbrainz-picard-3-0-brings-new-plugin-system-apple-silicon-support-and-ui-enhancements/ | **Copy:** the locale-alias fallback chain (sort name as the English fallback); a naming template with live preview. **Avoid:** batch-size ceilings. Throttle and queue MusicBrainz calls in the background |
| **Mp3tag** | Batch actions; extensible web-source scripts on Windows [U] https://www.videohelp.com/software/Mp3tag/reviews | The Mac version has no custom web sources [S] https://community.mp3tag.de/t/tag-source-for-mac-mp3tag/52076. Tag sources only return what the API returns, so original dates go missing [S] https://community.mp3tag.de/t/wanting-to-purchase-mp3tag-for-macos-but-i-am-having-a-few-struggles/63800 | **Copy:** named, reusable "action groups", which match our "rules". **Avoid:** sources that are hard to maintain per platform |
| **MusicBee** | Auto-organise rules per drive; device sync with on-the-fly FLAC-to-MP3; SD card detection [S] https://scarpie.altervista.org/musicbee-sync-flac-library-to-mp3-device/ | Windows-only; Wine setup is painful and buggy [U] https://lemmy.nullspace.lol/post/524787. Playlist paths on the device need hand fixes [U] https://forum.earlybird.club/threads/music-player-question.478609/ | **Copy:** sync with transcoding and playlist path rewriting. **Avoid:** platform lock-in |
| **foobar2000** | Properties dialog ("format from other fields", auto-fill); Masstagger component still updated (1.9.2, 2025-04-28) [S] https://www.foobar2000.org/components/view/foo_masstag/release/1.9.2 | Learning curve [U] https://hydrogenaudio.org/index.php/topic,65462.0.html | **Copy:** previewable title-formatting moves. **Avoid:** expert-only setup |
| **Lidarr** | Wanted/missing tracking | Metadata server down or rebuilding, returning 5xx errors (GH #5498); whole libraries land in "unmapped files" when tags don't match [U] https://lemmy.mariusdavid.fr/post/375352, https://forum.yunohost.org/t/music-library-not-recognized-by-lidarr/20104 | **Avoid:** depending on a single proxy metadata server; call MusicBrainz directly and cache |
| **Navidrome** | `.nsp` JSON smart playlists. Rules on `loved`, `playCount`, `rating`, `inTheLast`, `inPlaylist` and `filepath`; imported at scan time like `.m3u` [V] https://www.navidrome.org/docs/usage/features/smart-playlists/ | (none collected) | **Copy:** consider `.nsp`-compatible smart-playlist rules, so output works on Navidrome too |
| **Jellyfin** | (not assessed) | Ignores the album-artist tag (picks the artist with the most tracks); multi-value artists get duplicated; NFO files override tags [U] https://forum.jellyfin.org/t-issues-with-multi-artists-tag, https://forum.jellyfin.org/t-setting-and-displaying-correct-album-artist-for-an-album-with-multiple-artists | **Implication:** the audit should enforce a consistent ALBUMARTIST, "Various Artists" for compilations, and clean multi-value delimiters, because servers punish inconsistency |
| **Strawberry** | Player and organiser: MusicBrainz tags; art from many services; lyrics; transfer to USB mass-storage, MTP and iPod [S] https://flathub.org/en/apps/org.strawberrymusicplayer.strawberry | (none collected) | (none) |
| **Kid3** | Tags from filename and filename from tags; CSV, HTML and JSON export that round-trips; LRC import and export; 3.10.1 released Jul 2026 [S] https://kid3.kde.org, https://www.videohelp.com/software/Kid3/version-history | (none collected) | **Copy:** CSV round-trip as an escape hatch for bulk edits |
| **OneTagger** | **Closest stack precedent.** Rust backend, Vue 3 + Quasar frontend, WebView2 on Windows, plus a CLI, GPL-3.0 [V] https://github.com/Marekkon5/onetagger. Sources: Beatport, Traxsource, Juno, Discogs, iTunes, MusicBrainz, Beatsource, Spotify, Bandcamp, Musixmatch, Deezer, Shazam ID; custom platforms; auto-rename templates; inputs are folders or M3U [V] https://onetagger.github.io/ | DJ-centric. Its Spotify "audio features" feature depends on an endpoint Spotify closed to new apps on 2024-11-27 [S] https://developer.spotify.com/blog/2024-11-27-changes-to-the-web-api | **Copy:** a Rust + webview architecture, ordered fallback across sources, "fill empty only" vs overwrite. **Avoid:** Spotify-dependent features |
| **wrtag** (Go) | "Simpler, more composable, faster" than beets; copy, move, reflink; `sync` re-tag; add-ons for lyrics (LRCLib, Genius, Musixmatch) and ReplayGain; `wrtagweb` queues imports that need confirmation; GPL-3.0 [V] https://github.com/sentriz/wrtag | (none collected) | **Copy:** confirm/correct/reject queue for imperfect matches; reflink copies |
| **Harmony** | Merges release data from MusicBrainz, Spotify, Deezer, Bandcamp, Beatport, iTunes, Tidal and others, keyed by GTIN or URL (Deno) [U] https://gitea.susano-homelab.duckdns.org/fujin/metadata-agregator/raw/branch/master/docs/research/harmony/README.md | (none collected) | **Copy:** a multi-provider merge when MusicBrainz lacks the release |
| **Soundiiz / TuneMyMusic** | TuneMyMusic exports an Apple Music library or playlists to CSV/TXT [V] https://www.tunemymusic.com/transfer/apple-music-to-file. Matching uses title, artist, album and ISRC [S] https://www.tunemymusic.com/transfer/csv-to-apple-music | Soundiiz free tier: 200 tracks per playlist, one playlist at a time [U] https://routenote.com/blog/soundiiz-transfer-playlists-between-spotify-apple-music/. Reported "at best 96%" accuracy, parentheses break matches, single vs album version swaps go unflagged [U] https://ca.trustpilot.com/review/soundiiz.com?page=7, https://www.playlistgo.io/reviews/tunemymusic-review/ (written by a competing vendor) | **Copy:** an unmatched report. **Do better:** also flag *version substitutions*, not just misses |
| **New, 2026** | **dapctl** (Rust TUI, GPL-3.0): diff-based DAP sync, seven device profiles, firmware path limits, blake3 verification, ffmpeg transcode cache, m3u export, offline by default [V] https://docs.rs/crate/dapctl/1.0.0. **dapsync** (Navidrome to SD card, web UI with preview, `.m3u8`, incremental, Mar 2026) [V] https://forums.unraid.net/topic/197858-plugin-dapsync-sync-music-from-navidrome-to-your-dap-sd-card/. **Beetkeeper** (beets web UI, AGPL, Jun 2026) [U] https://awesome.ecosyste.ms/projects/github.com%2Fzach-overflow%2Fbeetkeeper | (none collected) | **Copy:** dapctl's device-profile spec and diff-before-copy |

**Lyrics source:** LRCLIB is reportedly free, needs no key, and `/api/get` takes track, artist, album and duration and prefers synced lyrics [U] https://pypi.org/project/lrclibapi/0.2.1, https://glama.ai/mcp/servers/e5ntqop63f.

### Gaps nobody fills well
This list is my synthesis from the evidence above.
1. **Library-wide normalisation as a reviewable, undoable change set.** beets and Picard apply changes per import or batch; no surveyed GUI shows a diff queue with rollback across the whole library.
2. **A beets-grade automated pipeline in a native cross-platform GUI.** beets is CLI-only with Windows path pain; MusicBee is Windows-only; Mp3tag on Mac lacks web sources; Picard needs ~200-file batches.
3. **Matching a streaming playlist to local files and writing m3u.** No single open-source tool does ISRC-first plus fuzzy matching against local files with output to m3u [S] https://rekordbox-set-list-manager.readthedocs.io/en/v1.0.0/_modules/rekordbox_set_list_manager/services/track_matcher.html (ISRC, then exact artist+title, then fuzzy token_sort, then filename; Rekordbox-only), https://pypi.org/project/sssync/ (ISRC, then fuzzy plus duration; Jellyfin/Qobuz).
4. **Apple Music for Windows export.** There is none (see Part B).
5. **DAP sync with device profiles, relative m3u8 and transcoding in a GUI.** Today it is a TUI (dapctl) or Navidrome-bound (dapsync). Relative paths are the usual cause of "empty playlist" [U] https://forums.sandisk.com/t/winamp-playlists-showing-up-as-empty-on-fuze/40752?page=2, https://fiio.com/newsinfo/69522.html.
6. **Non-Latin handling beyond MusicBrainz aliases.** Users want a per-artist override that survives re-import; the beets workaround is a `replace:` config entry [U] https://discourse.beets.io/t/how-to-override-one-artist-name/1970.
7. **Metadata resilience.** Lidarr's single-server outages show the risk of a single point of failure.

---

## Part B: Apple Music export

**1. Apple Music app for Windows: no export.**
- An Apple Developer Forums post (Sep 2025, about 1k views) found no Export Library or Export Playlist in the Windows app and no API, noted that `Library.musicdb` starts with `hfma` and is not SQLite, and got **zero replies** [V] https://developer.apple.com/forums/thread/802317.
- An Apple Community answer says there is no native export of user playlists in the Win11 app [U] https://discussions.apple.com/thread/255829217.
- Apple's official transfer goes **only to YouTube Music**. It covers only playlists you created, excludes curated and shared playlists, and needs an active subscription [S] https://support.apple.com/120030.

**2. macOS.**
- File > Library > Export Playlist (XML or Text) and Export Library (XML) are documented [V] https://support.apple.com/en-za/guide/music/mus27cd5060f/mac.
- "Share Library XML with other applications" was removed in Catalina; export is now manual and one-off [U] https://appleinsider.com/articles/19/10/07/apple-warning-djs-to-stick-with-macos-mojave-for-now.
- Scripting the Music app exposes persistent ID, rating and play count [U] https://www.macscripter.net/t/apple-music-export-playlist/72126. Persistent IDs differ between libraries [U] same.

**3. Library.musicdb decoders and format.**
- **Format:** a clear-text `hfma` envelope, then an AES-128-ECB payload (up to a Max Crypt Size), then zlib inflation.
- **Sections:**
  - `hsma`: section boundaries
  - `plma`: library master
  - `lama`/`iama`: albums
  - `lAma`/`iAma`: artists
  - `ltma`/`itma`: tracks
  - `lPma`/`lpma`: playlists
  - `boma`: data blocks (UTF-16 strings, a numeric block, file-path "book" data, playlist items under `ipfa`)
- **Where the fields are:** rating is at `itma`+65 (0–100 in steps of 20). Love/dislike is at `itma`+62 (2 = love, 3 = dislike). Date added is in the numeric `boma` at +112 (seconds since 1904). Play count and last played are not documented there. Page last updated Dec 2019, no key published [V] https://home.vollink.com/gary/playlister/musicdb.html.
- **Decoders:**
  - **amlx** (Python, MIT, 2026). Reads a temporary copy. **The user supplies the key.** Exports play count, last played, rating, favorite, date added, cloud/local status, file path, persistent ID, playlists (including smart and folders) with ordering, and artwork [V] https://github.com/TwinvisPR/amlx.
  - **musicdb-to-json** (MIT). The user supplies the key; the README says it is "published in several similar projects… not included here" [V] https://github.com/jsharkey13/musicdb-to-json.
  - **AppMusicLibParser / MusicParser** (.NET, GPL-3.0, Jan 2026). The key is "not provided with the code for legal reasons". Parses play count, rating, date added and likes [V] https://www.nuget.org/packages/appmusiclibparser.
  - **Apple-Music-Editor** (Python, MIT, also *writes* the file). **The key is in the source** ("see code") [V] https://github.com/achen1296/Apple-Music-Editor.
  - **musicdb2sqlite** (WIP) [V] https://github.com/rinsuki/musicdb2sqlite.
- **Takedowns and legal commentary:**
  - I found no takedown aimed at any musicdb project (absence of evidence only).
  - Closest precedent: in 2008 Apple's counsel had BluWiki pull the iTunesDB/iPodhash pages. EFF sued (2009), arguing users own their database files and the pages were not a circumvention "device", and Apple withdrew its demands [S] https://www.theregister.com/2009/04/27/eff_sues_apple/, https://techcrunch.com/2008/11/26/apple-sends-baseless-takedown-notice-to-hackers-discussing-itunesdb-code/amp/.

**4. After cancelling.**
- Catalog songs stop playing.
- "Any playlists added from the Apple Music catalog" become unavailable.
- Sync Library turns off.
- Music and playlists from before subscribing remain.
- No grace period is stated [V] https://support.apple.com/en-us/HT212047.

  **Implication:** export *before* cancellation, and keep the catalog-only tracks as "wanted" entries.

**5. Web API and MusicKit.**
- Every request needs a developer token signed with a MusicKit key from a **paid Apple Developer Program membership** [S] https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens. An open-source desktop app can't ship that key safely.
- Library songs carry **no ISRC**; you have to use `include=catalog` to reach the catalog song [U] https://developer.apple.com/forums/thread/132606.
- Catalog lookup accepts `filter[isrc]` but is per storefront [S] https://developer.apple.com/forums/thread/716866.
- Curated/catalog playlists can be read by ID [S] https://developer.apple.com/documentation/applemusicapi/get-a-catalog-playlist.
- A Chrome "Playlist Exporter" scrapes public playlist links to CSV/JSON (v0.1.1; its open-source claim is unverified) [U] https://chromeboard.com/extension/playlist-exporter-bapphgjdlipcpledcboppjnokpimflng.

**6. Matching streaming tracks to local files.**
- Common pattern: ISRC exact match, then normalised artist+title exact, then fuzzy (token_sort / Jaro-Winkler) with a duration penalty, then filename [S] rekordbox matcher link above; [S] https://www.nuget.org/packages/Ciderfy.
- Known failure modes: parentheses and versions, single vs album version, non-English titles [U] Trustpilot and PlaylistGo links above.
- amlx's field list includes no ISRC, which suggests musicdb-sourced matching will be metadata-only (inference).
- **Recommendation:**
  - Normalise: NFKC, case-fold, strip "feat." / "(Remastered …)" / "- Single".
  - Gate on duration within ±2–3 s (my threshold, not sourced).
  - Use album only as a tiebreaker.
  - Send anything below a confidence threshold, and any version mismatch, to the review queue.

### Recommended Apple export approach
- **Primary on macOS:** guide the user through File > Library > Export Library (XML), then parse it. This is documented, official and carries no legal risk.
- **Primary on Windows:** an opt-in, read-only musicdb reader working on a temporary copy, with **the user supplying the key** (the amlx / musicdb-to-json model). Keep it as a separately packaged module.
- **Fallback (all platforms):** a generic importer for CSV/TXT/M3U from TuneMyMusic, Soundiiz, browser exporters or Apple's Text export.

### Legal risk of shipping a musicdb decoder (not legal advice; needs counsel)
- **Low–moderate** if the key is *not* shipped and the tool is read-only, local and metadata-only:
  - The protected content is arguably the user's own library data. That was EFF's iTunesDB argument, and Apple backed down.
- **Higher** if the key is embedded in the source:
  - Apple is in the 9th Circuit, where *MDY v. Blizzard* removed the need to tie circumvention to infringement. The Federal Circuit's *Chamberlain* does require that link, so the courts are split [S] https://newmedialaw.proskauer.com/2011/01/04/ninth-circuit-says-dmca-anticircumvention-provision-gives-new-access-prevention-right-to-copyright-owners-mdy-v-blizzard-part-ii/.
  - The §1201(f) interoperability exemption was read narrowly in the DeCSS case (*Reimerdes*), as covering programs, not other works [S] https://www.copyright.gov/1201/comments/221.pdf.
  - Apple's iTunes for Windows licence forbids "decrypting" except where law prohibits that restriction [S] https://apple.com/legal/sla/docs/iTunesForWindows.pdf.
  - In the EU, Directive 2009/24 Art. 6 and Recital 17 allow decompilation for interoperability and make contrary contract terms void [S] https://www.legislation.gov.uk/eudr/2009/24/article/6.
- **Mitigations:**
  - Don't distribute the key.
  - Never write to the file.
  - Label the feature "interoperability with your own data".
  - Keep the module separable so a takedown can't sink the main repo.
  - Get counsel before release.
