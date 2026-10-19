//! A playlist of yours kept the same as one on your YouTube Music account ("Also on YouTube
//! Music"). Your playlist stays the truth; this only works out what to change, so the app can
//! pace it, write it and say what it did.
//!
//! Songs are matched by video id. An online song is its id. A file of yours is the id you
//! confirmed for it in the matching review, or an upload's; without one it stays in Venuta
//! only, in its place, and never goes anywhere.
//!
//! Each sync is a three-way merge against what both sides had the last time (`base`):
//! - a song added on either side goes to the other;
//! - a song taken out on either side (it was in the base) goes from the other;
//! - order: the side that changed its order wins; if both did, Venuta's.
//!
//! Guards: a reading of the YouTube Music playlist that didn't finish changes nothing; one side
//! having lost most of its songs (or all) is never taken as deletions without asking; a link
//! made on another account waits.

use crate::playlists::{Item, OnlineEntry, PlaylistEntry};
use media_access::AccountTrack;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The catalog the links are with.
pub const PROVIDER: &str = "remote_catalog";

/// One playlist's link, kept in `<library>/.venuta/links/<playlist>.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// Your playlist's name.
    pub playlist: String,
    /// Which account (and which of its channels) it was linked on.
    pub account: String,
    /// The YouTube Music playlist (without "VL").
    pub playlist_id: String,
    /// Its name there when last seen.
    #[serde(default)]
    pub title: String,
    /// The video ids both sides had after the last sync, in order.
    #[serde(default)]
    pub base: Vec<String>,
    /// A file of yours (see `file_key`) to the YouTube Music song you said it is.
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
    /// Files you said stay in Venuta only (never asked about again).
    #[serde(default)]
    pub local_only: BTreeSet<String>,
    /// When it last synced (seconds since 1970); 0 never.
    #[serde(default)]
    pub last_sync: u64,
    /// You undid a sync: nothing more is sent until you say so (your playlist as it is now
    /// would take out there what the undone sync brought in).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    /// Files of yours sent to your uploads that YouTube Music hasn't shown yet (it takes a while):
    /// each sync looks for them among your uploads, and a song found is the file's from then on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uploading: BTreeMap<String, Uploading>,
    /// What the last syncs did, newest first (a few kept).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<SyncNote>,
}

/// What one sync did, for the playlist's page (and Undo, when yours changed).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncNote {
    /// Seconds since 1970.
    pub at: u64,
    /// Songs that came into yours, and went from yours.
    #[serde(default)]
    pub pulled: usize,
    #[serde(default)]
    pub dropped: usize,
    /// Songs added on YouTube Music, taken out there, and moved there.
    #[serde(default)]
    pub pushed: usize,
    #[serde(default)]
    pub removed: usize,
    #[serde(default)]
    pub moved: usize,
    /// Yours as it was before (a copy in Set aside), when this sync changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
    /// What went wrong part way, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// A file on its way into your uploads: what to know it by when it shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uploading {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    /// When it was sent (seconds since 1970).
    #[serde(default)]
    pub at: u64,
}

/// Syncs remembered per link.
const HISTORY: usize = 20;

impl Link {
    /// The uploads that have shown up, each taken as its file's song; how many.
    pub fn claim_uploads(&mut self, uploads: &[AccountTrack]) -> usize {
        let taken: HashSet<String> = self.matches.values().cloned().collect();
        let mut claimed = 0;
        let waiting: Vec<(String, Uploading)> = self
            .uploading
            .iter()
            .map(|(k, u)| (k.clone(), u.clone()))
            .collect();
        for (key, u) in waiting {
            let title = crate::playlists::title_key(&u.title);
            let artist = crate::audit::name_key(&u.artist);
            let hit = uploads.iter().find(|t| {
                !taken.contains(&t.track.track_id)
                    && !self.matches.values().any(|v| *v == t.track.track_id)
                    && crate::playlists::title_key(&t.track.title) == title
                    && (artist.is_empty()
                        || t.track
                            .artists
                            .iter()
                            .any(|a| crate::audit::name_key(a) == artist))
            });
            if let Some(t) = hit {
                self.matches.insert(key.clone(), t.track.track_id.clone());
                self.uploading.remove(&key);
                claimed += 1;
            }
        }
        claimed
    }

    /// Remember a sync that did something (the newest first, a few kept).
    pub fn noted(&mut self, n: SyncNote) {
        let did = n.pulled + n.dropped + n.pushed + n.removed + n.moved > 0 || n.problem.is_some();
        if did {
            self.history.insert(0, n);
            self.history.truncate(HISTORY);
        }
    }
}

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("links")
}

fn file(data_dir: &Path, playlist: &str) -> PathBuf {
    dir(data_dir).join(format!("{}.json", crate::plan::sanitize(playlist, 100)))
}

/// The link of this playlist, if it has one.
pub fn load(data_dir: &Path, playlist: &str) -> Option<Link> {
    let text = std::fs::read_to_string(file(data_dir, playlist)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Keep a link (written whole, then moved into place, so it's never half there).
pub fn save(data_dir: &Path, link: &Link) -> Result<(), String> {
    let to = file(data_dir, &link.playlist);
    std::fs::create_dir_all(dir(data_dir)).map_err(|e| e.to_string())?;
    let tmp = to.with_extension("json.part");
    let text = serde_json::to_string_pretty(link).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &to).map_err(|e| e.to_string())
}

/// Unlink: the link goes; both playlists stay as they are.
pub fn forget(data_dir: &Path, playlist: &str) -> Result<(), String> {
    match std::fs::remove_file(file(data_dir, playlist)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// The playlist was renamed: its link follows.
pub fn rename(data_dir: &Path, from: &str, to: &str) -> Result<(), String> {
    let Some(mut link) = load(data_dir, from) else {
        return Ok(());
    };
    link.playlist = to.to_owned();
    save(data_dir, &link)?;
    if file(data_dir, from) != file(data_dir, to) {
        forget(data_dir, from)?;
    }
    Ok(())
}

/// Every link there is.
pub fn all(data_dir: &Path) -> Vec<Link> {
    let Ok(rd) = std::fs::read_dir(dir(data_dir)) else {
        return Vec::new();
    };
    let mut out: Vec<Link> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str(&t).ok())
        .collect();
    out.sort_by(|a, b| a.playlist.cmp(&b.playlist));
    out
}

/// What a file of yours is known by in a link: its audio checksum (it survives a move), else
/// its path. None for a song that isn't a file here.
pub fn file_key(e: &PlaylistEntry) -> Option<String> {
    if e.missing || e.path.is_empty() {
        return None;
    }
    Some(match &e.audio_md5 {
        Some(m) => format!("md5:{m}"),
        None => format!("path:{}", e.path),
    })
}

/// The YouTube Music song a line of your playlist is, if any.
pub fn video_of(e: &PlaylistEntry, link: &Link) -> Option<String> {
    if let Some(o) = &e.online {
        return (o.provider == PROVIDER && o.is_stable()).then(|| o.track_id.clone());
    }
    file_key(e).and_then(|k| link.matches.get(&k).cloned())
}

/// The files of yours the matching review asks about: no YouTube Music song yet, and not
/// kept to Venuta, nor on its way to your uploads. Their places in the playlist.
pub fn unmatched(entries: &[PlaylistEntry], link: &Link) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.online.is_none())
        .filter_map(|(i, e)| file_key(e).map(|k| (i, k)))
        .filter(|(_, k)| {
            !link.matches.contains_key(k)
                && !link.local_only.contains(k)
                && !link.uploading.contains_key(k)
        })
        .map(|(i, _)| i)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Venuta,
    YoutubeMusic,
}

/// Why a sync changed nothing and waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pause {
    /// The YouTube Music playlist couldn't be read to the end.
    Unread,
    /// You undid a sync; the next one waits for you.
    Held,
    /// Linked on another account (or another of its channels).
    OtherAccount { account: String },
    /// One side has lost most of what it had (or all): asked before anything goes.
    ManyGone { side: Side, gone: usize, of: usize },
}

/// What a sync does, worked out before anything is sent or written.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Your playlist after the merge; None when it stays as it is.
    pub items: Option<Vec<Item>>,
    /// The songs both sides end with, in their order: the next base.
    pub order: Vec<String>,
    /// To add on YouTube Music (they land at the end; `moves` puts them in place).
    pub add_remote: Vec<String>,
    /// To take out on YouTube Music: (video id, its place handle).
    pub remove_remote: Vec<(String, String)>,
    /// Songs that come into your playlist from YouTube Music.
    pub pulled: usize,
    /// Songs that go from your playlist (taken out on YouTube Music).
    pub dropped: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.items.is_none() && self.add_remote.is_empty() && self.remove_remote.is_empty()
    }
}

/// Each id once, in order (a song twice keeps its first place for the link).
fn firsts<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    ids.filter(|id| !id.is_empty() && seen.insert(*id))
        .collect()
}

/// The three-way merge (see the module's words). `account` is who is signed in now.
pub fn plan(
    entries: &[PlaylistEntry],
    link: &Link,
    account: &str,
    remote: &[AccountTrack],
    complete: bool,
    force: bool,
) -> Result<Plan, Pause> {
    if !link.account.is_empty() && link.account != account {
        return Err(Pause::OtherAccount {
            account: link.account.clone(),
        });
    }
    if link.held && !force {
        return Err(Pause::Held);
    }
    if !complete {
        return Err(Pause::Unread);
    }
    // Each line's song, its first time only (a second time stays as a line of yours).
    let mut seen = HashSet::new();
    let local: Vec<Option<String>> = entries
        .iter()
        .map(|e| video_of(e, link).filter(|v| seen.insert(v.clone())))
        .collect();
    let lids: Vec<&str> = local.iter().flatten().map(String::as_str).collect();
    let rids = firsts(remote.iter().map(|r| r.track.track_id.as_str()));
    let base: HashSet<&str> = link.base.iter().map(String::as_str).collect();
    let in_l: HashSet<&str> = lids.iter().copied().collect();
    let in_r: HashSet<&str> = rids.iter().copied().collect();

    if !force && !base.is_empty() {
        let lost = |have: &HashSet<&str>| base.iter().filter(|b| !have.contains(*b)).count();
        let many = |have: &[&str], gone: usize| {
            have.is_empty() || (base.len() >= 4 && gone * 2 > base.len())
        };
        let gone_r = lost(&in_r);
        if many(&rids, gone_r) && gone_r > 0 {
            return Err(Pause::ManyGone {
                side: Side::YoutubeMusic,
                gone: gone_r,
                of: base.len(),
            });
        }
        let gone_l = lost(&in_l);
        if many(&lids, gone_l) && gone_l > 0 {
            return Err(Pause::ManyGone {
                side: Side::Venuta,
                gone: gone_l,
                of: base.len(),
            });
        }
    }

    let keep = |id: &str| match (in_l.contains(id), in_r.contains(id)) {
        (true, true) => true,
        // New on one side stays (and goes to the other); gone from one side goes.
        (true, false) | (false, true) => !base.contains(id),
        (false, false) => false,
    };
    // Did a side change the order of the songs it shares with the base?
    let reordered = |ids: &[&str]| {
        let have: HashSet<&str> = ids.iter().copied().collect();
        let now: Vec<&str> = ids.iter().copied().filter(|i| base.contains(i)).collect();
        let was: Vec<&str> = link
            .base
            .iter()
            .map(String::as_str)
            .filter(|b| have.contains(b))
            .collect();
        now != was
    };
    let (spine, other) = if !reordered(&lids) && reordered(&rids) {
        (&rids, &lids)
    } else {
        (&lids, &rids)
    };
    let mut order: Vec<&str> = spine.iter().copied().filter(|i| keep(i)).collect();
    // The other side's new songs, each right after the song it follows there.
    let mut at = 0;
    for id in other.iter().copied().filter(|i| keep(i)) {
        match order.iter().position(|o| *o == id) {
            Some(p) => at = p + 1,
            None => {
                order.insert(at, id);
                at += 1;
            }
        }
    }

    // Your playlist: the merged songs in order, and each line without a song kept right after
    // the line of yours it followed.
    let line_of: HashMap<&str, usize> = local
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.as_deref().map(|v| (v, i)))
        .collect();
    let track_of: HashMap<&str, &AccountTrack> = remote
        .iter()
        .map(|r| (r.track.track_id.as_str(), r))
        .collect();
    let mut lines: Vec<Option<usize>> = Vec::new();
    let mut new_items: HashMap<usize, Item> = HashMap::new();
    let mut pulled = 0;
    for id in &order {
        match line_of.get(id) {
            Some(&j) => lines.push(Some(j)),
            None => {
                let Some(t) = track_of.get(id) else { continue };
                pulled += 1;
                new_items.insert(
                    lines.len(),
                    Item::online(OnlineEntry::from_remote(&t.track)),
                );
                lines.push(None);
            }
        }
    }
    let mut dropped = 0;
    for (j, v) in local.iter().enumerate() {
        match v {
            Some(v) if !keep(v) => dropped += 1,
            Some(_) => {}
            None => {
                let after = (0..j)
                    .rev()
                    .find_map(|k| lines.iter().position(|l| *l == Some(k)))
                    .map_or(0, |p| p + 1);
                // Shift the pulled songs' places that come after it.
                let moved: HashMap<usize, Item> = new_items
                    .drain()
                    .map(|(p, i)| (if p >= after { p + 1 } else { p }, i))
                    .collect();
                new_items = moved;
                lines.insert(after, Some(j));
            }
        }
    }
    let same = lines.len() == entries.len() && lines.iter().enumerate().all(|(i, l)| *l == Some(i));
    let items = (!same).then(|| {
        lines
            .iter()
            .enumerate()
            .map(|(p, l)| match l {
                Some(j) => entries[*j].item(),
                None => new_items.remove(&p).unwrap_or_default(),
            })
            .collect()
    });

    let kept: HashSet<&str> = order.iter().copied().collect();
    let remove_remote = remote
        .iter()
        .filter(|r| !kept.contains(r.track.track_id.as_str()))
        .filter_map(|r| {
            r.set_video_id
                .clone()
                .map(|s| (r.track.track_id.clone(), s))
        })
        .collect();
    let add_remote = order
        .iter()
        .filter(|i| !in_r.contains(*i))
        .map(|i| i.to_string())
        .collect();
    Ok(Plan {
        items,
        order: order.iter().map(|s| s.to_string()).collect(),
        add_remote,
        remove_remote,
        pulled,
        dropped,
    })
}

/// The fewest "this item before that one" moves that turn `prev` into `next` (the same
/// handles): the longest run already in order stays, each other moves, last first, before
/// the item that follows it in the new order (None: to the end).
pub fn moves(prev: &[String], next: &[String]) -> Vec<(String, Option<String>)> {
    let at: HashMap<&str, usize> = prev
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let seq: Vec<Option<usize>> = next.iter().map(|s| at.get(s.as_str()).copied()).collect();
    let mut tails: Vec<usize> = Vec::new();
    let mut tail_at: Vec<usize> = Vec::new();
    let mut back: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, v) in seq.iter().enumerate() {
        let Some(v) = *v else { continue };
        let lo = tails.partition_point(|t| *t < v);
        if lo == tails.len() {
            tails.push(v);
            tail_at.push(i);
        } else {
            tails[lo] = v;
            tail_at[lo] = i;
        }
        back[i] = lo.checked_sub(1).map(|l| tail_at[l]);
    }
    let mut stay = HashSet::new();
    let mut i = tail_at.last().copied();
    while let Some(k) = i {
        stay.insert(k);
        i = back[k];
    }
    (0..next.len())
        .rev()
        .filter(|i| !stay.contains(i) && seq[*i].is_some())
        .map(|i| (next[i].clone(), next.get(i + 1).cloned()))
        .collect()
}

/// The place handles of a YouTube Music playlist after `plan`'s removals and additions (added
/// songs land at the end, in the order sent), and the handles in the merged order: what
/// `moves` turns one into the other with.
pub fn after_edits(
    remote: &[AccountTrack],
    removed: &[(String, String)],
    placed: &[(String, String)],
    order: &[String],
) -> (Vec<String>, Vec<String>) {
    let gone: HashSet<&str> = removed.iter().map(|(_, s)| s.as_str()).collect();
    let mut now: Vec<(String, String)> = remote
        .iter()
        .filter_map(|r| Some((r.track.track_id.clone(), r.set_video_id.clone()?)))
        .filter(|(_, s)| !gone.contains(s.as_str()))
        .collect();
    now.extend(placed.iter().cloned());
    let mut handle: HashMap<&str, &str> = HashMap::new();
    for (v, s) in &now {
        handle.entry(v.as_str()).or_insert(s.as_str());
    }
    let wanted: Vec<String> = order
        .iter()
        .filter_map(|v| handle.get(v.as_str()).map(|s| s.to_string()))
        .collect();
    let set: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let current = now
        .iter()
        .map(|(_, s)| s.clone())
        .filter(|s| set.contains(s.as_str()))
        .collect();
    (current, wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_access::RemoteTrack;

    fn online(id: &str) -> PlaylistEntry {
        PlaylistEntry {
            path: String::new(),
            title: id.to_uppercase(),
            artist: "Someone".into(),
            album: String::new(),
            duration_ms: 0,
            audio_md5: None,
            missing: true,
            want: None,
            maybe: None,
            online: Some(OnlineEntry {
                provider: PROVIDER.into(),
                track_id: id.into(),
                title: id.to_uppercase(),
                artists: vec!["Someone".into()],
                album: None,
                duration_ms: None,
                isrc: None,
                artwork_url: None,
            }),
        }
    }

    fn file(path: &str, md5: &str) -> PlaylistEntry {
        PlaylistEntry {
            path: path.into(),
            title: path.into(),
            artist: "Someone".into(),
            album: String::new(),
            duration_ms: 0,
            audio_md5: Some(md5.into()),
            missing: false,
            want: None,
            maybe: None,
            online: None,
        }
    }

    fn remote(ids: &[&str]) -> Vec<AccountTrack> {
        ids.iter()
            .map(|id| AccountTrack {
                track: RemoteTrack {
                    provider: PROVIDER.into(),
                    track_id: (*id).into(),
                    title: id.to_uppercase(),
                    ..Default::default()
                },
                set_video_id: Some(format!("s-{id}")),
                upload: false,
            })
            .collect()
    }

    fn link(base: &[&str]) -> Link {
        Link {
            playlist: "Road".into(),
            account: "me".into(),
            playlist_id: "PL1".into(),
            base: base.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// What each line of the merged playlist is: its video id, or its file.
    fn lines(p: &Plan, entries: &[PlaylistEntry]) -> Vec<String> {
        let items: Vec<Item> = p
            .items
            .clone()
            .unwrap_or_else(|| entries.iter().map(|e| e.item()).collect());
        items
            .iter()
            .map(|i| match (&i.online, &i.path) {
                (Some(o), _) => o.track_id.clone(),
                (None, Some(p)) => p.clone(),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn linking_for_the_first_time_joins_both_and_takes_nothing_out() {
        let mine = [online("a"), online("b")];
        let p = plan(&mine, &link(&[]), "me", &remote(&["b", "c"]), true, false).unwrap();
        assert_eq!(p.order, ["a", "b", "c"]);
        assert_eq!(p.add_remote, ["a"]);
        assert!(p.remove_remote.is_empty());
        assert_eq!(lines(&p, &mine), ["a", "b", "c"]);
        assert_eq!((p.pulled, p.dropped), (1, 0));
    }

    #[test]
    fn a_song_added_on_youtube_music_comes_in_after_the_song_it_follows() {
        let mine = [online("a"), online("b")];
        let p = plan(
            &mine,
            &link(&["a", "b"]),
            "me",
            &remote(&["a", "x", "b"]),
            true,
            false,
        )
        .unwrap();
        assert_eq!(lines(&p, &mine), ["a", "x", "b"]);
        assert!(p.add_remote.is_empty() && p.remove_remote.is_empty());
    }

    #[test]
    fn a_song_taken_out_on_youtube_music_goes_from_yours() {
        let mine = [online("a"), online("b"), online("c")];
        let p = plan(
            &mine,
            &link(&["a", "b", "c"]),
            "me",
            &remote(&["a", "c"]),
            true,
            false,
        )
        .unwrap();
        assert_eq!(lines(&p, &mine), ["a", "c"]);
        assert_eq!(p.dropped, 1);
    }

    #[test]
    fn a_song_taken_out_here_goes_from_youtube_music() {
        let mine = [online("a"), online("c")];
        let p = plan(
            &mine,
            &link(&["a", "b", "c"]),
            "me",
            &remote(&["a", "b", "c"]),
            true,
            false,
        )
        .unwrap();
        assert_eq!(p.remove_remote, [("b".to_string(), "s-b".to_string())]);
        assert!(p.items.is_none());
    }

    #[test]
    fn nothing_changed_is_nothing_to_do() {
        let mine = [online("a"), online("b")];
        let p = plan(
            &mine,
            &link(&["a", "b"]),
            "me",
            &remote(&["a", "b"]),
            true,
            false,
        )
        .unwrap();
        assert!(p.is_empty());
    }

    #[test]
    fn a_reading_that_did_not_finish_changes_nothing() {
        let mine = [online("a")];
        let r = plan(
            &mine,
            &link(&["a", "b"]),
            "me",
            &remote(&["a"]),
            false,
            true,
        );
        assert_eq!(r.err(), Some(Pause::Unread));
    }

    #[test]
    fn an_empty_answer_is_never_taken_as_every_song_deleted() {
        let mine = [online("a"), online("b")];
        let r = plan(&mine, &link(&["a", "b"]), "me", &[], true, false);
        assert_eq!(
            r.err(),
            Some(Pause::ManyGone {
                side: Side::YoutubeMusic,
                gone: 2,
                of: 2
            })
        );
    }

    #[test]
    fn most_songs_gone_waits_to_be_asked_and_goes_through_when_you_say_so() {
        let mine: Vec<_> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|i| online(i))
            .collect();
        let l = link(&["a", "b", "c", "d", "e"]);
        let r = remote(&["a", "b"]);
        assert!(matches!(
            plan(&mine, &l, "me", &r, true, false),
            Err(Pause::ManyGone {
                side: Side::YoutubeMusic,
                gone: 3,
                of: 5
            })
        ));
        let p = plan(&mine, &l, "me", &r, true, true).unwrap();
        assert_eq!(lines(&p, &mine), ["a", "b"]);
    }

    #[test]
    fn your_playlist_emptied_waits_too() {
        let r = plan(
            &[],
            &link(&["a", "b"]),
            "me",
            &remote(&["a", "b"]),
            true,
            false,
        );
        assert!(matches!(
            r,
            Err(Pause::ManyGone {
                side: Side::Venuta,
                ..
            })
        ));
    }

    #[test]
    fn after_an_undo_nothing_is_sent_until_you_say_so() {
        let mut l = link(&["a", "b"]);
        l.held = true;
        let r = plan(&[online("a")], &l, "me", &remote(&["a", "b"]), true, false);
        assert_eq!(r.err(), Some(Pause::Held));
        let p = plan(&[online("a")], &l, "me", &remote(&["a", "b"]), true, true).unwrap();
        assert_eq!(p.remove_remote.len(), 1);
    }

    #[test]
    fn a_link_made_on_another_account_waits() {
        let r = plan(
            &[online("a")],
            &link(&["a"]),
            "someone else",
            &remote(&["a"]),
            true,
            false,
        );
        assert_eq!(
            r.err(),
            Some(Pause::OtherAccount {
                account: "me".into()
            })
        );
    }

    #[test]
    fn order_changed_only_on_youtube_music_is_taken() {
        let mine = [online("a"), online("b"), online("c")];
        let p = plan(
            &mine,
            &link(&["a", "b", "c"]),
            "me",
            &remote(&["c", "a", "b"]),
            true,
            false,
        )
        .unwrap();
        assert_eq!(p.order, ["c", "a", "b"]);
        assert_eq!(lines(&p, &mine), ["c", "a", "b"]);
    }

    #[test]
    fn order_changed_on_both_sides_keeps_yours() {
        let mine = [online("b"), online("a"), online("c")];
        let p = plan(
            &mine,
            &link(&["a", "b", "c"]),
            "me",
            &remote(&["c", "a", "b"]),
            true,
            false,
        )
        .unwrap();
        assert_eq!(p.order, ["b", "a", "c"]);
        assert!(p.items.is_none());
    }

    #[test]
    fn files_without_a_song_there_keep_their_place_and_matched_ones_go_across() {
        let mine = [
            online("a"),
            file("x.flac", "M1"),
            file("y.flac", "M2"),
            online("b"),
        ];
        let mut l = link(&["a", "b"]);
        l.matches.insert("md5:M2".into(), "y".into());
        let p = plan(&mine, &l, "me", &remote(&["a", "n", "b"]), true, false).unwrap();
        assert_eq!(p.add_remote, ["y"]);
        assert_eq!(lines(&p, &mine), ["a", "x.flac", "n", "y.flac", "b"]);
        assert_eq!(unmatched(&mine, &l), [1]);
        // One on its way to your uploads isn't asked about again.
        l.uploading.insert("md5:M1".into(), Uploading::default());
        assert!(unmatched(&mine, &l).is_empty());
    }

    #[test]
    fn a_song_twice_here_is_sent_once_and_both_lines_stay() {
        let mine = [online("a"), online("a")];
        let p = plan(&mine, &link(&[]), "me", &[], true, false).unwrap();
        assert_eq!(p.add_remote, ["a"]);
        assert!(p.items.is_none());
    }

    fn apply(order: &[String], moves: &[(String, Option<String>)]) -> Vec<String> {
        let mut out = order.to_vec();
        for (m, before) in moves {
            let from = out.iter().position(|x| x == m).unwrap();
            let it = out.remove(from);
            let to = before
                .as_ref()
                .and_then(|b| out.iter().position(|x| x == b))
                .unwrap_or(out.len());
            out.insert(to, it);
        }
        out
    }

    #[test]
    fn moves_give_exactly_the_new_order_with_few_steps() {
        let v = |s: &str| s.chars().map(|c| c.to_string()).collect::<Vec<_>>();
        for (a, b) in [
            ("abcde", "edcba"),
            ("abcdef", "badcfe"),
            ("abcd", "cadb"),
            ("abcd", "abcd"),
            ("abcd", "dabc"),
        ] {
            let m = moves(&v(a), &v(b));
            assert_eq!(apply(&v(a), &m), v(b), "{a} -> {b}");
            assert!(m.len() < a.len());
        }
        assert_eq!(
            moves(&v("abcd"), &v("dabc")),
            [("d".to_string(), Some("a".to_string()))]
        );
    }

    #[test]
    fn after_edits_lines_up_handles_for_the_moves() {
        let r = remote(&["a", "b", "c"]);
        let removed = [("b".to_string(), "s-b".to_string())];
        let placed = [("n".to_string(), "s-n".to_string())];
        let order: Vec<String> = ["n", "a", "c"].iter().map(|s| s.to_string()).collect();
        let (now, want) = after_edits(&r, &removed, &placed, &order);
        assert_eq!(now, ["s-a", "s-c", "s-n"]);
        assert_eq!(want, ["s-n", "s-a", "s-c"]);
        assert_eq!(apply(&now, &moves(&now, &want)), want);
    }

    #[test]
    fn an_upload_that_has_shown_up_becomes_its_files_song() {
        let mut l = link(&[]);
        l.uploading.insert(
            "md5:M1".into(),
            Uploading {
                title: "Night Drive".into(),
                artist: "Someone".into(),
                at: 1,
            },
        );
        l.uploading.insert(
            "md5:M2".into(),
            Uploading {
                title: "Not yet".into(),
                artist: String::new(),
                at: 1,
            },
        );
        let mut ups = remote(&["u1"]);
        ups[0].track.title = "Night Drive".into();
        ups[0].track.artists = vec!["Someone".into()];
        ups[0].upload = true;
        assert_eq!(l.claim_uploads(&ups), 1);
        assert_eq!(l.matches.get("md5:M1").map(String::as_str), Some("u1"));
        assert!(l.uploading.contains_key("md5:M2"));
        assert_eq!(l.claim_uploads(&ups), 0);
    }

    #[test]
    fn a_link_is_kept_found_renamed_and_forgotten() {
        let d = tempfile::tempdir().unwrap();
        let mut l = link(&["a"]);
        save(d.path(), &l).unwrap();
        assert_eq!(load(d.path(), "Road"), Some(l.clone()));
        rename(d.path(), "Road", "Long road").unwrap();
        assert!(load(d.path(), "Road").is_none());
        l.playlist = "Long road".into();
        assert_eq!(all(d.path()), [l]);
        forget(d.path(), "Long road").unwrap();
        assert!(all(d.path()).is_empty());
        forget(d.path(), "Long road").unwrap();
    }
}
