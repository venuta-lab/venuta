//! OAuth 2.0 for a desktop app (RFC 8252) with PKCE (RFC 7636): the system browser signs you
//! in, the answer comes back to a one-shot listener on 127.0.0.1, and there is no client
//! secret anywhere. Generic over the service: endpoints and scopes come from a [`Provider`].

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A service Venuta can sign in to.
#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub authorize_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    /// The path registered with the service (loopback, any port): "/callback".
    pub redirect_path: String,
}

/// The tokens a sign-in or a refresh gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Some services rotate it on every refresh: keep the newest.
    pub refresh_token: Option<String>,
    /// Seconds since 1970 when the access token stops working.
    pub expires_at: u64,
    pub scope: String,
}

/// RFC 7636 S256: BASE64URL(SHA256(verifier)), no padding.
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// URL-safe random text from `bytes` random bytes.
fn random(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("the system random source");
    URL_SAFE_NO_PAD.encode(b)
}

fn form(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Undo %XX and "+" in a query value.
fn unform(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'+' {
            out.push(b' ');
        } else if b[i] == b'%'
            && i + 3 <= b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A sign-in under way: the listener waiting for the browser, and what proves the answer is
/// ours (the state) and finishes it (the PKCE verifier).
pub struct Pending {
    listener: TcpListener,
    pub redirect_uri: String,
    state: String,
    verifier: String,
    pub authorize_url: String,
}

/// Start signing in: listen on 127.0.0.1 (the system picks the port) and build the address to
/// open in the browser.
pub fn begin(p: &Provider, client_id: &str) -> Result<Pending, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Couldn't listen for the sign-in: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{}", p.redirect_path);
    let state = random(24);
    let verifier = random(48); // 64 characters, within RFC 7636's 43–128
    let authorize_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={}&scope={}",
        p.authorize_url,
        form(client_id),
        form(&redirect_uri),
        challenge(&verifier),
        state,
        form(&p.scopes.join(" ")),
    );
    Ok(Pending {
        listener,
        redirect_uri,
        state,
        verifier,
        authorize_url,
    })
}

/// The page the browser shows once the answer has arrived.
fn page(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=utf-8><title>Venuta</title><style>body{{font:16px system-ui,sans-serif;background:#16161a;color:#eee;display:grid;place-items:center;height:100vh;margin:0}}main{{max-width:28rem;text-align:center}}h1{{font-size:1.4rem}}p{{color:#aaa}}</style></head><body><main><h1>{title}</h1><p>{text}</p></main></body></html>"
    )
}

fn respond(mut s: &TcpStream, status: &str, body: &str) {
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = s.flush();
}

/// Wait for the browser to come back with the answer (a code), for up to `timeout` or until
/// `cancel`. Requests for anything else (a favicon) are answered 404 and ignored.
pub fn wait(pending: &Pending, timeout: Duration, cancel: &AtomicBool) -> Result<String, String> {
    pending
        .listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Signing in was cancelled.".into());
        }
        if Instant::now() > until {
            return Err("Signing in took too long, so it stopped. Try again.".into());
        }
        let (stream, _) = match pending.listener.accept() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        // "GET /callback?code=…&state=… HTTP/1.1"
        let target = line.split_whitespace().nth(1).unwrap_or("");
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if !line.starts_with("GET ") || path != pending_path(&pending.redirect_uri) {
            respond(&stream, "404 Not Found", "");
            continue;
        }
        let param = |k: &str| {
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(key, _)| *key == k)
                .map(|(_, v)| unform(v))
        };
        if param("state").as_deref() != Some(pending.state.as_str()) {
            // Not the answer to this sign-in: refuse it, keep waiting for the real one.
            respond(
                &stream,
                "400 Bad Request",
                &page(
                    "This sign-in doesn't match",
                    "Go back to Venuta and start again.",
                ),
            );
            continue;
        }
        if let Some(err) = param("error") {
            respond(
                &stream,
                "200 OK",
                &page(
                    "Not connected",
                    "You can close this tab and go back to Venuta.",
                ),
            );
            return Err(if err == "access_denied" {
                "You chose not to connect.".into()
            } else {
                format!("The service said no ({err}).")
            });
        }
        let Some(code) = param("code").filter(|c| !c.is_empty()) else {
            respond(
                &stream,
                "400 Bad Request",
                &page(
                    "Something was missing",
                    "Go back to Venuta and start again.",
                ),
            );
            return Err("The service sent no code.".into());
        };
        respond(
            &stream,
            "200 OK",
            &page(
                "Venuta is connected",
                "You can close this tab and go back to Venuta.",
            ),
        );
        return Ok(code);
    }
}

fn pending_path(redirect_uri: &str) -> &str {
    redirect_uri
        .split("://")
        .nth(1)
        .and_then(|r| r.find('/').map(|i| &r[i..]))
        .unwrap_or("/")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(crate::net::USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into()
}

fn token_request(p: &Provider, fields: &[(&str, &str)]) -> Result<Tokens, String> {
    let mut resp = agent()
        .post(&p.token_url)
        .header("Accept", "application/json")
        .send_form(fields.iter().copied())
        .map_err(|e| format!("Couldn't reach {}: {e}", p.name))?;
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("{} answered in an unexpected way: {e}", p.name))?;
    if status != 200 {
        // The code (invalid_grant…) says what to do; the description says why, in words.
        let code = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("refused");
        let why = body
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or(code);
        return Err(format!("{} refused the sign-in ({code}): {why}", p.name));
    }
    let s = |k: &str| body.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    Ok(Tokens {
        access_token: s("access_token").ok_or("No access token in the answer.")?,
        refresh_token: s("refresh_token"),
        // A minute early, so a token never runs out mid-request.
        expires_at: now()
            + body
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(3600)
                .saturating_sub(60),
        scope: s("scope").unwrap_or_default(),
    })
}

/// Trade the code for tokens (with the PKCE verifier that only this sign-in knows).
pub fn exchange(
    p: &Provider,
    client_id: &str,
    pending: &Pending,
    code: &str,
) -> Result<Tokens, String> {
    token_request(
        p,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &pending.redirect_uri),
            ("client_id", client_id),
            ("code_verifier", &pending.verifier),
        ],
    )
}

/// A fresh access token from the refresh token. The refresh token may change: keep the new one.
pub fn refresh(p: &Provider, client_id: &str, refresh_token: &str) -> Result<Tokens, String> {
    let mut t = token_request(
        p,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ],
    )?;
    if t.refresh_token.is_none() {
        t.refresh_token = Some(refresh_token.to_owned());
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn provider(token_url: &str) -> Provider {
        Provider {
            name: "Test".into(),
            authorize_url: "https://accounts.example.com/authorize".into(),
            token_url: token_url.into(),
            scopes: vec!["playlist-read-private".into(), "user-library-read".into()],
            redirect_path: "/callback".into(),
        }
    }

    #[test]
    fn pkce_challenge_matches_rfc_7636() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_sign_in_address_carries_pkce_state_and_a_loopback_redirect() {
        let p = begin(&provider("http://127.0.0.1:1/token"), "abc123").unwrap();
        let u = &p.authorize_url;
        assert!(u.starts_with(
            "https://accounts.example.com/authorize?response_type=code&client_id=abc123&"
        ));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains(&format!("code_challenge={}", challenge(&p.verifier))));
        assert!(u.contains(&format!("state={}", p.state)));
        assert!(u.contains("scope=playlist-read-private%20user-library-read"));
        assert!(
            p.redirect_uri.starts_with("http://127.0.0.1:")
                && p.redirect_uri.ends_with("/callback")
        );
        assert!(u.contains(&format!("redirect_uri={}", form(&p.redirect_uri))));
        assert!((43..=128).contains(&p.verifier.len()));
        // Two sign-ins never share a secret.
        let q = begin(&provider("http://127.0.0.1:1/token"), "abc123").unwrap();
        assert_ne!(p.state, q.state);
        assert_ne!(p.verifier, q.verifier);
    }

    /// What the browser does when the service sends it back: one GET to the redirect address.
    fn browser(url: String) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let rest = url.trim_start_matches("http://");
            let (host, path) = rest.split_once('/').unwrap();
            let mut s = TcpStream::connect(host).unwrap();
            write!(s, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        })
    }

    #[test]
    fn the_listener_takes_only_the_answer_with_our_state() {
        let p = std::sync::Arc::new(begin(&provider("http://127.0.0.1:1/token"), "id").unwrap());
        let (uri, state) = (p.redirect_uri.clone(), p.state.clone());
        let waiting = {
            let p = p.clone();
            std::thread::spawn(move || wait(&p, Duration::from_secs(10), &AtomicBool::new(false)))
        };
        // One after another, as a browser would: a favicon, a forged answer, then the real one.
        let base = uri.trim_end_matches("/callback").to_owned();
        assert!(
            browser(format!("{base}/favicon.ico"))
                .join()
                .unwrap()
                .starts_with("HTTP/1.1 404")
        );
        assert!(
            browser(format!("{uri}?code=evil&state=wrong"))
                .join()
                .unwrap()
                .starts_with("HTTP/1.1 400")
        );
        assert!(
            browser(format!("{uri}?code=good%2Bcode&state={state}"))
                .join()
                .unwrap()
                .contains("Venuta is connected")
        );
        assert_eq!(waiting.join().unwrap().unwrap(), "good+code");
    }

    #[test]
    fn saying_no_and_waiting_too_long_end_the_sign_in() {
        let p = begin(&provider("http://127.0.0.1:1/token"), "id").unwrap();
        let no = browser(format!(
            "{}?error=access_denied&state={}",
            p.redirect_uri, p.state
        ));
        assert_eq!(
            wait(&p, Duration::from_secs(10), &AtomicBool::new(false)).unwrap_err(),
            "You chose not to connect."
        );
        no.join().unwrap();
        let q = begin(&provider("http://127.0.0.1:1/token"), "id").unwrap();
        assert!(
            wait(&q, Duration::from_millis(300), &AtomicBool::new(false))
                .unwrap_err()
                .contains("too long")
        );
        assert!(
            wait(&q, Duration::from_secs(10), &AtomicBool::new(true))
                .unwrap_err()
                .contains("cancelled")
        );
    }

    /// A token endpoint that checks the form and answers like a real one.
    fn token_server(check: fn(&str) -> bool, answer: &'static str) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let (status, text) = if check(&body) {
                ("200 OK", answer)
            } else {
                (
                    "400 Bad Request",
                    r#"{"error":"invalid_grant","error_description":"bad form"}"#,
                )
            };
            write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).unwrap();
        });
        format!("http://127.0.0.1:{port}/api/token")
    }

    #[test]
    fn the_code_is_traded_with_the_verifier_and_refresh_keeps_a_token() {
        let url = token_server(
            |b| {
                b.contains("grant_type=authorization_code")
                    && b.contains("code=c0de")
                    && b.contains("code_verifier=")
                    && b.contains("redirect_uri=http%3A%2F%2F127.0.0.1")
            },
            r#"{"access_token":"AT","token_type":"Bearer","expires_in":3600,"refresh_token":"RT","scope":"playlist-read-private"}"#,
        );
        let p = provider(&url);
        let pending = begin(&p, "client").unwrap();
        let t = exchange(&p, "client", &pending, "c0de").unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.refresh_token.as_deref()),
            ("AT", Some("RT"))
        );
        assert!(t.expires_at > now() + 3000);

        // A refresh that sends no new refresh token keeps the old one.
        let url = token_server(
            |b| b.contains("grant_type=refresh_token") && b.contains("refresh_token=RT"),
            r#"{"access_token":"AT2","expires_in":3600}"#,
        );
        let t = refresh(&provider(&url), "client", "RT").unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.refresh_token.as_deref()),
            ("AT2", Some("RT"))
        );

        // A refusal says why.
        let url = token_server(|_| false, "");
        assert!(
            refresh(&provider(&url), "client", "old")
                .unwrap_err()
                .contains("bad form")
        );
    }
}
