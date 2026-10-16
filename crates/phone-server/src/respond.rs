//! Answers in the Subsonic envelope, as JSON or XML (some apps still ask for XML).
//!
//! One shape for both: a JSON object's plain values become XML attributes, its objects and
//! arrays become child elements named by their key, and a `value` key becomes the element's
//! text (a lyric line).

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

pub const API_VERSION: &str = "1.16.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Xml,
}

impl Format {
    pub fn from_param(f: Option<&str>) -> Self {
        match f {
            Some("json") | Some("jsonp") => Format::Json,
            _ => Format::Xml,
        }
    }
}

/// Subsonic's error codes that Venuta uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Generic = 0,
    MissingParam = 10,
    WrongCredentials = 40,
    NotFound = 70,
}

fn envelope(status: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("status".into(), status.into());
    m.insert("version".into(), API_VERSION.into());
    m.insert("type".into(), "venuta".into());
    m.insert("serverVersion".into(), env!("CARGO_PKG_VERSION").into());
    m.insert("openSubsonic".into(), true.into());
    m
}

/// A successful answer: `payload`'s keys sit beside the envelope's (`{"album": {...}}`).
pub fn ok(f: Format, payload: Value) -> Response {
    let mut m = envelope("ok");
    if let Value::Object(p) = payload {
        m.extend(p);
    }
    render(f, m)
}

pub fn error(f: Format, code: Code, message: &str) -> Response {
    let mut m = envelope("failed");
    m.insert(
        "error".into(),
        json!({ "code": code as u8, "message": message }),
    );
    render(f, m)
}

fn render(f: Format, m: Map<String, Value>) -> Response {
    match f {
        Format::Json => {
            let body = json!({ "subsonic-response": Value::Object(m) }).to_string();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                body,
            )
                .into_response()
        }
        Format::Xml => {
            let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
            element(&mut out, "subsonic-response", &m, true);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                out,
            )
                .into_response()
        }
    }
}

fn element(out: &mut String, name: &str, m: &Map<String, Value>, root: bool) {
    out.push('<');
    out.push_str(name);
    if root {
        out.push_str(r#" xmlns="http://subsonic.org/restapi""#);
    }
    let mut text = None;
    for (k, v) in m {
        match v {
            Value::Object(_) | Value::Array(_) | Value::Null => {}
            _ if k == "value" => text = Some(scalar(v)),
            _ => {
                out.push(' ');
                out.push_str(k);
                out.push_str("=\"");
                out.push_str(&escape(&scalar(v)));
                out.push('"');
            }
        }
    }
    out.push('>');
    if let Some(t) = text {
        out.push_str(&escape(&t));
    }
    for (k, v) in m {
        match v {
            Value::Object(child) => element(out, k, child, false),
            Value::Array(items) => {
                for item in items {
                    match item {
                        Value::Object(child) => element(out, k, child, false),
                        // A list of plain values: each one an element's text (a genre name).
                        other => {
                            let mut one = Map::new();
                            one.insert("value".into(), other.clone());
                            element(out, k, &one, false);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters XML 1.0 can't carry at all.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body(r: Response) -> String {
        String::from_utf8(to_bytes(r.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn one_answer_reads_the_same_as_json_and_as_xml() {
        let payload = json!({
            "album": {
                "id": "al1", "name": "Kind of Blue & more", "songCount": 2,
                "song": [ { "id": "s1", "title": "So What" }, { "id": "s2", "title": "\"Blue\" in Green" } ]
            },
            "lyrics": { "line": [ { "start": 0, "value": "Tum <ho>" } ] }
        });
        let j: Value =
            serde_json::from_str(&body(ok(Format::Json, payload.clone())).await).unwrap();
        let r = &j["subsonic-response"];
        assert_eq!(r["status"], "ok");
        assert_eq!(r["openSubsonic"], true);
        assert_eq!(r["album"]["song"][1]["title"], "\"Blue\" in Green");

        let x = body(ok(Format::Xml, payload)).await;
        assert!(x.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?><subsonic-response xmlns="http://subsonic.org/restapi" "#), "{x}");
        assert!(
            x.contains(r#"<album id="al1" name="Kind of Blue &amp; more" songCount="2">"#),
            "{x}"
        );
        assert!(
            x.contains(r#"<song id="s2" title="&quot;Blue&quot; in Green"></song>"#),
            "{x}"
        );
        assert!(
            x.contains(r#"<line start="0">Tum &lt;ho&gt;</line>"#),
            "{x}"
        );
    }

    #[tokio::test]
    async fn errors_carry_a_code_and_say_failed() {
        let j: Value = serde_json::from_str(
            &body(error(
                Format::Json,
                Code::WrongCredentials,
                "Wrong username or password.",
            ))
            .await,
        )
        .unwrap();
        assert_eq!(j["subsonic-response"]["status"], "failed");
        assert_eq!(j["subsonic-response"]["error"]["code"], 40);
        let x = body(error(Format::Xml, Code::NotFound, "Not found")).await;
        assert!(
            x.contains(r#"<error code="70" message="Not found"></error>"#),
            "{x}"
        );
    }
}
