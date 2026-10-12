use anyhow::Result;
use clap::{Parser, Subcommand};
use engine::{Index, paths};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "librarian", version = engine::VERSION, about = "Headless music library engine")]
struct Cli {
    /// Index database path (default: per-library file in the app data folder)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    /// Print results as JSON
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a library folder into the index (read-only on the library)
    Scan { root: PathBuf },
    /// Scan, then report library health (read-only)
    Health { root: PathBuf },
    /// Decode every new or changed track: integrity, quality suspicions, loudness (read-only)
    Deepcheck { root: PathBuf },
    /// Write starter rules (aliases from the library, default layout) without overwriting existing files
    RulesInit { root: PathBuf, rules: PathBuf },
    /// Show what the planner would change (read-only)
    Plan {
        root: PathBuf,
        rules: PathBuf,
        /// Only albums whose folder contains this text
        #[arg(long)]
        only: Option<String>,
        /// With --json: print the compact view the Review screen uses
        #[arg(long)]
        view: bool,
    },
    /// Build a new library at TARGET from the plan (copies; the source is never written)
    Apply {
        root: PathBuf,
        rules: PathBuf,
        target: PathBuf,
        /// Only albums whose folder contains this text
        #[arg(long)]
        only: Option<String>,
    },
    /// Remove everything an apply run created, using its journal
    Undo { journal: PathBuf },
    /// Plan the albums waiting in LIBRARY/_Incoming (scans and looks them up first)
    IncomingPlan { library: PathBuf, rules: PathBuf },
    /// Import every ready album from LIBRARY/_Incoming into the library
    IncomingImport { library: PathBuf, rules: PathBuf },
    /// Set up a card (or any folder) to receive the library
    CardSetup {
        card: PathBuf,
        name: String,
        /// Top-level library folders to include (default: everything)
        #[arg(long)]
        include: Vec<String>,
    },
    /// Mirror the library onto a set-up card
    CardSync { library: PathBuf, card: PathBuf },
    /// Match your Apple Music playlists to the library (dry run unless --write)
    AppleImport {
        library: PathBuf,
        musicdb: PathBuf,
        key: String,
        #[arg(long)]
        write: bool,
    },
    /// Look up albums online (MusicBrainz, Apple, Deezer) and missing lyrics (LRCLIB)
    Lookup { root: PathBuf, rules: PathBuf },
    /// Make (cached) cover thumbnails for every album; prints their paths as JSON
    Covers {
        root: PathBuf,
        #[arg(long, default_value_t = 320)]
        size: u32,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();

    match cli.command {
        Command::Scan { root } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let mut index = Index::open(&db, &root_key)?;
            let report = engine::scan(&mut index, &root)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("index:      {}", db.display());
                println!(
                    "files:      {} seen, {} unchanged, {} added, {} changed, {} removed",
                    report.files_seen,
                    report.unchanged,
                    report.added,
                    report.changed,
                    report.removed
                );
                println!(
                    "audio:      {} files, {} read errors",
                    report.audio_files, report.read_errors
                );
                println!("time:       {} ms", report.elapsed_ms);
            }
        }
        Command::Health { root } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let mut index = Index::open(&db, &root_key)?;
            engine::scan(&mut index, &root)?;
            let started = std::time::Instant::now();
            let lib = engine::library::Library::load(&index)?;
            let report = engine::audit::run(&lib);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let s = &report.summary;
                println!(
                    "{} tracks in {} albums, {:.1} hours, {:.1} GB. {} tracks healthy. ({} ms)",
                    s.tracks,
                    s.albums,
                    s.hours,
                    s.bytes as f64 / 1e9,
                    s.healthy_tracks,
                    started.elapsed().as_millis()
                );
                for i in report.issues.iter() {
                    println!(
                        "\n[{:?}] {}: {} tracks in {} albums",
                        i.severity,
                        i.title,
                        i.tracks,
                        i.albums.len()
                    );
                    for e in i.examples.iter().take(3) {
                        println!("    {e}");
                    }
                }
            }
        }
        Command::Deepcheck { root } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let mut index = Index::open(&db, &root_key)?;
            engine::scan(&mut index, &root)?;
            let progress = engine::quality::Progress::default();
            let last = std::sync::atomic::AtomicUsize::new(0);
            let report = engine::quality::run(&mut index, &root, &progress, &|done, total| {
                // Print roughly every 5%.
                let step = (total / 20).max(1);
                if done / step != last.swap(done, std::sync::atomic::Ordering::Relaxed) / step {
                    eprintln!("  {done} / {total}");
                }
            })?;
            println!(
                "deep check: {} tracks in {} albums, {:.1} s{}",
                report.tracks,
                report.albums,
                report.elapsed_ms as f64 / 1000.0,
                if report.cancelled { " (cancelled)" } else { "" }
            );
        }
        Command::RulesInit { root, rules } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let index = Index::open(&db, &root_key)?;
            let lib = engine::library::Library::load(&index)?;
            let mut r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            if r.aliases.artists.is_empty() {
                r.aliases = engine::rules::bootstrap_aliases(&lib);
            }
            r.save_missing(&rules).map_err(anyhow::Error::msg)?;
            println!(
                "rules in {}: {} aliases, {} category rules",
                rules.display(),
                r.aliases.artists.len(),
                r.categories.rules.len()
            );
        }
        Command::Plan {
            root,
            rules,
            only,
            view,
        } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let index = Index::open(&db, &root_key)?;
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let started = std::time::Instant::now();
            let (_, mut plan) = engine::plan::build_checked(&index, &root, &r)?;
            let elapsed = started.elapsed().as_millis();
            if let Some(o) = &only {
                plan.albums.retain(|a| a.id.contains(o.as_str()));
            }
            if cli.json && view {
                println!("{}", serde_json::to_string(&plan.view())?);
            } else if cli.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                use engine::plan::{AlbumAction, Confidence};
                let count =
                    |c: Confidence| plan.albums.iter().filter(|a| a.confidence == c).count();
                let quarantine = plan
                    .albums
                    .iter()
                    .filter(|a| matches!(a.action, AlbumAction::Quarantine { .. }))
                    .count();
                println!(
                    "{} albums planned in {elapsed} ms: {} high, {} medium, {} low confidence; {} set aside as duplicates; {} decisions",
                    plan.albums.len(),
                    count(Confidence::High),
                    count(Confidence::Medium),
                    count(Confidence::Low),
                    quarantine,
                    plan.decisions.len()
                );
                for d in &plan.decisions {
                    println!(
                        "  {} {:?} {} | {}{}",
                        if d.auto { "✓" } else { "?" },
                        d.kind,
                        d.subject,
                        if d.auto { &d.proof } else { &d.why },
                        d.suggested
                            .as_ref()
                            .map(|s| format!(" → {s}"))
                            .unwrap_or_default()
                    );
                }
                for a in plan.albums.iter().take(if only.is_some() { 50 } else { 3 }) {
                    println!("\n{}  [{:?}]\n  -> {}", a.id, a.confidence, a.target_dir);
                    for n in &a.notes {
                        println!("  ! {n}");
                    }
                    for c in &a.changes {
                        println!(
                            "  {:<22} {:?} -> {:?}  ({:?}) {}",
                            c.field, c.before, c.after, c.confidence, c.reason
                        );
                    }
                    if let Some(t) = a.tracks.first() {
                        println!("  e.g. {}", t.target);
                    }
                }
            }
        }
        Command::Apply {
            root,
            rules,
            target,
            only,
        } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let index = Index::open(&db, &root_key)?;
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let (_, mut plan) = engine::plan::build_checked(&index, &root, &r)?;
            // Albums still waiting for an answer are not built.
            plan.albums.retain(|a| a.waiting_on.is_empty());
            if let Some(o) = &only {
                plan.albums.retain(|a| a.id.contains(o.as_str()));
            }
            let progress = engine::apply::ApplyProgress::default();
            let report = engine::apply::apply(&plan.albums, &root, &target, &progress, &|_, _| {})
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::IncomingPlan { library, rules } => {
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let lp = engine::enrich::EnrichProgress::default();
            let look = engine::intake::lookup(&library, &r, &lp, &|_, _| {})
                .map_err(anyhow::Error::msg)?;
            eprintln!(
                "looked up {} albums ({} MusicBrainz, {} Apple, {} Deezer matches), {} lyrics found",
                look.albums_looked_up,
                look.musicbrainz_matches,
                look.apple_matches,
                look.deezer_matches,
                look.lyrics_found
            );
            let plan = engine::intake::plan(&library, &r)?;
            for a in &plan.albums {
                println!(
                    "
{} [{:?}] {:?}
  -> {}",
                    a.id, a.confidence, a.action, a.target_dir
                );
                for n in &a.notes {
                    println!("  ! {n}");
                }
                for c in &a.changes {
                    println!(
                        "  {:<22} {:?} -> {:?}  {}",
                        c.field,
                        c.before.iter().take(2).collect::<Vec<_>>(),
                        c.after.iter().take(2).collect::<Vec<_>>(),
                        c.reason
                    );
                }
            }
        }
        Command::IncomingImport { library, rules } => {
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let mut plan = engine::intake::plan(&library, &r)?;
            plan.albums
                .retain(|a| matches!(a.action, engine::plan::AlbumAction::Migrate));
            let progress = engine::apply::ApplyProgress::default();
            let report = engine::intake::import(&library, &plan.albums, &progress, &|_, _| {})
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::CardSetup {
            card,
            name,
            include,
        } => {
            let c = engine::sync::setup_card(&card, &name, include, false)
                .map_err(anyhow::Error::msg)?;
            println!("card {} ({}) ready", c.name, c.id);
        }
        Command::CardSync { library, card } => {
            let mut idx = Index::open(
                &paths::index_path_for(&library),
                &paths::canonical_root_string(&library),
            )?;
            engine::scan(&mut idx, &library)?;
            let plan = engine::sync::plan(&library, &card).map_err(anyhow::Error::msg)?;
            eprintln!(
                "plan: {} copy ({} MB), {} patch, {} move, {} adopt, {} delete, {} unchanged",
                plan.copy,
                plan.bytes_to_copy / 1_000_000,
                plan.patch,
                plan.moves,
                plan.adopt,
                plan.delete,
                plan.unchanged
            );
            let progress = engine::sync::SyncProgress::default();
            let report = engine::sync::run(&library, &card, &plan, &progress, &|_, _, _| {})
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string(&report)?);
        }
        Command::AppleImport {
            library,
            musicdb,
            key,
            write,
        } => {
            let idx = Index::open(
                &paths::index_path_for(&library),
                &paths::canonical_root_string(&library),
            )?;
            let lib = engine::library::Library::load(&idx)?;
            let apple = engine::apple::read(&musicdb, &key).map_err(anyhow::Error::msg)?;
            let r = engine::playlists::import_apple(&library, &lib, &apple, "", write)
                .map_err(anyhow::Error::msg)?;
            println!(
                "{} of {} unique tracks matched",
                r.tracks_matched, r.tracks_total
            );
            for p in &r.playlists {
                println!("  {:28} {:4} / {}", p.name, p.matched, p.total);
            }
            let tiers = |n: u8| r.wanted.iter().filter(|w| w.tier == n).count();
            println!(
                "want-list: {} albums (tier 1: {}, tier 2: {}, tier 3: {})",
                r.wanted.len(),
                tiers(1),
                tiers(2),
                tiers(3)
            );
            for w in r.wanted.iter().take(8) {
                println!("  T{} {} / {}: {}", w.tier, w.album, w.artist, w.why);
            }
        }
        Command::Lookup { root, rules } => {
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let progress = engine::enrich::EnrichProgress::default();
            let last = std::sync::atomic::AtomicUsize::new(0);
            let report = engine::enrich::run_for_root(&root, &r, &progress, &|done, total| {
                let step = (total / 20).max(1);
                if done / step != last.swap(done, std::sync::atomic::Ordering::Relaxed) / step {
                    eprintln!("  {done} / {total}");
                }
            })
            .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Undo { journal } => {
            let n = engine::apply::undo(&journal).map_err(anyhow::Error::msg)?;
            println!("removed {n} files");
        }
        Command::Covers { root, size } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let index = Index::open(&db, &root_key)?;
            let lib = engine::library::Library::load(&index)?;
            let dirs: Vec<String> = lib.album_summaries().into_iter().map(|a| a.dir).collect();
            let started = std::time::Instant::now();
            let thumbs = engine::art::thumbnails(&root, lib.cover_requests(&dirs), size);
            eprintln!(
                "{} thumbnails in {} ms",
                thumbs.len(),
                started.elapsed().as_millis()
            );
            println!("{}", serde_json::to_string_pretty(&thumbs)?);
        }
    }
    Ok(())
}
