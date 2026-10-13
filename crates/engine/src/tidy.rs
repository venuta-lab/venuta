//! Tidy: bring your library in line with your rules, in place.
//!
//! Once a library is built (and the music it came from may be gone), every change happens
//! inside it: files are renamed or moved within the library (the same drive, so nothing is
//! copied), and tags are rewritten with the same audio-proven writer as album edits. Nothing
//! is deleted. Every step is journalled before the next, so a whole run can be undone, even
//! one cut short by a crash.

use crate::apply::{ApplyError, ApplyProgress, clean_lrc, journal_dir};
use crate::plan::{AlbumAction, AlbumPlan, Plan};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

/// What tidying one album would do.
#[derive(Debug, Clone, Serialize)]
pub struct TidyAlbum {
    pub id: String,
    pub title: String,
    pub album_artist: String,
    /// The album's folder now, and where it would go (equal when only tags change).
    pub from_dir: String,
    pub to_dir: String,
    /// Songs renamed or moved.
    pub moves: usize,
    /// Songs whose tags change.
    pub retags: usize,
    /// Tag fields that change, for the summary ("CATEGORY", "ARTIST", …).
    pub fields: Vec<String>,
    /// Set aside as a duplicate of another album.
    pub set_aside: bool,
    /// Why it can't be tidied (a different file is already where it would go); left as is.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TidyPreview {
    pub albums: Vec<TidyAlbum>,
    pub moves: usize,
    pub retags: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TidyReport {
    pub albums: usize,
    pub moved: usize,
    pub retagged: usize,
    pub errors: Vec<ApplyError>,
    pub journal: String,
    pub cancelled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Entry {
    Begin {
        root: String,
        albums: usize,
    },
    /// Tags of `file` (library-relative, where it was then) before they were rewritten.
    Retag {
        file: String,
        before: Vec<(String, String)>,
    },
    /// A file moved from one place to another (library-relative; `to` may be outside the
    /// music folder, `../Set aside/…`).
    Move {
        from: String,
        to: String,
    },
    Failed {
        file: String,
        message: String,
    },
    Finished {
        cancelled: bool,
    },
}

fn tags_differ(a: &[(String, String)], b: &[(String, String)]) -> bool {
    let norm = |v: &[(String, String)]| {
        let mut v: Vec<(String, String)> = v
            .iter()
            .map(|(k, x)| (k.to_ascii_uppercase(), x.clone()))
            .collect();
        v.sort();
        v
    };
    norm(a) != norm(b)
}

fn retags(t: &crate::plan::TrackPlan) -> bool {
    t.source.to_ascii_lowercase().ends_with(".flac")
        && (tags_differ(&t.tags_before, &t.tags_after) || t.embed_lyrics_from.is_some())
}

/// Where an album's files go: its own planned folder, or `Set aside` for a duplicate.
fn destination(root: &Path, a: &AlbumPlan, rel: &str) -> (PathBuf, String) {
    match &a.action {
        AlbumAction::Migrate => (root.join(rel), rel.to_owned()),
        AlbumAction::Quarantine { .. } => {
            let aside = crate::paths::beside(root, crate::paths::Helper::SetAside);
            let name = rel.rsplit('/').next().unwrap_or(rel);
            let p = aside.join(&a.source_dir).join(name);
            let shown = relative(root, &p);
            (p, shown)
        }
    }
}

/// A path as the journal keeps it: relative to the music folder, `../` for beside it.
fn relative(root: &Path, p: &Path) -> String {
    if let Ok(r) = p.strip_prefix(root) {
        return r.to_string_lossy().replace('\\', "/");
    }
    match (root.parent(), p.strip_prefix(root.parent().unwrap_or(root))) {
        (Some(_), Ok(r)) => format!("../{}", r.to_string_lossy().replace('\\', "/")),
        _ => p.to_string_lossy().replace('\\', "/"),
    }
}

fn track_dest(root: &Path, a: &AlbumPlan, t: &crate::plan::TrackPlan) -> PathBuf {
    destination(root, a, &t.target).0
}

fn moves(root: &Path, a: &AlbumPlan, t: &crate::plan::TrackPlan) -> bool {
    root.join(&t.source) != track_dest(root, a, t)
}

/// What tidying would do, album by album, checked against what is on disk.
pub fn preview(root: &Path, plan: &Plan) -> TidyPreview {
    let mut out = TidyPreview::default();
    let sources: HashSet<PathBuf> = plan
        .albums
        .iter()
        .flat_map(|a| a.tracks.iter().map(|t| root.join(&t.source)))
        .collect();
    for a in &plan.albums {
        let moved = a.tracks.iter().filter(|t| moves(root, a, t)).count();
        let retagged = a.tracks.iter().filter(|t| retags(t)).count();
        if moved == 0 && retagged == 0 {
            continue;
        }
        // A different file already where a song would go (and not itself moving away).
        let blocked = a.tracks.iter().find_map(|t| {
            let dst = track_dest(root, a, t);
            (moves(root, a, t) && dst.exists() && !sources.contains(&dst))
                .then(|| format!("{} is already taken by another file.", relative(root, &dst)))
        });
        let fields: Vec<String> = a
            .changes
            .iter()
            .map(|c| c.field.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let to_dir = match &a.action {
            AlbumAction::Migrate => a.target_dir.clone(),
            AlbumAction::Quarantine { .. } => relative(
                root,
                &crate::paths::beside(root, crate::paths::Helper::SetAside).join(&a.source_dir),
            ),
        };
        if blocked.is_some() {
            out.blocked += 1;
        } else {
            out.moves += moved;
            out.retags += retagged;
        }
        out.albums.push(TidyAlbum {
            id: a.id.clone(),
            title: a.title.clone(),
            album_artist: a.album_artist.clone(),
            from_dir: a.source_dir.clone(),
            to_dir,
            moves: moved,
            retags: retagged,
            fields,
            set_aside: matches!(a.action, AlbumAction::Quarantine { .. }),
            blocked,
        });
    }
    out
}

struct Journal {
    path: PathBuf,
    out: Mutex<BufWriter<File>>,
}

impl Journal {
    fn create() -> Result<Self, String> {
        let dir = journal_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let path = dir.join(format!("{stamp}-tidy.jsonl"));
        let f = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            out: Mutex::new(BufWriter::new(f)),
        })
    }

    /// Written and flushed before the step it describes is taken, so a crash never loses one.
    fn record(&self, e: &Entry) {
        if let Ok(mut w) = self.out.lock() {
            let _ = serde_json::to_writer(&mut *w, e);
            let _ = w.write_all(b"\n");
            let _ = w.flush();
            let _ = w.get_ref().sync_data();
        }
    }
}

fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::rename(from, to)
}

/// Tidy the chosen albums (all of them when `only` is empty).
pub fn apply(
    root: &Path,
    plan: &Plan,
    only: &[String],
    progress: &ApplyProgress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<TidyReport, String> {
    let chosen: HashSet<&str> = only.iter().map(String::as_str).collect();
    let pre = preview(root, plan);
    let todo: HashMap<&str, &TidyAlbum> = pre
        .albums
        .iter()
        .filter(|a| a.blocked.is_none() && (chosen.is_empty() || chosen.contains(a.id.as_str())))
        .map(|a| (a.id.as_str(), a))
        .collect();
    let albums: Vec<&AlbumPlan> = plan
        .albums
        .iter()
        .filter(|a| todo.contains_key(a.id.as_str()))
        .collect();
    let total: usize = albums.iter().map(|a| a.tracks.len()).sum();
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    on_progress(0, total);

    let journal = Journal::create()?;
    journal.record(&Entry::Begin {
        root: root.to_string_lossy().replace('\\', "/"),
        albums: albums.len(),
    });
    let mut report = TidyReport {
        albums: albums.len(),
        journal: journal.path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut left_dirs: BTreeMap<PathBuf, ()> = BTreeMap::new();
    let mut done = 0;
    for a in albums {
        if progress.cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let fail = |report: &mut TidyReport, file: &str, message: String| {
            journal.record(&Entry::Failed {
                file: file.to_owned(),
                message: message.clone(),
            });
            report.errors.push(ApplyError {
                album: a.id.clone(),
                file: file.to_owned(),
                message,
            });
        };
        let mut handled: HashSet<PathBuf> = HashSet::new();
        let mut target_dirs: HashMap<PathBuf, usize> = HashMap::new();
        for t in &a.tracks {
            let src = root.join(&t.source);
            let (dst, dst_rel) = destination(root, a, &t.target);
            handled.insert(src.clone());
            // 1. Tags, where the file is now.
            if retags(t) {
                let mut tags = t.tags_after.clone();
                if let Some(lrc) = &t.embed_lyrics_from
                    && let Ok(text) = crate::text::read_text(&root.join(lrc))
                {
                    tags.retain(|(k, _)| !k.eq_ignore_ascii_case("LYRICS"));
                    tags.push(("LYRICS".into(), clean_lrc(&text)));
                }
                match crate::media::read_facts(&src, crate::media::FileKind::Flac) {
                    Ok(facts) => {
                        journal.record(&Entry::Retag {
                            file: t.source.clone(),
                            before: facts.tags,
                        });
                        match crate::edit::write_safely(&src, &tags) {
                            Ok(()) => report.retagged += 1,
                            Err(e) => fail(&mut report, &t.source, e),
                        }
                    }
                    Err(e) => fail(&mut report, &t.source, e),
                }
            }
            // 2. The file itself, and its lyrics file beside it.
            if src != dst {
                journal.record(&Entry::Move {
                    from: t.source.clone(),
                    to: dst_rel.clone(),
                });
                match rename(&src, &dst) {
                    Ok(()) => {
                        report.moved += 1;
                        if let Some(p) = src.parent() {
                            left_dirs.insert(p.to_path_buf(), ());
                        }
                        let lrc = src.with_extension("lrc");
                        handled.insert(lrc.clone());
                        if lrc.exists() && !dst.with_extension("lrc").exists() {
                            let to = dst.with_extension("lrc");
                            journal.record(&Entry::Move {
                                from: relative(root, &lrc),
                                to: relative(root, &to),
                            });
                            if let Err(e) = rename(&lrc, &to) {
                                fail(&mut report, &relative(root, &lrc), e.to_string());
                            }
                        }
                    }
                    Err(e) => fail(&mut report, &t.source, format!("move: {e}")),
                }
            }
            if t.write_lrc
                && let Some((_, l)) = t
                    .tags_after
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("LYRICS"))
            {
                let to = dst.with_extension("lrc");
                if !to.exists() {
                    let _ = std::fs::write(&to, clean_lrc(l));
                }
            }
            if let Some(p) = dst.parent() {
                *target_dirs.entry(p.to_path_buf()).or_default() += 1;
            }
            done += 1;
            progress.done.store(done, Ordering::Relaxed);
            on_progress(done, total);
        }
        // 3. What else was in the album's folder (covers, booklets) goes where most songs went.
        let src_dir = root.join(&a.source_dir);
        if let Some((to_dir, _)) = target_dirs.into_iter().max_by_key(|(_, n)| *n)
            && to_dir != src_dir
            && let Ok(rd) = std::fs::read_dir(&src_dir)
        {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_file()
                    || handled.contains(&p)
                    || crate::media::FileKind::from_path(&p).is_audio()
                {
                    continue;
                }
                let to = to_dir.join(e.file_name());
                if to.exists() {
                    continue;
                }
                journal.record(&Entry::Move {
                    from: relative(root, &p),
                    to: relative(root, &to),
                });
                if let Err(err) = rename(&p, &to) {
                    fail(&mut report, &relative(root, &p), err.to_string());
                }
            }
        }
    }
    // 4. Folders left empty go (never anything with a file in it).
    for dir in left_dirs.keys().rev() {
        remove_empty_up(dir, root);
    }
    journal.record(&Entry::Finished {
        cancelled: report.cancelled,
    });
    Ok(report)
}

fn remove_empty_up(dir: &Path, stop: &Path) {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d == stop || !d.starts_with(stop) || std::fs::remove_dir(d).is_err() {
            break;
        }
        cur = d.parent();
    }
}

fn resolve(root: &Path, rel: &str) -> PathBuf {
    match rel.strip_prefix("../") {
        Some(rest) => root.parent().unwrap_or(root).join(rest),
        None => root.join(rel),
    }
}

/// Put everything a tidy run did back: files moved back, tags restored. Returns how many
/// steps were undone.
pub fn undo(journal: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(journal).map_err(|e| e.to_string())?;
    let entries: Vec<Entry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let Some(Entry::Begin { root, .. }) = entries.first() else {
        return Err("This record does not say which library it was for.".into());
    };
    let root = PathBuf::from(root);
    let mut undone = 0;
    let mut errors = Vec::new();
    let mut emptied: Vec<PathBuf> = Vec::new();
    for e in entries.iter().rev() {
        match e {
            Entry::Move { from, to } => {
                let (now, was) = (resolve(&root, to), resolve(&root, from));
                if now.exists() && !was.exists() {
                    match rename(&now, &was) {
                        Ok(()) => {
                            undone += 1;
                            if let Some(p) = now.parent() {
                                emptied.push(p.to_path_buf());
                            }
                        }
                        Err(err) => errors.push(format!("{to}: {err}")),
                    }
                }
            }
            Entry::Retag { file, before } => {
                let p = resolve(&root, file);
                // A run cut short mid-write leaves its working copy; the original was untouched.
                let mut part = p.as_os_str().to_owned();
                part.push(".part");
                let _ = std::fs::remove_file(PathBuf::from(part));
                match crate::edit::write_safely(&p, before) {
                    Ok(()) => undone += 1,
                    Err(err) => errors.push(format!("{file}: {err}")),
                }
            }
            _ => {}
        }
    }
    let top = root.parent().unwrap_or(&root).to_path_buf();
    for d in emptied {
        remove_empty_up(&d, &top);
    }
    if !errors.is_empty() {
        return Err(format!(
            "{} steps were undone; {} could not be: {}",
            undone,
            errors.len(),
            errors.join("; ")
        ));
    }
    let _ = std::fs::rename(journal, journal.with_extension("undone.jsonl"));
    Ok(undone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_paths_round_trip_inside_and_beside_the_music() {
        let home = Path::new("D:/Lib");
        let root = home.join("Music");
        for p in [
            root.join("Film/Taal (1999)/01 - A.flac"),
            home.join("Set aside/x/01.flac"),
        ] {
            let rel = relative(&root, &p);
            assert_eq!(resolve(&root, &rel), p, "{rel}");
        }
        assert_eq!(
            relative(&root, &home.join("Set aside/x/01.flac")),
            "../Set aside/x/01.flac"
        );
    }
}
