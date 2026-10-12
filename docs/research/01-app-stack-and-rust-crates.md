<!-- Research report produced 2026-10-12 by a research agent; claims tagged VERIFIED/UNVERIFIED with sources. -->

## Technical foundation report: FLAC library manager and DAP sync app (researched 2026-10-12)

Each claim is tagged **[V]** (verified at the cited URL today) or **[U]** (unverified, inference or secondary source). Crate versions and dates come from the crates.io API (`https://crates.io/api/v1/crates/<name>`) [V].

### 1. Tauri 2: confirmed as the right choice

**Version and plugins**
- Current release is `tauri` 2.12.3 and CLI 2.12.2, both 2026-10-10 [V] https://github.com/tauri-apps/tauri/releases
- Current plugin versions [V] https://crates.io/crates/tauri-plugin-updater:
  - shell 2.5.0
  - single-instance 2.5.3
  - autostart 2.7.0
  - updater 2.13.3
- The plugins need Rust 1.90 or later [V] https://v2.tauri.app/plugin/updater/

**Tray**
- The tray needs the `tray-icon` feature [V] https://v2.tauri.app/learn/system-tray/
- On Linux, tray click events are not supported. Only the right-click menu works [V] (same page).

**Single instance**
- The single-instance plugin must be registered first [V] https://v2.tauri.app/plugin/single-instance/
- Its callback receives the second launch's `args` and `cwd`, so a CLI call can be forwarded to the running app [V] (same page).

**Sidecar (ffmpeg)**
- Configured through `bundle.externalBin`. Each binary needs a `-<target-triple>` filename suffix [V] https://v2.tauri.app/develop/sidecar/
- Running it needs `shell:allow-execute` or `shell:allow-spawn`, and arguments can be checked with a regex [V] (same page).

**Updater**
- Signatures are mandatory: "This cannot be disabled" [V] https://v2.tauri.app/plugin/updater/
- A static JSON file on GitHub Releases works as the update endpoint [V] (same page).
- `tauri-action` v1.0.0 was released 2026-06-29 [V] https://github.com/tauri-apps/tauri-action/releases
- The documented CI matrix covers macOS arm64/x64, Ubuntu 22.04 x64/arm64 and Windows x64 [V] https://v2.tauri.app/distribute/pipelines/github/

**WebView2 install modes** [V] https://v2.tauri.app/distribute/windows-installer/

| Mode | Added installer size | Note |
|---|---|---|
| downloadBootstrapper (default) | +0 MB | needs internet at install |
| embedBootstrapper | about +1.8 MB | |
| offlineInstaller | about +127 MB | |
| fixedVersion | about +180 MB | figure from v1 docs [U] |

**ffmpeg licensing**
- Avoid `--enable-gpl` and `--enable-nonfree` [V] https://ffmpeg.org/legal.html
- That checklist asks for dynamic linking, shipping the exact matching source, the configure line, and attribution [V] (same page).
- Running a separate ffmpeg.exe as a sidecar is usually treated as "aggregation", not linking. I could not fetch the GNU FAQ to confirm this (HTTP 429), so treat it as **[U]** https://www.gnu.org/licenses/gpl-faq.html. Either way, publish the source and configure line.
- Latest FFmpeg is 9.0.2, released 2026-09-18 [V] https://ffmpeg.org/download.html
- BtbN's `win64-lgpl-9.0.zip` is 163 MiB and `lgpl-shared` is 73 MiB. These zips contain ffmpeg, ffprobe and ffplay [V] https://github.com/BtbN/FFmpeg-Builds
- The lgpl variant lacks libx264, libx265 and fdk-aac [V] (same page).
- A custom audio-only build would be far smaller [U].

**Windows code signing**
- Since 2024, EV certificates no longer give instant SmartScreen reputation; OV and EV build it the same way [V] https://v2.tauri.app/distribute/sign/windows/
- `signCommand` supports any signing tool [V] (same page).
- **SignPath Foundation is free for OSS** [V] https://signpath.org/. Conditions [V] https://signpath.org/terms:
  - OSI license and no proprietary code
  - project already released
  - builds come from CI
  - MFA for the whole team
  - a published "Code signing policy" page and privacy policy
  - manual approval of each release
- **Azure Artifact Signing** (formerly Trusted Signing):
  - Individual developers must be in the US or Canada. Organizations can be in the US, CA, EU, UK, AU, NZ, JP, KR, SG, CH, NO or IL [V] https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart
  - This likely rules it out for a maintainer in India [U].
  - Basic tier includes 5,000 signatures a month. Microsoft's page does not show the price [V] https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
  - $9.99/month comes from a forum post [U].

**Alternatives** (only to confirm or challenge the choice)

| Framework | Status | Fit |
|---|---|---|
| Electron | 44.7.0, 2026-10-07 [V] https://github.com/electron/electron/releases | Bundles Chromium. One Tauri vs Electron comparison found about 12–18 MB vs about 200–290 MB installers [U, secondary] https://www.digitalapplied.com/blog/desktop-apps-web-stack-tauri-electron-deno-wails-2026 |
| Wails | v2.14.0 stable (2026-08-10); v3 still at beta.28 (2026-10-05) [V] https://github.com/wailsapp/wails/releases | Similar footprint to Tauri, but Go has weaker audio and tag libraries than the Rust crates below [U] |
| Avalonia | 12.1.4, 2026-10-09 [V] https://github.com/AvaloniaUI/Avalonia/releases | Viable, but splits the ecosystem away from the Rust audio crates [U] |
| Flutter, PySide | not researched in depth [U] | — |

### 2. Rust crates

**Tags: `lofty` 0.25.4 (2026-09-20, MIT/Apache)**
- Writes FLAC Vorbis comments, MP4 ilst, Opus/Ogg Vorbis comments, and ID3v2 on MP3/WAV. ID3v2 inside FLAC is read-only [V] https://github.com/Serial-ATA/lofty-rs
- Multi-value fields work: `push` appends, `get_all` returns every value [V] https://docs.rs/lofty/latest/lofty/ogg/tag/struct.VorbisComments.html
- **Converting to the generic `Tag` drops every key that has no `ItemKey` mapping** [V] (same page). Use `VorbisComments` and `FlacFile` directly.
- `FlacProperties::signature()` returns the STREAMINFO audio MD5 [V] https://docs.rs/lofty/latest/lofty/flac/struct.FlacProperties.html
- **Padding:** lofty only keeps existing padding and adds some if missing. Reusing padding to avoid rewriting the file is still an open item [V] https://github.com/Serial-ATA/lofty-rs/issues/445
- **Corruption history:**
  - FLAC corruption bugs were fixed in #549 (2025) and #640 (2026-04) [V] https://github.com/Serial-ATA/lofty-rs/issues/640
  - As of 0.25.0, FLAC writes buffered the whole file in memory. The maintainer says lighter writes had caused those bugs [V] https://github.com/Serial-ATA/lofty-rs/issues/694
  - Open issue #731: `splice` truncates to the wrong length in 0.25.4. The maintainer says no current code path reaches it [V] https://github.com/Serial-ATA/lofty-rs/issues/731
  - **Mitigation:** write tags to a temp copy, check the audio MD5 or decode it, then rename over the original.

**Tag alternatives**
- `metaflac` 0.2.8 (2025-01-25) [V]
- `id3` 1.17.2 (2026-09-22) [V]
- `mp4ameta` 0.13.0 (2025-05-29) [V]
- `taglib` crate 1.0.0 dates from 2018 and is effectively dead [V] https://crates.io/crates/taglib

**Decoding: `symphonia` 0.6.1 (2026-08-13, MPL-2.0)** [V] https://github.com/pdeljanov/Symphonia
- FLAC: Excellent
- MP3: Excellent (non-default feature)
- ALAC: Great (non-default feature)
- AAC-LC: Great (non-default feature)
- **Opus: no native decoder. HE-AAC: not supported.**
- For Opus, use `symphonia-adapter-libopus` 0.3.0 (wraps the C libopus) [V] https://crates.io/crates/symphonia-adapter-libopus, or decode through ffmpeg.

**FFT**
- `rustfft` 6.4.1 (2025-09-18) and `realfft` 3.5.0 (2025-06-12) [V]. Both are mature and heavily used.

**Loudness**
- `ebur128` 0.1.10 (2024-10-26); repo last pushed 2025-12-13 [V] https://github.com/sdroege/ebur128
- It is a Rust port of libebur128 with true peak and loudness range, and claims "comparable performance" to the C library [V] (same page).
- Album gain by merging histograms is not documented [U].
- Alternative: `rsgain` v3.8 (2026-09-02, BSD-2). It does ReplayGain 2.0, has an Easy mode, uses `-m` threads per directory, and is in Scoop extras [V] https://github.com/complexlogic/rsgain. It is also a good reference oracle for testing.

**Hashing**
- `blake3` 1.8.7 (2026-08-20) [V]
- `update_mmap_rayon` is slower than plain `update` below about 128 KiB, and parallel reads can thrash spinning disks [V] https://docs.rs/blake3/latest/blake3/struct.Hasher.html
- `xxhash-rust` 0.8.19 (2026-09-28) [V]

**Images**

| Crate | Version | Notes |
|---|---|---|
| `image` | 0.25.10 (2026-03-10) | Uses `zune-jpeg` to decode [V]. Its `JpegEncoder` has quality 1–100 but fixed 4:2:2 subsampling [V] https://docs.rs/image/latest/image/codecs/jpeg/struct.JpegEncoder.html |
| `jpeg-encoder` | 0.7.1 (2026-07-27) | Explicit "Baseline and progressive", chroma subsampling, optimized Huffman tables, optional AVX2 [V] https://github.com/vstroebel/jpeg-encoder |
| `fast_image_resize` | 6.1.0 (2026-07-21) | [V] |
| `zune-jpeg` | 0.5.15 | Decoder only [V] https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |
| `mozjpeg` | 0.10.13 (2025-02-18) | Errors rely on unwinding, so with `panic=abort` any JPEG error aborts the process. This conflicts with Tauri's size profile [V] https://github.com/ImageOptim/mozjpeg-rust, https://v2.tauri.app/concept/size/ |

**SQLite**
- `rusqlite` 0.40.2 (2026-08-08); `bundled` ships SQLite 3.53.2 [V] https://github.com/rusqlite/rusqlite
- `sqlx` 0.9.0 (2026-05-21) [V]
- For a single-user local index, rusqlite on a dedicated thread is simpler [U].

**File watching: `notify` 8.2.0 stable, 9.0.0-rc.5 (2026-08-30)** [V]
- Docs warn that "notify may fail to receive all events" on very large trees [V] https://docs.rs/notify/latest/notify/
- Open Windows issues include missed events at scale (#412) and deletes reported as updates (#389) [V] https://github.com/notify-rs/notify/issues/412
- If the ReadDirectoryChangesW buffer overflows, all buffered events are discarded and the app must re-enumerate [V] https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw
- So treat watcher events as hints and run periodic reconcile scans by size and mtime.

**Removable drives**
- `sysinfo` 0.39.6 has `Disk::is_removable` but no serial and no events, only polling [V] https://docs.rs/sysinfo/latest/sysinfo/struct.Disk.html
- On Windows, use `windows` 0.62.2 (2025-10-06) [V] with:
  - `WM_DEVICECHANGE` / `DBT_DEVICEARRIVAL` [V] https://learn.microsoft.com/en-us/windows/win32/devio/wm-devicechange
  - `DEV_BROADCAST_VOLUME` (unit mask, `DBTF_MEDIA`). Media messages arrive only for devices with soft-eject [V] https://learn.microsoft.com/en-us/windows/win32/api/dbt/ns-dbt-dev_broadcast_volume
- The `GetVolumeInformationW` serial is assigned when the volume is formatted, so it changes on reformat [V] https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationw. Use the marker file as the primary ID and the serial as a secondary check.
- **Eject on Windows:** `CM_Request_Device_EjectW` returns a veto type and name. A non-NULL veto buffer suppresses the system UI [V] https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_request_device_ejectw. The app must close its own handles when it receives `DBT_DEVICEQUERYREMOVE`.
- Ejecting a built-in laptop SD reader may eject the whole reader [U].
- **Eject on Linux:** udisks2 `Drive.PowerOff()` flushes caches and powers the drive off when `CanPowerOff` is true [V] https://storaged.org/doc/udisks2-api/latest/gdbus-org.freedesktop.UDisks2.Drive.html
- **Eject on macOS:** DiskArbitration `DADiskUnmount` / `DADiskEject` [V] https://developer.apple.com/library/archive/documentation/DriversKernelHardware/Conceptual/DiskArbitrationProgGuide/ManipulatingDisks/ManipulatingDisks.html

**Concurrency**
- `tokio` 1.53.2 and `rayon` 1.12.0 [V]
- Tokio's docs say to limit CPU-bound `spawn_blocking` work with a semaphore, and that rayon "may also be a good fit" [V] https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
- Pattern: tokio for I/O and orchestration, rayon for decode, FFT and hashing [U].

**Fingerprinting**
- `rusty-chromaprint` 0.3.0 (2024-10-26, MIT) calls itself "in-progress", and AcoustID compatibility is not stated [V] https://github.com/darksv/rusty-chromaprint
- The C chromaprint v1.6.1 was released 2026-07-28 [V] https://github.com/acoustid/chromaprint
- Use an `fpcalc` sidecar if fingerprinting is needed; its licensing is [U].

### 3. SD card write performance
- Speed Class ratings are defined for sequential writes into unfragmented free allocation units, and fragmented areas are slower [V, via SDA page] https://www.sdcard.org/developers/overview/speed_class/
- I found no benchmark comparing parallel and sequential multi-file writes on exFAT [V: absence]. A patent describes cards leaving sequential mode when addresses stop being sequential [U].
- Recommendation: one writer per card, large buffers of about 1–4 MiB, and parallelism only across different cards [U].
- Windows defaults to **Quick removal** since 1809 (no write cache). **Better performance** caches writes but requires safe removal [V] https://learn.microsoft.com/windows/client-management/change-default-removal-policy-external-storage-media
- Calling `FlushFileBuffers` after every write is inefficient. Flushing a whole volume needs admin rights [V] https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers
- Plan: one `sync_all` per file before the rename, then measure on real cards.

### 4. Distribution

**Comparable Tauri apps**
- Musicat 0.17.2 (Tauri music player) [V] https://github.com/basharovV/musicat/releases:
  - x64 setup.exe 20 MiB
  - msi 22 MiB
  - dmg 22–23 MiB
  - AppImage 97 MiB
- Donut 0.33.0: setup.exe 25 MiB, AppImage 109 MiB [V] https://github.com/zhom/donutbrowser/releases
- Expected size for our app: about 15–25 MB plus ffmpeg [U].

**Package managers**
- **winget:** submit by PR to microsoft/winget-pkgs (Komac or wingetcreate). Installers must be MSIX, MSI, APPX or .exe [V] https://learn.microsoft.com/en-us/windows/package-manager/package/repository
- **Scoop:** extras already carries rsgain [V] https://github.com/complexlogic/rsgain
- **Homebrew:** "We will disable all Homebrew/homebrew-cask casks that fail Gatekeeper checks in September 2026" [V] https://brew.sh/2025/11/12/homebrew-5.0.0/. That means paid Apple Developer ID signing and notarization, or a self-hosted tap [U].

### Recommended stack

| Component | Choice | Why | Risk |
|---|---|---|---|
| Shell | Tauri 2.12 + React/TS; Cargo workspace `engine` (lib) + `cli` (bin) + `app` | Small installer; headless CLI needs no WebView | WebView differs across OSes |
| Tags | lofty 0.25 via `VorbisComments`/`FlacFile` APIs | Active; reads MD5 | In-place rewrites → temp copy + MD5 verify |
| Decode | symphonia 0.6 (FLAC/ALAC/AAC/MP3) + ffmpeg for Opus/HE-AAC | Pure Rust for the main formats | No native Opus |
| FFT | realfft / rustfft | Mature | — |
| Loudness | `ebur128` 0.1.10, check against rsgain | Pure Rust, true peak | Slow release cadence |
| Hash | blake3 (content), xxh3 (quick change check) | Fast | — |
| Art | image decode → fast_image_resize → jpeg-encoder baseline 4:2:0 | Pure Rust, explicit baseline | Test on cheap DAPs |
| Transcode | ffmpeg 9.0 LGPL sidecar, ideally a custom audio-only build | Covers all formats | LGPL source obligations; size |
| DB | rusqlite 0.40 bundled, WAL mode | Simple, embedded | — |
| Watch | notify 8.2 + reconcile scan | Cross-platform | Dropped events |
| Volumes | windows-rs WM_DEVICECHANGE + marker file; udisks2; DiskArbitration | Native APIs | Three OS-specific code paths |
| Signing | SignPath Foundation | Free for OSS | Policy overhead, approval not guaranteed |

### Risks and unknowns
1. Tag writes with lofty are not atomic: wrap every write in copy, verify, rename. #731 is still open.
2. Opus decoding needs C libopus or ffmpeg.
3. Whether running ffmpeg as a sidecar counts as aggregation under the LGPL [U]. Get a legal check and publish the configure line and source.
4. Azure Artifact Signing is unavailable to individuals outside the US and Canada. SignPath approval is discretionary.
5. macOS distribution through Homebrew needs a paid Apple Developer ID.
6. No hard data on SD write strategy. Benchmark one writer vs N writers, buffer sizes, and flush cost on real cards.
7. Ejecting a built-in SD card reader may remove the reader itself.
8. `ebur128` album-gain support and `rusty-chromaprint` compatibility with AcoustID are unverified.
9. The `windows` crate's last release was 2025-10-06. It is stable, but check that the APIs we need are present.
