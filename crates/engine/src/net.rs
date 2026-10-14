//! Polite HTTP for metadata lookups: one pace per host (each service's published limit),
//! retries that honour `Retry-After`, an honest User-Agent, and a local cache so the same
//! question is never asked twice. Free public services stay free by being treated well.

use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USER_AGENT: &str = concat!(
    "Venuta/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/venuta-lab/venuta)"
);

/// Minimum gap between requests to one host.
fn pace_for(host: &str) -> Duration {
    match host {
        "musicbrainz.org" => Duration::from_millis(1100), // 1 request/s
        "itunes.apple.com" => Duration::from_millis(3100), // ~20/min
        "api.deezer.com" => Duration::from_millis(150),   // well under 50 per 5 s
        "lrclib.net" => Duration::from_millis(250),
        "api.discogs.com" => Duration::from_millis(1100), // 60/min with a token
        "api.acoustid.org" => Duration::from_millis(350), // at most 3/s
        _ => Duration::from_millis(300),
    }
}

/// How long a cached answer stays good. Catalogue data barely changes.
const CACHE_TTL_SECS: i64 = 60 * 60 * 24 * 90;

pub struct Http {
    agent: ureq::Agent,
    next_slot: Mutex<HashMap<String, Instant>>,
    cache: Mutex<Connection>,
}

#[derive(Debug)]
pub enum NetError {
    /// Nothing at that address (404) — a real answer, cached like any other.
    NotFound,
    /// Network or server trouble after retries; not cached.
    Failed(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::NotFound => write!(f, "not found"),
            NetError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl Http {
    pub fn open() -> Result<Self, String> {
        let dir = crate::paths::app_data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(dir.join("cache.sqlite")).map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_secs(30))
            .map_err(|e| e.to_string())?;
        crate::index::use_wal(&conn).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS http (
                url        TEXT PRIMARY KEY,
                status     INTEGER NOT NULL,
                body       BLOB,
                fetched_at INTEGER NOT NULL
             );
             -- Older versions kept the user's keys in the address; those answers go.
             DELETE FROM http WHERE url LIKE '%token=%' OR url LIKE '%client=%';",
        )
        .map_err(|e| e.to_string())?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .build()
            .into();
        Ok(Self {
            agent,
            next_slot: Mutex::new(HashMap::new()),
            cache: Mutex::new(conn),
        })
    }

    /// GET a URL as text (JSON), from the cache when fresh.
    pub fn get_text(&self, url: &str) -> Result<String, NetError> {
        let bytes = self.get_bytes(url)?;
        String::from_utf8(bytes).map_err(|e| NetError::Failed(e.to_string()))
    }

    pub fn get_json(&self, url: &str) -> Result<serde_json::Value, NetError> {
        serde_json::from_str(&self.get_text(url)?)
            .map_err(|e| NetError::Failed(format!("bad JSON: {e}")))
    }

    /// GET a URL's bytes (also used for cover art), from the cache when fresh.
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>, NetError> {
        if let Some((status, body)) = self.cached(url) {
            return match status {
                200 => Ok(body),
                404 => Err(NetError::NotFound),
                s => Err(NetError::Failed(format!("HTTP {s}"))),
            };
        }
        let host = url
            .split("://")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .unwrap_or("")
            .to_owned();
        let mut attempt = 0u32;
        loop {
            self.wait_turn(&host);
            let result = self
                .agent
                .get(url)
                .header("Accept", "application/json")
                .call();
            match result {
                Ok(mut resp) => {
                    let status = resp.status().as_u16();
                    match status {
                        200 => {
                            let body = resp
                                .body_mut()
                                .with_config()
                                .limit(25 * 1024 * 1024)
                                .read_to_vec()
                                .map_err(|e| NetError::Failed(e.to_string()))?;
                            self.store(url, 200, &body);
                            return Ok(body);
                        }
                        404 => {
                            self.store(url, 404, &[]);
                            return Err(NetError::NotFound);
                        }
                        429 | 500..=599 if attempt < 4 => {
                            let wait = resp
                                .headers()
                                .get("retry-after")
                                .and_then(|v| v.to_str().ok())
                                .and_then(|v| v.trim().parse::<u64>().ok())
                                .map(Duration::from_secs)
                                .unwrap_or_else(|| Duration::from_millis(1500 * 2u64.pow(attempt)));
                            self.push_back(&host, wait.min(Duration::from_secs(60)));
                            attempt += 1;
                        }
                        s => return Err(NetError::Failed(format!("HTTP {s} from {host}"))),
                    }
                }
                Err(e) if attempt < 3 => {
                    self.push_back(&host, Duration::from_millis(2000 * 2u64.pow(attempt)));
                    attempt += 1;
                    if attempt == 3 {
                        return Err(NetError::Failed(e.to_string()));
                    }
                }
                Err(e) => return Err(NetError::Failed(e.to_string())),
            }
        }
    }

    fn wait_turn(&self, host: &str) {
        let pace = pace_for(host);
        let wait = {
            let mut slots = self.next_slot.lock().expect("pace lock");
            let now = Instant::now();
            let at = slots.get(host).copied().unwrap_or(now).max(now);
            slots.insert(host.to_owned(), at + pace);
            at - now
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    fn push_back(&self, host: &str, by: Duration) {
        let mut slots = self.next_slot.lock().expect("pace lock");
        let at = Instant::now() + by;
        slots.insert(host.to_owned(), at);
    }

    fn cached(&self, url: &str) -> Option<(u16, Vec<u8>)> {
        let conn = self.cache.lock().ok()?;
        let row: Option<(i64, Option<Vec<u8>>, i64)> = conn
            .query_row(
                "SELECT status, body, fetched_at FROM http WHERE url = ?1",
                params![cache_key(url)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .ok()?;
        let (status, body, at) = row?;
        (now() - at < CACHE_TTL_SECS).then(|| (status as u16, body.unwrap_or_default()))
    }

    fn store(&self, url: &str, status: u16, body: &[u8]) {
        if let Ok(conn) = self.cache.lock() {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO http (url, status, body, fetched_at) VALUES (?1, ?2, ?3, ?4)",
                params![cache_key(url), status, body, now()],
            );
        }
    }
}

/// The address as the cache keeps it: without the user's keys (Discogs' `token`, AcoustID's
/// `client`), so no secret is ever written to disk with the answers.
fn cache_key(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_owned();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !matches!(p.split('=').next(), Some("token" | "client")))
        .collect();
    if kept.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Percent-encode a query value.
pub fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

#[cfg(test)]
mod key_tests {
    #[test]
    fn keys_never_reach_the_cache() {
        assert_eq!(
            super::cache_key("https://api.discogs.com/releases/1?token=SECRET"),
            "https://api.discogs.com/releases/1"
        );
        assert_eq!(
            super::cache_key("https://x/s?barcode=1&token=SECRET&per_page=5"),
            "https://x/s?barcode=1&per_page=5"
        );
        assert_eq!(super::cache_key("https://x/y?a=1"), "https://x/y?a=1");
    }
}
