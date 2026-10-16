//! SD card sync: a one-way mirror of the library onto a card, safe to interrupt.
//!
//! * The card says who it is: `/.venuta/card.toml` (never the drive letter).
//! * The card remembers what it holds: `/.venuta/manifest.sqlite`, so any PC can
//!   sync it and changes are found by content, not by FAT/exFAT's unreliable file times.
//! * Order: clean up interrupted writes → delete → move → copy new (`.vpart`, flush, rename) →
//!   patch tags in place → sidecars. The manifest is updated after every file.
//! * Tag-only changes rewrite just the metadata at the start of the file, inside the space
//!   already there; the audio is never touched or re-copied.
//! * Music already on the card from another tool is recognised by its audio checksum and
//!   moved into place instead of being copied again.
//! * Files this app did not put on the card are never touched.

use crate::library::{Library, Track};
use crate::media::flac_layout::{self, BLOCK_PADDING};
use crate::playlists::{PlaylistEntry, WantEntry};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

pub const CARD_DIR: &str = ".venuta";

/// The card folder written before the app was named; renamed on first use.
const OLD_CARD_DIR: &str = ".music-librarian";

/// Move a card's folder from before the rename, so its identity and manifest carry over.
fn migrate_card(card_root: &Path) {
    let (new, old) = (card_root.join(CARD_DIR), card_root.join(OLD_CARD_DIR));
    if !new.exists() && old.is_dir() {
        let _ = std::fs::rename(old, new);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardConfig {
    pub id: String,
    pub name: String,
    /// Folder on the card that holds the music.
    #[serde(default = "default_folder")]
    pub folder: String,
    /// Top-level library folders to include (empty = everything).
    #[serde(default)]
    pub include: Vec<String>,
    /// Sync automatically when the card is inserted (while the app is running).
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub created_at: u64,
    /// The player that reads this card; its copies are written the way that player reads.
    #[serde(default)]
    pub player: crate::device::Player,
    /// Your answers to the player test (question id → what the player did).
    #[serde(default)]
    pub test_results: std::collections::BTreeMap<String, bool>,
    /// A selection instead of the whole library (None: everything, or the `include` folders).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<Pick>,
    /// Whose card it is: your own player's, a gift, or a car's.
    #[serde(default)]
    pub kind: CardKind,
    /// Smaller files on the way to the card (the library's own files never change).
    #[serde(default)]
    pub convert: Convert,
    /// What a gift card carries besides the music.
    #[serde(default)]
    pub extras: Extras,
    /// The volume's serial when the card was set up: a copied card (same settings, another
    /// card) is told apart by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u32>,
}

/// What a card carries when it isn't everything: every song any of these holds, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    /// Playlist names (smart playlists included).
    #[serde(default)]
    pub playlists: Vec<String>,
    /// Album folders, library-relative.
    #[serde(default)]
    pub albums: Vec<String>,
    /// Artists (album artists and singers), as the Music screen names them.
    #[serde(default)]
    pub artists: Vec<String>,
}

impl Pick {
    pub fn is_empty(&self) -> bool {
        self.playlists.is_empty() && self.albums.is_empty() && self.artists.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    /// For your own player, written the way it reads.
    #[default]
    Player,
    /// For someone else: README, what's missing, playlists that keep their gaps.
    Gift,
    /// For a car stereo or a USB stick: folders to browse, FAT32's limits.
    Car,
}

/// Conversions a card asks for. Off: every file as the library has it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Convert {
    /// Hi-res FLAC (more than 16 bits or 48 kHz) as 16-bit FLAC at 44.1 or 48 kHz.
    #[serde(default)]
    pub hires: bool,
    /// Lossless songs as AAC (.m4a), for phones and cars.
    #[serde(default)]
    pub aac: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extras {
    /// README.txt at the top: what this is, how to play it.
    #[serde(default = "yes")]
    pub readme: bool,
    /// Tracklist.txt in each album that isn't complete, and Missing.csv at the top.
    #[serde(default = "yes")]
    pub missing: bool,
    /// Playlists keep the songs not on the card as lines players skip and Venuta reads.
    #[serde(default = "yes")]
    pub gaps: bool,
}

impl Default for Extras {
    fn default() -> Self {
        Self {
            readme: true,
            missing: true,
            gaps: true,
        }
    }
}

fn yes() -> bool {
    true
}

fn default_folder() -> String {
    "Music".into()
}

pub fn card_config_path(card_root: &Path) -> PathBuf {
    migrate_card(card_root);
    card_root.join(CARD_DIR).join("card.toml")
}

pub fn read_card(card_root: &Path) -> Option<CardConfig> {
    let text = std::fs::read_to_string(card_config_path(card_root)).ok()?;
    let cfg: CardConfig = toml::from_str(&text).ok()?;
    // The card says where its music is and what it's called; a card is someone else's disk,
    // so neither may point anywhere but inside it (the id also names a scratch folder here).
    let id_ok = cfg.id.len() <= 64
        && cfg
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let folder_ok = cfg.folder.is_empty() || crate::paths::is_inside_rel(&cfg.folder);
    (id_ok && folder_ok).then_some(cfg)
}

/// What you choose for a card when setting it up or changing it. None: keep what it has.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CardSettings {
    pub name: String,
    #[serde(default)]
    pub include: Vec<String>,
    /// The selection (None or empty: everything, or the `include` folders).
    #[serde(default)]
    pub pick: Option<Pick>,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub player: Option<crate::device::Player>,
    #[serde(default)]
    pub kind: Option<CardKind>,
    #[serde(default)]
    pub convert: Option<Convert>,
    #[serde(default)]
    pub extras: Option<Extras>,
}

/// The serial of the volume a card is on, if it can be read.
fn serial_of(card_root: &Path) -> Option<u32> {
    let want = card_root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    crate::volumes::list()
        .into_iter()
        .find(|v| v.root.trim_end_matches(['\\', '/']).to_lowercase() == want)
        .map(|v| v.serial)
        .filter(|s| *s != 0)
}

/// Make a card known to the app. Keeps an existing id so re-running setup never forgets it.
pub fn setup_card(card_root: &Path, s: CardSettings) -> Result<CardConfig, String> {
    let CardSettings {
        name,
        include,
        pick,
        auto_sync,
        player,
        kind,
        convert,
        extras,
    } = s;
    let name = name.as_str();
    let existing = read_card(card_root);
    let kind = kind
        .or(existing.as_ref().map(|c| c.kind))
        .unwrap_or_default();
    let player = player
        .or(existing.as_ref().map(|c| c.player))
        .unwrap_or_default();
    if let Some((total, _)) = crate::volumes::space(card_root)
        && let Some(why) = too_big_for(player, total)
    {
        return Err(why);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let id = existing.as_ref().map(|c| c.id.clone()).unwrap_or_else(|| {
        let seed = format!(
            "{}:{now}:{:?}",
            card_root.display(),
            std::time::Instant::now()
        );
        blake3::hash(seed.as_bytes()).to_hex()[..20].to_owned()
    });
    let cfg = CardConfig {
        id,
        name: name.trim().to_owned(),
        folder: existing
            .as_ref()
            .map(|c| c.folder.clone())
            .unwrap_or_else(default_folder),
        include,
        auto_sync,
        player,
        test_results: existing
            .as_ref()
            .map(|c| c.test_results.clone())
            .unwrap_or_default(),
        pick: pick.filter(|p| !p.is_empty()),
        kind,
        convert: convert
            .or(existing.as_ref().map(|c| c.convert))
            .unwrap_or_default(),
        extras: extras
            .or(existing.as_ref().map(|c| c.extras))
            .unwrap_or_default(),
        // A card set up again keeps the serial it was first set up on: that is what tells a
        // copy of it apart.
        serial: existing
            .as_ref()
            .and_then(|c| c.serial)
            .or_else(|| serial_of(card_root)),
        created_at: existing.map(|c| c.created_at).unwrap_or(now),
    };
    let dir = card_root.join(CARD_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // A gift's own records stay out of sight: the music is what it is for.
    if cfg.kind == CardKind::Gift {
        hide(&dir);
    }
    let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    write_atomic(&card_config_path(card_root), body.as_bytes())?;
    Ok(cfg)
}

/// Make a card its own again: a new id and this volume's serial (for a card whose settings
/// were copied from another card's).
pub fn reidentify(card_root: &Path) -> Result<CardConfig, String> {
    let mut cfg = read_card(card_root).ok_or("This card isn't set up yet.")?;
    let seed = format!("{}:{:?}", card_root.display(), std::time::Instant::now());
    cfg.id = blake3::hash(seed.as_bytes()).to_hex()[..20].to_owned();
    cfg.serial = serial_of(card_root);
    let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    write_atomic(&card_config_path(card_root), body.as_bytes())?;
    Ok(cfg)
}

/// Why a card is too big for a player, if it is (the Echo Mini reads cards up to 256 GB).
pub fn too_big_for(player: crate::device::Player, total_bytes: u64) -> Option<String> {
    // Card makers' "256 GB" is a little under 256 * 10^9 once formatted; allow for it.
    const ECHO_MINI_MAX: u64 = 260_000_000_000;
    (player == crate::device::Player::EchoMini && total_bytes > ECHO_MINI_MAX).then(|| {
        format!(
            "The Echo Mini reads cards up to 256 GB, and this one is {} GB. Use a smaller card for it, or choose another player.",
            total_bytes / 1_000_000_000
        )
    })
}

/// Hidden from players and Explorer (Windows); dot-named everywhere else already.
fn hide(path: &Path) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, SetFileAttributesW,
        };
        let w: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let a = GetFileAttributesW(w.as_ptr());
            if a != u32::MAX {
                SetFileAttributesW(w.as_ptr(), a | FILE_ATTRIBUTE_HIDDEN);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = path;
}

// --- What the library wants on the card ------------------------------------------------------

/// How a song is changed on its way to the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// 16-bit FLAC at this rate (44.1 or 48 kHz).
    Cd16 { rate: u32 },
    /// AAC in an .m4a file.
    Aac,
}

impl Target {
    fn tag(self) -> &'static str {
        match self {
            Target::Cd16 { .. } => "cd16",
            Target::Aac => "aac",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Target::Cd16 { .. } => "flac",
            Target::Aac => "m4a",
        }
    }
}

/// AAC's bitrate on cards: the highest Windows' own encoder offers, close to transparent.
pub const AAC_KBPS: u64 = 192;

#[derive(Debug, Clone)]
struct Wanted {
    /// Where it goes, from the card's music folder ("/"-separated; "../" for beside it).
    rel: String,
    /// Where it comes from, library-relative (empty for text the card is given).
    src: String,
    size: u64,
    mtime_ns: i64,
    audio_md5: Option<String>,
    duration_ms: u64,
    is_flac: bool,
    is_image: bool,
    /// Text written for this card: a playlist, a README, a track list.
    text: Option<String>,
    /// Converted on the way (None: as the library has it).
    convert: Option<Target>,
}

impl Wanted {
    /// What a card copy depends on besides the source file: the player recipe (and any
    /// conversion) for music and covers, the card's own text for written files.
    fn stamp(&self, recipe_stamp: Option<&str>) -> Option<String> {
        match (&self.text, self.convert) {
            (Some(t), _) => Some(format!("pl:{}", blake3::hash(t.as_bytes()).to_hex())),
            (None, Some(c)) => Some(format!("{}+{}", recipe_stamp.unwrap_or("exact"), c.tag())),
            (None, None) if self.is_flac || self.is_image => recipe_stamp.map(str::to_owned),
            (None, None) => None,
        }
    }

    /// What it takes on the card; for a converted song, a close estimate.
    fn card_bytes(&self) -> u64 {
        match self.convert {
            Some(Target::Aac) => self.duration_ms * AAC_KBPS / 8 + 64_000,
            // Stereo 16-bit PCM, which FLAC brings to about 60%; never more than the original.
            Some(Target::Cd16 { rate }) => {
                (self.duration_ms * rate as u64 * 4 / 1000 * 6 / 10 + 64_000).min(self.size)
            }
            None => self.size,
        }
    }
}

/// A copy written from a conversion: it can't be moved or patched as the library's audio.
fn converted(stamp: Option<&str>) -> bool {
    stamp.is_some_and(|s| !s.starts_with("pl:") && s.contains('+'))
}

// --- Where things go on the card -----------------------------------------------------------

/// The longest path a card file gets, counted from the card's top: FiiO's players stop at 256
/// characters and exFAT names at 255; a few spare for the ".vpart" written first.
const MAX_CARD_PATH: usize = 248;

/// A file or folder name every card file system and player accepts: no `\ / : * ? " < > |`
/// or control characters, no trailing dots or spaces, one Unicode form.
fn safe_name(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mut out: String = s
        .nfc()
        .map(|c| {
            if c < ' ' || r#"\/:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() { "_".into() } else { out }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// `name` cut to at most `max` characters, marked with "~", keeping what makes it readable: a
/// track number at the start, a year at the end ("Album (2009)"), the extension.
fn shorten(name: &str, max: usize) -> String {
    if chars(name) <= max {
        return name.to_owned();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 5 && !e.contains(' ') => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let tail = stem
        .rfind(" (")
        .filter(|&i| stem.ends_with(')') && stem.len() - i <= 8)
        .map(|i| &stem[i..])
        .unwrap_or("");
    let body = &stem[..stem.len() - tail.len()];
    let room = max.saturating_sub(chars(&ext) + chars(tail) + 1).max(8);
    let cut: String = body.chars().take(room).collect();
    format!("{}~{tail}{ext}", cut.trim_end())
}

/// Card paths for library files (library path, new extension if converted): the library's own
/// layout, made safe for card file systems and shortened only where a path would be too long.
/// A folder is shortened the same way for every file in it, so an album stays one folder.
fn card_paths(folder: &str, files: &[(String, Option<&str>)]) -> HashMap<String, String> {
    let base = if folder.is_empty() {
        0
    } else {
        chars(folder) + 1
    };
    let split = |p: &str| -> (String, String) {
        match p.rsplit_once('/') {
            Some((d, n)) => (d.to_owned(), n.to_owned()),
            None => (String::new(), p.to_owned()),
        }
    };
    let named = |n: &str, ext: Option<&str>| -> String {
        let n = match ext {
            Some(e) => match n.rsplit_once('.') {
                Some((s, _)) => format!("{s}.{e}"),
                None => format!("{n}.{e}"),
            },
            None => n.to_owned(),
        };
        safe_name(&n)
    };
    // The longest file name in each folder decides how much room the folder may take.
    let mut longest: std::collections::BTreeMap<String, usize> = Default::default();
    for (src, ext) in files {
        let (d, n) = split(src);
        let l = chars(&named(&n, *ext));
        let e = longest.entry(d).or_default();
        *e = (*e).max(l);
    }
    let mut dirs: HashMap<String, String> = HashMap::new();
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut taken_dirs: HashMap<String, String> = HashMap::new();
    for (dir, file_len) in &longest {
        if dir.is_empty() {
            dirs.insert(String::new(), String::new());
            continue;
        }
        let src: Vec<&str> = dir.split('/').collect();
        let mut comps: Vec<String> = src.iter().map(|c| safe_name(c)).collect();
        // Folders above the album, shortened already for another album: the same here.
        for i in 0..comps.len().saturating_sub(1) {
            if let Some(p) = parents.get(&src[..=i].join("/")) {
                comps[i] = p.clone();
            }
        }
        let len = |c: &[String]| base + c.iter().map(|x| chars(x) + 1).sum::<usize>() + file_len;
        // The album folder gives way first, then the ones above it; never under 24 characters.
        for i in (0..comps.len()).rev() {
            let over = len(&comps).saturating_sub(MAX_CARD_PATH);
            if over == 0 {
                break;
            }
            let c = chars(&comps[i]);
            if c > 24 {
                comps[i] = shorten(&comps[i], (c - over).max(24));
                if i + 1 < comps.len() {
                    parents.insert(src[..=i].join("/"), comps[i].clone());
                }
            }
        }
        for i in 0..comps.len().saturating_sub(1) {
            parents
                .entry(src[..=i].join("/"))
                .or_insert_with(|| comps[i].clone());
        }
        let mut card = comps.join("/");
        // Two folders shortened to the same name stay two folders.
        let mut n = 2;
        while taken_dirs
            .get(&card.to_lowercase())
            .is_some_and(|d| d != dir)
        {
            card = format!("{} ~{n}", comps.join("/"));
            n += 1;
        }
        taken_dirs.insert(card.to_lowercase(), dir.clone());
        dirs.insert(dir.clone(), card);
    }
    let mut taken: HashSet<String> = HashSet::new();
    let mut out = HashMap::new();
    let mut sorted: Vec<&(String, Option<&str>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (src, ext) in sorted {
        let (d, n) = split(src);
        let card_dir = dirs.get(&d).cloned().unwrap_or_default();
        let room = MAX_CARD_PATH.saturating_sub(base + chars(&card_dir) + 1);
        let mut name = shorten(&named(&n, *ext), room);
        let join = |name: &str| {
            if card_dir.is_empty() {
                name.to_owned()
            } else {
                format!("{card_dir}/{name}")
            }
        };
        let mut n2 = 2;
        while taken.contains(&join(&name).to_lowercase()) {
            let (s, e) = name.rsplit_once('.').unwrap_or((&name, ""));
            name = format!("{s} ~{n2}.{e}");
            n2 += 1;
        }
        taken.insert(join(&name).to_lowercase());
        out.insert(src.clone(), join(&name));
    }
    out
}

// --- What a selection holds -------------------------------------------------------------

/// One spelling for one artist, whatever the case or stray spaces (as the Music screen has it).
fn artist_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(". ", ".")
        .to_lowercase()
}

/// Library songs (lower-cased paths) a selection holds: its albums, everything by its artists
/// (their albums and the songs they sing on), every song its playlists have.
fn picked_songs(lib: &Library, root: &Path, pick: &Pick) -> HashSet<String> {
    let albums: HashSet<String> = pick.albums.iter().map(|a| a.to_lowercase()).collect();
    let artists: HashSet<String> = pick.artists.iter().map(|a| artist_key(a)).collect();
    let mut out = HashSet::new();
    for t in &lib.tracks {
        let by_artist = !artists.is_empty() && {
            let album_artist = t.first("ALBUMARTIST").or(t.first("ARTIST")).unwrap_or("");
            artists.contains(&artist_key(album_artist))
                || t.tags.get("ARTIST").is_some_and(|v| {
                    v.iter()
                        .flat_map(|a| crate::plan::split_names(a))
                        .any(|n| artists.contains(&artist_key(&n)))
                })
        };
        if by_artist || albums.contains(&t.dir.to_lowercase()) {
            out.insert(t.path.to_lowercase());
        }
    }
    for name in &pick.playlists {
        if let Ok((entries, _)) = crate::playlists::read(root, lib, name) {
            out.extend(
                entries
                    .iter()
                    .filter(|e| !e.missing)
                    .map(|e| e.path.to_lowercase()),
            );
        }
    }
    out
}

/// A playlist as a card gets it: the songs it carries as paths from where the playlist sits
/// (`prefix` + card path), and with `gaps` every other song as a `#VENUTA-WANT` line, which
/// players skip and Venuta reads. Never a path to a file the card doesn't have: Poweramp would
/// match a different file of the same name. None when it holds none of the card's songs.
fn card_playlist(
    name: &str,
    entries: &[PlaylistEntry],
    on_card: &HashMap<String, String>,
    prefix: &str,
    gaps: bool,
) -> Option<String> {
    let mut out = format!("#EXTM3U\n#PLAYLIST:{name}\n");
    let mut songs = 0;
    for e in entries {
        let here = (!e.missing)
            .then(|| on_card.get(&e.path.to_lowercase()))
            .flatten();
        match here {
            Some(rel) => {
                out.push_str(&format!(
                    "#EXTINF:{},{} - {}\n{prefix}{rel}\n",
                    e.duration_ms / 1000,
                    e.artist,
                    e.title
                ));
                songs += 1;
            }
            None if gaps => {
                let w = e.want.clone().unwrap_or_else(|| WantEntry {
                    title: e.title.clone(),
                    artist: e.artist.clone(),
                    album: e.album.clone(),
                    duration_ms: e.duration_ms,
                    md5: e.audio_md5.clone(),
                    ..Default::default()
                });
                out.push_str(crate::playlists::WANT_TAG);
                out.push_str(&serde_json::to_string(&w).unwrap_or_default());
                out.push('\n');
            }
            None => {}
        }
    }
    (songs > 0).then_some(out)
}

// --- What a gift card says about itself ---------------------------------------------------

/// The first number in a tag ("3/12" → 3).
fn number(s: Option<&str>) -> u32 {
    s.map(|s| {
        s.chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect::<String>()
    })
    .and_then(|n| n.parse().ok())
    .unwrap_or(0)
}

/// "of" in "3/12", or TRACKTOTAL.
fn track_total(t: &Track) -> u32 {
    let of = t
        .first("TRACKNUMBER")
        .and_then(|s| s.split_once('/'))
        .map(|(_, n)| number(Some(n)))
        .unwrap_or(0);
    of.max(number(t.first("TRACKTOTAL")).max(number(t.first("TOTALTRACKS"))))
}

fn year_of(t: &Track) -> String {
    t.first("ORIGINALYEAR")
        .or(t.first("ORIGINALDATE"))
        .or(t.first("DATE"))
        .map(|d| d.chars().take(4).collect())
        .unwrap_or_default()
}

/// Today, as people write it ("13 October 2026"): the computer's own date.
fn today() -> String {
    #[cfg(windows)]
    {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        if (1..=12).contains(&t.wMonth) {
            return format!(
                "{} {} {}",
                t.wDay,
                MONTHS[usize::from(t.wMonth) - 1],
                t.wYear
            );
        }
    }
    // Elsewhere, the date in UTC.
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Days since 1970 to a calendar date (Howard Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// One song of an album, as a track list shows it.
struct Line {
    disc: u32,
    track: u32,
    title: String,
    artist: String,
    /// "here", "not on this card" or "not in the library".
    status: &'static str,
}

/// What a gift card carries about itself: a Tracklist.txt in every album it holds only some of,
/// Missing.csv (every song to look for next), and README.txt. As (card path, text).
fn gift_notes(
    cfg: &CardConfig,
    lib: &Library,
    on_card: &HashMap<String, String>,
    playlists: &[(String, Vec<PlaylistEntry>)],
    up: &str,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut by_dir: std::collections::BTreeMap<&str, Vec<&Track>> = Default::default();
    for t in &lib.tracks {
        by_dir.entry(t.dir.as_str()).or_default().push(t);
    }
    let mut missing =
        String::from("\u{feff}Album,Album artist,Disc,Track,Title,Artist,Why,Playlist\n");
    let (mut albums, mut partial, mut songs) = (0, 0, 0);
    for (dir, tracks) in &by_dir {
        let held: Vec<&&Track> = tracks
            .iter()
            .filter(|t| on_card.contains_key(&t.path.to_lowercase()))
            .collect();
        if held.is_empty() {
            continue;
        }
        albums += 1;
        songs += held.len();
        let first = held[0];
        let album = first.first("ALBUM").unwrap_or(dir).to_owned();
        let album_artist = first
            .first("ALBUMARTIST")
            .or(first.first("ARTIST"))
            .unwrap_or("")
            .to_owned();
        let mut lines: Vec<Line> = tracks
            .iter()
            .map(|t| Line {
                disc: number(t.first("DISCNUMBER")).max(1),
                track: number(t.first("TRACKNUMBER")),
                title: t
                    .first("TITLE")
                    .map(str::to_owned)
                    .unwrap_or_else(|| t.path.rsplit('/').next().unwrap_or("").to_owned()),
                artist: t.first("ARTIST").unwrap_or("").to_owned(),
                status: if on_card.contains_key(&t.path.to_lowercase()) {
                    "here"
                } else {
                    "not on this card"
                },
            })
            .collect();
        // Numbers the album has (by its own count) that the library doesn't.
        let mut discs: std::collections::BTreeMap<u32, (u32, HashSet<u32>)> = Default::default();
        for (t, l) in tracks.iter().zip(&lines) {
            let d = discs.entry(l.disc).or_default();
            d.0 = d.0.max(track_total(t));
            d.1.insert(l.track);
        }
        for (disc, (total, have)) in discs {
            for n in 1..=total {
                if !have.contains(&n) {
                    lines.push(Line {
                        disc,
                        track: n,
                        title: String::new(),
                        artist: String::new(),
                        status: "not in the library",
                    });
                }
            }
        }
        lines.sort_by_key(|l| (l.disc, l.track));
        if lines.iter().all(|l| l.status == "here") {
            continue;
        }
        partial += 1;
        let several_discs = lines.iter().any(|l| l.disc > 1);
        let mut text = format!(
            "{album}{}{}\r\n{} of {} songs are on this card.\r\n\r\n",
            match year_of(first).as_str() {
                "" => String::new(),
                y => format!(" ({y})"),
            },
            if album_artist.is_empty() {
                String::new()
            } else {
                format!(", {album_artist}")
            },
            held.len(),
            lines.len()
        );
        for l in &lines {
            let no = if several_discs {
                format!("{}-{:02}", l.disc, l.track)
            } else {
                format!("{:02}", l.track)
            };
            let title = if l.title.is_empty() {
                "(title not known)"
            } else {
                &l.title
            };
            text.push_str(&format!("{no}  {title}  [{}]\r\n", l.status));
            if l.status != "here" {
                missing.push_str(&format!(
                    "{},{},{},{},{},{},{},\n",
                    csv(&album),
                    csv(&album_artist),
                    l.disc,
                    l.track,
                    csv(title),
                    csv(&l.artist),
                    l.status
                ));
            }
        }
        if cfg.extras.missing {
            // Beside the album's songs on the card.
            let song = on_card[&held[0].path.to_lowercase()].clone();
            let card_dir = song.rsplit_once('/').map(|x| x.0).unwrap_or("");
            let rel = if card_dir.is_empty() {
                "Tracklist.txt".to_owned()
            } else {
                format!("{card_dir}/Tracklist.txt")
            };
            out.push((rel, text));
        }
    }
    // Songs the card's playlists have that the library doesn't.
    for (name, entries) in playlists {
        for e in entries.iter().filter(|e| e.missing) {
            missing.push_str(&format!(
                ",,,,{},{},not in the library,{}\n",
                csv(&e.title),
                csv(&e.artist),
                csv(name)
            ));
        }
    }
    let to_get = missing.lines().count() - 1;
    if cfg.extras.missing && to_get > 0 {
        out.push((format!("{up}Missing.csv"), missing));
    }
    if cfg.extras.readme {
        let date = today();
        let lists = playlists.len();
        let mut r = format!(
            "{}\r\nMade with Venuta on {date}.\r\n\r\nWhat's here\r\n  {songs} songs from {albums} albums, in the {} folder: one folder per album,\r\n  songs numbered in album order.\r\n",
            cfg.name.trim(),
            if cfg.folder.is_empty() {
                "top"
            } else {
                cfg.folder.as_str()
            },
        );
        if lists > 0 {
            r.push_str(&format!(
                "  {lists} playlist{} in the Playlists folder. Phones and most players open them; some\r\n  players and car stereos only browse folders.\r\n",
                if lists == 1 { "" } else { "s" }
            ));
        }
        if partial > 0 || to_get > 0 {
            r.push_str("\r\nWhat's missing\r\n");
            if partial > 0 {
                r.push_str(&format!(
                    "  {partial} album{} here {} not complete: each of those folders has a Tracklist.txt\r\n  saying which songs are on the card and which aren't.\r\n",
                    if partial == 1 { "" } else { "s" },
                    if partial == 1 { "is" } else { "are" },
                ));
            }
            if to_get > 0 {
                r.push_str("  Missing.csv lists every song not here, to look for next.\r\n");
            }
        }
        r.push_str("\r\nHow to play\r\n  On a phone or music player, open the music folder or a playlist. In a car, browse the folders.\r\n  To build your own library from it, copy the folders as they are.\r\n\r\nThis music is shared for personal listening.\r\n");
        out.push((format!("{up}README.txt"), r));
    }
    out
}

/// How a card's player reads playlists, from its profile and your answers to the player test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaylistStyle {
    /// In `Playlists/` beside the music folder, paths relative to the playlist.
    Relative,
    /// At the top of the card, paths from the top (the test found only these work).
    FromTop,
}

fn playlist_style(cfg: &CardConfig) -> Option<PlaylistStyle> {
    if !cfg.player.recipe().playlists {
        return None;
    }
    let answer = |id: &str| cfg.test_results.get(id).copied();
    match (answer("playlist_relative"), answer("playlist_top")) {
        (Some(false), Some(true)) => Some(PlaylistStyle::FromTop),
        (Some(false), Some(false)) => None,
        _ => Some(PlaylistStyle::Relative),
    }
}

// --- Everything a card gets ------------------------------------------------------------------

/// The FLAC a card's conversion settings turn this song into, if any.
///
/// `real_bits` is what the deep check measured: 16-bit audio stored as 24-bit gains nothing
/// from converting, so it goes as it is.
fn target_for(
    cfg: &CardConfig,
    is_flac: bool,
    rate: u32,
    bits: u32,
    real_bits: Option<u8>,
) -> Option<Target> {
    if !is_flac {
        return None;
    }
    if cfg.convert.aac {
        return Some(Target::Aac);
    }
    let bits = real_bits.map_or(bits, u32::from).min(bits);
    (cfg.convert.hires && (bits > 16 || rate > 48_000)).then_some(Target::Cd16 {
        rate: if rate.is_multiple_of(44_100) {
            44_100
        } else {
            48_000
        },
    })
}

fn wanted_files(
    index: &crate::Index,
    library_root: &Path,
    cfg: &CardConfig,
) -> crate::Result<Vec<Wanted>> {
    let style = playlist_style(cfg);
    let folder = cfg.folder.trim_matches('/');
    // From the card's music folder up to the card's top.
    let up = "../".repeat(folder.split('/').filter(|s| !s.is_empty()).count());
    let lib = Library::load(index)?;

    struct Row {
        rel: String,
        kind: String,
        size: u64,
        mtime_ns: i64,
        md5: Option<String>,
        rate: u32,
        bits: u32,
        duration_ms: u64,
    }
    let mut stmt = index.conn().prepare(
        "SELECT path, kind, size, mtime_ns, hex(audio_md5), COALESCE(sample_rate, 0),
                COALESCE(bit_depth, 0), COALESCE(duration_ms, 0)
         FROM files WHERE kind IN ('flac', 'other_audio', 'lyrics', 'image')",
    )?;
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                rel: r.get(0)?,
                kind: r.get(1)?,
                size: r.get::<_, i64>(2)? as u64,
                mtime_ns: r.get(3)?,
                md5: r.get::<_, Option<String>>(4)?.filter(|m| !m.is_empty()),
                rate: r.get::<_, i64>(5)? as u32,
                bits: r.get::<_, i64>(6)? as u32,
                duration_ms: r.get::<_, i64>(7)? as u64,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let include: HashSet<String> = cfg.include.iter().map(|s| s.to_lowercase()).collect();
    let in_scope = |rel: &str| {
        let top = rel.split('/').next().unwrap_or("").to_lowercase();
        !top.starts_with('_')
            && top != "playlists"
            && (include.is_empty() || include.contains(&top))
    };
    let picked = cfg
        .pick
        .as_ref()
        .map(|p| picked_songs(&lib, library_root, p));
    let lower_stem = |p: &str| p.rsplit_once('.').map(|x| x.0).unwrap_or(p).to_lowercase();
    let dir_of = |p: &str| p.rsplit_once('/').map(|x| x.0).unwrap_or("").to_lowercase();

    let songs: Vec<&Row> = rows
        .iter()
        .filter(|r| r.kind == "flac" || r.kind == "other_audio")
        .filter(|r| match &picked {
            Some(p) => p.contains(&r.rel.to_lowercase()),
            None => in_scope(&r.rel),
        })
        .collect();
    let song_dirs: HashSet<String> = songs.iter().map(|r| dir_of(&r.rel)).collect();
    let song_stems: HashMap<String, &str> = songs
        .iter()
        .map(|r| (lower_stem(&r.rel), r.rel.as_str()))
        .collect();
    let images: Vec<&Row> = rows
        .iter()
        .filter(|r| r.kind == "image")
        .filter(|r| match &picked {
            Some(_) => song_dirs.contains(&dir_of(&r.rel)),
            None => in_scope(&r.rel),
        })
        .collect();

    let real_bits: HashMap<String, u8> = lib
        .tracks
        .iter()
        .filter_map(|t| Some((t.path.to_lowercase(), t.quality.as_ref()?.effective_bits?)))
        .collect();
    let targets: HashMap<&str, Option<Target>> = songs
        .iter()
        .map(|r| {
            let real = real_bits.get(&r.rel.to_lowercase()).copied();
            (
                r.rel.as_str(),
                target_for(cfg, r.kind == "flac", r.rate, r.bits, real),
            )
        })
        .collect();
    // A library keeps its albums in its own Music folder, and the card has a music folder of its
    // own: the albums go straight into the card's, not into a Music folder inside it.
    let nested = library_root.join(crate::paths::MUSIC_DIR_NAME).is_dir();
    let shown = |rel: &str| -> String {
        let cut = crate::paths::MUSIC_DIR_NAME.len() + 1;
        match rel.get(..cut) {
            Some(head)
                if nested
                    && head.eq_ignore_ascii_case(&format!("{}/", crate::paths::MUSIC_DIR_NAME)) =>
            {
                rel[cut..].to_owned()
            }
            _ => rel.to_owned(),
        }
    };
    let files: Vec<(String, Option<&str>)> = songs
        .iter()
        .map(|r| (shown(&r.rel), targets[r.rel.as_str()].map(Target::ext)))
        .chain(images.iter().map(|r| (shown(&r.rel), None)))
        .collect();
    let by_shown = card_paths(folder, &files);
    let paths: HashMap<&str, &String> = songs
        .iter()
        .chain(images.iter())
        .map(|r| (r.rel.as_str(), &by_shown[&shown(&r.rel)]))
        .collect();

    let mut out = Vec::new();
    for r in &songs {
        out.push(Wanted {
            rel: paths[r.rel.as_str()].to_string(),
            src: r.rel.clone(),
            size: r.size,
            mtime_ns: r.mtime_ns,
            audio_md5: r.md5.clone(),
            duration_ms: r.duration_ms,
            is_flac: r.kind == "flac",
            is_image: false,
            text: None,
            convert: targets[r.rel.as_str()],
        });
    }
    for r in &images {
        out.push(Wanted {
            rel: paths[r.rel.as_str()].to_string(),
            src: r.rel.clone(),
            size: r.size,
            mtime_ns: r.mtime_ns,
            audio_md5: None,
            duration_ms: 0,
            is_flac: false,
            is_image: true,
            text: None,
            convert: None,
        });
    }
    // A song's lyrics keep its name on the card, so players find them.
    for r in rows.iter().filter(|r| r.kind == "lyrics") {
        let Some(song) = song_stems.get(&lower_stem(&r.rel)) else {
            continue;
        };
        let card = paths[*song].as_str();
        let stem = card.rsplit_once('.').map(|x| x.0).unwrap_or(card);
        out.push(Wanted {
            rel: format!("{stem}.lrc"),
            src: r.rel.clone(),
            size: r.size,
            mtime_ns: r.mtime_ns,
            audio_md5: None,
            duration_ms: 0,
            is_flac: false,
            is_image: false,
            text: None,
            convert: None,
        });
    }

    // Playlists, written for this card: every one (whole library) or the chosen ones.
    let on_card: HashMap<String, String> = songs
        .iter()
        .map(|r| (r.rel.to_lowercase(), paths[r.rel.as_str()].to_string()))
        .collect();
    let gift = cfg.kind == CardKind::Gift;
    let mut lists: Vec<(String, Vec<PlaylistEntry>)> = Vec::new();
    if let Some(style) = style {
        let names: Vec<String> = match &cfg.pick {
            Some(p) => p.playlists.clone(),
            None => crate::playlists::list(library_root, &lib)
                .into_iter()
                .map(|p| p.name)
                .collect(),
        };
        let into = |dir: &str| {
            if folder.is_empty() {
                String::new()
            } else {
                format!("{dir}{folder}/")
            }
        };
        for name in names {
            let Ok((entries, _)) = crate::playlists::read(library_root, &lib, &name) else {
                continue;
            };
            let file = safe_name(&format!("{name}.m3u8"));
            let (rel, prefix) = match style {
                PlaylistStyle::FromTop => (format!("{up}{file}"), into("")),
                PlaylistStyle::Relative => (
                    format!("{up}{}/{file}", crate::playlists::FOLDER),
                    if folder.is_empty() {
                        "../".into()
                    } else {
                        into("../")
                    },
                ),
            };
            let gaps = gift && cfg.extras.gaps;
            if let Some(text) = card_playlist(&name, &entries, &on_card, &prefix, gaps) {
                out.push(Wanted {
                    rel,
                    src: String::new(),
                    size: text.len() as u64,
                    mtime_ns: 0,
                    audio_md5: None,
                    duration_ms: 0,
                    is_flac: false,
                    is_image: false,
                    text: Some(text),
                    convert: None,
                });
                lists.push((name, entries));
            }
        }
    }
    if gift {
        for (rel, text) in gift_notes(cfg, &lib, &on_card, &lists, &up) {
            out.push(Wanted {
                rel,
                src: String::new(),
                size: text.len() as u64,
                mtime_ns: 0,
                audio_md5: None,
                duration_ms: 0,
                is_flac: false,
                is_image: false,
                text: Some(text),
                convert: None,
            });
        }
    }
    Ok(out)
}

// --- Choosing what a card carries ---------------------------------------------------------

/// Something a card can carry, with how much of the library it is.
#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    /// What the selection keeps: a playlist's name, an album's folder, an artist's name.
    pub id: String,
    pub name: String,
    /// The album's artist and year; "Smart playlist"; empty for artists.
    pub detail: String,
    pub songs: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Choices {
    pub playlists: Vec<Choice>,
    pub albums: Vec<Choice>,
    pub artists: Vec<Choice>,
}

/// Everything a card's selection can be made of, each with its songs and size.
pub fn choices(index: &crate::Index, library_root: &Path) -> crate::Result<Choices> {
    let lib = Library::load(index)?;
    let size: HashMap<String, u64> = lib
        .tracks
        .iter()
        .map(|t| (t.path.to_lowercase(), t.size))
        .collect();
    let mut playlists = Vec::new();
    for p in crate::playlists::list(library_root, &lib) {
        let picked = picked_songs(
            &lib,
            library_root,
            &Pick {
                playlists: vec![p.name.clone()],
                ..Default::default()
            },
        );
        playlists.push(Choice {
            id: p.name.clone(),
            detail: if p.smart {
                "Smart playlist".into()
            } else {
                String::new()
            },
            name: p.name,
            songs: picked.len(),
            bytes: picked.iter().filter_map(|s| size.get(s)).sum(),
        });
    }
    let mut albums: Vec<Choice> = lib
        .albums()
        .into_iter()
        .map(|(dir, tracks)| {
            let t = tracks[0];
            let artist = t.first("ALBUMARTIST").or(t.first("ARTIST")).unwrap_or("");
            let year = year_of(t);
            Choice {
                id: dir.to_owned(),
                name: t
                    .first("ALBUM")
                    .unwrap_or_else(|| dir.rsplit('/').next().unwrap_or(dir))
                    .to_owned(),
                detail: match (artist.is_empty(), year.is_empty()) {
                    (false, false) => format!("{artist}, {year}"),
                    (false, true) => artist.to_owned(),
                    (true, _) => year,
                },
                songs: tracks.len(),
                bytes: tracks.iter().map(|t| t.size).sum(),
            }
        })
        .collect();
    albums.sort_by_key(|a| a.name.to_lowercase());
    // Everyone with a song: album artists and the singers on each song, as one list.
    let mut by: HashMap<String, Choice> = HashMap::new();
    for t in &lib.tracks {
        let mut names: Vec<String> = t
            .first("ALBUMARTIST")
            .or(t.first("ARTIST"))
            .map(|a| vec![a.to_owned()])
            .unwrap_or_default();
        if let Some(v) = t.tags.get("ARTIST") {
            names.extend(v.iter().flat_map(|a| crate::plan::split_names(a)));
        }
        let mut seen = HashSet::new();
        for n in names {
            let key = artist_key(&n);
            if key.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            let c = by.entry(key).or_insert_with(|| Choice {
                id: n.trim().to_owned(),
                name: n.trim().to_owned(),
                detail: String::new(),
                songs: 0,
                bytes: 0,
            });
            c.songs += 1;
            c.bytes += t.size;
        }
    }
    let mut artists: Vec<Choice> = by.into_values().collect();
    artists.sort_by_key(|a| a.name.to_lowercase());
    Ok(Choices {
        playlists,
        albums,
        artists,
    })
}

/// What a card would carry with these settings, before anything is saved or written.
#[derive(Debug, Clone, Serialize)]
pub struct Estimate {
    pub songs: usize,
    /// What it all takes on the card (converted songs: estimated).
    pub bytes: u64,
    /// Room for it: free space plus what Venuta's own files there take now, less the reserve.
    pub room: u64,
    pub reserve: u64,
    pub fits: bool,
}

pub fn estimate(
    index: &crate::Index,
    library_root: &Path,
    card_root: &Path,
    s: &CardSettings,
) -> Result<Estimate, String> {
    let mut cfg = read_card(card_root).unwrap_or_else(|| CardConfig {
        id: String::new(),
        name: String::new(),
        folder: default_folder(),
        include: Vec::new(),
        auto_sync: false,
        created_at: 0,
        player: crate::device::Player::default(),
        test_results: Default::default(),
        pick: None,
        kind: CardKind::default(),
        convert: Convert::default(),
        extras: Extras::default(),
        serial: None,
    });
    cfg.include = s.include.clone();
    cfg.pick = s.pick.clone().filter(|p| !p.is_empty());
    if let Some(k) = s.kind {
        cfg.kind = k;
    }
    if let Some(c) = s.convert {
        cfg.convert = c;
    }
    if let Some(p) = s.player {
        cfg.player = p;
    }
    let wanted = wanted_files(index, library_root, &cfg).map_err(|e| e.to_string())?;
    let songs = wanted
        .iter()
        .filter(|w| w.text.is_none() && !w.is_image && !w.rel.ends_with(".lrc"))
        .count();
    let bytes = wanted.iter().map(Wanted::card_bytes).sum();
    let (total, free) = crate::volumes::space(card_root).unwrap_or((0, u64::MAX / 4));
    let ours: u64 = open_manifest(card_root)
        .ok()
        .filter(|_| read_card(card_root).is_some())
        .and_then(|c| {
            c.query_row("SELECT COALESCE(SUM(source_size), 0) FROM held", [], |r| {
                r.get::<_, i64>(0)
            })
            .ok()
        })
        .unwrap_or(0) as u64;
    let reserve = if total == 0 {
        0
    } else {
        (1_000_000_000 + total / 50).min(8_000_000_000)
    };
    let room = (free + ours).saturating_sub(reserve);
    Ok(Estimate {
        songs,
        bytes,
        room,
        reserve,
        fits: bytes <= room,
    })
}

// --- The manifest on the card -----------------------------------------------------------------

#[derive(Debug, Clone)]
struct Held {
    source_size: u64,
    source_mtime_ns: i64,
    audio_md5: Option<String>,
    /// The player recipe the copy was written with (none: an exact copy).
    recipe: Option<String>,
}

fn open_manifest(card_root: &Path) -> Result<Connection, String> {
    let dir = card_root.join(CARD_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let conn = Connection::open(dir.join("manifest.sqlite")).map_err(|e| e.to_string())?;
    // Rollback journal + full sync: the safest settings on a removable FAT/exFAT volume.
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         CREATE TABLE IF NOT EXISTS held (
            path            TEXT PRIMARY KEY,
            source_size     INTEGER NOT NULL,
            source_mtime_ns INTEGER NOT NULL,
            audio_md5       TEXT,
            synced_at       INTEGER NOT NULL
         );",
    )
    .map_err(|e| e.to_string())?;
    // Cards set up before player profiles: add the column, existing copies count as exact.
    let has_recipe = conn.prepare("SELECT recipe FROM held LIMIT 0").is_ok();
    if !has_recipe {
        conn.execute_batch("ALTER TABLE held ADD COLUMN recipe TEXT;")
            .map_err(|e| e.to_string())?;
    }
    Ok(conn)
}

/// A manifest path that stays on the card: inside its music folder, or (a gift's notes and
/// playlists) one level up, at the card's top, when the music has a folder of its own.
fn on_card(path: &str, in_folder: bool) -> bool {
    let up = path
        .strip_prefix("../")
        .or_else(|| path.strip_prefix("..\\"));
    match up {
        Some(rest) => in_folder && crate::paths::is_inside_rel(rest),
        None => crate::paths::is_inside_rel(path),
    }
}

fn load_manifest(conn: &Connection, in_folder: bool) -> Result<HashMap<String, Held>, String> {
    let mut stmt = conn
        .prepare("SELECT path, source_size, source_mtime_ns, audio_md5, recipe FROM held")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Held {
                    source_size: r.get::<_, i64>(1)? as u64,
                    source_mtime_ns: r.get(2)?,
                    audio_md5: r.get(3)?,
                    recipe: r.get(4)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    // A path that would leave the card is never trusted (so never deleted).
    rows.filter(|r| r.as_ref().map_or(true, |(p, _)| on_card(p, in_folder)))
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())
}

fn record(conn: &Connection, w: &Wanted, recipe: Option<&str>) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO held (path, source_size, source_mtime_ns, audio_md5, synced_at, recipe)
         VALUES (?1, ?2, ?3, ?4, strftime('%s','now'), ?5)",
        params![w.rel, w.size as i64, w.mtime_ns, w.audio_md5, recipe],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn forget(conn: &Connection, rel: &str) -> Result<(), String> {
    conn.execute("DELETE FROM held WHERE path = ?1", params![rel])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// --- Planning -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SyncPlan {
    pub card: CardConfig,
    pub copy: usize,
    pub patch: usize,
    pub moves: usize,
    pub adopt: usize,
    pub delete: usize,
    pub unchanged: usize,
    pub bytes_to_copy: u64,
    pub bytes_freed: u64,
    pub free_bytes: u64,
    /// The card's size (0 when it can't be read).
    pub total_bytes: u64,
    /// Kept free on top of what is written: room for the player's own files and the file
    /// system (1 GB plus 2% of the card, at most 8 GB).
    pub reserve_bytes: u64,
    pub fits: bool,
    /// Songs the card carries once synced.
    pub songs: usize,
    /// Worth knowing before syncing (a copied card, a car's limits, FAT32's 4 GB files).
    pub warnings: Vec<String>,
    #[serde(skip)]
    ops: Vec<Op>,
}

#[derive(Debug, Clone)]
enum Op {
    Delete(String),
    /// Card path → new card path (same audio); then patch if tags differ.
    Move {
        from: String,
        to: Wanted,
        from_foreign: bool,
    },
    Copy(Wanted),
    Patch(Wanted),
}

pub fn plan(library_root: &Path, card_root: &Path) -> Result<SyncPlan, String> {
    let index = crate::Index::open(
        &crate::paths::index_path_for(library_root),
        &crate::paths::canonical_root_string(library_root),
    )
    .map_err(|e| e.to_string())?;
    plan_in(&index, library_root, card_root)
}

/// [`plan`] with the library's index already open.
pub fn plan_in(
    index: &crate::Index,
    library_root: &Path,
    card_root: &Path,
) -> Result<SyncPlan, String> {
    let cfg = read_card(card_root).ok_or("This card isn't set up yet.")?;
    let wanted = wanted_files(index, library_root, &cfg).map_err(|e| e.to_string())?;
    let conn = open_manifest(card_root)?;
    let held = load_manifest(&conn, !cfg.folder.is_empty())?;
    let music = card_root.join(&cfg.folder);

    let wanted_set: HashSet<&str> = wanted.iter().map(|w| w.rel.as_str()).collect();
    // Held files no longer wanted: candidates for moving (same audio elsewhere) or deleting.
    // A converted copy isn't the library's audio, so it is never moved, only replaced.
    let mut leaving: HashMap<String, String> = held
        .iter()
        .filter(|(p, h)| !wanted_set.contains(p.as_str()) && !converted(h.recipe.as_deref()))
        .filter_map(|(p, h)| Some((h.audio_md5.clone()?, p.clone())))
        .collect();
    let mut moved_from: HashSet<String> = HashSet::new();
    // Music on the card that the manifest doesn't know: adopt it if the audio matches.
    let mut foreign = foreign_flacs(card_root, &cfg, &held);

    let space = crate::volumes::space(card_root);
    let total_bytes = space.map(|s| s.0).unwrap_or(0);
    if let Some(why) = too_big_for(cfg.player, total_bytes) {
        return Err(why);
    }
    let mut ops = Vec::new();
    let mut p = SyncPlan {
        card: cfg.clone(),
        copy: 0,
        patch: 0,
        moves: 0,
        adopt: 0,
        delete: 0,
        unchanged: 0,
        bytes_to_copy: 0,
        bytes_freed: 0,
        free_bytes: space.map(|s| s.1).unwrap_or(u64::MAX),
        total_bytes,
        reserve_bytes: if total_bytes == 0 {
            0
        } else {
            (1_000_000_000 + total_bytes / 50).min(8_000_000_000)
        },
        fits: true,
        songs: wanted
            .iter()
            .filter(|w| w.text.is_none() && !w.is_image && !w.rel.ends_with(".lrc"))
            .count(),
        warnings: Vec::new(),
        ops: Vec::new(),
    };
    let stamp = stamp_for(&cfg);
    for w in &wanted {
        let want = w.stamp(stamp.as_deref());
        let applies = w.is_flac || w.is_image || w.text.is_some();
        let same_recipe = |h: &Held| !applies || h.recipe == want;
        match held.get(&w.rel) {
            Some(h)
                if h.source_size == w.size
                    && h.source_mtime_ns == w.mtime_ns
                    && same_recipe(h)
                    && music.join(&w.rel).exists() =>
            {
                p.unchanged += 1
            }
            Some(h)
                if (w.is_image || w.text.is_some())
                    && !same_recipe(h)
                    && music.join(&w.rel).exists() =>
            {
                p.copy += 1;
                p.bytes_to_copy += w.card_bytes();
                ops.push(Op::Copy(w.clone()));
            }
            Some(h)
                if w.is_flac
                    && w.convert.is_none()
                    && !converted(h.recipe.as_deref())
                    && h.audio_md5.is_some()
                    && h.audio_md5 == w.audio_md5
                    && music.join(&w.rel).exists() =>
            {
                p.patch += 1;
                ops.push(Op::Patch(w.clone()));
            }
            _ => {
                let md5 = w.audio_md5.as_ref().filter(|_| w.convert.is_none());
                if let Some(md5) = md5
                    && let Some(from) = leaving.remove(md5)
                {
                    moved_from.insert(from.clone());
                    p.moves += 1;
                    ops.push(Op::Move {
                        from,
                        to: w.clone(),
                        from_foreign: false,
                    });
                } else if let Some(md5) = md5
                    && let Some(from) = foreign.remove(md5)
                {
                    p.adopt += 1;
                    ops.push(Op::Move {
                        from,
                        to: w.clone(),
                        from_foreign: true,
                    });
                } else {
                    p.copy += 1;
                    p.bytes_to_copy += w.card_bytes();
                    ops.push(Op::Copy(w.clone()));
                }
            }
        }
    }
    // What Venuta wrote and no longer wants (and didn't move into place): removed. Nothing
    // else on the card is ever touched.
    for path in held.keys() {
        if !wanted_set.contains(path.as_str()) && !moved_from.contains(path) {
            p.delete += 1;
            p.bytes_freed += std::fs::metadata(music.join(path))
                .map(|m| m.len())
                .unwrap_or(0);
            ops.push(Op::Delete(path.clone()));
        }
    }
    p.fits = p.bytes_to_copy.saturating_add(p.reserve_bytes)
        <= p.free_bytes.saturating_add(p.bytes_freed);
    p.warnings = warnings_for(card_root, &cfg, &wanted);
    // Deletes first (free space), then moves, copies, patches.
    ops.sort_by_key(|o| match o {
        Op::Delete(_) => 0,
        Op::Move { .. } => 1,
        Op::Copy(_) => 2,
        Op::Patch(_) => 3,
    });
    p.ops = ops;
    Ok(p)
}

/// FLAC files under the card's music folder that the manifest doesn't know, by audio MD5.
fn foreign_flacs(
    card_root: &Path,
    cfg: &CardConfig,
    held: &HashMap<String, Held>,
) -> HashMap<String, String> {
    let music = card_root.join(&cfg.folder);
    let mut out = HashMap::new();
    for e in walkdir::WalkDir::new(&music).into_iter().flatten() {
        if !e.file_type().is_file()
            || !e
                .path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("flac"))
        {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(&music) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        // The player test is the app's own and never adopted into the library's layout.
        if held.contains_key(&rel) || rel.starts_with(crate::device::TEST_FOLDER) {
            continue;
        }
        if let Ok(layout) = flac_layout::read_layout(e.path()) {
            let md5: String = layout.streaminfo[18..34]
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
            if md5 != "00000000000000000000000000000000" {
                out.insert(md5, rel);
            }
        }
    }
    out
}

// --- Running ------------------------------------------------------------------------------

#[derive(Default)]
pub struct SyncProgress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
}

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub copied: usize,
    pub patched: usize,
    pub moved: usize,
    pub adopted: usize,
    pub deleted: usize,
    pub bytes_written: u64,
    pub errors: Vec<(String, String)>,
    pub cancelled: bool,
    pub elapsed_ms: u128,
}

pub fn run(
    library_root: &Path,
    card_root: &Path,
    plan: &SyncPlan,
    progress: &SyncProgress,
    on_progress: &(dyn Fn(usize, usize, u64) + Sync),
) -> Result<SyncReport, String> {
    if !plan.fits {
        let short = (plan.bytes_to_copy + plan.reserve_bytes)
            .saturating_sub(plan.free_bytes + plan.bytes_freed);
        return Err(format!(
            "This doesn't fit on the card: {} more is needed. Choose less for it, or smaller files.",
            size_words(short)
        ));
    }
    let started = std::time::Instant::now();
    let music = card_root.join(&plan.card.folder);
    let conn = open_manifest(card_root)?;
    // What Venuta put on the card: the only files it may ever replace.
    let ours = load_manifest(&conn, !plan.card.folder.is_empty())?;
    let foreign = |rel: &str| {
        (music.join(rel).symlink_metadata().is_ok() && !ours.contains_key(rel)).then(|| {
            "A file Venuta didn't put there is already in this place on the card; it was left as it is."
                .to_owned()
        })
    };
    cleanup_parts(&music);
    let recipe = plan.card.player.recipe();
    let stamp = stamp_for(&plan.card);
    let stamp_of = |w: &Wanted| w.stamp(stamp.as_deref());
    let converting = plan
        .ops
        .iter()
        .filter(|op| matches!(op, Op::Copy(w) if w.convert.is_some()))
        .count();
    let total = plan.ops.len() + converting;
    progress.total.store(total, Ordering::Relaxed);
    let mut r = SyncReport::default();
    let scratch = std::env::temp_dir()
        .join("venuta-cards")
        .join(if plan.card.id.is_empty() {
            "card"
        } else {
            &plan.card.id
        });
    let _ = std::fs::remove_dir_all(&scratch);
    let ahead = convert_ahead(
        library_root,
        &plan.ops,
        &recipe,
        &scratch,
        progress,
        &|| on_progress(progress.done.load(Ordering::Relaxed), total, 0),
    );

    for (i, op) in plan.ops.iter().enumerate() {
        if progress.cancel.load(Ordering::Relaxed) {
            r.cancelled = true;
            break;
        }
        let result: Result<(), String> = match op {
            Op::Delete(rel) => {
                let path = music.join(rel);
                let res = match std::fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e.to_string()),
                };
                res.and_then(|()| forget(&conn, rel)).map(|()| {
                    remove_empty_parents(&path, &music);
                    r.deleted += 1;
                })
            }
            Op::Move {
                from,
                to,
                from_foreign,
            } => (|| {
                let src = music.join(from);
                let dst = music.join(&to.rel);
                if let Some(e) = foreign(&to.rel) {
                    return Err(e);
                }
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
                if !from_foreign {
                    forget(&conn, from)?;
                }
                remove_empty_parents(&src, &music);
                // Same audio; bring the tags up to date if they differ.
                let written = patch_for(&library_root.join(&to.src), &dst, &recipe)?;
                r.bytes_written += written;
                record(&conn, to, stamp_of(to).as_deref())?;
                if *from_foreign {
                    r.adopted += 1
                } else {
                    r.moved += 1
                }
                Ok(())
            })(),
            Op::Copy(w) => match (foreign(&w.rel), ahead.get(&w.rel)) {
                (Some(e), _) => Err(e),
                // Converted already: onto the card as it is.
                (None, Some(Ok(made))) => copy_file(made, &music.join(&w.rel)),
                (None, Some(Err(e))) => Err(e.clone()),
                (None, None) => {
                    copy_for(&library_root.join(&w.src), &music.join(&w.rel), w, &recipe)
                }
            }
            .and_then(|n| {
                r.bytes_written += n;
                r.copied += 1;
                record(&conn, w, stamp_of(w).as_deref())
            }),
            Op::Patch(w) => patch_for(&library_root.join(&w.src), &music.join(&w.rel), &recipe)
                .and_then(|n| {
                    r.bytes_written += n;
                    r.patched += 1;
                    record(&conn, w, stamp_of(w).as_deref())
                }),
        };
        if let Err(e) = result {
            let what = match op {
                Op::Delete(p) => p.clone(),
                Op::Move { to, .. } => to.rel.clone(),
                Op::Copy(w) | Op::Patch(w) => w.rel.clone(),
            };
            r.errors.push((what, e));
        }
        progress.done.store(converting + i + 1, Ordering::Relaxed);
        progress.bytes.store(r.bytes_written, Ordering::Relaxed);
        on_progress(converting + i + 1, total, r.bytes_written);
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if plan.card.kind == CardKind::Gift {
        hide(&card_root.join(CARD_DIR));
    }
    r.elapsed_ms = started.elapsed().as_millis();
    Ok(r)
}

/// "1.4 GB", "820 MB".
fn size_words(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("{} MB", bytes.div_ceil(1_000_000))
    }
}

/// What to know before a sync: a card whose settings were copied from another card, a car
/// stereo's limits, files FAT32 can't hold.
fn warnings_for(card_root: &Path, cfg: &CardConfig, wanted: &[Wanted]) -> Vec<String> {
    let mut out = Vec::new();
    let volume = crate::volumes::list().into_iter().find(|v| {
        v.root.trim_end_matches(['\\', '/']).to_lowercase()
            == card_root
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase()
    });
    if let (Some(was), Some(v)) = (cfg.serial, &volume)
        && v.serial != 0
        && v.serial != was
    {
        out.push(
            "This card's settings were copied from another card. Make it a card of its own, so the two are kept apart."
                .into(),
        );
    }
    if volume
        .as_ref()
        .is_some_and(|v| v.filesystem.eq_ignore_ascii_case("FAT32"))
    {
        let big = wanted
            .iter()
            .filter(|w| w.convert.is_none() && w.size >= 4 * 1024 * 1024 * 1024)
            .count();
        if big > 0 {
            out.push(format!(
                "{big} file{} too big for FAT32 (4 GB at most) and won't be copied.",
                if big == 1 { " is" } else { "s are" }
            ));
        }
    }
    if cfg.kind == CardKind::Car {
        let files = wanted.iter().filter(|w| w.text.is_none()).count();
        let depth = wanted
            .iter()
            .map(|w| cfg.folder.split('/').count() + w.rel.matches('/').count())
            .max()
            .unwrap_or(0);
        if files > 15_000 {
            out.push(format!(
                "{files} files: many car stereos read only about 15,000. Choose less for this card."
            ));
        }
        if depth > 8 {
            out.push(format!(
                "Folders go {depth} levels deep: many car stereos read only 8."
            ));
        }
    }
    out
}

/// The recipe stamp a card's copies are written with; None for exact copies.
fn stamp_for(cfg: &CardConfig) -> Option<String> {
    cfg.player.recipe().transform.then(|| cfg.player.stamp())
}

/// A new copy on the card, written for its player.
fn copy_for(src: &Path, dst: &Path, w: &Wanted, r: &crate::device::Recipe) -> Result<u64, String> {
    if let Some(text) = &w.text {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_atomic(dst, text.as_bytes())?;
        return Ok(text.len() as u64);
    }
    if let Some(t) = w.convert {
        return write_converted(src, dst, t, r);
    }
    if r.transform && w.is_flac {
        return write_card_flac(src, dst, r);
    }
    if w.is_image
        && let Ok(bytes) = std::fs::read(src)
        && let Some(small) = crate::device::card_image(&bytes, r)
    {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_atomic(dst, &small)?;
        return Ok(small.len() as u64);
    }
    copy_file(src, dst)
}

/// Bring a card copy's tags up to date for its player: in place when they fit before the
/// audio, otherwise the file is written again. The audio itself is never rewritten in place.
fn patch_for(src: &Path, dst: &Path, r: &crate::device::Recipe) -> Result<u64, String> {
    let is_flac = src
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("flac"));
    if !r.transform || !is_flac {
        return patch_or_copy(src, dst);
    }
    let (meta, _) = crate::device::card_metadata(src, r)?;
    let s = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
    if let Ok(d) = flac_layout::read_layout(dst)
        && d.id3v2_len == 0
        && d.streaminfo == s.streaminfo
        && d.audio_len() == s.audio_len()
    {
        // The card metadata ends with an 8 KiB padding block; resize it to fill the space.
        let core = &meta[..meta.len() - (4 + 8192)];
        let room = d.audio_offset as i64 - core.len() as i64 - 4;
        if (0..(1 << 24)).contains(&room) {
            let mut out = core.to_vec();
            let len = (room as u32).to_be_bytes();
            out.extend([0x80 | BLOCK_PADDING, len[1], len[2], len[3]]);
            out.resize(d.audio_offset as usize, 0);
            {
                let mut f = OpenOptions::new()
                    .write(true)
                    .open(dst)
                    .map_err(|e| e.to_string())?;
                f.write_all(&out).map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())?;
            }
            let after = flac_layout::read_layout(dst).map_err(|e| e.to_string())?;
            if after.audio_offset == d.audio_offset && after.streaminfo == d.streaminfo {
                return Ok(out.len() as u64);
            }
        }
    }
    write_card_flac(src, dst, r)
}

/// Write a card copy: the player's metadata, then the library file's audio bytes unchanged.
fn write_card_flac(src: &Path, dst: &Path, r: &crate::device::Recipe) -> Result<u64, String> {
    let (meta, audio_offset) = crate::device::card_metadata(src, r)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = part_path(dst);
    let result = (|| -> Result<u64, String> {
        let mut input = File::open(src).map_err(|e| format!("read: {e}"))?;
        input
            .seek(SeekFrom::Start(audio_offset))
            .map_err(|e| e.to_string())?;
        let mut output = File::create(&part).map_err(|e| format!("create: {e}"))?;
        output.write_all(&meta).map_err(|e| format!("write: {e}"))?;
        let mut buf = vec![0u8; 4 << 20];
        let mut n = meta.len() as u64;
        loop {
            let k = input.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if k == 0 {
                break;
            }
            output
                .write_all(&buf[..k])
                .map_err(|e| format!("write: {e}"))?;
            n += k as u64;
        }
        output.sync_all().map_err(|e| format!("flush: {e}"))?;
        drop(output);
        // Same audio as the library file, byte for byte, read back from the card.
        let s = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
        let d = flac_layout::read_layout(&part).map_err(|e| e.to_string())?;
        if s.streaminfo != d.streaminfo
            || s.audio_len() != d.audio_len()
            || flac_layout::audio_hash(src, &s).map_err(|e| e.to_string())?
                != flac_layout::audio_hash(&part, &d).map_err(|e| e.to_string())?
        {
            return Err("the card copy did not check out".into());
        }
        std::fs::rename(&part, dst).map_err(|e| format!("rename: {e}"))?;
        Ok(n)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn hash_file(path: &Path) -> Result<blake3::Hash, String> {
    let mut h = blake3::Hasher::new();
    let mut f = File::open(path).map_err(|e| format!("read back: {e}"))?;
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let k = f.read(&mut buf).map_err(|e| format!("read back: {e}"))?;
        if k == 0 {
            return Ok(h.finalize());
        }
        h.update(&buf[..k]);
    }
}

/// A FLAC stream's metadata blocks in memory, as (type, body).
type Blocks<'a> = Vec<(u8, &'a [u8])>;

/// The metadata blocks of a FLAC stream in memory, and where its audio starts.
fn flac_blocks(bytes: &[u8]) -> Result<(Blocks<'_>, usize), String> {
    if !bytes.starts_with(b"fLaC") {
        return Err("not a FLAC stream".into());
    }
    let mut at = 4;
    let mut out = Vec::new();
    loop {
        let head = bytes.get(at..at + 4).ok_or("short FLAC stream")?;
        let len = u32::from_be_bytes([0, head[1], head[2], head[3]]) as usize;
        let body = bytes.get(at + 4..at + 4 + len).ok_or("short FLAC stream")?;
        out.push((head[0] & 0x7F, body));
        at += 4 + len;
        if head[0] & 0x80 != 0 {
            return Ok((out, at));
        }
    }
}

/// A converted card copy: 16-bit FLAC (the card's tags and cover over the new audio) or AAC.
/// Written as `.vpart`, read back and checked, then renamed. Returns bytes written.
fn write_converted(
    src: &Path,
    dst: &Path,
    t: Target,
    r: &crate::device::Recipe,
) -> Result<u64, String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = part_path(dst);
    let result = convert_file(src, &part, t, r).and_then(|()| {
        let n = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        std::fs::rename(&part, dst).map_err(|e| format!("rename: {e}"))?;
        Ok(n)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// A library song converted for a card, complete (tags, cover) and checked, at `part`.
fn convert_file(
    src: &Path,
    part: &Path,
    t: Target,
    r: &crate::device::Recipe,
) -> Result<(), String> {
    let part = part.to_path_buf();
    let result = (|| -> Result<(), String> {
        match t {
            Target::Cd16 { rate } => {
                let encoded = crate::convert::flac16(src, rate)?;
                let (enc_blocks, audio_at) = flac_blocks(&encoded)?;
                let streaminfo = enc_blocks
                    .iter()
                    .find(|(k, _)| *k == 0)
                    .map(|(_, b)| *b)
                    .ok_or("no STREAMINFO")?;
                // The card's own blocks for this player, less the source's STREAMINFO and seek
                // table (they describe the other audio).
                let (meta, _) = crate::device::card_metadata(src, r)?;
                let (card_blocks, _) = flac_blocks(&meta)?;
                let mut out = b"fLaC".to_vec();
                let kept: Vec<&(u8, &[u8])> = card_blocks
                    .iter()
                    .filter(|(k, _)| *k != 0 && *k != 3)
                    .collect();
                let mut push = |kind: u8, body: &[u8], last: bool| {
                    let len = (body.len() as u32).to_be_bytes();
                    out.push(if last { 0x80 | kind } else { kind });
                    out.extend(&len[1..]);
                    out.extend(body);
                };
                push(0, streaminfo, kept.is_empty());
                for (i, (kind, body)) in kept.iter().enumerate() {
                    push(*kind, body, i + 1 == kept.len());
                }
                let frames = &encoded[audio_at..];
                out.extend(frames);
                write_atomic_checked(&part, &out)?;
                let d = flac_layout::read_layout(&part).map_err(|e| e.to_string())?;
                if d.streaminfo.as_slice() != streaminfo || d.audio_len() != frames.len() as u64 {
                    return Err("the converted copy did not check out".into());
                }
            }
            Target::Aac => {
                let _ = std::fs::remove_file(&part);
                crate::convert::aac(src, &part, AAC_KBPS as u32)?;
                let px = if r.cover_max_px > 0 {
                    r.cover_max_px
                } else {
                    1000
                };
                crate::convert::tag_m4a(src, &part, px)?;
                if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) < 1024 {
                    return Err("the converted copy came out empty".into());
                }
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Songs a sync converts, made first on this computer, several at once (one per processor
/// core), in a temporary folder: converting is the slow part, and the card is then written in
/// one sequential stream, which is what cards are fastest at. By card path.
fn convert_ahead(
    library_root: &Path,
    ops: &[Op],
    r: &crate::device::Recipe,
    dir: &Path,
    progress: &SyncProgress,
    tick: &(dyn Fn() + Sync),
) -> HashMap<String, Result<PathBuf, String>> {
    use rayon::prelude::*;
    let jobs: Vec<(usize, &Wanted, Target)> = ops
        .iter()
        .enumerate()
        .filter_map(|(i, op)| match op {
            Op::Copy(w) => w.convert.map(|t| (i, w, t)),
            _ => None,
        })
        .collect();
    if jobs.is_empty() {
        return HashMap::new();
    }
    let _ = std::fs::create_dir_all(dir);
    jobs.par_iter()
        .map(|(i, w, t)| {
            if progress.cancel.load(Ordering::Relaxed) {
                return (w.rel.clone(), Err("stopped".to_owned()));
            }
            let out = dir.join(format!("{i}.{}", t.ext()));
            let done = convert_file(&library_root.join(&w.src), &out, *t, r).map(|()| out);
            progress.done.fetch_add(1, Ordering::Relaxed);
            tick();
            (w.rel.clone(), done)
        })
        .collect()
}

/// Write bytes to `path`, flushed, and check what landed is what was sent.
fn write_atomic_checked(path: &Path, bytes: &[u8]) -> Result<(), String> {
    {
        let mut f = File::create(path).map_err(|e| format!("create: {e}"))?;
        f.write_all(bytes).map_err(|e| format!("write: {e}"))?;
        f.sync_all().map_err(|e| format!("flush: {e}"))?;
    }
    if hash_file(path)? != blake3::hash(bytes) {
        return Err("the card copy did not check out".into());
    }
    Ok(())
}

/// Copy as `.vpart`, flush to the card, then rename. Returns bytes written.
fn copy_file(src: &Path, dst: &Path) -> Result<u64, String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = part_path(dst);
    let result = (|| -> Result<u64, String> {
        let mut input = File::open(src).map_err(|e| format!("read: {e}"))?;
        let mut output = File::create(&part).map_err(|e| format!("create: {e}"))?;
        // One sequential stream with a large buffer: what SD cards are fastest at.
        let mut buf = vec![0u8; 4 << 20];
        let mut n = 0u64;
        let mut sent = blake3::Hasher::new();
        loop {
            let k = input.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if k == 0 {
                break;
            }
            sent.update(&buf[..k]);
            output
                .write_all(&buf[..k])
                .map_err(|e| format!("write: {e}"))?;
            n += k as u64;
        }
        output.sync_all().map_err(|e| format!("flush: {e}"))?;
        drop(output);
        let expected = std::fs::metadata(src).map(|m| m.len()).unwrap_or(n);
        if n != expected {
            return Err("size changed while copying".into());
        }
        // Read back from the card: what landed is what was sent.
        if hash_file(&part)? != sent.finalize() {
            return Err("the card copy did not check out".into());
        }
        std::fs::rename(&part, dst).map_err(|e| format!("rename: {e}"))?;
        Ok(n)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Bring a card file's tags in line with the library file of the same audio. If the new
/// metadata fits the space before the audio on the card, only that space is rewritten;
/// otherwise the whole file is copied. Returns bytes written.
fn patch_or_copy(src: &Path, dst: &Path) -> Result<u64, String> {
    let is_flac = src
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("flac"));
    if is_flac && let Ok(n) = patch_metadata(src, dst) {
        return Ok(n);
    }
    copy_file(src, dst)
}

fn patch_metadata(src: &Path, dst: &Path) -> Result<u64, String> {
    let s = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
    let d = flac_layout::read_layout(dst).map_err(|e| e.to_string())?;
    if s.id3v2_len != 0
        || d.id3v2_len != 0
        || s.streaminfo != d.streaminfo
        || s.audio_len() != d.audio_len()
    {
        return Err("not the same audio".into());
    }
    // Without a stored checksum the header can't prove the audio matches: compare the bytes.
    if s.streaminfo[18..34].iter().all(|b| *b == 0)
        && flac_layout::audio_hash(src, &s).map_err(|e| e.to_string())?
            != flac_layout::audio_hash(dst, &d).map_err(|e| e.to_string())?
    {
        return Err("not the same audio".into());
    }
    // New metadata = the source's blocks (without its padding) + padding to fill the card's space.
    let mut f = File::open(src).map_err(|e| e.to_string())?;
    let mut meta = b"fLaC".to_vec();
    for b in s.blocks.iter().filter(|b| b.kind != BLOCK_PADDING) {
        let mut raw = vec![0u8; 4 + b.len as usize];
        f.seek(SeekFrom::Start(b.offset))
            .map_err(|e| e.to_string())?;
        f.read_exact(&mut raw).map_err(|e| e.to_string())?;
        raw[0] &= 0x7F; // not the last block any more
        meta.extend(raw);
    }
    let room = d.audio_offset as i64 - meta.len() as i64 - 4;
    if !(0..(1 << 24)).contains(&room) {
        return Err("new tags don't fit".into());
    }
    let len = (room as u32).to_be_bytes();
    meta.extend([0x80 | BLOCK_PADDING, len[1], len[2], len[3]]);
    meta.resize(d.audio_offset as usize, 0);
    {
        let mut out = OpenOptions::new()
            .write(true)
            .open(dst)
            .map_err(|e| e.to_string())?;
        out.write_all(&meta).map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
    }
    // The audio region was not touched; confirm the file still reads as the same audio.
    let after = flac_layout::read_layout(dst).map_err(|e| e.to_string())?;
    if after.audio_offset != d.audio_offset || after.streaminfo != d.streaminfo {
        return Err("patch check failed".into());
    }
    Ok(meta.len() as u64)
}

fn cleanup_parts(music: &Path) {
    for e in walkdir::WalkDir::new(music).into_iter().flatten() {
        if e.file_type().is_file() && e.path().to_string_lossy().ends_with(".vpart") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn remove_empty_parents(path: &Path, stop: &Path) {
    let mut cur = path.parent();
    while let Some(dir) = cur {
        if dir == stop || !dir.starts_with(stop) || std::fs::remove_dir(dir).is_err() {
            break;
        }
        cur = dir.parent();
    }
}

fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".vpart");
    PathBuf::from(s)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let part = part_path(path);
    {
        let mut f = File::create(&part).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

/// Known cards and other removable drives, for the Cards screen.
#[derive(Debug, Clone, Serialize)]
pub struct CardStatus {
    pub volume: crate::volumes::Volume,
    pub card: Option<CardConfig>,
    pub last_sync: Option<i64>,
    pub files: usize,
}

pub fn cards() -> Vec<CardStatus> {
    crate::volumes::list()
        .into_iter()
        .filter_map(|v| {
            let root = PathBuf::from(&v.root);
            let card = read_card(&root);
            if card.is_none() && !v.removable {
                return None; // only removable drives are offered for setup
            }
            let (last_sync, files) = card
                .as_ref()
                .and_then(|_| {
                    let conn =
                        Connection::open(root.join(CARD_DIR).join("manifest.sqlite")).ok()?;
                    conn.query_row("SELECT MAX(synced_at), COUNT(*) FROM held", [], |r| {
                        Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)? as usize))
                    })
                    .optional()
                    .ok()
                    .flatten()
                })
                .unwrap_or((None, 0));
            Some(CardStatus {
                volume: v,
                card,
                last_sync,
                files,
            })
        })
        .collect()
}

/// Keep your answers to the player test on the card, next to its settings.
pub fn save_test_results(
    card_root: &Path,
    results: std::collections::BTreeMap<String, bool>,
) -> Result<CardConfig, String> {
    let mut cfg = read_card(card_root).ok_or("This card isn't set up yet.")?;
    cfg.test_results = results;
    let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    write_atomic(&card_config_path(card_root), body.as_bytes())?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_test_decides_how_playlists_are_written() {
        let mut cfg: CardConfig =
            toml::from_str("id = \"x\"\nname = \"Card\"\nplayer = \"poweramp\"").unwrap();
        assert_eq!(playlist_style(&cfg), Some(PlaylistStyle::Relative));
        cfg.test_results.insert("playlist_relative".into(), false);
        cfg.test_results.insert("playlist_top".into(), true);
        assert_eq!(playlist_style(&cfg), Some(PlaylistStyle::FromTop));
        cfg.test_results.insert("playlist_top".into(), false);
        assert_eq!(playlist_style(&cfg), None);
        cfg.test_results.clear();
        cfg.player = crate::device::Player::EchoMini;
        assert_eq!(playlist_style(&cfg), None);
    }

    fn entry(path: &str, title: &str, missing: bool) -> PlaylistEntry {
        PlaylistEntry {
            path: if missing { String::new() } else { path.into() },
            title: title.into(),
            artist: "Arijit".into(),
            album: "Album".into(),
            duration_ms: 187_000,
            audio_md5: None,
            missing,
            want: None,
            maybe: None,
            online: None,
        }
    }

    #[test]
    fn card_playlists_point_at_the_cards_songs_and_keep_gaps_only_as_comments() {
        let entries = vec![
            entry("Film/Atrangi Re (2021)/01 - Garda.flac", "Garda", false),
            entry("", "Kun", true),
            entry("Pop/Other/01.flac", "Left out", false),
        ];
        let on_card: HashMap<String, String> = [(
            "film/atrangi re (2021)/01 - garda.flac".to_owned(),
            "Film/Atrangi Re (2021)/01 - Garda.flac".to_owned(),
        )]
        .into();
        let plain = card_playlist("Road", &entries, &on_card, "../Venuta/", false).unwrap();
        assert_eq!(
            plain,
            "#EXTM3U\n#PLAYLIST:Road\n#EXTINF:187,Arijit - Garda\n../Venuta/Film/Atrangi Re (2021)/01 - Garda.flac\n"
        );
        // A gift keeps every other song, as lines players skip (never as a path).
        let gift = card_playlist("Road", &entries, &on_card, "../Venuta/", true).unwrap();
        assert_eq!(
            gift.lines()
                .filter(|l| l.starts_with("#VENUTA-WANT:"))
                .count(),
            2
        );
        assert!(gift.contains("\"title\":\"Kun\""));
        assert!(!gift.contains("Pop/Other"));
        // None of it on the card: no playlist at all.
        assert!(card_playlist("Road", &entries, &HashMap::new(), "../", true).is_none());
    }

    #[test]
    fn card_names_lose_only_what_cards_refuse() {
        assert_eq!(
            safe_name("Kesariya (From \"Brahmastra\")"),
            "Kesariya (From _Brahmastra_)"
        );
        assert_eq!(safe_name("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(safe_name("Album. "), "Album");
        assert_eq!(safe_name("Tum Hi Ho"), "Tum Hi Ho");
    }

    #[test]
    fn card_paths_keep_the_layout_and_shorten_only_what_is_too_long() {
        let long_title = "A".repeat(150);
        let long_album = format!("Film/{} (2009)", "B".repeat(150));
        let files = vec![
            ("Film/Aladin (2009)/01 - Genie Rap.flac".to_owned(), None),
            (
                "Film/Aladin (2009)/02 - Tak Dhina.flac".to_owned(),
                Some("m4a"),
            ),
            (format!("{long_album}/01 - {long_title}.flac"), None),
            (format!("{long_album}/02 - Short.flac"), None),
        ];
        let p = card_paths("Music", &files);
        assert_eq!(
            p["Film/Aladin (2009)/01 - Genie Rap.flac"],
            "Film/Aladin (2009)/01 - Genie Rap.flac"
        );
        assert_eq!(
            p["Film/Aladin (2009)/02 - Tak Dhina.flac"],
            "Film/Aladin (2009)/02 - Tak Dhina.m4a"
        );
        let a = &p[&format!("{long_album}/01 - {long_title}.flac")];
        let b = &p[&format!("{long_album}/02 - Short.flac")];
        for x in [a, b] {
            assert!(chars("Music/") + chars(x) <= MAX_CARD_PATH, "{x}");
        }
        // One album, one folder, still with its year; the song keeps its number.
        let dir = |x: &str| x.rsplit_once('/').unwrap().0.to_owned();
        assert_eq!(dir(a), dir(b));
        assert!(dir(a).ends_with("~ (2009)"));
        assert!(a.rsplit('/').next().unwrap().starts_with("01 - AAA"));
        assert!(a.ends_with(".flac"));
    }

    // --- A real (tiny) library, synced to a folder that stands in for a card -------------------

    /// A FLAC that decodes: `secs` of a sine at `rate`/`bits`, with these tags.
    fn make_flac(path: &Path, rate: usize, bits: usize, secs: f32, tags: &[(&str, &str)]) {
        use flacenc::component::BitRepr;
        use flacenc::error::Verify;
        let frames = (rate as f32 * secs) as usize;
        let amp = ((1i64 << (bits - 1)) as f32) * 0.5;
        let samples: Vec<i32> = (0..frames)
            .flat_map(|i| {
                let v =
                    ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * amp) as i32;
                [v, v]
            })
            .collect();
        let config = flacenc::config::Encoder::default().into_verified().unwrap();
        let source = flacenc::source::MemSource::from_samples(&samples, 2, bits, rate);
        let stream =
            flacenc::encode_with_fixed_block_size(&config, source, config.block_size).unwrap();
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).unwrap();
        let mut encoded = sink.as_slice().to_vec();
        crate::convert::fix_min_block(&mut encoded);
        let (blocks, audio_at) = flac_blocks(&encoded).unwrap();
        let mut comment = Vec::new();
        let vendor = b"test";
        comment.extend((vendor.len() as u32).to_le_bytes());
        comment.extend(vendor);
        comment.extend((tags.len() as u32).to_le_bytes());
        for (k, v) in tags {
            let e = format!("{k}={v}");
            comment.extend((e.len() as u32).to_le_bytes());
            comment.extend(e.as_bytes());
        }
        let mut out = b"fLaC".to_vec();
        let si = blocks[0].1;
        out.push(0);
        out.extend(&(si.len() as u32).to_be_bytes()[1..]);
        out.extend(si);
        out.push(0x80 | 4);
        out.extend(&(comment.len() as u32).to_be_bytes()[1..]);
        out.extend(&comment);
        out.extend(&encoded[audio_at..]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn song(album: &str, n: u32, title: &str) -> Vec<(&'static str, String)> {
        vec![
            ("TITLE", title.into()),
            ("ARTIST", "Vishal-Shekhar".into()),
            ("ALBUM", album.into()),
            ("TRACKNUMBER", n.to_string()),
            ("TRACKTOTAL", "4".into()),
            ("DATE", "2009".into()),
        ]
    }

    fn library() -> (tempfile::TempDir, PathBuf, crate::Index) {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("Music");
        std::fs::create_dir_all(home.path().join(".venuta")).unwrap();
        let a = root.join("Film/Aladin (2009)");
        for (n, t) in [
            (1, "Genie Rap"),
            (2, "Tak Dhina Dhin"),
            (3, "O Re Saawariya"),
        ] {
            let tags = song("Aladin", n, t);
            let tags: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (*k, v.as_str())).collect();
            make_flac(&a.join(format!("0{n} - {t}.flac")), 44_100, 16, 0.2, &tags);
        }
        std::fs::write(a.join("cover.jpg"), b"not really a jpeg").unwrap();
        let b = root.join("Pop/Other (2010)");
        let tags = song("Other", 1, "Four");
        let tags: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (*k, v.as_str())).collect();
        make_flac(&b.join("01 - Four.flac"), 96_000, 24, 0.2, &tags);
        std::fs::write(b.join("01 - Four.lrc"), "[00:00.00]Four").unwrap();
        let lists = home.path().join("Playlists");
        std::fs::create_dir_all(&lists).unwrap();
        std::fs::write(
            lists.join("Road.m3u8"),
            "#EXTM3U\n#PLAYLIST:Road\n#EXTINF:0,Vishal-Shekhar - Genie Rap\n../Music/Film/Aladin (2009)/01 - Genie Rap.flac\n#VENUTA-WANT:{\"title\":\"Kun Faya Kun\",\"artist\":\"A. R. Rahman\"}\n",
        )
        .unwrap();
        let mut index = crate::Index::open_in_memory(&root.to_string_lossy()).unwrap();
        crate::scan(&mut index, &root).unwrap();
        (home, root, index)
    }

    fn sync(index: &crate::Index, root: &Path, card: &Path) -> (SyncPlan, SyncReport) {
        let plan = plan_in(index, root, card).unwrap();
        let report = run(root, card, &plan, &SyncProgress::default(), &|_, _, _| {}).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        (plan, report)
    }

    #[test]
    fn a_library_kept_in_its_music_folder_isnt_nested_on_the_card() {
        // As the app opens a library: the index is the library's, songs under its Music folder.
        let (home, _music, _) = library();
        let mut index = crate::Index::open_in_memory(&home.path().to_string_lossy()).unwrap();
        crate::scan(&mut index, home.path()).unwrap();
        let card = tempfile::tempdir().unwrap();
        setup_card(
            card.path(),
            CardSettings {
                name: "Disc".into(),
                pick: Some(Pick {
                    playlists: vec!["Road".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        sync(&index, home.path(), card.path());
        let music = card.path().join("Music");
        assert!(
            music
                .join("Film/Aladin (2009)/01 - Genie Rap.flac")
                .is_file()
        );
        assert!(
            !music.join("Music").exists(),
            "no Music folder inside Music"
        );
        let road = std::fs::read_to_string(card.path().join("Playlists/Road.m3u8")).unwrap();
        assert!(
            road.contains("\n../Music/Film/Aladin (2009)/01 - Genie Rap.flac\n"),
            "{road}"
        );
    }

    #[test]
    fn paths_on_a_card_never_leave_it() {
        assert!(super::on_card("Film/A (2009)/01 - X.flac", true));
        assert!(super::on_card("../README.txt", true));
        assert!(
            !super::on_card("../README.txt", false),
            "no folder to step out of"
        );
        assert!(!super::on_card("../../Windows/x", true));
        assert!(!super::on_card("C:/Users/x", true));
    }

    #[test]
    fn a_gift_of_one_playlist_is_a_small_library_that_says_what_is_missing() {
        let (_home, root, index) = library();
        let card = tempfile::tempdir().unwrap();
        setup_card(
            card.path(),
            CardSettings {
                name: "Music for Riya".into(),
                pick: Some(Pick {
                    playlists: vec!["Road".into()],
                    ..Default::default()
                }),
                kind: Some(CardKind::Gift),
                ..Default::default()
            },
        )
        .unwrap();
        let (plan, _) = sync(&index, &root, card.path());
        assert_eq!(plan.songs, 1);
        let music = card.path().join("Music");
        let album = music.join("Film/Aladin (2009)");
        // The chosen song, in its album's own folder, with the album's cover; nothing else.
        assert!(album.join("01 - Genie Rap.flac").is_file());
        assert!(album.join("cover.jpg").is_file());
        assert!(!album.join("02 - Tak Dhina Dhin.flac").exists());
        assert!(!music.join("Pop").exists());
        // What's missing, for people.
        let list = std::fs::read_to_string(album.join("Tracklist.txt")).unwrap();
        assert!(list.contains("1 of 4 songs are on this card"), "{list}");
        assert!(
            list.contains("02  Tak Dhina Dhin  [not on this card]"),
            "{list}"
        );
        assert!(
            list.contains("04  (title not known)  [not in the library]"),
            "{list}"
        );
        let missing = std::fs::read_to_string(card.path().join("Missing.csv")).unwrap();
        assert!(
            missing.contains("Kun Faya Kun,A. R. Rahman,not in the library,Road"),
            "{missing}"
        );
        let readme = std::fs::read_to_string(card.path().join("README.txt")).unwrap();
        assert!(
            readme.starts_with("Music for Riya\r\nMade with Venuta on "),
            "{readme}"
        );
        // And for Venuta: the playlist keeps its gap as a line players skip.
        let road = std::fs::read_to_string(card.path().join("Playlists/Road.m3u8")).unwrap();
        assert!(road.contains("../Music/Film/Aladin (2009)/01 - Genie Rap.flac"));
        assert!(road.contains("#VENUTA-WANT:") && road.contains("Kun Faya Kun"));

        // Synced again: nothing to do.
        let again = plan_in(&index, &root, card.path()).unwrap();
        assert_eq!((again.copy, again.delete, again.patch), (0, 0, 0));

        // Now an album instead: what Venuta wrote goes, the new album comes, lyrics with it.
        setup_card(
            card.path(),
            CardSettings {
                name: "Music for Riya".into(),
                pick: Some(Pick {
                    albums: vec!["Pop/Other (2010)".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(card.path().join("Music/not ours.txt"), "keep me").unwrap();
        sync(&index, &root, card.path());
        assert!(
            !album.exists(),
            "the old album's folder, with its track list, is gone"
        );
        assert!(music.join("Pop/Other (2010)/01 - Four.flac").is_file());
        assert!(music.join("Pop/Other (2010)/01 - Four.lrc").is_file());
        assert!(!card.path().join("Playlists/Road.m3u8").exists());
        assert!(
            music.join("not ours.txt").is_file(),
            "never touch what Venuta didn't write"
        );
    }

    #[test]
    fn hi_res_goes_onto_a_card_as_16_bit_flac_of_the_same_length() {
        let (_home, root, index) = library();
        let card = tempfile::tempdir().unwrap();
        setup_card(
            card.path(),
            CardSettings {
                name: "Small".into(),
                pick: Some(Pick {
                    albums: vec!["Pop/Other (2010)".into()],
                    ..Default::default()
                }),
                convert: Some(Convert {
                    hires: true,
                    aac: false,
                }),
                ..Default::default()
            },
        )
        .unwrap();
        sync(&index, &root, card.path());
        let out = card.path().join("Music/Pop/Other (2010)/01 - Four.flac");
        let l = flac_layout::read_layout(&out).unwrap();
        let s = &l.streaminfo;
        let rate = (u32::from(s[10]) << 12) | (u32::from(s[11]) << 4) | (u32::from(s[12]) >> 4);
        let bits = (((s[12] & 1) << 4) | (s[13] >> 4)) + 1;
        let total = (u64::from(s[13] & 0x0F) << 32)
            | u64::from(u32::from_be_bytes([s[14], s[15], s[16], s[17]]));
        // 96 kHz is of the 48 kHz family: halved exactly, never moved to 44.1.
        assert_eq!((rate, bits), (48_000, 16));
        // 0.2 s at 96 kHz is 19,200 frames: 9,600 at 48 kHz.
        assert!((9_590..=9_610).contains(&total), "{total}");
        // The tags came along.
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.windows(10).any(|w| w == b"TITLE=Four"));
        // Synced again: the conversion is not done again.
        let again = plan_in(&index, &root, card.path()).unwrap();
        assert_eq!((again.copy, again.patch, again.delete), (0, 0, 0));
    }

    #[cfg(windows)]
    #[test]
    fn lossless_goes_onto_a_car_card_as_tagged_aac() {
        use lofty::file::TaggedFileExt;
        use lofty::tag::Accessor;
        let (_home, root, index) = library();
        let card = tempfile::tempdir().unwrap();
        setup_card(
            card.path(),
            CardSettings {
                name: "Car".into(),
                pick: Some(Pick {
                    albums: vec!["Film/Aladin (2009)".into()],
                    ..Default::default()
                }),
                kind: Some(CardKind::Car),
                convert: Some(Convert {
                    hires: false,
                    aac: true,
                }),
                ..Default::default()
            },
        )
        .unwrap();
        sync(&index, &root, card.path());
        let out = card
            .path()
            .join("Music/Film/Aladin (2009)/01 - Genie Rap.m4a");
        let tagged = lofty::read_from_path(&out).unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Genie Rap"));
        assert!(
            !card
                .path()
                .join("Music/Film/Aladin (2009)/01 - Genie Rap.flac")
                .exists()
        );
    }

    fn block(last: bool, kind: u8, body: &[u8]) -> Vec<u8> {
        let len = (body.len() as u32).to_be_bytes();
        let mut v = vec![(u8::from(last) << 7) | kind, len[1], len[2], len[3]];
        v.extend_from_slice(body);
        v
    }

    fn flac(comment: &[u8], padding: usize, audio: &[u8]) -> Vec<u8> {
        flac_with(&[9u8; 34], comment, padding, audio)
    }

    fn flac_with(streaminfo: &[u8; 34], comment: &[u8], padding: usize, audio: &[u8]) -> Vec<u8> {
        let mut v = b"fLaC".to_vec();
        v.extend(block(false, 0, streaminfo));
        v.extend(block(false, 4, comment));
        v.extend(block(true, BLOCK_PADDING, &vec![0u8; padding]));
        v.extend_from_slice(audio);
        v
    }

    #[test]
    fn patch_rewrites_only_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.flac");
        let dst = dir.path().join("dst.flac");
        // Library file: new, longer tags. Card file: old tags with generous padding.
        std::fs::write(
            &src,
            flac(b"NEW TAGS, LONGER THAN BEFORE", 10, b"AUDIO-FRAMES"),
        )
        .unwrap();
        std::fs::write(&dst, flac(b"OLD", 200, b"AUDIO-FRAMES")).unwrap();
        let before = flac_layout::read_layout(&dst).unwrap();
        patch_metadata(&src, &dst).unwrap();
        let after = flac_layout::read_layout(&dst).unwrap();
        assert_eq!(after.audio_offset, before.audio_offset);
        assert_eq!(
            flac_layout::audio_fingerprint(&dst).unwrap(),
            flac_layout::audio_fingerprint(&src).unwrap()
        );
        let bytes = std::fs::read(&dst).unwrap();
        assert!(
            bytes
                .windows(28)
                .any(|w| w == b"NEW TAGS, LONGER THAN BEFORE")
        );
    }

    #[test]
    fn patch_refuses_when_it_does_not_fit_or_audio_differs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.flac");
        let dst = dir.path().join("dst.flac");
        std::fs::write(&src, flac(&[b'x'; 500], 0, b"AUDIO")).unwrap();
        std::fs::write(&dst, flac(b"OLD", 10, b"AUDIO")).unwrap();
        assert!(patch_metadata(&src, &dst).is_err()); // too big: caller falls back to a copy
        // Different audio (different checksum in STREAMINFO).
        std::fs::write(&src, flac_with(&[7u8; 34], b"NEW", 0, b"OTHER")).unwrap();
        assert!(patch_metadata(&src, &dst).is_err());
        // No checksum stored (all zero): the audio bytes themselves are compared.
        let mut no_md5 = [9u8; 34];
        no_md5[18..34].fill(0);
        std::fs::write(&src, flac_with(&no_md5, b"NEW", 0, b"OTHER")).unwrap();
        std::fs::write(&dst, flac_with(&no_md5, b"OLD", 100, b"AUDIO")).unwrap();
        assert!(patch_metadata(&src, &dst).is_err());
        std::fs::write(&src, flac_with(&no_md5, b"NEW", 0, b"AUDIO")).unwrap();
        assert!(patch_metadata(&src, &dst).is_ok());
    }
}
