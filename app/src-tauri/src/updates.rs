//! New versions: found quietly, offered in the title bar, installed in one click. The manifest is
//! read first by `feed` (timeouts, retries, a second address, plain words for every failure);
//! the updater plugin then reads the same address, downloads the installer and refuses one that
//! doesn't match the key built into the app. See docs/decisions/0009-updates-that-arrive.md.

pub mod feed;
#[cfg(test)]
mod mock;

use feed::{Channel, Source};
pub use feed::{Kind, Problem};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::ipc::Channel as Progress;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

/// No data for this long is a stalled download.
const STALL: Duration = Duration::from_secs(45);
/// One request's limit (the manifest; the download is watched for stalls instead).
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Serialize, Clone, Debug)]
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

/// What a check found.
#[derive(Serialize, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Checked {
    Current { current: String },
    Found { info: UpdateInfo },
}

/// The update the last check found, kept for the install (no second check in between).
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

/// How one check goes: where it asks, and how patiently.
pub struct Plan {
    pub sources: Vec<Source>,
    pub channel: Channel,
    pub waits: Vec<Duration>,
    pub timeout: Duration,
    pub proxy: Option<String>,
}

impl Plan {
    /// The installed app's: its channel and proxy from Settings; for a test copy, the address in
    /// VENUTA_UPDATE_URL.
    fn for_app() -> Self {
        let s = engine::settings::load();
        let channel = if s.update_channel.as_deref() == Some("beta") {
            Channel::Beta
        } else {
            Channel::Stable
        };
        let sources = match (crate::test_copy(), std::env::var("VENUTA_UPDATE_URL")) {
            (true, Ok(url)) => vec![Source::Manifest(url)],
            _ => feed::sources(channel, feed::PRIMARY, feed::RELEASES_API),
        };
        Plan {
            sources,
            channel,
            waits: feed::WAITS.to_vec(),
            timeout: TIMEOUT,
            proxy: s.proxy.as_deref().and_then(engine::settings::proxy_address),
        }
    }
}

/// The key the app trusts, from its configuration.
fn pubkey<R: Runtime>(app: &AppHandle<R>) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The updater for one manifest address: the channel's rule for "newer", the proxy, and on
/// Windows the installer's arguments: restart Venuta without the arguments it was started with
/// (started with Windows that is --in-tray, which would hide the new version), into the folder
/// Venuta runs from.
fn updater<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    plan: &Plan,
) -> Result<tauri_plugin_updater::Updater, Problem> {
    let channel = plan.channel;
    let address = url
        .parse()
        .map_err(|e| Problem::new(Kind::Broken, format!("its address: {e}")))?;
    let mut b = app
        .updater_builder()
        .timeout(plan.timeout)
        .version_comparator(move |current, release| {
            release.version > current
                && (channel == Channel::Beta || release.version.pre.is_empty())
        })
        .endpoints(vec![address])
        .map_err(|e| from_plugin(&e))?;
    if let Some(p) = plan.proxy.as_deref().and_then(|p| p.parse().ok()) {
        b = b.proxy(p);
    }
    if cfg!(windows) {
        b = b.installer_arg("/R");
        if let Some(d) = std::env::current_exe()
            .ok()
            .and_then(|e| feed::install_dir_arg(&e))
        {
            b = b.installer_arg(d);
        }
    }
    b.build().map_err(|e| from_plugin(&e))
}

/// Read the manifest, then let the updater read the same address and keep what it found.
pub async fn check_with<R: Runtime>(
    app: &AppHandle<R>,
    plan: &Plan,
) -> Result<Option<Update>, Problem> {
    let current = app.package_info().version.clone();
    let key = pubkey(app);
    let (sources, channel, waits, timeout, proxy) = (
        plan.sources.clone(),
        plan.channel,
        plan.waits.clone(),
        plan.timeout,
        plan.proxy.clone(),
    );
    let found = tauri::async_runtime::spawn_blocking(move || {
        let agent = feed::agent(timeout, proxy.as_deref());
        let fetch =
            |u: &str| feed::with_retries(&waits, &std::thread::sleep, || feed::get(&agent, u));
        feed::find(&sources, &fetch, &current, channel, &key)
    })
    .await
    .map_err(|e| Problem::new(Kind::Network, e.to_string()))??;
    let Some((url, _)) = found else {
        return Ok(None);
    };
    let mut tries = 0;
    loop {
        match updater(app, &url, plan)?.check().await {
            Ok(u) => return Ok(u),
            Err(e) => {
                let p = from_plugin(&e);
                if !p.transient() || tries >= plan.waits.len() {
                    return Err(p);
                }
                tokio::time::sleep(plan.waits[tries]).await;
                tries += 1;
            }
        }
    }
}

/// Download the installer (its signature checked by the updater), watched for stalls and tried
/// again after network trouble.
pub async fn download_with(
    update: &Update,
    stall: Duration,
    waits: &[Duration],
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<Vec<u8>, Problem> {
    let mut tries = 0;
    loop {
        match download_once(update, stall, progress).await {
            Err(p) if p.transient() && tries < waits.len() => {
                tokio::time::sleep(waits[tries]).await;
                tries += 1;
            }
            r => return r,
        }
    }
}

async fn download_once(
    update: &Update,
    stall: Duration,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<Vec<u8>, Problem> {
    let last = Mutex::new(Instant::now());
    let mut done = 0u64;
    let download = update.download(
        |chunk, total| {
            done += chunk as u64;
            *last.lock().unwrap() = Instant::now();
            progress(done, total);
        },
        || {},
    );
    tokio::pin!(download);
    loop {
        tokio::select! {
            r = &mut download => return r.map_err(|e| from_plugin(&e)),
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if last.lock().unwrap().elapsed() > stall {
                    return Err(Problem::new(Kind::Stalled, ""));
                }
            }
        }
    }
}

/// The updater's errors, in the same plain words as the manifest's.
fn from_plugin(e: &tauri_plugin_updater::Error) -> Problem {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(r) if r.is_timeout() => Problem::new(Kind::Timeout, ""),
        E::Reqwest(r) => Problem::new(Kind::Network, r.to_string()),
        E::Network(s) => match s.rsplit(": ").next().and_then(|c| c.get(..3)) {
            Some(c) if c.starts_with('5') || c == "429" || c == "403" => {
                Problem::new(Kind::Busy, format!("HTTP {c}"))
            }
            Some("404") => Problem::new(Kind::NotPublished, ""),
            _ => Problem::new(Kind::Network, s),
        },
        // The manifest was read a moment ago; not reading it now is the network's doing.
        E::ReleaseNotFound => Problem::new(Kind::Network, "GitHub answered with an error"),
        E::Minisign(_) | E::SignedVersionMismatch { .. } | E::MissingSignedVersion => {
            Problem::new(Kind::Signature, "")
        }
        E::Base64(_) | E::SignatureUtf8(_) => Problem::new(Kind::Broken, "its signature"),
        E::TargetNotFound(_) | E::TargetsNotFound(_) => Problem::new(Kind::NoWindows, ""),
        E::Serialization(_) | E::Semver(_) | E::UrlParse(_) => {
            Problem::new(Kind::Broken, e.to_string())
        }
        E::InvalidUpdaterFormat | E::BinaryNotFoundInArchive => {
            Problem::new(Kind::Broken, "the download isn't an installer")
        }
        other => Problem::new(Kind::Installer, other.to_string()),
    }
}

fn info(u: &Update) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    }
}

/// Is there a newer version? Local only keeps Venuta from asking, unless asked to anyway (a
/// click on "Check once anyway").
#[tauri::command]
pub async fn update_check(app: AppHandle, anyway: Option<bool>) -> Result<Checked, Problem> {
    if engine::net::is_offline() && !anyway.unwrap_or(false) {
        return Err(Problem::new(Kind::LocalOnly, ""));
    }
    let found = check_with(&app, &Plan::for_app()).await?;
    let checked = match &found {
        None => Checked::Current {
            current: app.package_info().version.to_string(),
        },
        Some(u) => Checked::Found { info: info(u) },
    };
    *app.state::<Pending>().0.lock().unwrap() = found;
    Ok(checked)
}

/// Download the new version, install it and start it. The installer closes Venuta itself.
/// VENUTA_UPDATE_DRY stops after the checked download.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    on_progress: Progress<UpdateProgress>,
) -> Result<(), Problem> {
    let pending = app.state::<Pending>().0.lock().unwrap().take();
    let update = match pending {
        Some(u) => u,
        None => check_with(&app, &Plan::for_app())
            .await?
            .ok_or_else(|| Problem::new(Kind::UpToDate, ""))?,
    };
    let keep = |u: Update| *app.state::<Pending>().0.lock().unwrap() = Some(u);
    // The installer runs without administrator rights: say so before downloading for nothing.
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        && !feed::writable(&dir)
    {
        keep(update);
        return Err(Problem::new(Kind::Folder, dir.display().to_string()));
    }
    let send = |done, total| {
        let _ = on_progress.send(UpdateProgress { done, total });
    };
    let bytes = match download_with(&update, STALL, &feed::WAITS, &send).await {
        Ok(b) => b,
        Err(p) => {
            keep(update);
            return Err(p);
        }
    };
    if std::env::var_os("VENUTA_UPDATE_DRY").is_some() {
        keep(update);
        return Ok(());
    }
    // Restarted by the installer's /R (see `updater`), without the start's arguments.
    let update = update.restart_after_install(false);
    update
        .install(bytes)
        .map_err(|e| Problem::new(Kind::Installer, e.to_string()))?;
    // Windows has quit Venuta for the installer by now; elsewhere, start again.
    app.restart();
}

/// The latest release's page, to download the installer by hand.
#[tauri::command]
pub fn update_download_page(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(feed::RELEASES_PAGE, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Started by the installer an update just ran: show the window even when the version updated
/// from passed on --in-tray (0.1.0 to 0.3.0 restart the new version with their own arguments).
pub fn just_installed(version: &str) -> bool {
    feed::just_installed(&std::env::temp_dir(), version, Duration::from_secs(600))
}

/// Installers earlier updates left in the temp folder, cleared at start.
pub fn clean_old_installers() {
    feed::clean_old_installers(&std::env::temp_dir(), Duration::from_secs(3600));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_errors_become_plain_words() {
        use tauri_plugin_updater::Error as E;
        let k = |e: E| from_plugin(&e).kind;
        let status = |s: &str| E::Network(format!("Download request failed with status: {s}"));
        assert_eq!(k(status("503 Service Unavailable")), Kind::Busy);
        assert_eq!(k(status("404 Not Found")), Kind::NotPublished);
        assert_eq!(
            k(E::TargetsNotFound(vec!["windows-x86_64".into()])),
            Kind::NoWindows
        );
        let mismatch = E::SignedVersionMismatch {
            signed: "1".into(),
            announced: "2".into(),
        };
        assert_eq!(k(mismatch), Kind::Signature);
        assert_eq!(k(E::ReleaseNotFound), Kind::Network);
        assert_eq!(k(E::Io(std::io::Error::other("denied"))), Kind::Installer);
    }
}
