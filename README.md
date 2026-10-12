# Music Librarian (working title)

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

Status: 0.1, Windows first. See [docs/PLAN.md](docs/PLAN.md) for the design and progress, [docs/decisions](docs/decisions) for what was learned while building, and [docs/research](docs/research) for the sources behind it.

## What it will never do

- **It never writes to the folder you start from.** The first build copies your library into a new folder and changes only the copy.
- **It never deletes outright.** Removed files and playlists go to `_Quarantine` inside the library. Every run writes a journal, and every run can be undone.
- **Audio is never re-encoded.** Each tag write is verified: the audio bytes before and after must match, or the file is left as it was.
- **On a card, it never touches files it did not put there.**

## Using it

### 1. Library

Choose your music folder. The first scan reads every file once; after that, a rescan only looks at what changed.

- **Health** lists problems by kind: tags, duplicates, art, lyrics, paths and loudness.
- **Deep check** decodes every file. It verifies the checksum and finds lossy sources, upsampling, padded 24-bit and real loudness. It takes a few minutes for a large library and can be stopped and resumed.
- **Look up online** asks MusicBrainz, Apple's public catalogue, Deezer and LRCLIB for original years, composers, lyricists, labels, large covers and synced lyrics. Answers are cached for 90 days, and requests are paced to respect each service's limits.

### 2. Review and build

Review shows, album by album, exactly what would change and why, with a confidence level for each change.

- Approve albums one at a time or all at once.
- Answer the questions it asks, for example whether two artist spellings are the same person. Answers are saved to your rules.
- Choose where the new library goes, then **Build**. It copies, retags, adds covers and lyrics, verifies, and writes a journal. **Undo** removes everything that run created.

Rules are plain TOML files that you can edit in any text editor. Their folder is set in settings (default `%APPDATA%\music-librarian\rules`). The rules files are:

| File | Holds |
|---|---|
| `aliases.toml` | artist spellings |
| `categories.toml` | which folders are film, classical, and so on |
| `genres.toml` | genre mapping |
| `layout.toml` | the folder and file naming pattern |
| `strip.toml` | junk tags to remove |
| `titles.toml` | title clean-up |

### 3. Browse

Your albums as a wall of covers. Open one to edit album fields and track titles, and to rate songs; ratings save as soon as you click. **Add to playlist** puts the whole album on a playlist. Editing writes to your music files, so the first time, the app asks you to confirm that you have a backup.

### 4. Incoming

Drop new albums onto the screen, or into `_Incoming` inside your library. The app looks them up, shows the plan, and files them like everything else. It also flags songs you already have. Originals are kept in `_Incoming/.imported` until you clear them.

### 5. Playlists

Playlists are ordinary `.m3u8` files in `Playlists/` inside your library, so any player can use them.

- **Your playlists:** create, rename, reorder by dragging, remove songs, delete (to quarantine). If a song moves, the playlist finds it again by its audio checksum.
- **Smart playlists** are rules such as "rating at least 4" or "unrated songs on albums you rated highly". They rewrite themselves every time you open Playlists. *Top rated*, *Explore* and *Recently added* are made for you.
- **Apple Music (Windows):** the app reads your own `Library.musicdb` and rebuilds your playlists from the songs you have. Everything else goes on a want-list, sorted into albums to get whole, to explore, and to probably skip. The library file is encrypted. You enter its key once, and it is stored only on your computer; it is published in several open-source Apple Music tools and is not included here.
- **Rate unrated ★★★★** gives every unrated song in a playlist four stars. It is handy for Apple's *Favourite Songs*.

### 6. Cards

Insert an SD card and choose **Set up** to name it and pick which parts of the library it carries.

- **Sync** copies new music, moves renamed files, removes music you removed, and patches tag changes in place without rewriting the audio.
- If the card already has your music from another tool, the app adopts those files instead of copying them again.
- A pulled card is safe: the next sync finishes the job.
- With **auto-sync** on, a card is synced and ejected as soon as it is inserted while the app is open.

Shortcuts: **Ctrl+1 to Ctrl+6** switch sections, a two-finger swipe moves between them, and **Esc** closes an album.

### Where things live

| What | Where |
|---|---|
| Settings, the index for each library, caches, thumbnails, journals | `%APPDATA%\music-librarian\` |
| Rules and smart-playlist rules | the rules folder (see above) |
| Playlists | `<library>\Playlists\` |
| Card identity and manifest | `<card>\.music-librarian\` |

## Install

Download the Windows installer (`Music Librarian_x.y.z_x64-setup.exe`) from the releases page and run it. It installs for your user only, with no admin rights needed. The app is not code-signed yet, so Windows SmartScreen may warn you the first time: choose **More info**, then **Run anyway**.

## Development

Requirements: Rust (stable), Node 24 + pnpm 10, and on Windows the MSVC C++ build tools and WebView2.

```bash
cargo test                                  # engine + cli tests
cargo run -p cli -- scan <library-folder>   # headless scan (read-only)
pnpm --dir app install
pnpm --dir app tauri dev                    # desktop app with hot reload
pnpm --dir app tauri build                  # installer in target/release/bundle
```

The CLI (`librarian`) can do everything headless: `scan`, `health`, `deepcheck`, `plan`, `apply`, `undo`, `incoming-plan`, `incoming-import`, `card-setup`, `card-sync`, `apple-import` and `lookup`. Run `librarian --help` for details.

Layout:

| Folder | Contents |
|---|---|
| `crates/engine` | all the logic, no UI |
| `crates/cli` | the headless binary `librarian` |
| `app/` | the Tauri shell and React UI |
| `docs/` | plan, research and decision records |

## Licence

MIT, see [LICENSE](LICENSE). Metadata comes from [MusicBrainz](https://musicbrainz.org) (CC0), the iTunes Search API, the Deezer API and [LRCLIB](https://lrclib.net). Each is used within its published limits.
