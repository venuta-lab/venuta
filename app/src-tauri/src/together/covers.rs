//! Covers of the host's own songs, for the room. The host makes each one's thumbnail once, from
//! the library's covers, and serves it to the people in the room; a guest asks for the ones it
//! lacks and keeps them with its thumbnails. File paths never leave the host's computer: only the
//! picture goes over the room connection, asked for by the song's key.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use together::proto::RoomView;

/// The thumbnails' size (square, pixels): the same as the library's.
pub const SIZE: u32 = 320;
/// The most a host's picture may be.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Default)]
pub struct Covers {
    paths: Mutex<HashMap<String, PathBuf>>,
    asked: Mutex<HashSet<String>>,
}

impl Covers {
    pub fn get(&self, key: &str) -> Option<PathBuf> {
        self.paths.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: &str, path: PathBuf) {
        if let Ok(mut m) = self.paths.lock() {
            m.insert(key.to_owned(), path);
        }
    }

    /// True the first time a song's cover is asked for (each is fetched once).
    pub fn first_ask(&self, key: &str) -> bool {
        self.asked
            .lock()
            .is_ok_and(|mut a| a.insert(key.to_owned()))
    }

    /// The room as this computer's windows show it: the host's songs with their covers.
    pub fn decorate(&self, room: &mut RoomView) {
        let Ok(paths) = self.paths.lock() else {
            return;
        };
        for s in &mut room.queue {
            if s.host_file
                && s.artwork_url.is_none()
                && let Some(p) = paths.get(&s.key)
            {
                s.artwork_url = Some(asset_url(p));
            }
        }
    }
}

/// A file on this computer as the windows load it (the asset protocol, as `convertFileSrc`).
pub fn asset_url(p: &Path) -> String {
    format!(
        "http://asset.localhost/{}",
        urlencoding::encode(&p.to_string_lossy())
    )
}

/// Where a guest keeps a room song's cover: with the thumbnails (which the windows may load).
pub fn kept_path(key: &str) -> Option<PathBuf> {
    let hex = key
        .strip_prefix("host:")
        .filter(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    Some(engine::art::thumbs_dir().join(format!("room-{hex}-{SIZE}.jpg")))
}

/// The host's covers for these songs (key, album folder), made from the library. Slow the first
/// time (the thumbnails are made then), so off the async threads.
pub fn make(root: &Path, songs: &[(String, String)]) -> Vec<(String, PathBuf)> {
    if songs.is_empty() {
        return Vec::new();
    }
    let Ok((_, lib)) = crate::load_lib(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = songs.iter().map(|(_, d)| d.clone()).collect();
    dirs.sort();
    dirs.dedup();
    let by_dir: HashMap<String, PathBuf> =
        engine::art::thumbnails(root, lib.cover_requests(&dirs), SIZE)
            .into_iter()
            .map(|t| (t.dir, PathBuf::from(t.path)))
            .collect();
    songs
        .iter()
        .filter_map(|(key, dir)| Some((key.clone(), by_dir.get(dir)?.clone())))
        .collect()
}

/// A picture from the host, made safe here: decoded (within limits) and saved again as this
/// computer's own small JPEG. Nothing from the host is written as it came.
pub fn keep(key: &str, bytes: &[u8]) -> Option<PathBuf> {
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let out = kept_path(key)?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let small = reader
        .decode()
        .ok()?
        .resize_to_fill(SIZE, SIZE, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    std::fs::create_dir_all(engine::art::thumbs_dir()).ok()?;
    // A temp name then rename, so a half picture is never shown.
    let tmp = out.with_extension("jpg.part");
    small
        .save_with_format(&tmp, image::ImageFormat::Jpeg)
        .ok()?;
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_room_songs_key_names_a_kept_cover() {
        assert!(kept_path("host:0123456789abcdef0123456789abcdef").is_some());
        assert!(
            kept_path("host:../../etc").is_none(),
            "no way out of the folder"
        );
        assert!(kept_path("online:abcdefghijk").is_none());
    }

    #[test]
    fn what_isnt_a_picture_is_never_kept() {
        let key = "host:0123456789abcdef0123456789abcdef";
        assert!(keep(key, b"not a picture").is_none());
        assert!(keep(key, &vec![0u8; MAX_BYTES as usize + 1]).is_none());
    }

    #[test]
    fn each_cover_is_asked_for_once_and_loaded_as_a_local_asset() {
        let covers = Covers::default();
        covers.put("host:a", PathBuf::from(r"C:\thumbs\x y.jpg"));
        let url = asset_url(Path::new(r"C:\thumbs\x y.jpg"));
        assert!(url.starts_with("http://asset.localhost/C%3A%5Cthumbs%5Cx%20y.jpg"));
        assert!(covers.first_ask("host:a"));
        assert!(!covers.first_ask("host:a"), "asked once");
    }
}
