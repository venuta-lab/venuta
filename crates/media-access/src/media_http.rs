//! One HTTP agent for streamed audio, shared by every song.
//!
//! Each song used to open its own connections: one for the resolver's check, another for the
//! download, and again for the next song. Every one paid a fresh TLS handshake to the media
//! host. With one agent the connection stays open between them, so the check warms up the very
//! connection the download then uses, and the next song reuses it too.

use crate::types::MediaHeader;
use std::sync::OnceLock;
use std::time::Duration;

/// The shared agent. Cloning a `ureq::Agent` shares its connection pool.
pub fn media_agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_global(Some(Duration::from_secs(30)))
                .http_status_as_error(false)
                .max_idle_connections_per_host(4)
                .build()
                .into()
        })
        .clone()
}

/// Will the media host serve this address all the way to its end?
///
/// Asks for the last 256 bytes. Some stream addresses are served only for their first
/// megabyte: a check of the opening passes, and the song then stops a minute in. Asking for the
/// end is the one question that tells such an address from a healthy one. Without a known
/// length the check is a HEAD. A 2xx answer means yes; anything else, or no answer, means no.
pub fn serves_to_the_end(url: &str, headers: &[MediaHeader], length: Option<u64>) -> bool {
    let agent = media_agent();
    let mut request = match length.filter(|n| *n > 256) {
        Some(n) => agent
            .get(url)
            .header("Range", format!("bytes={}-{}", n - 256, n - 1))
            .header("Accept-Encoding", "identity")
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
        None => agent
            .head(url)
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
    };
    for h in headers {
        request = request.header(&h.name, &h.value);
    }
    match request.call() {
        Ok(mut response) => {
            let ok = response.status().is_success();
            // Read the few bytes out so the connection goes back to the pool for reuse.
            let _ = response.body_mut().read_to_vec();
            ok
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// A media host that serves only ranges ending before `cap` (like a capped address), and
    /// answers each connection once. Returns its address.
    fn host(cap: u64) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut end = None;
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    let l = line.trim().to_ascii_lowercase();
                    if l.is_empty() {
                        break;
                    }
                    if let Some(r) = l.strip_prefix("range: bytes=") {
                        end = r.split('-').nth(1).and_then(|e| e.parse::<u64>().ok());
                    }
                    line.clear();
                }
                let ok = end.is_some_and(|e| e < cap);
                let reply = if ok {
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\n\r\nabcd"
                } else {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"
                };
                let mut s = stream;
                let _ = s.write_all(reply.as_bytes());
            }
        });
        format!("http://{addr}/audio")
    }

    #[test]
    fn a_healthy_address_is_served_to_its_end() {
        let url = host(u64::MAX);
        assert!(serves_to_the_end(&url, &[], Some(5_000_000)));
    }

    #[test]
    fn an_address_served_only_for_its_first_megabyte_fails_the_check() {
        let url = host(1 << 20);
        assert!(!serves_to_the_end(&url, &[], Some(5_000_000)));
    }
}
