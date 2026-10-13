//! What you've listened to, kept with your library (`.venuta/listening.sqlite`), never in
//! your files: plays (a song counts once you've heard half of it, or four minutes), and where
//! you were (queue, song, position) so the next start picks up there.

use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn open() -> Result<Connection, String> {
    let dir = engine::paths::app_data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c = Connection::open(dir.join("listening.sqlite")).map_err(|e| e.to_string())?;
    c.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS plays (
             id INTEGER PRIMARY KEY,
             root TEXT NOT NULL,
             file TEXT NOT NULL,
             dir TEXT NOT NULL,
             at INTEGER NOT NULL,
             heard_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS plays_root_at ON plays(root, at);
         CREATE TABLE IF NOT EXISTS state (k TEXT PRIMARY KEY, v TEXT NOT NULL);",
    )
    .map_err(|e| e.to_string())?;
    // Where each play came from (a playlist), added later: there already, or added now.
    let has_ctx = c.prepare("SELECT ctx FROM plays LIMIT 0").is_ok();
    if !has_ctx {
        c.execute_batch("ALTER TABLE plays ADD COLUMN ctx TEXT")
            .map_err(|e| e.to_string())?;
    }
    Ok(c)
}

fn key(root: &str) -> String {
    engine::paths::canonical_root_string(Path::new(root))
}

/// How the player was playing: kept with the place, and put back after a restart.
pub struct HowItPlays<'a> {
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: player::queue::Repeat,
    /// While shuffled: the order before shuffling (see `Queue::unshuffled`).
    pub order: Option<&'a [usize]>,
}

/// Counts plays as they happen, from the player's status.
#[derive(Default)]
pub struct Tracker {
    conn: Option<Connection>,
    song: Option<String>,
    heard_ms: u64,
    last_pos: u64,
    counted: bool,
}

impl Tracker {
    /// One status: `song` is the playing file (library-relative) with its album folder.
    pub fn update(
        &mut self,
        root: &str,
        song: Option<(&str, &str)>,
        playing: bool,
        position_ms: u64,
        duration_ms: u64,
        from: &str,
    ) {
        let id = song.map(|(f, _)| f.to_owned());
        if id != self.song {
            self.song = id;
            self.heard_ms = 0;
            self.counted = false;
            self.last_pos = position_ms;
            return;
        }
        if playing {
            let step = position_ms.saturating_sub(self.last_pos);
            // Real listening moves a little at a time; a jump is a seek.
            if step > 0 && step < 2_000 {
                self.heard_ms += step;
            }
        }
        self.last_pos = position_ms;
        let enough = (duration_ms / 2).clamp(15_000, 240_000);
        if !self.counted
            && self.heard_ms >= enough
            && let Some((file, dir)) = song
        {
            self.counted = true;
            if self.conn.is_none() {
                self.conn = open().ok();
            }
            if let Some(c) = self.conn.as_ref() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or_default();
                // Only a playlist is kept as where it came from (to offer it again).
                let ctx = playlist_of(from).map(|_| from.to_owned());
                let _ = c.execute(
                    "INSERT INTO plays (root, file, dir, at, heard_ms, ctx) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![key(root), file, dir, now, self.heard_ms as i64, ctx],
                );
            }
        }
    }

    /// Remember where you were (the queue as JSON, the index and position), and how it plays.
    pub fn save_place(
        &mut self,
        root: &str,
        queue_json: Option<&str>,
        index: i64,
        position_ms: u64,
        from: &str,
        how: &HowItPlays,
    ) {
        let HowItPlays {
            volume,
            shuffle,
            repeat,
            order,
        } = how;
        if self.conn.is_none() {
            self.conn = open().ok();
        }
        let Some(c) = self.conn.as_ref() else { return };
        if let Some(q) = queue_json {
            let _ = c.execute(
                "INSERT OR REPLACE INTO state (k, v) VALUES ('queue', ?1)",
                params![q],
            );
        }
        let place = serde_json::json!({
            "root": root,
            "index": index,
            "position_ms": position_ms,
            "from": from,
            "volume": volume,
            "shuffle": shuffle,
            "repeat": repeat,
            "order": order,
        });
        let _ = c.execute(
            "INSERT OR REPLACE INTO state (k, v) VALUES ('place', ?1)",
            params![place.to_string()],
        );
    }
}

/// Keep the sound tools' settings.
pub fn save_sound(sound: &player::dsp::Sound) {
    if let (Ok(c), Ok(v)) = (open(), serde_json::to_string(sound)) {
        let _ = c.execute(
            "INSERT OR REPLACE INTO state (k, v) VALUES ('sound', ?1)",
            params![v],
        );
    }
}

pub fn saved_sound() -> Option<player::dsp::Sound> {
    let c = open().ok()?;
    let v: String = c
        .query_row("SELECT v FROM state WHERE k = 'sound'", [], |r| r.get(0))
        .ok()?;
    serde_json::from_str(&v).ok()
}

/// Where you were: the queue, the song and position in it.
pub fn saved_place() -> Option<(serde_json::Value, serde_json::Value)> {
    let c = open().ok()?;
    let get = |k: &str| -> Option<serde_json::Value> {
        let v: String = c
            .query_row("SELECT v FROM state WHERE k = ?1", params![k], |r| r.get(0))
            .ok()?;
        serde_json::from_str(&v).ok()
    };
    Some((get("queue")?, get("place")?))
}

/// The playlist a queue came from, when it did (the player's `from` is JSON: kind, ref, label).
fn playlist_of(from: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(from).ok()?;
    (v.get("kind")?.as_str()? == "playlist")
        .then(|| v.get("ref")?.as_str().map(str::to_owned))
        .flatten()
}

#[derive(Serialize, Default)]
pub struct Summary {
    /// Album folders, most recently played first.
    pub recent: Vec<String>,
    /// Album folders by plays, most first.
    pub top: Vec<(String, u32)>,
    /// Plays per song (library-relative path).
    pub counts: HashMap<String, u32>,
    /// Album folders played at least once, with when they were last played (ms).
    pub last_played: HashMap<String, i64>,
    /// Playlists played from, most recent first, with when (ms).
    pub recent_playlists: Vec<(String, i64)>,
}

/// What you've played in this folder.
#[tauri::command]
pub async fn listening_summary(root: String) -> Result<Summary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = open()?;
        let k = key(&root);
        let mut out = Summary::default();
        let mut stmt = c
            .prepare("SELECT dir, MAX(at) FROM plays WHERE root = ?1 GROUP BY dir ORDER BY MAX(at) DESC")
            .map_err(|e| e.to_string())?;
        for row in stmt
            .query_map(params![k], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
        {
            out.recent.push(row.0.clone());
            out.last_played.insert(row.0, row.1);
        }
        let mut stmt = c
            .prepare("SELECT dir, COUNT(*) AS n FROM plays WHERE root = ?1 GROUP BY dir ORDER BY n DESC LIMIT 40")
            .map_err(|e| e.to_string())?;
        out.top = stmt
            .query_map(params![k], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        let mut stmt = c
            .prepare("SELECT ctx, MAX(at) FROM plays WHERE root = ?1 AND ctx IS NOT NULL GROUP BY ctx ORDER BY MAX(at) DESC LIMIT 60")
            .map_err(|e| e.to_string())?;
        let mut seen = HashSet::new();
        for (ctx, at) in stmt
            .query_map(params![k], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
        {
            // The same playlist under another label: once, at its latest.
            if let Some(name) = playlist_of(&ctx).filter(|n| seen.insert(n.clone())) {
                out.recent_playlists.push((name, at));
            }
        }
        let mut stmt = c
            .prepare("SELECT file, COUNT(*) FROM plays WHERE root = ?1 GROUP BY file")
            .map_err(|e| e.to_string())?;
        out.counts = stmt
            .query_map(params![k], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::Tracker;

    const FROM: &str = r#"{"kind":"playlist","ref":"Hindi","label":"Hindi"}"#;

    #[test]
    fn a_play_counts_after_half_the_song_and_seeks_dont_count() {
        // Plays go to a scratch library, never yours.
        let dir = std::env::temp_dir().join(format!("venuta-listening-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: set before anything reads it, in a test process of its own.
        unsafe { std::env::set_var("VENUTA_HOME", &dir) };
        let mut t = Tracker::default();
        // A 3-minute song: 90 s of listening counts.
        let song = Some(("a.flac", "A"));
        t.update("R", song, true, 0, 180_000, FROM);
        t.update("R", song, true, 100_000, 180_000, FROM); // a seek: not heard
        assert_eq!(t.heard_ms, 0);
        for p in (100_000..=190_000).step_by(1_000) {
            t.update("R", song, true, p, 180_000, FROM);
        }
        assert!(t.counted, "heard {} ms", t.heard_ms);
        t.update("R", Some(("b.flac", "A")), true, 0, 180_000, FROM);
        assert!(!t.counted, "a new song starts afresh");
        assert_eq!(super::playlist_of(FROM).as_deref(), Some("Hindi"));
        assert_eq!(super::playlist_of("Taal"), None, "plain words from before");
    }
}
