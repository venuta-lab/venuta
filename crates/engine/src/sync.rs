//! SD card sync: a one-way mirror of the library onto a card, safe to interrupt.
//!
//! * The card says who it is: `/.music-librarian/card.toml` (never the drive letter).
//! * The card remembers what it holds: `/.music-librarian/manifest.sqlite`, so any PC can
//!   sync it and changes are found by content, not by FAT/exFAT's unreliable file times.
//! * Order: clean up interrupted writes → delete → move → copy new (`.part`, flush, rename) →
//!   patch tags in place → sidecars. The manifest is updated after every file.
//! * Tag-only changes rewrite just the metadata at the start of the file, inside the space
//!   already there; the audio is never touched or re-copied.
//! * Music already on the card from another tool is recognised by its audio checksum and
//!   moved into place instead of being copied again.
//! * Files this app did not put on the card are never touched.

use crate::media::flac_layout::{self, BLOCK_PADDING};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

pub const CARD_DIR: &str = ".music-librarian";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardConfig {
    pub id: String,
    pub name: String,
    /// Folder on the card that holds the music.
    #[serde(default = "default_folder")]
    pub folder: String,
    /// Top-level library folders to include (empty = everything).
    #[serde(default)]
    pub include: Vec<String>,
    /// Sync automatically when the card is inserted (while the app is running).
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub created_at: u64,
    /// The player that reads this card; its copies are written the way that player reads.
    #[serde(default)]
    pub player: crate::device::Player,
    /// Your answers to the player test (question id → what the player did).
    #[serde(default)]
    pub test_results: std::collections::BTreeMap<String, bool>,
}

fn default_folder() -> String {
    "Music".into()
}

pub fn card_config_path(card_root: &Path) -> PathBuf {
    card_root.join(CARD_DIR).join("card.toml")
}

pub fn read_card(card_root: &Path) -> Option<CardConfig> {
    let text = std::fs::read_to_string(card_config_path(card_root)).ok()?;
    toml::from_str(&text).ok()
}

/// Make a card known to the app. Keeps an existing id so re-running setup never forgets it.
pub fn setup_card(
    card_root: &Path,
    name: &str,
    include: Vec<String>,
    auto_sync: bool,
    player: Option<crate::device::Player>,
) -> Result<CardConfig, String> {
    let existing = read_card(card_root);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let id = existing.as_ref().map(|c| c.id.clone()).unwrap_or_else(|| {
        let seed = format!(
            "{}:{now}:{:?}",
            card_root.display(),
            std::time::Instant::now()
        );
        blake3::hash(seed.as_bytes()).to_hex()[..20].to_owned()
    });
    let cfg = CardConfig {
        id,
        name: name.trim().to_owned(),
        folder: existing
            .as_ref()
            .map(|c| c.folder.clone())
            .unwrap_or_else(default_folder),
        include,
        auto_sync,
        player: player
            .or(existing.as_ref().map(|c| c.player))
            .unwrap_or_default(),
        test_results: existing
            .as_ref()
            .map(|c| c.test_results.clone())
            .unwrap_or_default(),
        created_at: existing.map(|c| c.created_at).unwrap_or(now),
    };
    let dir = card_root.join(CARD_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    write_atomic(&card_config_path(card_root), body.as_bytes())?;
    Ok(cfg)
}

// --- What the library wants on the card ------------------------------------------------------

#[derive(Debug, Clone)]
struct Wanted {
    rel: String,
    size: u64,
    mtime_ns: i64,
    audio_md5: Option<String>,
    is_flac: bool,
    is_image: bool,
}

fn wanted_files(library_root: &Path, cfg: &CardConfig) -> crate::Result<Vec<Wanted>> {
    let index = crate::Index::open(
        &crate::paths::index_path_for(library_root),
        &crate::paths::canonical_root_string(library_root),
    )?;
    let mut stmt = index.conn().prepare(
        "SELECT path, kind, size, mtime_ns, hex(audio_md5) FROM files
         WHERE kind IN ('flac', 'other_audio', 'lyrics', 'image', 'playlist')",
    )?;
    let rows = stmt.query_map([], |r| {
        let kind: String = r.get(1)?;
        Ok(Wanted {
            rel: r.get::<_, String>(0)?,
            is_flac: kind == "flac",
            is_image: kind == "image",
            size: r.get::<_, i64>(2)? as u64,
            mtime_ns: r.get(3)?,
            audio_md5: r.get::<_, Option<String>>(4)?.filter(|m| !m.is_empty()),
        })
    })?;
    let include: HashSet<String> = cfg.include.iter().map(|s| s.to_lowercase()).collect();
    let mut out = Vec::new();
    for w in rows {
        let w = w?;
        let top = w.rel.split('/').next().unwrap_or("").to_lowercase();
        if top.starts_with('_') {
            continue;
        }
        // Playlists always travel (unless the player reads none); everything else follows the
        // card's selection.
        if top == "playlists" && !cfg.player.recipe().playlists {
            continue;
        }
        if !include.is_empty() && !include.contains(&top) && top != "playlists" {
            continue;
        }
        out.push(w);
    }
    Ok(out)
}

// --- The manifest on the card -----------------------------------------------------------------

#[derive(Debug, Clone)]
struct Held {
    source_size: u64,
    source_mtime_ns: i64,
    audio_md5: Option<String>,
    /// The player recipe the copy was written with (none: an exact copy).
    recipe: Option<String>,
}

fn open_manifest(card_root: &Path) -> Result<Connection, String> {
    let dir = card_root.join(CARD_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let conn = Connection::open(dir.join("manifest.sqlite")).map_err(|e| e.to_string())?;
    // Rollback journal + full sync: the safest settings on a removable FAT/exFAT volume.
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         CREATE TABLE IF NOT EXISTS held (
            path            TEXT PRIMARY KEY,
            source_size     INTEGER NOT NULL,
            source_mtime_ns INTEGER NOT NULL,
            audio_md5       TEXT,
            synced_at       INTEGER NOT NULL
         );",
    )
    .map_err(|e| e.to_string())?;
    // Cards set up before player profiles: add the column, existing copies count as exact.
    let has_recipe = conn.prepare("SELECT recipe FROM held LIMIT 0").is_ok();
    if !has_recipe {
        conn.execute_batch("ALTER TABLE held ADD COLUMN recipe TEXT;")
            .map_err(|e| e.to_string())?;
    }
    Ok(conn)
}

fn load_manifest(conn: &Connection) -> Result<HashMap<String, Held>, String> {
    let mut stmt = conn
        .prepare("SELECT path, source_size, source_mtime_ns, audio_md5, recipe FROM held")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Held {
                    source_size: r.get::<_, i64>(1)? as u64,
                    source_mtime_ns: r.get(2)?,
                    audio_md5: r.get(3)?,
                    recipe: r.get(4)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())
}

fn record(conn: &Connection, w: &Wanted, recipe: Option<&str>) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO held (path, source_size, source_mtime_ns, audio_md5, synced_at, recipe)
         VALUES (?1, ?2, ?3, ?4, strftime('%s','now'), ?5)",
        params![w.rel, w.size as i64, w.mtime_ns, w.audio_md5, recipe],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn forget(conn: &Connection, rel: &str) -> Result<(), String> {
    conn.execute("DELETE FROM held WHERE path = ?1", params![rel])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// --- Planning -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SyncPlan {
    pub card: CardConfig,
    pub copy: usize,
    pub patch: usize,
    pub moves: usize,
    pub adopt: usize,
    pub delete: usize,
    pub unchanged: usize,
    pub bytes_to_copy: u64,
    pub bytes_freed: u64,
    pub free_bytes: u64,
    pub fits: bool,
    #[serde(skip)]
    ops: Vec<Op>,
}

#[derive(Debug, Clone)]
enum Op {
    Delete(String),
    /// Card path → new card path (same audio); then patch if tags differ.
    Move {
        from: String,
        to: Wanted,
        from_foreign: bool,
    },
    Copy(Wanted),
    Patch(Wanted),
}

pub fn plan(library_root: &Path, card_root: &Path) -> Result<SyncPlan, String> {
    let cfg = read_card(card_root).ok_or("This card isn't set up yet.")?;
    let wanted = wanted_files(library_root, &cfg).map_err(|e| e.to_string())?;
    let conn = open_manifest(card_root)?;
    let held = load_manifest(&conn)?;
    let music = card_root.join(&cfg.folder);

    let wanted_set: HashSet<&str> = wanted.iter().map(|w| w.rel.as_str()).collect();
    // Held files no longer wanted: candidates for moving (same audio elsewhere) or deleting.
    let mut leaving: HashMap<String, String> = held
        .iter()
        .filter(|(p, _)| !wanted_set.contains(p.as_str()))
        .filter_map(|(p, h)| Some((h.audio_md5.clone()?, p.clone())))
        .collect();
    // Music on the card that the manifest doesn't know: adopt it if the audio matches.
    let mut foreign = foreign_flacs(card_root, &cfg, &held);

    let mut ops = Vec::new();
    let mut p = SyncPlan {
        card: cfg.clone(),
        copy: 0,
        patch: 0,
        moves: 0,
        adopt: 0,
        delete: 0,
        unchanged: 0,
        bytes_to_copy: 0,
        bytes_freed: 0,
        free_bytes: crate::volumes::space(card_root)
            .map(|s| s.1)
            .unwrap_or(u64::MAX),
        fits: true,
        ops: Vec::new(),
    };
    let stamp = stamp_for(&cfg);
    for w in &wanted {
        let applies = w.is_flac || w.is_image;
        let same_recipe = |h: &Held| !applies || h.recipe.as_deref() == stamp.as_deref();
        match held.get(&w.rel) {
            Some(h)
                if h.source_size == w.size
                    && h.source_mtime_ns == w.mtime_ns
                    && same_recipe(h)
                    && music.join(&w.rel).exists() =>
            {
                p.unchanged += 1
            }
            Some(h) if w.is_image && !same_recipe(h) && music.join(&w.rel).exists() => {
                p.copy += 1;
                p.bytes_to_copy += w.size;
                ops.push(Op::Copy(w.clone()));
            }
            Some(h)
                if w.is_flac
                    && h.audio_md5.is_some()
                    && h.audio_md5 == w.audio_md5
                    && music.join(&w.rel).exists() =>
            {
                p.patch += 1;
                ops.push(Op::Patch(w.clone()));
            }
            _ => {
                if let Some(md5) = &w.audio_md5
                    && let Some(from) = leaving.remove(md5)
                {
                    p.moves += 1;
                    ops.push(Op::Move {
                        from,
                        to: w.clone(),
                        from_foreign: false,
                    });
                } else if let Some(md5) = &w.audio_md5
                    && let Some(from) = foreign.remove(md5)
                {
                    p.adopt += 1;
                    ops.push(Op::Move {
                        from,
                        to: w.clone(),
                        from_foreign: true,
                    });
                } else {
                    p.copy += 1;
                    p.bytes_to_copy += w.size;
                    ops.push(Op::Copy(w.clone()));
                }
            }
        }
    }
    for (path, _) in held
        .iter()
        .filter(|(p, _)| !wanted_set.contains(p.as_str()))
    {
        if leaving.values().any(|v| v == path) {
            p.delete += 1;
            p.bytes_freed += std::fs::metadata(music.join(path))
                .map(|m| m.len())
                .unwrap_or(0);
            ops.push(Op::Delete(path.clone()));
        } else if held.get(path).is_some_and(|h| h.audio_md5.is_none()) {
            p.delete += 1;
            ops.push(Op::Delete(path.clone()));
        }
    }
    p.fits = p.bytes_to_copy <= p.free_bytes.saturating_add(p.bytes_freed);
    // Deletes first (free space), then moves, copies, patches.
    ops.sort_by_key(|o| match o {
        Op::Delete(_) => 0,
        Op::Move { .. } => 1,
        Op::Copy(_) => 2,
        Op::Patch(_) => 3,
    });
    p.ops = ops;
    Ok(p)
}

/// FLAC files under the card's music folder that the manifest doesn't know, by audio MD5.
fn foreign_flacs(
    card_root: &Path,
    cfg: &CardConfig,
    held: &HashMap<String, Held>,
) -> HashMap<String, String> {
    let music = card_root.join(&cfg.folder);
    let mut out = HashMap::new();
    for e in walkdir::WalkDir::new(&music).into_iter().flatten() {
        if !e.file_type().is_file()
            || !e
                .path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("flac"))
        {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(&music) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        // The player test is the app's own and never adopted into the library's layout.
        if held.contains_key(&rel) || rel.starts_with(crate::device::TEST_FOLDER) {
            continue;
        }
        if let Ok(layout) = flac_layout::read_layout(e.path()) {
            let md5: String = layout.streaminfo[18..34]
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
            if md5 != "00000000000000000000000000000000" {
                out.insert(md5, rel);
            }
        }
    }
    out
}

// --- Running ------------------------------------------------------------------------------

#[derive(Default)]
pub struct SyncProgress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
}

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub copied: usize,
    pub patched: usize,
    pub moved: usize,
    pub adopted: usize,
    pub deleted: usize,
    pub bytes_written: u64,
    pub errors: Vec<(String, String)>,
    pub cancelled: bool,
    pub elapsed_ms: u128,
}

pub fn run(
    library_root: &Path,
    card_root: &Path,
    plan: &SyncPlan,
    progress: &SyncProgress,
    on_progress: &(dyn Fn(usize, usize, u64) + Sync),
) -> Result<SyncReport, String> {
    if !plan.fits {
        return Err(format!(
            "Not enough space on the card: {} MB to copy, {} MB free. Choose fewer folders for this card.",
            plan.bytes_to_copy / 1_000_000,
            (plan.free_bytes + plan.bytes_freed) / 1_000_000
        ));
    }
    let started = std::time::Instant::now();
    let music = card_root.join(&plan.card.folder);
    let conn = open_manifest(card_root)?;
    cleanup_parts(&music);
    let recipe = plan.card.player.recipe();
    let stamp = stamp_for(&plan.card);
    let stamp_of = |w: &Wanted| {
        if w.is_flac || w.is_image {
            stamp.as_deref()
        } else {
            None
        }
    };
    let total = plan.ops.len();
    progress.total.store(total, Ordering::Relaxed);
    let mut r = SyncReport::default();

    for (i, op) in plan.ops.iter().enumerate() {
        if progress.cancel.load(Ordering::Relaxed) {
            r.cancelled = true;
            break;
        }
        let result: Result<(), String> = match op {
            Op::Delete(rel) => {
                let path = music.join(rel);
                let res = match std::fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e.to_string()),
                };
                res.and_then(|()| forget(&conn, rel)).map(|()| {
                    remove_empty_parents(&path, &music);
                    r.deleted += 1;
                })
            }
            Op::Move {
                from,
                to,
                from_foreign,
            } => (|| {
                let src = music.join(from);
                let dst = music.join(&to.rel);
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
                if !from_foreign {
                    forget(&conn, from)?;
                }
                remove_empty_parents(&src, &music);
                // Same audio; bring the tags up to date if they differ.
                let written = patch_for(&library_root.join(&to.rel), &dst, &recipe)?;
                r.bytes_written += written;
                record(&conn, to, stamp_of(to))?;
                if *from_foreign {
                    r.adopted += 1
                } else {
                    r.moved += 1
                }
                Ok(())
            })(),
            Op::Copy(w) => copy_for(&library_root.join(&w.rel), &music.join(&w.rel), w, &recipe)
                .and_then(|n| {
                    r.bytes_written += n;
                    r.copied += 1;
                    record(&conn, w, stamp_of(w))
                }),
            Op::Patch(w) => patch_for(&library_root.join(&w.rel), &music.join(&w.rel), &recipe)
                .and_then(|n| {
                    r.bytes_written += n;
                    r.patched += 1;
                    record(&conn, w, stamp_of(w))
                }),
        };
        if let Err(e) = result {
            let what = match op {
                Op::Delete(p) => p.clone(),
                Op::Move { to, .. } => to.rel.clone(),
                Op::Copy(w) | Op::Patch(w) => w.rel.clone(),
            };
            r.errors.push((what, e));
        }
        progress.done.store(i + 1, Ordering::Relaxed);
        progress.bytes.store(r.bytes_written, Ordering::Relaxed);
        on_progress(i + 1, total, r.bytes_written);
    }
    r.elapsed_ms = started.elapsed().as_millis();
    Ok(r)
}

/// The recipe stamp a card's copies are written with; None for exact copies.
fn stamp_for(cfg: &CardConfig) -> Option<String> {
    cfg.player.recipe().transform.then(|| cfg.player.stamp())
}

/// A new copy on the card, written for its player.
fn copy_for(src: &Path, dst: &Path, w: &Wanted, r: &crate::device::Recipe) -> Result<u64, String> {
    if r.transform && w.is_flac {
        return write_card_flac(src, dst, r);
    }
    if w.is_image
        && let Ok(bytes) = std::fs::read(src)
        && let Some(small) = crate::device::card_image(&bytes, r)
    {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_atomic(dst, &small)?;
        return Ok(small.len() as u64);
    }
    copy_file(src, dst)
}

/// Bring a card copy's tags up to date for its player: in place when they fit before the
/// audio, otherwise the file is written again. The audio itself is never rewritten in place.
fn patch_for(src: &Path, dst: &Path, r: &crate::device::Recipe) -> Result<u64, String> {
    let is_flac = src
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("flac"));
    if !r.transform || !is_flac {
        return patch_or_copy(src, dst);
    }
    let (meta, _) = crate::device::card_metadata(src, r)?;
    let s = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
    if let Ok(d) = flac_layout::read_layout(dst)
        && d.id3v2_len == 0
        && d.streaminfo == s.streaminfo
        && d.audio_len() == s.audio_len()
    {
        // The card metadata ends with an 8 KiB padding block; resize it to fill the space.
        let core = &meta[..meta.len() - (4 + 8192)];
        let room = d.audio_offset as i64 - core.len() as i64 - 4;
        if (0..(1 << 24)).contains(&room) {
            let mut out = core.to_vec();
            let len = (room as u32).to_be_bytes();
            out.extend([0x80 | BLOCK_PADDING, len[1], len[2], len[3]]);
            out.resize(d.audio_offset as usize, 0);
            {
                let mut f = OpenOptions::new()
                    .write(true)
                    .open(dst)
                    .map_err(|e| e.to_string())?;
                f.write_all(&out).map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())?;
            }
            let after = flac_layout::read_layout(dst).map_err(|e| e.to_string())?;
            if after.audio_offset == d.audio_offset && after.streaminfo == d.streaminfo {
                return Ok(out.len() as u64);
            }
        }
    }
    write_card_flac(src, dst, r)
}

/// Write a card copy: the player's metadata, then the library file's audio bytes unchanged.
fn write_card_flac(src: &Path, dst: &Path, r: &crate::device::Recipe) -> Result<u64, String> {
    let (meta, audio_offset) = crate::device::card_metadata(src, r)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = part_path(dst);
    let result = (|| -> Result<u64, String> {
        let mut input = File::open(src).map_err(|e| format!("read: {e}"))?;
        input
            .seek(SeekFrom::Start(audio_offset))
            .map_err(|e| e.to_string())?;
        let mut output = File::create(&part).map_err(|e| format!("create: {e}"))?;
        output.write_all(&meta).map_err(|e| format!("write: {e}"))?;
        let mut buf = vec![0u8; 4 << 20];
        let mut n = meta.len() as u64;
        loop {
            let k = input.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if k == 0 {
                break;
            }
            output
                .write_all(&buf[..k])
                .map_err(|e| format!("write: {e}"))?;
            n += k as u64;
        }
        output.sync_all().map_err(|e| format!("flush: {e}"))?;
        drop(output);
        // Same audio as the library file, byte for byte.
        let s = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
        let d = flac_layout::read_layout(&part).map_err(|e| e.to_string())?;
        if s.streaminfo != d.streaminfo || s.audio_len() != d.audio_len() {
            return Err("the card copy did not check out".into());
        }
        std::fs::rename(&part, dst).map_err(|e| format!("rename: {e}"))?;
        Ok(n)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Copy as `.part`, flush to the card, then rename. Returns bytes written.
fn copy_file(src: &Path, dst: &Path) -> Result<u64, String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = part_path(dst);
    let result = (|| -> Result<u64, String> {
        let mut input = File::open(src).map_err(|e| format!("read: {e}"))?;
        let mut output = File::create(&part).map_err(|e| format!("create: {e}"))?;
        // One sequential stream with a large buffer: what SD cards are fastest at.
        let mut buf = vec![0u8; 4 << 20];
        let mut n = 0u64;
        loop {
            let k = input.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if k == 0 {
                break;
            }
            output
                .write_all(&buf[..k])
                .map_err(|e| format!("write: {e}"))?;
            n += k as u64;
        }
        output.sync_all().map_err(|e| format!("flush: {e}"))?;
        drop(output);
        let expected = std::fs::metadata(src).map(|m| m.len()).unwrap_or(n);
        if n != expected {
            return Err("size changed while copying".into());
        }
        std::fs::rename(&part, dst).map_err(|e| format!("rename: {e}"))?;
        Ok(n)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Bring a card file's tags in line with the library file of the same audio. If the new
/// metadata fits the space before the audio on the card, only that space is rewritten;
/// otherwise the whole file is copied. Returns bytes written.
fn patch_or_copy(src: &Path, dst: &Path) -> Result<u64, String> {
    let is_flac = src
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("flac"));
    if is_flac && let Ok(n) = patch_metadata(src, dst) {
        return Ok(n);
    }
    copy_file(src, dst)
}

fn patch_metadata(src: &Path, dst: &Path) -> Result<u64, String> {
    let s = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
    let d = flac_layout::read_layout(dst).map_err(|e| e.to_string())?;
    if s.id3v2_len != 0
        || d.id3v2_len != 0
        || s.streaminfo != d.streaminfo
        || s.audio_len() != d.audio_len()
    {
        return Err("not the same audio".into());
    }
    // Without a stored checksum the header can't prove the audio matches: compare the bytes.
    if s.streaminfo[18..34].iter().all(|b| *b == 0)
        && flac_layout::audio_hash(src, &s).map_err(|e| e.to_string())?
            != flac_layout::audio_hash(dst, &d).map_err(|e| e.to_string())?
    {
        return Err("not the same audio".into());
    }
    // New metadata = the source's blocks (without its padding) + padding to fill the card's space.
    let mut f = File::open(src).map_err(|e| e.to_string())?;
    let mut meta = b"fLaC".to_vec();
    for b in s.blocks.iter().filter(|b| b.kind != BLOCK_PADDING) {
        let mut raw = vec![0u8; 4 + b.len as usize];
        f.seek(SeekFrom::Start(b.offset))
            .map_err(|e| e.to_string())?;
        f.read_exact(&mut raw).map_err(|e| e.to_string())?;
        raw[0] &= 0x7F; // not the last block any more
        meta.extend(raw);
    }
    let room = d.audio_offset as i64 - meta.len() as i64 - 4;
    if !(0..(1 << 24)).contains(&room) {
        return Err("new tags don't fit".into());
    }
    let len = (room as u32).to_be_bytes();
    meta.extend([0x80 | BLOCK_PADDING, len[1], len[2], len[3]]);
    meta.resize(d.audio_offset as usize, 0);
    {
        let mut out = OpenOptions::new()
            .write(true)
            .open(dst)
            .map_err(|e| e.to_string())?;
        out.write_all(&meta).map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
    }
    // The audio region was not touched; confirm the file still reads as the same audio.
    let after = flac_layout::read_layout(dst).map_err(|e| e.to_string())?;
    if after.audio_offset != d.audio_offset || after.streaminfo != d.streaminfo {
        return Err("patch check failed".into());
    }
    Ok(meta.len() as u64)
}

fn cleanup_parts(music: &Path) {
    for e in walkdir::WalkDir::new(music).into_iter().flatten() {
        if e.file_type().is_file() && e.path().to_string_lossy().ends_with(".part") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn remove_empty_parents(path: &Path, stop: &Path) {
    let mut cur = path.parent();
    while let Some(dir) = cur {
        if dir == stop || !dir.starts_with(stop) || std::fs::remove_dir(dir).is_err() {
            break;
        }
        cur = dir.parent();
    }
}

fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let part = part_path(path);
    {
        let mut f = File::create(&part).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

/// Known cards and other removable drives, for the Cards screen.
#[derive(Debug, Clone, Serialize)]
pub struct CardStatus {
    pub volume: crate::volumes::Volume,
    pub card: Option<CardConfig>,
    pub last_sync: Option<i64>,
    pub files: usize,
}

pub fn cards() -> Vec<CardStatus> {
    crate::volumes::list()
        .into_iter()
        .filter_map(|v| {
            let root = PathBuf::from(&v.root);
            let card = read_card(&root);
            if card.is_none() && !v.removable {
                return None; // only removable drives are offered for setup
            }
            let (last_sync, files) = card
                .as_ref()
                .and_then(|_| {
                    let conn =
                        Connection::open(root.join(CARD_DIR).join("manifest.sqlite")).ok()?;
                    conn.query_row("SELECT MAX(synced_at), COUNT(*) FROM held", [], |r| {
                        Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)? as usize))
                    })
                    .optional()
                    .ok()
                    .flatten()
                })
                .unwrap_or((None, 0));
            Some(CardStatus {
                volume: v,
                card,
                last_sync,
                files,
            })
        })
        .collect()
}

/// Keep your answers to the player test on the card, next to its settings.
pub fn save_test_results(
    card_root: &Path,
    results: std::collections::BTreeMap<String, bool>,
) -> Result<CardConfig, String> {
    let mut cfg = read_card(card_root).ok_or("This card isn't set up yet.")?;
    cfg.test_results = results;
    let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    write_atomic(&card_config_path(card_root), body.as_bytes())?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(last: bool, kind: u8, body: &[u8]) -> Vec<u8> {
        let len = (body.len() as u32).to_be_bytes();
        let mut v = vec![(u8::from(last) << 7) | kind, len[1], len[2], len[3]];
        v.extend_from_slice(body);
        v
    }

    fn flac(comment: &[u8], padding: usize, audio: &[u8]) -> Vec<u8> {
        flac_with(&[9u8; 34], comment, padding, audio)
    }

    fn flac_with(streaminfo: &[u8; 34], comment: &[u8], padding: usize, audio: &[u8]) -> Vec<u8> {
        let mut v = b"fLaC".to_vec();
        v.extend(block(false, 0, streaminfo));
        v.extend(block(false, 4, comment));
        v.extend(block(true, BLOCK_PADDING, &vec![0u8; padding]));
        v.extend_from_slice(audio);
        v
    }

    #[test]
    fn patch_rewrites_only_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.flac");
        let dst = dir.path().join("dst.flac");
        // Library file: new, longer tags. Card file: old tags with generous padding.
        std::fs::write(
            &src,
            flac(b"NEW TAGS, LONGER THAN BEFORE", 10, b"AUDIO-FRAMES"),
        )
        .unwrap();
        std::fs::write(&dst, flac(b"OLD", 200, b"AUDIO-FRAMES")).unwrap();
        let before = flac_layout::read_layout(&dst).unwrap();
        patch_metadata(&src, &dst).unwrap();
        let after = flac_layout::read_layout(&dst).unwrap();
        assert_eq!(after.audio_offset, before.audio_offset);
        assert_eq!(
            flac_layout::audio_fingerprint(&dst).unwrap(),
            flac_layout::audio_fingerprint(&src).unwrap()
        );
        let bytes = std::fs::read(&dst).unwrap();
        assert!(
            bytes
                .windows(28)
                .any(|w| w == b"NEW TAGS, LONGER THAN BEFORE")
        );
    }

    #[test]
    fn patch_refuses_when_it_does_not_fit_or_audio_differs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.flac");
        let dst = dir.path().join("dst.flac");
        std::fs::write(&src, flac(&[b'x'; 500], 0, b"AUDIO")).unwrap();
        std::fs::write(&dst, flac(b"OLD", 10, b"AUDIO")).unwrap();
        assert!(patch_metadata(&src, &dst).is_err()); // too big: caller falls back to a copy
        // Different audio (different checksum in STREAMINFO).
        std::fs::write(&src, flac_with(&[7u8; 34], b"NEW", 0, b"OTHER")).unwrap();
        assert!(patch_metadata(&src, &dst).is_err());
        // No checksum stored (all zero): the audio bytes themselves are compared.
        let mut no_md5 = [9u8; 34];
        no_md5[18..34].fill(0);
        std::fs::write(&src, flac_with(&no_md5, b"NEW", 0, b"OTHER")).unwrap();
        std::fs::write(&dst, flac_with(&no_md5, b"OLD", 100, b"AUDIO")).unwrap();
        assert!(patch_metadata(&src, &dst).is_err());
        std::fs::write(&src, flac_with(&no_md5, b"NEW", 0, b"AUDIO")).unwrap();
        assert!(patch_metadata(&src, &dst).is_ok());
    }
}
