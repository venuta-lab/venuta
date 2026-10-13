//! Desktop shell. Commands are thin wrappers that run engine work off the UI thread and
//! return serialisable results; all logic lives in the `engine` crate.

use engine::apply::{ApplyProgress, ApplyReport};
use engine::art::CoverThumb;
use engine::audit::HealthReport;
use engine::library::Library;
use engine::plan::{Plan, PlanView};
use engine::quality::{DeepCheckReport, Progress};
use engine::rules::Rules;
use engine::settings::{Decision, Settings};
use engine::{Index, ScanReport, paths};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

mod listening;
mod playback;

/// The running deep check, if any, so it can be cancelled.
#[derive(Default)]
struct Jobs {
    deep_check: Mutex<Option<Arc<Progress>>>,
    apply: Mutex<Option<Arc<ApplyProgress>>>,
    /// The last plan built, kept here because it is too large to send to the UI.
    plan: Mutex<Option<(String, Plan)>>,
    /// The last incoming plan, same reason.
    incoming_plan: Mutex<Option<(String, Plan)>>,
    /// The last tidy preview's plan, so Apply does exactly what was shown.
    tidy_plan: Mutex<Option<(String, Plan)>>,
    lookup: Mutex<Option<Arc<engine::enrich::EnrichProgress>>>,
    /// Running card syncs, by card root.
    syncs: Mutex<BTreeMap<String, Arc<engine::sync::SyncProgress>>>,
}

#[derive(Clone, Serialize)]
struct SyncProgressMsg {
    done: usize,
    total: usize,
    bytes: u64,
}

fn rules() -> CmdResult<Rules> {
    Rules::load(&PathBuf::from(
        settings_with_defaults().rules_dir.unwrap_or_default(),
    ))
}

/// Run a lookup job with progress and cancellation (library or incoming).
async fn run_lookup(
    jobs: &Jobs,
    on_progress: Channel<DeepProgress>,
    job: impl FnOnce(
        &engine::enrich::EnrichProgress,
        &(dyn Fn(usize, usize) + Sync),
    ) -> CmdResult<engine::enrich::EnrichReport>
    + Send
    + 'static,
) -> CmdResult<engine::enrich::EnrichReport> {
    let progress = Arc::new(engine::enrich::EnrichProgress::default());
    {
        let mut slot = jobs.lookup.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("A lookup is already running.".into());
        }
        *slot = Some(progress.clone());
    }
    let p = progress.clone();
    let result = blocking(move || {
        let notes = p.clone();
        job(&p, &|done, total| {
            let note = notes.note.lock().map(|n| n.clone()).unwrap_or_default();
            let _ = on_progress.send(DeepProgress { done, total, note });
        })
    })
    .await;
    if let Ok(mut slot) = jobs.lookup.lock() {
        *slot = None;
    }
    result
}

#[derive(Serialize)]
struct Review {
    plan: PlanView,
    /// Albums you left out of the build (and old approvals), by album id.
    decisions: BTreeMap<String, Decision>,
    /// Your answers to decisions, by decision id.
    answers: engine::review::Answers,
    settings: Settings,
}

#[derive(Serialize)]
struct Run {
    /// "build": a library built from a music folder; "tidy": a library tidied in place.
    kind: &'static str,
    journal: String,
    started_ms: u64,
    target: String,
    files: usize,
    /// Files that could not be built, and why (recorded since builds keep their problems).
    failed: Vec<RunFailure>,
    /// The run reached its end; `None` for runs from before this was recorded.
    finished: Option<bool>,
}

#[derive(Serialize)]
struct RunFailure {
    file: String,
    message: String,
}

fn settings_with_defaults() -> Settings {
    let mut s = engine::settings::load();
    // Builds go into your library's Music folder; an older saved target is only a suggestion
    // until a library is chosen.
    if let Some(home) = paths::home() {
        s.new_library = Some(paths::music_dir(&home).to_string_lossy().replace('\\', "/"));
    }
    if s.rules_dir.is_none() {
        s.rules_dir = Some(
            paths::app_data_dir()
                .join("rules")
                .to_string_lossy()
                .into_owned(),
        );
    }
    s
}

/// The plan for a library with your answers applied, comparing likely duplicates by decoding
/// them first when needed.
fn build_plan(root: &Path) -> CmdResult<Plan> {
    let index = open_index(root)?;
    let rules_dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
    let rules = Rules::load(&rules_dir)?;
    engine::plan::build_checked(&index, root, &rules)
        .map(|(_, plan)| plan)
        .map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
struct DeepProgress {
    done: usize,
    total: usize,
    /// What is happening, in words (online lookups only).
    #[serde(skip_serializing_if = "String::is_empty")]
    note: String,
}

/// Errors cross the IPC boundary as plain messages.
type CmdResult<T> = Result<T, String>;

fn open_index(root: &Path) -> CmdResult<Index> {
    Index::open(
        &paths::index_path_for(root),
        &paths::canonical_root_string(root),
    )
    .map_err(|e| e.to_string())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn engine_version() -> &'static str {
    engine::VERSION
}

/// Scan a library folder into its index. Read-only on the library. Progress streams over
/// `on_progress`.
#[tauri::command]
async fn scan_library(
    root: String,
    on_progress: Channel<engine::ScanProgress>,
) -> CmdResult<ScanReport> {
    blocking(move || {
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        engine::scan_with(&mut index, &root, &|p| {
            let _ = on_progress.send(p);
        })
        .map_err(|e| e.to_string())
    })
    .await
}

/// Health report from the current index (scan first for fresh results).
#[tauri::command]
async fn library_health(root: String) -> CmdResult<HealthReport> {
    blocking(move || {
        let index = open_index(Path::new(&root))?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        Ok(engine::audit::run(&lib))
    })
    .await
}

/// Cover thumbnails (cached JPEG paths + accent colour) for the given album folders.
#[tauri::command]
async fn album_covers(root: String, dirs: Vec<String>, size: u32) -> CmdResult<Vec<CoverThumb>> {
    blocking(move || {
        let root = PathBuf::from(root);
        let index = open_index(&root)?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        let requests = lib.cover_requests(&dirs);
        Ok(engine::art::thumbnails(
            &root,
            requests,
            size.clamp(64, 1024),
        ))
    })
    .await
}

/// Decode new or changed tracks (integrity, quality, loudness). Progress streams over `on_progress`.
#[tauri::command]
async fn deep_check(
    root: String,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<DeepCheckReport> {
    let progress = Arc::new(Progress::default());
    {
        let mut slot = jobs.deep_check.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("A deep check is already running.".into());
        }
        *slot = Some(progress.clone());
    }
    let p = progress.clone();
    let result = blocking(move || {
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())?;
        engine::quality::run(&mut index, &root, &p, &|done, total| {
            let _ = on_progress.send(DeepProgress {
                done,
                total,
                note: String::new(),
            });
        })
        .map_err(|e| e.to_string())
    })
    .await;
    if let Ok(mut slot) = jobs.deep_check.lock() {
        *slot = None;
    }
    result
}

#[tauri::command]
fn cancel_deep_check(jobs: State<'_, Jobs>) {
    if let Ok(slot) = jobs.deep_check.lock()
        && let Some(p) = slot.as_ref()
    {
        p.cancel.store(true, Ordering::Relaxed);
    }
}

/// Spectrogram of an album's first track (cached PNG path).
#[tauri::command]
async fn album_spectrogram(root: String, dir: String) -> CmdResult<String> {
    blocking(move || {
        let root = PathBuf::from(root);
        let index = open_index(&root)?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        let track = lib
            .tracks
            .iter()
            .filter(|t| t.dir == dir && t.kind == "flac")
            .min_by(|a, b| a.path.cmp(&b.path))
            .ok_or("No FLAC track in this album.")?;
        let key = match &track.audio_md5 {
            Some(m) => m.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            None => blake3::hash(format!("{}:{}", track.path, track.size).as_bytes())
                .to_hex()
                .to_string(),
        };
        engine::spectrogram::render_cached(&root.join(&track.path), &key)
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
fn get_settings() -> Settings {
    settings_with_defaults()
}

/// Whether a folder can hold the new library: empty, or one this app built.
#[tauri::command]
fn check_new_library(root: String, target: String) -> CmdResult<()> {
    engine::apply::check_target(Path::new(&root), Path::new(&target))
}

#[tauri::command]
fn set_settings(settings: Settings) -> CmdResult<()> {
    engine::settings::save(&settings)
}

/// Remember which folder is open (your library, or music being brought in).
#[tauri::command]
fn set_open_folder(path: String) -> CmdResult<()> {
    let mut s = engine::settings::load();
    s.open_folder = Some(path);
    engine::settings::save(&s)
}

/// Remember the look and feel with the library.
#[tauri::command]
fn save_appearance(appearance: serde_json::Value) -> CmdResult<()> {
    let mut s = engine::settings::load();
    s.appearance = Some(appearance);
    engine::settings::save(&s)
}

#[derive(Serialize)]
struct HomeInfo {
    /// Your library, once chosen.
    library: Option<String>,
    /// Where Venuta keeps its data (`<library>/.venuta`, or the per-user folder before).
    data_dir: String,
    /// A good place for a new library on this computer.
    suggestion: String,
}

/// Where your library is, and where a new one could go.
#[tauri::command]
fn library_home() -> HomeInfo {
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let saved = engine::settings::load().new_library;
    HomeInfo {
        library: paths::home().map(|h| slash(&h)),
        data_dir: slash(&paths::app_data_dir()),
        suggestion: saved.unwrap_or_else(suggest_home),
    }
}

/// `X:/Music Library` on the first drive after C: that exists, else in your Music folder.
fn suggest_home() -> String {
    if cfg!(windows) {
        for letter in 'D'..='Z' {
            if Path::new(&format!("{letter}:\\")).is_dir() {
                return format!("{letter}:/Music Library");
            }
        }
    }
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| {
            PathBuf::from(h)
                .join("Music")
                .join("Music Library")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .unwrap_or_else(|| "Music Library".into())
}

/// Whether a folder can be your library: new or empty, or already one.
#[tauri::command]
fn check_home(path: String) -> CmdResult<paths::HomeKind> {
    paths::check_home(Path::new(&path))
}

/// A music folder to bring in must be outside your library (and your library outside it):
/// the build copies one into the other. Opening the library itself is fine.
#[tauri::command]
fn check_music_folder(path: String, library: Option<String>) -> CmdResult<()> {
    let Some(home) = library.map(PathBuf::from).or_else(paths::home) else {
        return Ok(());
    };
    let music = Path::new(&path);
    let same = |a: &Path, b: &Path| {
        paths::canonical_root_string(a).eq_ignore_ascii_case(&paths::canonical_root_string(b))
    };
    // The library itself, or its Music folder, is simply opened.
    if same(music, &home) || same(music, &paths::music_dir(&home)) {
        return Ok(());
    }
    engine::apply::check_target(music, &home).map_err(|_| {
        format!(
            "{} and your library ({}) can't be inside one another. Keep them as separate folders.",
            music.display(),
            home.display()
        )
    })
}

/// Make a folder your library and move Venuta's data into it.
#[tauri::command]
async fn choose_home(app: AppHandle, path: String) -> CmdResult<paths::HomeKind> {
    let kind = blocking(move || paths::set_home(Path::new(&path))).await?;
    allow_data_assets(&app);
    Ok(kind)
}

/// Covers and spectrograms are files in the data folder; let the window show them.
fn allow_data_assets(app: &AppHandle) {
    let data = paths::app_data_dir();
    for sub in ["thumbs", "spectrograms"] {
        let dir = data.join(sub);
        let _ = std::fs::create_dir_all(&dir);
        let _ = app.asset_protocol_scope().allow_directory(&dir, true);
    }
}

/// Build the plan for a library and return what the Review screen shows.
#[tauri::command]
async fn review(app: AppHandle, root: String, jobs: State<'_, Jobs>) -> CmdResult<Review> {
    // Clips on question cards play your own files, so the music folder may be read.
    let _ = app.asset_protocol_scope().allow_directory(&root, true);
    let r = root.clone();
    let plan = blocking(move || build_plan(Path::new(&r))).await?;
    let view = plan.view();
    *jobs.plan.lock().map_err(|e| e.to_string())? = Some((root.clone(), plan));
    Ok(Review {
        plan: view,
        decisions: engine::settings::load_decisions(Path::new(&root)),
        answers: engine::settings::load_answers(Path::new(&root)),
        settings: settings_with_defaults(),
    })
}

#[derive(serde::Deserialize)]
struct AnswerItem {
    id: String,
    choice: String,
    /// The two names, for "same artist?" (answers to it are saved in your aliases).
    #[serde(default)]
    a: Option<String>,
    #[serde(default)]
    b: Option<String>,
}

/// Answer one or more decisions (or approve handled ones), then plan again.
#[tauri::command]
async fn answer_decisions(
    app: AppHandle,
    root: String,
    items: Vec<AnswerItem>,
    jobs: State<'_, Jobs>,
) -> CmdResult<Review> {
    let rootp = PathBuf::from(&root);
    let rules_dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
    let mut answers = engine::settings::load_answers(&rootp);
    for it in items {
        if it.id.starts_with("same:") {
            if let (Some(a), Some(b)) = (&it.a, &it.b) {
                let keep = it.choice.strip_prefix("same:");
                engine::rules::answer_same_artist(&rules_dir, a, b, keep)?;
            }
        } else {
            answers.insert(it.id, it.choice);
        }
    }
    engine::settings::save_answers(&rootp, &answers)?;
    review(app, root, jobs).await
}

/// Your answers to Organize decisions for this library, by decision id.
#[tauri::command]
fn list_answers(root: String) -> engine::review::Answers {
    engine::settings::load_answers(Path::new(&root))
}

/// Take back an answer, so the decision is asked (or handled) afresh.
#[tauri::command]
async fn clear_answer(
    app: AppHandle,
    root: String,
    id: String,
    jobs: State<'_, Jobs>,
) -> CmdResult<Review> {
    let rootp = PathBuf::from(&root);
    let mut answers = engine::settings::load_answers(&rootp);
    answers.remove(&id);
    engine::settings::save_answers(&rootp, &answers)?;
    review(app, root, jobs).await
}

/// Build the new library: every album except those you left out and those still waiting for
/// an answer. Progress streams over `on_progress`.
#[tauri::command]
async fn build_library(
    root: String,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<ApplyReport> {
    let settings = settings_with_defaults();
    let target = settings
        .new_library
        .ok_or("Choose where the new library should go first (Settings, Music folders).")?;
    let left_out = engine::settings::load_decisions(Path::new(&root));
    let albums: Vec<engine::plan::AlbumPlan> = {
        let guard = jobs.plan.lock().map_err(|e| e.to_string())?;
        let (plan_root, plan) = guard
            .as_ref()
            .ok_or("Open Organize first so the plan is up to date.")?;
        if *plan_root != root {
            return Err("The plan is for a different library; open Organize again.".into());
        }
        plan.albums
            .iter()
            .filter(|a| a.waiting_on.is_empty() && left_out.get(&a.id) != Some(&Decision::Skipped))
            .cloned()
            .collect()
    };
    if albums.is_empty() {
        return Err("Every album is waiting for an answer or left out.".into());
    }
    let progress = Arc::new(ApplyProgress::default());
    {
        let mut slot = jobs.apply.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("The library is already being built.".into());
        }
        *slot = Some(progress.clone());
    }
    let p = progress.clone();
    let result = blocking(move || {
        engine::apply::apply(
            &albums,
            Path::new(&root),
            Path::new(&target),
            &p,
            &|done, total| {
                let _ = on_progress.send(DeepProgress {
                    done,
                    total,
                    note: String::new(),
                });
            },
        )
    })
    .await;
    if let Ok(mut slot) = jobs.apply.lock() {
        *slot = None;
    }
    result
}

#[tauri::command]
fn set_decisions(
    root: String,
    changes: BTreeMap<String, Option<Decision>>,
) -> CmdResult<BTreeMap<String, Decision>> {
    let root = PathBuf::from(root);
    let mut d = engine::settings::load_decisions(&root);
    for (album, decision) in changes {
        match decision {
            Some(x) => d.insert(album, x),
            None => d.remove(&album),
        };
    }
    engine::settings::save_decisions(&root, &d)?;
    Ok(d)
}

#[tauri::command]
fn cancel_apply(jobs: State<'_, Jobs>) {
    if let Ok(slot) = jobs.apply.lock()
        && let Some(p) = slot.as_ref()
    {
        p.cancel.store(true, Ordering::Relaxed);
    }
}

/// Past build runs that can still be undone, newest first.
#[tauri::command]
fn list_runs() -> Vec<Run> {
    let dir = engine::apply::journal_dir();
    let mut runs: Vec<Run> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let kind = if name.ends_with("-apply.jsonl") {
                "build"
            } else if name.ends_with("-tidy.jsonl") {
                "tidy"
            } else {
                return None;
            };
            let text = std::fs::read_to_string(e.path()).ok()?;
            let lines: Vec<serde_json::Value> = text
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            let op =
                |v: &serde_json::Value| v.get("op").and_then(|o| o.as_str()).map(str::to_owned);
            let first = lines.first()?;
            let failed = lines
                .iter()
                .filter(|v| op(v).as_deref() == Some("failed"))
                .map(|v| RunFailure {
                    file: v
                        .get("file")
                        .and_then(|f| f.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    message: v
                        .get("message")
                        .and_then(|f| f.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect();
            let finished = lines
                .iter()
                .find(|v| op(v).as_deref() == Some("finished"))
                .map(|v| {
                    !v.get("cancelled")
                        .and_then(|c| c.as_bool())
                        .unwrap_or(false)
                });
            Some(Run {
                journal: e.path().to_string_lossy().into_owned(),
                started_ms: name.split('-').next()?.parse().ok()?,
                target: first
                    .get("target")
                    .or_else(|| first.get("root"))?
                    .as_str()?
                    .to_owned(),
                files: lines
                    .iter()
                    .filter(|v| matches!(op(v).as_deref(), Some("create" | "move" | "retag")))
                    .count(),
                kind,
                failed,
                finished,
            })
        })
        .filter(|r| r.files > 0)
        .collect();
    runs.sort_by_key(|r| std::cmp::Reverse(r.started_ms));
    runs
}

/// One change to your files, for the Activity timeline.
#[derive(Serialize)]
struct Change {
    /// "edit": tags or ratings changed in place.
    kind: &'static str,
    journal: String,
    started_ms: u128,
    files: usize,
    /// A few of the files that changed (library-relative).
    examples: Vec<String>,
}

/// Tag and rating edits that can still be undone, newest first.
#[tauri::command]
fn list_edits() -> Vec<Change> {
    let dir = engine::apply::journal_dir();
    let mut out: Vec<Change> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with("-edit.jsonl") {
                return None;
            }
            let text = std::fs::read_to_string(e.path()).ok()?;
            let files: Vec<String> = text
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v.get("file")?.as_str().map(str::to_owned))
                .collect();
            Some(Change {
                kind: "edit",
                journal: e.path().to_string_lossy().into_owned(),
                started_ms: name.split('-').next()?.parse().ok()?,
                files: files.len(),
                examples: files.into_iter().take(3).collect(),
            })
        })
        .filter(|c| c.files > 0)
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.started_ms));
    out
}

#[tauri::command]
async fn undo_edit(journal: String) -> CmdResult<usize> {
    blocking(move || engine::edit::undo_edits(Path::new(&journal))).await
}

#[derive(Serialize, serde::Deserialize)]
struct AliasesView {
    artists: Vec<engine::rules::Alias>,
    distinct: Vec<[String; 2]>,
}

/// Your artist names: each name with the spellings that mean it, and names kept apart.
#[tauri::command]
fn get_aliases() -> CmdResult<AliasesView> {
    let r = rules()?;
    Ok(AliasesView {
        artists: r.aliases.artists.clone(),
        distinct: r.aliases.distinct.clone(),
    })
}

#[tauri::command]
fn save_aliases(view: AliasesView) -> CmdResult<()> {
    let dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
    let mut r = Rules::load(&dir)?;
    r.aliases.artists = view.artists;
    r.aliases.distinct = view.distinct;
    r.save_aliases(&dir)
}

#[tauri::command]
async fn undo_run(journal: String) -> CmdResult<usize> {
    blocking(move || engine::apply::undo(Path::new(&journal))).await
}

// --- Listening --------------------------------------------------------------------------------

/// One song as the Music home shows it: enough to browse, sort and play, nothing more.
#[derive(Serialize)]
struct CatalogTrack {
    path: String,
    dir: String,
    title: String,
    /// Each singer, in order.
    artists: Vec<String>,
    album: String,
    album_artist: String,
    disc: u32,
    track: u32,
    duration_ms: u64,
    sample_rate: u32,
    bit_depth: u8,
    /// "FLAC", "MP3", "M4A"…
    format: String,
    year: String,
    genre: String,
    category: String,
    lyrics: bool,
    /// Your rating, 0–5.
    stars: u8,
    /// ReplayGain from the tags (dB, and peaks 0–1), for volume levelling.
    rg: player::dsp::Gain,
}

/// "-6.52 dB" → -6.52.
fn tag_number(v: Option<&str>) -> Option<f32> {
    v.and_then(|s| {
        s.trim()
            .trim_end_matches("dB")
            .trim_end_matches("db")
            .trim()
            .parse()
            .ok()
    })
}

/// Every song in the open folder, for the Music home.
#[tauri::command]
async fn music_catalog(root: String) -> CmdResult<Vec<CatalogTrack>> {
    blocking(move || {
        let index = open_index(Path::new(&root))?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        let num = |t: &engine::library::Track, k: &str| {
            t.first(k)
                .and_then(|v| v.split('/').next()?.trim().parse::<u32>().ok())
                .unwrap_or(0)
        };
        let mut out: Vec<CatalogTrack> =
            lib.tracks
                .iter()
                .map(|t| {
                    let stem = t.path.rsplit_once('.').map(|x| x.0).unwrap_or(&t.path);
                    let ext = t.path.rsplit('.').next().unwrap_or("").to_ascii_uppercase();
                    CatalogTrack {
                        path: t.path.clone(),
                        dir: t.dir.clone(),
                        title: t.first("TITLE").map(str::to_owned).unwrap_or_else(|| {
                            t.path.rsplit('/').next().unwrap_or(&t.path).to_owned()
                        }),
                        artists: t
                            .tags
                            .get("ARTIST")
                            .map(|v| {
                                v.iter()
                                    .flat_map(|a| engine::plan::split_names(a))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        album: t.first("ALBUM").unwrap_or_default().to_owned(),
                        album_artist: t
                            .first("ALBUMARTIST")
                            .or(t.first("ARTIST"))
                            .unwrap_or_default()
                            .to_owned(),
                        disc: num(t, "DISCNUMBER").max(1),
                        track: num(t, "TRACKNUMBER"),
                        duration_ms: t.duration_ms,
                        sample_rate: t.sample_rate,
                        bit_depth: t.bit_depth,
                        format: if ext == "FLAC" { "FLAC".into() } else { ext },
                        year: t
                            .first("ORIGINALYEAR")
                            .or(t.first("DATE"))
                            .map(|d| d.chars().take(4).collect())
                            .unwrap_or_default(),
                        genre: t.first("GENRE").unwrap_or_default().to_owned(),
                        category: t.first("CATEGORY").unwrap_or_default().to_owned(),
                        lyrics: t.tags.contains_key("LYRICS")
                            || lib.lyrics_files.contains(&format!("{stem}.lrc")),
                        stars: t
                            .first("RATING")
                            .map(engine::edit::rating_to_stars)
                            .unwrap_or(0),
                        rg: player::dsp::Gain {
                            track_db: tag_number(t.first("REPLAYGAIN_TRACK_GAIN")),
                            album_db: tag_number(t.first("REPLAYGAIN_ALBUM_GAIN")),
                            track_peak: tag_number(t.first("REPLAYGAIN_TRACK_PEAK")),
                            album_peak: tag_number(t.first("REPLAYGAIN_ALBUM_PEAK")),
                        },
                    }
                })
                .collect();
        out.sort_by(|a, b| {
            (a.dir.as_str(), a.disc, a.track, a.path.as_str()).cmp(&(
                b.dir.as_str(),
                b.disc,
                b.track,
                b.path.as_str(),
            ))
        });
        Ok(out)
    })
    .await
}

// --- Your library, tidied in place -----------------------------------------------------------

fn rules_dir() -> PathBuf {
    PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default())
}

/// The plan for a library's Music folder with your rules, optionally with another folder
/// arrangement tried instead of the saved one.
fn library_plan(root: &Path, template: Option<&str>) -> CmdResult<Plan> {
    let index = open_index(root)?;
    let mut rules = Rules::load(&rules_dir())?;
    if let Some(t) = template {
        rules.layout.template = t.to_owned();
    }
    engine::plan::build_checked(&index, root, &rules)
        .map(|(_, plan)| plan)
        .map_err(|e| e.to_string())
}

/// What tidying the library would change now, with your saved rules.
#[tauri::command]
async fn tidy_preview(root: String, jobs: State<'_, Jobs>) -> CmdResult<engine::tidy::TidyPreview> {
    let r = root.clone();
    let (plan, preview) = blocking(move || {
        let root = PathBuf::from(r);
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())?;
        let plan = library_plan(&root, None)?;
        let preview = engine::tidy::preview(&root, &plan);
        Ok((plan, preview))
    })
    .await?;
    *jobs.tidy_plan.lock().map_err(|e| e.to_string())? = Some((root, plan));
    Ok(preview)
}

/// Tidy the chosen albums (all when empty) from the last preview; the library is read again
/// afterwards and playlists follow the files.
#[tauri::command]
async fn tidy_apply(
    root: String,
    albums: Vec<String>,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<engine::tidy::TidyReport> {
    let plan = {
        let guard = jobs.tidy_plan.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some((r, p)) if *r == root => p.clone(),
            _ => return Err("Look at the changes first, then apply them.".into()),
        }
    };
    let progress = Arc::new(ApplyProgress::default());
    {
        let mut slot = jobs.apply.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err(
                "Something is already being built or tidied; wait for it to finish.".into(),
            );
        }
        *slot = Some(progress.clone());
    }
    let p = progress.clone();
    let result = blocking(move || {
        let root = PathBuf::from(root);
        let report = engine::tidy::apply(&root, &plan, &albums, &p, &|done, total| {
            let _ = on_progress.send(DeepProgress {
                done,
                total,
                note: String::new(),
            });
        })?;
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())?;
        if let Ok(lib) = Library::load(&index) {
            engine::playlists::repair_all(&root, &lib);
        }
        Ok(report)
    })
    .await;
    if let Ok(mut slot) = jobs.apply.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = jobs.tidy_plan.lock() {
        *slot = None;
    }
    result
}

/// Put back everything a tidy run did; the library is read again afterwards.
#[tauri::command]
async fn tidy_undo(root: String, journal: String) -> CmdResult<usize> {
    blocking(move || {
        let n = engine::tidy::undo(Path::new(&journal))?;
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())?;
        if let Ok(lib) = Library::load(&index) {
            engine::playlists::repair_all(&root, &lib);
        }
        Ok(n)
    })
    .await
}

/// How your library's folders are named: the saved arrangement (a path pattern).
#[tauri::command]
fn get_layout() -> CmdResult<String> {
    Ok(Rules::load(&rules_dir())?.layout.template)
}

fn check_template(t: &str) -> CmdResult<()> {
    for need in ["{album}", "{title}", "{track}"] {
        if !t.contains(need) {
            return Err(format!("The arrangement needs {need}."));
        }
    }
    if t.contains("..") || t.starts_with('/') || t.contains(':') || t.contains('\\') {
        return Err("The arrangement can only name folders inside your library.".into());
    }
    Ok(())
}

/// Save a new arrangement. Only the pattern line of layout.toml changes; your comments stay.
#[tauri::command]
fn set_layout(template: String) -> CmdResult<()> {
    check_template(&template)?;
    let dir = rules_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("layout.toml");
    // A JSON string is also a valid TOML basic string (same quoting and escapes).
    let line = format!(
        "template = {}",
        serde_json::to_string(&template).map_err(|e| e.to_string())?
    );
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            if !found && l.trim_start().starts_with("template") && l.contains('=') {
                found = true;
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        out.insert(0, line);
    }
    let tmp = file.with_extension("toml.part");
    std::fs::write(&tmp, out.join("\n") + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct LayoutExample {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct LayoutTrial {
    /// Albums whose folder would change.
    albums: usize,
    /// A few of them, from different shelves.
    examples: Vec<LayoutExample>,
}

/// What an arrangement would do to the library, without saving or moving anything.
#[tauri::command]
async fn try_layout(root: String, template: String) -> CmdResult<LayoutTrial> {
    check_template(&template)?;
    blocking(move || {
        let root = PathBuf::from(root);
        let plan = library_plan(&root, Some(&template))?;
        let changed: Vec<&engine::plan::AlbumPlan> = plan
            .albums
            .iter()
            .filter(|a| a.tracks.iter().any(|t| t.source != t.target))
            .collect();
        // One example per shelf first, then more, up to four.
        let mut seen = std::collections::HashSet::new();
        let mut picks: Vec<&engine::plan::AlbumPlan> = Vec::new();
        for a in changed.iter().filter(|a| a.tracks.len() > 2) {
            let shelf = a.source_dir.split('/').next().unwrap_or("").to_owned();
            if seen.insert(shelf) {
                picks.push(a);
            }
        }
        for a in &changed {
            if picks.len() >= 4 {
                break;
            }
            if !picks.iter().any(|p| p.id == a.id) {
                picks.push(a);
            }
        }
        picks.truncate(4);
        Ok(LayoutTrial {
            albums: changed.len(),
            examples: picks
                .iter()
                .map(|a| {
                    let t = &a.tracks[0];
                    LayoutExample {
                        from: t.source.clone(),
                        to: t.target.clone(),
                    }
                })
                .collect(),
        })
    })
    .await
}

/// Look the library's albums up online (credits, years, covers) and find missing lyrics.
#[tauri::command]
async fn lookup_online(
    root: String,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<engine::enrich::EnrichReport> {
    let r = rules()?;
    run_lookup(&jobs, on_progress, move |p, cb| {
        engine::enrich::run_for_root(Path::new(&root), &r, p, cb)
    })
    .await
}

#[tauri::command]
fn cancel_lookup(jobs: State<'_, Jobs>) {
    if let Ok(slot) = jobs.lookup.lock()
        && let Some(p) = slot.as_ref()
    {
        p.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
struct IncomingView {
    folder: String,
    plan: PlanView,
}

/// Scan and plan what is waiting in the incoming folder.
#[tauri::command]
async fn incoming(root: String, jobs: State<'_, Jobs>) -> CmdResult<IncomingView> {
    let r = rules()?;
    let rr = root.clone();
    let plan =
        blocking(move || engine::intake::plan(Path::new(&rr), &r).map_err(|e| e.to_string()))
            .await?;
    let view = plan.view();
    *jobs.incoming_plan.lock().map_err(|e| e.to_string())? = Some((root.clone(), plan));
    Ok(IncomingView {
        folder: engine::intake::incoming_dir(Path::new(&root))
            .to_string_lossy()
            .into_owned(),
        plan: view,
    })
}

#[tauri::command]
async fn incoming_lookup(
    root: String,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<engine::enrich::EnrichReport> {
    let r = rules()?;
    run_lookup(&jobs, on_progress, move |p, cb| {
        engine::intake::lookup(Path::new(&root), &r, p, cb)
    })
    .await
}

/// Import the chosen incoming albums into the library.
#[tauri::command]
async fn incoming_import(
    root: String,
    albums: Vec<String>,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<ApplyReport> {
    let chosen: Vec<engine::plan::AlbumPlan> = {
        let guard = jobs.incoming_plan.lock().map_err(|e| e.to_string())?;
        let (plan_root, plan) = guard.as_ref().ok_or("Open Incoming first.")?;
        if *plan_root != root {
            return Err("The incoming plan is for a different library.".into());
        }
        plan.albums
            .iter()
            .filter(|a| albums.contains(&a.id))
            .cloned()
            .collect()
    };
    if chosen.is_empty() {
        return Err("Choose albums to import.".into());
    }
    let progress = Arc::new(ApplyProgress::default());
    {
        let mut slot = jobs.apply.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("Something is already being written.".into());
        }
        *slot = Some(progress.clone());
    }
    let p = progress.clone();
    let result = blocking(move || {
        engine::intake::import(Path::new(&root), &chosen, &p, &|done, total| {
            let _ = on_progress.send(DeepProgress {
                done,
                total,
                note: String::new(),
            });
        })
    })
    .await;
    if let Ok(mut slot) = jobs.apply.lock() {
        *slot = None;
    }
    result
}

/// Copy dropped files or folders into the incoming folder.
#[tauri::command]
async fn add_to_incoming(root: String, paths: Vec<String>) -> CmdResult<usize> {
    blocking(move || {
        let sources: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        engine::intake::add(Path::new(&root), &sources)
    })
    .await
}

/// Cheap check for the incoming screen: how many audio files are waiting.
#[tauri::command]
fn incoming_count(root: String) -> usize {
    let dir = engine::intake::incoming_dir(Path::new(&root));
    walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .flatten()
        .filter(|e| {
            e.file_type().is_file() && engine::media::FileKind::from_path(e.path()).is_audio()
        })
        .count()
}

/// Attached cards (set up or not) and their state.
#[tauri::command]
async fn list_cards() -> CmdResult<Vec<engine::sync::CardStatus>> {
    blocking(|| Ok(engine::sync::cards())).await
}

/// Top-level folders of the library, for choosing what a card carries.
#[tauri::command]
fn library_folders(root: String) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('_') && !n.starts_with('.'))
        .collect();
    v.sort();
    v
}

#[tauri::command]
fn setup_card(
    card: String,
    name: String,
    include: Vec<String>,
    auto_sync: bool,
    player: Option<engine::device::Player>,
) -> CmdResult<engine::sync::CardConfig> {
    engine::sync::setup_card(Path::new(&card), &name, include, auto_sync, player)
}

#[derive(Serialize)]
struct PlayerInfo {
    id: engine::device::Player,
    label: &'static str,
    /// What a card copy for this player changes, in words.
    notes: Vec<String>,
}

/// Write the player test to a card (one of your songs in nine variants, and two playlists).
#[tauri::command]
async fn player_test_write(root: String, card: String) -> CmdResult<Vec<engine::device::TestItem>> {
    blocking(move || {
        let folder = engine::sync::read_card(Path::new(&card))
            .map(|c| c.folder)
            .unwrap_or_else(|| "Music".into());
        engine::device::write_test_kit(Path::new(&root), Path::new(&card), &folder)
    })
    .await
}

#[tauri::command]
fn player_test_remove(card: String) -> CmdResult<()> {
    let folder = engine::sync::read_card(Path::new(&card))
        .map(|c| c.folder)
        .unwrap_or_else(|| "Music".into());
    engine::device::remove_test_kit(Path::new(&card), &folder)
}

#[tauri::command]
fn player_test_save(
    card: String,
    results: BTreeMap<String, bool>,
) -> CmdResult<engine::sync::CardConfig> {
    engine::sync::save_test_results(Path::new(&card), results)
}

/// The players a card can be written for.
#[tauri::command]
fn players() -> Vec<PlayerInfo> {
    engine::device::Player::ALL
        .iter()
        .map(|&p| {
            let r = p.recipe();
            let mut notes = Vec::new();
            if !r.transform {
                notes.push("Files are copied exactly as they are in your library.".to_owned());
            } else {
                if r.join_artists {
                    notes
                        .push("Several singers written as one name list, separated by “;”.".into());
                }
                if r.date_from_original {
                    notes.push("The year shown is the original release year.".into());
                }
                notes.push(format!(
                    "Covers re-saved as standard JPEG, at most {} px.",
                    r.cover_max_px
                ));
                if !r.playlists {
                    notes.push(
                        "No playlist files (this player can't read them; use its favourites)."
                            .into(),
                    );
                }
            }
            PlayerInfo {
                id: p,
                label: p.label(),
                notes,
            }
        })
        .collect()
}

/// What a sync would do, without doing it.
#[tauri::command]
async fn plan_card(root: String, card: String) -> CmdResult<engine::sync::SyncPlan> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let mut index = open_index(&rootp)?;
        engine::scan(&mut index, &rootp).map_err(|e| e.to_string())?;
        engine::sync::plan(&rootp, Path::new(&card))
    })
    .await
}

/// Mirror the library onto a card. Progress streams over `on_progress`.
#[tauri::command]
async fn sync_card(
    root: String,
    card: String,
    on_progress: Channel<SyncProgressMsg>,
    jobs: State<'_, Jobs>,
) -> CmdResult<engine::sync::SyncReport> {
    let progress = Arc::new(engine::sync::SyncProgress::default());
    {
        let mut syncs = jobs.syncs.lock().map_err(|e| e.to_string())?;
        if syncs.contains_key(&card) {
            return Err("This card is already syncing.".into());
        }
        syncs.insert(card.clone(), progress.clone());
    }
    let (p, c) = (progress.clone(), card.clone());
    let result = blocking(move || {
        let rootp = PathBuf::from(&root);
        let mut index = open_index(&rootp)?;
        engine::scan(&mut index, &rootp).map_err(|e| e.to_string())?;
        let plan = engine::sync::plan(&rootp, Path::new(&c))?;
        engine::sync::run(&rootp, Path::new(&c), &plan, &p, &|done, total, bytes| {
            let _ = on_progress.send(SyncProgressMsg { done, total, bytes });
        })
    })
    .await;
    if let Ok(mut syncs) = jobs.syncs.lock() {
        syncs.remove(&card);
    }
    result
}

#[tauri::command]
fn cancel_sync(card: String, jobs: State<'_, Jobs>) {
    if let Ok(syncs) = jobs.syncs.lock()
        && let Some(p) = syncs.get(&card)
    {
        p.cancel.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
async fn eject_card(card: String) -> CmdResult<()> {
    blocking(move || engine::volumes::eject(Path::new(&card))).await
}

// --- Playlists -------------------------------------------------------------------------

fn load_lib(root: &Path) -> CmdResult<(Index, Library)> {
    let mut index = open_index(root)?;
    engine::scan(&mut index, root).map_err(|e| e.to_string())?;
    let lib = Library::load(&index).map_err(|e| e.to_string())?;
    Ok((index, lib))
}

fn wantlist_path() -> PathBuf {
    paths::app_data_dir().join("wantlist.json")
}

fn load_wanted() -> Vec<engine::playlists::WantedAlbum> {
    std::fs::read_to_string(wantlist_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// How albums you put on the list yourself are marked.
const BETTER_COPY: &str = "Get a better copy";

/// Put an album on "To get", because your copy has a problem (lossy, fake hi-res…).
#[tauri::command]
fn want_add(album: String, artist: String, why: String) -> CmdResult<usize> {
    let mut list = load_wanted();
    if !list.iter().any(|w| w.album == album && w.artist == artist) {
        list.insert(
            0,
            engine::playlists::WantedAlbum {
                album,
                artist,
                tier: 1,
                why: format!("{BETTER_COPY}: {why}"),
                saved_tracks: 0,
                playlists: vec![],
            },
        );
    }
    std::fs::write(
        wantlist_path(),
        serde_json::to_vec(&list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(list.len())
}

#[derive(Serialize)]
struct PlaylistsView {
    playlists: Vec<engine::playlists::PlaylistSummary>,
    smart: Vec<engine::playlists::SmartRule>,
    /// How many songs each smart rule selects right now.
    smart_counts: BTreeMap<String, usize>,
    /// Where playlists are saved (the folder may not exist yet).
    folder: String,
    wanted: Vec<engine::playlists::WantedAlbum>,
    apple_ready: bool,
}

/// Playlists in the library (repairing moved entries), smart rules and the want-list.
#[tauri::command]
async fn playlists(root: String) -> CmdResult<PlaylistsView> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (index, lib) = load_lib(&rootp)?;
        let rules_dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
        engine::playlists::ensure_default_smart(&rules_dir)?;
        // Opening the screen only writes into a library that already has playlists; the first
        // one is always made by something you do (new playlist, import, saving a rule).
        let folder = engine::playlists::folder(&rootp);
        if folder.is_dir() {
            engine::playlists::repair_all(&rootp, &lib);
            engine::playlists::refresh_smart(&rootp, &rules_dir, &lib, &index)?;
        }
        let evaluated = engine::playlists::evaluate_all(&rules_dir, &lib, &index)?;
        let s = settings_with_defaults();
        Ok(PlaylistsView {
            playlists: engine::playlists::list(&rootp, &lib),
            smart_counts: evaluated
                .iter()
                .map(|(r, p)| (r.name.clone(), p.len()))
                .collect(),
            smart: evaluated.into_iter().map(|(r, _)| r).collect(),
            folder: folder.to_string_lossy().into_owned(),
            wanted: std::fs::read_to_string(wantlist_path())
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default(),
            apple_ready: s.apple_key.is_some()
                && s.apple_library
                    .as_ref()
                    .or(engine::apple::default_library_path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .as_ref())
                    .is_some(),
        })
    })
    .await
}

#[tauri::command]
async fn playlist_tracks(
    root: String,
    name: String,
) -> CmdResult<Vec<engine::playlists::PlaylistEntry>> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::playlists::read(&rootp, &lib, &name).map(|r| r.0)
    })
    .await
}

/// Create or replace a playlist with these library paths, in order.
#[tauri::command]
async fn playlist_save(root: String, name: String, paths: Vec<String>) -> CmdResult<()> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::playlists::write(&rootp, &lib, &name, &paths, false)
    })
    .await
}

#[tauri::command]
fn playlist_delete(root: String, name: String) -> CmdResult<()> {
    engine::playlists::delete(Path::new(&root), &name)
}

#[tauri::command]
fn playlist_rename(root: String, from: String, to: String) -> CmdResult<()> {
    engine::playlists::rename(Path::new(&root), &from, &to)
}

/// Save a smart rule and write its playlist (and the other smart ones) into the library.
#[tauri::command]
async fn smart_save(root: String, rule: engine::playlists::SmartRule) -> CmdResult<()> {
    blocking(move || {
        let rules_dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
        engine::playlists::save_smart(&rules_dir, &rule)?;
        let rootp = PathBuf::from(&root);
        let (index, lib) = load_lib(&rootp)?;
        engine::playlists::refresh_smart(&rootp, &rules_dir, &lib, &index).map(|_| ())
    })
    .await
}

/// Bring your Apple Music playlists over: matched tracks become playlists, the rest the want-list.
#[tauri::command]
async fn apple_import(root: String) -> CmdResult<engine::playlists::ImportReport> {
    blocking(move || {
        let s = settings_with_defaults();
        let key = s
            .apple_key
            .ok_or("Enter your Apple Music library key in settings first.")?;
        let file = s
            .apple_library
            .map(PathBuf::from)
            .or_else(engine::apple::default_library_path)
            .ok_or("Couldn't find your Apple Music library.")?;
        let apple = engine::apple::read(&file, &key)?;
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let report = engine::playlists::import_apple(&rootp, &lib, &apple, "", true)?;
        // Albums you added yourself (better copies to get) stay on the list.
        let mut wanted = report.wanted.clone();
        for w in load_wanted()
            .into_iter()
            .filter(|w| w.why.starts_with(BETTER_COPY))
        {
            if !wanted
                .iter()
                .any(|x| x.album == w.album && x.artist == w.artist)
            {
                wanted.push(w);
            }
        }
        let _ = std::fs::write(
            wantlist_path(),
            serde_json::to_vec(&wanted).unwrap_or_default(),
        );
        // A copy you can open in a spreadsheet, next to your rules.
        if let Some(meta) = s
            .rules_dir
            .as_ref()
            .and_then(|r| Path::new(r).parent().map(Path::to_path_buf))
        {
            let _ = std::fs::write(
                meta.join("wanted-albums.csv"),
                engine::playlists::wanted_csv(&report.wanted),
            );
        }
        Ok(report)
    })
    .await
}

#[tauri::command]
fn set_apple(library: Option<String>, key: Option<String>) -> CmdResult<()> {
    let mut s = settings_with_defaults();
    if library.is_some() {
        s.apple_library = library;
    }
    if key.is_some() {
        s.apple_key = key;
    }
    engine::settings::save(&s)
}

/// Rate the songs of one playlist (e.g. Apple's Favourite Songs) that have no rating yet.
#[tauri::command]
async fn rate_playlist(
    root: String,
    name: String,
    stars: u8,
) -> CmdResult<engine::edit::EditReport> {
    if !settings_with_defaults().backup_confirmed {
        return Err("BACKUP_NOT_CONFIRMED".into());
    }
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let (entries, _) = engine::playlists::read(&rootp, &lib, &name)?;
        let unrated: HashSet<String> = lib
            .tracks
            .iter()
            .filter(|t| t.first("RATING").is_none())
            .map(|t| t.path.clone())
            .collect();
        let edits: Vec<engine::edit::TagEdit> = entries
            .into_iter()
            .filter(|e| !e.missing && unrated.contains(&e.path))
            .map(|e| engine::edit::TagEdit {
                file: e.path,
                set: vec![(
                    "RATING".into(),
                    engine::edit::stars_to_rating(stars).into_iter().collect(),
                )],
            })
            .collect();
        let report = engine::edit::edit_in_place(&rootp, &edits)?;
        let mut index = open_index(&rootp)?;
        engine::scan(&mut index, &rootp).map_err(|e| e.to_string())?;
        Ok(report)
    })
    .await
}

#[derive(Serialize)]
struct TrackRow {
    path: String,
    disc: u32,
    track: u32,
    title: String,
    artist: String,
    composer: String,
    lyricist: String,
    stars: u8,
    duration_ms: u64,
    sample_rate: u32,
    bit_depth: u8,
}

#[derive(Serialize)]
struct AlbumDetail {
    album: String,
    album_artist: String,
    original_year: String,
    genre: String,
    composer: String,
    lyricist: String,
    tracks: Vec<TrackRow>,
}

/// Tracks and editable album fields for one album folder.
#[tauri::command]
async fn album_detail(root: String, dir: String) -> CmdResult<AlbumDetail> {
    blocking(move || {
        let index = open_index(Path::new(&root))?;
        let lib = Library::load(&index).map_err(|e| e.to_string())?;
        let mut tracks: Vec<&engine::library::Track> =
            lib.tracks.iter().filter(|t| t.dir == dir).collect();
        let num = |t: &engine::library::Track, k: &str| {
            t.first(k)
                .and_then(|v| v.split('/').next()?.trim().parse::<u32>().ok())
                .unwrap_or(0)
        };
        tracks.sort_by_key(|t| (num(t, "DISCNUMBER"), num(t, "TRACKNUMBER"), t.path.clone()));
        let common = |k: &str| {
            let first = tracks
                .first()
                .and_then(|t| t.tags.get(k))
                .map(|v| v.join("; "))
                .unwrap_or_default();
            let same = tracks
                .iter()
                .all(|t| t.tags.get(k).map(|v| v.join("; ")).unwrap_or_default() == first);
            if same { first } else { String::new() }
        };
        let joined = |t: &engine::library::Track, k: &str| {
            t.tags.get(k).map(|v| v.join("; ")).unwrap_or_default()
        };
        Ok(AlbumDetail {
            album: common("ALBUM"),
            album_artist: common("ALBUMARTIST"),
            original_year: {
                let y = common("ORIGINALYEAR");
                if y.is_empty() {
                    common("DATE").chars().take(4).collect()
                } else {
                    y
                }
            },
            genre: common("GENRE"),
            composer: common("COMPOSER"),
            lyricist: common("LYRICIST"),
            tracks: tracks
                .iter()
                .map(|t| TrackRow {
                    path: t.path.clone(),
                    disc: num(t, "DISCNUMBER"),
                    track: num(t, "TRACKNUMBER"),
                    title: t.first("TITLE").unwrap_or_default().to_owned(),
                    artist: joined(t, "ARTIST"),
                    composer: joined(t, "COMPOSER"),
                    lyricist: joined(t, "LYRICIST"),
                    stars: t
                        .first("RATING")
                        .map(engine::edit::rating_to_stars)
                        .unwrap_or(0),
                    duration_ms: t.duration_ms,
                    sample_rate: t.sample_rate,
                    bit_depth: t.bit_depth,
                })
                .collect(),
        })
    })
    .await
}

/// Edit tags in place (the owner's own changes). Requires the one-time backup confirmation.
#[tauri::command]
async fn edit_tags(
    root: String,
    edits: Vec<engine::edit::TagEdit>,
) -> CmdResult<engine::edit::EditReport> {
    if !settings_with_defaults().backup_confirmed {
        return Err("BACKUP_NOT_CONFIRMED".into());
    }
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let report = engine::edit::edit_in_place(&rootp, &edits)?;
        // Refresh the index so every screen sees the change.
        let mut index = open_index(&rootp)?;
        engine::scan(&mut index, &rootp).map_err(|e| e.to_string())?;
        Ok(report)
    })
    .await
}

#[tauri::command]
fn confirm_backup() -> CmdResult<()> {
    let mut s = settings_with_defaults();
    s.backup_confirmed = true;
    engine::settings::save(&s)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Jobs::default())
        .manage(playback::Playback::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            allow_data_assets(app.handle());
            #[cfg(windows)]
            if let Some(w) = app.get_webview_window("main")
                && let Ok(hwnd) = w.hwnd()
            {
                playback::start_media_controls(&app.state::<playback::Playback>(), hwnd.0 as isize);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine_version,
            scan_library,
            library_health,
            album_covers,
            deep_check,
            cancel_deep_check,
            album_spectrogram,
            get_settings,
            set_settings,
            library_home,
            check_home,
            choose_home,
            check_music_folder,
            set_open_folder,
            music_catalog,
            playback::player_subscribe,
            playback::player_play,
            playback::player_toggle,
            playback::player_next,
            playback::player_previous,
            playback::player_seek,
            playback::player_volume,
            playback::player_options,
            playback::player_enqueue,
            playback::player_remove,
            playback::player_move,
            playback::player_jump,
            playback::player_devices,
            playback::player_output,
            playback::song_lyrics,
            playback::player_resume,
            playback::player_verify,
            playback::song_waveform,
            playback::player_sound,
            playback::player_stop_after,
            playback::player_sleep,
            listening::listening_summary,
            tidy_preview,
            tidy_apply,
            tidy_undo,
            get_layout,
            set_layout,
            try_layout,
            save_appearance,
            check_new_library,
            review,
            answer_decisions,
            clear_answer,
            list_answers,
            build_library,
            list_edits,
            undo_edit,
            get_aliases,
            save_aliases,
            set_decisions,
            cancel_apply,
            list_runs,
            undo_run,
            album_detail,
            edit_tags,
            confirm_backup,
            lookup_online,
            cancel_lookup,
            incoming,
            incoming_lookup,
            incoming_import,
            add_to_incoming,
            incoming_count,
            list_cards,
            library_folders,
            setup_card,
            players,
            want_add,
            player_test_write,
            player_test_remove,
            player_test_save,
            plan_card,
            sync_card,
            cancel_sync,
            eject_card,
            playlists,
            playlist_tracks,
            playlist_save,
            playlist_delete,
            playlist_rename,
            smart_save,
            apple_import,
            set_apple,
            rate_playlist
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
