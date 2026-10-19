# Using Venuta

## The places

**Music** is where you listen: the album playing (or one picked for today), your playlists,
shelves (jump back in, most played, hi-res, your categories), then albums, artists,
songs and playlists. Click to play; right-click a song or an album for the rest, including **Add
to playlist…**. Play goes in order, Shuffle turns shuffle on, and the queue says where it is
playing from.

**Now Playing** (click the cover in the player bar) shows the song large, with its lyrics, what's
next, the sound tools and the signal path. Every artist and the album open their pages; **More**
has Show in folder and Search the web.

**Metadata** lists the albums missing details; open one to edit its fields and song titles.

**Health** shows how complete and how sound the library is, with the tools: look up details
online, measure loudness and check quality, rescan, and the songs whose lyrics have no timing.

**Organize** tidies the library with your rules and files new music. What can be proven is done
for you, with its proof; what can't is asked, one question at a time, with the evidence side by
side.

**Playlists** (in Music) are ordinary `.m3u8` files in your library, so any player can read
them. Make them, drag to reorder, import them from elsewhere, or let smart playlists fill
themselves from rules. A playlist keeps every song, here or not: the ones you don't have play
from the online catalog in their places, so it plays straight through. Importing a playlist you already have fills it in and
keeps your own changes, and **Update from…** brings it up to date with its source. **To get**
lists the albums your playlists want and your library doesn't have.

**Import & export** (in Playlists) has two sides. **Bring in** takes a share link from Spotify,
Apple Music, YouTube Music, TIDAL, JioSaavn, Deezer or ListenBrainz, songs copied from an app,
a plain list, or any playlist file or folder of them, and shows what matched before anything is
saved; a bookmark reads playlists that only show when you are signed in. **Send out** writes
playlists in any format with paths for where they will be played, makes one backup of every
playlist, finds your songs on Deezer, Apple Music, Spotify or YouTube to rebuild a playlist
there, or copies it as text. **Check for new songs** looks at every imported playlist's source.
No account, nothing paid.

**Devices** writes SD cards for portable players. A card can carry the whole library or a
selection (playlists, albums, artists) in the library's own folder layout, converted to 16-bit
FLAC or AAC for players that need it; a gift card adds a tracklist and a list of what's missing.
**Test this player** puts a short test album on the card to check what your player reads.

**Activity** lists every build, edit, timing and answer, each with Undo. **Settings** holds the
look, startup and the tray, folders, online sources and keys, safety, and updates.

**The app icon** is yours to choose: Settings, Look and feel, App icon offers the Venuta mark (a V
with a peacock feather and a flute's holes) in 33 materials. Your choice is on the taskbar, the
tray, the windows and the shortcuts that open Venuta; the program file keeps Dusk glass.

## Lyrics

- **In Now Playing**, synced lyrics follow the song; click a line to jump to it.
- **Big screen** (F11): the lyrics filling a TV or a second monitor, with the next line coming
  in. While it shows, the other windows on that screen are minimized, and they come back as they
  were when it closes. Move the mouse for its controls; Esc closes it, Space plays and pauses,
  the arrows skip through the song, T adds Roman letters to lines in Indian scripts.
- **Desktop lyrics** (Ctrl+Shift+D, from anywhere, or the tray): the lines in a strip over every
  window. Unlocked, point at them to move them, size them from any edge and change them; **Lock**
  lets clicks pass through. Locked, point at them and a small lock appears to open them again.
- **Background, theme and style**, for each window on its own: Clear (nothing but the words),
  Glass, Frosted, the cover's light, or the theme's own scene; Venuta's theme or any other; and
  Clean, Karaoke, Glow, Outline, Serif or the theme's type, in the cover's colour, the accent or
  white, in four sizes. Both start out looking like Venuta. **Settings > Listening > Lyrics
  windows** has the same, for when nothing is playing.
- **Time lyrics yourself**: for words with no timing, play the song and press Space as each
  line begins; Backspace steps back, B marks a break, ← → nudge a line by a tenth of a second.
  The timing is saved as a `.lrc` beside the song (the song itself is never changed) and can be
  undone in Activity.

## Listen together

The people icon in the title bar starts a room: what you're playing goes on, now for everyone
in it. Copy the invite (a link, or a QR code) and send it to friends; each of them knocks, and
you let them in.

- **Everyone hears the same moment**, each from their best copy: their own file of the song, the
  online song, or yours, streamed to them from your PC. Over the internet or on your home Wi-Fi,
  with no account.
- **You choose who does what**: control, add songs, or just listen, for everyone new and for each
  person. You can undo the last skip or change, and take someone out.
- **The room's queue** shows in the queue drawer, with who added each song. Your own queue comes
  back, paused, when you leave.
- **Step out** pauses just for you; **Catch up** brings you back to where the room is.
- **Same room** (for speakers that hear each other) keeps every player within a millisecond or
  two, at the cost of bit-perfect playback while it corrects.
- **Settings > Listening > Listen together**: the name others see, hiding your IP address (the
  connection goes through a relay), and your own relay.

## Search

Search finds names and lyrics in any script and forgives spelling: "kesariya", "kesaria" and
केसरिया find the same song, and "dard mein zindagi" finds the line wherever it is written in
Devanagari.

Below your library, the **online catalog** answers too, with the real song ahead of covers and
remixes. Online songs play in the same player, and can go into playlists and the queue beside
your own. Your library always comes first.

## Shortcuts

| Keys | |
|---|---|
| Ctrl+K | Command palette: go anywhere, find anything |
| Ctrl+1 … Ctrl+5 | Music, Metadata, Health, Organize, Devices |
| Ctrl+H, Ctrl+, | Activity, Settings |
| / | Search your music |
| Space | Play or pause |
| Ctrl+← Ctrl+→ | Previous, next |
| L, Q | Lyrics, queue |
| F11 | Lyrics on the big screen |
| Ctrl+Shift+D | Desktop lyrics, from anywhere |
| Ctrl+M | Mini player |
| Ctrl+Shift+L | Light or dark |
| ? | Every shortcut |

Venuta keeps playing in the tray when you close the window (you're asked the first time), answers
media keys, and shows the song in the Windows media flyout.

![The mini player](screenshots/mini-player.jpg)

## Where things live

| What | Where |
|---|---|
| Your albums | `<library>\Music\` |
| Playlists | `<library>\Playlists\` |
| Set aside (never deleted) | `<library>\Set aside\` |
| Everything Venuta knows: settings, index, journals, caches | `<library>\.venuta\` |
| Which library is yours | `%APPDATA%\Venuta\library.json` |
| A card's identity and what it holds | `<card>\.venuta\` |

The whole library moves or backs up as one folder.

## The command line

`venuta` does the library work without the window: `scan`, `health`, `deepcheck`, `plan`,
`apply`, `undo`, `tidy`, `tidy-undo`, `rules-init`, `covers`, `incoming-plan`,
`incoming-import`, `card-setup`, `card-sync`, `apple-import` and `lookup`. `venuta --help` lists
them with their options.

```bash
cargo run -p cli -- scan <music-folder>     # read-only
```

## Sources

Details come from [MusicBrainz](https://musicbrainz.org) (CC0), the iTunes Search API, the
Deezer API and [LRCLIB](https://lrclib.net), each used within its published limits; Discogs and
AcoustID optionally, with your own keys.
