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
            r.save(&rules).map_err(anyhow::Error::msg)?;
            println!(
                "rules in {}: {} aliases, {} category rules",
                rules.display(),
                r.aliases.artists.len(),
                r.categories.rules.len()
            );
        }
        Command::Plan { root, rules, only } => {
            let root_key = paths::canonical_root_string(&root);
            let db = cli.db.unwrap_or_else(|| paths::index_path_for(&root));
            let index = Index::open(&db, &root_key)?;
            let lib = engine::library::Library::load(&index)?;
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let started = std::time::Instant::now();
            let mut plan = engine::plan::build(&lib, &r);
            let elapsed = started.elapsed().as_millis();
            if let Some(o) = &only {
                plan.albums.retain(|a| a.id.contains(o.as_str()));
            }
            if cli.json {
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
                    "{} albums planned in {elapsed} ms: {} high, {} medium, {} low confidence; {} set aside as duplicates; {} questions",
                    plan.albums.len(),
                    count(Confidence::High),
                    count(Confidence::Medium),
                    count(Confidence::Low),
                    quarantine,
                    plan.questions.len()
                );
                for q in &plan.questions {
                    println!("  ? {}", q.text);
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
            let lib = engine::library::Library::load(&index)?;
            let r = engine::rules::Rules::load(&rules).map_err(anyhow::Error::msg)?;
            let mut plan = engine::plan::build(&lib, &r);
            if let Some(o) = &only {
                plan.albums.retain(|a| a.id.contains(o.as_str()));
            }
            let progress = engine::apply::ApplyProgress::default();
            let report = engine::apply::apply(&plan.albums, &root, &target, &progress, &|_, _| {})
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
