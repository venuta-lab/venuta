//! "Also on YouTube Music": a playlist of yours linked to one on your account, the same on both.
//! The merge itself is `engine::ytm_links`; here it runs against the account, paced, and kept.
//!
//! A sync runs when you link, when you open the playlist, after you change it, at start and at
//! most every 30 minutes (the app asks for each; nothing here loops). Each one reads the
//! playlist there once, changes yours first (a copy kept for Undo), then YouTube Music, one
//! request at a time with a pause between.

use crate::playback::Playback;
use engine::playlists::{self, Foreign, OnlineEntry, PlaylistEntry};
use engine::ytm_links::{self, Link, Pause, SyncNote, Uploading};
use media_access::{AccountAccess, CancellationToken, MediaAccess, PlaylistEdit};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::State;

/// The pause between requests to YouTube Music within one sync.
const PACE: Duration = Duration::from_millis(450);
/// The pause between two uploads.
const UPLOAD_PACE: Duration = Duration::from_millis(1200);
/// What YouTube Music takes as an upload.
const UPLOAD_TYPES: [&str; 5] = ["flac", "m4a", "mp3", "ogg", "wma"];
const UPLOAD_MAX_BYTES: u64 = 300 * 1024 * 1024;

fn data_dir() -> PathBuf {
    engine::paths::app_data_dir()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// --- Which account a link is on -----------------------------------------------------------------

static ACCOUNT_KEY: Mutex<Option<String>> = Mutex::new(None);

/// Signed out, in as someone else, or another channel chosen: asked again next time.
pub fn forget_account_key() {
    if let Ok(mut k) = ACCOUNT_KEY.lock() {
        *k = None;
    }
}

/// Who is signed in, as a link remembers it: the account, and which of its channels. Your
/// YouTube Music choices are kept by the same key (`crate::ytm_choices`).
pub(crate) fn account_key(catalog: &dyn MediaAccess) -> Result<String, String> {
    if let Some(k) = ACCOUNT_KEY.lock().ok().and_then(|k| k.clone()) {
        return Ok(k);
    }
    let who = catalog
        .account(&CancellationToken::default())
        .map_err(|e| e.message)?
        .ok_or("Sign in to YouTube Music first.")?;
    let name = who
        .email
        .or(who.handle)
        .or(who.name)
        .ok_or("YouTube Music didn't say which account this is. Try again in a moment.")?;
    let channel = crate::ytm_library::chosen_identity().unwrap_or_else(|| "own".into());
    let key = format!("{name}#{channel}");
    if let Ok(mut k) = ACCOUNT_KEY.lock() {
        *k = Some(key.clone());
    }
    Ok(key)
}

// --- What the page shows ------------------------------------------------------------------------

/// A playlist's link as its page shows it.
#[derive(Serialize)]
pub struct LinkView {
    #[serde(flatten)]
    pub link: Link,
    /// Files of yours not asked about yet (the matching review has them).
    pub unmatched: usize,
    /// Songs that are (or go) there, and lines that stay here only.
    pub there: usize,
    pub only_here: usize,
}

fn entries_of(
    root: &Path,
    name: &str,
) -> Result<(Vec<PlaylistEntry>, Arc<engine::library::Library>), String> {
    let (_, lib) = crate::libcache::fresh(root)?;
    let (entries, _) = playlists::read(root, &lib, name)?;
    Ok((entries, lib))
}

/// This playlist's link, if it has one.
#[tauri::command]
pub async fn ytm_link(root: String, name: String) -> Result<Option<LinkView>, String> {
    crate::blocking(move || {
        let Some(link) = ytm_links::load(&data_dir(), &name) else {
            return Ok(None);
        };
        let (unmatched, (there, only_here)) = entries_of(Path::new(&root), &name)
            .map(|(e, _)| {
                (
                    ytm_links::unmatched(&e, &link).len(),
                    ytm_links::counts(&e, &link),
                )
            })
            .unwrap_or_default();
        Ok(Some(LinkView {
            link,
            unmatched,
            there,
            only_here,
        }))
    })
    .await
}

/// Every link of a playlist that is there (to sync them at start, and every half hour at
/// most). A deleted playlist's link stays, so Undo brings it back linked, but rests.
#[tauri::command]
pub fn ytm_links(root: String) -> Vec<Link> {
    let rootp = PathBuf::from(&root);
    ytm_links::all(&data_dir())
        .into_iter()
        .filter(|l| playlists::exists(&rootp, &l.playlist))
        .collect()
}

/// A file of yours the matching review asks about.
#[derive(Serialize)]
pub struct ReviewRow {
    pub key: String,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    /// The YouTube Music song that seems to be it.
    pub found: Option<OnlineEntry>,
    /// It can go to your uploads (a type and size YouTube Music takes).
    pub uploadable: bool,
    /// What you said before (checking matches): the song it is there, kept here only, or on
    /// its way to your uploads.
    pub current: Option<OnlineEntry>,
    pub current_id: Option<String>,
    pub kept_here: bool,
    pub uploading: bool,
}

fn uploadable(file: &Path) -> bool {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    UPLOAD_TYPES.contains(&ext.as_str())
        && std::fs::metadata(file).is_ok_and(|m| m.len() < UPLOAD_MAX_BYTES)
}

/// The files of yours with no YouTube Music song yet, each with the song that seems to be it
/// (looked for in the catalog, a few at a time).
#[tauri::command]
pub async fn ytm_link_review(
    pb: State<'_, Playback>,
    root: String,
    name: String,
    all: bool,
) -> Result<Vec<ReviewRow>, String> {
    let catalog = pb.catalog();
    let account = crate::ytm_library::account(&pb).ok();
    crate::blocking(move || {
        let rootp = PathBuf::from(&root);
        let (entries, _) = entries_of(&rootp, &name)?;
        let link = ytm_links::load(&data_dir(), &name).unwrap_or_default();
        let which = if all {
            ytm_links::files(&entries)
        } else {
            ytm_links::unmatched(&entries, &link)
        };
        let rows: Vec<&PlaylistEntry> = which.into_iter().map(|i| &entries[i]).collect();
        // The songs matched before, as the playlist there shows them.
        let mut known: std::collections::HashMap<String, OnlineEntry> = Default::default();
        if all
            && !link.matches.is_empty()
            && let Some(a) = &account
            && let Ok(p) = a.playlist(&link.playlist_id, &CancellationToken::default())
        {
            for t in &p.tracks {
                let id = link
                    .aliases
                    .get(&t.track.track_id)
                    .cloned()
                    .unwrap_or_else(|| t.track.track_id.clone());
                known.insert(id, OnlineEntry::from_remote(&t.track));
            }
        }
        let songs: Vec<Foreign> = rows
            .iter()
            .map(|e| Foreign {
                title: e.title.clone(),
                artist: e.artist.clone(),
                album: e.album.clone(),
                duration_ms: Some(e.duration_ms).filter(|d| *d > 0),
                ..Default::default()
            })
            .collect();
        let found = engine::imports::find_online(catalog.as_ref(), &songs, &|_, _| {});
        Ok(rows
            .into_iter()
            .zip(found)
            .filter_map(|(e, found)| {
                let key = ytm_links::file_key(e)?;
                let current_id = link.matches.get(&key).cloned();
                Some(ReviewRow {
                    current: current_id.as_ref().and_then(|v| known.get(v).cloned()),
                    current_id,
                    kept_here: link.local_only.contains(&key),
                    uploading: link.uploading.contains_key(&key),
                    key,
                    path: e.path.clone(),
                    title: e.title.clone(),
                    artist: e.artist.clone(),
                    album: e.album.clone(),
                    duration_ms: e.duration_ms,
                    found,
                    uploadable: uploadable(&rootp.join(&e.path)),
                })
            })
            .collect())
    })
    .await
}

/// What you chose in the matching review, file by file.
#[derive(Deserialize, Default)]
pub struct Choices {
    /// (file, YouTube Music song): this file is that song.
    #[serde(default)]
    pub matches: Vec<(String, String)>,
    /// These stay in Venuta only.
    #[serde(default)]
    pub local_only: Vec<String>,
    /// These go to your uploads (each one you ticked).
    #[serde(default)]
    pub upload: Vec<String>,
}

/// What a sync did, or why it waits.
#[derive(Serialize, Default)]
pub struct SyncReport {
    /// Changed nothing, and why (asked about in the page).
    pub paused: Option<Pause>,
    /// Another sync of this playlist was running; this one didn't.
    pub busy: bool,
    pub note: SyncNote,
    /// Files sent to your uploads now, and ones that couldn't be.
    pub uploaded: usize,
    pub upload_problems: Vec<String>,
}

// --- Linking, choosing, syncing -----------------------------------------------------------------

/// Link this playlist: to a playlist of yours there (`target`), or to a new one made with its
/// name. Then the first sync, which only adds (nothing is taken out on either side).
#[tauri::command]
pub async fn ytm_link_start(
    pb: State<'_, Playback>,
    root: String,
    name: String,
    target: Option<String>,
    privacy: String,
    choices: Choices,
) -> Result<SyncReport, String> {
    let account = crate::ytm_library::account(&pb)?;
    let catalog = pb.catalog();
    crate::blocking(move || {
        let data = data_dir();
        if ytm_links::load(&data, &name).is_some() {
            return Err("This playlist is linked already.".into());
        }
        let key = account_key(catalog.as_ref())?;
        let cancel = CancellationToken::default();
        let playlist_id = match target.filter(|t| !t.trim().is_empty()) {
            Some(t) => t.trim().trim_start_matches("VL").to_owned(),
            None => account
                .create_playlist(&name, None, &privacy, &[], &cancel)
                .map_err(|e| e.message)?,
        };
        let link = Link {
            playlist: name.clone(),
            account: key,
            playlist_id,
            ..Default::default()
        };
        ytm_links::save(&data, &link)?;
        choose_and_sync(
            account.as_ref(),
            &catalog,
            Path::new(&root),
            &name,
            choices,
            false,
        )
    })
    .await
}

/// Answers from a later matching review (new files since), then a sync.
#[tauri::command]
pub async fn ytm_link_choose(
    pb: State<'_, Playback>,
    root: String,
    name: String,
    choices: Choices,
) -> Result<SyncReport, String> {
    let account = crate::ytm_library::account(&pb)?;
    let catalog = pb.catalog();
    crate::blocking(move || {
        choose_and_sync(
            account.as_ref(),
            &catalog,
            Path::new(&root),
            &name,
            choices,
            false,
        )
    })
    .await
}

/// Sync now. `force`: you said go ahead after a sync waited (most songs gone on one side).
#[tauri::command]
pub async fn ytm_link_sync(
    pb: State<'_, Playback>,
    root: String,
    name: String,
    force: bool,
) -> Result<SyncReport, String> {
    let account = crate::ytm_library::account(&pb)?;
    let catalog = pb.catalog();
    crate::blocking(move || {
        choose_and_sync(
            account.as_ref(),
            &catalog,
            Path::new(&root),
            &name,
            Choices::default(),
            force,
        )
    })
    .await
}

/// Unlink: the link goes; both playlists stay as they are.
#[tauri::command]
pub fn ytm_unlink(name: String) -> Result<(), String> {
    ytm_links::forget(&data_dir(), &name)
}

/// Undo what a sync did to yours: the copy kept before it comes back, and the link holds
/// (the next sync waits for you, so the undo never takes songs out there by itself).
#[tauri::command]
pub fn ytm_link_undo(root: String, name: String, copy: String) -> Result<(), String> {
    playlists::restore_copy(Path::new(&root), &name, Path::new(&copy))?;
    let data = data_dir();
    if let Some(mut link) = ytm_links::load(&data, &name) {
        link.held = true;
        ytm_links::save(&data, &link)?;
    }
    Ok(())
}

/// The playlist was renamed: its link follows (called by the rename).
pub fn renamed(from: &str, to: &str) -> Result<(), String> {
    ytm_links::rename(&data_dir(), from, to)
}

static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// One sync of a playlist at a time; the second press is told so.
struct Running(String);
impl Running {
    fn start(name: &str) -> Option<Self> {
        let mut g = RUNNING.lock().ok()?;
        g.get_or_insert_with(HashSet::new)
            .insert(name.to_owned())
            .then(|| Self(name.to_owned()))
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut g) = RUNNING.lock()
            && let Some(s) = g.as_mut()
        {
            s.remove(&self.0);
        }
    }
}

fn choose_and_sync(
    account: &dyn AccountAccess,
    catalog: &Arc<dyn MediaAccess>,
    root: &Path,
    name: &str,
    choices: Choices,
    force: bool,
) -> Result<SyncReport, String> {
    let Some(_running) = Running::start(name) else {
        return Ok(SyncReport {
            busy: true,
            ..Default::default()
        });
    };
    let data = data_dir();
    let mut link =
        ytm_links::load(&data, name).ok_or("This playlist isn't linked to YouTube Music.")?;
    let cancel = CancellationToken::default();
    let mut report = SyncReport::default();

    // The review's answers first.
    for (file, video) in choices.matches {
        link.local_only.remove(&file);
        link.matches.insert(file, video);
    }
    for file in choices.local_only {
        link.matches.remove(&file);
        link.local_only.insert(file);
    }
    if !choices.upload.is_empty() {
        let (entries, _) = entries_of(root, name)?;
        for (n, key) in choices.upload.iter().enumerate() {
            let Some(e) = entries
                .iter()
                .find(|e| ytm_links::file_key(e).as_ref() == Some(key))
            else {
                continue;
            };
            if n > 0 {
                std::thread::sleep(UPLOAD_PACE);
            }
            let file = root.join(&e.path);
            let file_name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("song")
                .to_owned();
            let sent = std::fs::read(&file)
                .map_err(|err| format!("Couldn't read {file_name}: {err}"))
                .and_then(|bytes| {
                    account
                        .upload(&file_name, bytes, &cancel)
                        .map_err(|err| format!("{file_name}: {}", err.message))
                });
            match sent {
                Ok(()) => {
                    report.uploaded += 1;
                    // Its upload is what it is there from now on (once it shows).
                    link.matches.remove(key);
                    link.local_only.remove(key);
                    link.uploading.insert(
                        key.clone(),
                        Uploading {
                            title: e.title.clone(),
                            artist: e.artist.clone(),
                            at: now(),
                        },
                    );
                }
                Err(p) => report.upload_problems.push(p),
            }
        }
    }
    ytm_links::save(&data, &link)?;

    // Uploads sent earlier that have shown up now.
    if !link.uploading.is_empty()
        && let Ok(ups) = account.library_songs(true, &cancel)
        && link.claim_uploads(&ups) > 0
    {
        ytm_links::save(&data, &link)?;
    }

    let key = account_key(catalog.as_ref())?;
    let (entries, lib) = entries_of(root, name)?;
    let there = account
        .playlist(&link.playlist_id, &cancel)
        .map_err(|e| e.message)?;
    if !there.owned && !there.collaborative {
        return Err(format!(
            "“{}” on YouTube Music isn't yours, so Venuta can't change it. Unlink, or link to one of your playlists.",
            there.title
        ));
    }
    let plan = match ytm_links::plan(&entries, &link, &key, &there.tracks, there.complete, force) {
        Ok(p) => {
            link.held = false;
            p
        }
        Err(p) => {
            report.paused = Some(p);
            return Ok(report);
        }
    };
    let mut note = SyncNote {
        at: now(),
        pulled: plan.pulled,
        dropped: plan.dropped,
        ..Default::default()
    };

    // Yours first, a copy kept so it can be undone.
    if let Some(items) = &plan.items {
        let copy = playlists::keep_copy(root, name)?;
        let source = playlists::source_of(root, name);
        playlists::write_items(root, &lib, name, items, false, source.as_deref())?;
        note.undo = Some(copy.to_string_lossy().into_owned());
    }

    // Then YouTube Music, a request at a time. Stopped part way, the base stays as it was, so
    // the next sync picks up from where this one stopped.
    let id = link.playlist_id.clone();
    let mut sent = 0;
    let mut send = |edit: PlaylistEdit| {
        if sent > 0 {
            std::thread::sleep(PACE);
        }
        sent += 1;
        account
            .edit_playlist(&id, &[edit], &cancel)
            .map_err(|e| e.message)
    };
    let pushed = (|| -> Result<(), String> {
        if !plan.remove_remote.is_empty() {
            send(PlaylistEdit::Remove {
                items: plan.remove_remote.clone(),
            })?;
            note.removed = plan.remove_remote.len();
        }
        let mut placed = Vec::new();
        if !plan.add_remote.is_empty() {
            placed = send(PlaylistEdit::Add {
                video_ids: plan.add_remote.clone(),
            })?;
            note.pushed = plan.add_remote.len();
            // YouTube Music sometimes puts in another version than the one sent.
            link.note_swaps(&plan.add_remote, &placed);
        }
        let (now_order, wanted) = ytm_links::after_edits(
            &there.tracks,
            &plan.remove_remote,
            &placed,
            &plan.order,
            &link.aliases,
        );
        for (set_video_id, before) in ytm_links::moves(&now_order, &wanted) {
            send(PlaylistEdit::Move {
                set_video_id,
                before,
            })?;
            note.moved += 1;
        }
        Ok(())
    })();
    match pushed {
        Ok(()) => link.base = plan.order.clone(),
        Err(p) => note.problem = Some(p),
    }
    link.title = there.title.clone();
    link.last_sync = note.at;
    link.noted(note.clone());
    ytm_links::save(&data, &link)?;
    report.note = note;
    Ok(report)
}
