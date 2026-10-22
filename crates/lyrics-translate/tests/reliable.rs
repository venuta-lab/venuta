//! The offline engine is offered only for the song languages it translated well in the lyrics
//! tests (docs/research/lyrics-translation-test.md): Spanish and French songs, into English.

use lyrics_translate::catalog::{Catalog, Platform};
use lyrics_translate::reliable::{RELIABLE, offered, why_not};
use lyrics_translate::store::ModelStore;

#[test]
fn only_the_languages_that_passed_are_offered() {
    assert_eq!(RELIABLE, ["es", "fr"]);
    for (from, into) in [
        ("es", "en"),
        ("es-MX", "en-GB"),
        ("fr", "en"),
        ("fr-CA", "en"),
    ] {
        assert!(offered(from, into), "{from} into {into}");
        assert!(why_not(from).is_none());
    }
    // Failed, made things up, or no model: Hindi, Tamil, Japanese, Korean (L5); Punjabi (no
    // model); Italian, Dutch, German and Portuguese (kept the meaning on too few songs).
    for from in [
        "hi", "ta", "ja", "ko", "pa", "it", "nl", "de", "pt", "zh", "ar", "",
    ] {
        assert!(!offered(from, "en"), "{from}");
    }
    // Only into English: the other directions were never tested.
    assert!(!offered("en", "es") && !offered("es", "fr") && !offered("en", "hi"));
}

#[test]
fn the_reason_is_one_plain_sentence_that_points_to_the_key_engines() {
    let why = why_not("hi").unwrap();
    assert_eq!(
        why,
        "Translation on this device got Hindi lyrics wrong too often in our tests, so it isn't offered: choose DeepL, Microsoft, Google or AI with your own key in Settings > Lyrics."
    );
    assert_eq!(why.matches(". ").count(), 0, "one sentence");
    assert!(!why.contains('\u{2014}'), "no em dash");
    assert!(why_not("pa").unwrap().contains("Punjabi"));
    // A language with no name known here is still said plainly.
    assert!(why_not("xx").unwrap().contains("songs in this language"));
}

#[test]
fn the_model_list_says_which_models_are_offered() {
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(
        dir.path(),
        Catalog::bundled().for_platform(Platform::Desktop),
    );
    let models = store.models();
    let get = |id: &str| models.iter().find(|m| m.id == id).unwrap();
    assert!(get("es-en").offered && get("fr-en").offered);
    assert!(!get("hi-en").offered && !get("de-en").offered && !get("en-es").offered);
}
