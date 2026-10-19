//! The signed-in YouTube Music account, as the app uses it: its library in Library, the presses
//! that change it (like, save, follow, add, edit), uploads, brand accounts and collaborative
//! playlists. Every command here is one the person pressed; nothing runs on its own.
//!
//! Signed out, or with Local only on (the catalog then has no account), each says so in words.

use crate::playback::Playback;
use media_access::{
    AccessError, AccountAccess, AccountIdentityInfo, AccountPlaylist, AccountTrack,
    CancellationToken, FollowState, HistoryDay, LibraryShelf, Placements, PlaylistEdit,
    RemoteSuggestion, SavedState,
};
use std::sync::Arc;
use tauri::State;

/// The account's part of the catalog, signed in; or why not.
pub fn account(pb: &Playback) -> Result<Arc<dyn AccountAccess>, String> {
    pb.catalog()
        .account_access()
        .filter(|a| a.signed_in())
        .ok_or_else(|| "Sign in to YouTube Music first.".to_owned())
}

/// Run one account request off the main thread.
async fn with_account<T: Send + 'static>(
    pb: &Playback,
    f: impl FnOnce(&dyn AccountAccess, &CancellationToken) -> Result<T, AccessError> + Send + 'static,
) -> Result<T, String> {
    let a = account(pb)?;
    crate::blocking(move || f(a.as_ref(), &CancellationToken::default()).map_err(|e| e.message))
        .await
}

/// Signed in, and whether YouTube still takes the sign-in.
#[derive(serde::Serialize)]
pub struct AccountStatus {
    pub signed_in: bool,
    /// YouTube stopped taking it: "Sign in again".
    pub expired: bool,
}

#[tauri::command]
pub fn ytm_status(pb: State<'_, Playback>) -> AccountStatus {
    match pb.catalog().account_access() {
        Some(a) => AccountStatus {
            signed_in: a.signed_in(),
            expired: a.session_expired(),
        },
        None => AccountStatus {
            signed_in: false,
            expired: false,
        },
    }
}

/// A part of the account's library as cards (playlists, albums, artists, subscriptions,
/// podcasts, uploaded albums).
#[tauri::command]
pub async fn ytm_library(
    pb: State<'_, Playback>,
    shelf: LibraryShelf,
) -> Result<Vec<RemoteSuggestion>, String> {
    with_account(&pb, move |a, c| a.library(shelf, c)).await
}

/// The account's library songs, or its uploaded ones.
#[tauri::command]
pub async fn ytm_library_songs(
    pb: State<'_, Playback>,
    uploads: bool,
) -> Result<Vec<AccountTrack>, String> {
    with_account(&pb, move |a, c| a.library_songs(uploads, c)).await
}

/// One of the account's playlists, whole, with each song's place and whether it is the
/// account's own (so it can be changed).
#[tauri::command]
pub async fn ytm_playlist(
    pb: State<'_, Playback>,
    playlist_id: String,
) -> Result<AccountPlaylist, String> {
    with_account(&pb, move |a, c| a.playlist(&playlist_id, c)).await
}

/// Make a playlist on the account; its id.
#[tauri::command]
pub async fn ytm_playlist_create(
    pb: State<'_, Playback>,
    title: String,
    description: Option<String>,
    privacy: String,
    video_ids: Vec<String>,
) -> Result<String, String> {
    let title = title.trim().to_owned();
    if title.is_empty() || title.contains(['<', '>']) {
        return Err("A playlist name can't be empty or hold < or >.".into());
    }
    with_account(&pb, move |a, c| {
        a.create_playlist(&title, description.as_deref(), &privacy, &video_ids, c)
    })
    .await
}

/// Change a playlist the account owns; where added songs landed.
#[tauri::command]
pub async fn ytm_playlist_edit(
    pb: State<'_, Playback>,
    playlist_id: String,
    edits: Vec<PlaylistEdit>,
) -> Result<Placements, String> {
    with_account(&pb, move |a, c| a.edit_playlist(&playlist_id, &edits, c)).await
}

/// Delete a playlist the account owns (asked first, in the page).
#[tauri::command]
pub async fn ytm_playlist_delete(
    pb: State<'_, Playback>,
    playlist_id: String,
) -> Result<(), String> {
    with_account(&pb, move |a, c| a.delete_playlist(&playlist_id, c)).await
}

/// Whether an album is saved to the library, and the id to save it by.
#[tauri::command]
pub async fn ytm_album_saved(
    pb: State<'_, Playback>,
    album_id: String,
) -> Result<Option<SavedState>, String> {
    with_account(&pb, move |a, c| a.album_saved(&album_id, c)).await
}

/// Save (or un-save) an album or a playlist.
#[tauri::command]
pub async fn ytm_save(
    pb: State<'_, Playback>,
    playlist_id: String,
    saved: bool,
) -> Result<(), String> {
    with_account(&pb, move |a, c| a.save(&playlist_id, saved, c)).await
}

/// Whether the account follows an artist, and its channel.
#[tauri::command]
pub async fn ytm_artist_followed(
    pb: State<'_, Playback>,
    artist_id: String,
) -> Result<Option<FollowState>, String> {
    with_account(&pb, move |a, c| a.artist_followed(&artist_id, c)).await
}

/// Follow (or stop following) an artist.
#[tauri::command]
pub async fn ytm_subscribe(
    pb: State<'_, Playback>,
    channel_id: String,
    subscribed: bool,
) -> Result<(), String> {
    with_account(&pb, move |a, c| a.subscribe(&channel_id, subscribed, c)).await
}

/// The account's history, as YouTube keeps it.
#[tauri::command]
pub async fn ytm_history(pb: State<'_, Playback>) -> Result<Vec<HistoryDay>, String> {
    with_account(&pb, |a, c| a.history(c)).await
}

/// Send one of your files to the account's uploads. Only the person picks which.
#[tauri::command]
pub async fn ytm_upload(pb: State<'_, Playback>, path: String) -> Result<(), String> {
    let a = account(&pb)?;
    crate::blocking(move || {
        let p = std::path::PathBuf::from(&path);
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("That file has no name.")?
            .to_owned();
        let bytes = std::fs::read(&p).map_err(|e| format!("Couldn't read {name}: {e}"))?;
        a.upload(&name, bytes, &CancellationToken::default())
            .map_err(|e| e.message)
    })
    .await
}

/// The identities the sign-in can act as (itself and its brand accounts).
#[tauri::command]
pub async fn ytm_identities(pb: State<'_, Playback>) -> Result<Vec<AccountIdentityInfo>, String> {
    with_account(&pb, |a, c| a.identities(c)).await
}

/// Act as this identity from now on (None: the sign-in's own), and remember it.
#[tauri::command]
pub fn ytm_use_identity(
    app: tauri::AppHandle,
    pb: State<'_, Playback>,
    id: Option<String>,
) -> Result<(), String> {
    use tauri::Emitter;
    let a = account(&pb)?;
    a.use_identity(id.as_deref());
    crate::ytm_links::forget_account_key();
    remember_identity(id.as_deref())?;
    // Another channel's library from now on: everything shown for the last one is read again.
    let _ = app.emit("ytm-account-changed", ());
    Ok(())
}

/// Collaborators: an invite link for a playlist the account owns.
#[tauri::command]
pub async fn ytm_invite(pb: State<'_, Playback>, playlist_id: String) -> Result<String, String> {
    with_account(&pb, move |a, c| a.invite(&playlist_id, c)).await
}

/// Stop new collaborators joining (or let them again).
#[tauri::command]
pub async fn ytm_close_to_collaborators(
    pb: State<'_, Playback>,
    playlist_id: String,
    closed: bool,
) -> Result<(), String> {
    with_account(&pb, move |a, c| {
        a.close_to_collaborators(&playlist_id, closed, c)
    })
    .await
}

/// Join a collaborative playlist from its invite link; its playlist id.
#[tauri::command]
pub async fn ytm_join(pb: State<'_, Playback>, link: String) -> Result<String, String> {
    with_account(&pb, move |a, c| a.join(link.trim(), c)).await
}

// --- The identity chosen, kept across starts ---------------------------------------------------

const IDENTITY_KEY: &str = "account:youtube-music:identity";

fn remember_identity(id: Option<&str>) -> Result<(), String> {
    use engine::accounts::{SystemVault, Vault};
    match id {
        Some(id) => SystemVault.put(IDENTITY_KEY, id).map_err(|e| e.to_string()),
        None => {
            let _ = SystemVault.delete(IDENTITY_KEY);
            Ok(())
        }
    }
}

/// At startup, after the sign-in is restored: act as the identity chosen last time.
pub fn restore_identity(pb: &Playback) {
    use engine::accounts::{SystemVault, Vault};
    // The catalog itself, not Local only's stand-in: the choice holds for when it is turned off.
    if let (Some(a), Ok(Some(id))) = (
        pb.online_catalog().account_access(),
        SystemVault.get(IDENTITY_KEY),
    ) {
        a.use_identity(Some(&id));
    }
}

/// Signing out forgets the identity chosen too.
pub fn forget_identity() {
    crate::ytm_links::forget_account_key();
    let _ = remember_identity(None);
}

/// The identity chosen (None: the sign-in's own).
pub fn chosen_identity() -> Option<String> {
    use engine::accounts::{SystemVault, Vault};
    SystemVault.get(IDENTITY_KEY).ok().flatten()
}
