# Lyrics for as many songs as possible

Asked 2026-10-14: many songs still have no lyrics. Why, what else exists, and what Venuta should
show for songs that will never have words. Method: the real library's index (read-only copy),
120 of the songs without lyrics asked of LRCLIB several ways, and a sourced survey of lyrics
services and of how other players behave.

## Where the library stands (D:\Music, 2,626 songs)

| | Songs |
|---|---|
| Synced lyrics (`.lrc` beside the song) | 1,680 (64%) |
| Plain lyrics only (`.lrc`/`.txt` or the LYRICS tag) | 230 |
| None | 716 |
| of which in **Scores** | 47 |
| of which titled as instrumental (Instrumental, Theme, Flute, BGM, Karaoke, Dialogue…) | ~93 |
| of which sung songs | ~580, almost all Filmi |

Health counts all 716 as missing, instrumentals and scores included, so the score understates
the library.

## Why LRCLIB found nothing: 120 sung songs asked again

| What LRCLIB has for the song | Songs | Reached by |
|---|---|---|
| Synced lyrics | 12 | 4 as Venuta asks today (added since; Venuta never asks twice), 8 only with a cleaned title |
| Plain lyrics only | 19 | Venuta throws plain lyrics away |
| Marked instrumental | 7 | Venuta ignores the mark |
| Lyrics, for a version 3 to 15 s longer or shorter | 8 | a looser duration match |
| Nothing | 74 | |

Of the 74, about a quarter are score cues with English names inside film soundtracks ("Hope",
"Feeding the Witch", "Triumph", "Jitu Destroys", "Echoes of the Journey"): instrumentals no title
rule can spot. The rest are remixes, deep cuts and series songs nobody has uploaded.

How Venuta asks today (crates/engine/src/providers.rs `lrclib_find`), and what loses songs:

1. **Synced only.** Plain lyrics are dropped, so 230-plus songs show nothing though words exist.
2. **The instrumental flag is ignored**, and a miss is remembered forever: nothing is retried.
3. **Exact names.** LRCLIB's `/api/get` matches title and artist exactly after folding case and
   punctuation (verified in its source). `Kesariya (From "Brahmastra")`, `Kesariyaa`,
   `Pritam, Arijit Singh` and `Arun Date` (stored as `Arun Date feat. Anuradha Paudwal`) all miss
   the same song. Venuta's search pass uses the same raw title.
4. **±2 s.** Film, single, reprise and remastered versions differ by more.

## What else exists

| Source | Lyrics | Fit for this library | Can Venuta use it |
|---|---|---|---|
| **LRCLIB** | synced, plain, instrumental flag; publish back (proof of work, no account); full database dump (SQLite, ~44 GB) | good: Kesariya ~14 versions, Zingaat 20/20 in Devanagari, Vaseegara in Tamil, Marathi bhavgeet present | **yes**: open, the creator invites any use of the dumps |
| Your files | `.lrc`/`.txt` beside songs, LYRICS / UNSYNCEDLYRICS tags | already read | yes |
| Musixmatch | synced (supplies Spotify and Gaana in India) | probably the best Bollywood coverage | **no**: free API returns ~30% of each song, non-commercial; full lyrics need a licence; apps that get more pose as Musixmatch's own desktop app |
| JioSaavn | plain, many Hindi songs | excellent | **no**: its API policy calls any access without its credentials unauthorised and says it will be pursued |
| Genius | plain | fair | **no**: the API gives no lyrics; scraping breaks its terms |
| NetEase, QQ Music, Kugou | synced, word by word | thin for Indian music | no: undocumented, terms risk |
| Deezer, Gaana, Spotify, Apple Music, YouTube Music, LyricFind | various | | no: private, paid or behind sign-in |
| Lyricsify, Megalobiz, Syair, lyrics.ovh | LRC or plain, scraped | patchy, often broken | no |

Other players (foobar2000 OpenLyrics, Strawberry, beets, Feishin, LyricsX) mostly ship scrapers and
undocumented APIs behind a disclaimer. Venuta's line (no account, no paid service, nothing against
a site's terms) leaves **LRCLIB, your own files, and you**; that is enough if Venuta asks well and
gives back.

## What players show (and what Venuta should)

- **Apple Music** greys the Lyrics button when a song has none (Apple support 105076); plain lyrics
  show as a static sheet. No documented instrumental screen.
- **Spotify** says so in words ("Hmm, we don't know the lyrics for this one"), says something else
  when it couldn't load them, and marks plain lyrics "not time-synced yet".
- YouTube Music, Amazon, Tidal hide the button.
- No tag standard marks an instrumental. MusicBrainz does: an *instrumental* attribute on a
  recording's work, and the lyrics language `zxx` ("No lyrics"). LRCLIB has `instrumental`.

## Recommendation

**1. Ask LRCLIB well** (no new source, no risk). Several passes: as now; the title without
`(From …)`, `[…]`, `feat.`, `Remix`/`Reprise` notes, with the first artist and no album; free-text
search with the title and the film. Score the candidates locally: title (with Indian spelling
folded: aa→a, ee→i, w→v, sh→s, doubled letters), artists in common, duration (±2 s for synced
timing, wider for plain words), the script you prefer (D5). Keep plain lyrics. Keep the
instrumental flag. Ask again for misses after 30 days, politely (User-Agent, one at a time,
retry 429/503).

**2. Know what has no words**, and stop counting it: the Scores category, LRCLIB's flag, MusicBrainz's
instrumental attribute or `zxx` (Venuta already asks MusicBrainz for works), instrumental titles, and
your own "Mark as instrumental". Kept in the library's `.venuta`, never written into the music.

**3. Show each case plainly, after Apple.** The lyrics button stays where it is:
- Synced: as today.
- Plain: the words, still and scrollable, with "Not timed" and *Time it* (the lyrics maker).
- Instrumental: the button is dimmed; opened, it says "Instrumental" with a quiet ♪. Never "missing".
- None found: the button is dimmed; opened, "No lyrics found" with *Find lyrics* (pick from the
  candidates), *Paste or open lyrics*, *Mark as instrumental*.
- Couldn't reach LRCLIB: said so, with Try again (not "no lyrics").

**4. Health counts what can have lyrics.** "Lyrics" = songs with words / songs that have words
(instrumentals left out); a second line says how many are timed.

**5. Give back** (opt-in, each one confirmed): lyrics you time or paste, and songs you mark
instrumental, published to LRCLIB, so the next listener of Indian film music finds them.

**Not now:** the 44 GB offline dump (an option later for people offline), and every source in the
"no" rows above.

Expected, from the sample (rough, ±10 points): of the ~580 sung songs without lyrics, ~10% gain
synced lyrics, ~16% plain words, ~7% words via a version match; ~6% turn out instrumental, and about
a quarter of the remaining misses are score cues. Songs with words could go from **1,910 of 2,626
(73%)** to about **2,090 of ~2,350 that have words (~89%)**, with every instrumental shown as one.
The rest (about 250 remixes and rarities) are for you to paste or time, and to share.
