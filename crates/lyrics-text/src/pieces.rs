//! A romanized line and where each part of it came from, and the word timing that follows.
//!
//! Every converter in this crate returns a [`Romanized`]: the line in Roman letters plus
//! [`Piece`]s, each tying a stretch of the output to the stretch of the original it was made
//! from. [`timed_words`] then gives each Roman word the timing of the original words its
//! pieces came from, so the karaoke highlight runs on both lines: a Korean syllable timed on
//! its own keeps its own time ("sa" "rang" "hae"), a Japanese word read from two timed kanji
//! takes the time of both.

use std::ops::Range;

use crate::{Second, SecondSource, Word};

/// A line in Roman letters, with the source of each part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Romanized {
    pub text: String,
    /// In output order. Spaces the converter put between words belong to no piece.
    pub pieces: Vec<Piece>,
}

/// A stretch of the output and the stretch of the original it came from, both as byte
/// ranges (of [`Romanized::text`] and of the original line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub out: Range<usize>,
    pub src: Range<usize>,
}

impl Romanized {
    /// The line as a pronunciation line made on the device: `line` and `words` are the
    /// original's text and timed words (empty when the line is only timed as a whole).
    pub fn to_second(&self, line: &str, words: &[Word], lang: &str) -> Second {
        Second {
            text: self.text.clone(),
            words: timed_words(line, words, self),
            lang: lang.to_owned(),
            source: SecondSource::Made,
        }
    }

    /// The pieces' output texts in order, for tests and debugging ("sa|rang|hae").
    pub fn piece_texts(&self) -> Vec<&str> {
        self.pieces
            .iter()
            .map(|p| &self.text[p.out.clone()])
            .collect()
    }
}

/// Where each timed word sits in the line, as a byte range; `None` for a word that can't be
/// found (blank, or text the line doesn't have). Words are matched in order on their letters,
/// so spaces may differ between the words and the line.
fn word_ranges(line: &str, words: &[Word]) -> Vec<Option<Range<usize>>> {
    let letters: Vec<(usize, char)> = line
        .char_indices()
        .filter(|(_, c)| !c.is_whitespace())
        .collect();
    let mut k = 0;
    words
        .iter()
        .map(|w| {
            let mut range: Option<Range<usize>> = None;
            let save = k;
            for c in w.text.chars().filter(|c| !c.is_whitespace()) {
                match letters.get(k) {
                    Some(&(at, l)) if l == c => {
                        let end = at + l.len_utf8();
                        range = Some(range.map_or(at..end, |r| r.start..end));
                        k += 1;
                    }
                    _ => {
                        k = save;
                        return None;
                    }
                }
            }
            range
        })
        .collect()
}

/// The romanized line cut into timed words: each output word takes the start of the first and
/// the end of the last original word its pieces came from, and its text runs to the next
/// output word (trailing space included), so the words joined give the line back. Empty when
/// `words` is.
pub fn timed_words(line: &str, words: &[Word], roman: &Romanized) -> Vec<Word> {
    if words.is_empty() {
        return Vec::new();
    }
    let ranges = word_ranges(line, words);
    // (first word, last word, where the output word starts)
    let mut groups: Vec<(usize, usize, usize)> = Vec::new();
    for piece in &roman.pieces {
        let mut owners = ranges.iter().enumerate().filter_map(|(i, r)| {
            r.as_ref()
                .filter(|r| r.start < piece.src.end && piece.src.start < r.end)
                .map(|_| i)
        });
        let Some(first) = owners.next() else { continue };
        let last = owners.next_back().unwrap_or(first);
        match groups.last_mut() {
            Some(g) if first <= g.1 => g.1 = g.1.max(last),
            _ => groups.push((first, last, piece.out.start)),
        }
    }
    if groups.is_empty() {
        return vec![Word {
            start_ms: words[0].start_ms,
            end_ms: words[words.len() - 1].end_ms,
            text: roman.text.clone(),
        }];
    }
    groups[0].2 = 0;
    let mut out = Vec::with_capacity(groups.len());
    for (i, &(first, last, at)) in groups.iter().enumerate() {
        let end = groups.get(i + 1).map_or(roman.text.len(), |g| g.2);
        out.push(Word {
            start_ms: words[first].start_ms,
            end_ms: words[last].end_ms.max(words[first].start_ms),
            text: roman.text[at..end].to_owned(),
        });
    }
    out
}

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

/// Builds a [`Romanized`] for scripts written without spaces (Chinese, Japanese): words are
/// pushed with the source they came from and spaces go between them.
#[derive(Debug, Default)]
pub(crate) struct Builder {
    text: String,
    pieces: Vec<Piece>,
    /// A space goes before the next text.
    space: bool,
    /// Right after an opening mark: the next word joins it.
    glue: bool,
}

impl Builder {
    /// The next text starts a new word.
    pub fn split(&mut self) {
        if !self.text.is_empty() && !self.glue {
            self.space = true;
        }
    }

    /// Text that continues the current word (or starts one after [`Builder::split`]).
    pub fn push(&mut self, s: &str, src: Range<usize>) {
        if s.is_empty() {
            return;
        }
        if self.space {
            self.text.push(' ');
            self.space = false;
        }
        self.glue = false;
        let at = self.text.len();
        self.text.push_str(s);
        match self.pieces.last_mut() {
            // One source stretch read as several outputs in a row stays one piece.
            Some(p) if p.src == src && p.out.end == at => p.out.end = self.text.len(),
            _ => self.pieces.push(Piece {
                out: at..self.text.len(),
                src,
            }),
        }
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

    pub fn finish(self) -> Romanized {
        Romanized {
            text: self.text,
            pieces: self.pieces,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
        Word {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }

    fn roman(parts: &[(&str, Range<usize>)], spaces: &[usize]) -> Romanized {
        let mut b = Builder::default();
        for (i, (s, src)) in parts.iter().enumerate() {
            if spaces.contains(&i) {
                b.split();
            }
            b.push(s, src.clone());
        }
        b.finish()
    }

    #[test]
    fn one_roman_word_per_timed_syllable() {
        // 사랑해, each syllable timed: three bytes per syllable.
        let line = "사랑해";
        let words = [w(0, 100, "사"), w(100, 200, "랑"), w(200, 300, "해")];
        let r = roman(&[("sa", 0..3), ("rang", 3..6), ("hae", 6..9)], &[]);
        assert_eq!(
            timed_words(line, &words, &r),
            vec![w(0, 100, "sa"), w(100, 200, "rang"), w(200, 300, "hae")]
        );
    }

    #[test]
    fn a_piece_from_two_timed_words_takes_both_times() {
        // 未来 read as one piece, the two kanji timed one by one.
        let line = "未来へ";
        let words = [w(0, 100, "未"), w(100, 200, "来"), w(200, 300, "へ")];
        let r = roman(&[("mirai", 0..6), ("e", 6..9)], &[1]);
        assert_eq!(
            timed_words(line, &words, &r),
            vec![w(0, 200, "mirai "), w(200, 300, "e")]
        );
    }

    #[test]
    fn words_with_spaces_and_the_line_without() {
        let line = "君の 名は";
        let words = [w(0, 100, "君の "), w(100, 300, "名は")];
        let r = roman(
            &[("kimi", 0..3), ("no", 3..6), ("na", 7..10), ("wa", 10..13)],
            &[1, 2, 3],
        );
        assert_eq!(r.text, "kimi no na wa");
        assert_eq!(
            timed_words(line, &words, &r),
            vec![w(0, 100, "kimi no "), w(100, 300, "na wa")]
        );
    }

    #[test]
    fn no_timed_words_no_roman_words() {
        let r = roman(&[("a", 0..1)], &[]);
        assert!(timed_words("a", &[], &r).is_empty());
    }

    #[test]
    fn words_that_do_not_match_the_line_still_give_the_whole_line() {
        let r = roman(&[("sa", 0..3)], &[]);
        assert_eq!(
            timed_words("사", &[w(5, 9, "zzz")], &r),
            vec![w(5, 9, "sa")]
        );
    }

    #[test]
    fn punctuation_sits_with_its_word() {
        let mut b = Builder::default();
        b.push("kimi", 0..3);
        b.punct(",", Sits::Close, 3..6);
        b.punct("\"", Sits::Open, 6..9);
        b.push("ai", 9..12);
        b.punct("\"", Sits::Close, 12..15);
        b.split();
        b.push("Love", 15..19);
        assert_eq!(b.finish().text, "kimi, \"ai\" Love");
    }

    #[test]
    fn full_width_latin_is_narrowed() {
        assert_eq!(narrow('Ａ'), 'A');
        assert_eq!(narrow('９'), '9');
        assert_eq!(narrow('あ'), 'あ');
    }
}
