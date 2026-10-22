//! How Roman words are laid out in [`Pieces`]: spaces between the words of scripts written
//! without them (Chinese, Japanese), CJK punctuation as its Latin mark, and alphabets read
//! letter group by letter group with their case kept (Cyrillic, Greek).

use std::ops::Range;

use crate::Pieces;

/// How a punctuation mark sits between words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sits {
    /// Joins the word before, a space after: "," "." "!" closing quotes.
    Close,
    /// A space before, joins the word after: opening quotes and brackets.
    Open,
    /// Only a break between words (a middle dot, an ideographic space).
    Break,
}

/// CJK punctuation in Roman lines: "，" as ",", "「" as an opening quote.
pub(crate) fn cjk_punct(c: char) -> Option<(&'static str, Sits)> {
    use Sits::*;
    Some(match c {
        '，' | '、' | '､' => (",", Close),
        '。' | '｡' | '．' => (".", Close),
        '！' => ("!", Close),
        '？' => ("?", Close),
        '：' => (":", Close),
        '；' => (";", Close),
        '…' => ("...", Close),
        '‥' => ("..", Close),
        '～' | '〜' => ("~", Close),
        '—' | '―' | '－' => ("-", Close),
        '「' | '『' | '“' | '《' | '〈' | '﹁' | '﹃' => ("\"", Open),
        '」' | '』' | '”' | '》' | '〉' | '﹂' | '﹄' => ("\"", Close),
        '‘' => ("'", Open),
        '’' => ("'", Close),
        '（' | '(' => ("(", Open),
        '）' | ')' => (")", Close),
        '【' | '〔' | '［' | '[' => ("[", Open),
        '】' | '〕' | '］' | ']' => ("]", Close),
        ',' => (",", Close),
        '.' => (".", Close),
        '!' => ("!", Close),
        '?' => ("?", Close),
        ':' => (":", Close),
        ';' => (";", Close),
        '・' | '･' | '\u{3000}' => (" ", Break),
        _ => return None,
    })
}

/// Hiragana or katakana (with the long-vowel mark and the iteration marks).
pub(crate) fn is_kana(c: char) -> bool {
    matches!(c as u32, 0x3041..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F)
}

/// Full-width Latin letters and digits as plain ones ("Ａ" as "A").
pub(crate) fn narrow(c: char) -> char {
    match c as u32 {
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

/// Writes the words of a script written without spaces into [`Pieces`], with spaces between
/// them (each space part of the word before it).
pub(crate) struct Builder<'p, 'a> {
    out: &'p mut Pieces<'a>,
    /// Something was written by this builder.
    wrote: bool,
    /// A space goes before the next text.
    space: bool,
    /// Right after an opening mark: the next word joins it.
    glue: bool,
}

impl<'p, 'a> Builder<'p, 'a> {
    pub fn new(out: &'p mut Pieces<'a>) -> Self {
        Builder {
            out,
            wrote: false,
            space: false,
            glue: false,
        }
    }

    /// The next text starts a new word.
    pub fn split(&mut self) {
        if self.wrote && !self.glue {
            self.space = true;
        }
    }

    /// Text that continues the current word (or starts one after [`Builder::split`]).
    pub fn push(&mut self, s: &str, src: Range<usize>) {
        if s.is_empty() {
            return;
        }
        if self.space {
            self.out.space();
            self.space = false;
        }
        self.glue = false;
        self.wrote = true;
        self.out.push(src, s);
    }

    /// A punctuation mark, placed as it [`Sits`].
    pub fn punct(&mut self, s: &str, sits: Sits, src: Range<usize>) {
        match sits {
            Sits::Close => {
                self.space = false;
                self.push(s, src);
                self.split();
            }
            Sits::Open => {
                self.split();
                self.push(s, src);
                self.glue = true;
            }
            Sits::Break => self.split(),
        }
    }
}

/// Romanizes `line[range]`, an alphabet written with spaces (Cyrillic, Greek), word by word.
/// `convert` gets a word in lower case and returns its steps: how many letters each step reads
/// and what it writes. Case comes back from the original: a capital letter capitalises its
/// step, a word all in capitals stays in capitals. Anything that isn't a letter is kept as it
/// is (`other` may swap a mark: the Greek ";" is a question mark).
pub(crate) fn by_letters(
    line: &str,
    range: Range<usize>,
    out: &mut Pieces,
    is_letter: impl Fn(char) -> bool,
    convert: impl Fn(&[char]) -> Vec<(usize, String)>,
    other: impl Fn(char) -> Option<&'static str>,
) {
    let base = range.start;
    let chars: Vec<(usize, char)> = line[range]
        .char_indices()
        .map(|(i, c)| (base + i, c))
        .collect();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        let src = at..at + c.len_utf8();
        if !is_letter(c) {
            match other(c) {
                Some(s) => out.push(src, s),
                None => out.keep(src),
            }
            i += 1;
            continue;
        }
        let from = i;
        while i < chars.len() && is_letter(chars[i].1) {
            i += 1;
        }
        let word = &chars[from..i];
        let lower: Vec<char> = word
            .iter()
            .map(|&(_, c)| c.to_lowercase().next().unwrap_or(c))
            .collect();
        let all_upper = word.len() > 1 && word.iter().all(|&(_, c)| !c.is_lowercase());
        let mut k = 0;
        for (n, s) in convert(&lower) {
            let n = n.max(1).min(word.len() - k);
            let (src_at, first) = word[k];
            let last = word[k + n - 1];
            let src = src_at..last.0 + last.1.len_utf8();
            if all_upper {
                out.push(src, &s.to_uppercase());
            } else if first.is_uppercase() {
                let mut cs = s.chars();
                let cap: String = cs.next().into_iter().flat_map(char::to_uppercase).collect();
                out.begin(src.start);
                out.write(&cap);
                out.write(cs.as_str());
                out.end(src.end);
            } else {
                out.push(src, &s);
            }
            k += n;
            if k >= word.len() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_sits_with_its_word() {
        let line = "君、「愛」Love";
        let mut out = Pieces::new(line);
        let mut b = Builder::new(&mut out);
        b.push("kimi", 0..3);
        b.punct(",", Sits::Close, 3..6);
        b.punct("\"", Sits::Open, 6..9);
        b.push("ai", 9..12);
        b.punct("\"", Sits::Close, 12..15);
        b.split();
        b.push("Love", 15..19);
        assert_eq!(out.text(), "kimi, \"ai\" Love");
    }

    #[test]
    fn full_width_latin_is_narrowed() {
        assert_eq!(narrow('Ａ'), 'A');
        assert_eq!(narrow('９'), '9');
        assert_eq!(narrow('あ'), 'あ');
    }
}
