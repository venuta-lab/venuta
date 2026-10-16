# 0001: Spike S1, tag-write safety with lofty 0.25.4

- **Date:** 2026-10-12
- **Status:** accepted

## Context

Every change the app makes to the library is a tag write. The research ([01](../research/01-app-stack-and-rust-crates.md)) found that lofty is the best-maintained Rust tag library. It also found that lofty had FLAC corruption bugs fixed in 2025–2026 (#549, #640) and has one open (#731). We needed evidence on real files before building the apply step.

## Method

The test lives in `crates/engine/examples/s1_write_safety.rs`.

- **Sample.** 200 FLAC files spread evenly across my library, copied to a scratch folder. The originals were only read. The sample covers 16/44.1, 24/48 and 24/96 files, MD5-less files, and files with MusicBrainz, Tidal and ffmpeg tags.
- **Edits.** Six edits per file, each saved with `FlacFile::save_to`, 1,200 writes in total:
  - modify a title
  - add multi-value ARTIST, LYRICIST and ORIGINALDATE
  - a large Devanagari LYRICS tag
  - a 64 KiB tag that overflows the padding
  - remove and re-insert the front cover
  - strip keys and shrink the file
- **Checks after every write:**
  - **Audio fingerprint.** The STREAMINFO bytes and every byte from the first audio frame to EOF must equal the original's. This is `engine::media::flac_layout`. It is stricter than comparing decoded samples, because any changed byte fails.
  - **Tags** compared to a model of the expected result.
  - **Pictures** compared by hash.
- **Independent verification.** The final files were re-read with mutagen (Python) and compared against the expected tags.

## Results

| Check | Result |
|---|---|
| Audio bytes or STREAMINFO changed | **0 / 1,200 writes** |
| Picture problems | 0 |
| Save errors | 0 |
| Tag values, per key, in order (mutagen) | 198 / 200 exact. The 2 differences are below |
| Average write time | ~10 ms (full rewrite, warm OS cache) |

Behaviours found:

1. **Empty-valued fields are dropped on write.** Examples are `copyright=""` and `lyricist=""`, found in 2 files. Harmless, and they are junk anyway.
2. **Order between *different* keys is not preserved.** `ALBUMARTIST` (140 files), `encoder`, `ALBUM` and `BPM` move to the end. The order of repeated values of the *same* key (multi-value ARTIST) **is** preserved, and key case is preserved. Vorbis comments give no meaning to order between different keys.
3. **Every save rewrites the whole file.** lofty does not reuse existing padding (upstream issue #445), and the audio offset moved on ~100% of writes.

## Decision

- **Use lofty 0.25.4 for library writes**, always through *temp copy → write → verify audio fingerprint → rename*. The fingerprint check is mandatory on every write, not sampled.
- **The apply step's model must include empty-field removal**, so the review diff shows it and the user is never surprised.
- **Ignore order differences between keys** when comparing tags. Compare values per key, in order.
- **SD-card tag patching (M4) needs its own metadata-region writer.** It must write new VORBIS_COMMENT and PICTURE blocks into the existing metadata space, using padding, so audio bytes are never rewritten on the card. It can reuse `flac_layout` and the same fingerprint check. Without it, every retag on a card would re-copy about 30 MB.
- **Pin lofty to `=0.25.4`.** Upgrade only after this spike passes again on the new version.

## Not covered here

The `ebur128` vs `rsgain` loudness comparison, originally grouped under S1, moves to the M2 loudness work.
