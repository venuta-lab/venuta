<div align="center">

<img src="docs/media/logo.png" width="104" alt="The Venuta mark: a V with a peacock feather and a flute's holes">

# Venuta

**Your music, lossless and looked after.**

A music library and bit-perfect player for Windows. It keeps your files tidy, shows synced
lyrics everywhere, brings playlists in from any service, plays the online catalog beside your
own music, and writes SD cards for your portable player.

[![Latest release](https://img.shields.io/github/v/release/venuta-lab/venuta?style=flat-square&color=c98a5e&label=release)](https://github.com/venuta-lab/venuta/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/venuta-lab/venuta/total?style=flat-square&color=7c6cf0)](https://github.com/venuta-lab/venuta/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/venuta-lab/venuta/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/venuta-lab/venuta/actions/workflows/ci.yml)
[![Windows 10 and 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=flat-square)](#install)
[![MIT licence](https://img.shields.io/github/license/venuta-lab/venuta?style=flat-square&color=3fa66b)](LICENSE)

[![Rust](https://img.shields.io/badge/Rust-engine-b7410e?style=flat-square&logo=rust&logoColor=white)](crates)
[![Tauri 2](https://img.shields.io/badge/Tauri-2-24c8db?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![React 19](https://img.shields.io/badge/React-19-58c4dc?style=flat-square&logo=react&logoColor=white)](app)

**[Download for Windows](https://github.com/venuta-lab/venuta/releases/latest)** &nbsp;&nbsp;|&nbsp;&nbsp;
[Using Venuta](docs/GUIDE.md) &nbsp;&nbsp;|&nbsp;&nbsp; [What's new](CHANGELOG.md) &nbsp;&nbsp;|&nbsp;&nbsp;
[Themes](docs/THEMES.md)

<br>

<img src="docs/screenshots/music-home.jpg" alt="Music: the album playing, then your playlists" width="100%">

</div>

<br>

Free and open source. No account, no ads, no tracking, and no AI at runtime.

|  |  |
|---|---|
| **Bit-perfect playback**<br>Exclusive WASAPI, gapless, hi-res as it was recorded. | **Lyrics everywhere**<br>Synced in Now Playing, on a big screen, floating over the desktop. |
| **Playlists in and out**<br>From Spotify, Apple Music, YouTube Music and more, free, no sign-in. | **Your library, then the world**<br>Search your files first, then the online catalog. |
| **A library that stays tidy**<br>Your rules, reviewed before anything changes, all undoable. | **Eighteen looks**<br>Or make your own in the Theme Studio and share it. |

## Listen

<img src="docs/media/lyrics.gif" alt="Synced lyrics following Blinding Lights in Now Playing" width="100%">

- **Bit-perfect** through exclusive WASAPI, gapless, with a waveform seek bar and a signal path
  that shows every step from the file to your speakers.
- **Synced lyrics** that follow the song; click a line to jump to it. **Big screen** (F11) puts
  them on a TV or a second monitor; **desktop lyrics** (Ctrl+Shift+D) float over every window.
  Six styles, in the cover's colour.
- **Time lyrics yourself**: for words with no timing, tap Space as each line begins.
- **When you want them**: a 10-band equaliser, tone, volume levelling by song or album,
  crossfade, a sleep timer, a mini player, the tray and media keys.

| | |
|---|---|
| ![An album, hi-res](docs/screenshots/album.jpg) | ![The sound tools, all off until you want them](docs/screenshots/sound.jpg) |
| ![The lyric styles, for the big screen and the desktop](docs/screenshots/lyric-style.jpg) | ![The mini player](docs/screenshots/mini-player.jpg) |

## Playlists, in and out

<img src="docs/media/import.gif" alt="Pasting a Spotify playlist link and seeing which songs are already in the library" width="100%">

**Bring in** from anywhere, free and without signing in:

- **A share link** from Spotify, Apple Music, YouTube Music (the whole playlist, with its name),
  TIDAL, JioSaavn, Deezer (your favourites too) or ListenBrainz.
- **Songs copied** from Spotify's app, or a plain list of songs.
- **Any playlist file**, read by what's in it: M3U, PLS, XSPF, JSPF, WPL, ASX, CSV,
  spreadsheets and zips, or a whole folder from another player or a phone.
- **A bookmark** for playlists that only show when you're signed in.

You see what matched before anything is saved. Songs you don't have keep their place, marked,
so the playlist fills in as your library grows; **Check for new songs** keeps imported
playlists current.

**Send out** in any format, with paths for where it will be played (this PC, a phone, a card),
as one backup that brings every playlist back, or to Deezer, Apple Music, Spotify and YouTube
by finding your songs there. Playlists are plain `.m3u8` files beside your music, so every
player can read them.

| | |
|---|---|
| ![Bring in: every source on one screen](docs/screenshots/import.jpg) | ![Send out: files, a backup, or a streaming service](docs/screenshots/send-out.jpg) |

## Search your library, then the online catalog

<img src="docs/screenshots/search.jpg" alt="Search: the song in your library first, then the online catalog" width="100%">

One search for everything, forgiving of spelling and across scripts ("kesariya" finds
केसरिया, in names and in lyrics). Your library always comes first; below it the online catalog
puts the real recording ahead of covers and remixes. Online songs play in the same player and
go into playlists and the queue beside your own.

## A library that stays tidy

- **Your rules** for folders, file names, artist spellings and categories. Every change is a
  plan you review first, then a build you can undo in **Activity**.
- **Looks up** original years, composers, lyricists, sharper covers and synced lyrics from
  free public sources.
- **Checks the sound**: damaged files, lossy sources passed off as lossless, fake hi-res, and
  loudness for levelling.
- **Health** shows how complete and how sound the library is, and what would fix it.

| | |
|---|---|
| ![Health: how complete and how sound the library is](docs/screenshots/health.jpg) | ![Playlists and smart playlists](docs/screenshots/playlists.jpg) |

## Cards for portable players

**Devices** writes your whole library, or a selection of playlists, albums and artists, to an
SD card in your library's own layout. It converts to 16-bit FLAC or AAC for players that need
it, writes only what changed, and never touches files it didn't put there. A gift card adds a
tracklist and a list of what's missing; **Test this player** checks what a player can read.

## Make it yours

<img src="docs/media/themes.gif" alt="The same screen in eleven of Venuta's looks" width="100%">

Eleven built-in themes after the players you know and a few retro ones, seven more as theme
files, light and dark. The **Theme Studio** makes a theme from a few colours or the album
playing, with the whole app as the preview; share it as one `.venuta-theme` file. Making
themes by hand or with an AI assistant: [docs/THEMES.md](docs/THEMES.md). Pick the app icon
too, in 33 materials.

<details>
<summary><b>Every theme, side by side</b></summary>
<br>
<img src="docs/screenshots/themes.jpg" alt="Lumen dark and light, Green Room, Neo-Tokyo, Orchard, Bounty Session, Skin '99, Thermoptic and Sync Ratio" width="100%">
</details>

## What it will never do

- **Change the music you bring in.** The first build copies it into your library and changes
  only the copy.
- **Delete.** Duplicates, replaced files and removed playlists go to **Set aside**. Every change
  is journaled and can be undone in **Activity**.
- **Re-encode your audio.** Every tag write is checked: the audio before and after must match,
  or the file is left as it was.
- **Touch files on a card that it didn't put there.**
- **Download music.** The online catalog plays; it is never saved as files.

## Install

Download `Venuta_x.y.z_x64-setup.exe` from
[the latest release](https://github.com/venuta-lab/venuta/releases/latest) and run it. It
installs for your user, with no admin rights, on Windows 10 or 11. The installer isn't
code-signed yet, so SmartScreen may warn you: choose **More info**, then **Run anyway**.

**Updates come to you.** Once a day Venuta asks GitHub whether there is a new version and says
so; **Settings › About** shows what's new and installs it with one click, then restarts.
Every update is signed, and one that doesn't match Venuta's key is refused. Switch the daily
check off in the same place.

[Using Venuta](docs/GUIDE.md) walks through every place, the lyrics, shortcuts, and where
things are kept.

## Build it

Rust (stable), the MSVC build tools, Node 24 and pnpm 10:

```bash
pnpm --dir app install
pnpm --dir app tauri dev
```

[CONTRIBUTING.md](CONTRIBUTING.md) has the checks, how to test on a copy of a library, and how
to send a change. Security problems: [SECURITY.md](SECURITY.md).

| | |
|---|---|
| [`crates/engine`](crates/engine) | The library: scan, index, rules, plan and apply with undo, health, playlists, cards, lookups |
| [`crates/player`](crates/player) | Decoding, bit-perfect output, the sound tools |
| [`crates/media-access`](crates/media-access) | The online catalog |
| [`crates/cli`](crates/cli) | `venuta`, the engine from the command line |
| [`app/`](app) | The desktop app (Tauri) and its interface (React) |
| [`docs/`](docs) | [Decisions](docs/decisions), [research](docs/research), [plans](docs/plans) |

## Licence

MIT, see [LICENSE](LICENSE). Details come from [MusicBrainz](https://musicbrainz.org) (CC0),
the iTunes Search API, the Deezer API and [LRCLIB](https://lrclib.net), within their published
limits; Discogs and AcoustID optionally, with your own keys. The screenshots show a demo
library; album art and lyrics in them belong to their owners.
