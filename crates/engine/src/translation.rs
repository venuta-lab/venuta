//! Lyrics translated on this device (`lyrics_translate`), as the app (and later the phone)
//! offers it: the models listed, downloaded only when the listener asks, removed when asked; a
//! song translated when the listener has Translation on, its model is here, and the lyrics
//! carry nothing in their language. Nothing is sent anywhere: the lines are translated here,
//! and the only network use is a model's download, from Mozilla.
//!
//! The views ask for a song by its lyrics key (`file:<path>`, `online:<provider>:<id>`), so
//! the lines last served for each key are remembered here ([`Translations::serve`]): a
//! translation is made for exactly the lines the listener sees. Every lyrics window asks for
//! the song it shows; asks for the same song at the same time share one translation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use lyrics_translate::Translator;
use lyrics_translate::bergamot::Bergamot;
use lyrics_translate::cache::SongCache;
use lyrics_translate::catalog::{Catalog, Platform};
use lyrics_translate::store::{Fetch, Https, ModelStatus, ModelStore};
use lyrics_translate::{Shared, translate_song};

use crate::lyrics::LyricLine;

/// Songs served lately, at most: a window shows one song, and there are a few windows.
const SERVED_MAX: usize = 64;

pub struct Translations {
    store: ModelStore,
    cache: SongCache,
    /// Lyrics key → the sung lines last served for it, and when (to forget the oldest).
    served: Mutex<HashMap<String, (Vec<String>, u64)>>,
    running: Shared<Result<(), String>>,
    threads: usize,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Translations {
    /// Models under `models`, translations kept under `kept`.
    pub fn new(models: PathBuf, kept: PathBuf) -> Self {
        // Half the cores at most: the player keeps the rest.
        let threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).clamp(1, 8))
            .unwrap_or(1);
        Translations {
            store: ModelStore::new(models, Catalog::bundled().for_platform(Platform::current())),
            cache: SongCache::new(kept),
            served: Mutex::new(HashMap::new()),
            running: Shared::default(),
            threads,
        }
    }

    /// The one the app uses, under the app data folder.
    pub fn global() -> &'static Translations {
        static IT: OnceLock<Translations> = OnceLock::new();
        IT.get_or_init(|| {
            let data = crate::paths::app_data_dir();
            Translations::new(data.join("translation-models"), data.join("translations"))
        })
    }

    /// A song's lines as they go to a view: remembered for its key (so it can be translated),
    /// and given the translation made here for them, if any.
    pub fn serve(&self, key: &str, lines: &mut [LyricLine]) {
        if lines.is_empty() {
            return;
        }
        crate::lyrics::add_translation_from(&self.cache, lines);
        if key.is_empty() {
            return;
        }
        let mut served = lock(&self.served);
        let n = served.len() as u64;
        let stamp = served.values().map(|(_, at)| *at).max().unwrap_or(0) + 1;
        served.insert(
            key.to_owned(),
            (lines.iter().map(|l| l.text.clone()).collect(), stamp),
        );
        if n >= SERVED_MAX as u64 {
            let oldest = served
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                served.remove(&k);
            }
        }
    }

    /// Translates the song last served for `key` from `from` into `into` with the model on
    /// this device, and keeps it: the next read of the song's lyrics carries it. Several asks
    /// for the same song at once make one translation.
    pub fn translate(&self, key: &str, from: &str, into: &str) -> Result<(), String> {
        self.translate_with(&Bergamot::new(&self.store, self.threads), key, from, into)
    }

    /// [`translate`](Self::translate) with another engine (tests, and the key-based engines).
    pub fn translate_with(
        &self,
        engine: &dyn Translator,
        key: &str,
        from: &str,
        into: &str,
    ) -> Result<(), String> {
        let lines = lock(&self.served)
            .get(key)
            .map(|(l, _)| l.clone())
            .ok_or_else(|| "the song's lyrics aren't open".to_owned())?;
        let ask = format!("{key}\n{from}\n{into}\n{}", engine.engine());
        self.running.run(&ask, || {
            let texts: Vec<&str> = lines.iter().map(String::as_str).collect();
            translate_song(engine, &self.cache, from, into, &texts)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    /// Every model there is, with what is here and what is downloading.
    pub fn models(&self) -> Vec<ModelStatus> {
        self.store.models()
    }

    /// Starts downloading a model (on its own thread) and returns at once; its progress shows
    /// in [`models`](Self::models). Only when the listener asks, and never in Local only.
    pub fn download(&'static self, id: &str) -> Result<(), String> {
        self.download_with(id, &Https)
    }

    fn download_with(&'static self, id: &str, fetch: &'static dyn Fetch) -> Result<(), String> {
        if crate::net::is_offline() {
            return Err("Local only is on".into());
        }
        if !self.store.reserve(id).map_err(|e| e.to_string())? {
            return Ok(());
        }
        let id = id.to_owned();
        std::thread::Builder::new()
            .name("translation-model".into())
            .spawn(move || {
                // A failure is kept with the model (its `error`) for the list to show.
                if let Err(e) = self.store.download(&id, fetch, &mut |_, _| {}) {
                    tracing::warn!("translation model {id}: {e}");
                }
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Takes a model off this device (stopping its download).
    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.store.remove(id).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lyrics_translate::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Upper(AtomicUsize);
    impl Translator for Upper {
        fn engine(&self) -> &str {
            "upper"
        }
        fn version(&self, from: &str, _into: &str) -> Option<String> {
            (from == "ru").then(|| "1".into())
        }
        fn translate(&self, _: &str, _: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(lines.iter().map(|l| l.to_uppercase()).collect())
        }
    }

    fn song() -> Vec<LyricLine> {
        let mut lines: Vec<LyricLine> = ["привет", "мир"]
            .iter()
            .enumerate()
            .map(|(i, t)| LyricLine {
                start_ms: i as u64 * 1000,
                text: (*t).into(),
                ..Default::default()
            })
            .collect();
        crate::lyrics::finish(&mut lines);
        lines
    }

    #[test]
    fn a_served_song_is_translated_and_read_again_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let t = Translations::new(dir.path().join("m"), dir.path().join("k"));
        let engine = Upper(AtomicUsize::new(0));
        assert!(
            t.translate_with(&engine, "file:a.flac", "ru", "en")
                .is_err()
        );
        let mut lines = song();
        t.serve("file:a.flac", &mut lines);
        assert!(lines[0].translation.is_none());
        t.translate_with(&engine, "file:a.flac", "ru", "en")
            .unwrap();
        let mut again = song();
        t.serve("file:a.flac", &mut again);
        let tr = again[0].translation.as_ref().unwrap();
        assert_eq!(tr.text, "ПРИВЕТ");
        assert_eq!(
            tr.source,
            crate::lyrics::SecondSource::Translated("upper".into())
        );
        assert_eq!(tr.lang, "en");
        // Asked again: kept, not translated again.
        t.translate_with(&engine, "file:a.flac", "ru", "en")
            .unwrap();
        assert_eq!(engine.0.load(Ordering::SeqCst), 1);
        // A pair no model does is an answer, not a hang.
        assert!(
            t.translate_with(&engine, "file:a.flac", "pa", "en")
                .is_err()
        );
    }

    #[test]
    fn the_models_are_listed_and_none_is_here_at_first() {
        let dir = tempfile::tempdir().unwrap();
        let t = Translations::new(dir.path().join("m"), dir.path().join("k"));
        let models = t.models();
        let hi = models.iter().find(|m| m.id == "hi-en").unwrap();
        assert_eq!(hi.name, "Hindi to English");
        assert!(!hi.installed && hi.progress.is_none() && hi.size_bytes > 10_000_000);
        assert!(!dir.path().join("m").exists());
    }
}
