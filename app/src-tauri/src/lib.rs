//! Desktop shell. Commands are thin wrappers that run engine work off the UI thread and
//! return serialisable results; all logic lives in the `engine` crate.

use engine::art::CoverThumb;
use engine::audit::HealthReport;
use engine::library::Library;
use engine::{Index, ScanReport, paths};
use std::path::{Path, PathBuf};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            engine_version,
            scan_library,
            library_health,
            album_covers
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
