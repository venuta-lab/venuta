//! Your YouTube Music choices: what Venuta does with the account and channel in use, each a
//! switch, asked once at sign-in and kept per account (`engine::settings::YtmChoices`). Here
//! they are read and kept, and the catalog is told the two it acts on itself: recommendations
//! as the account or not, and searches as the account or not.
//!
//! The account is named the way linked playlists name it (`email#channel`), so a brand channel
//! is asked on its own, and a test copy or another account never inherits a switch.

use crate::playback::Playback;
use engine::settings::YtmChoices;
use media_access::{CancellationToken, MediaAccess, RemoteTrack};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use tauri::State;

/// The choices of the account in use, as Settings and the sign-in screen show them.
#[derive(Debug, Serialize, PartialEq)]
pub struct ChoicesView {
    /// Which account and channel they are for (`email#channel`).
    pub account: String,
    /// Asked already. False the first time: the choices are the safe ones, and the screen
    /// after sign-in shows them.
    pub asked: bool,
    pub choices: YtmChoices,
}

fn view_of(s: &engine::settings::Settings, account: String) -> ChoicesView {
    ChoicesView {
        asked: s.ytm_choices_of(&account).is_some(),
        choices: s.ytm_choices_for(&account),
        account,
    }
}

/// The catalog itself told what this account chose (Local only or not: they hold for when it
/// is turned off).
fn tell(online: &dyn MediaAccess, c: &YtmChoices) {
    online.set_personal(c.personal);
    online.set_signed_search(c.signed_search);
}

/// The choices of the account in use, read again and told to the catalog; None signed out or
/// in Local only. Asked when the sign-in comes back, and after any change of account or
/// channel.
#[tauri::command]
pub async fn ytm_choices(pb: State<'_, Playback>) -> Result<Option<ChoicesView>, String> {
    if crate::ytm_library::account(&pb).is_err() {
        return Ok(None);
    }
    let (catalog, online) = (pb.catalog(), pb.online_catalog());
    crate::blocking(move || {
        let account = crate::ytm_links::account_key(catalog.as_ref())?;
        let view = view_of(&engine::settings::load(), account);
        tell(online.as_ref(), &view.choices);
        Ok(Some(view))
    })
    .await
}

/// Keep an account's choices, all of them (closing the screen keeps the safe ones as shown, so
/// it isn't asked again). Told to the catalog when that account is the one in use.
#[tauri::command]
pub async fn ytm_set_choices(
    pb: State<'_, Playback>,
    account: String,
    choices: YtmChoices,
) -> Result<ChoicesView, String> {
    if account.trim().is_empty() {
        return Err("Which account are these for?".into());
    }
    let signed_in = crate::ytm_library::account(&pb).is_ok();
    let (catalog, online) = (pb.catalog(), pb.online_catalog());
    crate::blocking(move || {
        let mut s = engine::settings::load();
        s.set_ytm_choices(&account, choices);
        engine::settings::save(&s)?;
        let view = view_of(&s, account);
        if signed_in
            && crate::ytm_links::account_key(catalog.as_ref())
                .ok()
                .as_deref()
                == Some(view.account.as_str())
        {
            tell(online.as_ref(), &view.choices);
        }
        Ok(view)
    })
    .await
}

/// Count plays is on for the account in use. Off whenever it can't be told which account that
/// is: nothing is sent on a guess. Asks who is signed in only the first time (then kept).
pub fn counts_plays(catalog: &dyn MediaAccess) -> bool {
    crate::ytm_links::account_key(catalog)
        .is_ok_and(|k| engine::settings::load().ytm_choices_for(&k).count_plays)
}

/// The artists the catalog leaves out of what it picks: the ones you blocked while "Hide
/// artists I blocked" is on, none while it is off. The list itself is kept either way.
pub fn blocked_for_catalog(l: &engine::settings::Listening) -> Vec<(Option<String>, String)> {
    if !l.hide_blocked {
        return Vec::new();
    }
    l.blocked_artists
        .iter()
        .map(|b| (b.artist_id.clone(), b.name.clone()))
        .collect()
}

// --- New YouTube likes come in ------------------------------------------------------------------

/// The songs seen in each account's Liked Music the last time Venuta looked, by account.
fn seen_path() -> PathBuf {
    engine::paths::app_data_dir().join("ytm-likes-seen.json")
}

fn load_seen() -> BTreeMap<String, Vec<String>> {
    std::fs::read_to_string(seen_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_seen(seen: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    let p = seen_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.part");
    std::fs::write(&tmp, serde_json::to_vec(seen).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// A music video, a video someone uploaded, or an episode: not a song for Liked songs.
fn is_song(t: &RemoteTrack) -> bool {
    !matches!(
        t.video_type.as_deref(),
        Some("OMV" | "UGC" | "PODCAST_EPISODE")
    )
}

/// The songs liked there since Venuta last looked. The first look only notes what is there
/// (those come in with "Bring my Liked Music in"); a video or an episode never comes in.
fn fresh_likes<'a>(seen: Option<&[String]>, liked: &'a [RemoteTrack]) -> Vec<&'a RemoteTrack> {
    let Some(seen) = seen else {
        return Vec::new();
    };
    let seen: HashSet<&str> = seen.iter().map(String::as_str).collect();
    liked
        .iter()
        .filter(|t| is_song(t) && !seen.contains(t.track_id.as_str()))
        .collect()
}

/// What looking for new likes did.
#[derive(Serialize)]
pub struct NewLikes {
    /// Your Liked songs list in Venuta, when songs went into it.
    pub name: Option<String>,
    /// Songs that joined it.
    pub added: usize,
}

/// With "New YouTube likes come in" on: the songs liked on the account since Venuta last
/// looked join Liked songs here. Nothing is ever taken out, here or there. The app asks at
/// most once per start, and when the switch is turned on (that first look only notes what is
/// there).
#[tauri::command]
pub async fn ytm_new_likes_in(pb: State<'_, Playback>, root: String) -> Result<NewLikes, String> {
    crate::ytm_library::account(&pb)?;
    let catalog = pb.catalog();
    crate::blocking(move || {
        let none = NewLikes {
            name: None,
            added: 0,
        };
        let account = crate::ytm_links::account_key(catalog.as_ref())?;
        if !engine::settings::load()
            .ytm_choices_for(&account)
            .new_likes_in
        {
            return Ok(none);
        }
        let liked = catalog
            .account_liked(&CancellationToken::default())
            .map_err(|e| e.to_string())?;
        let mut seen = load_seen();
        let fresh = fresh_likes(seen.get(&account).map(Vec::as_slice), &liked);
        let mut result = none;
        if !fresh.is_empty() {
            let rootp = PathBuf::from(&root);
            let (_, lib) = crate::load_lib(&rootp)?;
            let items: Vec<engine::playlists::Item> = fresh
                .iter()
                .map(|t| {
                    engine::playlists::Item::online(engine::playlists::OnlineEntry::from_remote(t))
                })
                .collect();
            let (name, added) = engine::playlists::like_many(&rootp, &lib, &items)?;
            result = NewLikes {
                name: Some(name),
                added,
            };
        }
        // Everything seen stays seen: a song unliked here doesn't come back on the next start.
        let kept = seen.entry(account).or_default();
        let had: HashSet<String> = kept.iter().cloned().collect();
        kept.extend(
            liked
                .iter()
                .map(|t| t.track_id.clone())
                .filter(|id| !had.contains(id)),
        );
        save_seen(&seen)?;
        Ok(result)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, video_type: Option<&str>) -> RemoteTrack {
        RemoteTrack {
            provider: "remote_catalog".into(),
            track_id: id.into(),
            title: id.into(),
            video_type: video_type.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn blocked_artists_are_hidden_only_while_the_switch_is_on() {
        let mut l = engine::settings::Listening::default();
        l.blocked_artists.push(engine::settings::BlockedArtist {
            name: "Someone".into(),
            artist_id: Some("UC1".into()),
        });
        assert_eq!(
            blocked_for_catalog(&l),
            [(Some("UC1".to_owned()), "Someone".to_owned())]
        );
        l.hide_blocked = false;
        assert!(blocked_for_catalog(&l).is_empty());
        assert_eq!(l.blocked_artists.len(), 1, "the list itself is kept");
    }

    #[test]
    fn the_first_look_brings_nothing_in() {
        let liked = [track("a", Some("ATV"))];
        assert!(fresh_likes(None, &liked).is_empty());
    }

    #[test]
    fn only_songs_liked_since_the_last_look_come_in() {
        let liked = [
            track("new", Some("ATV")),
            track("unknown", None),
            track("video", Some("OMV")),
            track("upload", Some("UGC")),
            track("episode", Some("PODCAST_EPISODE")),
            track("old", Some("ATV")),
        ];
        let seen = ["old".to_owned()];
        let ids: Vec<&str> = fresh_likes(Some(&seen), &liked)
            .iter()
            .map(|t| t.track_id.as_str())
            .collect();
        assert_eq!(ids, ["new", "unknown"]);
    }

    #[test]
    fn an_account_never_asked_is_shown_the_safe_choices() {
        let mut s = engine::settings::Settings::default();
        s.listening.report_plays = true;
        let v = view_of(&s, "me@example.com#own".into());
        assert!(!v.asked);
        assert_eq!(v.choices, YtmChoices::default());
        s.set_ytm_choices(
            "me@example.com#own",
            YtmChoices {
                count_plays: true,
                ..Default::default()
            },
        );
        let v = view_of(&s, "me@example.com#own".into());
        assert!(v.asked && v.choices.count_plays);
    }
}
