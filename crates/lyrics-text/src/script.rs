//! Which script each character is in, and a line split into runs of one script.
//!
//! Scripts are named by their ISO 15924 codes, as the lyrics model stores them ("Deva",
//! "Hang", "Latn"). The ranges follow the Unicode blocks of each script, which is all lyrics
//! need: spaces, digits and punctuation are `Zyyy` (common to every script), combining marks
//! and joiners are `Zinh` (they belong to the letter before them).

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// A writing system (ISO 15924).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Script {
    Latn,
    Grek,
    Cyrl,
    Armn,
    Hebr,
    Arab,
    Syrc,
    Thaa,
    Deva,
    Beng,
    Guru,
    Gujr,
    Orya,
    Taml,
    Telu,
    Knda,
    Mlym,
    Sinh,
    Thai,
    Laoo,
    Tibt,
    Mymr,
    Geor,
    Hang,
    Ethi,
    Khmr,
    Hira,
    Kana,
    Hani,
    Bopo,
    /// Japanese: a line that mixes Han with kana (only from [`line_script`]).
    Jpan,
    /// Common to every script: spaces, digits, punctuation, symbols.
    Zyyy,
    /// Inherited: a combining mark or joiner, part of the letter before it.
    Zinh,
    /// Anything else.
    Zzzz,
}

impl Script {
    /// The ISO 15924 code ("Deva").
    pub fn code(self) -> &'static str {
        use Script::*;
        match self {
            Latn => "Latn",
            Grek => "Grek",
            Cyrl => "Cyrl",
            Armn => "Armn",
            Hebr => "Hebr",
            Arab => "Arab",
            Syrc => "Syrc",
            Thaa => "Thaa",
            Deva => "Deva",
            Beng => "Beng",
            Guru => "Guru",
            Gujr => "Gujr",
            Orya => "Orya",
            Taml => "Taml",
            Telu => "Telu",
            Knda => "Knda",
            Mlym => "Mlym",
            Sinh => "Sinh",
            Thai => "Thai",
            Laoo => "Laoo",
            Tibt => "Tibt",
            Mymr => "Mymr",
            Geor => "Geor",
            Hang => "Hang",
            Ethi => "Ethi",
            Khmr => "Khmr",
            Hira => "Hira",
            Kana => "Kana",
            Hani => "Hani",
            Bopo => "Bopo",
            Jpan => "Jpan",
            Zyyy => "Zyyy",
            Zinh => "Zinh",
            Zzzz => "Zzzz",
        }
    }

    /// Letters of a real script (not common, inherited or unknown).
    pub fn is_letters(self) -> bool {
        !matches!(self, Script::Zyyy | Script::Zinh | Script::Zzzz)
    }

    /// The script of one character.
    pub fn of(ch: char) -> Script {
        let c = ch as u32;
        if c < 0x80 {
            return if ch.is_ascii_alphabetic() {
                Script::Latn
            } else {
                Script::Zyyy
            };
        }
        match BLOCKS.binary_search_by(|&(lo, hi, _)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(i) => BLOCKS[i].2,
            Err(_) => Script::Zzzz,
        }
    }
}

/// Code point ranges and their scripts, sorted and apart (a test checks).
#[rustfmt::skip]
pub(crate) const BLOCKS: &[(u32, u32, Script)] = {
    use Script::*;
    &[
        (0x80, 0xA9, Zyyy), (0xAA, 0xAA, Latn), (0xAB, 0xB9, Zyyy), (0xBA, 0xBA, Latn),
        (0xBB, 0xBF, Zyyy), (0xC0, 0xD6, Latn), (0xD7, 0xD7, Zyyy), (0xD8, 0xF6, Latn),
        (0xF7, 0xF7, Zyyy), (0xF8, 0x2AF, Latn), (0x2B0, 0x2FF, Zyyy), (0x300, 0x36F, Zinh),
        (0x370, 0x3FF, Grek), (0x400, 0x52F, Cyrl), (0x530, 0x58F, Armn), (0x591, 0x5FF, Hebr),
        // Arabic punctuation and digits are shared with the other scripts that use them.
        (0x600, 0x60B, Arab), (0x60C, 0x60C, Zyyy), (0x60D, 0x61A, Arab), (0x61B, 0x61B, Zyyy),
        (0x61C, 0x61E, Arab), (0x61F, 0x61F, Zyyy), (0x620, 0x63F, Arab), (0x640, 0x640, Zyyy),
        (0x641, 0x64A, Arab), (0x64B, 0x655, Zinh), (0x656, 0x65F, Arab), (0x660, 0x669, Zyyy),
        (0x66A, 0x66F, Arab), (0x670, 0x670, Zinh), (0x671, 0x6FF, Arab), (0x700, 0x74F, Syrc),
        (0x750, 0x77F, Arab), (0x780, 0x7BF, Thaa), (0x8A0, 0x8FF, Arab),
        // The Indian scripts, block by block. The danda is in Devanagari's block and is read
        // with it ("." in English letters), whichever script it ends.
        (0x900, 0x97F, Deva), (0x980, 0x9FF, Beng), (0xA00, 0xA7F, Guru), (0xA80, 0xAFF, Gujr),
        (0xB00, 0xB7F, Orya), (0xB80, 0xBFF, Taml), (0xC00, 0xC7F, Telu), (0xC80, 0xCFF, Knda),
        (0xD00, 0xD7F, Mlym), (0xD80, 0xDFF, Sinh), (0xE00, 0xE7F, Thai), (0xE80, 0xEFF, Laoo),
        (0xF00, 0xFFF, Tibt), (0x1000, 0x109F, Mymr), (0x10A0, 0x10FF, Geor), (0x1100, 0x11FF, Hang),
        (0x1200, 0x139F, Ethi), (0x1780, 0x17FF, Khmr), (0x1AB0, 0x1AFF, Zinh), (0x1C80, 0x1C8F, Cyrl),
        (0x1C90, 0x1CBF, Geor), (0x1CD0, 0x1CFF, Deva), (0x1D00, 0x1D7F, Latn), (0x1DC0, 0x1DFF, Zinh),
        (0x1E00, 0x1EFF, Latn), (0x1F00, 0x1FFF, Grek), (0x2000, 0x200B, Zyyy), (0x200C, 0x200D, Zinh),
        (0x200E, 0x20CF, Zyyy), (0x20D0, 0x20FF, Zinh), (0x2100, 0x2BFF, Zyyy), (0x2DE0, 0x2DFF, Cyrl),
        (0x2E80, 0x2FDF, Hani), (0x3000, 0x3004, Zyyy), (0x3005, 0x3005, Hani), (0x3006, 0x3006, Zyyy),
        (0x3007, 0x3007, Hani), (0x3008, 0x3020, Zyyy), (0x3021, 0x3029, Hani), (0x302A, 0x3037, Zyyy),
        (0x3038, 0x303B, Hani), (0x303C, 0x303F, Zyyy), (0x3041, 0x3098, Hira), (0x3099, 0x309A, Zinh),
        (0x309B, 0x309F, Hira),
        // Katakana, with the long-vowel mark (ー) and the middle dot.
        (0x30A0, 0x30FF, Kana), (0x3100, 0x312F, Bopo), (0x3130, 0x318F, Hang), (0x31A0, 0x31BF, Bopo),
        (0x31F0, 0x31FF, Kana), (0x3400, 0x4DBF, Hani), (0x4E00, 0x9FFF, Hani), (0xA640, 0xA69F, Cyrl),
        (0xA8E0, 0xA8FF, Deva), (0xA960, 0xA97F, Hang), (0xAC00, 0xD7FF, Hang), (0xF900, 0xFAFF, Hani),
        (0xFB1D, 0xFB4F, Hebr), (0xFB50, 0xFDFF, Arab), (0xFE00, 0xFE0F, Zinh), (0xFE20, 0xFE2F, Zinh),
        (0xFE30, 0xFE4F, Zyyy), (0xFE70, 0xFEFF, Arab), (0xFF00, 0xFF20, Zyyy), (0xFF21, 0xFF3A, Latn),
        (0xFF3B, 0xFF40, Zyyy), (0xFF41, 0xFF5A, Latn), (0xFF5B, 0xFF65, Zyyy), (0xFF66, 0xFF9F, Kana),
        (0xFFE0, 0xFFEF, Zyyy), (0x1F000, 0x1FAFF, Zyyy), (0x20000, 0x3134F, Hani),
    ]
};

/// A stretch of a line in one script, as a byte range of the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub script: Script,
    pub range: Range<usize>,
}

/// The line split into runs of one script. A combining mark or joiner stays in the run of the
/// letter before it; spaces, digits and punctuation make runs of their own (`Zyyy`).
pub fn runs(text: &str) -> Runs<'_> {
    Runs { text, pos: 0 }
}

/// Iterator returned by [`runs`].
pub struct Runs<'a> {
    text: &'a str,
    pos: usize,
}

impl Iterator for Runs<'_> {
    type Item = Run;

    fn next(&mut self) -> Option<Run> {
        let rest = &self.text[self.pos..];
        let mut chars = rest.char_indices();
        let (_, first) = chars.next()?;
        let mut script = Script::of(first);
        if script == Script::Zinh {
            script = Script::Zyyy;
        }
        let mut end = rest.len();
        for (i, ch) in chars {
            let s = Script::of(ch);
            if s == script || s == Script::Zinh {
                continue;
            }
            end = i;
            break;
        }
        let start = self.pos;
        self.pos += end;
        Some(Run {
            script,
            range: start..self.pos,
        })
    }
}

/// The script a line is written in: the script with the most letters, not counting Latin
/// when another script is there (a Hindi line with one English word is a Devanagari line).
/// Han with any kana is Japanese (`Jpan`). `Zyyy` when the line has no letters.
pub fn line_script(text: &str) -> Script {
    // Small counts per script, kept without allocating.
    let mut counts: [(Script, u32); 8] = [(Script::Zyyy, 0); 8];
    let mut kana = false;
    let mut han = false;
    for ch in text.chars() {
        let s = Script::of(ch);
        if !s.is_letters() {
            continue;
        }
        kana |= matches!(s, Script::Hira | Script::Kana);
        han |= s == Script::Hani;
        if let Some(slot) = counts.iter_mut().find(|(k, n)| *k == s || *n == 0) {
            slot.0 = s;
            slot.1 += 1;
        }
    }
    if kana && han {
        return Script::Jpan;
    }
    let best = |latin: bool| {
        counts
            .iter()
            .filter(|(s, n)| *n > 0 && ((*s == Script::Latn) == latin))
            .max_by_key(|(_, n)| *n)
            .map(|(s, _)| *s)
    };
    best(false).or_else(|| best(true)).unwrap_or(Script::Zyyy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocks_are_sorted_and_apart() {
        for w in BLOCKS.windows(2) {
            assert!(
                w[0].0 <= w[0].1 && w[0].1 < w[1].0,
                "{:X}..{:X} then {:X}",
                w[0].0,
                w[0].1,
                w[1].0
            );
        }
    }
}
