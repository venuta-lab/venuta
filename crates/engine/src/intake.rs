//! Incoming: new music dropped into `<library>/_Incoming` becomes finished library albums.
//!
//! The incoming folder has its own index, so new files never mix with the library until
//! they are imported. Flow: scan → look up online → plan with your rules (category inferred
//! from the catalogues when no rule matches) → review → import (apply into the library, then
//! move the originals to `_Incoming/.imported/<when>/`, never deleting anything).

use crate::apply::{ApplyProgress, ApplyReport};
use crate::library::Library;
use crate::plan::{AlbumAction, Plan};
use crate::rules::Rules;
use crate::{Index, paths};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const INCOMING: &str = "_Incoming";

/// Where new music is dropped: `Add music here` beside a library's Music folder
/// (`_Incoming` inside older libraries).
pub fn incoming_dir(library_root: &Path) -> PathBuf {
    paths::beside(library_root, paths::Helper::AddMusic)
}

pub fn open_incoming_index(library_root: &Path) -> crate::Result<(PathBuf, Index)> {
    let dir = incoming_dir(library_root);
    std::fs::create_dir_all(&dir).map_err(|e| crate::Error::Io {
        path: dir.clone(),
        source: e,
    })?;
    let index = Index::open(
        &paths::index_path_for(&dir),
        &paths::canonical_root_string(&dir),
    )?;
    Ok((dir, index))
}

/// Scan the incoming folder and plan every album in it. Albums whose audio is already in the
/// library are set aside as duplicates instead of being imported twice.
pub fn plan(library_root: &Path, rules: &Rules) -> crate::Result<Plan> {
    // The folder is only created when something is added, so looking never writes anything.
    if !incoming_dir(library_root).is_dir() {
        return Ok(Plan::default());
    }
    let (dir, mut index) = open_incoming_index(library_root)?;
    crate::scan(&mut index, &dir)?;
    let lib = Library::load(&index)?;
    let mut plan = crate::plan::build(&lib, rules);

    // Already in the library? Compare audio checksums with the library's index.
    let library_index = Index::open(
        &paths::index_path_for(library_root),
        &paths::canonical_root_string(library_root),
    )?;
    let mut owned: HashMap<Vec<u8>, String> = HashMap::new();
    {
        let mut stmt = library_index
            .conn()
            .prepare("SELECT audio_md5, path FROM files WHERE audio_md5 IS NOT NULL")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (md5, path) = row?;
            owned.insert(md5, path);
        }
    }
    let md5_of: HashMap<i64, Vec<u8>> = lib
        .tracks
        .iter()
        .filter_map(|t| Some((t.id, t.audio_md5.clone()?)))
        .collect();
    for album in &mut plan.albums {
        let hits: Vec<&String> = album
            .tracks
            .iter()
            .filter_map(|t| md5_of.get(&t.file_id).and_then(|m| owned.get(m)))
            .collect();
        if !hits.is_empty() && hits.len() == album.tracks.len() {
            let where_ = crate::library::parent_dir(hits[0]).to_owned();
            album
                .notes
                .push(format!("Already in your library at “{where_}”."));
            album.action = AlbumAction::Quarantine {
                duplicate_of: where_,
            };
            album.confidence = crate::plan::Confidence::Low;
        } else if !hits.is_empty() {
            album.notes.push(format!(
                "{} of these {} tracks are already in your library.",
                hits.len(),
                album.tracks.len()
            ));
        }
    }
    Ok(plan)
}

/// Import approved incoming albums into the library, then move their originals aside.
pub fn import(
    library_root: &Path,
    albums: &[crate::plan::AlbumPlan],
    progress: &ApplyProgress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ApplyReport, String> {
    let incoming = incoming_dir(library_root);
    let report = crate::apply::apply(albums, &incoming, library_root, progress, on_progress)?;
    let failed: std::collections::HashSet<&str> =
        report.errors.iter().map(|e| e.album.as_str()).collect();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let done_dir = incoming.join(".imported").join(stamp.to_string());
    for album in albums.iter().filter(|a| !failed.contains(a.id.as_str())) {
        if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        for t in &album.tracks {
            let src = incoming.join(&t.source);
            let dst = done_dir.join(&t.source);
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::rename(&src, &dst);
            // Sidecars that travelled with the track (lyrics) move too.
            for ext in ["lrc", "LRC"] {
                let side = src.with_extension(ext);
                if side.exists() {
                    let _ = std::fs::rename(&side, dst.with_extension(ext));
                }
            }
        }
        // Leftovers in the album folder (cover.jpg, scans) move with it; then the folder goes.
        let album_src = incoming.join(&album.id);
        if album.id.is_empty() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&album_src) {
            for e in entries.flatten() {
                let dst = done_dir.join(&album.id).join(e.file_name());
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::rename(e.path(), dst);
            }
        }
        let _ = std::fs::remove_dir(&album_src);
    }
    // The library gained files: refresh its index.
    if let Ok(mut idx) = Index::open(
        &paths::index_path_for(library_root),
        &paths::canonical_root_string(library_root),
    ) {
        let _ = crate::scan(&mut idx, library_root);
    }
    Ok(report)
}

/// Copy dropped files or folders into the incoming folder (originals stay where they were).
pub fn add(library_root: &Path, sources: &[PathBuf]) -> Result<usize, String> {
    let incoming = incoming_dir(library_root);
    std::fs::create_dir_all(&incoming).map_err(|e| e.to_string())?;
    let mut copied = 0;
    for src in sources {
        let name = src.file_name().ok_or("bad path")?;
        if src.is_dir() {
            for entry in walkdir::WalkDir::new(src)
                .into_iter()
                .flatten()
                .filter(|e| e.file_type().is_file())
            {
                let rel = entry.path().strip_prefix(src).map_err(|e| e.to_string())?;
                let dst = incoming.join(name).join(rel);
                if dst.exists() {
                    continue;
                }
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::copy(entry.path(), &dst).map_err(|e| e.to_string())?;
                copied += 1;
            }
        } else {
            let dst = incoming.join(name);
            if !dst.exists() {
                std::fs::copy(src, &dst).map_err(|e| e.to_string())?;
                copied += 1;
            }
        }
    }
    Ok(copied)
}

/// Look up the incoming albums online (same sources and cache as the library).
pub fn lookup(
    library_root: &Path,
    rules: &Rules,
    progress: &crate::enrich::EnrichProgress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<crate::enrich::EnrichReport, String> {
    if !incoming_dir(library_root).is_dir() {
        return Ok(crate::enrich::EnrichReport::default());
    }
    let (dir, mut index) = open_incoming_index(library_root).map_err(|e| e.to_string())?;
    crate::scan(&mut index, &dir).map_err(|e| e.to_string())?;
    let lib = Library::load(&index).map_err(|e| e.to_string())?;
    let opts = crate::enrich::LookupOptions::from_settings(&crate::settings::load());
    crate::enrich::run(&mut index, &dir, &lib, rules, &opts, progress, on_progress)
        .map_err(|e| e.to_string())
}
