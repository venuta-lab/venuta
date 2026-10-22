//! The real engine on real models, downloaded from Mozilla. Ignored by default (it needs the
//! network once and about 40 MB); run with
//!
//! ```text
//! LYRICS_TRANSLATE_MODELS=<a folder to keep them> cargo test -p lyrics-translate --release \
//!     --test bergamot_real -- --ignored --nocapture
//! ```
//!
//! The lines are public-domain songs from the quality test (docs/research/lyrics-translation-test.md).

use std::path::PathBuf;
use std::time::Instant;

use lyrics_translate::bergamot::Bergamot;
use lyrics_translate::cache::SongCache;
use lyrics_translate::catalog::{Catalog, Platform};
use lyrics_translate::store::{Https, ModelStore};
use lyrics_translate::{Translator, translate_song};

fn models() -> (ModelStore, Option<tempfile::TempDir>) {
    let (dir, temp) = match std::env::var_os("LYRICS_TRANSLATE_MODELS") {
        Some(d) => (PathBuf::from(d), None),
        None => {
            let t = tempfile::tempdir().unwrap();
            (t.path().to_owned(), Some(t))
        }
    };
    let store = ModelStore::new(dir, Catalog::bundled().for_platform(Platform::current()));
    for id in ["es-en", "hi-en"] {
        if store.files(id).is_none() {
            let t = Instant::now();
            store.download(id, &Https, &mut |_, _| {}).unwrap();
            eprintln!("{id}: downloaded and checked in {:?}", t.elapsed());
        }
    }
    (store, temp)
}

const ES: [&str; 4] = [
    "La cucaracha, la cucaracha,",
    "ya no puede caminar",
    "Oíd, mortales, el grito sagrado:",
    "Que morir por la patria es vivir.",
];

#[test]
#[ignore = "downloads two models from Mozilla"]
fn spanish_and_hindi_come_out_in_english() {
    let (store, _keep) = models();
    let b = Bergamot::new(&store, 0);
    let out = b.translate("es", "en", &ES).unwrap();
    eprintln!("{out:#?}");
    assert_eq!(out.len(), 4);
    assert!(out[0].to_lowercase().contains("cockroach"));
    assert!(out[1].to_lowercase().contains("walk"));
    assert!(out[3].to_lowercase().contains("die"));

    let dir = tempfile::tempdir().unwrap();
    let cache = SongCache::new(dir.path());
    let hi = [
        "हम बुलबुलें हैं इसकी यह गुलसिताँ हमारा",
        "Hello",
        "हम बुलबुलें हैं इसकी यह गुलसिताँ हमारा",
    ];
    let e = translate_song(&b, &cache, "hi", "en", &hi).unwrap();
    eprintln!("{e:#?}");
    assert_eq!(e.engine, "bergamot");
    assert!(e.lines[0].is_some() && e.lines[1].is_none() && e.lines[0] == e.lines[2]);
}

#[test]
#[ignore = "downloads two models from Mozilla"]
fn a_whole_song_is_quick() {
    let (store, _keep) = models();
    let mut song: Vec<String> = Vec::new();
    while song.len() < 40 {
        for l in ES {
            song.push(format!("{l} {}", song.len()));
        }
    }
    let lines: Vec<&str> = song.iter().map(String::as_str).collect();
    for threads in [1, 4, 0] {
        let b = Bergamot::new(&store, threads);
        let t = Instant::now();
        let out = b.translate("es", "en", &lines).unwrap();
        let took = t.elapsed();
        assert_eq!(out.len(), 40);
        eprintln!("40 lines, {threads} threads (0: all cores), model load included: {took:?}");
    }
}

/// The lyrics test of the languages the engine might be offered for (docs/research/
/// lyrics-translation-test.md, "Western European languages"): every song of the corpus at
/// `LYRICS_CORPUS` (`{"songs": [{"id", "lang", "lines": [...]}]}`) translated into English,
/// written as JSON to `LYRICS_CORPUS_OUT`. The verdicts are a reader's, in the doc.
#[test]
#[ignore = "downloads five models from Mozilla and needs the corpus"]
fn western_european_lyrics() {
    let (Some(corpus), Some(out)) = (
        std::env::var_os("LYRICS_CORPUS"),
        std::env::var_os("LYRICS_CORPUS_OUT"),
    ) else {
        eprintln!("set LYRICS_CORPUS and LYRICS_CORPUS_OUT");
        return;
    };
    let (store, _keep) = models();
    let corpus: serde_json::Value =
        serde_json::from_slice(&std::fs::read(corpus).unwrap()).unwrap();
    let b = Bergamot::new(&store, 0);
    let mut answers = serde_json::Map::new();
    for song in corpus["songs"].as_array().unwrap() {
        let lang = song["lang"].as_str().unwrap();
        let id = format!("{lang}-en");
        if store.files(&id).is_none() {
            store.download(&id, &Https, &mut |_, _| {}).unwrap();
        }
        let lines: Vec<&str> = song["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap())
            .collect();
        let got = b.translate(lang, "en", &lines).unwrap();
        assert_eq!(got.len(), lines.len());
        answers.insert(song["id"].as_str().unwrap().to_owned(), got.into());
    }
    std::fs::write(out, serde_json::to_vec_pretty(&answers).unwrap()).unwrap();
}
