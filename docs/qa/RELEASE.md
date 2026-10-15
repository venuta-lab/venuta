# Before a release

Every release, in this order. Each step says how to check it and what passing means. The gates
come from [the review fixes](../plans/PLAN-REVIEW-FIXES.md); [FEATURES.md](FEATURES.md) is the
list of what must keep working.

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

## 5. Then release

`scripts/release.mjs` (it needs `git -c user.name=… -c user.email=…` on a machine without a git
identity). The maintainer signs and publishes.
