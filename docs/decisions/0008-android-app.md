# 0008: Venuta for Android, the stack and the plan

- **Date:** 2026-10-21
- **Status:** accepted
- **Starts:** after the Windows app is finished (Listen together's clean audio merged and live-tested, the 0.3.1 work released)

## Context

Venuta comes to Android in this repo, with every feature the Windows app has. It must run well on old and new phones, on tablets, and on a 720x720 square handheld with a d-pad (Anbernic RG Rotate: Unisoc T618, Mali-G52 MP2, 3 GB RAM, Android 12). That handheld is the low end we design for and the only real device we test on, beside the emulator.

The research behind these decisions:

- the stack and readiness report (stack options measured against low-end devices, every API and native dependency checked for Android, the repo layout, the device checks, the order of work);
- the Android and devices reports before it (Media3, storage, sideloading, the device mesh);
- a native Compose app already running on the same handheld, measured at 13 ms median frames, 2% jank and 118 MB of memory.

## Decisions

| # | Decision | Chosen | Why |
|---|---|---|---|
| 1 | UI | Kotlin + Jetpack Compose (Material 3, adaptive layouts) | The fastest stack on the low end with real numbers behind it; d-pad and gamepad focus built in; background playback needs Kotlin anyway. No Tauri trial: Android's WebView can't keep media playing in the background, its glass blur repaints whole screens on this GPU, and a relaunch bug is still open. |
| 2 | Core | The Rust crates as they are, plus `crates/app-core` (the app logic that has no Tauri in it) and `crates/venuta-ffi` (UniFFI 0.32.1, proc-macros, async on tokio, platform services as foreign traits) | One engine for both apps, so the library, YouTube Music, Listen together and the organiser behave the same. |
| 3 | Sound | Venuta's own Rust engine decodes and shapes the sound into a new AAudio output in `crates/player`; Media3's `MediaLibraryService` with a `SimpleBasePlayer` owns the session, notification, lock screen, Bluetooth and Android Auto | Keeps gapless, crossfade, levelling, EQ, the fades and Listen together's clock identical to the desktop. |
| 4 | Oldest Android | Android 8 (API 26); target API 35 | Covers nearly every phone in use and the AAudio floor. Effects step down by device instead of raising the floor. |
| 5 | Processors | 64-bit ARM (arm64-v8a), plus x86_64 for the emulator | Every phone sold in years, and the handheld, is 64-bit. A 32-bit build can be added later if people ask. |
| 6 | Effects | Three tiers chosen at run time: baked images on low-end GPUs (the desktop's small-canvas backdrop, ported as is), one cached blur on mid phones, live blur on Android 13 and up | Looks like Venuta everywhere, stays smooth on the handheld. |
| 7 | Screens | Adaptive layouts by window size: phone, tablet, foldable, landscape, and a console layout for small square screens (L1/R1 switch tabs, full-screen Now Playing, everything reachable by d-pad, B is Back) | One app, polished on every shape. |
| 8 | Library access | All files access, so the organiser, tag editor, cards and Set aside work as on the PC; if it is refused, the app still plays the music Android's media index sees | Full features for most people, and still useful for those who refuse. Distribution is sideloaded anyway (decision 12). |
| 9 | Saved sign-ins | A Keystore-backed vault written in Kotlin, behind the `Vault` trait the engine already has | `keyring` 3 silently keeps nothing on Android; the trait makes this a small, testable piece. |
| 10 | Streams and YouTube | The VISIONOS stream path first; the old JavaScript stream resolver and yt-dlp are left out of the Android build; YouTube's security check runs in a hidden Android WebView (the same `botguard.js`), with the YouTube web client as the fallback for Count plays | The JavaScript engine has no Android build and Android can't run yt-dlp; both are fallbacks on the desktop already. |
| 11 | Other services | Lyrics, details look-ups, playlist import and the update check go through the Rust core unchanged; Spotify sign-in uses a Custom Tab with the loopback address | All checked to work from Android. |
| 12 | Distribution | GitHub Releases (the same release as Windows), Obtainium and IzzyOnDroid; no Google Play | Play's policies rule out the YouTube features and self-updating. |
| 13 | Package and key | Package `io.github.venuta_lab.venuta`; one release signing key, made before the first public APK and backed up in two places | The package name and key are permanent once registered with Google's developer verification (2027). |
| 14 | Phone as a music server (Subsonic) | Not in the first release | No Android service type fits a LAN server cleanly; the PC keeps doing this. |
| 15 | Theme packs | The colour, type and surface tokens of `theme.json` are shared; a theme's own stylesheet stays desktop only | Most themes come over whole. |
| 16 | Lyrics over other apps | An opt-in overlay, in the extras phase | Matches the desktop lyrics strip for those who want it. |
| 17 | Repo | Keep `app/` (desktop) where it is; add `crates/app-core`, `crates/venuta-ffi`, `tools/uniffi-bindgen`, `android/` (`core` and `app` modules, built with cargo-ndk from Gradle), `design/tokens` and `shared/`. Rename to `apps/desktop` and `apps/android` only after Android ships | The release scripts point at `app/`; the desktop keeps shipping through every step. |
| 18 | Multi-device groundwork | In the first build: songs in history keyed by the song, not the file path; a device key in the system's secret store; settings split into profile and device | Cheap now, expensive once phones hold their own history. |

## How it is built

1. **Device and build checks** (one agent, the handheld on adb): cross-compile every crate, prove the 16 KB alignment, load the core from Kotlin, and measure on the handheld: frames, blur cost, cold start, memory, an hour of screen-off playback, Bluetooth timing, controller keycodes, the security check in a WebView, streams from its network, a Listen together connection, All files access on its ROM. The numbers go in a checklist; any failure reopens the decision it belongs to.
2. **The shared core** (serial, desktop stays releasable after every commit): `app-core` with the event sink and path setup, characterisation tests first around playback and Listen together, then the moves.
3. **In parallel** once the core exists: playback and history, Listen together, YouTube Music and online, the Android-only dependency fixes with an Android build check in CI, the AAudio output, and the design tokens.
4. **In parallel** once the bridge exists: the Compose app shell and layouts, the media service, and the Kotlin platform pieces (vault, security-check WebView, sign-in, file access, notifications).
5. **Feature phases**, several agents at once, until the Android app has every feature the desktop has, then release.

Every step keeps the repo's habits: tests first, the desktop's tests green, and a live check before anything is called done.
