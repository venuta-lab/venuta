# 0006: Whole playlists, from any source

Date: 2026-10-13. Status: accepted.

## Context

His Apple Music playlists showed only the songs his library had (Hindi: 268 of 398, English:
4 of 209). The first Apple importer kept only matched songs, and dropped the rest with their
places. Two more ways to lose songs were found while looking at the problem, and both affected
every playlist, whatever its source: saving a playlist dropped any line whose file wasn't in the
library, and an `.m3u` left by another player couldn't be opened at all. Importing a playlist
again could only replace the old one or sit beside it, so an import could never complete one,
or follow a source that changed.

## Decision

1. **No song is ever dropped.** A playlist line whose file isn't here becomes a wanted song
   (`#VENUTA-WANT:{json}`), with what the line says about it: the `#EXTINF` artist, title and
   length, else the file and folder names (`Dil Se (1998)/01 - Chaiyya Chaiyya.flac` gives title
   and album). A file that went away keeps its audio checksum (`md5`), and the same audio back
   anywhere in the library is that song again. Full paths into the library count. `.m3u` files in
   any encoding (Windows-1252 included) are read, and become `.m3u8` on their first save (the old
   file goes to Set aside).
2. **One importer.** The first Apple importer is gone; the CLI and the app both use the import
   review (`imports::preview` and `apply`).
3. **A playlist remembers where it came from.** Two more comment lines, ignored by players and left
   off cards:
   - `#VENUTA-SOURCE-ID:` names the playlist at its source: `spotify:<id>`, `deezer:<id>`,
     `apple:<name>`, `file:<path>|<name>`.
   - `#VENUTA-BASE:` holds the source's songs as they were at the last import, as short keys.
4. **Importing a playlist you have updates it** (the default; Keep both and Replace remain). The
   target is the playlist with the same source id, else the same name as a file name (so
   `still loves...` is `still loves`). The merge:
   - the source's order is the spine;
   - a song you have keeps the exact file it uses (a song you picked by hand stays your pick);
   - a waiting song becomes its file once the import finds it;
   - songs you added stay, right after the song they followed;
   - with a base from the same source: a song you took out stays out, and a song the source
     dropped goes. Without one (an older import), nothing is taken out.
   Songs are the same when they share a key: the file, the ISRC, or the title (film and feature
   notes ignored) with any one of their artists. The old version is copied to Set aside first, and
   the done screen offers Undo.
5. **Update from source** (a playlist's menu) reads the source again by its id, and goes through
   the same review. The Playlists home offers, once, to complete playlists that Apple Music's
   library (read on this computer, nothing else is checked unasked) has more songs of.
6. **Close matches are offered, never filled in silently.** Opening Playlists fills in exact
   matches only. A missing song with a close one (another version, length or credit) shows
   "Maybe …" with Yes and No. No is kept in the playlist (`not`), so it isn't offered again.
7. **Apple Music's library file gives song lengths**: the numbers record (`boma` sub-type 1)
   holds the length in ms at +176. It was checked against songs of known length, and matched files
   agree to the millisecond. Every one of his 3,459 playlist entries has one.

## Playing

- Playlists live in Music, as its Playlists tab; the separate place left the rail (Ctrl+1 to
  Ctrl+5 now).
- **Play** plays in order with shuffle off. **Shuffle** turns the player's shuffle on and starts
  on a random song, so the bar shows it and turning it off goes back to the playlist's order.
  Picking a song keeps the shuffle as it is.
- The queue says where it came from (`from` is JSON: kind, ref, label). "Playing from …" in the
  queue and Now Playing leads back to the playlist or album.
- A playlist edited while it plays changes what comes next (`Cmd::Follow`). The song playing
  plays on, and nothing happens once you have changed the queue by hand.
- Shuffle and repeat are kept across restarts, with the exact shuffled order and the order to
  go back to.
- Plays remember the playlist they came from, so Jump back in mixes playlists with albums.
- A playlist page draws only the rows on screen. 822 songs open in about 0.1 s, against 7.7 s
  when every row was a drag item. Rows move by dragging the handle, with the screen scrolling at
  its edges, or by Alt+Up and Alt+Down.

## Checked

- Engine tests: nothing dropped (gone files, other computers' paths, unknown paths on save), the
  checksum bringing a song back, `.m3u` in Windows-1252, update by name with yours kept and
  Undo, the three-way update (renamed, linked by source id), a hand-picked song kept, the
  `still loves...` name, close matches offered and a No remembered, playlists short of their
  source. Queue tests: following an edit (shuffled too), a shuffled order surviving a restart.
- In the app, on a scratch library: an `.m3u` from another player, a CSV import updating a
  partial playlist, its Undo, a later update with both sides changed, Update from source, the
  Apple banner and review on his real Apple library (read only, 21 playlists, 6.3 s), writing
  all 21 (3.3 s), dragging, Alt+Up, Maybe Yes and No. Playback, muted: Play in order, Shuffle on,
  a removal while playing (the song carried on), Playing from, a restart keeping shuffle,
  repeat and order, Jump back in. Light, dark, Neo-Tokyo, and a 760 px window.
