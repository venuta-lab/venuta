# 0007: Cards that carry a selection, laid out as a library

- **Date:** 2026-10-14
- **Status:** accepted (built; real players still to test)
- Research: [10](../research/10-cards-that-carry-a-selection.md). Plan: [PLAN-YOUTUBE-AND-CARDS](../plans/PLAN-YOUTUBE-AND-CARDS.md) part B.

## Context
He wants to share some of his music, a few playlists or albums, on a card, or put the whole
collection on one, and the card should be an organised library the other person can grow from,
saying what is missing. Until now a card carried the whole library or some top-level folders,
playlists went all or nothing, and a playlist never pulled its songs onto a card.

## Decision
- **A card is a saved selection**: everything, or playlists + albums + artists (album artists and
  singers, as the Music screen names them), unioned, each song once. Kept in `card.toml`
  (`pick`), with the card's kind (`player`, `gift`, `car`), `convert` and gift `extras`.
- **The library's own layout** for every file, partial albums included; never a folder per
  playlist. Card paths are made safe (no `\ / : * ? " < > |`, no trailing dots or spaces, NFC) and
  at most 248 characters from the card's top; only paths that would be longer are shortened,
  with "~", a whole album folder the same way, a song's `.lrc` keeping its song's name.
- **What's missing** (gift cards, each switchable): `Tracklist.txt` in each album the card holds
  only part of (here / not on this card / not in the library, by the album's own track count),
  `Missing.csv`, `README.txt`; playlists keep songs not on the card as `#VENUTA-WANT` lines
  (players skip them; Venuta reads them). A missing song is never written as a path:
  Poweramp matches entries by folder and file name and could play a different file.
- **Smaller files, per card**: hi-res as 16-bit FLAC within its rate family (96 to 48, 88.2 to
  44.1; dither only where there is detail below 16 bits; skipped for songs the deep check found
  to be 16-bit audio in a 24-bit file), or lossless as AAC 192 kbps by Windows' Media Foundation
  (nothing downloaded). Converted on this computer several at a time, then written in one
  stream; never moved or patched as the library's audio; a re-sync never converts again.
- **Safety**: every copy is read back and compared before it is renamed into place; a reserve
  (1 GB + 2% of the card, at most 8 GB) is kept free; the Echo Mini refuses cards over 256 GB;
  a card whose settings were copied from another is noticed by its volume serial and can be
  made its own; car and FAT32 limits are warned about; only what Venuta wrote is removed.
- **No card journal or Undo.** A card only holds copies and nothing else on it is touched, so a
  sync can't lose anything; choosing the old selection and syncing again restores it exactly.

## Evidence
- Engine tests (`sync.rs`): a gift of one playlist (folders, track list "1 of 4", Missing.csv,
  README, the playlist's gap), a re-sync that does nothing, a change of selection that removes
  only Venuta's files; card paths; 24/96 → 16/48 FLAC at the right length; AAC with tags
  (Windows); dither keeps 16-bit-in-24 audio exact.
- On his library (a hard-linked copy) to a stand-in card (a `subst` drive): 2 playlists and 2
  albums, 30 songs; Jodhaa Akbar 623 MB as 24/96, 266 MB as 16/48 (estimate 277 MB).
- flacenc writes the last block's size as the minimum block size; symphonia (and maybe players)
  refuse such a stream, so `convert::fix_min_block` corrects the header.
- Release build, one 6-minute 24/96 song: 16-bit FLAC in 5 s, AAC in 11 s, one core each.

## Still open
- B6: the player test on his real cards (does the Astell&Kern read M3U8, or need `.pls`? the
  Snowsky Disc?). Until then those players are written as now.
