//! The romanized line as pieces that remember which source bytes they came from, and the
//! romanized words timed by the original's words.
//!
//! A line is romanized whole (Hindi needs whole words to know which "a" is silent, Japanese
//! needs whole phrases to read them), but each piece of output keeps the byte range of the
//! source it was made from. A timed source word then gets the pieces that came from inside it:
//! "बातों" timed as "बा" + "तों" gives "baa" + "ton", each with its own timing. When one piece
//! came from several timed words (a whole word taken from the word list), those words become
//! one romanized word, timed from the first one's start to the last one's end.

use std::ops::Range;

use crate::Word;

/// One piece of output and the source bytes it came from (both byte ranges).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub src: Range<u32>,
    pub out: Range<u32>,
}

/// The romanized line being written: its text, and a span per piece.
#[derive(Debug, Clone)]
pub struct Pieces<'a> {
    line: &'a str,
    text: String,
    spans: Vec<Span>,
    /// Where the piece being written began, in the source and in the output.
    open: Option<(u32, u32)>,
}

impl<'a> Pieces<'a> {
    pub fn new(line: &'a str) -> Self {
        Self::with_capacity(line, line.len())
    }

    pub fn with_capacity(line: &'a str, bytes: usize) -> Self {
        Pieces {
            line,
            text: String::with_capacity(bytes),
            spans: Vec::with_capacity(16),
            open: None,
        }
    }

    /// The source line.
    pub fn line(&self) -> &'a str {
        self.line
    }

    /// One piece: `roman`, made from `src` (a byte range of the line).
    pub fn push(&mut self, src: Range<usize>, roman: &str) {
        let start = self.text.len() as u32;
        self.text.push_str(roman);
        self.spans.push(Span {
            src: src.start as u32..src.end as u32,
            out: start..self.text.len() as u32,
        });
    }

    /// The source bytes `src` as they are (Latin words, digits, spaces, punctuation).
    pub fn keep(&mut self, src: Range<usize>) {
        let line = self.line;
        self.push(src.clone(), &line[src]);
    }

    /// Starts a piece made from the source at `src_start`, written with [`Pieces::write`] and
    /// closed by [`Pieces::end`]. Lets a converter write a piece in parts without a buffer.
    pub fn begin(&mut self, src_start: usize) {
        self.open = Some((src_start as u32, self.text.len() as u32));
    }

    /// Writes into the open piece.
    pub fn write(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Closes the open piece; it came from the source up to `src_end`.
    pub fn end(&mut self, src_end: usize) {
        if let Some((src, out)) = self.open.take() {
            self.spans.push(Span {
                src: src..src_end as u32,
                out: out..self.text.len() as u32,
            });
        }
    }

    /// Writes `spelled` in place of the pieces written since `spans` and `bytes` (counts of
    /// spans and text bytes taken before them): each piece gets the part of `spelled` that
    /// lines up with its own text, found by the fewest edits between the two spellings. A
    /// converter spells a word letter by letter, then puts a listed spelling over it and keeps
    /// the letters' sources.
    pub fn respell(&mut self, spans: usize, bytes: usize, spelled: &str) {
        const MAX: usize = 48;
        let old = &self.text.as_bytes()[bytes..];
        let new = spelled.as_bytes();
        let pieces = self.spans.len() - spans;
        if pieces == 0 {
            return;
        }
        let (m, n) = (old.len(), new.len());
        if pieces == 1 || m > MAX || n > MAX {
            // One piece for the whole word.
            let (src_start, src_end) = (
                self.spans[spans].src.start,
                self.spans[self.spans.len() - 1].src.end,
            );
            self.spans.truncate(spans);
            self.text.truncate(bytes);
            self.push(src_start as usize..src_end as usize, spelled);
            return;
        }
        // Edit distances between prefixes, then the path back: where each old position meets
        // the new spelling (letters added at a boundary go to the piece before it).
        let mut d = [[0u8; MAX + 1]; MAX + 1];
        for (i, row) in d.iter_mut().enumerate().take(m + 1) {
            row[0] = i as u8;
        }
        for (j, cell) in d[0].iter_mut().enumerate().take(n + 1) {
            *cell = j as u8;
        }
        for i in 1..=m {
            for j in 1..=n {
                let sub = d[i - 1][j - 1] + u8::from(old[i - 1] != new[j - 1]);
                d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
            }
        }
        let mut at = [0u8; MAX + 1];
        let (mut i, mut j) = (m, n);
        at[m] = n as u8;
        while i > 0 || j > 0 {
            if i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + u8::from(old[i - 1] != new[j - 1]) {
                i -= 1;
                j -= 1;
            } else if j > 0 && d[i][j] == d[i][j - 1] + 1 {
                j -= 1;
            } else {
                i -= 1;
            }
            at[i] = at[i].max(j as u8);
        }
        let base = bytes as u32;
        let cut = |pos: u32| at[(pos - base) as usize] as u32 + base;
        for s in &mut self.spans[spans..] {
            s.out = cut(s.out.start)..cut(s.out.end);
        }
        // Keep the pieces contiguous from the word's start to its end.
        let first = &mut self.spans[spans];
        first.out.start = base;
        let last = self.spans.len() - 1;
        self.spans[last].out.end = base + n as u32;
        for k in spans + 1..=last {
            let prev_end = self.spans[k - 1].out.end;
            self.spans[k].out.start = prev_end;
            if self.spans[k].out.end < prev_end {
                self.spans[k].out.end = prev_end;
            }
        }
        self.text.truncate(bytes);
        self.text.push_str(spelled);
    }

    /// The text written so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Upper-cases the first letter of the line when it was made here (an English word the
    /// line starts with is left as it was written).
    pub fn capitalize_first(&mut self) {
        // Only a line that starts with a letter ("2024 mein" stays as it is).
        let Some(i) = self.text.find(char::is_alphanumeric) else {
            return;
        };
        if !self.text[i..].starts_with(char::is_alphabetic) {
            return;
        }
        let (line, text) = (self.line, &self.text);
        let made = self
            .spans
            .iter()
            .find(|s| (s.out.start as usize..s.out.end as usize).contains(&i))
            .is_some_and(|s| {
                line.get(s.src.start as usize..s.src.end as usize)
                    != text.get(s.out.start as usize..s.out.end as usize)
            });
        if made && let Some(first) = self.text.get_mut(i..i + 1) {
            first.make_ascii_uppercase();
        }
    }

    pub fn into_text(self) -> String {
        self.text
    }
}

/// The romanized words, each timed by the original word(s) it came from. Empty when the line
/// has no timed words, or when a word's text cannot be found in the line in order (the second
/// line then follows the line's timing only).
pub fn words(line: &str, timed: &[Word], out: &Pieces) -> Vec<Word> {
    if timed.is_empty() {
        return Vec::new();
    }
    // Where each timed word is in the line.
    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(timed.len());
    let mut at = 0;
    for w in timed {
        let t = w.text.trim();
        if t.is_empty() {
            ranges.push(at..at);
            continue;
        }
        let Some(i) = line[at..].find(t) else {
            return Vec::new();
        };
        ranges.push(at + i..at + i + t.len());
        at += i + t.len();
    }
    // The timed word a source byte belongs to: the last one starting at or before it (bytes
    // between words go with the word before them, bytes before the first with the first).
    let owner = |byte: usize| {
        ranges
            .iter()
            .rposition(|r| r.start <= byte && !r.is_empty())
            .unwrap_or(0)
    };

    let text = out.text();
    let mut words = Vec::with_capacity(timed.len());
    // The romanized word being built: its first and last timed words and where its text starts.
    let mut cur: Option<(usize, usize, usize)> = None;
    for span in out.spans() {
        let first = owner(span.src.start as usize);
        let last = if span.src.end > span.src.start {
            owner(span.src.end as usize - 1).max(first)
        } else {
            first
        };
        match &mut cur {
            Some((_, l, _)) if first <= *l => *l = (*l).max(last),
            _ => {
                if let Some((f, l, o)) = cur.take() {
                    words.push(timed_word(timed, f, l, &text[o..span.out.start as usize]));
                }
                cur = Some((first, last, span.out.start as usize));
            }
        }
    }
    if let Some((f, l, o)) = cur {
        words.push(timed_word(timed, f, l, &text[o..]));
    }
    words
}

fn timed_word(timed: &[Word], first: usize, last: usize, text: &str) -> Word {
    Word {
        start_ms: timed[first].start_ms,
        end_ms: timed[last].end_ms,
        text: text.to_string(),
    }
}
