//! A language model with the listener's own key: Anthropic's Claude, through the Messages API
//! (`POST /v1/messages`, the key in `x-api-key`). The whole song goes in one request (up to 200
//! lines), with a prompt written for lyrics, and the answer's shape is fixed by a JSON schema:
//! `{"lines": [...]}`. An answer with the wrong number of lines is asked for again once, then
//! is an error: lines are never shown under the wrong original.
//!
//! Unlike the translation services, Claude reads lines that mix languages or are written in
//! English letters (Hinglish), so it is given every line with words ([`Translator::wants`]).

use std::time::Duration;

use serde_json::{Value, json};

use super::http::{self, Answer};
use super::{Credentials, Keyed, chunks, pair};
use crate::{Error, Translator, langs, lines};

pub const ENDPOINT: &str = "https://api.anthropic.com";
/// The model used unless the listener enters another model id.
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
const API_VERSION: &str = "2023-06-01";
/// The default model's server-side fallback: a request its safety checks decline is answered by
/// another model instead (Claude API only).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

const MAX_LINES: usize = 200;
const MAX_CHARS: usize = 40_000;
const MAX_TOKENS: u32 = 16_000;

/// The instructions, for any song: the languages are in the request.
pub const PROMPT: &str = "You translate song lyrics for a music player that shows each translated line under the original line, in time with the music.

You get a song's lines as a JSON array. Answer with JSON {\"lines\": [...]} holding exactly one translated line for each line you were given, in the same order. Never merge, split, drop or reorder lines, never add lines, titles, notes or explanations, and never put two lines in one string.

- Keep the meaning and the tone: the feeling, the imagery and the register (tender, playful, angry, devotional). Write natural lines a listener understands at a glance rather than a word-for-word gloss, but don't embellish or invent.
- Read the whole song first. Translate a line or phrase that repeats the same way each time, and use the song for pronouns, metaphors and who is speaking to whom.
- Lines often mix languages, such as Hindi with English words (Hinglish), and are often written in English letters instead of their own script. Translate what the whole line means; keep the words already in the target language as they are.
- A line already wholly in the target language, or a line of sounds (oh, la la, na na), comes back unchanged.
- Keep the names of people and places.
- Keep each line about as long as the original.";

pub struct Claude {
    creds: Credentials,
    model: String,
    base: String,
}

impl Claude {
    pub fn new(creds: Credentials, model: Option<String>, endpoint: Option<String>) -> Self {
        let model = model
            .map(|m| m.trim().to_owned())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        Claude {
            creds,
            model,
            base: endpoint.unwrap_or_else(|| ENDPOINT.to_owned()),
        }
    }

    fn body(&self, from: &str, into: &str, run: &[&str], again: Option<usize>) -> Value {
        let named = |t: &str| match langs::name(t) {
            Some(n) => format!("{n} ({t})"),
            None => t.to_owned(),
        };
        let n = run.len();
        let mut ask = format!(
            "Translate this song from {} into {}.\nIt has {n} lines, as a JSON array. Answer with {{\"lines\": [...]}} holding exactly {n} strings, one for each line, in the same order.\n\n{}",
            named(from),
            named(into),
            serde_json::to_string(run).unwrap_or_default()
        );
        if let Some(got) = again {
            ask.push_str(&format!(
                "\n\nYour last answer had {got} lines. The answer must have exactly {n}: one for each line of the song, even a line that stays unchanged."
            ));
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": PROMPT,
            "messages": [{ "role": "user", "content": ask }],
            "output_config": {
                "format": {
                    "type": "json_schema",
                    "schema": {
                        "type": "object",
                        "properties": { "lines": { "type": "array", "items": { "type": "string" } } },
                        "required": ["lines"],
                        "additionalProperties": false
                    }
                }
            }
        });
        if self.model == DEFAULT_MODEL {
            // Translating a song needs less thinking than the model's default; and a request
            // its safety checks decline goes to the fallback model rather than failing.
            body["output_config"]["effort"] = json!("medium");
            body["fallbacks"] = json!("default");
        }
        body
    }

    /// One request: the lines Claude gave (None when its answer wasn't the JSON asked for).
    fn ask(&self, key: &str, body: &Value) -> Result<Option<Vec<String>>, Error> {
        let name = Keyed::Llm.name();
        let url = format!("{}/v1/messages", self.base.trim_end_matches('/'));
        let mut headers = vec![("x-api-key", key), ("anthropic-version", API_VERSION)];
        if self.model == DEFAULT_MODEL {
            headers.push(("anthropic-beta", FALLBACK_BETA));
        }
        let Answer { status, body } =
            http::post(name, &url, &headers, body, Duration::from_secs(180))?;
        let message = body.pointer("/error/message").and_then(Value::as_str);
        let refused = |detail: String| Error::Refused {
            engine: name.into(),
            detail,
        };
        match status {
            200 => {}
            401 | 403 => {
                return Err(Error::BadKey {
                    engine: name.into(),
                    hint: "",
                });
            }
            402 => {
                return Err(Error::Quota {
                    engine: name.into(),
                });
            }
            400 if message.is_some_and(|m| m.to_lowercase().contains("credit balance")) => {
                return Err(Error::Quota {
                    engine: name.into(),
                });
            }
            404 => {
                return Err(refused(format!(
                    "there is no model called {} for your key. Check the model id in Settings > Lyrics",
                    self.model
                )));
            }
            413 => return Err(refused("the song is too long for one request".into())),
            400 => {
                return Err(refused(http::scrub(
                    http::said(message, "it refused the request"),
                    key,
                )));
            }
            s => return Err(http::other(name, s, None)),
        }
        match body.get("stop_reason").and_then(Value::as_str) {
            Some("refusal") => return Err(refused("it declined to translate this song".into())),
            Some("max_tokens") => {
                return Err(refused("the song was too long for one answer".into()));
            }
            _ => {}
        }
        let text: String = body
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        Ok(parse(&text))
    }
}

/// The lines in Claude's answer: `{"lines": [...]}`, also when wrapped in other text.
fn parse(text: &str) -> Option<Vec<String>> {
    let json: Value = serde_json::from_str(text.trim()).ok().or_else(|| {
        let (a, b) = (text.find('{')?, text.rfind('}')?);
        serde_json::from_str(text.get(a..=b)?).ok()
    })?;
    json.get("lines")?
        .as_array()?
        .iter()
        .map(|l| l.as_str().map(str::to_owned))
        .collect()
}

impl Translator for Claude {
    fn engine(&self) -> &str {
        Keyed::Llm.id()
    }

    fn version(&self, from: &str, into: &str) -> Option<String> {
        pair(from, into).then(|| self.model.clone())
    }

    fn wants(&self, _from: &str, line: &str) -> bool {
        lines::has_words(line)
    }

    fn translate(&self, from: &str, into: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
        let key = self.creds.key().ok_or_else(|| Error::NoKey {
            engine: Keyed::Llm.name().into(),
        })?;
        let mut out = Vec::with_capacity(lines.len());
        for run in chunks(lines, MAX_LINES, MAX_CHARS) {
            let mut got = self.ask(key, &self.body(from, into, run, None))?;
            if got.as_ref().is_none_or(|g| g.len() != run.len()) {
                let had = got.as_ref().map_or(0, Vec::len);
                got = self.ask(key, &self.body(from, into, run, Some(had)))?;
            }
            match got {
                Some(g) if g.len() == run.len() => out.extend(g),
                Some(g) => {
                    return Err(Error::Lines {
                        expected: run.len(),
                        got: g.len(),
                    });
                }
                None => {
                    return Err(Error::Refused {
                        engine: Keyed::Llm.name().into(),
                        detail: "its answer wasn't in the form asked for".into(),
                    });
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_are_read_from_the_answer() {
        assert_eq!(parse(r#"{"lines": ["a", "b"]}"#).unwrap(), ["a", "b"]);
        assert_eq!(
            parse("Here you go:\n{\"lines\": [\"a\"]}\n").unwrap(),
            ["a"]
        );
        assert!(parse("no json").is_none());
        assert!(parse(r#"{"lines": [1]}"#).is_none());
    }
}
