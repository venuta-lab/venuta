# 0004: SD card sync and playlists (milestones 4 and 5)

- **Date:** 2026-10-12
- **Status:** accepted

## Card sync

**Context.** Cards are the owner's main way to listen, at 128 GB, 256 GB and 1.5 TB. Some cards already hold a copy of the collection in the old folder layout. A sync must never cost a full rewrite, and must never lose files that another tool put there.

**Decision.**
- **Identity.** Each card is identified by `/.music-librarian/card.toml` (id, name, folder, which library folders to include, auto-sync). The volume serial is only a secondary check.
- **Manifest.** `manifest.sqlite` lives on the card and records what the app wrote and each file's audio MD5. Files are diffed by content, not by modification time.
- **Write order:** clean up `.part` leftovers, then delete, move, copy, patch. Copies go to `.part`, are flushed, then renamed, so a pulled card leaves nothing half-written.
- **Tag-only changes** rewrite only the metadata region, in place, when it fits. Audio is never rewritten. When a file has no stored MD5, the audio bytes are compared before patching.
- **Adoption.** Music already on a card is adopted by audio MD5 and moved into the new layout, not copied again.
- **Foreign files.** Files the app did not write are never touched.
- **Space** is checked before any write. **Eject** uses the system's own eject, and is refused while something holds the card.

**Evidence.** Tested on a scratch card:
- the first sync adopted an old-layout file and copied the rest;
- a re-sync wrote 0 bytes;
- a tag edit patched 1.2 MB in place;
- a move and a delete were applied;
- a foreign file was left alone.

**Consequences.**
- Auto-sync runs while the app is open (the Cards screen checks for cards every 5 s). A tray watcher is deferred.
- Spike S3 (write strategy on real cards) still needs the owner's cards. One writer with a 4 MiB buffer is the default until then.

## Playlists

**Context.** Plan D18 says the `.m3u8` files are the source of truth, so any player can use them. The Apple Music playlists exist only in Apple's encrypted library file.

**Decision.**
- **Static playlists** are `.m3u8` files in `<library>/Playlists/` with `../` relative paths.
  - Each entry is preceded by `#LIBRARIAN-MD5:<audio md5>`. Players ignore the comment; the app uses it to find a track again after it moves or is renamed.
  - Opening Playlists repairs and rewrites any playlist with a moved entry.
  - Deleted playlists go to `_Quarantine/playlists`. Renaming also rewrites the `#PLAYLIST:` header.
- **Smart playlists** are TOML rules in `<rules>/playlists/`. They are written out as `.m3u8` with a `#LIBRARIAN-SMART` marker and rewritten every time Playlists opens.
  - Fields: rating, unrated, album has a rating, genre, language, category, composer, lyricist, album artist, year, added within N days. Rules can match all or any, with an optional limit.
  - Three defaults are created once: *Top rated*, *Explore* and *Recently added*.
- **Apple import** reads `Library.musicdb`. It decrypts with AES-128-ECB over the first `max_crypt` bytes, then inflates the zlib data.
  - The key is entered by the user and kept in local settings. It is never shipped or committed.
  - Apple's file is only read.
  - Tracks are matched by normalised title, then a shared artist, then album as a tiebreak. Only one clear candidate counts as a match.
- **Want-list.** Albums with no local match go on a tiered want-list, also written as `wanted-albums.csv` next to the rules.
  - Tier 1: three or more saved songs, an album artist with three or more albums already in the library, or a score.
  - Tier 3: covers, karaoke and sleep filler.
  - Tier 2: everything else.
  - Nothing is ever downloaded.
- **Ratings.** "Rate unrated ★★★★" gives every unrated song in a playlist 4 stars (`RATING=80`). Meant for Apple's *Favourite Songs*, it goes through the backup gate and the edit journal, so it can be undone.

**Evidence.**
- The reader matches the earlier Python decoder: 1,909 tracks and 21 playlists.
- A dry run matched 413 of 1,713 playlist songs and listed 947 albums: tier 1 242, tier 2 681, tier 3 24.
- The one suspicious miss, *Katyar Kaljat Ghusli*, is correct. The local folder holds the old stage recordings; Apple has the 2015 film soundtrack.

**Not built yet** (from plan §8): playlist folders, merge and split, editor undo and redo, a live preview in the smart builder, the macOS XML and CSV importers, and a fuzzy duration-based match stage.
