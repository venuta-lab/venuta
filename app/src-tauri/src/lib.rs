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
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri::ipc::Channel;

/// The running deep check, if any, so it can be cancelled.
#[derive(Default)]
struct Jobs {
    deep_check: Mutex<Option<Arc<Progress>>>,
    apply: Mutex<Option<Arc<ApplyProgress>>>,
    /// The last plan built, kept here because it is too large to send to the UI.
    plan: Mutex<Option<(String, Plan)>>,
    /// The last incoming plan, same reason.
    incoming_plan: Mutex<Option<(String, Plan)>>,
    lookup: Mutex<Option<Arc<engine::enrich::EnrichProgress>>>,
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
        job(&p, &|done, total| {
            let _ = on_progress.send(DeepProgress { done, total });
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
    decisions: BTreeMap<String, Decision>,
    settings: Settings,
}

#[derive(Serialize)]
struct Run {
    journal: String,
    started_ms: u64,
    target: String,
    files: usize,
}

fn settings_with_defaults() -> Settings {
    let mut s = engine::settings::load();
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

fn build_plan(root: &Path) -> CmdResult<Plan> {
    let index = open_index(root)?;
    let lib = Library::load(&index).map_err(|e| e.to_string())?;
    let rules_dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
    let rules = Rules::load(&rules_dir)?;
    Ok(engine::plan::build(&lib, &rules))
}

#[derive(Clone, Serialize)]
struct DeepProgress {
    done: usize,
    total: usize,
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

/// Scan a library folder into its index. Read-only on the library.
#[tauri::command]
async fn scan_library(root: String) -> CmdResult<ScanReport> {
    blocking(move || {
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        engine::scan(&mut index, &root).map_err(|e| e.to_string())
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
            let _ = on_progress.send(DeepProgress { done, total });
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

#[tauri::command]
fn set_settings(settings: Settings) -> CmdResult<()> {
    engine::settings::save(&settings)
}

/// Build the plan for a library and return what the Review screen shows.
#[tauri::command]
async fn review(root: String, jobs: State<'_, Jobs>) -> CmdResult<Review> {
    let r = root.clone();
    let plan = blocking(move || build_plan(Path::new(&r))).await?;
    let view = plan.view();
    *jobs.plan.lock().map_err(|e| e.to_string())? = Some((root.clone(), plan));
    Ok(Review {
        plan: view,
        decisions: engine::settings::load_decisions(Path::new(&root)),
        settings: settings_with_defaults(),
    })
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

/// "Are A and B the same artist?" `keep` is the spelling to use, or null for "different".
#[tauri::command]
async fn answer_question(
    root: String,
    a: String,
    b: String,
    keep: Option<String>,
    jobs: State<'_, Jobs>,
) -> CmdResult<Review> {
    let rules_dir = PathBuf::from(settings_with_defaults().rules_dir.unwrap_or_default());
    engine::rules::answer_same_artist(&rules_dir, &a, &b, keep.as_deref())?;
    review(root, jobs).await
}

/// Build the approved albums into the new library. Progress streams over `on_progress`.
#[tauri::command]
async fn apply_approved(
    root: String,
    on_progress: Channel<DeepProgress>,
    jobs: State<'_, Jobs>,
) -> CmdResult<ApplyReport> {
    let settings = settings_with_defaults();
    let target = settings
        .new_library
        .ok_or("Choose where the new library should go first.")?;
    let decisions = engine::settings::load_decisions(Path::new(&root));
    let albums: Vec<engine::plan::AlbumPlan> = {
        let guard = jobs.plan.lock().map_err(|e| e.to_string())?;
        let (plan_root, plan) = guard
            .as_ref()
            .ok_or("Open Review first so the plan is up to date.")?;
        if *plan_root != root {
            return Err("The plan is for a different library; open Review again.".into());
        }
        plan.albums
            .iter()
            .filter(|a| decisions.get(&a.id) == Some(&Decision::Approved))
            .cloned()
            .collect()
    };
    if albums.is_empty() {
        return Err("No albums are approved yet.".into());
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
                let _ = on_progress.send(DeepProgress { done, total });
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
            if !name.ends_with("-apply.jsonl") {
                return None;
            }
            let text = std::fs::read_to_string(e.path()).ok()?;
            let first: serde_json::Value = serde_json::from_str(text.lines().next()?).ok()?;
            Some(Run {
                journal: e.path().to_string_lossy().into_owned(),
                started_ms: name.split('-').next()?.parse().ok()?,
                target: first.get("target")?.as_str()?.to_owned(),
                files: text.lines().count().saturating_sub(1),
            })
        })
        .filter(|r| r.files > 0)
        .collect();
    runs.sort_by_key(|r| std::cmp::Reverse(r.started_ms));
    runs
}

#[tauri::command]
async fn undo_run(journal: String) -> CmdResult<usize> {
    blocking(move || engine::apply::undo(Path::new(&journal))).await
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
            let _ = on_progress.send(DeepProgress { done, total });
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
        .plugin(tauri_plugin_dialog::init())
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
            review,
            set_decisions,
            answer_question,
            apply_approved,
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
            incoming_count
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
