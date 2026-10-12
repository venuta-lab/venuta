//! Read your own Apple Music library (`Library.musicdb`) to bring your playlists over.
//!
//! Opt-in: the file is encrypted with a fixed key that this project does not ship; you enter
//! it once in settings (it is published in several open-source projects). The library file is
//! copied first and only the copy is read; nothing is ever written to Apple's files.
//!
//! Format (community reverse-engineering): `hfma` header, then a payload whose first N bytes
//! are AES-128-ECB encrypted (N stored in the header), then zlib-compressed. Inside: tracks
//! (`itma`) with string records (`boma`), playlists (`lpma`) with item records (`ipfa`).

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, KeyInit};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize)]
pub struct AppleTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub composer: String,
    pub genre: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplePlaylist {
    pub name: String,
    pub tracks: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppleLibrary {
    pub tracks: HashMap<u64, AppleTrack>,
    pub playlists: Vec<ApplePlaylist>,
}

/// Apple's built-in lists that are not playlists you made.
const SYSTEM_LISTS: &[&str] = &[
    "####!####",
    "Music",
    "Downloaded",
    "Music Videos",
    "TV & Movies",
    "Genius",
    "Hidden Cloud PlaylistOnly Tracks",
];

/// Where Apple Music for Windows keeps its library.
pub fn default_library_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let p = Path::new(&home)
        .join("Music")
        .join("Apple Music")
        .join("Apple Music Library.musiclibrary")
        .join("Library.musicdb");
    p.exists().then_some(p)
}

pub fn read(path: &Path, key: &str) -> Result<AppleLibrary, String> {
    let key: [u8; 16] = key
        .as_bytes()
        .try_into()
        .map_err(|_| "The key must be exactly 16 characters.".to_owned())?;
    // Read a copy's bytes: the original is only ever read, and only once.
    let raw = std::fs::read(path).map_err(|e| format!("read library: {e}"))?;
    if raw.len() < 0x60 || &raw[..4] != b"hfma" {
        return Err("This doesn't look like an Apple Music library file.".into());
    }
    let header = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let max_crypt = u32::from_le_bytes(raw[0x54..0x58].try_into().unwrap()) as usize;
    let mut payload = raw.get(header..).ok_or("truncated file")?.to_vec();
    let n = payload.len().min(max_crypt) / 16 * 16;
    let cipher = Aes128::new(&Array::from(key));
    for block in payload[..n].as_chunks_mut::<16>().0 {
        let mut b = Array::from(*block);
        cipher.decrypt_block(&mut b);
        block.copy_from_slice(&b);
    }
    let mut data = Vec::new();
    flate2::read::ZlibDecoder::new(&payload[..])
        .read_to_end(&mut data)
        .map_err(|_| "Couldn't unpack the library. Check the key.".to_owned())?;
    Ok(parse(&data))
}

fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

fn u64_at(d: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(i..i + 8)?.try_into().ok()?))
}

fn find_all(d: &[u8], tag: &[u8; 4]) -> Vec<usize> {
    d.windows(4)
        .enumerate()
        .filter(|(_, w)| w == tag)
        .map(|(i, _)| i)
        .collect()
}

fn parse(d: &[u8]) -> AppleLibrary {
    // Tracks: each `itma` is followed by its `boma` string records until the next `itma`.
    let mut tracks: HashMap<u64, AppleTrack> = HashMap::new();
    let mut current: Option<u64> = None;
    let mut i = 0;
    let playlist_start = find_all(d, b"lPma").first().copied().unwrap_or(d.len());
    while i + 8 < playlist_start {
        match &d[i..i + 4] {
            b"itma" => {
                if let Some(id) = u64_at(d, i + 16) {
                    tracks.insert(
                        id,
                        AppleTrack {
                            id,
                            ..Default::default()
                        },
                    );
                    current = Some(id);
                }
                i += u32_at(d, i + 4).unwrap_or(8).max(8) as usize;
            }
            b"boma" => {
                let total = u32_at(d, i + 8).unwrap_or(8).max(8) as usize;
                if let (Some(id), Some(sub)) = (current, u32_at(d, i + 12))
                    && let Some(s) = boma_string(d, i)
                    && let Some(t) = tracks.get_mut(&id)
                {
                    match sub {
                        0x2 => t.title = s,
                        0x3 => t.album = s,
                        0x4 => t.artist = s,
                        0x5 => t.genre = s,
                        0xC => t.composer = s,
                        0x1B => t.album_artist = s,
                        _ => {}
                    }
                }
                i += total;
            }
            _ => i += 1,
        }
    }
    // Playlists: everything between one `lpma` and the next belongs to it.
    let starts = find_all(d, b"lpma");
    let mut playlists = Vec::new();
    for (k, &a) in starts.iter().enumerate() {
        let b = starts.get(k + 1).copied().unwrap_or(d.len());
        let region = &d[a..b];
        let name = find_all(region, b"boma")
            .into_iter()
            .find(|&o| u32_at(region, o + 12) == Some(0xC8))
            .and_then(|o| boma_string(region, o))
            .unwrap_or_default();
        let items: Vec<u64> = find_all(region, b"ipfa")
            .into_iter()
            .filter_map(|o| u64_at(region, o + 20))
            .filter(|id| tracks.contains_key(id))
            .collect();
        if !SYSTEM_LISTS.contains(&name.as_str()) && !items.is_empty() {
            playlists.push(ApplePlaylist {
                name,
                tracks: items,
            });
        }
    }
    AppleLibrary { tracks, playlists }
}

/// A `boma` string record: encoding flag at +20 (1 = UTF-16LE), length at +24, text at +36.
fn boma_string(d: &[u8], i: usize) -> Option<String> {
    let enc = u32_at(d, i + 20)?;
    let len = u32_at(d, i + 24)? as usize;
    let bytes = d.get(i + 36..i + 36 + len)?;
    Some(if enc == 1 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    })
}
