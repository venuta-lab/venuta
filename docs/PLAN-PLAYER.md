# Venuta player plan

The goal is a home for listening: a bit-perfect hi-res player for your own library, with a
calm, beautiful interface. It's built in steps, and each step is usable on its own.

## Where things go

| Rail (top → bottom) | What it is |
|---|---|
| **Music** (Ctrl+1) — new home | Listening: home shelves, albums, artists, songs, genres, playlists; album and artist pages; Now Playing |
| **Metadata** (Ctrl+2) — was "Music" | The album wall for checking and editing tags and covers (what the old Music tab did) |
| Health (Ctrl+3) | Unchanged |
| Organize (Ctrl+4) | Unchanged |
| Playlists (Ctrl+5) | Unchanged for now; later merges into Music › Playlists |
| Devices (Ctrl+6) | Unchanged |

The **now-playing bar** sits at the bottom of every screen: cover, title and artist, controls,
seek bar, volume, a quality badge ("FLAC 24/96 · Bit-perfect"), output device, queue and
lyrics. Clicking the cover opens **Now Playing**.

## Principles

- **Bit-perfect by default.** In exclusive mode the samples reach the DAC unchanged, at the
  file's own rate and depth. Anything that would alter them (ReplayGain, crossfade, digital
  volume, resampling) is off by default. When it's on, the badge says so ("Volume levelled").
- **Honest signal path.** You can always see the source format, each processing step, and the
  output format and device.
- **Gapless, always.** The next track is decoded ahead of time and joined sample-accurately.
- **Never touches your files.** Play counts, history and queue live in `<library>/.venuta/`.
  Ratings go into the files only when you set one (existing RATING tag, journalled).
- **Fast and calm.** It starts instantly and scrolls smoothly over thousands of albums.
  Animations respond to what you do; nothing moves on its own except the slow backdrop.

## Engine (new crate `crates/player`)

```
file ──► decoder thread ──► ring buffer ──► output thread ──► device
          (symphonia)        (rtrb, ~2s)      WASAPI exclusive (event-driven)
          gapless prefetch                    └─ fallback: shared mode + rubato resampler
          seek / position                     └─ test sink (memory) for checks
```

- **Decode:** symphonia covers FLAC, ALAC/AAC (M4A), WAV, AIFF, MP3 and Ogg Vorbis. Output is
  integer samples at the source depth when possible (16/24/32), so exclusive mode passes them
  through as they are.
- **Output:**
  1. WASAPI exclusive, event-driven, using the `wasapi` crate (MIT). It switches the device to
     each track's format, with a fallback chain (24-in-32 → 32 → 16 bit), and reopens the
     device when the rate changes.
  2. Shared mode: the device mix rate, a high-quality rubato resampler, and float output.
  3. A memory sink for tests and the bit-perfect self-test.
- **Transport:** load, play, pause, stop, seek, next and previous, a queue with an index,
  repeat (off / all / one) and shuffle (songs / albums). Position events about 10 times a
  second.
- **Gapless:** while track N plays, track N+1 is opened and buffered. The output switches at
  the exact sample. When the format changes in exclusive mode, the device reopens with a
  short, clean pause instead of a click.
- **ReplayGain (optional):** track or album mode, with a pre-amp and clipping prevention. The
  tags already exist; the deep check writes them.
- **Volume:** in exclusive mode, the device's own hardware volume where it has one (stays
  bit-perfect). Otherwise digital volume, with the badge saying so.
- **Bit-perfect self-test:** play a file into the memory sink and compare the MD5 of the
  output samples with the file's STREAMINFO MD5. Settings → Players shows the result.
- **Media keys and Windows media overlay:** `souvlaki` (MIT), with title, artist, cover and
  playback state.

## Interface

**Music home**
- A greeting header with what's playing, or the last thing you played, as a large hero card.
- **Jump back in** (recently played albums), **Recently added**, **Most played**, **Rediscover**
  (albums not played in a while), then shelves by category (Film & Series, Independent…).
- Tabs: Home · Albums · Artists · Songs · Genres · Playlists. Search with `/`.

**Album page**
- Large cover with an accent from the art, title, artist, year, label, and format badges
  ("FLAC 24-bit 96 kHz", "Hi-Res").
- Play / Shuffle buttons, and a track list with numbers, titles, singers, durations, and a
  quality badge only when it differs from the album's.
- Double-click to play from a track. The right-click menu has Play next, Add to queue,
  Go to artist, and Edit details (opens Metadata).

**Artist page:** albums by year, top songs (by plays), and appearances on other albums.

**Now-playing bar** (always visible once something has played)
- Cover (click for Now Playing), title, artist (click to go to the artist).
- Previous, play/pause, next, shuffle, repeat.
- Seek bar with elapsed and remaining time. Hover shows a time preview; drag to scrub.
- Volume (wheel adjusts), quality badge (click for the signal path), output device, queue,
  lyrics.

**Now Playing** (full window)
- A big cover over the blurred artwork backdrop, with the accent following the cover.
- Synced lyrics scroll with the song: the current line is bright, the rest dim. Click a line
  to seek there.
- A side panel to switch between Up next (drag to reorder), the signal path, and details.
- Smooth shared-element animation from the bar's cover to the big cover.

**Queue:** a drawer from the right. Drag to reorder, remove, clear, save as a playlist.
"Playing from: Taal".

**Keyboard:**

| Key | Action |
|---|---|
| Space | Play / pause |
| ← / → | Seek 5 s |
| Shift ← / → | Seek 30 s |
| Ctrl ← / → | Previous / next |
| Ctrl ↑ / ↓ | Volume |
| L | Lyrics |
| Q | Queue |
| Ctrl+K | Command palette (with "Play …") |

## Data (in `<library>/.venuta/listening.sqlite`)
- `plays(file, at, seconds)` → play counts, recently played, history.
- `state(queue, index, position)`, so the next start picks up where you left off.

## Build order (one working, committed step at a time)

1. **Rename and home.** Rename Music → Metadata, add the new Music home with Albums and
   Artists browsing from the index (no audio yet), and give the album page a track list.
2. **Engine core.** The `crates/player` crate: decoding, ring buffer, transport, a gapless
   queue and the memory sink. Tests use generated test files (tones): position, seek
   accuracy, sample-accurate gapless joins, and an MD5 match (bit-perfect through the
   pipeline).
3. **Real output.** WASAPI exclusive with format negotiation and rate switching, plus the
   shared-mode fallback with rubato. Device list and selection. Night-time checks play
   digital silence only.
4. **App wiring.** Player state in the backend, Tauri commands and events, media keys and the
   Windows overlay, resuming the last queue.
5. **Now-playing bar and queue drawer,** with keyboard shortcuts and a Play action
   everywhere (album, song, palette).
6. **Now Playing view:** synced lyrics, signal path, up next, animations.
7. **Listening data:** plays and history, the Jump back in / Most played / Rediscover shelves,
   and play counts on album and artist pages.
8. **Options:** ReplayGain (off by default), hardware or digital volume, exclusive or shared
   mode, output device per session. They go in Settings → Players.
9. **Polish:** a waveform seek bar (computed on demand and cached), an optional spectrum,
   empty and loading states, reduced motion, light theme, and checks at phone-like narrow
   widths.

Later: DSD (DoP), ASIO, OpenSubsonic/Navidrome streaming of your own library, the official
YouTube Music window, Spotify remote control, playlist import/export, cards.

## Done means
- Steps 2–3 have automated tests, and the bit-perfect MD5 check passes on FLAC 16/44.1 and
  24/96.
- Every UI step is checked in the running app, with screenshots, in dark and light themes.
- Nothing ever writes to `D:\Collection`. Listening data stays inside `.venuta`.

## Progress (night of 2026-10-13)

| Step | State | Notes |
|---|---|---|
| 1. Rename + home | Done | Music (listening) and Metadata; Home, Albums, Artists, Songs, album and artist pages, search |
| 2. Engine core | Done | `crates/player`: gapless and sample-exact (tested); real FLACs match their STREAMINFO MD5 (16/44.1, 24/48, 24/96, 24/44.1) |
| 3. Real output | Done | WASAPI exclusive on the Realtek: 16/44.1 at 16-bit, 24/48 and 24/96 at 24-in-32, MP3 at 24-bit; shared fallback. Checked at volume 0 (silent) |
| 4. App wiring | Done | Commands and status stream, media keys and the Windows overlay, picking up where you left off |
| 5. Bar + queue | Done | Bar on every screen, queue drawer (drag to reorder), keyboard shortcuts, palette commands |
| 6. Now Playing | Done | Big cover, synced lyrics (click to seek), up next, signal path |
| 7. Listening data | Done | Plays (half the song or 4 min), Jump back in, Most played, resume (paused, same song, position, volume) |
| 8. Options | Partly | Output device and exclusive/shared in the badge. ReplayGain waits until loudness is measured for the library (only 18 songs have it) |
| 9. Polish | Ongoing | Bit-perfect check in the app; light theme checked; waveform seek bar next |
