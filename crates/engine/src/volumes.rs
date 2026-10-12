//! Drives: which are attached, which are removable, how much space they have, and ejecting
//! them safely. Windows uses the system APIs directly; macOS/Linux use their standard tools.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Volume {
    pub root: String,
    pub label: String,
    pub filesystem: String,
    /// Changes when the card is reformatted; a secondary check behind the card's own id file.
    pub serial: u32,
    pub removable: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Attached volumes (on Windows, every drive letter).
#[cfg(windows)]
pub fn list() -> Vec<Volume> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let w = wide(&root);
        let kind = unsafe { GetDriveTypeW(w.as_ptr()) };
        if kind != DRIVE_REMOVABLE && kind != DRIVE_FIXED {
            continue; // skip network, optical, RAM disks
        }
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        let mut serial = 0u32;
        let ok = unsafe {
            GetVolumeInformationW(
                w.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                &mut serial,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        };
        if ok == 0 {
            continue; // e.g. an empty card reader slot
        }
        let text = |b: &[u16]| {
            String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
        };
        let (total, free) = space(Path::new(&root)).unwrap_or((0, 0));
        out.push(Volume {
            root,
            label: text(&label),
            filesystem: text(&fs),
            serial,
            removable: kind == DRIVE_REMOVABLE,
            total_bytes: total,
            free_bytes: free,
        });
    }
    out
}

#[cfg(not(windows))]
pub fn list() -> Vec<Volume> {
    let bases = if cfg!(target_os = "macos") {
        vec!["/Volumes".to_owned()]
    } else {
        let user = std::env::var("USER").unwrap_or_default();
        vec![
            format!("/media/{user}"),
            format!("/run/media/{user}"),
            "/mnt".to_owned(),
        ]
    };
    let mut out = Vec::new();
    for base in bases {
        for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            let root = e.path().to_string_lossy().into_owned();
            out.push(Volume {
                label: e.file_name().to_string_lossy().into_owned(),
                root,
                filesystem: String::new(),
                serial: 0,
                removable: true,
                total_bytes: 0,
                free_bytes: 0,
            });
        }
    }
    out
}

/// (total, free) bytes for the volume holding `path`.
#[cfg(windows)]
pub fn space(path: &Path) -> Option<(u64, u64)> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let w = wide(&path.to_string_lossy());
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some((total, avail))
}

#[cfg(not(windows))]
pub fn space(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// Ask the system to eject a removable volume. Fails (with the reason) if a program still
/// has files open on it; nothing is forced.
pub fn eject(root: &Path) -> Result<(), String> {
    let root_s = root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_owned();
    let status = if cfg!(windows) {
        // The shell's own "Eject" verb: the same path Explorer uses, including its checks.
        let script = format!(
            "$d = (New-Object -ComObject Shell.Application).Namespace(17).ParseName('{root_s}'); if ($d) {{ $d.InvokeVerb('Eject') }} else {{ exit 2 }}"
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("diskutil").args(["eject", &root_s]).status()
    } else {
        std::process::Command::new("udisksctl").args(["unmount", "-b", &root_s]).status()
    }
    .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(
            "The system would not eject it. Close any program using the card and try again.".into(),
        );
    }
    // The shell verb returns before the volume is gone; wait briefly to report the truth.
    for _ in 0..20 {
        if !PathBuf::from(format!("{root_s}\\")).exists() && !root.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    if root.exists() {
        Err("The card is still attached. Something may still be using it.".into())
    } else {
        Ok(())
    }
}
