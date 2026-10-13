# Next, round 2: faster, finished, and a few new things

Asked 2026-10-13: "tests later; implement other things first; think of new ideas, creative and
useful; make a plan." Built one phase after another; each item is ticked when built and checked in
the running app. Same rules as always: no AI at runtime, no paid services, nothing downloaded
without a reason you can see, and nothing changed in your files without a preview and undo.

## 1. Speed first
- [ ] **The library stays in memory.** Every playlist action (open, save, move, add, refind;
      16 commands) reads every file's state and rebuilds the whole library from the index.
      Keep it loaded and refresh it only when the folder changes (the scanner already knows).
- [ ] **Apple Music's library is read once**, not on every visit to Playlists (it is decrypted for
      the completion banner); read again only when its file changes.
- [ ] **Opening Playlists does work only when something changed**: moved songs and arrived songs
      are checked when the library changes, not on every visit.
- [ ] **Starts faster**: the interface loads as one 750 KB file; the rarely used places
      (Settings, Organize, Import, the album editor) load when first opened.
- [ ] **Measured on your real library**, in the installed app: start, Music, a 822-song
      playlist, search, theme switch, before and after. Numbers in the plan, not guesses.

## 2. Finish what is half done
- [ ] **Covers for songs you don't have**: each missing album's cover looked up once (iTunes
      Search, free, about 20 a minute, so in the background) and kept; the empty sleeve until then.
- [ ] **Playlist changes in Activity**, each with Undo (updates from a source, merges, deletes),
      like builds and edits are.
- [ ] **Music's search finds playlists** (Ctrl+K does already) and songs waiting in them.
- [ ] **Lumen uses the cover's second colour**: Now Playing's background and the album header
      blend the two, so a two-colour cover looks like itself.

## 3. New: your music, understood (all on this computer)
- [ ] **Your year in music**: a page you open any time (and a summary each December). Hours
      listened, top songs, albums, composers and singers, the film year you played most, streaks,
      the album you came back to. From your own listening history; it never leaves the PC.
- [ ] **Keep playing from your library**: when an album or playlist ends, Venuta carries on with
      songs that belong with it. Ones that share playlists with it, then the same composer, film,
      singer or era, then the ones you rate highly. No AI: it shows why ("Because it shares 3
      playlists with Kesariya"). Off, or on, in the queue.
- [ ] **Credits**: browse by composer, lyricist and singer, not only album artist (A. R. Rahman
      the composer, Gulzar the lyricist, Arijit Singh the singer), with their years and films.
      The lookups already filled these tags in.
- [ ] **Lyrics in Latin script**: Hindi, Marathi and other Devanagari lyrics shown in Roman
      letters on request, by rule (no AI, no internet), so a song can be sung along by anyone.
- [ ] **Make synced lyrics**: for songs that have only plain lyrics, tap along once and Venuta
      writes the timed .lrc (kept as a change you can undo).
- [ ] **Bring over Apple Music's history**: play counts, loved songs and dates added from the
      library file (format research first), so Most played and your year in music are right from
      day one instead of starting from zero.

## 4. New: your listening setup
- [ ] **Headphone correction**: pick your headphones (AutoEq's free, MIT-licensed database of
      thousands of corrections) or import a correction file, for example Reference Audio
      Analyzer's for the Mega5EST. Applied as parametric EQ in the sound chain, with a preamp so
      it never clips. Each output device can have its own: the IEMs on the DAC, the Arya on the
      amp. Off is still bit-perfect.
- [ ] **Windows, natively**: play, previous and next on the taskbar preview; recent playlists and
      albums on the taskbar's right-click menu; optionally "Listening to" in Discord (off unless
      you turn it on).
- [ ] **Your phone as a remote**: a small page on your home network (scan a QR code, nothing
      leaves the house) with play, skip, volume, the queue and search; the PC keeps playing.

## 5. Accounts (when you want them)
- [ ] TIDAL and YouTube Music sign-in for playlists, the Spotify way.
- [ ] ListenBrainz (open and free): send what you play, if you want a record online.
- [ ] Your own server (Navidrome / OpenSubsonic).

## Waiting on you
- Complete your Apple playlists (Music, Playlists, Review and complete).
- Spotify Client ID for the real test; your SD cards for the player test.
- Making the repository public, then release 0.2.0 (updates only work once it is public).
- The empty index folder D:\Music\.venuta\libraries\b88a2c7d96e7f892: delete it, or tell me to.
