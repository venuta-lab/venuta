# Music Librarian (working title)

A free, open-source desktop app that builds and keeps a perfect personal lossless music library,
and mirrors it to SD cards for portable players.

- **Audit** the library: tags, duplicates, art, lyrics, loudness, paths, suspicious quality.
- **Organise** it with your own rules, as a reviewable, undoable change set.
- **Intake** new albums from an `_Incoming` folder with metadata, art, lyrics and loudness.
- **Browse, edit and curate**: tags, ratings, playlists.
- **Sync** to SD cards: one-way mirror, safe to interrupt, fast.

No AI at runtime, no accounts, no paid services, never downloads music.

> Built for one person's library first, shared as-is in case it helps you. No support promised.

Status: pre-alpha (milestones 1–2 working: health audit, deep check, review and migration, album editor). See [docs/PLAN.md](docs/PLAN.md) for the design and [docs/research](docs/research) for the sources behind it.

## Development

Requirements: Rust (stable), Node 24 + pnpm 10, and on Windows the MSVC C++ build tools and WebView2.

```bash
cargo test                                  # engine + cli tests
cargo run -p cli -- scan <library-folder>   # headless scan (read-only)
pnpm --dir app install
pnpm --dir app tauri dev                    # desktop app with hot reload
```

Layout: `crates/engine` (all logic, no UI) · `crates/cli` (headless binary `librarian`) · `app/` (Tauri shell + React UI) · `docs/` (plan, research, decision records).
