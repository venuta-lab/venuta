# 08: Four themes after the players people know

Asked 2026-10-13: "cider, apple music, Spotify, youtube music: add these themes, polished and
perfectly implemented everywhere."

## Names

The repository will be public, so the themes carry their own names. Each one says in its line
what it is after: **Green Room** (Spotify), **Orchard** (Apple Music), **Redline** (YouTube Music)
and **Glasshouse** (Cider). Typefaces are free near matches, because the players' own are not
licensed for reuse: Figtree for Spotify Mix, Inter for SF Pro, Roboto for YouTube Music.

## Measured

Values were read from each player's web app in the built-in browser on 2026-10-13, signed out
(computed styles and CSS variables).

| | Spotify (open.spotify.com) | Apple Music (music.apple.com) | YouTube Music (music.youtube.com) |
|---|---|---|---|
| Page | `--background-base` #121212 panels, 8px radius, 8px gaps on #000 | `--pageBG` #1f1f1f | #030303 (`--ytmusic-general-background-c`) |
| Surfaces | highlight #1f1f1f, elevated #2a2a2a, tinted `#ffffff1a` | sidebar `rgba(38,38,40,.6)`, player `#2d2d2de0` floating | #181818, #212121 (player bar) |
| Text | #fff, subdued #b3b3b3 | `#ffffffeb` / `a3` / `66` | #fff, #aaa |
| Accent | #1ed760 (green Play, black icon, 48–56px circle) | key #fa586a on dark (#fa233b light); buttons #fa2d48 | links #ff4e45; red progress; white pill buttons |
| Titles | 96px / 800, tight | 26px / 700 | Roboto, YouTube Sans |
| Rows | 56px, 4px radius, white 10% on hover, 40px art r4 | 46px, hairlines | thin separators, 48px nav items r8 |
| Header | gradient from the cover colour (oklch) into the page | plain | blurred cover |

Cider: version 1's source (open source) gives key #fc3c44 and Cider pink #ff2654, with base #1e1e1e
and sidebar #2e2e2e at 50% over an acrylic window. The current app (cider.sh) has black chrome and
an inset rounded stage. The album header is tinted from the cover, Play and Shuffle are pills in
a light version of that colour with dark type, and the player floats inside the stage.

## How they are built

- Tokens per theme in `themes.css` (colours, surfaces, radii, typefaces), plus signature pieces
  on stable hooks the components carry:
  - `.stage`: the page's panel;
  - `.np-bar` and `.rail-main`: the real bar and rail, never the previews;
  - `.hero-glow`, `.hero-title`, `.play-main`, `.shuffle-main`: album and playlist headers;
  - `.card`, `.card-art`, `.card-play`, `.row`: cards and rows;
  - `.seg`, `.seg-item`, `.seg-on`: tabs and choices;
  - `.np-play`, `.palette-on`, `.toast`: the player's Play, the palette and notices.
- `--art` is the cover playing (or the album open), as a tint deep enough for white type. It is
  set for every theme; Green Room and Glasshouse colour their headers with it.
- Glasshouse follows the cover for its accent too, like Venuta. The other three keep their
  players' fixed accents. Orchard has a light and a dark version, the rest are dark like their
  players.
- Orchard and Glasshouse float the player over the page. The page leaves room for it, and notices
  rise above it.
- Each has its own seek line (`ThemedTrack`), which the mini player uses too.

## Checked

In the test copy (scratch library, muted): Music home, an album, Playlists, a playlist, Health,
Organize, Devices, Activity, Settings with its gallery, Now Playing, the queue, a context menu, the
palette, a notice, and the mini player, for each theme (Orchard light and dark).
