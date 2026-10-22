//! Where the offline engine is offered: only for the song languages whose lyrics it translated
//! well in the tests (docs/research/lyrics-translation-test.md), and only into English, the one
//! direction tested. Elsewhere it got the meaning wrong too often, or made lines up (Hindi,
//! Tamil, Japanese, Korean), has no model (Punjabi), or kept the meaning on too few songs
//! (German, Portuguese, Italian, Dutch); the key-based engines are the way there.

use crate::langs;

/// The song languages the offline engine is offered for: Spanish (4 of 4 songs kept their
/// meaning) and French (3 of 3).
pub const RELIABLE: [&str; 2] = ["es", "fr"];

/// Whether the offline engine is offered for a song in `from`, read in `into`.
pub fn offered(from: &str, into: &str) -> bool {
    RELIABLE.contains(&langs::base(from).as_str()) && langs::base(into) == "en"
}

/// Why the offline engine isn't offered for songs in `from`, in one sentence that points to the
/// key-based engines; None when it is offered (into English).
pub fn why_not(from: &str) -> Option<String> {
    if RELIABLE.contains(&langs::base(from).as_str()) {
        return None;
    }
    let songs = match langs::name(from) {
        Some(n) => format!("{n} lyrics"),
        None => "songs in this language".to_owned(),
    };
    Some(format!(
        "Translation on this device got {songs} wrong too often in our tests, so it isn't offered: choose DeepL, Microsoft, Google or AI with your own key in Settings > Lyrics."
    ))
}
