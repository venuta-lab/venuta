<!-- Research report produced 2026-10-12 by a research agent; claims tagged VERIFIED/UNVERIFIED with sources. -->

# DAP + SD-card sync research (as of 2026-10-12)

**How to read the tags.** **[V]** means I fetched the page and it says this. **[U]** means it came only from a search snippet, a forum anecdote, or a page I couldn't open (Head-Fi, Rockbox, MusicBee wiki and forum, Poweramp forum and Headfonia all blocked the fetch). Inferences are labelled "inference". Numbered sources are listed at the end.

## 1. Device findings

**Snowsky Disc identified.** It is FiiO's SNOWSKY DISC, a digital audio player shaped like a CD Discman. It does not play discs. It runs on an Ingenic X2000 chip with FiiO's own Linux system and takes microSD cards up to 2 TB [8][V]. Retailers list the DACs as dual CS43131 and support for FLAC/WAV/AIFF/ALAC up to 384 kHz/32-bit plus DSD128 [12][U]. FiiO's own page names two different DAC chips in two places [8][V].

### Capability matrix

| Device (firmware, date) | Playlists | Cover art | Lyrics | ReplayGain | Filesystem / card | Notes |
|---|---|---|---|---|---|---|
| **FiiO SNOWSKY Echo Mini** (V3.9.0, 2026-08-31 [2][3][V]) | **No M3U.** The chip ignores .m3u files [4][V]. A spec table also says no M3U [6][U]. On-device Favorites only [1][V]. | Embedded JPG [1][V]. The community consensus is a square JPEG at 500–600 px; 300 px works in a pinch [5][V]. Images of 1000 px or more cause trouble [U, Reddit via search]. Baseline JPEG is not confirmed for this device. | LRC files and embedded lyrics [1][V]. A lyrics on/off switch was added in V2.8.0 [2][V]. | Unknown. V1.2.5 added a "Gain" setting, but nothing says it is ReplayGain [2][V]. | Official limit 256 GB [1][V]. Users report FAT32 and exFAT both working [U]. The RECHO custom firmware reads both [7][V]. | **The library is built from tags and is not rescanned at boot.** Since V1.7.0 it no longer updates itself on startup [2][V]. Artist grouping uses Album Artist (V1.5.0); CD-number sorting arrived in V3.5.0 [2][V]. **No gapless playback** [6][U]; RECHO advertises gapless as its own feature [7][V]. Unsupported ID3 frames (GEOB/PRIV) stop other tags from displaying [5][V]. Before V2.8.0, a changed drive letter wiped Favorites; V3.0.0 formats internal memory and Favorites [2][V]. Remove the SD card before a firmware update [2][V]. |
| **FiiO SNOWSKY DISC** (V2.57, 2026-09-11 [3][V]) | **M3U import and export** and multiple playlists since V2.40 (2026-08-24). V2.57 "improved M3U import compatibility" [9][V][8][V]. The path format, encoding and folder are **not documented**. | Embedded art [11][U]. **Separate image files are also read:** V2.57 fixed "external cover art not displaying" [9][V]. The expected filename is unknown. | Lyrics are displayed [8][V]. Bilingual lyrics improved in V1.86 [9][V]. LRC files are read only if tagged correctly before copying [11][U]. | Not in any changelog [9][V]. | Up to 2 TB [8][V]. Filesystem not documented. | Gapless added in V2.09; V2.21 and V2.28 fixed gapless bugs [9][V]. V2.40 fixed library-scanning bugs [9][V]. The Artist/Album views do not group by album artist (reviewed March 2026, before V2.40) [10][V]. |
| **Astell&Kern, current Crimson UI line** (SP4000 V1.041 2026-08-31; SP4000T V1.022 2026-09-14; PD20 V1.022 2026-08-05 [13][V]) | Playlists are built on the device. The SP3000M manual does not cover M3U import [14][V]. A third-party spec table claims M3U support on the PD10 [18][V], and on the PD20 and KANN MAX [U]. Older AK100s needed CRLF line endings and device-style paths (`/mnt/SD0/Music`) [19][U]. **Crimson UI M3U support: unknown, needs testing.** | Embedded art plus `cover.jpeg`/`folder.jpeg`. JPEG works on every model; later models also take PNG (2019 source) [16][V]. | Lyrics shown by tapping the cover [14][V]. LRC sidecar support not documented. | Yes, but the player **calibrates it on the device** ("start the calibration process") [14][V]. Whether it reads REPLAYGAIN_* tags is not documented. Limited to files of 24-bit/192 kHz or less [17][V]. | FAT32 preferred [14][V]. exFAT works; the official card limit is 1 TB [15][V]. | The library comes from metadata, not folders [15][V]. Copy only into `Music/` [15][V]. "Initialize Media Scan" rescans everything, and the time grows with file count [14][V]. PCM 8–768 kHz [14][V]. |
| **Anbernic RG Rotate** (Android 12, Unisoc T618, 32 GB internal [20][V]) | Depends on the app. **Poweramp** reads relative paths but writes absolute ones, and matches entries by last folder plus filename [25][U]. **Symfonium** needs paths relative to the playlist or source root, with playlist and music in the same source, and `\` changed to `/` [27][U]. **Musicolet** imports M3U [U]. **Neutron** reads M3U [29][U]. | Embedded art (app-dependent). | Poweramp: `.lrc` in the same folder, plus embedded USLT/SYLT, since build 948 (January 2023) [26][U]. Musicolet: LRC and embedded [28][V]. Neutron: LRC [29][U]. | Poweramp and Neutron yes [U]. | Microsoft-style partitioned SD cards are limited to 2 TB [22][V]. Up to 2 TB card [21][U]. **Never "adopt" the card:** an adopted card is encrypted and cannot be mounted elsewhere [22][V]. | On Android 10+, apps can modify only the primary volume through MediaStore [23][V]. Writing to the SD root needs `MANAGE_EXTERNAL_STORAGE` [24][V]. **Inference:** a sync tool should therefore write the card from the PC or over adb, not from an on-device app. After an adb push, forcing a MediaStore rescan is unreliable [U]. |

**Rockbox:** the docs were behind a bot wall, so nothing from Rockbox could be verified.

## 2. Sync prior art

- **beets-alternatives**
  - On update it adds missing files and deletes ones that no longer match.
  - It transcodes to the first listed format [30][V].
  - It **patches tags in place** when the target file's modification time is older than the source's [30][V].
  - It can embed, resize, and deinterlace cover art, i.e. make progressive JPEGs baseline [30][V].
  - **Its state lives in the source database**, as an `alt.<collection_id>` attribute (alternatives.py line 320) [30][V]. Nothing is stored on the target.
- **MusicBee**
  - It copies a file only when the library copy is newer [31][U].
  - Editing only tags or play counts causes the whole file to be re-uploaded [32][U].
  - A card not detected automatically is added as a "virtual device" by right-clicking its drive letter [31][U]. **Inference:** it is identified by drive letter.
- **FreeFileSync**
  - It writes `sync.ffs_db` after the first sync [33][V], one copy per side [U].
  - Detecting moved files needs stable file IDs; FAT may lack them [33][V].
  - Daylight saving time is handled with an exact one-hour shift [34][V].
  - Its fail-safe copy writes a temp file, then renames it [U].
- **rsync**
  - By default it writes a new copy, then moves it into place [36][V].
  - `--modify-window=1` exists for FAT's 2-second resolution [36][V].
  - `--delay-updates` puts all files into place at the end [36][V].
- **Syncthing**
  - It never writes directly to the destination: it uses `.syncthing.*.tmp` (or `~syncthing~*.tmp` on Windows), then renames [37][V].
  - It re-hashes a file only when its modification time, size, or permissions change [37][V].
  - Its index database lives in the config folder, not on the target [37][V].
- **robocopy:** `/FFT` (FAT 2-second times), `/DST` (one-hour shift), `/MIR`, `/Z` (restartable) [38][V].
- **foobar2000:** I found no built-in sync. FoobarSync, a third-party delta-sync script, stopped development in 2015 [39][U].
- **Strawberry:** copies to USB mass-storage and MTP devices with transcoding, and has a history of transcode-crash fixes [U].
- **EchoList (built for the Echo Mini):** a sync journal resumes interrupted syncs [4][V]. This is a direct precedent for a journal kept on the target side.

## 3. FAT32 and exFAT details

- **Timestamps.** FAT stores write times in **local time with 2-second resolution** [40][V]. After a daylight-saving change, FAT times can appear an hour off until reboot [40][V].
  - exFAT adds a 10 ms increment field and a **UtcOffset** field in 15-minute steps. The offset can be flagged as invalid [41][V].
  - **Inference:** never use modification time alone as the change signal. Allow a 2-second window and a one-hour shift.
- **Names.** exFAT is case-insensitive but keeps case. Names can be up to 255 characters [41][V].
  - Windows reserves `< > : " / \ | ? *` and control characters 1–31, plus names like CON/NUL/COM1. Names must not end in a space or a period [42][V].
  - MAX_PATH is 260 characters unless long paths are enabled [42][V].
- **Files per directory.** exFAT directories max out at 256 MB, about 2.79 million files [41][V].
  - FAT32 allows 65,535 entries [44][U]; FiiO's FAQ says 65,536 [43][V].
  - Long filenames use several entries each, so the practical FAT32 limit is about 20–32k files [U].
- **Clusters.** exFAT clusters can be up to 32 MB [41][V].
  - **Arithmetic:** with 128 KB clusters, each 2 KB `.lrc` file takes 128 KB. 20,000 lyrics files plus 2,000 cover files waste about 2.7 GB.
  - **Inference:** prefer embedded lyrics and art where the device supports them, or format with a smaller cluster size.
- **Formatting.** The SD Association "strongly recommends" its own formatter (v5.0.4 for Windows) over the operating system's [45][V].
- **Crash safety.**
  - Plain exFAT has a single FAT; a second copy exists only on TexFAT [41][V].
  - It has a VolumeDirty flag that should be set to 1 while metadata is being updated [41][V].
  - **Inference:** there is no journal, so a rename survives power loss only as well as the FAT and directory writes do.

## 4. SD card reliability

- **Windows removal policy.** Since Windows 10 1809 the default is **Quick removal**, which turns off write caching [47][V]. "Better performance" requires Safely Remove [47][V].
- **Speed ratings.**
  - **A1/A2** promise minimum random IOPS plus a sustained sequential speed. A2 performance needs a host that also supports A2 [48][V].
  - **Speed Class, UHS and V-class** numbers are a minimum sequential write speed, measured on unfragmented space [49][V].
  - **Inference:** for bulk FLAC copies the sequential V30/U3 rating matters more than A1/A2.
- **Fake cards.**
  - f3write/f3read fill the card with data and read it back; f3probe is fast but destructive and Linux-only [50][V].
  - f3write.h2w produces files "exactly like H2testw" [50][V].
- **Write endurance:** I found no authoritative source. **Inference:** a library sync is a light, mostly sequential workload.

## 5. Recommended sync protocol

1. **Identify the card by a marker file, not the drive letter.**
   - Write `/.<app>/card.json` with a UUID, device profile, and schema version. Cross-check it against the volume serial.
   - Drive letters change, which caused the MusicBee virtual-device problem [31][U] and wiped Echo Mini Favorites [2][V].
2. **Pre-flight checks.**
   - Filesystem type (exFAT or FAT32) and free space.
   - Whether the volume is dirty [41]; if so, ask the user to run chkdsk first.
   - Any leftover journal from an earlier run.
   - Offer an f3/H2testw check the first time a card is used [50].
3. **Load the manifest from the card** (`/.<app>/manifest.json` or SQLite).
   - Record per file: target path, size, audio-stream hash, tag hash, art hash, profile version, and the target modification time as observed.
   - The card is the source of truth, so a sync works from any PC. This differs from beets-alternatives, which keeps state on the source [30].
   - Never touch files that are not in the manifest.
4. **Diff by hashes, not timestamps.**
   - Use modification time only as a quick-check hint, with a 2-second window and a one-hour shift allowed [36][38][40].
   - Sort each change into one of five actions: add, remove, retag, re-art, or move.
5. **Write a journal on the card** listing the planned operations before changing anything (the EchoList precedent [4]).
6. **Order of operations:**
   - Deletes first, to free space.
   - Then new or changed audio, each written as `name.flac.part` in the same folder, flushed, then renamed. This is the rsync and Syncthing approach [36][37].
   - The `.part` extension keeps the player's scanner away from half-written files (inference).
   - Then tag-only changes: patch the tags in place, as beets-alternatives does [30], instead of re-copying the file (MusicBee's complaint [32]). If the new tags won't fit, use temp file plus rename.
   - Then sidecars: `.lrc`, and `cover.jpg` where the profile wants it.
   - **Playlists last**, once every file they reference exists.
   - Then replace the manifest atomically.
7. **Apply each device profile.**
   - **Echo Mini:**
     - Re-encode art as baseline JPEG, square, 500–600 px [5].
     - Strip unusual tag frames [5].
     - No M3U [4]; leave it out or offer EchoList-style virtual playlists.
     - Remind the user to run "update media library" on the device [2].
   - **DISC:** M3U with paths and encoding to be confirmed by testing [9].
   - **A&K:**
     - Write to `Music/` only [15] and embed JPEG art [16].
     - Don't rely on ReplayGain tags, since the player calibrates on the device [14].
     - M3U support to be tested.
   - **Android:**
     - `.m3u8` (UTF-8 [51]) with relative paths and forward slashes, in the same tree as the music [27].
     - `.lrc` beside each track [26].
     - The card must stay portable, not adopted [22].
8. **Verify.**
   - Read back and hash every newly written file, or a sample plus all changed files.
   - Rescan the card for stray `.part` files.
   - Clear the journal.
9. **Eject.** Flush buffers, then eject safely through Windows. Show "safe to remove" only after the eject succeeds [47].

## 6. Risks and unknowns

- **Untested on real devices:**
  - Whether A&K Crimson UI imports M3U files, and with what path syntax.
  - The DISC's M3U path, encoding, and folder rules, and the filename it reads for external cover art.
  - Echo Mini and DISC support for progressive JPEG and their maximum art size.
  - ReplayGain support on both Snowsky players.
- **Exact model needed:** the A&K model is unknown. Older A&K models may behave differently [15].
- **Echo Mini firmware:** upgrades can wipe internal storage and Favorites [2]. Its official card limit is 256 GB [1]; anything larger is anecdotal.
- **Android:** the adb rescan behaviour and each app's M3U parsing are taken from forums and need testing on the RG Rotate.
- **Assumption:** that exFAT renames survive surprise removal is an inference. The spec has no journaling outside TexFAT [41].

## Sources
[1] https://www.fiio.com/echomini · [2] https://forum.fiio.com/note/showNoteContent.do?id=202501210934383987154 · [3] https://www.fiio.com/fiionews · [4] https://github.com/purpleturtle21/echo-list · [5] https://community.mp3tag.de/t/fiio-snowsky-echo-mini/71000 · [6] https://www.hifioasis.com/dap/snowsky-echo-mini/ · [7] https://recho.atzkey.workers.dev/ · [8] https://www.fiio.com/DISC · [9] https://forum.fiio.com/note/showNoteContent.do?id=202601311712087234434 · [10] https://www.pragmaticaudio.com/reviews/2026/03/fiio-snowsky-disc/ · [11] https://www.headfonia.com/fiio-snowsky-disc-review/2/ · [12] https://addictedtoaudio.com.au/products/fiio-snowsky-disc-digital-audio-player · [13] https://www.astellnkern.com/en/support/download.php · [14] https://www.manualslib.com/manual/3595654/Dreamus-Astell-And-Kern-Sp3000m.html · [15] https://bloomaudio.com/blogs/articles/how-do-i-get-music-onto-my-astell-kern-dap · [16] https://www.blisshq.com/music-library-management-blog/2019/03/12/astell-kern-album-art · [17] https://twister6.com/2021/11/04/astell-kern-sp2000t/2/ · [18] https://www.hifioasis.com/?p=14160 · [19] https://www.head-fi.org/articles/syncing-music-and-making-playlists-work-with-the-astell-kern-ak100-or-ak120-and-a-mac.16437/ · [20] https://www.notebookcheck.com/Retro-Handheld-mit-rotierendem-Touchscreen-startet-in-den-Verkauf.1293907.0.html · [21] https://www.notebookcheck.com/Anbernic-RG-Rotate-im-Unboxing-Video-gezeigt-Specs-offiziell-bestaetigt.1286695.0.html · [22] https://source.android.com/docs/core/storage/adoptable · [23] https://developer.android.com/training/data-storage/shared/media · [24] https://developer.android.com/training/data-storage/manage-all-files · [25] https://forum.powerampapp.com/topic/4438-relative-path-names-in-m3u-playlists/ · [26] https://forum.powerampapp.com/topic/23478-support-for-lrc-lyrics-synced-lyrics/page/2/ · [27] https://support.symfonium.app/t/m3u-playlist-in-saf-mode-relative-path/2718 , https://support.symfonium.app/t/m3u-pls-import-from-local-device/2570 · [28] https://krosbits.in/musicolet/ · [29] https://alternativeto.net/software/neutron-music-player/about · [30] https://github.com/geigerzaehler/beets-alternatives (README and beetsplug/alternatives.py) · [31] https://musicbee.fandom.com/wiki/Devices · [32] https://getmusicbee.com/forum/index.php?topic=9470.0 · [33] https://freefilesync.org/manual.php?topic=synchronization-settings · [34] https://freefilesync.org/manual.php?topic=daylight-saving-time · [36] https://download.samba.org/pub/rsync/rsync.1 · [37] https://docs.syncthing.net/users/syncing.html · [38] https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/robocopy · [39] https://foobarsync.lima-city.de/ · [40] https://learn.microsoft.com/en-us/windows/win32/sysinfo/file-times · [41] https://learn.microsoft.com/en-us/windows/win32/fileio/exfat-specification · [42] https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file · [43] https://fiio.com/newsinfo/63364.html · [44] https://technet.microsoft.com/en-us/cc160978(v=msdn.10) · [45] https://www.sdcard.org/downloads/formatter/ · [47] https://learn.microsoft.com/en-us/windows/client-management/client-tools/change-default-removal-policy-external-storage-media · [48] https://www.sdcard.org/developers/sd-standard-overview/application-performance-class/ · [49] https://www.sdcard.org/developers/sd-standard-overview/speed-class/ · [50] https://fight-flash-fraud.readthedocs.io/en/latest/introduction.html · [51] https://en.wikipedia.org/wiki/M3U

Not cited above: Strawberry (snippets only) and FreeFileSync fail-safe copy (forum, https://freefilesync.org/forum/viewtopic.php?p=40695) [U].
