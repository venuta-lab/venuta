# Plan: whole playlists, played from Music

Asked 2026-10-13: "I see only few songs in playlists. I imported my Apple playlists; it should
show all songs from that playlist whether it is available locally or not, show visually what is
available, so I can easily get the missing ones." Then: "this is not just for Apple Music, for all
playlists. Implement it properly."

## Status (2026-10-13)

Built for every playlist, whatever its source (record: docs/decisions/0006-whole-playlists.md):

- [x] §1 One importer; Apple song lengths read from the library file (all 3,459 entries).
- [x] §2 Complete from Apple Music: the banner and the reviewed update, with Undo. **His real
      playlists are untouched until he presses Review and complete.**
- [x] §3 Update from source (Spotify, Deezer, Apple, files), three-way, for any imported playlist.
- [x] §4 The playlist page: every song in place, readable missing rows, coverage, All / In your
      library / To get (by album), Maybe Yes/No, Search the web, Copy; drawn a screen at a time.
- [x] §5 Cards show "268 of 398 here"; To get by playlist; Ctrl+K lists playlists.
- [x] §6 Playlists in Music (D5: one place), standard Play/Shuffle, Playing from, following
      edits while playing, shuffle and repeat kept across restarts, Jump back in.
- Decisions taken as recommended: D1 in place, D3 in their places, D4 the browser's web search
  (Google; Windows gives an app no way to ask for the default engine), D5 one place.
- [ ] D2 covers for songs you don't have: not built. It needs a lookup per album (iTunes
      Search allows about 20 a minute), a cache, and the images allowed in the window; missing
      songs show an empty sleeve meanwhile.
- Undo is on the import's done screen and the copy in Set aside, not in Activity.

## What is actually wrong

The app can already keep a song you don't have: an imported playlist writes it as a
`#VENUTA-WANT:` line, and the playlist page shows it as "Not in your library" with **Find**.
His playlists simply never got those lines.

They were made by the **first Apple importer** (`playlists::import_apple`, still behind the CLI's
`apple-import` and the app command `apple_import`, which no screen calls any more). It writes only
the songs it matched and drops the rest, so the gaps, and their places in the order, are gone
from the files. The newer import (Playlists, Import, the review step) keeps every song.

Measured on D:\Music today (Apple's library read-only, nothing written):

| Playlist | In Apple Music | In Venuta's file |
|---|---|---|
| Favourite Songs | 822 | 258 |
| Liked Playlists | 699 | 168 |
| SangeetArtist | 555 | 162 |
| Hindi | 398 | 268 |
| still loves... | 162 | 0 |
| English | 209 | 4 |
| Marathi | 153 | 30 |
| Escape_point | 83 | 39 |
| Shazams | 44 | 5 |
| Meaning of life | 36 | 3 |
| Best of TVF | 31 | 9 |
| Scores | 26 | 1 |
| Old, Bhakti, Anime, Ballads, Always❤️, Dhurandhar, Dastaan | 20, 13, 12, 11, 11, 9, 1 | 0, 0, 0, 3, 5, 6, 0 |
| Lord of the Rings Sleep, Games | 150, 14 | not brought over |

1,713 different songs in all. (`still loves...` became `still loves` when the name was made safe
for a file name, so name matching must allow for that.)

## The plan

### 1. One way in: retire the first Apple importer
- The CLI's `apple-import` and the app's `apple_import` go through the same `imports::preview`
  and `apply` as everything else, so no path can drop a song again. The old function and its
  test go; the CLI keeps a dry run that prints the same three counts as the review step.
- `imports::apple_library` skips playlist entries Apple's file doesn't describe (`filter_map`):
  count them and say so in the review ("3 songs Apple's file has no details for") instead of
  losing them silently.
- Read more from Apple's file where it is there: **duration** (matching and the time column) and
  the playlist's **persistent id** (so an update finds the same playlist even after a rename).
  Format research first on a copy; tests on a small fixture cut from it with the strings
  replaced.

### 2. Bring back what the old import dropped (a repair you review)
A new **"Complete from Apple Music"** step, offered once on the Playlists screen when Venuta
finds playlists that came from Apple and are short of it ("19 playlists are missing 2,496
songs Apple Music still has"):
- For each, rebuild the full list **in Apple's order**: songs already in the playlist keep their
  exact file; the rest are matched again; what's still not here becomes a wanted line in its
  place.
- Songs **you added in Venuta** after the import stay, next to the song they followed. (The old
  files carry no record of songs you removed by hand; the review shows every change, so he can
  untick any he doesn't want back.)
- Shown like the import review: per playlist "+564 songs, 0 here, 18 close matches to check";
  close matches confirmed the usual way.
- The old file is **set aside** first and the change journaled, so Activity can undo it. Nothing
  is written until he presses Complete.
- The two playlists never brought over (Lord of the Rings Sleep, Games) are offered in the same
  step.
- The repair records each playlist's source and Apple id, so it isn't offered again.

### 3. Keeping in step later: update from the source
So a playlist that changes in Apple Music (or Spotify) can be brought up to date without losing
edits made in Venuta, each import keeps what it brought in (`.venuta/imports/<playlist>.json`:
source, source id, the song list as imported). Updating is then a three-way merge: what came in
last time, what the source has now, what the playlist has now. Added at the source: added. Removed
at the source: removed, unless you added it yourself. Your additions and removals are kept. The same
review screen shows it. ("Update from Apple Music" in the playlist's menu.)

### 4. The playlist page: every song, and what's here at a glance
- **All songs, in order, numbered as in the source.** Missing songs stay in their place, not at the
  bottom.
- **Missing rows are readable, not ghosted.** Full-strength title and artist (today they are 60%
  opacity, hard to read on glass); the difference is carried by a small "not here" mark on the
  cover and a quiet hollow cover style, plus the words "Not in your library", never colour alone.
  The album column still shows the album, since that's what you need to go and get it.
- **Header shows coverage:** "398 songs, 268 in your library, 130 to get", with a thin bar of how
  complete it is. The play time counts only songs you can play.
- **Filter:** All, In your library, To get (a segmented control above the list; remembered per
  playlist). "To get" can be **grouped by album**: "130 songs from 74 albums", biggest first, each
  album with its songs and "Add to To get" / "Find in your library".
- **Playing:** Play and Shuffle play what's here and skip the rest (already so); a missing row
  isn't clickable to play, but its menu offers: Find it in your library…, Copy "Artist – Title",
  Search the web (opens his browser on a search; Venuta never downloads), Show its album in To get,
  Remove from playlist.
- **Close matches** found later (a song came in with a slightly different title) show on the row:
  "Maybe *Kesariya (From "Brahmastra")*?" with Yes / Not this one, instead of being missed because
  only exact matches are filled in automatically.
- **Covers for missing songs:** see decision D2.

### 5. Around the app
- **Playlists home:** each card says "268 of 398" with the same thin bar; sort by "Most to get".
- **To get** (the want-list) fills from these lines: with every gap back, his list grows to about
  1,180 albums. Its tiers already put the albums with the most saved songs first; it gains a
  filter by playlist ("what Hindi needs").
- **When new music arrives**, missing songs are matched again (already so, exact only); the
  notification says which playlists gained songs.
- **Cards and exports** keep only playable songs (already so: wanted lines are Venuta's own and are
  left out).
- **Search** (Ctrl+K) finds a missing song by name and opens its playlist at that row.

### 6. Playlists in Music, played the standard way
Asked the same day: "show playlists in the Music tab, so we can play music from playlists
directly: sequence, loop, shuffle, all the standard stuff."

What's there today: Music's home has a "Your playlists" shelf (20 cards, a play button on
hover), but there is no Playlists tab beside Albums, Artists and Songs, and opening a card jumps
to the Playlists place, out of Music. The player already has shuffle and repeat (off, all, one) in
the bar. What's off:
- **Shuffle buttons shuffle the list themselves** (the playlist page, the card's menu) instead of
  turning on the player's shuffle, so the bar's shuffle button shows off while the order is
  random, turning shuffle off can't bring back the playlist's order, and if shuffle was already on
  the list is shuffled twice.
- **Nothing says what you're playing from.** The player knows ("Hindi"), but the bar and the queue
  don't show it or lead back to it.
- **Jump back in** has albums only, never the playlist you were listening to.
- Repeat and shuffle should survive a restart (to be checked; the engine keeps them, the app may
  not restore them).

The plan:
- **A Playlists tab in Music:** Home, Albums, Artists, Songs, **Playlists**. It's the same grid as the
  Playlists place, with Recent first, then Smart, then by source (Apple Music, Spotify,
  your own), and a search box. Opening one shows the playlist page **inside Music**, and Back returns
  to where you were. See D5 for what happens to the separate Playlists place.
- **Playing, the standard way** (Apple Music and Spotify behave like this):
  - **Play** starts at the top, in the playlist's order. Clicking a song plays from there, with the
    whole playlist as the queue, so repeat-all wraps to the start.
  - **Shuffle** turns the player's shuffle on and starts on a random song. The bar shows it on;
    turning it off goes back to playlist order from the song playing.
  - **Repeat:** off, the whole playlist, or this song, from the bar (already there). It's kept with
    shuffle across restarts.
  - **Play next** and **Add to queue** work for a whole playlist and for any song (already there).
  - Missing songs are skipped everywhere: play, shuffle, next, repeat.
  - The playing row and the playing card show it (the equaliser). The card's button pauses and
    resumes instead of starting over (already so).
- **"Playing from Hindi"** under the title in the bar and at the top of the queue, clickable back to
  the playlist. It shows the album or "Songs" the same way.
- **If the playlist changes while it plays:** songs you add, remove or move change what comes
  next. The song playing never stops.
- **Jump back in** includes playlists you played recently, beside the albums.
- **Shortcuts:** Enter plays the selected song; Ctrl+K finds a playlist and Enter plays it;
  Space, media keys and the tray work as now.

### 7. How it will be checked
- Tests first, for: the old importer's replacement keeping every song and its order; the repair
  merge (songs kept, matched, wanted, user additions placed, the `still loves...` name);
  the three-way update; Apple entries without details counted; Apple duration and id parsing on
  the fixture.
- Playback, in the player's own queue tests: Shuffle starts on a random song and unshuffles
  to playlist order; repeat-all wraps; missing songs never queued; a live edit changes what
  comes next only. In the app: the bar's shuffle and repeat match what plays, "Playing from"
  opens the playlist, both survive a restart.
- End to end in the debug copy on the scratch library, with a playlist made by the old importer:
  repair reviewed, applied, undone in Activity.
- Screenshots of the playlist page (all, to get, grouped by album) in both light and dark, Lumen
  and a retro theme, and at a narrow window.
- His real playlists are repaired only when he presses Complete himself.

## Decisions for him

- **D1. Repair in place or side by side?** Recommended: in place (same names, old files set aside,
  undoable). Alternative: new playlists named "Hindi (Apple Music)" next to the old ones.
- **D2. Covers for songs you don't have.** Recommended: look them up once from the iTunes Search
  API (free, already a source Venuta uses), cached, only when the playlist is opened. Alternative:
  no artwork, a plain placeholder.
- **D3. Where missing songs sit.** Recommended: in their place, in order. Alternative: collected at
  the bottom under "To get".
- **D4. "Search the web" goes to** his default search engine (recommended), or a store he names.
- **D5. One place for playlists or two?** Recommended: playlists live in Music (the new tab), and
  the tab also holds New, Import and To get, so the separate Playlists place leaves the sidebar
  and Ctrl+1 to Ctrl+6 move up by one. Alternative: keep both, Music for listening and Playlists
  for managing, both opening the same playlist page.

## Order of work
1. Retire the first Apple importer, and read duration and ids from Apple's file (§1).
2. Complete from Apple Music, reviewed and undoable (§2). His playlists are then whole.
3. The playlist page with every song (§4) and the coverage on cards (§5).
4. Playlists in Music, and standard playback: shuffle mode, playing-from, live queue, Jump back
   in (§6).
5. Updating from the source (§3), and To get by playlist (§5).
