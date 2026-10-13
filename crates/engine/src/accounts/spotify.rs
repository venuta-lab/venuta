//! Spotify's Web API, read only: who you are, your playlists, their songs and your Liked Songs.
//! Written for the API as it is after the February 2026 changes (`/items`, `item`), paging at
//! 50 and waiting when asked to (429 + Retry-After). Nothing personal is cached.

use super::oauth::Provider;
use crate::imports::ForeignPlaylist;
use crate::playlists::Foreign;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const API: &str = "https://api.spotify.com/v1";

pub fn provider() -> Provider {
    Provider {
        name: "Spotify".into(),
        authorize_url: "https://accounts.spotify.com/authorize".into(),
        token_url: "https://accounts.spotify.com/api/token".into(),
        scopes: vec![
            "playlist-read-private".into(),
            "playlist-read-collaborative".into(),
            "user-library-read".into(),
        ],
        redirect_path: "/callback".into(),
    }
}

/// The id used for Liked Songs, beside real playlist ids.
pub const LIKED: &str = "liked";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpotifyPlaylist {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub songs: u64,
    /// Spotify shares the songs only of playlists you made or collaborate on.
    pub readable: bool,
    pub image: Option<String>,
}

#[derive(Debug)]
pub enum ApiError {
    /// The access token has run out or was revoked: refresh, or sign in again.
    Unauthorized,
    Failed(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "Spotify asked to sign in again."),
            ApiError::Failed(e) => write!(f, "{e}"),
        }
    }
}

pub struct Api {
    agent: ureq::Agent,
    token: String,
    base: String,
}

impl Api {
    pub fn new(token: &str) -> Self {
        Self::with_base(token, API)
    }

    pub fn with_base(token: &str, base: &str) -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .user_agent(crate::net::USER_AGENT)
                .http_status_as_error(false)
                .build()
                .into(),
            token: token.to_owned(),
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    /// GET a path (or a full `next` address), waiting when Spotify asks to.
    fn get(&self, path_or_url: &str) -> Result<serde_json::Value, ApiError> {
        let url = if path_or_url.starts_with("http") {
            path_or_url.to_owned()
        } else {
            format!("{}{path_or_url}", self.base)
        };
        for _ in 0..4 {
            let mut resp = self
                .agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/json")
                .call()
                .map_err(|e| ApiError::Failed(format!("Couldn't reach Spotify: {e}")))?;
            match resp.status().as_u16() {
                200 => return resp.body_mut().read_json().map_err(|e| ApiError::Failed(format!("Spotify answered in an unexpected way: {e}"))),
                401 => return Err(ApiError::Unauthorized),
                429 => {
                    // Wait as long as asked (at most a minute), then try again.
                    let secs: u64 = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok()).unwrap_or(2);
                    std::thread::sleep(Duration::from_secs(secs.min(60)));
                }
                500 | 502 | 503 | 504 => std::thread::sleep(Duration::from_secs(2)),
                403 => return Err(ApiError::Failed("Spotify didn't allow this. Check that your Spotify account is added as a user of your app (Spotify for Developers, your app, User management).".into())),
                s => return Err(ApiError::Failed(format!("Spotify answered HTTP {s}."))),
            }
        }
        Err(ApiError::Failed(
            "Spotify is busy; try again in a minute.".into(),
        ))
    }

    /// Every page of a paged list, items in order.
    fn all(&self, first: &str) -> Result<Vec<serde_json::Value>, ApiError> {
        let mut out = Vec::new();
        let mut next = Some(first.to_owned());
        while let Some(url) = next.take() {
            let page = self.get(&url)?;
            out.extend(
                page.get("items")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
            );
            next = page.get("next").and_then(|v| v.as_str()).map(str::to_owned);
        }
        Ok(out)
    }

    /// Your Spotify id and the name to show.
    pub fn me(&self) -> Result<(String, String), ApiError> {
        let me = self.get("/me")?;
        let id = me
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let name = me
            .get("display_name")
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(&id)
            .to_owned();
        Ok((id, name))
    }

    /// Your playlists, Liked Songs first.
    pub fn playlists(&self, me: &str) -> Result<Vec<SpotifyPlaylist>, ApiError> {
        let liked = self.get("/me/tracks?limit=1")?;
        let mut out = vec![SpotifyPlaylist {
            id: LIKED.into(),
            name: "Liked Songs".into(),
            owner: me.into(),
            songs: liked.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
            readable: true,
            image: None,
        }];
        out.extend(
            self.all("/me/playlists?limit=50")?
                .iter()
                .filter_map(|p| playlist(p, me)),
        );
        Ok(out)
    }

    /// The songs of one playlist (or Liked Songs), in order.
    pub fn songs(&self, id: &str) -> Result<Vec<Foreign>, ApiError> {
        let items = if id == LIKED {
            self.all("/me/tracks?limit=50")?
        } else {
            self.all(&format!(
                "/playlists/{}/items?limit=50&additional_types=track",
                crate::net::enc(id)
            ))?
        };
        Ok(items.iter().filter_map(song).collect())
    }

    /// The chosen playlists, read for import.
    pub fn read(&self, chosen: &[SpotifyPlaylist]) -> Result<Vec<ForeignPlaylist>, ApiError> {
        chosen
            .iter()
            .map(|p| {
                Ok(ForeignPlaylist {
                    name: p.name.clone(),
                    source: "Spotify".into(),
                    tracks: self.songs(&p.id)?,
                })
            })
            .collect()
    }
}

/// One entry of `/me/playlists`. Spotify renamed `tracks` to `items` (a count object either way).
fn playlist(p: &serde_json::Value, me: &str) -> Option<SpotifyPlaylist> {
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
    let owner = p.get("owner").and_then(|o| s(o, "id")).unwrap_or_default();
    let count = p
        .get("items")
        .or(p.get("tracks"))
        .and_then(|t| t.get("total"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let collaborative = p
        .get("collaborative")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(SpotifyPlaylist {
        id: s(p, "id")?,
        name: s(p, "name").unwrap_or_else(|| "Untitled".into()),
        readable: owner == me || collaborative,
        owner: p
            .get("owner")
            .and_then(|o| s(o, "display_name"))
            .unwrap_or(owner),
        songs: count,
        image: p
            .get("images")
            .and_then(|i| i.as_array())
            .and_then(|i| i.last())
            .and_then(|i| s(i, "url")),
    })
}

/// One playlist entry or Liked Song as a song to match: the track is under `item` (since
/// February 2026) or `track`. Podcast episodes are left out; local files keep their tags.
fn song(entry: &serde_json::Value) -> Option<Foreign> {
    let t = entry
        .get("item")
        .or(entry.get("track"))
        .filter(|t| !t.is_null())?;
    if t.get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|k| k != "track")
    {
        return None;
    }
    let s = |k: &str| {
        t.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let artists: Vec<String> = t
        .get("artists")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(Foreign {
        title: s("name"),
        artist: artists.join(", "),
        album: t
            .get("album")
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        duration_ms: t.get("duration_ms").and_then(|v| v.as_u64()),
        isrc: t
            .get("external_ids")
            .and_then(|e| e.get("isrc"))
            .and_then(|v| v.as_str())
            .map(|i| i.to_ascii_uppercase()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[test]
    fn playlists_say_which_ones_spotify_will_share() {
        let mine = json!({"id":"p1","name":"Drive","owner":{"id":"me","display_name":"Akash"},"collaborative":false,"items":{"total":12},"images":[{"url":"big"},{"url":"small"}]});
        let shared = json!({"id":"p2","name":"Together","owner":{"id":"friend"},"collaborative":true,"tracks":{"total":3}});
        let followed = json!({"id":"p3","name":"Top 50","owner":{"id":"spotify","display_name":"Spotify"},"collaborative":false,"items":{"total":50}});
        let p = playlist(&mine, "me").unwrap();
        assert_eq!(
            (p.readable, p.songs, p.owner.as_str(), p.image.as_deref()),
            (true, 12, "Akash", Some("small"))
        );
        assert!(playlist(&shared, "me").unwrap().readable);
        let f = playlist(&followed, "me").unwrap();
        assert!(!f.readable);
        assert_eq!(f.owner, "Spotify");
    }

    #[test]
    fn songs_read_from_item_or_track_with_isrc_and_skip_episodes() {
        let new = json!({"added_at":"x","item":{"type":"track","name":"Kun Faya Kun","duration_ms":472000,"artists":[{"name":"A.R. Rahman"},{"name":"Javed Ali"}],"album":{"name":"Rockstar"},"external_ids":{"isrc":"inz031100123"}}});
        let old = json!({"track":{"name":"Tum Tak","artists":[{"name":"A.R. Rahman"}],"album":{"name":"Raanjhanaa"},"duration_ms":304000}});
        let episode = json!({"item":{"type":"episode","name":"A podcast"}});
        let gone = json!({"item":null});
        let a = song(&new).unwrap();
        assert_eq!(
            (a.title.as_str(), a.artist.as_str(), a.album.as_str()),
            ("Kun Faya Kun", "A.R. Rahman, Javed Ali", "Rockstar")
        );
        assert_eq!(
            (a.duration_ms, a.isrc.as_deref()),
            (Some(472000), Some("INZ031100123"))
        );
        assert_eq!(song(&old).unwrap().title, "Tum Tak");
        assert!(song(&episode).is_none());
        assert!(song(&gone).is_none());
    }

    /// A tiny Spotify: answers each request from a list, in order, and records what it was asked.
    fn server(
        answers: Vec<(u16, &'static str, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}/v1", l.local_addr().unwrap().port());
        let b2 = base.clone();
        let h = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for (status, extra, body) in answers {
                let (mut s, _) = l.accept().unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.to_ascii_lowercase().starts_with("authorization:") {
                        auth = h.trim().to_owned();
                    }
                    if h == "\r\n" {
                        break;
                    }
                }
                asked.push(format!(
                    "{} {}",
                    line.split_whitespace().nth(1).unwrap(),
                    auth
                ));
                let body = body.replace("{BASE}", &b2);
                write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            asked
        });
        (base, h)
    }

    #[test]
    fn pages_are_followed_and_a_429_is_waited_out() {
        let page1 = json!({"items":[{"item":{"name":"One","artists":[{"name":"X"}],"album":{"name":"A"}}}],"next":"{BASE}/playlists/p1/items?offset=50&limit=50"}).to_string();
        let page2 = json!({"items":[{"item":{"name":"Two","artists":[{"name":"Y"}],"album":{"name":"B"}}}],"next":null}).to_string();
        let (base, h) = server(vec![
            (429, "Retry-After: 1\r\n", "{}".into()),
            (200, "", page1),
            (200, "", page2),
        ]);
        let api = Api::with_base("TOKEN", &base);
        let songs = api.songs("p1").unwrap();
        assert_eq!(
            songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            vec!["One", "Two"]
        );
        let asked = h.join().unwrap();
        assert_eq!(asked.len(), 3);
        assert!(asked[0].starts_with("/v1/playlists/p1/items?limit=50&additional_types=track"));
        assert!(asked.iter().all(|a| a.ends_with("Bearer TOKEN")));
        assert!(asked[2].starts_with("/v1/playlists/p1/items?offset=50"));
    }

    #[test]
    fn an_expired_token_is_reported_as_unauthorized() {
        let (base, h) = server(vec![(401, "", r#"{"error":{"status":401}}"#.into())]);
        assert!(matches!(
            Api::with_base("old", &base).me(),
            Err(ApiError::Unauthorized)
        ));
        h.join().unwrap();
    }
}
