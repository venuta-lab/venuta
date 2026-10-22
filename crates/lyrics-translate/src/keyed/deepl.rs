//! DeepL API (Free and Pro), with the listener's key: `POST /v2/translate`, the key in
//! `Authorization: DeepL-Auth-Key`, up to 50 lines a request, each line one sentence, and the
//! whole song as context (DeepL reads it but doesn't translate or count it). A key ending in
//! `:fx` is a DeepL API Free key, which has its own address.

use std::time::Duration;

use serde_json::{Value, json};

use super::http::{self, Answer};
use super::{Credentials, Keyed, chunks, one_for_one, pair};
use crate::{Error, Translator};

pub const FREE: &str = "https://api-free.deepl.com";
pub const PRO: &str = "https://api.deepl.com";

/// Lines a request, as DeepL allows; characters a request, well under its 128 KiB.
const MAX_LINES: usize = 50;
const MAX_CHARS: usize = 30_000;
/// The context sent with each request, at most.
const MAX_CONTEXT: usize = 4_000;

/// DeepL API Free's address for a Free key (ending in `:fx`), Pro's for any other.
pub fn endpoint_for(key: &str) -> &'static str {
    if key.trim().ends_with(":fx") {
        FREE
    } else {
        PRO
    }
}

/// "Free" or "Pro", from the key.
pub fn plan(key: &str) -> &'static str {
    if endpoint_for(key) == FREE {
        "Free"
    } else {
        "Pro"
    }
}

pub struct DeepL {
    creds: Credentials,
    base: Option<String>,
}

impl DeepL {
    pub fn new(creds: Credentials, endpoint: Option<String>) -> Self {
        DeepL {
            creds,
            base: endpoint,
        }
    }
}

/// A song's language, as DeepL names it ("HI", "ZH").
fn source(tag: &str) -> String {
    crate::langs::base(tag).to_ascii_uppercase()
}

/// The language to translate into, as DeepL asks for it: English and Portuguese by country,
/// Chinese by script.
fn target(tag: &str) -> String {
    let lower = tag.to_ascii_lowercase().replace('_', "-");
    let has = |p: &str| lower.split('-').skip(1).any(|x| x == p);
    match crate::langs::base(tag).as_str() {
        "en" if has("gb") || has("uk") || has("au") || has("nz") || has("ie") || has("in") => {
            "EN-GB".into()
        }
        "en" => "EN-US".into(),
        "pt" if has("pt") => "PT-PT".into(),
        "pt" => "PT-BR".into(),
        "zh" if has("hant") || has("tw") || has("hk") || has("mo") => "ZH-HANT".into(),
        "zh" => "ZH-HANS".into(),
        b => b.to_ascii_uppercase(),
    }
}

fn context(lines: &[&str]) -> String {
    let mut c = String::new();
    for l in lines {
        if c.chars().count() + l.chars().count() + 1 > MAX_CONTEXT {
            break;
        }
        c.push_str(l);
        c.push('\n');
    }
    c.trim_end().to_owned()
}

impl Translator for DeepL {
    fn engine(&self) -> &str {
        Keyed::DeepL.id()
    }

    fn version(&self, from: &str, into: &str) -> Option<String> {
        pair(from, into).then(|| "v2".to_owned())
    }

    fn translate(&self, from: &str, into: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
        let name = Keyed::DeepL.name();
        let key = self.creds.key().ok_or_else(|| Error::NoKey {
            engine: name.into(),
        })?;
        let base = self.base.as_deref().unwrap_or_else(|| endpoint_for(key));
        let url = format!("{}/v2/translate", base.trim_end_matches('/'));
        let auth = format!("DeepL-Auth-Key {key}");
        let ctx = context(lines);
        let mut out = Vec::with_capacity(lines.len());
        for run in chunks(lines, MAX_LINES, MAX_CHARS) {
            let body = json!({
                "text": run,
                "source_lang": source(from),
                "target_lang": target(into),
                "split_sentences": "0",
                "context": ctx,
            });
            let Answer { status, body } = http::post(
                name,
                &url,
                &[("Authorization", &auth)],
                &body,
                Duration::from_secs(30),
            )?;
            let message = body.get("message").and_then(Value::as_str);
            match status {
                200 => {}
                401 | 403 => {
                    return Err(Error::BadKey {
                        engine: name.into(),
                        hint: "",
                    });
                }
                456 => {
                    return Err(Error::Quota {
                        engine: name.into(),
                    });
                }
                400 | 404 | 413 | 414 => {
                    return Err(Error::Refused {
                        engine: name.into(),
                        detail: http::scrub(http::said(message, "it refused the request"), key),
                    });
                }
                s => return Err(http::other(name, s, None)),
            }
            let got: Vec<String> = body
                .get("translations")
                .and_then(Value::as_array)
                .map(|t| {
                    t.iter()
                        .map(|x| {
                            x.get("text")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned()
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.extend(one_for_one(run.len(), got)?);
        }
        Ok(out)
    }
}
