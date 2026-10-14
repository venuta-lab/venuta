//! Lyrics in windows of their own: the big screen (a whole screen, for a TV or a second
//! monitor) and the desktop lyrics (the lines alone, over everything, clicks passing through).
//! Both are the same app at `#stage` and `#overlay`, hearing the player like the mini player.
//! Async commands: making a window from a synchronous command deadlocks on Windows.

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};

#[derive(Serialize)]
pub struct MonitorInfo {
    name: String,
    width: u32,
    height: u32,
    primary: bool,
    /// The screen Venuta's window is on.
    current: bool,
}

fn screens(app: &AppHandle) -> (Vec<Monitor>, Option<Monitor>, Option<Monitor>) {
    let Some(main) = app.get_webview_window("main") else {
        return (Vec::new(), None, None);
    };
    (
        main.available_monitors().unwrap_or_default(),
        main.primary_monitor().ok().flatten(),
        main.current_monitor().ok().flatten(),
    )
}

/// The screens attached, for choosing where the big screen opens.
#[tauri::command]
pub fn monitors(app: AppHandle) -> Vec<MonitorInfo> {
    let (all, primary, current) = screens(&app);
    let same =
        |a: &Monitor, b: &Option<Monitor>| b.as_ref().is_some_and(|b| b.position() == a.position());
    all.iter()
        .enumerate()
        .map(|(i, m)| MonitorInfo {
            name: m
                .name()
                .cloned()
                .unwrap_or_else(|| format!("Screen {}", i + 1)),
            width: m.size().width,
            height: m.size().height,
            primary: same(m, &primary),
            current: same(m, &current),
        })
        .collect()
}

/// Lyrics filling a screen: the one named, or the one Venuta is on. Closed with `open: false`.
#[tauri::command]
pub async fn lyrics_stage(
    open: bool,
    monitor: Option<String>,
    app: AppHandle,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("stage") {
        let _ = w.close();
    }
    if !open {
        return Ok(());
    }
    let (all, _, current) = screens(&app);
    let target = monitor
        .and_then(|n| {
            all.iter()
                .find(|m| m.name().is_some_and(|x| *x == n))
                .cloned()
        })
        .or(current);
    let w = WebviewWindowBuilder::new(&app, "stage", WebviewUrl::App("index.html#stage".into()))
        .title("Venuta lyrics")
        .decorations(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    crate::app_icon::apply(&app);
    // Placed in the screen's own pixels (screens can scale differently), then made full.
    if let Some(m) = target {
        let p = m.position();
        let _ = w.set_position(PhysicalPosition::new(p.x + 40, p.y + 40));
    }
    let _ = w.set_fullscreen(true);
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

/// Whether the desktop lyrics are showing.
#[tauri::command]
pub fn desktop_lyrics_open(app: AppHandle) -> bool {
    app.get_webview_window("overlay").is_some()
}

/// Show the desktop lyrics where they were (`bounds`: x, y, width, height in logical pixels),
/// or along the bottom of the screen Venuta is on; or close them.
#[tauri::command]
pub async fn desktop_lyrics(
    open: bool,
    bounds: Option<(f64, f64, f64, f64)>,
    locked: bool,
    app: AppHandle,
) -> Result<(), String> {
    if !open {
        if let Some(w) = app.get_webview_window("overlay") {
            let _ = w.close();
        }
        return Ok(());
    }
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
        let _ = w.set_ignore_cursor_events(locked);
        return Ok(());
    }
    let (all, _, current) = screens(&app);
    // Saved bounds only if they're still on a screen (a monitor may have gone).
    let on_screen = |(x, y, w, h): (f64, f64, f64, f64)| {
        all.iter().any(|m| {
            let s = m.scale_factor();
            let (mx, my) = (m.position().x as f64 / s, m.position().y as f64 / s);
            let (mw, mh) = (m.size().width as f64 / s, m.size().height as f64 / s);
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            cx >= mx && cx <= mx + mw && cy >= my && cy <= my + mh
        })
    };
    let place = bounds.filter(|b| on_screen(*b)).or_else(|| {
        current.map(|m| {
            let area = m.work_area();
            let s = m.scale_factor();
            let (w, h) = (920.0, 170.0);
            let x = (area.position.x as f64 + area.size.width as f64 / 2.0) / s - w / 2.0;
            let y = (area.position.y as f64 + area.size.height as f64) / s - h - 40.0;
            (x, y, w, h)
        })
    });
    let mut b = WebviewWindowBuilder::new(
        &app,
        "overlay",
        WebviewUrl::App("index.html#overlay".into()),
    )
    .title("Venuta desktop lyrics")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(true)
    .focused(false)
    .min_inner_size(320.0, 90.0);
    if let Some((x, y, w, h)) = place {
        b = b.position(x, y).inner_size(w, h);
    }
    let w = b.build().map_err(|e| e.to_string())?;
    let _ = w.set_ignore_cursor_events(locked);
    crate::app_icon::apply(&app);
    // Their buttons light up in every window (they go out in desktop::on_window_event).
    let _ = app.emit(DESKTOP_LYRICS_EVENT, true);
    Ok(())
}

/// Told to every window when the desktop lyrics come (true) or go (false), however that happened.
pub const DESKTOP_LYRICS_EVENT: &str = "venuta://desktop-lyrics";

/// Locked, clicks go through the desktop lyrics to what is under them; unlocked, they can be
/// dragged, sized and changed.
#[tauri::command]
pub fn desktop_lyrics_lock(lock: bool, app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("overlay") {
        w.set_ignore_cursor_events(lock)
            .map_err(|e| e.to_string())?;
        if !lock {
            let _ = w.set_focus();
        }
    }
    Ok(())
}

/// Ask the main window to show or hide the desktop lyrics (it knows where they were left), or
/// to unlock them for moving: from the tray and the Ctrl+Shift+D shortcut.
pub fn ask_main(app: &AppHandle, what: &str) {
    let _ = app.emit_to("main", "desktop-lyrics", what);
}
