//! In-place edits: the owner's own changes (tags, ratings) to files in the library they are
//! browsing. Same safety as apply: write a `.part` copy beside the file, prove the audio is
//! byte-identical, flush, then replace the original. The previous tags are journalled so any
//! edit can be undone.

use crate::media::flac_layout;
use crate::plan::TagSet;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

/// One file's change: each key gets exactly these values (empty list removes the key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagEdit {
    pub file: String,
    pub set: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Default, Serialize)]
pub struct EditReport {
    pub changed: usize,
    pub unchanged: usize,
    pub errors: Vec<(String, String)>,
    pub journal: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EditEntry {
    root: String,
    file: String,
    before: Vec<(String, String)>,
}

pub fn edit_in_place(root: &Path, edits: &[TagEdit]) -> Result<EditReport, String> {
    let mut report = EditReport::default();
    let mut journal: Option<(PathBuf, File)> = None;
    for e in edits {
        let path = root.join(&e.file);
        match apply_one(&path, &e.set) {
            Ok(None) => report.unchanged += 1,
            Ok(Some(before)) => {
                report.changed += 1;
                if journal.is_none() {
                    let dir = crate::apply::journal_dir();
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or_default();
                    let p = dir.join(format!("{stamp}-edit.jsonl"));
                    let f = OpenOptions::new()
                        .create_new(true)
                        .append(true)
                        .open(&p)
                        .map_err(|e| e.to_string())?;
                    journal = Some((p, f));
                }
                if let Some((_, f)) = journal.as_mut() {
                    let entry = EditEntry {
                        root: root.to_string_lossy().into(),
                        file: e.file.clone(),
                        before,
                    };
                    let _ = serde_json::to_writer(&mut *f, &entry);
                    let _ = f.write_all(b"\n");
                    let _ = f.flush();
                }
            }
            Err(msg) => report.errors.push((e.file.clone(), msg)),
        }
    }
    report.journal = journal.map(|(p, _)| p.to_string_lossy().into_owned());
    Ok(report)
}

/// Put back the tags an edit journal recorded. Returns how many files were restored.
pub fn undo_edits(journal: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(journal).map_err(|e| e.to_string())?;
    let mut restored = 0;
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<EditEntry>(line) else {
            continue;
        };
        let path = Path::new(&entry.root).join(&entry.file);
        write_safely(&path, &entry.before)?;
        restored += 1;
    }
    let _ = std::fs::rename(journal, journal.with_extension("undone.jsonl"));
    Ok(restored)
}

/// Returns the previous tags when the file changed, `None` when it was already as requested.
fn apply_one(
    path: &Path,
    set: &[(String, Vec<String>)],
) -> Result<Option<Vec<(String, String)>>, String> {
    if !path
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".flac")
    {
        return Err("only FLAC files can be edited so far".into());
    }
    let before = read_tags(path)?;
    let mut tags = TagSet(before.clone());
    for (key, values) in set {
        let values: Vec<String> = values
            .iter()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect();
        if values.is_empty() {
            tags.remove(key);
        } else {
            tags.set(key, values);
        }
    }
    if tags.0 == before {
        return Ok(None);
    }
    write_safely(path, &tags.0)?;
    Ok(Some(before))
}

fn read_tags(path: &Path) -> Result<Vec<(String, String)>, String> {
    Ok(crate::media::read_facts(path, crate::media::FileKind::Flac)?.tags)
}

/// Replace a file's tags without ever risking its audio.
pub(crate) fn write_safely(path: &Path, tags: &[(String, String)]) -> Result<(), String> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::copy(path, &part).map_err(|e| format!("copy: {e}"))?;
    let result = (|| -> Result<(), String> {
        let original = flac_layout::audio_fingerprint(path).map_err(|e| e.to_string())?;
        let mut flac = {
            let mut r = BufReader::new(File::open(&part).map_err(|e| e.to_string())?);
            FlacFile::read_from(&mut r, ParseOptions::new().implicit_conversions(false))
                .map_err(|e| e.to_string())?
        };
        if flac.vorbis_comments().is_none() {
            flac.set_vorbis_comments(Default::default());
        }
        let vc = flac.vorbis_comments_mut().expect("just ensured");
        let _ = vc.take_items().count();
        for (k, v) in tags {
            vc.push(k.clone(), v.clone());
        }
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&part)
            .map_err(|e| e.to_string())?;
        flac.save_to(&mut f, WriteOptions::default())
            .map_err(|e| format!("write tags: {e}"))?;
        f.sync_all().map_err(|e| format!("flush: {e}"))?;
        drop(f);
        let after = flac_layout::audio_fingerprint(&part).map_err(|e| e.to_string())?;
        if after != original {
            return Err("audio changed during tag writing; the original was left untouched".into());
        }
        // On Windows this replaces the original atomically (MoveFileEx with REPLACE_EXISTING).
        std::fs::rename(&part, path).map_err(|e| format!("replace: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Star rating (0–5) ↔ RATING tag value (0–100, the MusicBee convention).
pub fn stars_to_rating(stars: u8) -> Option<String> {
    (stars > 0).then(|| (u32::from(stars.min(5)) * 20).to_string())
}

pub fn rating_to_stars(rating: &str) -> u8 {
    rating
        .trim()
        .parse::<f32>()
        .map(|r| {
            if r <= 1.0 && r > 0.0 {
                r * 5.0
            } else {
                r / 20.0
            }
        }) // Picard writes 0–1
        .map(|s| s.round().clamp(0.0, 5.0) as u8)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings_round_trip() {
        assert_eq!(stars_to_rating(4).as_deref(), Some("80"));
        assert_eq!(stars_to_rating(0), None);
        assert_eq!(rating_to_stars("80"), 4);
        assert_eq!(rating_to_stars("100"), 5);
        assert_eq!(rating_to_stars("0.6"), 3); // Picard's scale
        assert_eq!(rating_to_stars("junk"), 0);
    }
}
