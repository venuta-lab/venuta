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
//! that order, else the first plain one. Answers are kept (a found answer until Venuta reads
//! lyrics in a new way, `crate::lyrics::VERSION`; "none" for a day), so a song is asked about
//! once.
//!
//! A source that turns Venuta away (401, 403: it wants a key, or answers only its own app) or
//! can't be reached is not saying "no lyrics for this song": that is never kept as a miss. A
//! source that turned Venuta away isn't asked again until Venuta restarts, and Settings says
//! it isn't answering.
//!
//! The catalog's own lyrics (YouTube Music) come through the app's catalog client: the
//! caller passes them in as a source like any other.

use base64::Engine as _;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The sources, in their default order, with the names shown for them.
pub const SOURCES: [(&str, &str); 9] = [
    ("lrclib", "LRCLIB"),
    ("youtube", "YouTube Music"),
    ("simpmusic", "SimpMusic"),
    ("lyricsplus", "LyricsPlus"),
    ("boidu", "Better Lyrics"),
    ("netease", "Netease"),
    ("amll", "AMLL TTML DB"),
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
use crate::lyrics::{
    carried_translation, finish, mark_source, read_lrc, romaji, roman_lang, timed_by_chars, ttml,
};

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
    /// Who made the song, as the source wrote it among the lyrics ("作词 : Ayase"): taken out
    /// of the sung lines and kept here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credits: Vec<String>,
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

// --- When a source doesn't answer ------------------------------------------------------------

/// Why a source gave no answer. Neither is "no lyrics for this song", so neither is kept as one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskError {
    /// It turned Venuta away (401, 403: it wants a key, or answers only its own app).
    Refused(String),
    /// It couldn't be reached, was busy, or answered with something unreadable.
    Down(String),
}

impl std::fmt::Display for AskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AskError::Refused(why) | AskError::Down(why) => f.write_str(why),
        }
    }
}

impl From<String> for AskError {
    fn from(why: String) -> Self {
        AskError::Down(why)
    }
}

/// What an HTTP answer means: its body (200), "not there" (404), or why there is none. The
/// reason a source gives for turning Venuta away is kept, to show.
pub fn answer_of(status: u16, body: &str) -> Result<Option<String>, AskError> {
    let why = || {
        let v: Option<Value> = serde_json::from_str(body).ok();
        ["/message", "/error/reason", "/reason", "/error"]
            .iter()
            .find_map(|k| {
                v.as_ref()?
                    .pointer(k)?
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("HTTP {status}"))
    };
    match status {
        200 => Ok(Some(body.to_owned())),
        404 => Ok(None),
        401 | 403 => Err(AskError::Refused(why())),
        429 => Err(AskError::Down(format!("Busy: {}", why()))),
        s => Err(AskError::Down(format!("HTTP {s}"))),
    }
}

/// How each source has answered since Venuta started.
static HEALTH: Mutex<Vec<(String, Health)>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, PartialEq)]
enum Health {
    Answering,
    Refused(String),
    Down(String),
}

fn note(source: &str, answer: &Result<Option<Found>, AskError>) {
    let h = match answer {
        Ok(_) => Health::Answering,
        Err(AskError::Refused(why)) => Health::Refused(why.clone()),
        Err(AskError::Down(why)) => Health::Down(why.clone()),
    };
    if let Ok(mut all) = HEALTH.lock() {
        all.retain(|(s, _)| s != source);
        all.push((source.to_owned(), h));
    }
}

/// Why a source turned Venuta away this session, when it did.
fn refused(source: &str) -> Option<String> {
    HEALTH.lock().ok()?.iter().find_map(|(s, h)| match h {
        Health::Refused(why) if s == source => Some(why.clone()),
        _ => None,
    })
}

/// How a source has answered since Venuta started, for Settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceHealth {
    pub source: String,
    pub label: String,
    /// "answering", "refused" (not asked again until Venuta restarts), "down" (didn't answer
    /// the last time) or "unknown" (not asked yet).
    pub state: String,
    /// What the source said, when it said why.
    pub why: Option<String>,
}

/// Every source's health, in the default order.
pub fn health() -> Vec<SourceHealth> {
    let all = HEALTH.lock().map(|h| h.clone()).unwrap_or_default();
    SOURCES
        .iter()
        .map(|(id, label)| {
            let h = all.iter().find(|(s, _)| s == id).map(|(_, h)| h);
            let (state, why) = match h {
                None => ("unknown", None),
                Some(Health::Answering) => ("answering", None),
                Some(Health::Refused(w)) => ("refused", Some(w.clone())),
                Some(Health::Down(w)) => ("down", Some(w.clone())),
            };
            SourceHealth {
                source: (*id).into(),
                label: (*label).into(),
                state: state.into(),
                why,
            }
        })
        .collect()
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

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

/// TTML (Apple's lyrics, and AMLL's): lines, words and the second lines they carry
/// ([`crate::lyrics::ttml`]).
pub fn parse_ttml(xml: &str) -> Vec<LyricLine> {
    ttml::read(xml, "").0.lines
}

/// LyricsPlus's syllables (`[{time, duration, text}]`) as timed words.
fn kpoe_syllables(v: Option<&Value>) -> Vec<Word> {
    v.and_then(Value::as_array)
        .map(|ws| {
            ws.iter()
                .filter_map(|w| {
                    let ms = w.get("time").and_then(Value::as_u64)?;
                    let d = w.get("duration").and_then(Value::as_u64).unwrap_or(0);
                    Some(Word {
                        start_ms: ms,
                        end_ms: ms + d,
                        text: w.get("text").and_then(Value::as_str)?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A second line's text and language as LyricsPlus sends them (`{lang, text}`), when it has
/// words.
fn kpoe_second(v: Option<&Value>) -> Option<(String, Option<&str>)> {
    let v = v?;
    let text = v
        .get("text")
        .and_then(Value::as_str)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let lang = v
        .get("lang")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|l| !l.is_empty());
    (!text.is_empty()).then_some((text, lang))
}

/// LyricsPlus's own JSON: lines with their time, syllables when it times words, and each
/// line's translation and transliteration when the lyrics it relays carry them (Apple's,
/// with their language; a transliteration timed by its own syllables). `lang` is the song's.
fn parse_kpoe(v: &Value, lang: Option<&str>) -> Vec<LyricLine> {
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
                    let words = if word_timed {
                        kpoe_syllables(l.get("syllabus"))
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
                    if text.is_empty() {
                        return None;
                    }
                    let script = crate::lyrics::script_of(&text);
                    let roman = kpoe_second(l.get("transliteration"))
                        .filter(|(r, _)| *r != text && script != "Latn")
                        .map(|(r, rl)| Second {
                            words: kpoe_syllables(
                                l.get("transliteration").and_then(|t| t.get("syllabus")),
                            ),
                            lang: rl.map_or_else(|| roman_lang(lang, &script), str::to_owned),
                            text: r,
                            source: SecondSource::Lyrics("lyricsplus".into()),
                        });
                    let translation = kpoe_second(l.get("translation"))
                        .filter(|(t, _)| *t != text)
                        .map(|(t, tl)| {
                            let mut s = carried_translation(&t, "lyricsplus");
                            if let Some(tl) = tl {
                                s.lang = tl.to_owned();
                            }
                            s
                        });
                    Some(LyricLine {
                        start_ms: ms,
                        end_ms: ms + duration,
                        text,
                        words,
                        script,
                        roman,
                        translation,
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
    let (lyrics, text) = if body.starts_with('<') {
        ttml::read(body, source)
    } else {
        let mut lines = read_lrc(body).lines;
        mark_source(&mut lines, source);
        let text = if lines.is_empty() {
            body.to_owned()
        } else {
            String::new()
        };
        (
            Lyrics {
                lines,
                ..Default::default()
            },
            text,
        )
    };
    let f = Found {
        source: source.into(),
        lyrics,
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

/// A second LRC (a translation, or Roman letters) as its timed rows.
fn rows(lrc: Option<&str>) -> Vec<(u64, String)> {
    lrc.map(|t| {
        read_lrc(t)
            .lines
            .into_iter()
            .filter(|l| !l.text.trim().is_empty())
            .map(|l| (l.start_ms, l.text))
            .collect()
    })
    .unwrap_or_default()
}

/// The row nearest `ms`, at most `within` ms from it.
fn row_near(rows: &[(u64, String)], ms: u64, within: u64) -> Option<&str> {
    rows.iter()
        .filter(|(at, _)| at.abs_diff(ms) <= within)
        .min_by_key(|(at, _)| at.abs_diff(ms))
        .map(|(_, t)| t.as_str())
}

/// A line's Roman letters as a source sends them (NetEase's, AMLL's: syllable by syllable for
/// Japanese and Korean), joined into words by the line and timed by its words when they are;
/// as sent when they can't be joined. None when they say nothing new.
fn roman_from(l: &LyricLine, spaced: &str, provider: &str, lang: Option<&str>) -> Option<Second> {
    let sent = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if sent.is_empty() || sent == l.text {
        return None;
    }
    let original: String = if l.words.is_empty() {
        l.text.clone()
    } else {
        l.words.iter().map(|w| w.text.as_str()).collect()
    };
    let (text, words) = match romaji::join_syllables_at(&original, &sent) {
        Some(joined) => (
            joined
                .iter()
                .map(|(w, _)| w.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            timed_by_chars(&l.words, &joined),
        ),
        None => (sent, Vec::new()),
    };
    Some(Second {
        text,
        words,
        lang: roman_lang(lang, &l.script),
        source: SecondSource::Lyrics(provider.to_owned()),
    })
}

/// A language as Kugou names it ("日语"), as BCP 47.
fn kugou_language(name: &str) -> Option<String> {
    Some(
        match name.trim() {
            "日语" => "ja",
            "韩语" => "ko",
            "英语" => "en",
            "国语" | "普通话" | "华语" => "zh",
            "粤语" => "yue",
            "泰语" => "th",
            _ => return None,
        }
        .to_owned(),
    )
}

/// Kugou's KRC key (the format is XOR'd with it, after a "krc1" mark, then zlib-compressed).
const KRC_KEY: [u8; 16] = [
    64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105,
];

/// A KRC download's text.
fn krc_text(content: &str) -> Option<String> {
    use std::io::Read as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content.trim())
        .ok()?;
    let body = bytes.strip_prefix(b"krc1")?;
    let plain: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ KRC_KEY[i % KRC_KEY.len()])
        .collect();
    let mut text = String::new();
    flate2::read::ZlibDecoder::new(&plain[..])
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// KRC: `[start,length]` lines of `<offset,length,0>word` words, and a `[language:]` block
/// (base64 JSON) holding each line's Roman letters word by word (type 0) and its translation
/// (type 1).
fn parse_krc(text: &str, lang: Option<&str>) -> Vec<LyricLine> {
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut romans: Vec<Vec<String>> = Vec::new();
    let mut translations: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim().trim_start_matches('﻿');
        let Some(r) = raw.strip_prefix('[') else {
            continue;
        };
        let Some((head, body)) = r.split_once(']') else {
            continue;
        };
        if let Some(block) = head.strip_prefix("language:") {
            let json: Option<Value> = base64::engine::general_purpose::STANDARD
                .decode(block.trim())
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            for part in json
                .as_ref()
                .and_then(|j| j.get("content"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let rows: Vec<Vec<String>> = part
                    .get("lyricContent")
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .map(|row| {
                                row.as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|x| x.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                match part.get("type").and_then(Value::as_u64) {
                    Some(0) => romans = rows,
                    Some(1) => translations = rows.into_iter().map(|r| r.concat()).collect(),
                    _ => {}
                }
            }
            continue;
        }
        let Some((start, length)) = head.split_once(',') else {
            continue;
        };
        let (Ok(start), Ok(length)) = (start.trim().parse::<u64>(), length.trim().parse::<u64>())
        else {
            continue;
        };
        let mut words = Vec::new();
        for seg in body.split('<').skip(1) {
            let Some((stamp, word)) = seg.split_once('>') else {
                continue;
            };
            let mut n = stamp.split(',').map(|x| x.trim().parse::<u64>());
            let (Some(Ok(off)), Some(Ok(len))) = (n.next(), n.next()) else {
                continue;
            };
            words.push(Word {
                start_ms: start + off,
                end_ms: start + off + len,
                text: word.to_owned(),
            });
        }
        let text = words.iter().map(|w| w.text.as_str()).collect::<String>();
        lines.push(LyricLine {
            start_ms: start,
            end_ms: start + length,
            text: text.split_whitespace().collect::<Vec<_>>().join(" "),
            words,
            ..Default::default()
        });
    }
    finish(&mut lines);
    for (i, l) in lines.iter_mut().enumerate() {
        // Roman letters word by word, timed by the original's words.
        if let Some(row) = romans.get(i).filter(|r| r.len() == l.words.len()) {
            let parts: Vec<(&str, &str)> = l
                .words
                .iter()
                .zip(row)
                .map(|(w, r)| (w.text.as_str(), r.as_str()))
                .collect();
            let joined = romaji::join_parts(&parts);
            let text = joined
                .iter()
                .map(|j| j.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if !text.trim().is_empty() && text != l.text {
                l.roman = Some(Second {
                    words: joined
                        .iter()
                        .map(|j| Word {
                            start_ms: l.words[j.first].start_ms,
                            end_ms: l.words[j.last].end_ms,
                            text: j.text.clone(),
                        })
                        .collect(),
                    text,
                    lang: roman_lang(lang, &l.script),
                    source: SecondSource::Lyrics("kugou".into()),
                });
            }
        }
        if let Some(t) = translations.get(i).filter(|t| !t.trim().is_empty()) {
            l.translation = Some(carried_translation(t, "kugou"));
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

/// An answer's body as JSON, read by [`answer_of`].
fn json_of(mut resp: ureq::http::Response<ureq::Body>) -> Result<Option<Value>, AskError> {
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    match answer_of(status, &body)? {
        Some(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| AskError::Down(e.to_string())),
        None => Ok(None),
    }
}

/// GET JSON; None for "not there" (404), an error when the source turned Venuta away or had
/// trouble.
fn get_json(
    agent: &ureq::Agent,
    url: &str,
    referer: Option<&str>,
) -> Result<Option<Value>, AskError> {
    if crate::net::is_offline() {
        return Err(AskError::Down("Local only is on".into()));
    }
    let mut req = agent.get(url);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    json_of(req.call().map_err(|e| e.to_string())?)
}

/// GET text; None for "not there" (404), as [`get_json`].
fn get_text(agent: &ureq::Agent, url: &str) -> Result<Option<String>, AskError> {
    if crate::net::is_offline() {
        return Err(AskError::Down("Local only is on".into()));
    }
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    answer_of(status, &body)
}

fn artists_line(req: &LyricsRequest) -> String {
    req.artists.join(", ")
}

fn duration_s(req: &LyricsRequest) -> Option<u64> {
    (req.duration_ms > 0).then(|| (req.duration_ms as f64 / 1000.0).round() as u64)
}

fn lrclib(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
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

fn simpmusic(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
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

fn lyricsplus(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
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
    Ok(lyricsplus_found(req, &v))
}

/// A length as LyricsPlus sends it: ms as a number, or text ("3:33.234", "1:02:03.5", "45.25").
fn total_ms(v: &Value) -> Option<u64> {
    if let Some(ms) = v.as_f64() {
        return (ms >= 0.0).then(|| ms.round() as u64);
    }
    let mut secs = 0.0;
    for part in v.as_str()?.trim().split(':') {
        let part = part.trim();
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return None;
        }
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some((secs * 1000.0).round() as u64)
}

/// LyricsPlus's answer, when it is the song asked about.
fn lyricsplus_found(req: &LyricsRequest, v: &Value) -> Option<Found> {
    // Its length is in its metadata: another recording's lyrics are not these.
    if let Some(total) = v.pointer("/metadata/totalDuration").and_then(total_ms)
        && req.duration_ms > 0
        && (total as f64 - req.duration_ms as f64).abs() / 1000.0 > SAME_LENGTH_S
    {
        return None;
    }
    // The song's language, as LyricsPlus labels it ("ja", "hi").
    let lang = v
        .pointer("/metadata/language")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned);
    let lines = parse_kpoe(v, lang.as_deref());
    (!lines.is_empty()).then(|| Found {
        source: "lyricsplus".into(),
        lyrics: Lyrics {
            lines,
            lang,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn boidu(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
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

/// The song's NetEase id, from NetEase's search: the closest in length of the songs that are
/// the one asked about.
fn netease_id(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<i64>, AskError> {
    let q = format!(
        "{} {}",
        search_title(&req.title),
        req.artists.first().cloned().unwrap_or_default()
    );
    if crate::net::is_offline() {
        return Err(AskError::Down("Local only is on".into()));
    }
    let resp = agent
        .post("https://music.163.com/api/search/get")
        .header("Referer", "https://music.163.com/")
        .send_form([
            ("s", q.as_str()),
            ("type", "1"),
            ("limit", "5"),
            ("offset", "0"),
        ])
        .map_err(|e| e.to_string())?;
    let Some(v) = json_of(resp)? else {
        return Ok(None);
    };
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
    Ok(closest(req, &songs)
        .and_then(|s| s.get("id"))
        .and_then(Value::as_i64))
}

fn netease(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
    let Some(id) = netease_id(agent, req)? else {
        return Ok(None);
    };
    netease_lyrics(req, id, |url| {
        get_json(agent, url, Some("https://music.163.com/"))
    })
}

/// AMLL's lyrics library (amll-ttml-db, CC0: TTML written by its contributors, every word
/// timed, many Asian songs with their Roman letters and a Chinese translation), found by the
/// song's NetEase id.
fn amll(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
    let Some(id) = netease_id(agent, req)? else {
        return Ok(None);
    };
    amll_lyrics(req, id, |url| get_text(agent, url))
}

/// AMLL's TTML for a song by its NetEase id (None: the library hasn't it).
fn amll_lyrics(
    req: &LyricsRequest,
    id: i64,
    ask: impl Fn(&str) -> Result<Option<String>, AskError>,
) -> Result<Option<Found>, AskError> {
    let url = format!(
        "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/ncm-lyrics/{id}.ttml"
    );
    Ok(ask(&url)?.and_then(|ttml| amll_found(req, &ttml)))
}

/// AMLL's TTML, read, with its contributor credited.
fn amll_found(req: &LyricsRequest, ttml: &str) -> Option<Found> {
    let mut f = found_from_text("amll", ttml)?;
    // <amll:meta key="ttmlAuthorGithubLogin" value="someone"/>
    let author = ttml
        .split("<amll:meta ")
        .skip(1)
        .filter_map(|m| m.split_once("/>").map(|(attrs, _)| attrs))
        .find(|attrs| attrs.contains("key=\"ttmlAuthorGithubLogin\""))
        .and_then(|attrs| attrs.split("value=\"").nth(1))
        .and_then(|v| v.split('"').next())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    f.credit = Some(match author {
        Some(a) => format!("Timed by {a} for the AMLL TTML DB"),
        None => "From the AMLL TTML DB".into(),
    });
    credits_out(f, req)
}

/// NetEase's credit lines, sent as JSON lines (`{"t":0,"c":[{"tx":"作词: "},{"tx":"Ayase"}]}`)
/// among its `lrc` and `yrc` lines by the newer call: taken out as text ("作词: Ayase"), the
/// rest of the lyrics kept.
fn netease_credits(text: &str) -> (String, Vec<String>) {
    let mut kept = String::with_capacity(text.len());
    let mut credits = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('{') {
            let said = serde_json::from_str::<Value>(t).ok().and_then(|v| {
                v.get("c").and_then(Value::as_array).map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p.get("tx").and_then(Value::as_str))
                        .collect::<String>()
                })
            });
            if let Some(said) = said.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()) {
                credits.push(said);
            }
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    (kept, credits)
}

/// NetEase's word-timed lyrics (`yrc`): `[start,length]` lines of `(start,length,0)word`
/// words, times in ms.
fn read_yrc(text: &str) -> Vec<LyricLine> {
    static STAMP: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\((\d+),(\d+),-?\d+\)").expect("a valid pattern")
    });
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Some((head, body)) = raw.trim().strip_prefix('[').and_then(|r| r.split_once(']'))
        else {
            continue;
        };
        let Some((start, length)) = head.split_once(',') else {
            continue;
        };
        let (Ok(start), Ok(length)) = (start.trim().parse::<u64>(), length.trim().parse::<u64>())
        else {
            continue;
        };
        let stamps: Vec<regex::Captures> = STAMP.captures_iter(body).collect();
        let mut words: Vec<Word> = Vec::new();
        for (k, c) in stamps.iter().enumerate() {
            let (Some(whole), Ok(at), Ok(long)) =
                (c.get(0), c[1].parse::<u64>(), c[2].parse::<u64>())
            else {
                continue;
            };
            let to = stamps
                .get(k + 1)
                .and_then(|n| n.get(0))
                .map_or(body.len(), |n| n.start());
            let text = &body[whole.end()..to];
            if !text.is_empty() {
                words.push(Word {
                    start_ms: at,
                    end_ms: at + long,
                    text: text.to_owned(),
                });
            }
        }
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !text.is_empty() {
            lines.push(LyricLine {
                start_ms: start,
                end_ms: start + length,
                text,
                words,
                ..Default::default()
            });
        }
    }
    lines.sort_by_key(|l| l.start_ms);
    finish(&mut lines);
    lines
}

/// Whether two copies of a line have the same words (letters and digits, any case).
fn same_words(a: &str, b: &str) -> bool {
    let f = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    f(a) == f(b)
}

/// For each word-timed line, the line-timed copy of it (the same words, at most 1.5 s away,
/// nearest, in order), so what NetEase stamped at the line-timed lines finds its line.
fn lines_met(timed: &[LyricLine], lines: &[LyricLine]) -> Vec<Option<usize>> {
    let mut next = 0;
    timed
        .iter()
        .map(|t| {
            let j = (next..lines.len())
                .filter(|&j| {
                    lines[j].start_ms.abs_diff(t.start_ms) <= 1_500
                        && same_words(&lines[j].text, &t.text)
                })
                .min_by_key(|&j| lines[j].start_ms.abs_diff(t.start_ms))?;
            next = j + 1;
            Some(j)
        })
        .collect()
}

/// NetEase's lyrics answer, from the newer call (`song/lyric/v1`) or the older one
/// (`song/lyric`): the lines (word by word from `yrc` when it has them), their translations
/// (Chinese; `ytlrc` stamped at the word-timed lines, `tlyric` at the line-timed ones) and
/// Roman letters (`yromalrc`, `romalrc`), and the credits it sends among them.
fn netease_found(req: &LyricsRequest, l: &Value) -> Option<Found> {
    let field = |k: &str| {
        l.pointer(&format!("/{k}/lyric"))
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
    };
    let (lrc, mut credits) = netease_credits(field("lrc").unwrap_or(""));
    let (yrc, yrc_credits) = netease_credits(field("yrc").unwrap_or(""));
    if credits.is_empty() {
        credits = yrc_credits;
    }
    let by_line = found_from_text("netease", &lrc);
    let line_timed: Vec<LyricLine> = by_line
        .as_ref()
        .map(|f| f.lyrics.lines.clone())
        .unwrap_or_default();
    let word_timed = read_yrc(&yrc);
    let by_word = !word_timed.is_empty();
    let (tl, ro) = (rows(field("tlyric")), rows(field("romalrc")));
    let (ytl, yro) = (rows(field("ytlrc")), rows(field("yromalrc")));
    let mut f = if by_word {
        Found {
            source: "netease".into(),
            lyrics: Lyrics {
                lines: word_timed,
                ..Default::default()
            },
            ..Default::default()
        }
    } else {
        by_line?
    };
    // Each line's line-timed copy, where `tlyric` and `romalrc` are stamped.
    let met: Vec<Option<usize>> = if by_word {
        lines_met(&f.lyrics.lines, &line_timed)
    } else {
        (0..f.lyrics.lines.len()).map(Some).collect()
    };
    for (line, met) in f.lyrics.lines.iter_mut().zip(met) {
        let at = met.map(|j| line_timed[j].start_ms);
        let second = |by_word_rows: &[(u64, String)], by_line_rows: &[(u64, String)]| {
            by_word
                .then(|| row_near(by_word_rows, line.start_ms, 100))
                .flatten()
                .or_else(|| at.and_then(|ms| row_near(by_line_rows, ms, 0)))
                .map(str::to_owned)
        };
        let translated = second(&ytl, &tl);
        let spaced = second(&yro, &ro);
        if let Some(t) = translated.filter(|t| *t != line.text) {
            line.translation = Some(carried_translation(&t, "netease"));
        } else if line.translation.is_none() {
            // A translation the line-timed copy carried itself (a line at the same time).
            line.translation = met.and_then(|j| line_timed[j].translation.clone());
        }
        if let Some(r) = spaced.and_then(|r| roman_from(line, &r, "netease", None)) {
            line.roman = Some(r);
        }
    }
    f.credits = credits;
    credits_out(f, req)
}

/// NetEase's lyrics for a song by its id: the newer call first (as LX Music asks it; it adds
/// the word-timed lyrics), the older one when it has nothing or fails. Asked without its
/// encryption, it answers the same (checked on 2026-10-22 for three songs).
fn netease_lyrics(
    req: &LyricsRequest,
    id: i64,
    ask: impl Fn(&str) -> Result<Option<Value>, AskError>,
) -> Result<Option<Found>, AskError> {
    let v1 = format!(
        "https://music.163.com/api/song/lyric/v1?id={id}&cp=false&tv=0&lv=0&rv=0&kv=0&yv=0&ytv=0&yrv=0"
    );
    if let Ok(Some(v)) = ask(&v1)
        && let Some(f) = netease_found(req, &v)
    {
        return Ok(Some(f));
    }
    let old = format!("https://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1&rv=-1");
    Ok(ask(&old)?.and_then(|v| netease_found(req, &v)))
}

fn qq(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
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
    Ok(qq_found(req, &l))
}

/// QQ Music's lyrics answer (`fcg_query_lyric_new`).
fn qq_found(req: &LyricsRequest, l: &Value) -> Option<Found> {
    let text = |key: &str| {
        let raw = l
            .get(key)
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())?;
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_else(|| unescape(raw)),
        )
    };
    let mut f = found_from_text("qq", &text("lyric")?)?;
    // Its translation, when it has one, is a second LRC at the same times.
    if let Some(tr) = text("trans") {
        f.lyrics.lines = with_translation(f.lyrics.lines, &tr, "qq");
    }
    credits_out(f, req)
}

fn kugou(agent: &ureq::Agent, req: &LyricsRequest) -> Result<Option<Found>, AskError> {
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
    let lang = first
        .get("language")
        .and_then(Value::as_str)
        .and_then(kugou_language);
    let download = |fmt: &str| {
        get_json(
            agent,
            &format!(
                "https://lyrics.kugou.com/download?ver=1&client=pc&id={}&accesskey={}&fmt={fmt}",
                enc(&id),
                enc(key)
            ),
            None,
        )
    };
    // KRC times every word and carries Roman letters and translations; LRC when it can't be read.
    if let Some(d) = download("krc")?
        && let Some(f) = kugou_krc_found(req, &d, lang.as_deref())
    {
        return Ok(Some(f));
    }
    let Some(d) = download("lrc")? else {
        return Ok(None);
    };
    Ok(kugou_found(req, &d).map(|mut f| {
        f.lyrics.lang = lang;
        f
    }))
}

/// Kugou's KRC download (`fmt=krc`): every word timed, with Roman letters and translations
/// when it has them.
fn kugou_krc_found(req: &LyricsRequest, d: &Value, lang: Option<&str>) -> Option<Found> {
    let text = krc_text(d.get("content").and_then(Value::as_str)?)?;
    let lines = parse_krc(&text, lang);
    if lines.is_empty() {
        return None;
    }
    let f = Found {
        source: "kugou".into(),
        lyrics: Lyrics {
            lines,
            lang: lang.map(str::to_owned),
            ..Default::default()
        },
        ..Default::default()
    };
    credits_out(f, req)
}

/// Kugou's lyrics download (`fmt=lrc`).
fn kugou_found(req: &LyricsRequest, d: &Value) -> Option<Found> {
    let content = d.get("content").and_then(Value::as_str)?;
    let text = base64::engine::general_purpose::STANDARD
        .decode(content.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())?;
    let f = found_from_text("kugou", &text)?;
    credits_out(f, req)
}

/// The lines that name who made the song ("作词 : Ayase", and the "Artist - Title" line some
/// sources start with) taken out of the sung lines, and kept as credits.
fn credits_out(mut f: Found, req: &LyricsRequest) -> Option<Found> {
    let lines = std::mem::take(&mut f.lyrics.lines);
    let title = fold(&search_title(&req.title));
    let artists: Vec<String> = req
        .artists
        .iter()
        .map(|a| fold(a))
        .filter(|a| a.chars().count() >= 3)
        .collect();
    // Only before the first sung line: a dash further in is part of the words.
    let mut leading = true;
    let mut sung = Vec::new();
    for l in lines {
        let names_song = || {
            let f = fold(&l.text);
            l.text.contains(" - ")
                && ((title.chars().count() >= 2 && f.contains(&title))
                    || artists.iter().any(|a| f.contains(a.as_str())))
        };
        if crate::lyrics::is_credit(&l.text) || (leading && names_song()) {
            f.credits.push(l.text.trim().to_owned());
        } else {
            leading &= l.text.trim().is_empty();
            sung.push(l);
        }
    }
    f.lyrics.lines = sung;
    f.usable().then_some(f)
}

/// The catalog's own lyrics, asked of the app's catalog client (see the module notes).
pub type CatalogAsk<'a> = &'a (dyn Fn(&LyricsRequest) -> Result<Option<Found>, String> + Sync);

/// Ask one source. One that turned Venuta away this session isn't asked again.
pub fn ask(
    source: &str,
    req: &LyricsRequest,
    catalog: CatalogAsk,
) -> Result<Option<Found>, AskError> {
    if let Some(why) = refused(source) {
        return Err(AskError::Refused(why));
    }
    if crate::net::is_offline() && source != "youtube" {
        return Err(AskError::Down("Local only is on".into()));
    }
    let agent = agent();
    let answer = match source {
        "lrclib" => lrclib(&agent, req),
        "youtube" => catalog(req).map_err(AskError::Down),
        "simpmusic" => simpmusic(&agent, req),
        "lyricsplus" => lyricsplus(&agent, req),
        "boidu" => boidu(&agent, req),
        "netease" => netease(&agent, req),
        "amll" => amll(&agent, req),
        "qq" => qq(&agent, req),
        "kugou" => kugou(&agent, req),
        other => return Err(AskError::Down(format!("no lyrics source called {other}"))),
    };
    if !crate::net::is_offline() {
        note(source, &answer);
    }
    answer.map(|found| {
        found.map(|mut f| {
            crate::lyrics::add_pronunciation(&mut f.lyrics);
            f
        })
    })
}

/// Every source's answer so far (None: not in yet) as Automatic's: the first timed one in your
/// order (see [`choose`]), else "no lyrics" only when every source said so. When one turned
/// Venuta away, had trouble or never answered, there is no answer (and nothing to keep).
pub fn outcome(
    answers: &[Option<Result<Option<Found>, AskError>>],
    all_in: bool,
) -> Option<Result<Option<Found>, AskError>> {
    let plain: Vec<Option<Option<Found>>> = answers
        .iter()
        .map(|a| a.as_ref().map(|r| r.as_ref().ok().cloned().flatten()))
        .collect();
    match choose(&plain, all_in)? {
        Some(f) => Some(Ok(Some(f))),
        None => {
            let troubled = answers.iter().find_map(|a| match a {
                None => Some(AskError::Down("Some sources didn't answer in time".into())),
                Some(Err(e)) => Some(e.clone()),
                Some(Ok(_)) => None,
            });
            Some(troubled.map_or(Ok(None), Err))
        }
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

/// Automatic: every source in `sources` asked at once (each a moment after the one before),
/// the first timed answer in that order taken. Sources that turned Venuta away this session are
/// left out. An error when nothing was found and a source didn't answer (so the miss isn't
/// sure).
pub fn automatic(
    req: &LyricsRequest,
    sources: &[&str],
    catalog: CatalogAsk,
) -> Result<Option<Found>, AskError> {
    let sources: Vec<&str> = sources
        .iter()
        .copied()
        .filter(|s| refused(s).is_none())
        .collect();
    if sources.is_empty() {
        return Ok(None);
    }
    type Answer = Result<Option<Found>, AskError>;
    let (tx, rx) = mpsc::channel::<(usize, Answer)>();
    std::thread::scope(|scope| {
        for (i, s) in sources.iter().enumerate() {
            let tx = tx.clone();
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(150 * i as u64));
                let _ = tx.send((i, ask(s, req, catalog)));
            });
        }
        drop(tx);
        let started = Instant::now();
        let mut answers: Vec<Option<Answer>> = vec![None; sources.len()];
        let pick = loop {
            if let Some(pick) = outcome(&answers, false) {
                break pick;
            }
            let left = ALL_TIMEOUT.saturating_sub(started.elapsed());
            match rx.recv_timeout(left) {
                Ok((i, f)) => answers[i] = Some(f),
                Err(_) => {
                    break outcome(&answers, true)
                        .unwrap_or_else(|| Err(AskError::Down("No source answered".into())));
                }
            }
        };
        let Ok(Some(found)) = pick else {
            return pick;
        };
        if wants_pairing(&found) {
            // A copy of the song in Latin letters may be among the other answers. Every source
            // was asked at once and the scope waits for each anyway, so waiting for them here
            // costs no time.
            while answers.iter().any(Option::is_none) {
                let left = ALL_TIMEOUT.saturating_sub(started.elapsed());
                match rx.recv_timeout(left) {
                    Ok((i, f)) => answers[i] = Some(f),
                    Err(_) => break,
                }
            }
        }
        Ok(Some(paired_pick(found, &answers)))
    })
}

/// Automatic's pick, its lines in another script paired with a copy of the song in Latin
/// letters among the other answers, in your order, when it wants one ([`pair_from`]).
fn paired_pick(mut found: Found, answers: &[Option<Result<Option<Found>, AskError>>]) -> Found {
    if wants_pairing(&found) {
        let others: Vec<&Found> = answers
            .iter()
            .flatten()
            .filter_map(|a| a.as_ref().ok()?.as_ref())
            .collect();
        pair_from(&mut found, &others);
    }
    found
}

/// Whether an answer has lines in another script still wanting a pronunciation from the
/// lyrics (none yet, or only one made on the device): then a copy in Latin letters is looked
/// for among the other answers.
fn wants_pairing(f: &Found) -> bool {
    f.synced()
        && f.lyrics
            .lines
            .iter()
            .filter(|l| {
                !matches!(l.script.as_str(), "Latn" | "Zyyy" | "")
                    && l.roman
                        .as_ref()
                        .is_none_or(|r| r.source == SecondSource::Made)
            })
            .count()
            >= 3
}

/// The answer's lines in another script paired with a copy of the song in Latin letters from
/// another source's answer (the first that pairs, in the order given): that copy's lines
/// become their pronunciation, `Paired(source)` (see [`crate::lyrics::pair`]). The source,
/// when one paired.
pub fn pair_from(found: &mut Found, others: &[&Found]) -> Option<String> {
    others
        .iter()
        .filter(|o| o.source != found.source && o.synced())
        .find(|o| {
            crate::lyrics::pair::pair_roman(
                &mut found.lyrics.lines,
                found.lyrics.lang.as_deref(),
                &o.lyrics.lines,
                &o.source,
            )
            .is_some()
        })
        .map(|o| o.source.clone())
}

/// The answer to show after asking again about a song whose kept answer was in an older
/// format (`stale`), and what is kept: a new answer replaces it; when the source now has
/// nothing, the old one stays (in today's format, so it isn't asked about again); when the
/// source didn't answer, the old one is shown and the song is asked about again next time.
pub fn renewed(
    kept: &Kept,
    song: &str,
    slot: &str,
    answer: Result<Option<Found>, AskError>,
    stale: Option<Found>,
) -> Option<Found> {
    match (answer, stale) {
        (Ok(None), Some(old)) => {
            kept.put(song, slot, Some(&old));
            Some(old)
        }
        (Err(_), old) => old,
        (answer, _) => {
            remember(kept, song, slot, &answer);
            answer.ok().flatten()
        }
    }
}

/// Keep an answer for a song (`slot`: the source, or "auto"): what was found, or that there
/// is none. A source that turned Venuta away or didn't answer is not saying "none": nothing is
/// kept, so the song is asked about again.
pub fn remember(kept: &Kept, song: &str, slot: &str, answer: &Result<Option<Found>, AskError>) {
    if let Ok(found) = answer {
        kept.put(song, slot, found.as_ref());
    }
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
        Self::open_at(&dir.join("cache.sqlite"))
    }

    /// Kept answers in the given database file.
    pub fn open_at(file: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(file).map_err(|e| e.to_string())?;
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

    fn row(&self, song: &str, source: &str) -> Option<(Option<String>, i64)> {
        self.conn
            .query_row(
                "SELECT found, at FROM lyrics WHERE song = ?1 AND source = ?2",
                params![song, source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// A kept answer, read in today's shape (old rows get their line ends and scripts).
    fn read(json: &str) -> Option<Found> {
        let mut f: Found = serde_json::from_str(json).ok()?;
        finish(&mut f.lyrics.lines);
        crate::lyrics::add_pronunciation(&mut f.lyrics);
        Some(f)
    }

    /// Some(answer) when one is kept and still good: found in today's answer format
    /// ([`crate::lyrics::VERSION`]), or "none" from the last day.
    pub fn get(&self, song: &str, source: &str) -> Option<Option<Found>> {
        match self.row(song, source)? {
            (Some(json), _) => Self::read(&json)
                .filter(|f| f.lyrics.version >= crate::lyrics::VERSION)
                .map(Some),
            (None, at) if now_ms() - at < MISS_TTL_MS => Some(None),
            _ => None,
        }
    }

    /// A found answer kept in an older answer format: asked for again, and shown meanwhile or
    /// when the source can't answer now.
    pub fn stale(&self, song: &str, source: &str) -> Option<Found> {
        let (json, _) = self.row(song, source)?;
        Self::read(&json?).filter(|f| f.lyrics.version < crate::lyrics::VERSION)
    }

    /// Keep an answer (None: there is none), in today's answer format.
    pub fn put(&self, song: &str, source: &str, found: Option<&Found>) {
        let json = found.and_then(|f| {
            let mut f = f.clone();
            f.lyrics.version = crate::lyrics::VERSION;
            serde_json::to_string(&f).ok()
        });
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
        let offline = Err(AskError::Down("Local only is on".to_owned()));
        assert_eq!(refused, offline);
        assert_ne!(
            get_json(&agent(), "http://127.0.0.1:9/lyrics", None),
            offline
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

    #[test]
    fn a_source_that_turns_venuta_away_is_not_a_song_without_lyrics() {
        // SimpMusic and Better Lyrics as they answered on 2026-10-21.
        assert_eq!(
            answer_of(
                403,
                include_str!("../tests/fixtures/lyrics/simpmusic-403.json")
            ),
            Err(AskError::Refused(
                "This API is only available to SimpMusic clients".into()
            ))
        );
        assert_eq!(
            answer_of(
                401,
                include_str!("../tests/fixtures/lyrics/betterlyrics-401.json")
            ),
            Err(AskError::Refused(
                "Uncached queries require a valid API key via X-API-Key header".into()
            ))
        );
        assert_eq!(answer_of(404, ""), Ok(None), "not there: no lyrics");
        assert_eq!(answer_of(200, "{}"), Ok(Some("{}".into())));
        assert!(matches!(answer_of(503, ""), Err(AskError::Down(_))));
        assert!(matches!(answer_of(429, ""), Err(AskError::Down(_))));
    }

    #[test]
    fn a_refusing_source_rests_for_the_session_and_says_so() {
        // A made-up id, so other tests' sources are left alone.
        note(
            "test-refuses",
            &Err(AskError::Refused("API key required".into())),
        );
        let r = ask("test-refuses", &req("Song", &["Someone"], 200), &|_| {
            Ok(None)
        });
        assert_eq!(
            r,
            Err(AskError::Refused("API key required".into())),
            "not asked again"
        );
        note("test-down", &Err(AskError::Down("HTTP 503".into())));
        assert_eq!(refused("test-down"), None, "trouble isn't a refusal");
        note("boidu", &Err(AskError::Refused("API key required".into())));
        let h = health();
        let b = h.iter().find(|h| h.source == "boidu").unwrap();
        assert_eq!(
            (b.state.as_str(), b.why.as_deref()),
            ("refused", Some("API key required"))
        );
        assert_eq!(h.len(), SOURCES.len());
    }

    #[test]
    fn automatic_keeps_a_miss_only_when_every_source_said_none() {
        let none = || Some(Ok(None));
        let refused = || Some(Err(AskError::Refused("no".into())));
        assert_eq!(outcome(&[none(), none()], true), Some(Ok(None)));
        assert!(matches!(outcome(&[none(), refused()], true), Some(Err(_))));
        assert!(
            matches!(outcome(&[none(), None], true), Some(Err(_))),
            "one never answered"
        );
        assert_eq!(outcome(&[none(), None], false), None, "still waiting");
        // A timed answer wins whatever the others did.
        let found = || {
            Some(Ok(Some(Found {
                source: "b".into(),
                lyrics: Lyrics {
                    lines: vec![LyricLine {
                        text: "x".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Default::default()
            })))
        };
        assert!(matches!(
            outcome(&[refused(), found()], true),
            Some(Ok(Some(_)))
        ));

        let dir = tempfile::tempdir().unwrap();
        let kept = Kept::open_at(&dir.path().join("cache.sqlite")).unwrap();
        remember(&kept, "s", "auto", &Err(AskError::Refused("no".into())));
        assert_eq!(
            kept.get("s", "auto"),
            None,
            "a refusal is never kept as a miss"
        );
        remember(&kept, "s", "auto", &Ok(None));
        assert_eq!(kept.get("s", "auto"), Some(None), "a real miss is");
    }

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/lyrics")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn lyricsplus_turns_away_another_recording_by_its_length() {
        // Idol (YOASOBI) as LyricsPlus answered on 2026-10-21: "totalDuration": "3:33.234".
        let v = fixture("lyricsplus-idol.json");
        let idol = |secs| req("アイドル", &["YOASOBI"], secs);
        assert!(
            lyricsplus_found(&idol(213), &v).is_some(),
            "the same length"
        );
        assert!(
            lyricsplus_found(&idol(260), &v).is_none(),
            "47 s longer is another recording"
        );
        assert!(lyricsplus_found(&idol(0), &v).is_some(), "length unknown");
        // The length as a number of ms, as it was sent before.
        let mut n = v.clone();
        n["metadata"]["totalDuration"] = serde_json::json!(213_234);
        assert!(lyricsplus_found(&idol(260), &n).is_none());
        assert_eq!(total_ms(&serde_json::json!("1:02:03.5")), Some(3_723_500));
        assert_eq!(total_ms(&serde_json::json!("45.25")), Some(45_250));
        assert_eq!(total_ms(&serde_json::json!("soon")), None);
    }

    #[test]
    fn lyricsplus_lines_carry_their_translation_and_transliteration() {
        // LyricsPlus sends both per line when the lyrics it relays carry them (its
        // `translation {lang, text}` and `transliteration {lang, text, syllabus}`, from Apple's
        // TTML head). None of the answers captured on 2026-10-21 and 2026-10-22 had them, so
        // they are added here to Idol's, in that shape.
        let mut v = fixture("lyricsplus-idol.json");
        v["lyrics"][0]["transliteration"] = serde_json::json!({
            "lang": "ja-Latn",
            "text": "Muteki no egao de arasu media",
            "syllabus": [
                { "time": 589, "duration": 807, "text": "Muteki no " },
                { "time": 1396, "duration": 470, "text": "egao " },
                { "time": 1866, "duration": 179, "text": "de " },
                { "time": 2045, "duration": 301, "text": "arasu " },
                { "time": 2346, "duration": 851, "text": "media" }
            ]
        });
        v["lyrics"][0]["translation"] = serde_json::json!({
            "lang": "en",
            "text": "Taking the media by storm with an invincible smile"
        });
        v["lyrics"][1]["transliteration"] =
            serde_json::json!({ "lang": "", "text": "Shiritai sono himitsu misuteriasu" });
        v["lyrics"][1]["translation"] = serde_json::json!({ "text": "想知道那个秘密" });
        let idol = req("アイドル", &["YOASOBI"], 213);
        let lp = lyricsplus_found(&idol, &v).unwrap();
        let first = &lp.lyrics.lines[0];
        let r = first.roman.as_ref().unwrap();
        assert_eq!(r.text, "Muteki no egao de arasu media");
        assert_eq!(
            (r.lang.as_str(), &r.source),
            ("ja-Latn", &SecondSource::Lyrics("lyricsplus".into()))
        );
        assert_eq!(
            r.words
                .iter()
                .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (589, 1396, "Muteki no "),
                (1396, 1866, "egao "),
                (1866, 2045, "de "),
                (2045, 2346, "arasu "),
                (2346, 3197, "media")
            ],
            "timed by its own syllables"
        );
        let t = first.translation.as_ref().unwrap();
        assert_eq!(
            (t.text.as_str(), t.lang.as_str()),
            ("Taking the media by storm with an invincible smile", "en")
        );
        let second = &lp.lyrics.lines[1];
        let r = second.roman.as_ref().unwrap();
        assert_eq!(
            (r.text.as_str(), r.lang.as_str(), r.words.len()),
            ("Shiritai sono himitsu misuteriasu", "ja-Latn", 0),
            "no language sent: the song's"
        );
        assert_eq!(
            second.translation.as_ref().map(|t| t.lang.as_str()),
            Some("zh"),
            "no language sent: told by its letters"
        );
        // Kept through the made pronunciation `ask` adds.
        let mut lyrics = lp.lyrics.clone();
        crate::lyrics::add_pronunciation(&mut lyrics);
        assert_eq!(lyrics.lines[0].roman, lp.lyrics.lines[0].roman);

        // A line already in Latin letters gets no pronunciation, whatever is sent.
        let mut v = fixture("pairing/kesariya.lyricsplus.json");
        v["lyrics"][0]["transliteration"] =
            serde_json::json!({ "lang": "hi-Latn", "text": "Mujhko itna bataye koyi" });
        let kesariya = req("Kesariya", &["Arijit Singh"], 268);
        let lp = lyricsplus_found(&kesariya, &v).unwrap();
        assert_eq!(lp.lyrics.lines[0].roman, None);
    }

    #[test]
    fn neteases_newer_call_is_read_like_the_old_one() {
        // Idol as NetEase's `song/lyric/v1` answered on 2026-10-22: its credits come as JSON
        // lines ({"t":0,"c":[{"tx":"作词: "},{"tx":"Ayase"}]}), the rest as before.
        let idol = idol_req();
        let v1 = netease_found(&idol, &fixture("netease-v1-idol.json")).unwrap();
        let old = netease_found(&idol, &fixture("netease-idol.json")).unwrap();
        assert_eq!(
            v1.credits,
            ["作词: Ayase", "作曲: Ayase", "编曲: Ayase", "制作人: Ayase"]
        );
        assert_eq!(texts(&v1), texts(&old), "the same sung lines");
        let first = &v1.lyrics.lines[0];
        assert_eq!(
            first.roman.as_ref().map(|r| r.text.as_str()),
            Some("muteki no egao de arasu media")
        );
        assert_eq!(
            first.translation.as_ref().map(|t| t.lang.as_str()),
            Some("zh-Hans")
        );
        assert_eq!(
            v1.lyrics.lines.iter().filter(|l| l.roman.is_some()).count(),
            old.lyrics
                .lines
                .iter()
                .filter(|l| l.roman.is_some())
                .count()
        );
    }

    #[test]
    fn neteases_word_timed_lyrics_are_the_lines_when_it_has_them() {
        // 孤勇者 (Eason Chan) from `song/lyric/v1` on 2026-10-22: `yrc` times every word.
        let song = req("孤勇者", &["陈奕迅"], 256);
        let f = netease_found(&song, &fixture("netease-v1-guyongzhe.json")).unwrap();
        let sung = f
            .lyrics
            .lines
            .iter()
            .find(|l| l.text == "都 是勇敢的")
            .unwrap();
        assert_eq!((sung.start_ms, sung.end_ms), (22_310, 26_610));
        assert_eq!(
            sung.words
                .iter()
                .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (22_310, 25_190, "都 "),
                (25_190, 25_500, "是"),
                (25_500, 25_790, "勇"),
                (25_790, 26_020, "敢"),
                (26_020, 26_610, "的")
            ]
        );
        assert!(f.lyrics.lines.iter().all(|l| !l.text.contains("\"tx\"")));
        assert!(
            f.credits.contains(&"作词: 唐恬".to_owned()),
            "{:?}",
            f.credits
        );
        // The copyright and production lines it sends after the words are credits too.
        for credit in [
            "词版权管理方：北京梦织音传媒有限公司",
            "曲版权管理方：索尼音乐版权代理（北京）有限公司",
            "录音作品及MV版权：EAS MUSIC LTD",
            "联合出品：拳头游戏/腾讯游戏/腾讯视频",
            "出品监制：霍锦 卢泓宇",
        ] {
            assert!(
                f.credits.iter().any(|c| c == credit),
                "{credit}: {:?}",
                f.credits
            );
        }
        assert!(
            f.lyrics
                .lines
                .iter()
                .all(|l| !l.text.contains("版权") && !l.text.contains("出品")),
            "{:?}",
            texts(&f)
        );
        assert!(
            f.lyrics
                .lines
                .iter()
                .filter(|l| !l.words.is_empty())
                .count()
                > 40
        );

        // With its word-timed translation and Roman letters (`ytlrc`, `yromalrc`, stamped at
        // the word-timed lines' starts, give or take a little), and the line-timed ones
        // (`tlyric`, `romalrc`, stamped at the `lrc` lines') for lines those don't cover. This
        // shape was not in any captured answer, so it is written here.
        let v = serde_json::json!({
            "code": 200,
            "lrc": { "lyric": "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"Ayase\"}]}\n[00:00.270]無敵の笑顔で\n[00:03.250]知りたい\n" },
            "tlyric": { "lyric": "[00:00.270]无敌笑容\n[00:03.250]想知道\n" },
            "romalrc": { "lyric": "[00:00.270]mu te ki no e ga o de\n[00:03.250]shi ri ta i\n" },
            "yrc": { "lyric": "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"Ayase\"}]}\n[273,2800](273,850,0)無敵(1123,180,0)の(1303,600,0)笑顔(1903,190,0)で\n[3283,2600](3283,300,0)知(3583,200,0)り(3783,200,0)た(3983,200,0)い\n" },
            "ytlrc": { "lyric": "[00:00.300]露出无敌笑容\n" },
            "yromalrc": { "lyric": "[00:00.273]mu te ki no e ga o de\n" }
        });
        let f = netease_found(&idol_req(), &v).unwrap();
        assert_eq!(f.credits, ["作词: Ayase"]);
        let first = &f.lyrics.lines[0];
        assert_eq!(first.words.len(), 4, "the word-timed lines are the lines");
        let r = first.roman.as_ref().unwrap();
        assert_eq!(r.text, "muteki no egao de");
        assert_eq!(
            r.words
                .iter()
                .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (273, 1123, "muteki "),
                (1123, 1303, "no "),
                (1303, 1903, "egao "),
                (1903, 2093, "de")
            ],
            "timed by the line's words"
        );
        assert_eq!(
            first.translation.as_ref().map(|t| t.text.as_str()),
            Some("露出无敌笑容"),
            "the word-timed translation first"
        );
        let second = &f.lyrics.lines[1];
        assert_eq!(
            second.translation.as_ref().map(|t| t.text.as_str()),
            Some("想知道"),
            "the line-timed one, met through the same line in `lrc`"
        );
        assert_eq!(
            second.roman.as_ref().map(|r| r.text.as_str()),
            Some("shiritai")
        );
        assert_eq!(second.roman.as_ref().unwrap().words.len(), 1);
    }

    #[test]
    fn amlls_library_by_the_songs_netease_id() {
        let song = req("夜に駆ける", &["YOASOBI"], 261);
        let ttml = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/lyrics/amll-yoru-ni-kakeru.ttml"),
        )
        .unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let f = amll_lyrics(&song, 1409311773, |url| {
            asked.borrow_mut().push(url.to_owned());
            Ok(Some(ttml.clone()))
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            *asked.borrow(),
            [
                "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/ncm-lyrics/1409311773.ttml"
            ]
        );
        assert_eq!(f.source, "amll");
        assert_eq!(
            f.credit.as_deref(),
            Some("Timed by mizuhara37 for the AMLL TTML DB")
        );
        let first = &f.lyrics.lines[0];
        assert!(!first.words.is_empty());
        assert_eq!(
            first.roman.as_ref().map(|r| &r.source),
            Some(&SecondSource::Lyrics("amll".into()))
        );
        assert_eq!(
            first
                .translation
                .as_ref()
                .map(|t| (t.lang.as_str(), &t.source)),
            Some(("zh-CN", &SecondSource::Lyrics("amll".into())))
        );
        // Not in the library: no lyrics, not trouble.
        assert_eq!(amll_lyrics(&song, 1, |_| Ok(None)), Ok(None));
        assert_eq!(label("amll"), "AMLL TTML DB");
        assert!(order(&[]).iter().any(|(id, on)| *id == "amll" && *on));
    }

    fn idol_req() -> LyricsRequest {
        req("アイドル", &["YOASOBI"], 213)
    }

    /// A song's LRCLIB and LyricsPlus answers as captured on 2026-10-22, each as `ask` gives
    /// it (with its made pronunciation).
    fn pairing_answers(song: &str) -> (Found, Found) {
        let lr = fixture(&format!("pairing/{song}.lrclib.json"));
        let mut lrclib = found_from_text("lrclib", lr["syncedLyrics"].as_str().unwrap()).unwrap();
        let lp = fixture(&format!("pairing/{song}.lyricsplus.json"));
        let mut lyricsplus = lyricsplus_found(&LyricsRequest::default(), &lp).unwrap();
        crate::lyrics::add_pronunciation(&mut lrclib.lyrics);
        crate::lyrics::add_pronunciation(&mut lyricsplus.lyrics);
        (lrclib, lyricsplus)
    }

    fn paired(f: &Found) -> Vec<(&str, &str)> {
        f.lyrics
            .lines
            .iter()
            .filter_map(|l| {
                let r = l.roman.as_ref()?;
                matches!(r.source, SecondSource::Paired(_))
                    .then_some((l.text.as_str(), r.text.as_str()))
            })
            .collect()
    }

    fn not_latin(f: &Found) -> usize {
        f.lyrics
            .lines
            .iter()
            .filter(|l| !matches!(l.script.as_str(), "Latn" | "Zyyy"))
            .count()
    }

    #[test]
    fn a_romanized_copy_of_the_song_is_paired_with_the_original_by_time() {
        // Hindi and Punjabi songs whose LRCLIB lyrics are in Devanagari or Gurmukhi, and whose
        // LyricsPlus lyrics are Apple's, in Latin letters.
        for song in ["kesariya", "apna-bana-le", "chaleya", "heeriye"] {
            let (mut lrclib, lyricsplus) = pairing_answers(song);
            let made = lrclib.clone();
            assert_eq!(
                pair_from(&mut lrclib, &[&lyricsplus]).as_deref(),
                Some("lyricsplus"),
                "{song}"
            );
            let (n, of) = (paired(&lrclib).len(), not_latin(&lrclib));
            eprintln!("{song}: {n} of {of} lines paired");
            assert!(n * 10 >= of * 8, "{song}: {n} of {of}");
            // Lines it couldn't pair keep the pronunciation made on the device.
            for (l, m) in lrclib.lyrics.lines.iter().zip(&made.lyrics.lines) {
                if l.roman
                    .as_ref()
                    .is_some_and(|r| r.source == SecondSource::Made)
                {
                    assert_eq!(l.roman, m.roman);
                }
            }
        }
        let (mut lrclib, lyricsplus) = pairing_answers("kesariya");
        pair_from(&mut lrclib, &[&lyricsplus]);
        let first = &lrclib.lyrics.lines[0];
        assert_eq!(first.text, "मुझको इतना बताए कोई");
        let r = first.roman.as_ref().unwrap();
        assert_eq!(r.text, "Mujhko itna bataye koyi");
        assert_eq!(r.source, SecondSource::Paired("lyricsplus".into()));
        assert_eq!(r.lang, "hi-Latn");
        // Apple's word timing, moved to where this copy starts the line (9 440 ms; Apple's
        // starts at 9 615).
        assert_eq!(r.words[0].start_ms, first.start_ms);
        assert_eq!(
            r.words.iter().map(|w| w.text.as_str()).collect::<String>(),
            r.text
        );
        assert!(
            r.words
                .iter()
                .all(|w| w.end_ms <= first.end_ms.max(w.start_ms))
        );

        // Apple splits a line LRCLIB keeps whole: both halves are its pronunciation.
        let (mut lrclib, lyricsplus) = pairing_answers("heeriye");
        pair_from(&mut lrclib, &[&lyricsplus]);
        let whole = lrclib
            .lyrics
            .lines
            .iter()
            .find(|l| l.text == "ਤੇਰੀ ਹੋਕੇ ਮਰਾਂ, ਜਿੰਦ-ਜਾਨ ਕਰਾਂ")
            .unwrap();
        let r = whole.roman.as_ref().unwrap();
        assert_eq!(r.text, "Teri hoke maraan Jind jaan karaan");
        assert_eq!(r.lang, "pa-Latn");
    }

    #[test]
    fn another_song_or_a_translation_at_the_same_times_is_never_paired() {
        let (kesariya, _) = pairing_answers("kesariya");
        let (_, apna) = pairing_answers("apna-bana-le");
        // Another song in Latin letters.
        let mut f = kesariya.clone();
        assert_eq!(pair_from(&mut f, &[&apna]), None);
        assert_eq!(f, kesariya, "left as it was");
        // Another song's words at exactly this song's times.
        let mut retimed = apna.clone();
        retimed.lyrics.lines = kesariya
            .lyrics
            .lines
            .iter()
            .zip(apna.lyrics.lines.iter().cycle())
            .map(|(k, a)| LyricLine {
                start_ms: k.start_ms,
                end_ms: k.end_ms,
                text: a.text.clone(),
                script: a.script.clone(),
                ..Default::default()
            })
            .collect();
        assert_eq!(pair_from(&mut f, &[&retimed]), None);
        // English at exactly this song's times: what a translation looks like.
        let english = [
            "The morning train was late again",
            "We waited by the window for an hour",
            "Somebody left a coat upon the bench",
            "The rain stopped just before the evening",
        ];
        let mut translated = retimed.clone();
        for (l, e) in translated
            .lyrics
            .lines
            .iter_mut()
            .zip(english.iter().cycle())
        {
            l.text = (*e).to_owned();
        }
        assert_eq!(pair_from(&mut f, &[&translated]), None);
        assert_eq!(f, kesariya);
        // Both copies already in Latin letters: nothing to pair.
        let (mut tum, tum_lp) = pairing_answers("tum-hi-ho");
        assert_eq!(pair_from(&mut tum, &[&tum_lp]), None);
        // The first copy that pairs is the one used.
        let (mut f, lp) = pairing_answers("kesariya");
        assert_eq!(
            pair_from(&mut f, &[&translated, &apna, &lp]).as_deref(),
            Some("lyricsplus")
        );
    }

    #[test]
    fn automatic_pairs_its_pick_with_another_sources_copy() {
        // LRCLIB first in your order: its Devanagari copy is the pick, and LyricsPlus's Latin
        // one (answered later) its pronunciation.
        let (lrclib, lyricsplus) = pairing_answers("kesariya");
        let answers = vec![
            Some(Ok(Some(lrclib.clone()))),
            Some(Ok(None)),
            Some(Err(AskError::Refused("no".into()))),
            Some(Ok(Some(lyricsplus.clone()))),
        ];
        let pick = outcome(&answers, true).unwrap().unwrap().unwrap();
        assert_eq!(pick.source, "lrclib");
        assert!(wants_pairing(&pick));
        let f = paired_pick(pick, &answers);
        assert!(paired(&f).len() > 30);
        assert_eq!(
            f.lyrics.lines[0].text, lrclib.lyrics.lines[0].text,
            "still LRCLIB's"
        );
        // LyricsPlus first: its Latin lines are the pick and want nothing.
        let answers = vec![Some(Ok(Some(lyricsplus.clone()))), Some(Ok(Some(lrclib)))];
        let pick = outcome(&answers, true).unwrap().unwrap().unwrap();
        assert!(!wants_pairing(&pick));
        assert_eq!(paired_pick(pick, &answers), lyricsplus);
    }

    #[test]
    fn pairing_measured_on_every_captured_song() {
        // Both ways round: LRCLIB's copy as the original, then LyricsPlus's.
        for song in [
            "kesariya",
            "apna-bana-le",
            "chaleya",
            "heeriye",
            "tum-hi-ho",
            "channa-mereya",
        ] {
            let (lrclib, lyricsplus) = pairing_answers(song);
            for (name, mut original, other) in [
                ("LRCLIB", lrclib.clone(), &lyricsplus),
                ("LyricsPlus", lyricsplus.clone(), &lrclib),
            ] {
                let by = pair_from(&mut original, &[other]);
                eprintln!(
                    "{song}, {name} first: {} of {} lines not in Latin letters paired ({by:?})",
                    paired(&original).len(),
                    not_latin(&original)
                );
            }
        }
    }

    #[test]
    fn neteases_newer_call_first_and_the_old_one_when_it_has_nothing() {
        let idol = idol_req();
        let asked = std::cell::RefCell::new(Vec::new());
        let ask = |url: &str| {
            asked.borrow_mut().push(url.to_owned());
            Ok(Some(if url.contains("/v1?") {
                serde_json::json!({ "code": 200, "nolyric": true })
            } else {
                fixture("netease-idol.json")
            }))
        };
        let f = netease_lyrics(&idol, 2034742057, ask).unwrap().unwrap();
        assert_eq!(f.lyrics.lines[0].text, "無敵の笑顔で荒らすメディア");
        assert_eq!(
            *asked.borrow(),
            [
                "https://music.163.com/api/song/lyric/v1?id=2034742057&cp=false&tv=0&lv=0&rv=0&kv=0&yv=0&ytv=0&yrv=0",
                "https://music.163.com/api/song/lyric?id=2034742057&lv=1&kv=1&tv=-1&rv=-1"
            ]
        );
        // The newer call answers: the old one isn't asked.
        let asked = std::cell::RefCell::new(0);
        let f = netease_lyrics(&idol, 1, |_| {
            *asked.borrow_mut() += 1;
            Ok(Some(fixture("netease-v1-idol.json")))
        })
        .unwrap();
        assert!(f.is_some());
        assert_eq!(*asked.borrow(), 1);
        // The newer call fails: the old one is asked; when both fail, the old one's error.
        let f = netease_lyrics(&idol, 1, |url| {
            if url.contains("/v1?") {
                Err(AskError::Down("HTTP 503".into()))
            } else {
                Ok(Some(fixture("netease-idol.json")))
            }
        })
        .unwrap();
        assert!(f.is_some());
        assert_eq!(
            netease_lyrics(&idol, 1, |_| Err(AskError::Down("HTTP 503".into()))),
            Err(AskError::Down("HTTP 503".into()))
        );
    }

    fn texts(f: &Found) -> Vec<&str> {
        f.lyrics.lines.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn credit_lines_from_the_chinese_sources_are_not_sung() {
        // Idol (YOASOBI) on NetEase and QQ Music, Gunjou on Kugou, as they answered on 2026-10-21.
        let idol = req("アイドル", &["YOASOBI"], 213);
        let ne = netease_found(&idol, &fixture("netease-idol.json")).unwrap();
        assert_eq!(
            &texts(&ne)[..2],
            ["無敵の笑顔で荒らすメディア", "知りたいその秘密ミステリアス"]
        );
        assert_eq!(
            ne.credits,
            [
                "作词 : Ayase",
                "作曲 : Ayase",
                "编曲 : Ayase",
                "制作人 : Ayase"
            ]
        );
        assert!(
            ne.lyrics.lines[0].translation.is_some(),
            "the translations still meet their lines"
        );

        let qq = qq_found(&idol, &fixture("qq-idol.json")).unwrap();
        assert_eq!(texts(&qq)[0], "無敵の笑顔で荒らすメディア");
        assert_eq!(
            qq.credits,
            [
                "アイドル - YOASOBI (ヨアソビ)",
                "词：Ayase",
                "曲：Ayase",
                "编曲：Ayase"
            ]
        );

        let gunjou = req("群青", &["YOASOBI"], 248);
        let kg = kugou_found(&gunjou, &fixture("kugou-gunjou-lrc.json")).unwrap();
        assert_eq!(texts(&kg)[0], "嗚呼 いつもの様に");
        assert_eq!(
            kg.credits,
            [
                "YOASOBI - 群青 - THE FIRST TAKE",
                "作词：Ayase",
                "作曲：Ayase"
            ]
        );
        // A sung line with a dash further in is still sung.
        assert!(kg.lyrics.lines.len() > 80);
    }

    #[test]
    fn what_the_sources_send_beside_the_lines_is_kept() {
        let idol = req("アイドル", &["YOASOBI"], 213);
        // LyricsPlus labels the song's language.
        let lp = lyricsplus_found(&idol, &fixture("lyricsplus-idol.json")).unwrap();
        assert_eq!(lp.lyrics.lang.as_deref(), Some("ja"));

        // NetEase: Roman letters (romalrc, syllable by syllable) joined into words, at the
        // line's own time, and its Chinese translation.
        let ne = netease_found(&idol, &fixture("netease-idol.json")).unwrap();
        let first = &ne.lyrics.lines[0];
        assert_eq!(first.text, "無敵の笑顔で荒らすメディア");
        let roman = first.roman.as_ref().unwrap();
        assert_eq!(roman.text, "muteki no egao de arasu media");
        assert_eq!(roman.lang, "ja-Latn");
        assert_eq!(roman.source, SecondSource::Lyrics("netease".into()));
        let tr = first.translation.as_ref().unwrap();
        assert_eq!(
            (tr.text.as_str(), tr.lang.as_str()),
            ("露出无敌笑容来摧毁媒体", "zh-Hans")
        );
        let english = ne
            .lyrics
            .lines
            .iter()
            .find(|l| l.text == "You're my savior")
            .unwrap();
        assert!(english.roman.is_none(), "nothing to romanize");
        assert!(
            ne.lyrics.lines.iter().filter(|l| l.roman.is_some()).count() > 70,
            "every Japanese line"
        );

        // QQ Music's translation (empty in the captured answer; one added here).
        let mut qq = fixture("qq-idol.json");
        qq["trans"] = Value::String("[00:00.74]露出无敌笑容\n".into());
        let qq = qq_found(&idol, &qq).unwrap();
        let tr = qq.lyrics.lines[0].translation.as_ref().unwrap();
        assert_eq!(
            (tr.text.as_str(), tr.lang.as_str()),
            ("露出无敌笑容", "zh-Hans")
        );
        assert_eq!(tr.source, SecondSource::Lyrics("qq".into()));

        // Kugou's KRC: every word timed, Roman letters timed by the words, translations.
        let gunjou = req("群青", &["YOASOBI"], 248);
        let kg = kugou_krc_found(&gunjou, &fixture("kugou-gunjou-krc.json"), Some("ja")).unwrap();
        assert_eq!(kg.lyrics.lang.as_deref(), Some("ja"));
        assert_eq!(
            kg.credits,
            [
                "YOASOBI - 群青 - THE FIRST TAKE",
                "作词：Ayase",
                "作曲：Ayase"
            ]
        );
        let l = &kg.lyrics.lines[0];
        assert_eq!(l.text, "嗚呼 いつもの様に");
        assert_eq!((l.start_ms, l.end_ms), (4493, 6727));
        assert_eq!(l.words[0].start_ms, 4493);
        let roman = l.roman.as_ref().unwrap();
        assert_eq!(roman.text, "aa itsumono you ni");
        assert_eq!(
            roman
                .words
                .iter()
                .map(|w| (w.start_ms, w.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (4493, "aa"),
                (5148, "itsumono"),
                (5807, "you"),
                (6203, "ni")
            ]
        );
        assert_eq!(
            l.translation
                .as_ref()
                .map(|t| (t.text.as_str(), t.lang.as_str())),
            Some(("啊啊 一如既往般", "zh-Hans"))
        );
        // The same lines as its LRC, as many and at the same times.
        let lrc = kugou_found(&gunjou, &fixture("kugou-gunjou-lrc.json")).unwrap();
        assert_eq!(kg.lyrics.lines.len(), lrc.lyrics.lines.len());
        assert_eq!(kugou_language("日语").as_deref(), Some("ja"));
        assert_eq!(krc_text("not krc"), None);
    }

    #[test]
    fn answers_kept_in_an_older_format_are_asked_for_again() {
        let dir = tempfile::tempdir().unwrap();
        let kept = Kept::open_at(&dir.path().join("cache.sqlite")).unwrap();
        // A NetEase answer as version 1 kept it: `ms`, a translation as text, no version.
        let old = r#"{"source":"netease","lines":[{"ms":273,"text":"無敵の笑顔で","translation":"露出无敌笑容"},{"ms":3283,"text":"知りたい"}],"text":"","instrumental":false,"credit":null}"#;
        kept.conn
            .execute(
                "INSERT INTO lyrics (song, source, found, at) VALUES ('s', 'auto', ?1, 0)",
                params![old],
            )
            .unwrap();
        assert_eq!(kept.get("s", "auto"), None, "asked for again");
        let stale = kept.stale("s", "auto").unwrap();
        let first = &stale.lyrics.lines[0];
        assert_eq!((first.start_ms, first.end_ms), (273, 3283));
        assert_eq!(first.script, "Jpan");
        assert_eq!(first.translation.as_ref().unwrap().lang, "zh-Hans");

        // The source didn't answer: the old answer is shown, and asked for again next time.
        let shown = renewed(
            &kept,
            "s",
            "auto",
            Err(AskError::Down("x".into())),
            Some(stale.clone()),
        );
        assert_eq!(shown.as_ref(), Some(&stale));
        assert_eq!(kept.get("s", "auto"), None);
        // It has nothing now: the old answer stays, kept in today's format.
        renewed(&kept, "s", "auto", Ok(None), Some(stale.clone()));
        let now = kept.get("s", "auto").unwrap().unwrap();
        assert_eq!(now.lyrics.version, crate::lyrics::VERSION);
        assert_eq!(kept.stale("s", "auto"), None);
        // A new answer replaces it.
        let mut fresh = stale.clone();
        fresh.lyrics.lines[0].text = "new".into();
        assert_eq!(
            renewed(&kept, "t", "auto", Ok(Some(fresh.clone())), None)
                .map(|f| f.lyrics.lines[0].text.clone()),
            Some("new".into())
        );
        assert_eq!(
            kept.get("t", "auto").unwrap().unwrap().lyrics.lines[0].text,
            "new"
        );
    }
}
