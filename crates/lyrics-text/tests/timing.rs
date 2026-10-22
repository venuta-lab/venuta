//! The romanized line keeps the original's word timing.

use lyrics_text::{Line, Word, romanize};

fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
    Word {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

fn timed(text: &str, words: &[Word]) -> Vec<(u64, u64, String)> {
    let r = romanize(&Line::new(text).words(words)).expect("romanized");
    let joined: String = r.words.iter().map(|w| w.text.as_str()).collect();
    if !r.words.is_empty() {
        assert_eq!(joined, r.text, "the words put together are the line");
    }
    r.words
        .into_iter()
        .map(|w| (w.start_ms, w.end_ms, w.text))
        .collect()
}

#[test]
fn each_word_keeps_its_time() {
    let line = "जो वादे किये सारे";
    let words = [
        w(0, 400, "जो "),
        w(400, 900, "वादे "),
        w(900, 1300, "किये "),
        w(1300, 2000, "सारे"),
    ];
    assert_eq!(
        timed(line, &words),
        [
            (0, 400, "Jo ".into()),
            (400, 900, "waade ".into()),
            (900, 1300, "kiye ".into()),
            (1300, 2000, "saare".into())
        ]
    );
}

#[test]
fn words_given_without_their_spaces() {
    let words = [w(0, 400, "जो"), w(400, 900, "वादे")];
    assert_eq!(
        timed("जो वादे", &words),
        [(0, 400, "Jo ".into()), (400, 900, "waade".into())]
    );
}

#[test]
fn syllables_timed_one_by_one_keep_their_times() {
    // "बातों" sung as "बा" + "तों": the word is read whole ("baaton", not "baa" + "ton" read
    // apart), then split where the syllables were.
    let words = [w(0, 200, "बा"), w(200, 500, "तों")];
    assert_eq!(
        timed("बातों", &words),
        [(0, 200, "Baa".into()), (200, 500, "ton".into())]
    );
}

#[test]
fn a_word_from_the_word_list_spans_its_syllables() {
    // "में" comes whole from the word list ("mein"): its two timed halves become one word.
    let words = [w(0, 100, "मे"), w(100, 300, "ं"), w(300, 600, " दिल")];
    let got = timed("में दिल", &words);
    assert_eq!(got, [(0, 300, "Mein ".into()), (300, 600, "dil".into())]);
}

#[test]
fn english_words_keep_their_time_too() {
    let words = [w(0, 300, "Baby "), w(300, 600, "तू "), w(600, 900, "मेरी")];
    assert_eq!(
        timed("Baby तू मेरी", &words),
        [
            (0, 300, "Baby ".into()),
            (300, 600, "tu ".into()),
            (600, 900, "meri".into())
        ]
    );
}

#[test]
fn no_word_timing_when_the_words_do_not_match_the_line() {
    let words = [w(0, 300, "something else")];
    assert!(timed("दिल", &words).is_empty());
    assert!(timed("दिल", &[]).is_empty());
}
