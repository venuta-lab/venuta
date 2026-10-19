# Working on Venuta

Thanks for helping. This page is everything you need to build Venuta, change it, and send the
change back.

## How changes come in

`main` is the released app, and only the maintainer writes to it. Your changes arrive as pull
requests from your fork.

1. **Fork** `venuta` to your own account (the Fork button), and clone your fork.
2. Add the original as `upstream`, so you can stay current:

   ```bash
   git remote add upstream https://github.com/venuta-lab/venuta.git
   git fetch upstream
   ```

3. **Branch** from the latest `main` for each change: `feat/lyrics-fonts`, `fix/card-paths`.

   ```bash
   git switch -c feat/your-change upstream/main
   ```

4. Commit in small steps with [Conventional Commits](https://www.conventionalcommits.org)
   (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`). Say what the user notices, not
   which function moved.
5. Push to your fork and **open a pull request** against `venuta-lab/venuta` `main`. Fill in the
   template. CI builds and tests it; the maintainer reviews, and merges with a squash.
6. Before starting something large, open an issue (or a draft pull request) so we agree on the
   shape first.

## Build it

Windows 10 or 11 (the player uses WASAPI; the engine also builds on Linux and macOS).

- **Rust** (stable, via [rustup](https://rustup.rs); `rust-toolchain.toml` picks the version)
- **Visual Studio Build Tools** with "Desktop development with C++"
- **CMake**, for the Opus decoder: the C++ workload above includes one (its folder is
  `...\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`). Put it on `PATH`, or set `CMAKE`
  to that `cmake.exe`, or `winget install Kitware.CMake`. `scripts/hybrid-dev.ps1` finds it for you.
- **Node 24** and **pnpm 10**
- **WebView2** (already on Windows 11)

```bash
pnpm --dir app install
pnpm --dir app tauri dev        # the app, reloading as you edit
```

Before you push, the same checks CI runs:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir app typecheck        # not `tsc -p .`: that checks nothing here
pnpm --dir app test
```

A release installer: `pnpm --dir app tauri build --bundles nsis` (the maintainer signs releases).

## Never on real music

Test on a **copy**. Run a second Venuta beside your own, with its own data, so nothing you do
touches your library or your settings:

```bash
# Git Bash
VENUTA_MULTI=1 VENUTA_HOME="D:/venuta-test" pnpm --dir app tauri dev
```

`VENUTA_HOME` is the library folder this copy uses (it keeps everything in `.venuta` inside it);
`VENUTA_MULTI=1` lets it run while your own Venuta is open, and shows a red "Test copy" badge.

## The rules the code keeps

- **Nothing is lost.** Venuta never deletes music. Anything it changes, moves or renames is
  written to a journal first and can be undone from Activity, exactly. Duplicates and removals
  go to "Set aside", never the recycle bin. A change that can't be undone doesn't ship.
- **Only the user starts work that writes.** Opening a screen reads; writing happens on a click
  the user understood.
- **Engine first.** Library logic lives in `crates/engine` with tests; the app calls it. The UI
  never decides what happens to files.
- **Words in the app** are plain sentences in sentence case, saying what happens ("Save
  timing", "Set aside 3 copies"). No em dashes.
- **No keys, personal paths or music in the repo.** API keys come from the user, at run time.
  Song lyrics and album art stay out of commits and screenshots.
- Don't run Prettier: there's no project config and it would rewrite whole files.

## Where things are

| Path | What |
|---|---|
| `crates/engine` | The library: scan, index (SQLite), rules, plan and apply with undo, health, playlists, cards, lookups, lyrics |
| `crates/player` | The audio engine: decoding, WASAPI output, queue, sound tools |
| `crates/media-access` | The online catalog, the same for every provider |
| `crates/ytmusic` | The YouTube Music client: search, suggestions, pages, radio, lyrics |
| `crates/together` | Listen together rooms: the messages, the host's room, the shared clock, the invite |
| `crates/phone-server` | Your library to phones on the same Wi-Fi (OpenSubsonic) |
| `crates/cli` | `venuta`, the engine from the command line |
| `app/src-tauri` | The desktop shell: commands the UI calls, tray, windows |
| `app/src` | The interface (React, Tailwind, Motion) |
| `docs/decisions` | Why things are the way they are (read before changing a rule) |
| `docs/research` | Sourced research behind the decisions |
| `docs/PLAN-*.md` | Plans, done and to come |

## Questions

Open an issue, or ask in your pull request.
