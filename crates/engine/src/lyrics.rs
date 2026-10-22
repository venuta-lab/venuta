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

pub mod pair;
pub mod romaji;
pub mod ttml;

/// The answer format: bumped when lyrics are read in a new way, so answers kept from before
/// are asked for again (and pick up what the new reading keeps).
///
/// 1: lines with `ms`, words and a translation as plain text (before October 2026).
/// 2: this contract (start and end, script, pronunciation and translation with their language
///    and source, the song's language, credit lines kept apart).
/// 3: the second lines the sources carry (Apple-style TTML's, LyricsPlus's, NetEase's
///    word-timed lyrics from its newer call) and Automatic's pick paired with another source's
///    copy in Latin letters.
pub const VERSION: u32 = 3;

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

/// A line's writing system (ISO 15924): the one most of its letters are in, Latin losing a
/// tie. Japanese (kana, with or without kanji) is "Jpan"; a line with no letters is "Zyyy".
/// (`lyrics_text::script`, shared with the pronunciation reader.)
pub fn script_of(text: &str) -> String {
    lyrics_text::script::line_script(text).code().to_owned()
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

/// The languages written in a script other than Latin letters (ISO 639 codes), and the one a
/// song in it most likely is (as the views guess it from the letters, lib/lyricsModel.ts).
fn written_in(script: &str) -> (&'static [&'static str], &'static str) {
    match script {
        "Deva" => (
            &[
                "hi", "mr", "ne", "sa", "mai", "bho", "kok", "doi", "brx", "new", "awa", "mag",
                "hne", "raj", "bh", "sd", "ks", "pi",
            ],
            "hi",
        ),
        "Beng" => (&["bn", "as", "mni", "sat"], "bn"),
        "Guru" => (&["pa"], "pa"),
        "Gujr" => (&["gu"], "gu"),
        "Orya" => (&["or"], "or"),
        "Taml" => (&["ta"], "ta"),
        "Telu" => (&["te"], "te"),
        "Knda" => (&["kn", "tcy", "kok"], "kn"),
        "Mlym" => (&["ml"], "ml"),
        "Sinh" => (&["si"], "si"),
        "Arab" => (
            &[
                "ur", "ar", "fa", "ps", "sd", "ks", "pa", "ug", "ckb", "ku", "prs", "bal", "skr",
                "ms", "az",
            ],
            "ur",
        ),
        "Thaa" => (&["dv"], "dv"),
        "Syrc" => (&["syr", "aii"], "syr"),
        "Hebr" => (&["he", "iw", "yi", "lad"], "he"),
        "Thai" => (&["th"], "th"),
        "Laoo" => (&["lo"], "lo"),
        "Khmr" => (&["km"], "km"),
        "Mymr" => (&["my", "shn"], "my"),
        "Tibt" => (&["bo", "dz"], "bo"),
        "Geor" => (&["ka"], "ka"),
        "Armn" => (&["hy"], "hy"),
        "Ethi" => (&["am", "ti"], "am"),
        "Hang" => (&["ko"], "ko"),
        "Jpan" | "Hira" | "Kana" => (&["ja"], "ja"),
        "Hani" | "Bopo" => (
            &[
                "zh", "ja", "ko", "yue", "cmn", "wuu", "nan", "hak", "lzh", "gan", "hsn", "vi",
            ],
            "zh",
        ),
        "Cyrl" => (
            &[
                "ru", "uk", "bg", "sr", "mk", "be", "kk", "ky", "tg", "mn", "tt", "ba", "cv",
                "sah", "ce", "os", "bs", "uz", "az", "ab",
            ],
            "ru",
        ),
        "Grek" => (&["el", "grc"], "el"),
        _ => (&[], ""),
    }
}

/// The song's language: the source's label, unless most of its lines with letters are in a
/// script that language isn't written in (Devanagari lines labelled "en", as LyricsPlus labels
/// some Hindi songs): then the language their script most likely means. Lines in Latin letters
/// can be in any language, so they never overrule a label; with no label, none is made up (the
/// views read the letters themselves).
pub fn song_lang(tag: Option<&str>, lines: &[LyricLine]) -> Option<String> {
    let tag = tag.map(str::trim).filter(|t| !t.is_empty())?;
    let mut counts: Vec<(&str, usize)> = Vec::new();
    let mut lettered = 0;
    for l in lines {
        let script = match l.script.as_str() {
            "Hira" | "Kana" => "Jpan",
            s => s,
        };
        if matches!(script, "" | "Zyyy" | "Zinh" | "Zzzz") {
            continue;
        }
        lettered += 1;
        match counts.iter_mut().find(|(s, _)| *s == script) {
            Some((_, n)) => *n += 1,
            None => counts.push((script, 1)),
        }
    }
    // Lines of kanji alone beside lines with kana are Japanese too.
    if let Some(hani) = counts.iter().position(|(s, _)| *s == "Hani")
        && counts.iter().any(|(s, _)| *s == "Jpan")
    {
        let (_, n) = counts.remove(hani);
        if let Some((_, j)) = counts.iter_mut().find(|(s, _)| *s == "Jpan") {
            *j += n;
        }
    }
    let Some(&(script, n)) = counts.iter().max_by_key(|(_, n)| *n) else {
        return Some(tag.to_owned());
    };
    let (langs, likely) = written_in(script);
    let base = tag.split(['-', '_']).next().unwrap_or("").to_lowercase();
    if n * 2 <= lettered || langs.is_empty() || langs.contains(&base.as_str()) {
        Some(tag.to_owned())
    } else {
        Some(likely.to_owned())
    }
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

/// A pronunciation in Roman letters, made on the device, for every line in another script
/// that has none yet (one the lyrics carried, or paired from another copy, comes first). Each
/// romanized word keeps the timing of the word it came from. Source `Made`, language like
/// "hi-Latn"; scripts with no reader yet (Urdu, Arabic, Thai) get none, and Japanese lines
/// with kanji get none until the dictionary is installed ([`waits_for_japanese_dictionary`]).
/// One made before is made again, so kept answers follow the reader as it improves (it takes
/// well under a millisecond a song).
pub fn add_pronunciation(lyrics: &mut Lyrics) {
    add_pronunciation_lines(&mut lyrics.lines, lyrics.lang.as_deref());
}

/// [`add_pronunciation`] for lines whose song language is `song_lang` (BCP 47), if known.
pub fn add_pronunciation_lines(lines: &mut [LyricLine], song_lang: Option<&str>) {
    let dictionary = japanese_dictionary();
    pronounce(lines, song_lang, dictionary.as_deref());
}

/// The installed Japanese dictionary's folder, under the app data folder.
pub fn japanese_dictionary() -> Option<std::path::PathBuf> {
    use lyrics_text::japanese::dictionary;
    dictionary::installed(&crate::paths::app_data_dir(), &dictionary::IPADIC)
}

/// Whether some line waits for the Japanese dictionary to get its pronunciation (it has
/// kanji, and the dictionary isn't installed): the app fetches it then
/// ([`install_japanese_dictionary`]) and reads the lyrics again.
pub fn waits_for_japanese_dictionary(lines: &[LyricLine], song_lang: Option<&str>) -> bool {
    japanese_dictionary().is_none() && wants_japanese_dictionary(lines, song_lang)
}

/// Downloads, checks and installs the Japanese dictionary under the app data folder (10.5 MB,
/// from lindera's own release); `progress` gets the stage and returns false to stop.
pub fn install_japanese_dictionary(
    progress: &mut dyn FnMut(lyrics_text::japanese::dictionary::Progress) -> bool,
) -> Result<std::path::PathBuf, String> {
    use lyrics_text::japanese::dictionary;
    dictionary::ensure(&crate::paths::app_data_dir(), &dictionary::IPADIC, progress)
        .map_err(|e| e.to_string())
}

/// Lines that would need the Japanese dictionary: Japanese ones with kanji and no
/// pronunciation from the lyrics.
fn wants_japanese_dictionary(lines: &[LyricLine], song_lang: Option<&str>) -> bool {
    let hint = song_hint(lines, song_lang);
    let japanese_song = hint
        .as_deref()
        .is_some_and(|l| l == "ja" || l.starts_with("ja-"));
    lines.iter().any(|l| {
        l.roman
            .as_ref()
            .is_none_or(|r| r.source == SecondSource::Made)
            && (l.script == "Jpan" || (japanese_song && l.script == "Hani"))
            && lyrics_text::japanese::needs_dictionary(&l.text)
    })
}

/// The song's language, or what its lines say of it when unknown: Japanese when any line is
/// in kana, and for Cyrillic the language its letters show (Ukrainian і, Serbian ђ), read over
/// the whole song since one line may not tell.
fn song_hint(lines: &[LyricLine], song_lang: Option<&str>) -> Option<String> {
    if let Some(l) = song_lang {
        return Some(l.to_owned());
    }
    if lines.iter().any(|l| l.script == "Jpan") {
        return Some("ja".into());
    }
    let cyrillic: Vec<&str> = lines
        .iter()
        .filter(|l| l.script == "Cyrl")
        .map(|l| l.text.as_str())
        .collect();
    (!cyrillic.is_empty()).then(|| {
        lyrics_text::cyrillic::detect(&cyrillic.join(
            "
",
        ))
        .code()
        .to_owned()
    })
}

fn pronounce(
    lines: &mut [LyricLine],
    song_lang: Option<&str>,
    dictionary: Option<&std::path::Path>,
) {
    let hint = song_hint(lines, song_lang);
    let mut words: Vec<lyrics_text::Word> = Vec::new();
    for l in lines {
        if l.roman
            .as_ref()
            .is_some_and(|r| r.source != SecondSource::Made)
        {
            continue;
        }
        l.roman = None;
        if l.text.trim().is_empty() {
            continue;
        }
        words.clear();
        words.extend(l.words.iter().map(|w| lyrics_text::Word {
            start_ms: w.start_ms,
            end_ms: w.end_ms,
            text: w.text.clone(),
        }));
        let line = lyrics_text::Line::new(&l.text)
            .words(&words)
            .lang(hint.as_deref())
            .japanese_dictionary(dictionary);
        if let Some(r) = lyrics_text::romanize(&line) {
            l.roman = Some(Second {
                text: r.text,
                words: r
                    .words
                    .into_iter()
                    .map(|w| Word {
                        start_ms: w.start_ms,
                        end_ms: w.end_ms,
                        text: w.text,
                    })
                    .collect(),
                lang: r.lang,
                source: SecondSource::Made,
            });
        }
    }
}

/// Romanized words timed by the original's timed words: each word (with the `char` positions
/// it came from, in the original words' text put together) from the start of the first word it
/// overlaps to the end of the last. Each but the last ends in a space, so the words put together
/// are the line. Empty when the original has no timed words or a word overlaps none.
pub(crate) fn timed_by_chars(
    timed: &[Word],
    joined: &[(String, std::ops::Range<usize>)],
) -> Vec<Word> {
    if timed.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::with_capacity(timed.len());
    let mut at = 0;
    for w in timed {
        let n = w.text.chars().count();
        spans.push(at..at + n);
        at += n;
    }
    let mut out = Vec::with_capacity(joined.len());
    for (k, (text, range)) in joined.iter().enumerate() {
        let mut over = spans
            .iter()
            .zip(timed)
            .filter(|(s, _)| s.start < range.end && range.start < s.end)
            .map(|(_, w)| w);
        let Some(first) = over.next() else {
            return Vec::new();
        };
        let last = over.next_back().unwrap_or(first);
        let mut text = text.clone();
        if k + 1 < joined.len() {
            text.push(' ');
        }
        out.push(Word {
            start_ms: first.start_ms,
            end_ms: last.end_ms.max(first.start_ms),
            text,
        });
    }
    out
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

// --- Credit lines ---------------------------------------------------------------------------

/// Words that name who made a song, as the Chinese services write them inside the lyrics
/// ("作词 : Ayase", "词：Ayase", "制作人 : Ayase"), and their English kin.
const CREDIT_WORDS: &[&str] = &[
    "作词",
    "作詞",
    "作曲",
    "编曲",
    "編曲",
    "词曲",
    "詞曲",
    "词",
    "詞",
    "曲",
    "制作人",
    "製作人",
    "制作",
    "製作",
    "监制",
    "監製",
    "出品人",
    "演唱",
    "原唱",
    "歌手",
    "混音",
    "混音师",
    "混音師",
    "母带",
    "母带处理",
    "母帶",
    "录音",
    "錄音",
    "录音师",
    "錄音師",
    "和声",
    "和聲",
    "和音",
    "伴唱",
    "配唱",
    "人声",
    "人聲",
    "吉他",
    "贝斯",
    "貝斯",
    "鼓",
    "键盘",
    "鍵盤",
    "钢琴",
    "鋼琴",
    "弦乐",
    "弦樂",
    "合成器",
    "出品",
    "发行",
    "發行",
    "统筹",
    "統籌",
    "企划",
    "企劃",
    "策划",
    "策劃",
    "监修",
    "監修",
    "封面",
    "版权",
    "版權",
    "后期",
    "後期",
    "op",
    "sp",
    "lyrics",
    "lyric",
    "lyricist",
    "composer",
    "composed",
    "music",
    "arranger",
    "arranged",
    "arrangement",
    "producer",
    "produced",
    "writer",
    "written",
    "songwriter",
    "mixed",
    "mixing",
    "mastered",
    "mastering",
    "recorded",
    "recording",
    "vocals",
    "vocal",
];

/// English credits written "Composed by Someone".
const BY_WORDS: &[&str] = &[
    "lyrics", "lyric", "words", "music", "composed", "written", "arranged", "produced", "mixed",
    "mastered", "recorded", "vocals",
];

/// How the Chinese services end the name of a job on a song, or of who holds its rights, when
/// they make one up from smaller words ("词版权管理方", "录音作品及MV版权", "联合出品",
/// "出品监制", "人声录音室", "音乐制作人"): a head in Chinese characters ending in one of these
/// names a job, not words to sing.
const JOB_ENDINGS: &[&str] = &[
    "版权",
    "版權",
    "管理方",
    "出品",
    "出品方",
    "出品人",
    "监制",
    "監製",
    "制作",
    "製作",
    "制作人",
    "製作人",
    "录音",
    "錄音",
    "录音师",
    "錄音師",
    "录音室",
    "錄音室",
    "录音棚",
    "錄音棚",
    "混音",
    "混音师",
    "混音師",
    "母带",
    "母帶",
    "编辑",
    "編輯",
    "编写",
    "編寫",
    "统筹",
    "統籌",
    "企划",
    "企劃",
    "策划",
    "策劃",
    "发行",
    "發行",
    "工程师",
    "工程師",
    "总监",
    "總監",
    "推广",
    "推廣",
    "宣传",
    "宣傳",
    "设计",
    "設計",
    "鸣谢",
    "鳴謝",
];

/// Whether a credit line's head (before its colon) is a job's name made up from smaller words:
/// short, in Chinese characters (with Latin letters for a name like "MV"), ending in a job's
/// name ([`JOB_ENDINGS`]). A sung line before a colon ("他说：…") ends in no such word.
fn names_a_job(head: &str) -> bool {
    use lyrics_text::script::Script;
    let han = |c: char| Script::of(c) == Script::Hani;
    head.chars().count() <= 16
        && head.chars().any(han)
        && head.chars().all(|c| han(c) || c.is_ascii_alphanumeric())
        && JOB_ENDINGS.iter().any(|e| head.ends_with(e))
}

/// Whether a lyric line names who made the song instead of being sung: "作词 : Ayase",
/// "作曲/编曲：X", "Lyrics by: X", "Composed by X", and who holds its rights or put it out
/// ("词版权管理方：X", "联合出品：X", "OP/SP：X").
pub fn is_credit(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    // Credit words (joined by / & and the like), then a colon and a name.
    if let Some(i) = t.find([':', '：']) {
        let head = t[..i].trim();
        let name = t[i..].trim_start_matches([':', '：']).trim();
        let credit_word = |part: &str| {
            let p = part.trim().to_lowercase();
            let p = p.strip_suffix(" by").unwrap_or(&p).trim();
            CREDIT_WORDS.contains(&p) || names_a_job(p)
        };
        if !name.is_empty()
            && !head.is_empty()
            && head
                .split(['/', '&', '、', ',', '，', '|', '｜'])
                .all(credit_word)
        {
            return true;
        }
    }
    // "Composed by Someone".
    let lower = t.to_lowercase();
    BY_WORDS.iter().any(|w| {
        lower
            .strip_prefix(w)
            .and_then(|r| r.strip_prefix(" by "))
            .is_some_and(|name| !name.trim().is_empty())
    })
}

/// Credit lines taken out of the sung lines (in order), the rest kept.
pub fn split_credits(lines: Vec<LyricLine>) -> (Vec<LyricLine>, Vec<String>) {
    let mut credits = Vec::new();
    let mut sung = Vec::new();
    for l in lines {
        if is_credit(&l.text) {
            credits.push(l.text.trim().to_owned());
        } else {
            sung.push(l);
        }
    }
    (sung, credits)
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
    fn a_pronunciation_is_made_for_lines_in_another_script() {
        let mut lyrics = Lyrics {
            lines: vec![
                LyricLine {
                    start_ms: 0,
                    end_ms: 2_000,
                    text: "जो वादे किये".into(),
                    words: vec![
                        Word {
                            start_ms: 0,
                            end_ms: 400,
                            text: "जो ".into(),
                        },
                        Word {
                            start_ms: 400,
                            end_ms: 900,
                            text: "वादे ".into(),
                        },
                        Word {
                            start_ms: 900,
                            end_ms: 2_000,
                            text: "किये".into(),
                        },
                    ],
                    ..Default::default()
                },
                LyricLine {
                    start_ms: 2_000,
                    text: "Kesariya tera".into(),
                    ..Default::default()
                },
                LyricLine {
                    start_ms: 4_000,
                    text: "मन".into(),
                    roman: Some(Second {
                        text: "Mann".into(),
                        words: Vec::new(),
                        lang: "hi-Latn".into(),
                        source: SecondSource::Lyrics("lyricsplus".into()),
                    }),
                    ..Default::default()
                },
            ],
            lang: Some("hi".into()),
            version: VERSION,
        };
        add_pronunciation(&mut lyrics);
        let made = lyrics.lines[0].roman.as_ref().expect("made");
        assert_eq!(made.text, "Jo waade kiye");
        assert_eq!(made.lang, "hi-Latn");
        assert_eq!(made.source, SecondSource::Made);
        let timed: Vec<(u64, u64, &str)> = made
            .words
            .iter()
            .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
            .collect();
        assert_eq!(
            timed,
            [(0, 400, "Jo "), (400, 900, "waade "), (900, 2_000, "kiye")]
        );
        assert_eq!(lyrics.lines[1].roman, None, "a Latin line needs none");
        assert_eq!(
            lyrics.lines[2].roman.as_ref().map(|r| r.text.as_str()),
            Some("Mann"),
            "the lyrics' own is kept"
        );
        let once = lyrics.clone();
        add_pronunciation(&mut lyrics);
        assert_eq!(lyrics, once, "made again, the same");
    }

    fn line_of(text: &str) -> LyricLine {
        let mut l = LyricLine {
            text: text.into(),
            ..Default::default()
        };
        finish(std::slice::from_mut(&mut l));
        l
    }

    #[test]
    fn a_ready_made_pronunciation_always_beats_one_made_on_the_device() {
        let ready = |text: &str, source: SecondSource| Second {
            text: text.into(),
            words: Vec::new(),
            lang: "hi-Latn".into(),
            source,
        };
        let mut lyrics = Lyrics {
            lines: vec![
                LyricLine {
                    roman: Some(ready(
                        "Mujhko itna bataye koyi",
                        SecondSource::Paired("lyricsplus".into()),
                    )),
                    ..line_of("मुझको इतना बताए कोई")
                },
                LyricLine {
                    roman: Some(ready("Kaise tujhse", SecondSource::Lyrics("amll".into()))),
                    ..line_of("कैसे तुझसे")
                },
                LyricLine {
                    roman: Some(ready("Stale guess", SecondSource::Made)),
                    ..line_of("रब्बा ने")
                },
                LyricLine {
                    roman: Some(ready("Rang jaun", SecondSource::Corrected)),
                    ..line_of("रंग जाऊँ")
                },
            ],
            lang: Some("hi".into()),
            version: VERSION,
        };
        add_pronunciation(&mut lyrics);
        let romans: Vec<(&str, &SecondSource)> = lyrics
            .lines
            .iter()
            .map(|l| {
                let r = l.roman.as_ref().unwrap();
                (r.text.as_str(), &r.source)
            })
            .collect();
        assert_eq!(romans[0].0, "Mujhko itna bataye koyi", "paired: kept");
        assert_eq!(romans[1].0, "Kaise tujhse", "from the lyrics: kept");
        assert_eq!(romans[3].0, "Rang jaun", "corrected by hand: kept");
        assert_eq!(
            romans[2],
            ("Rabba ne", &SecondSource::Made),
            "one made before is made again"
        );
    }

    #[test]
    fn pronunciation_for_every_script_family() {
        let mut lines = vec![
            line_of("사랑해"),
            line_of("我爱你"),
            line_of("Я тебя люблю"),
            line_of("Σ' αγαπώ"),
        ];
        pronounce(&mut lines, None, None);
        let made: Vec<(&str, &str)> = lines
            .iter()
            .map(|l| {
                let r = l.roman.as_ref().expect("made");
                assert_eq!(r.source, SecondSource::Made);
                (r.text.as_str(), r.lang.as_str())
            })
            .collect();
        assert_eq!(
            made,
            [
                ("Saranghae", "ko-Latn"),
                ("Wo ai ni", "zh-Latn"),
                ("Ya tebya lyublyu", "ru-Latn"),
                ("S' agapo", "el-Latn"),
            ]
        );
    }

    #[test]
    fn japanese_lines_with_kanji_wait_for_the_dictionary() {
        let mut lines = vec![line_of("君の名は"), line_of("さよなら"), line_of("未来")];
        pronounce(&mut lines, None, None);
        assert_eq!(lines[0].roman, None);
        assert_eq!(
            lines[1].roman.as_ref().map(|r| r.text.as_str()),
            Some("Sayonara")
        );
        // The kana lines make it a Japanese song: kanji alone are not read as Chinese.
        assert_eq!(lines[2].roman, None);
        assert!(wants_japanese_dictionary(&lines, None));
        let chinese = vec![line_of("我爱你")];
        assert!(!wants_japanese_dictionary(&chinese, None));
        let korean = vec![line_of("사랑해")];
        assert!(!wants_japanese_dictionary(&korean, None));
    }

    #[test]
    fn the_whole_song_tells_its_cyrillic_language() {
        // One line has no letter only Ukrainian uses; another line does.
        let mut lines = vec![line_of("Я тебе кохаю"), line_of("моя Україно")];
        pronounce(&mut lines, None, None);
        let r = lines[0].roman.as_ref().unwrap();
        assert_eq!(
            (r.text.as_str(), r.lang.as_str()),
            ("Ya tebe kokhaiu", "uk-Latn")
        );
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

    #[test]
    fn credit_lines_are_known() {
        for credit in [
            " 作词 : Ayase",
            "作曲 : Ayase",
            "编曲 : Ayase",
            "制作人 : Ayase",
            "词：Ayase",
            "曲：Ayase",
            "作词/作曲：Ayase",
            "Lyrics by: Ayase",
            "Composed by Ayase",
            "Producer: Someone",
        ] {
            assert!(is_credit(credit), "{credit}");
        }
        for sung in [
            "無敵の笑顔で荒らすメディア",
            "You're my savior",
            "Music is my life",
            "Time: it's running out",
            "曲",
            "Written in the stars",
        ] {
            assert!(!is_credit(sung), "{sung}");
        }
    }

    #[test]
    fn copyright_and_production_lines_are_credits() {
        // As NetEase sends them after the words (孤勇者, 2026-10-22), and the lines it writes the
        // same way for other songs.
        for credit in [
            "词版权管理方：北京梦织音传媒有限公司",
            "曲版权管理方：索尼音乐版权代理（北京）有限公司",
            "录音作品及MV版权：EAS MUSIC LTD",
            "联合出品：拳头游戏/腾讯游戏/腾讯视频",
            "出品监制：霍锦 卢泓宇",
            "出品：网易云音乐",
            "监制：Someone",
            "OP/SP：Sony Music",
            "制作人：钱雷",
            "人声录音室：雅旺录音室",
            "人声编辑: 汝文博",
            "音乐制作：Someone",
            "版權管理方：Someone",
        ] {
            assert!(is_credit(credit), "{credit}");
        }
        // Sung lines that happen to hold a colon stay sung.
        for sung in [
            "他说：我爱你",
            "曲终人散：只剩下我",
            "我对你说：不要走",
            "出品：",
            "版权",
            "Time: it's running out",
            "Baby: you are the one",
            "听我说：这世界出品的梦",
            "愛してる：ずっと",
        ] {
            assert!(!is_credit(sung), "{sung}");
        }
    }

    fn lines_of(texts: &[&str]) -> Vec<LyricLine> {
        let mut lines: Vec<LyricLine> = texts
            .iter()
            .map(|t| LyricLine {
                text: (*t).to_owned(),
                ..Default::default()
            })
            .collect();
        finish(&mut lines);
        lines
    }

    #[test]
    fn a_songs_language_is_its_label_unless_most_of_its_lines_letters_disprove_it() {
        let hindi = lines_of(&["तू मेरा कोई ना होके भी कुछ लागे", "किया रे जो भी तूने", "♪"]);
        assert_eq!(song_lang(Some("en"), &hindi).as_deref(), Some("hi"));
        assert_eq!(song_lang(Some("mr"), &hindi).as_deref(), Some("mr"));
        // Nothing said: nothing made up (the views read the letters themselves).
        assert_eq!(song_lang(None, &hindi), None);
        // Mostly English with a Hindi line or two: the label stands.
        let mostly_english = lines_of(&[
            "I want you to stay",
            "Every night and day",
            "Hold me close tonight",
            "तू मेरा कोई ना होके भी",
        ]);
        assert_eq!(
            song_lang(Some("en"), &mostly_english).as_deref(),
            Some("en")
        );
        // Latin letters can be any language's.
        let latin = lines_of(&["Tu mera koi na hoke bhi kuchh laage"]);
        assert_eq!(song_lang(Some("id"), &latin).as_deref(), Some("id"));
        // Korean and Punjabi lines labelled English; Chinese lines labelled Japanese stand
        // (Japanese is written with Chinese characters too).
        let korean = lines_of(&["사랑해 너를", "보고 싶어"]);
        assert_eq!(song_lang(Some("en"), &korean).as_deref(), Some("ko"));
        let punjabi = lines_of(&["ਤੇਰੀ ਹੋਕੇ ਮਰਾਂ", "ਹੀਰੀਏ-ਹੀਰੀਏ, ਆ"]);
        assert_eq!(song_lang(Some("hi"), &punjabi).as_deref(), Some("pa"));
        let chinese = lines_of(&["我们都是勇敢的", "爱你孤身走暗巷"]);
        assert_eq!(song_lang(Some("ja"), &chinese).as_deref(), Some("ja"));
        assert_eq!(song_lang(Some("en"), &chinese).as_deref(), Some("zh"));
        // No letters at all: nothing to go on.
        assert_eq!(
            song_lang(Some("en"), &lines_of(&["♪", "..."])).as_deref(),
            Some("en")
        );
    }
}
