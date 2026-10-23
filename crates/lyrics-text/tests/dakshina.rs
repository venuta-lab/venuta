//! Accuracy on Google's Dakshina hand-romanized sentences (CC BY-SA 4.0; the fixtures and their
//! credit are in `tests/fixtures/dakshina`). A word counts as right when it is spelled the way
//! an annotator spelled it: in that sentence, in another sentence, or in the lexicons (people
//! spell words several ways, and any of them is how people spell it).
//!
//! The fixtures hold the test split, which the rules were never tuned on (they were tuned on
//! the dev split and the train lexicons, with `cargo run -p lyrics-text --example dakshina --`
//! `misses`, `score dev` and `rules`).

use lyrics_text::indic::{Lang, word_to_latin};

struct Score {
    words: u32,
    /// Spelled as some annotator spelled the word.
    any: u32,
    /// Spelled as the annotator of that very sentence did.
    same: u32,
}

fn score(lang: Lang) -> Score {
    let path = format!(
        "{}/tests/fixtures/dakshina/{}.test.tsv",
        env!("CARGO_MANIFEST_DIR"),
        lang.code()
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut s = Score {
        words: 0,
        any: 0,
        same: 0,
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split('\t');
        let (Some(word), Some(count), Some(seen)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let lexicon = f.next().unwrap_or("");
        let count: u32 = count.parse().expect("count");
        let got = word_to_latin(word, lang, true);
        let mut known = false;
        for pair in seen.split(' ') {
            let (r, c) = pair.rsplit_once(':').expect("spelling:count");
            if r == got {
                s.same += c.parse::<u32>().expect("count");
                known = true;
            }
        }
        known |= lexicon.split(' ').any(|r| r == got);
        s.words += count;
        if known {
            s.any += count;
        }
    }
    s
}

fn report(lang: Lang) -> f64 {
    let s = score(lang);
    let pct = |n: u32| 100.0 * n as f64 / s.words.max(1) as f64;
    println!(
        "{}: {} words, {:.2}% as some annotator spells them, {:.2}% as that sentence's annotator did",
        lang.code(),
        s.words,
        pct(s.any),
        pct(s.same)
    );
    pct(s.any)
}

#[test]
fn hindi_words_are_spelled_as_people_spell_them() {
    let pct = report(Lang::Hi);
    assert!(pct >= 95.6, "Hindi: {pct:.2}%");
}

#[test]
fn the_other_languages_dakshina_covers() {
    // Reported, and held at what they reach today so a change that makes one worse is seen.
    for (lang, floor) in [
        (Lang::Mr, 88.8),
        (Lang::Bn, 87.5),
        (Lang::Gu, 86.2),
        (Lang::Pa, 91.7),
        (Lang::Ta, 79.8),
        (Lang::Te, 90.9),
        (Lang::Kn, 90.6),
        (Lang::Ml, 72.1),
    ] {
        let pct = report(lang);
        assert!(
            pct >= floor,
            "{}: {pct:.2}% (was at least {floor}%)",
            lang.code()
        );
    }
}
