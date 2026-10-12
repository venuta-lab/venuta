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
    }
    Ok(())
}
