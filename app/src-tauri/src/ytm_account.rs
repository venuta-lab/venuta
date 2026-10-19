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
use media_access::{CancellationToken, RemoteAccount, RemoteCollection, RemoteRating};
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
        pb.online_catalog().set_account_cookie(Some(&cookie));
        crate::ytm_library::restore_identity(&pb);
    }
    // Google renews the cookies as the catalog answers: what is kept follows them (tried again
    // next time if it couldn't be kept, and said once).
    let catalog = pb.online_catalog();
    let app = app.clone();
    std::thread::Builder::new()
        .name("venuta-ytm-account".into())
        .spawn(move || {
            let mut kept = SystemVault.get(VAULT_KEY).ok().flatten();
            let mut said = false;
            loop {
                std::thread::sleep(Duration::from_secs(300));
                let now = catalog.account_cookie();
                if kept.is_some() && now.is_some() && now != kept {
                    let Some(c) = &now else { continue };
                    match SystemVault.put(VAULT_KEY, c) {
                        Ok(()) => {
                            // The slot of the account in use follows too (for switching back).
                            let _ = crate::ytm_accounts::keep_active(&SystemVault, c);
                            kept = now
                        }
                        Err(e) if !said => {
                            said = true;
                            let _ = app.emit("ytm-account-error", e);
                        }
                        Err(_) => {}
                    }
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
            // Signed in either way; if it can't be kept, only for as long as Venuta is open.
            let kept = SystemVault.put(VAULT_KEY, &jar);
            pb.online_catalog().set_account_cookie(Some(&jar));
            // A new sign-in starts as itself, not as the brand account chosen under the last one.
            crate::ytm_library::forget_identity();
            // Kept on its own too, so you can switch back to it later.
            if let Ok(Some(who)) = pb.online_catalog().account(&CancellationToken::default()) {
                let _ = crate::ytm_accounts::remember(&SystemVault, &who, &jar);
            }
            if let Some(w) = app.get_webview_window(WINDOW) {
                let _ = w.close();
            }
            let _ = app.emit("ytm-account-changed", ());
            if let Err(e) = kept {
                let _ = app.emit(
                    "ytm-account-error",
                    format!("Signed in until Venuta closes. {e}"),
                );
            }
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
    pb.online_catalog().set_account_cookie(None);
    crate::ytm_library::forget_identity();
    // Only this account is forgotten; others kept stay there to switch to.
    if let Some(id) = crate::ytm_accounts::active(&SystemVault) {
        let _ = crate::ytm_accounts::forget(&SystemVault, &id);
    }
    clear_web_session(&app);
    let _ = app.emit("ytm-account-changed", ());
    Ok(())
}

/// The sign-in window's Google session goes (so the next sign-in starts fresh). Your kept
/// sign-ins are not touched.
fn clear_web_session(app: &AppHandle) {
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
}

/// The sign-ins kept, the one in use marked. One signed in before these were kept is added the
/// first time it is listed.
#[tauri::command]
pub async fn ytm_saved_accounts(
    pb: State<'_, Playback>,
) -> Result<Vec<crate::ytm_accounts::SavedAccount>, String> {
    let catalog = pb.online_catalog();
    crate::blocking(move || {
        if crate::ytm_accounts::active(&SystemVault).is_none()
            && let Some(jar) = catalog.account_cookie()
            && let Ok(Some(who)) = catalog.account(&CancellationToken::default())
        {
            let _ = crate::ytm_accounts::remember(&SystemVault, &who, &jar);
        }
        Ok(crate::ytm_accounts::list(&SystemVault))
    })
    .await
}

/// Add another Google account: the one in use stays kept, the sign-in window starts fresh, and
/// the new sign-in becomes the one in use. Closing the window changes nothing.
#[tauri::command]
pub async fn ytm_add_account(app: AppHandle, pb: State<'_, Playback>) -> Result<(), String> {
    if let Some(jar) = pb.online_catalog().account_cookie() {
        crate::ytm_accounts::keep_active(&SystemVault, &jar)?;
    }
    clear_web_session(&app);
    ytm_sign_in(app).await
}

/// Use another kept sign-in.
#[tauri::command]
pub async fn ytm_switch_account(
    app: AppHandle,
    pb: State<'_, Playback>,
    id: String,
) -> Result<(), String> {
    let catalog = pb.online_catalog();
    let now = catalog.account_cookie();
    let jar = crate::ytm_accounts::switch(&SystemVault, &id, now.as_deref())?;
    SystemVault.put(VAULT_KEY, &jar)?;
    catalog.set_account_cookie(Some(&jar));
    // Its own channel to start with; a brand account is chosen again under it.
    crate::ytm_library::forget_identity();
    let _ = app.emit("ytm-account-changed", ());
    Ok(())
}

/// Forget a kept sign-in that isn't in use.
#[tauri::command]
pub fn ytm_forget_account(id: String) -> Result<(), String> {
    if crate::ytm_accounts::active(&SystemVault).as_deref() == Some(id.as_str()) {
        return Err("That's the account in use. Sign out of it instead.".into());
    }
    crate::ytm_accounts::forget(&SystemVault, &id)
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

/// Rate an online song on the account (Likes and dislikes, the account in use's own switch).
/// False when no one is signed in or the switch is off for that account: nothing was sent.
#[tauri::command]
pub async fn ytm_rate(
    pb: State<'_, Playback>,
    track_id: String,
    rating: RemoteRating,
) -> Result<bool, String> {
    let access = pb.catalog();
    if access.account_cookie().is_none() {
        return Ok(false);
    }
    tauri::async_runtime::spawn_blocking(move || {
        // The window asks only with the switch on; said again here, so nothing reaches the
        // account on a stale or wrong account's choice.
        if !crate::ytm_choices::rates_on_account(access.as_ref()) {
            return Ok(false);
        }
        access
            .rate_on_account(&track_id, rating, &CancellationToken::default())
            .map(|()| true)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What bringing in your YouTube Music likes did.
#[derive(serde::Serialize)]
pub struct BroughtIn {
    /// Your Liked songs list in Venuta.
    pub name: Option<String>,
    /// Songs in the account's Liked Music.
    pub found: usize,
    /// Of those, the ones Venuta didn't have as liked yet.
    pub added: usize,
}

/// Your YouTube Music likes into Venuta's Liked songs, newest first; the ones liked already stay
/// where they are. Only when you ask (Settings, Accounts).
#[tauri::command]
pub async fn ytm_bring_in_likes(
    pb: State<'_, Playback>,
    root: String,
) -> Result<BroughtIn, String> {
    let access = pb.catalog();
    if access.account_cookie().is_none() {
        return Err("Sign in to YouTube Music first.".into());
    }
    crate::blocking(move || {
        let tracks = access
            .account_liked(&CancellationToken::default())
            .map_err(|e| e.to_string())?;
        let rootp = std::path::PathBuf::from(&root);
        let (_, lib) = crate::load_lib(&rootp)?;
        let items: Vec<engine::playlists::Item> = tracks
            .iter()
            .map(|t| {
                engine::playlists::Item::online(engine::playlists::OnlineEntry::from_remote(t))
            })
            .collect();
        if items.is_empty() {
            return Ok(BroughtIn {
                name: engine::playlists::liked_name(&rootp),
                found: 0,
                added: 0,
            });
        }
        let (name, added) = engine::playlists::like_many(&rootp, &lib, &items)?;
        Ok(BroughtIn {
            name: Some(name),
            found: items.len(),
            added,
        })
    })
    .await
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
