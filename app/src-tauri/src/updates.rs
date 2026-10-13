//! New versions: ask GitHub whether there is one, and install it when you say so. Updates are
//! signed; one that doesn't match the key built into the app is refused.

use serde::Serialize;
use tauri::AppHandle;
use tauri::ipc::Channel;
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct UpdateProgress {
    pub done: u64,
    pub total: Option<u64>,
}

/// The updater, pointed at the releases (or, for a test copy, at VENUTA_UPDATE_URL).
fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let mut b = app
        .updater_builder()
        .timeout(std::time::Duration::from_secs(20));
    if let Ok(url) = std::env::var("VENUTA_UPDATE_URL") {
        let url = url.parse().map_err(|e| format!("VENUTA_UPDATE_URL: {e}"))?;
        b = b.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    b.build().map_err(|e| e.to_string())
}

/// A newer version, if there is one.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = updater(&app)?.check().await.map_err(|e| e.to_string())?;
    Ok(found.map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    }))
}

/// Download the new version (its signature is checked), install it and start it. The
/// installer closes Venuta itself. VENUTA_UPDATE_DRY stops after the checked download.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), String> {
    let update = updater(&app)?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Venuta is up to date.")?;
    let mut done = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let _ = on_progress.send(UpdateProgress { done, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    if std::env::var_os("VENUTA_UPDATE_DRY").is_some() {
        return Ok(());
    }
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}
