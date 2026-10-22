//! Windows kept one of each kind, whatever asks for them (a button, the tray, a shortcut,
//! Settings) and however quickly: the big screen and the desktop lyrics (`lyric_windows`), and
//! the mini player (`playback`).
//!
//! Tauri knows its windows by label, and it checks a label is free before it makes the window and
//! takes the window in only once it is made; and a window's end makes it forget whichever window
//! has that label by then. So a second ask while the first window was being made (a button pressed
//! again, about a tenth of a second) made a second window of the same label, and the end of either
//! left the other unknown: alive on screen, see-through with nothing in it, its every call to the
//! app failing ("failed to acquire webview reference"), and the button reading it as closed and
//! making another at each press. Measured 2026-10-21: ten such windows in a test copy.
//!
//! So here: each opening and closing of a kind waits its turn; a closing is done only once the
//! window is gone; each window has a label never used before (the kind and a number), so the end
//! of an old one can never be taken for the new one's; and before a window is made, anything of
//! its kind still about (Tauri's, or one only Windows still knows) is cleared away.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// A kind of window there is at most one of.
pub(crate) trait WindowKind: Copy + Eq + Hash + Send + Sync + 'static {
    /// Every kind of its family (each has its own turn).
    const ALL: &'static [Self];
    /// Whose windows they are, for the log ("the lyrics windows").
    const WHOSE: &'static str;
    /// Its name: the start of each of its windows' labels.
    fn label(self) -> &'static str;
    /// Its windows' title (how one the app lost is found).
    fn title(self) -> &'static str;
    /// The kind of one of its windows, from its label (`overlay-3`; the plain `overlay` too).
    fn of_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| {
            label == k.label()
                || label
                    .strip_prefix(k.label())
                    .and_then(|r| r.strip_prefix('-'))
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
    }
}

/// Which window is each kind's now.
#[derive(Debug)]
pub(crate) struct Book<K> {
    current: HashMap<K, String>,
    made: u64,
}

impl<K> Default for Book<K> {
    fn default() -> Self {
        Book {
            current: HashMap::new(),
            made: 0,
        }
    }
}

impl<K: WindowKind> Book<K> {
    pub(crate) fn current(&self, kind: K) -> Option<String> {
        self.current.get(&kind).cloned()
    }
    /// A label for a new window of `kind`, never used before; that kind's window from now.
    pub(crate) fn name(&mut self, kind: K) -> String {
        self.made += 1;
        let label = format!("{}-{}", kind.label(), self.made);
        self.current.insert(kind, label.clone());
        label
    }
    /// The window `label` is gone: its kind, if it was that kind's window now.
    pub(crate) fn gone(&mut self, label: &str) -> Option<K> {
        let kind = K::of_label(label)?;
        if self.current.get(&kind).is_some_and(|l| l == label) {
            self.current.remove(&kind);
            Some(kind)
        } else {
            None
        }
    }
}

/// What the shelf needs of the windows themselves: Tauri's, or a test's.
pub(crate) trait Host<K: WindowKind> {
    /// How a window is to be placed and behave.
    type Place;
    /// Whether the window `label` is there.
    fn has(&self, label: &str) -> bool;
    /// Every window there is, by label.
    fn labels(&self) -> Vec<String>;
    /// Make the window `label` for `kind`, placed as `place` says, and show it.
    async fn make(&self, kind: K, label: &str, place: &Self::Place) -> Result<(), String>;
    /// Bring the window `label` forward, to `place`'s ways.
    fn bring(&self, kind: K, label: &str, place: &Self::Place);
    /// End the window `label`: gone once `has` says so.
    fn end(&self, label: &str);
    /// End any window of `kind` only the system still knows (one the app lost), and wait until
    /// they are gone. How many there were.
    async fn end_lost(&self, kind: K) -> usize;
    /// A closing of `kind` is done (whether there was a window to close or not).
    fn closed(&self, _kind: K) {}
}

/// How long a window may take to end before it is left to itself.
const ENDING: Duration = Duration::from_secs(3);

/// The windows of a family of kinds: at most one of each, whoever asks and however quickly.
pub(crate) struct Shelf<K: WindowKind> {
    turns: HashMap<K, tokio::sync::Mutex<()>>,
    book: Mutex<Book<K>>,
}

impl<K: WindowKind> Default for Shelf<K> {
    fn default() -> Self {
        Shelf {
            turns: K::ALL
                .iter()
                .map(|&k| (k, tokio::sync::Mutex::new(())))
                .collect(),
            book: Mutex::default(),
        }
    }
}

impl<K: WindowKind> Shelf<K> {
    /// Each kind's openings and closings, one at a time (the kinds apart).
    fn turn(&self, kind: K) -> &tokio::sync::Mutex<()> {
        &self.turns[&kind]
    }
    fn book(&self) -> MutexGuard<'_, Book<K>> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// The label of the window that is `kind`'s now (there or not).
    pub(crate) fn current(&self, kind: K) -> Option<String> {
        self.book().current(kind)
    }
    /// The label of `kind`'s window, if it is there.
    pub(crate) fn live<H: Host<K>>(&self, host: &H, kind: K) -> Option<String> {
        let label = self.current(kind)?;
        host.has(&label).then_some(label)
    }
    /// A window gone: its kind, if it was that kind's window now.
    pub(crate) fn gone(&self, label: &str) -> Option<K> {
        self.book().gone(label)
    }

    pub(crate) async fn open<H: Host<K>>(
        &self,
        host: &H,
        kind: K,
        place: &H::Place,
    ) -> Result<(), String> {
        let _turn = self.turn(kind).lock().await;
        self.open_now(host, kind, place).await
    }

    pub(crate) async fn close<H: Host<K>>(&self, host: &H, kind: K) {
        let _turn = self.turn(kind).lock().await;
        self.close_now(host, kind).await;
    }

    /// Closed if it is showing, else opened: whether it shows now.
    pub(crate) async fn toggle<H: Host<K>>(
        &self,
        host: &H,
        kind: K,
        place: &H::Place,
    ) -> Result<bool, String> {
        let _turn = self.turn(kind).lock().await;
        if self.live(host, kind).is_some() {
            self.close_now(host, kind).await;
            Ok(false)
        } else {
            self.open_now(host, kind, place).await.map(|()| true)
        }
    }

    async fn open_now<H: Host<K>>(
        &self,
        host: &H,
        kind: K,
        place: &H::Place,
    ) -> Result<(), String> {
        if let Some(label) = self.live(host, kind) {
            host.bring(kind, &label, place);
            return Ok(());
        }
        // Never two of a kind: whatever of it is still about goes first.
        for label in host
            .labels()
            .into_iter()
            .filter(|l| K::of_label(l) == Some(kind))
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
                K::WHOSE,
            );
        }
        let label = self.book().name(kind);
        let made = host.make(kind, &label, place).await;
        if made.is_err() {
            self.book().gone(&label);
        }
        made
    }

    async fn close_now<H: Host<K>>(&self, host: &H, kind: K) {
        if let Some(label) = self.live(host, kind) {
            self.end_now(host, &label).await;
        }
        host.closed(kind);
    }

    /// End a window and wait until it is gone (or has taken too long: then it is left, still
    /// known, so a press can try again).
    async fn end_now<H: Host<K>>(&self, host: &H, label: &str) {
        host.end(label);
        let until = Instant::now() + ENDING;
        while host.has(label) {
            if Instant::now() >= until {
                crate::playback::note_play(
                    &format!("{label} didn't close in {} s", ENDING.as_secs()),
                    K::WHOSE,
                );
                return;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
}

// --- What every Tauri host does alike ---------------------------------------------------------

/// Whether Tauri has the window `label`.
pub(crate) fn has(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label).is_some()
}

/// Every window Tauri has, by label.
pub(crate) fn labels(app: &AppHandle) -> Vec<String> {
    app.webview_windows().into_keys().collect()
}

/// End the window `label`. Destroyed, not asked to close: nothing on its page can hold it open,
/// and a page that can no longer reach the app (as a lost one couldn't) goes all the same.
pub(crate) fn end(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.destroy();
    }
}

/// End the windows titled `title` that Windows has and the app doesn't know, and wait until they
/// are gone (or have taken too long). How many there were.
pub(crate) async fn end_lost(app: &AppHandle, title: &'static str) -> usize {
    #[cfg(windows)]
    {
        let known: Vec<isize> = app
            .webview_windows()
            .values()
            .filter_map(|w| w.hwnd().ok())
            .map(|h| h.0 as isize)
            .collect();
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
        let _ = (app, title);
        0
    }
}

/// Windows of Venuta's that Windows has and the app doesn't know (anything of a window's title
/// not among the app's own): found, and ended the way the app ends its own (asked of the window's
/// own thread).
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

/// A host that works as Tauri does, for the tests of every kind: a window takes a while to make,
/// its end comes a little after it is asked, and its end is told by label.
#[cfg(test)]
pub(crate) mod fake {
    use super::{Host, Shelf, WindowKind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub(crate) struct Fake<K: WindowKind> {
        pub shelf: Arc<Shelf<K>>,
        /// The windows there are, by label.
        pub windows: Mutex<Vec<String>>,
        /// Windows only the system still knows (lost by the app before).
        pub lost: AtomicUsize,
        /// The most of one kind there ever were at once.
        pub most: AtomicUsize,
        pub made: AtomicUsize,
        /// What was last done for each kind: shown (made or brought forward) or closed.
        pub last: Mutex<Vec<(K, bool)>>,
    }

    impl<K: WindowKind> Fake<K> {
        pub fn with(shelf: Arc<Shelf<K>>) -> Arc<Fake<K>> {
            Arc::new(Fake {
                shelf,
                windows: Mutex::default(),
                lost: AtomicUsize::new(0),
                most: AtomicUsize::new(0),
                made: AtomicUsize::new(0),
                last: Mutex::default(),
            })
        }
        /// The windows of `kind` there are (the lost ones too).
        pub fn of(&self, kind: K) -> usize {
            self.windows
                .lock()
                .unwrap()
                .iter()
                .filter(|l| K::of_label(l) == Some(kind))
                .count()
                + self.lost.load(Ordering::SeqCst)
        }
        /// Whether what was last done for `kind` showed it (None: nothing done yet).
        pub fn shown(&self, kind: K) -> Option<bool> {
            self.last
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|(k, _)| *k == kind)
                .map(|(_, s)| *s)
        }
        fn did(&self, kind: K, shown: bool) {
            self.last.lock().unwrap().push((kind, shown));
        }
    }

    impl<K: WindowKind> Host<K> for Arc<Fake<K>> {
        type Place = ();
        fn has(&self, label: &str) -> bool {
            self.windows.lock().unwrap().iter().any(|l| l == label)
        }
        fn labels(&self) -> Vec<String> {
            self.windows.lock().unwrap().clone()
        }
        async fn make(&self, kind: K, label: &str, _place: &()) -> Result<(), String> {
            // A window and its page take a while to make.
            tokio::time::sleep(Duration::from_millis(40)).await;
            self.windows.lock().unwrap().push(label.to_owned());
            self.made.fetch_add(1, Ordering::SeqCst);
            self.most.fetch_max(self.of(kind), Ordering::SeqCst);
            self.did(kind, true);
            Ok(())
        }
        fn bring(&self, kind: K, _label: &str, _place: &()) {
            self.did(kind, true);
        }
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
        async fn end_lost(&self, _kind: K) -> usize {
            self.lost.swap(0, Ordering::SeqCst)
        }
        fn closed(&self, kind: K) {
            self.did(kind, false);
        }
    }
}
