//! Where Venuta keeps its own data: inside your library, in `<library>/.venuta`.
//!
//! Everything Venuta knows (settings, the index of each music folder, your answers, undo
//! journals, caches) lives with the library it belongs to, so the library can be moved, backed
//! up or removed as one folder. The only thing kept elsewhere is a note of where the library
//! is ([`POINTER_FILE`] in the per-user folder), because the app needs it to find the library
//! on start. Until a library is chosen, the per-user folder is used as before.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub const APP_DIR_NAME: &str = "Venuta";

/// Venuta's folder inside a library.
pub const DATA_DIR_NAME: &str = ".venuta";

/// Inside a library: the albums (what players and cards see) …
pub const MUSIC_DIR_NAME: &str = "Music";

/// … and, beside them, the folders for working with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Helper {
    /// Your playlists (`.m3u8`).
    Playlists,
    /// Drop new music here to bring it in.
    AddMusic,
    /// Duplicates and copies left out of the library, to check and delete when you like.
    SetAside,
}

impl Helper {
    /// The folder's name beside `Music`, and its name inside an older or outside folder.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Helper::Playlists => ("Playlists", "Playlists"),
            Helper::AddMusic => ("Add music here", "_Incoming"),
            Helper::SetAside => ("Set aside", "_Quarantine"),
        }
    }
}

/// A library's albums folder.
pub fn music_dir(home: &Path) -> PathBuf {
    home.join(MUSIC_DIR_NAME)
}

/// The library a `Music` folder belongs to, if it is one.
pub fn home_of(music: &Path) -> Option<PathBuf> {
    let parent = music.parent()?;
    let named = music
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(MUSIC_DIR_NAME));
    (named && parent.join(DATA_DIR_NAME).is_dir()).then(|| parent.to_path_buf())
}

/// Where a helper folder is for the music in `root`: beside it in a library, inside it
/// otherwise (as before libraries had a `Music` folder).
pub fn beside(root: &Path, helper: Helper) -> PathBuf {
    let (new, old) = helper.names();
    match home_of(root) {
        Some(home) => home.join(new),
        None => root.join(old),
    }
}

/// The note of where the library is, in the per-user folder.
pub const POINTER_FILE: &str = "library.json";

/// The folder used before the app was named; moved to the new name on first start.
const OLD_APP_DIR_NAME: &str = "music-librarian";

/// The per-user folder (`%APPDATA%\Venuta` on Windows). Holds only [`POINTER_FILE`] once a
/// library is chosen.
pub fn user_dir() -> PathBuf {
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

#[derive(Default, Serialize, Deserialize)]
struct Pointer {
    library: Option<String>,
}

/// `None`: not read yet. `Some(None)`: no library chosen.
static HOME: RwLock<Option<Option<PathBuf>>> = RwLock::new(None);

/// Your library, if one is chosen. `VENUTA_HOME` overrides the saved choice (for the CLI).
pub fn home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("VENUTA_HOME") {
        return Some(PathBuf::from(h));
    }
    if let Ok(g) = HOME.read()
        && let Some(h) = g.as_ref()
    {
        return h.clone();
    }
    let saved = std::fs::read_to_string(user_dir().join(POINTER_FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<Pointer>(&s).ok())
        .and_then(|p| p.library)
        .map(PathBuf::from);
    if let Ok(mut g) = HOME.write() {
        *g = Some(saved.clone());
    }
    saved
}

/// Venuta's data folder: `<library>/.venuta`, or the per-user folder before a library exists.
pub fn app_data_dir() -> PathBuf {
    match home() {
        Some(h) => h.join(DATA_DIR_NAME),
        None => user_dir(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeKind {
    /// Missing or empty: a new library starts here.
    New,
    /// A library Venuta already looks after.
    Existing,
}

/// Can this folder be your library? Only an empty (or not yet existing) folder, or one that is
/// already a Venuta library. A folder of other files is never taken over.
pub fn check_home(p: &Path) -> Result<HomeKind, String> {
    if p.join(DATA_DIR_NAME).is_dir()
        || p.join(crate::apply::LIBRARY_MARKER).exists()
        || p.join(crate::apply::OLD_LIBRARY_MARKER).exists()
    {
        return Ok(HomeKind::Existing);
    }
    if p.exists() && !p.is_dir() {
        return Err(format!("{} is a file, not a folder.", p.display()));
    }
    let has_files = std::fs::read_dir(p)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if has_files {
        return Err(format!(
            "{} already has files in it. Your library needs a folder of its own: choose an empty one, \
             or a new one. Music you already have is brought in at the next step.",
            p.display()
        ));
    }
    Ok(HomeKind::New)
}

/// Make `p` a library: its `.venuta` folder (hidden on Windows) and the library marker.
pub fn make_home(p: &Path) -> Result<HomeKind, String> {
    let kind = check_home(p)?;
    let data = p.join(DATA_DIR_NAME);
    std::fs::create_dir_all(&data)
        .map_err(|e| format!("Could not create {}: {e}", data.display()))?;
    std::fs::create_dir_all(music_dir(p)).map_err(|e| e.to_string())?;
    let marker = p.join(crate::apply::LIBRARY_MARKER);
    if !marker.exists() {
        std::fs::write(&marker, "A library built by Venuta.\n").map_err(|e| e.to_string())?;
    }
    hide(&data);
    hide(&marker);
    Ok(kind)
}

/// Choose `p` as your library and remember the choice. Data from before libraries kept their
/// own (the per-user folder) moves in, once. Another library's data stays with it; only your
/// settings (keys, sources, look) come along to a library that has none yet.
pub fn set_home(p: &Path) -> Result<HomeKind, String> {
    let kind = make_home(p)?;
    let to = p.join(DATA_DIR_NAME);
    let previous = home().map(|h| h.join(DATA_DIR_NAME));
    let legacy = user_dir();
    if legacy.is_dir() {
        move_data(&legacy, &to)?;
    }
    if let Some(prev) = previous.filter(|d| *d != to) {
        carry_settings(&prev, &to)?;
    }
    let pointer = Pointer {
        library: Some(p.to_string_lossy().replace('\\', "/")),
    };
    let dir = user_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(POINTER_FILE),
        serde_json::to_vec_pretty(&pointer).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if let Ok(mut g) = HOME.write() {
        *g = Some(Some(p.to_path_buf()));
    }
    Ok(kind)
}

/// Copy `settings.json` from one data folder to another that has none, minus what belongs to
/// the old library (which folder was open).
pub fn carry_settings(from: &Path, to: &Path) -> Result<(), String> {
    let (src, dst) = (from.join("settings.json"), to.join("settings.json"));
    if dst.exists() || !src.exists() {
        return Ok(());
    }
    let mut v: serde_json::Value = std::fs::read_to_string(&src)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.remove("open_folder");
        o.remove("new_library");
    }
    std::fs::write(
        &dst,
        serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Forget the library choice (its folder and data stay where they are).
pub fn forget_home() -> Result<(), String> {
    let f = user_dir().join(POINTER_FILE);
    if f.exists() {
        std::fs::remove_file(&f).map_err(|e| e.to_string())?;
    }
    if let Ok(mut g) = HOME.write() {
        *g = Some(None);
    }
    Ok(())
}

/// Move every entry of `from` into `to`, except [`POINTER_FILE`] and anything `to` already
/// has (that stays in `from`, untouched). Works across drives. Returns how many entries moved.
pub fn move_data(from: &Path, to: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut moved = 0;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == POINTER_FILE {
            continue;
        }
        let dst = to.join(&name);
        if dst.exists() {
            continue;
        }
        let src = entry.path();
        if std::fs::rename(&src, &dst).is_err() {
            // Another drive: copy, check, then remove the original.
            copy_tree(&src, &dst)?;
            if tree_size(&src) != tree_size(&dst) {
                let _ = remove_tree(&dst);
                return Err(format!(
                    "Copying {} did not complete; nothing was removed.",
                    src.display()
                ));
            }
            remove_tree(&src)?;
        }
        moved += 1;
    }
    Ok(moved)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for e in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            copy_tree(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst)
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", src.display()))
    }
}

fn tree_size(p: &Path) -> u64 {
    if p.is_dir() {
        std::fs::read_dir(p)
            .map(|d| d.flatten().map(|e| tree_size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        p.metadata().map(|m| m.len()).unwrap_or(0)
    }
}

fn remove_tree(p: &Path) -> Result<(), String> {
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
    .map_err(|e| e.to_string())
}

/// Hide a file or folder in Explorer (Windows); elsewhere the leading dot already does.
fn hide(p: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("attrib")
            .arg("+h")
            .arg(p)
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(not(windows))]
    let _ = p;
}

/// Index database location for a music folder. One index per folder, named by a hash of
/// the folder so several can coexist.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_folders_sit_beside_the_music() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("Music Library");
        make_home(&home).unwrap();
        let music = music_dir(&home);
        assert!(music.is_dir(), "a library starts with its Music folder");
        assert_eq!(home_of(&music).as_deref(), Some(home.as_path()));
        assert_eq!(beside(&music, Helper::SetAside), home.join("Set aside"));
        assert_eq!(
            beside(&music, Helper::AddMusic),
            home.join("Add music here")
        );
        assert_eq!(beside(&music, Helper::Playlists), home.join("Playlists"));
        // Any other folder (a music folder being brought in, an older library) keeps its
        // helpers inside, as before.
        let old = dir.path().join("Old");
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(home_of(&old), None);
        assert_eq!(beside(&old, Helper::SetAside), old.join("_Quarantine"));
        assert_eq!(beside(&old, Helper::AddMusic), old.join("_Incoming"));
        assert_eq!(beside(&old, Helper::Playlists), old.join("Playlists"));
    }

    #[test]
    fn a_home_must_be_empty_new_or_already_ours() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("Music Library");
        assert_eq!(check_home(&fresh), Ok(HomeKind::New), "does not exist yet");
        std::fs::create_dir_all(&fresh).unwrap();
        assert_eq!(check_home(&fresh), Ok(HomeKind::New), "empty");

        let music = dir.path().join("Collection");
        std::fs::create_dir_all(music.join("Album")).unwrap();
        std::fs::write(music.join("Album/01.flac"), b"x").unwrap();
        assert!(
            check_home(&music).is_err(),
            "a folder of other files is never taken over"
        );

        let ours = dir.path().join("Ours");
        std::fs::create_dir_all(ours.join(DATA_DIR_NAME)).unwrap();
        std::fs::write(ours.join("song.flac"), b"x").unwrap();
        assert_eq!(check_home(&ours), Ok(HomeKind::Existing));

        let built = dir.path().join("Built");
        std::fs::create_dir_all(&built).unwrap();
        std::fs::write(built.join(crate::apply::LIBRARY_MARKER), b"").unwrap();
        std::fs::write(built.join("song.flac"), b"x").unwrap();
        assert_eq!(
            check_home(&built),
            Ok(HomeKind::Existing),
            "a library built before .venuta"
        );
    }

    #[test]
    fn make_home_creates_the_data_folder_and_marker() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("Music Library");
        make_home(&lib).unwrap();
        assert!(lib.join(DATA_DIR_NAME).is_dir());
        assert!(lib.join(crate::apply::LIBRARY_MARKER).is_file());
        // A library is never made inside another folder of music.
        let music = dir.path().join("Collection");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::write(music.join("01.flac"), b"x").unwrap();
        assert!(make_home(&music).is_err());
        assert!(!music.join(DATA_DIR_NAME).exists());
    }

    #[test]
    fn move_data_brings_everything_and_keeps_what_is_already_there() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("AppData");
        let new = dir.path().join("lib/.venuta");
        std::fs::create_dir_all(old.join("thumbs")).unwrap();
        std::fs::write(old.join("thumbs/a.jpg"), b"jpg").unwrap();
        std::fs::write(old.join("settings.json"), b"{\"old\":1}").unwrap();
        std::fs::write(old.join(POINTER_FILE), b"{}").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("settings.json"), b"{\"new\":1}").unwrap();

        let moved = move_data(&old, &new).unwrap();
        assert_eq!(
            moved, 1,
            "thumbs moved; settings already there; the pointer stays"
        );
        assert_eq!(std::fs::read(new.join("thumbs/a.jpg")).unwrap(), b"jpg");
        assert_eq!(
            std::fs::read(new.join("settings.json")).unwrap(),
            b"{\"new\":1}"
        );
        assert!(!old.join("thumbs").exists());
        assert!(
            old.join("settings.json").exists(),
            "not overwritten, not lost"
        );
        assert!(old.join(POINTER_FILE).exists());
    }

    #[test]
    fn settings_come_along_to_a_new_library_but_not_its_open_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            a.join("settings.json"),
            br#"{"discogs_token":"t","open_folder":"D:/A"}"#,
        )
        .unwrap();
        carry_settings(&a, &b).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(b.join("settings.json")).unwrap()).unwrap();
        assert_eq!(v["discogs_token"], "t");
        assert!(v.get("open_folder").is_none());
        // A library with its own settings keeps them.
        std::fs::write(b.join("settings.json"), b"{}").unwrap();
        carry_settings(&a, &b).unwrap();
        assert_eq!(std::fs::read(b.join("settings.json")).unwrap(), b"{}");
    }
}
