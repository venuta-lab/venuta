//! Venuta's own spellings of a few very common lyric words, where the house style (the way
//! Apple Music and most lyric sites write them) differs from what the rules or the Dakshina
//! table give. Kept short: the rules and the table carry everything else.

use super::Lang;

/// The house spelling of `word` (NFC), if it has one.
pub fn house(lang: Lang, word: &str) -> Option<&'static str> {
    match lang {
        Lang::Hi => hindi(word),
        // Film-song spellings Dakshina's few annotators did not use.
        Lang::Ta => match word {
            "காதல்" => Some("kaadhal"),
            _ => None,
        },
        Lang::Ml => match word {
            "എന്റെ" => Some("ente"),
            "നിന്റെ" => Some("ninte"),
            _ => None,
        },
        _ => None,
    }
}

// Keys are as the word lists keep them: a candrabindu written as an anusvara.
fn hindi(word: &str) -> Option<&'static str> {
    Some(match word {
        "हां" => "haan",
        "वो" | "वह" => "woh",
        "वादा" => "waada",
        "वादे" => "waade",
        "वादों" => "waadon",
        "यह" => "yeh",
        _ => return None,
    })
}
