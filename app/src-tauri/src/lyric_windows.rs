//! Lyrics in windows of their own, each with one job: the big screen (`stage`: filling a screen,
//! for a TV or a second monitor) and the desktop lyrics (`overlay`: the lines in a strip over
//! everything, moved, sized, on top or not, and locked or not). Both are the same app at `#stage`
//! and `#overlay`, hearing the player like the mini player, frameless and see-through: what's
//! behind the words is drawn by the page (nothing, glass, the cover, a theme), or for frosted
//! glass blurred by Windows itself. Locked, the desktop lyrics let clicks through to what is under
//! them, all but their lock button: the pointer is watched here, and over that button the window
//! takes clicks for that moment only.
//! Async commands: making a window from a synchronous command deadlocks on Windows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Monitor, PhysicalPosition,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
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

/// Which lyrics window: its label, page and first size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Stage,
    Overlay,
}

impl Kind {
    fn of(s: &str) -> Result<Kind, String> {
        match s {
            "stage" => Ok(Kind::Stage),
            "overlay" => Ok(Kind::Overlay),
            _ => Err(format!("no lyrics window called {s}")),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Kind::Stage => "stage",
            Kind::Overlay => "overlay",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Kind::Stage => "Venuta lyrics",
            Kind::Overlay => "Venuta desktop lyrics",
        }
    }
    /// The smallest it can be made (logical pixels).
    fn min(self) -> (f64, f64) {
        match self {
            Kind::Stage => (480.0, 270.0),
            Kind::Overlay => (360.0, 130.0),
        }
    }
    /// Where it goes the first time (or when its place is off every screen): the desktop lyrics
    /// along the bottom of the screen, the big screen as a window in its middle.
    fn first_place(self, m: &Monitor) -> (f64, f64, f64, f64) {
        let area = m.work_area();
        let s = m.scale_factor();
        let (ax, ay) = (area.position.x as f64 / s, area.position.y as f64 / s);
        let (aw, ah) = (area.size.width as f64 / s, area.size.height as f64 / s);
        match self {
            Kind::Overlay => {
                // The lines, and the band above them for the bar and the lock.
                let (w, h) = (920.0_f64.min(aw - 40.0), 200.0);
                (ax + (aw - w) / 2.0, ay + ah - h - 40.0, w, h)
            }
            Kind::Stage => {
                let (w, h) = ((aw * 0.6).max(640.0), (ah * 0.6).max(360.0));
                (ax + (aw - w) / 2.0, ay + (ah - h) / 2.0, w, h)
            }
        }
    }
}

/// How a lyrics window is placed and behaves (the page keeps these per window).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Where it was as a window, logical pixels (x, y, width, height).
    pub bounds: Option<(f64, f64, f64, f64)>,
    /// Filling a screen.
    pub full: bool,
    /// Over every other window.
    pub on_top: bool,
    /// Clicks go through it (all but its lock button).
    pub locked: bool,
    /// The screen it fills, by name (none: the one Venuta is on).
    pub monitor: Option<String>,
}

/// Told to every window when the desktop lyrics come (true) or go (false), however that happened.
pub const DESKTOP_LYRICS_EVENT: &str = "venuta://desktop-lyrics";
/// Told to every window when either lyrics window comes or goes: `{ kind, open }`.
pub const LYRICS_WINDOW_EVENT: &str = "venuta://lyrics-window";
/// Told to a locked lyrics window as the pointer comes over it, over its lock button, or leaves.
const POINTER_EVENT: &str = "lyrics-pointer";

#[derive(Serialize, Clone)]
struct Shown {
    kind: &'static str,
    open: bool,
}

fn tell_shown(app: &AppHandle, kind: Kind, open: bool) {
    let _ = app.emit(
        LYRICS_WINDOW_EVENT,
        Shown {
            kind: kind.label(),
            open,
        },
    );
    if kind == Kind::Overlay {
        let _ = app.emit(DESKTOP_LYRICS_EVENT, open);
    }
}

/// A lyrics window closed, however (its own Close, the tray, the shortcut, a button elsewhere):
/// every window is told, and its lock is let go.
pub fn closed(app: &AppHandle, label: &str) {
    if let Ok(kind) = Kind::of(label) {
        app.state::<Locks>().forget(kind);
        // The big screen gone: the windows it cleared from its screen come back.
        if kind == Kind::Stage {
            crate::clear_screen::on_close();
        }
        tell_shown(app, kind, false);
    }
}

/// Show a lyrics window placed as `place` says (or bring it forward), or close it.
#[tauri::command]
pub async fn lyrics_window(
    kind: String,
    open: bool,
    place: Option<Place>,
    app: AppHandle,
) -> Result<(), String> {
    let kind = Kind::of(&kind)?;
    if !open {
        if let Some(w) = app.get_webview_window(kind.label()) {
            let _ = w.close();
        }
        return Ok(());
    }
    let mut place = place.unwrap_or(Place {
        bounds: None,
        full: kind == Kind::Stage,
        on_top: kind == Kind::Overlay,
        locked: kind == Kind::Overlay,
        monitor: None,
    });
    // Each its own job: the big screen fills a screen, never on top or locked (Esc closes it);
    // the desktop lyrics are a window of their own.
    place.full = kind == Kind::Stage;
    if kind == Kind::Stage {
        place.on_top = false;
        place.locked = false;
    }
    if let Some(w) = app.get_webview_window(kind.label()) {
        apply(&app, kind, &w, &place);
        let _ = w.show();
        bare(kind, &w);
        if !place.locked {
            let _ = w.set_focus();
        }
        return Ok(());
    }
    let (all, _, current) = screens(&app);
    let target = place
        .monitor
        .as_ref()
        .and_then(|n| all.iter().find(|m| m.name().is_some_and(|x| x == n)))
        .cloned()
        .or(current);
    let (min_w, min_h) = kind.min();
    let w = WebviewWindowBuilder::new(
        &app,
        kind.label(),
        WebviewUrl::App(format!("index.html#{}", kind.label()).into()),
    )
    .title(kind.title())
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(true)
    .always_on_top(place.on_top)
    .skip_taskbar(kind == Kind::Overlay)
    .focused(kind == Kind::Stage && !place.locked)
    .min_inner_size(min_w, min_h)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    crate::app_icon::apply(&app);
    // Its place as a window: where it was, if that's still on a screen; else the first place.
    let bounds = place
        .bounds
        .filter(|b| on_screen(&all, *b))
        .or_else(|| target.as_ref().map(|m| kind.first_place(m)));
    if let Some((x, y, width, height)) = bounds {
        let _ = w.set_position(LogicalPosition::new(x, y));
        let _ = w.set_size(LogicalSize::new(width, height));
    }
    if place.full {
        // Placed in the screen's own pixels (screens can scale differently), then made full.
        if let Some(m) = &target {
            let p = m.position();
            let _ = w.set_position(PhysicalPosition::new(p.x + 40, p.y + 40));
        }
        let _ = w.set_fullscreen(true);
    }
    lock(&app, kind, &w, place.locked);
    let _ = w.show();
    bare(kind, &w);
    if kind == Kind::Stage {
        let _ = w.set_focus();
        // Its screen cleared: the other windows there minimized until it closes.
        #[cfg(windows)]
        if let Ok(h) = w.hwnd() {
            crate::clear_screen::on_open(h.0 as isize);
        }
    }
    tell_shown(&app, kind, true);
    Ok(())
}

/// A lyrics window is only its page. A frameless window still carries Windows' title-bar styles
/// (the window menu, minimize, maximize), and through a see-through page Windows draws their
/// buttons and a thin border over the words; the window's own changes (locking, on top, full
/// screen, the blur) put them back each time. So after each, they come off again: no buttons, no
/// border line, and the corners its job wants (the desktop lyrics rounded by Windows, so the blur,
/// the theme and the page share one edge; the big screen square). The sizing frame stays: the
/// desktop lyrics are sized from their edges.
fn bare(kind: Kind, w: &WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE,
            DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos,
            WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU,
        };
        let Ok(h) = w.hwnd() else {
            return;
        };
        unsafe {
            let style = GetWindowLongPtrW(h, GWL_STYLE);
            let bare = style & !((WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) as isize);
            if bare != style {
                SetWindowLongPtrW(h, GWL_STYLE, bare);
                let _ = SetWindowPos(
                    h,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOZORDER
                        | SWP_NOOWNERZORDER
                        | SWP_NOACTIVATE
                        | SWP_FRAMECHANGED,
                );
            }
            // Windows 11 only (earlier ones have neither, and say so).
            let none = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_BORDER_COLOR,
                (&none as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            let corner = if kind == Kind::Stage {
                DWMWCP_DONOTROUND
            } else {
                DWMWCP_ROUND
            };
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&corner as *const _ as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = (kind, w);
}

/// A shown window to `place`'s ways (full screen, on top, locked), without moving it otherwise.
fn apply(app: &AppHandle, kind: Kind, w: &WebviewWindow, place: &Place) {
    let _ = w.set_always_on_top(place.on_top);
    set_full(w, place.full, place.bounds);
    lock(app, kind, w, place.locked);
}

fn set_full(w: &WebviewWindow, full: bool, bounds: Option<(f64, f64, f64, f64)>) {
    if w.is_fullscreen().unwrap_or(false) == full {
        return;
    }
    let _ = w.set_fullscreen(full);
    // Back to a window: where it was as one.
    if !full && let Some((x, y, width, height)) = bounds {
        let _ = w.set_position(LogicalPosition::new(x, y));
        let _ = w.set_size(LogicalSize::new(width, height));
    }
}

/// Change a shown lyrics window: full screen or not (back to `bounds` as a window), on top or
/// not, locked or not. What isn't given stays.
#[tauri::command]
pub fn lyrics_window_set(
    kind: String,
    full: Option<bool>,
    on_top: Option<bool>,
    locked: Option<bool>,
    bounds: Option<(f64, f64, f64, f64)>,
    app: AppHandle,
) -> Result<(), String> {
    let kind = Kind::of(&kind)?;
    let Some(w) = app.get_webview_window(kind.label()) else {
        return Ok(());
    };
    if let Some(full) = full {
        set_full(&w, full, bounds);
    }
    if let Some(on_top) = on_top {
        w.set_always_on_top(on_top).map_err(|e| e.to_string())?;
    }
    if let Some(locked) = locked {
        lock(&app, kind, &w, locked);
        if !locked {
            let _ = w.set_focus();
        }
    }
    bare(kind, &w);
    Ok(())
}

/// Frosted glass behind a lyrics window's words: Windows blurs what's behind it (the page tints
/// it), or not. The same acrylic as the main window.
#[tauri::command]
pub fn lyrics_window_frost(kind: String, on: bool, app: AppHandle) -> Result<(), String> {
    let kind = Kind::of(&kind)?;
    let Some(w) = app.get_webview_window(kind.label()) else {
        return Ok(());
    };
    let effects = on.then(|| {
        tauri::window::EffectsBuilder::new()
            .effect(tauri::window::Effect::Acrylic)
            // Almost no tint of its own: the page's tint follows the theme.
            .color(tauri::window::Color(16, 16, 20, 8))
            .build()
    });
    w.set_effects(effects).map_err(|e| e.to_string())?;
    bare(kind, &w);
    Ok(())
}

/// Whether a lyrics window is showing.
#[tauri::command]
pub fn lyrics_window_open(kind: String, app: AppHandle) -> Result<bool, String> {
    Ok(app.get_webview_window(Kind::of(&kind)?.label()).is_some())
}

/// Where a locked window's lock button is (logical pixels in the window: x, y, width, height):
/// over it, the window takes clicks. `None`: no button shown.
#[tauri::command]
pub fn lyrics_lock_spot(
    kind: String,
    spot: Option<(f64, f64, f64, f64)>,
    app: AppHandle,
) -> Result<(), String> {
    let kind = Kind::of(&kind)?;
    if let Some(s) = app.state::<Locks>().spots.lock().unwrap().get_mut(&kind) {
        s.rect = spot;
    }
    Ok(())
}

// --- The big screen and the desktop lyrics as before (kept for what calls them) -------------

/// Lyrics filling a screen: the one named, or the one Venuta is on. Closed with `open: false`.
#[tauri::command]
pub async fn lyrics_stage(
    open: bool,
    monitor: Option<String>,
    app: AppHandle,
) -> Result<(), String> {
    let place = Place {
        bounds: None,
        full: true,
        on_top: false,
        locked: false,
        monitor,
    };
    lyrics_window("stage".into(), open, Some(place), app).await
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
    let place = Place {
        bounds,
        full: false,
        on_top: true,
        locked,
        monitor: None,
    };
    lyrics_window("overlay".into(), open, Some(place), app).await
}

/// Locked, clicks go through the desktop lyrics to what is under them; unlocked, they can be
/// dragged, sized and changed.
#[tauri::command]
pub fn desktop_lyrics_lock(lock: bool, app: AppHandle) -> Result<(), String> {
    lyrics_window_set("overlay".into(), None, None, Some(lock), None, app)
}

/// Ask the main window to show or hide the desktop lyrics (it knows where they were left), or
/// to unlock them for moving: from the tray and the Ctrl+Shift+D shortcut.
pub fn ask_main(app: &AppHandle, what: &str) {
    let _ = app.emit_to("main", "desktop-lyrics", what);
}

// --- Locked windows and their lock button ----------------------------------------------------

/// A saved place is still usable when its middle is on one of the screens (a monitor may have
/// gone since).
fn on_screen(all: &[Monitor], (x, y, w, h): (f64, f64, f64, f64)) -> bool {
    all.iter().any(|m| {
        let s = m.scale_factor();
        let (mx, my) = (m.position().x as f64 / s, m.position().y as f64 / s);
        let (mw, mh) = (m.size().width as f64 / s, m.size().height as f64 / s);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        cx >= mx && cx <= mx + mw && cy >= my && cy <= my + mh
    })
}

/// A locked window: where its lock button is, and what the pointer was last doing.
#[derive(Default)]
struct Spot {
    rect: Option<(f64, f64, f64, f64)>,
    inside: bool,
    over: bool,
}

/// The locked lyrics windows, and the one thread watching the pointer while any is.
#[derive(Default)]
pub struct Locks {
    spots: Mutex<HashMap<Kind, Spot>>,
    watching: AtomicBool,
}

impl Locks {
    fn forget(&self, kind: Kind) {
        self.spots.lock().unwrap().remove(&kind);
    }
}

#[derive(Serialize, Clone)]
struct Pointer {
    inside: bool,
    over: bool,
}

fn lock(app: &AppHandle, kind: Kind, w: &WebviewWindow, locked: bool) {
    let _ = w.set_ignore_cursor_events(locked);
    bare(kind, w);
    let locks = app.state::<Locks>();
    if !locked {
        locks.forget(kind);
        return;
    }
    locks.spots.lock().unwrap().entry(kind).or_default();
    if !locks.watching.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::Builder::new()
            .name("venuta-lyrics-lock".into())
            .spawn(move || watch(app))
            .ok();
    }
}

/// While any lyrics window is locked: where the pointer is, every 50 ms. Over a window, it is
/// told (its lock button shows); over the button, the window takes clicks.
fn watch(app: AppHandle) {
    let locks = app.state::<Locks>();
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let kinds: Vec<Kind> = locks.spots.lock().unwrap().keys().copied().collect();
        if kinds.is_empty() {
            // Nothing locked: stop. A window locked in the moment between found this thread
            // still watching and started none, so it watches on unless another has started.
            locks.watching.store(false, Ordering::SeqCst);
            if locks.spots.lock().unwrap().is_empty() || locks.watching.swap(true, Ordering::SeqCst)
            {
                return;
            }
            continue;
        }
        let Ok(cursor) = app.cursor_position() else {
            continue;
        };
        for kind in kinds {
            let Some(w) = app.get_webview_window(kind.label()) else {
                locks.forget(kind);
                continue;
            };
            let (Ok(pos), Ok(size), Ok(scale)) =
                (w.outer_position(), w.outer_size(), w.scale_factor())
            else {
                continue;
            };
            let window = (
                pos.x as f64,
                pos.y as f64,
                size.width as f64,
                size.height as f64,
            );
            let mut spots = locks.spots.lock().unwrap();
            let Some(spot) = spots.get_mut(&kind) else {
                continue;
            };
            let (inside, over) = pointer_on(cursor.x, cursor.y, window, scale, spot.rect);
            if over != spot.over {
                let _ = w.set_ignore_cursor_events(!over);
                bare(kind, &w);
            }
            if (inside, over) != (spot.inside, spot.over) {
                let _ = w.emit_to(kind.label(), POINTER_EVENT, Pointer { inside, over });
            }
            spot.inside = inside;
            spot.over = over;
        }
    }
}

/// Whether the pointer (screen pixels) is over a window (its place and size in screen pixels)
/// and over its lock button (logical pixels in the window, at the window's scale).
fn pointer_on(
    x: f64,
    y: f64,
    (wx, wy, ww, wh): (f64, f64, f64, f64),
    scale: f64,
    button: Option<(f64, f64, f64, f64)>,
) -> (bool, bool) {
    let inside = x >= wx && x < wx + ww && y >= wy && y < wy + wh;
    let over = inside
        && button.is_some_and(|(bx, by, bw, bh)| {
            let (lx, ly) = ((x - wx) / scale, (y - wy) / scale);
            lx >= bx && lx < bx + bw && ly >= by && ly < by + bh
        });
    (inside, over)
}

#[cfg(test)]
mod tests {
    use super::pointer_on;

    #[test]
    fn the_lock_button_takes_clicks_only_under_the_pointer() {
        // A window at (1000, 500), 800 x 200 screen pixels, at 125 %; its button at 20, 20, 40 x 40.
        let win = (1000.0, 500.0, 800.0, 200.0);
        let button = Some((20.0, 20.0, 40.0, 40.0));
        assert_eq!(
            pointer_on(900.0, 600.0, win, 1.25, button),
            (false, false),
            "outside"
        );
        assert_eq!(
            pointer_on(1500.0, 600.0, win, 1.25, button),
            (true, false),
            "over the window"
        );
        // The button's middle: 40 logical = 50 screen pixels in.
        assert_eq!(
            pointer_on(1050.0, 550.0, win, 1.25, button),
            (true, true),
            "over the button"
        );
        assert_eq!(
            pointer_on(1100.0, 550.0, win, 1.25, button),
            (true, false),
            "just past it"
        );
        assert_eq!(
            pointer_on(1050.0, 550.0, win, 1.25, None),
            (true, false),
            "no button shown"
        );
    }
}
