//! Lyrics from the lyrics services for a song with none of its own (an online song, or a file
//! without an `.lrc` or lyrics tag): Automatic, or the source you chose for that song, with
//! your timing offset and the name you gave a wrongly named song (`engine::lyrics_online`).
//! YouTube Music's own lyrics come through the catalog client.

use crate::playback::Playback;
use engine::lyrics_online::{self as lo, Choice, Found, LyricsRequest, SourceStatus};
use media_access::{CancellationToken, MediaAccess};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

/// The song lyrics are asked for: its key (`online:<provider>:<id>`, or the file's path) and
/// what it is.
#[derive(Debug, Deserialize)]
pub struct LyricsAsk {
    pub key: String,
    #[serde(flatten)]
    pub song: LyricsRequest,
}

#[derive(Debug, Serialize)]
pub struct LyricsAnswer {
    pub found: Option<Found>,
    /// The source's name ("LRCLIB"), when found.
    pub label: Option<String>,
    /// What you chose for this song (source, offset, the name to ask by).
    pub choice: Choice,
}

fn catalog_lyrics(
    access: &Arc<dyn MediaAccess>,
    req: &LyricsRequest,
) -> Result<Option<Found>, String> {
    let Some(id) = req.video_id.as_deref().filter(|i| !i.is_empty()) else {
        return Ok(None);
    };
    let found = access
        .lyrics(id, &CancellationToken::default())
        .map_err(|e| e.to_string())?;
    Ok(found.map(|l| {
        let mut lines: Vec<lo::LyricLine> = l
            .lines
            .into_iter()
            .map(|(start_ms, text)| lo::LyricLine {
                start_ms,
                text,
                ..Default::default()
            })
            .collect();
        engine::lyrics::finish(&mut lines);
        Found {
            source: "youtube".into(),
            lyrics: lo::Lyrics {
                lines,
                ..Default::default()
            },
            text: l.text,
            credit: l.credit,
            ..Default::default()
        }
    }))
}

fn sources_on() -> Vec<&'static str> {
    lo::order(&engine::settings::load().listening.lyrics_sources)
        .into_iter()
        .filter_map(|(id, on)| on.then_some(id))
        .collect()
}

/// A song's lyrics: from the source you chose for it, else Automatic (the first timed answer
/// in your order). Kept answers are used; `fresh` asks again.
#[tauri::command]
pub async fn lyrics_find(
    ask: LyricsAsk,
    source: Option<String>,
    fresh: Option<bool>,
    pb: State<'_, Playback>,
    app: tauri::AppHandle,
) -> Result<LyricsAnswer, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        let choice = lo::choices().remove(&ask.key).unwrap_or_default();
        let req = choice.applied(&ask.song);
        let from = source.or_else(|| choice.source.clone());
        let kept = lo::Kept::open().ok();
        let slot = from.clone().unwrap_or_else(|| "auto".into());
        let cached = (!fresh.unwrap_or(false))
            .then(|| kept.as_ref().and_then(|k| k.get(&ask.key, &slot)))
            .flatten();
        let catalog = |r: &LyricsRequest| catalog_lyrics(&access, r);
        let found = match cached {
            Some(f) => f,
            None => {
                // Found before in an older answer format: asked for again, kept to fall back on.
                let stale = kept.as_ref().and_then(|k| k.stale(&ask.key, &slot));
                let answer = match from.as_deref() {
                    Some(s) => lo::ask(s, &req, &catalog),
                    None => lo::automatic(&req, &sources_on(), &catalog),
                };
                // A source that turned Venuta away or didn't answer isn't kept as "none".
                match &kept {
                    Some(k) => lo::renewed(k, &ask.key, &slot, answer, stale),
                    None => answer.ok().flatten().or(stale),
                }
            }
        };
        if let Some(f) = &found {
            crate::japanese_dictionary::fetch_if_waiting(
                &app,
                &f.lyrics.lines,
                f.lyrics.lang.as_deref(),
            );
        }
        Ok(LyricsAnswer {
            label: found.as_ref().map(|f| lo::label(&f.source).to_owned()),
            found,
            choice,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What every source has for a song (for the picker), in your order, switched-off ones too.
#[tauri::command]
pub async fn lyrics_check(
    ask: LyricsAsk,
    pb: State<'_, Playback>,
) -> Result<Vec<SourceStatus>, String> {
    let access = pb.catalog();
    tauri::async_runtime::spawn_blocking(move || {
        let choice = lo::choices().remove(&ask.key).unwrap_or_default();
        let req = choice.applied(&ask.song);
        let order = lo::order(&engine::settings::load().listening.lyrics_sources);
        let catalog = |r: &LyricsRequest| catalog_lyrics(&access, r);
        let all = lo::check_all(&req, &order, &catalog);
        // What each said is kept, so choosing one shows it at once.
        if let Ok(k) = lo::Kept::open() {
            for (status, found) in &all {
                if status.status != "failed" {
                    k.put(&ask.key, &status.source, found.as_ref());
                }
            }
        }
        Ok(all.into_iter().map(|(s, _)| s).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Choose a song's lyrics source (None: Automatic again), and the name to ask by.
#[tauri::command]
pub fn lyrics_choose(
    key: String,
    source: Option<String>,
    title: Option<String>,
    artists: Option<Vec<String>>,
    rename: Option<bool>,
) -> Result<Choice, String> {
    let renamed = rename.unwrap_or(false);
    let c = lo::set_choice(&key, |c| {
        c.source = source;
        if renamed {
            c.title = title.filter(|t| !t.trim().is_empty());
            c.artists = artists.filter(|a| !a.is_empty());
        }
    })?;
    // A new name is a new question: what was found under the old one goes.
    if renamed && let Ok(k) = lo::Kept::open() {
        k.forget(&key);
    }
    Ok(c)
}

/// Forget every lyrics answer kept (they are asked again as songs play). Returns how many.
#[tauri::command]
pub fn lyrics_forget_all() -> Result<usize, String> {
    Ok(lo::Kept::open()?.clear())
}

/// How much earlier or later a song's lyrics run (positive: later), in ms.
#[tauri::command]
pub fn lyrics_offset(key: String, offset_ms: i64) -> Result<Choice, String> {
    lo::set_choice(&key, |c| c.offset_ms = offset_ms.clamp(-15_000, 15_000))
}

/// How each lyrics source has answered since Venuta started (Settings > Lyrics): answering,
/// turned Venuta away (not asked again until it restarts), not answering, or not asked yet.
#[tauri::command]
pub fn lyrics_sources_health() -> Vec<lo::SourceHealth> {
    lo::health()
}
