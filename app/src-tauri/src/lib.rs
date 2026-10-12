//! Desktop shell. Commands are thin wrappers that run engine work off the UI thread and
//! return serialisable results; all logic lives in the `engine` crate.

use engine::art::CoverThumb;
use engine::audit::HealthReport;
use engine::library::Library;
use engine::quality::{DeepCheckReport, Progress};
use engine::{Index, ScanReport, paths};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri::ipc::Channel;

/// The running deep check, if any, so it can be cancelled.
#[derive(Default)]
struct Jobs {
    deep_check: Mutex<Option<Arc<Progress>>>,
}

#[derive(Clone, Serialize)]
struct DeepProgress {
    done: usize,
    total: usize,
}

/// Errors cross the IPC boundary as plain messages.
type CmdResult<T> = Result<T, String>;

fn open_index(root: &Path) -> CmdResult<Index> {
    Index::open(
        &paths::index_path_for(root),
        &paths::canonical_root_string(root),
    )
    .map_err(|e| e.to_string())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn engine_version() -> &'static str {
    engine::VERSION
}

/// Scan a library folder into its index. Read-only on the library.
#[tauri::command]
async fn scan_library(root: String) -> CmdResult<ScanReport> {
    blocking(move || {
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())
    })
    .await
}

/// Health report from the current index (scan first for fresh results).
#[tauri::command]
async fn library_health(root: String) -> CmdResult<HealthReport> {
    blocking(move || {
        let index = open_index(Path::new(&root))?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        Ok(engine::audit::run(&lib))
    })
    .await
}

/// Cover thumbnails (cached JPEG paths + accent colour) for the given album folders.
#[tauri::command]
async fn album_covers(root: String, dirs: Vec<String>, size: u32) -> CmdResult<Vec<CoverThumb>> {
    blocking(move || {
        let root = PathBuf::from(root);
        let index = open_index(&root)?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        let requests = lib.cover_requests(&dirs);
        Ok(engine::art::thumbnails(
            &root,
            requests,
            size.clamp(64, 1024),
        ))
    })
    .await
}

/// Decode new or changed tracks (integrity, quality, loudness). Progress streams over `on_progress`.
#[tauri::command]
async fn deep_check(
    root: String,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<DeepCheckReport> {
    let progress = Arc::new(Progress::default());
    {
        let mut slot = jobs.deep_check.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("A deep check is already running.".into());
        }
        *slot = Some(progress.clone());
    }
    let p = progress.clone();
    let result = blocking(move || {
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())?;
        engine::quality::run(&mut index, &root, &p, &|done, total| {
            let _ = on_progress.send(DeepProgress { done, total });
        })
        .map_err(|e| e.to_string())
    })
    .await;
    if let Ok(mut slot) = jobs.deep_check.lock() {
        *slot = None;
    }
    result
}

#[tauri::command]
fn cancel_deep_check(jobs: State<'_, Jobs>) {
    if let Ok(slot) = jobs.deep_check.lock()
        && let Some(p) = slot.as_ref()
    {
        p.cancel.store(true, Ordering::Relaxed);
    }
}

/// Spectrogram of an album's first track (cached PNG path).
#[tauri::command]
async fn album_spectrogram(root: String, dir: String) -> CmdResult<String> {
    blocking(move || {
        let root = PathBuf::from(root);
        let index = open_index(&root)?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        let track = lib
            .tracks
            .iter()
            .filter(|t| t.dir == dir && t.kind == "flac")
            .min_by(|a, b| a.path.cmp(&b.path))
            .ok_or("No FLAC track in this album.")?;
        let key = match &track.audio_md5 {
            Some(m) => m.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            None => blake3::hash(format!("{}:{}", track.path, track.size).as_bytes())
                .to_hex()
                .to_string(),
        };
        engine::spectrogram::render_cached(&root.join(&track.path), &key)
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Jobs::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            engine_version,
            scan_library,
            library_health,
            album_covers,
            deep_check,
            cancel_deep_check,
            album_spectrogram
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
