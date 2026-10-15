//! Finding a song on another service, free: no account, no key. Used to send a playlist out
//! (Copy for Spotify, Apple Music links, Deezer) and to know a song better on the way in.
//!
//! - **Deezer**: by recording code (ISRC), else its search. Credits film songs to the composer,
//!   so a match rests on the title and length as much as the artist.
//! - **Apple Music**: Apple's iTunes Search (about 20 a minute).
//! - **Spotify**: ListenBrainz Labs (experimental; strong on Western music, weak on Indian).
//! - **MusicBrainz**: recordings by ISRC (1 a second).
//!
//! YouTube Music is found through Venuta's own catalogue search (`imports::catalog_online_for`).
//! Every answer is cached and each service paced (`net::Http`). Readers are kept apart from the
//! fetching so they're tested against saved answers (`tests/fixtures/bridges`).

use crate::net::{Http, NetError, enc};
use crate::playlists::Foreign;
use crate::songid::{Sureness, same_song};
use serde::Serialize;
use serde_json::Value;

/// The services a song can be looked for on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Spotify,
    AppleMusic,
    Deezer,
}

/// A song found on another service.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Elsewhere {
    pub service: Service,
    pub id: String,
    /// Where it opens (its page on that service).
    pub url: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    /// The same song; otherwise likely, for you to check.
    pub sure: bool,
    pub why: String,
}

/// Find `want` on `service`. `Ok(None)`: not there (or not found well enough to offer).
pub fn find(http: &Http, service: Service, want: &Foreign) -> Result<Option<Elsewhere>, String> {
    let err = |e: NetError| e.to_string();
    match service {
        Service::Deezer => {
            if let Some(isrc) = want.isrc.as_deref().filter(|i| i.trim().len() >= 12) {
                match http.get_json(&format!(
                    "https://api.deezer.com/track/isrc:{}",
                    enc(isrc.trim())
                )) {
                    Ok(v) => {
                        if let Some(found) = deezer_track(&v) {
                            return Ok(Some(found));
                        }
                    }
                    Err(NetError::NotFound) => {}
                    Err(e) => return Err(err(e)),
                }
            }
            let v = http
                .get_json(&format!(
                    "https://api.deezer.com/search?q={}&limit=10",
                    enc(&query(want))
                ))
                .map_err(err)?;
            Ok(deezer_search(&v, want))
        }
        Service::AppleMusic => {
            let v = http
                .get_json(&format!(
                    "https://itunes.apple.com/search?term={}&media=music&entity=song&limit=10",
                    enc(&query(want))
                ))
                .map_err(err)?;
            Ok(itunes_search(&v, want))
        }
        Service::Spotify => {
            let artist = crate::plan::split_names(&want.artist)
                .into_iter()
                .next()
                .unwrap_or_default();
            let v = http
                .get_json(&format!(
                    "https://labs.api.listenbrainz.org/spotify-id-from-metadata/json?artist_name={}&release_name={}&track_name={}",
                    enc(&artist),
                    enc(&want.album),
                    enc(&want.title)
                ))
                .map_err(err)?;
            Ok(listenbrainz_ids(&v, "spotify_track_ids")
                .into_iter()
                .next()
                .map(|id| Elsewhere {
                    service: Service::Spotify,
                    url: format!("https://open.spotify.com/track/{id}"),
                    id,
                    title: want.title.clone(),
                    artist: want.artist.clone(),
                    album: want.album.clone(),
                    duration_ms: want.duration_ms,
                    sure: true,
                    why: "ListenBrainz knows this song on Spotify".into(),
                }))
        }
    }
}

/// What a search is asked: the title with notes cut off, and the first artist.
fn query(want: &Foreign) -> String {
    let title = want
        .title
        .split(['(', '['])
        .next()
        .unwrap_or(&want.title)
        .trim();
    let artist = crate::plan::split_names(&want.artist)
        .into_iter()
        .next()
        .unwrap_or_default();
    format!("{title} {artist}").trim().to_owned()
}

fn s(v: Option<&Value>) -> String {
    v.and_then(|x| x.as_str()).unwrap_or_default().to_owned()
}

/// The best of several listings, if one is the song: sure first, then on your album (a song is
/// often listed on its single, its soundtrack and compilations), then the nearest in length, then
/// the service's own order.
fn best(
    want: &Foreign,
    listings: Vec<(Foreign, String, String)>,
    service: Service,
) -> Option<Elsewhere> {
    let album = crate::audit::name_key(&want.album);
    listings
        .into_iter()
        .enumerate()
        .filter_map(|(rank, (have, id, url))| {
            let (sure, why) = same_song(want, &have)?;
            // Your album named first ("Brahmastra (Original Soundtrack)") beats only mentioned
            // ("Kesariya (From Brahmastra) - Single").
            let theirs = crate::audit::name_key(&have.album);
            let on_album = match () {
                _ if album.is_empty() => 0,
                _ if theirs.starts_with(&album) => 2,
                _ if theirs.contains(&album) => 1,
                _ => 0,
            };
            let off = match (want.duration_ms, have.duration_ms) {
                (Some(a), Some(b)) => a.abs_diff(b),
                _ => u64::MAX,
            };
            Some((
                (
                    sure,
                    on_album,
                    std::cmp::Reverse(off),
                    std::cmp::Reverse(rank),
                ),
                have,
                id,
                url,
                why,
            ))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(rank, have, id, url, why)| (rank.0, have, id, url, why))
        .map(|(sure, have, id, url, why)| Elsewhere {
            service,
            id,
            url,
            title: have.title,
            artist: have.artist,
            album: have.album,
            duration_ms: have.duration_ms,
            sure: sure == Sureness::Sure,
            why: why.into(),
        })
}

/// A Deezer track (`/track/<id>` or `/track/isrc:<code>`): every name credited.
pub fn deezer_track(v: &Value) -> Option<Elsewhere> {
    if v.get("error").is_some() {
        return None;
    }
    let id = v.get("id")?.as_u64()?.to_string();
    let credited: Vec<String> = v
        .get("contributors")
        .and_then(|c| c.as_array())
        .map(|c| c.iter().map(|x| s(x.get("name"))).collect())
        .unwrap_or_default();
    Some(Elsewhere {
        service: Service::Deezer,
        url: s(v.get("link")),
        id,
        title: s(v.get("title")),
        artist: if credited.is_empty() {
            s(v.get("artist").and_then(|a| a.get("name")))
        } else {
            credited.join(", ")
        },
        album: s(v.get("album").and_then(|a| a.get("title"))),
        duration_ms: v.get("duration").and_then(|d| d.as_u64()).map(|d| d * 1000),
        sure: true,
        why: "Same recording code (ISRC)".into(),
    })
}

/// Deezer's search answer: the listing that's the song, if any.
pub fn deezer_search(v: &Value, want: &Foreign) -> Option<Elsewhere> {
    let listings = v
        .get("data")?
        .as_array()?
        .iter()
        .map(|t| {
            (
                Foreign {
                    title: s(t.get("title")),
                    artist: s(t.get("artist").and_then(|a| a.get("name"))),
                    album: s(t.get("album").and_then(|a| a.get("title"))),
                    duration_ms: t.get("duration").and_then(|d| d.as_u64()).map(|d| d * 1000),
                    ..Default::default()
                },
                t.get("id")
                    .and_then(|i| i.as_u64())
                    .unwrap_or_default()
                    .to_string(),
                s(t.get("link")),
            )
        })
        .collect();
    best(want, listings, Service::Deezer)
}

/// Apple's iTunes Search answer: the listing that's the song, if any.
pub fn itunes_search(v: &Value, want: &Foreign) -> Option<Elsewhere> {
    let listings = v
        .get("results")?
        .as_array()?
        .iter()
        .filter(|t| s(t.get("kind")) == "song")
        .map(|t| {
            (
                Foreign {
                    title: s(t.get("trackName")),
                    artist: s(t.get("artistName")),
                    album: s(t.get("collectionName")),
                    duration_ms: t.get("trackTimeMillis").and_then(|d| d.as_u64()),
                    ..Default::default()
                },
                t.get("trackId")
                    .and_then(|i| i.as_u64())
                    .unwrap_or_default()
                    .to_string(),
                s(t.get("trackViewUrl")),
            )
        })
        .collect();
    best(want, listings, Service::AppleMusic)
}

/// ListenBrainz Labs' answer: the ids under `key` (`spotify_track_ids`, `apple_music_track_ids`).
pub fn listenbrainz_ids(v: &Value, key: &str) -> Vec<String> {
    v.as_array()
        .and_then(|a| a.first())
        .and_then(|x| x.get(key))
        .and_then(|ids| ids.as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// MusicBrainz recordings for an ISRC: (recording id, title, length).
pub fn musicbrainz_recordings(v: &Value) -> Vec<(String, String, Option<u64>)> {
    v.get("recordings")
        .and_then(|r| r.as_array())
        .map(|r| {
            r.iter()
                .map(|x| {
                    (
                        s(x.get("id")),
                        s(x.get("title")),
                        x.get("length").and_then(|l| l.as_u64()),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bridges")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn kesariya() -> Foreign {
        Foreign {
            title: "Kesariya".into(),
            artist: "Arijit Singh".into(),
            album: "Brahmastra".into(),
            duration_ms: Some(268_000),
            ..Default::default()
        }
    }

    #[test]
    fn deezer_by_recording_code_is_sure_and_credits_everyone() {
        let found = deezer_track(&saved("deezer-isrc.json")).unwrap();
        assert_eq!(found.id, "1828361287");
        assert!(found.sure);
        assert!(found.artist.contains("Arijit Singh"), "{}", found.artist);
        assert!(found.url.starts_with("https://www.deezer.com/"));
    }

    #[test]
    fn deezer_search_finds_the_song_not_its_remixes() {
        let found = deezer_search(&saved("deezer-search.json"), &kesariya()).unwrap();
        assert_eq!(found.id, "1828361287");
        // Credited to the composer only: offered, for you to check.
        assert!(!found.sure);
    }

    #[test]
    fn apple_music_by_itunes_search() {
        let found = itunes_search(&saved("itunes-search.json"), &kesariya()).unwrap();
        // On the Brahmastra soundtrack, as in the library (also listed on its single and compilations).
        assert_eq!(found.id, "1648663570");
        assert!(found.sure);
        assert!(found.url.contains("music.apple.com"), "{}", found.url);
    }

    #[test]
    fn spotify_ids_from_listenbrainz_when_it_knows_the_song() {
        assert_eq!(
            listenbrainz_ids(&saved("lb-spotify-western.json"), "spotify_track_ids").len(),
            4
        );
        assert!(listenbrainz_ids(&saved("lb-spotify.json"), "spotify_track_ids").is_empty());
    }

    #[test]
    fn musicbrainz_knows_the_recording_by_its_code() {
        let r = musicbrainz_recordings(&saved("musicbrainz-isrc.json"));
        assert_eq!(r[0].0, "393ad701-a13d-4f1c-9e3f-65dfd7cac167");
        assert_eq!(r[0].2, Some(268_000));
    }

    #[test]
    fn searches_ask_for_the_title_without_notes() {
        let want = Foreign {
            title: "Kesariya (From \"Brahmastra\")".into(),
            artist: "Arijit Singh, Pritam".into(),
            ..Default::default()
        };
        assert_eq!(query(&want), "Kesariya Arijit Singh");
    }
}
