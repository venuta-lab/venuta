//! The big screen clears its screen: while it shows, the other windows on that screen are
//! minimized (the ones Win+M would: visible app windows, never tool windows such as the desktop
//! lyrics and the mini player, the taskbar, the desktop or hidden ones), and when it closes they
//! come back where and as they were (maximized ones maximized), in their order, and the one you
//! were using has the focus again. Only that screen: a TV beside your desk leaves the desk alone.

use std::sync::Mutex;

/// What is known of a window, to tell whether it is cleared away.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    pub visible: bool,
    pub minimized: bool,
    /// A tool window (no taskbar button: the desktop lyrics, the mini player, palettes).
    pub tool: bool,
    /// Owned by another window (a dialog of it): it goes with its owner.
    pub owned: bool,
    /// Hidden by the system (an app suspended in the background).
    pub cloaked: bool,
    pub titled: bool,
    /// It has a minimize button (the desktop and the taskbar have none).
    pub minimize_box: bool,
    /// The desktop or a taskbar.
    pub shell: bool,
    /// On the screen the big screen fills.
    pub same_screen: bool,
    /// The big screen itself.
    pub stage: bool,
}

/// Whether a window is one the big screen clears away.
pub fn clears(f: &Facts) -> bool {
    f.visible
        && !f.minimized
        && !f.tool
        && !f.owned
        && !f.cloaked
        && f.titled
        && f.minimize_box
        && !f.shell
        && f.same_screen
        && !f.stage
}

#[cfg(windows)]
mod win {
    use super::{Facts, clears};
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetForegroundWindow,
        GetWindow, GetWindowLongW, GetWindowPlacement, GetWindowTextLengthW, IsIconic, IsWindow,
        IsWindowVisible, SW_SHOWMAXIMIZED, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE,
        SetForegroundWindow, SetWindowPlacement, ShowWindowAsync, WINDOWPLACEMENT,
        WPF_RESTORETOMAXIMIZED, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX,
    };
    use windows::core::BOOL;

    /// The windows cleared away (top first), each with its placement, and the one in front.
    pub struct Cleared {
        windows: Vec<(isize, WINDOWPLACEMENT)>,
        front: isize,
    }
    // Plain numbers and a plain struct: safe to keep across threads.
    unsafe impl Send for Cleared {}

    const SHELL: [&str; 4] = [
        "Progman",
        "WorkerW",
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
    ];

    fn facts(h: HWND, stage: HWND) -> Facts {
        unsafe {
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            let style = GetWindowLongW(h, GWL_STYLE) as u32;
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            let mut class = [0u16; 64];
            let n = GetClassNameW(h, &mut class).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            Facts {
                visible: IsWindowVisible(h).as_bool(),
                minimized: IsIconic(h).as_bool(),
                tool: ex & WS_EX_TOOLWINDOW.0 != 0,
                owned: GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()),
                cloaked: cloaked != 0,
                titled: GetWindowTextLengthW(h) > 0,
                minimize_box: style & WS_MINIMIZEBOX.0 != 0,
                shell: SHELL.contains(&class.as_str()),
                same_screen: MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST)
                    == MonitorFromWindow(stage, MONITOR_DEFAULTTONEAREST),
                stage: h == stage,
            }
        }
    }

    /// The windows on the big screen's screen, minimized (without taking the focus), and how
    /// each was.
    pub fn clear(stage: isize) -> Cleared {
        let stage = HWND(stage as *mut core::ffi::c_void);
        struct Walk {
            stage: HWND,
            found: Vec<HWND>,
        }
        unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
            let walk = unsafe { &mut *(l.0 as *mut Walk) };
            if clears(&facts(h, walk.stage)) {
                walk.found.push(h);
            }
            BOOL(1)
        }
        let mut walk = Walk {
            stage,
            found: Vec::new(),
        };
        let front = unsafe { GetForegroundWindow() }.0 as isize;
        // Top to bottom, as Windows lists them.
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut walk as *mut Walk as isize)) };
        let mut windows = Vec::new();
        for h in walk.found {
            let mut p = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if unsafe { GetWindowPlacement(h, &mut p) }.is_ok() {
                // Async: a window not answering never holds the big screen up.
                let _ = unsafe { ShowWindowAsync(h, SW_SHOWMINNOACTIVE) };
                windows.push((h.0 as isize, p));
            }
        }
        Cleared { windows, front }
    }

    /// Back as they were: bottom first, so they keep their order; the one in front last, with
    /// the focus. A window closed meanwhile, or brought back by hand, is left alone.
    pub fn restore(c: Cleared) {
        for (h, mut p) in c.windows.into_iter().rev() {
            let h = HWND(h as *mut core::ffi::c_void);
            unsafe {
                if !IsWindow(Some(h)).as_bool() || !IsIconic(h).as_bool() {
                    continue;
                }
                let maximized = p.showCmd == SW_SHOWMAXIMIZED.0 as u32
                    || p.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0;
                p.showCmd = if maximized {
                    SW_SHOWMAXIMIZED.0 as u32
                } else {
                    SW_SHOWNOACTIVATE.0 as u32
                };
                let _ = SetWindowPlacement(h, &p);
            }
        }
        let front = HWND(c.front as *mut core::ffi::c_void);
        if !front.is_invalid() && unsafe { IsWindow(Some(front)) }.as_bool() {
            let _ = unsafe { SetForegroundWindow(front) };
        }
    }
}

#[cfg(windows)]
static CLEARED: Mutex<Option<win::Cleared>> = Mutex::new(None);

/// The big screen is showing (its window `hwnd`): clear its screen (once).
pub fn on_open(hwnd: isize) {
    #[cfg(windows)]
    {
        let mut c = CLEARED.lock().unwrap();
        if c.is_none() {
            *c = Some(win::clear(hwnd));
        }
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// The big screen closed: everything back, off the UI thread (a window that doesn't answer
/// never holds Venuta up).
pub fn on_close() {
    #[cfg(windows)]
    if let Some(c) = CLEARED.lock().unwrap().take() {
        std::thread::Builder::new()
            .name("venuta-screen-back".into())
            .spawn(move || win::restore(c))
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{Facts, clears};

    fn app() -> Facts {
        Facts {
            visible: true,
            titled: true,
            minimize_box: true,
            same_screen: true,
            ..Default::default()
        }
    }

    #[test]
    fn an_app_window_on_that_screen_is_cleared_away() {
        assert!(clears(&app()));
    }

    #[test]
    fn everything_else_stays() {
        let stays = |f: Facts, why: &str| assert!(!clears(&f), "{why}");
        stays(
            Facts {
                same_screen: false,
                ..app()
            },
            "another screen (your desk)",
        );
        stays(
            Facts {
                tool: true,
                ..app()
            },
            "a tool window (desktop lyrics, mini player)",
        );
        stays(
            Facts {
                stage: true,
                ..app()
            },
            "the big screen itself",
        );
        stays(
            Facts {
                minimized: true,
                ..app()
            },
            "already minimized (and so not brought back)",
        );
        stays(
            Facts {
                visible: false,
                ..app()
            },
            "hidden",
        );
        stays(
            Facts {
                cloaked: true,
                ..app()
            },
            "hidden by the system",
        );
        stays(
            Facts {
                owned: true,
                ..app()
            },
            "a dialog (goes with its window)",
        );
        stays(
            Facts {
                titled: false,
                ..app()
            },
            "no title (a helper window)",
        );
        stays(
            Facts {
                minimize_box: false,
                ..app()
            },
            "can't be minimized",
        );
        stays(
            Facts {
                shell: true,
                ..app()
            },
            "the desktop or the taskbar",
        );
    }
}
