//! Microsoft Translator (Azure AI Translator, v3), with the listener's key and, for a regional
//! or multi-service resource, its region: `POST /translate?api-version=3.0&from=..&to=..`, the
//! key in `Ocp-Apim-Subscription-Key`, the region in `Ocp-Apim-Subscription-Region`, up to 100
//! lines and 10,000 characters a request.

use std::time::Duration;

use serde_json::{Value, json};

use super::http::{self, Answer};
use super::{Credentials, Keyed, chunks, one_for_one, pair};
use crate::{Error, Translator};

pub const ENDPOINT: &str = "https://api.cognitive.microsofttranslator.com";

const MAX_LINES: usize = 100;
const MAX_CHARS: usize = 10_000;

pub struct Microsoft {
    creds: Credentials,
    base: String,
}

impl Microsoft {
    pub fn new(creds: Credentials, endpoint: Option<String>) -> Self {
        Microsoft {
            creds,
            base: endpoint.unwrap_or_else(|| ENDPOINT.to_owned()),
        }
    }
}

/// A language as Microsoft names it: Chinese by script, Portugal's Portuguese apart.
fn code(tag: &str) -> String {
    let lower = tag.to_ascii_lowercase().replace('_', "-");
    let has = |p: &str| lower.split('-').skip(1).any(|x| x == p);
    match crate::langs::base(tag).as_str() {
        "zh" if has("hant") || has("tw") || has("hk") || has("mo") => "zh-Hant".into(),
        "zh" => "zh-Hans".into(),
        "pt" if has("pt") => "pt-pt".into(),
        "no" => "nb".into(),
        "mn" => "mn-Cyrl".into(),
        b => b.to_owned(),
    }
}

impl Translator for Microsoft {
    fn engine(&self) -> &str {
        Keyed::Microsoft.id()
    }

    fn version(&self, from: &str, into: &str) -> Option<String> {
        pair(from, into).then(|| "v3".to_owned())
    }

    fn translate(&self, from: &str, into: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
        let name = Keyed::Microsoft.name();
        let key = self.creds.key().ok_or_else(|| Error::NoKey {
            engine: name.into(),
        })?;
        let url = format!(
            "{}/translate?api-version=3.0&from={}&to={}",
            self.base.trim_end_matches('/'),
            code(from),
            code(into)
        );
        let region = self
            .creds
            .region
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty() && !r.eq_ignore_ascii_case("global"));
        let mut headers = vec![("Ocp-Apim-Subscription-Key", key)];
        if let Some(r) = region {
            headers.push(("Ocp-Apim-Subscription-Region", r));
        }
        let bad_key = || Error::BadKey {
            engine: name.into(),
            hint: " (or its region)",
        };
        let mut out = Vec::with_capacity(lines.len());
        for run in chunks(lines, MAX_LINES, MAX_CHARS) {
            let body = Value::Array(run.iter().map(|l| json!({ "Text": l })).collect());
            let Answer { status, body } =
                http::post(name, &url, &headers, &body, Duration::from_secs(30))?;
            let err = body.get("error");
            let code = err.and_then(|e| e.get("code")).and_then(Value::as_u64);
            let message = err.and_then(|e| e.get("message")).and_then(Value::as_str);
            match (status, code) {
                (200, _) => {}
                // 403001: the free tier's characters for the month are used up.
                (403, Some(403001)) => {
                    return Err(Error::Quota {
                        engine: name.into(),
                    });
                }
                (401 | 403, _) => return Err(bad_key()),
                (400 | 404 | 408 | 413, _) => {
                    return Err(Error::Refused {
                        engine: name.into(),
                        detail: http::scrub(http::said(message, "it refused the request"), key),
                    });
                }
                (s, _) => return Err(http::other(name, s, None)),
            }
            let got: Vec<String> = body
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|i| {
                            i.pointer("/translations/0/text")
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
