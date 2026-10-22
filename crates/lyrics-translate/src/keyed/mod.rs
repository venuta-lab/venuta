//! Engines that run with the listener's own key (the plan, Translation, layer 4): DeepL,
//! Microsoft Translator, Google Cloud Translation and Claude. Each is a [`Translator`]; the app
//! builds one only when the listener asks for a translation (or turned on translating
//! automatically with their key), with the key they entered, kept in the system's secret store.
//!
//! - A song goes in as few requests as the service allows (one for Claude, which then reads the
//!   whole song as context), and every line sent gets exactly one line back, or an error.
//! - A service that asks to wait ("too many requests") is waited for, a few seconds at most,
//!   and asked again twice; then it is said to be busy.
//! - Every failure is a plain sentence ([`Error`]): the key not accepted, the quota used up,
//!   busy, no connection, or what the service said.
//! - The key goes only in the header the service asks for: never in an address, an error or a
//!   log, and [`Credentials`]' `Debug` hides it.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Error, Translator};

pub mod claude;
pub mod deepl;
pub mod google;
pub(crate) mod http;
pub mod microsoft;

/// An engine with the listener's own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Keyed {
    #[serde(rename = "deepl")]
    DeepL,
    #[serde(rename = "microsoft")]
    Microsoft,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "llm")]
    Llm,
}

impl Keyed {
    pub const ALL: [Keyed; 4] = [Keyed::DeepL, Keyed::Microsoft, Keyed::Google, Keyed::Llm];

    /// Its name where a line came from (`Translated("deepl")`) and in settings.
    pub fn id(self) -> &'static str {
        match self {
            Keyed::DeepL => "deepl",
            Keyed::Microsoft => "microsoft",
            Keyed::Google => "google",
            Keyed::Llm => "llm",
        }
    }

    pub fn from_id(id: &str) -> Option<Keyed> {
        Keyed::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The service's name, as the listener reads it.
    pub fn name(self) -> &'static str {
        match self {
            Keyed::DeepL => "DeepL",
            Keyed::Microsoft => "Microsoft Translator",
            Keyed::Google => "Google Cloud Translation",
            Keyed::Llm => "Claude",
        }
    }
}

/// What the listener entered for an engine: the key, and for Microsoft the resource's region.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

impl Credentials {
    pub fn new(key: impl Into<String>) -> Self {
        Credentials {
            key: key.into(),
            region: None,
        }
    }

    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// The key as entered, without spaces around it; None when blank.
    pub(crate) fn key(&self) -> Option<&str> {
        let k = self.key.trim();
        (!k.is_empty()).then_some(k)
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("key", &if self.key.is_empty() { "" } else { "(hidden)" })
            .field("region", &self.region)
            .finish()
    }
}

/// What else an engine can be told: the model (Claude), and another address (tests, a proxy).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub model: Option<String>,
    pub endpoint: Option<String>,
}

impl Options {
    /// The engine's service at `endpoint` instead of its own.
    pub fn at(endpoint: &str) -> Self {
        Options {
            model: None,
            endpoint: Some(endpoint.to_owned()),
        }
    }
}

/// An engine, with the listener's key.
pub fn engine(which: Keyed, creds: Credentials, opts: Options) -> Box<dyn Translator> {
    let endpoint = opts.endpoint.filter(|e| !e.trim().is_empty());
    match which {
        Keyed::DeepL => Box::new(deepl::DeepL::new(creds, endpoint)),
        Keyed::Microsoft => Box::new(microsoft::Microsoft::new(creds, endpoint)),
        Keyed::Google => Box::new(google::Google::new(creds, endpoint)),
        Keyed::Llm => Box::new(claude::Claude::new(creds, opts.model, endpoint)),
    }
}

/// The line a key is tried with (Settings, Test): one short line, from Spanish, or from French
/// for a listener who reads Spanish.
pub fn test_line(into: &str) -> (&'static str, &'static str) {
    if crate::langs::base(into) == "es" {
        ("fr", "Bonjour")
    } else {
        ("es", "Buenos días")
    }
}

/// Whether a pair is worth asking for: two different languages.
pub(crate) fn pair(from: &str, into: &str) -> bool {
    let (f, t) = (crate::langs::base(from), crate::langs::base(into));
    !f.is_empty() && !t.is_empty() && f != t
}

/// `lines` cut into runs of at most `max_lines` lines and `max_chars` characters (a line longer
/// than that is a run of its own).
pub(crate) fn chunks<'a>(
    lines: &'a [&'a str],
    max_lines: usize,
    max_chars: usize,
) -> Vec<&'a [&'a str]> {
    let mut out = Vec::new();
    let (mut start, mut chars) = (0, 0);
    for (i, l) in lines.iter().enumerate() {
        let n = l.chars().count();
        if i > start && (i - start >= max_lines || chars + n > max_chars) {
            out.push(&lines[start..i]);
            start = i;
            chars = 0;
        }
        chars += n;
    }
    if start < lines.len() {
        out.push(&lines[start..]);
    }
    out
}

/// Lines back from a service, checked one for one.
pub(crate) fn one_for_one(sent: usize, got: Vec<String>) -> Result<Vec<String>, Error> {
    if got.len() == sent {
        Ok(got)
    } else {
        Err(Error::Lines {
            expected: sent,
            got: got.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_by_lines_and_by_characters() {
        let l = ["aa", "bb", "cc", "dd", "ee"];
        let sizes = |r: Vec<&[&str]>| r.iter().map(|c| c.len()).collect::<Vec<_>>();
        assert_eq!(sizes(chunks(&l, 2, 100)), [2, 2, 1]);
        assert_eq!(sizes(chunks(&l, 10, 5)), [2, 2, 1]);
        assert_eq!(sizes(chunks(&["toolong", "a"], 10, 3)), [1, 1]);
        assert!(chunks(&[], 10, 10).is_empty());
    }

    #[test]
    fn the_test_line_is_never_in_the_language_you_read() {
        assert_eq!(test_line("en").0, "es");
        assert_eq!(test_line("es-MX").0, "fr");
    }
}
