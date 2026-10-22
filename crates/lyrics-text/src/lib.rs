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
pub mod cyrillic;
pub mod greek;
#[cfg(feature = "japanese")]
pub mod japanese;
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

/// What the converters here need to know about a song, besides the line.
#[derive(Default)]
pub struct Context<'a> {
    /// The song's language (BCP 47), when known: it picks the Cyrillic table and reads Han
    /// characters as Japanese rather than Chinese.
    pub lang: Option<&'a str>,
    /// The Japanese romanizer, with its dictionary when it is installed.
    #[cfg(feature = "japanese")]
    pub japanese: Option<&'a japanese::Japanese>,
}

impl<'a> Context<'a> {
    /// For a song in `lang` (BCP 47).
    #[allow(clippy::needless_update)] // Without the Japanese feature `lang` is the only field.
    pub fn of_song(lang: &'a str) -> Self {
        Context {
            lang: Some(lang),
            ..Context::default()
        }
    }
}

/// The pronunciation line made on the device for a line in `script` (ISO 15924, as the line
/// carries it: "Hang", "Hani", "Jpan", "Cyrl", "Grek"), timed by `words`. `None` when no
/// converter here reads that script, or when a Japanese line needs the dictionary and it isn't
/// loaded ([`japanese::needs_dictionary`] says when to fetch it).
pub fn romanize(line: &str, words: &[Word], script: &str, cx: &Context) -> Option<Second> {
    let japanese_song = cx.lang.is_some_and(|l| l == "ja" || l.starts_with("ja-"));
    let (roman, lang) = match script {
        "Hang" | "Kore" => (korean::romanize(line), korean::LANG.to_owned()),
        "Hani" | "Hans" | "Hant" if !japanese_song => {
            (chinese::romanize(line), chinese::LANG.to_owned())
        }
        #[cfg(feature = "japanese")]
        "Jpan" | "Hira" | "Kana" | "Hani" | "Hans" | "Hant" => {
            let fallback = japanese::Japanese::kana_only();
            let j = cx.japanese.unwrap_or(&fallback);
            (j.romanize(line).ok()?, japanese::LANG.to_owned())
        }
        "Cyrl" => {
            let table = cx
                .lang
                .and_then(cyrillic::Lang::from_tag)
                .unwrap_or_else(|| cyrillic::detect(line));
            (cyrillic::romanize(line, table), table.tag().to_owned())
        }
        "Grek" => (greek::romanize(line), greek::LANG.to_owned()),
        _ => return None,
    };
    Some(roman.to_second(line, words, &lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_script_to_its_converter() {
        let cx = Context::default();
        let text = |s: &str, script: &str| romanize(s, &[], script, &cx).map(|s| s.text);
        assert_eq!(text("사랑해", "Hang").as_deref(), Some("saranghae"));
        assert_eq!(text("我爱你", "Hani").as_deref(), Some("wo ai ni"));
        assert_eq!(
            text("Я тебя люблю", "Cyrl").as_deref(),
            Some("Ya tebya lyublyu")
        );
        assert_eq!(text("Σ' αγαπώ", "Grek").as_deref(), Some("S' agapo"));
        assert_eq!(text("hello", "Latn"), None);
        let second = romanize("사랑해", &[], "Hang", &cx).unwrap();
        assert_eq!(second.lang, "ko-Latn");
        assert_eq!(second.source, SecondSource::Made);
    }

    #[cfg(feature = "japanese")]
    #[test]
    fn japanese_lines_and_han_in_a_japanese_song() {
        let ja = Context::of_song("ja");
        let text =
            |s: &str, script: &str, cx: &Context| romanize(s, &[], script, cx).map(|s| s.text);
        assert_eq!(text("さくら", "Jpan", &ja).as_deref(), Some("sakura"));
        // Kanji with no dictionary loaded: nothing yet, not a Chinese reading.
        assert_eq!(text("桜", "Hani", &ja), None);
        assert_eq!(text("桜", "Jpan", &Context::default()), None);
    }

    #[test]
    fn the_song_language_picks_the_cyrillic_table() {
        let uk = Context::of_song("uk");
        let s = romanize("Київ", &[], "Cyrl", &uk).unwrap();
        assert_eq!((s.text.as_str(), s.lang.as_str()), ("Kyiv", "uk-Latn"));
    }
}
