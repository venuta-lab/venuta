//! Where YouTube's security check (BotGuard) runs: a hidden window of Venuta's own, never shown,
//! opened the first time a token is needed (Count plays, a play to tell) and kept while Venuta
//! runs, so a token for each song is a few milliseconds.
//!
//! The window is private (InPrivate): it has no cookies, never the sign-in's. It opens one tiny
//! page of YouTube's (`robots.txt`, signed out) so the program runs on YouTube's own origin as it
//! does on YouTube's page: run anywhere else (`about:blank`) the attestation answers with no
//! integrity token at all (measured 2026-10-19). Nothing in it may navigate anywhere else. The
//! catalog fetches the challenge, Google's interpreter script and the attestation
//! (`ytmusic::potoken`, signed out); this window only runs the JavaScript: the program, its
//! snapshot, and the minter made from the integrity token. Each new session opens a fresh window,
//! so nothing of an old program lingers.
//!
//! The page side is `botguard.js`. A call is a numbered job; its outcome is read back by asking
//! the page for it every few milliseconds (WebView2 can't hand back a promise).

use media_access::potoken::{BotGuardVm, Challenge, VmFuture};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The page side, put into the page once it has loaded.
const BRIDGE: &str = include_str!("botguard.js");
/// Every window of it is named this, then its number.
const LABEL: &str = "ytm-botguard";
/// The page the program runs on.
const PAGE: &str = "https://www.youtube.com/robots.txt";
/// How often a running job is asked about.
const POLL: Duration = Duration::from_millis(15);
/// The page loading.
const LOAD_TIMEOUT: Duration = Duration::from_secs(15);
/// The longest any one job may take (the page side times its own steps out sooner).
const JOB_TIMEOUT: Duration = Duration::from_secs(40);

/// The hidden window BotGuard runs in.
pub struct WebviewBotGuard {
    app: AppHandle,
    /// A data folder of its own (the dev probe only). None: the app's, in private mode.
    data_directory: Option<PathBuf>,
    jobs: AtomicU64,
    windows: AtomicU64,
    window: Mutex<Option<WebviewWindow>>,
}

impl WebviewBotGuard {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            data_directory: None,
            jobs: AtomicU64::new(1),
            windows: AtomicU64::new(1),
            window: Mutex::new(None),
        }
    }

    /// Its windows keep their browser data in `dir` (for a check run beside the app).
    pub fn with_data_directory(mut self, dir: PathBuf) -> Self {
        self.data_directory = Some(dir);
        self
    }

    fn current(&self) -> Option<WebviewWindow> {
        self.window
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// A fresh window with the page side in it: any earlier one closed first.
    async fn fresh_window(&self) -> Result<WebviewWindow, String> {
        if let Some(old) = self.window.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = old.destroy();
        }
        let label = format!("{LABEL}-{}", self.windows.fetch_add(1, Ordering::Relaxed));
        let url = tauri::Url::parse(PAGE).map_err(|e| e.to_string())?;
        let (loaded_tx, loaded_rx) = tokio::sync::oneshot::channel::<()>();
        let loaded_tx = Mutex::new(Some(loaded_tx));
        let mut builder = WebviewWindowBuilder::new(&self.app, &label, WebviewUrl::External(url))
            .title("Venuta")
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .incognito(true)
            .inner_size(320.0, 240.0)
            // Nothing in it may open or go anywhere else.
            .on_navigation(|url| url.as_str() == PAGE)
            .on_page_load(move |_w, payload| {
                if matches!(payload.event(), PageLoadEvent::Finished)
                    && let Some(tx) = loaded_tx.lock().ok().and_then(|mut t| t.take())
                {
                    let _ = tx.send(());
                }
            });
        if let Some(dir) = &self.data_directory {
            builder = builder.data_directory(dir.clone());
        }
        let window = builder
            .build()
            .map_err(|e| format!("the hidden window didn't open: {e}"))?;
        // A blank page may finish before the handler is in place: then the bridge simply goes in
        // once the wait is over.
        let _ = tokio::time::timeout(LOAD_TIMEOUT, loaded_rx).await;
        window
            .eval(BRIDGE)
            .map_err(|e| format!("the hidden window took no script: {e}"))?;
        *self.window.lock().unwrap_or_else(|e| e.into_inner()) = Some(window.clone());
        Ok(window)
    }

    /// Run `method(id, arg)` on the page side and wait for its outcome.
    async fn job(&self, window: &WebviewWindow, method: &str, arg: Value) -> Result<Value, String> {
        let id = self.jobs.fetch_add(1, Ordering::Relaxed);
        let call =
            format!("window.__venutaBg ? (window.__venutaBg.{method}({id}, {arg}), true) : false");
        let started = ask(window, call).await?;
        if started != Value::Bool(true) {
            return Err("the page side isn't there".into());
        }
        let deadline = Instant::now() + JOB_TIMEOUT;
        let take = format!("window.__venutaBg ? window.__venutaBg.take({id}) : null");
        loop {
            tokio::time::sleep(POLL).await;
            let outcome = ask(window, take.clone()).await?;
            if let Some(done) = outcome.as_object() {
                let value = done.get("value").cloned().unwrap_or(Value::Null);
                return if done.get("ok") == Some(&Value::Bool(true)) {
                    Ok(value)
                } else {
                    Err(value.as_str().unwrap_or("failed").to_owned())
                };
            }
            if Instant::now() >= deadline {
                return Err("timed out".into());
            }
        }
    }
}

/// Evaluate `js` in the window and read what it came to (as JSON).
async fn ask(window: &WebviewWindow, js: String) -> Result<Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Mutex::new(Some(tx));
    window
        .eval_with_callback(js, move |result| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(result);
            }
        })
        .map_err(|e| format!("the hidden window took no script: {e}"))?;
    let text = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .map_err(|_| "the hidden window didn't answer".to_owned())?
        .map_err(|_| "the hidden window closed".to_owned())?;
    Ok(read_result(&text))
}

/// What WebView2 hands back for a script: its value as JSON (a string value arrives as a JSON
/// string, which may itself be JSON). Anything unreadable is null.
fn read_result(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::String(inner)) => serde_json::from_str(&inner).unwrap_or(Value::String(inner)),
        Ok(v) => v,
        Err(_) => Value::Null,
    }
}

impl BotGuardVm for WebviewBotGuard {
    fn start<'a>(&'a self, script: &'a str, challenge: &'a Challenge) -> VmFuture<'a, String> {
        Box::pin(async move {
            let window = self.fresh_window().await?;
            let arg = json!({
                "script": script,
                "program": challenge.program,
                "globalName": challenge.global_name,
                "config": challenge.page_config,
            });
            let answer = self.job(&window, "start", arg).await?;
            answer
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "BotGuard gave no answer".to_owned())
        })
    }

    fn ready_minter<'a>(&'a self, integrity_token: &'a str) -> VmFuture<'a, ()> {
        Box::pin(async move {
            let window = self.current().ok_or("BotGuard isn't running")?;
            self.job(&window, "minter", json!(integrity_token)).await?;
            Ok(())
        })
    }

    fn mint<'a>(&'a self, binding: &'a str) -> VmFuture<'a, String> {
        Box::pin(async move {
            let window = self.current().ok_or("BotGuard isn't running")?;
            let token = self.job(&window, "mint", json!(binding)).await?;
            token
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "no token".to_owned())
        })
    }
}

/// BotGuard runs in the app from now on (the catalog asks for tokens when it needs them).
pub fn install(app: &AppHandle) {
    media_access::potoken::po_tokens()
        .install(std::sync::Arc::new(WebviewBotGuard::new(app.clone())));
}

#[cfg(test)]
mod tests {
    use super::*;

    // WebView2 hands a script's value back as JSON; a string value comes doubly encoded.
    #[test]
    fn script_results_are_read_either_way() {
        assert_eq!(read_result("null"), Value::Null);
        assert_eq!(read_result("true"), Value::Bool(true));
        assert_eq!(
            read_result(r#"{"done":true,"ok":true,"value":"abc"}"#)["value"],
            "abc"
        );
        assert_eq!(
            read_result(r#""{\"done\":true,\"ok\":false,\"value\":\"no\"}""#)["value"],
            "no"
        );
        assert_eq!(read_result(r#""plain""#), Value::String("plain".into()));
        assert_eq!(read_result("not json"), Value::Null);
    }

    // The page side keeps to what the app polls for: one global, the four calls.
    #[test]
    fn the_page_side_has_each_call() {
        for name in [
            "take(id)",
            "start(id, c)",
            "minter(id, integrityToken)",
            "mint(id, binding)",
        ] {
            assert!(BRIDGE.contains(name), "{name}");
        }
        assert!(BRIDGE.contains("window.__venutaBg = {"));
        assert!(
            !BRIDGE.contains("fetch("),
            "the page asks nothing of the network"
        );
    }
}
