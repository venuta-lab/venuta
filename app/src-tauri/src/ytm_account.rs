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

/// How often the cookies in use are compared with the ones kept. A compare of two strings in
/// memory, no request: Google renews the cookies on the catalog's own answers.
const RENEW_EVERY: Duration = Duration::from_secs(15);

/// The cookies the vault holds for the sign-in in use, as this run last wrote or read them.
/// One lock for "compare, then keep" and for every sign-in, switch and sign-out, so a renewal
/// can never put back cookies a sign-out has just taken away.
static KEPT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn kept() -> std::sync::MutexGuard<'static, Option<String>> {
    KEPT.lock().unwrap_or_else(|e| e.into_inner())
}

/// The cookies in use, renewed by Google: kept when they differ from what the vault holds, and
/// in the slot of the account in use too (for switching back). Signed out keeps nothing. True
/// when something was written.
fn keep_renewed(
    v: &dyn Vault,
    kept: &mut Option<String>,
    now: Option<String>,
) -> Result<bool, String> {
    let Some(now) = now else {
        return Ok(false);
    };
    if kept.as_deref() == Some(now.as_str()) {
        return Ok(false);
    }
    v.put(VAULT_KEY, &now)?;
    let _ = crate::ytm_accounts::keep_active(v, &now);
    *kept = Some(now);
    Ok(true)
}

/// A sign-in that just worked, kept (None in `kept` when the vault refused it, so the renewal
/// tries again). No kept account is the one in use until it says who it is.
fn keep_sign_in(v: &dyn Vault, kept: &mut Option<String>, jar: &str) -> Result<(), String> {
    crate::ytm_accounts::clear_active(v);
    *kept = None;
    v.put(VAULT_KEY, jar)?;
    *kept = Some(jar.to_owned());
    Ok(())
}

/// Sign out of what is kept: the sign-in in use and its slot. Every part is tried even when
/// Windows refuses one; what it refused, in words.
fn forget_sign_in(v: &dyn Vault, kept: &mut Option<String>) -> Vec<String> {
    *kept = None;
    let mut problems = Vec::new();
    if let Err(e) = v.delete(VAULT_KEY) {
        problems.push(e);
    }
    // Only this account is forgotten; others kept stay there to switch to.
    if let Some(id) = crate::ytm_accounts::active(v)
        && let Err(e) = crate::ytm_accounts::forget(v, &id)
    {
        problems.push(e);
    }
    problems
}

/// At startup: a kept sign-in is used again, and kept current while Venuta runs, whenever it
/// was made (at start or later).
pub fn restore(app: &AppHandle) {
    let pb = app.state::<Playback>();
    {
        let mut kept = kept();
        if let Ok(Some(cookie)) = SystemVault.get(VAULT_KEY) {
            pb.online_catalog().set_account_cookie(Some(&cookie));
            crate::ytm_library::restore_identity(&pb);
            *kept = Some(cookie);
        }
    }
    // Google renews the cookies as the catalog answers: what is kept follows them (tried again
    // next time if it couldn't be kept, and said once).
    let catalog = pb.online_catalog();
    let app = app.clone();
    std::thread::Builder::new()
        .name("venuta-ytm-account".into())
        .spawn(move || {
            let mut said = false;
            loop {
                std::thread::sleep(RENEW_EVERY);
                let mut kept = kept();
                // Read under the lock: a sign-out in between is either all before or all after.
                match keep_renewed(&SystemVault, &mut kept, catalog.account_cookie()) {
                    Ok(_) => {}
                    Err(e) if !said => {
                        said = true;
                        let _ = app.emit("ytm-account-error", e);
                    }
                    Err(_) => {}
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
            let catalog = pb.online_catalog();
            // Signed in either way; if it can't be kept, only for as long as Venuta is open (the
            // renewal tries keeping it again).
            let kept = {
                let mut kept = kept();
                let done = keep_sign_in(&SystemVault, &mut kept, &jar);
                catalog.set_account_cookie(Some(&jar));
                done
            };
            // A new sign-in starts as itself, not as the brand account chosen under the last one.
            crate::ytm_library::forget_identity(&pb);
            // Kept on its own too, so you can switch back to it later: as the cookies are now,
            // since the answer may have renewed them. If the account doesn't say who it is, no
            // slot is the one in use, and Settings asks again when it lists the accounts.
            if let Ok(Some(who)) = catalog.account(&CancellationToken::default()) {
                let jar = catalog.account_cookie().unwrap_or_else(|| jar.clone());
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
    // Signed out in memory first, whatever Windows then says about the kept copy, and under the
    // same lock as the renewal, so it can't keep the cookies again a moment later.
    let problems = {
        let mut kept = kept();
        pb.online_catalog().set_account_cookie(None);
        forget_sign_in(&SystemVault, &mut kept)
    };
    crate::ytm_library::forget_identity(&pb);
    clear_web_session(&app);
    let _ = app.emit("ytm-account-changed", ());
    if !problems.is_empty() {
        let _ = app.emit(
            "ytm-account-error",
            format!(
                "Signed out. Windows didn't let Venuta forget the kept sign-in, so it may be back at the next start. {}",
                problems.join(" ")
            ),
        );
    }
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
    let kept_now = {
        let mut kept = kept();
        let now = catalog.account_cookie();
        let jar = crate::ytm_accounts::switch(&SystemVault, &id, now.as_deref())?;
        // Switched either way (the slot in use is this one now); if it can't be kept as the one
        // in use, only until Venuta closes, and the renewal tries again.
        let done = SystemVault.put(VAULT_KEY, &jar);
        catalog.set_account_cookie(Some(&jar));
        *kept = done.is_ok().then_some(jar);
        done
    };
    // Its own channel to start with; a brand account is chosen again under it.
    crate::ytm_library::forget_identity(&pb);
    let _ = app.emit("ytm-account-changed", ());
    if let Err(e) = kept_now {
        let _ = app.emit(
            "ytm-account-error",
            format!("Switched until Venuta closes. {e}"),
        );
    }
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

/// Rate an online song on the account (Settings: like on YouTube Music too). False when no one
/// is signed in, so nothing was sent.
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

    use crate::ytm_accounts::tests::Memory;

    fn who(email: &str) -> RemoteAccount {
        RemoteAccount {
            name: None,
            email: Some(email.into()),
            handle: None,
            picture: None,
        }
    }

    /// A vault that refuses to forget one key (Credential Manager saying no).
    struct Stubborn(Memory, &'static str);
    impl Vault for Stubborn {
        fn put(&self, k: &str, v: &str) -> Result<(), String> {
            self.0.put(k, v)
        }
        fn get(&self, k: &str) -> Result<Option<String>, String> {
            self.0.get(k)
        }
        fn delete(&self, k: &str) -> Result<(), String> {
            if k == self.1 {
                return Err("Access is denied.".into());
            }
            self.0.delete(k)
        }
    }

    // The bug: with nothing kept at start, a sign-in made while Venuta ran never had its renewed
    // cookies kept, so the next start brought back the stale ones.
    #[test]
    fn cookies_renewed_after_a_sign_in_made_while_running_are_kept() {
        let v = Memory::default();
        let mut kept = None;
        keep_sign_in(&v, &mut kept, "SAPISID=a; TS=1").unwrap();
        crate::ytm_accounts::remember(&v, &who("a@x.com"), "SAPISID=a; TS=1").unwrap();

        assert!(keep_renewed(&v, &mut kept, Some("SAPISID=a; TS=2".into())).unwrap());
        assert_eq!(
            v.get(VAULT_KEY).unwrap().as_deref(),
            Some("SAPISID=a; TS=2")
        );
        let slot = crate::ytm_accounts::switch(&v, "a@x.com", None).unwrap();
        assert_eq!(slot, "SAPISID=a; TS=2", "its own slot follows too");
        // The same again writes nothing.
        assert!(!keep_renewed(&v, &mut kept, Some("SAPISID=a; TS=2".into())).unwrap());
    }

    #[test]
    fn cookies_renewed_after_a_kept_sign_in_restored_at_start_are_kept() {
        let v = Memory::default();
        v.put(VAULT_KEY, "SAPISID=a; TS=1").unwrap();
        let mut kept = v.get(VAULT_KEY).unwrap();
        assert!(!keep_renewed(&v, &mut kept, Some("SAPISID=a; TS=1".into())).unwrap());
        assert!(keep_renewed(&v, &mut kept, Some("SAPISID=a; TS=2".into())).unwrap());
        assert_eq!(
            v.get(VAULT_KEY).unwrap().as_deref(),
            Some("SAPISID=a; TS=2")
        );
    }

    // After a sign-out the catalog holds no cookie; the renewal never puts one back.
    #[test]
    fn after_a_sign_out_nothing_is_kept_again() {
        let v = Memory::default();
        let mut kept = None;
        keep_sign_in(&v, &mut kept, "SAPISID=a").unwrap();
        crate::ytm_accounts::remember(&v, &who("a@x.com"), "SAPISID=a").unwrap();
        assert!(forget_sign_in(&v, &mut kept).is_empty());
        assert!(!keep_renewed(&v, &mut kept, None).unwrap());
        assert_eq!(v.get(VAULT_KEY).unwrap(), None);
        assert!(crate::ytm_accounts::list(&v).is_empty());
    }

    #[test]
    fn a_refused_delete_does_not_stop_the_sign_out() {
        let v = Stubborn(Memory::default(), VAULT_KEY);
        let mut kept = None;
        keep_sign_in(&v, &mut kept, "SAPISID=a").unwrap();
        crate::ytm_accounts::remember(&v, &who("a@x.com"), "SAPISID=a").unwrap();
        let problems = forget_sign_in(&v, &mut kept);
        assert_eq!(problems, ["Access is denied."]);
        assert_eq!(kept, None);
        // The rest went anyway.
        assert!(crate::ytm_accounts::list(&v).is_empty());
        assert_eq!(crate::ytm_accounts::active(&v), None);
    }

    // The account check right after a sign-in failed: the new cookies must not land in the slot
    // of the account used before.
    #[test]
    fn a_sign_in_not_known_yet_keeps_nothing_in_the_last_accounts_slot() {
        let v = Memory::default();
        let mut kept = None;
        crate::ytm_accounts::remember(&v, &who("a@x.com"), "SAPISID=a").unwrap();
        keep_sign_in(&v, &mut kept, "SAPISID=b").unwrap();
        keep_renewed(&v, &mut kept, Some("SAPISID=b; TS=2".into())).unwrap();
        let a = crate::ytm_accounts::switch(&v, "a@x.com", None).unwrap();
        assert_eq!(a, "SAPISID=a");
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
