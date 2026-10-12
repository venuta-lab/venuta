//! Incremental scanning: walk the library, re-read only files whose size or mtime changed,
//! and write the result to the index in one transaction.

use crate::error::{Error, IoContext, Result};
use crate::index::{Index, ObservedFile};
use crate::media::{self, FileKind};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// Top-level folders the app owns inside the library; they are not part of the library.
pub const RESERVED_DIRS: &[&str] = &["_Incoming", "_Review", "_Quarantine"];

#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanReport {
    pub files_seen: usize,
    pub unchanged: usize,
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub read_errors: usize,
    pub audio_files: usize,
    pub elapsed_ms: u128,
}

struct Entry {
    rel_path: String,
    abs_path: PathBuf,
    kind: FileKind,
    size: u64,
    mtime_ns: i128,
}

/// Scan `root` into `index`. Safe to run repeatedly; an unchanged library does almost no work.
pub fn scan(index: &mut Index, root: &Path) -> Result<ScanReport> {
    let started = Instant::now();
    if !root.is_dir() {
        return Err(Error::BadRoot(root.to_path_buf()));
    }
    let entries = walk(root)?;
    let mut stamps = index.stamps()?;

    let mut report = ScanReport {
        files_seen: entries.len(),
        ..Default::default()
    };
    let mut to_read = Vec::new();
    for e in entries {
        match stamps.remove(&e.rel_path) {
            Some(s) if s.size == e.size && s.mtime_ns == e.mtime_ns => report.unchanged += 1,
            Some(_) => {
                report.changed += 1;
                to_read.push(e);
            }
            None => {
                report.added += 1;
                to_read.push(e);
            }
        }
    }
    // Whatever is left in `stamps` was indexed before but no longer exists.
    let vanished: Vec<i64> = stamps.values().map(|s| s.id).collect();
    report.removed = vanished.len();

    // Reading tags is I/O + parsing per file; rayon spreads it across cores.
    let observed: Vec<ObservedFile> = to_read
        .into_par_iter()
        .map(|e| {
            let (facts, read_error) = if e.kind.is_audio() {
                match media::read_facts(&e.abs_path, e.kind) {
                    Ok(f) => (Some(f), None),
                    Err(msg) => (None, Some(msg)),
                }
            } else {
                (None, None)
            };
            ObservedFile {
                rel_path: e.rel_path,
                kind: e.kind,
                size: e.size,
                mtime_ns: e.mtime_ns,
                facts,
                read_error,
            }
        })
        .collect();
    report.read_errors = observed.iter().filter(|o| o.read_error.is_some()).count();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    index.apply_scan(&observed, &vanished, now)?;

    report.audio_files = index.conn().query_row(
        "SELECT COUNT(*) FROM files WHERE kind IN ('flac', 'other_audio')",
        [],
        |r| r.get::<_, i64>(0),
    )? as usize;
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

fn walk(root: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden entries and the app's reserved top-level folders.
            if e.depth() > 0 && name.starts_with('.') {
                return false;
            }
            !(e.depth() == 1 && e.file_type().is_dir() && RESERVED_DIRS.contains(&name.as_ref()))
        });
    for entry in walker {
        let entry = entry.map_err(|e| {
            let path = e
                .path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.to_path_buf());
            Error::Io {
                path,
                source: e
                    .into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("walk error")),
            }
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let abs_path = entry.path().to_path_buf();
        let meta = entry.metadata().map_err(|e| Error::Io {
            path: abs_path.clone(),
            source: e
                .into_io_error()
                .unwrap_or_else(|| std::io::Error::other("metadata error")),
        })?;
        let mtime_ns = meta
            .modified()
            .at(&abs_path)?
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i128)
            .unwrap_or_default();
        let rel = abs_path
            .strip_prefix(root)
            .expect("walkdir yields paths under root")
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path(abs_path.clone()))?
            .replace('\\', "/");
        out.push(Entry {
            kind: FileKind::from_path(&abs_path),
            rel_path: rel,
            abs_path,
            size: meta.len(),
            mtime_ns,
        });
    }
    Ok(out)
}
