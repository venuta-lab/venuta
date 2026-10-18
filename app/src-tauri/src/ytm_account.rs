//! Signing in to YouTube Music, so the online catalog answers for your account: your Home,
//! your search, your radios, and your playlists and Liked Music in Library.
//!
//! You sign in in Google's own page, in a window of its own; Venuta never sees what you type.
//! When that window comes back to music.youtube.com, its YouTube cookies are what signed-in
//! requests carry. They are kept in the system's credential store (like Spotify's sign-in),
//! never in a file, and kept current as Google renews them. Signing out forgets them and signs
//! the window out of Google.

use crate::playback::Playback;
use engine::accounts::{SystemVault, Vault};
use media_access::{CancellationToken, RemoteAccount, RemoteCollection};
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

const VAULT_KEY: &str = "account:youtube-music";
const WINDOW: &str = "ytm-sign-in";
/// Google's sign-in, back to YouTube Music when done (the sign that it worked).
const SIGN_IN_URL: &str =
    "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/";

/// The YouTube cookies of a cookie jar, as one header (the most specific domain wins a name).
fn youtube_cookies(mut cookies: Vec<tauri::webview::Cookie<'static>>) -> String {
    cookies.sort_by_key(|c| c.domain().unwrap_or_default().len());
    let mut jar = std::collections::BTreeMap::new();
    for c in cookies {
        let domain = c.domain().unwrap_or_default().trim_start_matches('.');
        if domain == "youtube.com" || domain.ends_with(".youtube.com") {
            jar.insert(c.name().to_string(), c.value().to_string());
        }
    }
    jar.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// At startup: a kept sign-in is used again, and kept current while Venuta runs.
pub fn restore(app: &AppHandle) {
    let pb = app.state::<Playback>();
    if let Ok(Some(cookie)) = SystemVault.get(VAULT_KEY) {
        pb.catalog().set_account_cookie(Some(&cookie));
    }
    // Google renews the cookies as the catalog answers: what is kept follows them.
    let catalog = pb.catalog();
    std::thread::Builder::new()
        .name("venuta-ytm-account".into())
        .spawn(move || {
            let mut kept = SystemVault.get(VAULT_KEY).ok().flatten();
            loop {
                std::thread::sleep(Duration::from_secs(300));
                let now = catalog.account_cookie();
                if kept.is_some() && now.is_some() && now != kept {
                    if let Some(c) = &now {
                        let _ = SystemVault.put(VAULT_KEY, c);
                    }
                    kept = now;
                }
            }
        })
        .ok();
}

/// Open Google's sign-in. Returns at once; the window says `ytm-account-changed` when done.
/// Async on purpose: a window made from a command that runs on the main thread never opens.
#[tauri::command]
pub async fn ytm_sign_in(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(WINDOW) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = tauri::Url::parse(SIGN_IN_URL).map_err(|e| e.to_string())?;
    let done = app.clone();
    WebviewWindowBuilder::new(&app, WINDOW, WebviewUrl::External(url))
        .title("Sign in to YouTube Music")
        .inner_size(480.0, 720.0)
        .on_page_load(move |_w, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished)
                && payload.url().host_str() == Some("music.youtube.com")
            {
                let app = done.clone();
                // Read off the page-load callback (reading cookies inside it can deadlock).
                std::thread::spawn(move || finish_sign_in(app));
            }
        })
        .build()
        .map_err(|e| format!("Couldn't open the sign-in window: {e}"))?;
    Ok(())
}

fn finish_sign_in(app: AppHandle) {
    // The cookies arrive a beat after the page: look a few times.
    for _ in 0..8 {
        let (tx, rx) = std::sync::mpsc::channel();
        let a = app.clone();
        let _ = app.run_on_main_thread(move || {
            let jar = a
                .get_webview_window(WINDOW)
                .and_then(|w| w.cookies().ok())
                .map(youtube_cookies)
                .unwrap_or_default();
            let _ = tx.send(jar);
        });
        let jar = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        if ytmusic_signed_in(&jar) {
            let pb = app.state::<Playback>();
            if let Err(e) = SystemVault.put(VAULT_KEY, &jar) {
                let _ = app.emit("ytm-account-error", e);
                return;
            }
            pb.catalog().set_account_cookie(Some(&jar));
            if let Some(w) = app.get_webview_window(WINDOW) {
                let _ = w.close();
            }
            let _ = app.emit("ytm-account-changed", ());
            return;
        }
        std::thread::sleep(Duration::from_millis(600));
    }
    // Back on YouTube Music but not signed in yet (a step of Google's): keep waiting.
}

/// A cookie jar that signed-in requests can use (it holds the session's SAPISID).
fn ytmusic_signed_in(jar: &str) -> bool {
    jar.split(';').any(|kv| {
        let kv = kv.trim();
        (kv.starts_with("SAPISID=") || kv.starts_with("__Secure-3PAPISID=")) && kv.len() > 9
    })
}

/// Sign out: the kept sign-in is forgotten, and the sign-in window's Google session too (so the
/// next sign-in asks again).
#[tauri::command]
pub async fn ytm_sign_out(app: AppHandle, pb: State<'_, Playback>) -> Result<(), String> {
    SystemVault.delete(VAULT_KEY)?;
    pb.catalog().set_account_cookie(None);
    if let Some(main) = app.get_webview_window("main")
        && let Ok(cookies) = main.cookies()
    {
        for c in cookies {
            let domain = c
                .domain()
                .unwrap_or_default()
                .trim_start_matches('.')
                .to_owned();
            let google = domain == "google.com" || domain.ends_with(".google.com");
            let youtube = domain == "youtube.com" || domain.ends_with(".youtube.com");
            if google || youtube {
                let _ = main.delete_cookie(c);
            }
        }
    }
    let _ = app.emit("ytm-account-changed", ());
    Ok(())
}

/// Who is signed in to YouTube Music, or None.
#[tauri::command]
pub async fn ytm_account(pb: State<'_, Playback>) -> Result<Option<RemoteAccount>, String> {
    let access = pb.catalog();
    if access.account_cookie().is_none() {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || {
        access
            .account(&CancellationToken::default())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Your YouTube Music playlists (empty when signed out).
#[tauri::command]
pub async fn ytm_playlists(pb: State<'_, Playback>) -> Result<Vec<RemoteCollection>, String> {
    let access = pb.catalog();
    if access.account_cookie().is_none() {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(move || {
        access
            .account_playlists(&CancellationToken::default())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::webview::Cookie;

    fn cookie(s: &str) -> Cookie<'static> {
        Cookie::parse(s.to_string()).unwrap()
    }

    #[test]
    fn the_youtube_cookies_are_kept_and_the_rest_left_out() {
        let jar = youtube_cookies(vec![
            cookie("SAPISID=abc; Domain=.youtube.com"),
            cookie("SID=def; Domain=.youtube.com"),
            cookie("PREF=broad; Domain=.youtube.com"),
            cookie("PREF=specific; Domain=music.youtube.com"),
            cookie("SAPISID=notthis; Domain=.google.com"),
        ]);
        assert_eq!(jar, "PREF=specific; SAPISID=abc; SID=def");
        assert!(ytmusic_signed_in(&jar));
        assert!(!ytmusic_signed_in("PREF=x; VISITOR_INFO1_LIVE=y"));
    }
}
