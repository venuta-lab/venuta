## What this changes

<!-- One or two sentences: what a user notices, or what is now possible. -->

## Why

<!-- The problem, or the issue it closes (Closes #123). -->

## How it was checked

- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets` pass
- [ ] `pnpm typecheck` and `pnpm test` pass (in `app/`)
- [ ] Tried in the app on a copy of a library, not on real music (say how)
- [ ] Screenshots for anything visible, in at least one light and one dark theme

## Safety

- [ ] Nothing here can change, move or delete a user's files without a journal and Undo
- [ ] No keys, personal paths or music in the diff
