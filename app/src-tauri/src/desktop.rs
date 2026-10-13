//! Venuta as a Windows app: one at a time, an icon in the tray, what closing the window does,
//! and starting with Windows (in the tray, without a window).

use crate::playback::{NowPlaying, Playback};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// Started by Windows at sign-in: stay in the tray.
pub const IN_TRAY_ARG: &str = "--in-tray";

/// Bring the main window back, wherever it went (tray, minimised, behind others).
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The tray icon and its menu; the tooltip and first line follow the song playing.
pub fn tray(app: &AppHandle) -> tauri::Result<()> {
    let now = MenuItem::with_id(app, "now", "Nothing playing", false, None::<&str>)?;
    let play = MenuItem::with_id(app, "play", "Play", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next", true, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", "Previous", true, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", "Mini player", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show Venuta", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Venuta", true, None::<&str>)?;
    let line = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(
        app,
        &[
            &now,
            &line()?,
            &play,
            &next,
            &previous,
            &line()?,
            &mini,
            &show,
            &line()?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("venuta")
        .tooltip(if crate::test_copy() { "Venuta (test copy)" } else { "Venuta" })
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| {
            let pb = app.state::<Playback>();
            match e.id.as_ref() {
                "play" => pb.toggle(),
                "next" => pb.next(),
                "previous" => pb.previous(),
                "mini" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::playback::mini_player(true, app).await;
                    });
                }
                "show" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;

    // Follow the player: what's playing, and whether the first action plays or pauses.
    let rx = app.state::<Playback>().follow();
    std::thread::Builder::new()
        .name("venuta-tray".into())
        .spawn(move || {
            let mut shown: (String, bool) = (String::from("\u{0}"), false);
            for n in rx {
                let NowPlaying {
                    title,
                    artist,
                    playing,
                    file,
                    ..
                } = n;
                let text = if file.is_some() {
                    if artist.is_empty() {
                        title
                    } else {
                        format!("{title} – {artist}")
                    }
                } else {
                    String::new()
                };
                if (text.clone(), playing) == shown {
                    continue;
                }
                // Windows cuts tooltips at 127 characters.
                let tip: String = if text.is_empty() {
                    "Venuta".into()
                } else {
                    format!("Venuta: {text}")
                };
                let _ = tray.set_tooltip(Some(tip.chars().take(120).collect::<String>()));
                let _ = now.set_text(if text.is_empty() {
                    "Nothing playing".to_owned()
                } else {
                    text.chars().take(60).collect()
                });
                let _ = play.set_text(if playing { "Pause" } else { "Play" });
                shown = (text, playing);
            }
        })
        .ok();
    Ok(())
}

/// Closing the main window: keep playing in the tray, or quit, as chosen. The first time, the
/// window asks (and remembers); until then it stays open.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        match engine::settings::load().close_action.as_deref() {
            Some("quit") => window.app_handle().exit(0),
            Some(_) => {
                api.prevent_close();
                let _ = window.hide();
            }
            None => {
                api.prevent_close();
                let _ = window.emit("venuta://close-asked", ());
            }
        }
    }
}

/// Remember what closing the window does ("tray" or "quit"), and do it now if asked.
#[tauri::command]
pub fn set_close_action(action: String, now: bool, app: AppHandle) -> Result<(), String> {
    if action != "tray" && action != "quit" {
        return Err(format!("Unknown close action: {action}"));
    }
    let mut s = engine::settings::load();
    s.close_action = Some(action.clone());
    engine::settings::save(&s)?;
    if now {
        if action == "quit" {
            app.exit(0);
        } else if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    Ok(())
}
