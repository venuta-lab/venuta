# Plan: YouTube Music for what you don't have, and cards that carry a selection

Asked 2026-10-13. Research: [10 cards](research/10-cards-that-carry-a-selection.md),
[11 YouTube Music](research/11-youtube-music-in-venuta.md). Nothing here is built until he picks
the decisions at the end. Same rules as always: nothing extracted or downloaded from YouTube,
nothing paid, nothing sent unless asked, nothing on a card touched that Venuta didn't write.

---

## Part A: YouTube Music, for songs you don't have and for people with no collection

**The idea.** Venuta stays your library first. Where you have no file (a playlist's waiting song,
a new album you're curious about, a whole new listener with no collection), YouTube Music fills
in, clearly marked, and "I like this" puts the song on your To get list with where to buy it.
When the file arrives, the playlist slot fills by itself (that part works today).

### A0. Spike first (an hour; nothing signed in, nothing paid)
Embeds in a Tauri window (Error 153? the app identity Referer), how many art tracks of Universal,
Sony, Warner, T-Series, Saregama and indie labels embed, the hand-over from a local FLAC to YouTube
and back (gap, loudness, exclusive output released cleanly), autoplay after a local song.
Then, with him: Google sign-in in a second window, ads with Premium. **If embeds can't be made to
play, A2 and A3 change to "open in YouTube Music"** and A1 and A4 still stand.

### A1. Listen before you get it, and where to buy (no Google at all)
- On every To get item and every waiting song: a 30-second preview (iTunes, with its store badge,
  as Apple requires; Deezer where iTunes has none) and buy links in his region: Bandcamp (FLAC),
  CD via Discogs and Amazon.in, MusicBrainz's own purchase links, iTunes Store (marked "lossy AAC").
- The region is a setting (India by default for him); no affiliate tags, ever.

### A2. Waiting songs play from YouTube Music, in your queue
- A waiting song with a YouTube link plays in its place: Venuta's queue hands it to YouTube's own
  player, and takes back the next local song when it ends.
- **The player must be visible** (YouTube's rule): a video panel in Now Playing and in the mini
  player (at least 480x270 and 200x200). With Venuta hidden in the tray, a YouTube song pauses or
  is skipped (decision Y2); it never plays hidden.
- Marked everywhere: a "YouTube" chip on the row, "YouTube · lossy" in the player, the bit-perfect
  mark grey while it plays. YouTube is credited, its player untouched, its ads untouched.
- Linking, first by hand: "Play from YouTube Music" on a waiting song opens the search there; paste
  a link, or pick from what A3 found. The link is kept in the playlist line (`yt` in
  `#VENUTA-WANT`), re-checked before it plays.
- If a label has turned embedding off: the row says so and offers "Open in YouTube Music".

### A3. Finding the match by itself
- ISRC → Deezer → song.link → the YouTube Music song; oEmbed checks it's the label's own track
  ("Artist - Topic"); the length must agree within 3 seconds. Unsure: it asks.
- Done in the background for a playlist when you ask ("Find on YouTube Music"), about 10 songs a
  minute (song.link's limit), shown as it goes.
- Optional: your own YouTube Data API key adds search by title (100 a day). Never shipped in the app.

### A4. YouTube Music itself, in a Venuta window
- For people with no collection (first run offers it: "No music yet? Listen with YouTube Music")
  and for discovering: the real music.youtube.com in its own window, signed into your own Google
  account, its own storage. Venuta changes nothing on the page.
- A slim Venuta bar outside the page, reading only what plays (from the page's media info):
  **Add to To get** (with buy links), **Find in my library** (plays your own copy if you have it),
  **Add to a playlist** (a waiting song, already linked to this YouTube song).
- If Google refuses sign-in in the window: "Open YouTube Music in your browser" instead; never a
  workaround.

### A5. Later: your YouTube playlists both ways (optional)
Import your YouTube Music playlists as Venuta playlists (songs you have play from files, the rest
wait with their YouTube links); export a Venuta playlist to YouTube. Needs your own Google OAuth
client (Google caps unverified apps at 100 users); the Takeout import that exists today stays the
no-setup way.

---

## Part B: Cards that carry a selection, laid out as a library

**The idea.** A card carries either your whole library (as now) or a selection: some playlists,
albums, artists or smart playlists. Every song goes where your library's rules put it (category,
album and year, track number), even when only some songs of an album are chosen, so the card is a
small, organised library someone can grow from, and it says what is missing.

### B1. A card is a saved selection
- When you set up or edit a card: **Everything**, or **A selection** of playlists, albums, artists
  and smart playlists (search and tick; each shows its size and how much is already on the card).
  The card carries everything any of them holds, each song once.
- **The planner**, before anything is written: total, free space, a reserve (1 GB + 5%), and per
  pick what it adds. If it doesn't fit, it says by how much and offers: smaller files for hi-res
  (B4), a lighter format (B4), or leaving out picks you choose. It never swaps your picks for
  others.
- **Playlists pull their songs onto the card** (today they don't). A song that leaves every pick
  is removed after the preview; an album folder goes only if Venuta made it and it's empty.
- Recognised on insert: "Update the card Car: 12 songs to add, 3 to remove, 2 changed, 1.4 GB more".

### B2. What's missing, for people and for Venuta
- Playlists on the card keep their missing songs as comment lines (players skip them, Venuta reads
  them), never as paths that point nowhere (Poweramp could match a different file).
- A partial album folder gets `Tracklist.txt`: every track, there or not. Folder names never change
  (no "[2 of 12]"), so adding the rest later breaks nothing; track numbers stay "3/12".
- Gift cards (B5) also get a `README` (what this is, how to play it, who made it) and `Missing.csv`
  (what to look for next).
- Opened in Venuta on someone else's computer, a gift card is a library that already knows its
  To get list.

### B3. Safe on every card
- Path and name safety: at most 255 characters for the whole path (FiiO's limit), no `\ / : * ? " < > |`,
  no trailing dots or spaces, one Unicode form; FAT32's 4 GB file limit.
- Player limits: Echo Mini only takes cards up to 256 GB (FAT32/exFAT), so the 1.5 TB card is
  refused for it with the reason; car profiles warn past 15,000 files or 8 folder levels.
- What M4 left open: the card's volume id is checked against its own record; every copied song is
  read back and compared (audio MD5); a card journal so a sync can be undone from Activity.

### B4. Smaller files when a card needs them (a choice, per card)
- **Hi-res as 16-bit/44.1 kHz FLAC**: still lossless for every player, about a third of the size.
- **AAC for phones and cars**, by Windows' own encoder (nothing to download); Opus only for players
  that read it (not the Echo Mini).
- Converted once and kept in `.venuta/converted`, so a re-sync never converts again; the same path
  with a new extension, so playlists stay right; your library's files are never changed.

### B5. Kinds of card
- **My player** (as today, per player profile).
- **A gift**: no hidden app clutter visible to a player, README and missing lists, a short neutral
  note that the music is shared for personal listening.
- **Car or USB stick**: FAT32, AAC or MP3, folders only, the car limits.

### B6. Proved on the real players
The player test kit on his cards: does the Astell&Kern read M3U8 (or need `.pls`)? The Snowsky
Disc? Results set each profile; until then those players get folders only, with playlists as an
extra.

---

## Order of work (proposed)

1. **A0 spike** (an hour): it decides how much of Part A is possible.
2. **B1, B2, B3**: cards that carry a selection, laid out as a library, saying what's missing, safe.
   Entirely offline, his own daily need.
3. **A1**: previews and where to buy.
4. **A2, A3**: waiting songs play from YouTube Music.
5. **B4, B5**: smaller files and kinds of card.
6. **A4**: the YouTube Music window.
7. **B6** with his cards; **A5** if wanted.

Every phase ends with a test on the real thing (his library copy, his cards, a real embed) before
the next starts, and a line in the docs.

## Decisions

2026-10-13: YouTube Music (Part A) waits; he will think about it (he liked Limusic's look; its
stream extraction and ad-free play are out, the rest maps to legitimate pieces: a native catalog
from Deezer, YouTube's own player where the cover sits). Part B is built now. C1 and C2 were left
open, so the defaults: 16-bit FLAC for hi-res and AAC for phones and cars (no Opus yet: the Echo
Mini can't play it); gift cards carry README, missing lists and playlists with their gaps, each
switchable per card.

### Still open
- **Y1. Where a YouTube song is shown** while it plays: a video panel in Now Playing and the mini
  player, or a small always-on-top video window of its own?
- **Y2. When Venuta is in the tray** and reaches a YouTube song: pause until you open it, or skip
  to the next song you have?
- **C1. Smaller files**: which conversions to offer (16-bit FLAC for hi-res; AAC; Opus)?
- **C2. What a gift card carries** besides the music: README and missing lists; playlists Venuta
  can read with their gaps; nothing at all beyond folders.
- **Order**: the one above, or something else first.
