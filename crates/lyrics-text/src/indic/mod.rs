//! The Indian scripts in English letters, the casual way people write them ("Jo waade kiye
//! saare bas baaton mein reh gaye"), not an academic spelling.
//!
//! Each word is looked up in the language's word lists first ([`lexicon`]); otherwise it is
//! read letter by letter:
//! 1. split into letters, each a consonant (or none, for a vowel letter) with its vowel, its
//!    nasal mark and its visarga;
//! 2. the silent "a" is found ([`Lang::schwa`]): in Hindi and the languages read like it the "a"
//!    every consonant carries is not said at the end of a word ("dil", not "dila") or between
//!    two sounded letters ("sam-jh-na"), except where three consonants would meet; Bengali
//!    keeps it as "o" more often; Sanskrit, Odia and the Dravidian scripts say it;
//! 3. each letter is spelled with the language's conventions: long vowels as aa, ee, oo (ee
//!    and oo inside a word, i and u at its end), nasal endings as mein, hain, baaton, "y"
//!    between vowels (kiye, gaye), "eh" before a silent h (reh, kehna), w or v for व, and the
//!    Dravidian consonants as people write them (Tamil "kaadhal", "nandri", Malayalam "ente").
//!
//! Every letter becomes one piece of output with its source bytes, so a word timed syllable by
//! syllable keeps its timing ([`crate::timing`]).

use std::cell::RefCell;
use std::ops::Range;

use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

use crate::{Pieces, Script};

pub mod letters;
pub mod lexicon;
mod words;

use letters::{C, Kind, V};

/// A language written in an Indian script: which rules read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Hi,
    Mr,
    Ne,
    Sa,
    Bn,
    Gu,
    Pa,
    Or,
    Ta,
    Te,
    Kn,
    Ml,
}

impl Lang {
    pub const ALL: [Lang; 12] = [
        Lang::Hi,
        Lang::Mr,
        Lang::Ne,
        Lang::Sa,
        Lang::Bn,
        Lang::Gu,
        Lang::Pa,
        Lang::Or,
        Lang::Ta,
        Lang::Te,
        Lang::Kn,
        Lang::Ml,
    ];

    /// Its BCP 47 code ("hi").
    pub fn code(self) -> &'static str {
        match self {
            Lang::Hi => "hi",
            Lang::Mr => "mr",
            Lang::Ne => "ne",
            Lang::Sa => "sa",
            Lang::Bn => "bn",
            Lang::Gu => "gu",
            Lang::Pa => "pa",
            Lang::Or => "or",
            Lang::Ta => "ta",
            Lang::Te => "te",
            Lang::Kn => "kn",
            Lang::Ml => "ml",
        }
    }

    /// The script it is written in.
    pub fn script(self) -> Script {
        match self {
            Lang::Hi | Lang::Mr | Lang::Ne | Lang::Sa => Script::Deva,
            Lang::Bn => Script::Beng,
            Lang::Gu => Script::Gujr,
            Lang::Pa => Script::Guru,
            Lang::Or => Script::Orya,
            Lang::Ta => Script::Taml,
            Lang::Te => Script::Telu,
            Lang::Kn => Script::Knda,
            Lang::Ml => Script::Mlym,
        }
    }

    /// A language code (BCP 47 or ISO 639-1/2/3: "hi", "mr-IN", "pan", "nep").
    pub fn from_code(code: &str) -> Option<Lang> {
        let primary = code.split(['-', '_']).next()?.trim();
        let mut buf = [0u8; 3];
        if primary.len() > 3 || !primary.is_ascii() {
            return None;
        }
        for (b, c) in buf.iter_mut().zip(primary.bytes()) {
            *b = c.to_ascii_lowercase();
        }
        Some(match &buf[..primary.len()] {
            b"hi" | b"hin" | b"bho" | b"mai" | b"awa" | b"mag" | b"raj" | b"hne" | b"bra"
            | b"bgc" => Lang::Hi,
            b"mr" | b"mar" | b"kok" | b"gom" => Lang::Mr,
            b"ne" | b"nep" | b"npi" => Lang::Ne,
            b"sa" | b"san" => Lang::Sa,
            b"bn" | b"ben" | b"as" | b"asm" => Lang::Bn,
            b"gu" | b"guj" => Lang::Gu,
            b"pa" | b"pan" => Lang::Pa,
            b"or" | b"ori" | b"ory" => Lang::Or,
            b"ta" | b"tam" => Lang::Ta,
            b"te" | b"tel" => Lang::Te,
            b"kn" | b"kan" => Lang::Kn,
            b"ml" | b"mal" => Lang::Ml,
            _ => return None,
        })
    }

    /// The language a run of `script` is read as: the song's language when it is written in
    /// that script, otherwise the script's most common language.
    pub fn of(script: Script, hint: Option<&str>) -> Option<Lang> {
        let default = match script {
            Script::Deva => Lang::Hi,
            Script::Beng => Lang::Bn,
            Script::Guru => Lang::Pa,
            Script::Gujr => Lang::Gu,
            Script::Orya => Lang::Or,
            Script::Taml => Lang::Ta,
            Script::Telu => Lang::Te,
            Script::Knda => Lang::Kn,
            Script::Mlym => Lang::Ml,
            _ => return None,
        };
        Some(
            hint.and_then(Lang::from_code)
                .filter(|l| l.script() == script)
                .unwrap_or(default),
        )
    }

    /// How the unwritten "a" is treated.
    fn schwa(self) -> Schwa {
        match self {
            Lang::Hi | Lang::Mr | Lang::Gu | Lang::Pa => Schwa::Drop,
            Lang::Ne | Lang::Bn => Schwa::DropKeepClusters,
            Lang::Sa | Lang::Or | Lang::Ta | Lang::Te | Lang::Kn | Lang::Ml => Schwa::Keep,
        }
    }

    /// Whether a word no list knows is spelled the familiar way (see [`Options::familiar`]).
    /// Hindi, Marathi, Punjabi and Bengali spell it plainly: people write the long vowels
    /// doubled in the common words they know well ("baat", "saare", "deewana", kept that way
    /// in the word lists) and single in names and rarer words ("Pakistan", "Kashmir",
    /// "rashtriya"); in Dakshina's Hindi lexicon three spellings in four write a long a single,
    /// wherever it stands in the word. Gujarati and the Dravidian languages are written the
    /// familiar way. Measured on Dakshina's dev sentences, words no lexicon has, with
    /// `cargo run -p lyrics-text --example dakshina -- choose`: plain Hindi 50% against 37%,
    /// Marathi 48% against 18%, Punjabi 47% against 43%; familiar Gujarati 25% against 17%,
    /// Tamil 43% against 32%, Telugu 56% against 35%, Kannada 63% against 40%, Malayalam 36%
    /// against 32%.
    pub fn familiar_unknown(self) -> bool {
        !matches!(self, Lang::Hi | Lang::Mr | Lang::Pa | Lang::Bn)
    }

    /// Whether the "a" between two sounded letters is silent ("sam-jh-na"). Marathi and
    /// Gujarati annotators keep it more often than not (measured as above: Marathi 48% against
    /// 46%, Gujarati 25% against 22%), so there only the last "a" of a word goes.
    pub fn drops_medial_a(self) -> bool {
        self.schwa() != Schwa::Keep && !matches!(self, Lang::Mr | Lang::Gu)
    }

    fn dravidian(self) -> bool {
        matches!(self, Lang::Ta | Lang::Te | Lang::Kn | Lang::Ml)
    }

    fn spelling(self) -> &'static Spelling {
        match self {
            Lang::Hi | Lang::Gu | Lang::Pa => &NORTH,
            Lang::Mr => &MARATHI,
            Lang::Ne | Lang::Sa => &NEPALI,
            Lang::Bn => &BENGALI,
            Lang::Or => &ODIA,
            Lang::Ta => &TAMIL,
            Lang::Te | Lang::Kn | Lang::Ml => &SOUTH,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Schwa {
    /// Not said at the end of a word or between sounded letters (Hindi).
    Drop,
    /// The same, but said after a cluster at the end of a word (Bengali "jonno", Nepali).
    DropKeepClusters,
    /// Always said.
    Keep,
}

/// How a language spells its vowels: (inside a word, at its end) for the long ones.
struct Spelling {
    a: &'static str,
    aa: (&'static str, &'static str),
    ii: (&'static str, &'static str),
    uu: (&'static str, &'static str),
    ai: &'static str,
    au: &'static str,
    r: &'static str,
}

static NORTH: Spelling = Spelling {
    a: "a",
    aa: ("aa", "a"),
    ii: ("ee", "i"),
    uu: ("oo", "u"),
    ai: "ai",
    au: "au",
    r: "ri",
};
// Marathi says ऋ "ru" ("krupa", "pruthvi"), as four annotators in five write it.
static MARATHI: Spelling = Spelling { r: "ru", ..NORTH };
static NEPALI: Spelling = Spelling {
    a: "a",
    aa: ("a", "a"),
    ii: ("i", "i"),
    uu: ("u", "u"),
    ai: "ai",
    au: "au",
    r: "ri",
};
static BENGALI: Spelling = Spelling {
    a: "o",
    aa: ("a", "a"),
    ii: ("i", "i"),
    uu: ("u", "u"),
    ai: "oi",
    au: "ou",
    r: "ri",
};
static ODIA: Spelling = Spelling {
    a: "a",
    aa: ("a", "a"),
    ii: ("i", "i"),
    uu: ("u", "u"),
    ai: "ai",
    au: "au",
    r: "ru",
};
static TAMIL: Spelling = Spelling {
    a: "a",
    aa: ("aa", "aa"),
    ii: ("ee", "ee"),
    uu: ("oo", "oo"),
    ai: "ai",
    au: "au",
    r: "ru",
};
static SOUTH: Spelling = Spelling {
    a: "a",
    aa: ("aa", "aa"),
    ii: ("ee", "ee"),
    uu: ("oo", "oo"),
    ai: "ai",
    au: "au",
    r: "ru",
};

/// The scripts this module reads.
pub fn covers(script: Script) -> bool {
    letters::block_start(script).is_some()
}

/// Romanizes `line[range]`, a run of `script`, into `out`. Returns the language it was read as.
pub fn romanize(
    script: Script,
    line: &str,
    range: Range<usize>,
    hint: Option<&str>,
    out: &mut Pieces,
) -> Option<&'static str> {
    let lang = Lang::of(script, hint)?;
    let run = &line[range.clone()];
    let base = range.start;
    let mut word_start: Option<usize> = None;
    for (i, ch) in run.char_indices() {
        let k = letters::kind(script, ch);
        if k.in_word() {
            word_start.get_or_insert(i);
            continue;
        }
        if let Some(s) = word_start.take() {
            word(lang, script, line, base + s..base + i, out, ENGINE);
        }
        let src = base + i..base + i + ch.len_utf8();
        match k {
            Kind::Digit(d) => out.push(src, DIGITS[d as usize]),
            Kind::Danda => out.push(src, "."),
            Kind::Om => out.push(src, "om"),
            Kind::Avagraha => out.push(src, ""),
            Kind::Text(t) => out.push(src, t),
            _ => out.keep(src),
        }
    }
    if let Some(s) = word_start {
        word(lang, script, line, base + s..range.end, out, ENGINE);
    }
    Some(lang.code())
}

const ENGINE: Options = Options {
    lists: true,
    familiar: None,
    medial: None,
};

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// How a word is read, for tools and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Read the Dakshina table (the house list is always read).
    pub lists: bool,
    /// Spell a word no list knows the familiar way: long vowels inside it as aa, ee, oo, a
    /// Malayalam dead consonant at its end with a "u" ("ninnu"), a Gurmukhi addak as a doubled
    /// letter ("ikk"). Plainly otherwise ("a", "i", "u", "ninn", "ik"). `None`: the language's
    /// own way ([`Lang::familiar_unknown`]).
    pub familiar: Option<bool>,
    /// Drop the "a" between two sounded letters. `None`: [`Lang::drops_medial_a`].
    pub medial: Option<bool>,
}

/// One word in English letters, for tools and tests. `lists` says whether the word lists are
/// read (the house list always is; without `lists`, the Dakshina table is not).
pub fn word_to_latin(word: &str, lang: Lang, lists: bool) -> String {
    word_to_latin_with(
        word,
        lang,
        Options {
            lists,
            familiar: None,
            medial: None,
        },
    )
}

/// [`word_to_latin`] with all its options.
pub fn word_to_latin_with(word: &str, lang: Lang, opts: Options) -> String {
    let mut out = Pieces::new(word);
    let script = lang.script();
    let mut start = None;
    for (i, ch) in word.char_indices() {
        if letters::kind(script, ch).in_word() {
            start.get_or_insert(i);
        } else {
            if let Some(s) = start.take() {
                self::word(lang, script, word, s..i, &mut out, opts);
            }
            out.keep(i..i + ch.len_utf8());
        }
    }
    if let Some(s) = start {
        self::word(lang, script, word, s..word.len(), &mut out, opts);
    }
    out.into_text()
}

/// A consonant (or none, for a vowel letter) with its vowel and marks.
#[derive(Debug, Clone, Copy)]
struct Unit {
    start: u32,
    end: u32,
    c: Option<C>,
    v: Vs,
    nasal: Nasal,
    visarga: bool,
    /// Gurmukhi addak before it: the consonant is doubled.
    double: bool,
    /// A chillu (a consonant written dead as one letter).
    chillu: bool,
    /// Gurmukhi ੲ ੳ: its vowel is the sign that follows.
    bearer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vs {
    /// The unwritten "a", said.
    Inherent,
    /// The unwritten "a", silent here.
    Dropped,
    /// No vowel (virama).
    Dead,
    Is(V),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nasal {
    None,
    Anusvara,
    Candra,
}

impl Unit {
    fn sounds(&self) -> bool {
        !matches!(self.v, Vs::Dropped | Vs::Dead)
    }

    fn cons(&self) -> bool {
        self.c.is_some()
    }

    fn is(&self, c: C) -> bool {
        self.c == Some(c)
    }
}

thread_local! {
    static UNITS: RefCell<Vec<Unit>> = const { RefCell::new(Vec::new()) };
    static KEY: RefCell<String> = const { RefCell::new(String::new()) };
    static FOLD: RefCell<String> = const { RefCell::new(String::new()) };
}

/// One word (`line[range]`, letters only) into `out`.
fn word(
    lang: Lang,
    script: Script,
    line: &str,
    range: Range<usize>,
    out: &mut Pieces,
    opts: Options,
) {
    let text = &line[range.clone()];
    let found = KEY.with_borrow_mut(|key| {
        let k = canonical(lang, text, key);
        if opts.lists {
            lexicon::lookup(lang, k)
        } else {
            words::house(lang, k)
        }
    });
    let familiar = opts.familiar.unwrap_or(lang.familiar_unknown());
    let medial = opts.medial.unwrap_or(lang.drops_medial_a());
    if let Some(spelled) = found {
        // Spelled the rules' way letter by letter first, so the list's spelling can be split
        // where the letters are ("baa" + "ton" for a word timed syllable by syllable).
        UNITS.with_borrow_mut(|units| {
            units.clear();
            parse(script, text, range.start, units);
            silent_a(lang, units, medial, text, range.start);
            let (spans, bytes) = (out.spans().len(), out.text().len());
            spell(lang, units, out, familiar);
            out.respell(spans, bytes, spelled);
        });
        return;
    }
    UNITS.with_borrow_mut(|units| {
        units.clear();
        parse(script, text, range.start, units);
        silent_a(lang, units, medial, text, range.start);
        spell(lang, units, out, familiar);
    });
}

/// The word as the word lists keep it (see [`lookup_key`]).
fn canonical<'a>(lang: Lang, text: &'a str, buf: &'a mut String) -> &'a str {
    let joiners = text.contains(['\u{200C}', '\u{200D}']);
    // Only a candrabindu, or a nasal consonant with a virama, can need folding.
    let folds = folds_nasals(lang) && {
        let mut prev_nasal = false;
        text.chars().any(|c| {
            let o = block_offset(lang, c);
            let hit = o == Some(0x01) || (prev_nasal && o == Some(0x4D));
            prev_nasal = matches!(o, Some(0x19 | 0x1E | 0x23 | 0x28 | 0x2E));
            hit
        })
    };
    if !joiners && !folds && is_nfc_quick(text.chars()) == IsNormalized::Yes {
        return text;
    }
    buf.clear();
    buf.extend(text.nfc().filter(|c| !matches!(c, '\u{200C}' | '\u{200D}')));
    if folds {
        fold_nasals(lang, buf);
    }
    buf
}

/// The key a word is kept under in the word lists: NFC, no joiners, and in the Devanagari and
/// Gujarati languages one spelling of a nasal before a consonant: candrabindu as anusvara, and
/// a nasal consonant joined to a consonant of its own class as anusvara ("सम्प्रदाय" and
/// "संप्रदाय", "हालाँकि" and "हालांकि" are the same word).
pub fn lookup_key(word: &str, lang: Lang) -> String {
    let mut buf = String::new();
    canonical(lang, word, &mut buf).to_owned()
}

fn folds_nasals(lang: Lang) -> bool {
    matches!(lang, Lang::Hi | Lang::Mr | Lang::Ne | Lang::Gu)
}

/// The offset of `c` in the language's block.
fn block_offset(lang: Lang, c: char) -> Option<u32> {
    let start = letters::block_start(lang.script())?;
    (c as u32).checked_sub(start).filter(|o| *o < 0x80)
}

/// Candrabindu as anusvara, and a nasal joined to the first consonant of its class (ङ्क ञ्च
/// ण्ट न्त म्प) as anusvara, in place (no allocation once the buffer has grown).
fn fold_nasals(lang: Lang, buf: &mut String) {
    let Some(start) = letters::block_start(lang.script()) else {
        return;
    };
    let at = |o: u32| char::from_u32(start + o).unwrap_or('\0');
    let off = |c: char| (c as u32).wrapping_sub(start);
    let mut out = FOLD.with_borrow_mut(std::mem::take);
    out.clear();
    let mut chars = buf.chars().peekable();
    let mut first = true;
    while let Some(c) = chars.next() {
        let o = off(c);
        if o == 0x01 {
            out.push(at(0x02));
            first = false;
            continue;
        }
        let class = match o {
            0x19 => Some(0x15..=0x18),
            0x1E => Some(0x1A..=0x1D),
            0x23 => Some(0x1F..=0x22),
            0x28 => Some(0x24..=0x27),
            0x2E => Some(0x2A..=0x2D),
            _ => None,
        };
        if let Some(class) = class
            && !first
            && chars.peek() == Some(&at(0x4D))
        {
            let mut ahead = chars.clone();
            ahead.next();
            if ahead.next().is_some_and(|n| class.contains(&off(n))) {
                chars.next();
                out.push(at(0x02));
                continue;
            }
        }
        out.push(c);
        first = false;
    }
    // Keep both buffers for the next word.
    let old = std::mem::replace(buf, out);
    FOLD.with_borrow_mut(|f| *f = old);
}

fn parse(script: Script, text: &str, base: usize, units: &mut Vec<Unit>) {
    let mut double = false;
    for (i, ch) in text.char_indices() {
        let at = (base + i) as u32;
        let end = at + ch.len_utf8() as u32;
        let unit = |c, v| Unit {
            start: at,
            end,
            c,
            v,
            nasal: Nasal::None,
            visarga: false,
            double: false,
            chillu: false,
            bearer: false,
        };
        let kind = letters::kind(script, ch);
        match kind {
            Kind::Cons(c) => {
                let mut u = unit(Some(c), Vs::Inherent);
                u.double = std::mem::take(&mut double);
                units.push(u);
                continue;
            }
            Kind::Vowel(v) => {
                units.push(unit(None, Vs::Is(v)));
                continue;
            }
            Kind::Bearer(v) => {
                let mut u = unit(None, Vs::Is(v));
                u.bearer = true;
                units.push(u);
                continue;
            }
            Kind::Dead(c) => {
                let mut u = unit(Some(c), Vs::Dead);
                u.chillu = true;
                units.push(u);
                continue;
            }
            _ => {}
        }
        let Some(last) = units.last_mut() else {
            continue;
        };
        last.end = end;
        match kind {
            Kind::Sign(v) => {
                last.v = match last.v {
                    // A vowel letter with a sign on it (ਅ + ਾ, ੲ + ਿ): the vowel the sign says.
                    Vs::Is(_) if last.c.is_none() && last.bearer => {
                        last.bearer = false;
                        Vs::Is(v)
                    }
                    Vs::Is(V::A) if last.c.is_none() => Vs::Is(v),
                    // A vowel sign in two parts (ে + া).
                    Vs::Is(V::E) if v == V::Aa || v == V::Uu => Vs::Is(V::O),
                    Vs::Is(V::Ee) if v == V::Aa => Vs::Is(V::Oo),
                    _ => Vs::Is(v),
                };
            }
            Kind::Mark(m) => {
                last.v = match (last.v, m) {
                    (Vs::Is(V::E | V::Ee) | Vs::Inherent, V::Au) => Vs::Is(V::Au),
                    (Vs::Is(V::E), V::Ai) => Vs::Is(V::Ai),
                    (Vs::Is(V::I), V::Ii) => Vs::Is(V::Ii),
                    (Vs::Is(V::E), V::Ii) => Vs::Is(V::Ee),
                    (Vs::Is(V::O), V::Ii) => Vs::Is(V::Oo),
                    (v, _) => v,
                };
            }
            Kind::Virama => {
                if last.c.is_some() && last.v == Vs::Inherent {
                    last.v = Vs::Dead;
                }
            }
            Kind::Nukta => {
                if let Some(c) = last.c {
                    last.c = Some(c.nukta());
                }
            }
            Kind::Anusvara => last.nasal = Nasal::Anusvara,
            Kind::Candrabindu => last.nasal = Nasal::Candra,
            Kind::Visarga => last.visarga = true,
            Kind::Addak => double = true,
            _ => {}
        }
    }
}

/// Marks the unwritten "a" that is not said. `text` is the word, starting at `base` in the line.
fn silent_a(lang: Lang, units: &mut [Unit], medial: bool, text: &str, base: usize) {
    let schwa = lang.schwa();
    if schwa == Schwa::Keep {
        return;
    }
    let n = units.len();
    if n < 2 {
        return;
    }
    drop_final_a(schwa, units);
    // Marathi writes its case endings onto the word, and the word's own last "a" goes before
    // them as at a word's end: "ghar-chya", "sachin-la", "kolhapur-pasun", "albam-madhil".
    if lang == Lang::Mr
        && let Some(ending) = MARATHI_ENDINGS
            .iter()
            .filter(|e| text.len() > e.len() && text.ends_with(**e))
            .max_by_key(|e| e.len())
    {
        let at = base + text.len() - ending.len();
        if let Some(k) = units.iter().position(|u| u.start as usize == at)
            && k >= 2
        {
            drop_final_a(schwa, &mut units[..k]);
        }
    }
    // Inside the word, right to left: V C a C V -> V C C V ("sam-jh-na", "dhad-kan"), but not
    // where it would make three consonants meet, nor before -iya ("kesariya", "rasiya"), nor
    // after a nasal vowel ("zindagi").
    for i in (1..n - 1).rev() {
        let (prev, u, next) = (units[i - 1], units[i], units[i + 1]);
        if !medial || !u.cons() || u.v != Vs::Inherent || u.nasal != Nasal::None || u.visarga {
            continue;
        }
        if !prev.sounds() || prev.nasal != Nasal::None || prev.visarga {
            continue;
        }
        if !next.cons() || !next.sounds() {
            continue;
        }
        if matches!(next.v, Vs::Is(V::I | V::Ii))
            && units
                .get(i + 2)
                .is_some_and(|w| w.is(C::Y) && w.v == Vs::Is(V::Aa))
        {
            continue;
        }
        units[i].v = Vs::Dropped;
    }
}

/// The case endings Marathi writes onto a word, after which the word's last "a" is not said.
/// Only the ones annotators write that way more often than not; "-ही", "-ने", "-वर" are left
/// out (annotators keep the "a" before them as often as they drop it).
static MARATHI_ENDINGS: &[&str] = &[
    "च्या",
    "चा",
    "ची",
    "चे",
    "चं",
    "ला",
    "मध्ये",
    "मधील",
    "मधून",
    "मधे",
    "वरील",
    "वरून",
    "वरुन",
    "कडे",
    "कडून",
    "पासून",
    "पर्यंत",
    "साठी",
];

/// At the end of a word: "dil", not "dila". Said after some clusters ("mitra", "satya",
/// "krishna"; in Bengali and Nepali after any), after -ī in -īya ("bhartiya"), and when it
/// carries a nasal ("evam").
fn drop_final_a(schwa: Schwa, units: &mut [Unit]) {
    let n = units.len();
    if n < 2 {
        return;
    }
    let last = units[n - 1];
    if last.cons() && last.v == Vs::Inherent && last.nasal == Nasal::None && !last.visarga {
        let cluster = units[n - 2].cons() && units[n - 2].v == Vs::Dead;
        let iya =
            (last.is(C::Y) || last.is(C::Yy)) && matches!(units[n - 2].v, Vs::Is(V::I | V::Ii));
        let keep = iya
            || cluster
                && (schwa == Schwa::DropKeepClusters
                    || matches!(last.c, Some(C::Y | C::R | C::Ya))
                    || (units[n - 2].is(C::Ss) && last.is(C::Nn)));
        if !keep {
            units[n - 1].v = Vs::Dropped;
        }
    }
}

/// Writes the word's letters into `out`, one piece per letter.
fn spell(lang: Lang, units: &[Unit], out: &mut Pieces, familiar: bool) {
    let sp = lang.spelling();
    let n = units.len();
    for i in 0..n {
        let u = units[i];
        let prev = i.checked_sub(1).map(|p| units[p]);
        let next = units.get(i + 1).copied();
        out.begin(u.start as usize);
        if let Some(c) = u.c {
            if u.double && familiar {
                out.write(first_letter(consonant(lang, units, i, c)));
            }
            out.write(consonant(lang, units, i, c));
        } else if let Some(p) = prev
            && p.sounds()
            && !(p.cons() && p.v == Vs::Inherent && lang.dravidian())
        {
            // A vowel letter after a vowel takes a "y": ki-e "kiye", ga-e "gaye", gi-aa "giya".
            if let Vs::Is(v) = u.v {
                let glide = match v {
                    V::E | V::Ee => true,
                    V::Aa => matches!(p.v, Vs::Is(V::I | V::Ii)),
                    _ => false,
                };
                if glide && !lang.dravidian() {
                    out.write("y");
                }
            }
        }
        let end = i + 1 == n;
        match u.v {
            Vs::Inherent => {
                // "a" before a silent h is said "e": reh, yeh, kehna, pehle.
                // Only in a word's first syllable: "tarah", "jagah" keep their "a".
                let eh = matches!(lang, Lang::Hi | Lang::Pa)
                    && i == 0
                    && u.cons()
                    && next.is_some_and(|w| w.is(C::H) && !w.sounds() && w.nasal == Nasal::None);
                out.write(if eh { "e" } else { sp.a });
            }
            Vs::Dropped => {}
            Vs::Dead => {
                // Malayalam says a dead consonant at the end of its own words with a short "u"
                // (samvruthokaram): "ninnu", "aanu". An English word ends on its consonant.
                if lang == Lang::Ml
                    && end
                    && !u.chillu
                    && familiar
                    && !malayalam_loan_ending(units, i)
                {
                    out.write("u");
                }
            }
            Vs::Is(v) => {
                // Said short at the end of a word, and before a nasal ending -īn ("nahin",
                // "kahin") or the -īya ending ("bhartiya").
                let short_end = end && (u.nasal == Nasal::None || v == V::Ii);
                let iya = v == V::Ii
                    && i + 2 == n
                    && next.is_some_and(|w| (w.is(C::Y) || w.is(C::Yy)) && w.v == Vs::Inherent);
                out.write(vowel(sp, v, short_end || iya, lang, familiar));
            }
        }
        if u.nasal != Nasal::None {
            out.write(nasal(lang, u, next, end));
        }
        if u.visarga && (end || lang == Lang::Sa) {
            out.write("h");
        }
        out.end(u.end as usize);
    }
}

fn first_letter(s: &str) -> &str {
    s.get(..1).unwrap_or("")
}

fn vowel(sp: &Spelling, v: V, end: bool, lang: Lang, familiar: bool) -> &'static str {
    let pick = |(mid, fin): (&'static str, &'static str)| if end { fin } else { mid };
    match v {
        V::A => {
            if lang == Lang::Bn {
                "o"
            } else {
                "a"
            }
        }
        V::Aa => {
            if familiar {
                pick(sp.aa)
            } else {
                "a"
            }
        }
        V::I => "i",
        V::Ii => {
            if familiar {
                pick(sp.ii)
            } else {
                "i"
            }
        }
        V::U => "u",
        V::Uu => {
            if familiar {
                pick(sp.uu)
            } else {
                "u"
            }
        }
        V::R | V::Rr => sp.r,
        V::L => "li",
        V::Ec => "e",
        V::E | V::Ee => "e",
        V::Ai => sp.ai,
        V::Oc | V::O | V::Oo => "o",
        V::Au => sp.au,
    }
}

/// The nasal mark on a vowel: "mein", "hain", "baaton", "sambhal", "sang".
fn nasal(lang: Lang, u: Unit, next: Option<Unit>, end: bool) -> &'static str {
    let next_c = next.and_then(|w| w.c);
    if lang == Lang::Bn {
        return if u.nasal == Nasal::Anusvara {
            "ng"
        } else {
            "n"
        };
    }
    if lang.dravidian() || lang == Lang::Sa {
        return match next_c {
            Some(c) if c.is_labial() => "m",
            Some(C::Y | C::R | C::L | C::V | C::Sh | C::Ss | C::S | C::H) | None => "m",
            Some(_) => "n",
        };
    }
    if end || next_c.is_none() {
        return match u.v {
            Vs::Is(V::E | V::Ee) => "in",
            Vs::Inherent if u.cons() => "m",
            _ => "n",
        };
    }
    match next_c {
        // Marathi's plural -ां, -ीं keep their n before -ना, -नी, -मध्ये ("mulanna", "lokanni",
        // "gharanmadhye"), and -ां before a labial too ("gavanpasun").
        Some(C::N | C::M | C::Nn) if lang == Lang::Mr && u.v != Vs::Inherent => "n",
        Some(c) if lang == Lang::Mr && c.is_labial() && u.v == Vs::Is(V::Aa) => "n",
        // The nasal is heard in a following n or m: "maine".
        Some(C::N | C::M | C::Nn) if u.v != Vs::Inherent => "",
        // Only the anusvara takes the place of the next letter: a candrabindu stays "n".
        Some(c) if c.is_labial() && u.nasal == Nasal::Anusvara => "m",
        _ => "n",
    }
}

/// How a consonant is spelled where it stands in the word.
fn consonant(lang: Lang, units: &[Unit], i: usize, c: C) -> &'static str {
    let prev = i.checked_sub(1).map(|p| units[p]);
    let next = units.get(i + 1);
    let after_dead = prev.is_some_and(|p| p.cons() && p.v == Vs::Dead);
    let after = |c: C| prev.is_some_and(|p| p.is(c) && p.v == Vs::Dead);
    let before = |c: C| next.is_some_and(|w| w.is(c));
    let dead = units[i].v == Vs::Dead;
    let initial = i == 0;
    if lang == Lang::Ta {
        return tamil(units, i, c);
    }
    match c {
        C::K => {
            if lang == Lang::Bn && dead && before(C::Ss) && initial {
                ""
            } else {
                "k"
            }
        }
        C::Kh => "kh",
        C::G => "g",
        C::Gh => "gh",
        C::Ng => {
            if after(C::Ng) {
                "g"
            } else if before(C::K)
                || before(C::G)
                || before(C::Kh)
                || before(C::Gh)
                || dead && lang != Lang::Bn
            {
                "n"
            } else {
                "ng"
            }
        }
        // A doubled ch is written once in Malayalam ("chalachithram").
        C::C => {
            if lang == Lang::Ml && dead && before(C::C) {
                ""
            } else {
                "ch"
            }
        }
        // Bengali writes ছ "ch" too ("hoyeche", "chara").
        C::Ch => {
            if lang == Lang::Bn {
                "ch"
            } else {
                "chh"
            }
        }
        C::J => {
            if dead && before(C::Ny) {
                match lang {
                    Lang::Mr => "d",
                    Lang::Sa => "j",
                    _ => "g",
                }
            } else {
                "j"
            }
        }
        // Marathi झ is said, and mostly written, "z" ("zad", "zop").
        C::Jh => {
            if lang == Lang::Mr {
                "z"
            } else {
                "jh"
            }
        }
        C::Ny => {
            if after(C::J) {
                match lang {
                    Lang::Mr => "ny",
                    Lang::Sa => "n",
                    Lang::Bn => {
                        if i == 1 {
                            ""
                        } else {
                            "g"
                        }
                    }
                    _ => "y",
                }
            } else if before(C::C)
                || before(C::Ch)
                || before(C::J)
                || before(C::Jh)
                || !lang.dravidian()
            {
                "n"
            } else if after(C::Ny) {
                "j"
            } else if dead {
                "n"
            } else {
                "nj"
            }
        }
        // Malayalam ട between vowels is said, and written, "d" ("avide", "nadathi"), and so
        // is ണ്ട ("undu", "kandu", "vendi").
        C::Tt => {
            if lang == Lang::Ml && (after(C::Nn) || !dead && prev.is_some_and(|p| p.sounds())) {
                "d"
            } else {
                "t"
            }
        }
        C::Tth => "th",
        C::Dd => "d",
        C::Ddh => "dh",
        C::Nn | C::N | C::Nnn => "n",
        // Malayalam writes its dental t "th" (and a doubled one too: "chithram", "purathu");
        // Telugu and Kannada annotators mostly write "t".
        C::T => match lang {
            Lang::Ml => {
                if dead && before(C::T) {
                    ""
                } else {
                    "th"
                }
            }
            _ => "t",
        },
        C::Th => "th",
        // Malayalam ദ്ധ is written "dh" ("budhi", "yudham").
        C::D => {
            if lang == Lang::Ml && dead && before(C::Dh) {
                ""
            } else {
                "d"
            }
        }
        C::Dh => "dh",
        C::P => "p",
        // ph where it starts a syllable at the start of a word ("phir", "phool"), f elsewhere
        // ("kaafi", "taraf", "khilaaf": Persian words written without the nukta). Marathi and
        // Malayalam annotators write f two times in three wherever it stands ("fakt", "falam").
        C::Ph => {
            if matches!(lang, Lang::Mr | Lang::Ml)
                || matches!(lang, Lang::Hi | Lang::Pa | Lang::Gu) && !(initial && units[i].sounds())
            {
                "f"
            } else {
                "ph"
            }
        }
        C::B => {
            // Bengali ্ব after a consonant: "bishwo", "dwara" (but "mb", "bb").
            if lang == Lang::Bn && after_dead && !after(C::M) && !after(C::B) {
                "w"
            } else {
                "b"
            }
        }
        C::Bh => "bh",
        C::M => "m",
        C::Y | C::Yy => "y",
        C::Ya => {
            if after_dead {
                "y"
            } else {
                "j"
            }
        }
        C::R => "r",
        C::Rr => {
            if lang == Lang::Ml
                && (after(C::Rr) || dead && before(C::Rr) || after(C::N) || after(C::Nnn))
            {
                "t"
            } else {
                "r"
            }
        }
        C::L | C::Ll => "l",
        C::Lll => {
            if lang.dravidian() {
                "zh"
            } else {
                "l"
            }
        }
        C::V => match lang {
            // Hindi writes w before a long a or an o ("waade", "wala", "woh") and after a
            // consonant ("dwaar", "swar", "vishwas"), v elsewhere ("vartamaan", "vidhan") and
            // after an r ("purv", "sarv").
            Lang::Hi => {
                let u = units[i];
                if after_dead {
                    if after(C::R) { "v" } else { "w" }
                } else {
                    match u.v {
                        Vs::Is(V::Aa | V::O | V::Oo | V::Oc) => "w",
                        _ => "v",
                    }
                }
            }
            _ => "v",
        },
        C::W => "w",
        C::Sh | C::Ss => {
            // Bengali ক্ষ: "kh" at the start of a word, "kkh" inside one.
            if lang == Lang::Bn && after(C::K) {
                "kh"
            } else {
                "sh"
            }
        }
        C::S => "s",
        C::H => "h",
        C::Q => "q",
        C::Khh => "kh",
        C::Ghh => "gh",
        C::Z => "z",
        C::Rd => match lang {
            Lang::Hi | Lang::Mr | Lang::Ne | Lang::Sa => "d",
            _ => "r",
        },
        C::Rh => match lang {
            Lang::Hi | Lang::Mr | Lang::Ne | Lang::Sa => "dh",
            _ => "rh",
        },
        C::F => "f",
    }
}

/// Tamil: one letter for k/g, ch/s, t/d, th/dh, p/b, said by where it stands.
fn tamil(units: &[Unit], i: usize, c: C) -> &'static str {
    let u = units[i];
    let prev = i.checked_sub(1).map(|p| units[p]);
    let next = units.get(i + 1);
    let after = |c: C| prev.is_some_and(|p| p.is(c) && p.v == Vs::Dead);
    let after_nasal = prev.is_some_and(|p| p.v == Vs::Dead && p.c.is_some_and(C::is_nasal));
    let between_vowels = prev.is_some_and(|p| p.sounds());
    let geminate_first = u.v == Vs::Dead && next.is_some_and(|w| w.is(c));
    let dead = u.v == Vs::Dead;
    // After r, l, zh or y a stop is voiced too ("maanavargal").
    let after_soft = prev.is_some_and(|p| {
        p.v == Vs::Dead && matches!(p.c, Some(C::R | C::L | C::Ll | C::Lll | C::Y))
    });
    match c {
        C::K => {
            if dead || after(C::K) || !(after_nasal || between_vowels || after_soft) {
                "k"
            } else {
                "g"
            }
        }
        // A doubled ch is written in full ("pachchai"), and so is one that ends a word.
        C::C => {
            if geminate_first || after(C::C) || after(C::Tt) || after(C::Rr) {
                "ch"
            } else if after(C::Ny) || after_nasal {
                "j"
            } else if dead && next.is_none() {
                "ch"
            } else {
                "s"
            }
        }
        C::Tt => {
            if dead || after(C::Tt) || !(after_nasal || between_vowels) {
                "t"
            } else {
                "d"
            }
        }
        // th wherever it stands: "thth" doubled ("muththam", "paththu"), "nth" after n
        // ("vanthathu", "sontham"), as about four annotators in five write them.
        C::T => "th",
        C::P => {
            if after_nasal {
                "b"
            } else {
                "p"
            }
        }
        C::Rr => {
            if geminate_first {
                "t"
            } else if after(C::Nnn) || after(C::N) {
                "dr"
            } else {
                "r"
            }
        }
        C::Ng => {
            if after(C::Ng) {
                "g"
            } else {
                "n"
            }
        }
        C::Ny => {
            if next.is_some_and(|w| w.is(C::C)) {
                "n"
            } else if after(C::Ny) {
                "j"
            } else {
                "nj"
            }
        }
        C::Lll => "zh",
        C::Ll | C::L => "l",
        C::Nn | C::N | C::Nnn => "n",
        C::M => "m",
        C::Y => "y",
        C::R => "r",
        C::V => "v",
        C::J => "j",
        C::Sh | C::Ss => "sh",
        C::S => "s",
        C::H => "h",
        other => consonant(Lang::Te, units, i, other),
    }
}

/// Whether the dead consonant ending a Malayalam word ends an English word or name ("bus",
/// "blog", "agent", "ticket"), said without the samvruthokaram "u" of Malayalam's own words
/// ("aanu", "athu", "ennu", "paattu"). The endings Malayalam's own words seldom have: s, d, g,
/// b, f, j, sh, ng, y, a single k or p, and റ്റ, ന്റ (the English t, nt). In Dakshina's
/// sentences an "u" follows these in one ending in twenty, and the others in two in three or
/// more.
fn malayalam_loan_ending(units: &[Unit], i: usize) -> bool {
    let prev = i.checked_sub(1).map(|p| units[p]);
    let after = |c: C| prev.is_some_and(|p| p.is(c) && p.v == Vs::Dead);
    match units[i].c {
        Some(
            C::S
            | C::Dd
            | C::G
            | C::B
            | C::Ph
            | C::F
            | C::J
            | C::Ss
            | C::Sh
            | C::Z
            | C::Ng
            | C::D
            | C::Y,
        ) => true,
        Some(C::K) => !after(C::K),
        Some(C::P) => !after(C::P) && !after(C::M),
        Some(C::Rr) => after(C::Rr) || after(C::N) || after(C::Nnn),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_by_code() {
        assert_eq!(Lang::from_code("hi"), Some(Lang::Hi));
        assert_eq!(Lang::from_code("mr-IN"), Some(Lang::Mr));
        assert_eq!(Lang::from_code("PAN"), Some(Lang::Pa));
        assert_eq!(Lang::from_code("en"), None);
        assert_eq!(
            Lang::of(Script::Deva, Some("ta")),
            Some(Lang::Hi),
            "a Tamil hint does not read Devanagari as Tamil"
        );
        assert_eq!(Lang::of(Script::Deva, Some("nep")), Some(Lang::Ne));
        assert_eq!(Lang::of(Script::Latn, Some("hi")), None);
    }

    /// Each word as the rules alone spell it (no Dakshina table).
    #[track_caller]
    fn rules(lang: Lang, cases: &[(&str, &str)]) {
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|(native, want)| {
                let got = word_to_latin(native, lang, false);
                (got != *want).then(|| format!("{native}: want {want}, got {got}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{lang:?}:\n{}", wrong.join("\n"));
    }

    #[test]
    fn malayalam_chillus_are_dead_consonants_with_no_vowel() {
        rules(
            Lang::Ml,
            &[
                ("അവൻ", "avan"),
                ("അവൾ", "aval"),
                ("അവർ", "avar"),
                ("കണ്ണിൽ", "kannil"),
                ("കൺ", "kan"),
            ],
        );
    }

    #[test]
    fn malayalam_says_a_final_virama_as_u_in_its_own_words() {
        rules(
            Lang::Ml,
            &[
                ("ആണ്", "aanu"),
                ("അത്", "athu"),
                ("എന്ന്", "ennu"),
                ("കഴിച്ച്", "kazhichu"),
                ("പാട്ട്", "paattu"),
            ],
        );
    }

    #[test]
    fn malayalam_loanwords_end_on_their_consonant() {
        // English words end on the consonant ("bus", "blog", "agent"), not on a samvruthokaram.
        rules(
            Lang::Ml,
            &[
                ("ബസ്", "bas"),
                ("ക്ലാസ്", "klaas"),
                ("ബ്ലോഗ്", "blog"),
                ("ഏജന്റ്", "ejant"),
                ("ടിക്കറ്റ്", "tikkatt"),
                ("ബാങ്ക്", "baank"),
                ("ക്ലബ്", "klab"),
            ],
        );
    }

    #[test]
    fn malayalam_doubles_kk_and_tt_and_writes_tth_th() {
        rules(
            Lang::Ml,
            &[
                ("നോക്കുക", "nokkuka"),
                ("ഒത്തിരി", "othiri"),
                ("കുട്ടി", "kutti"),
            ],
        );
    }

    #[test]
    fn malayalam_nt_after_n_retroflex_is_nd() {
        // ണ്ട is said, and written, "nd": "undu", "kandu", "vendi".
        rules(
            Lang::Ml,
            &[("ഉണ്ട്", "undu"), ("കണ്ടു", "kandu"), ("വേണ്ടി", "vendi")],
        );
    }

    #[test]
    fn malayalam_n_dental_and_alveolar() {
        // ന്ത (dental) "nth", ന്റ (alveolar) "nt".
        rules(Lang::Ml, &[("മുന്തിരി", "munthiri"), ("അവന്റെ", "avante")]);
    }

    #[test]
    fn malayalam_zh_ddh_and_f() {
        rules(
            Lang::Ml,
            &[
                ("മഴ", "mazha"),
                ("പുഴ", "puzha"),
                ("ബുദ്ധി", "budhi"),
                ("യുദ്ധം", "yudham"),
                ("ഫലം", "falam"),
            ],
        );
    }

    #[test]
    fn tamil_stops_by_where_they_stand() {
        rules(
            Lang::Ta,
            &[
                // க: k at the start and doubled, g between vowels and after a nasal.
                ("கடல்", "kadal"),
                ("புகழ்", "pugazh"),
                ("மகன்", "magan"),
                ("அக்கா", "akkaa"),
                ("தங்கம்", "thangam"),
                // ச: s alone, ch doubled, j after a nasal.
                ("சந்தோஷம்", "santhosham"),
                ("பஞ்சு", "panju"),
                // ட: d between vowels, t doubled.
                ("பாடல்", "paadal"),
                ("பாட்டு", "paattu"),
                // ழ zh, ற r (tr doubled, dr after n), ன n, a final u.
                ("தமிழ்", "thamizh"),
                ("ஆறு", "aaru"),
                ("வெற்றி", "vetri"),
                ("நன்றி", "nandri"),
                ("அவன்", "avan"),
            ],
        );
    }

    #[test]
    fn tamil_nth_thth_and_chch() {
        // As most people write them: ந்த "nth", த்த "thth", ச்ச "chch".
        rules(
            Lang::Ta,
            &[
                ("வந்தது", "vanthathu"),
                ("சொந்தம்", "sontham"),
                ("முத்தம்", "muththam"),
                ("பத்து", "paththu"),
                ("பச்சை", "pachchai"),
                ("இந்தச்", "inthach"),
            ],
        );
    }

    #[test]
    fn marathi_nasal_plurals_keep_their_n() {
        // The plural oblique -ां is said before -ना, -नी, -मध्ये, -पासून: "mulanna", "lokanni".
        rules(
            Lang::Mr,
            &[
                ("मुलांना", "mulanna"),
                ("लोकांनी", "lokanni"),
                ("घरांमध्ये", "gharanmadhye"),
                ("गावांपासून", "gavanpasun"),
            ],
        );
    }

    #[test]
    fn marathi_r_vowel_z_and_f() {
        rules(
            Lang::Mr,
            &[
                ("कृपा", "krupa"),
                ("पृथ्वी", "pruthvi"),
                ("झाड", "zad"),
                ("झोप", "zop"),
                ("फक्त", "fakt"),
                ("फूल", "ful"),
            ],
        );
        // Hindi keeps its own: "kripa", "jhad", "phul".
        rules(
            Lang::Hi,
            &[("कृपा", "kripa"), ("झाड़", "jhad"), ("फूल", "phul")],
        );
        // "my" and "your" keep their "jh" (Venuta's word list).
        rules(
            Lang::Mr,
            &[("माझा", "majha"), ("माझ्या", "majhya"), ("तुझी", "tujhi")],
        );
    }

    #[test]
    fn marathi_drops_the_a_of_a_stem_before_its_case_ending() {
        // अल्बम + मधील, घर + च्या, सचिन + ला: the stem's last "a" goes, as at a word's end.
        rules(
            Lang::Mr,
            &[
                ("अल्बममधील", "albammadhil"),
                ("घरच्या", "gharchya"),
                ("सचिनला", "sachinla"),
                ("कोल्हापूरपासून", "kolhapurpasun"),
                // A short stem keeps it: "kala", not "kla".
                ("कला", "kala"),
            ],
        );
    }

    #[test]
    fn marathi_ll_and_dny() {
        rules(
            Lang::Mr,
            &[("जवळ", "javal"), ("बाळ", "bal"), ("ज्ञान", "dnyan")],
        );
    }
}
