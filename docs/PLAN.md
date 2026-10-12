# Music Librarian: design and plan

> **Status:** v0.3, 2026-10-12. v0.3 added Browse, tag editor, playlist editor, smart playlist builder and preview player (D18–D20). The decisions are made (§12). The working title is a placeholder until the project is named.
> **Basis:** five research reports in [`research/`](research/), with every claim sourced and marked VERIFIED or UNVERIFIED. Also an audit of the owner's library (`D:\Collection`, 2,688 FLAC files) and their Apple Music library.
> **v0.2 changes:**
> - Player-agnostic: MusicBee is now just one optional player.
> - SD sync is now a mirror of a device-safe library. Per-device profiles are deferred.
> - All open decisions are taken.

## Progress (2026-10-12)

| Milestone | State |
|---|---|
| M-1 Safety | ✅ Apple export saved in `D:\Music-meta\apple-export\2026-10-12`. 🔲 Capture `9XM` / `Romance` (owner) |
| M0 Foundations | ✅ Repo, workspace, Tauri shell, CI workflow. ✅ Spike S1 ([0001](decisions/0001-s1-tag-write-safety.md)). 🔲 S2 player read-back (owner, about 5 min). 🔲 S3 card write speed (needs a card) |
| M1 Scan + health | ✅ Index, 23 checks, deep check (integrity, cutoff, upsampling, padded 24-bit, loudness), spectrograms ([0002](decisions/0002-deep-check-and-migration.md)) |
| M2 Rules + plan + apply | ✅ Rules files, planner, Review screen with questions, build into a new library with undo, album editor with ratings and in-place edits. 🔲 Owner runs the full migration into `D:\Music` |
| M3 Providers + intake | ✅ Polite cached HTTP, MusicBrainz / Apple / Deezer / LRCLIB, lookup job (barcode, then a strict title search), planner uses years, credits, labels, covers and lyrics, category inference, Incoming screen with drag-and-drop and import ([0003](decisions/0003-online-lookups-and-intake.md)). 🔲 Folder watcher in the tray (Incoming refreshes while open) |
| M4 SD sync | ✅ Card identity and manifest on the card, content diff, in-place tag patches, adoption of old-layout copies, foreign files untouched, space check, eject, auto-sync while the app is open ([0004](decisions/0004-card-sync-and-playlists.md)). 🔲 Sync the three real cards (owner). 🔲 S3 on real cards |
| M5 Playlists + Apple + want-list | ✅ `.m3u8` playlists that repair themselves when tracks move, editor (create, rename, reorder by drag, remove, delete to quarantine, add an album from Browse), smart playlist builder, Apple Music import, tiered want-list with CSV, rate a playlist 4★ ([0004](decisions/0004-card-sync-and-playlists.md)). 🔲 Import after the migration (owner). Later: folders, merge, editor undo, live preview |
| M6 Release | ✅ MIT licence, README user guide, Windows installer. 🔲 Updater, code signing, public GitHub release |

---

## 1. What this is

This is a free, open-source desktop app that builds and keeps a **perfect personal lossless library** and copies it to SD cards. It has a minimal UI, and the engine can also run headless.

**It takes over every non-playback job that MusicBee does today:** scanning, tag editing, organising and device sync. Any player then just *plays* the library: MusicBee or foobar2000 on the PC, the DAP's own player, Poweramp on Android. No player is required, and no player's database matters.

It has five jobs:

1. **Audit.** It finds everything wrong with the library: tags, duplicates, art, lyrics, loudness, paths and suspicious quality.
2. **Organise.** It applies the owner's rules as a reviewable, undoable change set.
3. **Intake.** Anything dropped into `_Incoming` becomes a finished, correctly filed album, with metadata, art, lyrics and loudness.
4. **Browse and curate.** Browse and search the whole library and edit tags by hand (one track or many). Rate tracks, replace art, preview tracks, and **create and edit playlists**: static, smart and folders, with imports from Apple Music and others, a want-list, and smart playlists that stay up to date.
5. **Sync.** It mirrors the library one way to SD cards, safely, quickly and resumably.

It is **not** a hi-fi player (it only has a preview player for curating), a downloader or ripper, a streaming client, or an AI tool. Nothing at runtime needs an account, a paid service or AI.

**It is built for one owner, then shared as-is.**

## 2. Principles

| # | Principle | In practice |
|---|---|---|
| P1 | **It never damages a file** | Changes go to a temp copy, the audio MD5 is verified, then the copy is renamed into place. Everything is journalled and undoable. Nothing is hard-deleted; removed files go to `_Quarantine`. |
| P2 | **It never guesses** | Uncertain data goes to Review with the reason shown. |
| P3 | **Rules come first** | The owner's rules files are the authority. Online sources only propose values, and every value records where it came from. |
| P4 | **Running it twice changes nothing** | Re-applying rules changes nothing. Re-syncing an up-to-date card writes 0 bytes. |
| P5 | **Open standards; works in any player** | Standard Vorbis comments, `cover.jpg`, `.lrc` and `.m3u8`. The library depends on no app, including this one. |
| P6 | **The library is device-safe by construction** | Files are made compatible with every reasonable player, so the master works anywhere and a card is a plain copy. |
| P7 | **Work is incremental** | Only what changed is re-scanned, re-fetched or re-copied. |
| P8 | **Respects the services it uses** | Per-host rate limits, caching, honest User-Agent. Lyrics are never bundled. |
| P9 | **Works offline** | Everything except metadata and lyrics lookups works without internet. |

## 3. The evidence that shaped this design

| Finding (source) | Design consequence |
|---|---|
| Owner's library:<br>• COMPOSER empty on 95% of files, GENRE empty on 91%<br>• 6+ spellings per composer, lyricists in ALBUMARTIST<br>• re-release dates in DATE<br>• 5 duplicate albums, paths up to 332 characters<br>• progressive JPEG on 316 of 328 albums<br>• 9 fake hi-res albums, 0 lossy transcodes<br>• lyrics: 899 in Devanagari, 623 in Latin script, 50 in Tamil/Malayalam (audit 2026-10-12) | Rules engine and alias table, separate credit fields, ORIGINALDATE, art re-encoding. The audit is milestone 1. |
| beets/Picard pain:<br>• CLI interrogation<br>• batch limits<br>• Windows path bugs<br>• non-Latin names depending on MusicBrainz aliases (research 05) | Review queue as the main interface. Our own overrides survive re-imports. |
| No GUI does library-wide normalisation as a reviewable, undoable change set (05) | Core feature: plan → review → apply → undo. |
| OneTagger (Rust + webview) and dapctl (Rust DAP sync) exist (05) | The stack is proven. Borrow dapctl's diff-before-copy approach. |
| lofty 0.25.4 is active but had FLAC corruption bugs (#549, #640, #731 open). Its generic `Tag` drops unmapped keys (01) | Use the low-level `VorbisComments`/`FlacFile` APIs. Write to a temp copy, verify, rename (P1). |
| `notify` can drop events, and Windows buffer overflows discard events (01) | Watcher events are hints only. Reconcile scans are the truth. |
| The volume serial changes on reformat. MusicBee identifies devices by drive letter (01, 04) | A marker file on each card is its identity. |
| FAT stores local time at 2-second resolution, with DST shifts. exFAT's UTC offset can be invalid (04) | Sync compares hashes from a manifest stored on the card. File times are only hints. |
| Syncthing/rsync write a temp file then rename. beets-alternatives patches tags in place. EchoList keeps a journal on the target (04) | `.part` + rename, tag-only patching, journal on the card. |
| Rockbox and many cheap firmwares can't show progressive JPEG. Small players prefer modest art. Echo Mini and Rockbox read `.lrc` but not embedded FLAC lyrics (03, 04) | **The master library embeds baseline JPEG ≤1000px and keeps both a `LYRICS` tag and a `.lrc` file**, so any player is satisfied without per-device conversion. |
| Ratings conventions differ: Picard 0–1, MusicBee 0–100 (03) | One convention: `RATING` 0–100 in the file, so ratings travel with the music to any player. |
| iTunes: no key, UPC lookup, art up to 3000px, no credits. Deezer: no key, UPC/ISRC lookup, art up to 1400px, ISRC can hit compilations. MusicBrainz: CC0, composer/lyricist credits, patchy coverage. Discogs: credits, art ≤600px. JioSaavn: best Indian credits but its terms forbid automation. Spotify, Qobuz and the Apple Music API are closed (02) | Look up by UPC first, then ISRC within the album. Per-field source order (§7). JioSaavn and TIDAL are opt-in and off by default. |
| LRCLIB: no key, ±2s duration match, both scripts, variable quality (02) | Validate lyrics before accepting them. |
| Apple Music for Windows has no export. Decoders leave the key out for legal reasons. Catalog playlists vanish after cancelling (05) | The Windows reader is opt-in and the user supplies the key. macOS uses the official XML. Generic CSV/M3U as a fallback. Export before cancelling. |
| Azure signing is US/Canada-only for individuals. SignPath is free for OSS after a first release (01) | Ship unsigned at first, apply to SignPath after v1. |

## 4. Architecture

### 4.1 Shape

```
┌──────────────────────── Tauri 2 app (window + tray) ────────────────────────┐
│  React + TypeScript UI  ◄── typed commands / event stream ──►  app crate     │
└───────────────────────────────────────────────┬──────────────────────────────┘
                                                │ links
┌─────────────────────────── engine (Rust library crate) ──────────────────────┐
│ index   audit   rules   plan/apply/undo   intake   providers   lyrics   art   │
│ loudness   playlists   importers   sync   jobs (queue, progress, cancel)     │
└───────────────┬──────────────────────────────┬───────────────────────────────┘
        ┌───────┴───────┐               ┌──────┴───────┐
        │  cli crate    │               │ ffmpeg LGPL  │  (only for non-FLAC
        │  (headless)   │               │  sidecar     │   intake / spectrograms)
        └───────────────┘               └──────────────┘
```

- **Cargo workspace:**
  - `crates/engine`: all logic, no UI.
  - `crates/cli`: a thin binary.
  - `app/`: the Tauri shell plus `ui/`.

  The UI only calls engine commands. A second launch is forwarded to the running app by the single-instance plugin.
- **Jobs:** scan, apply, fetch and sync are jobs with progress, cancel and a final report. Only one writing job per volume runs at a time.
- **Concurrency:** tokio for I/O and orchestration; rayon for CPU work (decode, FFT, hashing, image resizing, loudness). One sequential writer per SD card. Different cards can run in parallel.
- **HTTP:** one rate-limited queue per host, with backoff that honours `Retry-After`, and a SQLite response cache.

### 4.2 Where things live

| Location | Contents |
|---|---|
| **`D:\Music`** (library root) | Music only: audio, `cover.jpg`, `.lrc`, and `Playlists/*.m3u8`. Plus `_Incoming/`, `_Review/` and `_Quarantine/`. The `_` folders are never synced. |
| **`D:\Music-meta`** | `rules/` (TOML, git-friendly), `apple-export/`, `reports/`. Not synced to cards. |
| App data (`%APPDATA%\<app>\`) | `index.sqlite`, `cache.sqlite`, `journal/` (undo), logs. |
| Each SD card | `/.<app>/card.toml` (identity), `/.<app>/manifest.sqlite`, `/.<app>/journal`, then `Music/…` mirrored from the library, and `Music/Playlists/`. |

### 4.3 The index (SQLite, WAL mode, rusqlite)

- **`files`:** path, size, mtime, quick hash (xxh3), content hash (blake3, computed lazily), audio MD5 (from STREAMINFO), audio properties, tag hash.
- **`tags`:** every key is stored, including unknown ones, and multi-value order is kept.
- **`releases` / `tracks`:** the logical model.
- **`issues`**, **`provenance`** (which source set each value), **`plans`/`changes`**, and **`stats`** (ratings mirrored from files).

**Identity.** A track is identified by its audio MD5 (or a decoded-audio blake3 hash when the MD5 is 0), so it survives moves and retags. A release is identified by UPC, then MBID, then a hash of (album artist, album, original year).

**Incremental scans.** If size and mtime are unchanged, the file is skipped. The target is under 1 second for an unchanged library of 3,000 files.

**Player writes.** A player such as MusicBee may write `RATING` or play counts into files. The scan detects these as tag-only changes and adopts them; they are never treated as conflicts.

## 5. The library model

### 5.1 Canonical tag schema (Vorbis comments, following the Picard mapping)

| Field | Key(s) | Film soundtrack | Other | Notes |
|---|---|---|---|---|
| Title | `TITLE` | Song title. Drop `(From "X")` when X is the album | Same | Keep version qualifiers (Reprise, Unplugged, Female, Remix) |
| Track artists | `ARTIST` (one key per singer) | Singers | Performers | Repeated keys are the Vorbis standard |
| Album artist | `ALBUMARTIST` (single value) | Composer(s), canonical spelling. 3+ distinct composers become `Various Artists` | Primary artist | One value only, always |
| Composer | `COMPOSER` ×N | Music director(s) | Songwriter(s) | |
| Lyricist | `LYRICIST` ×N | Lyricist(s) | Lyricist(s) | |
| Album | `ALBUM` | Film title, no `(Original Motion Picture Soundtrack)` | Album title | Keep edition qualifiers |
| Original date | `ORIGINALDATE` + `ORIGINALYEAR` | Film release year | First release | Folder names use this |
| Release date | `DATE` | This edition | Same | |
| Release type | `RELEASETYPE` ×N | `album`, `soundtrack` | `album`/`single`/`ep`/`live`/`compilation` | |
| Genre | `GENRE` ×N | From `genres.toml` | Same | Small controlled vocabulary (§12 D9) |
| Language | `LANGUAGE` | ISO 639-3 (`hin`, `mar`, `tam`, `mal`, `tel`, `urd`, `pan`, `eng`, `jpn`, `kor`, `zho`, `fil`) | Same | |
| Category | `CATEGORY` | `Film & Series` | `Independent` / `Scores` / `International` / `Collections` | Drives the layout |
| Label | `LABEL`, `CATALOGNUMBER` | | | |
| Identifiers | `ISRC`, `BARCODE` (migrated from `UPC`), `MUSICBRAINZ_*` | | | Keep all existing |
| Numbering | `TRACKNUMBER`, `TRACKTOTAL`+`TOTALTRACKS`, `DISCNUMBER`, `DISCTOTAL`+`TOTALDISCS`, `DISCSUBTITLE` | | | Both total spellings, for maximum player compatibility |
| Compilation | `COMPILATION=1` only when album artist is Various Artists | | | |
| Rating | `RATING` 0–100 | | | Stays in the file, so every player and device sees it |
| Loudness | `REPLAYGAIN_{TRACK,ALBUM}_{GAIN,PEAK}` (RG 2.0, −18 LUFS) | | | |
| Lyrics | `LYRICS` (LRC text) + sidecar `<track>.lrc` (UTF-8) | | | Both are kept identical. `UNSYNCEDLYRICS` is removed once `LYRICS` is valid |
| Cover | One PICTURE block, type 3, **baseline JPEG ≤1000px**, written right after the Vorbis comment | | | The full-resolution image lives in `cover.jpg` |
| **Stripped** | `major_brand`, `minor_version`, `compatible_brands`, ffmpeg `encoder`, `length`, `tidal_*`, watermarks (Telegram, Samperals…), junk `comment`s | | | Through `strip.toml` |

### 5.2 Layout (`layout.toml`)

```
{category}/{language_name}/{albumartist}/{originalyear} - {album}{edition}/{disc_prefix}{track:02} - {title}.flac
```

```
Film & Series/Hindi/A.R. Rahman/2004 - Swades/01 - Yeh Jo Des Hai Tera.flac
Film & Series/Marathi/Ajay-Atul/2009 - Jogwa/03 - Lallati Bhandar.flac
Film & Series/Hindi/Various Artists/2023 - Animal/01 - Arjan Vailly.flac
Scores/Games/Borislav Slavov/2023 - Baldur's Gate 3/1-01 - ….flac
International/English/Pink Floyd/1975 - Wish You Were Here/01 - Shine On You Crazy Diamond (Parts I-V).flac
Independent/Hindi/Anuv Jain/2018 - Baarishein/01 - Baarishein.flac
Collections/Coke Studio Pakistan/2017 - Season 10/…
Playlists/Hindi.m3u8
```

- **One folder = one release, always.** That keeps album gain, `cover.jpg` and player grouping correct.
- **Sanitisation:**
  - Normalise to NFC.
  - Replace Windows/exFAT/Android-illegal characters and avoid reserved names.
  - No trailing dot or space.
  - Each name component ≤ 120 characters, relative path ≤ 180 characters. Titles are shortened deterministically; the full title stays in the tag.
- **Sidecars:** `cover.jpg` (full resolution, ≥1400px target) and `<file>.lrc`.

### 5.3 Rules files (`D:\Music-meta\rules\`)

| File | Purpose |
|---|---|
| `aliases.toml` | Canonical names and their variants, e.g. `"A.R. Rahman" = ["A. R. Rahman", "A.  R.  Rahman", "AR Rahman"]` |
| `credits/<release-key>.toml` | Manual credits per release or track, keyed by UPC/ISRC so they survive re-imports |
| `genres.toml` | The vocabulary plus mappings from provider genres, e.g. `"Bollywood" -> "Filmi"`, `"Films/Games" -> ignore` |
| `categories.toml` | Rules that classify a release into a category and language |
| `layout.toml` | Path template, limits, sidecar policy |
| `strip.toml` | Tags and watermark patterns to remove |
| `playlists/*.toml` | Smart playlist rules |
| `cards/*.toml` | Selection rules for each SD card (§9) |

**New name variants** go to Review: "Is `A R Rahman` the same as `A.R. Rahman`?" One click adds the variant to `aliases.toml`. This is the only routine manual step, and it fades as the alias table grows.

## 6. Core pipelines

### 6.1 Audit (read-only)

- **Tags:** missing fields, alias violations, multi-value ALBUMARTIST, lyricists found in ALBUMARTIST, junk/watermarks, inconsistency within an album, track gaps.
- **Dates:** re-release dates where DATE is much later than ORIGINALDATE or the folder year.
- **Duplicates:** identical audio MD5, then same ISRC, then (optionally) a Chromaprint match with duration within ±2s.
- **Art:** missing, under 1000px, progressive, multiple pictures, embedded image differs from `cover.jpg`.
- **Lyrics:** missing, unsynced only, `.lrc` and tag disagree.
- **Loudness:** ReplayGain tags missing or partial.
- **Paths:** too long, illegal characters, not matching the layout.
- **Quality**, reported as a suspicion with a spectrogram, never as a verdict:
  - lossy-source cutoff below 19.5 kHz;
  - fake hi-res (nothing above the lower rate's Nyquist frequency);
  - fake 24-bit (wasted bits);
  - integrity (MD5 = 0, or an optional full decode against the MD5).

### 6.2 Plan → Review → Apply → Undo

1. **Plan.** Rules, provider proposals and audit results become a **change set** made of:
   - `SetTag`, `RemoveTag`, `Move`, `WriteArt`, `WriteLyrics`, `WriteLoudness`, `Quarantine`.

   Each change records its before and after values, the rule or provider responsible, and a confidence score.
2. **Review.** Changes are grouped by release as a side-by-side diff. Approve all, per release, edit (which can update the rules), or reject (which is remembered). Rules can auto-approve high-confidence changes that have no conflicts.
3. **Apply.** For each file:
   - copy to a temp file in the same folder;
   - write with lofty's FLAC API;
   - re-read and verify the audio MD5 and decoded audio;
   - `sync_all`, then atomic rename;
   - journal the before-state.

   Moves happen after retags.
4. **Undo.** Any applied change set can be reverted from its journal.
5. **Backup gate.** The first apply on a library requires confirming a backup exists.

### 6.3 Intake (`_Incoming`)

1. **Detect.** The watcher gives hints and a reconcile scan confirms. A folder counts as settled after 30 seconds without changes.
2. **Group** the files into releases.
3. **Identify**, in this order:
   - UPC → Deezer + iTunes (`in`, then `us`) + MusicBrainz;
   - the ISRC set;
   - a text search;
   - AcoustID (opt-in).
4. **Merge** per field (§7), recording provenance and confidence.
5. **Apply rules.**
6. **Enrich:** art, lyrics, loudness.
7. **Decide:** confident releases are filed into the library; everything else goes to `_Review/` with the questions it needs answered.
8. **Formats.** Non-FLAC lossless input (ALAC, WAV, AIFF) is converted to FLAC losslessly. Lossy input (MP3, AAC) is accepted but marked `lossy` and listed on the upgrade list. It is never "converted to FLAC".

### 6.4 Loudness

- Track and album gain are computed with `ebur128` (ReplayGain 2.0, −18 LUFS, sample peak). Album gain uses the whole folder.
- Results are validated against `rsgain easy` on a reference set.
- Any player's own volume analysis should stay off, so the two never mix.

## 7. Metadata sources (which field comes from where; research 02)

| Field | 1st | 2nd | 3rd | Fallback |
|---|---|---|---|---|
| Release identity | UPC → Deezer + iTunes (in/us) | MusicBrainz barcode → MBID | ISRC set | Text search / AcoustID |
| Album title, tracklist | iTunes (cleaned by rules) | Deezer | MusicBrainz | File tags |
| Original year | MusicBrainz first-release / release-group | Wikidata film P577 | Deezer/iTunes, flagged if later | File tags |
| Album artist (composer) | MusicBrainz release artist-credit | Discogs "Music By" | Wikidata P86 | iTunes (with review) |
| Singers | iTunes | Deezer | MusicBrainz | File tags |
| Composer / lyricist per track | `credits/*.toml` | MusicBrainz work relations | Discogs extraartists | Opt-in plugins: TIDAL (user's credentials), JioSaavn (off, with a terms warning) |
| Label | Deezer | iTunes ℗ line | MusicBrainz | File tags |
| Genre | Deezer + MusicBrainz + Discogs → `genres.toml` mapping | | | |
| Cover art | iTunes `3000x3000bb` | Deezer 1400 | Cover Art Archive | Existing art. Under 1400px goes to review |
| Synced lyrics | LRCLIB `/api/get` (exact duration) | LRCLIB `/api/search` (±2s, synced, script check) | TIDAL (plugin) | None |

**Lyrics are accepted only if:**
- they are synced;
- the timestamps are monotonic and the last one falls within the track's duration;
- the script matches the policy (§12 D5);
- there is no translation marker;
- the line count is plausible.

## 8. Playlists, Apple import, want-list

- **Model.** A playlist is an ordered list of track identities (audio MD5 + ISRC), so it is path-independent.
- **Editing (full CRUD):**
  - Create, rename, duplicate and delete playlists (deleted ones go to `_Quarantine/playlists`).
  - Organise playlists into folders.
  - Add tracks from anywhere: drag from Browse, multi-select, "add album" or "add all results of this search".
  - Reorder by drag. Sort by any field.
  - Remove tracks, flag duplicates, shuffle-and-save.
  - Merge playlists, or split one by album, language and so on.
  - Convert a smart playlist into a static snapshot.
  - Undo and redo work inside the editor. Every saved version is kept in the journal.
- **Smart playlist builder.** A visual rule editor (field, operator, value; all/any; limit; sort) that writes `playlists/*.toml`. A live preview shows the matching tracks and their count while you build it.
- **Source of truth (D18).**
  - Static playlists are the `.m3u8` files in `D:\Music\Playlists\`, an open standard.
  - The app keeps them correct automatically: when a track is moved or renamed, every playlist that references it is rewritten.
  - If another player edits a `.m3u8` (e.g. MusicBee), the change is detected by hash and imported. Paths are resolved back to track identities, and anything unresolvable goes to Review.
  - Smart playlists live in TOML. They are written out as `.m3u8` with a `#SMART` header comment, and edits to them from outside are ignored with a warning.
- **Smart playlists** (`playlists/*.toml`) use a small rule language modelled on Navidrome's `.nsp`:
  - fields: rating, playlist membership, composer, lyricist, genre, language, year, added date, unrated, album has a highly rated track;
  - example, *Explore*: tracks that are unrated and on an album containing a track rated ≥80.
- **Output.** `D:\Music\Playlists\*.m3u8` (UTF-8, `#EXTM3U`, relative paths with forward slashes). These play directly in MusicBee, foobar2000 and Android players, and they sync to cards unchanged.
- **Importers:**
  - Apple Music on Windows: musicdb reader, opt-in, the user supplies the key, read-only, works on a temp copy.
  - Apple Music on macOS: the official Library XML.
  - Generic CSV, TXT or M3U.
- **Matching** has four stages:
  1. ISRC.
  2. Normalised artist + title exact.
  3. Fuzzy match with duration within ±3s, using album as a tiebreaker.
  4. Review.

  Version substitutions are flagged.
- **Want-list** (CSV export, never any acquisition):
  - **Tier 1:** 3+ saved tracks, a core composer, or a score.
  - **Tier 2:** explore.
  - **Tier 3:** compilations, covers, karaoke and sleep filler. Look for the original release instead.
- **Upgrade list:** art under 1400px, lossy or suspicious sources, incomplete albums.
- **Seeding ratings.** Apple "Favourite Songs" become `RATING=80` on matched tracks, after review.

## 9. SD card sync (mirror of a device-safe library)

The cards are 128 GB, 256 GB and 1.5 TB. The library is 88 GB today and might reach 300–400 GB. Because the master library is already device-safe (P6), **a card is a plain mirror**: no conversion and no per-device profiles in v1. Per-device profiles are designed but deferred (§11, *Later*).

**Selection per card** (`cards/*.toml`):
- Default: everything.
- Optional: include or exclude by category, language, playlist or rating.

This only matters once the library outgrows the 128/256 GB cards; the 1.5 TB card holds everything.

**Protocol** (research 04 §5):

1. **Identify the card.** Detect insertion (`WM_DEVICECHANGE`), then read `/.<app>/card.toml`, which holds a UUID and the card name. The volume serial is a cross-check. A new card asks "Set up this card?" once.
2. **Pre-flight.** Check filesystem type, free space against the plan, the volume-dirty bit (chkdsk first if set), and any leftover journal (resume). Offer a capacity check on first use.
3. **Plan.** Compare the card's manifest with the selected library state **by hash**. File times are only hints, with a 2-second window and a 1-hour DST tolerance. Each difference becomes one of: add, remove, retag, move, or playlist.
4. **Journal** the plan to the card.
5. **Execute, in this order:**
   1. Deletes (only files listed in the manifest; nothing else on the card is touched).
   2. Moves, as renames on the card.
   3. New audio, written as `.part`, flushed with `sync_all`, then renamed.
   4. Tag-only changes, patched in place, with temp + rename if the new tags don't fit.
   5. Sidecars.
   6. Playlists, last.
   7. The manifest, swapped atomically.
6. **Verify.** Read back and blake3-hash every written file, sweep for stray `.part` files, clear the journal.
7. **Eject.** Flush, then `CM_Request_Device_EjectW`. "Safe to remove" is shown only after the eject succeeds.

**Speed.** Retags patch the tags in place instead of re-copying whole files, which matters most for the first sync after migration. Writes are one sequential stream with 1–4 MiB buffers, tuned in spike S3. Nothing is converted, so the SD card's write speed is the only limit.

**Migrating the existing cards.** They hold old-layout copies made by MusicBee. On first setup, matching audio MD5s let the tool *move and retag* the files already on the card instead of copying everything again.

## 10. User interface

There are seven screens, plus a mini preview player and a tray icon.

1. **Browse:**
   - Switch views between albums (cover grid), artists, composers, lyricists, genres, languages, years and folders.
   - Instant search across every field. It is backed by the index, so results appear while typing.
   - Filters such as "rated ≥ 80", "unrated", "lossy", "has issues".
   - A detail panel shows an album's art, credits, tracks, lyrics and quality info.
2. **Tag editor** (inside Browse):
   - Edit one track, or many at once with mixed values shown as `<keep>`.
   - Edit album-level fields once for the whole album.
   - Replace art by dropping an image.
   - Edit or re-fetch lyrics.
   - Set ratings with keys 1–5.

   Manual edits go through the same plan → apply → journal pipeline (P1). They are applied immediately, with no review step, but can still be undone. Renames and moves follow automatically, as do playlist rewrites. An edit can also be saved as a rule, e.g. "always spell it this way".
3. **Playlists:**
   - A tree of folders and playlists.
   - The editor (§8): drag tracks in from Browse, drag to reorder, multi-select, undo and redo.
   - The smart playlist builder with live preview.
   - The Apple import with its match status.
   - The want-list tiers and the upgrade list.
4. **Library health:** a health score and issues by type with counts. Click through to the evidence: a spectrogram, or a tag diff. Actions: *Scan*, *Plan fixes*.
5. **Review:** change sets grouped by release, showing a before/after diff, provenance chips, and approve/edit/reject. One-click answers for questions such as "Same artist?".
6. **Incoming:** a drop zone and live status for each release: identifying, enriching, needs review, filed.
7. **Cards:** one tile per card showing last sync, free space, pending changes, *Sync now*, auto-sync on insert, and selection rules.

**Preview player (D19).** It is enough to curate with: play, pause, seek, next and previous within the current list, and keyboard shortcuts. It is how you work through the *Explore* playlist and Tier 2 albums: listen, rate, keep or drop. It is deliberately **not** a hi-fi player. It has no bit-perfect output, exclusive mode, gapless playback or DSP. Serious listening stays in your player of choice and on the DAPs.

**Rules** are edited inside Review (one click adds an alias or credit) and in a simple Rules panel with a live path preview. They are always saved to TOML.

**Tray:** watches `_Incoming` and card insertion, and sends notifications such as "3 albums filed", "1.5 TB card synced: safe to remove", "2 items need review".

## 11. Build order and milestones

The engine is built once. The existing library is its first and hardest test, and sync comes after migration so cards aren't rewritten twice. Apple data is perishable, so it's captured first.

| Milestone | Delivers | Exit criteria |
|---|---|---|
| **M-1 Safety (now)** | Save the decoded Apple export to `D:\Music-meta\apple-export\`. Capture `9XM`/`Romance`. Confirm SD copies are full FLAC. | Files exist; backup confirmed. |
| **M0 Foundations + spikes** | Repo, workspace, Tauri shell, CI (Win/macOS/Linux), synthetic FLAC fixture corpus, spikes S1–S3. | Spike results recorded in `docs/decisions/`. |
| **M1 Scan + Library Health** | Index, every audit check, Library screen, HTML/CSV report. **Read-only.** | Reproduces the 2026-10-12 audit. Rescan of an unchanged library under 1s. |
| **M2 Rules + Plan/Review/Apply/Undo + Browse** | Rules files, alias detection, change sets, Review screen, journal/undo, quarantine, backup gate, ReplayGain, art re-encoding, lyrics normalisation, **Browse screen, tag editor (single and bulk), ratings, preview player**. | `D:\Collection` rebuilt as `D:\Music` (copy, then transform), with **zero audio-MD5 changes**. Undo tested. `D:\Collection` untouched until the owner signs off. |
| **M3 Providers + Intake** | iTunes/Deezer/MusicBrainz/Discogs/Wikidata/LRCLIB clients, cache, rate limits, merge and provenance, Incoming screen, watcher. | 20 varied sample releases are filed with ≥90% needing no review. |
| **M4 SD card sync** | Card identity, manifest, journal, ordering, in-place retag, verify, eject, tray auto-sync, selection rules, migration of existing cards. | All three cards synced. A pulled-card test resumes cleanly. A no-change re-sync writes 0 bytes. |
| **M5 Playlists + Apple + want-list** | Playlist model, **playlist editor (create/edit/reorder/folders/merge/undo)**, **smart playlist builder with live preview**, two-way `.m3u8` sync, playlist rewrites on move, importers, matching, review, want-list, upgrade list, rating seeding. | 21 Apple playlists rebuilt locally. A new playlist can be created and edited in the app. An edit made in MusicBee is picked up. Moving a track keeps every playlist valid. Want-list tiered and exported. |
| **M6 Polish + release** | README, user guide, rules reference, installers, updater, LGPL ffmpeg build plus its source, MIT licence, policy pages, then a SignPath application. | Public v1.0 on GitHub. |
| *Later* | Per-device profiles: conversion (e.g. 24/96 → 16/44), smaller art for tiny players, playlist workarounds such as Echo Mini has no M3U, Android over adb. Spectrogram viewer polish. | Only when a device needs it. |

**Spikes in M0:**

- **S1 Tag-write safety.** Run lofty 0.25.x on 200 copies of real files: write, re-read, check the audio MD5, full decode compare, measure padding. Also compare `ebur128` album gain with rsgain.
- **S2 Player read-back.** Write one test album in the canonical schema and check it in MusicBee, foobar2000 and one Android player:
  - repeated `ARTIST` keys;
  - `ORIGINALDATE`;
  - `RATING`;
  - `LYRICS` + `.lrc`;
  - `LANGUAGE`/`CATEGORY`;
  - baseline art.

  The schema only changes if a mainstream player breaks.
- **S3 Card write strategy.** On the owner's cards, compare one writer against several, buffer sizes of 1, 4 and 8 MiB, and the cost of `sync_all`.

### Tech stack (versions as of 2026-10-12, research 01)

| Component | Choice |
|---|---|
| Shell | Tauri 2.12 (tray, single-instance, autostart, updater, shell) + React 19 + TypeScript + Tailwind |
| Tags | `lofty` 0.25, low-level FLAC/Vorbis APIs |
| Decode / FFT | `symphonia` 0.6 + `realfft`; ffmpeg sidecar for non-FLAC intake |
| Loudness | `ebur128` 0.1, validated against `rsgain` 3.8 |
| Hashing | `blake3` + `xxhash-rust` (xxh3) |
| Images | `zune-jpeg`/`image` → `fast_image_resize` → `jpeg-encoder` (baseline, 4:2:0) |
| Database | `rusqlite` 0.40 (bundled SQLite, WAL) |
| Watch | `notify` 8.2 + reconcile scans |
| Volumes | `windows` 0.62 (WM_DEVICECHANGE, GetVolumeInformationW, CM_Request_Device_EjectW); udisks2 and DiskArbitration later |
| Fingerprints (optional) | `fpcalc` (Chromaprint 1.6) sidecar |
| Preview playback | HTML `<audio>` in the webview, which decodes FLAC, MP3, AAC, Opus and WAV, streamed through Tauri's asset protocol with range requests. Rare formats fall back to an ffmpeg-decoded stream |
| Browse UI | Virtualised lists and grids (TanStack Virtual) to keep 30k+ tracks smooth. Album-art thumbnails are cached in the app data folder |
| Licence | MIT. The musicdb module carries its own disclaimer |

### Testing

- **Synthetic fixtures:**
  - progressive art;
  - MD5 = 0;
  - padded 24-bit;
  - multi-value and unknown keys;
  - long and illegal names;
  - Devanagari, Urdu and Tamil text;
  - multi-disc releases.
- **Golden tests** for path rendering, aliases, merge and the sync planner.
- **Property tests** for sanitisation.
- **Write-safety tests:** audio MD5 and samples unchanged after any write, plus fuzzing on copies of real files.
- **Sync fault tests:** a simulated pull at random points, then resume, verify, and a re-sync must write 0 bytes.
- **Owner's library:** plan-only dry runs before every apply.

## 12. Decisions (made 2026-10-12)

| # | Decision | Why |
|---|---|---|
| D1 | **Player-agnostic.** This app owns all library and sync work. MusicBee stays as a PC player only (or is replaced by any player). MusicBee device sync is retired after M4. | MusicBee is "just a player". Depending on it would lock the library to one Windows app. |
| D2 | **No MusicBee history migration.** Play counts in MusicBee's database are not carried over. | It's a player-specific database. There are no `RATING` tags in the files today, so nothing in the files is lost. Ratings start from Apple Favourites, and from then on they live in the files. |
| D3 | **Categories:** `Film & Series / Independent / Scores / International / Collections`, then language. Web series (Kota Factory, Panchayat) go in Film & Series. Game, film and TV underscores go in Scores. | Matches how the collection already thinks (film music by language and composer), and gives scores and games their own home. |
| D4 | **ALBUMARTIST = composer** for film releases. Duos count as one (Vishal-Shekhar, Ajay-Atul). 3+ distinct composers become `Various Artists`. | The composer is the author of a film album; singers change on every track. |
| D5 | **Lyrics script:** keep existing lyrics as they are. For new fetches, prefer the native script (Devanagari for Hindi/Marathi, Tamil, Malayalam; Urdu in Nastaliq where available), fall back to romanized. | Native script is already the majority (899 vs 623) and is unambiguous; romanization is inconsistent. Device font support is a "Later" concern. |
| D6 | **Album titles** drop `(Original Motion Picture Soundtrack)` and similar suffixes. Edition qualifiers stay. | GENRE/RELEASETYPE already say it, and the suffix wastes screen width on every device. |
| D7 | **Art:** full resolution in `cover.jpg` (≥1400px target). Embedded: one baseline JPEG ≤1000px, about q90. | Baseline displays everywhere, including Rockbox and cheap firmwares. 1000px is sharp on DAP screens and cheap in size. |
| D8 | **Library root:** build a new `D:\Music` by copy-then-transform. `D:\Collection` stays read-only until sign-off, then the owner deletes it. | Zero-risk migration. D: has 242 GB free, enough for the 88 GB copy. |
| D9 | **Genre vocabulary v1:** Filmi, Film Score, TV Soundtrack, Game Score, Indie Pop, Indie Rock, Ghazal, Sufi, Qawwali, Devotional, Hindustani Classical, Carnatic Classical, Folk, Fusion, Rock, Pop, Electronic, Ambient, Anime, K-Pop, OPM. Editable. | Small enough to stay consistent, specific enough for smart playlists. |
| D10 | **Ratings:** `RATING` 0–100 in the file. | It travels with the music to any player or card. |
| D11 | **Lossy files:** accepted at intake, tagged as lossy, put on the upgrade list, never converted to FLAC. | Honest library; no fake lossless. |
| D12 | **Fake hi-res** (9 albums): kept as-is and labelled in the audit. | The label shipped them that way; downsampling gains nothing on the master, and the cards have space. |
| D13 | **Duplicates:** keep the copy with the best (art, completeness, tags) score; the others go to `_Quarantine`. `Arijit's Aura` is kept as a compilation album (Various Artists, Collections) since it's a real release. | Never lose a file; compilations are legitimate albums. |
| D14 | **Rules and exports** in `D:\Music-meta`. | Not synced, git-friendly, survives reinstalls. |
| D15 | **SD cards = mirror** with optional selection. Per-device profiles are deferred. | The cards are large and the library is device-safe, so it's simplest and fastest. |
| D16 | **Stack:** Tauri 2 + Rust engine + React/TS. **Licence:** MIT. **Name:** later. | Research 01; easiest licence for a share-as-is project. |
| D17 | **Apple export:** the Windows musicdb reader with a user-supplied key (opt-in module). The owner's export is captured now in M-1. | Legal caution (research 05); the data is perishable. |
| D18 | **Playlists:** static playlists are `.m3u8` files in `D:\Music\Playlists\`, synced both ways: the app rewrites them when tracks move, and imports external edits. Smart playlists are defined in TOML and written out as `.m3u8`. | An open format that any player and card can use, and edits made in another player are never lost. |
| D19 | **A preview player built in, not a hi-fi player.** | Curating (Explore, Tier 2 albums, building playlists) needs listening. Bit-perfect listening through the Protocol Max stays in a real player. |
| D20 | **Manual edits skip review but are journalled.** Tag edits, ratings, art and playlist edits apply immediately and can be undone. They can be saved as a rule. | You already decided when you made the edit, so a review step would only slow you down, while undo keeps P1. |

## 13. Risks

| Risk | Mitigation |
|---|---|
| lofty write bugs | Temp copy + MD5/decode verify + rename; spike S1; pinned versions |
| Free APIs change or go down | Per-field fallbacks, cache, degrade to file tags + Review |
| Thin Indian credit data | `credits/*.toml` as the authority; optional plugins |
| exFAT has no journal | `.part` + rename, journal on the card, dirty-bit pre-flight, verify |
| Some device can't handle something in the master library | Master is built device-safe (P6); per-device profiles are designed and ready in *Later* |
| Legal (musicdb, lyrics, ffmpeg) | Key never shipped; lyrics only fetched on the user's machine; LGPL ffmpeg with its source published |
| Scope creep from publishing | "Built for me, shared as-is" |
| SmartScreen warning on an unsigned build | Documented; SignPath after v1 |
