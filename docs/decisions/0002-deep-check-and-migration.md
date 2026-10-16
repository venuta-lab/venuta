# 0002: Deep check, planner and apply (milestones 1–2)

- **Date:** 2026-10-12
- **Status:** accepted

## Deep check (quality and loudness)

We run one decode per album (symphonia 0.6 FLAC with `verify`). That single pass produces:
- the MD5 integrity result;
- the averaged spectrum, sampled every 12th 8192-point block;
- the OR of all samples, which gives the bits actually used;
- EBU R128 loudness per track and per album (`ebur128`, `loudness_global_multiple`).

**Full owner library: 2,688 tracks (192 h) in 4 m 40 s.**

| Finding | Result |
|---|---|
| MD5 verified | 2,645 tracks |
| No MD5 stored | 43 tracks |
| Damaged | 0 |
| Decode errors | 0 |
| Upsampled "hi-res" (nothing above 23 kHz) | 14 albums, matching the independent Python/ffmpeg audit exactly |
| Padded 24-bit (only 16 bits used) | 87 tracks |
| Cutoff below 19.5 kHz on ≤48 kHz files | 106 tracks in 33 albums |

**Why the cutoff results differ from the Python audit, and why we keep ours:**
- The earlier Python audit reported 22,050 Hz (Nyquist) for these tracks.
- The likely cause is noise-shaped dither near Nyquist, which a single-bin threshold reads as content. Our 500 Hz smoothing ignores it.
- The flagged set is plausible: deliberately lo-fi tracks (cut at 10.9 kHz), Lata Mangeshkar-era recordings, and early-2000s film albums re-issued from band-limited masters.
- Shown as a **note with a spectrogram**, never as a verdict.

**Detail:** symphonia left-justifies FLAC samples to 32 bits, so effective bits = `32 − trailing_zeros(OR of all samples)`.

## Planner decisions made while testing on the real library

1. **The composer comes from my folders for soundtracks.** For example, `…/A_R_Rahman/2004_Swades` gives "A.R. Rahman", with spelling canonicalised through the aliases.
2. **Album artist, album and year are per track for non-soundtrack folders.** A folder of loose singles (e.g. `Anuv_Jain`) holds many releases. Without this, every single was filed under the folder's most common album.
3. **Extra names in the old album artist go into `CREDITS`, not `LYRICIST`.** They mix lyricists, remix composers and cast (e.g. "Vishal-Shekhar, Zeus, Manj Musik, John Stewart Eduri"), so guessing roles would mislabel people. Milestone 3's credit sources will sort them.
4. **`ARTIST` holds the readable singer string; `ARTISTS` holds one value per singer** (the Picard convention). This shows correctly in every player while keeping structured data. Spike S2 will confirm per player.
5. **A folder year that differs from DATE is medium confidence.** It is right for some albums (Gangster 2006 vs DATE 2024) and wrong for others (the My Name Is Khan folder says 2017; the film is from 2010). Milestone 3 will settle it with MusicBrainz first-release dates.
6. **`UNSYNCEDLYRICS` is only removed when it has the same words as `LYRICS`.** The first version removed it whenever `LYRICS` existed. That would have deleted Devanagari lyrics sitting beside romanized synced lyrics (Roja).
7. **Soundtrack labels are removed anywhere in the album title,** not only at the end: "Interstellar (Original Motion Picture Soundtrack) [Expanded Edition]" becomes "Interstellar [Expanded Edition]".
8. **Exact duplicate albums (identical MD5 sets, or a subset) are set aside.** My composer folder is preferred over Collections, and underscore naming over spaces.

Result on my library: **330 albums planned in about 0.3 s.**

| Outcome | Albums |
|---|---|
| High confidence | 159 |
| Medium confidence | 165 |
| Low confidence | 4 |
| Set aside as duplicates | 2 |

There are also **3 same-artist questions**: Pritam/Pritam Chakraborty, Madan Mohan/Late Madan Mohan, The Local Train/The Local Music Train.

## Apply and in-place edits

- **Migration copies the source and never writes to it.** Each file goes through: `.part` copy → tag write → audio fingerprint unchanged → flush → rename. A journal records every created file, and undo removes them along with any folders left empty.
- **Windows needs a writable handle to flush.** `FlushFileBuffers` fails on a read-only handle. The first test run caught this: every file failed before its rename, so no partial file reached a final name.
- **Test on 4 albums (65 tracks):**
  - audio MD5 identical on all;
  - tags equal the plan exactly (mutagen);
  - re-run skips all;
  - undo removes all 99 created files.
- **In-place edits** (owner's tag edits and ratings) use the same proof and journal the previous tags. Undo restores every tag; the bytes can differ only in metadata padding, see 0001. A one-time backup confirmation is required first.
