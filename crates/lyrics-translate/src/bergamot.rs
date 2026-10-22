//! Mozilla's Firefox Translations models, run here through `fxtranslate` (MPL-2.0), a
//! pure-Rust port of the engine Firefox uses. A model is loaded (memory-mapped) for a song and
//! let go after it, so nothing stays in memory between songs: loading takes a few tenths of a
//! second, a song's translation a few seconds at most, and a song is translated once.

use fxtranslate::engine::Engine;
use fxtranslate::shortlist::Shortlist;

use crate::store::ModelStore;
use crate::{Error, Translator};

/// The engine's name in the lyrics (`Translated("bergamot")`).
pub const ENGINE: &str = "bergamot";

pub struct Bergamot<'a> {
    store: &'a ModelStore,
    threads: usize,
}

impl<'a> Bergamot<'a> {
    /// Translates with the models in `store`, on `threads` threads (0: one per core; each
    /// shares the one copy of the model).
    pub fn new(store: &'a ModelStore, threads: usize) -> Self {
        Bergamot { store, threads }
    }

    fn load(&self, from: &str, into: &str) -> Result<Engine, Error> {
        let m = self
            .store
            .catalog()
            .find(from, into)
            .ok_or_else(|| Error::NoModel {
                from: from.to_owned(),
                into: into.to_owned(),
            })?;
        let files = self
            .store
            .files(&m.id)
            .ok_or_else(|| Error::NotInstalled(m.id.clone()))?;
        let shortlist = Shortlist::load(&files.lex)?;
        let engine = Engine::load_mmapped(&files.model, &files.src_vocab, &files.trg_vocab)
            .map_err(Error::Engine)?
            .with_shortlist(shortlist)
            .with_threads(self.threads);
        Ok(engine)
    }
}

impl Translator for Bergamot<'_> {
    fn engine(&self) -> &str {
        ENGINE
    }

    fn version(&self, from: &str, into: &str) -> Option<String> {
        let m = self.store.catalog().find(from, into)?;
        Some(format!("{} {} {}", m.id, m.arch, m.version))
    }

    fn translate(&self, from: &str, into: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
        let engine = self.load(from, into)?;
        // Each line is a sentence to the model; the batch runs them side by side.
        Ok(engine.translate_batch(lines))
    }
}
