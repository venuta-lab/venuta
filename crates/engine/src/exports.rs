//! Playlists out, in every format people use: M3U8 (with the path style the place it goes
//! needs), M3U, PLS, XSPF, JSPF, WPL, CSV (the layout Soundiiz and TuneMyMusic read), a plain list
//! of songs, and iTunes XML. Everything Venuta writes it can read back whole: songs you don't have
//! yet and online songs ride along as `#VENUTA-WANT` / `#VENUTA-ONLINE` lines in M3U8 (other
//! players skip them) and as Venuta's extension in JSPF.

use crate::library::Library;
use crate::playlists::{self, OnlineEntry, WantEntry};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The formats Venuta writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    M3u8,
    /// Plain M3U with a byte-order mark, for players that want `.m3u`.
    M3u,
    Pls,
    Xspf,
    Jspf,
    Wpl,
    Csv,
    /// "Artist - Title", one per line.
    Txt,
    ItunesXml,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::M3u8 => "m3u8",
            Format::M3u => "m3u",
            Format::Pls => "pls",
            Format::Xspf => "xspf",
            Format::Jspf => "jspf",
            Format::Wpl => "wpl",
            Format::Csv => "csv",
            Format::Txt => "txt",
            Format::ItunesXml => "xml",
        }
    }

    /// Formats that point at your files (and so take a path style).
    pub fn has_paths(self) -> bool {
        matches!(
            self,
            Format::M3u8
                | Format::M3u
                | Format::Pls
                | Format::Xspf
                | Format::Wpl
                | Format::ItunesXml
        )
    }
}

/// How file paths are written, for where the playlist will be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PathStyle {
    /// Full paths on this PC (MusicBee, foobar2000, Windows Media Player, VLC here).
    ThisPc,
    /// Relative to the folder the playlist is saved in (moves with the music).
    NextToPlaylist,
    /// Relative to your Music folder, with forward slashes: the playlist goes in the Music folder
    /// of a phone or player (Poweramp, FiiO, Sony Walkman, Rockbox).
    InMusicFolder,
    /// Each path starts with `prefix` and goes on from your Music folder, with forward slashes:
    /// a server's view of the same music (Navidrome, Jellyfin, Plex) or a device's own root.
    StartsWith { prefix: String },
}

/// What to write and how.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub format: Format,
    pub style: PathStyle,
    /// Songs you don't have yet: kept (as Venuta lines in M3U8, entries without a file elsewhere)
    /// or left out.
    pub include_missing: bool,
}

/// One song as it goes out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    pub mbid: Option<String>,
    /// The file, when you have it.
    pub file: Option<PathBuf>,
    pub want: Option<WantEntry>,
    pub online: Option<OnlineEntry>,
}

/// A playlist's songs, from its file in the library, with what the library knows of each.
pub fn songs_of(music: &Path, lib: &Library, name: &str) -> Result<Vec<Song>, String> {
    let (entries, _) = playlists::read(music, lib, name)?;
    let by_path: std::collections::HashMap<&str, &crate::library::Track> =
        lib.tracks.iter().map(|t| (t.path.as_str(), t)).collect();
    Ok(entries
        .into_iter()
        .map(|e| {
            let track = (!e.missing)
                .then(|| by_path.get(e.path.as_str()).copied())
                .flatten();
            let tag = |k: &str| track.and_then(|t| t.first(k)).map(str::to_owned);
            Song {
                isrc: tag("ISRC").or_else(|| e.want.as_ref().and_then(|w| w.isrc.clone())),
                mbid: tag("MUSICBRAINZ_TRACKID"),
                file: (!e.missing && !e.path.is_empty()).then(|| music.join(&e.path)),
                duration_ms: Some(e.duration_ms).filter(|d| *d > 0),
                want: e
                    .missing
                    .then(|| e.want.clone())
                    .flatten()
                    .filter(|_| e.online.is_none()),
                online: e.online.clone().filter(|_| e.missing),
                title: e.title,
                artist: e.artist,
                album: e.album,
            }
        })
        .collect())
}

/// The playlist as text in `opts.format`. `music` is your Music folder; `saved_in` the folder
/// the file goes in (for relative paths).
pub fn render(name: &str, songs: &[Song], opts: &Options, music: &Path, saved_in: &Path) -> String {
    let kept: Vec<&Song> = songs
        .iter()
        .filter(|s| s.file.is_some() || (opts.include_missing && opts.format != Format::ItunesXml))
        .collect();
    let path = |s: &Song| {
        s.file
            .as_deref()
            .map(|f| styled(f, &opts.style, music, saved_in))
    };
    let label = |s: &Song| {
        if s.artist.is_empty() {
            s.title.clone()
        } else {
            format!("{} - {}", s.artist, s.title)
        }
    };
    let secs = |s: &Song| s.duration_ms.map(|d| (d + 500) / 1000);
    match opts.format {
        Format::M3u8 | Format::M3u => {
            let mut out = String::new();
            if opts.format == Format::M3u {
                out.push('\u{feff}');
            }
            out.push_str(&format!("#EXTM3U\n#PLAYLIST:{name}\n"));
            for s in &kept {
                match (path(s), &s.online, &s.want) {
                    (Some(p), _, _) => out.push_str(&format!(
                        "#EXTINF:{},{}\n{p}\n",
                        secs(s).map(|x| x as i64).unwrap_or(-1),
                        label(s)
                    )),
                    (None, Some(o), _) => {
                        out.push_str(&playlists::encode_online(o));
                        out.push('\n');
                    }
                    (None, None, w) => {
                        let w = w.clone().unwrap_or_else(|| want_of(s));
                        out.push_str(&format!(
                            "{}{}\n",
                            playlists::WANT_TAG,
                            serde_json::to_string(&w).unwrap_or_default()
                        ))
                    }
                }
            }
            out
        }
        Format::Pls => {
            let files: Vec<&&Song> = kept.iter().filter(|s| s.file.is_some()).collect();
            let mut out = String::from("[playlist]\n");
            for (i, s) in files.iter().enumerate() {
                let n = i + 1;
                out.push_str(&format!(
                    "File{n}={}\nTitle{n}={}\n",
                    path(s).unwrap_or_default(),
                    label(s)
                ));
                if let Some(d) = secs(s) {
                    out.push_str(&format!("Length{n}={d}\n"));
                }
            }
            out.push_str(&format!("NumberOfEntries={}\nVersion=2\n", files.len()));
            out
        }
        Format::Xspf => {
            let mut out = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n  <title>{}</title>\n  <trackList>\n",
                xml(name)
            );
            for s in &kept {
                out.push_str("    <track>\n");
                if let Some(f) = &s.file {
                    let loc = match &opts.style {
                        PathStyle::ThisPc => file_url(f),
                        _ => url_path(&styled(f, &opts.style, music, saved_in)),
                    };
                    out.push_str(&format!("      <location>{}</location>\n", xml(&loc)));
                }
                if let Some(m) = &s.mbid {
                    out.push_str(&format!(
                        "      <identifier>https://musicbrainz.org/recording/{m}</identifier>\n"
                    ));
                }
                out.push_str(&format!("      <title>{}</title>\n", xml(&s.title)));
                if !s.artist.is_empty() {
                    out.push_str(&format!("      <creator>{}</creator>\n", xml(&s.artist)));
                }
                if !s.album.is_empty() {
                    out.push_str(&format!("      <album>{}</album>\n", xml(&s.album)));
                }
                if let Some(d) = s.duration_ms {
                    out.push_str(&format!("      <duration>{d}</duration>\n"));
                }
                out.push_str("    </track>\n");
            }
            out.push_str("  </trackList>\n</playlist>\n");
            out
        }
        Format::Jspf => {
            let tracks: Vec<serde_json::Value> = kept
                .iter()
                .map(|s| {
                    let mut t = serde_json::json!({ "title": s.title });
                    if !s.artist.is_empty() {
                        t["creator"] = s.artist.clone().into();
                    }
                    if !s.album.is_empty() {
                        t["album"] = s.album.clone().into();
                    }
                    if let Some(d) = s.duration_ms {
                        t["duration"] = d.into();
                    }
                    if let Some(m) = &s.mbid {
                        t["identifier"] =
                            serde_json::json!([format!("https://musicbrainz.org/recording/{m}")]);
                    }
                    if let Some(f) = &s.file {
                        let loc = match &opts.style {
                            PathStyle::ThisPc => file_url(f),
                            _ => url_path(&styled(f, &opts.style, music, saved_in)),
                        };
                        t["location"] = serde_json::json!([loc]);
                    }
                    let mut ext = serde_json::Map::new();
                    if let Some(i) = &s.isrc {
                        ext.insert("isrc".into(), i.clone().into());
                    }
                    if let Some(o) = &s.online {
                        ext.insert("online".into(), serde_json::to_value(o).unwrap_or_default());
                    }
                    if s.file.is_none() && s.online.is_none() {
                        ext.insert("wanted".into(), true.into());
                    }
                    if !ext.is_empty() {
                        t["extension"] = serde_json::json!({ JSPF_EXTENSION: ext });
                    }
                    t
                })
                .collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "playlist": { "title": name, "creator": "Venuta", "track": tracks }
            }))
            .unwrap_or_default()
                + "\n"
        }
        Format::Wpl => {
            let mut out = format!(
                "<?wpl version=\"1.0\"?>\n<smil>\n  <head>\n    <meta name=\"Generator\" content=\"Venuta\"/>\n    <title>{}</title>\n  </head>\n  <body>\n    <seq>\n",
                xml(name)
            );
            for s in kept.iter().filter(|s| s.file.is_some()) {
                out.push_str(&format!(
                    "      <media src=\"{}\"/>\n",
                    xml(&path(s).unwrap_or_default())
                ));
            }
            out.push_str("    </seq>\n  </body>\n</smil>\n");
            out
        }
        Format::Csv => {
            let mut out = String::from(
                "\u{feff}title,artist,album,isrc,platform,trackId,duration,addedDate,addedBy,url,playlist\n",
            );
            for s in &kept {
                let (platform, id, url) = match &s.online {
                    Some(o) => (
                        "youtube",
                        o.track_id.clone(),
                        format!("https://music.youtube.com/watch?v={}", o.track_id),
                    ),
                    None => ("", String::new(), String::new()),
                };
                let row = [
                    s.title.clone(),
                    s.artist.clone(),
                    s.album.clone(),
                    s.isrc.clone().unwrap_or_default(),
                    platform.into(),
                    id,
                    secs(s).map(|d| format!("{d}s")).unwrap_or_default(),
                    String::new(),
                    String::new(),
                    url,
                    name.into(),
                ];
                out.push_str(
                    &row.iter()
                        .map(|c| csv_cell(c))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                out.push('\n');
            }
            out
        }
        Format::Txt => kept.iter().map(|s| label(s) + "\n").collect(),
        Format::ItunesXml => itunes_xml(name, &kept),
    }
}

/// Venuta's own JSPF extension: ISRC, an online song, a song still wanted.
pub const JSPF_EXTENSION: &str = "https://github.com/venuta-lab/venuta";

fn want_of(s: &Song) -> WantEntry {
    WantEntry {
        title: s.title.clone(),
        artist: s.artist.clone(),
        album: s.album.clone(),
        duration_ms: s.duration_ms.unwrap_or(0),
        isrc: s.isrc.clone(),
        ..Default::default()
    }
}

/// A file's path as `style` writes it.
pub fn styled(file: &Path, style: &PathStyle, music: &Path, saved_in: &Path) -> String {
    let forward = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let in_music = file.strip_prefix(music).ok().map(forward);
    match style {
        PathStyle::ThisPc => file.to_string_lossy().replace('/', "\\"),
        PathStyle::NextToPlaylist => relative(file, saved_in)
            .map(|r| r.to_string_lossy().replace('/', "\\"))
            .unwrap_or_else(|| file.to_string_lossy().replace('/', "\\")),
        PathStyle::InMusicFolder => in_music.unwrap_or_else(|| forward(file)),
        PathStyle::StartsWith { prefix } => {
            let rest = in_music.unwrap_or_else(|| forward(file));
            format!("{}/{}", prefix.trim_end_matches(['/', '\\']), rest)
        }
    }
}

/// `file` relative to `dir`, when both are on the same drive.
fn relative(file: &Path, dir: &Path) -> Option<PathBuf> {
    let f: Vec<_> = file.components().collect();
    let d: Vec<_> = dir.components().collect();
    if f.first() != d.first() {
        return None;
    }
    let same = f.iter().zip(&d).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in same..d.len() {
        out.push("..");
    }
    for c in &f[same..] {
        out.push(c);
    }
    Some(out)
}

fn file_url(f: &Path) -> String {
    let p = f.to_string_lossy().replace('\\', "/");
    let p = if p.starts_with('/') {
        p
    } else {
        format!("/{p}")
    };
    format!("file://{}", url_path(&p))
}

/// A path with each part percent-encoded, slashes kept.
fn url_path(p: &str) -> String {
    p.split('/')
        .map(|part| {
            if part.len() == 2 && part.ends_with(':') {
                part.to_owned()
            } else {
                urlencoding::encode(part).into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn itunes_xml(name: &str, songs: &[&Song]) -> String {
    let mut tracks = String::new();
    let mut items = String::new();
    for (i, s) in songs.iter().filter(|s| s.file.is_some()).enumerate() {
        let id = i + 1;
        tracks.push_str(&format!(
            "\t\t<key>{id}</key>\n\t\t<dict>\n\t\t\t<key>Track ID</key><integer>{id}</integer>\n\t\t\t<key>Name</key><string>{}</string>\n\t\t\t<key>Artist</key><string>{}</string>\n\t\t\t<key>Album</key><string>{}</string>\n{}\t\t\t<key>Location</key><string>{}</string>\n\t\t</dict>\n",
            xml(&s.title),
            xml(&s.artist),
            xml(&s.album),
            s.duration_ms.map(|d| format!("\t\t\t<key>Total Time</key><integer>{d}</integer>\n")).unwrap_or_default(),
            xml(&file_url(s.file.as_deref().unwrap_or(Path::new(""))).replacen("file://", "file://localhost", 1)),
        ));
        items.push_str(&format!(
            "\t\t\t\t<dict><key>Track ID</key><integer>{id}</integer></dict>\n"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Major Version</key><integer>1</integer>\n\t<key>Minor Version</key><integer>1</integer>\n\t<key>Application Version</key><string>Venuta</string>\n\t<key>Tracks</key>\n\t<dict>\n{tracks}\t</dict>\n\t<key>Playlists</key>\n\t<array>\n\t\t<dict>\n\t\t\t<key>Name</key><string>{}</string>\n\t\t\t<key>Playlist Items</key>\n\t\t\t<array>\n{items}\t\t\t</array>\n\t\t</dict>\n\t</array>\n</dict>\n</plist>\n",
        xml(name)
    )
}

/// Write `text` to `to` safely: a part file first, then put in place, so a playlist is never
/// left half written.
pub fn write(to: &Path, text: &str) -> Result<(), String> {
    let part = to.with_extension(format!(
        "{}.part",
        to.extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    std::fs::write(&part, text).map_err(|e| format!("{}: {e}", to.display()))?;
    std::fs::rename(&part, to).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {e}", to.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::read_bytes;
    use crate::playlists::Foreign;

    fn music() -> PathBuf {
        PathBuf::from(r"D:\Library\Music")
    }

    fn songs() -> Vec<Song> {
        vec![
            Song {
                title: "Kesariya".into(),
                artist: "Arijit Singh".into(),
                album: "Brahmastra".into(),
                duration_ms: Some(268_000),
                isrc: Some("INS172203702".into()),
                mbid: Some("393ad701-a13d-4f1c-9e3f-65dfd7cac167".into()),
                file: Some(music().join(r"Film\Brahmastra (2022)\04 - Kesariya.flac")),
                ..Default::default()
            },
            Song {
                title: "Rock & Roll, \"Live\"".into(),
                artist: "Led Zeppelin".into(),
                album: "BBC Sessions".into(),
                duration_ms: Some(220_000),
                want: Some(WantEntry {
                    title: "Rock & Roll, \"Live\"".into(),
                    artist: "Led Zeppelin".into(),
                    album: "BBC Sessions".into(),
                    duration_ms: 220_000,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Song {
                title: "Tum Hi Ho".into(),
                artist: "Arijit Singh".into(),
                duration_ms: Some(262_000),
                online: Some(OnlineEntry {
                    provider: "remote_catalog".into(),
                    track_id: "Umqb9KENgmk".into(),
                    title: "Tum Hi Ho".into(),
                    artists: vec!["Arijit Singh".into()],
                    album: None,
                    duration_ms: Some(262_000),
                    isrc: None,
                    artwork_url: None,
                }),
                ..Default::default()
            },
        ]
    }

    fn back(format: Format, include_missing: bool) -> Vec<Foreign> {
        let opts = Options {
            format,
            style: PathStyle::ThisPc,
            include_missing,
        };
        let text = render(
            "Road Trip",
            &songs(),
            &opts,
            &music(),
            Path::new(r"D:\Exports"),
        );
        let lists = read_bytes(text.as_bytes(), "Road Trip", Path::new(r"D:\Exports"))
            .unwrap_or_else(|e| panic!("{format:?}: {e}\n{text}"));
        assert_eq!(lists.len(), 1, "{format:?}");
        lists.into_iter().next().unwrap().tracks
    }

    #[test]
    fn every_format_reads_back() {
        for format in [
            Format::M3u8,
            Format::M3u,
            Format::Xspf,
            Format::Jspf,
            Format::Csv,
            Format::Txt,
        ] {
            let t = back(format, true);
            assert_eq!(t.len(), 3, "{format:?}");
            assert_eq!(t[0].title, "Kesariya", "{format:?}");
            assert_eq!(t[1].title, "Rock & Roll, \"Live\"", "{format:?}");
            assert_eq!(t[2].title, "Tum Hi Ho", "{format:?}");
        }
        // Formats that only hold files: the song you have.
        for format in [Format::Pls, Format::Wpl, Format::ItunesXml] {
            let t = back(format, true);
            assert_eq!(t.len(), 1, "{format:?}");
        }
    }

    #[test]
    fn what_rides_along_comes_back() {
        // M3U8: the wanted song and the online one, whole.
        let t = back(Format::M3u8, true);
        assert_eq!(t[1].album, "BBC Sessions");
        assert_eq!(t[2].youtube.as_deref(), Some("Umqb9KENgmk"));
        // JSPF: the recording code and MusicBrainz id too.
        let t = back(Format::Jspf, true);
        assert_eq!(t[0].isrc.as_deref(), Some("INS172203702"));
        assert_eq!(
            t[0].mbid.as_deref(),
            Some("393ad701-a13d-4f1c-9e3f-65dfd7cac167")
        );
        assert_eq!(t[2].youtube.as_deref(), Some("Umqb9KENgmk"));
        // CSV: the recording code and the length.
        let t = back(Format::Csv, true);
        assert_eq!(t[0].isrc.as_deref(), Some("INS172203702"));
        assert_eq!(t[0].duration_ms, Some(268_000));
        // Without the songs you don't have.
        assert_eq!(back(Format::M3u8, false).len(), 1);
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "Windows paths: a backslash is no separator elsewhere"
    )]
    fn paths_for_where_the_playlist_goes() {
        let f = music().join(r"Film\Brahmastra (2022)\04 - Kesariya.flac");
        let saved = PathBuf::from(r"D:\Library\Playlists");
        assert_eq!(
            styled(&f, &PathStyle::ThisPc, &music(), &saved),
            r"D:\Library\Music\Film\Brahmastra (2022)\04 - Kesariya.flac"
        );
        assert_eq!(
            styled(&f, &PathStyle::NextToPlaylist, &music(), &saved),
            r"..\Music\Film\Brahmastra (2022)\04 - Kesariya.flac"
        );
        assert_eq!(
            styled(&f, &PathStyle::InMusicFolder, &music(), &saved),
            "Film/Brahmastra (2022)/04 - Kesariya.flac"
        );
        assert_eq!(
            styled(
                &f,
                &PathStyle::StartsWith {
                    prefix: "/music/".into()
                },
                &music(),
                &saved
            ),
            "/music/Film/Brahmastra (2022)/04 - Kesariya.flac"
        );
    }

    #[test]
    fn writing_never_leaves_half_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("Road.m3u8");
        write(&to, "#EXTM3U\n").unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "#EXTM3U\n");
        assert!(!dir.path().join("Road.m3u8.part").exists());
    }
}

// --- Sending out to services, free ------------------------------------------------------------

/// Songs found on a service, and the ones that weren't.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OnService {
    /// (place in the playlist, the song there).
    pub found: Vec<(usize, crate::bridges::Elsewhere)>,
    /// Places of the songs the service didn't show.
    pub missing: Vec<usize>,
}

/// Find each song on `service` (free bridges, paced and cached). `on_progress(done, total)`.
pub fn on_service(
    http: &crate::net::Http,
    service: crate::bridges::Service,
    songs: &[Song],
    country: &str,
    on_progress: &mut dyn FnMut(usize, usize),
) -> OnService {
    let mut out = OnService::default();
    for (i, s) in songs.iter().enumerate() {
        let want = crate::playlists::Foreign {
            title: s.title.clone(),
            artist: s.artist.clone(),
            album: s.album.clone(),
            duration_ms: s.duration_ms,
            isrc: s.isrc.clone(),
            mbid: s.mbid.clone(),
            ..Default::default()
        };
        match crate::bridges::find(http, service, &want, country) {
            Ok(Some(e)) => out.found.push((i, e)),
            _ => out.missing.push(i),
        }
        on_progress(i + 1, songs.len());
    }
    out
}

/// "Copy for Spotify": one song link per line, to paste into a playlist in Spotify's app.
pub fn spotify_paste(found: &OnService) -> String {
    found
        .found
        .iter()
        .map(|(_, e)| format!("{}\n", e.url))
        .collect()
}

/// YouTube links that each open up to 50 songs as one list (YouTube's own limit), to save on
/// YouTube Music: `https://www.youtube.com/watch_videos?video_ids=…`.
pub fn youtube_lists(video_ids: &[String]) -> Vec<String> {
    video_ids
        .chunks(50)
        .map(|c| {
            format!(
                "https://www.youtube.com/watch_videos?video_ids={}",
                c.join(",")
            )
        })
        .collect()
}

/// A page that lists a playlist's songs with a link to each on a service, to add by hand where
/// the service takes nothing else (Apple Music, JioSaavn). Opens in your browser.
pub fn links_page(name: &str, service: &str, songs: &[Song], found: &OnService) -> String {
    let link_at: std::collections::HashMap<usize, &crate::bridges::Elsewhere> =
        found.found.iter().map(|(i, e)| (*i, e)).collect();
    let mut rows = String::new();
    for (i, s) in songs.iter().enumerate() {
        let who = if s.artist.is_empty() {
            String::new()
        } else {
            format!("<span>{}</span>", xml(&s.artist))
        };
        match link_at.get(&i) {
            Some(e) => rows.push_str(&format!(
                "<li><a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a>{who}</li>\n",
                xml(&e.url),
                xml(&s.title)
            )),
            None => rows.push_str(&format!(
                "<li class=\"none\">{}{who}<em>Not found on {}</em></li>\n",
                xml(&s.title),
                xml(service)
            )),
        }
    }
    let style = concat!(
        ":root{color-scheme:light dark;--ink:#1d1b20;--soft:#6b6770;--line:#e7e3ea;--bg:#fbfaf8}\n",
        "@media (prefers-color-scheme:dark){:root{--ink:#ece9ef;--soft:#a6a1ab;--line:#2c2a30;--bg:#141316}}\n",
        "body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.5 system-ui,sans-serif}\n",
        "main{max-width:640px;margin:0 auto;padding:32px 16px}h1{font-size:24px;margin:0 0 4px}\n",
        "p{color:var(--soft);margin:0 0 20px}ol{padding-left:28px}\n",
        "li{padding:8px 0;border-bottom:1px solid var(--line)}li span{display:block;color:var(--soft);font-size:13px}\n",
        "a{color:inherit;font-weight:600}li.none{color:var(--soft)}em{display:block;font-size:12px}\n"
    );
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{name} on {service}</title>\n<style>\n{style}</style></head>\n<body><main><h1>{name}</h1><p>{found} of {total} songs found on {service}. Open each and add it to your playlist there.</p>\n<ol>\n{rows}</ol></main></body></html>\n",
        name = xml(name),
        service = xml(service),
        found = found.found.len(),
        total = songs.len(),
    )
}

// --- Backup and restore ---------------------------------------------------------------------------

/// Everything about your playlists in one zip: each playlist as JSPF (every song, with what's
/// known: files, songs you don't have, online songs, recording codes), the To get list, and the
/// smart playlists' rules. Bringing the zip in (import) puts every playlist back. Returns how
/// many playlists it holds.
pub fn backup(music: &Path, lib: &Library, rules: &Path, to: &Path) -> Result<usize, String> {
    use std::io::Write;
    let part = to.with_extension("zip.part");
    let file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut z = zip::ZipWriter::new(file);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let err = |e: zip::result::ZipError| e.to_string();
    let opts = Options {
        format: Format::Jspf,
        style: PathStyle::ThisPc,
        include_missing: true,
    };
    let mut n = 0;
    for p in playlists::list(music, lib) {
        let songs = songs_of(music, lib, &p.name)?;
        let text = render(&p.name, &songs, &opts, music, music);
        z.start_file(
            format!("Playlists/{}.jspf", crate::plan::sanitize(&p.name, 100)),
            o,
        )
        .map_err(err)?;
        z.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        n += 1;
    }
    let wanted = playlists::wanted_now(music, lib);
    z.start_file("To get.csv", o).map_err(err)?;
    z.write_all(playlists::wanted_csv(&wanted).as_bytes())
        .map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(playlists::smart_dir(rules))
        .into_iter()
        .flatten()
        .flatten()
    {
        if e.path().extension().is_some_and(|x| x == "toml")
            && let Ok(body) = std::fs::read(e.path())
        {
            z.start_file(
                format!("Smart playlists/{}", e.file_name().to_string_lossy()),
                o,
            )
            .map_err(err)?;
            z.write_all(&body).map_err(|e| e.to_string())?;
        }
    }
    z.finish().map_err(err)?;
    std::fs::rename(&part, to).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {e}", to.display())
    })?;
    Ok(n)
}

#[cfg(test)]
mod out_tests {
    use super::*;

    #[test]
    fn youtube_takes_fifty_songs_a_link() {
        let ids: Vec<String> = (0..120).map(|i| format!("id{i:09}")).collect();
        let links = youtube_lists(&ids);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].matches(',').count(), 49);
        assert!(links[2].ends_with("id000000119"));
    }

    #[test]
    fn copy_for_spotify_and_a_page_of_links() {
        let found = OnService {
            found: vec![(
                0,
                crate::bridges::Elsewhere {
                    service: crate::bridges::Service::Spotify,
                    id: "3z8h0TU7ReDPLIbEnYhWZb".into(),
                    url: "https://open.spotify.com/track/3z8h0TU7ReDPLIbEnYhWZb".into(),
                    title: "Bohemian Rhapsody".into(),
                    artist: "Queen".into(),
                    album: String::new(),
                    duration_ms: None,
                    sure: true,
                    why: String::new(),
                },
            )],
            missing: vec![1],
        };
        assert_eq!(
            spotify_paste(&found),
            "https://open.spotify.com/track/3z8h0TU7ReDPLIbEnYhWZb\n"
        );
        let songs = [
            Song {
                title: "Bohemian Rhapsody".into(),
                artist: "Queen".into(),
                ..Default::default()
            },
            Song {
                title: "Kesariya".into(),
                ..Default::default()
            },
        ];
        let page = links_page("Road & Trip", "Apple Music", &songs, &found);
        assert!(page.contains("Road &amp; Trip"));
        assert!(page.contains("1 of 2 songs found on Apple Music"));
        assert!(page.contains("Not found on Apple Music"));
    }
}
