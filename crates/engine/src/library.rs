//! In-memory view of the indexed library: tracks grouped into albums (one folder = one
//! release). Built from the index in one pass; cheap enough to rebuild on every request for
//! libraries in the tens of thousands of tracks.

use crate::error::Result;
use crate::index::Index;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct Picture {
    pub pic_type: u8,
    pub format: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub progressive: bool,
    pub data_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub dir: String,
    pub kind: String,
    pub size: u64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub audio_md5: Option<Vec<u8>>,
    pub vendor: Option<String>,
    /// Upper-cased key → values in file order.
    pub tags: HashMap<String, Vec<String>>,
    /// Keys exactly as written, in file order (for junk detection).
    pub raw_keys: Vec<String>,
    pub pictures: Vec<Picture>,
}

impl Track {
    pub fn first(&self, key: &str) -> Option<&str> {
        self.tags
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    pub fn has(&self, key: &str) -> bool {
        self.tags
            .get(key)
            .is_some_and(|v| v.iter().any(|s| !s.trim().is_empty()))
    }

    pub fn front_cover(&self) -> Option<&Picture> {
        self.pictures
            .iter()
            .find(|p| p.pic_type == 3)
            .or_else(|| self.pictures.first())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumSummary {
    /// Folder relative to the library root; the album's identity for now.
    pub dir: String,
    pub title: String,
    pub album_artist: String,
    pub year: Option<String>,
    pub tracks: usize,
    pub duration_ms: u64,
    /// A track whose embedded art represents the album, if any.
    pub cover_track: Option<String>,
}

pub struct Library {
    pub tracks: Vec<Track>,
    /// Paths of `.lrc` sidecars, for lyrics checks.
    pub lyrics_files: std::collections::HashSet<String>,
}

impl Library {
    pub fn load(index: &Index) -> Result<Self> {
        let conn = index.conn();
        let mut tracks: Vec<Track> = Vec::new();
        let mut by_id: HashMap<i64, usize> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT id, path, kind, size, COALESCE(duration_ms,0), COALESCE(sample_rate,0),
                        COALESCE(bit_depth,0), audio_md5, vendor
                 FROM files WHERE kind IN ('flac','other_audio') ORDER BY path",
            )?;
            let rows = stmt.query_map([], |r| {
                let path: String = r.get(1)?;
                Ok(Track {
                    id: r.get(0)?,
                    dir: parent_dir(&path).to_owned(),
                    path,
                    kind: r.get(2)?,
                    size: r.get::<_, i64>(3)? as u64,
                    duration_ms: r.get::<_, i64>(4)? as u64,
                    sample_rate: r.get::<_, i64>(5)? as u32,
                    bit_depth: r.get::<_, i64>(6)? as u8,
                    audio_md5: r.get(7)?,
                    vendor: r.get(8)?,
                    tags: HashMap::new(),
                    raw_keys: Vec::new(),
                    pictures: Vec::new(),
                })
            })?;
            for t in rows {
                let t = t?;
                by_id.insert(t.id, tracks.len());
                tracks.push(t);
            }
        }
        {
            let mut stmt = conn
                .prepare("SELECT file_id, key, raw_key, value FROM tags ORDER BY file_id, idx")?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                if let Some(&i) = by_id.get(&r.get::<_, i64>(0)?) {
                    let t = &mut tracks[i];
                    t.tags.entry(r.get(1)?).or_default().push(r.get(3)?);
                    t.raw_keys.push(r.get(2)?);
                }
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT file_id, pic_type, format, width, height, COALESCE(progressive,0), data_hash
                 FROM pictures ORDER BY file_id, idx",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                if let Some(&i) = by_id.get(&r.get::<_, i64>(0)?) {
                    tracks[i].pictures.push(Picture {
                        pic_type: r.get::<_, i64>(1)? as u8,
                        format: r.get(2)?,
                        width: r.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                        height: r.get::<_, Option<i64>>(4)?.map(|v| v as u32),
                        progressive: r.get::<_, i64>(5)? != 0,
                        data_hash: r.get(6)?,
                    });
                }
            }
        }
        let lyrics_files = {
            let mut stmt = conn.prepare("SELECT path FROM files WHERE kind = 'lyrics'")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(Self {
            tracks,
            lyrics_files,
        })
    }

    /// Tracks grouped by folder, folders in path order.
    pub fn albums(&self) -> BTreeMap<&str, Vec<&Track>> {
        let mut map: BTreeMap<&str, Vec<&Track>> = BTreeMap::new();
        for t in &self.tracks {
            map.entry(t.dir.as_str()).or_default().push(t);
        }
        map
    }

    /// What to read to make each album's cover thumbnail. Albums without art are skipped.
    pub fn cover_requests(&self, dirs: &[String]) -> Vec<crate::art::CoverRequest> {
        let albums = self.albums();
        dirs.iter()
            .filter_map(|d| {
                let tracks = albums.get(d.as_str())?;
                let (track, pic) = tracks
                    .iter()
                    .find_map(|t| t.front_cover().map(|p| (t, p)))?;
                Some(crate::art::CoverRequest {
                    dir: d.clone(),
                    track_rel: track.path.clone(),
                    data_hash: pic.data_hash.clone(),
                })
            })
            .collect()
    }

    pub fn album_summaries(&self) -> Vec<AlbumSummary> {
        self.albums()
            .into_iter()
            .map(|(dir, tracks)| summarize(dir, &tracks))
            .collect()
    }
}

pub fn summarize(dir: &str, tracks: &[&Track]) -> AlbumSummary {
    let title = most_common(tracks.iter().filter_map(|t| t.first("ALBUM")))
        .unwrap_or_else(|| dir.rsplit('/').next().unwrap_or(dir).to_owned());
    let album_artist = most_common(
        tracks
            .iter()
            .filter_map(|t| t.first("ALBUMARTIST").or_else(|| t.first("ARTIST"))),
    )
    .unwrap_or_default();
    let year = most_common(tracks.iter().filter_map(|t| t.first("DATE")))
        .map(|d| d.chars().take(4).collect());
    AlbumSummary {
        dir: dir.to_owned(),
        title,
        album_artist,
        year,
        tracks: tracks.len(),
        duration_ms: tracks.iter().map(|t| t.duration_ms).sum(),
        cover_track: tracks
            .iter()
            .find(|t| t.front_cover().is_some())
            .map(|t| t.path.clone()),
    }
}

pub fn parent_dir(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

pub(crate) fn most_common<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(v, _)| v.to_owned())
}
