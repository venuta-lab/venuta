# Hybrid stream resolver (maintenance)

Product layers talk only to `MediaAccess`. Full-length online audio is unlocked by a **replaceable stream tool**, not by app UI code.

**Test and release use the same tool.** Dev (`hybrid-dev`), Settings → Install, first online play, and the Windows installer all land on the official standalone yt-dlp binary (or your override).

## Install / update (dev, test, brother machines)

```powershell
# Standalone binary → %LOCALAPPDATA%\Venuta\bin\yt-dlp.exe (no Python)
pwsh -File scripts/install-stream-tool.ps1
pwsh -File scripts/install-stream-tool.ps1 -Check

# Also stage app/src-tauri/resources for a local `tauri build`
pwsh -File scripts/install-stream-tool.ps1 -Bundle

# Or pip module
pwsh -File scripts/install-stream-tool.ps1 -Python
```

Same fetch as the app:

```powershell
node scripts/fetch-stream-tool.mjs
node scripts/fetch-stream-tool.mjs --bundle
```

## What to update when playback breaks

1. Update **yt-dlp** (primary unlock tool):
   - `pwsh -File scripts/install-stream-tool.ps1` (re-download standalone)
   - Settings → Online sources → **Install stream tool**
   - or `python3 -m pip install -U yt-dlp`
   - or replace the binary pointed at by `VENUTA_STREAM_RESOLVER`
2. Optional: update **bgutil** pot helper if yt-dlp asks for PO tokens:
   - keep `http://127.0.0.1:4416` running from `_tools/bgutil-ytdlp-pot-provider`
   - or `pip install -U bgutil-ytdlp-pot-provider`
3. Do **not** change playlist/UI code for CDN 403s.

## Live catalog in the app

**On by default** in the desktop app. Fixture-only opt-out:

```powershell
$env:VENUTA_FAKE_CATALOG = "1"
# optional: $env:VENUTA_STREAM_RESOLVER = "C:\path\to\yt-dlp.exe"
```

`scripts/hybrid-dev.ps1` prefetches the stream tool, then starts tauri dev with an isolated test library.

## Discovery order

1. `VENUTA_STREAM_RESOLVER` — path to a yt-dlp-compatible CLI
2. Beside the Venuta executable / `resources/` (release bundle)
3. `%LOCALAPPDATA%\Venuta\bin\yt-dlp.exe` (managed auto-download)
4. `yt-dlp` on `PATH`
5. `python3` / `python` / `py` with `-m yt_dlp`
6. `%LOCALAPPDATA%\Python\bin\python3.exe -m yt_dlp` (Windows store layout)

If nothing is found, the app downloads the official standalone binary into the managed path (startup thread, first play, or Settings).

## Release packaging

`scripts/release.mjs` runs `fetch-stream-tool.mjs --bundle-only`, temporarily adds `bundle.resources` for the NSIS build, then ships `yt-dlp.exe` next to the app. Installed copies still fall back to managed download if the sidecar is missing or outdated.

## Prove full track still works

```powershell
cargo run -p player --example live_remote_decode --features live-provider -- "blue train"
```

Expect `source_duration_secs` well above 90 (full song), not ~60.

## Split of responsibilities

| Layer | Owner of breakage |
|-------|-------------------|
| Search / browse (InnerTube) | sealed `live_provider` |
| Full stream URL + headers | stream tool (yt-dlp) |
| Ranged HTTP open + decode | `player` |
| Queue / playlists / UI | product (Fake or trait only) |
