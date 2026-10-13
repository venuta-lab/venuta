# Venuta (वेणुता)

*The essence of the flute.*

A free, open-source desktop app that builds and keeps a perfect personal lossless music library, and mirrors it to SD cards for portable players.

- **Audit** the library: tags, duplicates, art, lyrics, loudness, paths and suspicious quality.
- **Organise** it with your own rules, as a change set you review and can undo.
- **Look up** years, credits, labels, covers and synced lyrics from free public sources.
- **Intake** new albums through an `_Incoming` folder, filed the same way.
- **Browse, edit and curate:** tags, ratings, playlists and smart playlists.
- **Bring over** your Apple Music playlists, with a want-list of what you don't own yet.
- **Sync** to SD cards: a one-way mirror that is safe to interrupt and only writes what changed.

There is no AI at runtime, no account and no paid service, and the app never downloads music.

> Built for one person's library first, and shared as-is in case it helps you. No support is promised.

Status: 0.1, Windows first, built for one library and shared as-is. See [docs/PLAN.md](docs/PLAN.md) for the design and progress, [docs/decisions](docs/decisions) for what was learned while building, and [docs/research](docs/research) for the sources behind it.

## What it will never do

- **It never writes to the folder you start from.** The first build copies your library into a new folder and changes only the copy.
- **It never deletes outright.** Removed files and playlists go to `_Quarantine` inside the library. Every run writes a journal, and every run can be undone.
- **Audio is never re-encoded.** Each tag write is verified: the audio bytes before and after must match, or the file is left as it was.
- **On a card, it never touches files it did not put there.**

## Using it

The first time, **Set up your library** walks you through it: where your music is, where the clean library goes, reading it, and looking up details online. After that the app has seven places, in the rail on the left.

### Music

Every album as a cover wall, with search and your own top-level folders as filters. Open an album to edit its details and song titles, rate songs (saved as you click) and add it to a playlist. Editing writes to your music files, so the first time the app asks you to confirm that you have a backup.

### Health

How complete and how sound your library is, as a dashboard:

- a ring with how complete the details are today, and after you build;
- a tile per detail (composer, synced lyrics, genre, album artist, covers, volume levelling), showing the albums still missing it;
- a tile per sound problem (damaged, lossy, fake hi-res, padded 24-bit, cut-off highs, missing songs), opening the albums with their spectrograms, and **Add to To get** for a better copy;
- the tools: **Look up details online** (MusicBrainz, Apple's catalogue, Deezer, LRCLIB, plus Discogs and AcoustID with your keys), **Check quality** (decodes every song), **Rescan**, and the music folder.

### Organize

Where your library is tidied and new music is filed.

- **Questions for you:** only what can't be proven, one card at a time, with the evidence side by side, clips from your own files (Space), a recommendation and its reason. Number keys answer, Enter accepts the recommendation, Ctrl+Enter accepts them all.
- **Handled for you:** what was proven automatically (identical songs set aside, spellings unified, years settled by the recording codes), each with its exact proof, to approve or change.
- **What the build changes:** a few stories (sharper covers, synced lyrics, credits, years…) that open to the albums and their exact before and after; any album can be left out.
- **Build library** copies, retags, adds covers and lyrics, verifies, and writes a journal. Albums waiting for an answer are held back and built later. Then **Switch to the new library**.
- **New music:** drop albums on the window, or into `_Incoming`, and they are filed the same way.

Rules are plain TOML files in your rules folder (Settings, Music folders): artist names, categories, folder layout, genres, title clean-up and tags to strip. Artist names can also be edited in Settings.

### Playlists

Playlists are ordinary `.m3u8` files in `Playlists/` inside your library, so any player can use them. Create, rename, reorder by dragging, remove songs, delete (to quarantine); a moved song is found again by its audio checksum. Smart playlists are rules ("rating at least 4", "unrated songs on albums you rated highly") that rewrite themselves. Your Apple Music playlists can be brought over; what you don't own goes on **To get**.

### Devices

Each SD card is written for the player that reads it: **Poweramp**, **Snowsky Disc**, **Echo Mini**, **Astell&Kern**, or an exact copy. The sound is never re-encoded; only the tags and covers are adjusted (several singers joined with ";", the original year in DATE, covers at the player's size). Only changes are written, a pulled card is safe, and music already on a card is recognised instead of copied again. **Test this player** writes a short test album to check what your player reads.

### Activity and Settings

**Activity** lists every build, every edit to your files and every answer, each with undo. **Settings** holds the look (light or dark, Glass, Frosted, Solid or AMOLED, the accent colour), music folders, artist names, online sources and keys, players, safety and about.

Shortcuts: **Ctrl+K** opens the command palette, **Ctrl+1 to Ctrl+5** switch between Music, Health, Organize, Playlists and Devices, **Ctrl+,** opens Settings, **/** searches your music, **?** lists every shortcut.

### Where things live

| What | Where |
|---|---|
| Settings, the index for each library, caches, thumbnails, journals | `%APPDATA%\Venuta\` |
| Rules and smart-playlist rules | the rules folder (Settings, Music folders) |
| Playlists | `<library>\Playlists\` |
| Card identity and manifest | `<card>\.venuta\` |

## Install

Download the Windows installer (`Venuta_x.y.z_x64-setup.exe`) from the releases page and run it. It installs for your user only, with no admin rights needed. The app is not code-signed yet, so Windows SmartScreen may warn you the first time: choose **More info**, then **Run anyway**.

## Development

Requirements: Rust (stable), Node 24 + pnpm 10, and on Windows the MSVC C++ build tools and WebView2.

```bash
cargo test                                  # engine + cli tests
cargo run -p cli -- scan <library-folder>   # headless scan (read-only)
pnpm --dir app install
pnpm --dir app tauri dev                    # desktop app with hot reload
pnpm --dir app tauri build                  # installer in target/release/bundle
```

The CLI (`venuta`) can do everything headless: `scan`, `health`, `deepcheck`, `plan`, `apply`, `undo`, `incoming-plan`, `incoming-import`, `card-setup`, `card-sync`, `apple-import` and `lookup`. Run `venuta --help` for details.

Layout:

| Folder | Contents |
|---|---|
| `crates/engine` | all the logic, no UI |
| `crates/cli` | the headless binary `venuta` |
| `app/` | the Tauri shell and React UI |
| `docs/` | plan, research and decision records |

## Licence

MIT, see [LICENSE](LICENSE). Metadata comes from [MusicBrainz](https://musicbrainz.org) (CC0), the iTunes Search API, the Deezer API and [LRCLIB](https://lrclib.net). Each is used within its published limits.
