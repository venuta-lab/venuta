//! Greek in Roman letters by ELOT 743 (ISO 843, transcription), the system Greek passports and
//! road signs use: "Efcharisto", "Bampas", "Angelos", "Psychi". Accents are dropped; a
//! diaeresis keeps two vowels apart ("Chaidepse"). A small table of our own.

use std::ops::Range;

use crate::Pieces;
use crate::layout::by_letters;

/// The language of the pronunciation (it is written "el-Latn").
pub const LANG: &str = "el";

/// A Greek letter (or a combining accent written after one).
pub fn is_greek(c: char) -> bool {
    (('\u{0370}'..='\u{03FF}').contains(&c) && c.is_alphabetic())
        || ('\u{0300}'..='\u{036F}').contains(&c)
}

/// The text in Roman letters.
pub fn romanize(text: &str) -> String {
    let mut out = Pieces::new(text);
    write(text, 0..text.len(), &mut out);
    out.into_text()
}

/// Romanizes `line[range]` into `out`, a piece per letter group.
pub fn write(line: &str, range: Range<usize>, out: &mut Pieces) {
    by_letters(line, range, out, is_greek, steps, |c| match c {
        // The Greek question mark, and ";" written for it.
        ';' | '\u{037E}' => Some("?"),
        '\u{0387}' => Some(";"),
        _ => None,
    })
}

/// A letter without its accent, whether it has a diaeresis, and how many characters it took.
struct Letter {
    base: char,
    diaeresis: bool,
    len: usize,
}

fn letters(w: &[char]) -> Vec<Letter> {
    let mut out: Vec<Letter> = Vec::with_capacity(w.len());
    for &c in w {
        if ('\u{0300}'..='\u{036F}').contains(&c) {
            match out.last_mut() {
                Some(l) => {
                    l.len += 1;
                    l.diaeresis |= c == '\u{0308}';
                }
                None => out.push(Letter {
                    base: c,
                    diaeresis: false,
                    len: 1,
                }),
            }
            continue;
        }
        let (base, diaeresis) = match c {
            'ά' => ('α', false),
            'έ' => ('ε', false),
            'ή' => ('η', false),
            'ί' => ('ι', false),
            'ό' => ('ο', false),
            'ύ' => ('υ', false),
            'ώ' => ('ω', false),
            'ϊ' | 'ΐ' => ('ι', true),
            'ϋ' | 'ΰ' => ('υ', true),
            'ς' => ('σ', false),
            c => (c, false),
        };
        out.push(Letter {
            base,
            diaeresis,
            len: 1,
        });
    }
    out
}

const VOICELESS: &str = "θκξπστφχψ";

fn steps(w: &[char]) -> Vec<(usize, String)> {
    let ls = letters(w);
    let mut out = Vec::with_capacity(ls.len());
    let mut k = 0;
    while k < ls.len() {
        let b = ls[k].base;
        let next = ls.get(k + 1).filter(|l| !l.diaeresis).map(|l| l.base);
        let initial = k == 0;
        let pair: Option<String> = match (b, next) {
            ('α' | 'ε' | 'η', Some('υ')) => {
                let after = ls.get(k + 2).map(|l| l.base);
                let f = after.is_none_or(|a| VOICELESS.contains(a));
                let v = match b {
                    'α' => "a",
                    'ε' => "e",
                    _ => "i",
                };
                Some(format!("{v}{}", if f { "f" } else { "v" }))
            }
            ('α', Some('ι')) => Some("ai".into()),
            ('ε', Some('ι')) => Some("ei".into()),
            ('ο', Some('ι')) => Some("oi".into()),
            ('ο', Some('υ')) => Some("ou".into()),
            ('υ', Some('ι')) => Some("yi".into()),
            ('μ', Some('π')) => Some(if initial { "b" } else { "mp" }.into()),
            ('ν', Some('τ')) => Some(if initial { "d" } else { "nt" }.into()),
            ('γ', Some('κ')) => Some(if initial { "g" } else { "nk" }.into()),
            ('γ', Some('γ')) => Some("ng".into()),
            ('γ', Some('ξ')) => Some("nx".into()),
            ('γ', Some('χ')) => Some("nch".into()),
            _ => None,
        };
        if let Some(s) = pair {
            out.push((ls[k].len + ls[k + 1].len, s));
            k += 2;
            continue;
        }
        let s = match b {
            'α' => "a",
            'β' => "v",
            'γ' => "g",
            'δ' => "d",
            'ε' => "e",
            'ζ' => "z",
            'η' => "i",
            'θ' => "th",
            'ι' => "i",
            'κ' => "k",
            'λ' => "l",
            'μ' => "m",
            'ν' => "n",
            'ξ' => "x",
            'ο' => "o",
            'π' => "p",
            'ρ' => "r",
            'σ' => "s",
            'τ' => "t",
            'υ' => "y",
            'φ' => "f",
            'χ' => "ch",
            'ψ' => "ps",
            'ω' => "o",
            c if ('\u{0300}'..='\u{036F}').contains(&c) => "",
            c => {
                out.push((ls[k].len, c.to_string()));
                k += 1;
                continue;
            }
        };
        out.push((ls[k].len, s.to_owned()));
        k += 1;
    }
    out
}
