//! Lyrics as Venuta keeps them (the contract in `docs/design/lyrics-second-line.md`), and the
//! one reader of LRC: your own `.lrc` files, the lyrics in your tags, and what the lyrics
//! services send all go through [`read_lrc`]. Nothing here reads files or the network, so the
//! desktop app, the phone server and Android share it.
//!
//! What the reader understands:
//! - `[mm:ss.xx]` line stamps (also `[mm:ss]`, `[mm:ss,xx]` and `[mm:ss:xx]`), several on one
//!   line for a chorus sung again;
//! - `<mm:ss.xx>` word stamps (enhanced LRC): kept as each word's time, never as text;
//! - `[offset:±ms]`: a positive offset makes every line come sooner, as the format says;
//! - header tags (`[ar:…]`, `[ti:…]`), kept apart from the lines;
//! - stamped lines with no words: a break, where the words leave the screen;
//! - a second line at the same time as another: the common way to give a line its
//!   translation. The first one in the file is the sung line (or the word-timed one), the
//!   other its translation, so a translation is never lit up in place of what is sung.

use serde::{Deserialize, Deserializer, Serialize};

/// The answer format: bumped when lyrics are read in a new way, so answers kept from before
/// are asked for again (and pick up what the new reading keeps).
///
/// 1: lines with `ms`, words and a translation as plain text (before October 2026).
/// 2: this contract (start and end, script, pronunciation and translation with their language
///    and source, the song's language, credit lines kept apart).
pub const VERSION: u32 = 2;

/// One word of a line, when its time is known.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Word {
    #[serde(alias = "ms")]
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Where a second line came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "by", rename_all = "lowercase")]
pub enum SecondSource {
    /// The lyrics carried it (the provider's id: "netease", "kugou", "file").
    Lyrics(String),
    /// Paired by time from another provider's copy of the song.
    Paired(String),
    /// Made by Venuta, on the device.
    Made,
    /// Translated by an engine (its name).
    Translated(String),
    /// Corrected by hand.
    Corrected,
}

/// A line under the sung one: its pronunciation, or its meaning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Second {
    pub text: String,
    /// Pronunciation only: timed by the original's words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
    /// BCP 47: "ja-Latn" for pronunciation, "zh-Hans" for a Chinese translation, "und" when
    /// it isn't known.
    pub lang: String,
    pub source: SecondSource,
}

/// One timed line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    #[serde(alias = "ms")]
    pub start_ms: u64,
    /// When it ends: as the lyrics say, else when the next line starts.
    #[serde(default)]
    pub end_ms: u64,
    /// The original, as sung.
    pub text: String,
    /// Word by word, when the words are timed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
    /// The line's writing system (ISO 15924: "Latn", "Deva", "Jpan", "Hang", "Hani"), "Zyyy"
    /// for a line with no letters.
    #[serde(default)]
    pub script: String,
    /// Its pronunciation in Roman letters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roman: Option<Second>,
    /// Its meaning in another language.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "translation_of"
    )]
    pub translation: Option<Second>,
}

/// A translation as kept now, or as answers of version 1 kept it (plain text, which only
/// NetEase gave, in Chinese).
fn translation_of<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Second>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Old(String),
        New(Second),
    }
    Ok(match Option::<Either>::deserialize(d)? {
        None => None,
        Some(Either::New(s)) => Some(s),
        Some(Either::Old(text)) => Some(Second {
            text,
            words: Vec::new(),
            lang: "zh-Hans".into(),
            source: SecondSource::Lyrics("netease".into()),
        }),
    })
}

/// A song's lyrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    /// Timed lines; empty when only the words are known.
    pub lines: Vec<LyricLine>,
    /// The song's language (BCP 47), when the lyrics say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// The answer format ([`VERSION`]); 0 or 1 for answers kept before it.
    #[serde(default)]
    pub version: u32,
}

/// An LRC text, read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lrc {
    /// Timed lines in time order, the offset already applied.
    pub lines: Vec<LyricLine>,
    /// Header tags in file order (`("ar", "Someone")`), without the offset.
    pub tags: Vec<(String, String)>,
    /// The `[offset:]` the file asked for, in ms (already applied to `lines`).
    pub offset_ms: i64,
}

impl Lrc {
    /// The value of a header tag (`ar`, `ti`), when the file has it.
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

// --- Scripts --------------------------------------------------------------------------------

/// The writing system of one letter (ISO 15924), None for marks, digits and punctuation.
fn script_of_char(c: char) -> Option<&'static str> {
    let u = c as u32;
    Some(match u {
        _ if c.is_ascii_alphabetic() => "Latn",
        0x00C0..=0x024F | 0x1E00..=0x1EFF => "Latn",
        0x0370..=0x03FF | 0x1F00..=0x1FFF => "Grek",
        0x0400..=0x052F => "Cyrl",
        0x0590..=0x05FF => "Hebr",
        0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "Arab",
        0x0900..=0x097F => "Deva",
        0x0980..=0x09FF => "Beng",
        0x0A00..=0x0A7F => "Guru",
        0x0A80..=0x0AFF => "Gujr",
        0x0B00..=0x0B7F => "Orya",
        0x0B80..=0x0BFF => "Taml",
        0x0C00..=0x0C7F => "Telu",
        0x0C80..=0x0CFF => "Knda",
        0x0D00..=0x0D7F => "Mlym",
        0x0E00..=0x0E7F => "Thai",
        0x3041..=0x309F => "Hira",
        0x30A1..=0x30FA | 0x30FC..=0x30FF | 0x31F0..=0x31FF => "Kana",
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7A3 => "Hang",
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F | 0x3005 => "Hani",
        _ => return None,
    })
}

/// A line's writing system (ISO 15924): the one most of its letters are in. Japanese (kana,
/// with or without kanji) is "Jpan"; a line with no letters is "Zyyy".
pub fn script_of(text: &str) -> String {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for c in text.chars() {
        if let Some(s) = script_of_char(c) {
            match counts.iter_mut().find(|(k, _)| *k == s) {
                Some((_, n)) => *n += 1,
                None => counts.push((s, 1)),
            }
        }
    }
    let has = |s: &str| counts.iter().any(|(k, _)| *k == s);
    if has("Hira") || has("Kana") {
        let jp: usize = counts
            .iter()
            .filter(|(k, _)| matches!(*k, "Hira" | "Kana" | "Hani"))
            .map(|(_, n)| n)
            .sum();
        let other = counts
            .iter()
            .filter(|(k, _)| !matches!(*k, "Hira" | "Kana" | "Hani"))
            .max_by_key(|(_, n)| *n);
        if other.is_none_or(|(_, n)| *n <= jp) {
            return "Jpan".into();
        }
    }
    // On a tie the other script wins: a Latin word in a Hindi line leaves it Hindi.
    counts
        .iter()
        .max_by_key(|(k, n)| (*n, *k != "Latn"))
        .map_or("Zyyy", |(k, _)| k)
        .to_owned()
}

/// The language a script most likely means, for labelling a second line: only where the
/// script says it (Japanese kana, Korean Hangul); else None.
pub fn lang_of_script(script: &str) -> Option<&'static str> {
    match script {
        "Jpan" | "Hira" | "Kana" => Some("ja"),
        "Hang" => Some("ko"),
        _ => None,
    }
}

/// The BCP 47 tag of a pronunciation in Roman letters: "ja-Latn", or "und-Latn".
pub fn roman_lang(song_lang: Option<&str>, script: &str) -> String {
    let base = song_lang
        .and_then(|l| l.split('-').next())
        .filter(|l| !l.is_empty() && *l != "und")
        .or_else(|| lang_of_script(script));
    format!("{}-Latn", base.unwrap_or("und"))
}

/// The BCP 47 tag of a translation the lyrics carried, from its own letters. The Chinese
/// services write theirs in simplified Chinese.
pub fn translation_lang(text: &str, provider: &str) -> String {
    match script_of(text).as_str() {
        "Hani" if matches!(provider, "netease" | "qq" | "kugou") => "zh-Hans".into(),
        "Hani" => "zh".into(),
        "Jpan" => "ja".into(),
        "Hang" => "ko".into(),
        "Zyyy" => "und".into(),
        s => format!("und-{s}"),
    }
}

/// A translation the lyrics carried.
pub fn carried_translation(text: &str, provider: &str) -> Second {
    Second {
        text: text.trim().to_owned(),
        words: Vec::new(),
        lang: translation_lang(text, provider),
        source: SecondSource::Lyrics(provider.to_owned()),
    }
}

/// Lines made whole: each line's end (when the lyrics didn't say: the next line's start, else
/// its last word's end, else 5 s on) and its script.
pub fn finish(lines: &mut [LyricLine]) {
    for i in 0..lines.len() {
        let next = lines.get(i + 1).map(|l| l.start_ms);
        let l = &mut lines[i];
        if l.end_ms <= l.start_ms {
            let words_end = l.words.iter().map(|w| w.end_ms).max();
            l.end_ms = next
                .filter(|n| *n > l.start_ms)
                .or(words_end)
                .unwrap_or(l.start_ms + 5_000)
                .max(l.start_ms);
        }
        if l.script.is_empty() {
            l.script = script_of(&l.text);
        }
    }
}

/// Every translation a provider's lyrics carried, marked as theirs.
pub fn mark_source(lines: &mut [LyricLine], provider: &str) {
    for l in lines {
        if let Some(t) = &mut l.translation
            && t.source == SecondSource::Lyrics(String::new())
        {
            t.source = SecondSource::Lyrics(provider.to_owned());
            t.lang = translation_lang(&t.text, provider);
        }
    }
}

// --- LRC ------------------------------------------------------------------------------------

/// A stamp's time: `mm:ss`, `mm:ss.xx`, `mm:ss,xx` or `mm:ss:xx`. Digits only, so a header
/// like `[ar:Someone]` is never read as a time.
pub fn stamp_ms(t: &str) -> Option<u64> {
    let t = t.trim();
    let mut parts = t.splitn(3, ':');
    let m = parts.next()?.trim();
    let s = parts.next()?.trim();
    let frac = parts.next().map(str::trim);
    if m.is_empty() || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let s = match frac {
        Some(f) if !s.contains(['.', ',']) => format!("{s}.{f}"),
        Some(_) => return None,
        None => s.replace(',', "."),
    };
    let (whole, part) = s.split_once('.').unwrap_or((&s, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let m: u64 = m.parse().ok()?;
    let secs: f64 = s.parse().ok()?;
    Some(m * 60_000 + (secs * 1000.0).round() as u64)
}

/// `01:02.34`: a time to the hundredth, as players write it.
pub fn format_stamp(ms: u64) -> String {
    let cs = (ms + 5) / 10;
    format!("{:02}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

/// A header tag's key and value (`ar:Someone`), when the bracket holds one.
fn header(tag: &str) -> Option<(String, String)> {
    let (k, v) = tag.split_once(':')?;
    let k = k.trim();
    let keyish = !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '#' || c == '_' || c == '-');
    (keyish && !k.bytes().all(|b| b.is_ascii_digit()))
        .then(|| (k.to_ascii_lowercase(), v.trim().to_owned()))
}

/// The offset a header asks for: `+500`, `-250`, `0`.
fn offset_of(v: &str) -> Option<i64> {
    let v = v.trim();
    v.strip_prefix('+').unwrap_or(v).parse().ok()
}

/// One line as written, before same-time lines are paired.
struct Raw {
    ms: u64,
    text: String,
    words: Vec<Word>,
}

/// Whether a line's body holds a `<mm:ss.xx>` word stamp.
fn has_word_stamp(body: &str) -> bool {
    body.match_indices('<').any(|(i, _)| {
        body[i + 1..]
            .split_once('>')
            .is_some_and(|(tag, _)| stamp_ms(tag).is_some())
    })
}

/// A line's words from `<mm:ss.xx>` stamps, and its text without them. Text before the first
/// stamp is a word starting with the line; a stamp after a word ends that word.
fn words_of(body: &str, line_ms: u64) -> (Vec<Word>, String) {
    let body = body.trim();
    if !has_word_stamp(body) {
        return (
            Vec::new(),
            body.split_whitespace().collect::<Vec<_>>().join(" "),
        );
    }
    let mut words: Vec<Word> = Vec::new();
    let mut text = String::new();
    let mut at = Some(line_ms);
    let mut rest = body;
    loop {
        let (before, after) = rest.split_once('<').unwrap_or((rest, ""));
        if !before.is_empty() {
            text.push_str(before);
            let blank = before.trim().is_empty();
            match (at.take(), words.last_mut()) {
                // Spaces between two stamps belong to the word before.
                (_, Some(last)) if blank => last.text.push_str(before),
                (Some(ms), _) if !blank => words.push(Word {
                    start_ms: ms,
                    end_ms: 0,
                    text: before.to_owned(),
                }),
                (_, Some(last)) => last.text.push_str(before),
                _ => {}
            }
        }
        if after.is_empty() {
            break;
        }
        let Some((tag, next)) = after.split_once('>') else {
            let literal = format!("<{after}");
            text.push_str(&literal);
            if let Some(last) = words.last_mut() {
                last.text.push_str(&literal);
            }
            break;
        };
        match stamp_ms(tag) {
            Some(ms) => {
                if let Some(last) = words.last_mut()
                    && last.end_ms == 0
                {
                    last.end_ms = ms.max(last.start_ms);
                }
                at = Some(ms);
            }
            None => {
                let literal = format!("<{tag}>");
                text.push_str(&literal);
                if let Some(last) = words.last_mut() {
                    last.text.push_str(&literal);
                }
            }
        }
        rest = next;
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (words, text)
}

/// Read an LRC text. Lines without a stamp (and so plain lyrics) give no timed lines. A
/// translation found at the same time as a line is marked as the lyrics' own, with no
/// provider yet ([`mark_source`] names it).
pub fn read_lrc(text: &str) -> Lrc {
    let mut raws: Vec<Raw> = Vec::new();
    let mut tags = Vec::new();
    let mut offset_ms = 0i64;
    for raw in text.lines() {
        let mut rest = raw.trim().trim_start_matches('\u{feff}').trim_start();
        let mut stamps = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some((tag, after)) = r.split_once(']') else {
                break;
            };
            if let Some(ms) = stamp_ms(tag) {
                stamps.push(ms);
                rest = after;
                continue;
            }
            if stamps.is_empty()
                && let Some((k, v)) = header(tag)
            {
                if k == "offset" {
                    offset_ms = offset_of(&v).unwrap_or(0);
                } else {
                    tags.push((k, v));
                }
                rest = after.trim_start();
                continue;
            }
            // A bracket after the stamps is part of the words ("[Chorus] la la").
            break;
        }
        let Some(&first) = stamps.first() else {
            continue;
        };
        let (words, body) = words_of(rest, first);
        for ms in stamps {
            // A line sung again keeps its words' timing, moved with it.
            let moved = |t: u64| (t + ms).saturating_sub(first);
            let words = words
                .iter()
                .map(|w| Word {
                    start_ms: moved(w.start_ms),
                    end_ms: if w.end_ms == 0 { 0 } else { moved(w.end_ms) },
                    text: w.text.clone(),
                })
                .collect();
            raws.push(Raw {
                ms,
                text: body.clone(),
                words,
            });
        }
    }
    // In time order; lines at the same time stay in the file's order (the sung line first).
    raws.sort_by_key(|r| r.ms);
    let mut lines: Vec<LyricLine> = Vec::new();
    for r in raws {
        match lines.last_mut().filter(|l| l.start_ms == r.ms) {
            Some(l) if r.text.is_empty() || r.text == l.text => {
                if l.words.is_empty() && !r.words.is_empty() {
                    l.words = r.words;
                }
            }
            Some(l) if l.text.is_empty() => {
                l.text = r.text;
                l.words = r.words;
            }
            Some(l) => {
                // The word-timed one is the one sung.
                let other = if l.words.is_empty() && !r.words.is_empty() {
                    l.words = r.words;
                    std::mem::replace(&mut l.text, r.text)
                } else {
                    r.text
                };
                let text = match l.translation.take() {
                    Some(t) => format!("{} / {other}", t.text),
                    None => other,
                };
                l.translation = Some(Second {
                    lang: String::new(),
                    text,
                    words: Vec::new(),
                    source: SecondSource::Lyrics(String::new()),
                });
            }
            None => lines.push(LyricLine {
                start_ms: r.ms,
                text: r.text,
                words: r.words,
                ..Default::default()
            }),
        }
    }
    if offset_ms != 0 {
        let shift = |ms: u64| (ms as i64 - offset_ms).max(0) as u64;
        for l in &mut lines {
            l.start_ms = shift(l.start_ms);
            for w in &mut l.words {
                w.start_ms = shift(w.start_ms);
                if w.end_ms != 0 {
                    w.end_ms = shift(w.end_ms);
                }
            }
        }
    }
    // A word with no end ends at the next word, the last at the next line, at most 3 s on.
    for i in 0..lines.len() {
        let next = lines.get(i + 1).map(|l| l.start_ms);
        let line = &mut lines[i];
        let n = line.words.len();
        for w in 0..n {
            if line.words[w].end_ms != 0 {
                continue;
            }
            let start = line.words[w].start_ms;
            let end = if w + 1 < n {
                line.words[w + 1].start_ms
            } else {
                next.unwrap_or(start + 3_000).min(start + 3_000)
            };
            line.words[w].end_ms = end.max(start);
        }
    }
    for l in &mut lines {
        if let Some(t) = &mut l.translation {
            t.lang = translation_lang(&t.text, "");
        }
    }
    finish(&mut lines);
    Lrc {
        lines,
        tags,
        offset_ms,
    }
}

/// Whether lyrics text has times (at least one stamped line).
pub fn is_timed(text: &str) -> bool {
    !read_lrc(text).lines.is_empty()
}

/// Lines as LRC: header tags first, then each line with its stamp, its words' stamps (and the
/// last word's end), and its translation as a second line at the same time.
pub fn write_lrc(tags: &[(String, String)], lines: &[LyricLine]) -> String {
    let mut out = String::new();
    for (k, v) in tags {
        out.push_str(&format!("[{k}:{v}]\n"));
    }
    let mut sorted: Vec<&LyricLine> = lines.iter().collect();
    sorted.sort_by_key(|l| l.start_ms);
    for l in sorted {
        let stamp = format_stamp(l.start_ms);
        let words: Vec<&Word> = l.words.iter().filter(|w| !w.text.is_empty()).collect();
        out.push_str(&format!("[{stamp}]"));
        if words.is_empty() {
            out.push_str(l.text.trim());
        } else {
            for (i, w) in words.iter().enumerate() {
                let text = if i + 1 == words.len() {
                    w.text.trim_end()
                } else {
                    &w.text
                };
                out.push_str(&format!("<{}>{text}", format_stamp(w.start_ms)));
            }
            if let Some(last) = words.last()
                && last.end_ms > last.start_ms
            {
                out.push_str(&format!("<{}>", format_stamp(last.end_ms)));
            }
        }
        out.push('\n');
        if let Some(t) = l
            .translation
            .as_ref()
            .map(|t| t.text.trim())
            .filter(|t| !t.is_empty())
        {
            out.push_str(&format!("[{stamp}]{t}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(l: &Lrc) -> Vec<(u64, &str)> {
        l.lines
            .iter()
            .map(|x| (x.start_ms, x.text.as_str()))
            .collect()
    }

    #[test]
    fn line_stamps_several_on_a_line_and_headers() {
        let l = read_lrc(
            "\u{feff}[ar:Someone]\n[ti:A Song]\n[00:01.25]First\n[00:10.00][01:00.50]Again\n\nnot timed\n[00:05.5]Mid\n[00:07]No fraction\n[00:08:50]Colon hundredths",
        );
        assert_eq!(
            texts(&l),
            [
                (1250, "First"),
                (5500, "Mid"),
                (7000, "No fraction"),
                (8500, "Colon hundredths"),
                (10_000, "Again"),
                (60_500, "Again"),
            ]
        );
        assert_eq!(l.tag("ar"), Some("Someone"));
        assert_eq!(l.tag("TI"), Some("A Song"));
        assert_eq!(l.lines[0].end_ms, 5500, "a line ends when the next starts");
        assert_eq!(l.lines[5].end_ms, 65_500, "the last one 5 s on");
        assert_eq!(l.lines[0].script, "Latn");
        assert!(read_lrc("Just words\nwith no times").lines.is_empty());
        assert!(read_lrc("[ar:Someone]\nwords").lines.is_empty());
        assert!(!is_timed("[ar:Someone]\nwords"));
        assert!(is_timed("[00:00.00]"));
    }

    #[test]
    fn word_stamps_are_timing_never_text() {
        let l = read_lrc(
            "[00:05.00]<00:05.00>Word <00:05.40>by <00:05.90>word\n[00:10.00]<00:10.00>Ends <00:10.50>here<00:11.20>",
        );
        assert_eq!(l.lines[0].text, "Word by word");
        let w = &l.lines[0].words;
        assert_eq!(
            w.iter()
                .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (5000, 5400, "Word "),
                (5400, 5900, "by "),
                (5900, 8900, "word")
            ],
            "the last word ends with the next line, at most 3 s on"
        );
        assert_eq!(l.lines[1].text, "Ends here");
        assert_eq!(
            l.lines[1].words[1].end_ms, 11_200,
            "a stamp after the last word is its end"
        );
        // Text before the first word stamp starts with the line; a non-time <tag> is text.
        let l = read_lrc("[00:01.00]Hello <00:01.50>you <3");
        assert_eq!(l.lines[0].text, "Hello you <3");
        assert_eq!(l.lines[0].words[0].start_ms, 1000);
        assert_eq!(l.lines[0].words[1].text, "you <3");
        // A sung-again line moves its words with it.
        let l = read_lrc("[00:01.00][00:31.00]<00:01.00>La <00:01.50>la");
        assert_eq!(l.lines[1].words[1].start_ms, 31_500);
    }

    #[test]
    fn the_offset_moves_every_time() {
        let l =
            read_lrc("[offset:+500]\n[00:01.00]<00:01.00>Soon <00:01.40>now\n[00:00.20]Clamped");
        assert_eq!(l.offset_ms, 500);
        assert_eq!(texts(&l), [(0, "Clamped"), (500, "Soon now")]);
        assert_eq!(l.lines[1].words[1].start_ms, 900);
        assert!(l.tag("offset").is_none(), "applied, not kept as a tag");
        let l = read_lrc("[offset:-250]\n[00:01.00]Later");
        assert_eq!(l.lines[0].start_ms, 1250);
    }

    #[test]
    fn blank_stamped_lines_are_breaks() {
        let l = read_lrc("[00:01.00]Sung\n[00:04.00]\n[00:20.00]Again");
        assert_eq!(texts(&l), [(1000, "Sung"), (4000, ""), (20_000, "Again")]);
        assert_eq!(l.lines[1].script, "Zyyy");
    }

    #[test]
    fn a_line_at_the_same_time_is_the_translation_not_the_sung_line() {
        let l = read_lrc(
            "[00:01.00]無敵の笑顔で\n[00:01.00]露出无敌笑容\n[00:04.00]Same\n[00:04.00]Same\n[00:06.00]\n[00:06.00]Words after a blank\n[00:09.00]Mujhko itna\n[00:09.00]Tell me",
        );
        assert_eq!(
            texts(&l),
            [
                (1000, "無敵の笑顔で"),
                (4000, "Same"),
                (6000, "Words after a blank"),
                (9000, "Mujhko itna")
            ]
        );
        let t = l.lines[0].translation.as_ref().unwrap();
        assert_eq!(t.text, "露出无敌笑容");
        assert_eq!(t.lang, "zh", "Chinese by its letters");
        assert_eq!(l.lines[0].script, "Jpan");
        assert_eq!(
            l.lines[1].translation, None,
            "the same words twice are one line"
        );
        assert_eq!(l.lines[2].translation, None);
        assert_eq!(l.lines[3].translation.as_ref().unwrap().lang, "und-Latn");
        // The word-timed one is the one sung, whichever comes first.
        let l = read_lrc("[00:01.00]Translated\n[00:01.00]<00:01.00>Sung <00:01.50>words");
        assert_eq!(l.lines[0].text, "Sung words");
        assert_eq!(l.lines[0].words.len(), 2);
        assert_eq!(l.lines[0].translation.as_ref().unwrap().text, "Translated");
        // Named for the provider once known.
        let mut lines = l.lines;
        mark_source(&mut lines, "file");
        assert_eq!(
            lines[0].translation.as_ref().unwrap().source,
            SecondSource::Lyrics("file".into())
        );
    }

    #[test]
    fn written_lines_read_back_the_same() {
        let src = "[ar:Someone]\n[00:01.00]<00:01.00>Word <00:01.40>by <00:01.90>word<00:02.50>\n[00:01.00]Translated\n[00:04.00]\n[01:01.39]Plain\n";
        let l = read_lrc(src);
        let out = write_lrc(&l.tags, &l.lines);
        assert_eq!(out, src);
        assert_eq!(read_lrc(&out), l);
        assert_eq!(format_stamp(61_390), "01:01.39");
        assert_eq!(format_stamp(1_999), "00:02.00");
    }

    #[test]
    fn scripts_by_most_letters() {
        assert_eq!(script_of("Kesariya tera"), "Latn");
        assert_eq!(script_of("मुझको इतना बताए कोई"), "Deva");
        assert_eq!(script_of("तेरा मेरा, Kesariya!"), "Deva");
        assert_eq!(script_of("天才的なアイドル様"), "Jpan");
        assert_eq!(script_of("사랑해"), "Hang");
        assert_eq!(script_of("露出无敌笑容"), "Hani");
        assert_eq!(script_of("…"), "Zyyy");
        assert_eq!(roman_lang(None, "Jpan"), "ja-Latn");
        assert_eq!(roman_lang(Some("hi"), "Deva"), "hi-Latn");
        assert_eq!(roman_lang(None, "Hani"), "und-Latn");
        assert_eq!(translation_lang("露出无敌笑容", "netease"), "zh-Hans");
    }

    #[test]
    fn old_answers_still_read() {
        // Version 1 kept `ms` and a translation as text.
        let old: LyricLine = serde_json::from_str(
            r#"{"ms":1000,"text":"x","words":[{"ms":1000,"end_ms":1400,"text":"x"}],"translation":"翻译"}"#,
        )
        .unwrap();
        assert_eq!(old.start_ms, 1000);
        assert_eq!(old.words[0].start_ms, 1000);
        let t = old.translation.unwrap();
        assert_eq!((t.text.as_str(), t.lang.as_str()), ("翻译", "zh-Hans"));
        let back: Second = serde_json::from_str(
            &serde_json::to_string(&Second {
                text: "kimi".into(),
                words: vec![],
                lang: "ja-Latn".into(),
                source: SecondSource::Made,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(back.source, SecondSource::Made);
        assert_eq!(
            serde_json::to_value(SecondSource::Lyrics("netease".into())).unwrap(),
            serde_json::json!({ "kind": "lyrics", "by": "netease" })
        );
    }
}
