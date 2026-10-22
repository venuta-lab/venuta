//! Lyric lines in Roman letters, made on the device (`docs/design/lyrics-second-line.md`).
//!
//! One converter per writing system. Each returns a [`pieces::Romanized`]: the line in Roman
//! letters and, for every part of it, the characters of the original it came from. That map is
//! what keeps the karaoke highlight running on the second line: [`pieces::timed_words`] gives
//! each Roman word the timing of the original words it was made from.
//!
//! - [`korean`] writes Korean in Revised Romanization, with the sound changes between syllables.
//! - [`chinese`] writes Mandarin pinyin, phrase-aware, Simplified and Traditional.
//! - [`japanese`] writes Hepburn romaji: kana always, kanji once the dictionary is downloaded.
//! - [`cyrillic`] writes Russian, Ukrainian, Bulgarian and Serbian.
//! - [`greek`] writes Greek.
//!
//! Nothing here reads the network except [`japanese::dictionary`], and only when asked.

pub mod chinese;
pub mod korean;
pub mod pieces;

/// One word of a line, when its time is known.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Word {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Where a second line came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondSource {
    /// The lyrics carried it (the provider's id).
    Lyrics(String),
    /// Paired by time from another provider's copy of the song.
    Paired(String),
    /// Made by Venuta, on the device.
    Made,
    /// Translated by an engine (its name).
    Translated(String),
    /// Corrected by hand.
    Corrected,
}

/// A line under the sung one: its pronunciation, or its meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Second {
    pub text: String,
    /// Pronunciation only: timed by the original's words.
    pub words: Vec<Word>,
    /// BCP 47: "ko-Latn" for pronunciation, "en" for a translation.
    pub lang: String,
    pub source: SecondSource,
}
