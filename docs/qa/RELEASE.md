# Before a release

Every release, in this order. Each step says how to check it and what passing means. [FEATURES.md](FEATURES.md)
is the list of what must keep working.

## 1. CI is green

On the branch to release: `cargo fmt --all --check`; clippy with `-D warnings` for `engine`,
`cli`, `player` and `app`; the Rust tests; `pnpm --dir app lint`, `typecheck`, `test`;
`node scripts/theme-docs.mjs --check`; `pnpm --dir app build`. The same steps as
`.github/workflows/ci.yml`; run them locally before pushing.

## 2. The gates pass, in a test copy

The gates are measured in a running test copy, as a person uses it, on a **production build of
the interface** (the development build draws everything twice and is several times slower), and
on a **quiet machine** (a model server or a build running at the same time shows up as slow
frames that aren't the app's).

1. A test copy of the library, so nothing of yours changes: hard links of the music
   (`cp -al <library>/Music <copy>/Music`) and a plain copy of `.venuta` and `Playlists`. The
   gates play songs silently (volume 0) and pause after; they change no file.
2. The interface built and served on the dev port: `pnpm --dir app build`, then
   `pnpm --dir app exec vite preview --port 5173 --strictPort`.
3. The app, debug build, pointed at the copy with the inspector on:
   `VENUTA_HOME=<copy> VENUTA_MULTI=1 VENUTA_FAKE_CATALOG=1
   WEBVIEW2_USER_DATA_FOLDER=<a folder of its own>
   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 target/debug/app.exe`
   (`VENUTA_FAKE_CATALOG=1` keeps the gates off the live online catalog).
4. `QA_PORT=9333 QA_BIG_PLAYLIST="<your largest playlist>" node scripts/qa/gates.mjs`

| Gate | Passes when |
|---|---|
| Places open and scroll | No frame over 50 ms opening Metadata, Health, Organize, Devices, Music and its tabs, or scrolling them, three rounds |
| Queue of the whole library | Opens in under 150 ms; no frame over 50 ms opening or scrolling it |
| Big playlist | Opens in under 300 ms; no frame over 50 ms |
| Typing in search | No keystroke over 50 ms, also the first search after starting |
| Right-click | The browser's own menu never shows; empty space shows nothing; things show their menu |
| Memory | Within 10 MB of the start after 200 page visits |
| Console | No errors |

And the CPU, with `pwsh scripts/qa/cpu.ps1 -Seconds 20 -Split` while the copy is: idle and shown
(under 3% of a core), playing and shown (under 10%), minimised and paused (under 1%).

## 3. Every theme still draws every screen

In the same test copy (its inspector port as `VENUTA_CDP_PORT`): `node scripts/theme-shots.mjs take
<after>` on the release, and `take <before>` on the last release, then
`node scripts/theme-shots.mjs compare <before> <after>`. Every difference it counts is one that was
meant; look through the shots too: nothing unreadable, nothing missing, the album background where
the theme has one.

## 4. FEATURES.md, walked through

Each line of [FEATURES.md](FEATURES.md), done by hand in the test copy, ticked. New features of
the release are added to it first.

## 4b. Listen together, on two computers

When the release touches listening together or the player's sound path: the live matrix in
[listen-together-live.md](listen-together-live.md), every network row, every capture clean.

## 5. Then release

`scripts/release.mjs` (it needs `git -c user.name=… -c user.email=…` on a machine without a git
identity). The maintainer signs and publishes.

To try the real installer first, `node scripts/release.mjs <x.y.z> --local` builds the same signed
installer into `target/release/bundle/nsis` and stops: nothing committed, tagged, pushed or
released.

## 6. Every installed version can update to it

Every version ever released asks
`https://github.com/venuta-lab/venuta/releases/latest/download/latest.json`, looks for
`windows-x86_64-nsis` then `windows-x86_64`, and installs only an installer signed by the update
key (see [decision 0009](../decisions/0009-updates-that-arrive.md)). So:

- `release.mjs` (and the release workflow) refuse to go on unless
  `node scripts/update-feed.mjs check` passes: each released tag's endpoint, platform and key,
  against the new `latest.json` and the installer's bytes. It needs the tags (`git fetch --tags`).
- A stable release is published as **the latest release** (keep "Set as the latest release"
  ticked), with both `latest.json` and the installer. A pre-release is made with `--prerelease`
  (workflow: **prerelease**), is never the latest, and only copies with Pre-releases on see it.
- Never upload a `latest.json` by hand, and never mark a stable release as a pre-release (v0.1.0
  was, and until v0.2.0 was published every update check failed).
- Once published: `node scripts/update-feed.mjs live`. It reads what GitHub serves, as each
  released version asks it, and the installer's address; every version must say "ok".
