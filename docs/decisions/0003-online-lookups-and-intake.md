# 0003: Online lookups and Incoming (milestone 3)

- **Date:** 2026-10-12
- **Status:** accepted

## Sources and their jobs

These were confirmed live on the owner's albums (Rockstar, UPC 8902894697408) before any code was written.

| Field | Source | Why this one |
|---|---|---|
| Original year | MusicBrainz release-group `first-release-date` | The only source that separates the first release from digital re-releases. It replaces the folder-vs-date guess, which is wrong for some folders (decision 0002) |
| Composer, lyricist per track | MusicBrainz work relations (`composer`/`writer`, `lyricist`), matched by ISRC, then disc/track position, then exact title | Structured, CC0 credits. Rockstar: A. R. Rahman / Irshad Kamil |
| Label | MusicBrainz (brand, e.g. "T-Series"), then Deezer (legal name) | Brand names are what people browse by |
| Release type | MusicBrainz primary + secondary types | |
| Cover | Apple `3000x3000bb` (serves its largest, e.g. 1400 for Rockstar), then Deezer `1400x1400` | Only fetched when the embedded art is under 1400 px |
| Synced lyrics | LRCLIB `get`, plus `search` within ±2 s | Validated: in time order, ends within the track, ≥4 lines, no translations, preferred script (native per D5, romanized fallback) |
| Category (when no rule matches) | Catalogue genres: Apple's "Bollywood"/"Tamil"/"Telugu"/…, MusicBrainz soundtrack type | Always medium confidence, with a note suggesting the owner add a rule |

**Not used:**
- JioSaavn: its terms forbid automation.
- Spotify, the Apple Music API and Qobuz: closed to free apps.
- Discogs and Wikidata: kept as possible fallbacks, not needed yet.

## Politeness and resilience

- **Per-host pacing:** MusicBrainz 1.1 s, Apple 3.1 s, Deezer 0.15 s, LRCLIB 0.25 s. `Retry-After` is honoured on 429/5xx, and network failures are retried with backoff.
- **Every answer is cached in `cache.sqlite` for 90 days,** including "not found", so a re-run asks nothing twice.
- **Each source runs on its own thread,** so Apple's slow pace never holds up MusicBrainz or lyrics.
- **Results are saved as they arrive** (index schema v3). A stopped run resumes, and failed albums are retried next time.

## Incoming

- **`<library>/_Incoming` has its own index,** so nothing mixes with the library until it is imported.
- **The folder is created only when files are added,** so just opening the screen writes nothing.
- **Tracks whose audio MD5 is already in the library are flagged.** When all of an album's tracks are already there, it is set aside.
- **Import is apply (copy, verify, rename) into the library.** The originals then move to `_Incoming/.imported/<time>/`; nothing is deleted.
- **Live test (copy of Rockstar under an unrecognised folder name):**
  - inferred Film & Series/Hindi/A.R. Rahman/2011 – Rockstar;
  - MusicBrainz credits, label T-Series;
  - LRCLIB lyrics for the track that had none;
  - cover upgraded from 1280 px to 1400 px.

  All 15 tracks were imported, and the originals moved aside.
