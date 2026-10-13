# Playlists, complete

Goal: playlists you can play, build and bring over from anywhere, with every song accounted
for. Then the full UI polish pass (docs/PLAN-POLISH.md) covers these screens too.

## What exists (2026-10-13)
- `.m3u8` files in `<library>/Playlists`, paths relative to the playlist, each entry with its
  audio MD5 so moved files are found again; smart playlists from TOML rules; Apple Music library
  import (key needed); a want-list; sync to cards per player profile.
- Missing: playing a playlist; adding songs from where you are; songs not in the library are
  dropped (no trace); only one import source; the screen is a settings-style editor.

## Decisions
1. **Songs not in your library stay in the playlist**, in order, as `#VENUTA-WANT:` comment lines
   (title, artist, album, duration, ISRC). Other players ignore them; Venuta shows them as
   "not in your library", can match them later, and lists their albums on the want-list.
2. **Where it came from** is kept in the file (`#VENUTA-SOURCE:Spotify 2026-10-13`).
3. **Matching** is graded, never guessed:
   - *exact*: same ISRC, or same title and artist (album or duration settles twins);
   - *close*: same song by a looser key (remaster/feat/"From …" notes, a different version
     word, durations within 3 s): you confirm these;
   - *not here*: kept as a want entry.
4. **Import sources** (all keyless, nothing is downloaded):
   - Apple Music library (existing), and Apple Music / iTunes exported playlists (.xml, .txt);
   - Spotify account data (`Playlist1.json`, `YourLibrary.json`) and Exportify CSV;
   - Deezer public playlist link (Deezer's public API);
   - YouTube Music via Google Takeout (`*.csv` of video IDs; titles from YouTube's public
     oEmbed endpoint);
   - TIDAL, Amazon and the rest via the CSV that TuneMyMusic / Soundiiz export;
   - any `.m3u`, `.m3u8` or `.pls` from another player.
5. **Editing saves as you go** (reorder, remove, add), with Undo for the last change;
   delete moves the file to Set aside.
6. **Playing**: a playlist plays like an album (click a song, Play, Shuffle, Play next, Add to
   queue). Its title, mosaic cover and glow come from its songs.

## Steps
1. Engine: entries with wants and source; graded matcher (+ tests); parsers for every source
   (+ tests); import preview/apply; duplicate, export.
2. App commands: preview, apply, resolve a missing song, duplicate, export, add-to-playlist.
3. Shared catalog (lib/catalog.ts) so any screen can play library paths.
4. Playlists place rebuilt: grid of playlists (mosaic covers, grouped: smart, yours, imported),
   playlist page (Detail frame, Play/Shuffle, rows like albums, drag to reorder, remove, missing
   songs with "find it"), smart rule editor, want-list page, import sheet with review step.
5. Add to playlist from everywhere: song and album right-click menus, Now Playing, the queue
   ("Save as playlist"), the palette.
6. Music home: a "Your playlists" shelf.
7. Verify every flow in the running app with real files from each source; commit.
