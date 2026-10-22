//! One JSON request to a service, waited for and asked again when the service says it is busy.

use std::time::Duration;

use serde_json::Value;

use crate::Error;

/// A generic User-Agent: nothing about the listener is sent.
const USER_AGENT: &str = "Venuta (https://github.com/venuta-lab/venuta)";
/// How often a busy service is asked again, and the longest wait before each time.
const RETRIES: u32 = 2;
const MAX_WAIT: Duration = Duration::from_secs(10);

/// What a service answered: its status, and its body as JSON (Null when it wasn't JSON).
pub(crate) struct Answer {
    pub status: u16,
    pub body: Value,
}

/// Whether a status means "busy, ask again later".
fn busy(status: u16) -> bool {
    matches!(status, 429 | 503 | 529)
}

/// POSTs `body` to `url` with `headers`. A busy answer (429, 503, 529) is waited for (as long as
/// its Retry-After says, at most [`MAX_WAIT`]; else 1 then 2 seconds) and asked again
/// [`RETRIES`] times, then is [`Error::Busy`]; no connection is [`Error::Unreachable`]. Any
/// other answer is the caller's to read.
pub(crate) fn post(
    engine: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
    timeout: Duration,
) -> Result<Answer, Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .build()
        .into();
    let text = body.to_string();
    for attempt in 0..=RETRIES {
        let mut req = agent.post(url).header("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        // The error says what went wrong with the connection, never the request (or its key).
        let mut resp = req.send(text.as_str()).map_err(|_| Error::Unreachable {
            engine: engine.to_owned(),
        })?;
        let status = resp.status().as_u16();
        if busy(status) && attempt < RETRIES {
            let wait = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(1 << attempt));
            std::thread::sleep(wait.min(MAX_WAIT));
            continue;
        }
        if busy(status) {
            return Err(Error::Busy {
                engine: engine.to_owned(),
            });
        }
        let raw = resp
            .body_mut()
            .read_to_string()
            .map_err(|_| Error::Unreachable {
                engine: engine.to_owned(),
            })?;
        return Ok(Answer {
            status,
            body: serde_json::from_str(&raw).unwrap_or(Value::Null),
        });
    }
    unreachable!("the last attempt returns")
}

/// What a service said, short, for an error the listener reads.
pub(crate) fn said(text: Option<&str>, fallback: &str) -> String {
    let t = text
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(fallback);
    let mut s: String = t.chars().take(200).collect();
    if t.chars().count() > 200 {
        s.push('…');
    }
    s
}

/// The answer's body for a status the engines don't know: a server problem, said plainly.
pub(crate) fn other(engine: &str, status: u16, message: Option<&str>) -> Error {
    let detail = if status >= 500 {
        format!("the service had a problem (HTTP {status}). Try again later")
    } else {
        said(message, &format!("HTTP {status}"))
    };
    Error::Refused {
        engine: engine.to_owned(),
        detail,
    }
}

/// `text` with the key taken out, in case a service ever repeats it back.
pub(crate) fn scrub(text: String, key: &str) -> String {
    if key.len() >= 4 && text.contains(key) {
        text.replace(key, "(your key)")
    } else {
        text
    }
}
