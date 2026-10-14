# Venuta (वेणुता)

*The essence of the flute.*

A Windows app for a lossless music library. It builds the library and keeps it tidy, plays it
bit-perfect, brings your playlists over from anywhere, and writes it to SD cards for portable
players.

![Music: the album playing, then your playlists and shelves](docs/screenshots/music-home.jpg)

No AI at runtime, no account, no paid service.

## What it does

- **Plays** your files bit-perfect (exclusive WASAPI), with synced lyrics, a waveform seek bar,
  an equaliser and volume levelling when you want them, and a mini player.
- **Lyrics everywhere**: on a whole screen for the TV, floating over the desktop, in six styles;
  time the ones that have no timing by tapping along.
- **Keeps the library tidy** with your own rules (folders, file names, artist spellings,
  categories, covers), as changes you review first and can undo.
- **Looks up** original years, composers, lyricists, sharper covers and synced lyrics from free
  public sources.
- **Checks the sound**: damaged files, lossy sources, fake hi-res, loudness.
- **Brings playlists over** from Spotify, Apple Music, YouTube Music, Deezer, TIDAL and others,
  matched to your files; songs you don't have keep their place until you do.
- **Writes SD cards**: the whole library or a selection, in your library's layout, converted
  for the player when it needs it, changing only what changed.
- **Searches across scripts**: "kesariya" finds केसरिया, in names and in lyrics.
- **Looks how you like**: eleven built-in themes, seven more as theme files, and a Theme Studio
  to make your own from a few colours or the album playing, then share it as one file. Making
  themes by hand or with an AI assistant: [docs/THEMES.md](docs/THEMES.md).

| | |
|---|---|
| ![Now Playing](docs/screenshots/now-playing.jpg) | ![An album](docs/screenshots/album.jpg) |
| ![Playlists, as files beside your music](docs/screenshots/playlists.jpg) | ![Health: how complete and how sound the library is](docs/screenshots/health.jpg) |
| ![Eleven looks](docs/screenshots/themes.jpg) | ![The lyric styles](docs/screenshots/lyric-style.jpg) |

## What it will never do

- **Change the music you bring in.** The first build copies it into your library and changes
  only the copy.
- **Delete.** Duplicates, replaced files and removed playlists go to **Set aside**. Every change
  is journaled and can be undone in **Activity**.
- **Re-encode your audio.** Every tag write is checked: the audio before and after must match,
  or the file is left as it was.
- **Touch files on a card that it didn't put there.**

## Install

Download `Venuta_x.y.z_x64-setup.exe` from [Releases](../../releases) and run it. It installs
for your user, with no admin rights. The installer isn't code-signed yet, so SmartScreen may
warn you: choose **More info**, then **Run anyway**. Windows 10 or 11.

[Using Venuta](docs/GUIDE.md) walks through every place, the lyrics, shortcuts, and where
things are kept.

## Build it

Rust (stable), the MSVC build tools, Node 24 and pnpm 10:

```bash
pnpm --dir app install
pnpm --dir app tauri dev
```

[CONTRIBUTING.md](CONTRIBUTING.md) has the checks, how to test on a copy of a library, and how
to send a change.

| | |
|---|---|
| `crates/engine` | The library: scan, index, rules, plan and apply with undo, health, playlists, cards, lookups |
| `crates/player` | Decoding, bit-perfect output, the sound tools |
| `crates/cli` | `venuta`, the engine from the command line |
| `app/` | The desktop app (Tauri) and its interface (React) |
| `docs/` | [Decisions](docs/decisions), [research](docs/research), [plans](docs/plans), including [phones and tablets](docs/plans/PLAN-MOBILE.md) |

## Licence

MIT, see [LICENSE](LICENSE). Details come from [MusicBrainz](https://musicbrainz.org) (CC0),
the iTunes Search API, the Deezer API and [LRCLIB](https://lrclib.net), within their published
limits; Discogs and AcoustID optionally, with your own keys. Album art in the screenshots belongs
to its owners.
