//! Desktop shell. Commands are thin wrappers that run engine work off the UI thread and
//! return serialisable results; all logic lives in the `engine` crate.

use engine::{Index, ScanReport, paths};
use std::path::PathBuf;

/// Errors cross the IPC boundary as plain messages.
type CmdResult<T> = Result<T, String>;

#[tauri::command]
fn engine_version() -> &'static str {
    engine::VERSION
}

/// Scan a library folder into its index. Read-only on the library.
#[tauri::command]
async fn scan_library(root: String) -> CmdResult<ScanReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(root);
        let root_key = paths::canonical_root_string(&root);
        let mut index =
            Index::open(&paths::index_path_for(&root), &root_key).map_err(|e| e.to_string())?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![engine_version, scan_library])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
