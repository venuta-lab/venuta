//! Player profiles: each card is written for the player that reads it.
//!
//! The library on the computer keeps the standard, complete tags (one ARTIST entry per
//! singer, ORIGINALDATE next to DATE, a large cover). Players read less, and differently:
//! Poweramp splits artists on ";" but not ",", most players show only DATE, the Echo Mini
//! needs a small baseline JPEG cover and reads no playlists. A card copy keeps the audio bytes
//! untouched and rewrites only the metadata for its player.

use crate::media::flac_layout::{self, BLOCK_PADDING, BLOCK_PICTURE, BLOCK_VORBIS_COMMENT};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Player {
    /// An exact copy of the library, for players not listed.
    #[default]
    Standard,
    Poweramp,
    SnowskyDisc,
    EchoMini,
    AstellKern,
}

impl Player {
    pub const ALL: [Player; 5] = [
        Player::Poweramp,
        Player::SnowskyDisc,
        Player::EchoMini,
        Player::AstellKern,
        Player::Standard,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Player::Standard => "Exact copy",
            Player::Poweramp => "Poweramp (Android)",
            Player::SnowskyDisc => "Snowsky Disc",
            Player::EchoMini => "Snowsky Echo Mini",
            Player::AstellKern => "Astell&Kern",
        }
    }

    pub fn recipe(self) -> Recipe {
        let base = Recipe {
            transform: true,
            join_artists: true,
            date_from_original: true,
            cover_max_px: 1000,
            playlists: true,
        };
        match self {
            Player::Standard => Recipe {
                transform: false,
                join_artists: false,
                date_from_original: false,
                cover_max_px: 0,
                playlists: true,
            },
            Player::Poweramp | Player::SnowskyDisc | Player::AstellKern => base,
            // Covers of 1000 px or more fail on the Echo Mini; it reads no playlist files.
            Player::EchoMini => Recipe {
                cover_max_px: 600,
                playlists: false,
                ..base
            },
        }
    }

    /// Changes whenever what a card copy looks like changes, so cards are brought up to date.
    pub fn stamp(self) -> String {
        let r = self.recipe();
        format!(
            "v1:{}:{}:{}:{}",
            u8::from(r.join_artists),
            u8::from(r.date_from_original),
            r.cover_max_px,
            u8::from(r.playlists)
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    /// False: copy files exactly.
    pub transform: bool,
    /// One ARTIST (and COMPOSER, LYRICIST, GENRE) value, names joined with "; ".
    pub join_artists: bool,
    /// DATE set to the original year, because players read only DATE.
    pub date_from_original: bool,
    /// Largest embedded and folder cover, in pixels (0 = as it is).
    pub cover_max_px: u32,
    /// Copy playlist files to the card.
    pub playlists: bool,
}

/// Keys whose several values become one, joined with "; ".
const JOINED: &[&str] = &["ARTIST", "COMPOSER", "LYRICIST", "GENRE", "ALBUMARTIST"];

/// The card's tags from the library's, for this recipe. Keys are kept as written.
pub fn card_tags(tags: &[(String, String)], r: &Recipe) -> Vec<(String, String)> {
    if !r.transform {
        return tags.to_vec();
    }
    let upper = |k: &str| k.to_ascii_uppercase();
    let original = tags
        .iter()
        .find(|(k, _)| upper(k) == "ORIGINALYEAR" || upper(k) == "ORIGINALDATE")
        .map(|(_, v)| v.chars().take(4).collect::<String>());
    let has_lyrics = tags.iter().any(|(k, _)| upper(k) == "LYRICS");
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in tags {
        let key = upper(k);
        if key == "UNSYNCEDLYRICS" && has_lyrics {
            continue; // Poweramp reads LYRICS first anyway; one copy avoids mixed results
        }
        if r.join_artists && JOINED.contains(&key.as_str()) {
            match out.iter_mut().find(|(ok, _)| upper(ok) == key) {
                Some((_, existing)) => {
                    existing.push_str("; ");
                    existing.push_str(v);
                }
                None => out.push((k.clone(), v.clone())),
            }
            continue;
        }
        if key == "DATE"
            && r.date_from_original
            && let Some(y) = &original
        {
            out.push(("RELEASEDATE".into(), v.clone()));
            out.push((k.clone(), y.clone()));
            continue;
        }
        if key == "LYRICS" {
            out.push((k.clone(), line_timed(v)));
            continue;
        }
        out.push((k.clone(), v.clone()));
    }
    if r.date_from_original
        && let Some(y) = &original
        && !out.iter().any(|(k, _)| upper(k) == "DATE")
    {
        out.push(("DATE".into(), y.clone()));
    }
    out
}

/// Word-level timestamps ("<00:12.30>") removed; players show lyrics a line at a time.
fn line_timed(lrc: &str) -> String {
    let mut out = String::with_capacity(lrc.len());
    let mut chars = lrc.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let rest: String = chars.clone().take(10).collect();
            if rest.len() >= 6 && rest.as_bytes().get(2) == Some(&b':') && rest.contains('>') {
                for c2 in chars.by_ref() {
                    if c2 == '>' {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The card copy's metadata (everything before the audio) for a library FLAC file, and where
/// the audio starts in the source.
pub fn card_metadata(src: &Path, r: &Recipe) -> Result<(Vec<u8>, u64), String> {
    let layout = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
    let mut f = File::open(src).map_err(|e| e.to_string())?;
    let mut body = |b: &flac_layout::BlockInfo| -> Result<Vec<u8>, String> {
        let mut raw = vec![0u8; b.len as usize];
        f.seek(SeekFrom::Start(b.offset + 4))
            .map_err(|e| e.to_string())?;
        f.read_exact(&mut raw).map_err(|e| e.to_string())?;
        Ok(raw)
    };
    let mut kept: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut comment: Option<Vec<u8>> = None;
    let mut picture: Option<Vec<u8>> = None;
    for b in &layout.blocks {
        match b.kind {
            BLOCK_PADDING => {}
            BLOCK_VORBIS_COMMENT => comment = Some(body(b)?),
            // Exactly one picture: Poweramp shows the last one, the Echo Mini is fragile.
            BLOCK_PICTURE => {
                let p = body(b)?;
                if picture.is_none() || picture_type(&p) == Some(3) {
                    picture = Some(p);
                }
            }
            kind => kept.push((kind, body(b)?)),
        }
    }
    let mut blocks = kept;
    if let Some(c) = comment {
        let (vendor, tags) = parse_comment(&c)?;
        blocks.push((
            BLOCK_VORBIS_COMMENT,
            build_comment(&vendor, &card_tags(&tags, r)),
        ));
    }
    if let Some(p) = picture {
        let p = if r.cover_max_px > 0 {
            resize_picture(&p, r.cover_max_px).unwrap_or(p)
        } else {
            p
        };
        blocks.push((BLOCK_PICTURE, p));
    }
    let mut meta = b"fLaC".to_vec();
    for (kind, data) in &blocks {
        let len = (data.len() as u32).to_be_bytes();
        meta.extend([*kind & 0x7F, len[1], len[2], len[3]]);
        meta.extend(data);
    }
    // Room for later tag changes, so a card can be patched in place.
    let pad = 8192u32.to_be_bytes();
    meta.extend([0x80 | BLOCK_PADDING, pad[1], pad[2], pad[3]]);
    meta.extend(std::iter::repeat_n(0u8, 8192));
    Ok((meta, layout.audio_offset))
}

fn picture_type(p: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(p.get(..4)?.try_into().ok()?))
}

/// A PICTURE block body with its image re-saved as a baseline JPEG of at most `max_px`.
/// Returns None when it is already small enough and a JPEG, or can't be read.
fn resize_picture(p: &[u8], max_px: u32) -> Option<Vec<u8>> {
    let mut i = 4usize;
    let rd = |i: &mut usize| -> Option<u32> {
        let v = u32::from_be_bytes(p.get(*i..*i + 4)?.try_into().ok()?);
        *i += 4;
        Some(v)
    };
    let kind = picture_type(p)?;
    let mime_len = rd(&mut i)? as usize;
    let mime = std::str::from_utf8(p.get(i..i + mime_len)?)
        .ok()?
        .to_owned();
    i += mime_len;
    let desc_len = rd(&mut i)? as usize;
    let desc = p.get(i..i + desc_len)?.to_vec();
    i += desc_len;
    let (w, h) = (rd(&mut i)?, rd(&mut i)?);
    i += 8; // depth, colours
    let data_len = rd(&mut i)? as usize;
    let data = p.get(i..i + data_len)?;
    let info = crate::media::picture::inspect(data);
    let progressive = info.as_ref().is_some_and(|x| x.progressive);
    if w.max(h) <= max_px && mime == "image/jpeg" && !progressive {
        return None;
    }
    let jpeg = crate::apply::baseline_jpeg(data, max_px).ok()?;
    let img = image::load_from_memory(&jpeg).ok()?;
    let mut out = Vec::new();
    out.extend(kind.to_be_bytes());
    out.extend((10u32).to_be_bytes());
    out.extend(b"image/jpeg");
    out.extend((desc.len() as u32).to_be_bytes());
    out.extend(&desc);
    out.extend(img.width().to_be_bytes());
    out.extend(img.height().to_be_bytes());
    out.extend(24u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend((jpeg.len() as u32).to_be_bytes());
    out.extend(&jpeg);
    Some(out)
}

/// A Vorbis comment block: the vendor string and the tags in order.
type Comment = (Vec<u8>, Vec<(String, String)>);

fn parse_comment(c: &[u8]) -> Result<Comment, String> {
    let mut i = 0usize;
    let rd = |i: &mut usize| -> Result<u32, String> {
        let v = u32::from_le_bytes(
            c.get(*i..*i + 4)
                .ok_or("short comment")?
                .try_into()
                .map_err(|_| "short")?,
        );
        *i += 4;
        Ok(v)
    };
    let vlen = rd(&mut i)? as usize;
    let vendor = c.get(i..i + vlen).ok_or("short vendor")?.to_vec();
    i += vlen;
    let n = rd(&mut i)?;
    let mut tags = Vec::new();
    for _ in 0..n {
        let len = rd(&mut i)? as usize;
        let raw = c.get(i..i + len).ok_or("short field")?;
        i += len;
        let s = String::from_utf8_lossy(raw);
        if let Some((k, v)) = s.split_once('=') {
            tags.push((k.to_owned(), v.to_owned()));
        }
    }
    Ok((vendor, tags))
}

fn build_comment(vendor: &[u8], tags: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((vendor.len() as u32).to_le_bytes());
    out.extend(vendor);
    out.extend((tags.len() as u32).to_le_bytes());
    for (k, v) in tags {
        let field = format!("{k}={v}");
        out.extend((field.len() as u32).to_le_bytes());
        out.extend(field.as_bytes());
    }
    out
}

/// A folder cover for the card: re-saved at the player's size when it is larger (or
/// progressive); otherwise None (copy the file as it is).
pub fn card_image(bytes: &[u8], r: &Recipe) -> Option<Vec<u8>> {
    if !r.transform || r.cover_max_px == 0 {
        return None;
    }
    let info = crate::media::picture::inspect(bytes)?;
    let big = info.width.max(info.height) > r.cover_max_px;
    // The Snowsky Disc refuses cover files over 1.35 MB.
    if !big && !info.progressive && bytes.len() <= 1_350_000 {
        return None;
    }
    crate::apply::baseline_jpeg(bytes, r.cover_max_px).ok()
}

// --- The player test --------------------------------------------------------------------

/// The folder the player test is written to, inside the card's music folder.
pub const TEST_FOLDER: &str = "~ Player test";

/// One part of the test, and the question it answers.
#[derive(Debug, Clone, Serialize)]
pub struct TestItem {
    pub id: String,
    pub file: String,
    pub checks: String,
}

/// A PICTURE block (front cover) for JPEG bytes.
fn picture_block(jpeg: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(jpeg).ok()?;
    let mut out = Vec::new();
    out.extend(3u32.to_be_bytes());
    out.extend(10u32.to_be_bytes());
    out.extend(b"image/jpeg");
    out.extend(0u32.to_be_bytes());
    out.extend(img.width().to_be_bytes());
    out.extend(img.height().to_be_bytes());
    out.extend(24u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend((jpeg.len() as u32).to_be_bytes());
    out.extend(jpeg);
    Some(out)
}

/// Write `src`'s audio with exactly these tags and picture to `dst`.
fn write_variant(
    src: &Path,
    dst: &Path,
    tags: &[(String, String)],
    picture: Option<&[u8]>,
) -> Result<(), String> {
    let layout = flac_layout::read_layout(src).map_err(|e| e.to_string())?;
    let mut f = File::open(src).map_err(|e| e.to_string())?;
    let mut meta = b"fLaC".to_vec();
    let push = |meta: &mut Vec<u8>, kind: u8, data: &[u8]| {
        let len = (data.len() as u32).to_be_bytes();
        meta.extend([kind & 0x7F, len[1], len[2], len[3]]);
        meta.extend(data);
    };
    let mut vendor = b"Venuta player test".to_vec();
    for b in &layout.blocks {
        let mut raw = vec![0u8; b.len as usize];
        f.seek(SeekFrom::Start(b.offset + 4))
            .map_err(|e| e.to_string())?;
        f.read_exact(&mut raw).map_err(|e| e.to_string())?;
        match b.kind {
            BLOCK_PADDING | BLOCK_PICTURE => {}
            BLOCK_VORBIS_COMMENT => {
                if let Ok((v, _)) = parse_comment(&raw) {
                    vendor = v;
                }
            }
            kind => push(&mut meta, kind, &raw),
        }
    }
    push(
        &mut meta,
        BLOCK_VORBIS_COMMENT,
        &build_comment(&vendor, tags),
    );
    if let Some(p) = picture {
        push(&mut meta, BLOCK_PICTURE, p);
    }
    meta.extend([0x80 | BLOCK_PADDING, 0, 16, 0]);
    meta.extend(std::iter::repeat_n(0u8, 4096));
    let mut audio = Vec::new();
    f.seek(SeekFrom::Start(layout.audio_offset))
        .map_err(|e| e.to_string())?;
    f.read_to_end(&mut audio).map_err(|e| e.to_string())?;
    meta.extend(audio);
    std::fs::write(dst, meta).map_err(|e| e.to_string())
}

fn has_devanagari(s: &str) -> bool {
    s.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

/// Write the player test to a card: one of your songs in nine variants, a cover file and two
/// playlists, under `<music folder>/~ Player test`. Returns the questions to answer.
pub fn write_test_kit(
    library_root: &Path,
    card_root: &Path,
    music_folder: &str,
) -> Result<Vec<TestItem>, String> {
    // A song with synced lyrics beside it and a cover file in its folder, Hindi if possible.
    let mut pick: Option<(std::path::PathBuf, String)> = None;
    for e in walkdir::WalkDir::new(library_root).into_iter().flatten() {
        let p = e.path();
        if p.extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("flac"))
            && let Some(dir) = p.parent()
            && dir.join("cover.jpg").exists()
            && let Ok(lrc) = crate::text::read_text(&p.with_extension("lrc"))
            && lrc.contains("[00:")
        {
            let hindi = has_devanagari(&lrc);
            if pick
                .as_ref()
                .is_none_or(|(_, l)| hindi && !has_devanagari(l))
            {
                pick = Some((p.to_path_buf(), lrc));
            }
            if hindi {
                break;
            }
        }
    }
    let (src, lrc) =
        pick.ok_or("No song with a lyrics file and a cover was found in your library.")?;
    let folder = src.parent().ok_or("no folder")?;
    let cover = std::fs::read(folder.join("cover.jpg")).map_err(|e| e.to_string())?;
    let jpeg = |px: u32| {
        crate::apply::baseline_jpeg(&cover, px)
            .ok()
            .and_then(|j| picture_block(&j))
    };
    let (small, large) = (jpeg(600), jpeg(1400));
    let dir = card_root.join(music_folder).join(TEST_FOLDER);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let clean = crate::apply::clean_lrc(&lrc);
    let tags = |n: u32, title: &str, extra: &[(&str, &str)]| -> Vec<(String, String)> {
        let mut t: Vec<(String, String)> = vec![
            ("TITLE".into(), title.into()),
            ("ALBUM".into(), "Player test".into()),
            ("ALBUMARTIST".into(), "Venuta".into()),
            ("TRACKNUMBER".into(), n.to_string()),
            ("DATE".into(), "2004".into()),
            ("ORIGINALDATE".into(), "1999".into()),
            ("ORIGINALYEAR".into(), "1999".into()),
        ];
        t.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        t
    };
    struct Variant {
        id: &'static str,
        file: &'static str,
        checks: &'static str,
        tags: Vec<(String, String)>,
        picture: Option<Vec<u8>>,
        lrc: Option<String>,
    }
    let one = [("ARTIST", "Singer One")];
    let bom = format!("\u{feff}{clean}");
    let variants = vec![
        Variant {
            id: "artists_semicolon",
            file: "01 - Artists with semicolons",
            checks: "Two singers in one ARTIST value, separated by \";\". Are they listed as two separate artists?",
            tags: tags(
                1,
                "Artists with semicolons",
                &[("ARTIST", "Singer One; Singer Two")],
            ),
            picture: small.clone(),
            lrc: None,
        },
        Variant {
            id: "artists_entries",
            file: "02 - Artists as two entries",
            checks: "Two ARTIST entries. Are both singers shown?",
            tags: tags(
                2,
                "Artists as two entries",
                &[("ARTIST", "Singer One"), ("ARTIST", "Singer Two")],
            ),
            picture: small.clone(),
            lrc: None,
        },
        Variant {
            id: "lyrics_tag",
            file: "03 - Lyrics inside the file",
            checks: "Synced lyrics in the LYRICS tag only. Do lyrics show and follow the song?",
            tags: tags(
                3,
                "Lyrics inside the file",
                &[("ARTIST", "Singer One"), ("LYRICS", clean.trim_end())],
            ),
            picture: small.clone(),
            lrc: None,
        },
        Variant {
            id: "lyrics_file",
            file: "04 - Lyrics file only",
            checks: "A .lrc file beside the song (UTF-8), nothing inside. Do lyrics show?",
            tags: tags(4, "Lyrics file only", &one),
            picture: small.clone(),
            lrc: Some(clean.clone()),
        },
        Variant {
            id: "lyrics_bom",
            file: "05 - Lyrics file with BOM",
            checks: "The same .lrc with a byte-order mark. Do lyrics show, and are they readable?",
            tags: tags(5, "Lyrics file with BOM", &one),
            picture: small.clone(),
            lrc: Some(bom),
        },
        Variant {
            id: "cover_600",
            file: "06 - Cover 600 px",
            checks: "A 600 px cover inside the file. Is it shown?",
            tags: tags(6, "Cover 600 px", &one),
            picture: small.clone(),
            lrc: None,
        },
        Variant {
            id: "cover_1400",
            file: "07 - Cover 1400 px",
            checks: "A 1400 px cover inside the file. Is it shown?",
            tags: tags(7, "Cover 1400 px", &one),
            picture: large,
            lrc: None,
        },
        Variant {
            id: "hindi",
            file: "08 - Hindi title",
            checks: "A title and artist in Devanagari (तेरे लिए, लता मंगेशकर). Readable, not boxes?",
            tags: tags(8, "तेरे लिए", &[("ARTIST", "लता मंगेशकर")]),
            picture: small.clone(),
            lrc: None,
        },
        Variant {
            id: "cover_file",
            file: "09 - Cover file only",
            checks: "No cover inside; cover.jpg in the folder. Is a cover shown?",
            tags: tags(9, "Cover file only", &one),
            picture: None,
            lrc: None,
        },
    ];
    let mut out = Vec::new();
    for v in &variants {
        let dst = dir.join(format!("{}.flac", v.file));
        write_variant(&src, &dst, &v.tags, v.picture.as_deref())?;
        if let Some(text) = &v.lrc {
            std::fs::write(dst.with_extension("lrc"), text).map_err(|e| e.to_string())?;
        }
        out.push(TestItem {
            id: v.id.into(),
            file: format!("{}.flac", v.file),
            checks: v.checks.into(),
        });
    }
    if let Ok(j) = crate::apply::baseline_jpeg(&cover, 1000) {
        std::fs::write(dir.join("cover.jpg"), j).map_err(|e| e.to_string())?;
    }
    // Two playlist path styles: relative to the playlist, and from the card's top.
    let names: Vec<String> = variants
        .iter()
        .map(|v| format!("{}.flac", v.file))
        .collect();
    let rel: String = names
        .iter()
        .map(|n| format!("../{music_folder}/{TEST_FOLDER}/{n}\n"))
        .collect();
    let top: String = names
        .iter()
        .map(|n| format!("{music_folder}/{TEST_FOLDER}/{n}\n"))
        .collect();
    let pl = card_root.join("Playlists");
    std::fs::create_dir_all(&pl).map_err(|e| e.to_string())?;
    std::fs::write(pl.join("~ Player test.m3u8"), format!("#EXTM3U\n{rel}"))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        card_root.join("~ Player test (from the top).m3u8"),
        format!("#EXTM3U\n{top}"),
    )
    .map_err(|e| e.to_string())?;
    out.push(TestItem {
        id: "playlist_relative".into(),
        file: "Playlists/~ Player test.m3u8".into(),
        checks: "A playlist with paths relative to it. Does it list nine songs that play?".into(),
    });
    out.push(TestItem {
        id: "playlist_top".into(),
        file: "~ Player test (from the top).m3u8".into(),
        checks: "A playlist at the top of the card, paths from the top. Does it list nine songs?"
            .into(),
    });
    out.push(TestItem {
        id: "year_original".into(),
        file: "(any song)".into(),
        checks: "Is the year shown 1999 (the original year) rather than 2004 (the date)?".into(),
    });
    Ok(out)
}

/// Remove the player test from a card.
pub fn remove_test_kit(card_root: &Path, music_folder: &str) -> Result<(), String> {
    let dir = card_root.join(music_folder).join(TEST_FOLDER);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(card_root.join("Playlists").join("~ Player test.m3u8"));
    let _ = std::fs::remove_file(card_root.join("~ Player test (from the top).m3u8"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn poweramp_recipe_joins_names_and_uses_the_original_year() {
        let r = Player::Poweramp.recipe();
        let out = card_tags(
            &tags(&[
                ("TITLE", "Tere Liye"),
                ("ARTIST", "Lata Mangeshkar"),
                ("ARTIST", "Roop Kumar Rathod"),
                ("DATE", "2014-02-11"),
                ("ORIGINALYEAR", "2004"),
                ("LYRICS", "[00:01.00]<00:01.00>Tere <00:01.50>Liye"),
                ("UNSYNCEDLYRICS", "Tere Liye"),
            ]),
            &r,
        );
        assert!(out.contains(&("ARTIST".into(), "Lata Mangeshkar; Roop Kumar Rathod".into())));
        assert!(out.contains(&("DATE".into(), "2004".into())));
        assert!(out.contains(&("RELEASEDATE".into(), "2014-02-11".into())));
        assert!(out.contains(&("LYRICS".into(), "[00:01.00]Tere Liye".into())));
        assert!(!out.iter().any(|(k, _)| k == "UNSYNCEDLYRICS"));
        assert_eq!(out.iter().filter(|(k, _)| k == "ARTIST").count(), 1);
    }

    #[test]
    fn exact_copy_changes_nothing() {
        let t = tags(&[("ARTIST", "A"), ("ARTIST", "B"), ("DATE", "2014")]);
        assert_eq!(card_tags(&t, &Player::Standard.recipe()), t);
    }

    #[test]
    fn echo_mini_has_small_covers_and_no_playlists() {
        let r = Player::EchoMini.recipe();
        assert_eq!(r.cover_max_px, 600);
        assert!(!r.playlists);
        assert_ne!(Player::EchoMini.stamp(), Player::Poweramp.stamp());
    }
}
