//! The app's icon, chosen in Settings from the ones in `public/app-icons`: the windows, the
//! tray and (on Windows) the Start menu, desktop and pinned taskbar shortcuts that open this
//! Venuta. The program file keeps the default it was built with (Windows reads that from the
//! file itself). The choice is this computer's, kept beside `library.json`.

use std::path::PathBuf;
use tauri::{AppHandle, Manager, image::Image};

/// The icon Venuta has unless another is chosen (the one built into the program).
pub const DEFAULT: &str = "dusk-glass";

fn choice_file() -> PathBuf {
    // A test copy keeps its own choice, so trying icons never changes the Venuta in use.
    if crate::test_copy() {
        return engine::paths::app_data_dir().join("app-icon");
    }
    engine::paths::user_dir().join("app-icon")
}

/// An icon's name: lower-case letters, digits and dashes, as the files are named.
fn valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The chosen icon's name (the default when none was chosen, or the choice is gone).
pub fn chosen() -> String {
    std::fs::read_to_string(choice_file())
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| valid(s))
        .unwrap_or_else(|| DEFAULT.to_owned())
}

/// An icon's picture (256 px PNG), from the app's own files.
fn png(app: &AppHandle, id: &str) -> Option<Vec<u8>> {
    if !valid(id) {
        return None;
    }
    // While developing, the interface is served by Vite and its files aren't inside the app.
    if cfg!(debug_assertions) {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../public/app-icons")
            .join(format!("{id}.png"));
        if let Ok(b) = std::fs::read(p) {
            return Some(b);
        }
    }
    app.asset_resolver()
        .get(format!("app-icons/{id}.png"))
        .map(|a| a.bytes)
}

/// The chosen icon as a window and tray icon (None: the default, which they already have).
pub fn image(app: &AppHandle) -> Option<Image<'static>> {
    let id = chosen();
    if id == DEFAULT {
        return None;
    }
    Image::from_bytes(&png(app, &id)?)
        .ok()
        .map(|i| i.to_owned())
}

/// Every open window and the tray take the chosen icon (or the default back).
pub fn apply(app: &AppHandle) {
    let icon = image(app).or_else(|| app.default_window_icon().cloned().map(|i| i.to_owned()));
    let Some(icon) = icon else { return };
    for w in app.webview_windows().values() {
        let _ = w.set_icon(icon.clone());
    }
    if let Some(tray) = app.tray_by_id("venuta") {
        let _ = tray.set_icon(Some(icon));
    }
}

/// The icon in use.
#[tauri::command]
pub fn app_icon() -> String {
    chosen()
}

/// Use this icon from now on: the windows and tray at once, and the shortcuts on this
/// computer that open Venuta.
#[tauri::command]
pub async fn set_app_icon(app: AppHandle, id: String) -> Result<(), String> {
    let bytes = png(&app, &id).ok_or("That icon isn't one of Venuta's.")?;
    if id == DEFAULT {
        let _ = std::fs::remove_file(choice_file());
    } else {
        if let Some(dir) = choice_file().parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(choice_file(), &id).map_err(|e| e.to_string())?;
    }
    apply(&app);
    // Shortcuts open the Venuta in use, so a test copy leaves them alone.
    #[cfg(windows)]
    if !crate::test_copy() {
        let ico = if id == DEFAULT {
            None
        } else {
            Some(shortcuts::write_ico(&id, &bytes)?)
        };
        tauri::async_runtime::spawn_blocking(move || shortcuts::point_at(ico.as_deref()))
            .await
            .map_err(|e| e.to_string())??;
    }
    #[cfg(not(windows))]
    let _ = bytes;
    Ok(())
}

#[cfg(windows)]
mod shortcuts {
    use std::path::{Path, PathBuf};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile, STGM_READWRITE,
    };
    use windows::Win32::UI::Shell::{
        IShellLinkW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, ShellLink,
    };
    use windows::core::{HSTRING, Interface};

    /// The icon as a Windows .ico (256 down to 16 px), kept beside `library.json`.
    pub fn write_ico(id: &str, png: &[u8]) -> Result<PathBuf, String> {
        use image::codecs::ico::{IcoEncoder, IcoFrame};
        let img = image::load_from_memory(png).map_err(|e| e.to_string())?;
        let frames = [256u32, 64, 48, 32, 24, 16]
            .iter()
            .map(|&s| {
                let small = img.resize_exact(s, s, image::imageops::FilterType::Lanczos3);
                let rgba = small.to_rgba8();
                IcoFrame::as_png(rgba.as_raw(), s, s, image::ExtendedColorType::Rgba8)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dir = engine::paths::user_dir().join("icons");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{id}.ico"));
        let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        IcoEncoder::new(file)
            .encode_images(&frames)
            .map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// The shortcuts that may open Venuta: the Start menu, the desktop, pinned to the taskbar.
    fn candidates() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
            out.push(app_data.join(r"Microsoft\Windows\Start Menu\Programs\Venuta.lnk"));
            out.push(
                app_data.join(
                    r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\Venuta.lnk",
                ),
            );
        }
        if let Some(profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            out.push(profile.join(r"Desktop\Venuta.lnk"));
            out.push(profile.join(r"OneDrive\Desktop\Venuta.lnk"));
        }
        out
    }

    fn same(a: &Path, b: &Path) -> bool {
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }

    /// Each shortcut that opens this Venuta shows `ico` (None: the program's own icon again).
    /// Shortcuts to anything else are left alone.
    pub fn point_at(ico: Option<&Path>) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = (|| -> windows::core::Result<()> {
                for lnk in candidates().into_iter().filter(|p| p.is_file()) {
                    let link: IShellLinkW =
                        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                    let file: IPersistFile = link.cast()?;
                    if file
                        .Load(&HSTRING::from(lnk.as_os_str()), STGM_READWRITE)
                        .is_err()
                    {
                        continue;
                    }
                    let mut target = [0u16; 1024];
                    if link.GetPath(&mut target, std::ptr::null_mut(), 0).is_err() {
                        continue;
                    }
                    let len = target.iter().position(|&c| c == 0).unwrap_or(target.len());
                    let target = PathBuf::from(String::from_utf16_lossy(&target[..len]));
                    if !same(&target, &exe) {
                        continue;
                    }
                    let (icon, index) = match ico {
                        Some(p) => (p.to_path_buf(), 0),
                        None => (exe.clone(), 0),
                    };
                    link.SetIconLocation(&HSTRING::from(icon.as_os_str()), index)?;
                    file.Save(None, true)?;
                }
                Ok(())
            })();
            // Explorer and the taskbar draw shortcuts from a cache: tell them it changed.
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
            CoUninitialize();
            result.map_err(|e| e.to_string())
        }
    }
}
