//! Lyrics in windows of their own, each with one job: the big screen (`stage`: filling a screen,
//! for a TV or a second monitor) and the desktop lyrics (`overlay`: the lines in a strip over
//! everything, moved, sized, on top or not, and locked or not). Both are the same app at `#stage`
//! and `#overlay`, hearing the player like the mini player, frameless and see-through: what's
//! behind the words is drawn by the page (nothing, glass, the cover, a theme), or for frosted
//! glass blurred by Windows itself. Locked, the desktop lyrics let clicks through to what is under
//! them, all but their lock button: the pointer is watched here, and over that button the window
//! takes clicks for that moment only.
//! Async commands: making a window from a synchronous command deadlocks on Windows.
//!
//! One window of each kind, whatever asks for it (the buttons, the tray, the shortcut, Settings),
//! however quickly: see [`Shelf`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
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
    /// Its name: what the pages ask by, and the start of each of its windows' labels.
    fn label(self) -> &'static str {
        match self {
            Kind::Stage => "stage",
            Kind::Overlay => "overlay",
        }
    }
    /// The kind of a lyrics window, from its label (`overlay-3`; plain `overlay` too).
    fn of_label(label: &str) -> Option<Kind> {
        [Kind::Stage, Kind::Overlay].into_iter().find(|k| {
            label == k.label()
                || label
                    .strip_prefix(k.label())
                    .and_then(|r| r.strip_prefix('-'))
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
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
    /// Where it goes the first time (or when its place is off every screen), in that screen's
    /// own pixels: the desktop lyrics along the bottom, the big screen as a window in its middle.
    fn first_place(self, s: &Screen) -> Px {
        let px = |logical: f64| (logical * s.scale).round() as i32;
        match self {
            Kind::Overlay => {
                // The lines, and the band above them for the bar and the lock.
                let (w, h) = (px(920.0).min(s.w - px(40.0)), px(200.0));
                (s.x + (s.w - w) / 2, s.y + s.h - h - px(40.0), w, h)
            }
            Kind::Stage => {
                let (w, h) = ((s.w * 6 / 10).max(px(640.0)), (s.h * 6 / 10).max(px(360.0)));
                (s.x + (s.w - w) / 2, s.y + (s.h - h) / 2, w, h)
            }
        }
    }
}

/// A window's place in screen pixels (x, y, width, height), the same across every screen
/// whatever each one's scale: the one way a place means the same thing everywhere.
type Px = (i32, i32, i32, i32);

/// A screen, as placing needs it: its work area (screen pixels, without the taskbar) and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Screen {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: f64,
}

impl Screen {
    fn of(m: &Monitor) -> Screen {
        let a = m.work_area();
        Screen {
            x: a.position.x,
            y: a.position.y,
            w: a.size.width as i32,
            h: a.size.height as i32,
            scale: m.scale_factor(),
        }
    }
    /// How much of `p` is on this screen (pixels across, pixels down).
    fn overlap(&self, p: Px) -> (i32, i32) {
        let across = (p.0 + p.2).min(self.x + self.w) - p.0.max(self.x);
        let down = (p.1 + p.3).min(self.y + self.h) - p.1.max(self.y);
        (across.max(0), down.max(0))
    }
}

/// Enough of a window on a screen to see it and take hold of it.
const ENOUGH: (i32, i32) = (120, 48);

/// Where a lyrics window goes: where it was kept, if enough of it is on a screen connected now
/// (brought fully onto that screen, and no bigger than it); else its first place on `target`, the
/// screen the player is on. None with no screens known.
fn place_px(kind: Kind, screens: &[Screen], kept: Option<Px>, target: usize) -> Option<Px> {
    let on = kept.and_then(|p| {
        screens
            .iter()
            .map(|s| (s, s.overlap(p)))
            .filter(|(_, (a, d))| *a >= ENOUGH.0 && *d >= ENOUGH.1)
            .max_by_key(|(_, (a, d))| i64::from(*a) * i64::from(*d))
            .map(|(s, _)| (s, p))
    });
    if let Some((s, (x, y, w, h))) = on {
        let (w, h) = (w.min(s.w), h.min(s.h));
        let x = x.clamp(s.x, s.x + s.w - w);
        let y = y.clamp(s.y, s.y + s.h - h);
        return Some((x, y, w, h));
    }
    let s = screens.get(target).or(screens.first())?;
    Some(kind.first_place(s))
}

/// The screens connected now, and which of them the player is on.
fn screens_now(app: &AppHandle) -> (Vec<Screen>, usize) {
    let (all, _, current) = screens(app);
    let at = current
        .and_then(|c| all.iter().position(|m| m.position() == c.position()))
        .unwrap_or(0);
    (all.iter().map(Screen::of).collect(), at)
}

/// Put a window at `p`, in screen pixels. Moving onto a screen of another scale makes Windows
/// resize it for that screen, so the size is set after the move, and the place once more after.
fn put(w: &WebviewWindow, (x, y, width, height): Px) {
    let _ = w.set_position(PhysicalPosition::new(x, y));
    let _ = w.set_size(PhysicalSize::new(width.max(1) as u32, height.max(1) as u32));
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// Where a shown window is now, in screen pixels.
fn where_now(w: &WebviewWindow) -> Option<Px> {
    let p = w.outer_position().ok()?;
    let s = w.outer_size().ok()?;
    Some((p.x, p.y, s.width as i32, s.height as i32))
}

/// How a lyrics window is placed and behaves (the page keeps these per window).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Where it was as a window, in screen pixels (x, y, width, height).
    pub px: Option<Px>,
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

/// A window gone, however (its own Close, the tray, the shortcut, a button elsewhere). When it
/// was a lyrics window's window now, every window is told, and its lock is let go; an older one's
/// end (one cleared away before another was made) changes nothing.
pub fn closed(app: &AppHandle, label: &str) {
    if let Some(kind) = app.state::<Shelf>().gone(label) {
        app.state::<Locks>().forget(kind);
        // The big screen gone: the windows it cleared from its screen come back.
        if kind == Kind::Stage {
            crate::clear_screen::on_close();
        }
        tell_shown(app, kind, false);
    }
}

// --- One window of each kind ------------------------------------------------------------------
//
// Tauri knows its windows by label, and it checks a label is free before it makes the window and
// takes the window in only once it is made; and a window's end makes it forget whichever window
// has that label by then. So a second ask while the first window was being made (a button pressed
// again, about a tenth of a second) made a second window of the same label, and the end of either
// left the other unknown: alive on screen, see-through with nothing in it, its every call to the
// app failing ("failed to acquire webview reference"), and the button reading it as closed and
// making another at each press. Measured 2026-10-21: ten such windows in a test copy.
//
// So here: each opening and closing of a kind waits its turn; a closing is done only once the
// window is gone; each window has a label never used before (the kind and a number), so the end
// of an old one can never be taken for the new one's; and before a window is made, anything of
// its kind still about (Tauri's, or one only Windows still knows) is cleared away.

/// Which window is each kind's now.
#[derive(Debug, Default)]
struct Book {
    current: HashMap<Kind, String>,
    made: u64,
}

impl Book {
    fn current(&self, kind: Kind) -> Option<String> {
        self.current.get(&kind).cloned()
    }
    /// A label for a new window of `kind`, never used before; that kind's window from now.
    fn name(&mut self, kind: Kind) -> String {
        self.made += 1;
        let label = format!("{}-{}", kind.label(), self.made);
        self.current.insert(kind, label.clone());
        label
    }
    /// The window `label` is gone: its kind, if it was that kind's window now.
    fn gone(&mut self, label: &str) -> Option<Kind> {
        let kind = Kind::of_label(label)?;
        if self.current.get(&kind).is_some_and(|l| l == label) {
            self.current.remove(&kind);
            Some(kind)
        } else {
            None
        }
    }
}

/// What the shelf needs of the windows themselves: Tauri's ([`Tauri`]), or a test's.
trait Host {
    /// Whether the window `label` is there.
    fn has(&self, label: &str) -> bool;
    /// Every window there is, by label.
    fn labels(&self) -> Vec<String>;
    /// Make the window `label` for `kind`, placed as `place` says, and show it.
    async fn make(&self, kind: Kind, label: &str, place: &Place) -> Result<(), String>;
    /// Bring the window `label` forward, to `place`'s ways.
    fn bring(&self, kind: Kind, label: &str, place: &Place);
    /// End the window `label`: gone once `has` says so.
    fn end(&self, label: &str);
    /// End any window of `kind` only the system still knows (one the app lost), and wait until
    /// they are gone. How many there were.
    async fn end_lost(&self, kind: Kind) -> usize;
}

/// How long a window may take to end before it is left to itself.
const ENDING: Duration = Duration::from_secs(3);

/// The lyrics windows: at most one of each kind, whoever asks and however quickly.
#[derive(Default)]
pub struct Shelf {
    stage: tokio::sync::Mutex<()>,
    overlay: tokio::sync::Mutex<()>,
    book: Mutex<Book>,
}

impl Shelf {
    /// Each kind's openings and closings, one at a time (the two kinds apart).
    fn turn(&self, kind: Kind) -> &tokio::sync::Mutex<()> {
        match kind {
            Kind::Stage => &self.stage,
            Kind::Overlay => &self.overlay,
        }
    }
    fn book(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// The label of `kind`'s window, if it is there.
    fn live(&self, host: &impl Host, kind: Kind) -> Option<String> {
        let label = self.book().current(kind)?;
        host.has(&label).then_some(label)
    }
    /// A window gone: its kind, if it was that kind's window now.
    fn gone(&self, label: &str) -> Option<Kind> {
        self.book().gone(label)
    }

    async fn open(&self, host: &impl Host, kind: Kind, place: &Place) -> Result<(), String> {
        let _turn = self.turn(kind).lock().await;
        self.open_now(host, kind, place).await
    }

    async fn close(&self, host: &impl Host, kind: Kind) {
        let _turn = self.turn(kind).lock().await;
        self.close_now(host, kind).await;
    }

    /// Closed if it is showing, else opened: whether it shows now.
    async fn toggle(&self, host: &impl Host, kind: Kind, place: &Place) -> Result<bool, String> {
        let _turn = self.turn(kind).lock().await;
        if self.live(host, kind).is_some() {
            self.close_now(host, kind).await;
            Ok(false)
        } else {
            self.open_now(host, kind, place).await.map(|()| true)
        }
    }

    async fn open_now(&self, host: &impl Host, kind: Kind, place: &Place) -> Result<(), String> {
        if let Some(label) = self.live(host, kind) {
            host.bring(kind, &label, place);
            return Ok(());
        }
        // Never two of a kind: whatever of it is still about goes first.
        for label in host
            .labels()
            .into_iter()
            .filter(|l| Kind::of_label(l) == Some(kind))
        {
            self.end_now(host, &label).await;
        }
        let lost = host.end_lost(kind).await;
        if lost > 0 {
            crate::playback::note_play(
                &format!(
                    "cleared away {lost} {} window(s) the app had lost",
                    kind.label()
                ),
                "the lyrics windows",
            );
        }
        let label = self.book().name(kind);
        let made = host.make(kind, &label, place).await;
        if made.is_err() {
            self.book().gone(&label);
        }
        made
    }

    async fn close_now(&self, host: &impl Host, kind: Kind) {
        if let Some(label) = self.live(host, kind) {
            self.end_now(host, &label).await;
        }
    }

    /// End a window and wait until it is gone (or has taken too long: then it is left, still
    /// known, so a press can try again).
    async fn end_now(&self, host: &impl Host, label: &str) {
        host.end(label);
        let until = Instant::now() + ENDING;
        while host.has(label) {
            if Instant::now() >= until {
                crate::playback::note_play(
                    &format!("{label} didn't close in {} s", ENDING.as_secs()),
                    "the lyrics windows",
                );
                return;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
}

/// The window that is `kind`'s now, if it is there.
fn window(app: &AppHandle, kind: Kind) -> Option<WebviewWindow> {
    let label = app.state::<Shelf>().book().current(kind)?;
    app.get_webview_window(&label)
}

/// The lyrics windows as Tauri has them.
struct Tauri<'a>(&'a AppHandle);

impl Host for Tauri<'_> {
    fn has(&self, label: &str) -> bool {
        self.0.get_webview_window(label).is_some()
    }

    fn labels(&self) -> Vec<String> {
        self.0.webview_windows().into_keys().collect()
    }

    fn bring(&self, kind: Kind, label: &str, place: &Place) {
        let (app, Some(w)) = (self.0, self.0.get_webview_window(label)) else {
            return;
        };
        apply(app, kind, &w, place);
        // Still on a screen? (One may have gone, or changed its scale, since.)
        if !place.full {
            let (all, at) = screens_now(app);
            let now = where_now(&w);
            if let Some(p) = place_px(kind, &all, now, at)
                && Some(p) != now
            {
                put(&w, p);
            }
        }
        let _ = w.show();
        bare(kind, &w);
        if !place.locked {
            let _ = w.set_focus();
        }
    }

    async fn make(&self, kind: Kind, label: &str, place: &Place) -> Result<(), String> {
        let app = self.0;
        let (all, _, current) = screens(app);
        let target = place
            .monitor
            .as_ref()
            .and_then(|n| all.iter().find(|m| m.name().is_some_and(|x| x == n)))
            .cloned()
            .or(current);
        let (min_w, min_h) = kind.min();
        let w = WebviewWindowBuilder::new(
            app,
            label,
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
        crate::app_icon::apply(app);
        // Its place as a window, in screen pixels: where it was, if that's still on a screen;
        // else its first place on the screen the player is on.
        let (screens, at) = screens_now(app);
        let at = target
            .as_ref()
            .and_then(|t| all.iter().position(|m| m.position() == t.position()))
            .unwrap_or(at);
        let spot = place_px(kind, &screens, place.px, at);
        if let Some(p) = spot {
            put(&w, p);
        }
        if place.full {
            // Placed in the screen's own pixels (screens can scale differently), then made full.
            if let Some(m) = &target {
                let p = m.position();
                let _ = w.set_position(PhysicalPosition::new(p.x + 40, p.y + 40));
            }
            let _ = w.set_fullscreen(true);
        }
        lock(app, kind, &w, place.locked);
        let _ = w.show();
        // A window hidden until now may take its new screen's scale only as it shows: placed
        // again.
        if !place.full
            && let Some(p) = spot
            && where_now(&w) != Some(p)
        {
            put(&w, p);
        }
        bare(kind, &w);
        if kind == Kind::Stage {
            let _ = w.set_focus();
            // Its screen cleared: the other windows there minimized until it closes.
            #[cfg(windows)]
            if let Ok(h) = w.hwnd() {
                crate::clear_screen::on_open(h.0 as isize);
            }
        }
        tell_shown(app, kind, true);
        Ok(())
    }

    fn end(&self, label: &str) {
        // Destroyed, not asked to close: nothing on its page can hold it open, and a page that
        // can no longer reach the app (as a lost one couldn't) goes all the same.
        if let Some(w) = self.0.get_webview_window(label) {
            let _ = w.destroy();
        }
    }

    async fn end_lost(&self, kind: Kind) -> usize {
        #[cfg(windows)]
        {
            let known: Vec<isize> = self
                .0
                .webview_windows()
                .values()
                .filter_map(|w| w.hwnd().ok())
                .map(|h| h.0 as isize)
                .collect();
            let title = kind.title();
            let lost = tokio::task::spawn_blocking(move || lost::find(title, &known))
                .await
                .unwrap_or_default();
            for &h in &lost {
                lost::end(h);
            }
            let until = Instant::now() + ENDING;
            while lost.iter().any(|&h| lost::there(h)) && Instant::now() < until {
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            lost.len()
        }
        #[cfg(not(windows))]
        {
            let _ = kind;
            0
        }
    }
}

/// Windows of Venuta's that Windows has and the app doesn't know (anything of a lyrics window's
/// title not among the app's own): found, and ended the way the app ends its own (asked of the
/// window's own thread).
#[cfg(windows)]
mod lost {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
        PostMessageW, RegisterWindowMessageA,
    };
    use windows::core::{BOOL, s};

    /// The window class of every window the app makes.
    const CLASS: &str = "Tauri Window";

    fn text(h: HWND, class: bool) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe {
            if class {
                GetClassNameW(h, &mut buf)
            } else {
                GetWindowTextW(h, &mut buf)
            }
        };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// This process's windows titled `title` that are none of `known`.
    pub fn find(title: &str, known: &[isize]) -> Vec<isize> {
        struct Walk<'a> {
            pid: u32,
            title: &'a str,
            known: &'a [isize],
            found: Vec<isize>,
        }
        unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
            let walk = unsafe { &mut *(l.0 as *mut Walk) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            if pid == walk.pid
                && !walk.known.contains(&(h.0 as isize))
                && text(h, true) == CLASS
                && text(h, false) == walk.title
            {
                walk.found.push(h.0 as isize);
            }
            BOOL(1)
        }
        let mut walk = Walk {
            pid: std::process::id(),
            title,
            known,
            found: Vec::new(),
        };
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut walk as *mut Walk as isize)) };
        walk.found
    }

    /// End it as the app ends its windows: the window's own thread destroys it.
    pub fn end(h: isize) {
        unsafe {
            let destroy = RegisterWindowMessageA(s!("Tao::DestroyMsg"));
            let _ = PostMessageW(
                Some(HWND(h as *mut core::ffi::c_void)),
                destroy,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    pub fn there(h: isize) -> bool {
        unsafe { IsWindow(Some(HWND(h as *mut core::ffi::c_void))) }.as_bool()
    }
}

/// How each kind is placed, whatever was asked: the big screen fills a screen, never on top or
/// locked (Esc closes it); the desktop lyrics are a window of their own.
fn settled(kind: Kind, place: Option<Place>) -> Place {
    let mut place = place.unwrap_or(Place {
        px: None,
        full: kind == Kind::Stage,
        on_top: kind == Kind::Overlay,
        locked: kind == Kind::Overlay,
        monitor: None,
    });
    place.full = kind == Kind::Stage;
    if kind == Kind::Stage {
        place.on_top = false;
        place.locked = false;
    }
    place
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
    let shelf = app.state::<Shelf>();
    if open {
        shelf.open(&Tauri(&app), kind, &settled(kind, place)).await
    } else {
        shelf.close(&Tauri(&app), kind).await;
        Ok(())
    }
}

/// Close a lyrics window if it is showing, else show it as `place` says, in one step (a button
/// asking first and acting after could act on what was already changing): whether it shows now.
#[tauri::command]
pub async fn lyrics_window_toggle(
    kind: String,
    place: Option<Place>,
    app: AppHandle,
) -> Result<bool, String> {
    let kind = Kind::of(&kind)?;
    app.state::<Shelf>()
        .toggle(&Tauri(&app), kind, &settled(kind, place))
        .await
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
    set_full(w, place.full, place.px);
    lock(app, kind, w, place.locked);
}

fn set_full(w: &WebviewWindow, full: bool, px: Option<Px>) {
    if w.is_fullscreen().unwrap_or(false) == full {
        return;
    }
    let _ = w.set_fullscreen(full);
    // Back to a window: where it was as one.
    if !full && let Some(p) = px {
        put(w, p);
    }
}

/// Change a shown lyrics window: full screen or not (back to `px` as a window), on top or
/// not, locked or not. What isn't given stays.
#[tauri::command]
pub fn lyrics_window_set(
    kind: String,
    full: Option<bool>,
    on_top: Option<bool>,
    locked: Option<bool>,
    px: Option<Px>,
    app: AppHandle,
) -> Result<(), String> {
    let kind = Kind::of(&kind)?;
    let Some(w) = window(&app, kind) else {
        return Ok(());
    };
    if let Some(full) = full {
        set_full(&w, full, px);
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
    let Some(w) = window(&app, kind) else {
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
    Ok(window(&app, Kind::of(&kind)?).is_some())
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
        px: None,
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
    window(&app, Kind::Overlay).is_some()
}

/// Show the desktop lyrics where they were (`px`: x, y, width, height in screen pixels),
/// or along the bottom of the screen Venuta is on; or close them.
#[tauri::command]
pub async fn desktop_lyrics(
    open: bool,
    px: Option<Px>,
    locked: bool,
    app: AppHandle,
) -> Result<(), String> {
    let place = Place {
        px,
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
            let Some(w) = window(&app, kind) else {
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
                let _ = w.emit_to(w.label(), POINTER_EVENT, Pointer { inside, over });
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
mod placing {
    use super::{Kind, Screen, place_px};

    // A 4K primary at 150 %, and a 1080p screen to its right at 100 %.
    const BIG: Screen = Screen {
        x: 0,
        y: 0,
        w: 3840,
        h: 2100,
        scale: 1.5,
    };
    const SIDE: Screen = Screen {
        x: 3840,
        y: 0,
        w: 1920,
        h: 1040,
        scale: 1.0,
    };
    // A 1080p screen to the left of the primary, at 125 % (negative coordinates).
    const LEFT: Screen = Screen {
        x: -1920,
        y: 0,
        w: 1920,
        h: 1040,
        scale: 1.25,
    };

    fn inside(p: (i32, i32, i32, i32), s: &Screen) -> bool {
        p.0 >= s.x && p.1 >= s.y && p.0 + p.2 <= s.x + s.w && p.1 + p.3 <= s.y + s.h
    }

    #[test]
    fn the_first_place_is_on_the_players_screen_in_its_own_pixels() {
        for (i, s) in [BIG, SIDE, LEFT].iter().enumerate() {
            let p = place_px(Kind::Overlay, &[BIG, SIDE, LEFT], None, i).unwrap();
            assert!(inside(p, s), "{p:?} on screen {i}");
            // 920 x 200 at that screen's scale, along its bottom, in the middle.
            assert_eq!(p.2, (920.0 * s.scale).round() as i32);
            assert_eq!(p.3, (200.0 * s.scale).round() as i32);
            assert_eq!(p.0 - s.x, s.w - p.0 + s.x - p.2, "centred");
        }
    }

    #[test]
    fn a_kept_place_on_a_connected_screen_is_used_as_it_was() {
        let kept = (4000, 700, 900, 200);
        assert_eq!(
            place_px(Kind::Overlay, &[BIG, SIDE], Some(kept), 0),
            Some(kept)
        );
        let left = (-1800, 300, 1000, 240);
        assert_eq!(
            place_px(Kind::Overlay, &[BIG, LEFT], Some(left), 0),
            Some(left)
        );
    }

    #[test]
    fn a_place_on_a_screen_no_longer_there_moves_to_the_players_screen() {
        // Kept on the side screen, which is unplugged now.
        let p = place_px(Kind::Overlay, &[BIG], Some((4000, 700, 900, 200)), 0).unwrap();
        assert!(inside(p, &BIG), "{p:?}");
        assert_eq!(p.2, 1380, "sized for the screen it's on now");
    }

    #[test]
    fn a_place_partly_off_its_screen_is_brought_fully_onto_it() {
        // Mostly below the side screen's bottom, and wider than it.
        let p = place_px(Kind::Overlay, &[BIG, SIDE], Some((4500, 980, 2400, 200)), 0).unwrap();
        assert!(inside(p, &SIDE), "{p:?}");
        assert_eq!(p.2, SIDE.w, "no wider than the screen");
        // Across the seam between two screens: onto the one holding most of it.
        let p = place_px(Kind::Overlay, &[BIG, SIDE], Some((3700, 600, 900, 200)), 0).unwrap();
        assert!(inside(p, &SIDE), "{p:?}");
    }

    #[test]
    fn a_sliver_on_a_screen_is_not_enough_to_keep() {
        // Only 40 pixels of it on any screen: too little to see or grab.
        let p = place_px(Kind::Overlay, &[BIG], Some((3800, 500, 900, 200)), 0).unwrap();
        assert!(inside(p, &BIG));
        assert_ne!(p.0, 3800);
    }

    #[test]
    fn no_screens_known_means_no_place_to_set() {
        assert_eq!(place_px(Kind::Overlay, &[], None, 0), None);
        assert_eq!(
            place_px(Kind::Overlay, &[], Some((1, 2, 300, 100)), 0),
            None
        );
    }
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

#[cfg(test)]
mod one_of_each {
    //! The lost lyrics window (2026-10-21): pressed twice while it was being made, Tauri made two
    //! windows of one label, and the end of either made it forget the label, so the other lived on
    //! unknown, deaf to the app, and every press after made another. These hold a host that works
    //! as Tauri does: a window takes a while to make, its end comes a little after it is asked,
    //! and its end is told by label.
    use super::{Book, Host, Kind, Place, Shelf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct Fake {
        shelf: Arc<Shelf>,
        /// The windows there are, by label.
        windows: Mutex<Vec<String>>,
        /// Windows only the system still knows (lost by the app before).
        lost: AtomicUsize,
        /// The most of one kind there ever were at once.
        most: AtomicUsize,
        made: AtomicUsize,
    }

    impl Fake {
        fn with(shelf: Arc<Shelf>) -> Arc<Fake> {
            Arc::new(Fake {
                shelf,
                ..Default::default()
            })
        }
        fn of(&self, kind: Kind) -> usize {
            self.windows
                .lock()
                .unwrap()
                .iter()
                .filter(|l| Kind::of_label(l) == Some(kind))
                .count()
                + self.lost.load(Ordering::SeqCst)
        }
    }

    impl Host for Arc<Fake> {
        fn has(&self, label: &str) -> bool {
            self.windows.lock().unwrap().iter().any(|l| l == label)
        }
        fn labels(&self) -> Vec<String> {
            self.windows.lock().unwrap().clone()
        }
        async fn make(&self, kind: Kind, label: &str, _place: &Place) -> Result<(), String> {
            // A window and its page take a while to make.
            tokio::time::sleep(Duration::from_millis(40)).await;
            self.windows.lock().unwrap().push(label.to_owned());
            self.made.fetch_add(1, Ordering::SeqCst);
            self.most.fetch_max(self.of(kind), Ordering::SeqCst);
            Ok(())
        }
        fn bring(&self, _kind: Kind, _label: &str, _place: &Place) {}
        fn end(&self, label: &str) {
            // Gone a little later, and told by its label, as Tauri tells it.
            let me = self.clone();
            let label = label.to_owned();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(25)).await;
                me.windows.lock().unwrap().retain(|l| *l != label);
                me.shelf.gone(&label);
            });
        }
        async fn end_lost(&self, _kind: Kind) -> usize {
            self.lost.swap(0, Ordering::SeqCst)
        }
    }

    fn place() -> Place {
        Place {
            px: None,
            full: false,
            on_top: true,
            locked: true,
            monitor: None,
        }
    }

    #[test]
    fn every_window_has_a_label_of_its_own() {
        let mut book = Book::default();
        let a = book.name(Kind::Overlay);
        let b = book.name(Kind::Overlay);
        let c = book.name(Kind::Stage);
        assert_ne!(a, b);
        assert_eq!(Kind::of_label(&a), Some(Kind::Overlay));
        assert_eq!(Kind::of_label(&c), Some(Kind::Stage));
        assert_eq!(book.current(Kind::Overlay), Some(b));
    }

    #[test]
    fn an_old_windows_end_never_forgets_the_new_one() {
        let mut book = Book::default();
        let old = book.name(Kind::Overlay);
        let new = book.name(Kind::Overlay);
        assert_eq!(book.gone(&old), None, "the old one's end changes nothing");
        assert_eq!(book.current(Kind::Overlay), Some(new.clone()));
        assert_eq!(book.gone(&new), Some(Kind::Overlay));
        assert_eq!(book.current(Kind::Overlay), None);
        assert_eq!(book.gone("main"), None);
    }

    #[test]
    fn labels_are_read_back_to_their_kind() {
        assert_eq!(Kind::of_label("overlay"), Some(Kind::Overlay));
        assert_eq!(Kind::of_label("overlay-12"), Some(Kind::Overlay));
        assert_eq!(Kind::of_label("stage-3"), Some(Kind::Stage));
        for other in [
            "main",
            "mini",
            "overlay-",
            "overlay-x",
            "stagehand",
            "ytm-botguard-1",
        ] {
            assert_eq!(Kind::of_label(other), None, "{other}");
        }
    }

    #[tokio::test]
    async fn two_asks_at_once_make_one_window() {
        let shelf = Arc::new(Shelf::default());
        let host = Fake::with(shelf.clone());
        let p = place();
        let (a, b) = tokio::join!(
            shelf.open(&host, Kind::Overlay, &p),
            shelf.open(&host, Kind::Overlay, &p)
        );
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(
            host.made.load(Ordering::SeqCst),
            1,
            "the second brings it forward"
        );
        assert_eq!(host.most.load(Ordering::SeqCst), 1);
        assert!(shelf.live(&host, Kind::Overlay).is_some());
    }

    #[tokio::test]
    async fn presses_in_quick_succession_open_close_and_open_with_never_two() {
        let shelf = Arc::new(Shelf::default());
        let host = Fake::with(shelf.clone());
        let p = place();
        let (a, b, c) = tokio::join!(
            shelf.toggle(&host, Kind::Overlay, &p),
            shelf.toggle(&host, Kind::Overlay, &p),
            shelf.toggle(&host, Kind::Overlay, &p)
        );
        assert_eq!((a, b, c), (Ok(true), Ok(false), Ok(true)));
        assert_eq!(host.most.load(Ordering::SeqCst), 1);
        assert_eq!(host.of(Kind::Overlay), 1);
        // The one there is the one the app knows: asked about, it answers.
        let live = shelf.live(&host, Kind::Overlay).expect("known");
        assert!(host.has(&live));
        // And one more press closes it: nothing is left behind.
        assert_eq!(shelf.toggle(&host, Kind::Overlay, &p).await, Ok(false));
        assert_eq!(host.of(Kind::Overlay), 0);
        assert!(shelf.live(&host, Kind::Overlay).is_none());
    }

    #[tokio::test]
    async fn a_close_is_done_only_once_the_window_is_gone() {
        let shelf = Arc::new(Shelf::default());
        let host = Fake::with(shelf.clone());
        shelf.open(&host, Kind::Stage, &place()).await.unwrap();
        shelf.close(&host, Kind::Stage).await;
        assert_eq!(host.of(Kind::Stage), 0);
        // Opened again at once: a window of its own, never one still ending.
        shelf.open(&host, Kind::Stage, &place()).await.unwrap();
        assert_eq!(host.of(Kind::Stage), 1);
        assert_eq!(host.most.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn anything_of_a_kind_left_about_goes_before_another_is_made() {
        let shelf = Arc::new(Shelf::default());
        let host = Fake::with(shelf.clone());
        // One the app knows by label but isn't its window now, and one only the system knows.
        host.windows.lock().unwrap().push("overlay-90".into());
        host.windows.lock().unwrap().push("stage-91".into());
        host.lost.store(1, Ordering::SeqCst);
        shelf.open(&host, Kind::Overlay, &place()).await.unwrap();
        assert_eq!(host.of(Kind::Overlay), 1, "only the new one");
        let live = shelf.live(&host, Kind::Overlay).unwrap();
        assert_ne!(live, "overlay-90");
        assert!(host.has("stage-91"), "the other kind's is its own");
    }

    #[tokio::test]
    async fn the_kinds_wait_only_for_their_own() {
        let shelf = Arc::new(Shelf::default());
        let host = Fake::with(shelf.clone());
        let p = place();
        let (a, b) = tokio::join!(
            shelf.open(&host, Kind::Stage, &p),
            shelf.open(&host, Kind::Overlay, &p)
        );
        assert!(a.is_ok() && b.is_ok());
        assert_eq!((host.of(Kind::Stage), host.of(Kind::Overlay)), (1, 1));
    }
}
