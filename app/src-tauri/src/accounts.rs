//! Signed-in services for the app (Spotify): connect through the system browser, keep the
//! sign-in in the credential store, and read playlists for import. See
//! docs/decisions/0005-accounts-oauth.md.

use engine::accounts::spotify::{self, Api, ApiError, SpotifyPlaylist};
use engine::accounts::{self, Saved, SystemVault, oauth};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

const SPOTIFY: &str = "spotify";

/// Spotify's sign-in, or for a test copy a stand-in (VENUTA_SPOTIFY_ACCOUNTS = its base).
fn provider() -> oauth::Provider {
    let mut p = spotify::provider();
    if let Ok(base) = std::env::var("VENUTA_SPOTIFY_ACCOUNTS") {
        p.authorize_url = format!("{base}/authorize");
        p.token_url = format!("{base}/api/token");
    }
    p
}

/// Spotify's API, or for a test copy a stand-in (VENUTA_SPOTIFY_API).
fn api(token: &str) -> Api {
    match std::env::var("VENUTA_SPOTIFY_API") {
        Ok(base) => Api::with_base(token, &base),
        Err(_) => Api::new(token),
    }
}

/// The access token in memory (never stored), and a way to cancel a sign-in under way.
#[derive(Default, Clone)]
pub struct Accounts {
    spotify: Arc<Mutex<Option<(String, u64)>>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Serialize)]
pub struct AccountStatus {
    /// A Client ID is set in Settings.
    pub ready: bool,
    pub connected: bool,
    pub name: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn client_id() -> Result<String, String> {
    engine::settings::load()
        .spotify_client_id
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "Add your Spotify Client ID in Settings, Online sources, first.".into())
}

fn saved(client_id: &str) -> Result<Option<Saved>, String> {
    accounts::load(&SystemVault, SPOTIFY, client_id)
}

#[tauri::command]
pub fn spotify_status() -> Result<AccountStatus, String> {
    let Ok(id) = client_id() else {
        return Ok(AccountStatus {
            ready: false,
            connected: false,
            name: None,
        });
    };
    let s = saved(&id)?;
    Ok(AccountStatus {
        ready: true,
        connected: s.is_some(),
        name: s.map(|s| s.name),
    })
}

/// Sign in: open Spotify's page in the browser, wait for the answer (five minutes at most, or
/// until cancelled), trade it for tokens, and keep the refresh token in the credential store.
#[tauri::command]
pub async fn spotify_connect(app: AppHandle, acc: State<'_, Accounts>) -> Result<String, String> {
    let id = client_id()?;
    let p = provider();
    let pending = oauth::begin(&p, &id)?;
    acc.cancel.store(false, Ordering::Relaxed);
    if std::env::var_os("VENUTA_SPOTIFY_ACCOUNTS").is_some() {
        // A test copy: the stand-in agrees at once; follow its redirect without a browser.
        let url = pending.authorize_url.clone();
        std::thread::spawn(move || {
            let _ = ureq::get(&url).call();
        });
    } else {
        app.opener()
            .open_url(&pending.authorize_url, None::<&str>)
            .map_err(|e| format!("Couldn't open the browser: {e}"))?;
    }
    let acc = acc.inner().clone();
    crate::blocking(move || {
        let code = oauth::wait(&pending, Duration::from_secs(300), &acc.cancel)?;
        let t = oauth::exchange(&p, &id, &pending, &code)?;
        let (user_id, name) = api(&t.access_token).me().map_err(|e| e.to_string())?;
        let refresh_token = t
            .refresh_token
            .clone()
            .ok_or("Spotify gave no refresh token.")?;
        accounts::save(
            &SystemVault,
            SPOTIFY,
            &Saved {
                refresh_token,
                user_id,
                name: name.clone(),
                client_id: id,
            },
        )?;
        *acc.spotify.lock().map_err(|e| e.to_string())? = Some((t.access_token, t.expires_at));
        Ok(name)
    })
    .await
}

/// Stop a sign-in that is waiting for the browser.
#[tauri::command]
pub fn spotify_cancel(acc: State<'_, Accounts>) {
    acc.cancel.store(true, Ordering::Relaxed);
}

/// Forget the sign-in on this computer. (Spotify has no way to revoke it from here: the app
/// can be removed at spotify.com/account/apps.)
#[tauri::command]
pub fn spotify_disconnect(acc: State<'_, Accounts>) -> Result<(), String> {
    if let Ok(mut t) = acc.spotify.lock() {
        *t = None;
    }
    accounts::forget(&SystemVault, SPOTIFY)
}

/// A working access token: the one in memory, else a fresh one from the refresh token (which
/// Spotify may replace; the new one is kept).
fn access(acc: &Accounts, fresh: bool) -> Result<(String, Saved), String> {
    let id = client_id()?;
    let s = saved(&id)?.ok_or("Sign in to Spotify first.")?;
    if !fresh
        && let Ok(t) = acc.spotify.lock()
        && let Some((token, until)) = t.as_ref()
        && *until > now()
    {
        return Ok((token.clone(), s));
    }
    let t = match oauth::refresh(&provider(), &id, &s.refresh_token) {
        Ok(t) => t,
        Err(e) if e.contains("invalid_grant") || e.to_lowercase().contains("revoked") => {
            // The sign-in no longer works (removed at Spotify, or expired): start over.
            let _ = accounts::forget(&SystemVault, SPOTIFY);
            return Err("Spotify needs you to sign in again.".into());
        }
        Err(e) => return Err(e),
    };
    let mut s = s;
    if let Some(r) = t.refresh_token.as_ref().filter(|r| **r != s.refresh_token) {
        s.refresh_token = r.clone();
        accounts::save(&SystemVault, SPOTIFY, &s)?;
    }
    if let Ok(mut m) = acc.spotify.lock() {
        *m = Some((t.access_token.clone(), t.expires_at));
    }
    Ok((t.access_token, s))
}

/// Run Spotify requests; if the token was refused, refresh once and try again.
fn with_api<T>(
    acc: &Accounts,
    f: impl Fn(&Api, &Saved) -> Result<T, ApiError>,
) -> Result<T, String> {
    let (token, s) = access(acc, false)?;
    match f(&api(&token), &s) {
        Err(ApiError::Unauthorized) => {
            let (token, s) = access(acc, true)?;
            f(&api(&token), &s).map_err(|e| e.to_string())
        }
        other => other.map_err(|e| e.to_string()),
    }
}

/// One Spotify playlist read again by its id (to update the playlist made from it).
pub fn read_one(
    acc: &Accounts,
    id: &str,
    name: &str,
) -> Result<Vec<engine::imports::ForeignPlaylist>, String> {
    let one = SpotifyPlaylist {
        id: id.to_owned(),
        name: name.to_owned(),
        owner: String::new(),
        songs: 0,
        readable: true,
        image: None,
    };
    with_api(acc, |api, _| api.read(std::slice::from_ref(&one)))
}

/// Your Spotify playlists (Liked Songs first), and which of them Spotify will share.
#[tauri::command]
pub async fn spotify_playlists(acc: State<'_, Accounts>) -> Result<Vec<SpotifyPlaylist>, String> {
    let acc = acc.inner().clone();
    crate::blocking(move || with_api(&acc, |api, s| api.playlists(&s.user_id))).await
}

/// Read the chosen playlists and match them, the same as an import from files.
#[tauri::command]
pub async fn spotify_read(
    root: String,
    chosen: Vec<SpotifyPlaylist>,
    acc: State<'_, Accounts>,
) -> Result<crate::ImportRead, String> {
    let acc = acc.inner().clone();
    crate::blocking(move || {
        let (readable, held): (Vec<SpotifyPlaylist>, Vec<SpotifyPlaylist>) =
            chosen.into_iter().partition(|p| p.readable);
        let problems = held
            .iter()
            .map(|p| {
                format!(
                    "Spotify doesn't share the songs of “{}” ({}'s playlist). Bring it over with an Exportify or account-data file instead.",
                    p.name, p.owner
                )
            })
            .collect();
        let lists = with_api(&acc, |api, _| api.read(&readable))?;
        let rootp = PathBuf::from(&root);
        let (_, lib) = crate::load_lib(&rootp)?;
        Ok(crate::ImportRead {
            lists: engine::imports::preview(&rootp, &lib, &lists),
            problems,
        })
    })
    .await
}
