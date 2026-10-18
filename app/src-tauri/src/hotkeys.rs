//! Shortcuts that work from any app (Settings, Startup and tray): play and pause, next, like and
//! the rest, each on the keys you choose. Ctrl+Shift+D (desktop lyrics) is on unless you change
//! it; the others are off until you set them, so Venuta never takes keys another app uses.
//!
//! Settings keep only what you changed (`hotkeys`: action to keys, "" for off); this module adds
//! the defaults, registers them with Windows, and tells the window which one was pressed.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Every action a shortcut can do, in the order Settings lists them.
pub const ACTIONS: [&str; 8] = [
    "play_pause",
    "next",
    "previous",
    "like",
    "volume_up",
    "volume_down",
    "show",
    "desktop_lyrics",
];

/// The keys each action has before you change anything.
fn default_keys(action: &str) -> &'static str {
    match action {
        "desktop_lyrics" => "Ctrl+Shift+KeyD",
        _ => "",
    }
}

/// Every action and its keys now ("" for off): your changes over the defaults. Unknown actions
/// in the settings (from a newer Venuta) are left out.
pub fn effective(changed: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|a| {
            let keys = changed
                .get(*a)
                .map(|k| k.trim().to_owned())
                .unwrap_or_else(|| default_keys(a).to_owned());
            ((*a).to_owned(), keys)
        })
        .collect()
}

/// Whether these keys can be a shortcut from anywhere: they parse, and hold a modifier (a bare
/// letter would fire while you type in other apps). F13 to F24 and media keys may stand alone.
pub fn usable(keys: &str) -> Result<Shortcut, String> {
    let s = Shortcut::from_str(keys).map_err(|_| format!("{keys} isn't a shortcut"))?;
    let alone = keys.split('+').count() == 1;
    let lone_ok = {
        let k = keys.trim().to_ascii_uppercase();
        k.starts_with("MEDIA")
            || k.starts_with("AUDIOVOLUME")
            || k.strip_prefix('F')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (13..=24).contains(&n))
    };
    if alone && !lone_ok {
        return Err(
            "Use Ctrl, Alt, Shift or Win with it, so typing in other apps never sets it off."
                .into(),
        );
    }
    Ok(s)
}

/// Which action each registered shortcut does (by the shortcut's id).
static REGISTERED: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);

/// A shortcut that couldn't be set, and why (another app may hold those keys).
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Problem {
    pub action: String,
    pub keys: String,
    pub reason: String,
}

/// Register the shortcuts in these settings (all of them again; the old ones go first).
pub fn apply(app: &AppHandle, changed: &BTreeMap<String, String>) -> Vec<Problem> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut map = HashMap::new();
    let mut problems = Vec::new();
    for (action, keys) in effective(changed) {
        if keys.is_empty() {
            continue;
        }
        let problem = |reason: String| Problem {
            action: action.clone(),
            keys: keys.clone(),
            reason,
        };
        match usable(&keys) {
            Ok(s) if map.contains_key(&s.id()) => {
                problems.push(problem("Another Venuta shortcut has these keys.".into()))
            }
            Ok(s) => match gs.register(s) {
                Ok(()) => {
                    map.insert(s.id(), action.clone());
                }
                Err(_) => problems.push(problem("Another app has these keys already.".into())),
            },
            Err(e) => problems.push(problem(e)),
        }
    }
    *REGISTERED.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
    problems
}

/// A shortcut was pressed: do its action. The window itself does the ones that need it (liking,
/// the volume, the player), even while it's hidden in the tray.
pub fn pressed(app: &AppHandle, shortcut: &Shortcut) {
    let action = REGISTERED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(&shortcut.id()).cloned());
    match action.as_deref() {
        Some("desktop_lyrics") => crate::lyric_windows::ask_main(app, "toggle"),
        Some("show") => crate::desktop::toggle_main(app),
        Some(a) => {
            let _ = app.emit_to("main", "global-hotkey", a);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn your_changes_go_over_the_defaults() {
        let none = effective(&BTreeMap::new());
        assert_eq!(none.len(), ACTIONS.len());
        assert_eq!(
            none["desktop_lyrics"], "Ctrl+Shift+KeyD",
            "as it always was"
        );
        assert_eq!(none["play_pause"], "", "off until you set it");

        let mut changed = BTreeMap::new();
        changed.insert("play_pause".to_owned(), " Ctrl+Alt+Space ".to_owned());
        changed.insert("desktop_lyrics".to_owned(), String::new());
        changed.insert("from_a_newer_venuta".to_owned(), "Ctrl+Alt+KeyX".to_owned());
        let now = effective(&changed);
        assert_eq!(now["play_pause"], "Ctrl+Alt+Space");
        assert_eq!(now["desktop_lyrics"], "", "switched off stays off");
        assert!(!now.contains_key("from_a_newer_venuta"));
    }

    #[test]
    fn a_shortcut_from_anywhere_needs_a_modifier() {
        assert!(usable("Ctrl+Alt+Space").is_ok());
        assert!(usable("Ctrl+Shift+KeyD").is_ok());
        assert!(usable("Alt+ArrowRight").is_ok());
        assert!(usable("Ctrl+Alt+Digit1").is_ok());
        assert!(
            usable("KeyP").is_err(),
            "a bare letter fires while typing elsewhere"
        );
        assert!(usable("Space").is_err());
        assert!(usable("F13").is_ok());
        assert!(usable("F5").is_err());
        assert!(usable("MediaPlayPause").is_ok());
        assert!(usable("Ctrl+Nothing").is_err());
    }
}
