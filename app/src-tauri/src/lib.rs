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

mod accounts;
mod app_icon;
mod clipboard;
mod desktop;
mod libcache;
mod listening;
mod lyric_windows;
mod lyrics_find;
mod online;
mod phone;
mod playback;
mod theme_packs;
mod updates;
mod ytm_account;

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

/// What the caches take (Settings, Online sources).
#[tauri::command]
async fn cache_sizes() -> CmdResult<engine::cache::CacheSizes> {
    blocking(|| Ok(engine::cache::sizes(&paths::app_data_dir()))).await
}

/// Clear the caches; returns what they took.
#[tauri::command]
async fn clear_cache() -> CmdResult<engine::cache::CacheSizes> {
    blocking(|| engine::cache::clear(&paths::app_data_dir())).await
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
        libcache::scan_with(&mut index, &root, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await
}

/// Health report from the current index (scan first for fresh results).
#[tauri::command]
async fn library_health(root: String) -> CmdResult<HealthReport> {
    blocking(move || {
        let index = open_index(Path::new(&root))?;
        let lib = libcache::indexed(&index, Path::new(&root))?;
        // Which .lrc files hold words with no times: read here, so the audit stays pure.
        let plain = engine::lrc::untimed_sidecars(Path::new(&root), &lib.lyrics_files);
        Ok(engine::audit::run_with(&lib, &plain))
    })
    .await
}

/// Cover thumbnails (cached JPEG paths + accent colour) for the given album folders.
#[tauri::command]
async fn album_covers(root: String, dirs: Vec<String>, size: u32) -> CmdResult<Vec<CoverThumb>> {
    blocking(move || {
        let root = PathBuf::from(root);
        let index = open_index(&root)?;
        let lib = libcache::indexed(&index, &root)?;
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
        libcache::scan(&mut index, &root)?;
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
        let lib = libcache::indexed(&index, &root)?;
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

/// Change some settings: only the fields given are written, onto what is saved now (so a
/// screen holding an older copy can't undo a change made elsewhere).
#[tauri::command]
fn set_settings(
    change: serde_json::Value,
    pb: tauri::State<'_, playback::Playback>,
) -> CmdResult<()> {
    let serde_json::Value::Object(change) = change else {
        return Err("Settings changes must be an object.".into());
    };
    let listening = change.contains_key("listening");
    let mut now = serde_json::to_value(engine::settings::load()).map_err(|e| e.to_string())?;
    if let serde_json::Value::Object(map) = &mut now {
        map.extend(change);
    }
    let s: Settings = serde_json::from_value(now).map_err(|e| e.to_string())?;
    engine::settings::save(&s)?;
    if listening {
        apply_listening(&pb, &s.listening);
    }
    Ok(())
}

/// What the online catalog must know of your listening choices: who it never offers, and
/// whether music videos are left out.
fn apply_listening(pb: &playback::Playback, l: &engine::settings::Listening) {
    let blocked: Vec<(Option<String>, String)> = l
        .blocked_artists
        .iter()
        .map(|b| (b.artist_id.clone(), b.name.clone()))
        .collect();
    let catalog = pb.catalog();
    catalog.set_blocked_artists(&blocked);
    catalog.set_hide_videos(l.hide_videos);
    catalog.set_region(Some(l.catalog_region.as_str()).filter(|c| !c.is_empty()));
    catalog.set_data_saver(l.stream_quality == "low");
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

/// The proxy chosen in Settings, for every online request: the app's own (they read the usual
/// proxy variables), the stream tool's (it is given them), and the window's (covers, sign-in).
/// Set once, before anything starts, so it takes effect from this start.
fn use_proxy() {
    let Some(proxy) = engine::settings::load()
        .proxy
        .as_deref()
        .and_then(engine::settings::proxy_address)
    else {
        return;
    };
    // SAFETY: the first thing the app does, before any thread starts that reads the environment.
    unsafe {
        for name in ["ALL_PROXY", "HTTPS_PROXY", "HTTP_PROXY"] {
            std::env::set_var(name, &proxy);
        }
        // The app's own local addresses (its window's dev server, the phone server) never go out.
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1,::1");
        let browser = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            format!("{browser} --proxy-server={proxy} --proxy-bypass-list=localhost;127.0.0.1")
                .trim(),
        );
    }
}

/// Start Venuta again (a proxy chosen in Settings is used from the next start).
#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// What helps find a problem, as text to paste into a report: Venuta's version, the computer,
/// how online music is set up, and the last lines of the playback log (what played, how fast
/// online songs opened). No keys, accounts or addresses.
#[tauri::command]
fn diagnostics() -> String {
    let l = engine::settings::load().listening;
    let region = media_access::catalog_region();
    let log = std::fs::read_to_string(paths::app_data_dir().join("logs").join("playback.log"))
        .unwrap_or_default();
    let tail: Vec<&str> = log.lines().rev().take(40).collect();
    let mut out = format!(
        "Venuta {}\n{} {}\nRegion: {} (set: {})\nStream quality: {}\nAutoplay: {}, history: {}, hide videos: {}\nLyrics sources: {}\nData folder: {}\n\nLast of the playback log:\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        region.country,
        if l.catalog_region.is_empty() {
            "automatic"
        } else {
            &l.catalog_region
        },
        l.stream_quality,
        l.autoplay,
        l.history,
        l.hide_videos,
        if l.lyrics_sources.is_empty() {
            "usual order".to_owned()
        } else {
            l.lyrics_sources.join(", ")
        },
        paths::app_data_dir().display(),
    );
    for line in tail.into_iter().rev() {
        out.push_str(line);
        out.push('\n');
    }
    out
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

/// Whether `root` is your library (or inside it) or the folder you opened.
fn own_folder(root: &str) -> bool {
    let Ok(r) = Path::new(root).canonicalize() else {
        return false;
    };
    let inside = |p: Option<PathBuf>| {
        p.and_then(|p| p.canonicalize().ok())
            .is_some_and(|p| r.starts_with(p))
    };
    inside(paths::home()) || inside(settings_with_defaults().open_folder.map(PathBuf::from))
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
    // Clips on question cards play your own files, so the music folder may be read: your
    // library, or the folder you opened, never one the window merely names.
    if own_folder(&root) {
        let _ = app.asset_protocol_scope().allow_directory(&root, true);
    }
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
    /// "edit": tags or ratings changed in place; "lyrics": a song's timing saved as its .lrc.
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
            let kind = if name.ends_with("-edit.jsonl") {
                "edit"
            } else if name.ends_with("-lyrics.jsonl") {
                "lyrics"
            } else {
                return None;
            };
            let text = std::fs::read_to_string(e.path()).ok()?;
            let files: Vec<String> = text
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v.get("file")?.as_str().map(str::to_owned))
                .collect();
            Some(Change {
                kind,
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

/// Edits and undos wait while a build or a tidy is moving files: a tag written to a file
/// that is mid-move would leave two copies.
fn files_settled(jobs: &Jobs) -> CmdResult<()> {
    match jobs.apply.lock() {
        Ok(slot) if slot.is_some() => {
            Err("Venuta is moving files right now. Try again when that's done.".into())
        }
        _ => Ok(()),
    }
}

/// A journal Venuta wrote: a file directly in its own journal folder. Undo replays a journal's
/// entries, so it never takes one from anywhere else.
fn own_journal(journal: &str) -> CmdResult<PathBuf> {
    let dir = engine::apply::journal_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = Path::new(journal)
        .canonicalize()
        .map_err(|_| "That change's record is gone.".to_string())?;
    if path.parent() != Some(dir.as_path()) {
        return Err("That record isn't one of Venuta's.".into());
    }
    Ok(path)
}

#[tauri::command]
async fn undo_edit(journal: String, jobs: State<'_, Jobs>) -> CmdResult<usize> {
    files_settled(&jobs)?;
    blocking(move || {
        let journal_path = own_journal(&journal)?;
        let path = journal_path.as_path();
        if !journal.ends_with("-lyrics.jsonl") {
            return engine::edit::undo_edits(path);
        }
        // A timing: the old .lrc back (or none), and the library read again so Health knows.
        let root = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| {
                let line = t.lines().next()?.to_owned();
                serde_json::from_str::<engine::lrc::LyricsEntry>(&line).ok()
            })
            .map(|e| PathBuf::from(e.root));
        let n = engine::lrc::undo(path)?;
        if let Some(root) = root {
            let mut index = open_index(&root)?;
            libcache::scan(&mut index, &root)?;
        }
        Ok(n)
    })
    .await
}

/// Save lyrics timed by tapping along as the song's .lrc (`lines`: ms and words), undoable in
/// Activity (returns the journal, for Undo). The song's own file is never touched.
#[tauri::command]
async fn save_synced_lyrics(
    root: String,
    path: String,
    lines: Vec<(u64, String)>,
    jobs: State<'_, Jobs>,
) -> CmdResult<String> {
    files_settled(&jobs)?;
    blocking(move || {
        let root = PathBuf::from(root);
        let journal = engine::lrc::save(&root, &path, &lines)?;
        let mut index = open_index(&root)?;
        libcache::scan(&mut index, &root)?;
        Ok(journal.to_string_lossy().into_owned())
    })
    .await
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
async fn undo_run(journal: String, jobs: State<'_, Jobs>) -> CmdResult<usize> {
    files_settled(&jobs)?;
    blocking(move || engine::apply::undo(&own_journal(&journal)?)).await
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
        let lib = libcache::indexed(&index, Path::new(&root))?;
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
                        rg: gain_of(t),
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

/// The saved queue's levelling values as they are now (songs measured since it was saved).
pub(crate) fn fresh_gains(
    root: &str,
    mut items: Vec<playback::QueueItem>,
) -> Vec<playback::QueueItem> {
    if let Ok(index) = open_index(Path::new(root))
        && let Ok(lib) = libcache::indexed(&index, Path::new(root))
    {
        let by: std::collections::HashMap<&str, &engine::library::Track> =
            lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
        for it in &mut items {
            if let Some(t) = by.get(it.path.as_str()) {
                it.rg = gain_of(t);
            }
        }
    }
    items
}

/// A song's volume levelling: its ReplayGain tags, else Venuta's own loudness measurement
/// (the same ReplayGain 2.0 reference), so levelling works without writing to the files.
fn gain_of(t: &engine::library::Track) -> player::dsp::Gain {
    let q = t.quality.as_ref();
    let db = |lufs: Option<f64>| {
        lufs.filter(|l| l.is_finite())
            .map(|l| engine::quality::replaygain_db(l) as f32)
    };
    let peak = |p: Option<f64>| p.filter(|p| p.is_finite() && *p > 0.0).map(|p| p as f32);
    player::dsp::Gain {
        track_db: tag_number(t.first("REPLAYGAIN_TRACK_GAIN"))
            .or_else(|| db(q.and_then(|q| q.lufs))),
        album_db: tag_number(t.first("REPLAYGAIN_ALBUM_GAIN"))
            .or_else(|| db(q.and_then(|q| q.album_lufs))),
        track_peak: tag_number(t.first("REPLAYGAIN_TRACK_PEAK"))
            .or_else(|| peak(q.and_then(|q| q.peak))),
        album_peak: tag_number(t.first("REPLAYGAIN_ALBUM_PEAK"))
            .or_else(|| peak(q.and_then(|q| q.album_peak))),
    }
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
        libcache::scan(&mut index, &root)?;
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
        libcache::scan(&mut index, &root)?;
        if let Ok(lib) = libcache::indexed(&index, &root) {
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
async fn tidy_undo(root: String, journal: String, jobs: State<'_, Jobs>) -> CmdResult<usize> {
    files_settled(&jobs)?;
    blocking(move || {
        let n = engine::tidy::undo(&own_journal(&journal)?)?;
        let root = PathBuf::from(root);
        let mut index = open_index(&root)?;
        libcache::scan(&mut index, &root)?;
        if let Ok(lib) = libcache::indexed(&index, &root) {
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

/// Set a card up, or change what it carries and how (see `engine::sync::CardSettings`).
#[tauri::command]
fn setup_card(
    card: String,
    settings: engine::sync::CardSettings,
) -> CmdResult<engine::sync::CardConfig> {
    engine::sync::setup_card(Path::new(&card), settings)
}

/// What a card's selection can be made of: playlists, albums and artists, with their sizes.
#[tauri::command]
async fn card_choices(root: String) -> CmdResult<engine::sync::Choices> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let index = open_index(&rootp)?;
        engine::sync::choices(&index, &rootp).map_err(|e| e.to_string())
    })
    .await
}

/// What a card would carry with these settings (songs, size, room), before saving anything.
#[tauri::command]
async fn card_estimate(
    root: String,
    card: String,
    settings: engine::sync::CardSettings,
) -> CmdResult<engine::sync::Estimate> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let index = open_index(&rootp)?;
        engine::sync::estimate(&index, &rootp, Path::new(&card), &settings)
    })
    .await
}

/// A card whose settings were copied from another card: give it its own identity.
#[tauri::command]
fn reidentify_card(card: String) -> CmdResult<engine::sync::CardConfig> {
    engine::sync::reidentify(Path::new(&card))
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
        libcache::scan(&mut index, &rootp)?;
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
        libcache::scan(&mut index, &rootp)?;
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

fn load_lib(root: &Path) -> CmdResult<(Index, Arc<Library>)> {
    libcache::fresh(root)
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

/// "To get": albums the playlists want now, then the ones kept on the list by themselves
/// (better copies you asked for, an older Apple Music import). The same album shows once.
fn wanted_all(root: &Path, lib: &Library) -> Vec<engine::playlists::WantedAlbum> {
    let mut list = engine::playlists::wanted_now(root, lib);
    for mut w in load_wanted() {
        match list
            .iter_mut()
            .find(|x| x.album == w.album && x.artist == w.artist)
        {
            Some(x) => {
                for p in w.playlists {
                    if !x.playlists.contains(&p) {
                        x.playlists.push(p);
                    }
                }
                x.tier = x.tier.min(w.tier);
                x.kept = true;
            }
            None => {
                w.kept = true;
                list.push(w)
            }
        }
    }
    list.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then(b.saved_tracks.cmp(&a.saved_tracks))
    });
    list
}

/// Take an album off "To get" (one kept on the list by itself).
#[tauri::command]
fn want_remove(album: String, artist: String) -> CmdResult<()> {
    let list: Vec<_> = load_wanted()
        .into_iter()
        .filter(|w| !(w.album == album && w.artist == artist))
        .collect();
    std::fs::write(
        wantlist_path(),
        serde_json::to_vec(&list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// "To get" as a spreadsheet (CSV), where you choose. Returns how many albums it lists.
#[tauri::command]
async fn wanted_export(root: String, to: String) -> CmdResult<usize> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let list = wanted_all(&rootp, &lib);
        std::fs::write(&to, engine::playlists::wanted_csv(&list)).map_err(|e| e.to_string())?;
        Ok(list.len())
    })
    .await
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
                kept: true,
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
    /// Up to four album folders each smart rule starts with, for its cover (before a file exists).
    smart_dirs: BTreeMap<String, Vec<String>>,
    /// Where playlists are saved (the folder may not exist yet).
    folder: String,
    wanted: Vec<engine::playlists::WantedAlbum>,
    apple_ready: bool,
}

/// The playlists as they are saved, for a shelf on the Music home: no rescan and nothing
/// rewritten, so it is quick (Playlists itself does the repairs).
#[tauri::command]
async fn playlist_shelf(root: String) -> CmdResult<Vec<engine::playlists::PlaylistSummary>> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let index = open_index(&rootp)?;
        let lib = libcache::indexed(&index, &rootp)?;
        Ok(engine::playlists::list(&rootp, &lib))
    })
    .await
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
            // Songs a playlist was waiting for that have since come into the library.
            engine::imports::refind_all(&rootp, &lib);
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
            smart_dirs: evaluated
                .iter()
                .map(|(r, paths)| {
                    let mut dirs: Vec<String> = Vec::new();
                    for p in paths {
                        let d = p
                            .rsplit_once('/')
                            .map(|(d, _)| d.to_owned())
                            .unwrap_or_default();
                        if !dirs.contains(&d) {
                            dirs.push(d);
                        }
                        if dirs.len() == 4 {
                            break;
                        }
                    }
                    (r.name.clone(), dirs)
                })
                .collect(),
            smart: evaluated.into_iter().map(|(r, _)| r).collect(),
            folder: folder.to_string_lossy().into_owned(),
            wanted: wanted_all(&rootp, &lib),
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
        let (mut entries, _) = engine::playlists::read(&rootp, &lib, &name)?;
        // Songs it waits for that may be in your library under another name or version.
        engine::playlists::suggest(&lib, &mut entries);
        Ok(entries)
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

/// Delete a playlist (its file goes to Set aside); returns what Undo needs to put it back.
#[tauri::command]
fn playlist_delete(root: String, name: String) -> CmdResult<engine::playlists::Deleted> {
    engine::playlists::delete(Path::new(&root), &name)
}

/// Undo a delete: the playlist's file comes back from Set aside, as it was.
#[tauri::command]
fn playlist_undelete(root: String, name: String, aside: String, was: String) -> CmdResult<()> {
    engine::playlists::undelete(Path::new(&root), &name, Path::new(&aside), Path::new(&was))
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

/// Save a playlist exactly as shown: songs in the library and songs still wanted, in order
/// (its source line stays).
#[tauri::command]
async fn playlist_save_items(
    root: String,
    name: String,
    items: Vec<engine::playlists::Item>,
) -> CmdResult<()> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let source = engine::playlists::source_of(&rootp, &name);
        engine::playlists::write_items(&rootp, &lib, &name, &items, false, source.as_deref())
    })
    .await
}

/// Make a new, empty playlist (a draft named, or given its first song); never over one there.
#[tauri::command]
async fn playlist_create(root: String, name: String) -> CmdResult<()> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::playlists::create(&rootp, &lib, &name)
    })
    .await
}

/// Add songs to a playlist (making it if it's new), skipping ones already in it: library songs
/// by path, online songs by their stable id. Returns how many were added.
#[tauri::command]
async fn playlist_add(
    root: String,
    name: String,
    items: Vec<engine::playlists::Item>,
    // Add them even when they are in it already (asked for, after "already in").
    again: Option<bool>,
) -> CmdResult<usize> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::playlists::add_with(&rootp, &lib, &name, &items, again.unwrap_or(false))
    })
    .await
}

/// What is liked: your files by path and online songs by id (and the liked list's name).
#[tauri::command]
async fn liked_songs(root: String) -> CmdResult<engine::playlists::LikedSet> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        Ok(engine::playlists::liked_set(&rootp, &lib))
    })
    .await
}

/// Like a song (on top of your Liked songs) or take it out; returns what is liked now.
#[tauri::command]
async fn like_song(
    root: String,
    item: engine::playlists::Item,
    liked: bool,
) -> CmdResult<engine::playlists::LikedSet> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        if liked {
            engine::playlists::like(&rootp, &lib, &item)?;
        } else {
            engine::playlists::unlike(&rootp, &lib, &item)?;
        }
        Ok(engine::playlists::liked_set(&rootp, &lib))
    })
    .await
}

/// A playlist as the player gets it, in order: each song from your file when it's here, else
/// online; songs still to get are left out (see `engine::playlists::playables`).
#[tauri::command]
async fn playlist_queue(root: String, name: String) -> CmdResult<Vec<media_access::Playable>> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let (entries, _) = engine::playlists::read(&rootp, &lib, &name)?;
        Ok(engine::playlists::playables(&rootp, &entries))
    })
    .await
}

/// A copy of a playlist; returns the name it got.
#[tauri::command]
async fn playlist_duplicate(root: String, name: String) -> CmdResult<String> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::playlists::duplicate(&rootp, &lib, &name, &format!("{name} copy"))
    })
    .await
}

/// Save a playlist for other apps (full paths); returns how many songs it holds.
#[tauri::command]
async fn playlist_export(root: String, name: String, to: String) -> CmdResult<usize> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::playlists::export(&rootp, &lib, &name, Path::new(&to))
    })
    .await
}

// --- Playlists out ----------------------------------------------------------------------------

/// Export playlists in any format and path style. One playlist: `to` is the file. Several: `to`
/// is a folder, each written as "<name>.<ext>". Returns the files written.
#[tauri::command]
async fn playlists_export(
    root: String,
    names: Vec<String>,
    to: String,
    options: engine::exports::Options,
) -> CmdResult<Vec<String>> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let to = PathBuf::from(&to);
        // Paths for a phone or a server start inside the Music folder.
        let music = Some(paths::music_dir(&rootp))
            .filter(|m| m.is_dir())
            .unwrap_or_else(|| rootp.clone());
        let mut written = Vec::new();
        for name in &names {
            let file = if names.len() == 1 && !to.is_dir() {
                to.clone()
            } else {
                to.join(format!(
                    "{}.{}",
                    engine::plan::sanitize(name, 100),
                    options.format.extension()
                ))
            };
            let songs = engine::exports::songs_of(&rootp, &lib, name)?;
            let saved_in = file.parent().unwrap_or(Path::new("."));
            let text = engine::exports::render(name, &songs, &options, &music, saved_in);
            engine::exports::write(&file, &text)?;
            written.push(file.to_string_lossy().replace('\\', "/"));
        }
        Ok(written)
    })
    .await
}

/// A playlist as text to copy: a plain list of songs, or any other format.
#[tauri::command]
async fn playlist_text(
    root: String,
    name: String,
    format: engine::exports::Format,
) -> CmdResult<String> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let songs = engine::exports::songs_of(&rootp, &lib, &name)?;
        let options = engine::exports::Options {
            format,
            style: engine::exports::PathStyle::ThisPc,
            include_missing: true,
        };
        Ok(engine::exports::render(
            &name, &songs, &options, &rootp, &rootp,
        ))
    })
    .await
}

/// Every playlist, the To get list and the smart playlists' rules in one zip. Bringing the zip
/// in puts the playlists back.
#[tauri::command]
async fn playlists_backup(root: String, to: String) -> CmdResult<usize> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::exports::backup(&rootp, &lib, &rules_dir(), Path::new(&to))
    })
    .await
}

/// A playlist's songs found on a service, free: Spotify links to paste into Spotify's app, or a
/// page of Apple Music or Deezer links (saved, its path returned) to add by hand.
#[derive(Serialize)]
struct OnServiceOut {
    total: usize,
    found: usize,
    /// The places of songs the service didn't show.
    missing: Vec<usize>,
    /// Spotify: one link a line, to paste.
    paste: String,
    /// A page listing every song with its link there, to open in the browser.
    page: String,
}

#[tauri::command]
async fn playlist_on_service(
    root: String,
    name: String,
    service: engine::bridges::Service,
    country: Option<String>,
    on_progress: Channel<DeepProgress>,
) -> CmdResult<OnServiceOut> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let songs = engine::exports::songs_of(&rootp, &lib, &name)?;
        let http = engine::net::Http::open()?;
        let country = country.unwrap_or_default();
        let found =
            engine::exports::on_service(&http, service, &songs, &country, &mut |done, total| {
                let _ = on_progress.send(DeepProgress {
                    done,
                    total,
                    note: String::new(),
                });
            });
        let label = match service {
            engine::bridges::Service::Spotify => "Spotify",
            engine::bridges::Service::AppleMusic => "Apple Music",
            engine::bridges::Service::Deezer => "Deezer",
        };
        let dir = paths::app_data_dir().join("exports");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let page = dir.join(format!(
            "{} on {label}.html",
            engine::plan::sanitize(&name, 100)
        ));
        engine::exports::write(
            &page,
            &engine::exports::links_page(&name, label, &songs, &found),
        )?;
        Ok(OnServiceOut {
            total: songs.len(),
            found: found.found.len(),
            missing: found.missing.clone(),
            paste: engine::exports::spotify_paste(&found),
            page: page.to_string_lossy().replace('\\', "/"),
        })
    })
    .await
}

/// A playlist as YouTube links of up to 50 songs each (YouTube's limit), to save on YouTube
/// Music without signing in to Venuta. Songs found through Venuta's catalogue.
#[derive(Serialize)]
struct YouTubeLinks {
    links: Vec<String>,
    found: usize,
    total: usize,
}

#[tauri::command]
async fn playlist_youtube_links(
    root: String,
    name: String,
    on_progress: Channel<DeepProgress>,
    pb: State<'_, playback::Playback>,
) -> CmdResult<YouTubeLinks> {
    let catalog = pb.catalog();
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let songs = engine::exports::songs_of(&rootp, &lib, &name)?;
        let cancel = media_access::CancellationToken::default();
        let mut ids = Vec::new();
        for (i, s) in songs.iter().enumerate() {
            let id = match &s.online {
                Some(o) if o.provider == "remote_catalog" => Some(o.track_id.clone()),
                _ => engine::imports::catalog_online_for(
                    catalog.as_ref(),
                    &engine::playlists::Foreign {
                        title: s.title.clone(),
                        artist: s.artist.clone(),
                        album: s.album.clone(),
                        duration_ms: s.duration_ms,
                        isrc: s.isrc.clone(),
                        ..Default::default()
                    },
                    &cancel,
                )
                .map(|o| o.track_id),
            };
            ids.extend(id);
            let _ = on_progress.send(DeepProgress {
                done: i + 1,
                total: songs.len(),
                note: String::new(),
            });
        }
        Ok(YouTubeLinks {
            links: engine::exports::youtube_lists(&ids),
            found: ids.len(),
            total: songs.len(),
        })
    })
    .await
}

/// Look for a playlist's missing songs again (after new music came in).
#[tauri::command]
async fn playlist_refind(root: String, name: String) -> CmdResult<usize> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        engine::imports::refind(&rootp, &lib, &name)
    })
    .await
}

type AppleLists = (Vec<engine::imports::ForeignPlaylist>, Vec<String>);

/// Apple Music's library as last read, and what it was read from: the file (with its size and
/// time, so a change in Apple Music is seen) and the key.
struct AppleRead {
    file: PathBuf,
    size: u64,
    modified: Option<std::time::SystemTime>,
    key: blake3::Hash,
    lists: AppleLists,
}

static APPLE: std::sync::LazyLock<Mutex<Option<AppleRead>>> =
    std::sync::LazyLock::new(Default::default);

/// Your Apple Music library's playlists (with your key), and what couldn't be read. Decrypting
/// the library takes a while, so it is read once and again only when its file changes.
fn apple_lists() -> CmdResult<AppleLists> {
    let s = settings_with_defaults();
    let key = s
        .apple_key
        .ok_or("Enter your Apple Music library key in Settings, Online sources, first.")?;
    let file = s
        .apple_library
        .map(PathBuf::from)
        .or_else(engine::apple::default_library_path)
        .ok_or("Couldn't find your Apple Music library on this computer.")?;
    let meta = std::fs::metadata(&file).map_err(|e| e.to_string())?;
    let (size, modified, key_hash) = (
        meta.len(),
        meta.modified().ok(),
        blake3::hash(key.as_bytes()),
    );
    let mut kept = APPLE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(k) = kept.as_ref()
        && k.file == file
        && k.size == size
        && k.modified == modified
        && k.key == key_hash
    {
        return Ok(k.lists.clone());
    }
    let apple = engine::apple::read(&file, &key)?;
    let lists = engine::imports::apple_library(&apple);
    *kept = Some(AppleRead {
        file,
        size,
        modified,
        key: key_hash,
        lists: lists.clone(),
    });
    Ok(lists)
}

/// Playlists you have fewer songs of than the source still has (an older import kept only the
/// songs it matched), and the source's playlists you never brought over. Only sources read on
/// this computer are checked (Apple Music's library); nothing is written.
#[tauri::command]
async fn playlist_sources_check(root: String) -> CmdResult<Vec<engine::imports::Behind>> {
    blocking(move || {
        let s = settings_with_defaults();
        if s.apple_key.is_none() {
            return Ok(vec![]);
        }
        let Ok((lists, _)) = apple_lists() else {
            return Ok(vec![]);
        };
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        Ok(engine::imports::behind(&rootp, &lib, &lists))
    })
    .await
}

/// Read a playlist's source again, to update the playlist from it (the review shows what would
/// change; nothing is written yet).
#[tauri::command]
async fn playlist_update_read(
    root: String,
    name: String,
    acc: State<'_, accounts::Accounts>,
    pb: State<'_, playback::Playback>,
) -> CmdResult<ImportRead> {
    let acc = acc.inner().clone();
    let catalog = pb.catalog();
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let header = engine::playlists::header_of(&rootp, &name);
        let id = header.source_id.ok_or_else(|| {
            format!("Venuta doesn't know where “{name}” came from. Import it again, and choose to update this playlist in the review.")
        })?;
        let (lists, problems) = read_source(&acc, catalog.as_ref(), &name, &id)?;
        let (_, lib) = load_lib(&rootp)?;
        Ok(ImportRead {
            lists: preview_online(&rootp, &lib, &lists, catalog.as_ref(), None),
            problems,
        })
    })
    .await
}

/// Read a playlist again from where it came from (`source_id`): an account, a share link, the
/// Apple Music library or a file. Songs copied from an app or a page can't be read again.
fn read_source(
    acc: &accounts::Accounts,
    catalog: &dyn media_access::MediaAccess,
    name: &str,
    id: &str,
) -> Result<(Vec<engine::imports::ForeignPlaylist>, Vec<String>), String> {
    use engine::links::Link;
    let (kind, rest) = id.split_once(':').unwrap_or(("", ""));
    let link = |l: Link| -> Result<(Vec<engine::imports::ForeignPlaylist>, Vec<String>), String> {
        let http = engine::net::Http::open()?;
        let (p, note) = engine::links::read(&http, &l)?;
        Ok((vec![p], note.into_iter().collect()))
    };
    let (lists, problems) = match kind {
        // Signed in to Spotify: its account; otherwise its share page (public playlists).
        "spotify" => match accounts::read_one(acc, rest, name) {
            Ok(l) => (l, vec![]),
            Err(_) => link(Link::Spotify(rest.to_owned()))?,
        },
        "applemusic" => {
            let url = format!("https://music.apple.com/us/playlist/x/{rest}");
            link(Link::AppleMusic(url))?
        }
        "tidal" => link(Link::Tidal(rest.to_owned()))?,
        "jiosaavn" => link(Link::JioSaavn(rest.to_owned()))?,
        "deezer" => link(Link::Deezer(rest.to_owned()))?,
        "deezer-favourites" => link(Link::DeezerFavourites(rest.to_owned()))?,
        "listenbrainz" => link(Link::ListenBrainz(rest.to_owned()))?,
        "youtube" => (vec![youtube_playlist(catalog, rest)?], vec![]),
        "apple" => {
            let (all, p) = apple_lists()?;
            (
                all.into_iter()
                    .filter(|l| l.id.as_deref() == Some(id))
                    .collect(),
                p,
            )
        }
        "file" => {
            let (path, list) = rest.rsplit_once('|').unwrap_or((rest, ""));
            if !Path::new(path).is_file() {
                return Err(format!(
                    "The file “{name}” came from is gone ({path}). Export it again and import that file; the review offers to update this playlist."
                ));
            }
            let mut all = engine::imports::read_file(Path::new(path))?;
            if all.len() > 1 {
                all.retain(|l| l.name == list);
            }
            (all, vec![])
        }
        _ => {
            return Err(format!(
                "“{name}” came from songs you copied, so there's nothing to read again. Copy them again and paste them in Import & export to update it."
            ));
        }
    };
    if lists.is_empty() {
        return Err(format!("“{name}” isn't at its source any more."));
    }
    Ok((lists, problems))
}

/// Every playlist that came from somewhere Venuta can read again, read again: the ones whose
/// source has new songs or dropped some, ready to review and update together.
#[tauri::command]
async fn playlists_check(
    root: String,
    on_progress: Channel<DeepProgress>,
    acc: State<'_, accounts::Accounts>,
    pb: State<'_, playback::Playback>,
) -> CmdResult<ImportRead> {
    let acc = acc.inner().clone();
    let catalog = pb.catalog();
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let sources: Vec<(String, String)> = engine::playlists::list(&rootp, &lib)
            .into_iter()
            .filter(|p| !p.smart)
            .filter_map(|p| {
                let id = engine::playlists::header_of(&rootp, &p.name).source_id?;
                Some((p.name, id))
            })
            .collect();
        let mut lists = Vec::new();
        let mut problems = Vec::new();
        for (i, (name, id)) in sources.iter().enumerate() {
            let _ = on_progress.send(DeepProgress {
                done: i,
                total: sources.len(),
                note: format!("Checking “{name}”"),
            });
            match read_source(&acc, catalog.as_ref(), name, id) {
                Ok((found, mut p)) => {
                    lists.extend(found);
                    problems.append(&mut p);
                }
                Err(e) => problems.push(e),
            }
        }
        let _ = on_progress.send(DeepProgress {
            done: sources.len(),
            total: sources.len(),
            note: String::new(),
        });
        // The same note for several playlists ("enter your Apple Music key") said once.
        let mut said: Vec<(String, usize)> = Vec::new();
        for p in problems {
            match said.iter_mut().find(|(m, _)| *m == p) {
                Some((_, n)) => *n += 1,
                None => said.push((p, 1)),
            }
        }
        let problems: Vec<String> = said
            .into_iter()
            .map(|(m, n)| {
                if n > 1 {
                    format!("{m} ({n} playlists)")
                } else {
                    m
                }
            })
            .collect();
        // Only the ones with something new or gone at the source; then their songs you don't
        // have are looked for online (not every playlist's, every time).
        let changed: Vec<_> = engine::imports::preview(&rootp, &lib, &lists)
            .into_iter()
            .filter(|l| l.target.as_ref().is_some_and(|t| t.added + t.removed > 0))
            .map(|l| (l.name, l.source_id))
            .collect();
        lists.retain(|l| changed.iter().any(|(n, id)| *n == l.name && *id == l.id));
        let preview = preview_online(&rootp, &lib, &lists, catalog.as_ref(), Some(&on_progress));
        Ok(ImportRead {
            lists: preview,
            problems,
        })
    })
    .await
}

/// Look a song up on the web, in your browser (Venuta itself never downloads music). The
/// address is made here from the words, so the page can only ever ask for a search.
#[tauri::command]
fn search_web(app: AppHandle, words: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let q: String = words
        .chars()
        .filter(|c| !c.is_control())
        .take(300)
        .collect();
    if q.trim().is_empty() {
        return Ok(());
    }
    let url = format!(
        "https://www.google.com/search?q={}",
        urlencoding::encode(q.trim())
    );
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Sites Import & export sends you to: services' own pages and the free transfer tools. Nothing
/// else is ever opened from here.
const SEND_HOSTS: &[&str] = &[
    "www.youtube.com",
    "music.youtube.com",
    "open.spotify.com",
    "www.spotify.com",
    "music.apple.com",
    "www.deezer.com",
    "www.tunemymusic.com",
    "soundiiz.com",
    "freeyourmusic.com",
    "takeout.google.com",
    "exportify.app",
    "privacy.apple.com",
    "listen.tidal.com",
    "www.jiosaavn.com",
];

/// Open a link Import & export made (a YouTube list of songs, a service's page, a free transfer
/// tool), in your browser. Only https, only those sites.
#[tauri::command]
fn open_send_link(app: AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let host = url
        .strip_prefix("https://")
        .and_then(|r| r.split(['/', '?', '#']).next())
        .unwrap_or_default();
    if !SEND_HOSTS.contains(&host) {
        return Err("Venuta only opens music services' pages from here.".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Open a page of links Import & export saved (in Venuta's own exports folder), in your browser.
#[tauri::command]
fn open_saved_page(app: AppHandle, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = paths::app_data_dir().join("exports");
    let file = PathBuf::from(&path);
    let (Ok(d), Ok(f)) = (dir.canonicalize(), file.canonicalize()) else {
        return Err("That page isn't there any more.".into());
    };
    if !f.starts_with(&d) || f.extension().is_none_or(|e| e != "html") {
        return Err("Venuta only opens its own pages from here.".into());
    }
    app.opener()
        .open_path(f.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Show a song in File Explorer, selected. Only a file inside the open folder.
#[tauri::command]
fn show_in_folder(app: AppHandle, root: String, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let rootp = PathBuf::from(&root);
    let file = rootp.join(&path);
    let (Ok(r), Ok(f)) = (rootp.canonicalize(), file.canonicalize()) else {
        return Err("That song isn't in its folder any more.".into());
    };
    if !f.starts_with(&r) {
        return Err("That file is outside the library.".into());
    }
    app.opener()
        .reveal_item_in_dir(&file)
        .map_err(|e| e.to_string())
}

/// Put back how a playlist was before an update (the copy kept in Set aside).
#[tauri::command]
fn playlist_restore(root: String, name: String, copy: String) -> CmdResult<()> {
    engine::playlists::restore_copy(Path::new(&root), &name, Path::new(&copy))
}

/// What an import read: the playlists, and what couldn't be read (one file of three, say).
#[derive(serde::Serialize)]
struct ImportRead {
    lists: Vec<engine::imports::PreviewPlaylist>,
    problems: Vec<String>,
}

/// Read playlists from files other apps made (any format, folders and zips too), a share link
/// (Spotify, Apple Music, YouTube Music, TIDAL, JioSaavn, Deezer, ListenBrainz), or the Apple
/// Music library, and match every song. Nothing is written yet.
#[tauri::command]
async fn import_preview(
    root: String,
    files: Vec<String>,
    link: Option<String>,
    text: Option<String>,
    apple: bool,
    on_progress: Channel<DeepProgress>,
    pb: tauri::State<'_, playback::Playback>,
) -> CmdResult<ImportRead> {
    let catalog = pb.catalog();
    blocking(move || {
        let mut lists: Vec<engine::imports::ForeignPlaylist> = Vec::new();
        let mut problems: Vec<String> = Vec::new();
        for f in &files {
            match engine::imports::read_file(Path::new(f)) {
                Ok(mut l) => lists.append(&mut l),
                Err(e) => problems.push(e),
            }
        }
        // Pasted text: playlist links, songs copied from Spotify's app, or a list of songs.
        let mut links: Vec<String> = link.into_iter().collect();
        if let Some(text) = text.as_deref().filter(|t| !t.trim().is_empty()) {
            let (pasted_links, mut pasted_lists) = engine::links::pasted(text);
            if pasted_links.is_empty() && pasted_lists.is_empty() {
                problems.push("No songs or playlist links found in what was pasted.".into());
            }
            links.extend(pasted_links);
            lists.append(&mut pasted_lists);
        }
        let needs_web = !links.is_empty()
            || lists.iter().any(|l| {
                l.tracks
                    .iter()
                    .any(|t| t.youtube.is_some() || t.spotify.is_some())
            });
        if needs_web {
            let http = engine::net::Http::open()?;
            for link in links.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
                use engine::links::Link;
                match engine::links::parse(link) {
                    Some(Link::YouTube(id)) => match youtube_playlist(catalog.as_ref(), &id) {
                        Ok(p) => lists.push(p),
                        Err(e) => problems.push(e),
                    },
                    Some(l) => match engine::links::read(&http, &l) {
                        Ok((p, note)) => {
                            lists.push(p);
                            problems.extend(note);
                        }
                        Err(e) => problems.push(e),
                    },
                    None => problems.push(
                        "That isn't a playlist link Venuta can read. It reads links from Spotify, Apple Music, YouTube Music, TIDAL, JioSaavn, Deezer and ListenBrainz."
                            .into(),
                    ),
                }
            }
            // Naming songs that came as ids is the slow part: said as it goes.
            let progress = &on_progress;
            let tell = |what: &str| {
                let what = what.to_owned();
                move |done: usize, total: usize| {
                    let _ = progress.send(DeepProgress {
                        done,
                        total,
                        note: what.clone(),
                    });
                }
            };
            engine::imports::youtube_titles(&http, &mut lists, &mut tell("Asking YouTube for song names"));
            engine::links::spotify_names(&http, &mut lists, &mut tell("Asking Spotify for song names"));
        }
        if apple {
            let (mut l, mut p) = apple_lists()?;
            lists.append(&mut l);
            problems.append(&mut p);
        }
        if lists.is_empty() {
            return Err(if problems.is_empty() {
                "No playlists found.".into()
            } else {
                problems.join("\n")
            });
        }
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        Ok(ImportRead {
            lists: preview_online(&rootp, &lib, &lists, catalog.as_ref(), Some(&on_progress)),
            problems,
        })
    })
    .await
}

/// [`engine::imports::preview`], with every song you don't have looked for in the online
/// catalog, so the playlist plays through without a gap. Said as it goes, when asked.
fn preview_online(
    root: &Path,
    lib: &engine::library::Library,
    lists: &[engine::imports::ForeignPlaylist],
    catalog: &dyn media_access::MediaAccess,
    progress: Option<&Channel<DeepProgress>>,
) -> Vec<engine::imports::PreviewPlaylist> {
    engine::imports::preview_with(root, lib, lists, Some(catalog), &|done, total| {
        if let Some(p) = progress {
            let _ = p.send(DeepProgress {
                done,
                total,
                note: "Finding the songs you don't have online".into(),
            });
        }
    })
}

/// The online catalog's song for each of these (or none when it isn't sure): for a playlist's
/// songs you don't have, so they play online. Ids and what to show only, never a stream address.
#[tauri::command]
async fn playlist_find_online(
    songs: Vec<engine::playlists::Foreign>,
    on_progress: Channel<DeepProgress>,
    pb: tauri::State<'_, playback::Playback>,
) -> CmdResult<Vec<Option<engine::playlists::OnlineEntry>>> {
    let catalog = pb.catalog();
    blocking(move || {
        Ok(engine::imports::find_online(
            catalog.as_ref(),
            &songs,
            &|done, total| {
                let _ = on_progress.send(DeepProgress {
                    done,
                    total,
                    note: String::new(),
                });
            },
        ))
    })
    .await
}

#[derive(Serialize)]
struct ImportDone {
    written: Vec<engine::imports::Written>,
    /// Albums now on the want-list because of these playlists.
    wanted: usize,
}

/// Write the playlists you confirmed; the albums they still want go on the want-list.
/// A YouTube or YouTube Music playlist, through Venuta's catalogue (free, no sign-in): every
/// page of it, with its name.
fn youtube_playlist(
    catalog: &dyn media_access::MediaAccess,
    id: &str,
) -> Result<engine::imports::ForeignPlaylist, String> {
    let list = catalog
        .playlist(id, &media_access::CancellationToken::default())
        .map_err(|e| {
            format!("YouTube didn't show that playlist ({e}). Is it public or unlisted?")
        })?;
    if list.tracks.is_empty() {
        return Err("YouTube shows no songs in that playlist. Is it public or unlisted?".into());
    }
    Ok(engine::imports::ForeignPlaylist {
        name: list
            .title
            .unwrap_or_else(|| "YouTube Music playlist".into()),
        source: "YouTube Music".into(),
        id: Some(format!("youtube:{id}")),
        tracks: list
            .tracks
            .into_iter()
            .map(|t| engine::playlists::Foreign {
                title: t.title,
                artist: t.artists.join(", "),
                album: t.album.unwrap_or_default(),
                duration_ms: t.duration_ms,
                youtube: Some(t.track_id),
                ..Default::default()
            })
            .collect(),
    })
}

#[tauri::command]
async fn import_apply(
    root: String,
    choices: Vec<engine::imports::Choice>,
) -> CmdResult<ImportDone> {
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let (_, lib) = load_lib(&rootp)?;
        let today = chrono_today();
        // The wanted songs live in the playlists themselves; "To get" reads them from there.
        let (written, fresh) = engine::imports::apply(&rootp, &lib, &choices, &today)?;
        Ok(ImportDone {
            written,
            wanted: fresh.len(),
        })
    })
    .await
}

/// Today as YYYY-MM-DD (for "Imported from Spotify 2026-10-13").
fn chrono_today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

/// Rate the songs of one playlist (e.g. Apple's Favourite Songs) that have no rating yet.
#[tauri::command]
async fn rate_playlist(
    root: String,
    name: String,
    stars: u8,
    jobs: State<'_, Jobs>,
) -> CmdResult<engine::edit::EditReport> {
    files_settled(&jobs)?;
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
        libcache::scan(&mut index, &rootp)?;
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
        let lib = libcache::indexed(&index, Path::new(&root))?;
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
    jobs: State<'_, Jobs>,
) -> CmdResult<engine::edit::EditReport> {
    files_settled(&jobs)?;
    if !settings_with_defaults().backup_confirmed {
        return Err("BACKUP_NOT_CONFIRMED".into());
    }
    blocking(move || {
        let rootp = PathBuf::from(&root);
        let report = engine::edit::edit_in_place(&rootp, &edits)?;
        // Refresh the index so every screen sees the change.
        let mut index = open_index(&rootp)?;
        libcache::scan(&mut index, &rootp)?;
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

/// A test copy running beside the Venuta in use (VENUTA_MULTI): it says so, everywhere.
pub fn test_copy() -> bool {
    std::env::var_os("VENUTA_MULTI").is_some()
}

#[tauri::command]
fn is_test_copy() -> bool {
    test_copy()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use_proxy();
    // Online timings (finding a stream, first audio, searches) go to the playback log.
    media_access::set_timing_log(|line| playback::note_play(line, "the online catalog"));
    let mut builder = tauri::Builder::default();
    // One Venuta at a time: opening it again brings the open one forward. (VENUTA_MULTI lets a
    // test copy run beside the one in use.)
    if !test_copy() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            desktop::show_main(app);
        }));
    }
    builder
        .manage(Jobs::default())
        .manage(playback::Playback::default())
        .manage(online::OnlineSearch::default())
        .manage(accounts::Accounts::default())
        .manage(phone::PhoneServer::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
                    if event.state() == ShortcutState::Pressed
                        && shortcut.matches(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyD)
                    {
                        lyric_windows::ask_main(app, "toggle");
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![desktop::IN_TRAY_ARG]),
        ))
        .on_window_event(desktop::on_window_event)
        .setup(|app| {
            allow_data_assets(app.handle());
            if test_copy()
                && let Some(w) = app.get_webview_window("main")
            {
                let _ = w.set_title("Venuta (test copy)");
            }
            if let Err(e) = desktop::tray(app.handle()) {
                eprintln!("tray: {e}");
            }
            // The icon chosen in Settings, on the window, the tray and the shortcuts.
            app_icon::settle(app.handle());
            // Ctrl+Shift+D shows or hides the desktop lyrics from anywhere. Another program
            // (or a second copy of Venuta) may have it already: then only the tray does.
            {
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
                let keys = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyD);
                if let Err(e) = app.global_shortcut().register(keys) {
                    eprintln!("Ctrl+Shift+D: {e}");
                }
            }
            // Started with Windows: stay in the tray. Otherwise show the window (it starts
            // hidden so it never flashes before this choice).
            if !std::env::args().any(|a| a == desktop::IN_TRAY_ARG) {
                desktop::show_main(app.handle());
            }
            #[cfg(windows)]
            if let Some(w) = app.get_webview_window("main")
                && let Ok(hwnd) = w.hwnd()
            {
                playback::start_media_controls(&app.state::<playback::Playback>(), hwnd.0 as isize);
                apply_listening(
                    &app.state::<playback::Playback>(),
                    &engine::settings::load().listening,
                );
            }
            // A YouTube Music sign-in kept from before is used again.
            ytm_account::restore(app.handle());
            // Ensure the stream tool is ready before the first online play (download if needed).
            std::thread::Builder::new()
                .name("venuta-ensure-stream-tool".into())
                .spawn(|| {
                    let _ = media_access::ensure_stream_tool();
                })
                .ok();
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
            playback::player_follow,
            playback::player_restore,
            playback::player_jump,
            playback::player_devices,
            playback::player_output,
            playback::song_lyrics,
            playback::all_lyrics,
            playback::player_resume,
            playback::player_verify,
            playback::song_waveform,
            playback::online_waveform,
            playback::player_sound,
            playback::player_stop_after,
            playback::player_sleep,
            playback::mini_player,
            playback::mini_player_toggle,
            lyric_windows::monitors,
            lyric_windows::lyrics_stage,
            lyric_windows::desktop_lyrics,
            lyric_windows::desktop_lyrics_lock,
            lyric_windows::desktop_lyrics_open,
            clipboard::clipboard_text,
            playback::player_spectrum,
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
            save_synced_lyrics,
            app_icon::app_icon,
            app_icon::set_app_icon,
            theme_packs::themes_list,
            theme_packs::theme_file,
            theme_packs::theme_save,
            theme_packs::theme_write_text,
            theme_packs::theme_add_file,
            theme_packs::theme_history,
            theme_packs::theme_set_aside,
            theme_packs::themes_stamp,
            theme_packs::theme_export,
            theme_packs::theme_inspect,
            theme_packs::theme_install,
            theme_packs::theme_duplicate,
            theme_packs::theme_show,
            show_in_folder,
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
            reidentify_card,
            card_choices,
            card_estimate,
            players,
            want_add,
            want_remove,
            desktop::set_close_action,
            desktop::quit_app,
            ytm_account::ytm_sign_in,
            ytm_account::ytm_sign_out,
            ytm_account::ytm_account,
            ytm_account::ytm_playlists,
            ytm_account::ytm_rate,
            cache_sizes,
            clear_cache,
            ytm_account::ytm_bring_in_likes,
            diagnostics,
            restart_app,
            desktop::song_notification,
            lyrics_find::lyrics_forget_all,
            desktop::set_ui_zoom,
            updates::update_check,
            accounts::spotify_status,
            accounts::spotify_connect,
            accounts::spotify_cancel,
            accounts::spotify_disconnect,
            accounts::spotify_playlists,
            accounts::spotify_read,
            updates::update_install,
            playlist_shelf,
            wanted_export,
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
            playlist_save_items,
            playlist_add,
            liked_songs,
            like_song,
            playlist_queue,
            online::online_search,
            online::online_search_tracks,
            online::online_suggest,
            online::online_collections,
            online::online_videos,
            online::online_moods,
            online::online_mood,
            online::online_shelf_more,
            online::online_album_page,
            online::online_radio,
            online::online_home,
            online::open_link,
            listening::listening_plays,
            listening::listening_forget,
            lyrics_find::lyrics_find,
            lyrics_find::lyrics_check,
            lyrics_find::lyrics_choose,
            lyrics_find::lyrics_offset,
            online::online_collection,
            online::online_artist,
            online::online_album_tracks,
            online::online_catalog_status,
            phone::phone_server_status,
            phone::phone_server_start,
            phone::phone_server_resume,
            phone::phone_server_stop,
            phone::phone_server_add,
            phone::phone_server_remove,
            online::online_ensure_stream_tool,
            online::online_lyrics,
            playlist_duplicate,
            playlist_export,
            playlists_check,
            open_send_link,
            open_saved_page,
            playlists_export,
            playlist_text,
            playlists_backup,
            playlist_on_service,
            playlist_youtube_links,
            playlist_refind,
            playlist_sources_check,
            playlist_update_read,
            playlist_restore,
            playlist_undelete,
            playlist_create,
            is_test_copy,
            search_web,
            import_preview,
            playlist_find_online,
            import_apply,
            set_apple,
            rate_playlist
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
