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
//! made on another account waits. A file of yours that can't be found now is still in your
//! playlist, so its song stays there; and after an Undo, what the undone sync brought in comes
//! in again rather than going there. A song goes from YouTube Music only when you took it out of
//! yours since the last sync.

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
    /// A song YouTube Music put in when another was sent (it sometimes swaps in another
    /// version): its id there, to the id sent. The two are one song for the link.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
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
    /// What YouTube Music actually added for the songs sent (in the order sent): a different
    /// id is remembered as the same song.
    pub fn note_swaps(&mut self, sent: &[String], placed: &[(String, String)]) {
        if sent.len() != placed.len() {
            return;
        }
        for (want, (got, _)) in sent.iter().zip(placed) {
            if want != got {
                self.aliases.insert(got.clone(), want.clone());
            }
        }
    }

    /// A song's id there, as the link knows it.
    fn id_there<'a>(&'a self, r: &'a AccountTrack) -> &'a str {
        self.aliases
            .get(&r.track.track_id)
            .map_or(r.track.track_id.as_str(), String::as_str)
    }

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

/// Where a deleted playlist's link waits for Undo, under the name of its file in Set aside
/// (which is never taken twice). Not read by `all`, so it never syncs.
fn aside_file(data_dir: &Path, aside: &Path) -> PathBuf {
    let of = aside
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir(data_dir)
        .join("set-aside")
        .join(format!("{}.json", crate::plan::sanitize(&of, 140)))
}

/// The playlist was deleted (its file went to `aside` in Set aside): its link goes with it. A new
/// playlist given the same name then starts unlinked, and only Undo brings this link back.
pub fn deleted(data_dir: &Path, playlist: &str, aside: &Path) -> Result<(), String> {
    let from = file(data_dir, playlist);
    if !from.is_file() {
        return Ok(());
    }
    let to = aside_file(data_dir, aside);
    std::fs::create_dir_all(to.parent().unwrap_or(data_dir)).map_err(|e| e.to_string())?;
    std::fs::rename(&from, &to).map_err(|e| e.to_string())
}

/// Undo of a delete: the link comes back with its playlist (never over a link made since).
pub fn undeleted(data_dir: &Path, playlist: &str, aside: &Path) -> Result<(), String> {
    let from = aside_file(data_dir, aside);
    let to = file(data_dir, playlist);
    if !from.is_file() || to.exists() {
        return Ok(());
    }
    std::fs::rename(&from, &to).map_err(|e| e.to_string())
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

/// What a line of yours that is a file is known by, found or not: a file that can't be found now
/// (a drive unplugged, a NAS asleep, moved before a rescan) is still known by the checksum its
/// line was written with. None for a line with no way to tell which file it was.
fn line_key(e: &PlaylistEntry) -> Option<String> {
    if !e.missing {
        return file_key(e);
    }
    let md5 = e
        .audio_md5
        .clone()
        .or_else(|| e.want.as_ref().and_then(|w| w.md5.clone()))?;
    Some(format!("md5:{}", md5.to_uppercase()))
}

/// The YouTube Music song a line of your playlist is, if any. A matched file that can't be
/// found now is still its song: it is in your playlist, so it isn't taken out anywhere.
pub fn video_of(e: &PlaylistEntry, link: &Link) -> Option<String> {
    if let Some(o) = &e.online {
        return (o.provider == PROVIDER && o.is_stable()).then(|| o.track_id.clone());
    }
    line_key(e).and_then(|k| link.matches.get(&k).cloned())
}

/// You undid a sync: yours is back as it was before it. The songs that sync brought in are no
/// longer ones both sides had, so the next sync brings them in again rather than taking them out
/// on YouTube Music; and the link waits for you (`held`). A song the undone sync took out of
/// yours (gone there) is yours again, so it goes back there.
pub fn undone(link: &mut Link, entries: &[PlaylistEntry]) {
    let have: HashSet<String> = entries.iter().filter_map(|e| video_of(e, link)).collect();
    link.base.retain(|id| have.contains(id));
    link.held = true;
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

/// Every file of yours in the playlist that isn't an online song: what "Check matches" lists.
pub fn files(entries: &[PlaylistEntry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.online.is_none() && file_key(e).is_some())
        .map(|(i, _)| i)
        .collect()
}

/// How the playlist stands: songs that are (or go) there, and lines that stay here only.
pub fn counts(entries: &[PlaylistEntry], link: &Link) -> (usize, usize) {
    let there = entries
        .iter()
        .filter(|e| video_of(e, link).is_some())
        .count();
    (there, entries.len() - there)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Venuta,
    YoutubeMusic,
}

/// What the reading of the playlist there says about itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Read {
    /// Every page came.
    pub complete: bool,
    /// Songs it holds that YouTube didn't list: unavailable ones it hides. A song of the last
    /// sync missing from the list may be one of these, so it is never taken as taken out.
    pub hidden: usize,
}

impl From<bool> for Read {
    fn from(complete: bool) -> Self {
        Read {
            complete,
            hidden: 0,
        }
    }
}

/// What you said go ahead past, after a sync waited and asked. Each answer covers only the pause
/// it was given for: "Sync again" after an Undo still asks when most songs are gone, and "Sync
/// anyway" for one side still asks about the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Allow {
    /// "Sync again", after an Undo.
    pub held: bool,
    /// "Sync anyway": most songs gone on this side are taken as gone.
    pub many_gone: Option<Side>,
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
    read: impl Into<Read>,
    allow: Allow,
) -> Result<Plan, Pause> {
    let read = read.into();
    if !link.account.is_empty() && link.account != account {
        return Err(Pause::OtherAccount {
            account: link.account.clone(),
        });
    }
    if link.held && !allow.held {
        return Err(Pause::Held);
    }
    if !read.complete {
        return Err(Pause::Unread);
    }
    // Each line's song, its first time only (a second time stays as a line of yours).
    let mut seen = HashSet::new();
    let local: Vec<Option<String>> = entries
        .iter()
        .map(|e| video_of(e, link).filter(|v| seen.insert(v.clone())))
        .collect();
    let lids: Vec<&str> = local.iter().flatten().map(String::as_str).collect();
    let rids = firsts(remote.iter().map(|r| link.id_there(r)));
    let base: HashSet<&str> = link.base.iter().map(String::as_str).collect();
    // A file of yours that can't be found now and was matched by its path (no checksum): its
    // line can't say which song it is, so every song matched by a path no line has now counts
    // as still yours. It is never taken out there, and never comes in as a second line.
    let unknown_line = entries
        .iter()
        .any(|e| e.online.is_none() && e.missing && line_key(e).is_none());
    let keys_now: HashSet<String> = entries.iter().filter_map(line_key).collect();
    let still_yours: HashSet<&str> = if unknown_line {
        link.matches
            .iter()
            .filter(|(k, _)| k.starts_with("path:") && !keys_now.contains(*k))
            .map(|(_, v)| v.as_str())
            .collect()
    } else {
        HashSet::new()
    };
    let in_l: HashSet<&str> = lids
        .iter()
        .copied()
        .chain(still_yours.iter().copied())
        .collect();
    let listed: HashSet<&str> = rids.iter().copied().collect();
    // The playlist there hides songs YouTube can't play now (its header counts more than it
    // lists). A song of the last sync still here but not listed there may be one of them: it
    // stays, here and in the base, and is never sent again (it is there already, hidden). When
    // more are missing than are hidden, some were taken out, but not which: all stay.
    let maybe_hidden: Vec<&str> = if read.hidden > 0 {
        base.iter()
            .copied()
            .filter(|b| in_l.contains(b) && !listed.contains(b))
            .collect()
    } else {
        Vec::new()
    };
    let in_r: HashSet<&str> = listed.iter().copied().chain(maybe_hidden).collect();

    if !base.is_empty() {
        let lost = |have: &HashSet<&str>| base.iter().filter(|b| !have.contains(*b)).count();
        let many = |have: &HashSet<&str>, gone: usize| {
            have.is_empty() || (base.len() >= 4 && gone * 2 > base.len())
        };
        let gone_r = lost(&in_r);
        if many(&in_r, gone_r) && gone_r > 0 && allow.many_gone != Some(Side::YoutubeMusic) {
            return Err(Pause::ManyGone {
                side: Side::YoutubeMusic,
                gone: gone_r,
                of: base.len(),
            });
        }
        let gone_l = lost(&in_l);
        if many(&in_l, gone_l) && gone_l > 0 && allow.many_gone != Some(Side::Venuta) {
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
    let track_of: HashMap<&str, &AccountTrack> =
        remote.iter().map(|r| (link.id_there(r), r)).collect();
    let mut lines: Vec<Option<usize>> = Vec::new();
    let mut new_items: HashMap<usize, Item> = HashMap::new();
    let mut pulled = 0;
    for id in &order {
        match line_of.get(id) {
            Some(&j) => lines.push(Some(j)),
            None if still_yours.contains(id) => {}
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
        .filter(|r| !kept.contains(link.id_there(r)))
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
    aliases: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let same = |v: &str| aliases.get(v).cloned().unwrap_or_else(|| v.to_owned());
    let gone: HashSet<&str> = removed.iter().map(|(_, s)| s.as_str()).collect();
    let mut now: Vec<(String, String)> = remote
        .iter()
        .filter_map(|r| Some((same(&r.track.track_id), r.set_video_id.clone()?)))
        .filter(|(_, s)| !gone.contains(s.as_str()))
        .collect();
    now.extend(placed.iter().map(|(v, s)| (same(v), s.clone())));
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
                added_by: None,
                votes: None,
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

    /// Nothing gone past: the sync that nobody answered yet.
    const ASK: Allow = Allow {
        held: false,
        many_gone: None,
    };
    /// "Sync again" after an Undo.
    const HELD: Allow = Allow {
        held: true,
        many_gone: None,
    };
    /// Past every pause there is an answer for (only for guards that come before them all).
    const ANYWAY: Allow = Allow {
        held: true,
        many_gone: Some(Side::YoutubeMusic),
    };
    /// "Sync anyway" when most songs went on one side.
    fn gone(side: Side) -> Allow {
        Allow {
            held: false,
            many_gone: Some(side),
        }
    }

    /// A file of yours that can't be found now (a drive unplugged), its line as written.
    fn lost_file(md5: &str) -> PlaylistEntry {
        PlaylistEntry {
            path: String::new(),
            title: "Night Drive".into(),
            artist: "Someone".into(),
            album: String::new(),
            duration_ms: 0,
            audio_md5: Some(md5.into()),
            missing: true,
            want: Some(crate::playlists::WantEntry {
                title: "Night Drive".into(),
                md5: Some(md5.into()),
                ..Default::default()
            }),
            maybe: None,
            online: None,
        }
    }

    // The bug: Undo put yours back but kept the base of the sync it undid, so "Sync again" took
    // out on YouTube Music the songs that sync had brought in from there.
    #[test]
    fn undo_then_sync_again_takes_nothing_out_on_youtube_music() {
        let before = [online("a"), online("b")];
        let mut l = link(&["a", "b"]);
        let there = remote(&["a", "x", "b"]);
        let p = plan(&before, &l, "me", &there, true, ASK).unwrap();
        assert_eq!(p.pulled, 1);
        l.base = p.order.clone();

        // Undo: yours is back as it was before that sync.
        undone(&mut l, &before);
        assert!(l.held);
        assert_eq!(
            plan(&before, &l, "me", &there, true, ASK).err(),
            Some(Pause::Held)
        );
        let again = plan(&before, &l, "me", &there, true, HELD).unwrap();
        assert!(again.remove_remote.is_empty(), "{:?}", again.remove_remote);
        assert_eq!(lines(&again, &before), ["a", "x", "b"], "it comes in again");
    }

    #[test]
    fn sync_again_after_an_undo_still_asks_when_most_are_gone_there() {
        let mine: Vec<_> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|i| online(i))
            .collect();
        let mut l = link(&["a", "b", "c", "d", "e"]);
        undone(&mut l, &mine);
        assert_eq!(
            plan(&mine, &l, "me", &remote(&["a"]), true, HELD).err(),
            Some(Pause::ManyGone {
                side: Side::YoutubeMusic,
                gone: 4,
                of: 5
            })
        );
    }

    // "Sync anyway" answers the side it was asked about; the other side still asks.
    #[test]
    fn going_past_one_sides_losses_still_asks_about_the_other() {
        let mine = [online("a"), online("c")];
        let l = link(&["a", "b", "c", "d", "e"]);
        let there = remote(&["a", "b"]);
        assert_eq!(
            plan(&mine, &l, "me", &there, true, gone(Side::YoutubeMusic)).err(),
            Some(Pause::ManyGone {
                side: Side::Venuta,
                gone: 3,
                of: 5
            })
        );
    }

    // A matched file that can't be found now is still in your playlist: its song stays there.
    #[test]
    fn a_matched_file_that_cant_be_found_now_keeps_its_song_there() {
        let mine = [online("a"), lost_file("M2")];
        let mut l = link(&["a", "y"]);
        l.matches.insert("md5:M2".into(), "y".into());
        let p = plan(&mine, &l, "me", &remote(&["a", "y"]), true, ASK).unwrap();
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(counts(&mine, &l), (2, 0));
        assert!(unmatched(&mine, &l).is_empty(), "and it isn't asked about");

        // Its line written again as a song to get keeps only the checksum it wants.
        let mut want_only = lost_file("M2");
        want_only.audio_md5 = None;
        let mine = [online("a"), want_only];
        let p = plan(&mine, &l, "me", &remote(&["a", "y"]), true, ASK).unwrap();
        assert!(p.is_empty(), "{p:?}");
    }

    // Known by its path only, a missing file's song can't be told from the others: every song
    // matched by a path no line has now stays there, and none comes in as a second line.
    #[test]
    fn a_missing_file_known_by_its_path_only_keeps_its_song_there() {
        let mut lost = lost_file("M");
        lost.audio_md5 = None;
        lost.want.as_mut().unwrap().md5 = None;
        let mine = [online("a"), lost];
        let mut l = link(&["a", "y"]);
        l.matches.insert("path:A/y.flac".into(), "y".into());
        let p = plan(&mine, &l, "me", &remote(&["a", "y"]), true, ASK).unwrap();
        assert!(p.remove_remote.is_empty(), "{:?}", p.remove_remote);
        assert!(p.items.is_none());
        assert_eq!(p.order, ["a", "y"]);
    }

    #[test]
    fn linking_for_the_first_time_joins_both_and_takes_nothing_out() {
        let mine = [online("a"), online("b")];
        let p = plan(&mine, &link(&[]), "me", &remote(&["b", "c"]), true, ASK).unwrap();
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
            ASK,
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
            ASK,
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
            ASK,
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
            ASK,
        )
        .unwrap();
        assert!(p.is_empty());
    }

    #[test]
    fn a_song_youtube_hides_now_stays_yours_and_is_not_sent_again() {
        let mine = [online("a"), online("b"), online("c")];
        let l = link(&["a", "b", "c"]);
        let there = remote(&["a", "c"]);
        let hides_one = Read {
            complete: true,
            hidden: 1,
        };
        let p = plan(&mine, &l, "me", &there, hides_one, Allow::default()).unwrap();
        assert_eq!(lines(&p, &mine), ["a", "b", "c"]);
        assert_eq!(p.dropped, 0);
        assert!(p.add_remote.is_empty(), "it is there already, hidden");
        assert_eq!(p.order, ["a", "b", "c"], "and stays in the base");
        // Hiding nothing, the same answer is a song taken out there.
        let p = plan(&mine, &l, "me", &there, true, Allow::default()).unwrap();
        assert_eq!(p.dropped, 1);
    }

    #[test]
    fn songs_youtube_hides_are_not_counted_as_most_gone() {
        let mine: Vec<_> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|i| online(i))
            .collect();
        let l = link(&["a", "b", "c", "d", "e"]);
        let there = remote(&["a", "b"]);
        let hides = Read {
            complete: true,
            hidden: 3,
        };
        let p = plan(&mine, &l, "me", &there, hides, Allow::default()).unwrap();
        assert_eq!(p.dropped, 0);
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
            ANYWAY,
        );
        assert_eq!(r.err(), Some(Pause::Unread));
    }

    #[test]
    fn an_empty_answer_is_never_taken_as_every_song_deleted() {
        let mine = [online("a"), online("b")];
        let r = plan(&mine, &link(&["a", "b"]), "me", &[], true, ASK);
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
            plan(&mine, &l, "me", &r, true, ASK),
            Err(Pause::ManyGone {
                side: Side::YoutubeMusic,
                gone: 3,
                of: 5
            })
        ));
        let p = plan(&mine, &l, "me", &r, true, gone(Side::YoutubeMusic)).unwrap();
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
            ASK,
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
        let r = plan(&[online("a")], &l, "me", &remote(&["a", "b"]), true, ASK);
        assert_eq!(r.err(), Some(Pause::Held));
        let p = plan(&[online("a")], &l, "me", &remote(&["a", "b"]), true, HELD).unwrap();
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
            ASK,
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
            ASK,
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
            ASK,
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
        let p = plan(&mine, &l, "me", &remote(&["a", "n", "b"]), true, ASK).unwrap();
        assert_eq!(p.add_remote, ["y"]);
        assert_eq!(lines(&p, &mine), ["a", "x.flac", "n", "y.flac", "b"]);
        assert_eq!(unmatched(&mine, &l), [1]);
        // One on its way to your uploads isn't asked about again.
        l.uploading.insert("md5:M1".into(), Uploading::default());
        assert!(unmatched(&mine, &l).is_empty());
        assert_eq!(files(&mine), [1, 2]);
        assert_eq!(counts(&mine, &l), (3, 1));
    }

    #[test]
    fn a_song_twice_here_is_sent_once_and_both_lines_stay() {
        let mine = [online("a"), online("a")];
        let p = plan(&mine, &link(&[]), "me", &[], true, ASK).unwrap();
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
        let (now, want) = after_edits(&r, &removed, &placed, &order, &BTreeMap::new());
        assert_eq!(now, ["s-a", "s-c", "s-n"]);
        assert_eq!(want, ["s-n", "s-a", "s-c"]);
        assert_eq!(apply(&now, &moves(&now, &want)), want);
    }

    #[test]
    fn a_song_swapped_for_another_version_there_is_still_the_same_song() {
        let mine = [online("a")];
        let mut l = link(&[]);
        let p = plan(&mine, &l, "me", &[], true, ASK).unwrap();
        assert_eq!(p.add_remote, ["a"]);
        let placed = [("a2".to_string(), "s-a2".to_string())];
        l.note_swaps(&p.add_remote, &placed);
        l.base = p.order.clone();
        let there = remote(&["a2"]);
        // Not taken as "a" gone there and "a2" new.
        assert!(plan(&mine, &l, "me", &there, true, ASK).unwrap().is_empty());
        let (now, want) = after_edits(&there, &[], &[], &l.base, &l.aliases);
        assert_eq!(
            (now, want),
            (vec!["s-a2".to_string()], vec!["s-a2".to_string()])
        );
        // Taken out here, it goes there by its own id.
        let p = plan(&[online("b")], &l, "me", &there, true, ASK).unwrap();
        assert_eq!(p.remove_remote, [("a2".to_string(), "s-a2".to_string())]);
        assert_eq!(p.add_remote, ["b"]);
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

    // Links are kept by the playlist's name. A deleted playlist's link goes to Set aside with
    // it, so a new playlist given that name starts unlinked (it would have taken the old link
    // over, and its first "Sync anyway" emptied the playlist there), and Undo brings it back.
    #[test]
    fn a_deleted_playlists_link_goes_aside_with_it_and_comes_back_with_undo() {
        let d = tempfile::tempdir().unwrap();
        let l = link(&["a"]);
        save(d.path(), &l).unwrap();
        let aside = d.path().join("Set aside/playlists/1700000000000-Road.m3u8");

        deleted(d.path(), "Road", &aside).unwrap();
        assert!(
            load(d.path(), "Road").is_none(),
            "a new “Road” is not linked"
        );
        assert!(all(d.path()).is_empty(), "and it isn't synced");

        undeleted(d.path(), "Road", &aside).unwrap();
        assert_eq!(load(d.path(), "Road"), Some(l.clone()));

        // A playlist with no link deleted, and put back, has none.
        deleted(d.path(), "Drive", &aside).unwrap();
        undeleted(d.path(), "Drive", &aside).unwrap();
        assert!(load(d.path(), "Drive").is_none());
        // A link made since under the name is never replaced by the old one.
        deleted(d.path(), "Road", &aside).unwrap();
        let mut new = link(&[]);
        new.playlist_id = "PL2".into();
        save(d.path(), &new).unwrap();
        undeleted(d.path(), "Road", &aside).unwrap();
        assert_eq!(load(d.path(), "Road").unwrap().playlist_id, "PL2");
    }
}
