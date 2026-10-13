//! One library per root, kept in memory. Building a library reads the whole index (every file,
//! tag and picture), and opening a playlist, saving one or moving a song all need it, often
//! several at once. So it is built once and handed out until the index's change counter moves
//! (whoever wrote: a scan, the deep check, a lookup).
//!
//! The folder is watched, so a command scans only when something in it changed (a scan that
//! finds nothing still walks every folder). Without a watcher, after one of its errors, or
//! every few minutes regardless, it scans anyway.
//!
//! Scans of one root also run one at a time: many at once each found the same new files, read
//! them all, and queued to write them, until one gave up with "database is locked". A command
//! that arrives while a scan is running waits for it, and if a scan started after it asked, that
//! scan's result is fresh enough and it does not scan again.

use crate::{CmdResult, open_index};
use engine::library::Library;
use engine::{Index, ScanProgress, ScanReport, paths};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Libraries kept at once (the library and a card or two); the least recently used goes.
const KEEP: usize = 4;
/// Even with nothing seen changing, look again after this long.
const AT_MOST: Duration = Duration::from_secs(300);

struct Slot {
    /// When the latest scan of this root started; held for the whole of a scan.
    scan: Mutex<Option<Instant>>,
    /// The library, the change counter it was built at, and when it was last handed out.
    lib: Mutex<Option<(i64, Arc<Library>, Instant)>>,
    /// Something the scanner reads changed since the last scan began (or it can't be known).
    changed: Arc<AtomicBool>,
    watcher: Mutex<Option<RecommendedWatcher>>,
}

impl Slot {
    fn new(root: &Path) -> Self {
        let changed = Arc::new(AtomicBool::new(true));
        Self {
            scan: Mutex::new(None),
            lib: Mutex::new(None),
            watcher: Mutex::new(watch(root, changed.clone())),
            changed,
        }
    }
}

/// Watch `root`, setting `changed` for anything the scanner would see. None if it can't be
/// watched (then every command scans, as before).
fn watch(root: &Path, changed: Arc<AtomicBool>) -> Option<RecommendedWatcher> {
    let base = root.to_path_buf();
    let mut w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| match ev {
        Ok(ev) => {
            if !matches!(ev.kind, EventKind::Access(_))
                && ev.paths.iter().any(|p| scanned(&base, p))
            {
                changed.store(true, Ordering::Relaxed);
            }
        }
        // Events were lost (too many at once, a drive gone): assume anything changed.
        Err(_) => changed.store(true, Ordering::Relaxed),
    })
    .ok()?;
    w.watch(root, RecursiveMode::Recursive).ok()?;
    Some(w)
}

/// Whether the scanner reads `path`: not hidden (the index and thumbnails live in `.venuta`),
/// and not in one of the app's own top-level folders.
fn scanned(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        // Not obviously inside (another spelling of the path): count it.
        return true;
    };
    let mut parts = rel.components().filter_map(|c| match c {
        Component::Normal(s) => Some(s.to_string_lossy()),
        _ => None,
    });
    let Some(first) = parts.next() else {
        return true;
    };
    if first.starts_with('.') || engine::scan::RESERVED_DIRS.contains(&first.as_ref()) {
        return false;
    }
    parts.all(|p| !p.starts_with('.'))
}

static SLOTS: LazyLock<Mutex<HashMap<String, Arc<Slot>>>> = LazyLock::new(Default::default);

fn hold<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn slot(root: &Path) -> Arc<Slot> {
    let key = paths::canonical_root_string(root);
    let mut all = hold(&SLOTS);
    if let Some(s) = all.get(&key) {
        return s.clone();
    }
    if all.len() >= KEEP {
        let oldest = all
            .iter()
            .filter_map(|(k, s)| {
                let used = s.lib.try_lock().ok()?.as_ref().map(|l| l.2);
                Some((k.clone(), used))
            })
            .min_by_key(|(_, used)| *used)
            .map(|(k, _)| k);
        if let Some(k) = oldest {
            all.remove(&k);
        }
    }
    let s = Arc::new(Slot::new(&PathBuf::from(root)));
    all.insert(key, s.clone());
    s
}

/// The library as built from the index, from memory when nothing was written since.
fn built(slot: &Slot, index: &Index) -> CmdResult<Arc<Library>> {
    let mut held = hold(&slot.lib);
    let now = index.generation().map_err(|e| e.to_string())?;
    if let Some((at, lib, used)) = held.as_mut()
        && *at == now
    {
        *used = Instant::now();
        return Ok(lib.clone());
    }
    let lib = Arc::new(Library::load(index).map_err(|e| e.to_string())?);
    *held = Some((now, lib.clone(), Instant::now()));
    Ok(lib)
}

/// Start a scan with the scan lock held: what changes from here on is for the next one.
fn begin(slot: &Slot, started: &mut Option<Instant>) {
    *started = Some(Instant::now());
    slot.changed.store(false, Ordering::Relaxed);
}

/// Scan `root` (one scan of it at a time), reporting progress.
pub fn scan_with(
    index: &mut Index,
    root: &Path,
    progress: &(dyn Fn(ScanProgress) + Sync),
) -> CmdResult<ScanReport> {
    let slot = slot(root);
    let mut started = hold(&slot.scan);
    begin(&slot, &mut started);
    engine::scan_with(index, root, progress).map_err(|e| {
        slot.changed.store(true, Ordering::Relaxed);
        e.to_string()
    })
}

/// Scan `root` (one scan of it at a time).
pub fn scan(index: &mut Index, root: &Path) -> CmdResult<ScanReport> {
    scan_with(index, root, &|_| {})
}

/// The library as the folder is now. Scanned first when something in it changed (or it isn't
/// watched, or it's been a while), unless a scan began after this was asked.
pub fn fresh(root: &Path) -> CmdResult<(Index, Arc<Library>)> {
    let asked = Instant::now();
    let slot = slot(root);
    let mut index = open_index(root)?;
    {
        let mut started = hold(&slot.scan);
        let watched = hold(&slot.watcher).is_some();
        let needed = !watched
            || slot.changed.load(Ordering::Relaxed)
            || started.is_none_or(|s| s.elapsed() > AT_MOST);
        if needed && !started.is_some_and(|s| s >= asked) {
            begin(&slot, &mut started);
            if let Err(e) = engine::scan(&mut index, root) {
                slot.changed.store(true, Ordering::Relaxed);
                return Err(e.to_string());
            }
        }
    }
    let lib = built(&slot, &index)?;
    Ok((index, lib))
}

/// The library as indexed, without scanning (while the first scan of a big library runs, from
/// what is indexed so far).
pub fn indexed(index: &Index, root: &Path) -> CmdResult<Arc<Library>> {
    built(&slot(root), index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_and_app_folders_are_not_changes() {
        let root = Path::new("D:/Music");
        assert!(scanned(root, Path::new("D:/Music/Music/A/01.flac")));
        assert!(scanned(root, Path::new("D:/Music/Playlists/Road.m3u8")));
        assert!(!scanned(
            root,
            Path::new("D:/Music/.venuta/libraries/x/index.sqlite-wal")
        ));
        assert!(!scanned(root, Path::new("D:/Music/_Incoming/new.flac")));
        assert!(!scanned(root, Path::new("D:/Music/Music/A/.hidden.flac")));
        assert!(scanned(root, Path::new("E:/elsewhere/01.flac")));
    }

    #[test]
    fn a_change_in_the_folder_is_noticed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Music")).unwrap();
        std::fs::create_dir_all(dir.path().join(".venuta")).unwrap();
        let changed = Arc::new(AtomicBool::new(false));
        let _w = watch(dir.path(), changed.clone()).expect("watchable");
        let wait = |want: bool| {
            for _ in 0..40 {
                if changed.load(Ordering::Relaxed) == want {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            changed.load(Ordering::Relaxed) == want
        };

        // The app's own writes into .venuta don't count.
        std::fs::write(dir.path().join(".venuta/index.sqlite"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!changed.load(Ordering::Relaxed));

        std::fs::write(dir.path().join("Music/01.flac"), b"x").unwrap();
        assert!(wait(true));
    }
}
