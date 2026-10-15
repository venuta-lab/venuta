//! Playlist files read by what's in them, not by their names: a `.txt` that is a CSV, a `.json`
//! that is a ListenBrainz list, a zip from a data download. The readers for the formats added
//! with "Playlists in and out" live here (XSPF, JSPF, WPL/ZPL, ASX, a pasted list of songs, Excel
//! sheets); the older ones (M3U, PLS, CSV, iTunes, Spotify's data) stay in `imports`.

use crate::playlists::Foreign;
use regex::Regex;
use std::io::Read;
use std::sync::LazyLock;

/// What a file turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    M3u,
    Pls,
    Xspf,
    Jspf,
    Wpl,
    Asx,
    ItunesXml,
    ItunesTxt,
    Csv,
    SpotifyJson,
    /// A zip: an Excel sheet, or a data download holding playlist files.
    Zip,
    /// A list of songs, one per line, in any of the usual ways.
    Songs,
    /// XML or JSON that isn't a playlist.
    Unknown,
}

/// Look at the start of a file and say what it is.
pub fn detect(bytes: &[u8], text: &str) -> Kind {
    if bytes.starts_with(b"PK\x03\x04") {
        return Kind::Zip;
    }
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let head: String = t.chars().take(4000).collect::<String>().to_lowercase();
    if head.starts_with('<') {
        return if head.contains("<playlist") && head.contains("xspf") {
            Kind::Xspf
        } else if head.contains("<smil") {
            Kind::Wpl
        } else if head.contains("<asx") {
            Kind::Asx
        } else if head.contains("<plist") || head.contains("<!doctype plist") {
            Kind::ItunesXml
        } else {
            Kind::Unknown
        };
    }
    if head.starts_with("[playlist]") {
        return Kind::Pls;
    }
    if head.starts_with('{') || head.starts_with('[') {
        return match serde_json::from_str::<serde_json::Value>(t) {
            Ok(v) if v.get("playlist").and_then(|p| p.get("track")).is_some() => Kind::Jspf,
            Ok(_) => Kind::SpotifyJson,
            Err(_) => Kind::Unknown,
        };
    }
    if head.starts_with("#extm3u") || head.contains("\n#extinf") || head.starts_with("#extinf") {
        return Kind::M3u;
    }
    let first = head.lines().next().unwrap_or_default();
    if first.contains('\t') && first.contains("name") && first.contains("artist") {
        return Kind::ItunesTxt;
    }
    if first.contains(',') || head.contains("video id") {
        let has = |names: &[&str]| {
            first
                .split(',')
                .any(|c| names.contains(&c.trim().trim_matches('"')))
        };
        if (has(&["track name", "title", "track", "name", "song", "song title"])
            && has(&[
                "artist",
                "artist name",
                "artist name(s)",
                "creator",
                "artists",
            ]))
            || head.contains("video id")
        {
            return Kind::Csv;
        }
    }
    // Lines that are paths to audio files: a bare M3U.
    let lines: Vec<&str> = head
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !lines.is_empty() && lines.iter().filter(|l| is_audio_path(l)).count() * 2 > lines.len() {
        return Kind::M3u;
    }
    Kind::Songs
}

fn is_audio_path(l: &str) -> bool {
    static AUDIO: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\.(flac|mp3|m4a|aac|ogg|oga|opus|wav|aiff?|wma|alac|ape|wv|dsf|dff)$")
            .expect("valid")
    });
    (l.contains('/') || l.contains('\\')) && AUDIO.is_match(l)
}

// --- A little XML --------------------------------------------------------------------------

/// The blocks `<tag …>…</tag>` in `text`, in order (no nesting of the same tag).
fn blocks<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
    let re = Regex::new(&format!(r"(?is)<{tag}(?:\s[^>]*)?>(.*?)</{tag}\s*>")).expect("valid");
    re.captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect()
}

/// The text of the first `<tag>` in `text`, unescaped.
fn field(text: &str, tag: &str) -> Option<String> {
    blocks(text, tag)
        .first()
        .map(|s| unescape(s.trim()))
        .filter(|s| !s.is_empty())
}

/// Every value of `attr` on `<tag … attr="…">` elements, in order.
fn attrs(text: &str, tag: &str, attr: &str) -> Vec<String> {
    let re = Regex::new(&format!(
        r#"(?is)<{tag}\b[^>]*?\b{attr}\s*=\s*(?:"([^"]*)"|'([^']*)')"#
    ))
    .expect("valid");
    re.captures_iter(text)
        .filter_map(|c| c.get(1).or(c.get(2)).map(|m| unescape(m.as_str())))
        .collect()
}

fn unescape(s: &str) -> String {
    static ENT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|amp|lt|gt|quot|apos);").expect("valid")
    });
    let s = s
        .strip_prefix("<![CDATA[")
        .and_then(|x| x.strip_suffix("]]>"))
        .unwrap_or(s);
    ENT.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        match e {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            _ => {
                let n = e
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| e.strip_prefix('#').and_then(|d| d.parse().ok()));
                n.and_then(char::from_u32)
                    .map(String::from)
                    .unwrap_or_default()
            }
        }
    })
    .into_owned()
}

/// A `file://` address as a path on this computer; anything else as written.
pub fn file_location(loc: &str) -> String {
    let Some(rest) = loc.strip_prefix("file://") else {
        return loc.to_owned();
    };
    let decoded = urlencoding::decode(rest)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| rest.to_owned());
    // file:///D:/x → D:/x ; file:///home/x → /home/x ; file://server/share → //server/share
    match decoded.strip_prefix('/') {
        Some(p) if p.len() > 1 && p.as_bytes()[1] == b':' => p.to_owned(),
        Some(p) => format!("/{p}"),
        None => format!("//{decoded}"),
    }
}

/// A MusicBrainz recording id inside an identifier ("https://musicbrainz.org/recording/<id>").
fn recording_id(s: &str) -> Option<String> {
    static MBID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)musicbrainz\.org/recording/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})")
            .expect("valid")
    });
    MBID.captures(s).map(|c| c[1].to_lowercase())
}

// --- XSPF and JSPF ---------------------------------------------------------------------------

/// One song of an XSPF or JSPF list: what the list says, or the file it points at.
pub struct Listed {
    pub location: Option<String>,
    pub song: Foreign,
}

/// XSPF (VLC, Soundiiz, ListenBrainz, Strawberry…): name and songs.
pub fn xspf(text: &str) -> (Option<String>, Vec<Listed>) {
    let list = blocks(text, "trackList").first().copied().unwrap_or(text);
    let head = text.split("<trackList").next().unwrap_or_default();
    let name = field(head, "title");
    let songs = blocks(list, "track")
        .into_iter()
        .map(|t| Listed {
            location: field(t, "location").map(|l| file_location(&l)),
            song: Foreign {
                title: field(t, "title").unwrap_or_default(),
                artist: field(t, "creator").unwrap_or_default(),
                album: field(t, "album").unwrap_or_default(),
                duration_ms: field(t, "duration")
                    .and_then(|d| d.parse().ok())
                    .filter(|d| *d > 0),
                mbid: blocks(t, "identifier").iter().find_map(|i| recording_id(i)),
                ..Default::default()
            },
        })
        .collect();
    (name, songs)
}

/// JSPF (ListenBrainz's lists): name and songs.
pub fn jspf(text: &str) -> (Option<String>, Vec<Listed>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
    else {
        return (None, vec![]);
    };
    let p = &v["playlist"];
    let s = |x: &serde_json::Value| x.as_str().unwrap_or_default().trim().to_owned();
    let songs = p["track"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let venuta = &t["extension"][crate::exports::JSPF_EXTENSION];
            let ids: Vec<String> = match &t["identifier"] {
                serde_json::Value::String(one) => vec![one.clone()],
                serde_json::Value::Array(many) => many.iter().map(s).collect(),
                _ => vec![],
            };
            Listed {
                location: t["location"]
                    .as_array()
                    .and_then(|l| l.first())
                    .or(Some(&t["location"]))
                    .and_then(|l| l.as_str())
                    .map(file_location),
                song: Foreign {
                    title: s(&t["title"]),
                    artist: s(&t["creator"]),
                    album: s(&t["album"]),
                    duration_ms: t["duration"].as_u64().filter(|d| *d > 0),
                    mbid: ids.iter().find_map(|i| recording_id(i)),
                    // Venuta's own extension: the recording code, an online song.
                    isrc: venuta["isrc"].as_str().map(str::to_owned),
                    youtube: venuta["online"]["track_id"]
                        .as_str()
                        .filter(|_| venuta["online"]["provider"] == "remote_catalog")
                        .map(str::to_owned),
                    ..Default::default()
                },
            }
        })
        .collect();
    let name = Some(s(&p["title"])).filter(|n| !n.is_empty());
    (name, songs)
}

// --- WPL / ZPL and ASX -----------------------------------------------------------------------

/// Windows Media Player (and Zune) lists: name and the files, in order.
pub fn wpl(text: &str) -> (Option<String>, Vec<String>) {
    let head = blocks(text, "head").first().copied().unwrap_or_default();
    (field(head, "title"), attrs(text, "media", "src"))
}

/// One entry of an ASX list: its title, author and file, as far as the list says.
pub type AsxEntry = (Option<String>, Option<String>, Option<String>);

/// ASX lists: name, and its entries.
pub fn asx(text: &str) -> (Option<String>, Vec<AsxEntry>) {
    let head = text.split("<entry").next().unwrap_or_default();
    let head = head.split("<ENTRY").next().unwrap_or(head);
    let entries = blocks(text, "entry")
        .into_iter()
        .map(|e| {
            (
                field(e, "title"),
                field(e, "author"),
                attrs(e, "ref", "href").into_iter().next(),
            )
        })
        .collect();
    (field(head, "title"), entries)
}

// --- A list of songs ---------------------------------------------------------------------------

/// A list of songs as people write or copy them, one per line: "Artist - Title", "Title by
/// Artist", numbered or not, or columns copied from a spreadsheet or a web page (title, artist,
/// album, length). Links and blank lines are left for others; a header row is skipped.
pub fn songs(text: &str) -> Vec<Foreign> {
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
        // "1. ", "2) ", "03 ", "4 - ": a list's own numbering, not "99 Luftballons".
        Regex::new(r"^\s*(?:\d{1,4}\s*[.):]\s*|0\d{1,3}\s+|\d{1,4}\s+-\s+)").expect("valid")
    });
    static BY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+by\s+").expect("valid"));
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with("http://") || line.starts_with("https://") {
            continue;
        }
        if line.contains('\t') {
            let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
            let lower: Vec<String> = cols.iter().map(|c| c.to_lowercase()).collect();
            if lower
                .iter()
                .any(|c| c == "title" || c == "name" || c == "song")
                && lower.iter().any(|c| c.contains("artist"))
            {
                continue; // a header row
            }
            let length = cols.iter().find_map(|c| clock_ms(c));
            let words: Vec<&str> = cols
                .iter()
                .copied()
                .filter(|c| !c.is_empty() && clock_ms(c).is_none())
                .collect();
            if let Some(title) = words.first() {
                out.push(Foreign {
                    title: NUMBER.replace(title, "").trim().to_owned(),
                    artist: words.get(1).map(|s| s.to_string()).unwrap_or_default(),
                    album: words.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    duration_ms: length,
                    ..Default::default()
                });
            }
            continue;
        }
        let line = NUMBER.replace(line, "");
        let line = line.trim();
        let dash = [" - ", " – ", " — "]
            .iter()
            .find_map(|d| line.split_once(d));
        let (title, artist) = if let Some((a, t)) = dash {
            (t.trim(), a.trim())
        } else if let Some(m) = BY.find_iter(line).last() {
            (line[..m.start()].trim(), line[m.end()..].trim())
        } else {
            (line, "")
        };
        if !title.is_empty() {
            out.push(Foreign {
                title: title.to_owned(),
                artist: artist.to_owned(),
                ..Default::default()
            });
        }
    }
    out
}

/// "3:45" or "1:02:03" as milliseconds.
fn clock_ms(s: &str) -> Option<u64> {
    let parts: Vec<u64> = s
        .split(':')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [m, sec] if *sec < 60 => Some((m * 60 + sec) * 1000),
        [h, m, sec] if *m < 60 && *sec < 60 => Some((h * 3600 + m * 60 + sec) * 1000),
        _ => None,
    }
}

// --- Zips: Excel sheets and data downloads ---------------------------------------------------

/// The files inside a zip: (name, bytes), skipping folders, audio, pictures and anything large.
pub fn zip_entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("That zip can't be opened ({e})."))?;
    let skip = Regex::new(
        r"(?i)\.(flac|mp3|m4a|aac|ogg|opus|wav|aiff?|wma|jpe?g|png|gif|webp|mp4|mkv|pdf)$",
    )
    .expect("valid");
    let mut out = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() || skip.is_match(f.name()) || f.size() > 200 * 1024 * 1024 {
            continue;
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((f.name().to_owned(), buf));
    }
    Ok(out)
}

/// The first sheet of an Excel file (FreeYourMusic and others export these), as rows of text.
pub fn xlsx_rows(entries: &[(String, Vec<u8>)]) -> Option<Vec<Vec<String>>> {
    let get = |name: &str| {
        entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
    };
    get("xl/workbook.xml")?;
    let shared: Vec<String> = get("xl/sharedStrings.xml")
        .map(|s| {
            blocks(&s, "si")
                .into_iter()
                .map(|si| {
                    blocks(si, "t")
                        .iter()
                        .map(|t| unescape(t))
                        .collect::<String>()
                })
                .collect()
        })
        .unwrap_or_default();
    let sheet = get("xl/worksheets/sheet1.xml")?;
    let cell = Regex::new(r#"(?is)<c\b([^>]*)>(.*?)</c>"#).expect("valid");
    let col_of = Regex::new(r#"\br="([A-Z]+)\d+""#).expect("valid");
    let kind_of = Regex::new(r#"\bt="(\w+)""#).expect("valid");
    let mut rows = Vec::new();
    for row in blocks(&sheet, "row") {
        let mut cells: Vec<String> = Vec::new();
        for c in cell.captures_iter(row) {
            let attrs = &c[1];
            let body = &c[2];
            let col = col_of
                .captures(attrs)
                .map(|m| {
                    m[1].bytes()
                        .fold(0usize, |n, b| n * 26 + (b - b'A' + 1) as usize)
                        - 1
                })
                .unwrap_or(cells.len());
            let value = match kind_of.captures(attrs).map(|k| k[1].to_owned()).as_deref() {
                Some("s") => field(body, "v")
                    .and_then(|v| v.parse::<usize>().ok())
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default(),
                Some("inlineStr") => blocks(body, "t").iter().map(|t| unescape(t)).collect(),
                _ => field(body, "v").unwrap_or_default(),
            };
            if cells.len() <= col {
                cells.resize(col + 1, String::new());
            }
            cells[col] = value;
        }
        if cells.iter().any(|c| !c.trim().is_empty()) {
            rows.push(cells);
        }
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_file_is_comes_from_inside_it() {
        let k = |t: &str| detect(t.as_bytes(), t);
        assert_eq!(k("#EXTM3U\n#EXTINF:1,a - b\nx.mp3"), Kind::M3u);
        assert_eq!(k("D:\\Music\\a.flac\nD:\\Music\\b.mp3\n"), Kind::M3u);
        assert_eq!(k("[playlist]\nFile1=a.mp3"), Kind::Pls);
        assert_eq!(
            k("<?xml version=\"1.0\"?><playlist xmlns=\"http://xspf.org/ns/0/\">"),
            Kind::Xspf
        );
        assert_eq!(k("<?wpl version=\"1.0\"?><smil>"), Kind::Wpl);
        assert_eq!(k("<asx version=\"3.0\">"), Kind::Asx);
        assert_eq!(k("<?xml?><!DOCTYPE plist PUBLIC><plist>"), Kind::ItunesXml);
        assert_eq!(k("{\"playlist\":{\"track\":[]}}"), Kind::Jspf);
        assert_eq!(k("{\"playlists\":[]}"), Kind::SpotifyJson);
        assert_eq!(k("Name\tArtist\tAlbum\nA\tB\tC"), Kind::ItunesTxt);
        assert_eq!(k("Track name,Artist name,Album\nA,B,C"), Kind::Csv);
        assert_eq!(k("title,artist,album,isrc\nA,B,C,D"), Kind::Csv);
        assert_eq!(
            k("Arijit Singh - Kesariya\nQueen - Bohemian Rhapsody"),
            Kind::Songs
        );
        assert_eq!(detect(b"PK\x03\x04rest", ""), Kind::Zip);
    }

    #[test]
    fn file_addresses_become_paths() {
        assert_eq!(
            file_location("file:///D:/Music/A%20B.flac"),
            "D:/Music/A B.flac"
        );
        assert_eq!(file_location("file:///home/me/a.mp3"), "/home/me/a.mp3");
        assert_eq!(file_location("Music/a.mp3"), "Music/a.mp3");
    }

    #[test]
    fn songs_keep_titles_that_start_with_numbers() {
        let s = songs("99 Luftballons by Nena\n1. Ariana Grande - 7 Rings");
        assert_eq!(s[0].title, "99 Luftballons");
        assert_eq!(s[0].artist, "Nena");
        // "1. " is the list's numbering; the title's own number stays.
        assert_eq!(
            (s[1].title.as_str(), s[1].artist.as_str()),
            ("7 Rings", "Ariana Grande")
        );
    }

    #[test]
    fn an_excel_sheet_reads_as_rows() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("xl/workbook.xml", "<workbook/>"),
                (
                    "xl/sharedStrings.xml",
                    "<sst><si><t>Track name</t></si><si><t>Artist name</t></si><si><t>Kesariya</t></si><si><r><t>Arijit </t></r><r><t>Singh</t></r></si></sst>",
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c></row><row r=\"2\"><c r=\"A2\" t=\"s\"><v>2</v></c><c r=\"B2\" t=\"s\"><v>3</v></c><c r=\"D2\"><v>268</v></c></row></sheetData></worksheet>",
                ),
            ] {
                z.start_file(name, o).unwrap();
                std::io::Write::write_all(&mut z, body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let entries = zip_entries(buf.get_ref()).unwrap();
        let rows = xlsx_rows(&entries).unwrap();
        assert_eq!(rows[0], ["Track name", "Artist name"]);
        assert_eq!(rows[1], ["Kesariya", "Arijit Singh", "", "268"]);
    }
}
