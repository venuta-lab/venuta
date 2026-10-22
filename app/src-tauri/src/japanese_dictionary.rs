//! The Japanese dictionary (kanji readings for the pronunciation line), fetched in the
//! background the first time a song's lyrics wait for it, never bundled. Progress goes out as
//! `venuta://japanese-dictionary` events ({state, done, total, error}); once it is "ready",
//! reading the lyrics again gives their Japanese lines a pronunciation. Showing it is the
//! lyrics views' part.

use std::sync::atomic::{AtomicBool, Ordering};

use engine::lyrics::LyricLine;
use lyrics_text::japanese::dictionary::Progress;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const EVENT: &str = "venuta://japanese-dictionary";

/// One fetch at a time; cleared when it ends, so a failed one is tried again with the next
/// song that waits.
static FETCHING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone)]
struct Status {
    /// "downloading", "checking", "unpacking", "ready" or "failed".
    state: &'static str,
    done: u64,
    total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Starts fetching the dictionary when some of `lines` wait for it and it isn't on the way.
pub fn fetch_if_waiting(app: &AppHandle, lines: &[LyricLine], song_lang: Option<&str>) {
    if engine::net::is_offline() || !engine::lyrics::waits_for_japanese_dictionary(lines, song_lang)
    {
        return;
    }
    if FETCHING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let send = |s: Status| {
            let _ = app.emit(EVENT, s);
        };
        let mut shown = u64::MAX;
        let result = engine::lyrics::install_japanese_dictionary(&mut |p| {
            let (state, done, total) = match p {
                Progress::Downloading { done, total } => ("downloading", done, total),
                Progress::Checking => ("checking", 0, 0),
                Progress::Unpacking => ("unpacking", 0, 0),
            };
            // One event per percent at most.
            let pct = (done * 100).checked_div(total).unwrap_or(u64::MAX - 1);
            if state != "downloading" || pct != shown {
                shown = pct;
                send(Status {
                    state,
                    done,
                    total,
                    error: None,
                });
            }
            true
        });
        send(match result {
            Ok(_) => Status {
                state: "ready",
                done: 0,
                total: 0,
                error: None,
            },
            Err(e) => Status {
                state: "failed",
                done: 0,
                total: 0,
                error: Some(e),
            },
        });
        FETCHING.store(false, Ordering::Release);
    });
}
