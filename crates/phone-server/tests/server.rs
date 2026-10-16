//! The server over a real socket, as a phone app talks to it.

use engine::library::{Library, Track};
use phone_server::{Phone, Source};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixed {
    root: PathBuf,
    lib: Arc<Library>,
    played: Mutex<Vec<String>>,
}

impl Source for Fixed {
    fn root(&self) -> PathBuf {
        self.root.clone()
    }
    fn library(&self) -> Result<Arc<Library>, String> {
        Ok(self.lib.clone())
    }
    fn played(&self, path: &str) {
        self.played.lock().unwrap().push(path.to_owned());
    }
}

fn track(id: i64, path: &str, title: &str, n: &str) -> Track {
    let tags: HashMap<String, Vec<String>> = [
        ("TITLE", title),
        ("ARTIST", "Miles Davis"),
        ("ALBUM", "Kind of Blue"),
        ("TRACKNUMBER", n),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), vec![(*v).to_owned()]))
    .collect();
    Track {
        id,
        path: path.into(),
        dir: path.rsplit_once('/').unwrap().0.into(),
        kind: "flac".into(),
        size: 1000,
        duration_ms: 545_000,
        sample_rate: 44_100,
        bit_depth: 16,
        audio_md5: None,
        vendor: None,
        tags,
        raw_keys: vec![],
        pictures: vec![],
        quality: None,
    }
}

/// One request, the whole answer back: (status line, headers, body).
async fn get(port: u16, path: &str, extra: &str) -> (String, String, Vec<u8>) {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: pc\r\nConnection: close\r\n{extra}\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    let (status, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
    (
        status.to_owned(),
        headers.to_lowercase(),
        buf[split + 4..].to_vec(),
    )
}

fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice::<serde_json::Value>(body).unwrap()["subsonic-response"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_phone_signs_in_browses_streams_and_scrobbles() {
    let root = tempfile_dir();
    let album = "Music/Jazz/Miles Davis/Kind of Blue (1959)";
    std::fs::create_dir_all(root.join(album)).unwrap();
    let audio: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(root.join(format!("{album}/01 - So What.flac")), &audio).unwrap();
    std::fs::write(
        root.join(format!("{album}/01 - So What.lrc")),
        "[00:01.00]So what\n[00:03.50]Indeed\n",
    )
    .unwrap();
    let lib = Library::of_tracks(vec![
        track(1, &format!("{album}/01 - So What.flac"), "So What", "1"),
        track(
            2,
            &format!("{album}/02 - Freddie Freeloader.flac"),
            "Freddie Freeloader",
            "2",
        ),
    ]);
    let source = Arc::new(Fixed {
        root: root.clone(),
        lib: Arc::new(lib),
        played: Mutex::new(vec![]),
    });
    let phone = Phone {
        name: "Phone".into(),
        user: "phone".into(),
        password: "sesame".into(),
    };
    let server = phone_server::start(0, source.clone(), vec![phone])
        .await
        .unwrap();
    let port = server.addr.port();
    let auth = "u=phone&p=sesame&v=1.16.1&c=test&f=json";

    let (status, _, body) = get(port, &format!("/rest/ping.view?{auth}"), "").await;
    assert!(status.contains("200"), "{status}");
    assert_eq!(json(&body)["status"], "ok");

    let (_, _, body) = get(port, "/rest/ping?u=phone&p=wrong&f=json", "").await;
    assert_eq!(json(&body)["error"]["code"], 40);

    let (_, _, body) = get(port, &format!("/rest/getAlbumList2?type=newest&{auth}"), "").await;
    let albums = json(&body)["albumList2"]["album"].clone();
    assert_eq!(albums[0]["name"], "Kind of Blue");
    let album_id = albums[0]["id"].as_str().unwrap().to_owned();

    let (_, _, body) = get(port, &format!("/rest/getAlbum?id={album_id}&{auth}"), "").await;
    let songs = json(&body)["album"]["song"].clone();
    assert_eq!(songs[0]["title"], "So What");
    assert_eq!(songs[1]["track"], 2);
    let song_id = songs[0]["id"].as_str().unwrap().to_owned();

    // The whole library for an empty search, as apps copy it.
    let (_, _, body) = get(
        port,
        &format!("/rest/search3?query=&songCount=500&{auth}"),
        "",
    )
    .await;
    assert_eq!(
        json(&body)["searchResult3"]["song"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Streaming the file as it is, and from the middle (seeking).
    let (status, headers, body) = get(port, &format!("/rest/stream?id={song_id}&{auth}"), "").await;
    assert!(status.contains("200"), "{status}");
    assert!(headers.contains("content-type: audio/flac"), "{headers}");
    assert_eq!(body, audio);
    let (status, _, body) = get(
        port,
        &format!("/rest/stream?id={song_id}&{auth}"),
        "Range: bytes=500-\r\n",
    )
    .await;
    assert!(status.contains("206"), "{status}");
    assert_eq!(body, &audio[500..]);

    let (_, _, body) = get(
        port,
        &format!("/rest/getLyricsBySongId?id={song_id}&{auth}"),
        "",
    )
    .await;
    let l = json(&body)["lyricsList"]["structuredLyrics"][0].clone();
    assert_eq!(l["synced"], true);
    assert_eq!(l["line"][1]["start"], 3500);
    assert_eq!(l["line"][1]["value"], "Indeed");

    // What apps ask for beside the library (notes, similar songs, podcasts...): answered, empty,
    // so a screen that asks never fails on it.
    for (method, key) in [
        ("getAlbumInfo2", "albumInfo"),
        ("getAlbumInfo", "albumInfo"),
        ("getArtistInfo2", "artistInfo2"),
        ("getArtistInfo", "artistInfo"),
        ("getSimilarSongs2", "similarSongs2"),
        ("getTopSongs", "topSongs"),
        ("getPodcasts", "podcasts"),
        ("getInternetRadioStations", "internetRadioStations"),
        ("getBookmarks", "bookmarks"),
        ("getShares", "shares"),
    ] {
        let (_, _, body) = get(port, &format!("/rest/{method}?id={album_id}&{auth}"), "").await;
        let r = json(&body);
        assert_eq!(r["status"], "ok", "{method}: {r}");
        assert!(r[key].is_object(), "{method} answers {key}: {r}");
    }

    // XML for the apps that ask for it.
    let (_, headers, body) = get(port, "/rest/ping?u=phone&p=sesame", "").await;
    assert!(headers.contains("text/xml"), "{headers}");
    assert!(String::from_utf8_lossy(&body).contains(r#"status="ok""#));

    let (_, _, body) = get(port, &format!("/rest/scrobble?id={song_id}&{auth}"), "").await;
    assert_eq!(json(&body)["status"], "ok");
    assert_eq!(
        *source.played.lock().unwrap(),
        [format!("{album}/01 - So What.flac")]
    );

    server.stop();
    let _ = std::fs::remove_dir_all(&root);
}

fn tempfile_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("venuta-phone-server-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
