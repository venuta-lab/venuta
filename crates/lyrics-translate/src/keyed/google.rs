//! Google Cloud Translation, Basic (v2), with the listener's API key: `POST
//! /language/translate/v2`, the key in the `X-Goog-Api-Key` header (never in the address), plain
//! text in and out, up to 128 lines and 5,000 characters a request.

use std::time::Duration;

use serde_json::{Value, json};

use super::http::{self, Answer};
use super::{Credentials, Keyed, chunks, one_for_one, pair};
use crate::{Error, Translator};

pub const ENDPOINT: &str = "https://translation.googleapis.com";

const MAX_LINES: usize = 128;
const MAX_CHARS: usize = 5_000;

pub struct Google {
    creds: Credentials,
    base: String,
}

impl Google {
    pub fn new(creds: Credentials, endpoint: Option<String>) -> Self {
        Google {
            creds,
            base: endpoint.unwrap_or_else(|| ENDPOINT.to_owned()),
        }
    }
}

/// A language as Google names it: Chinese by country.
fn code(tag: &str) -> String {
    let lower = tag.to_ascii_lowercase().replace('_', "-");
    let has = |p: &str| lower.split('-').skip(1).any(|x| x == p);
    match crate::langs::base(tag).as_str() {
        "zh" if has("hant") || has("tw") || has("hk") || has("mo") => "zh-TW".into(),
        "zh" => "zh-CN".into(),
        b => b.to_owned(),
    }
}

/// The reasons Google gives for a failure (`errors[].reason` and `details[].reason`).
fn reasons(err: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    for list in ["errors", "details"] {
        if let Some(items) = err.and_then(|e| e.get(list)).and_then(Value::as_array) {
            out.extend(
                items
                    .iter()
                    .filter_map(|i| i.get("reason").and_then(Value::as_str))
                    .map(str::to_owned),
            );
        }
    }
    out
}

/// A failure, from its status and what Google said.
fn failure(name: &str, key: &str, status: u16, body: &Value) -> Error {
    let err = body.get("error");
    let message = err.and_then(|e| e.get("message")).and_then(Value::as_str);
    let why = reasons(err);
    let any = |r: &[&str]| why.iter().any(|w| r.contains(&w.as_str()));
    let engine = name.to_owned();
    if status == 401
        || any(&["API_KEY_INVALID", "keyInvalid", "API_KEY_SERVICE_BLOCKED"])
        || (status == 400 && message.is_some_and(|m| m.contains("API key")))
    {
        return Error::BadKey { engine, hint: "" };
    }
    if any(&["dailyLimitExceeded", "quotaExceeded", "billingNotEnabled"]) {
        return Error::Quota { engine };
    }
    if any(&[
        "rateLimitExceeded",
        "userRateLimitExceeded",
        "RATE_LIMIT_EXCEEDED",
    ]) {
        return Error::Busy { engine };
    }
    if any(&["accessNotConfigured", "SERVICE_DISABLED"]) {
        return Error::Refused {
            engine,
            detail: "the Cloud Translation API isn't turned on for this key's project".into(),
        };
    }
    match status {
        403 => Error::BadKey { engine, hint: "" },
        400 | 404 | 413 => Error::Refused {
            engine,
            detail: http::scrub(http::said(message, "it refused the request"), key),
        },
        s => http::other(name, s, None),
    }
}

impl Translator for Google {
    fn engine(&self) -> &str {
        Keyed::Google.id()
    }

    fn version(&self, from: &str, into: &str) -> Option<String> {
        pair(from, into).then(|| "v2".to_owned())
    }

    fn translate(&self, from: &str, into: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
        let name = Keyed::Google.name();
        let key = self.creds.key().ok_or_else(|| Error::NoKey {
            engine: name.into(),
        })?;
        let url = format!("{}/language/translate/v2", self.base.trim_end_matches('/'));
        let mut out = Vec::with_capacity(lines.len());
        for run in chunks(lines, MAX_LINES, MAX_CHARS) {
            let body = json!({
                "q": run,
                "source": code(from),
                "target": code(into),
                "format": "text",
            });
            let Answer { status, body } = http::post(
                name,
                &url,
                &[("X-Goog-Api-Key", key)],
                &body,
                Duration::from_secs(30),
            )?;
            if status != 200 {
                return Err(failure(name, key, status, &body));
            }
            let got: Vec<String> = body
                .pointer("/data/translations")
                .and_then(Value::as_array)
                .map(|t| {
                    t.iter()
                        .map(|x| {
                            x.get("translatedText")
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
