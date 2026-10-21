//! Lyrics from the free lyrics services, for songs that have none of their own: an online
//! song, or a file of yours with no `.lrc` or lyrics tag (shown only, never written to it).
//!
//! Each source is asked the way it answers best (by the catalog song's id, or by title,
//! artists and length), and every answer is checked to be the same song: its length within a
//! few seconds, and its title or artist the one asked for. A service's fuzzy search that
//! offers a different song of the same length (the reason a song once showed another's
//! lyrics) is turned away.
//!
//! "Automatic" asks every source in your order at once and takes the first timed answer in
//! that order, else the first plain one. Answers are kept (a found answer for good, "none"
//! for a day), so a song is asked about once.
//!
//! The catalog's own lyrics (YouTube Music) come through the app's catalog client: the
//! caller passes them in as a source like any other.

use base64::Engine as _;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The sources, in their default order, with the names shown for them.
pub const SOURCES: [(&str, &str); 8] = [
    ("lrclib", "LRCLIB"),
    ("youtube", "YouTube Music"),
    ("simpmusic", "SimpMusic"),
    ("lyricsplus", "LyricsPlus"),
    ("boidu", "Better Lyrics"),
    ("netease", "Netease"),
    ("qq", "QQ Music"),
    ("kugou", "Kugou"),
];

/// A source's name for people ("LRCLIB"), or its id when unknown.
pub fn label(source: &str) -> &str {
    SOURCES
        .iter()
        .find(|(id, _)| *id == source)
        .map_or(source, |(_, l)| l)
}

/// How long one source may take before it is passed over.
const ASK_TIMEOUT: Duration = Duration::from_secs(8);
/// Automatic waits at most this long for the sources before taking what it has.
const ALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Lengths further apart than this are another recording.
const SAME_LENGTH_S: f64 = 5.0;
/// "No lyrics" is believed this long before a song is asked about again.
const MISS_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// The song lyrics are asked for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LyricsRequest {
    /// The catalog song's id, when it is one (some sources answer by it).
    #[serde(default)]
    pub video_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album: String,
    /// 0 when unknown.
    #[serde(default)]
    pub duration_ms: u64,
}

pub use crate::lyrics::{LyricLine, Lyrics, Second, SecondSource, Word};
use crate::lyrics::{carried_translation, finish, mark_source, read_lrc};

/// Lyrics one source gave.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// The source's id ("lrclib").
    pub source: String,
    /// Timed lines (empty when only the words are known), the song's language, the format.
    #[serde(flatten)]
    pub lyrics: Lyrics,
    /// The words as text, when not timed.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub instrumental: bool,
    /// Who the source says wrote them down ("Source: Musixmatch").
    #[serde(default)]
    pub credit: Option<String>,
}

impl Found {
    pub fn synced(&self) -> bool {
        !self.lyrics.lines.is_empty()
    }
    fn usable(&self) -> bool {
        self.synced() || !self.text.trim().is_empty() || self.instrumental
    }
}

/// A source's answer for the picker: what it has for this song.
#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub source: String,
    pub label: String,
    /// "synced", "plain", "none" or "failed".
    pub status: String,
    pub on: bool,
}

/// The sources in your order: \`setting\` lists ids, "-id" for one switched off; any it doesn't
/// name follow in their default order, on. Returns (id, on).
pub fn order(setting: &[String]) -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    for s in setting {
        let (id, on) = s
            .strip_prefix('-')
            .map_or((s.as_str(), true), |id| (id, false));
        if let Some((known, _)) = SOURCES.iter().find(|(k, _)| *k == id.trim())
            && !out.iter().any(|(o, _)| o == known)
        {
            out.push((known, on));
        }
    }
    for (id, _) in SOURCES {
        if !out.iter().any(|(o, _)| *o == id) {
            out.push((id, true));
        }
    }
    out
}

// --- Is it the same song? -------------------------------------------------------------------

/// A title or name reduced to what tells it apart: lower case, letters and digits only, the
/// parts in brackets and the film and feature notes left out.
pub fn fold(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut cut = lower.as_str();
    for mark in [
        " (from", " [from", " - from", " (feat", " [feat", " feat.", " ft.", " | ",
    ] {
        if let Some(i) = cut.find(mark) {
            cut = &cut[..i];
        }
    }
    let mut out = String::new();
    let mut depth = 0i32;
    for c in cut.chars() {
        match c {
            '(' | '[' | '{' | '【' | '（' => depth += 1,
            ')' | ']' | '}' | '】' | '）' => depth = (depth - 1).max(0),
            c if depth == 0 && c.is_alphanumeric() => out.push(c),
            _ => {}
        }
    }
    out
}

/// The title to ask by: what is before a "|", without bracketed notes.
pub fn search_title(title: &str) -> String {
    let base = title.split('|').next().unwrap_or(title);
    let mut out = String::new();
    let mut depth = 0i32;
    for c in base.chars() {
        match c {
            '(' | '[' | '{' | '【' | '（' => depth += 1,
            ')' | ']' | '}' | '】' | '）' => depth = (depth - 1).max(0),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        title.trim().to_owned()
    } else {
        t
    }
}

fn latin(s: &str) -> bool {
    s.chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_ascii())
}

/// Whether an answer is the song asked about: its length close (when both are known) and its
/// title or one of its artists the same. Titles written in another script (a Devanagari title
/// for a Roman one) can't be compared letter by letter: then the artist decides.
pub fn same_song(req: &LyricsRequest, title: &str, artist: &str, duration_s: Option<f64>) -> bool {
    if let Some(d) = duration_s.filter(|d| *d > 0.0)
        && req.duration_ms > 0
        && (d - req.duration_ms as f64 / 1000.0).abs() > SAME_LENGTH_S
    {
        return false;
    }
    let (a, b) = (fold(&req.title), fold(title));
    let title_known = !a.is_empty() && !b.is_empty() && latin(&req.title) == latin(title);
    let title_same = title_known
        && (a == b || (a.len() >= 3 && b.contains(&a)) || (b.len() >= 3 && a.contains(&b)));
    let theirs = fold(artist);
    let artist_same = !theirs.is_empty()
        && req
            .artists
            .iter()
            .map(|x| fold(x))
            .filter(|x| x.len() >= 3)
            .any(|x| theirs.contains(&x) || x.contains(&theirs));
    title_same || (artist_same && (!title_known || title_same))
}

// --- Reading the formats --------------------------------------------------------------------

/// A TTML time: "12.345", "1:02.5", "00:01:02.500" or "1234ms".
fn ttml_time(t: &str) -> Option<u64> {
    let t = t.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.parse::<f64>().ok().map(|x| x as u64);
    }
    let t = t.strip_suffix('s').unwrap_or(t);
    let mut secs = 0.0;
    for part in t.split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some((secs * 1000.0).round() as u64)
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')?;
    Some(&tag[at..at + end])
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

/// TTML (Apple's lyrics): \`<p begin end>\` lines, \`<span begin end>\` words.
pub fn parse_ttml(xml: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<p ") {
        let after = &rest[i..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let tag = &after[..open_end];
        let Some(close) = after.find("</p>") else {
            break;
        };
        let body = &after[open_end + 1..close];
        rest = &after[close + 4..];
        let Some(ms) = attr(tag, "begin").and_then(ttml_time) else {
            continue;
        };
        let end_ms = attr(tag, "end").and_then(ttml_time).unwrap_or(0);
        let mut words = Vec::new();
        let mut b = body;
        while let Some(j) = b.find("<span") {
            let s = &b[j..];
            let Some(te) = s.find('>') else { break };
            let stag = &s[..te];
            let Some(ce) = s.find("</span>") else { break };
            let inner = &s[te + 1..ce];
            if !inner.contains('<')
                && let (Some(ws), Some(we)) = (
                    attr(stag, "begin").and_then(ttml_time),
                    attr(stag, "end").and_then(ttml_time),
                )
            {
                // A space after the span belongs to the word (the words of a line run on).
                let spaced = s[ce + 7..].starts_with(' ');
                words.push(Word {
                    start_ms: ws,
                    end_ms: we,
                    text: format!("{}{}", unescape(inner), if spaced { " " } else { "" }),
                });
            }
            b = &s[ce + 7..];
        }
        let text = if words.is_empty() {
            let mut t = String::new();
            let mut depth = 0;
            for c in body.chars() {
                match c {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    c if depth == 0 => t.push(c),
                    _ => {}
                }
            }
            unescape(t.trim())
        } else {
            words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<String>()
                .trim()
                .to_owned()
        };
        if !text.is_empty() {
            out.push(LyricLine {
                start_ms: ms,
                end_ms,
                text,
                words,
                ..Default::default()
            });
        }
    }
    finish(&mut out);
    out
}

/// LyricsPlus's own JSON: lines with their time, and syllables when it times words.
fn parse_kpoe(v: &Value) -> Vec<LyricLine> {
    let word_timed = v.get("type").and_then(Value::as_str) == Some("Word");
    let mut lines: Vec<LyricLine> = v
        .get("lyrics")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|l| {
                    let ms = l.get("time").and_then(Value::as_u64)?;
                    let duration = l.get("duration").and_then(Value::as_u64).unwrap_or(0);
                    let text = l
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_owned();
                    let words: Vec<Word> = if word_timed {
                        l.get("syllabus")
                            .and_then(Value::as_array)
                            .map(|ws| {
                                ws.iter()
                                    .filter_map(|w| {
                                        let ms = w.get("time").and_then(Value::as_u64)?;
                                        let d =
                                            w.get("duration").and_then(Value::as_u64).unwrap_or(0);
                                        Some(Word {
                                            start_ms: ms,
                                            end_ms: ms + d,
                                            text: w.get("text").and_then(Value::as_str)?.to_owned(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let text = if text.is_empty() {
                        words
                            .iter()
                            .map(|w| w.text.as_str())
                            .collect::<String>()
                            .trim()
                            .to_owned()
                    } else {
                        text
                    };
                    (!text.is_empty()).then_some(LyricLine {
                        start_ms: ms,
                        end_ms: ms + duration,
                        text,
                        words,
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    finish(&mut lines);
    lines
}

/// Lyrics in any of the formats the sources send: TTML, LRC, or text.
fn found_from_text(source: &str, body: &str) -> Option<Found> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    let mut lines = if body.starts_with('<') {
        parse_ttml(body)
    } else {
        read_lrc(body).lines
    };
    mark_source(&mut lines, source);
    let text = if lines.is_empty() && !body.starts_with('<') {
        body.to_owned()
    } else {
        String::new()
    };
    let f = Found {
        source: source.into(),
        lyrics: Lyrics {
            lines,
            ..Default::default()
        },
        text,
        ..Default::default()
    };
    f.usable().then_some(f)
}

/// Translations from a second LRC, put on the lines that start at the same time.
fn with_translation(mut lines: Vec<LyricLine>, translated: &str, provider: &str) -> Vec<LyricLine> {
    let t = read_lrc(translated).lines;
    for l in &mut lines {
        if let Some(x) = t
            .iter()
            .find(|x| x.start_ms == l.start_ms && !x.text.is_empty() && x.text != l.text)
        {
            l.translation = Some(carried_translation(&x.text, provider));
        }
    }
    lines
}

// --- The sources ----------------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(ASK_TIMEOUT))
        .user_agent(crate::net::USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into()
}

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// GET JSON; None for "not there" (404, 401), an error for trouble.
fn get_json(
    agent: &ureq::Agent,
    url: &str,
    referer: Option<&str>,
) -> Result<Option<Value>, String> {
    if crate::net::is_offline() {
        return Err("Local only is on".into());
    }
    let mut req = agent.get(url);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    match resp.status().as_u16() {
        200 => {
            let text = resp
                .body_mut()
                .read_to_string()
                .map_err(|e| e.to_string())?;
            serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| e.to_string())
        }
        401 | 404 => Ok(None),
        s => Err(format!("HTTP {s}")),
    }
}

fn artists_line(req: &LyricsRequest) -> String {
    req.artists.join(", ")
}

fn duration_s(req: &LyricsRequest) -> Option<u64> {
    (req.duration_ms > 0).then(|| (req.duration_ms as f64 / 1000.0).round() as u64)
}

fn lrclib(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let base = "https://lrclib.net/api";
    let title = search_title(&req.title);
    let mut candidates: Vec<(Value, bool)> = Vec::new();
    let mut url = format!(
        "{base}/get?track_name={}&artist_name={}",
        enc(&title),
        enc(&artists_line(req))
    );
    if !req.album.is_empty() {
        url.push_str(&format!("&album_name={}", enc(&req.album)));
    }
    if let Some(d) = duration_s(req) {
        url.push_str(&format!("&duration={d}"));
    }
    if let Some(v) = get_json(agent, &url, None)? {
        candidates.push((v, true));
    }
    if !candidates.iter().any(|(v, _)| {
        v.get("syncedLyrics")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    }) {
        let first = req.artists.first().cloned().unwrap_or_default();
        for q in [
            format!(
                "{base}/search?track_name={}&artist_name={}",
                enc(&title),
                enc(&first)
            ),
            format!("{base}/search?q={}", enc(&format!("{title} {first}"))),
        ] {
            if let Some(Value::Array(list)) = get_json(agent, &q, None)? {
                candidates.extend(list.into_iter().map(|v| (v, false)));
            }
            if candidates.iter().any(|(v, _)| {
                v.get("syncedLyrics")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            }) {
                break;
            }
        }
    }
    // Only the song asked about: the exact answer first, then the closest in length.
    let target = req.duration_ms as f64 / 1000.0;
    let mut kept: Vec<(Value, bool)> = candidates
        .into_iter()
        .filter(|(v, _)| {
            let t = v.get("trackName").and_then(Value::as_str).unwrap_or("");
            let a = v.get("artistName").and_then(Value::as_str).unwrap_or("");
            same_song(req, t, a, v.get("duration").and_then(Value::as_f64))
        })
        .collect();
    kept.sort_by(|(a, ea), (b, eb)| {
        let da = (a.get("duration").and_then(Value::as_f64).unwrap_or(target) - target).abs();
        let db = (b.get("duration").and_then(Value::as_f64).unwrap_or(target) - target).abs();
        eb.cmp(ea).then(da.total_cmp(&db))
    });
    let pick = kept
        .iter()
        .find(|(v, _)| v.get("instrumental").and_then(Value::as_bool) == Some(true))
        .or_else(|| {
            kept.iter().find(|(v, _)| {
                v.get("syncedLyrics")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.trim().is_empty())
            })
        })
        .or_else(|| {
            kept.iter().find(|(v, _)| {
                v.get("plainLyrics")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.trim().is_empty())
            })
        });
    let Some((v, _)) = pick else { return Ok(None) };
    if v.get("instrumental").and_then(Value::as_bool) == Some(true) {
        return Ok(Some(Found {
            source: "lrclib".into(),
            instrumental: true,
            ..Default::default()
        }));
    }
    let synced = v.get("syncedLyrics").and_then(Value::as_str).unwrap_or("");
    let mut lines = read_lrc(synced).lines;
    mark_source(&mut lines, "lrclib");
    let text = if lines.is_empty() {
        v.get("plainLyrics")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned()
    } else {
        String::new()
    };
    let f = Found {
        source: "lrclib".into(),
        lyrics: Lyrics {
            lines,
            ..Default::default()
        },
        text,
        ..Default::default()
    };
    Ok(f.usable().then_some(f))
}

fn simpmusic(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let Some(id) = req.video_id.as_deref().filter(|i| !i.is_empty()) else {
        return Ok(None);
    };
    let Some(v) = get_json(
        agent,
        &format!("https://api-lyrics.simpmusic.org/v1/{}", enc(id)),
        None,
    )?
    else {
        return Ok(None);
    };
    let mut list: Vec<&Value> = v
        .get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    list.sort_by_key(|x| std::cmp::Reverse(x.get("vote").and_then(Value::as_i64).unwrap_or(0)));
    for x in &list {
        for field in ["richSyncLyrics", "syncedLyrics"] {
            if let Some(t) = x
                .get(field)
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
            {
                let mut lines = read_lrc(t).lines;
                mark_source(&mut lines, "simpmusic");
                if !lines.is_empty() {
                    return Ok(Some(Found {
                        source: "simpmusic".into(),
                        lyrics: Lyrics {
                            lines,
                            ..Default::default()
                        },
                        ..Default::default()
                    }));
                }
            }
        }
    }
    Ok(list
        .iter()
        .find_map(|x| {
            x.get("plainLyric")
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
        })
        .map(|t| Found {
            source: "simpmusic".into(),
            text: t.trim().to_owned(),
            ..Default::default()
        }))
}

fn lyricsplus(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let mut url = format!(
        "https://lyricsplus.binimum.org/v2/lyrics/get?title={}&artist={}",
        enc(&search_title(&req.title)),
        enc(&artists_line(req))
    );
    if !req.album.is_empty() {
        url.push_str(&format!("&album={}", enc(&req.album)));
    }
    if let Some(d) = duration_s(req) {
        url.push_str(&format!("&duration={d}"));
    }
    let Some(v) = get_json(agent, &url, None)? else {
        return Ok(None);
    };
    // Its length is in its metadata: another recording's lyrics are not these.
    if let Some(total) = v.pointer("/metadata/totalDuration").and_then(Value::as_f64)
        && req.duration_ms > 0
        && (total / 1000.0 - req.duration_ms as f64 / 1000.0).abs() > SAME_LENGTH_S
    {
        return Ok(None);
    }
    let lines = parse_kpoe(&v);
    Ok((!lines.is_empty()).then(|| Found {
        source: "lyricsplus".into(),
        lyrics: Lyrics {
            lines,
            ..Default::default()
        },
        ..Default::default()
    }))
}

fn boidu(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let Some(d) = duration_s(req) else {
        return Ok(None);
    };
    let base = format!(
        "https://lyrics-api.boidu.dev/getLyrics?s={}&a={}&d={d}",
        enc(&search_title(&req.title)),
        enc(&artists_line(req))
    );
    let mut answer = None;
    if !req.album.is_empty() {
        answer = get_json(agent, &format!("{base}&al={}", enc(&req.album)), None)?;
    }
    if answer.is_none() {
        answer = get_json(agent, &base, None)?;
    }
    let Some(v) = answer else { return Ok(None) };
    let body = ["ttml", "syncedLyrics", "lyrics", "lrc"]
        .iter()
        .find_map(|k| {
            v.get(*k)
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
        });
    Ok(body.and_then(|b| found_from_text("boidu", b)))
}

/// The closest in length within the tolerance, among the songs that are the one asked about.
fn closest<'a>(
    req: &LyricsRequest,
    list: &'a [(Value, String, String, Option<f64>)],
) -> Option<&'a Value> {
    let target = req.duration_ms as f64 / 1000.0;
    list.iter()
        .filter(|(_, t, a, d)| same_song(req, t, a, *d))
        .min_by(|x, y| {
            let dx = x.3.map_or(f64::MAX, |d| (d - target).abs());
            let dy = y.3.map_or(f64::MAX, |d| (d - target).abs());
            dx.total_cmp(&dy)
        })
        .map(|(v, ..)| v)
}

fn netease(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let q = format!(
        "{} {}",
        search_title(&req.title),
        req.artists.first().cloned().unwrap_or_default()
    );
    let mut resp = agent
        .post("https://music.163.com/api/search/get")
        .header("Referer", "https://music.163.com/")
        .send_form([
            ("s", q.as_str()),
            ("type", "1"),
            ("limit", "5"),
            ("offset", "0"),
        ])
        .map_err(|e| e.to_string())?;
    if resp.status().as_u16() != 200 {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: Value = serde_json::from_str(
        &resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let songs: Vec<(Value, String, String, Option<f64>)> = v
        .pointer("/result/songs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    let artists = s
                        .get("artists")
                        .and_then(Value::as_array)
                        .map(|x| {
                            x.iter()
                                .filter_map(|a| a.get("name").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    (
                        s.clone(),
                        s.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        artists,
                        s.get("duration")
                            .and_then(Value::as_f64)
                            .map(|ms| ms / 1000.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let Some(id) = closest(req, &songs)
        .and_then(|s| s.get("id"))
        .and_then(Value::as_i64)
    else {
        return Ok(None);
    };
    let Some(l) = get_json(
        agent,
        &format!("https://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1"),
        Some("https://music.163.com/"),
    )?
    else {
        return Ok(None);
    };
    let Some(lrc) = l
        .pointer("/lrc/lyric")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    else {
        return Ok(None);
    };
    let Some(mut f) = found_from_text("netease", lrc) else {
        return Ok(None);
    };
    if let Some(t) = l
        .pointer("/tlyric/lyric")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    {
        f.lyrics.lines = with_translation(f.lyrics.lines, t, "netease");
    }
    Ok(Some(f))
}

fn qq(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let referer = Some("https://y.qq.com/");
    let q = format!(
        "{} {}",
        search_title(&req.title),
        req.artists.first().cloned().unwrap_or_default()
    );
    let Some(v) = get_json(
        agent,
        &format!(
            "https://c.y.qq.com/splcloud/fcgi-bin/smartbox_new.fcg?format=json&key={}",
            enc(&q)
        ),
        referer,
    )?
    else {
        return Ok(None);
    };
    let mids: Vec<String> = v
        .pointer("/data/song/itemlist")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|i| i.get("mid").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if mids.is_empty() {
        return Ok(None);
    }
    let Some(d) = get_json(
        agent,
        &format!(
            "https://c.y.qq.com/v8/fcg-bin/fcg_play_single_song.fcg?format=json&songmid={}",
            enc(&mids.join(","))
        ),
        referer,
    )?
    else {
        return Ok(None);
    };
    let songs: Vec<(Value, String, String, Option<f64>)> = d
        .get("data")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    let artists = s
                        .get("singer")
                        .and_then(Value::as_array)
                        .map(|x| {
                            x.iter()
                                .filter_map(|a| a.get("name").and_then(Value::as_str))
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    (
                        s.clone(),
                        s.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        artists,
                        s.get("interval").and_then(Value::as_f64),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let Some(mid) = closest(req, &songs)
        .and_then(|s| s.get("mid"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let Some(l) = get_json(
        agent,
        &format!(
            "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid={}&format=json&nobase64=1",
            enc(mid)
        ),
        referer,
    )?
    else {
        return Ok(None);
    };
    let Some(raw) = l
        .get("lyric")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    else {
        return Ok(None);
    };
    let text = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| unescape(raw));
    Ok(found_from_text("qq", &text))
}

fn kugou(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, String> {
    let q = format!(
        "{} {}",
        search_title(&req.title),
        req.artists.first().cloned().unwrap_or_default()
    );
    let Some(v) = get_json(
        agent,
        &format!(
            "https://songsearch.kugou.com/song_search_v2?keyword={}&page=1&pagesize=5",
            enc(&q)
        ),
        None,
    )?
    else {
        return Ok(None);
    };
    let songs: Vec<(Value, String, String, Option<f64>)> = v
        .pointer("/data/lists")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        s.clone(),
                        s.get("SongName")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        s.get("SingerName")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        s.get("Duration").and_then(Value::as_f64),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let Some(hash) = closest(req, &songs)
        .and_then(|s| s.get("FileHash"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let Some(c) = get_json(
        agent,
        &format!(
            "https://krcs.kugou.com/search?ver=1&man=yes&client=mobi&hash={}",
            enc(hash)
        ),
        None,
    )?
    else {
        return Ok(None);
    };
    let Some(first) = c
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    else {
        return Ok(None);
    };
    let (Some(id), Some(key)) = (
        first
            .get("id")
            .map(|i| i.to_string().trim_matches('"').to_owned()),
        first.get("accesskey").and_then(Value::as_str),
    ) else {
        return Ok(None);
    };
    let Some(d) = get_json(
        agent,
        &format!(
            "https://lyrics.kugou.com/download?ver=1&client=pc&id={}&accesskey={}&fmt=lrc",
            enc(&id),
            enc(key)
        ),
        None,
    )?
    else {
        return Ok(None);
    };
    let Some(content) = d.get("content").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(text) = base64::engine::general_purpose::STANDARD
        .decode(content.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    else {
        return Ok(None);
    };
    Ok(found_from_text("kugou", &text))
}

/// The catalog's own lyrics, asked of the app's catalog client (see the module notes).
pub type CatalogAsk<'a> = &'a (dyn Fn(&LyricsRequest) -> Result<Option<Found>, String> + Sync);

/// Ask one source.
pub fn ask(
    source: &str,
    req: &LyricsRequest,
    catalog: CatalogAsk,
) -> Result<Option<Found>, String> {
    let agent = agent();
    match source {
        "lrclib" => lrclib(&agent, req),
        "youtube" => catalog(req),
        "simpmusic" => simpmusic(&agent, req),
        "lyricsplus" => lyricsplus(&agent, req),
        "boidu" => boidu(&agent, req),
        "netease" => netease(&agent, req),
        "qq" => qq(&agent, req),
        "kugou" => kugou(&agent, req),
        other => Err(format!("no lyrics source called {other}")),
    }
}

/// Of answers in your order (None: not in yet), the first timed one in that order once every
/// earlier one is in; with \`all_in\`, else the first plain one. None while it must still wait.
pub fn choose(answers: &[Option<Option<Found>>], all_in: bool) -> Option<Option<Found>> {
    for a in answers {
        match a {
            // An earlier source still to answer could be the timed one: wait, unless time is up.
            None if !all_in => return None,
            Some(Some(f)) if f.synced() || f.instrumental => return Some(Some(f.clone())),
            _ => {}
        }
    }
    Some(first_plain(answers))
}

fn first_plain(answers: &[Option<Option<Found>>]) -> Option<Found> {
    answers
        .iter()
        .flatten()
        .flatten()
        .find(|f| f.usable())
        .cloned()
}

/// Automatic: every source in \`sources\` asked at once (each a moment after the one before),
/// the first timed answer in that order taken.
pub fn automatic(req: &LyricsRequest, sources: &[&str], catalog: CatalogAsk) -> Option<Found> {
    if sources.is_empty() {
        return None;
    }
    let (tx, rx) = mpsc::channel::<(usize, Option<Found>)>();
    std::thread::scope(|scope| {
        for (i, s) in sources.iter().enumerate() {
            let tx = tx.clone();
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(150 * i as u64));
                let _ = tx.send((i, ask(s, req, catalog).ok().flatten()));
            });
        }
        drop(tx);
        let started = Instant::now();
        let mut answers: Vec<Option<Option<Found>>> = vec![None; sources.len()];
        loop {
            if let Some(pick) = choose(&answers, false) {
                return pick;
            }
            let left = ALL_TIMEOUT.saturating_sub(started.elapsed());
            match rx.recv_timeout(left) {
                Ok((i, f)) => answers[i] = Some(f),
                Err(_) => return choose(&answers, true).flatten(),
            }
        }
    })
}

/// Every source's answer for the picker (switched-off ones too, marked so), at once.
pub fn check_all(
    req: &LyricsRequest,
    order: &[(&'static str, bool)],
    catalog: CatalogAsk,
) -> Vec<(SourceStatus, Option<Found>)> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = order
            .iter()
            .map(|(id, on)| {
                let id = *id;
                let on = *on;
                scope.spawn(move || {
                    let answer = ask(id, req, catalog);
                    let status = match &answer {
                        Ok(Some(f)) if f.synced() || f.instrumental => "synced",
                        Ok(Some(_)) => "plain",
                        Ok(None) => "none",
                        Err(_) => "failed",
                    };
                    (
                        SourceStatus {
                            source: id.into(),
                            label: label(id).into(),
                            status: status.into(),
                            on,
                        },
                        answer.ok().flatten(),
                    )
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

// --- Your choices per song -------------------------------------------------------------------

/// What you chose for one song's lyrics: a source (instead of Automatic), how much earlier or
/// later they run, and the title and artists to ask by when the song was named wrongly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Positive: the lyrics come later.
    #[serde(default)]
    pub offset_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artists: Option<Vec<String>>,
}

impl Choice {
    fn is_empty(&self) -> bool {
        self.source.is_none()
            && self.offset_ms == 0
            && self.title.is_none()
            && self.artists.is_none()
    }
    /// The song to ask about: the one played, named as you named it.
    pub fn applied(&self, req: &LyricsRequest) -> LyricsRequest {
        let mut r = req.clone();
        if let Some(t) = self.title.as_ref().filter(|t| !t.trim().is_empty()) {
            r.title = t.trim().to_owned();
            r.album.clear();
            // Asked by name now: an id would answer for the song as it was named.
            r.video_id = None;
        }
        if let Some(a) = self.artists.as_ref() {
            r.artists = a
                .iter()
                .map(|x| x.trim().to_owned())
                .filter(|x| !x.is_empty())
                .collect();
        }
        r
    }
}

fn choices_file() -> std::path::PathBuf {
    crate::paths::app_data_dir().join("lyrics-choices.json")
}

/// Every song's choice, by the song's key.
pub fn choices() -> std::collections::BTreeMap<String, Choice> {
    std::fs::read_to_string(choices_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Change one song's choice (an empty one is forgotten).
pub fn set_choice(key: &str, change: impl FnOnce(&mut Choice)) -> Result<Choice, String> {
    let mut all = choices();
    let mut c = all.get(key).cloned().unwrap_or_default();
    change(&mut c);
    if c.is_empty() {
        all.remove(key);
    } else {
        all.insert(key.to_owned(), c.clone());
    }
    let file = choices_file();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = file.with_extension("json.part");
    std::fs::write(
        &part,
        serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
    Ok(c)
}

// --- Kept answers ----------------------------------------------------------------------------

/// Answers kept for a song (\`key\`) per source ("auto" for Automatic's): found ones for good,
/// "none" for a day.
pub struct Kept {
    conn: Connection,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Kept {
    pub fn open() -> Result<Self, String> {
        let dir = crate::paths::app_data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(dir.join("cache.sqlite")).map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_secs(30))
            .map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lyrics (
                song TEXT NOT NULL,
                source TEXT NOT NULL,
                found TEXT,
                at INTEGER NOT NULL,
                PRIMARY KEY (song, source)
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Kept { conn })
    }

    /// Some(answer) when one is kept and still good.
    pub fn get(&self, song: &str, source: &str) -> Option<Option<Found>> {
        let row: Option<(Option<String>, i64)> = self
            .conn
            .query_row(
                "SELECT found, at FROM lyrics WHERE song = ?1 AND source = ?2",
                params![song, source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten();
        match row {
            Some((Some(json), _)) => serde_json::from_str(&json).ok().map(Some),
            Some((None, at)) if now_ms() - at < MISS_TTL_MS => Some(None),
            _ => None,
        }
    }

    pub fn put(&self, song: &str, source: &str, found: Option<&Found>) {
        let json = found.and_then(|f| serde_json::to_string(f).ok());
        let _ = self.conn.execute(
            "INSERT OR REPLACE INTO lyrics (song, source, found, at) VALUES (?1, ?2, ?3, ?4)",
            params![song, source, json, now_ms()],
        );
    }

    /// Forget every kept answer (Settings: forget kept lyrics). Returns how many.
    pub fn clear(&self) -> usize {
        self.conn.execute("DELETE FROM lyrics", []).unwrap_or(0)
    }

    /// Forget what Automatic found for a song (the sources' order changed, or a new search).
    pub fn forget(&self, song: &str) {
        let _ = self
            .conn
            .execute("DELETE FROM lyrics WHERE song = ?1", params![song]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_keeps_every_lookup_at_home() {
        // Nothing listens on port 9: were the request sent, the error would be another one.
        crate::net::set_offline(true);
        let refused = get_json(&agent(), "http://127.0.0.1:9/lyrics", None);
        crate::net::set_offline(false);
        assert_eq!(refused, Err("Local only is on".to_owned()));
        assert_ne!(
            get_json(&agent(), "http://127.0.0.1:9/lyrics", None),
            Err("Local only is on".to_owned())
        );
    }

    fn req(title: &str, artists: &[&str], secs: u64) -> LyricsRequest {
        LyricsRequest {
            video_id: None,
            title: title.into(),
            artists: artists.iter().map(|a| a.to_string()).collect(),
            album: String::new(),
            duration_ms: secs * 1000,
        }
    }

    #[test]
    fn another_song_of_the_same_length_is_not_this_one() {
        // The bug: Zingaat (Ajay-Atul) showed Kesariya's lyrics, a fuzzy hit of similar length.
        let zingaat = req("Zingaat", &["Ajay-Atul"], 230);
        assert!(!same_song(
            &zingaat,
            "Kesariya",
            "Pritam, Arijit Singh",
            Some(229.0)
        ));
        assert!(same_song(&zingaat, "Zingaat", "Ajay-Atul", Some(231.0)));
        // A Devanagari title can't be read against a Roman one: the artist decides.
        assert!(same_song(&zingaat, "झिंगाट", "Ajay-Atul", Some(230.0)));
        assert!(!same_song(&zingaat, "झिंगाट", "Someone Else", Some(230.0)));
        // Film notes don't make a title another.
        let tum = req(
            "Tum Hi Ho (From \"Aashiqui 2\")",
            &["Mithoon", "Arijit Singh"],
            262,
        );
        assert!(same_song(&tum, "Tum Hi Ho", "Arijit Singh", Some(261.0)));
        // Too long apart is another recording.
        assert!(!same_song(&tum, "Tum Hi Ho", "Arijit Singh", Some(300.0)));
    }

    #[test]
    fn every_source_reads_lrc_with_the_shared_reader() {
        let f = found_from_text(
            "netease",
            "[ar:Someone]
[00:01.00]One
[00:01.00]Uno
[00:05.00]<00:05.00>Word <00:05.40>by",
        )
        .unwrap();
        let l = &f.lyrics.lines;
        assert_eq!(
            l.iter().map(|l| l.start_ms).collect::<Vec<_>>(),
            [1000, 5000]
        );
        assert_eq!(l[1].text, "Word by", "word stamps are timing, not text");
        assert_eq!(l[1].words.len(), 2);
        let t = l[0].translation.as_ref().unwrap();
        assert_eq!(t.text, "Uno");
        assert_eq!(t.source, SecondSource::Lyrics("netease".into()));
    }

    #[test]
    fn ttml_lines_and_word_times() {
        let xml = r#"<tt><body><div><p begin="00:01.000" end="00:03.000"><span begin="00:01.000" end="00:01.500">Hel</span><span begin="00:01.500" end="00:02.000">lo</span> <span begin="00:02.100" end="00:03.000">you</span></p><p begin="4.5" end="6">Plain &amp; line</p></div></body></tt>"#;
        let lines = parse_ttml(xml);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].start_ms, lines[0].end_ms), (1000, 3000));
        assert_eq!(lines[0].text, "Hello you");
        assert_eq!(lines[0].words.len(), 3);
        assert_eq!(
            lines[1],
            LyricLine {
                start_ms: 4500,
                end_ms: 6000,
                text: "Plain & line".into(),
                script: "Latn".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn titles_are_asked_without_their_notes() {
        assert_eq!(
            search_title("Gehra Hua (From \"Dhurandhar\") | Official Video"),
            "Gehra Hua"
        );
        assert_eq!(fold("Tum Hi Ho (feat. X)"), "tumhiho");
    }

    #[test]
    fn a_song_named_wrongly_is_asked_about_by_the_name_you_gave() {
        let played = LyricsRequest {
            video_id: Some("v1".into()),
            album: "Sairat".into(),
            ..req("Zingat Hindi", &["Ajay-Atul"], 230)
        };
        let c = Choice {
            title: Some("Zingaat".into()),
            artists: Some(vec!["Ajay Gogavale".into()]),
            ..Default::default()
        };
        let asked = c.applied(&played);
        assert_eq!(asked.title, "Zingaat");
        assert_eq!(asked.artists, ["Ajay Gogavale"]);
        assert_eq!(
            asked.video_id, None,
            "by name, not by the id of the song as it was named"
        );
        assert_eq!(asked.duration_ms, 230_000, "the length is the song's own");
        assert_eq!(Choice::default().applied(&played), played);
    }

    #[test]
    fn your_order_with_switched_off_sources_and_the_rest_after() {
        let o = order(&["youtube".into(), "-lrclib".into(), "nonsense".into()]);
        assert_eq!(o[0], ("youtube", true));
        assert_eq!(o[1], ("lrclib", false));
        assert_eq!(o.len(), SOURCES.len());
        assert!(o[2..].iter().all(|(_, on)| *on));
    }

    fn timed(source: &str) -> Option<Option<Found>> {
        Some(Some(Found {
            source: source.into(),
            lyrics: Lyrics {
                lines: vec![LyricLine {
                    text: "x".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        }))
    }
    fn plain(source: &str) -> Option<Option<Found>> {
        Some(Some(Found {
            source: source.into(),
            text: "x".into(),
            ..Default::default()
        }))
    }

    #[test]
    fn automatic_takes_the_first_timed_answer_in_your_order() {
        // The second source is timed, but the first hasn't answered: wait for it.
        assert_eq!(choose(&[None, timed("b")], false), None);
        // The first has nothing: the second's timed lyrics win.
        assert_eq!(
            choose(&[Some(None), timed("b")], false)
                .flatten()
                .map(|f| f.source),
            Some("b".into())
        );
        // A plain answer first doesn't stop a timed one after it.
        assert_eq!(
            choose(&[plain("a"), timed("b")], false)
                .flatten()
                .map(|f| f.source),
            Some("b".into())
        );
        // Nothing timed at all: the first plain one, once all are in.
        assert_eq!(
            choose(&[plain("a"), Some(None)], true)
                .flatten()
                .map(|f| f.source),
            Some("a".into())
        );
        assert_eq!(
            choose(&[plain("a"), None], false),
            None,
            "still waiting for a timed one"
        );
        assert_eq!(
            choose(&[plain("a"), None], true)
                .flatten()
                .map(|f| f.source),
            Some("a".into())
        );
    }
}
