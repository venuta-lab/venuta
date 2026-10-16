//! The OpenSubsonic methods phone apps use, answered from the catalog. Read-only: a phone can
//! play, download, search and scrobble, never change the library.

use crate::App;
use crate::catalog::{Album, Artist, Catalog, Song};
use crate::respond::{self, Code, Format};
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use engine::library::Library;
use serde_json::{Map, Value, json};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

/// Request parameters, from the address and (formPost) the body; a key may repeat.
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    }
    fn num(&self, k: &str, default: usize) -> usize {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
}

enum Out {
    Ok(Value),
    Err(Code, String),
    File(PathBuf),
}

fn missing(what: &str) -> Out {
    Out::Err(
        Code::MissingParam,
        format!("Required parameter is missing: {what}"),
    )
}

fn not_found(what: &str) -> Out {
    Out::Err(Code::NotFound, format!("{what} not found"))
}

pub async fn rest(
    State(app): State<Arc<App>>,
    Path(method): Path<String>,
    req: Request,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    if !peer.is_some_and(crate::lan::is_lan) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (parts, body) = req.into_parts();
    let mut pairs: Vec<(String, String)> = parts
        .uri
        .query()
        .and_then(|q| serde_urlencoded::from_str(q).ok())
        .unwrap_or_default();
    let form = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    if parts.method == Method::POST
        && form
        && let Ok(bytes) = axum::body::to_bytes(body, 64 * 1024).await
    {
        pairs.extend(
            serde_urlencoded::from_bytes::<Vec<(String, String)>>(&bytes).unwrap_or_default(),
        );
    }
    let p = Params(pairs);
    let f = Format::from_param(p.get("f"));
    let phones = app.phones();
    let Ok(phone) = crate::auth::check(&phones, |k| p.get(k)) else {
        return respond::error(f, Code::WrongCredentials, "Wrong username or password.");
    };
    let user = phone.user.clone();
    let method = method.trim_end_matches(".view").to_owned();

    let app2 = app.clone();
    let out = tokio::task::spawn_blocking(move || -> Out {
        match app2.snapshot() {
            Ok((lib, cat)) => answer(&app2, &lib, &cat, &method, &p, &user),
            Err(e) => Out::Err(Code::Generic, e),
        }
    })
    .await
    .unwrap_or_else(|e| Out::Err(Code::Generic, e.to_string()));

    match out {
        Out::Ok(v) => respond::ok(f, v),
        Out::Err(code, m) => respond::error(f, code, &m),
        Out::File(path) => {
            // The file as it is, with ranges for seeking (and a type from its extension).
            let req = Request::from_parts(parts, Body::empty());
            match tower_http::services::ServeFile::new(path)
                .oneshot(req)
                .await
            {
                Ok(r) => r.map(Body::new),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
    }
}

fn answer(app: &App, lib: &Library, c: &Catalog, method: &str, p: &Params, user: &str) -> Out {
    let root = app.root();
    match method {
        "ping" => Out::Ok(json!({})),
        "getLicense" => Out::Ok(json!({ "license": { "valid": true } })),
        "getOpenSubsonicExtensions" => Out::Ok(json!({ "openSubsonicExtensions": [
            { "name": "songLyrics", "versions": [1] },
            { "name": "formPost", "versions": [1] },
        ]})),
        "getMusicFolders" => {
            Out::Ok(json!({ "musicFolders": { "musicFolder": [ { "id": 1, "name": "Music" } ] } }))
        }
        "getUser" => Out::Ok(json!({ "user": {
            "username": user, "scrobblingEnabled": true, "adminRole": false, "settingsRole": false,
            "downloadRole": true, "uploadRole": false, "playlistRole": false, "coverArtRole": false,
            "commentRole": false, "podcastRole": false, "streamRole": true, "jukeboxRole": false,
            "shareRole": false, "videoConversionRole": false, "folder": [1],
        }})),
        "getScanStatus" | "startScan" => {
            Out::Ok(json!({ "scanStatus": { "scanning": false, "count": c.songs.len() } }))
        }
        "getArtists" => Out::Ok(json!({ "artists": {
            "ignoredArticles": "The",
            "index": by_letter(c, |a| artist_json(c, a)),
        }})),
        "getIndexes" => Out::Ok(json!({ "indexes": {
            "lastModified": 0,
            "ignoredArticles": "The",
            "index": by_letter(c, |a| json!({ "id": a.id, "name": a.name })),
        }})),
        "getArtist" => match p.get("id") {
            None => missing("id"),
            Some(id) => match c.artist(id) {
                None => not_found("Artist"),
                Some(a) => {
                    let mut v = artist_json(c, a);
                    v["album"] = a.albums.iter().map(|&i| album_json(&c.albums[i])).collect();
                    Out::Ok(json!({ "artist": v }))
                }
            },
        },
        "getAlbum" => match p.get("id") {
            None => missing("id"),
            Some(id) => match c.album(id) {
                None => not_found("Album"),
                Some(a) => {
                    let mut v = album_json(a);
                    v["song"] = a.songs.iter().map(|&i| song_json(&c.songs[i])).collect();
                    Out::Ok(json!({ "album": v }))
                }
            },
        },
        // Folder browsing, for apps that browse that way: an artist holds albums, an album songs.
        "getMusicDirectory" => match p.get("id") {
            None => missing("id"),
            Some(id) => {
                if let Some(a) = c.artist(id) {
                    let child: Vec<Value> =
                        a.albums.iter().map(|&i| album_json(&c.albums[i])).collect();
                    Out::Ok(json!({ "directory": { "id": a.id, "name": a.name, "child": child } }))
                } else if let Some(a) = c.album(id) {
                    let child: Vec<Value> =
                        a.songs.iter().map(|&i| song_json(&c.songs[i])).collect();
                    Out::Ok(
                        json!({ "directory": { "id": a.id, "name": a.name, "parent": a.artist_id, "child": child } }),
                    )
                } else {
                    not_found("Directory")
                }
            }
        },
        "getSong" => match p.get("id").map(|id| c.song(id)) {
            None => missing("id"),
            Some(None) => not_found("Song"),
            Some(Some(s)) => Out::Ok(json!({ "song": song_json(s) })),
        },
        "getAlbumList2" | "getAlbumList" => {
            let key = if method == "getAlbumList2" {
                "albumList2"
            } else {
                "albumList"
            };
            match album_list(c, p) {
                Ok(list) => Out::Ok(
                    json!({ key: { "album": list.into_iter().map(album_json).collect::<Vec<_>>() } }),
                ),
                Err(o) => o,
            }
        }
        "getRandomSongs" => {
            let size = p.num("size", 10).min(500);
            let picked = shuffled(c.songs.len()).into_iter().take(size);
            Out::Ok(
                json!({ "randomSongs": { "song": picked.map(|i| song_json(&c.songs[i])).collect::<Vec<_>>() } }),
            )
        }
        "search3" | "search2" => {
            let key = if method == "search3" {
                "searchResult3"
            } else {
                "searchResult2"
            };
            Out::Ok(json!({ key: search(c, p) }))
        }
        "getGenres" => Out::Ok(json!({ "genres": { "genre": genres(c) } })),
        "getSongsByGenre" => match p.get("genre") {
            None => missing("genre"),
            Some(g) => {
                let list: Vec<Value> = c
                    .songs
                    .iter()
                    .filter(|s| {
                        s.genre
                            .as_deref()
                            .is_some_and(|x| x.eq_ignore_ascii_case(g))
                    })
                    .skip(p.num("offset", 0))
                    .take(p.num("count", 10).min(500))
                    .map(song_json)
                    .collect();
                Out::Ok(json!({ "songsByGenre": { "song": list } }))
            }
        },
        "getPlaylists" => {
            Out::Ok(json!({ "playlists": { "playlist": playlists(&root, lib, c, user) } }))
        }
        "getPlaylist" => match p.get("id") {
            None => missing("id"),
            Some(id) => match playlist(&root, lib, c, user, id) {
                Some(v) => Out::Ok(json!({ "playlist": v })),
                None => not_found("Playlist"),
            },
        },
        "getLyricsBySongId" => match p.get("id").map(|id| c.song(id)) {
            None => missing("id"),
            Some(None) => not_found("Song"),
            Some(Some(s)) => {
                Out::Ok(json!({ "lyricsList": { "structuredLyrics": lyrics(&root, lib, s) } }))
            }
        },
        "getLyrics" => {
            let (artist, title) = (p.get("artist").unwrap_or(""), p.get("title").unwrap_or(""));
            let found = c.songs.iter().find(|s| {
                s.title.eq_ignore_ascii_case(title)
                    && (artist.is_empty()
                        || s.artist.to_lowercase().contains(&artist.to_lowercase()))
            });
            let text = found
                .and_then(|s| lyrics(&root, lib, s).into_iter().next())
                .map(|l| {
                    l["line"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x["value"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            Out::Ok(json!({ "lyrics": { "artist": artist, "title": title, "value": text } }))
        }
        "getStarred2" => Out::Ok(json!({ "starred2": {} })),
        "getStarred" => Out::Ok(json!({ "starred": {} })),
        "getNowPlaying" => Out::Ok(json!({ "nowPlaying": {} })),
        "getPlayQueue" | "savePlayQueue" | "star" | "unstar" | "setRating" => Out::Ok(json!({})),
        "scrobble" => {
            // Only finished plays count ("submission" left out means true).
            if p.get("submission") != Some("false") {
                for id in p.0.iter().filter(|(k, _)| k == "id").map(|(_, v)| v) {
                    if let Some(s) = c.song(id) {
                        app.played(&s.path);
                    }
                }
            }
            Out::Ok(json!({}))
        }
        "stream" | "download" => match p.get("id").map(|id| c.song(id)) {
            None => missing("id"),
            Some(None) => not_found("Song"),
            Some(Some(s)) => Out::File(root.join(&s.path)),
        },
        "getCoverArt" => match p.get("id") {
            None => missing("id"),
            Some(id) => match cover(&root, c, id, p.get("size").and_then(|s| s.parse().ok())) {
                Some(path) => Out::File(path),
                None => not_found("Cover art"),
            },
        },
        // What apps ask for beside the library: notes from the web, similar and top songs,
        // podcasts, radio, bookmarks, shares. Venuta has none of these, so each is answered
        // empty, never with an error that would fail the screen asking.
        "getAlbumInfo" | "getAlbumInfo2" => Out::Ok(json!({ "albumInfo": {} })),
        "getArtistInfo" => Out::Ok(json!({ "artistInfo": {} })),
        "getArtistInfo2" => Out::Ok(json!({ "artistInfo2": {} })),
        "getSimilarSongs" => Out::Ok(json!({ "similarSongs": {} })),
        "getSimilarSongs2" => Out::Ok(json!({ "similarSongs2": {} })),
        "getTopSongs" => Out::Ok(json!({ "topSongs": {} })),
        "getPodcasts" => Out::Ok(json!({ "podcasts": {} })),
        "getNewestPodcasts" => Out::Ok(json!({ "newestPodcasts": {} })),
        "getInternetRadioStations" => Out::Ok(json!({ "internetRadioStations": {} })),
        "getBookmarks" => Out::Ok(json!({ "bookmarks": {} })),
        "getShares" => Out::Ok(json!({ "shares": {} })),
        other => Out::Err(Code::Generic, format!("Venuta doesn't answer {other} yet.")),
    }
}

/// A JSON object without its empty values (apps read a missing number better than a null).
fn clean(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(_, v)| !v.is_null())
                .collect::<Map<_, _>>(),
        ),
        other => other,
    }
}

fn content_type(suffix: &str) -> &'static str {
    match suffix {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" | "alac" | "mp4" => "audio/mp4",
        "ogg" | "opus" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "aiff" | "aif" => "audio/aiff",
        _ => "application/octet-stream",
    }
}

pub fn song_json(s: &Song) -> Value {
    let secs = s.duration_ms / 1000;
    let kbps = (s.size * 8 / 1000).checked_div(secs.max(1)).unwrap_or(0);
    clean(json!({
        "id": s.id, "parent": s.album_id, "isDir": false, "title": s.title, "album": s.album,
        "artist": s.artist, "track": s.track, "discNumber": s.disc, "year": s.year, "genre": s.genre,
        "coverArt": s.album_id, "size": s.size, "contentType": content_type(&s.suffix),
        "suffix": s.suffix, "duration": secs, "bitRate": kbps, "path": s.path, "type": "music",
        "mediaType": "song", "isVideo": false, "albumId": s.album_id, "artistId": s.artist_id,
        "samplingRate": s.sample_rate, "bitDepth": s.bit_depth,
    }))
}

pub fn album_json(a: &Album) -> Value {
    clean(json!({
        "id": a.id, "name": a.name, "title": a.name, "album": a.name, "isDir": true,
        "parent": a.artist_id, "artist": a.artist, "artistId": a.artist_id, "coverArt": a.id,
        "songCount": a.songs.len(), "duration": a.duration_ms / 1000, "year": a.year,
        "genre": a.genre, "created": "2000-01-01T00:00:00Z",
    }))
}

fn artist_json(c: &Catalog, a: &Artist) -> Value {
    json!({
        "id": a.id, "name": a.name, "albumCount": a.albums.len(),
        "coverArt": a.albums.first().map(|&i| c.albums[i].id.clone()),
    })
}

/// Artists under the letter they start with ("The" set aside), as Subsonic indexes them.
fn by_letter(c: &Catalog, each: impl Fn(&Artist) -> Value) -> Vec<Value> {
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for a in &c.artists {
        let name = a.name.strip_prefix("The ").unwrap_or(&a.name);
        let first = name
            .chars()
            .next()
            .map_or('#', |ch| ch.to_ascii_uppercase());
        let letter = if first.is_ascii_alphabetic() {
            first.to_string()
        } else {
            "#".into()
        };
        match groups.iter_mut().find(|(l, _)| *l == letter) {
            Some((_, list)) => list.push(each(a)),
            None => groups.push((letter, vec![each(a)])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
        .into_iter()
        .map(|(name, artist)| json!({ "name": name, "artist": artist }))
        .collect()
}

fn album_list<'a>(c: &'a Catalog, p: &Params) -> Result<Vec<&'a Album>, Out> {
    let kind = p.get("type").ok_or_else(|| missing("type"))?;
    let size = p.num("size", 10).min(500);
    let offset = p.num("offset", 0);
    let mut list: Vec<&Album> = c.albums.iter().collect();
    match kind {
        "alphabeticalByName" => {}
        "alphabeticalByArtist" => list.sort_by_key(|a| a.artist.to_lowercase()),
        "newest" | "recent" | "frequent" | "highest" => {
            list.sort_by_key(|a| std::cmp::Reverse(a.added))
        }
        "random" => {
            let order = shuffled(list.len());
            list = order.into_iter().map(|i| &c.albums[i]).collect();
        }
        "starred" => list.clear(),
        "byYear" => {
            let from: u32 = p.get("fromYear").and_then(|v| v.parse().ok()).unwrap_or(0);
            let to: u32 = p.get("toYear").and_then(|v| v.parse().ok()).unwrap_or(9999);
            let (lo, hi) = (from.min(to), from.max(to));
            list.retain(|a| a.year.is_some_and(|y| (lo..=hi).contains(&y)));
            list.sort_by_key(|a| a.year);
            if from > to {
                list.reverse();
            }
        }
        "byGenre" => {
            let g = p.get("genre").ok_or_else(|| missing("genre"))?;
            list.retain(|a| {
                a.genre
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(g))
            });
        }
        other => {
            return Err(Out::Err(
                Code::Generic,
                format!("Unknown list type: {other}"),
            ));
        }
    }
    Ok(list.into_iter().skip(offset).take(size).collect())
}

/// Every index from 0 to n in an order that changes each time it's asked.
fn shuffled(n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut seed = [0u8; 8];
    let _ = getrandom::fill(&mut seed);
    let mut x = u64::from_le_bytes(seed) | 1;
    for i in (1..order.len()).rev() {
        // xorshift: enough to shuffle a song list.
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        order.swap(i, (x % (i as u64 + 1)) as usize);
    }
    order
}

/// Words in any order, in the title, artist or album; an empty query is everything (apps ask
/// that way to copy the whole library).
fn search(c: &Catalog, p: &Params) -> Value {
    let q = p
        .get("query")
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let has = |hay: &str| {
        let hay = hay.to_lowercase();
        words.iter().all(|w| hay.contains(w))
    };
    let page = |kind: &str| {
        (
            p.num(&format!("{kind}Offset"), 0),
            p.num(&format!("{kind}Count"), 20).min(10_000),
        )
    };
    let (ao, an) = page("artist");
    let (lo, ln) = page("album");
    let (so, sn) = page("song");
    let artists: Vec<Value> = c
        .artists
        .iter()
        .filter(|a| has(&a.name))
        .skip(ao)
        .take(an)
        .map(|a| artist_json(c, a))
        .collect();
    let albums: Vec<Value> = c
        .albums
        .iter()
        .filter(|a| has(&format!("{} {}", a.name, a.artist)))
        .skip(lo)
        .take(ln)
        .map(album_json)
        .collect();
    let songs: Vec<Value> = c
        .songs
        .iter()
        .filter(|s| has(&format!("{} {} {}", s.title, s.artist, s.album)))
        .skip(so)
        .take(sn)
        .map(song_json)
        .collect();
    json!({ "artist": artists, "album": albums, "song": songs })
}

fn genres(c: &Catalog) -> Vec<Value> {
    let mut counts: Vec<(String, usize, usize)> = Vec::new();
    for s in &c.songs {
        if let Some(g) = &s.genre {
            match counts.iter_mut().find(|(n, ..)| n.eq_ignore_ascii_case(g)) {
                Some(e) => e.1 += 1,
                None => counts.push((g.clone(), 1, 0)),
            }
        }
    }
    for a in &c.albums {
        if let Some(e) = a
            .genre
            .as_ref()
            .and_then(|g| counts.iter_mut().find(|(n, ..)| n.eq_ignore_ascii_case(g)))
        {
            e.2 += 1;
        }
    }
    counts.sort_by_key(|c| c.0.to_lowercase());
    counts
        .into_iter()
        .map(|(g, songs, albums)| json!({ "value": g, "songCount": songs, "albumCount": albums }))
        .collect()
}

fn playlist_id(name: &str) -> String {
    format!(
        "pl{}",
        name.bytes().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn playlist_name(id: &str) -> Option<String> {
    let hex = id.strip_prefix("pl")?;
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

/// A playlist's songs that are files in the library (online and missing songs stay behind).
fn playlist_songs<'a>(
    root: &std::path::Path,
    lib: &Library,
    c: &'a Catalog,
    name: &str,
) -> Option<Vec<&'a Song>> {
    let (entries, _) = engine::playlists::read(root, lib, name).ok()?;
    Some(
        entries
            .iter()
            .filter(|e| !e.missing)
            .filter_map(|e| c.by_path(&e.path))
            .collect(),
    )
}

fn playlist_head(name: &str, songs: &[&Song], user: &str) -> Value {
    json!({
        "id": playlist_id(name), "name": name, "songCount": songs.len(),
        "duration": songs.iter().map(|s| s.duration_ms / 1000).sum::<u64>(),
        "public": false, "owner": user, "created": "2000-01-01T00:00:00Z", "changed": "2000-01-01T00:00:00Z",
        "coverArt": songs.first().map(|s| s.album_id.clone()),
    })
}

fn playlists(root: &std::path::Path, lib: &Library, c: &Catalog, user: &str) -> Vec<Value> {
    engine::playlists::list(root, lib)
        .into_iter()
        .filter_map(|p| {
            let songs = playlist_songs(root, lib, c, &p.name)?;
            (!songs.is_empty()).then(|| playlist_head(&p.name, &songs, user))
        })
        .collect()
}

fn playlist(
    root: &std::path::Path,
    lib: &Library,
    c: &Catalog,
    user: &str,
    id: &str,
) -> Option<Value> {
    let name = playlist_name(id)?;
    let songs = playlist_songs(root, lib, c, &name)?;
    let mut v = playlist_head(&name, &songs, user);
    v["entry"] = songs.iter().map(|s| song_json(s)).collect();
    Some(v)
}

/// The song's lyrics: its `.lrc` file, else what Venuta found online, else the lyrics in its
/// tags. Timed lines carry their start in milliseconds.
fn lyrics(root: &std::path::Path, lib: &Library, s: &Song) -> Vec<Value> {
    let stem = s.path.rsplit_once('.').map_or(s.path.as_str(), |x| x.0);
    let text = std::fs::read_to_string(root.join(format!("{stem}.lrc")))
        .ok()
        .or_else(|| {
            lib.fetched_lyrics
                .get(&s.track_id)
                .map(|f| f.synced.clone())
        })
        .or_else(|| {
            let t = lib.tracks.iter().find(|t| t.id == s.track_id)?;
            ["LYRICS", "UNSYNCEDLYRICS"]
                .iter()
                .find_map(|k| t.tags.get(*k)?.first().cloned())
        });
    let Some(text) = text else { return Vec::new() };
    let timed = parse_lrc(&text);
    let lines: Vec<Value> = if timed.is_empty() {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| json!({ "value": l }))
            .collect()
    } else {
        timed
            .iter()
            .map(|(ms, l)| json!({ "start": ms, "value": l }))
            .collect()
    };
    if lines.is_empty() {
        return Vec::new();
    }
    vec![json!({
        "displayArtist": s.artist, "displayTitle": s.title, "lang": "xxx",
        "synced": !timed.is_empty(), "line": lines,
    })]
}

/// `[mm:ss.xx]` lines, in time order; tags like `[ar:…]` and word timings left out.
pub fn parse_lrc(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(inner) = rest.strip_prefix('[') {
            let Some(end) = inner.find(']') else { break };
            let tag = &inner[..end];
            let Some((m, s)) = tag.split_once(':') else {
                break;
            };
            match (m.trim().parse::<u64>(), s.trim().parse::<f64>()) {
                (Ok(m), Ok(s)) => stamps.push(m * 60_000 + (s * 1000.0).round() as u64),
                _ => break,
            }
            rest = &inner[end + 1..];
        }
        if stamps.is_empty() {
            continue;
        }
        // Word timings ("<00:12.30>") are for karaoke; apps show a line at a time.
        let mut text = String::new();
        let mut skip = false;
        for ch in rest.chars() {
            match ch {
                '<' => skip = true,
                '>' if skip => skip = false,
                c if !skip => text.push(c),
                _ => {}
            }
        }
        let text = text.trim().to_owned();
        for ms in stamps {
            out.push((ms, text.clone()));
        }
    }
    out.sort_by_key(|(ms, _)| *ms);
    out
}

/// The picture file for anything with a cover: a song, an album, an artist or a playlist.
fn cover(root: &std::path::Path, c: &Catalog, id: &str, size: Option<u32>) -> Option<PathBuf> {
    let song = c
        .song(id)
        .or_else(|| {
            c.album(id)
                .and_then(|a| a.songs.first())
                .map(|&i| &c.songs[i])
        })
        .or_else(|| {
            let a = c.artist(id)?;
            let album = &c.albums[*a.albums.first()?];
            album.songs.first().map(|&i| &c.songs[i])
        })?;
    if let (Some(hash), Some(size)) = (&song.picture, size) {
        let dir = song.path.rsplit_once('/').map_or("", |x| x.0).to_owned();
        let made = engine::art::thumbnails(
            root,
            vec![engine::art::CoverRequest {
                dir,
                track_rel: song.path.clone(),
                data_hash: hash.clone(),
            }],
            size.clamp(64, 1500),
        );
        if let Some(t) = made.into_iter().next() {
            return Some(PathBuf::from(t.path));
        }
    }
    engine::art::cover_file_for(&root.join(&song.path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_come_out_timed_and_in_order() {
        let lines = parse_lrc(
            "[ar:Pink Floyd]\n[00:12.50]Ticking away\n[01:02.00][00:05.00]Both <00:05.20>times\nno time here\n",
        );
        assert_eq!(
            lines,
            [
                (5_000, "Both times".to_owned()),
                (12_500, "Ticking away".to_owned()),
                (62_000, "Both times".to_owned()),
            ]
        );
    }

    #[test]
    fn a_playlist_id_round_trips_any_name() {
        for name in ["Always❤️", "Best of TVF", "Escape_point"] {
            assert_eq!(playlist_name(&playlist_id(name)).as_deref(), Some(name));
        }
        assert_eq!(playlist_name("plzz"), None);
    }
}
