# Android spike: device and build checks

Results of the readiness checks for the Android app (decision record
[0008](decisions/0008-android-app.md); the check table is in the "Venuta Android stack and
readiness" report, section "Spike checks to run on the RG Rotate").

- **Run:** 2026-10-21, branch `spike/android`, on the owner's Anbernic RG Rotate (adb serial
  `ROTATE11197433`) and the Windows build PC.
- **Toolchain:** rustc 1.99.0, cargo-ndk 4.1.2, NDK 29.0.14206865 (r29), the SDK's CMake 3.31.6,
  build-tools 35.0.0, AGP 8.7.3, Kotlin 2.1.0, Gradle 8.9, JDK 17, UniFFI 0.32.1, JNA 5.17.0.
- **Scope:** checks 1 to 9. Checks 10 to 22 are not run yet.
- **What is in the branch:** `spike/venuta-spike-ffi` (a throwaway UniFFI facade over engine,
  player, media-access with `live-provider`, ytmusic, together and iroh),
  `spike/uniffi-bindgen` (the host bindgen binary), `spike/android` (a minimal Kotlin app,
  package `io.github.venuta_lab.spike`, no Compose), `spike/env.sh` (the build environment) and
  `spike/build.sh` (Rust core, bindings, APK). The media-access manifest gains one Android-only
  line (check 6).
- **Device rules kept:** reads only (getprop, dumpsys, `wm size`/`wm density` without arguments,
  short getevent captures). One test app was installed, run twice for about 4 seconds each and
  uninstalled; Ghostbox was not touched and was the resumed app before and after. The screen had
  been turned off with the power button at about 18:19, before the test app ran (18:22), and was
  left that way.

## Summary

| # | Check | Result |
|---|---|---|
| 1 | Device facts | Recorded. 240 dpi: a 480 x 480 dp panel, 480 x 432 dp for the app; not low-RAM; WebView 155 |
| 2 | GPU driver | Recorded. Mali-G52 r34p0, OpenGL ES 3.2, Vulkan 1.1.191 |
| 3 | Controller | Partly recorded: layout, sources and key map read; **needs the owner to press buttons** |
| 4 | Audio HAL | Recorded. 48 kHz 16-bit mixer only, no FastMixer, no MMAP, offload MP3 only and not to the speaker; Bluetooth off, so A2DP latency **needs a headset** |
| 5 | Core builds | **Pass** after check 6's fix. engine, ytmusic, together build as is; player stops on rquickjs-sys |
| 6 | Live provider and rquickjs | **Pass with bindgen.** Gate-out is not a cfg flip; decision below |
| 7 | 16 KB alignment | **Pass.** Every LOAD segment `align 2**14`, `zipalign -c -P 16` OK |
| 8 | Load and one call | **Pass.** No `UnsatisfiedLinkError`, UniFFI checksums pass, engine writes under `filesDir` |
| 9 | Hidden runtime traps | **Both confirmed.** keyring 3 keeps nothing even in the same process; iroh without the JNI context panics (caught) and falls back, with it no panic |

Sizes of the arm64 core (`libvenuta_spike.so`, everything the facade reaches; see check 5):
29.7 MB unstripped, **21.6 MB stripped** (as packaged), 9.8 MB gzipped. x86_64: 30.1 MB
unstripped, 24.3 MB stripped. The arm64-only APK is **22.9 MB** (24.2 MB with the stray iroh libraries of check 5), because AGP stores native
libraries uncompressed and page aligned.

## Checklist

### 1. Device facts: recorded

Commands: `getprop` (`ro.config.low_ram`, `ro.build.version.*`, `ro.product.*`,
`ro.sf.lcd_density`, `ro.product.cpu.abilist`), `wm size`, `wm density`, `getconf PAGESIZE`,
`/proc/meminfo`, `dumpsys webviewupdate`, `dumpsys display`, `pm list packages`; in the test app
`ActivityManager`, `resources.displayMetrics`, `configuration`, `WebView.getCurrentWebViewPackage()`.

| Fact | Value |
|---|---|
| Model, firmware | Anbernic RG Rotate, `ums512_1h10` (Unisoc T618), build `V1.44`, fingerprint `UNISOC/ums512_1h10_Natv/...:12/SP1A.210812.016/24010:user/release-keys` |
| Android | 12 (API 31), first API level 31, security patch **2022-07-05**, kernel 5.4.161 aarch64 |
| ABIs | `arm64-v8a, armeabi-v7a, armeabi` |
| Page size | 4096 (16 KB alignment is still required by the build target, not by this device) |
| Panel | 720 x 720 px, 60 Hz only, one display mode; `wm density` 240 (hdpi, density 1.5) |
| Square size | **480 x 480 dp** physical; the app window is 720 x 696 px, `screenWidthDp` x `screenHeightDp` **480 x 432**, `smallestScreenWidthDp` 480 |
| Hinge or modes | None reported: one display, one mode, no hinge or posture property. The "open hinge" size in the plan does not exist on this ROM |
| Low RAM | `ro.config.low_ram` unset, `isLowRamDevice` **false**; `memoryClass` 192 MB, `largeMemoryClass` 512 MB |
| Memory | 2,533 MB total to Android, about 1.2 GB available with Ghostbox resident; low-memory threshold 216 MB; `ro.lmk.use_minfree_levels=true` |
| WebView | `com.google.android.webview` **155.0.8059.43** (targetSdk 37), multiprocess on; the stock `com.android.webview` 90.0.4430.91 is still installed but not used |
| Google Play | Present: `com.android.vending`, `com.google.android.gms`, `com.google.android.gsf` (plus `app.revanced.android.gms`) |
| Locale | `persist.sys.locale=en-US` |

What it means:

- Window size classes: width 480 dp is compact (under 600), height 432 dp is compact (under 480).
  The console layout of decision 7 is right, but it is designed at **480 x 432 dp**, not the
  360 dp the report thought likely. A 48 to 56 dp strip is 11 to 13% of the height.
- Not low-RAM, so `isLowRamDevice` cannot pick the effect tier for this class. The tier picker
  of decision 6 needs API level plus a measured frame budget (or the GPU name) instead.
- WebView 155 is current, so the androidx.webkit profile APIs for the BotGuard WebView (check 18)
  are likely available; the exact minimum is still check 18's job.

### 2. GPU driver: recorded

Commands: `dumpsys SurfaceFlinger | grep -iE "GLES|EGL"`, `getprop ro.hardware.vulkan`,
`getprop ro.hardware.egl`, `getprop ro.opengles.version`, `cmd gpu vkjson`, `pm list features`,
`dumpsys gpu`, `getprop debug.hwui.renderer`.

| Fact | Value |
|---|---|
| GLES | `ARM, Mali-G52, OpenGL ES 3.2 v1.r34p0-01eac0`, `ro.opengles.version=196610` (3.2), AEP present |
| EGL | 1.4 Android META-EGL, `ro.hardware.egl=mali` |
| Vulkan | driver `vulkan.ums512.so` (`ro.hardware.vulkan` unset), **apiVersion 1.1.191**, conformance 1.2.1.6, `vulkan.level=1`, compute feature present |
| Updatable driver | `Stable Game Driver: unsupported` |
| HWUI renderer | `debug.hwui.renderer` unset: Android 12's default, Skia on OpenGL |

What it means: Compose draws through HWUI on GLES here. RenderEffect blur (API 31) runs on this
driver; its cost is check 12. Nothing here needs Vulkan, and nothing reopens decision 6.

### 3. Controller: partly recorded, needs the owner to press buttons

Commands: `getevent -i`, `getevent -lp /dev/input/event3`, `dumpsys input`,
`cat /system/usr/keylayout/Vendor_484b_Product_1101.kl`, a 20 second
`timeout 20 getevent -lt /dev/input/event3` and a 5 second capture of `event2` (gpio-keys); in
the test app `InputDevice` sources, motion ranges and `hasKeys()`, plus `onKeyDown` and
`onGenericMotionEvent` logging.

Raw result:

- The built-in pad is **`retrogame_joypad`** (`/dev/input/event3`, USB bus id, vendor `0x484b`,
  product `0x1101`, reported as **external**, controller number 1). Android sources
  `0x01000511` = **`SOURCE_GAMEPAD | SOURCE_JOYSTICK | SOURCE_KEYBOARD`**, not `SOURCE_DPAD`.
- Kernel keys: `BTN_SOUTH/EAST/C/NORTH/WEST/Z`, `BTN_TL/TR/TL2/TR2`, `BTN_SELECT/START/MODE`,
  `BTN_THUMBL/THUMBR`, `KEY_BACK`, `KEY_F9`, `KEY_F10`, and `BTN_DPAD_UP/DOWN/LEFT/RIGHT`.
  Axes: two sticks (`ABS_X/Y`, `ABS_RX/RY`, range +-16384), two analogue triggers
  (`ABS_Z`, `ABS_RZ`, 0 to 16384) and **`ABS_HAT0X/HAT0Y`** (-1 to 1).
- Key layout `Vendor_484b_Product_1101.kl` (an edited Xbox 360 layout): 304 `BUTTON_A`,
  305 `BUTTON_B`, 306 `BUTTON_C`, 307 `BUTTON_X`, 308 `BUTTON_Y`, 309 `BUTTON_Z`, 310 `L1`,
  311 `R1`, 312 `L2`, 313 `R2`, 314 `SELECT`, 315 `START`, 316 `MODE`, 317/318 `THUMBL/THUMBR`,
  158 `BACK`, 67 `F9`, 68 `F10`; axes `X Y Z RZ`, `LTRIGGER RTRIGGER`, `HAT_X HAT_Y`.
  **There is no line for the `BTN_DPAD_*` codes (544 to 547).**
- `hasKeys()` on the pad: A, B, X, Y, L1, R1, L2, R2, START, SELECT, MODE, BACK, THUMBL, THUMBR,
  C, Z yes; **DPAD_UP/DOWN/LEFT/RIGHT no**. Motion ranges include `HAT_X`, `HAT_Y`, `LTRIGGER`,
  `RTRIGGER`, `GAS`, `BRAKE`.
- Other devices: `gpio-keys` (volume, power, `KEY_F9`, `KEY_F12`), `goodix-ts` (touch screen),
  and **`virtual_gamepad`** (vendor `0xabcd`, a touch-screen-class device): Anbernic's
  key-to-touch mapping layer, the likely "firmware remap" path for games.
- No events arrived in either capture: nobody pressed buttons (the screen was off).

What it means for the plan:

- The d-pad almost certainly arrives as **`AXIS_HAT_X/Y` motion**, not DPAD key events: no key map
  line, `hasKeys` says no DPAD keys, and the source is not `SOURCE_DPAD`. Android's view root
  turns hat motion into synthetic DPAD key events for focus, so Compose focus traversal should
  still work, but the console layout's key handling must accept both, and repeat throttling
  (check 17) has to be tested on the synthetic events. To confirm with presses.
- B is `BUTTON_B` (305), so routing B to Back is the app's job, as decision 7 says. Which
  physical face button sends 304 versus 305 (Nintendo or Xbox label positions) is not known
  until someone presses them.
- L2 and R2 come both as keys and as analogue axes; seek on L2/R2 can use the keys.
- **Still needed (owner):** with the screen on, press each of: d-pad four ways, A, B, X, Y, L1,
  R1, L2, R2, Start, Select, Home/function keys, both stick clicks, while
  `adb shell getevent -lt /dev/input/event3` runs (and once with the test app's key logging).
  Also say whether the firmware has a mouse or touch-mapping mode switch.

### 4. Audio HAL: recorded; A2DP needs a headset

Commands: `dumpsys media.audio_flinger`, `dumpsys media.audio_policy`, `getprop aaudio.*`
and the offload properties, `dumpsys bluetooth_manager`; in the test app
`AudioManager.getProperty(PROPERTY_OUTPUT_SAMPLE_RATE / FRAMES_PER_BUFFER)`,
`getDevices(GET_DEVICES_OUTPUTS)`, `isOffloadedPlaybackSupported()` and the audio features.
OboeTester was not installed (one test app only).

| Fact | Value |
|---|---|
| Primary output | `AudioOut_D`, MIXER, **48,000 Hz, PCM 16-bit, stereo only** (the HAL profile offers no other rate or format), HAL frame count 960 (20 ms), speaker |
| Fast path | A `fast` output (`AUDIO_OUTPUT_FLAG_FAST`) exists, but every thread says **"No FastMixer"**; `FEATURE_AUDIO_LOW_LATENCY` false, `FEATURE_AUDIO_PRO` false |
| MMAP | **None**: no MMAP output in the policy, `aaudio.mmap_policy` and `aaudio.mmap_exclusive_policy` unset |
| Offload | One `compressed_offload` output: **MP3 only** (8 to 48 kHz), to earpiece, wired and USB, **not to the speaker**. From the app on the speaker route: MP3 true, AAC false, Opus false, PCM false |
| From the app | `PROPERTY_OUTPUT_SAMPLE_RATE` 48000, `PROPERTY_OUTPUT_FRAMES_PER_BUFFER` 960; outputs: earpiece and speaker at 48000, plus a telephony output |
| Bluetooth | `bluetooth` HW module with a2dp output (legacy, no a2dp offload properties); **Bluetooth is off** (`enabled: false`) |

What it means:

- **Decision 3 stands and gets stronger.** Media3's main gain over the Rust route is offload for
  screen-off battery, and here offload is MP3 only and never on the speaker; there is no MMAP
  either. Venuta's own decoder into a power-saving AAudio stream with a large buffer is the
  only real path on this device class.
- Every 44.1 kHz song is resampled to 48 kHz by AudioFlinger. "Bit-perfect" must not be claimed
  on this device. The engine can resample with its own `rubato` before AAudio (better quality
  than the HAL's resampler, same CPU class) and should ask for 48 kHz only when the device says
  so, never force it otherwise.
- Ask AAudio for `PERFORMANCE_MODE_POWER_SAVING` (or none): a low-latency request gains nothing
  without a FastMixer or MMAP.
- **Still needed:** A2DP latency with a Bluetooth headset (the owner turns Bluetooth on and pairs
  one; settings were not changed here). This joins check 16.

### 5. Core builds: pass (after check 6)

Commands (from `spike/env.sh`): `cargo ndk -t arm64-v8a --platform 26 build --release -p engine
-p ytmusic -p together`, then `-p player`, then `-p venuta-spike-ffi` for arm64-v8a and x86_64.

Raw result:

- `engine`, `ytmusic`, `together`: **built first time** in 2 min 34 s, including
  `libsqlite3-sys` (bundled), `ring`, `blake3`, `keyring` 3 (compiles, see check 9), `lofty`,
  `symphonia`, `image`, reqwest with rustls.
- `player`: **stopped at `rquickjs-sys` 0.9.0**, exactly as predicted: "couldn't find file
  `bindings/aarch64-linux-android.rs`", "try the `bindgen` feature instead". `player` always
  enables `media-access/live-provider`, so the player alone pulls innertube-rs.
- `opusic-sys` (bundled libopus through CMake) **built** with the SDK's CMake 3.31.6 and
  `ANDROID_NDK_HOME`; no extra setting was needed.
- `venuta-spike-ffi` (engine + player + media-access live + ytmusic + together + iroh 1.3 +
  tokio, UniFFI 0.32.1): built for **arm64-v8a** (4 min clean) and **x86_64** (12 min) once
  check 6's fix was in.

Sizes (`llvm-strip`, `llvm-size`, `llvm-nm -S`):

| Library | Unstripped | Stripped |
|---|---|---|
| arm64-v8a `libvenuta_spike.so` | 29.7 MB | **21.6 MB** (9.8 MB gzipped) |
| x86_64 `libvenuta_spike.so` | 30.1 MB | 24.3 MB |

Largest parts of the arm64 library by symbol size (generic code is counted under `core`,
`alloc`, `std`): sqlite 0.66 MB, QuickJS C 0.62 MB, innertube-rs 0.81 MB, iroh 0.56 MB plus noq
0.51 MB and netlink 0.44 MB, h2 0.54 MB, tokio 0.50 MB, rustls 0.49 MB, media-access 0.48 MB,
ytmusic 0.24 MB, reqwest and hyper 0.7 MB. `.text` is 14.8 MB, unwind tables
(`.eh_frame` + `.gcc_except_table`) 2.7 MB.

What it means:

- The sizes cover only what the facade reaches: scanning, the player, the live catalog and an
  iroh endpoint, reached through a never-taken branch (`link_everything(false)`). The real
  `venuta-ffi` will reach more of `engine`, so expect the core to land around **22 to 26 MB per
  ABI**. Against Ghostbox's 118 MB memory budget this is code mapped from the APK, not heap, but
  it is the bulk of the download.
- AGP stores `.so` files uncompressed (`useLegacyPackaging=false` is the default from minSdk
  23), so the APK is about the size of the stripped library. If download size matters more
  than install-time extraction, `packaging.jniLibs.useLegacyPackaging = true` halves it (about
  10 MB compressed). To decide with the release work; not a blocker.
- `cargo ndk -o <jniLibs>` also copies **`libiroh-*.so` and `libiroh_relay-*.so`** (1.4 + 1.0 MB
  unstripped), because iroh and iroh-relay declare `crate-type = ["lib", "cdylib"]`. Nothing
  loads them. The Gradle task must copy only `libvenuta_ffi.so` (fixed in `spike/build.sh`).

### 6. Live provider and the rquickjs decision: pass with bindgen

Commands: as check 5 with `-p player` (which enables `live-provider`), after adding to
`crates/media-access/Cargo.toml`:

```toml
[target.'cfg(target_os = "android")'.dependencies]
rquickjs-sys = { version = "=0.9.0", features = ["bindgen"] }
```

Raw result:

1. With only `LIBCLANG_PATH` set to the NDK's `bin` (it ships `libclang.dll`, so no separate
   LLVM install is needed): bindgen fails with `'stdbool.h' file not found`. The NDK's
   `libclang.dll` looks for its resource headers relative to the working directory
   (`ignoring nonexistent directory "lib/clang/21/include"`).
2. Setting `BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android` does nothing: **cargo-ndk 4.1.2
   overwrites that variable** with its own `--sysroot ... -I...`.
3. Adding `C_INCLUDE_PATH=<ndk>/toolchains/llvm/prebuilt/windows-x86_64/lib/clang/21/include`
   fixes it: `player` and the facade build for both ABIs in 4.5 min.

Decision taken: **bindgen**, for now. The gate-out is not a cfg flip: `live_provider.rs`
(3,664 lines) uses innertube-rs in 25 places, and not only for the legacy resolver. Its search
path (`parse_music_search_response`, `MusicSearchResults`, `MusicTrackItem`,
`MusicSearchFilter`) is innertube-rs's parser, plus `Innertube::new()` and
`resolve_stream_url_full` with the decipherer. Gating it out means moving the search parse into
`ytmusic` first, which belongs to the app-core and Android dependency-fixes work. Bindgen costs
two environment variables (`spike/env.sh`) and about 1.4 MB of the library (QuickJS C 0.62 MB,
innertube-rs 0.81 MB, rquickjs 0.02 MB). On a Linux CI runner the `C_INCLUDE_PATH` workaround is
probably not needed (it is a Windows libclang issue); to confirm when the CI job is written.

What it means for decision 10: the outcome stands (the JS resolver and the 4416 PO-token source
are useless on a phone and should leave the Android build), but it moves from "first build" to
"after the search parse moves into ytmusic". Until then Android builds with bindgen, and
nothing blocks the next checks. `stream_resolve.rs` (yt-dlp) also compiles on Android and needs
its `cfg(not(target_os = "android"))` gate, as planned.

### 7. 16 KB alignment: pass

Commands: `llvm-objdump -p libvenuta_spike.so | grep -A1 LOAD` on the cargo output and on every
`.so` taken out of the APK; `zipalign -c -P 16 -v 4 app-release.apk` (build-tools 35.0.0).

Raw result: the three LOAD segments of `libvenuta_spike.so` are `align 2**14`. Inside the APK,
`libvenuta_spike.so`, JNA's `libjnidispatch.so` (5.17.0) and the two stray iroh libraries are
all `align 2**14`. `zipalign -c -P 16`: every `lib/arm64-v8a/*.so` OK, "Verification succesful".

What it means: NDK r29 aligns by default and cargo-ndk adds nothing for r28 and later; JNA 5.17
is already compliant. No flags are needed. The CI job keeps `zipalign -c -P 16` as a guard.

### 8. Load and one call: pass

Commands: `spike/build.sh` (cargo-ndk, `uniffi-bindgen generate --library ... --language
kotlin`, `gradlew assembleRelease`), `adb install`, `adb shell am start -W -n
io.github.venuta_lab.spike/.MainActivity`, `adb logcat -s VENUTA_SPIKE VENUTA_SPIKE_RS`.

Raw result (on the device, release build, screen off):

| Step | Result |
|---|---|
| `System.loadLibrary("venuta_spike")` | OK in 4 ms; `JNI_OnLoad` ran |
| First UniFFI call (JNA load, checksum checks, `core_info`) | **OK, 64 ms**; no `UnsatisfiedLinkError`, the checksum check passed |
| `core_info` before any setup | `arch aarch64 os android panic=unwind`; `HOME`, `TMPDIR`, `XDG_DATA_HOME`, `APPDATA`, `LANG` all **unset**; `temp_dir=/data/local/tmp`; `user_dir=./Venuta` |
| `init_paths(filesDir, cacheDir)` | `user_dir=/data/user/0/io.github.venuta_lab.spike/files/Venuta`; a file written and read back; `engine::Index::open` created its SQLite index (90,112 bytes) |
| `decode_smoke` | the player's decoder read a generated 1 s 44.1 kHz WAV: 88,200 samples in 1 ms |
| `async_echo(50)` (UniFFI async on tokio, called with `runBlocking`) | returned, "slept 89 ms on tokio" |

What it means:

- The UniFFI 0.32.1 recipe works as the plan describes: proc-macros, library-mode bindgen from
  the Android `.so` on Windows, JNA, tokio async. Generated Kotlin needs JNA (`@aar`) and
  kotlinx-coroutines.
- **UniFFI trap:** an error variant with a field named `message` does not compile in Kotlin
  (`'message' hides member of supertype 'Throwable'`). The facade's `uniffi::Error` enum must
  name the field something else (`reason`), or use a flat error.
- **Paths trap, worse than the report said:** before setup, `user_dir()` is `./Venuta`, relative
  to the process's working directory (`/`, not writable), and `std::env::temp_dir()` is
  `/data/local/tmp`, which apps cannot write. So the `paths::init(user, cache, temp)` of the
  plan must run before anything else, and must cover `temp_dir()` users too: `paths.rs`'s
  "before a library" folder lives in `temp_dir()`, which on Android is the cache directory the
  system may clear. On Android it belongs under `filesDir`. The spike set `XDG_DATA_HOME`,
  `HOME` and `TMPDIR` with `set_var` as a stand-in; that is unsafe once other threads run, so the
  real init should be explicit values, not environment variables.
- `LANG` is unset, so `region.rs`'s fallback finds nothing: the locale has to come from Kotlin at
  init, as planned.

### 9. Hidden runtime traps: both confirmed

Commands: the test app calls `vault_put_get` (the engine's own `SystemVault`, keyring 3), is
force-stopped (`am force-stop io.github.venuta_lab.spike`, the test app only) and started again
with `--es phase restart` to call `vault_get`; then `iroh_probe` (an `iroh::dns::DnsResolver`
lookup of `dns.iroh.link` and an `Endpoint::builder(presets::N0).bind()` plus `online()`, as
Listen together does) before and after `iroh::dns::install_android_jni_context` with the
application context, under the workspace's release profile (`panic = unwind`, confirmed).

Raw result:

- keyring 3: `put=Ok(())`, **`get_same_process=Ok(None)`**, and after the restart `Ok(None)`.
  It does not even keep a value within one process: keyring 3's mock store gives every new
  `Entry` its own empty credential, and the engine makes a new `Entry` per call. No error is
  ever returned.
- iroh without the context: two panics, both caught, both
  `ndk-context-0.1.1/src/lib.rs:72:30: android context was not initialized` (the DNS
  system-config read, on two threads). Then DNS answered through the fallback nameservers in
  17 ms, the endpoint bound in 57 ms and was **online in 423 ms** with the n0 relay
  (`use1-1.relay.n0.iroh.link`) and direct IPv4 and IPv6 addresses.
- iroh after `install_android_jni_context(vm, globalRef(applicationContext))`: **no panic**, DNS
  17 ms, bind 37 ms, **online in 331 ms**, same relay.

What it means:

- Decision 9 is confirmed: the Kotlin Keystore `Vault` is required, and until it exists every
  Android sign-in vanishes silently, inside one session too. The same applies to decision 18's
  "device key in the system's secret store": on Android that is the Kotlin vault as well.
  Until the Kotlin vault lands, `SystemVault` on Android should return an error, not succeed,
  so nothing pretends to be saved.
- iroh works on this Wi-Fi either way, but only because the release profile unwinds. The fix is
  confirmed: the facade keeps the `JavaVM` in `JNI_OnLoad` and gets the application context
  from Kotlin (`Application.onCreate`, before any endpoint), since `JNI_OnLoad` has no context to
  give. Keep `panic` unset (unwind) in the release profile regardless, as the report says.

## What reopens a decision in 0008

Nothing is overturned. Three decisions change in detail:

- **Decision 7 (screens):** the handheld is 480 x 432 dp for the app at 240 dpi, not 360 dp. The
  console layout stays; its design size changes.
- **Decision 10 (streams):** leaving innertube-rs out of the Android build needs the search parse
  moved into `ytmusic` first; until then Android builds with rquickjs bindgen (check 6).
- **Decision 6 (effects):** `isLowRamDevice` is false on this low-end device, so it cannot choose
  the tier; use API level plus a measured frame budget.

Decision 3 (Rust engine into AAudio) is made stronger by check 4, and decision 9 (Kotlin vault)
is confirmed by check 9.

## What the next checks need

- **Owner, for checks 3 and 4:** button presses with the screen on (list in check 3); a Bluetooth
  headset paired, with Bluetooth turned on by the owner, for the A2DP latency and check 16.
- **Check 10 (async foreign trait):** builds on `spike/venuta-spike-ffi`; add a `BotGuardVm`-shaped
  foreign trait with a Kotlin `suspend` implementation. No device needed.
- **Checks 11 to 14 (frames, blur, cold start, memory):** a Compose test app with real artwork, R8
  and a Baseline Profile, using the same package and the same install-and-uninstall rule; design
  it at 480 x 432 dp. Cold start should be measured with the 21.6 MB core loaded, since
  `loadLibrary` plus the first call already costs about 70 ms.
- **Check 15 (screen-off hour):** needs the AAudio `Device` in `crates/player` first (power-saving
  mode, 48 kHz, the engine's own resampling), and an hour of device time agreed with the owner,
  since Ghostbox shares the device.
- **Checks 19 and 20 (streams, Listen together):** the facade already reaches the live provider and
  iroh. Check 20 can reuse `iroh_probe` with a ticket from the PC.
- **Build:** use `spike/env.sh` (the `C_INCLUDE_PATH` and `LIBCLANG_PATH` lines) for anything that
  builds `player`; copy only the facade `.so` into `jniLibs`.
