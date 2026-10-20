//! The signed-in YouTube Music account, as the app uses it: its library in Library, the presses
//! that change it (like, save, follow, add, edit), uploads, brand accounts and collaborative
//! playlists. Every command here is one the person pressed; nothing runs on its own.
//!
//! Signed out, or with Local only on (the catalog then has no account), each says so in words.

use crate::playback::Playback;
use media_access::{
    AccessError, AccountAccess, AccountIdentityInfo, AccountPlaylist, AccountTrack,
    CancellationToken, FollowState, HistoryDay, LibraryShelf, MenuPress, Placements, PlaylistEdit,
    RemoteSuggestion, SavedState, TasteArtist,
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

/// Take one play out of the account's history (its row's × in Stats).
#[tauri::command]
pub async fn ytm_history_remove(pb: State<'_, Playback>, token: String) -> Result<(), String> {
    with_account(&pb, move |a, c| a.remove_from_history(&token, c)).await
}

/// Vote on a song of a collaborative playlist, or take the vote back: the token its button sends.
#[tauri::command]
pub async fn ytm_vote(pb: State<'_, Playback>, token: String) -> Result<(), String> {
    with_account(&pb, move |a, c| a.vote(&token, c)).await
}

/// One press on an online song's own menu: Save to library or Remove from library, Pin to Listen
/// again or Unpin, Not interested. The token is the one its menu holds for that way.
#[tauri::command]
pub async fn ytm_menu_press(
    pb: State<'_, Playback>,
    token: String,
    press: MenuPress,
) -> Result<(), String> {
    with_account(&pb, move |a, c| a.menu_press(&token, press, c)).await
}

/// One online song's own menu as the account sees it, read when its menu is opened and it was
/// listed with none (a search sent signed out). Only the song's id goes, never the words.
#[tauri::command]
pub async fn ytm_song_menu(
    pb: State<'_, Playback>,
    track_id: String,
) -> Result<Option<media_access::TrackMenu>, String> {
    let id = track_id.trim().to_owned();
    if id.is_empty() {
        return Ok(None);
    }
    with_account(&pb, move |a, c| a.song_menu(&id, c)).await
}

/// The artists YouTube Music's taste builder offers ("Choose artists you like").
#[tauri::command]
pub async fn ytm_taste_profile(pb: State<'_, Playback>) -> Result<Vec<TasteArtist>, String> {
    with_account(&pb, |a, c| a.taste_profile(c)).await
}

/// Tell YouTube Music the artists chosen (only on Done): every one shown, and the chosen ones.
#[tauri::command]
pub async fn ytm_set_taste_profile(
    pb: State<'_, Playback>,
    impressions: Vec<String>,
    selected: Vec<String>,
) -> Result<(), String> {
    if selected.is_empty() {
        return Err("Choose at least one artist first.".into());
    }
    with_account(&pb, move |a, c| {
        a.set_taste_profile(&impressions, &selected, c)
    })
    .await
}

/// Take one past search out of the account's search history (asked first, in the search box).
#[tauri::command]
pub async fn ytm_remove_past_search(pb: State<'_, Playback>, token: String) -> Result<(), String> {
    with_account(&pb, move |a, c| a.remove_past_search(&token, c)).await
}

/// Delete one of the account's uploads, a song or an album (asked first, where it is pressed).
#[tauri::command]
pub async fn ytm_delete_upload(pb: State<'_, Playback>, entity_id: String) -> Result<(), String> {
    let id = entity_id.trim().to_owned();
    if id.is_empty() {
        return Err("YouTube Music gave no way to delete that upload.".into());
    }
    with_account(&pb, move |a, c| a.delete_upload(&id, c)).await
}

/// The most a cover picture may weigh here: YouTube Music's own limit isn't published, and a
/// photo bigger than this is far past what a playlist cover shows.
const COVER_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Whether a picked file can be a playlist cover: a JPEG or PNG, not empty, not huge.
fn cover_file_ok(name: &str, len: u64) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if ![".jpg", ".jpeg", ".png"].iter().any(|e| lower.ends_with(e)) {
        return Err("A playlist cover can be a JPEG or a PNG picture.".into());
    }
    if len == 0 {
        return Err(format!("{name} is empty."));
    }
    if len > COVER_MAX_BYTES {
        return Err(format!("{name} is over 10 MB. Pick a smaller picture."));
    }
    Ok(())
}

/// Give a playlist the account owns this picture as its cover. Only the person picks which.
#[tauri::command]
pub async fn ytm_playlist_cover(
    pb: State<'_, Playback>,
    playlist_id: String,
    path: String,
) -> Result<(), String> {
    let a = account(&pb)?;
    crate::blocking(move || {
        let p = std::path::PathBuf::from(&path);
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("That file has no name.")?
            .to_owned();
        let len = std::fs::metadata(&p)
            .map_err(|e| format!("Couldn't read {name}: {e}"))?
            .len();
        cover_file_ok(&name, len)?;
        let bytes = std::fs::read(&p).map_err(|e| format!("Couldn't read {name}: {e}"))?;
        a.set_playlist_cover(&playlist_id, bytes, &CancellationToken::default())
            .map_err(|e| e.message)
    })
    .await
}

/// Take a playlist's own cover away (asked first, in the page); the one YouTube Music makes
/// from its songs again, when it said.
#[tauri::command]
pub async fn ytm_playlist_cover_clear(
    pb: State<'_, Playback>,
    playlist_id: String,
) -> Result<Option<String>, String> {
    with_account(&pb, move |a, c| a.clear_playlist_cover(&playlist_id, c)).await
}

/// Send one of your files to the account's uploads. Only the person picks which.
#[tauri::command]
pub async fn ytm_upload(pb: State<'_, Playback>, path: String) -> Result<(), String> {
    let a = account(&pb)?;
    // On a brand channel YouTube Music would put it in the main account's uploads.
    crate::ytm_links::uploads_allowed(chosen_identity().as_deref())?;
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
    let all = with_account(&pb, |a, c| a.identities(c)).await?;
    let own = all.iter().find(|i| i.own).map(|i| i.id.clone());
    // Your own channel kept as a choice (by its id, from before): no choice at all, so it is
    // acted as without `onBehalfOfUser` and uploads are allowed again.
    if let Some(id) = chosen_identity()
        && brand_choice(Some(&id), own.as_deref()).is_none()
    {
        forget_identity(&pb);
    }
    if let Ok(mut o) = OWN_CHANNEL.lock() {
        *o = own;
    }
    Ok(all)
}

/// Act as this identity from now on (None: the sign-in's own), and remember it. Your own
/// channel, chosen by its id, is no identity: it is acted as by sending none.
#[tauri::command]
pub fn ytm_use_identity(
    app: tauri::AppHandle,
    pb: State<'_, Playback>,
    id: Option<String>,
) -> Result<(), String> {
    use tauri::Emitter;
    let a = account(&pb)?;
    let own = OWN_CHANNEL.lock().ok().and_then(|o| o.clone());
    let id = brand_choice(id.as_deref(), own.as_deref());
    a.use_identity(id);
    crate::ytm_links::forget_account_key();
    remember_identity(id)?;
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

/// The sign-in's own channel's id, as the channel list last said it (None until it is read).
static OWN_CHANNEL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The brand identity a choice is: none for your own channel (`own`), which YouTube acts as
/// when no identity is sent. Only a brand channel is kept as a choice.
fn brand_choice<'a>(id: Option<&'a str>, own: Option<&str>) -> Option<&'a str> {
    id.filter(|id| Some(*id) != own)
}

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

/// Signing in, out or switching forgets the identity chosen too: kept and in use (the catalog
/// does that on a new cookie as well; this says it here, where the choice is kept).
pub fn forget_identity(pb: &Playback) {
    if let Some(a) = pb.online_catalog().account_access() {
        a.use_identity(None);
    }
    crate::ytm_links::forget_account_key();
    let _ = remember_identity(None);
    // Another sign-in has another own channel: read again with its channel list.
    if let Ok(mut o) = OWN_CHANNEL.lock() {
        *o = None;
    }
}

/// The identity chosen (None: the sign-in's own).
pub fn chosen_identity() -> Option<String> {
    use engine::accounts::{SystemVault, Vault};
    SystemVault.get(IDENTITY_KEY).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::{brand_choice, cover_file_ok};

    // Your own channel is acted as by sending no identity: choosing it (by its id, as the list
    // gives it) keeps no choice, so uploads stay allowed and no `onBehalfOfUser` goes out.
    #[test]
    fn choosing_your_own_channel_keeps_no_brand_identity() {
        assert_eq!(brand_choice(Some("own-sync"), Some("own-sync")), None);
        assert_eq!(brand_choice(None, Some("own-sync")), None);
        assert_eq!(
            brand_choice(Some("brand-page"), Some("own-sync")),
            Some("brand-page")
        );
        // Not knowing which is your own (the list not read yet), a choice is kept as it came.
        assert_eq!(brand_choice(Some("brand-page"), None), Some("brand-page"));
        assert!(
            crate::ytm_links::uploads_allowed(brand_choice(Some("own-sync"), Some("own-sync")))
                .is_ok()
        );
    }

    // A cover is a picture YouTube Music takes: a JPEG or a PNG, with something in it, and not a
    // whole camera roll's worth of bytes. Said in words, before anything is sent.
    #[test]
    fn a_cover_is_a_jpeg_or_png_of_a_sensible_size() {
        assert!(cover_file_ok("Road trip.JPG", 120_000).is_ok());
        assert!(cover_file_ok("cover.jpeg", 1).is_ok());
        assert!(cover_file_ok("cover.png", 10 * 1024 * 1024).is_ok());
        assert_eq!(
            cover_file_ok("cover.webp", 10).unwrap_err(),
            "A playlist cover can be a JPEG or a PNG picture."
        );
        assert_eq!(
            cover_file_ok("cover.png", 0).unwrap_err(),
            "cover.png is empty."
        );
        assert!(
            cover_file_ok("big.png", 10 * 1024 * 1024 + 1)
                .unwrap_err()
                .contains("over 10 MB")
        );
    }
}
