//! Second lines for lyrics: a sung line in another script, said in English letters (the
//! "pronunciation" line), timed by the original's words so karaoke runs on both lines.
//!
//! Pure Rust with no platform code, so the desktop app and Android share it. Nothing here
//! reads the network except [`japanese::dictionary`], and only when asked.
//!
//! How a line is read ([`romanize`]):
//! 1. The line is split into runs of one script ([`script::runs`]). Latin words, digits,
//!    spaces and punctuation are kept as they are. Chinese and Japanese, written without
//!    spaces, are read a stretch at a time (kana, kanji and their punctuation together), so
//!    words and phrases are read whole.
//! 2. Each run goes to its script family's module, which writes its romanized text into
//!    [`Pieces`], keeping which source bytes every piece came from:
//!    - [`indic`] reads the Indian scripts, the casual way people write them;
//!    - [`korean`] writes Revised Romanization, with the sound changes between syllables;
//!    - [`chinese`] writes Mandarin pinyin, phrase-aware, Simplified and Traditional;
//!    - [`japanese`] writes Hepburn romaji, kana always, kanji once the dictionary is there;
//!    - [`cyrillic`] reads Russian, Ukrainian, Bulgarian and Serbian;
//!    - [`greek`] reads Greek.
//! 3. [`timing::words`] gives each romanized word the timing of the original word(s) it came from.
//!
//! Adding a script family: a `pub mod` here, and one arm in [`convert`]. The module's entry
//! point reads `line[range]`, pushes its pieces with absolute byte ranges into `out`, and says
//! which language it read the run as.

use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub mod chinese;
pub mod cyrillic;
pub mod greek;
pub mod indic;
#[cfg(feature = "japanese")]
pub mod japanese;
pub mod korean;
mod layout;
pub mod script;
pub mod timing;

pub use script::Script;
pub use timing::Pieces;

/// One timed word of a line (or of a second line).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// A line in English letters: its pronunciation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roman {
    pub text: String,
    /// Each romanized word timed by the original word(s) it came from. Their texts, put
    /// together, are `text`. Empty when the original has no word timing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
    /// BCP 47 of the pronunciation: "hi-Latn".
    pub lang: String,
}

/// A lyric line to romanize.
#[derive(Debug, Clone, Copy, Default)]
pub struct Line<'a> {
    /// The line as sung.
    pub text: &'a str,
    /// Its timed words, when known: their texts appear in `text` in order.
    pub words: &'a [Word],
    /// The song's language, when known (BCP 47 or ISO 639: "hi", "mr", "pan", "ta-IN").
    /// Picks the rules where a script has several languages (Devanagari: Hindi, Marathi,
    /// Nepali, Sanskrit; Cyrillic: Russian, Ukrainian, Bulgarian, Serbian), and reads Han
    /// characters as Japanese in a Japanese song. Without it each script is read as its most
    /// common language.
    pub lang: Option<&'a str>,
    /// The Japanese dictionary's folder, when it is installed
    /// ([`japanese::dictionary::installed`]). Without it, Japanese lines with kanji get no
    /// pronunciation ([`japanese::needs_dictionary`] says which).
    pub japanese_dictionary: Option<&'a Path>,
}

impl<'a> Line<'a> {
    pub fn new(text: &'a str) -> Self {
        Line {
            text,
            words: &[],
            lang: None,
            japanese_dictionary: None,
        }
    }

    pub fn words(self, words: &'a [Word]) -> Self {
        Line { words, ..self }
    }

    pub fn lang(self, lang: Option<&'a str>) -> Self {
        Line { lang, ..self }
    }

    pub fn japanese_dictionary(self, dir: Option<&'a Path>) -> Self {
        Line {
            japanese_dictionary: dir,
            ..self
        }
    }
}

/// The line in English letters, or `None` when there is nothing to romanize (the line is
/// already Latin, or its script has no reader here), or when it is Japanese with kanji and
/// the dictionary isn't installed yet.
pub fn romanize(line: &Line) -> Option<Roman> {
    let text = line.text;
    let mut out = Pieces::with_capacity(text, text.len() + text.len() / 2);
    // Letters read per language, for the pronunciation's tag (a line rarely has more than two).
    let mut read: [(&'static str, usize); 4] = [("", 0); 4];
    let mut waits = false;
    let mut pos = 0;
    while pos < text.len() {
        let (range, lang) = match cjk_stretch(text, pos) {
            Some(end) => {
                let range = pos..end;
                (range.clone(), cjk(line, range, &mut out, &mut waits))
            }
            None => {
                let run = script::runs(&text[pos..]).next()?;
                let range = pos..family_end(run.script, text, pos + run.range.end);
                (
                    range.clone(),
                    convert(run.script, text, range, line.lang, &mut out),
                )
            }
        };
        match lang {
            Some(lang) => {
                if let Some(slot) = read.iter_mut().find(|(l, n)| *l == lang || *n == 0) {
                    slot.0 = lang;
                    slot.1 += range.len();
                }
            }
            None => out.keep(range.clone()),
        }
        pos = range.end;
    }
    if waits {
        return None;
    }
    let (lang, _) = read
        .iter()
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)?;
    out.capitalize_first();
    let words = timing::words(text, line.words, &out);
    Some(Roman {
        text: out.into_text(),
        words,
        lang: format!("{lang}-Latn"),
    })
}

/// One script run, romanized into `out` by its family's module. Returns the language it was
/// read as, or `None` when this script has no reader (the caller keeps the run as it is).
fn convert(
    script: Script,
    line: &str,
    range: Range<usize>,
    hint: Option<&str>,
    out: &mut Pieces,
) -> Option<&'static str> {
    match script {
        s if indic::covers(s) => indic::romanize(s, line, range, hint, out),
        Script::Hang => {
            korean::write(line, range, korean::Options::default(), out);
            Some(korean::LANG)
        }
        Script::Cyrl => {
            let lang = hint
                .and_then(cyrillic::Lang::from_tag)
                .unwrap_or_else(|| cyrillic::detect(line));
            cyrillic::write(line, range, lang, out);
            Some(lang.code())
        }
        Script::Grek => {
            greek::write(line, range, out);
            Some(greek::LANG)
        }
        _ => None,
    }
}

/// Where a run of `script` starting at `start` ends, taking in what belongs to it beyond its
/// own letters: an apostrophe inside a Cyrillic word (Ukrainian м'ята), the Greek question
/// mark (";") after a Greek word.
fn family_end(script: Script, text: &str, end: usize) -> usize {
    let mut end = end;
    match script {
        Script::Cyrl => {
            while let Some(rest) = text.get(end..) {
                let mut cs = rest.chars();
                match (cs.next(), cs.next()) {
                    (Some('\'' | '’' | 'ʼ'), Some(c)) if Script::of(c) == Script::Cyrl => {
                        let after = end + rest.chars().next().map_or(0, char::len_utf8);
                        end = script::runs(&text[after..])
                            .next()
                            .map_or(after, |r| after + r.range.end);
                    }
                    _ => break,
                }
            }
        }
        Script::Grek => {
            while text[end..].starts_with([';', '\u{037E}', '\u{0387}']) {
                end += text[end..].chars().next().map_or(1, char::len_utf8);
            }
        }
        _ => {}
    }
    end
}

/// A stretch of Chinese or Japanese starting at `pos`: from its first kana or kanji (or an
/// opening mark before one) to its last, with the punctuation, spaces and digits between them
/// and the CJK marks after. `None` when `pos` starts no such stretch.
fn cjk_stretch(text: &str, pos: usize) -> Option<usize> {
    let mut end = None;
    for (i, c) in text[pos..].char_indices() {
        let at = pos + i + c.len_utf8();
        match Script::of(c) {
            Script::Hira | Script::Kana | Script::Hani => end = Some(at),
            Script::Zinh => {
                if end.is_some() {
                    end = Some(at);
                }
            }
            s if s.is_letters() => break,
            _ if c.is_ascii() => {
                end?;
            }
            // CJK punctuation and the ideographic space: part of the stretch.
            _ => {
                if end.is_some() && layout::cjk_punct(c).is_some() {
                    end = Some(at);
                } else if end.is_none() && layout::cjk_punct(c).is_none() {
                    return None;
                }
            }
        }
    }
    end
}

/// A stretch of Chinese or Japanese, romanized into `out`: Japanese when it has kana or the
/// song is Japanese, else Mandarin. Spaces keep it apart from words of other scripts around it
/// ("wo shi Jay"). `waits` is set when it is Japanese with kanji and the dictionary isn't
/// installed.
fn cjk(
    line: &Line,
    range: Range<usize>,
    out: &mut Pieces,
    waits: &mut bool,
) -> Option<&'static str> {
    let text = line.text;
    let part = &text[range.clone()];
    let kana = part.chars().any(layout::is_kana);
    let japanese_song = line
        .lang
        .is_some_and(|l| l.eq_ignore_ascii_case("ja") || l.to_ascii_lowercase().starts_with("ja-"));
    if out.ends_in_word() {
        out.space();
    }
    let lang = if kana || japanese_song {
        japanese_into(line, range.clone(), out, waits)
    } else {
        chinese::write(text, range.clone(), chinese::Options::default(), out);
        Some(chinese::LANG)
    };
    let next_is_word = text[range.end..]
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric);
    if lang.is_some() && next_is_word {
        out.space();
    }
    lang
}

#[cfg(feature = "japanese")]
fn japanese_into(
    line: &Line,
    range: Range<usize>,
    out: &mut Pieces,
    waits: &mut bool,
) -> Option<&'static str> {
    let reader = japanese::shared(line.japanese_dictionary);
    match reader.write(line.text, range, out) {
        Ok(()) => Some(japanese::LANG),
        Err(japanese::NeedsDictionary) => {
            *waits = true;
            None
        }
    }
}

#[cfg(not(feature = "japanese"))]
fn japanese_into(_: &Line, _: Range<usize>, _: &mut Pieces, _: &mut bool) -> Option<&'static str> {
    None
}
