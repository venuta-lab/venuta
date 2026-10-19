//! Venuta as a Windows app: one at a time, an icon in the tray, what closing the window does,
//! and starting with Windows (in the tray, without a window).

use crate::playback::{NowPlaying, Playback};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
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

/// Show the main window, or put it away when it's in front (the Show Venuta shortcut).
pub fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let front = w.is_visible().unwrap_or(false)
            && !w.is_minimized().unwrap_or(false)
            && w.is_focused().unwrap_or(false);
        if front {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

/// The sleep timer's minutes, as in the player bar and Sound.
const SLEEP_MINUTES: [u32; 5] = [15, 30, 45, 60, 90];

/// A sleep timer choice from the tray.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SleepPick {
    Minutes(u32),
    EndOfSong,
    Off,
}

fn sleep_pick(id: &str) -> Option<SleepPick> {
    match id.strip_prefix("sleep-")? {
        "song" => Some(SleepPick::EndOfSong),
        "off" => Some(SleepPick::Off),
        m => m
            .parse()
            .ok()
            .filter(|m| SLEEP_MINUTES.contains(m))
            .map(SleepPick::Minutes),
    }
}

/// One at a time, as in the player bar: a timer turns off stopping after the song, and the other
/// way round.
fn set_sleep(pb: &tauri::State<'_, Playback>, pick: SleepPick) {
    let (minutes, after_song) = match pick {
        SleepPick::Minutes(m) => (Some(f64::from(m)), false),
        SleepPick::EndOfSong => (None, true),
        SleepPick::Off => (None, false),
    };
    let _ = crate::playback::player_sleep(minutes, pb.clone());
    let _ = crate::playback::player_stop_after(after_song, pb.clone());
}

/// The tray icon and its menu; the tooltip and first line follow the song playing.
pub fn tray(app: &AppHandle) -> tauri::Result<()> {
    let now = MenuItem::with_id(app, "now", "Nothing playing", false, None::<&str>)?;
    let play = MenuItem::with_id(app, "play", "Play", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next", true, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", "Previous", true, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", "Mini player", true, None::<&str>)?;
    let lyrics = MenuItem::with_id(app, "lyrics", "Desktop lyrics", true, Some("Ctrl+Shift+D"))?;
    let move_lyrics = MenuItem::with_id(
        app,
        "lyrics-move",
        "Move desktop lyrics",
        true,
        None::<&str>,
    )?;
    let sleep_items = SLEEP_MINUTES
        .iter()
        .map(|m| {
            MenuItem::with_id(
                app,
                format!("sleep-{m}"),
                format!("In {m} minutes"),
                true,
                None::<&str>,
            )
        })
        .chain([
            MenuItem::with_id(
                app,
                "sleep-song",
                "At the end of this song",
                true,
                None::<&str>,
            ),
            MenuItem::with_id(app, "sleep-off", "Turn off", true, None::<&str>),
        ])
        .collect::<tauri::Result<Vec<_>>>()?;
    let sleep_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = sleep_items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let sleep = Submenu::with_items(app, "Sleep timer", true, &sleep_refs)?;
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
            &sleep,
            &line()?,
            &mini,
            &lyrics,
            &move_lyrics,
            &show,
            &line()?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("venuta")
        .tooltip(if crate::test_copy() {
            "Venuta (test copy)"
        } else {
            "Venuta"
        })
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
                        let _ = crate::playback::mini_player_toggle(app).await;
                    });
                }
                "lyrics" => crate::lyric_windows::ask_main(app, "toggle"),
                "lyrics-move" => crate::lyric_windows::ask_main(app, "unlock"),
                "show" => show_main(app),
                "quit" => app.exit(0),
                id => {
                    if let Some(pick) = sleep_pick(id) {
                        set_sleep(&pb, pick);
                    }
                }
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
    // A lyrics window gone (its own Close, the tray, the shortcut, a button here): every window
    // is told, instead of each asking every few seconds.
    if matches!(event, WindowEvent::Destroyed) {
        crate::lyric_windows::closed(window.app_handle(), window.label());
    }
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

/// A new song, said in a Windows notification (Settings: song notifications).
#[tauri::command]
pub fn song_notification(app: AppHandle, title: String, artist: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(artist)
        .show()
        .map_err(|e| e.to_string())
}

/// Quit Venuta (Ctrl+Q), whatever closing the window would do.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// How large the whole window draws (Ctrl + / - / 0): 1 is as designed.
#[tauri::command]
pub fn set_ui_zoom(window: tauri::WebviewWindow, scale: f64) -> Result<(), String> {
    window
        .set_zoom(scale.clamp(0.6, 2.0))
        .map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::{SleepPick, sleep_pick};

    #[test]
    fn the_trays_sleep_choices() {
        assert_eq!(sleep_pick("sleep-30"), Some(SleepPick::Minutes(30)));
        assert_eq!(sleep_pick("sleep-song"), Some(SleepPick::EndOfSong));
        assert_eq!(sleep_pick("sleep-off"), Some(SleepPick::Off));
        assert_eq!(sleep_pick("sleep-31"), None, "only the minutes offered");
        assert_eq!(sleep_pick("play"), None);
    }
}
