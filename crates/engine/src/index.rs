//! The library index: a local SQLite database describing every file under the library root.
//! It is a cache of the files, never the source of truth; deleting it only costs a rescan.

use crate::error::{Error, Result};
use crate::media::{FileKind, MediaFacts};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::Path;

const SCHEMA_VERSION: i32 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE files (
    id          INTEGER PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,   -- relative to the library root, '/' separators
    kind        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    mtime_ns    INTEGER NOT NULL,
    scanned_at  INTEGER NOT NULL,       -- unix seconds
    sample_rate INTEGER,
    bit_depth   INTEGER,
    channels    INTEGER,
    duration_ms INTEGER,
    audio_md5   BLOB,                   -- STREAMINFO MD5, NULL if unset
    vendor      TEXT,
    tags_hash   BLOB,
    read_error  TEXT
);
CREATE INDEX files_kind ON files(kind);
CREATE INDEX files_md5 ON files(audio_md5);

CREATE TABLE tags (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    idx     INTEGER NOT NULL,           -- position in the file, keeps multi-value order
    key     TEXT NOT NULL,              -- upper-cased for lookups
    raw_key TEXT NOT NULL,              -- exactly as stored in the file
    value   TEXT NOT NULL,
    PRIMARY KEY (file_id, idx)
);
CREATE INDEX tags_key_value ON tags(key, value);

CREATE TABLE pictures (
    file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    pic_type    INTEGER NOT NULL,
    mime        TEXT,
    bytes       INTEGER NOT NULL,
    format      TEXT,                   -- jpeg / png / other, from the image header
    width       INTEGER,
    height      INTEGER,
    progressive INTEGER,
    data_hash   BLOB NOT NULL,
    PRIMARY KEY (file_id, idx)
);
"#;

/// What the index remembers about a file to decide whether it changed.
#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub id: i64,
    pub size: u64,
    pub mtime_ns: i128,
}

/// A file as observed on disk, ready to be written to the index.
#[derive(Debug, Clone)]
pub struct ObservedFile {
    pub rel_path: String,
    pub kind: FileKind,
    pub size: u64,
    pub mtime_ns: i128,
    pub facts: Option<MediaFacts>,
    pub read_error: Option<String>,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open (or create) the index for `library_root`. An index is bound to one root.
    pub fn open(db_path: &Path, library_root: &str) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut index = Self { conn };
        index.migrate()?;
        index.bind_root(library_root)?;
        Ok(index)
    }

    /// In-memory index, for tests.
    pub fn open_in_memory(library_root: &str) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut index = Self { conn };
        index.migrate()?;
        index.bind_root(library_root)?;
        Ok(index)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(())
    }

    fn bind_root(&mut self, root: &str) -> Result<()> {
        let existing: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'root'", [], |r| {
                r.get(0)
            })
            .optional()?;
        match existing {
            Some(indexed) if indexed != root => Err(Error::RootMismatch {
                indexed,
                requested: root.to_owned(),
            }),
            Some(_) => Ok(()),
            None => {
                self.conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('root', ?1)",
                    params![root],
                )?;
                Ok(())
            }
        }
    }

    /// Every indexed file's change stamp, keyed by relative path.
    pub fn stamps(&self) -> Result<HashMap<String, Stamp>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, size, mtime_ns FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                Stamp {
                    id: r.get(0)?,
                    size: r.get::<_, i64>(2)? as u64,
                    mtime_ns: r.get::<_, i64>(3)? as i128,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Apply one scan's results atomically: upsert changed files, drop vanished ones.
    pub fn apply_scan(
        &mut self,
        observed: &[ObservedFile],
        vanished: &[i64],
        now: i64,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut del = tx.prepare("DELETE FROM files WHERE id = ?1")?;
            for id in vanished {
                del.execute(params![id])?;
            }

            let mut upsert = tx.prepare(
                "INSERT INTO files (path, kind, size, mtime_ns, scanned_at, sample_rate, bit_depth,
                                    channels, duration_ms, audio_md5, vendor, tags_hash, read_error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(path) DO UPDATE SET
                    kind = excluded.kind, size = excluded.size, mtime_ns = excluded.mtime_ns,
                    scanned_at = excluded.scanned_at, sample_rate = excluded.sample_rate,
                    bit_depth = excluded.bit_depth, channels = excluded.channels,
                    duration_ms = excluded.duration_ms, audio_md5 = excluded.audio_md5,
                    vendor = excluded.vendor, tags_hash = excluded.tags_hash,
                    read_error = excluded.read_error
                 RETURNING id",
            )?;
            let mut clear_tags = tx.prepare("DELETE FROM tags WHERE file_id = ?1")?;
            let mut clear_pics = tx.prepare("DELETE FROM pictures WHERE file_id = ?1")?;
            let mut ins_tag = tx.prepare(
                "INSERT INTO tags (file_id, idx, key, raw_key, value) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut ins_pic = tx.prepare(
                "INSERT INTO pictures (file_id, idx, pic_type, mime, bytes, format, width, height,
                                       progressive, data_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;

            for f in observed {
                let audio = f.facts.as_ref().and_then(|m| m.audio.as_ref());
                let tags_hash = f
                    .facts
                    .as_ref()
                    .filter(|_| f.kind.is_audio())
                    .map(|m| crate::media::tags_hash(&m.tags).to_vec());
                let id: i64 = upsert.query_row(
                    params![
                        f.rel_path,
                        f.kind.as_str(),
                        f.size as i64,
                        f.mtime_ns as i64,
                        now,
                        audio.map(|a| a.sample_rate),
                        audio.map(|a| a.bit_depth),
                        audio.map(|a| a.channels),
                        audio.map(|a| a.duration_ms as i64),
                        audio.and_then(|a| a.audio_md5).map(|m| m.to_vec()),
                        f.facts.as_ref().and_then(|m| m.vendor.clone()),
                        tags_hash,
                        f.read_error,
                    ],
                    |r| r.get(0),
                )?;
                clear_tags.execute(params![id])?;
                clear_pics.execute(params![id])?;
                let Some(facts) = &f.facts else { continue };
                for (i, (k, v)) in facts.tags.iter().enumerate() {
                    ins_tag.execute(params![id, i as i64, k.to_ascii_uppercase(), k, v])?;
                }
                for (i, p) in facts.pictures.iter().enumerate() {
                    let img = p.image.as_ref();
                    ins_pic.execute(params![
                        id,
                        i as i64,
                        p.pic_type,
                        p.mime,
                        p.bytes as i64,
                        img.map(|x| format!("{:?}", x.format).to_ascii_lowercase()),
                        img.map(|x| x.width),
                        img.map(|x| x.height),
                        img.map(|x| x.progressive),
                        p.data_hash.to_vec(),
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Read-only access for queries elsewhere in the engine (audit, reports).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
