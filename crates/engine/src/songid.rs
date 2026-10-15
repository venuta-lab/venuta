//! Telling songs apart, and telling the same song apart from its spellings. One loose spelling
//! shared with the app's search box (`app/src/lib/textsearch.ts` `searchKey`), so a name matches
//! the same way when you search for it and when a playlist from elsewhere brings it in:
//! "Kesariyaa" is "Kesariya", "Arijeet" is "Arijit", "Khwaab" is "Kwab". Both sides are tested
//! against one table (`app/tests/fixtures/spelling-keys.json`).
//!
//! Names in Indian scripts are read in Roman letters by the app only (not here yet): tags and
//! playlists from services come in Roman letters.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// What a name is searched and matched by: no accents, no case, words split on anything that
/// isn't a letter or digit, each word in one loose spelling, joined by single spaces.
pub fn search_key(text: &str) -> String {
    let plain: String = text
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase();
    plain
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(fold_word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// [`search_key`] without the spaces: the key a title or a name is filed under.
pub fn fold_key(text: &str) -> String {
    search_key(text).replace(' ', "")
}

/// One word in its loose spelling: the same steps, in the same order, as the app's `foldWord`.
fn fold_word(w: &str) -> String {
    let s = w
        .replace("ph", "f")
        .replace("ck", "k")
        .replace('q', "k")
        .replace('x', "ks")
        .replace('w', "v")
        .replace('z', "j");
    // c not before h is k.
    let s = map_pairs(&s, |a, b| (a == 'c' && b != Some('h')).then_some(("k", 1)));
    // An h after a consonant is dropped (bh, dh, kh, th, ch…).
    let s = map_pairs(&s, |a, b| {
        ("bcdfgjklmnprstv".contains(a) && b == Some('h')).then(|| (char_str(a), 2))
    });
    let s = s.replace("aa", "a");
    let s = map_pairs(&s, |a, b| {
        (matches!((a, b), ('e', Some('e')) | ('i', Some('i')))).then_some(("i", 2))
    });
    let s = map_pairs(&s, |a, b| {
        (matches!((a, b), ('o', Some('o')) | ('u', Some('u')))).then_some(("u", 2))
    });
    // iy before a vowel is i (Kesariya / Kesaria).
    let chars: Vec<char> = s.chars().collect();
    let mut t = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == 'i'
            && chars.get(i + 1) == Some(&'y')
            && chars.get(i + 2).is_some_and(|c| "aeiou".contains(*c))
        {
            t.push('i');
            i += 2;
        } else {
            t.push(chars[i]);
            i += 1;
        }
    }
    // A letter repeated is one letter.
    let mut out = String::with_capacity(t.len());
    let mut last = None;
    for c in t.chars() {
        if last != Some(c) {
            out.push(c);
        }
        last = Some(c);
    }
    out
}

fn char_str(c: char) -> &'static str {
    // Only ever called with ASCII consonants.
    const ALL: &str = "bcdfgjklmnprstv";
    let i = ALL.find(c).expect("a consonant");
    &ALL[i..i + 1]
}

/// Walk the word left to right, as a global regular expression does: where `f` matches at a
/// letter (seeing the next one), write its text and skip that many letters.
fn map_pairs(s: &str, f: impl Fn(char, Option<char>) -> Option<(&'static str, usize)>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        match f(chars[i], chars.get(i + 1).copied()) {
            Some((text, used)) => {
                out.push_str(text);
                i += used;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_spelling_table_as_the_search_box() {
        let table = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../app/tests/fixtures/spelling-keys.json"),
        )
        .expect("the shared table");
        let v: serde_json::Value = serde_json::from_str(&table).unwrap();
        for case in v["cases"].as_array().unwrap() {
            let name = case[0].as_str().unwrap();
            let key = case[1].as_str().unwrap();
            assert_eq!(search_key(name), key, "{name:?}");
        }
    }

    #[test]
    fn keys_join_the_words() {
        assert_eq!(fold_key("Kesariyaa"), fold_key("Kesariya"));
        assert_eq!(fold_key("Arijeet Singh"), "arijitsing");
    }
}
