//! The guest's way to the host's files (design 4.1): the player reads a host song like any
//! online stream, from a small HTTP server on this computer (127.0.0.1 only, a random token in
//! every address), and each request it makes (a range, for seeking; a HEAD) is asked of the
//! host on a media stream of its own (`Transport::open_media`), the bytes passed on as they
//! come.

use crate::net;
use std::sync::Arc;
use together::transport::{Get, PeerId, Transport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A request the player made, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub head: bool,
    /// The host song's key (`host:` and 32 hex digits).
    pub key: String,
    /// `Range: bytes=from-` or `bytes=from-to` (inclusive, as HTTP has it).
    pub range: Option<(u64, Option<u64>)>,
}

/// Why a request isn't served: its status line.
pub const FORBIDDEN: &str = "403 Forbidden";
pub const NOT_FOUND: &str = "404 Not Found";

/// Read a request's head: only GET or HEAD of `/s/host:<32 hex>?t=<token>`, the token
/// compared in constant time.
pub fn read_request(head: &str, token: &str) -> Result<Request, &'static str> {
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("");
    let target = first.next().unwrap_or("");
    let Some((path, query)) = target.split_once('?') else {
        return Err(NOT_FOUND);
    };
    let key = path.strip_prefix("/s/").unwrap_or("");
    let token_ok = query.strip_prefix("t=").is_some_and(|t| {
        t.len() == token.len()
            && t.bytes()
                .zip(token.bytes())
                .fold(0u8, |a, (x, y)| a | (x ^ y))
                == 0
    });
    let key_ok = key
        .strip_prefix("host:")
        .is_some_and(|h| h.len() == 32 && h.bytes().all(|c| c.is_ascii_hexdigit()));
    if !token_ok || !key_ok || !(method == "GET" || method == "HEAD") {
        return Err(FORBIDDEN);
    }
    let range = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if !k.trim().eq_ignore_ascii_case("range") {
            return None;
        }
        let (a, b) = v.trim().strip_prefix("bytes=")?.split_once('-')?;
        Some((a.trim().parse::<u64>().ok()?, b.trim().parse::<u64>().ok()))
    });
    Ok(Request {
        head: method == "HEAD",
        key: key.to_owned(),
        range,
    })
}

fn reply(code: &str) -> String {
    format!("HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

/// The server, for as long as it is kept (dropped: it stops).
pub struct Proxy {
    port: u16,
    token: String,
    task: tokio::task::JoinHandle<()>,
}

impl Proxy {
    /// Serve the host's files from `host` over `t`, on a port of 127.0.0.1. Called on the
    /// async runtime its work runs on.
    pub async fn start(t: Arc<dyn Transport>, host: PeerId) -> Result<Proxy, String> {
        let token = net::random_hex(16);
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let tok = token.clone();
        let task = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (t, tok) = (t.clone(), tok.clone());
                tokio::spawn(async move { serve(tcp, t.as_ref(), host, &tok).await });
            }
        });
        Ok(Proxy { port, token, task })
    }

    /// Where it listens, and the token its addresses carry.
    pub fn address(&self) -> (u16, String) {
        (self.port, self.token.clone())
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// One request: asked of the host, its bytes passed on.
async fn serve(mut tcp: TcpStream, t: &dyn Transport, host: PeerId, token: &str) {
    let mut head = Vec::new();
    let mut b = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match tcp.read(&mut b).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&b[..n]),
        }
        if head.len() > 16 * 1024 {
            return;
        }
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let req = match read_request(&text, token) {
        Ok(r) => r,
        Err(code) => {
            let _ = tcp.write_all(reply(code).as_bytes()).await;
            return;
        }
    };
    let (from, to) = req.range.unwrap_or((0, None));
    // HEAD asks for one byte: enough for the size and the type.
    let end = if req.head {
        Some(from + 1)
    } else {
        to.map(|t| t.saturating_add(1))
    };
    let get = Get {
        get: req.key.clone(),
        from,
        to: end,
    };
    let Ok(mut stream) = t.open_media(host, get).await else {
        let _ = tcp.write_all(reply(NOT_FOUND).as_bytes()).await;
        return;
    };
    let size = stream.size.or(stream.total.map(|n| from + n)).unwrap_or(0);
    let mime = stream
        .mime
        .clone()
        .unwrap_or_else(|| "application/octet-stream".into());
    let last = to
        .unwrap_or(size.saturating_sub(1))
        .min(size.saturating_sub(1));
    let len = if size == 0 || from > last {
        0
    } else {
        last - from + 1
    };
    let status = if req.range.is_some() {
        format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{last}/{size}\r\n")
    } else {
        "HTTP/1.1 200 OK\r\n".to_owned()
    };
    let headers = format!(
        "{status}Content-Type: {mime}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
    );
    if tcp.write_all(headers.as_bytes()).await.is_err() || req.head {
        return;
    }
    let mut sent = 0u64;
    while sent < len {
        match stream.body.read_chunk().await {
            Ok(Some(c)) => {
                let c = &c[..c.len().min((len - sent) as usize)];
                if tcp.write_all(c).await.is_err() {
                    return;
                }
                sent += c.len() as u64;
            }
            Ok(None) | Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    const KEY: &str = "host:0123456789abcdef0123456789abcdef";

    #[test]
    fn only_a_host_song_with_the_token_is_served() {
        let ok = format!("GET /s/{KEY}?t={TOKEN} HTTP/1.1\r\nRange: bytes=100-199\r\n\r\n");
        assert_eq!(
            read_request(&ok, TOKEN),
            Ok(Request {
                head: false,
                key: KEY.into(),
                range: Some((100, Some(199)))
            })
        );
        let open = format!("HEAD /s/{KEY}?t={TOKEN} HTTP/1.1\r\nrange: bytes=5-\r\n\r\n");
        assert_eq!(
            read_request(&open, TOKEN).map(|r| (r.head, r.range)),
            Ok((true, Some((5, None))))
        );
        let wrong = format!("GET /s/{KEY}?t={} HTTP/1.1\r\n\r\n", "f".repeat(32));
        assert_eq!(read_request(&wrong, TOKEN), Err(FORBIDDEN));
        let path = format!("GET /s/../etc?t={TOKEN} HTTP/1.1\r\n\r\n");
        assert_eq!(read_request(&path, TOKEN), Err(FORBIDDEN));
        let post = format!("POST /s/{KEY}?t={TOKEN} HTTP/1.1\r\n\r\n");
        assert_eq!(read_request(&post, TOKEN), Err(FORBIDDEN));
        assert_eq!(
            read_request("GET /nothing HTTP/1.1\r\n\r\n", TOKEN),
            Err(NOT_FOUND)
        );
    }
}
