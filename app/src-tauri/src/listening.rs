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
    // Plays are written while summaries are read and the place is saved: wait for one another.
    c.busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| e.to_string())?;
    engine::index::use_wal(&c).map_err(|e| e.to_string())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS plays (
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
    // Columns added later: there already, or added now. Where each play came from (a playlist),
    // and for an online song (`file` is `online:<provider>:<id>`, `dir` empty) what it was,
    // since no library file says so.
    for column in ["ctx", "title", "artists", "album", "art"] {
        if c.prepare(&format!("SELECT {column} FROM plays LIMIT 0"))
            .is_err()
        {
            c.execute_batch(&format!("ALTER TABLE plays ADD COLUMN {column} TEXT"))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(c)
}

/// What an online song was, kept with its plays (a library file says this of itself).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OnlineSong {
    /// `online:<provider>:<id>`.
    pub file: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art: Option<String>,
}

/// Whether plays are kept at all (Settings: keep a history of what you play).
fn history_on() -> bool {
    engine::settings::load().listening.history
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

/// A play heard somewhere else (a phone, through the phone server): counted like one here.
pub fn record_play(root: &str, file: &str, heard_ms: u64) {
    let dir = file.rsplit_once('/').map_or("", |x| x.0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    if let Ok(c) = open() {
        let _ = c.execute(
            "INSERT INTO plays (root, file, dir, at, heard_ms, ctx) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![key(root), file, dir, now, heard_ms as i64],
        );
    }
}

/// How long an online song is heard before its play is told to the YouTube Music account (as
/// YouTube Music's own app does); a shorter song, nearly all of it. A skip is never told as a
/// play (How much I listened tells it as the seconds heard).
pub const REPORT_AFTER_MS: u64 = 30_000;

/// Whether this much heard of a song this long is a play to tell the account.
pub fn worth_reporting(heard_ms: u64, duration_ms: u64) -> bool {
    let needed = if duration_ms > 0 {
        REPORT_AFTER_MS.min(duration_ms * 9 / 10)
    } else {
        REPORT_AFTER_MS
    };
    needed > 0 && heard_ms >= needed
}

/// How much of an online song was heard is first told after this much of it (How much I
/// listened), as YouTube's own player first tells it; a song skipped sooner is never told.
pub const FIRST_HEARD_AFTER_MS: u64 = 10_000;
/// Then at most this often while it plays (YouTube's own player tells it more often), and once
/// more when the song ends, is skipped or replaced, or the music stops.
pub const HEARD_EVERY_MS: u64 = 40_000;

/// What the tracker has to tell the account about an online song. The reporter decides itself
/// whether anything is sent (the account's switches, Local only).
#[derive(Debug, Clone, PartialEq)]
pub enum Told {
    /// Heard long enough to be a play (Count plays).
    Play { video_id: String, play: u64 },
    /// How much was heard since the last report (How much I listened).
    Heard(Listened),
}

/// How much of one play of an online song was heard since its last report.
#[derive(Debug, Clone, PartialEq)]
pub struct Listened {
    pub video_id: String,
    /// Which play this is: a new one each time a song starts.
    pub play: u64,
    /// The stretches heard, each from where it started to where it stopped (a seek or a pause
    /// ends one), in milliseconds of the song.
    pub segments: Vec<(u64, u64)>,
    /// Where the song is now.
    pub at_ms: u64,
    /// How long the song is (0: not known).
    pub length_ms: u64,
    pub playing: bool,
    /// The play's last report: it ended, was skipped or replaced, or the music stopped.
    pub last: bool,
}

/// Tells the account what was played and heard of an online song; decides itself whether to.
pub type Reporter = Box<dyn Fn(Told) + Send>;

/// Counts plays as they happen, from the player's status.
#[derive(Default)]
pub struct Tracker {
    conn: Option<Connection>,
    song: Option<String>,
    heard_ms: u64,
    last_pos: u64,
    counted: bool,
    /// This play of the song was told to the account already.
    reported: bool,
    /// Which play this is (see `Listened::play`).
    play: u64,
    /// Where the stretch being heard now started, while one is.
    stretch: Option<u64>,
    /// Stretches heard and ended since the last report.
    stretches: Vec<(u64, u64)>,
    /// Heard since the last report.
    unreported_ms: u64,
    /// Reports of how much was heard made for this play.
    heard_reports: u32,
    playing: bool,
    length_ms: u64,
    /// Tells the account (Count plays, How much I listened).
    pub reporter: Option<Reporter>,
}

impl Tracker {
    /// One status: `song` is the playing file (library-relative) with its album folder, or an
    /// online song's `online:<provider>:<id>` with no folder and `online` saying what it is.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        root: &str,
        song: Option<(&str, &str)>,
        online: Option<&OnlineSong>,
        playing: bool,
        position_ms: u64,
        duration_ms: u64,
        from: &str,
    ) {
        let id = song.map(|(f, _)| f.to_owned());
        if id != self.song {
            // The song before ends here: what was heard of it since its last report.
            self.tell_heard(true);
            self.song = id;
            self.heard_ms = 0;
            self.counted = false;
            self.reported = false;
            self.play += 1;
            self.stretch = None;
            self.stretches.clear();
            self.unreported_ms = 0;
            self.heard_reports = 0;
            self.playing = playing;
            self.length_ms = duration_ms;
            self.last_pos = position_ms;
            return;
        }
        if duration_ms > 0 {
            self.length_ms = duration_ms;
        }
        let step = position_ms.saturating_sub(self.last_pos);
        // Real listening moves a little at a time; a jump is a seek, and it, a step back or a
        // pause ends the stretch being heard.
        if playing && step > 0 && step < 2_000 {
            self.heard_ms += step;
            self.unreported_ms += step;
            self.stretch.get_or_insert(self.last_pos);
        } else if !playing || step >= 2_000 || position_ms < self.last_pos {
            self.end_stretch();
        }
        self.last_pos = position_ms;
        self.playing = playing;
        self.tell_heard(false);
        // An online song heard long enough: told to the YouTube Music account, once a play.
        if !self.reported
            && worth_reporting(self.heard_ms, duration_ms)
            && let Some((file, _)) = song
            && let Some(id) = file.strip_prefix("online:remote_catalog:")
        {
            self.reported = true;
            if let Some(r) = &self.reporter {
                r(Told::Play {
                    video_id: id.to_owned(),
                    play: self.play,
                });
            }
        }
        let enough = (duration_ms / 2).clamp(15_000, 240_000);
        if !self.counted
            && self.heard_ms >= enough
            && let Some((file, dir)) = song
        {
            self.counted = true;
            if !history_on() {
                return;
            }
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
                let o = online.filter(|o| o.file == file);
                let _ = c.execute(
                    "INSERT INTO plays (root, file, dir, at, heard_ms, ctx, title, artists, album, art)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        key(root),
                        file,
                        dir,
                        now,
                        self.heard_ms as i64,
                        ctx,
                        o.map(|o| o.title.clone()),
                        o.and_then(|o| serde_json::to_string(&o.artists).ok()),
                        o.map(|o| o.album.clone()).filter(|a| !a.is_empty()),
                        o.and_then(|o| o.art.clone()),
                    ],
                );
            }
        }
    }

    /// The stretch being heard ends where the song was last seen.
    fn end_stretch(&mut self) {
        if let Some(start) = self.stretch.take()
            && self.last_pos > start
        {
            self.stretches.push((start, self.last_pos));
        }
    }

    /// Tell how much of the online song playing was heard, when a report is due: the first after
    /// ten seconds of it, then every forty; or (`last`) whatever is left untold when it ends,
    /// once ten seconds of it have been heard.
    fn tell_heard(&mut self, last: bool) {
        let Some(video_id) = self
            .song
            .as_deref()
            .and_then(|s| s.strip_prefix("online:remote_catalog:"))
        else {
            return;
        };
        let due = if last {
            self.unreported_ms > 0 && self.heard_ms >= FIRST_HEARD_AFTER_MS
        } else if self.heard_reports == 0 {
            self.unreported_ms >= FIRST_HEARD_AFTER_MS
        } else {
            self.unreported_ms >= HEARD_EVERY_MS
        };
        if !due {
            return;
        }
        let video_id = video_id.to_owned();
        let mut segments = std::mem::take(&mut self.stretches);
        if let Some(start) = self.stretch.take() {
            if self.last_pos > start {
                segments.push((start, self.last_pos));
            }
            // Still being heard: the next report's stretch goes on from here.
            if !last {
                self.stretch = Some(self.last_pos);
            }
        }
        self.unreported_ms = 0;
        self.heard_reports += 1;
        if let Some(r) = &self.reporter {
            r(Told::Heard(Listened {
                video_id,
                play: self.play,
                segments,
                at_ms: self.last_pos,
                length_ms: self.length_ms,
                playing: self.playing,
                last,
            }));
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
            .prepare("SELECT dir, MAX(at) FROM plays WHERE root = ?1 AND dir <> '' GROUP BY dir ORDER BY MAX(at) DESC")
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
            .prepare("SELECT dir, COUNT(*) AS n FROM plays WHERE root = ?1 AND dir <> '' GROUP BY dir ORDER BY n DESC LIMIT 40")
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
            .prepare("SELECT file, COUNT(*) FROM plays WHERE root = ?1 AND file NOT LIKE 'online:%' GROUP BY file")
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

/// One play, for Stats and History: when, how long it was heard, and which song (a library
/// path, or an online song with what it was).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Play {
    pub at: i64,
    pub heard_ms: i64,
    pub file: String,
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub art: Option<String>,
    /// Where it was played from (a playlist), when kept.
    pub ctx: Option<String>,
}

fn plays_since(c: &Connection, root: &str, since: i64, limit: i64) -> Result<Vec<Play>, String> {
    let mut stmt = c
        .prepare(
            "SELECT at, heard_ms, file, title, artists, album, art, ctx FROM plays
             WHERE root = ?1 AND at >= ?2 ORDER BY at DESC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![key(root), since, limit], |r| {
            let artists: Option<String> = r.get(4)?;
            Ok(Play {
                at: r.get(0)?,
                heard_ms: r.get(1)?,
                file: r.get(2)?,
                title: r.get(3)?,
                artists: artists
                    .and_then(|a| serde_json::from_str(&a).ok())
                    .unwrap_or_default(),
                album: r.get(5)?,
                art: r.get(6)?,
                ctx: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// Your plays since `since` (ms), newest first, at most `limit`: what Stats counts and History
/// lists, your files and online songs alike.
#[tauri::command]
pub async fn listening_plays(
    root: String,
    since: i64,
    limit: Option<i64>,
) -> Result<Vec<Play>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = open()?;
        plays_since(&c, &root, since, limit.unwrap_or(50_000).clamp(1, 200_000))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Forget plays: all of them, or those since `since` (ms). Returns how many.
#[tauri::command]
pub async fn listening_forget(root: String, since: Option<i64>) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = open()?;
        c.execute(
            "DELETE FROM plays WHERE root = ?1 AND at >= ?2",
            params![key(&root), since.unwrap_or(0)],
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::worth_reporting;
    use super::{Listened, Told, Tracker};
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_play_is_told_after_thirty_seconds_and_a_short_song_near_its_end() {
        assert!(!worth_reporting(29_000, 240_000), "a skip is never told");
        assert!(worth_reporting(30_000, 240_000));
        // A 20-second song: most of it.
        assert!(!worth_reporting(15_000, 20_000));
        assert!(worth_reporting(18_000, 20_000));
        // No length known: thirty seconds.
        assert!(worth_reporting(30_000, 0));
    }

    /// A tracker that keeps what it told.
    fn telling() -> (Tracker, Arc<Mutex<Vec<Told>>>) {
        let told = Arc::new(Mutex::new(Vec::<Told>::new()));
        let seen = told.clone();
        let t = Tracker {
            reporter: Some(Box::new(move |t: Told| seen.lock().unwrap().push(t))),
            ..Default::default()
        };
        (t, told)
    }

    fn plays(told: &Arc<Mutex<Vec<Told>>>) -> Vec<String> {
        told.lock()
            .unwrap()
            .iter()
            .filter_map(|t| match t {
                Told::Play { video_id, .. } => Some(video_id.clone()),
                Told::Heard(_) => None,
            })
            .collect()
    }

    fn heard(told: &Arc<Mutex<Vec<Told>>>) -> Vec<Listened> {
        told.lock()
            .unwrap()
            .iter()
            .filter_map(|t| match t {
                Told::Heard(l) => Some(l.clone()),
                Told::Play { .. } => None,
            })
            .collect()
    }

    const ONLINE: Option<(&str, &str)> = Some(("online:remote_catalog:vid1", ""));
    const NEXT: Option<(&str, &str)> = Some(("online:remote_catalog:vid2", ""));

    /// Play `song` a second at a time from `from` to `to` seconds (a 200-second song).
    fn listen(t: &mut Tracker, song: Option<(&str, &str)>, from: u64, to: u64) {
        for s in from..=to {
            t.update("", song, None, true, s * 1_000, 200_000, "");
        }
    }

    #[test]
    fn an_online_song_is_told_once_a_play_and_a_file_never() {
        let (mut t, told) = telling();
        // The song starts, then plays a second at a time for 40 seconds.
        listen(&mut t, ONLINE, 0, 40);
        assert_eq!(plays(&told), ["vid1"], "told once, not every second after");
        // A file of yours: never told, not as a play and not how much of it was heard.
        let before = told.lock().unwrap().len();
        let file = Some(("Album/01.flac", "Album"));
        listen(&mut t, file, 0, 200);
        t.update("", None, None, false, 0, 0, "");
        assert_eq!(plays(&told), ["vid1"]);
        let after: Vec<Told> = told.lock().unwrap()[before..].to_vec();
        assert!(
            after
                .iter()
                .all(|x| matches!(x, Told::Heard(l) if l.video_id == "vid1" && l.last)),
            "only the online song's own last report: {after:?}"
        );
    }

    // How much was heard goes as YouTube's player tells it: first after ten seconds, then every
    // forty, and a last report when the song ends or another replaces it; each says the stretch
    // heard since the one before. The play itself still waits for thirty seconds.
    #[test]
    fn listening_time_is_told_at_its_pace_and_when_the_song_ends() {
        let (mut t, told) = telling();
        listen(&mut t, ONLINE, 0, 200);
        t.update("", NEXT, None, true, 0, 200_000, "");
        let h = heard(&told);
        let stretches: Vec<_> = h.iter().map(|l| (l.segments.clone(), l.last)).collect();
        assert_eq!(
            stretches,
            [
                (vec![(0, 10_000)], false),
                (vec![(10_000, 50_000)], false),
                (vec![(50_000, 90_000)], false),
                (vec![(90_000, 130_000)], false),
                (vec![(130_000, 170_000)], false),
                (vec![(170_000, 200_000)], true),
            ]
        );
        assert!(
            h.iter()
                .all(|l| l.video_id == "vid1" && l.length_ms == 200_000)
        );
        assert!(h.iter().all(|l| l.play == h[0].play), "one listen");
        assert_eq!(h[1].at_ms, 50_000);
        assert_eq!(plays(&told), ["vid1"]);
        // The play is told at thirty seconds: after the first report, before the second.
        let order: Vec<bool> = told
            .lock()
            .unwrap()
            .iter()
            .map(|x| matches!(x, Told::Play { .. }))
            .collect();
        assert_eq!(&order[..3], [false, true, false]);
    }

    // A skip after ten seconds says ten seconds (and no more); one sooner says nothing.
    #[test]
    fn a_skip_is_told_as_the_seconds_heard() {
        let (mut t, told) = telling();
        listen(&mut t, ONLINE, 0, 10);
        t.update("", NEXT, None, true, 0, 200_000, "");
        let h = heard(&told);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].segments, [(0, 10_000)]);
        assert!(plays(&told).is_empty(), "not a play");

        let (mut t, told) = telling();
        listen(&mut t, ONLINE, 0, 13);
        t.update("", NEXT, None, true, 0, 200_000, "");
        let h = heard(&told);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].segments, [(10_000, 13_000)]);
        assert!(h[1].last && h[1].playing);

        let (mut t, told) = telling();
        listen(&mut t, ONLINE, 0, 9);
        t.update("", NEXT, None, true, 0, 200_000, "");
        listen(&mut t, NEXT, 0, 5);
        assert!(
            told.lock().unwrap().is_empty(),
            "a skip in the first ten seconds"
        );
    }

    // A seek starts a new stretch; a pause ends one, and stopping then says it was paused.
    #[test]
    fn seeks_and_pauses_are_their_own_stretches() {
        let (mut t, told) = telling();
        listen(&mut t, ONLINE, 0, 6);
        listen(&mut t, ONLINE, 100, 104);
        let h = heard(&told);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].segments, [(0, 6_000), (100_000, 104_000)]);
        assert_eq!(h[0].at_ms, 104_000);

        listen(&mut t, ONLINE, 105, 108);
        t.update("", ONLINE, None, false, 108_000, 200_000, "");
        t.update("", None, None, false, 0, 0, "");
        let h = heard(&told);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].segments, [(104_000, 108_000)]);
        assert!(h[1].last && !h[1].playing);
    }

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
        t.update("R", song, None, true, 0, 180_000, FROM);
        t.update("R", song, None, true, 100_000, 180_000, FROM); // a seek: not heard
        assert_eq!(t.heard_ms, 0);
        for p in (100_000..=190_000).step_by(1_000) {
            t.update("R", song, None, true, p, 180_000, FROM);
        }
        assert!(t.counted, "heard {} ms", t.heard_ms);
        t.update("R", Some(("b.flac", "A")), None, true, 0, 180_000, FROM);
        assert!(!t.counted, "a new song starts afresh");
        assert_eq!(super::playlist_of(FROM).as_deref(), Some("Hindi"));
        assert_eq!(super::playlist_of("Taal"), None, "plain words from before");

        // An online song counts too, with what it was; the library's summaries stay your files.
        let online = super::OnlineSong {
            file: "online:remote_catalog:t-1".into(),
            title: "Zingaat".into(),
            artists: vec!["Ajay-Atul".into()],
            album: "Sairat".into(),
            art: None,
        };
        let o = Some((online.file.as_str(), ""));
        t.update("R", o, Some(&online), true, 0, 120_000, "");
        for p in (0..=70_000).step_by(1_000) {
            t.update("R", o, Some(&online), true, p, 120_000, "");
        }
        assert!(t.counted);
        let c = super::open().unwrap();
        let plays = super::plays_since(&c, "R", 0, 100).unwrap();
        assert_eq!(plays[0].file, "online:remote_catalog:t-1");
        assert_eq!(plays[0].title.as_deref(), Some("Zingaat"));
        assert_eq!(plays[0].artists, ["Ajay-Atul"]);
        assert_eq!(plays[1].file, "a.flac");
        assert_eq!(
            plays[1].title, None,
            "a library file says what it is itself"
        );
    }
}
