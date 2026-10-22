//! Russian, Ukrainian, Bulgarian and Serbian in Roman letters, the way each is read:
//!
//! - Ukrainian: the national system of 2010 (Cabinet of Ministers resolution 55), as passports
//!   use it: "Kyiv", "Yuliia", "zgh" for зг.
//! - Bulgarian: the Streamlined System (law of 2009): ъ "a", щ "sht", final ия "ia".
//! - Serbian: the Serbian Latin alphabet itself (Gaj's), which Serbs write every day: "Đorđe",
//!   "ljubav", "Ćao".
//! - Russian: a simplified BGN/PCGN, the spelling English-language press uses: "Tsoy",
//!   "Yeltsin", "zhyolty" written "zholty" after ж, ч, ш, щ.
//!
//! Small tables of our own; the `translit` crate's systems (GOST 7.79 B, "shh", "cz") read
//! less naturally and `iuliia` has no Bulgarian or Serbian. The script is shared, so the
//! song's language picks the table ([`Lang::from_tag`]), or the letters do ([`detect`]).

use std::ops::Range;

use crate::Pieces;
use crate::layout::by_letters;

/// Which language's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Russian,
    Ukrainian,
    Bulgarian,
    Serbian,
}

impl Lang {
    /// From a BCP 47 tag ("uk", "sr-Cyrl-RS"); `None` for a language without a table here.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        Some(match primary.as_str() {
            "ru" => Lang::Russian,
            "uk" => Lang::Ukrainian,
            "bg" => Lang::Bulgarian,
            "sr" => Lang::Serbian,
            _ => return None,
        })
    }

    /// Its language code ("uk"); the pronunciation is tagged "uk-Latn".
    pub fn code(self) -> &'static str {
        match self {
            Lang::Russian => "ru",
            Lang::Ukrainian => "uk",
            Lang::Bulgarian => "bg",
            Lang::Serbian => "sr",
        }
    }
}

/// A Cyrillic letter.
pub fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&c)
}

fn is_letter(c: char) -> bool {
    // An apostrophe inside a word belongs to it (Ukrainian м'ята).
    is_cyrillic(c) || matches!(c, '\'' | '’' | 'ʼ')
}

/// The language a text is most likely in, from letters only one of them uses. Give it the
/// whole song: one line may have none of them. Russian when nothing tells.
pub fn detect(text: &str) -> Lang {
    let lower: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let has = |set: &[char]| lower.iter().any(|c| set.contains(c));
    if has(&['ђ', 'ћ', 'џ', 'љ', 'њ', 'ј']) {
        return Lang::Serbian;
    }
    if has(&['і', 'ї', 'є', 'ґ']) {
        return Lang::Ukrainian;
    }
    if has(&['ы', 'э', 'ё']) {
        return Lang::Russian;
    }
    // Bulgarian ъ is a vowel, so a consonant follows it (път, сърце); in Russian it comes
    // only before е, ё, ю, я.
    let bulgarian = lower
        .windows(2)
        .any(|w| w[0] == 'ъ' && is_cyrillic(w[1]) && !"аеёиоуыэюяъь".contains(w[1]));
    if bulgarian {
        Lang::Bulgarian
    } else {
        Lang::Russian
    }
}

/// The text in Roman letters, by `lang`'s table.
pub fn romanize(text: &str, lang: Lang) -> String {
    let mut out = Pieces::new(text);
    write(text, 0..text.len(), lang, &mut out);
    out.into_text()
}

/// Romanizes `line[range]` into `out`, a piece per letter group.
pub fn write(line: &str, range: Range<usize>, lang: Lang, out: &mut Pieces) {
    by_letters(line, range, out, is_letter, |w| steps(w, lang), |_| None)
}

fn steps(w: &[char], lang: Lang) -> Vec<(usize, String)> {
    let mut out = Vec::with_capacity(w.len());
    let mut k = 0;
    while k < w.len() {
        let (n, s) = step(w, k, lang);
        out.push((n, s));
        k += n;
    }
    out
}

const VOWELS: &str = "аеёиоуыэюяєіїъь";

fn step(w: &[char], k: usize, lang: Lang) -> (usize, String) {
    let c = w[k];
    let prev = k.checked_sub(1).map(|p| w[p]);
    let next = w.get(k + 1).copied();
    // Only a word's first letter: after an apostrophe is not (Mar'iana, m'iata).
    let initial = prev.is_none();
    let one = |s: &str| (1, s.to_owned());
    match lang {
        Lang::Russian => {
            let after_vowel = prev.is_some_and(|p| VOWELS.contains(p));
            match c {
                'е' if initial || after_vowel => one("ye"),
                'ё' if prev.is_some_and(|p| "жчшщ".contains(p)) => one("o"),
                'ы' if next == Some('й') && k + 2 == w.len() => (2, "y".into()),
                _ => one(common(c).unwrap_or(&c.to_string())),
            }
        }
        Lang::Ukrainian => match c {
            'з' if next == Some('г') => (2, "zgh".into()),
            'г' => one("h"),
            'ґ' => one("g"),
            'и' => one("y"),
            'і' => one("i"),
            'є' => one(if initial { "ye" } else { "ie" }),
            'ї' => one(if initial { "yi" } else { "i" }),
            'й' => one(if initial { "y" } else { "i" }),
            'ю' => one(if initial { "yu" } else { "iu" }),
            'я' => one(if initial { "ya" } else { "ia" }),
            '\'' | '’' | 'ʼ' => one(""),
            _ => one(common(c).unwrap_or(&c.to_string())),
        },
        Lang::Bulgarian => match c {
            'и' if next == Some('я') && k + 2 == w.len() => (2, "ia".into()),
            'х' => one("h"),
            'щ' => one("sht"),
            'ъ' => one("a"),
            'ь' => one("y"),
            _ => one(common(c).unwrap_or(&c.to_string())),
        },
        Lang::Serbian => one(match c {
            'ђ' => "đ",
            'ж' => "ž",
            'ј' => "j",
            'љ' => "lj",
            'њ' => "nj",
            'ћ' => "ć",
            'х' => "h",
            'ц' => "c",
            'ч' => "č",
            'џ' => "dž",
            'ш' => "š",
            _ => return one(common(c).unwrap_or(&c.to_string())),
        }),
    }
}

/// What most of the tables share.
fn common(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' => "e",
        'ё' => "yo",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' | 'ь' => "",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        'і' => "i",
        'ї' => "yi",
        'є' => "ye",
        'ґ' => "g",
        'ў' => "w",
        'ј' => "j",
        'љ' => "lj",
        'њ' => "nj",
        'ђ' => "dj",
        'ћ' => "c",
        'џ' => "dzh",
        '\'' | '’' | 'ʼ' => "'",
        _ => return None,
    })
}
