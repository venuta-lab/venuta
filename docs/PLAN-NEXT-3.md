# Next, round 3: speed, lyrics, search, and sharing through Plex

Chosen 2026-10-13 from research 09: make synced lyrics, lyrics on the big screen, search across
scripts, and everything for speed. Added: sharing with family and friends, collaborative
playlists, and making use of his Plex Pass. Everything else from research 09 is set aside.

## 1. Speed and a better experience (first, and measured): done 2026-10-13
Measured on a copy of his library (416 albums, 2,630 songs, 4,934 files, 24 playlists; the
songs hard-linked from D:\Music\Music so nothing of his is written), release builds, the
previous version (33a10dc) against this one on the same copy. Commands timed from the page,
median of repeats; start is process start to album covers on screen, median of five.

| | before | after |
|---|---:|---:|
| Start to covers on screen | 1.03–1.28 s | 0.88 s |
| First paint | 292 ms | 204 ms |
| Playlists home | 410 ms | 45 ms |
| An 822-song playlist | 75 ms | 11–14 ms |
| An album | 43 ms | 2–3 ms |
| Music catalog (all songs) | 72 ms | 22–28 ms |
| Health | 70 ms | 20–25 ms |
| Playlist shelf on the Music home | 110 ms | 6–8 ms |
| Theme switch, to the next frame | | 12–32 ms |
| Search, per keystroke | | 6–17 ms |
| Now Playing opens | | about 20 ms |

- [x] **Measured** (above). The first start on a new index also failed before: about 16
      commands opened the new index at once and the switch to WAL, which SQLite doesn't wait
      for, gave "database is locked". It now retries; listening and the web cache also wait.
- [x] **The library stays in memory** (`app/src-tauri/src/libcache.rs`): built once, used until
      the index's change counter moves (schema v6: triggers on every table a library is built
      from, so any writer counts). Scans of a folder run one at a time, and only when the
      folder watcher saw a change (or every 5 minutes, or without a watcher). Building the
      library alone took 291 ms; a scan that finds nothing, 47 ms.
- [x] **Apple Music's library read once**, again when its file, the key or the file chosen
      changes. Smaller than thought: decrypting his library takes about 25 ms.
- [x] **Playlists does work only when something changed**: the real cost was each playlist
      read building a lookup of every song (16 small strings per checksum, 120 times a visit);
      the library now builds it once. Smart playlists that say the same are no longer
      rewritten on every visit (that also made every visit a change to rescan).
- [x] **Faster start**: Settings, Organize, Devices, Activity, first-run setup and Import load
      when first opened; the start loads 638 KB of script instead of 764 KB.
- [x] Covers, theme switching, search and Now Playing measured: nothing slow enough to change.
      `crates/engine/tests/timings.rs` times each Playlists step on a copy of a library.

## 2. Search across scripts: done 2026-10-13
- [x] Searching "kesariya" finds केसरिया and the other way round, for titles, artists, albums,
      playlists and lyrics (`app/src/lib/textsearch.ts`): Devanagari read in Roman letters by
      rule, with Hindi's dropped "a" (dhadkan, zindagi, kesariya); Gurmukhi, Bengali, Gujarati,
      Odia, Tamil, Telugu, Kannada and Malayalam read through the same table. Names and queries
      fold to one loose spelling (aa/a, ee/i, ph/f, w/v, z/j, iya/ia, doubled letters), and a
      word of 5 letters or more may be off by one ("arjit"); exact results come first.
- [x] The same search in Music, the Metadata wall, Ctrl+K, a playlist's Find, Add to playlist,
      the Playlists filter and artist names.
- [x] **In the lyrics**: Music's search also finds songs by a line of their words, shown in its
      script and in Roman letters. His library: names are all in Roman letters, but 1,142 of the
      1,910 songs with lyrics are in Devanagari. Reading them takes 0.2 s, folding 46,507 lines
      0.8 s in slices (no frame longer than 17 ms), then 8 ms a key.

## 3. Lyrics on the big screen, and on the desktop: done 2026-10-14
- [x] **Big screen** (Now Playing's screen button, F11, the palette): its own full-screen
      window (`#stage`): the cover blurred as light, the line being sung large with the one
      before and the next coming in, the song and progress small at the bottom. The pointer and
      two corner buttons (Style, Close) show only while the mouse moves; Esc closes, T switches
      Roman letters, Space plays and pauses.
- [x] Roman letters under each line in an Indian script, from search's own rules (a switch in
      the style panel, and T).
- [x] With more than one screen, the button asks which (each drawn in its own shape).
- [x] **Desktop lyrics** (Now Playing, the tray, Ctrl+Shift+D from anywhere): a transparent,
      always-on-top window with no frame; locked, clicks go through it (checked with
      WindowFromPoint). Unlocked (the tray's "Move desktop lyrics", or the palette) it shows a
      dashed edge and one dark-glass bar: Lock in place, the look, colour, size, one line or
      two, close. It remembers where it was (back on screen if a monitor went), fades after 4 s
      paused. Its button in Now Playing lights while it shows.
- [x] **Looks**: Clean, Karaoke (fills word by word in reading order, however the line wraps),
      Glow, Outline, Serif, Theme; size S to XL; the cover's colour, the accent or white. One
      choice for both, shared live between windows. Chosen in a **style panel** that draws each
      look as itself on a stage lit by the cover: in Now Playing (the app's glass), on the big
      screen (dark glass), and as a one-row strip on the desktop lyrics.
- [x] **Now Playing, redesigned around it**: the lyrics' tools sit beside the tabs in the tabs'
      own pill (style, timing, big screen, desktop), named on hover with their keys. The song's
      side is a way in: every artist and the album open their pages (the bar's artist names
      now do too), with Add to playlist, Go to album and More (each artist, timing, Show in
      folder, Search the web).

## 4. Make synced lyrics: done 2026-10-14
- [x] For a song with plain lyrics (or pasted ones: Now Playing offers it when there are
      none): play it and press Space as each line begins; Backspace steps back (rewinding no
      further than the line above), B marks a break in the singing; nudge any line ±0.1 s
      (Shift ±0.5) by ear, R or a click plays from it. Timed songs can be fixed the same way.
- [x] Saved as a timed .lrc beside the song (`crates/engine/src/lrc.rs`), as a change Activity
      lists ("Lyrics timed") and undoes exactly: the .lrc that was there before comes back byte
      for byte, or the new one goes. **Not in the song's tags**: that would rewrite the song
      file for words every player already reads from the .lrc; say if you want tags too.
- [x] Health: "Lyrics without timing" (words in the tags or in a .lrc beside the song, with no
      times, and nothing timed found online): 230 songs in the test copy. See songs lists them,
      each with Time it (plays it and opens the maker).

## 5. Plex: Venuta keeps the library, Plex shares it (later: research and brainstorm first)
His Plex Media Server is installed on this PC, so Plex can serve the very library Venuta keeps
tidy, to his phone (Plexamp) and to the people he shares it with, anywhere.
- [ ] **Connect to Plex** the way Plex asks desktop apps to sign in (a code shown in Venuta,
      approved at plex.tv; the token kept in the Credential Manager, like Spotify's). Venuta
      finds the server and its music library, or offers to make one pointing at the library.
- [ ] **Plex sees changes at once**: after a build, an import or an Organize change, Venuta asks
      Plex to rescan only the folders that changed.
- [ ] **Synced lyrics in Plexamp**: Plex shows only lyrics in sidecar .lrc files (not ones
      inside the tags), so Venuta writes one beside every song with synced lyrics, named and
      encoded the way Plex needs (lowercase .lrc, UTF-8).
- [ ] **Your playlists in Plexamp**: each playlist (and smart playlist) kept in step in Plex, so
      it plays on the phone too; changes made in Plexamp come back into Venuta's .m3u8, merged
      the same way imports are (nothing of either side lost).
- [ ] **One history**: plays and ratings from Plexamp (his phone, the car) come back into
      Venuta's history and stars, and stars set in Venuta show in Plexamp.

## 6. Sharing with family and friends (later, with 5)
Plex has no shared music playlists (Plexamp users have asked since 2020), and lists are for
films only. Venuta can do it, because it already merges two versions of a playlist safely.
- [ ] **Shared playlists for the family**: mark a playlist "shared with" people in your Plex
      Home. Venuta keeps a copy in each person's Plex account, collects what each of them added or
      removed in Plexamp, merges it (three-way, as imports already do), and sends the result back
      to everyone. A real collaborative playlist, on everyone's own phone.
- [ ] **Who added what**: each song of a shared playlist remembers who added it, shown on the
      row.
- [ ] **For friends** (who see your library through Plex but aren't in your Home): share a
      playlist as a Plex collection they can play, kept up to date.
- [ ] **Share a song, album or playlist outside Plex**: one link that opens in whatever app a
      friend uses (song.link: Spotify, Apple Music, YouTube Music and more; free), as a message to
      paste or a picture card (cover, title, a QR code) for WhatsApp. A playlist goes as a list
      of those links, plus a file that TuneMyMusic and Soundiiz import into any service.
- [ ] **What the family is playing**: with plays coming back from Plex per person, a "Family"
      shelf: what each person played lately, the songs everyone loves, and each one's year in
      music if they want it.

## Decisions for him
- **Plex server**: this PC's (installed, not running now)? Does it already have a music library,
  and on which folder?
- **Who**: which people are in your Plex Home (family) and which are friends with access?
- **Playlists in Plex**: all of them, or only the ones you choose?

## Order of work
1, then 2, 3 and 4 (search and lyrics share the transliteration). 5 and 6 after research.
