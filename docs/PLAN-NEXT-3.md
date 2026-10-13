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

## 2. Search across scripts
- [ ] Searching "kesariya" finds केसरिया and the other way round, for titles, artists, albums,
      playlists and lyrics: every name is also indexed in Roman letters by rule (Devanagari
      first: Hindi, Marathi; the same for Gurmukhi and the other Indian scripts), and a typed
      query matches loosely ("kesariya", "kesaria", "kesariyaa" all find it).
- [ ] The same search in Music, Ctrl+K and the song finders (Add songs, Find).

## 3. Lyrics on the big screen, and on the desktop
- [ ] A full-window lyrics view (from Now Playing, or F11): the cover's colours, large synced
      lines, the next line coming in, the song and progress small at the bottom. Made for a TV or
      a second screen, readable from the sofa.
- [ ] Lyrics in Latin script as a switch there, from the same rules as search (so a Hindi song
      can be sung along to by anyone).
- [ ] Choose which screen it opens on (a second monitor or the TV); Esc closes it.
- [ ] **Desktop lyrics**: the lines alone, floating over the desktop and every window: no
      window, no background, clicks pass through to what is under them. Drag to place (it
      remembers), lock it, size it; one line or two; it fades when nothing plays. From the tray,
      the mini player or Ctrl+Shift+D.
- [ ] **Lyrics looks**: a few styles for both modes, each with its own type: e.g. a clean
      sans with a soft shadow, a karaoke style that fills each word as it is sung, a glow in the
      cover's colour, an outlined style readable over any wallpaper, a serif for slow songs, and
      the theme's own (Neo-Tokyo, Skin '99…). Size, colour (the cover's, the accent, white) and
      Devanagari or Latin script per style.

## 4. Make synced lyrics
- [ ] For a song with plain lyrics (or lyrics you paste), play it and tap a key at each line;
      nudge any line by ear afterwards (play from it, ±0.1 s); save.
- [ ] Saved as a timed .lrc beside the song (and in the song's tags), as a change you can undo
      in Activity.
- [ ] Health lists the songs that have plain lyrics but no timing, as the ones to make.

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
