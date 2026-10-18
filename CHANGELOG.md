# Changes

## 0.3.0 (2026-10-18)

- **Listen together.** Start a room from the title bar and send the invite. Everyone hears the
  same song at the same moment, each from their own file, the online song, or yours streamed to
  them, over the internet or your home Wi-Fi, with no account. You choose who can control, add
  songs or just listen.
- **Lyrics windows, made over.** The big screen fills a TV or a second monitor and clears that
  screen while it shows. The desktop lyrics are a strip you move, size from any edge and lock so
  clicks pass through. Each has its own background (Clear, Glass, Frosted, the cover's light or
  the theme's scene), its own theme and its own style.
- **The online catalog beside your music.** Songs, albums, artists, playlists, podcasts and
  videos, with suggestions as you type, moods and genres with their own pictures, and pages like
  a music app's. Online songs start in a fraction of a second, play as Opus, show their waveform
  and are levelled like your files.
- **Your music, always in reach.** Your music on Home and in Search opens everything on this PC,
  and genres bring your files and the catalog together. **Local only** shows just your own music
  and makes no online requests at all.
- **Sign in with Google** for your YouTube Music Home, playlists and radios. Likes go both ways.
- **Liked songs**, radio from any song or artist, autoplay when a queue runs out, dislike a song,
  block an artist, and **Listening**: your stats and history.
- **A new Home**: a greeting, mood chips, Quick picks, the catalog's rows and the Shortcuts you
  pin. **Edit Home** puts sections in your order and sizes the cards.
- **Eight lyrics sources**, chosen per song, and lyrics for online songs everywhere.
- **Phones over Wi-Fi**: Subsonic apps on Android and iPhone play your library and keep albums
  and playlists for offline.
- **Shortcuts from anywhere** on the keys you choose, one search in the title bar, Ctrl+K finds
  music first, the interface size, a proxy for online requests, what Venuta keeps (and clearing
  it), song notifications, and pre-releases for trying a version early.
- **Fixed**: albums go straight into a card's music folder; ejecting from a card reader says it
  worked; Layer Zero's cursor stays steady while a song plays.

## 0.2.3 (2026-10-16)

- **Playlists play straight through.** Songs you don't have play from the online catalog in
  their places, found for you when you bring a playlist in, update it, or open it (with Undo).
  Only the songs the catalog isn't sure about wait on To get.
- **Fixed**: playing an online song no longer opens a console window.

## 0.2.2 (2026-10-16)

- **A polished installer**: the Venuta icon on the setup file, and branded install screens.

## 0.2.1 (2026-10-16)

- **Fixed**: two lyric timings saved in the same moment each keep their own Undo in
  Activity (the second used to replace the first's).

## 0.2.0 (2026-10-15)

- **Import & export**, one screen with two sides. **Bring in** reads a share link from
  Spotify, Apple Music, YouTube Music (the whole playlist, with its name), TIDAL, JioSaavn,
  Deezer (and your Deezer favourites) or ListenBrainz, free and without signing in; songs
  copied from Spotify; a plain list; or any playlist file by what's in it (M3U, PLS, XSPF,
  JSPF, WPL, ASX, CSV, spreadsheets, zips). It shows what matched before anything is saved,
  and a close match you turn down stays turned down. A bookmark reads playlists that only
  show when you're signed in.
- **Send out**: every format, with paths for where the playlist will be played (this PC, a
  phone, a card); one backup of every playlist that brings them all back, empty ones too;
  your songs found on Deezer, Apple Music, Spotify or YouTube to rebuild a playlist there,
  free; or the playlist as text.
- **Check for new songs** in every imported playlist at once.
- A playlist from a phone or another computer finds the same files here.
- **The online catalog** beside your library: search shows your library first, then the
  online catalog with the real song ahead of covers. Online songs play in the same player and
  go into playlists and the queue; Home follows what you play.
- **Search**: one rule for every search box, its forgiveness scaled to what you type; the
  first search after starting stays smooth.
- **Add songs** to a playlist is one search, your library first, then online; "already in
  this playlist", with Add again.
- **Undo** when a play replaces your queue, and when a playlist is deleted. New playlist is a
  draft until it has a name or a song.
- **Drag** music where it should go, with handles that show; one menu for the window and every
  right-click, read from one list of actions.
- **Quicker**: Playlists opens in a fifth of the time; Albums, Artists, Metadata and Home
  open without a stall; a whole-library queue opens at once; the window rests when nothing
  is happening.
- **Fixed**: a genre like R&B stays whole (it was split into "R" and "B"); online search
  results keep their columns inside the window; the song columns in a search sit with their
  songs, not over the Top result; the theme search box no longer draws its icon over the text.

## 0.1.1 (2026-10-14)

- **Themes anyone can make.** The Theme Studio (Settings, Look and feel, New theme) makes a
  theme from the theme in use, the album playing, or a few colours, with the whole app as the
  preview: Simple makes every value from a few choices and keeps text readable; Advanced edits
  every value, fonts, a picture behind everything, effects and an expert stylesheet. Undo,
  Revert, and every save keeps the version before it.
- **Seven new themes**: Midnight Sakura, Amber Terminal, Fjord, Velvet Lounge, Synthwave, Matcha
  and Ink Wash, made as theme files like yours.
- **Share themes** as one `.venuta-theme` file (Export… and Import…, or drop it on Venuta), with
  a preview and anything wrong shown before it's added. Themes are folders in your library's
  `.venuta/themes`; edit one by hand and Venuta reloads it as you save. docs/THEMES.md lists
  every value, and Settings copies a prompt for making a theme with an AI assistant.
- **Look and feel, rearranged**: the theme in use with its mode and actions, the themes you used
  lately in one row, and every theme in a browser that opens when you want it (search, filters,
  groups that show one row until opened). Colour, Window, Motion and App icon are one line each
  saying what they are set to, opened to change them.
- **The album background switch works in every theme that has it** (it did nothing in Frosted,
  Neo-Tokyo, Thermoptic, Sync Ratio and Layer Zero), and **Frosted shows the album** as a calm
  tint of the window, the way Windows tints Mica with the wallpaper (it was all but invisible).
- Built-in themes come first: in the theme browser, its filters, and the suggestions.
- A theme that can't load never breaks Venuta: the theme it's built on shows, and Settings says
  why. Reduced motion no longer fades the accent between albums.
- **A logo**: the Venuta mark, a V with a peacock feather and a flute's holes, on the program,
  the installer and the title bar.
- **Choose the app icon** in Settings, Look and feel: 33 materials, on the taskbar, the tray, the
  windows and the shortcuts that open Venuta.
- Every request to play is noted in `.venuta/logs/playback.log` (what asked, and from where), to
  find out what started a song nobody pressed play on.

## 0.1.0 (2026-10-14)

The first tagged version.

- **Library**: scan and index, your own rules for folders, names, artist spellings and
  categories, a plan you review before anything changes, builds and tidies that can all be
  undone, and Set aside instead of deleting.
- **Lookups**: original years, composers, lyricists, sharper covers and synced lyrics from
  MusicBrainz, Apple, Deezer and LRCLIB (Discogs and AcoustID with your own keys).
- **Health**: completeness and sound checks (damage, lossy sources, fake hi-res, loudness).
- **Player**: bit-perfect exclusive WASAPI, gapless, waveform seek bar, equaliser, tone,
  volume levelling, crossfade, sleep timer, mini player, tray and media keys.
- **Lyrics**: synced lyrics in Now Playing, a big screen for a TV or second monitor, desktop
  lyrics over every window, six styles, and timing lyrics yourself by tapping along.
- **Playlists**: `.m3u8` files beside your music, smart playlists, imports from Spotify, Apple
  Music, YouTube Music, Deezer, TIDAL and others, with missing songs kept in their places.
- **Cards**: the whole library or a selection on an SD card, in your library's layout,
  converted for players that need it, gift cards that say what's missing.
- **Search** across scripts, in names and lyrics.
- **Eleven looks**, after the players you know and a few retro ones.
