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
    /// Deep-check results, if the track has been analysed since it last changed.
    pub quality: Option<Quality>,
}

#[derive(Debug, Clone)]
pub struct Quality {
    pub verify_ok: Option<bool>,
    pub decode_error: Option<String>,
    pub effective_bits: Option<u8>,
    pub cutoff_hz: Option<u32>,
    pub hires_db: Option<f32>,
    pub lufs: Option<f64>,
    pub peak: Option<f64>,
    pub album_lufs: Option<f64>,
    pub album_peak: Option<f64>,
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
    /// Image files (e.g. cover.jpg) per album folder: (relative path, bytes).
    pub images: HashMap<String, Vec<(String, u64)>>,
    /// Online lookup results per album folder (empty until "Look up online" has run).
    pub meta: HashMap<String, crate::enrich::AlbumMeta>,
    /// Validated synced lyrics found online, per track id.
    pub fetched_lyrics: HashMap<i64, crate::providers::LyricsFound>,
    /// Decoded-audio comparisons of likely duplicates, by (smaller id, larger id).
    pub pairs: HashMap<(i64, i64), crate::compare::Comparison>,
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
                    quality: None,
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
        {
            // Only results that still match the file (same size and mtime) count.
            let mut stmt = conn.prepare(
                "SELECT q.file_id, q.verify_ok, q.decode_error, q.effective_bits, q.cutoff_hz,
                        q.hires_db, q.lufs, q.peak, q.album_lufs, q.album_peak
                 FROM quality q JOIN files f ON f.id = q.file_id
                 WHERE q.size = f.size AND q.mtime_ns = f.mtime_ns",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                if let Some(&i) = by_id.get(&r.get::<_, i64>(0)?) {
                    tracks[i].quality = Some(Quality {
                        verify_ok: r.get(1)?,
                        decode_error: r.get(2)?,
                        effective_bits: r.get::<_, Option<i64>>(3)?.map(|v| v as u8),
                        cutoff_hz: r.get::<_, Option<i64>>(4)?.map(|v| v as u32),
                        hires_db: r.get::<_, Option<f64>>(5)?.map(|v| v as f32),
                        lufs: r.get(6)?,
                        peak: r.get(7)?,
                        album_lufs: r.get(8)?,
                        album_peak: r.get(9)?,
                    });
                }
            }
        }
        let lyrics_files = {
            let mut stmt = conn.prepare("SELECT path FROM files WHERE kind = 'lyrics'")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut images: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT path, size FROM files WHERE kind = 'image'")?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?;
            for row in rows {
                let (path, size) = row?;
                images
                    .entry(parent_dir(&path).to_owned())
                    .or_default()
                    .push((path, size));
            }
        }
        Ok(Self {
            tracks,
            lyrics_files,
            images,
            meta: crate::enrich::load_album_meta(index)?,
            fetched_lyrics: crate::enrich::load_lyrics(index)?,
            pairs: crate::compare::load(index)?,
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
                // "track:<path>": that one track's art (a single from a folder of singles).
                if let Some(path) = d.strip_prefix("track:") {
                    let t = self.tracks.iter().find(|t| t.path == path)?;
                    let pic = t.front_cover()?;
                    return Some(crate::art::CoverRequest {
                        dir: d.clone(),
                        track_rel: t.path.clone(),
                        data_hash: pic.data_hash.clone(),
                    });
                }
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
