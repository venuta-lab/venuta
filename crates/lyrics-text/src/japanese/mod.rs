//! Japanese in Hepburn romaji, word by word: "kimi no na wa", "yume naraba".
//!
//! Kana are read by a table ([`kana`]); kanji need a dictionary. With the IPADIC dictionary
//! (through lindera, MIT; the dictionary itself is downloaded the first time a Japanese line
//! needs it, see [`dictionary`]) each line is cut into words and each word read as the
//! dictionary reads it. Particles read as they are said: は "wa", へ "e", を "o". Endings join
//! their word ("yokatta", "shiranai", "shite"), particles stand apart.
//!
//! Until the dictionary is there, lines in kana alone still come out (words cut at spaces and
//! where hiragana turns to katakana, は read "ha"); a line with kanji gives [`NeedsDictionary`].
//!
//! Each kana is a piece of its own and a kanji's reading points at the kanji, so a line timed
//! syllable by syllable keeps its timing on the Roman line; a reading that spans several timed
//! kanji takes the time of all of them.

pub mod dictionary;
mod kana;

use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;

use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

use crate::chinese::{is_han, other};
use crate::pieces::{Builder, Romanized, is_kana};
use kana::{Kana, hiragana, write_word};

/// The BCP 47 tag of the pronunciation line.
pub const LANG: &str = "ja-Latn";

/// The line has kanji and the dictionary isn't loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("this line needs the Japanese dictionary")]
pub struct NeedsDictionary;

/// The dictionary folder could not be read.
#[derive(Debug, thiserror::Error)]
#[error("could not load the Japanese dictionary: {0}")]
pub struct LoadError(String);

/// Whether a line has kanji, and so needs the dictionary to be read.
pub fn needs_dictionary(line: &str) -> bool {
    line.chars().any(|c| is_han(c) || c == '々')
}

/// A Japanese romanizer: kana alone, or with the dictionary loaded.
pub struct Japanese {
    segmenter: Option<Segmenter>,
}

impl Japanese {
    /// Without the dictionary: lines in kana only.
    pub fn kana_only() -> Self {
        Japanese { segmenter: None }
    }

    /// With the dictionary unpacked in `dir` (as [`dictionary::install`] leaves it). Loading
    /// takes a moment: keep one and reuse it.
    pub fn with_dictionary(dir: &Path) -> Result<Self, LoadError> {
        let dict =
            lindera::dictionary::load_fs_dictionary(dir).map_err(|e| LoadError(e.to_string()))?;
        Ok(Japanese {
            segmenter: Some(Segmenter::new(Mode::Normal, dict, None)),
        })
    }

    pub fn has_dictionary(&self) -> bool {
        self.segmenter.is_some()
    }

    /// The line in romaji.
    pub fn romanize(&self, line: &str) -> Result<Romanized, NeedsDictionary> {
        match &self.segmenter {
            Some(s) => Ok(with_words(s, line)),
            None if needs_dictionary(line) => Err(NeedsDictionary),
            None => Ok(kana_words(line)),
        }
    }
}

/// Kana only, cut at spaces, punctuation and where hiragana turns to katakana.
fn kana_words(line: &str) -> Romanized {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut b = Builder::default();
    let mut word: Vec<Kana> = Vec::new();
    let mut last_kata: Option<bool> = None;
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if is_kana(c) && !matches!(c, '・' | '･') {
            let kata = ('\u{30A0}'..='\u{30FF}').contains(&c);
            // ー goes with the word it lengthens.
            if c != 'ー' {
                if last_kata.is_some_and(|k| k != kata) {
                    flush(&mut word, &mut b);
                }
                last_kata = Some(kata);
            }
            word.push(Kana {
                c: hiragana(c),
                src: at..at + c.len_utf8(),
            });
            i += 1;
            continue;
        }
        flush(&mut word, &mut b);
        last_kata = None;
        i = other(&chars, i, &mut b);
    }
    flush(&mut word, &mut b);
    b.finish()
}

fn flush(word: &mut Vec<Kana>, b: &mut Builder) {
    if !word.is_empty() {
        b.split();
        write_word(word, b);
        word.clear();
    }
}

/// What the dictionary says about a word.
struct Tok<'a> {
    src: Range<usize>,
    surface: &'a str,
    pos: String,
    sub: String,
    base: String,
    reading: Option<String>,
}

fn tokens<'a>(s: &'a Segmenter, line: &'a str) -> Vec<Tok<'a>> {
    let Ok(mut toks) = s.segment(Cow::Borrowed(line)) else {
        return Vec::new();
    };
    toks.iter_mut()
        .map(|t| {
            let src = t.byte_start..t.byte_end;
            let d: Vec<String> = t.details().into_iter().map(str::to_owned).collect();
            let field = |i: usize| d.get(i).filter(|f| *f != "*").cloned();
            let mut reading = field(7);
            // は and へ said "wa" and "e" (particles, こんにちは): the pronunciation says so.
            if let (Some(r), Some(p)) = (&mut reading, field(8)) {
                let surface = &line[src.clone()];
                for (kana, said, read) in [('は', 'ワ', "ワ"), ('へ', 'エ', "エ")] {
                    if surface.ends_with(kana) && p.ends_with(said) {
                        r.pop();
                        r.push_str(read);
                    }
                }
            }
            Tok {
                surface: &line[src.clone()],
                src,
                pos: field(0).unwrap_or_default(),
                sub: field(1).unwrap_or_default(),
                base: field(6).unwrap_or_default(),
                reading,
            }
        })
        .collect()
}

/// Whether a word joins the one before: endings, suffixes, and て, ば after a verb.
fn joins(prev: &Tok, t: &Tok) -> bool {
    let inflects = matches!(prev.pos.as_str(), "動詞" | "形容詞" | "助動詞");
    match t.pos.as_str() {
        _ if prev.pos == "接頭詞" => true,
        "助動詞" => inflects && !matches!(t.base.as_str(), "だ" | "です"),
        "動詞" | "名詞" | "形容詞" => t.sub == "接尾",
        "助詞" => {
            inflects
                && t.sub == "接続助詞"
                && matches!(
                    t.surface,
                    "て" | "で" | "ば" | "ても" | "でも" | "たり" | "だり" | "ちゃ"
                )
        }
        _ => false,
    }
}

/// A word's kana, each pointing at its source: kana written in the word point at themselves,
/// the reading of its kanji at the kanji.
fn aligned(line: &str, t: &Tok) -> Vec<Kana> {
    let surface: Vec<(usize, char)> = line[t.src.clone()]
        .char_indices()
        .map(|(i, c)| (t.src.start + i, c))
        .collect();
    let src = |&(at, c): &(usize, char)| at..at + c.len_utf8();
    let reading: Vec<char> = match &t.reading {
        Some(r) => r.chars().map(hiragana).collect(),
        None if surface.iter().all(|&(_, c)| is_kana(c)) => {
            surface.iter().map(|&(_, c)| hiragana(c)).collect()
        }
        // No reading for kanji: keep them as written.
        None => {
            return surface
                .iter()
                .map(|s| Kana {
                    c: s.1,
                    src: src(s),
                })
                .collect();
        }
    };
    let same = |s: &(usize, char), r: char| is_kana(s.1) && hiragana(s.1) == r;
    let mut p = 0;
    while p < surface.len() && p < reading.len() && same(&surface[p], reading[p]) {
        p += 1;
    }
    let mut q = 0;
    while q < surface.len() - p
        && q < reading.len() - p
        && same(
            &surface[surface.len() - 1 - q],
            reading[reading.len() - 1 - q],
        )
    {
        q += 1;
    }
    let mut out: Vec<Kana> = Vec::with_capacity(reading.len());
    out.extend((0..p).map(|k| Kana {
        c: reading[k],
        src: src(&surface[k]),
    }));
    let middle = &surface[p..surface.len() - q];
    let middle_src = match (middle.first(), middle.last()) {
        (Some(a), Some(z)) => Some(a.0..z.0 + z.1.len_utf8()),
        // The reading has kana the writing doesn't: they go with the kana before them.
        _ => surface.get(p.saturating_sub(1)).map(src),
    };
    if let Some(m) = middle_src {
        out.extend(
            reading[p..reading.len() - q]
                .iter()
                .map(|&c| Kana { c, src: m.clone() }),
        );
    }
    let s0 = surface.len() - q;
    let r0 = reading.len() - q;
    out.extend((0..q).map(|k| Kana {
        c: reading[r0 + k],
        src: src(&surface[s0 + k]),
    }));
    out
}

fn japanese(c: char) -> bool {
    is_kana(c) && !matches!(c, '・' | '･') || is_han(c) || c == '々'
}

fn with_words(s: &Segmenter, line: &str) -> Romanized {
    let toks = tokens(s, line);
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut b = Builder::default();
    let mut word: Vec<Kana> = Vec::new();
    let mut prev: Option<&Tok> = None;
    let mut at = 0;
    for t in &toks {
        if t.src.start > at && line[at..t.src.start].chars().any(char::is_whitespace) {
            flush(&mut word, &mut b);
            prev = None;
        }
        at = t.src.end;
        if !t.surface.chars().any(japanese) {
            // Punctuation, Latin words, digits: as the Chinese reader places them.
            flush(&mut word, &mut b);
            prev = None;
            let from = chars.partition_point(|&(i, _)| i < t.src.start);
            let to = chars.partition_point(|&(i, _)| i < t.src.end);
            let mut i = from;
            while i < to {
                i = other(&chars[..to], i, &mut b);
            }
            continue;
        }
        if !prev.is_some_and(|p| joins(p, t)) {
            flush(&mut word, &mut b);
        }
        word.extend(aligned(line, t));
        prev = Some(t);
    }
    flush(&mut word, &mut b);
    b.finish()
}
