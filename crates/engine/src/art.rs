//! Cover thumbnails for the UI, and an accent colour picked from each cover.
//! Thumbnails are cached by the hash of the embedded image, so they are made once.

use crate::paths;
use image::imageops::FilterType;
use image::{ImageReader, RgbImage};
use lofty::config::ParseOptions;
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::ogg::OggPictureStorage;
use lofty::picture::PictureType;
use rayon::prelude::*;
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct CoverThumb {
    pub dir: String,
    /// Absolute path of the cached JPEG thumbnail.
    pub path: String,
    /// A vivid colour from the cover, as `#rrggbb`.
    pub accent: Option<String>,
}

pub struct CoverRequest {
    pub dir: String,
    pub track_rel: String,
    pub data_hash: Vec<u8>,
}

pub fn thumbs_dir() -> PathBuf {
    paths::app_data_dir().join("thumbs")
}

/// Make (or reuse) thumbnails for many albums in parallel. Failures are skipped.
pub fn thumbnails(root: &Path, requests: Vec<CoverRequest>, size: u32) -> Vec<CoverThumb> {
    let dir = thumbs_dir();
    let _ = std::fs::create_dir_all(&dir);
    requests
        .into_par_iter()
        .filter_map(|r| thumbnail(root, &dir, r, size).ok())
        .collect()
}

fn thumbnail(root: &Path, dir: &Path, req: CoverRequest, size: u32) -> Result<CoverThumb, String> {
    let hex: String = req
        .data_hash
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect();
    let out = dir.join(format!("{hex}-{size}.jpg"));
    let img = if out.exists() {
        ImageReader::open(&out)
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?
            .to_rgb8()
    } else {
        let data = front_cover_bytes(&root.join(&req.track_rel))?;
        let decoded = ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?;
        let small = decoded
            .resize_to_fill(size, size, FilterType::Lanczos3)
            .to_rgb8();
        // Write to a temp name then rename, so a crash never leaves a half thumbnail.
        let tmp = out.with_extension("jpg.part");
        small
            .save_with_format(&tmp, image::ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
        small
    };
    Ok(CoverThumb {
        dir: req.dir,
        path: out.to_string_lossy().into_owned(),
        accent: accent_color(&img),
    })
}

fn front_cover_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut r = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let flac = FlacFile::read_from(&mut r, ParseOptions::new()).map_err(|e| e.to_string())?;
    let pics = flac.pictures();
    pics.iter()
        .find(|(p, _)| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pics.first())
        .map(|(p, _)| p.data().to_vec())
        .ok_or_else(|| "no picture".to_owned())
}

/// Any audio file's front cover (FLAC above, the rest through their tags).
fn cover_bytes_any(path: &Path) -> Result<Vec<u8>, String> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("flac"))
    {
        return front_cover_bytes(path);
    }
    use lofty::file::TaggedFileExt;
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let pics: Vec<_> = file.tags().iter().flat_map(|t| t.pictures()).collect();
    pics.iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pics.first())
        .map(|p| p.data().to_vec())
        .ok_or_else(|| "no picture".to_owned())
}

/// A picture file of a song's cover that other programs can open (the Windows media flyout,
/// the tray): the album folder's own image, else the embedded art written to a small cache
/// (named by its hash, so the same cover is written once; only the latest few are kept).
pub fn cover_file_for(track: &Path) -> Option<PathBuf> {
    let dir = track.parent()?;
    for name in [
        "cover.jpg",
        "cover.png",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "front.png",
    ] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let data = cover_bytes_any(track).ok()?;
    let ext = if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpg"
    };
    let hash = blake3::hash(&data).to_hex();
    let out_dir = paths::app_data_dir().join("now-playing");
    std::fs::create_dir_all(&out_dir).ok()?;
    let out = out_dir.join(format!("{}.{ext}", &hash.as_str()[..16]));
    if !out.is_file() {
        let tmp = out.with_extension(format!("{ext}.part"));
        std::fs::write(&tmp, &data).ok()?;
        std::fs::rename(&tmp, &out).ok()?;
        // Keep the newest eight.
        let mut old: Vec<_> = std::fs::read_dir(&out_dir)
            .ok()?
            .flatten()
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        old.sort();
        let n = old.len().saturating_sub(8);
        for (_, p) in old.into_iter().take(n) {
            let _ = std::fs::remove_file(p);
        }
    }
    Some(out)
}

/// Pick a vivid, mid-bright colour: average the most saturated pixels of a small copy.
pub fn accent_color(img: &RgbImage) -> Option<String> {
    let small = image::imageops::thumbnail(img, 32, 32);
    let mut scored: Vec<(f32, [u8; 3])> = small
        .pixels()
        .map(|p| {
            let [r, g, b] = p.0;
            let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
            let max = rf.max(gf).max(bf);
            let min = rf.min(gf).min(bf);
            let sat = if max == 0.0 { 0.0 } else { (max - min) / max };
            // Prefer saturated colours that are neither near-black nor near-white.
            let light = 1.0 - (max - 0.65).abs() * 1.6;
            (sat * light.max(0.0), [r, g, b])
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let top = &scored[..(scored.len() / 7).max(1)];
    if top[0].0 < 0.12 {
        return None; // greyscale cover: let the UI use its default accent
    }
    let n = top.len() as u32;
    let sum = top.iter().fold([0u32; 3], |acc, (_, c)| {
        [
            acc[0] + c[0] as u32,
            acc[1] + c[1] as u32,
            acc[2] + c[2] as u32,
        ]
    });
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        sum[0] / n,
        sum[1] / n,
        sum[2] / n
    ))
}
