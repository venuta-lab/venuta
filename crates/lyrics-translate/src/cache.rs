//! Translations kept per song: a song is its sung lines (other lyrics for the same song are
//! another song here), so a translation is never put on lines it wasn't made for. One small
//! JSON file per song, named by the SHA-256 of its lines, with an entry per language and engine.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Error;

/// A song's translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The song's language and the language it was translated into (BCP 47).
    pub from: String,
    pub into: String,
    /// The engine ("bergamot") and what answered (its model and version).
    pub engine: String,
    pub version: String,
    /// When it was made (ms since 1970).
    pub at_ms: u64,
    /// One per sung line: None for a line left as it is.
    pub lines: Vec<Option<String>>,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    entries: Vec<Entry>,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The translations kept in a folder.
pub struct SongCache {
    dir: PathBuf,
}

impl SongCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        SongCache { dir: dir.into() }
    }

    fn path(&self, lines: &[&str]) -> PathBuf {
        let mut h = Sha256::new();
        for l in lines {
            h.update(l.trim().as_bytes());
            h.update(b"\n");
        }
        let hex: String = h.finalize()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.dir.join(format!("{hex}.json"))
    }

    fn read(&self, lines: &[&str]) -> File {
        std::fs::read(self.path(lines))
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .unwrap_or_default()
    }

    /// The translation made by `engine` at `version`, if kept.
    pub fn get(
        &self,
        lines: &[&str],
        from: &str,
        into: &str,
        engine: &str,
        version: &str,
    ) -> Option<Entry> {
        self.read(lines).entries.into_iter().find(|e| {
            e.from == from
                && e.into == into
                && e.engine == engine
                && e.version == version
                && e.lines.len() == lines.len()
        })
    }

    /// The song's most recent translation, whatever its language and engine.
    pub fn latest(&self, lines: &[&str]) -> Option<Entry> {
        self.read(lines)
            .entries
            .into_iter()
            .filter(|e| e.lines.len() == lines.len())
            .max_by_key(|e| e.at_ms)
    }

    /// Keeps a translation, in place of the one by the same engine into the same language.
    pub fn put(&self, lines: &[&str], entry: &Entry) -> Result<(), Error> {
        let mut file = self.read(lines);
        file.entries
            .retain(|e| !(e.into == entry.into && e.engine == entry.engine));
        file.entries.push(entry.clone());
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path(lines);
        // Written aside and moved into place, so a reader never sees half a file.
        let part = path.with_extension(format!("{}.part", std::process::id()));
        std::fs::write(
            &part,
            serde_json::to_vec(&file).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&part, &path)?;
        Ok(())
    }

    /// Forgets every kept translation. Returns how many songs had one.
    pub fn clear(&self) -> usize {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        rd.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter(|e| std::fs::remove_file(e.path()).is_ok())
            .count()
    }
}
