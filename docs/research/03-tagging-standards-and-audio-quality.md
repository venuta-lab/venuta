<!-- Research report produced 2026-10-12 by a research agent; claims tagged VERIFIED/UNVERIFIED with sources. -->

# Tagging and audio-quality research for a FLAC library organiser (MusicBee and portable players)

**How the labels work.** **[V]** means VERIFIED: I read it in a primary doc, spec, source code or official wiki page. **[U]** means UNVERIFIED: it comes only from search snippets, forum posts or blogs, or I couldn't open the page. The MusicBee forum, the Hydrogenaudio wiki and rockbox.org pages sit behind Cloudflare or Anubis bot checks, and I did not try to get past them. Claims that rest on them are marked [U]. The MusicBee fandom wiki pages I read say they are current to v3.0, so treat them as older than today's MusicBee. Sources are listed as [S#] at the end.

---

## 1. Vorbis comment ground rules
- Field names are case-insensitive printable ASCII with no "=", and values are UTF-8 [S1][S2] **[V]**.
- Repeating a field, for example several ARTIST= lines, is "permissible, and encouraged" [S1] **[V]**.
- The FLAC spec (RFC 9639) formally defines only one field name, the channel mask. For everything else it points to MusicBrainz's tag list [S2] **[V]**.
- Picard writes multi-value tags as repeated keys, not a joined string [S4] **[V]**.
- MusicBee shows multiple values split by ";" in its UI, and the separator can't be changed [S7] **[V]**, [S16] **[U]**.
- I found nothing confirming whether MusicBee writes one ARTIST key per value or a single ";"-joined value in FLAC. **[U]** Test this with Tag Inspector before relying on either.
- A forum post says MusicBee's device sync splits ";" artists into separate tags when it converts files [S16] **[U]**.

## 2. Proposed canonical tag schema

| Field | Vorbis key | Example | MusicBee display | Device / compat notes |
|---|---|---|---|---|
| Title | TITLE | Tum Hi Ho | Title [S7] **[V]** | Universal [S2] **[V]** |
| Singers | ARTIST (one key per singer) + ARTISTS | Arijit Singh | Artist (multi-value); "Artists" virtual field [S7][S8] **[V]** | Rockbox reads Artist [S11] **[V]**. Picard's mapping has a separate ARTISTS key [S3] **[V]**. Untested: whether cheap players show only the first ARTIST value **[U]** |
| Display string | DISPLAY ARTIST | Arijit Singh & Shreya Ghoshal | Shown in place of the split artists [S7][S8] **[V]** | MusicBee-specific |
| Soundtrack composer(s) | ALBUMARTIST | A. R. Rahman | Album Artist [S7] **[V]** | Rockbox supports Albumartist for Vorbis [S11] **[V]**. Multi-value Album Artist is a disputed area in MusicBee [S16] **[U]**. Keep it to a single value |
| Music director | COMPOSER | Mithoon | Composer, plus DISPLAY COMPOSER [S7] **[V]** | Rockbox: Composer for Vorbis [S11] **[V]** |
| Lyricist | LYRICIST | Irshad Kamil | Lyricist [S7] **[V]** | Same key in Picard [S3] **[V]** |
| Album | ALBUM | Aashiqui 2 | Album | Universal [S2] **[V]** |
| This release's date | DATE | 2013-04-05 | Year ← DATE [S7] **[V]** | Picard: YYYY-MM-DD [S5] **[V]** |
| Original date | ORIGINALDATE (+ ORIGINALYEAR) | 2013 | Needs a **custom tag**. The wiki maps Original Year to Vorbis "ORIGINAL YEAR" [S9] **[V]**, but forum posts say ORIGYEAR and that ORIGINALDATE is ignored [S17] **[U]** | Picard writes ORIGINALDATE and ORIGINALYEAR [S3] **[V]** |
| Release type | RELEASETYPE (multi) | album; soundtrack | Custom tag (not in MusicBee's preset list [S9] **[V]**) | Picard: multi-value, from the release group [S5] **[V]** |
| Genre | GENRE | Filmi | Genre (multi-value) | Universal |
| Language | LANGUAGE | hin (ISO 639-3) | Custom tag "Language" → LANGUAGE [S9] **[V]** | Picard uses ISO 639-3 [S6] **[V]** |
| Script | SCRIPT | Deva / Latn (ISO 15924) | Custom tag | [S5] **[V]** |
| Label | LABEL | T-Series | MusicBee's "Publisher" is ORGANIZATION per the wiki [S7] **[V]**, so it **conflicts with LABEL** | Picard writes LABEL [S3] **[V]** |
| Catalog no. / barcode | CATALOGNUMBER / BARCODE (not UPC) | | Custom tags | [S3] **[V]** |
| ISRC | ISRC | INT101300123 | Custom tag ISRC [S9] **[V]** | Recommended by Xiph [S1] **[V]** |
| Track / disc | TRACKNUMBER, TRACKTOTAL, DISCNUMBER, DISCTOTAL **and** TOTALDISCS | 3 / 9 | MusicBee reads TOTALTRACKS or TRACKTOTAL, and TOTALDISCS [S7] **[V]** | Picard writes both total spellings [S3] **[V]** |
| Disc subtitle | DISCSUBTITLE | | Custom "Set Subtitle" [S9] **[V]** | |
| Media | MEDIA | Digital Media | Custom "Media Type" [S9] **[V]** | |
| Compilation | COMPILATION=1 only for Various Artists | | iTunes Compilation [S7] **[V]** | Picard: "1 for Various Artist albums" [S5] **[V]** |
| Rating | RATING, 0–100 | 80 | Track rating [S7] **[V]** | 0–100 is an informal convention [S18] **[V]**, Kodi [S19] **[U]**. See risk 1 for the clash with Picard |
| Love | LOVE RATING | L | Love | Seen in one Symfonium thread only, and others say love is database-only [S20] **[U]** |
| Loudness | REPLAYGAIN_TRACK_GAIN/PEAK, REPLAYGAIN_ALBUM_GAIN/PEAK | -7.02 dB / 0.988 | Read by MusicBee [S12] **[V]** | See §4 |
| Lyrics | LYRICS (LRC text) + sidecar `.lrc` | [00:12.30]… | Lyrics; the "Lyrics?" column shows synched vs unsynched [S8] **[V]** | See §3 |
| Cover | PICTURE block, type 3 (front cover) | baseline JPEG | Artwork | See §5 |

## 3. Synced lyrics
- **LRC format.** LRC has no formal spec. Lines look like `[mm:ss.xx]line`. "Enhanced" (A2) LRC adds per-word `<mm:ss.xx>` tags [S21] **[V]**.
- **Picard** reads UNSYNCEDLYRICS but writes only LYRICS. It never writes synced lyrics [S4] **[V]**.
- **MusicBee** stores lyrics either embedded (the default) or as an external `.txt`/`.lrc` file named like the track [S10] **[V]**. Which wins when both exist is not documented **[U]**. Forum posts say device sync doesn't convert between embedded and `.lrc` [S22] **[U]**.
- **Rockbox** (lrcplayer plugin) reads `.lrc`, `.lrc8` (UTF-8), `.snc` and `.txt`. Embedded lyrics work for MP3 SYLT/USLT only, so **embedded FLAC lyrics are not read**. Word time tags are accepted [S23] **[V]**.
  - Because `.lrc8` is described as the UTF-8 variant, plain `.lrc` may not be read as UTF-8. That matters for Devanagari lyrics (my inference).
- **Poweramp** reads the Vorbis LYRICS tag, tries to parse it as LRC first, and also supports `.lrc` files [S24] **[U]**.
- **Echo Mini.** FiiO says LRC is supported [S25] **[V]**. One owner says lyrics only show from a `.lrc` with the same name as the track, and embedded lyrics don't show [S26] **[U]**.
- **Snowsky Disc.** `.lrc` reportedly works after firmware 1.65 [S27] **[U]**.
- **Astell&Kern.** Lyrics display exists, but the source format isn't documented **[U]**.
- → **Write both:** LRC text in LYRICS, plus a UTF-8 sidecar `.lrc` with the same name as the track.

## 4. ReplayGain 2.0
- **Spec.** I couldn't open the Hydrogenaudio spec page (Cloudflare). The rsgain and loudgain READMEs say RG2 means EBU R128 / ITU BS.1770 loudness at a **−18 LUFS** reference, with uppercase tags written as `REPLAYGAIN_TRACK_GAIN [-]a.bb dB` and a peak as a linear `c.dddddd` [S13][S14] **[V]** (secondhand relative to the spec).
- loudgain's author says RG2 dropped `REPLAYGAIN_REFERENCE_LOUDNESS` [S14] **[V]** (author's claim).
- **rsgain:**
  - Runs on Windows 10+, and is also in Scoop [S13] **[V]**.
  - `rsgain easy <dir>` uses these defaults: −18 LUFS, album tags on, sample peak, clip protection for positive gains only, uppercase tags [S13] **[V]**.
  - It assumes **one folder = one album**, and `-p no_album` turns album tags off [S13] **[V]**.
  - `-m N` scans in parallel, and `-S` skips files that already have RG tags [S13] **[V]**.
- **loudgain** is ReplayGain 2.0, but its README says the maintainer currently has no time for it [S14] **[V]**.
- **r128gain** is FFmpeg-based. The man page calls it beta, and Opus output gain is "EXPERIMENTAL" [S28] **[U]**.
- **Opus** uses R128_TRACK_GAIN and R128_ALBUM_GAIN instead: a Q7.8 integer string referenced to −23 LUFS, with no peak tags (RFC 7845) [S13] **[V]**.
- **Player support:**
  - Rockbox reads RG from Vorbis tags. Modes are Album (falls back to track gain), Track, and Track-if-shuffling, plus prevent-clipping and a ±12 dB pre-amp [S29] **[V]**.
  - Poweramp has track/album RG and a preamp [S30] **[U]**.
  - A&K: SE180 firmware 1.22 lists "ReplayGain support" [S31] **[U]**. A review says the SP2000T's RG works only at ≤24/192 [S31b] **[U]**.
  - Echo Mini: no evidence either way.
- **MusicBee** has its own volume analysis that writes RG tags [S12] **[V]**. Its option to auto-tag new files is off by default [S10] **[V]**; keep it off so it doesn't mix with rsgain values.
  - Device sync's "level volume" uses RG tags only when it converts files [S15] **[V]**.

## 5. Cover art
- **FLAC limits.** A metadata block can be at most 2²⁴−1 bytes (about 16 MiB). Picture type 3 = front cover, and players generally show type 3 [S2] **[V]**. Picard refuses images bigger than a block [S4] **[V]**.
- **Block order.** Picard moves PICTURE blocks after the Vorbis comment because "Windows fails to read FLAC tags" otherwise [S4] **[V]**.
- **Rockbox:**
  - Its manual says embedded art is only JPEG in ID3v2/MP4 and marks Vorbis as unsupported [S11] **[V]**.
  - But its current source parses FLAC PICTURE blocks (JPEG/PNG) and uses **the first PICTURE block only** [S32] **[V]**. So the manual is stale.
  - **No progressive or multi-scan JPEG**, and no RLE BMP [S11] **[V]**.
  - Lookup order: embedded, then `./filename.*`, `./albumtitle.*`, `./cover.*`, `./folder.jpg`, and so on [S11] **[V]**.
- **Echo Mini** shows embedded JPG [S25] **[V]**. One owner says very large art won't display and 500×500 works [S26] **[U]**.
- **MusicBee:**
  - Its default display priority is Cover.\*, Artwork.\*, Front.\*, \<Filename\>.\*, then Embedded. **A folder cover beats embedded art** [S10] **[V]**.
  - Device sync can embed art and resize it to N px (default 200 when ticked) [S15] **[V]**.
- → **Recommendation (not from a source):** one front-cover PICTURE, baseline JPEG, put first, around 500–600 px for devices. Keep the full-resolution `cover.jpg` on the PC only.

## 6. Audio quality checks
- **Integrity.** STREAMINFO holds an MD5 of the decoded audio, and 0 means "unknown" [S2] **[V]**. `flac -t` decodes without writing a file and reports MD5 mismatches [S33] **[V]**. Files with a zero MD5 can't be checked this way (my inference).
- **Lossy source detection** looks for a spectral cutoff.
  - Fakin' The Funk (Windows, paid above 100 flagged files) compares the stated bitrate with the frequency cutoff [S34] **[U]**.
  - Spek is a free spectrogram viewer for Windows, macOS and Linux [S35] **[V]**.
  - auCDtect is old and fails at high bitrates. Lossless Audio Checker is no longer available, and its domain no longer resolves (my check) [S36] **[V]**.
  - Reviewers conclude there is "no reliable" way to tell high-bitrate MP3 from lossless [S36] **[V]**.
  - Genuine old or analog masters can have a 16 kHz cutoff, which causes false positives [S36] **[V]**.
  - → Report findings as **suspicion with a spectrogram**, never as a verdict.
- **Fake hi-res (upsampled):** content that stops at the Nyquist limit of a lower sample rate [S37] **[U]**.
- **Fake 24-bit:**
  - `sox file.flac -n stats` prints a "Bit-depth" pair. The **right-hand figure** counts bits above the LSBs that are fixed at zero, so 16-bit audio padded to 24 shows right-hand 16 [S38] **[V]**.
  - FLAC itself signals "wasted bits" (zero LSBs) in each subframe [S2] **[V]**.
  - A 16-bit source that was gain-changed or dithered will defeat this check [S38] **[V]**.

## 7. Duplicate detection
- **Audio MD5** covers decoded samples only, so it stays the same after retagging [S2] **[V]**. It only finds bit-identical audio (my inference).
- **ISRC** is a per-recording code [S5] **[V]**.
- **Chromaprint:** `fpcalc` fingerprints the first **120 s** by default (`-length`) and can output JSON. It is MIT-licensed, with FFmpeg parts bundled [S39] **[V]**.
- **Suggested tiers** (my design, not from a source): identical MD5, then same ISRC, then fingerprint similarity with a duration tolerance of about 2 s.
- MusicBee can send suspected duplicates to the Inbox [S15b] **[V]**.

## 8. File and folder naming
- **Picard's default script** is `$if2(%albumartist%,%artist%)/$if(%albumartist%,%album%/,)` + disc prefix + `$num(%tracknumber%,2) ` + `%title%` [S40] **[V]**.
  - Windows compatibility mode replaces illegal characters with `_` and enforces 259 characters unless "long paths" is on [S41] **[V]**.
- **beets defaults:**
  - Path: `$albumartist/$album%aunique{}/$track $title`.
  - Its replace rules remove `\/`, a leading `.`, control characters, `<>:"?*|`, a trailing `.` and spaces at either end [S42] **[V]**.
- **MusicBee** file organisation:
  - Default template: `<Album Artist>\<Album>\<Disc-Track#> <Title>`.
  - Illegal characters become a space [S43] **[V]**.
- **Windows:**
  - Reserved characters: `<>:"/\|?*` and codes 0–31.
  - Reserved names: CON, PRN, AUX, NUL, COM1–9, LPT1–9, and the ¹²³ superscript variants.
  - No trailing space or dot.
  - MAX_PATH is 260 unless the opt-in is set.
  - Don't assume case sensitivity [S44] **[V]**.
- **exFAT** names are at most 255 UTF-16 characters, use the same illegal set as FAT, and are case-insensitive but case-preserving [S45][S46] **[V]**. FAT32 has a 255-character name limit [S46] **[V]**.
- **Android** rejects `"*/:<>?\|`, codes 0–31 and DEL on FAT [S47] **[U]**.
- **Unicode normalisation.** HFS+ stored names as NFD, while APFS preserves whatever form it's given [S48] **[U]**. → Normalise to **NFC**.
- **Rockbox** replaces `\/:<>?*|` with `_` when it looks for `albumtitle.jpg` [S11] **[V]**, so `cover.jpg` is the safer name.

---

## MusicBee gotchas
1. Moving files outside MusicBee breaks the links, and they have to be relinked or play counts and date added are lost [S43] **[V]**. Your app should move files *through* MusicBee or rely on relink.
2. External tag edits are only picked up by a monitored-folder scan, either at startup or continuously [S15b] **[V]**. Auto-organise may not react straight away to tag changes made outside MusicBee [S43] **[V]**.
3. New files go to the **Inbox** by default [S15b] **[V]**.
4. The setting "Don't update file modification time" breaks device sync, which uses mtime to decide what to update [S10] **[V]**.
5. Copying tags or converting files drops any tags that aren't configured as fields or custom tags, so define them all [S7] **[V]**.
6. Custom tags are limited to 16 [S9] **[V]**.
7. The Publisher (ORGANIZATION) vs LABEL mismatch, and the Original Year key mismatch (see table).
8. A folder `Cover.*` beats embedded art [S10] **[V]**.
9. **.mbl library file:** binary and undocumented. Community parsers exist, and forum users warn the format can change without notice [S49] **[U]**. Use the optional iTunes XML export instead [S15b] **[V]**.
10. **.xautopf auto-playlists:** XML with a `SmartPlaylist` root and Conditions/Condition elements, but no published schema [S50] **[U]**.
11. **Device sync settings** [S15] **[V]**:
    - Naming template, defaulting to `Music\<Album Artist>\<Album>\<Disc-Track#> - <Title>`.
    - On-the-fly conversion.
    - Playlists as M3U/M3U8, and others.
    - Artwork embed/resize/folder.jpg.
    - Optional deletion of files not on the sync list.
    - How MusicBee identifies a device: **not documented**.

## Risks
1. **Rating collision.** Picard writes `RATING:<email>`, or plain `RATING` when the email field is empty, as a **0–1 float** [S4][S40b] **[V]**. MusicBee uses 0–100 in RATING [S18]. Your app must own one convention.
2. Manual/firmware claims are stale or conflict (the Rockbox manual vs its code, MusicBee wiki v3.0 vs forum posts). **Test on each device.**
3. Cheap DAPs: progressive JPEG, large art, embedded lyrics and multi-value artists may all fail **[U]**.
4. Lossy-detection false positives on older Indian film masters, which are often band-limited (my inference from [S36]).
5. rsgain album gain is wrong if a folder isn't exactly one album [S13].
6. Long Devanagari titles × 3 bytes per character × deep paths can hit the 255-character name and 260-character path limits.
7. Possible Unicode mojibake in `.lrc` files on Rockbox; use `.lrc8` [S23].

## Sources
- S1 https://xiph.org/vorbis/doc/v-comment.html
- S2 https://www.rfc-editor.org/rfc/rfc9639
- S3 https://picard-docs.musicbrainz.org/en/latest/appendices/tag_mapping.html
- S4 https://github.com/metabrainz/picard/blob/master/picard/formats/vorbis.py
- S5 https://picard-docs.musicbrainz.org/en/latest/variables/tags_basic.html
- S6 https://picard-docs.musicbrainz.org/en/latest/variables/tags_advanced.html
- S7 https://musicbee.fandom.com/wiki/Tagging
- S8 https://musicbee.fandom.com/wiki/Special_Fields
- S9 https://musicbee.fandom.com/wiki/Custom_Tags
- S10 https://musicbee.fandom.com/wiki/Tags_Preferences
- S11 https://github.com/Rockbox/rockbox/blob/master/manual/appendix/album_art_info.tex and …/file_formats.tex
- S12 https://musicbee.fandom.com/wiki/Analyze_Volume
- S13 https://github.com/complexlogic/rsgain
- S14 https://github.com/Moonbase59/loudgain
- S15 https://musicbee.fandom.com/wiki/Devices
- S15b https://musicbee.fandom.com/wiki/Library_Preferences
- S16 https://getmusicbee.com/forum/index.php?topic=16999.0 and https://getmusicbee.com/forum/index.php?topic=21929.0
- S17 https://getmusicbee.com/forum/index.php?topic=31433.0 and https://getmusicbee.com/forum/index.php?topic=39759.0
- S18 https://github.com/quodlibet/quodlibet/issues/3533
- S19 https://forum.kodi.tv/showthread.php?tid=338196
- S20 https://support.symfonium.app/t/synchronising-favourite-with-love-rating-vorbis-comment/12094
- S21 https://en.wikipedia.org/wiki/LRC_(file_format)
- S22 https://getmusicbee.com/forum/index.php?topic=24768.0
- S23 https://github.com/Rockbox/rockbox/blob/master/manual/plugins/lrcplayer.tex
- S24 https://forum.powerampapp.com/topic/25888-display-lyrics-from-unsyncedlyrics-tag-instead-of-vorbis-lyrics-tag/
- S25 https://www.fiio.com/echomini
- S26 https://maibuith.com/posts/fiio-snowsky-echo
- S27 https://www.tiktok.com/@madmotchi/video/7622398755626601750
- S28 https://www.mankier.com/1/r128gain
- S29 https://github.com/Rockbox/rockbox/blob/master/manual/configure_rockbox/playback_options.tex
- S30 https://forum.powerampapp.com/kb/en_us/guides/using-replygain-to-even-out-volume-differences-between-different-songs-r52/
- S31 https://www.astellnkern.com/support/download_detail.jsp?bbsNo=269
- S31b https://twister6.com/2021/11/04/astell-kern-sp2000t/2/
- S32 https://github.com/Rockbox/rockbox/blob/master/lib/rbcodec/metadata/flac.c
- S33 https://xiph.org/flac/documentation_tools_flac.html
- S34 https://djtechtools.com/2018/01/30/review-fakin-funk-low-bitrate-detection-utility
- S35 https://www.spek.cc/
- S36 https://www.thewelltemperedcomputer.com/SW/AudioTools/Detect.htm
- S37 https://github.com/NaughtyDogOfSchrodinger/lossless-checker
- S38 https://manpages.debian.org/testing/sox/sox.1.en.html
- S39 https://github.com/acoustid/chromaprint (src/cmd/fpcalc.cpp, LICENSE.md)
- S40 https://github.com/metabrainz/picard/blob/master/picard/const/defaults.py
- S40b https://github.com/metabrainz/picard/blob/master/picard/const/__init__.py (RATING_STEPS = 6)
- S41 https://picard-docs.musicbrainz.org/en/latest/config/options_filerenaming_compat.html
- S42 https://beets.readthedocs.io/en/stable/reference/config.html
- S43 https://musicbee.fandom.com/wiki/File_Organization
- S44 https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file
- S45 https://learn.microsoft.com/en-us/windows/win32/fileio/exfat-specification
- S46 https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison
- S47 https://dev.library.kiwix.org/content/stackoverflow_en_nopic_2021-08/questions/2679699/what-characters-allowed-in-file-names-on-android
- S48 https://eclecticlight.co/2017/04/06/apfs-is-currently-unusable-with-most-non-english-languages/
- S49 http://justsolve.archiveteam.org/wiki/MusicBee_Library and https://getmusicbee.com/forum/index.php?topic=41818.0
- S50 https://musicbee.fandom.com/wiki/API_Method_Reference and http://extensionary.blogspot.com/2014/04/xautopf.html
