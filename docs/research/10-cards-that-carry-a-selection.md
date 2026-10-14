# 10: Cards that carry a selection (some playlists, or everything)

Asked 2026-10-13: "If I want to share music but not all, only a few playlists, we should be able to
put them on a card as well, or the entire collection." Then: "Those songs should be synced but
still keep the directory structure as per the rules. An album might not have all its songs, but
it will still be a structured, organised library, so that person can start building his own
library from there, knowing what is missing."

Read 2026-10-13. Head-Fi, the Poweramp forum, JRiver's wiki and Audirvana's forum were partly
blocked from here; claims marked **[test]** need a two-song card in the real player before we
rely on them.

## What Venuta does today (code, not plans)

- `crates/engine/src/sync.rs`: a card carries `.venuta/card.toml` (id, name, folder, include,
  player, auto-sync) and `.venuta/manifest.sqlite` (every file Venuta wrote, with its source size,
  mtime, audio MD5 and recipe stamp). The plan diffs by content: unchanged, copy, patch tags in
  place, move, adopt a matching FLAC already on the card, delete what left. Writes go to `.part`,
  are flushed and renamed. Files Venuta didn't write are never touched.
- **Selection is top-level library folders only.** No playlists, albums, artists or rules.
- **Playlists go to every card all or nothing**; each is rewritten for the card with the songs
  not on it left out, and `#VENUTA-WANT` lines dropped. A playlist's songs are **not** pulled
  onto the card.
- Five player profiles (`device.rs`) change tags and cover size; none converts audio, shortens
  paths, or changes separators. The player test kit decides where playlists go (beside the
  music, at the top, or none).
- Missing: undo or a card journal, read-back verify, volume-serial check, conversion, path-length
  and illegal-name handling, gift-card extras.

## How other tools choose what goes on a device

- **MusicBee**: playlists and folders chosen are added together; "Synchronise music" (everything)
  is on by default, untick it to sync by playlist. ([MusicBee wiki](https://musicbee.fandom.com/wiki/File:Play_logo-64.png))
- **iTunes classic**: selected playlists, artists, genres and albums, plus "fill free space". Its
  famous failure: when the picks didn't fit and the user accepted auto-select, it threw the picks
  away and synced from every playlist. ([Apple help](https://help.apple.com/itunes/mac/10.0.1/Contents/en/searchindex/itns26841.html),
  [discussion](https://discussions.apple.com/thread/2361355)). Users asked to see each playlist's
  size on the sync screen. ([Kirkville](https://kirkville.com/iwant-itunes-sync-screen-should-display-the-size-of-playlists-albums-etc/))
- **JRiver**: "delete files not in sync list" wiped people's maps and documents; JRiver narrowed it
  to its own media types. Winamp asks before deleting. ([JRiver](https://wiki.jriver.com/index.php/Handheld_Sync_Options))
- **foobar2000**: FoobarSync syncs a query one way, with conversion rules, path templates and limits
  by size or free space, and keeps its settings on the target. ([FoobarSync](http://foobarsync.lima-city.de/wikka/HomePage/))
- **beets-alternatives**: a collection is a query; `update` adds what matches, moves what changed
  path, removes what no longer matches. ([GitHub](https://github.com/geigerzaehler/beets-alternatives))
- **Symfonium**: offline rules per playlist, artist, genre or smart playlist; a file leaves when
  no rule covers it any more; "pin" keeps it regardless. ([docs](https://docs.symfonium.app/wiki/other/offline-media-cache-downloads-rolling-cache-playback-cache-automatic-cache-and-more/))

**Take-away**: a card is a saved list of rules (the union is what goes), each file once however
many playlists hold it; show sizes before writing; never let "make it fit" replace explicit picks;
remove only what we wrote and no rule covers any more.

## Playlists on cards, player by player

| Player | Playlists | Notes |
|---|---|---|
| Poweramp (RG Rotate) | M3U8, relative paths read | Matches an entry by last folder + file name, so a line for a missing song can land on a different file with the same name. Rewrite playlists in place under the same name, or it shows duplicates. ([forum](https://forum.powerampapp.com/topic/7220-are-relative-paths-supported-in-playlists/), [forum](https://forum.powerampapp.com/topic/19544-songs-missing-from-playlists/)) |
| Echo Mini | none | Folders only. **Cards up to 256 GB, FAT32/exFAT (no NTFS)**: the 1.5 TB card won't work in it. No ALAC or Opus. ([FiiO](https://www.fiio.com/echomini_parameters)) |
| Snowsky Disc | none documented **[test]** | Folder browsing; cards up to 2 TB; covers <= 1.35 MB. |
| Astell&Kern | **[test]** | Library from tags, scanned on insert; older models wanted `.pls` with CRLF in a Playlists folder; some users see imported playlists empty. ([Bloom Audio](https://bloomaudio.com/blogs/articles/how-do-i-get-music-onto-my-astell-kern-dap), [Head-Fi](https://www.head-fi.org/threads/astell-kern-a-norma-sr15-syncing-playlists.918911/)) |
| Car stereos | usually none | Sony: no M3U. Volvo: FAT32, 15,000 files, 1,000 folders, 8 levels; audio files only. ([Sony](https://www.sony.ca/en/electronics/support/car-marine-entertainment/articles/00137380), [Volvo](https://www.volvocars.com/lb/support/car/v40/2015/article/85b7292d40af4726c0a801e800118eb2/)) |

General: M3U8 is UTF-8, `#EXTM3U` first, paths relative to the playlist, `/` (some old players want
`\`), CRLF for a few devices; never mix separators; brackets break VLC's relative paths.
([Wikipedia](https://en.wikipedia.org/wiki/M3U), [Roon](https://community.roonlabs.com/t/most-but-not-all-m3u-playlists-not-being-imported/158949))

Because so many players browse folders and ignore playlists, **the library's own folder layout is
what makes a selection card usable everywhere**, which is exactly what was asked for.

## Limits of the card itself

- FiiO: path + file name at most 256 characters. FAT32: 4 GB per file, 65,535 entries per folder.
  exFAT: 255-character names. ([FiiO](https://fiio.com/newsinfo/63364.html), [Microsoft](https://learn.microsoft.com/it-it/previous-versions/technet-magazine/cc160978(v=msdn.10)))
- Names: replace `\ / : * ? " < > |`, trailing dots and spaces; one Unicode form.

## Making a selection fit

| Format | Roughly |
|---|---|
| FLAC 24/96 | 20-25 MB a minute |
| FLAC 16/44.1 | 6-7 MB a minute (a 24/96 song drops to about a third, still lossless for every player) |
| AAC ~256 kbps | 1.9 MB a minute (phones, cars) |
| Opus 128 kbps | 1 MB a minute (near transparent, but the Echo Mini can't play it) |

([Xiph](https://wiki.xiph.org/Opus_Recommended_Settings), [Hydrogenaudio](https://hydrogenaudio.org/index.php/topic,114791.0.html))
Conversion keeps the same path with a new extension, so playlists stay right.

## Partial albums and "what is missing"

- beets `missing` lists absent tracks per album from MusicBrainz; Picard colours albums by
  completeness. ([beets](https://beets.readthedocs.io/en/stable/plugins/missing.html))
- **Never put counts in folder names** ("[2 of 12]"): folders would be renamed as songs arrive and
  playlists would break. Keep TRACKNUMBER/TRACKTOTAL so players show "3/12".
- A `Tracklist.txt` in a partial album folder (every track, present or missing) and a card-level
  `Missing.csv` / README tell a person what to get next.
- In playlists, a missing song is a plain comment line, never `#EXTINF` + a path (no dead entries,
  no wrong-file matches in Poweramp); Venuta can still read it back.
- Giving copies to others is not clearly covered by India's personal-use fair dealing (s.52(1)(a));
  a gift card carries a short neutral note, nothing is blocked. ([BananaIP](https://bananaip.com/ip-news-center/copyrights-fair-dealing-fair-use-india))

## Design requirements (carried into the plan)

1. A card is a saved **selection**: whole library, or rules (playlists, albums, artists, smart
   rules), unioned; each file once.
2. Files keep **the library's layout**, partial albums included; playlist membership never decides
   a file's place.
3. **Only what Venuta wrote** is updated or removed; a song leaves when no rule covers it, after a
   preview; an album folder is removed only if Venuta made it and it is empty.
4. A **planner** before writing: size per rule, total, free space, a reserve; when it doesn't fit,
   offer 16/44.1, AAC/Opus, or dropping the lowest rule. Never silently replace picks.
5. **Per-player** playlist writer and format policy; players without playlists get folders only.
6. **Missing songs** recorded for people (README, Tracklist.txt, Missing.csv) and for Venuta
   (comment lines, and the card's own playlists with their wants), so the card is a seed library.
7. **Path safety** for every card filesystem, car limits on car profiles.
8. **Recognise the card** on insert and offer the update with what changes and the net size.
