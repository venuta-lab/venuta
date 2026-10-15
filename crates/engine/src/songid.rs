//! Telling songs apart, and telling the same song apart from its spellings. One loose spelling
//! shared with the app's search box (`app/src/lib/textsearch.ts` `searchKey`), so a name matches
//! the same way when you search for it and when a playlist from elsewhere brings it in:
//! "Kesariyaa" is "Kesariya", "Arijeet" is "Arijit", "Khwaab" is "Kwab". Both sides are tested
//! against one table (`app/tests/fixtures/spelling-keys.json`).
//!
//! Names in Indian scripts are read in Roman letters by the app only (not here yet): tags and
//! playlists from services come in Roman letters.

use crate::audit::name_key;
use crate::plan::split_names;
use crate::playlists::{Foreign, title_fold_key, title_key};
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

/// How sure a song found somewhere else is the one asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sureness {
    /// Worth showing, for you to check.
    Likely,
    /// The same song.
    Sure,
}

/// Is `have` (a song as another service lists it) the song `want` describes, and how sure?
///
/// The title has to agree (as written or in the loose spelling; version words like Remix or
/// Lofi Flip must agree too), and a length known on both sides within 3 s. A shared artist, or
/// the same recording code, makes it sure. Without a shared artist an agreeing length still
/// makes it likely: services credit Indian film songs to the composer, not the singer.
pub fn same_song(want: &Foreign, have: &Foreign) -> Option<(Sureness, &'static str)> {
    let isrc = |f: &Foreign| f.isrc.as_ref().map(|i| i.trim().to_uppercase());
    if isrc(want).is_some() && isrc(want) == isrc(have) {
        return Some((Sureness::Sure, "Same recording code (ISRC)"));
    }
    let titles = title_key(&want.title) == title_key(&have.title)
        || title_fold_key(&want.title) == title_fold_key(&have.title);
    if !titles || title_key(&want.title).is_empty() {
        return None;
    }
    let near = match (want.duration_ms, have.duration_ms) {
        (Some(a), Some(b)) if a.abs_diff(b) <= 3_000 => Some(true),
        (Some(_), Some(_)) => Some(false),
        _ => None,
    };
    let keys = |s: &str| -> Vec<String> {
        split_names(s)
            .iter()
            .flat_map(|n| [name_key(n), format!("~{}", fold_key(n))])
            .filter(|k| k.len() > 1)
            .collect()
    };
    let theirs = keys(&have.artist);
    let shared = keys(&want.artist).iter().any(|k| theirs.contains(k));
    match (near, shared) {
        (Some(false), _) => None,
        (Some(true), true) => Some((Sureness::Sure, "Same song, artist and length")),
        (Some(true), false) => Some((
            Sureness::Likely,
            "Same song and length, credited differently",
        )),
        (None, true) => Some((Sureness::Likely, "Same song and artist")),
        (None, false) => None,
    }
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

    fn song(title: &str, artist: &str, ms: Option<u64>) -> Foreign {
        Foreign {
            title: title.into(),
            artist: artist.into(),
            duration_ms: ms,
            ..Default::default()
        }
    }

    #[test]
    fn the_same_song_elsewhere() {
        let want = song("Kesariya", "Arijit Singh", Some(268_000));
        // As Apple lists it: every name credited, the film note in the title.
        let apple = song(
            "Kesariya (From \"Brahmastra\")",
            "Pritam, Arijit Singh & Amitabh Bhattacharya",
            Some(268_165),
        );
        assert_eq!(same_song(&want, &apple).map(|x| x.0), Some(Sureness::Sure));
        // As Deezer's search lists it: the composer only.
        let deezer = song("Kesariya (From \"Brahmastra\")", "Pritam", Some(268_000));
        assert_eq!(
            same_song(&want, &deezer).map(|x| x.0),
            Some(Sureness::Likely)
        );
        // Another version: not this song.
        let lofi = song(
            "Kesariya (Lofi Flip)",
            "VIBIE, Arijit Singh & Pritam",
            Some(240_158),
        );
        assert_eq!(same_song(&want, &lofi), None);
        // A different length: not this song.
        let edit = song("Kesariya", "Arijit Singh", Some(197_000));
        assert_eq!(same_song(&want, &edit), None);
        // Spelled differently, same artist and length.
        let spelled = song("Kesariyaa", "Arijeet Singh", Some(268_500));
        assert_eq!(
            same_song(&want, &spelled).map(|x| x.0),
            Some(Sureness::Sure)
        );
    }

    #[test]
    fn keys_join_the_words() {
        assert_eq!(fold_key("Kesariyaa"), fold_key("Kesariya"));
        assert_eq!(fold_key("Arijeet Singh"), "arijitsing");
    }
}
