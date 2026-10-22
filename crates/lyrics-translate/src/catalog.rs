//! The translation models there are: Mozilla's Firefox Translations models, one per language
//! pair and direction, each file with its address, its size and the SHA-256 it must have
//! (`data/bergamot-models.json`, written by `scripts/translation-models.mjs` from Mozilla's
//! model registry and Firefox's own records of the same files). No model is shipped with the
//! app: each is downloaded from Mozilla when the listener asks for it.

use serde::Deserialize;

use crate::Error;

const BUNDLED: &str = include_str!("../data/bergamot-models.json");

/// What a file of a model is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    /// The network's weights.
    Model,
    /// The vocabulary, shared by both languages.
    Vocab,
    /// The vocabularies of each side, for pairs that have two (Chinese, Japanese, Korean).
    #[serde(rename = "srcvocab")]
    SrcVocab,
    #[serde(rename = "trgvocab")]
    TrgVocab,
    /// The lexical shortlist: the words worth considering for each source word (speed).
    Lex,
}

/// One file of a model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelFile {
    pub kind: FileKind,
    /// Its name once unpacked ("model.hien.intgemm.alphas.bin").
    pub name: String,
    /// Where it is, gzipped.
    pub url: String,
    /// Its size as downloaded, and unpacked.
    pub download_bytes: u64,
    pub bytes: u64,
    /// The SHA-256 of the unpacked file, lowercase hex.
    pub sha256: String,
}

/// Which devices a model is meant for: Mozilla trains a lighter one for phones for a few
/// pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[serde(rename = "all")]
    All,
    Desktop,
    Android,
}

impl Platform {
    /// The platform this build runs on.
    pub fn current() -> Platform {
        if cfg!(target_os = "android") {
            Platform::Android
        } else {
            Platform::Desktop
        }
    }
}

/// A model: one language into another.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelInfo {
    /// "hi-en": what it translates from and into.
    pub id: String,
    /// BCP 47 ("hi", "zh-Hans").
    pub from: String,
    pub to: String,
    pub from_name: String,
    pub to_name: String,
    /// "tiny", "base-memory", "base".
    pub arch: String,
    pub platform: Platform,
    pub version: String,
    pub files: Vec<ModelFile>,
}

impl ModelInfo {
    /// "Hindi to English".
    pub fn name(&self) -> String {
        format!("{} to {}", self.from_name, self.to_name)
    }

    /// Its size as downloaded, and on the device.
    pub fn download_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.download_bytes).sum()
    }
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    pub fn file(&self, kind: FileKind) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.kind == kind)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    /// Who publishes the models.
    pub publisher: String,
    models: Vec<ModelInfo>,
}

/// The base of a language tag, for matching ("hi-IN" is "hi"); Chinese keeps its script,
/// which picks the model ("zh" alone means simplified).
fn base(tag: &str) -> String {
    let mut parts = tag.split(['-', '_']);
    let first = parts.next().unwrap_or("").to_ascii_lowercase();
    if first == "zh" {
        let hant = tag.split(['-', '_']).any(|p| {
            p.eq_ignore_ascii_case("hant")
                || p.eq_ignore_ascii_case("tw")
                || p.eq_ignore_ascii_case("hk")
                || p.eq_ignore_ascii_case("mo")
        });
        return if hant { "zh-hant" } else { "zh-hans" }.to_owned();
    }
    first
}

impl Catalog {
    /// The models known to this build.
    pub fn bundled() -> Catalog {
        Catalog::from_json(BUNDLED).expect("the bundled model list is valid")
    }

    pub fn from_json(json: &str) -> Result<Catalog, Error> {
        serde_json::from_str(json).map_err(|e| Error::Engine(format!("model list: {e}")))
    }

    /// Only the models for `p` (the phone's lighter ones on Android, the full ones elsewhere).
    pub fn for_platform(mut self, p: Platform) -> Catalog {
        self.models
            .retain(|m| m.platform == Platform::All || m.platform == p);
        self
    }

    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    pub fn get(&self, id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The model that translates a song in `from` into `into` (BCP 47, regions ignored).
    pub fn find(&self, from: &str, into: &str) -> Option<&ModelInfo> {
        let (f, t) = (base(from), base(into));
        if f == t {
            return None;
        }
        self.models
            .iter()
            .find(|m| base(&m.from) == f && base(&m.to) == t)
    }
}
