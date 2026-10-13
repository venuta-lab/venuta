//! Where Venuta keeps its own data.

use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "Venuta";

/// The folder used before the app was named; moved to the new name on first start.
const OLD_APP_DIR_NAME: &str = "music-librarian";

/// Per-user application data directory (index, caches, journals).
pub fn app_data_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    let base = base.unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join(APP_DIR_NAME);
    // Settings, indexes, caches and journals from before the rename come along, once.
    static MOVED: std::sync::Once = std::sync::Once::new();
    MOVED.call_once(|| {
        let old = base.join(OLD_APP_DIR_NAME);
        if !dir.exists() && old.is_dir() {
            let _ = std::fs::rename(&old, &dir);
        }
    });
    dir
}

/// Index database location for a library root. One index per library, named by a hash of
/// the root so several libraries can coexist.
pub fn index_path_for(root: &Path) -> PathBuf {
    let key = canonical_root_string(root);
    let short = blake3::hash(key.as_bytes()).to_hex();
    app_data_dir()
        .join("libraries")
        .join(&short[..16])
        .join("index.sqlite")
}

/// Normalised string form of a library root, used to bind an index to its library.
pub fn canonical_root_string(root: &Path) -> String {
    let p = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let s = p.to_string_lossy().replace('\\', "/");
    // Strip the Windows verbatim prefix that canonicalize adds.
    s.strip_prefix("//?/").map(str::to_owned).unwrap_or(s)
}
