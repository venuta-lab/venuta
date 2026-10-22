//! Second lines for lyrics: a sung line in another script, said in English letters (the
//! "pronunciation" line), timed by the original's words so karaoke runs on both lines.
//!
//! Pure Rust with no files, network or platform code, so the desktop app and Android share it.
//!
//! How a line is read ([`romanize`]):
//! 1. The line is split into runs of one script ([`script::runs`]). Latin words, digits,
//!    spaces and punctuation are kept as they are.
//! 2. Each run goes to its script family's module, which writes its romanized text into
//!    [`Pieces`], keeping which source bytes every piece came from.
//! 3. [`timing::words`] gives each romanized word the timing of the original word(s) it came from.
//!
//! Adding a script family: a `pub mod` here, and one arm in [`convert`]. The module's entry
//! point reads `line[range]`, pushes its pieces with absolute byte ranges into `out`, and says
//! which language it read the run as.

use std::ops::Range;

use serde::{Deserialize, Serialize};

pub mod indic;
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
    /// Nepali, Sanskrit). Without it each script is read as its most common language.
    pub lang: Option<&'a str>,
}

impl<'a> Line<'a> {
    pub fn new(text: &'a str) -> Self {
        Line {
            text,
            words: &[],
            lang: None,
        }
    }

    pub fn words(self, words: &'a [Word]) -> Self {
        Line { words, ..self }
    }

    pub fn lang(self, lang: Option<&'a str>) -> Self {
        Line { lang, ..self }
    }
}

/// The line in English letters, or `None` when there is nothing to romanize (the line is
/// already Latin, or its script has no reader here).
pub fn romanize(line: &Line) -> Option<Roman> {
    let text = line.text;
    let mut out = Pieces::with_capacity(text, text.len() + text.len() / 2);
    // Letters read per language, for the pronunciation's tag (a line rarely has more than two).
    let mut read: [(&'static str, usize); 4] = [("", 0); 4];
    for run in script::runs(text) {
        match convert(run.script, text, run.range.clone(), line.lang, &mut out) {
            Some(lang) => {
                if let Some(slot) = read.iter_mut().find(|(l, n)| *l == lang || *n == 0) {
                    slot.0 = lang;
                    slot.1 += run.range.len();
                }
            }
            None => out.keep(run.range),
        }
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
        _ => None,
    }
}
