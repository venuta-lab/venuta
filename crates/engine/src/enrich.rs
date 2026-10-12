//! "Look up online": fetch what the files can't tell us, store it in the index, and let the
//! planner use it. Each source runs on its own thread at its own pace, so the slowest service
//! (Apple, ~20 requests a minute) never holds the others up. Results are saved as they
//! arrive; a stopped or failed run resumes where it left off.

use crate::Index;
use crate::library::Library;
use crate::net::{Http, NetError};
use crate::providers::{self, LyricsFound, ReleaseInfo};
use crate::rules::Rules;
use rusqlite::params;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

/// Embedded art smaller than this is worth replacing with a catalogue cover.
pub const GOOD_ART_PX: u32 = 1400;

#[derive(Default)]
pub struct EnrichProgress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

#[derive(Debug, Default, Serialize)]
pub struct EnrichReport {
    pub albums_looked_up: usize,
    pub albums_without_barcode: usize,
    pub musicbrainz_matches: usize,
    pub apple_matches: usize,
    pub deezer_matches: usize,
    pub lyrics_checked: usize,
    pub lyrics_found: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub elapsed_ms: u128,
}

enum Msg {
    Release {
        dir: String,
        which: &'static str,
        info: Option<Box<ReleaseInfo>>,
    },
    Lyrics {
        file_id: i64,
        found: Option<LyricsFound>,
    },
    Error(String),
}

/// What an album needs from each source.
struct AlbumJob {
    dir: String,
    barcode: String,
    need_apple: bool,
}

/// Album without a barcode: a strict title + artist search on MusicBrainz.
struct TitleJob {
    dir: String,
    title: String,
    artist: String,
    tracks: usize,
}

struct LyricsJob {
    file_id: i64,
    title: String,
    artist: String,
    album: String,
    duration_s: u64,
    language: Option<String>,
}

pub fn run(
    index: &mut Index,
    lib: &Library,
    rules: &Rules,
    progress: &EnrichProgress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> crate::Result<EnrichReport> {
    let started = std::time::Instant::now();
    let mut report = EnrichReport::default();
    let http = Http::open().map_err(|e| crate::Error::Io {
        path: crate::paths::app_data_dir(),
        source: std::io::Error::other(e),
    })?;

    // --- What to look up ----------------------------------------------------------------
    let done_albums: HashSet<String> = {
        let mut stmt = index
            .conn()
            .prepare("SELECT dir FROM album_meta WHERE error IS NULL")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let checked_lyrics: HashSet<i64> = {
        let mut stmt = index.conn().prepare("SELECT file_id FROM track_lyrics")?;
        stmt.query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut albums: Vec<AlbumJob> = Vec::new();
    let mut by_title: Vec<TitleJob> = Vec::new();
    for (dir, tracks) in lib.albums() {
        if done_albums.contains(dir) {
            continue;
        }
        let barcode = crate::library::most_common(
            tracks
                .iter()
                .filter_map(|t| t.first("BARCODE").or_else(|| t.first("UPC"))),
        );
        let Some(barcode) =
            barcode.filter(|b| b.chars().all(|c| c.is_ascii_digit()) && b.len() >= 8)
        else {
            report.albums_without_barcode += 1;
            let title = crate::library::most_common(tracks.iter().filter_map(|t| t.first("ALBUM")));
            let artist = crate::library::most_common(
                tracks
                    .iter()
                    .filter_map(|t| t.first("ALBUMARTIST").or_else(|| t.first("ARTIST"))),
            )
            .and_then(|a| crate::plan::split_names(&a).into_iter().next());
            if let (Some(title), Some(artist)) = (title, artist) {
                by_title.push(TitleJob {
                    dir: dir.to_owned(),
                    title,
                    artist,
                    tracks: tracks.len(),
                });
            }
            continue;
        };
        let best_art = tracks
            .iter()
            .filter_map(|t| t.front_cover())
            .filter_map(|p| Some(p.width?.min(p.height?)))
            .max()
            .unwrap_or(0);
        albums.push(AlbumJob {
            dir: dir.to_owned(),
            barcode,
            need_apple: best_art < GOOD_ART_PX,
        });
    }
    let mut lyrics: Vec<LyricsJob> = Vec::new();
    for t in lib
        .tracks
        .iter()
        .filter(|t| t.kind == "flac" && !checked_lyrics.contains(&t.id))
    {
        let sidecar = format!(
            "{}.lrc",
            t.path.rsplit_once('.').map(|x| x.0).unwrap_or(&t.path)
        );
        if t.has("LYRICS") || t.has("UNSYNCEDLYRICS") || lib.lyrics_files.contains(&sidecar) {
            continue;
        }
        let (Some(title), Some(artist)) = (t.first("TITLE"), t.first("ARTIST")) else {
            continue;
        };
        let first_artist = crate::plan::split_names(artist)
            .into_iter()
            .next()
            .unwrap_or_default();
        lyrics.push(LyricsJob {
            file_id: t.id,
            title: title.to_owned(),
            artist: first_artist,
            album: t.first("ALBUM").unwrap_or_default().to_owned(),
            duration_s: (t.duration_ms + 500) / 1000,
            language: rules.category_for(&t.dir).and_then(|r| r.language.clone()),
        });
    }

    let apple_count = albums.iter().filter(|a| a.need_apple).count();
    let total = albums.len() * 2 + apple_count + by_title.len() + lyrics.len();
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    on_progress(0, total);
    report.albums_looked_up = albums.len() + by_title.len();
    report.lyrics_checked = lyrics.len();

    // Rows for the albums we are about to look up, so per-source results can be merged in.
    {
        let now = now();
        let tx = index.conn_mut().transaction()?;
        for t in &by_title {
            tx.execute(
                "INSERT INTO album_meta (dir, barcode, fetched_at) VALUES (?1, NULL, ?2)
                 ON CONFLICT(dir) DO UPDATE SET error = NULL",
                params![t.dir, now],
            )?;
        }
        for a in &albums {
            tx.execute(
                "INSERT INTO album_meta (dir, barcode, fetched_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(dir) DO UPDATE SET barcode = excluded.barcode, error = NULL",
                params![a.dir, a.barcode, now],
            )?;
        }
        tx.commit()?;
    }

    // --- One thread per source; results flow back here and are saved as they arrive ------
    let (tx, rx) = mpsc::channel::<Msg>();
    std::thread::scope(|scope| {
        let http = &http;
        let cancel = &progress.cancel;
        let send = |tx: &mpsc::Sender<Msg>, m: Msg| {
            let _ = tx.send(m);
        };
        type Lookup = fn(&Http, &str) -> Result<Option<ReleaseInfo>, NetError>;
        let lookups: [(&'static str, Lookup, bool); 3] = [
            ("mb", providers::musicbrainz_by_barcode, false),
            ("deezer", providers::deezer_by_upc, false),
            ("apple", providers::apple_by_upc, true),
        ];
        for (which, lookup, only_needed) in lookups {
            let tx = tx.clone();
            let albums = &albums;
            scope.spawn(move || {
                for a in albums.iter().filter(|a| !only_needed || a.need_apple) {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    match lookup(http, &a.barcode) {
                        Ok(info) => send(
                            &tx,
                            Msg::Release {
                                dir: a.dir.clone(),
                                which,
                                info: info.map(Box::new),
                            },
                        ),
                        Err(e) => send(&tx, Msg::Error(format!("{which} {}: {e}", a.dir))),
                    }
                }
            });
        }
        {
            // Shares MusicBrainz's pace with the barcode thread (pacing is per host).
            let tx = tx.clone();
            let by_title = &by_title;
            scope.spawn(move || {
                for t in by_title {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    match providers::musicbrainz_by_title(http, &t.title, &t.artist, t.tracks) {
                        Ok(info) => send(
                            &tx,
                            Msg::Release {
                                dir: t.dir.clone(),
                                which: "mb",
                                info: info.map(Box::new),
                            },
                        ),
                        Err(e) => send(&tx, Msg::Error(format!("mb {}: {e}", t.dir))),
                    }
                }
            });
        }
        {
            let tx = tx.clone();
            let lyrics = &lyrics;
            scope.spawn(move || {
                for l in lyrics {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let prefer = providers::preferred_script(l.language.as_deref());
                    match providers::lrclib_find(
                        http,
                        &l.title,
                        &l.artist,
                        &l.album,
                        l.duration_s,
                        prefer,
                    ) {
                        Ok(found) => send(
                            &tx,
                            Msg::Lyrics {
                                file_id: l.file_id,
                                found,
                            },
                        ),
                        Err(e) => send(&tx, Msg::Error(format!("lyrics {}: {e}", l.title))),
                    }
                }
            });
        }
        drop(tx);

        let mut done = 0usize;
        let conn = index.conn_mut();
        for msg in rx {
            done += 1;
            match msg {
                Msg::Release { dir, which, info } => {
                    if info.is_some() {
                        match which {
                            "mb" => report.musicbrainz_matches += 1,
                            "apple" => report.apple_matches += 1,
                            _ => report.deezer_matches += 1,
                        }
                    }
                    let json = info.map(|i| serde_json::to_string(&i).unwrap_or_default());
                    // Column name comes from a fixed list, never from data.
                    let sql = format!(
                        "UPDATE album_meta SET {which} = ?1, fetched_at = ?2 WHERE dir = ?3"
                    );
                    let _ = conn.execute(&sql, params![json, now(), dir]);
                }
                Msg::Lyrics { file_id, found } => {
                    report.lyrics_found += usize::from(found.is_some());
                    let json = found.map(|f| serde_json::to_string(&f).unwrap_or_default());
                    let _ = conn.execute(
                        "INSERT OR REPLACE INTO track_lyrics (file_id, found, checked_at) VALUES (?1, ?2, ?3)",
                        params![file_id, json, now()],
                    );
                }
                Msg::Error(e) => report.errors.push(e),
            }
            progress.done.store(done, Ordering::Relaxed);
            on_progress(done, total);
        }
    });

    // Albums whose lookups failed outright are retried next time.
    for e in &report.errors {
        if let Some(dir) = e
            .split_once(' ')
            .map(|x| x.1)
            .and_then(|r| r.split_once(": ").map(|x| x.0))
        {
            let _ = index.conn().execute(
                "UPDATE album_meta SET error = ?1 WHERE dir = ?2",
                params![e, dir],
            );
        }
    }
    report.cancelled = progress.cancel.load(Ordering::Relaxed);
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

/// Lookup results per album folder, for the planner.
#[derive(Debug, Clone, Default)]
pub struct AlbumMeta {
    pub mb: Option<ReleaseInfo>,
    pub apple: Option<ReleaseInfo>,
    pub deezer: Option<ReleaseInfo>,
}

pub fn load_album_meta(index: &Index) -> crate::Result<HashMap<String, AlbumMeta>> {
    let mut stmt = index
        .conn()
        .prepare("SELECT dir, mb, apple, deezer FROM album_meta")?;
    let parse = |s: Option<String>| s.and_then(|j| serde_json::from_str::<ReleaseInfo>(&j).ok());
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (dir, mb, apple, deezer) = row?;
        out.insert(
            dir,
            AlbumMeta {
                mb: parse(mb),
                apple: parse(apple),
                deezer: parse(deezer),
            },
        );
    }
    Ok(out)
}

pub fn load_lyrics(index: &Index) -> crate::Result<HashMap<i64, LyricsFound>> {
    let mut stmt = index
        .conn()
        .prepare("SELECT file_id, found FROM track_lyrics WHERE found IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, json) = row?;
        if let Ok(f) = serde_json::from_str(&json) {
            out.insert(id, f);
        }
    }
    Ok(out)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Used by callers that only have a root path.
pub fn run_for_root(
    root: &Path,
    rules: &Rules,
    progress: &EnrichProgress,
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<EnrichReport, String> {
    let mut index = crate::Index::open(
        &crate::paths::index_path_for(root),
        &crate::paths::canonical_root_string(root),
    )
    .map_err(|e| e.to_string())?;
    let lib = Library::load(&index).map_err(|e| e.to_string())?;
    run(&mut index, &lib, rules, progress, on_progress).map_err(|e| e.to_string())
}
