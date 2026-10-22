//! Mandarin Chinese in Hanyu Pinyin, one syllable per character, tone marks optional (off by
//! default: lyrics lines read easier without them).
//!
//! Readings come from pinyin-pro's Rust port (`pinyin_pro`, MIT), which segments the line
//! into words so polyphonic characters are read in context (银行 "yin hang", 行长 "hang
//! zhang"). A short table of our own fixes the polyphones it gets wrong in lyrics (还给
//! "huan gei", 慢慢地 "man man de"). Traditional characters are read through their simplified
//! form (OpenCC's table, Apache-2.0, `data/opencc-ts.txt`), so the phrase table finds them;
//! the original characters stay in the line.
//!
//! Cantonese (Jyutping) is not offered: the one Rust crate (`rust-canto`) carries its readings
//! under CC BY 4.0 and builds zstd from C, so it is not a clean dependency for Android.

use std::collections::HashMap;
use std::sync::OnceLock;

use pinyin_pro::options::{PinyinOptions, PinyinOutput, ToneType, TypeMode};

use crate::pieces::{Builder, Romanized, cjk_punct, narrow};

/// How to write the line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Tone marks ("nǐ hǎo"). Off by default ("ni hao").
    pub tones: bool,
}

/// The BCP 47 tag of the pronunciation line.
pub const LANG: &str = "zh-Latn-pinyin";

/// A Han character (CJK ideographs and their extensions, compatibility ideographs, 〇).
pub fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F | 0x3007)
}

/// The line in pinyin without tone marks.
pub fn romanize(line: &str) -> Romanized {
    romanize_with(line, Options::default())
}

/// The line in pinyin.
pub fn romanize_with(line: &str, opts: Options) -> Romanized {
    let mut b = Builder::default();
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_han(chars[i].1) {
            let start = i;
            while i < chars.len() && is_han(chars[i].1) {
                i += 1;
            }
            han_run(line, &chars[start..i], opts, &mut b);
            continue;
        }
        i = other(&chars, i, &mut b);
    }
    b.finish()
}

/// Anything that isn't Han at `i`: a space, a punctuation mark, or a word as written (Latin
/// letters, digits). Returns where the next thing starts.
pub(crate) fn other(chars: &[(usize, char)], mut i: usize, b: &mut Builder) -> usize {
    let (at, c) = chars[i];
    let range = at..at + c.len_utf8();
    if c.is_whitespace() {
        b.split();
        return i + 1;
    }
    if let Some((s, sits)) = cjk_punct(c).filter(|_| !inside_latin(chars, i)) {
        b.punct(s, sits, range);
        return i + 1;
    }
    b.split();
    let mut word = String::new();
    let mut end = at;
    while i < chars.len() {
        let (at, c) = chars[i];
        let stop = c.is_whitespace()
            || is_han(c)
            || crate::pieces::is_kana(c)
            || (cjk_punct(c).is_some() && !inside_latin(chars, i));
        if stop {
            break;
        }
        word.push(narrow(c));
        end = at + c.len_utf8();
        i += 1;
    }
    b.push(&word, range.start..end);
    b.split();
    i
}

/// An ASCII mark between two Latin letters or digits ("3.14", "don't") is part of the word.
fn inside_latin(chars: &[(usize, char)], i: usize) -> bool {
    let alnum =
        |k: Option<&(usize, char)>| k.is_some_and(|(_, c)| narrow(*c).is_ascii_alphanumeric());
    chars[i].1.is_ascii() && i > 0 && alnum(chars.get(i - 1)) && alnum(chars.get(i + 1))
}

fn han_run(line: &str, run: &[(usize, char)], opts: Options, b: &mut Builder) {
    let simplified: Vec<char> = run.iter().map(|&(_, c)| simplify(c)).collect();
    let text: String = simplified.iter().collect();
    let mut syllables = readings(&text);
    fix_polyphones(&simplified, &mut syllables);
    for (&(at, c), syllable) in run.iter().zip(&syllables) {
        let src = at..at + c.len_utf8();
        b.split();
        if syllable.is_empty() {
            b.push(&line[src.clone()], src);
        } else if opts.tones {
            b.push(syllable, src);
        } else {
            b.push(&strip_tones(syllable), src);
        }
    }
}

/// One toned syllable per character (empty when pinyin-pro has no reading for it).
fn readings(text: &str) -> Vec<String> {
    let n = text.chars().count();
    let opts = PinyinOptions {
        tone_type: ToneType::Symbol,
        type_mode: TypeMode::Array,
        ..PinyinOptions::default()
    };
    let unread = |s: String, c: char| if s.chars().eq([c]) { String::new() } else { s };
    match pinyin_pro::pinyin(text, opts) {
        PinyinOutput::Arr(v) if v.len() == n => v
            .into_iter()
            .zip(text.chars())
            .map(|(s, c)| unread(s, c))
            .collect(),
        // Not expected for a run of Han characters; read them one by one.
        _ => text
            .chars()
            .map(|c| unread(pinyin_pro::get_single_word_pinyin(&c.to_string()), c))
            .collect(),
    }
}

/// A character in context, its place in the phrase, and how it is read there: the polyphones
/// pinyin-pro reads the other way in lyrics.
const FIXES: &[(&str, usize, &str)] = &[
    ("还给", 0, "huán"),
    ("还回", 0, "huán"),
    ("得走", 0, "děi"),
    ("得去", 0, "děi"),
    ("教我", 0, "jiāo"),
    ("教你", 0, "jiāo"),
    ("教他", 0, "jiāo"),
    ("教她", 0, "jiāo"),
    ("种花", 0, "zhòng"),
    ("种树", 0, "zhòng"),
    ("种下", 0, "zhòng"),
    ("数一数", 0, "shǔ"),
    ("数一数", 2, "shǔ"),
    ("数不清", 0, "shǔ"),
    ("数不尽", 0, "shǔ"),
    ("数着", 0, "shǔ"),
    ("数星星", 0, "shǔ"),
    ("努力地", 2, "de"),
    ("用力地", 2, "de"),
    ("拼命地", 2, "de"),
    ("温柔地", 2, "de"),
    ("舍不得", 0, "shě"),
    ("舍得", 0, "shě"),
    ("切菜", 0, "qiē"),
    ("切开", 0, "qiē"),
];

fn fix_polyphones(chars: &[char], syllables: &mut [String]) {
    for &(phrase, k, reading) in FIXES {
        let p: Vec<char> = phrase.chars().collect();
        if chars.len() < p.len() {
            continue;
        }
        for at in 0..=chars.len() - p.len() {
            if chars[at..at + p.len()] == p[..] {
                syllables[at + k] = reading.to_owned();
            }
        }
    }
    // 地 after a doubled character is the adverb ending: 慢慢地 "man man de".
    for at in 2..chars.len() {
        if chars[at] == '地' && chars[at - 1] == chars[at - 2] {
            syllables[at] = "de".to_owned();
        }
    }
}

fn strip_tones(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ā' | 'á' | 'ǎ' | 'à' => 'a',
            'ē' | 'é' | 'ě' | 'è' => 'e',
            'ī' | 'í' | 'ǐ' | 'ì' => 'i',
            'ō' | 'ó' | 'ǒ' | 'ò' => 'o',
            'ū' | 'ú' | 'ǔ' | 'ù' => 'u',
            'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' => 'ü',
            'ń' | 'ň' | 'ǹ' => 'n',
            'ḿ' => 'm',
            c => c,
        })
        .collect()
}

/// A Traditional character's Simplified form (itself when it has none).
fn simplify(c: char) -> char {
    static TABLE: OnceLock<HashMap<char, char>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        include_str!("../data/opencc-ts.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut cs = l.chars();
                Some((cs.next()?, cs.next()?))
            })
            .collect()
    });
    table.get(&c).copied().unwrap_or(c)
}
