//! The bundled model list: every model complete and pinned, one per pair on each platform.

use lyrics_translate::catalog::{Catalog, FileKind, Platform};

#[test]
fn every_model_is_complete_and_pinned() {
    let c = Catalog::bundled();
    assert!(c.models().len() > 100, "{}", c.models().len());
    for m in c.models() {
        let kinds: Vec<FileKind> = m.files.iter().map(|f| f.kind).collect();
        assert!(kinds.contains(&FileKind::Model), "{}", m.id);
        assert!(kinds.contains(&FileKind::Lex), "{}", m.id);
        assert!(
            kinds.contains(&FileKind::Vocab)
                || (kinds.contains(&FileKind::SrcVocab) && kinds.contains(&FileKind::TrgVocab)),
            "{}",
            m.id
        );
        for f in &m.files {
            assert_eq!(f.sha256.len(), 64, "{} {}", m.id, f.name);
            assert!(f.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(
                f.url.starts_with(
                    "https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/"
                ),
                "{}",
                f.url
            );
            assert!(f.download_bytes > 0 && f.bytes > 0);
        }
        assert_eq!(m.id, format!("{}-{}", m.from, m.to));
    }
}

#[test]
fn one_model_per_pair_on_each_platform() {
    for p in [Platform::Desktop, Platform::Android] {
        let c = Catalog::bundled().for_platform(p);
        let mut ids: Vec<&str> = c.models().iter().map(|m| m.id.as_str()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "{p:?}");
    }
}

#[test]
fn the_tested_languages_are_there_and_punjabi_is_not() {
    let desktop = Catalog::bundled().for_platform(Platform::Desktop);
    for id in ["hi-en", "ta-en", "es-en", "ja-en", "ko-en", "en-hi"] {
        assert!(desktop.get(id).is_some(), "{id}");
    }
    assert!(desktop.get("pa-en").is_none());
    assert_eq!(desktop.get("hi-en").unwrap().name(), "Hindi to English");
    // Desktop gets the larger Japanese model, phones the lighter one.
    assert_eq!(desktop.get("ja-en").unwrap().arch, "base");
    let android = Catalog::bundled().for_platform(Platform::Android);
    assert_eq!(android.get("ja-en").unwrap().arch, "base-memory");
}

#[test]
fn a_song_language_finds_its_model() {
    let c = Catalog::bundled().for_platform(Platform::Desktop);
    assert_eq!(c.find("hi", "en").unwrap().id, "hi-en");
    // Region and script subtags of the song are not the model's business.
    assert_eq!(c.find("hi-IN", "en-GB").unwrap().id, "hi-en");
    assert_eq!(c.find("zh", "en").unwrap().id, "zh-Hans-en");
    assert_eq!(c.find("zh-Hant", "en").unwrap().id, "zh-Hant-en");
    assert!(c.find("pa", "en").is_none());
    assert!(c.find("en", "en").is_none());
}
