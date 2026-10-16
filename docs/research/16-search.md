# 16: Search, local and online, at its best

Researched 2026-10-15. The goal: every search bar gives the best results, your library first and
the online catalog below, with filters (all, songs, albums, artists, covers, others) and the
popular recording ahead of covers and mixes, as Apple Music, YouTube Music and Spotify do.
Live checks were signed out, no key; "(unconfirmed)" marks what wasn't confirmed.

## 1. Why results feel random today (the root cause)

- **The online filters are broken.** Venuta asks YouTube Music for Songs, Albums and Artists with
  the filter codes of the `innertube-rs` 0.9.0 crate. Those codes are malformed (one stray byte
  after a length-16 field), so YouTube ignores them and answers each with the **unfiltered mixed
  list**: the top card's "More from YouTube" uploads (slowed + reverb, sped up, covers), podcast
  episodes, cover videos. Verified live: the crate's Songs/Videos/Albums/Artists codes all return
  the exact unfiltered response. The codes ytmusicapi uses work (20 official songs a page, with
  play counts and continuations).
- **Albums and artists are lost.** The crate reads an item's id only from video links; albums and
  artists carry theirs in `navigationEndpoint.browseEndpoint.browseId`, so they come back without
  an id and Venuta drops them.
- **Nothing is ranked.** Online results keep YouTube's order (which, even with the right filter,
  is relevance, not popularity: "tum hi ho" songs came back 1.2B, 1.1M, 2.6B, 1.2K plays…). Local
  results are "exact, then near", each in folder order; title, artist and album count the same;
  your plays and stars aren't used.
- **What YouTube sends is thrown away**: the top result card, `musicVideoType` (official audio
  vs official video vs user upload), play/view counts, album type and year, artist audience,
  "showing results for" / "did you mean", continuations, entity suggestions.
- **Plumbing**: the three online requests run one after another (about 1.5 to 2 s); one cancel
  token is shared by every search box, so one search can blank another; no cache; the session
  asks as if in the US (`gl=US`), not India.
- **Elsewhere**: Ctrl+K finds albums and playlists but not songs or artists; "artists" means album
  artists only (a singer who's only on others' albums isn't found); there's no online artist page;
  no suggestions as you type, no recent searches, no top result.

## 2. How the big services do it

- **Before typing**: recent searches and something to browse (Spotify's categories, YouTube
  Music's Explore, Apple's scope and categories).
- **As you type**: a few query completions, then entities with art and a type label (song,
  artist, album). Spotify runs results on every keystroke; YouTube Music's suggestions endpoint
  answers in 53 to 142 ms, with up to 5 entity rows (e.g. "Tum Hi Ho · Song · Arijit Singh · 1.2B
  plays").
- **Results**: one **Top result** chosen by intent ("arijit" → the artist; "kesariya" → the song),
  then sections by type, each with **See all**, which is the same as tapping that type's **chip**
  (Spotify: Top, Artists, Songs, Playlists, Albums, Podcasts, Profiles; YouTube Music: Songs,
  Videos, Albums, Artists, Community playlists, Featured playlists, Episodes, Podcasts, Profiles).
- **Library vs catalogue**: Apple Music and YouTube Music switch between them (never mixed);
  Spotify's library search is separate. **Roon** is the one player that merges your files with
  TIDAL/Qobuz into one result set. A "your music first, online below" page is rare: Venuta's
  chance to do it well.
- **Ranking** (Spotify publishes the factors: your history, current and all-time popularity):
  text match decides the tier (exact, prefix, all words, phonetic, typo), popularity and your own
  plays order within it; one canonical copy of each recording (single, album, soundtrack and
  compilation collapsed); official above user uploads.
- **Typos**: Algolia's widely copied defaults: 1 typo from 4 letters, 2 from 8. YouTube Music
  auto-corrects romanised Hindi ("arjit sing kesarya" → "arijit singh kesariya") and says so.
- **Hindi**: metadata is romanised with no standard spelling. JioSaavn's approach: phonetic
  equivalence classes, the spelling as typed always above its variants. Devanagari queries:
  Deezer found nothing for "केसरिया" but Kesariya first for "kesariya"; YouTube Music's Songs
  filter does find "तुम ही हो" (Arijit's first). So online, send Latin letters (Venuta's search
  box already converts them for the library).

## 3. Telling the real song from covers and mixes

- **YouTube Music's own label** on every result (`musicVideoType`): **ATV** = the official audio,
  the album version (YouTube Music calls it "Song"); **OMV** = the official music video (same
  recording, with video; some cover artists' uploads are marked OMV too); **UGC** = user uploads
  (covers, lyric videos, slowed + reverb, live bootlegs); **PODCAST_EPISODE**. So: songs from the
  Songs filter (all ATV), videos only under Videos, user uploads never in Songs.
- **Words in titles, album and artist** (case-insensitive, in brackets or after "-" or "|"):
  - *Covers & mixes* (folded into one "Covers & versions" row, never hidden for good): karaoke,
    "in the style of", "originally performed by", "made famous by", tribute, cover, nightcore,
    slowed, reverb, sped up, 8D / 16D, bass boosted, lofi flip, mashup, ringtone, "whatsapp
    status", 1 hour / 10 hours / loop, lyric video / lyrics (for user uploads), piano / flute /
    guitar cover; and seen live in Hindi results: Jhankar, Super Jhankar Beats, "Sleep Lofi",
    "Bollywood Instrumentals", "Revival" by an unrelated label.
  - *Official other versions* (lower, never folded away): live, unplugged, acoustic, reprise,
    female / sad version, remix, dance / club mix, extended, edit, remaster, recreated,
    instrumental on the original album. `(From "Film")` is canonical in Bollywood: cut only to
    compare.
  - *Overrides*: an official-audio song whose artist matches is never a cover; a user upload whose
    channel isn't the credited artist leans cover; a word in the query (e.g. "lofi") lifts that
    version instead.
- **Duplicates**: the same song on its film album, a "Greatest Hits" and a single (both 1.2B
  plays): one row (normalised title + artist set + length within 3 s), the most played copy.

## 4. Popularity, free

| Source | Free, no key | Signal | Indian music |
|---|---|---|---|
| YouTube Music (already used) | yes | "1.2B plays" on songs, "707M views" on videos, artists' monthly audience | the best |
| Deezer | yes | track `rank`, artist `nb_fan` (`rank` = 100000 looks like a default) | good |
| ListenBrainz popularity | needs a token now (401) | listens per recording | thin |
| Last.fm, Spotify | need a key / no longer give popularity to new apps | | |

YouTube's own counts come with the search Venuta already makes; Deezer only as a backup.

## 5. What YouTube Music's search gives, free and signed out

- **Unfiltered search**: one top result card (`musicCardShelfRenderer`) plus one interleaved list
  of 20 to 29 items (3 to 6 songs without lengths, 1 to 3 artists, 2 to 4 albums, playlists,
  videos, episodes, profiles), and "showing results for" / "did you mean" when it corrected the
  query. No continuation. About 0.4 to 0.8 s, 237 KB.
- **The top card is unreliable for Indian queries**: "tum hi ho", "kesariya" and "तुम ही हो" gave a
  podcast **episode**; "blinding lights" gave the video, not the song. Right for artists
  ("arijit singh") and some songs. So it's a hint, never trusted alone.
- **Filtered search** (working codes from ytmusicapi): Songs (all ATV, with lengths and plays, 20 a
  page, continuations), Videos, Albums (type and year), Artists (audience), Playlists, Featured and
  Community playlists, Profiles, Podcasts, Episodes; each with an "ignore spelling" variant for
  "Search instead for".
- **Suggestions** (`music/get_search_suggestions`): about 6 completions (typed part bold) and up to
  5 entities with art; 53 to 142 ms; Devanagari prefixes give text only.
- **Locale**: `gl=IN` for Indian ranking; keep `hl=en` and classify by page/video type, never by
  label text (labels translate).
- **Limits**: none hit in about 35 requests; keep to two at a time, cache (query, filter) ten
  minutes, back off on 429/403.
- **Film metadata**: titles carry `(From "Brahmastra")`; the album is the soundtrack; artists mix
  composer and singers without roles; many regional re-voicings and devotional songs share titles:
  plays tell them apart.

## 6. Measuring it

A fixed set of about 60 searches with the expected top result: Hindi titles, variant spellings
("lag jaa gale", "aahista", "kesariyaa"), Devanagari, artists (partial and full), films/albums,
English songs, lyric lines, typos. Online answers recorded once so the test is repeatable;
re-recorded monthly. Measures: **right first** (success@1), MRR@10, **covers in the top 5**,
**duplicates in the top 5**, local keystroke time (p95 ≤ 50 ms), first online result (p95 ≤ 800
ms).

Sources: ytmusicapi (search.py, parsers, FAQ), YouTube Music help (Song/Video), Spotify for
Artists (search ranking), Spotify SIGIR 2023 and KDD 2021, Apple Support (Music search), Roon's
2021 search overhaul, JioSaavn phonetics blog, Microsoft Research (Bollywood search), MusicBrainz
style guides, Picard, Algolia (typos, ranking, debouncing), ListenBrainz popularity docs, Spotify
February 2026 migration guide; live signed-out requests to music.youtube.com, api.deezer.com and
api.listenbrainz.org on 2026-10-15; Venuta's code (paths in the plan).
