//! For tests: where their files go. `temp_dir()` is this test process's own folder in the
//! system's temp (`venuta-tests/<pid>`), in place of `std::env::temp_dir()`, so what the suites
//! write is kept in one place: the first use in a process clears the folders earlier runs left
//! there (any over an hour old; a run beside it is younger), so the temp folder never fills up.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// Folders of earlier runs older than this are cleared.
const STALE: Duration = Duration::from_secs(3600);

/// This test process's folder in the system's temp, made and cleared of earlier runs on first use.
pub fn temp_dir() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let base = std::env::temp_dir().join("venuta-tests");
        clear_stale(&base, SystemTime::now());
        let root = base.join(std::process::id().to_string());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        root
    })
    .clone()
}

fn clear_stale(base: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > STALE));
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earlier_runs_folders_are_cleared_and_a_recent_one_is_kept() {
        let base = tempfile::tempdir().unwrap();
        let old = base.path().join("1111");
        let recent = base.path().join("2222");
        std::fs::create_dir_all(old.join("a")).unwrap();
        std::fs::create_dir_all(&recent).unwrap();
        // As if the old one were two hours old: judged against a clock two hours ahead, with the
        // recent one made after that moment's hour began.
        let later = SystemTime::now() + Duration::from_secs(2 * 3600);
        std::fs::write(recent.join("x"), b"x").unwrap();
        clear_stale(base.path(), later);
        assert!(!old.exists(), "an earlier run's folder is cleared");
        assert!(!recent.exists(), "two hours on, so is this one");
        std::fs::create_dir_all(&recent).unwrap();
        clear_stale(base.path(), SystemTime::now());
        assert!(recent.exists(), "a run beside it is kept");
    }

    #[test]
    fn the_folder_is_this_processes_own_and_stays_the_same() {
        let a = temp_dir();
        assert!(a.ends_with(std::process::id().to_string()));
        assert!(a.exists());
        assert_eq!(a, temp_dir());
    }
}
