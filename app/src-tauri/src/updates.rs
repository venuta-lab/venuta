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

/// The releases' page on GitHub, newest first, pre-releases among them.
const RELEASES_API: &str = "https://api.github.com/repos/venuta-lab/venuta/releases?per_page=10";

/// The update manifest of the newest published release, pre-releases too (the beta channel):
/// the stable endpoint (releases/latest) never names a pre-release.
fn newest_manifest() -> Result<Option<String>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .user_agent(engine::net::USER_AGENT)
        .build()
        .into();
    let text = agent
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok(newest_manifest_in(&text))
}

/// Of GitHub's releases list, the newest published one's latest.json.
fn newest_manifest_in(json: &str) -> Option<String> {
    let releases: serde_json::Value = serde_json::from_str(json).ok()?;
    releases.as_array()?.iter().find_map(|r| {
        if r.get("draft").and_then(serde_json::Value::as_bool) == Some(true) {
            return None;
        }
        r.get("assets")?.as_array()?.iter().find_map(|a| {
            (a.get("name")?.as_str()? == "latest.json")
                .then(|| a.get("browser_download_url")?.as_str().map(str::to_owned))
                .flatten()
        })
    })
}

/// The updater, pointed at the releases (pre-releases too on the beta channel; for a test
/// copy, at VENUTA_UPDATE_URL).
fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let mut b = app
        .updater_builder()
        .timeout(std::time::Duration::from_secs(20));
    if let (true, Ok(url)) = (crate::test_copy(), std::env::var("VENUTA_UPDATE_URL")) {
        let url = url.parse().map_err(|e| format!("VENUTA_UPDATE_URL: {e}"))?;
        b = b.endpoints(vec![url]).map_err(|e| e.to_string())?;
    } else if engine::settings::load().update_channel.as_deref() == Some("beta")
        && let Some(url) = newest_manifest()?
    {
        let url = url
            .parse()
            .map_err(|e| format!("the newest release: {e}"))?;
        b = b.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    b.build().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_beta_channel_takes_the_newest_published_release_pre_releases_too() {
        let list = r#"[
            {"draft": true, "assets": [{"name": "latest.json", "browser_download_url": "https://x/draft/latest.json"}]},
            {"draft": false, "prerelease": true, "assets": [{"name": "Venuta_setup.exe", "browser_download_url": "https://x/b/setup.exe"}, {"name": "latest.json", "browser_download_url": "https://x/beta/latest.json"}]},
            {"draft": false, "prerelease": false, "assets": [{"name": "latest.json", "browser_download_url": "https://x/stable/latest.json"}]}
        ]"#;
        assert_eq!(
            super::newest_manifest_in(list).as_deref(),
            Some("https://x/beta/latest.json")
        );
        assert_eq!(super::newest_manifest_in("[]"), None);
        assert_eq!(super::newest_manifest_in("not json"), None);
    }
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
