//! Apply: the only code that writes music files.
//!
//! Migration mode builds a new library from the plan: every source file is *copied* to its
//! target, and the copy is changed. The source library is never written to.
//!
//! For every track: copy to `name.part` → fingerprint the audio → write tags, artwork and
//! lyrics → fingerprint again (must be identical) → flush → rename into place. A file that
//! already exists at the target is never overwritten unless it is our own finished result.
//! Every created file is recorded in a journal so the whole run can be undone.

use crate::media::flac_layout;
use crate::plan::{AlbumAction, AlbumPlan, ArtPlan, TrackPlan};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::ogg::OggPictureStorage;
use lofty::picture::{MimeType, Picture, PictureType};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default, Serialize)]
pub struct ApplyReport {
    pub albums: usize,
    pub tracks_written: usize,
    pub tracks_skipped: usize,
    pub quarantined_albums: usize,
    pub errors: Vec<ApplyError>,
    pub journal: String,
    pub cancelled: bool,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyError {
    pub album: String,
    pub file: String,
    pub message: String,
}

#[derive(Default)]
pub struct ApplyProgress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum JournalEntry {
    Begin {
        source: String,
        target: String,
        albums: usize,
    },
    Create {
        path: String,
    },
}

/// Append-only record of what a run created, flushed line by line so a crash keeps it.
struct Journal {
    path: PathBuf,
    out: Mutex<BufWriter<File>>,
}

impl Journal {
    fn create(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let path = dir.join(format!("{stamp}-apply.jsonl"));
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            out: Mutex::new(BufWriter::new(file)),
        })
    }

    fn record(&self, entry: &JournalEntry) {
        if let Ok(mut w) = self.out.lock() {
            let _ = serde_json::to_writer(&mut *w, entry);
            let _ = w.write_all(b"\n");
            let _ = w.flush();
        }
    }
}

pub fn journal_dir() -> PathBuf {
    crate::paths::app_data_dir().join("journal")
}

/// Build the new library at `target_root` from the given (approved) album plans.
pub fn apply(
    albums: &[AlbumPlan],
    source_root: &Path,
    target_root: &Path,
    progress: &ApplyProgress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ApplyReport, String> {
    let started = std::time::Instant::now();
    check_target(source_root, target_root)?;
    std::fs::create_dir_all(target_root).map_err(|e| e.to_string())?;
    let marker = target_root.join(LIBRARY_MARKER);
    if !marker.exists() {
        std::fs::write(
            &marker,
            "A library built by Music Librarian.
",
        )
        .map_err(|e| e.to_string())?;
    }
    let journal = Journal::create(&journal_dir()).map_err(|e| e.to_string())?;
    journal.record(&JournalEntry::Begin {
        source: source_root.to_string_lossy().into(),
        target: target_root.to_string_lossy().into(),
        albums: albums.len(),
    });

    let total: usize = albums.iter().map(|a| a.tracks.len()).sum();
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    on_progress(0, total);

    // Only needed when a plan asks for a sharper cover from a catalogue.
    let http = albums
        .iter()
        .any(|a| a.cover_url.is_some())
        .then(crate::net::Http::open)
        .transpose()?;
    let errors = Mutex::new(Vec::new());
    let written = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let quarantined = AtomicUsize::new(0);

    // Albums in parallel, a few at a time: copying is disk-bound and many writers hurt.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .map_err(|e| e.to_string())?;
    pool.install(|| {
        albums.par_iter().for_each(|album| {
            if progress.cancel.load(Ordering::Relaxed) {
                return;
            }
            let ctx = AlbumCtx {
                album,
                source_root,
                target_root,
                journal: &journal,
                http: http.as_ref(),
            };
            let result = match &album.action {
                AlbumAction::Migrate => {
                    ctx.migrate(progress, on_progress, total, &written, &skipped)
                }
                AlbumAction::Quarantine { .. } => {
                    quarantined.fetch_add(1, Ordering::Relaxed);
                    ctx.quarantine(progress, on_progress, total)
                }
            };
            if let Err(mut errs) = result
                && let Ok(mut e) = errors.lock()
            {
                e.append(&mut errs);
            }
        });
    });

    Ok(ApplyReport {
        albums: albums.len(),
        tracks_written: written.into_inner(),
        tracks_skipped: skipped.into_inner(),
        quarantined_albums: quarantined.into_inner(),
        errors: errors.into_inner().unwrap_or_default(),
        journal: journal.path.to_string_lossy().into(),
        cancelled: progress.cancel.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis(),
    })
}

struct AlbumCtx<'a> {
    album: &'a AlbumPlan,
    source_root: &'a Path,
    target_root: &'a Path,
    journal: &'a Journal,
    http: Option<&'a crate::net::Http>,
}

impl AlbumCtx<'_> {
    fn err(&self, file: &str, message: impl Into<String>) -> ApplyError {
        ApplyError {
            album: self.album.id.clone(),
            file: file.to_owned(),
            message: message.into(),
        }
    }

    fn migrate(
        &self,
        progress: &ApplyProgress,
        on_progress: &(dyn Fn(usize, usize) + Sync),
        total: usize,
        written: &AtomicUsize,
        skipped: &AtomicUsize,
    ) -> Result<(), Vec<ApplyError>> {
        let mut errors = Vec::new();
        // The album's player-safe cover, made once and embedded in every track. A sharper
        // catalogue cover is used when planned and it really is an image; otherwise the file's own.
        let downloaded = self.album.cover_url.as_ref().and_then(|url| {
            let bytes = self.http?.get_bytes(url).ok()?;
            image::load_from_memory(&bytes).is_ok().then_some(bytes)
        });
        let used_download = downloaded.is_some();
        let original_cover = downloaded.or_else(|| self.original_cover());
        let embedded = match (&self.album.art, &original_cover) {
            (ArtPlan::Reencode { max_px }, Some(bytes)) => match baseline_jpeg(bytes, *max_px) {
                Ok(b) => Some(b),
                Err(e) => {
                    errors.push(self.err("cover", format!("couldn't re-encode cover art: {e}")));
                    None
                }
            },
            _ => None,
        };

        for t in &self.album.tracks {
            if progress.cancel.load(Ordering::Relaxed) {
                break;
            }
            match self.migrate_track(t, embedded.as_deref()) {
                Ok(true) => {
                    written.fetch_add(1, Ordering::Relaxed);
                }
                Ok(false) => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => errors.push(self.err(&t.source, e)),
            }
            let done = progress.done.fetch_add(1, Ordering::Relaxed) + 1;
            on_progress(done, total);
        }

        // Full-resolution cover.jpg beside the tracks, for the PC and players that look for it.
        if let Some(first) = self.album.tracks.first() {
            let dir = self
                .target_root
                .join(crate::library::parent_dir(&first.target));
            let cover_target = dir.join("cover.jpg");
            let bytes = match (&self.album.cover_from, used_download) {
                (Some(rel), false) => std::fs::read(self.source_root.join(rel)).ok(),
                _ => original_cover.clone(),
            };
            if let Some(bytes) = bytes
                && !cover_target.exists()
            {
                match write_atomic(&cover_target, &bytes) {
                    Ok(()) => self.created(&cover_target),
                    Err(e) => errors.push(self.err("cover.jpg", e)),
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Returns Ok(true) when written, Ok(false) when already up to date.
    fn migrate_track(&self, t: &TrackPlan, cover: Option<&[u8]>) -> Result<bool, String> {
        let src = self.source_root.join(&t.source);
        let dst = self.target_root.join(&t.target);
        let is_flac = t.source.to_ascii_lowercase().ends_with(".flac");

        if dst.exists() {
            // Ours from an earlier run with the same result? Then nothing to do.
            if is_flac && self.already_done(&src, &dst, t)? {
                self.sidecars(t, &dst)?;
                return Ok(false);
            }
            if !is_flac && file_len(&src) == file_len(&dst) {
                return Ok(false);
            }
            return Err(format!(
                "a different file already exists at {}; left untouched",
                t.target
            ));
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create folder: {e}"))?;
        }
        let part = part_path(&dst);
        std::fs::copy(&src, &part).map_err(|e| format!("copy: {e}"))?;

        let result = (|| -> Result<(), String> {
            if !is_flac {
                return Ok(()); // other formats are copied as they are for now
            }
            let before =
                flac_layout::audio_fingerprint(&part).map_err(|e| format!("read copy: {e}"))?;
            self.write_tags(&part, t, cover)?;
            let after =
                flac_layout::audio_fingerprint(&part).map_err(|e| format!("re-read copy: {e}"))?;
            if before != after {
                return Err("audio changed during tag writing; the copy was discarded".into());
            }
            Ok(())
        })();
        // Flushing needs a writable handle on Windows (FlushFileBuffers).
        let result = result.and_then(|()| {
            OpenOptions::new()
                .write(true)
                .open(&part)
                .and_then(|f| f.sync_all())
                .map_err(|e| format!("flush: {e}"))
        });
        let result =
            result.and_then(|()| std::fs::rename(&part, &dst).map_err(|e| format!("rename: {e}")));
        if let Err(e) = result {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        self.created(&dst);
        self.sidecars(t, &dst)?;
        Ok(true)
    }

    fn write_tags(&self, path: &Path, t: &TrackPlan, cover: Option<&[u8]>) -> Result<(), String> {
        let mut flac = {
            let mut r = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
            FlacFile::read_from(&mut r, ParseOptions::new().implicit_conversions(false))
                .map_err(|e| format!("parse: {e}"))?
        };
        if flac.vorbis_comments().is_none() {
            flac.set_vorbis_comments(Default::default());
        }
        let vc = flac.vorbis_comments_mut().expect("just ensured");
        let _ = vc.take_items().count();
        for (k, v) in &t.tags_after {
            vc.push(k.clone(), v.clone());
        }
        if let Some(lrc) = &t.embed_lyrics_from {
            let text = std::fs::read_to_string(self.source_root.join(lrc))
                .map_err(|e| format!("read lyrics: {e}"))?;
            let _ = vc.remove("LYRICS").count();
            vc.push(
                "LYRICS".into(),
                text.trim_start_matches('\u{feff}').to_owned(),
            );
        }
        if let Some(bytes) = cover {
            let _ = flac.remove_pictures();
            let pic = Picture::unchecked(bytes.to_vec())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Jpeg)
                .build();
            flac.insert_picture(pic, None)
                .map_err(|e| format!("cover: {e}"))?;
        }
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        flac.save_to(&mut f, WriteOptions::default())
            .map_err(|e| format!("write tags: {e}"))
    }

    /// Our own finished copy: same audio as the source and exactly the planned tags.
    fn already_done(&self, src: &Path, dst: &Path, t: &TrackPlan) -> Result<bool, String> {
        let a = flac_layout::audio_fingerprint(src).map_err(|e| e.to_string())?;
        let b = match flac_layout::audio_fingerprint(dst) {
            Ok(b) => b,
            Err(_) => return Ok(false),
        };
        if a != b {
            return Ok(false);
        }
        let facts = crate::media::read_facts(dst, crate::media::FileKind::Flac)?;
        let mut want: Vec<(String, String)> = t.tags_after.clone();
        let mut have = facts.tags;
        if t.embed_lyrics_from.is_some() {
            have.retain(|(k, _)| !k.eq_ignore_ascii_case("LYRICS"));
            want.retain(|(k, _)| !k.eq_ignore_ascii_case("LYRICS"));
        }
        let norm = |v: &mut Vec<(String, String)>| {
            v.iter_mut().for_each(|(k, _)| *k = k.to_ascii_uppercase());
            v.sort();
        };
        norm(&mut want);
        norm(&mut have);
        Ok(want == have)
    }

    fn sidecars(&self, t: &TrackPlan, dst: &Path) -> Result<(), String> {
        let lrc_target = dst.with_extension("lrc");
        if lrc_target.exists() {
            return Ok(());
        }
        let text = if let Some(rel) = &t.copy_lrc_from {
            std::fs::read(self.source_root.join(rel)).map_err(|e| format!("read lyrics: {e}"))?
        } else if t.write_lrc {
            match t
                .tags_after
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("LYRICS"))
            {
                Some((_, v)) => v.as_bytes().to_vec(),
                None => return Ok(()),
            }
        } else {
            return Ok(());
        };
        write_atomic(&lrc_target, &text).map_err(|e| format!("write lyrics file: {e}"))?;
        self.created(&lrc_target);
        Ok(())
    }

    /// Set a duplicate album aside, unchanged, under `_Quarantine/<original folder>`.
    fn quarantine(
        &self,
        progress: &ApplyProgress,
        on_progress: &(dyn Fn(usize, usize) + Sync),
        total: usize,
    ) -> Result<(), Vec<ApplyError>> {
        let mut errors = Vec::new();
        let base = self.target_root.join("_Quarantine").join(&self.album.id);
        for t in &self.album.tracks {
            let name = t.source.rsplit('/').next().unwrap_or(&t.source);
            let dst = base.join(name);
            if !dst.exists() {
                let r = std::fs::create_dir_all(&base)
                    .and_then(|_| std::fs::copy(self.source_root.join(&t.source), part_path(&dst)))
                    .and_then(|_| std::fs::rename(part_path(&dst), &dst));
                match r {
                    Ok(()) => self.created(&dst),
                    Err(e) => errors.push(self.err(&t.source, format!("set aside: {e}"))),
                }
            }
            let done = progress.done.fetch_add(1, Ordering::Relaxed) + 1;
            on_progress(done, total);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The album's largest embedded front cover, as stored in the source files.
    fn original_cover(&self) -> Option<Vec<u8>> {
        self.album
            .tracks
            .iter()
            .filter(|t| t.source.to_ascii_lowercase().ends_with(".flac"))
            .filter_map(|t| {
                let mut r = BufReader::new(File::open(self.source_root.join(&t.source)).ok()?);
                let f = FlacFile::read_from(&mut r, ParseOptions::new()).ok()?;
                f.pictures()
                    .iter()
                    .find(|(p, _)| p.pic_type() == PictureType::CoverFront)
                    .or_else(|| f.pictures().first())
                    .map(|(p, _)| p.data().to_vec())
            })
            .max_by_key(Vec::len)
    }

    fn created(&self, path: &Path) {
        self.journal.record(&JournalEntry::Create {
            path: path.to_string_lossy().into(),
        });
    }
}

/// Re-encode cover art as a baseline (non-progressive) JPEG no larger than `max_px`.
pub fn baseline_jpeg(bytes: &[u8], max_px: u32) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let img = if img.width().max(img.height()) > max_px {
        img.resize(max_px, max_px, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&img.to_rgb8())
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Undo an apply run: delete every file it created (and folders left empty).
pub fn undo(journal_path: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(journal_path).map_err(|e| e.to_string())?;
    let mut target_root: Option<PathBuf> = None;
    let mut created: Vec<PathBuf> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<JournalEntry>(line) {
            Ok(JournalEntry::Begin { target, .. }) => target_root = Some(PathBuf::from(target)),
            Ok(JournalEntry::Create { path }) => created.push(PathBuf::from(path)),
            Err(_) => {} // a torn last line from a crash
        }
    }
    let root = target_root.ok_or("journal has no start record")?;
    let mut removed = 0;
    for p in created.iter().rev() {
        if p.starts_with(&root) && p.exists() && std::fs::remove_file(p).is_ok() {
            removed += 1;
        }
    }
    // Remove folders the run left empty, deepest first, never the target root itself.
    let mut dirs: Vec<PathBuf> = created
        .iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for d in dirs {
        let mut cur = d;
        while cur.starts_with(&root) && cur != root {
            if std::fs::remove_dir(&cur).is_err() {
                break; // not empty, or gone
            }
            match cur.parent() {
                Some(p) => cur = p.to_path_buf(),
                None => break,
            }
        }
    }
    let done = journal_path.with_extension("undone.jsonl");
    let _ = std::fs::rename(journal_path, done);
    Ok(removed)
}

fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = part_path(path);
    {
        let mut f = File::create(&part).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

fn file_len(p: &Path) -> Option<u64> {
    std::fs::metadata(p).ok().map(|m| m.len())
}

/// Marks a folder as a library this app built, so later builds may add to it.
pub const LIBRARY_MARKER: &str = ".music-librarian-library";

/// The new library must be its own folder: not the current one, not inside it (or around it),
/// and either empty or one this app built. A folder of other music is never mixed in.
pub fn check_target(source_root: &Path, target_root: &Path) -> Result<(), String> {
    if same_path(source_root, target_root) {
        return Err("The new library must be a different folder from the current one.".into());
    }
    let (src, dst) = (resolve(source_root), resolve(target_root));
    if dst.starts_with(&src) || src.starts_with(&dst) {
        return Err(
            "The new library can't be inside the current one, or the other way round.".into(),
        );
    }
    let has_other_files = std::fs::read_dir(target_root)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if has_other_files && !target_root.join(LIBRARY_MARKER).exists() {
        return Err(format!(
            "{} already has files that this app didn't put there. Choose an empty folder for the new library.",
            target_root.display()
        ));
    }
    Ok(())
}

/// The canonical form of a path that may not exist yet: its nearest existing parent,
/// canonicalised, plus the rest (so both sides of a comparison use the same form).
fn resolve(p: &Path) -> PathBuf {
    let mut base = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut rest = Vec::new();
    while !base.exists() {
        match base.file_name() {
            Some(n) => rest.push(n.to_owned()),
            None => break,
        }
        base.pop();
    }
    let mut out = std::fs::canonicalize(&base).unwrap_or(base);
    out.extend(rest.iter().rev());
    out
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;

    #[test]
    fn new_library_must_be_empty_or_ours_and_not_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Collection");
        std::fs::create_dir_all(&src).unwrap();
        assert!(check_target(&src, &src).is_err());
        assert!(check_target(&src, &src.join("New")).is_err());
        assert!(check_target(&src, tmp.path()).is_err());

        let fresh = tmp.path().join("Music");
        assert!(
            check_target(&src, &fresh).is_ok(),
            "a folder that doesn't exist yet"
        );
        std::fs::create_dir_all(&fresh).unwrap();
        assert!(check_target(&src, &fresh).is_ok(), "an empty folder");
        std::fs::write(fresh.join("old.flac"), b"x").unwrap();
        assert!(check_target(&src, &fresh).is_err(), "someone else's music");
        std::fs::write(fresh.join(LIBRARY_MARKER), b"").unwrap();
        assert!(check_target(&src, &fresh).is_ok(), "a library we built");
    }
}
