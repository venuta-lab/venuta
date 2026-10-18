//! What Venuta keeps only to be quicker, and clearing it (Settings, Online sources). All of it
//! comes back on its own when needed: answers looked up online (tags, covers, the catalog),
//! lyrics found online, waveforms and spectrograms. Never touched here: settings, likes,
//! playlists, history, lyric choices, the library's own index.

use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// Folders of drawn pictures of songs, made again from the audio when a song is shown.
const PICTURES: [&str; 4] = ["waves-rms", "waves", "waves-online", "spectrograms"];

/// How much each kind takes, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CacheSizes {
    /// Answers looked up online: tags, covers, catalog pages.
    pub answers: u64,
    /// Lyrics found online.
    pub lyrics: u64,
    /// Waveforms and spectrograms.
    pub pictures: u64,
}

impl CacheSizes {
    pub fn total(&self) -> u64 {
        self.answers + self.lyrics + self.pictures
    }
}

fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => folder_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn open(dir: &Path) -> Option<Connection> {
    let file = dir.join("cache.sqlite");
    if !file.exists() {
        return None;
    }
    let conn = Connection::open(file).ok()?;
    conn.busy_timeout(Duration::from_secs(10)).ok()?;
    Some(conn)
}

/// Bytes a table's rows hold (0 when it isn't there).
fn table_bytes(conn: &Connection, sql: &str) -> u64 {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
        .ok()
        .flatten()
        .unwrap_or(0)
        .max(0) as u64
}

/// What the caches in this data folder take.
pub fn sizes(dir: &Path) -> CacheSizes {
    let (answers, lyrics) = match open(dir) {
        Some(c) => (
            table_bytes(
                &c,
                "SELECT SUM(LENGTH(url) + IFNULL(LENGTH(body), 0)) FROM http",
            ),
            table_bytes(
                &c,
                "SELECT SUM(LENGTH(song) + LENGTH(source) + IFNULL(LENGTH(found), 0)) FROM lyrics",
            ),
        ),
        None => (0, 0),
    };
    CacheSizes {
        answers,
        lyrics,
        pictures: PICTURES.iter().map(|p| folder_size(&dir.join(p))).sum(),
    }
}

/// Clear them all; returns what they took.
pub fn clear(dir: &Path) -> Result<CacheSizes, String> {
    let was = sizes(dir);
    if let Some(c) = open(dir) {
        for table in ["http", "lyrics"] {
            // A table not made yet has nothing to clear.
            let there: bool = c
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if there {
                c.execute(&format!("DELETE FROM {table}"), [])
                    .map_err(|e| e.to_string())?;
            }
        }
        // Give the space back; another part of Venuta reading at the same moment only delays it.
        let _ = c.execute_batch("VACUUM");
    }
    for p in PICTURES {
        let d = dir.join(p);
        if d.exists() {
            std::fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
    }
    Ok(was)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_takes_the_caches_and_leaves_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let c = Connection::open(d.join("cache.sqlite")).unwrap();
        c.execute_batch(
            "CREATE TABLE http (url TEXT PRIMARY KEY, status INTEGER NOT NULL, body BLOB, fetched_at INTEGER NOT NULL);
             CREATE TABLE lyrics (song TEXT NOT NULL, source TEXT NOT NULL, found TEXT, at INTEGER NOT NULL, PRIMARY KEY (song, source));
             CREATE TABLE other (x TEXT);
             INSERT INTO http VALUES ('https://a', 200, x'00112233', 1);
             INSERT INTO lyrics VALUES ('song', 'lrclib', '[00:01]hi', 1);
             INSERT INTO other VALUES ('kept');",
        )
        .unwrap();
        drop(c);
        std::fs::create_dir_all(d.join("waves-rms")).unwrap();
        std::fs::write(d.join("waves-rms/a.bin"), [0u8; 600]).unwrap();
        std::fs::write(d.join("settings.json"), b"{}").unwrap();
        std::fs::write(d.join("lyrics-choices.json"), b"{}").unwrap();

        let before = sizes(d);
        assert_eq!(before.answers, 9 + 4);
        assert_eq!(before.lyrics, 4 + 6 + 9);
        assert_eq!(before.pictures, 600);
        assert_eq!(clear(d).unwrap(), before, "says what it freed");

        assert_eq!(sizes(d).total(), 0);
        assert!(!d.join("waves-rms").exists());
        assert!(d.join("settings.json").exists() && d.join("lyrics-choices.json").exists());
        let c = Connection::open(d.join("cache.sqlite")).unwrap();
        let kept: String = c
            .query_row("SELECT x FROM other", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, "kept", "only the cache tables are emptied");
    }

    #[test]
    fn nothing_kept_yet_is_nothing_to_clear() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(sizes(dir.path()), CacheSizes::default());
        assert_eq!(clear(dir.path()).unwrap(), CacheSizes::default());
        // A cache file from a version before lyrics were kept.
        Connection::open(dir.path().join("cache.sqlite"))
            .unwrap()
            .execute_batch("CREATE TABLE http (url TEXT PRIMARY KEY, status INTEGER NOT NULL, body BLOB, fetched_at INTEGER NOT NULL);")
            .unwrap();
        assert!(clear(dir.path()).is_ok());
    }
}
