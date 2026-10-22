# 0009 Updates that arrive

## Context

The owner's report: sometimes the app does not show that an update exists, and the manual
"Check for updates" sometimes does not work. Every installed version, from 0.1.0 on, must reach
the newest release in one click.

What every released version asks, read from each tag (`git show <tag>:app/src-tauri/tauri.conf.json`
and `Cargo.lock`):

| Version | Endpoint | Key id | Windows install mode | Updater plugin |
|---|---|---|---|---|
| 0.1.0, 0.1.1, 0.2.0 to 0.2.3, 0.3.0 | `https://github.com/venuta-lab/venuta/releases/latest/download/latest.json` | `E3A9CF88E79B3655` | `passive` (NSIS `currentUser`) | 2.13.3 |

All seven are the same. The plugin looks for the platform `windows-x86_64-nsis` first, then
`windows-x86_64`, and asks for that platform even when there is nothing newer (a manifest
without it is an error, not "up to date"). In 0.1.0 to 0.2.3 the check code is identical; 0.3.0
adds the pre-release channel (GitHub's releases API) and returns "nothing new" when Local only
is on.

GitHub, read on 2026-10-22: the endpoint answers 302 to `v0.3.0/latest.json`, which has version
0.3.0, one platform `windows-x86_64`, the installer's address (200, 54,287,880 bytes) and a
signature by key `E3A9CF88E79B3655` whose trusted comment says `file:Venuta_0.3.0_x64-setup.exe
version:0.3.0`; the signature checks out against the installer's bytes. v0.1.0 is marked
pre-release; v0.1.1 is a tag without a release.

## What went wrong, with the evidence

1. **A found update disappears.** The quiet check ran at most once a day and saved its time even
   when it found something; the next starts that day checked nothing, so the top of the app and
   Settings showed nothing (`checkAtStart`, every version).
2. **The notice is missed.** The only sign of an update was a toast eight seconds after start.
   Started with Windows, Venuta starts in the tray (`--in-tray`), the window is hidden, and the
   toast closes itself after a few seconds; any other notice replaces it.
3. **Running for days never checks again.** The check ran only at start.
4. **A failed quiet check is thrown away**, with no second try (offline at start, Wi-Fi still
   joining).
5. **Manual check under Local only says "Venuta is up to date"** (0.3.0): the command returned
   "nothing new" without asking.
6. **The download can hang for ever.** The plugin downloads with no timeout (`Update.timeout` is
   `None`); the button stays disabled on "Downloading" until Venuta is restarted.
7. **Errors in other words.** "Could not fetch a valid release JSON from the remote" and the
   plugin's other messages reached the user as they are; any message with "status code" or "not
   found" became "No published version was found on GitHub yet."
8. **The endpoint has no fallback and no retry.** One request, twenty seconds, once.
9. **Pre-release flag on a stable release.** v0.1.0 is marked pre-release, and GitHub's
   `releases/latest` never names one: until v0.2.0 was published every check failed.
10. **After an update Venuta seemed to vanish.** The plugin restarts the app with the arguments
    it was started with; started with Windows that is `--in-tray`, so the new version opened
    hidden in the tray.
11. **The update goes where the registry says, not where Venuta runs.** The installer takes its
    folder from `HKCU\Software\Venuta\Venuta`. A copy run from another folder is not the one
    updated.
12. **Each update leaves its 50 MB installer** in `%TEMP%\Venuta-<version>-updater-*`.

## Decision

- **When it checks.** A quiet check 10 seconds after start, every 4 hours while Venuta runs, and
  again when the network comes back after a failed check (30 minutes after a failure
  otherwise). No once-a-day limit: the manifest is one small file and the endpoint is not
  GitHub's rate-limited API.
- **What it shows.** A found update stays found: a button in the title bar ("Update to x.y.z")
  until it is installed, and one notice per version, shown when the window is visible.
  Settings › System › Updates shows the state, when it last checked, and any error in plain
  words with Try again and a link to download the installer. A manual check always ends in
  "up to date", "found" or "failed, and why".
- **How it checks.** The app reads the manifest itself first (timeouts, three tries with
  back-off for network trouble and busy servers, then the fallback), checks its version, its
  platform and its key, and only then hands that one address to the updater plugin, which reads
  it again and does the signed download. Stable: `releases/latest/download/latest.json`, then
  GitHub's releases list (the newest stable release with a `latest.json`). Pre-releases (opt in):
  the releases list first, then the stable address. The proxy from Settings is used.
- **Local only.** Local only promises no online requests, update checks included, so the quiet
  checks don't run while it is on. A manual check says so plainly and offers "Check once
  anyway", which makes exactly that one check.
- **How it installs.** Download with progress; no data for 45 seconds is a stalled download
  (tried again twice); the plugin checks the signature. The installer runs passive, into the
  folder Venuta runs from (`/D=` when that folder holds Venuta's `uninstall.exe`), restarts
  Venuta without the start's arguments (so it opens visible), and a folder Venuta can't write to
  is said before anything is downloaded. Anything that fails offers the installer's page.
- **The release.** `latest.json` carries both `windows-x86_64-nsis` and `windows-x86_64`, the same
  address and signature. `scripts/update-feed.mjs check` reads every released tag's
  configuration from git and fails the release unless each of those versions would find,
  accept and verify this manifest; `live` checks what GitHub serves. Stable releases are made
  with `--latest`, pre-releases with `--prerelease --latest=false`, so a pre-release never
  becomes the release every installed copy reads.

## Consequences

Old versions keep their own behaviour (the once-a-day check, the toast, Settings' "Install and
restart") but always find a valid update, as long as the release passes `update-feed.mjs`.
From the next release on, an installed copy shows the update in the title bar until it is
installed.
