//! Timed lyrics you make yourself (tapping along in the lyrics maker): written as a `.lrc`
//! beside the song, which every player reads, as a change you can undo in Activity. The song's
//! own file is never touched.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Whether lyrics text has times (`[mm:ss.xx]` at a line's start).
pub fn is_timed(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start();
        let Some(rest) = l.strip_prefix('[') else {
            return false;
        };
        let Some((stamp, _)) = rest.split_once(']') else {
            return false;
        };
        let Some((m, s)) = stamp.split_once(':') else {
            return false;
        };
        !m.is_empty()
            && m.chars().all(|c| c.is_ascii_digit())
            && s.replace([',', '.'], "")
                .chars()
                .all(|c| c.is_ascii_digit())
            && !s.is_empty()
    })
}

/// `[01:02.34]words`, one line each, in time order.
pub fn format(lines: &[(u64, String)]) -> String {
    let mut sorted: Vec<&(u64, String)> = lines.iter().collect();
    sorted.sort_by_key(|(ms, _)| *ms);
    let mut out = String::new();
    for (ms, text) in sorted {
        let cs = ms / 10;
        out.push_str(&format!(
            "[{:02}:{:02}.{:02}]{}\n",
            cs / 6000,
            (cs / 100) % 60,
            cs % 100,
            text.trim()
        ));
    }
    out
}

/// One saved timing, so it can be undone: the `.lrc` (library-relative) and what it held before
/// (None: there was none).
#[derive(Debug, Serialize, Deserialize)]
pub struct LyricsEntry {
    pub root: String,
    pub file: String,
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_bytes: Option<Vec<u8>>,
    /// What this timing wrote, so undo can tell whether the file changed since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// The `.lrc` path (library-relative) for a song.
pub fn lrc_for(song: &str) -> String {
    // The extension is only a dot in the file's own name, not one in a folder's.
    let name_at = song.rfind('/').map_or(0, |i| i + 1);
    match song[name_at..].rfind('.') {
        Some(dot) if dot > 0 => format!("{}.lrc", &song[..name_at + dot]),
        _ => format!("{song}.lrc"),
    }
}

/// The `.lrc` files beside songs (library-relative, as the index lists them) that hold words with
/// no times, so Health can say those songs need timing.
pub fn untimed_sidecars<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a String>,
) -> std::collections::HashSet<String> {
    files
        .into_iter()
        .filter(|f| {
            crate::text::read_text(&root.join(f.as_str()))
                .is_ok_and(|t| !t.trim().is_empty() && !is_timed(&t))
        })
        .cloned()
        .collect()
}

/// Write timed lyrics for `song` (library-relative) as its `.lrc`, journaled in `journals`.
/// Returns the journal's path.
pub fn save_in(
    journals: &Path,
    root: &Path,
    song: &str,
    lines: &[(u64, String)],
) -> Result<PathBuf, String> {
    if lines.is_empty() {
        return Err("There are no timed lines to save.".into());
    }
    if !crate::paths::is_inside_rel(song) {
        return Err("That song isn't in the library.".into());
    }
    let file = lrc_for(song);
    let path = root.join(&file);
    // Kept as it was, byte for byte (a BOM, another encoding, Windows line ends).
    let (before, before_bytes) = match path.is_file() {
        false => (None, None),
        true => match String::from_utf8(std::fs::read(&path).map_err(|e| e.to_string())?) {
            Ok(text) => (Some(text), None),
            Err(e) => (None, Some(e.into_bytes())),
        },
    };
    std::fs::create_dir_all(journals).map_err(|e| e.to_string())?;
    let mut millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    // Two saves in one millisecond would share a journal, and the first could no longer be
    // undone on its own: the next free millisecond instead.
    while journals.join(format!("{millis}-lyrics.jsonl")).exists() {
        millis += 1;
    }
    let journal = journals.join(format!("{millis}-lyrics.jsonl"));
    // The journal first: if writing the lyrics then fails, undo finds nothing changed.
    let entry = LyricsEntry {
        root: root.to_string_lossy().into_owned(),
        file: file.clone(),
        before,
        before_bytes,
        after: Some(format(lines)),
    };
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    std::fs::write(&journal, format!("{line}\n")).map_err(|e| e.to_string())?;
    write_atomic(&path, format(lines).as_bytes())?;
    Ok(journal)
}

/// [`save_in`] with Venuta's own journal folder (Activity lists it from there).
pub fn save(root: &Path, song: &str, lines: &[(u64, String)]) -> Result<PathBuf, String> {
    save_in(&crate::apply::journal_dir(), root, song, lines)
}

/// Put a saved timing back as it was: the old `.lrc`, or none. Returns files restored.
pub fn undo(journal: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(journal).map_err(|e| e.to_string())?;
    let mut n = 0;
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()) {
        let e: LyricsEntry = serde_json::from_str(line).map_err(|e| e.to_string())?;
        // Only ever a .lrc inside the library it names.
        if !crate::paths::is_inside_rel(&e.file) || !e.file.to_ascii_lowercase().ends_with(".lrc") {
            return Err(
                "This change can't be undone here: it names a file outside the library.".into(),
            );
        }
        let path = Path::new(&e.root).join(&e.file);
        // Timed again (or edited) since: undoing this would throw the newer words away.
        if let Some(after) = &e.after
            && std::fs::read(&path).ok().as_deref() != Some(after.as_bytes())
        {
            return Err(
                "These lyrics changed after this timing was saved. Undo the newer change first."
                    .into(),
            );
        }
        match (e.before_bytes, e.before) {
            (Some(old), _) => write_atomic(&path, &old)?,
            (None, Some(old)) => write_atomic(&path, old.as_bytes())?,
            (None, None) => match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.to_string()),
            },
        }
        n += 1;
    }
    let done = journal.with_file_name(
        journal
            .file_name()
            .map(|f| f.to_string_lossy().replace(".jsonl", ".undone.jsonl"))
            .unwrap_or_default(),
    );
    std::fs::rename(journal, done).map_err(|e| e.to_string())?;
    Ok(n)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_lines_are_written_in_order_to_the_hundredth() {
        let lines = vec![
            (61_390, "Second".to_owned()),
            (5_000, " First ".to_owned()),
            (0, String::new()),
        ];
        assert_eq!(
            format(&lines),
            "[00:00.00]\n[00:05.00]First\n[01:01.39]Second\n"
        );
        assert!(is_timed(&format(&lines)));
        assert!(!is_timed("Just words\nwith no times"));
        assert!(!is_timed("[ar:Someone]\nwords"));
    }

    #[test]
    fn a_saved_timing_can_be_undone_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let (root, journals) = (dir.path().join("Music"), dir.path().join("journal"));
        std::fs::create_dir_all(root.join("A")).unwrap();
        let lines = vec![(1_000, "One".to_owned())];
        // No .lrc before: undo removes the new one.
        let j = save_in(&journals, &root, "A/01 - One.flac", &lines).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("A/01 - One.lrc")).unwrap(),
            "[00:01.00]One\n"
        );
        undo(&j).unwrap();
        assert!(!root.join("A/01 - One.lrc").exists());
        // A plain .lrc before: undo puts it back exactly, BOM and line ends and all.
        let plain = b"\xEF\xBB\xBFOne\r\nTwo\r\n";
        std::fs::write(root.join("A/01 - One.lrc"), plain).unwrap();
        let sidecar = "A/01 - One.lrc".to_owned();
        assert_eq!(
            untimed_sidecars(&root, [&sidecar]).len(),
            1,
            "words, no times"
        );
        let j = save_in(&journals, &root, "A/01 - One.flac", &lines).unwrap();
        assert!(untimed_sidecars(&root, [&sidecar]).is_empty());
        undo(&j).unwrap();
        assert_eq!(std::fs::read(root.join("A/01 - One.lrc")).unwrap(), plain);
        // Not UTF-8 (an old Windows file): still exact.
        let latin = b"Caf\xE9\n";
        std::fs::write(root.join("A/01 - One.lrc"), latin).unwrap();
        let j = save_in(&journals, &root, "A/01 - One.flac", &lines).unwrap();
        undo(&j).unwrap();
        assert_eq!(std::fs::read(root.join("A/01 - One.lrc")).unwrap(), latin);
        // Two timings: the older can't be undone first, as that would lose the newer one.
        let first = save_in(&journals, &root, "A/01 - One.flac", &lines).unwrap();
        let newer = vec![(2_000, "One".to_owned())];
        let second = save_in(&journals, &root, "A/01 - One.flac", &newer).unwrap();
        assert!(undo(&first).is_err());
        undo(&second).unwrap();
        undo(&first).unwrap();
        assert_eq!(std::fs::read(root.join("A/01 - One.lrc")).unwrap(), latin);
    }
}
