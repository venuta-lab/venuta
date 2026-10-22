//! Which lines of a song to translate, and the engine's output made tidy.

use lyrics_text::script::{Script, line_script};

/// The scripts a language is written in, for the languages the models cover; None when it
/// isn't known here (then any line with letters is translated).
fn scripts_of(lang: &str) -> Option<&'static [Script]> {
    use Script::*;
    let base = lang.split(['-', '_']).next().unwrap_or(lang);
    Some(match base {
        "hi" | "mr" | "ne" | "sa" => &[Deva],
        "bn" | "as" => &[Beng],
        "pa" => &[Guru],
        "gu" => &[Gujr],
        "or" => &[Orya],
        "ta" => &[Taml],
        "te" => &[Telu],
        "kn" => &[Knda],
        "ml" => &[Mlym],
        "si" => &[Sinh],
        "ur" | "ar" | "fa" | "ug" | "ps" => &[Arab],
        "he" | "yi" => &[Hebr],
        "ru" | "uk" | "be" | "bg" | "mk" | "kk" | "mn" => &[Cyrl],
        // Serbian is written in both.
        "sr" => &[Cyrl, Latn],
        "el" => &[Grek],
        "th" => &[Thai],
        "ka" => &[Geor],
        "hy" => &[Armn],
        "ja" => &[Jpan, Hira, Kana, Hani],
        "ko" => &[Hang, Hani],
        "zh" => &[Hani, Bopo],
        "my" => &[Mymr],
        "km" => &[Khmr],
        "lo" => &[Laoo],
        // The languages written in Latin letters that the models cover.
        "af" | "az" | "bs" | "ca" | "cs" | "da" | "de" | "en" | "es" | "et" | "eu" | "fi"
        | "fr" | "gl" | "hbs" | "hr" | "hu" | "id" | "is" | "it" | "lt" | "lv" | "ms" | "nb"
        | "nl" | "nn" | "no" | "pl" | "pt" | "ro" | "sk" | "sl" | "sq" | "sv" | "tl" | "tr"
        | "vi" => &[Latn],
        _ => return None,
    })
}

/// Whether a line of a song in `from` is worth translating: it has letters, in the script of
/// the song's language (an English line in a Hindi song is left alone: the Hindi model would
/// make it up, and the listener reads it already).
pub fn wanted(from: &str, text: &str) -> bool {
    let script = line_script(text);
    if matches!(script, Script::Zyyy | Script::Zinh | Script::Zzzz) {
        return false;
    }
    scripts_of(from).is_none_or(|s| s.contains(&script))
}

/// A word as compared for repeats: its letters, in lower case.
fn bare(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The longest run of one word said again and again in `text` (at least 1).
fn longest_run(text: &str) -> usize {
    let words: Vec<String> = text.split_whitespace().map(bare).collect();
    let mut best = 1;
    let mut run = 1;
    for w in words.windows(2) {
        run = if !w[0].is_empty() && w[0] == w[1] {
            run + 1
        } else {
            1
        };
        best = best.max(run);
    }
    best
}

/// `out`, the engine's translation of `src`, trimmed, and with a word it got stuck repeating
/// cut back to as many times as the original says one (twice at least): small models
/// sometimes loop ("Doraji Doraji Doraji ..." twelve times for a line that sings it three).
/// The run keeps its first words and its last, so the punctuation that ends it stays.
pub fn tidy(src: &str, out: &str) -> String {
    let allowed = longest_run(src).max(2);
    let words: Vec<&str> = out.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let key = bare(words[i]);
        let mut j = i + 1;
        while !key.is_empty() && j < words.len() && bare(words[j]) == key {
            j += 1;
        }
        if j - i > allowed {
            kept.extend(&words[i..i + allowed - 1]);
            kept.push(words[j - 1]);
        } else {
            kept.extend(&words[i..j]);
        }
        i = j;
    }
    kept.join(" ")
}
