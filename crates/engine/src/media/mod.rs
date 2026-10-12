//! Reading facts out of media files. Read-only: nothing here ever writes to a file.

mod flac;
pub mod picture;

use serde::{Deserialize, Serialize};
use std::path::Path;

pub use picture::{ImageFormat, ImageInfo};

/// What kind of file this is, decided by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// Lossless audio we read fully.
    Flac,
    /// Other audio we recognise but do not read deeply yet.
    OtherAudio,
    Lyrics,
    Image,
    Playlist,
    Other,
}

impl FileKind {
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "flac" => Self::Flac,
            "mp3" | "m4a" | "aac" | "alac" | "ogg" | "opus" | "wav" | "aif" | "aiff" | "wv"
            | "ape" | "dsf" | "dff" => Self::OtherAudio,
            "lrc" | "lrc8" => Self::Lyrics,
            "jpg" | "jpeg" | "png" | "webp" => Self::Image,
            "m3u" | "m3u8" | "pls" | "xspf" => Self::Playlist,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::OtherAudio => "other_audio",
            Self::Lyrics => "lyrics",
            Self::Image => "image",
            Self::Playlist => "playlist",
            Self::Other => "other",
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, Self::Flac | Self::OtherAudio)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioProps {
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub channels: u8,
    pub duration_ms: u64,
    /// MD5 of the decoded audio from STREAMINFO. `None` when the encoder left it unset (all zero).
    pub audio_md5: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureFacts {
    /// FLAC/ID3 picture type (3 = front cover).
    pub pic_type: u8,
    pub mime: Option<String>,
    pub bytes: u64,
    /// Parsed from the image itself; `None` if unreadable.
    pub image: Option<ImageInfo>,
    /// blake3 of the image bytes, to compare embedded art with `cover.jpg`.
    pub data_hash: [u8; 32],
}

/// Everything the scanner learns from one audio file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaFacts {
    pub audio: Option<AudioProps>,
    /// Every tag, in file order, with the key exactly as stored. Unknown keys are kept.
    pub tags: Vec<(String, String)>,
    /// Vorbis comment vendor string (identifies the last encoder/tagger).
    pub vendor: Option<String>,
    pub pictures: Vec<PictureFacts>,
}

/// Read an audio file's facts. Errors are returned as text so the caller can record them
/// against the file instead of aborting a scan.
pub fn read_facts(path: &Path, kind: FileKind) -> Result<MediaFacts, String> {
    match kind {
        FileKind::Flac => flac::read(path),
        _ => Ok(MediaFacts::default()),
    }
}

/// Stable hash of a tag list, used to detect tag-only changes cheaply.
pub fn tags_hash(tags: &[(String, String)]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for (k, v) in tags {
        h.update(k.as_bytes());
        h.update(&[0x1F]);
        h.update(v.as_bytes());
        h.update(&[0x1E]);
    }
    *h.finalize().as_bytes()
}
