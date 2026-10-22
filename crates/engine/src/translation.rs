//! Lyrics translated (`lyrics_translate`), as the app (and later the phone) offers it:
//!
//! - **On this device**: the models listed, downloaded only when the listener asks, removed
//!   when asked; a song translated when the listener has Translation on, its model is here, and
//!   the lyrics carry nothing in their language. Nothing is sent anywhere. Offered only for the
//!   song languages it translated well in the tests ([`lyrics_translate::reliable`]).
//! - **With the listener's own key** (DeepL, Microsoft, Google, Claude): the key kept in the
//!   system's secret store ([`Vault`]), never in a file or a log; a song sent only when the
//!   listener asks (Translate, or translating automatically with their key, which they turn
//!   on), never in Local only, and only with a key they entered.
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
use lyrics_translate::keyed::{self, Options};
pub use lyrics_translate::keyed::{Credentials, Keyed};
use lyrics_translate::reliable;
use lyrics_translate::store::{Fetch, Https, ModelStatus, ModelStore};
use lyrics_translate::{Shared, translate_song};
use serde::Serialize;

use crate::accounts::Vault;
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
    /// for the same song at once make one translation. Refused, in one sentence, for a song
    /// language it isn't offered for.
    pub fn translate(&self, key: &str, from: &str, into: &str) -> Result<(), String> {
        if !reliable::offered(from, into) {
            return Err(not_offered(from));
        }
        self.translate_with(&Bergamot::new(&self.store, self.threads), key, from, into)
    }

    /// Translates the song last served for `key` with an engine and the listener's own key,
    /// kept in `vault`: only when the listener asked, never in Local only, never without a key
    /// they entered. `model` is the language model's id (Claude; None: its default). Kept like
    /// any translation, so asking again sends nothing.
    pub fn translate_keyed(
        &self,
        vault: &dyn Vault,
        ask: Ask,
        key: &str,
        from: &str,
        into: &str,
    ) -> Result<(), String> {
        self.translate_keyed_at(vault, ask, None, crate::net::is_offline(), key, from, into)
    }

    #[allow(clippy::too_many_arguments)]
    fn translate_keyed_at(
        &self,
        vault: &dyn Vault,
        ask: Ask,
        endpoint: Option<String>,
        offline: bool,
        key: &str,
        from: &str,
        into: &str,
    ) -> Result<(), String> {
        if offline {
            return Err(local_only(ask.engine));
        }
        let creds = load_key(vault, ask.engine)?.ok_or_else(|| no_key(ask.engine))?;
        let engine = keyed::engine(
            ask.engine,
            creds,
            Options {
                model: ask.model,
                endpoint,
            },
        );
        self.translate_with(&*engine, key, from, into)
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
        self.offered(id)?;
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

    /// Whether a model is offered for download (only for the song languages the engine
    /// translated well); the reason when it isn't.
    fn offered(&self, id: &str) -> Result<(), String> {
        match self.store.catalog().get(id) {
            Some(m) if !reliable::offered(&m.from, &m.to) => Err(not_offered(&m.from)),
            _ => Ok(()),
        }
    }
}

/// A translation asked of an engine with the listener's own key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub engine: Keyed,
    /// The language model's id (Claude); None: its default.
    pub model: Option<String>,
}

/// Why the offline engine isn't offered for songs in `from`, for the listener.
fn not_offered(from: &str) -> String {
    reliable::why_not(from).unwrap_or_else(|| {
        "Translation on this device is only offered into English: choose DeepL, Microsoft, Google or AI with your own key in Settings > Lyrics.".to_owned()
    })
}

fn local_only(which: Keyed) -> String {
    format!("Local only is on, so nothing is sent to {}.", which.name())
}

fn no_key(which: Keyed) -> String {
    lyrics_translate::Error::NoKey {
        engine: which.name().into(),
    }
    .to_string()
}

// --- Keys ------------------------------------------------------------------------------------

/// Where an engine's key is kept in the secret store.
fn vault_key(which: Keyed) -> String {
    format!("translation-key:{}", which.id())
}

/// Keeps the key (and region) the listener entered for `which`, in the system's secret store.
pub fn save_key(vault: &dyn Vault, which: Keyed, creds: &Credentials) -> Result<(), String> {
    let creds = Credentials {
        key: creds.key.trim().to_owned(),
        region: creds
            .region
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned),
    };
    if creds.key.is_empty() {
        return Err(format!("Enter a {} key first.", which.name()));
    }
    let text = serde_json::to_string(&creds).map_err(|e| e.to_string())?;
    vault
        .put(&vault_key(which), &text)
        .map_err(|_| "Couldn't keep the key in the system's secret store.".to_owned())
}

/// The key kept for `which`, if any.
pub fn load_key(vault: &dyn Vault, which: Keyed) -> Result<Option<Credentials>, String> {
    let Some(text) = vault
        .get(&vault_key(which))
        .map_err(|_| "Couldn't read the key from the system's secret store.".to_owned())?
    else {
        return Ok(None);
    };
    // Unreadable: as good as none.
    Ok(serde_json::from_str::<Credentials>(&text)
        .ok()
        .filter(|c| !c.key.trim().is_empty()))
}

/// Forgets the key kept for `which`.
pub fn forget_key(vault: &dyn Vault, which: Keyed) -> Result<(), String> {
    vault
        .delete(&vault_key(which))
        .map_err(|_| "Couldn't forget the key in the system's secret store.".to_owned())
}

/// An engine's key as the settings show it: whether one is kept, never the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyState {
    pub engine: String,
    pub name: String,
    pub kept: bool,
    /// Microsoft: the region kept with the key.
    pub region: Option<String>,
    /// DeepL: "Free" or "Pro", from the key.
    pub plan: Option<String>,
}

/// Every engine, with whether a key is kept for it.
pub fn keys(vault: &dyn Vault) -> Vec<KeyState> {
    Keyed::ALL
        .into_iter()
        .map(|which| {
            let kept = load_key(vault, which).ok().flatten();
            KeyState {
                engine: which.id().to_owned(),
                name: which.name().to_owned(),
                plan: kept
                    .as_ref()
                    .filter(|_| which == Keyed::DeepL)
                    .map(|c| keyed::deepl::plan(&c.key).to_owned()),
                region: kept.as_ref().and_then(|c| c.region.clone()),
                kept: kept.is_some(),
            }
        })
        .collect()
}

/// Tries a key with one short line (Settings, Test): the line, and what came back. `creds`
/// is the key as typed, or None for the one kept. Never in Local only.
pub fn test_key(
    vault: &dyn Vault,
    ask: Ask,
    creds: Option<Credentials>,
    into: &str,
) -> Result<(String, String), String> {
    test_key_at(vault, ask, creds, None, crate::net::is_offline(), into)
}

fn test_key_at(
    vault: &dyn Vault,
    ask: Ask,
    creds: Option<Credentials>,
    endpoint: Option<String>,
    offline: bool,
    into: &str,
) -> Result<(String, String), String> {
    if offline {
        return Err(local_only(ask.engine));
    }
    let creds = match creds.filter(|c| !c.key.trim().is_empty()) {
        Some(c) => c,
        None => load_key(vault, ask.engine)?.ok_or_else(|| no_key(ask.engine))?,
    };
    let (from, line) = keyed::test_line(into);
    let engine = keyed::engine(
        ask.engine,
        creds,
        Options {
            model: ask.model,
            endpoint,
        },
    );
    let out = engine
        .translate(from, into, &[line])
        .map_err(|e| e.to_string())?;
    Ok((line.to_owned(), out.into_iter().next().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lyrics_translate::Error;
    use std::sync::Arc;
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

    /// The system's secret store, in memory.
    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);
    impl Vault for Memory {
        fn put(&self, k: &str, v: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn get(&self, k: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn delete(&self, k: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }

    /// A local stand-in for DeepL: each request answered with "EN <line>", and counted.
    fn deepl_here() -> (String, Arc<AtomicUsize>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut len = 0;
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    let h = h.trim_end().to_ascii_lowercase();
                    if h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if let Some(v) = h.strip_prefix("authorization:") {
                        auth = v.trim().to_owned();
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                seen.fetch_add(1, Ordering::SeqCst);
                let asked: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let (status, answer) = if auth == "deepl-auth-key good:fx" {
                    let t: Vec<serde_json::Value> = asked["text"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|l| serde_json::json!({"text": format!("EN {}", l.as_str().unwrap())}))
                        .collect();
                    (200, serde_json::json!({ "translations": t }).to_string())
                } else {
                    (403, "{}".to_owned())
                };
                let mut out = stream;
                let _ = write!(
                    out,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (url, count)
    }

    fn deepl() -> Ask {
        Ask {
            engine: Keyed::DeepL,
            model: None,
        }
    }

    #[test]
    fn keys_are_kept_in_the_secret_store_and_never_shown() {
        let v = Memory::default();
        assert!(keys(&v).iter().all(|k| !k.kept));
        assert!(save_key(&v, Keyed::DeepL, &Credentials::new("  ")).is_err());
        save_key(&v, Keyed::DeepL, &Credentials::new(" secret:fx ")).unwrap();
        save_key(
            &v,
            Keyed::Microsoft,
            &Credentials::new("ms").with_region(" westeurope "),
        )
        .unwrap();
        // In the vault, under the engine's name, trimmed.
        let kept =
            v.0.lock()
                .unwrap()
                .get("translation-key:deepl")
                .cloned()
                .unwrap();
        assert!(kept.contains("\"secret:fx\""));
        assert_eq!(
            load_key(&v, Keyed::DeepL).unwrap().unwrap().key,
            "secret:fx"
        );
        // What the settings get: whether one is kept, the plan and region, never the key.
        let ks = keys(&v);
        let shown = serde_json::to_string(&ks).unwrap();
        assert!(!shown.contains("secret"), "{shown}");
        let d = ks.iter().find(|k| k.engine == "deepl").unwrap();
        assert!(d.kept && d.plan.as_deref() == Some("Free"));
        let m = ks.iter().find(|k| k.engine == "microsoft").unwrap();
        assert_eq!(m.region.as_deref(), Some("westeurope"));
        assert!(!ks.iter().find(|k| k.engine == "llm").unwrap().kept);
        forget_key(&v, Keyed::DeepL).unwrap();
        assert!(load_key(&v, Keyed::DeepL).unwrap().is_none());
    }

    #[test]
    fn a_song_is_sent_only_with_a_key_and_never_in_local_only() {
        let dir = tempfile::tempdir().unwrap();
        let t = Translations::new(dir.path().join("m"), dir.path().join("k"));
        let (url, count) = deepl_here();
        let mut lines = song();
        t.serve("file:a.flac", &mut lines);
        let v = Memory::default();
        let run = |offline| {
            t.translate_keyed_at(
                &v,
                deepl(),
                Some(url.clone()),
                offline,
                "file:a.flac",
                "ru",
                "en",
            )
        };
        assert_eq!(
            run(false).unwrap_err(),
            "Enter your DeepL key in Settings > Lyrics first."
        );
        save_key(&v, Keyed::DeepL, &Credentials::new("good:fx")).unwrap();
        assert_eq!(
            run(true).unwrap_err(),
            "Local only is on, so nothing is sent to DeepL."
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        run(false).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut again = song();
        t.serve("file:a.flac", &mut again);
        let tr = again[0].translation.as_ref().unwrap();
        assert_eq!(tr.text, "EN привет");
        assert_eq!(
            tr.source,
            crate::lyrics::SecondSource::Translated("deepl".into())
        );
        // Asked again: kept, nothing sent (the listener's quota isn't spent twice).
        run(false).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // A key the service refuses is said plainly.
        save_key(&v, Keyed::DeepL, &Credentials::new("bad")).unwrap();
        let mut other = song();
        other[1].text = "мир!".into();
        t.serve("file:b.flac", &mut other);
        let e = t
            .translate_keyed_at(
                &v,
                deepl(),
                Some(url.clone()),
                false,
                "file:b.flac",
                "ru",
                "en",
            )
            .unwrap_err();
        assert_eq!(
            e,
            "DeepL didn't accept your key. Check it in Settings > Lyrics."
        );
    }

    #[test]
    fn a_key_is_tried_with_one_short_line() {
        let (url, count) = deepl_here();
        let v = Memory::default();
        let (line, back) = test_key_at(
            &v,
            deepl(),
            Some(Credentials::new("good:fx")),
            Some(url.clone()),
            false,
            "en",
        )
        .unwrap();
        assert_eq!(
            (line.as_str(), back.as_str()),
            ("Buenos días", "EN Buenos días")
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Nothing typed and nothing kept: nothing sent.
        assert!(test_key_at(&v, deepl(), None, Some(url.clone()), false, "en").is_err());
        assert!(
            test_key_at(
                &v,
                deepl(),
                Some(Credentials::new("good:fx")),
                Some(url),
                true,
                "en"
            )
            .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_offline_engine_is_refused_where_it_proved_unreliable() {
        let dir = tempfile::tempdir().unwrap();
        let t = Translations::new(dir.path().join("m"), dir.path().join("k"));
        let mut lines = song();
        t.serve("file:a.flac", &mut lines);
        let e = t.translate("file:a.flac", "hi", "en").unwrap_err();
        assert!(e.contains("Hindi lyrics wrong too often"), "{e}");
        assert!(t.translate("file:a.flac", "en", "hi").is_err());
        assert!(
            t.offered("hi-en")
                .unwrap_err()
                .contains("Settings > Lyrics")
        );
        assert!(t.offered("es-en").is_ok() && t.offered("fr-en").is_ok());
        let hi = t.models().into_iter().find(|m| m.id == "hi-en").unwrap();
        assert!(!hi.offered);
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
