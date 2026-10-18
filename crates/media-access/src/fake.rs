//! In-process fake catalog + HTTP fixture audio for product tests.

use crate::access::{AccessError, AccessErrorKind, MediaAccess};
use crate::types::{
    CancellationToken, EphemeralAudio, Page, PageRequest, RemoteAlbum, RemoteArtist,
    RemoteArtistPage, RemoteTrack,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread;

/// Serves a tiny WAV over HTTP and returns deterministic catalog rows.
pub struct FakeMediaAccess {
    base_url: String,
    /// Keep the server thread's listener alive for the process lifetime of tests.
    _shutdown: Mutex<Option<TcpListener>>,
}

impl FakeMediaAccess {
    /// Bind a local fixture server and return a provider pointed at it.
    pub fn with_in_memory_fixture() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake media fixture");
        let address = listener.local_addr().expect("fixture addr");
        let server = listener
            .try_clone()
            .expect("clone fixture listener for accept loop");
        thread::spawn(move || {
            for stream in server.incoming().flatten() {
                let _ = handle_client(stream);
            }
        });
        Self {
            base_url: format!("http://{address}/fixture.wav"),
            _shutdown: Mutex::new(Some(listener)),
        }
    }

    pub fn fixture_url(&self) -> &str {
        &self.base_url
    }
}

impl MediaAccess for FakeMediaAccess {
    fn search_tracks(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        ensure_active(cancel)?;
        let query = query.trim();
        if query.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search query must not be empty",
            ));
        }
        // Pages of at most 4 songs (the live catalog pages too), so "More" can be tried offline.
        let start = match page.cursor.as_deref() {
            None => 0,
            Some(c) => c
                .strip_prefix("fake:")
                .and_then(|n| n.parse::<usize>().ok())
                .ok_or_else(|| {
                    AccessError::new(AccessErrorKind::InvalidInput, "catalog cursor is invalid")
                })?,
        };
        let all = search_hits(query);
        let end = (start + page.limit.min(FAKE_PAGE)).min(all.len());
        Ok(Page {
            items: all
                .get(start..end)
                .map(<[RemoteTrack]>::to_vec)
                .unwrap_or_default(),
            next_cursor: (end < all.len()).then(|| format!("fake:{end}")),
        })
    }

    fn search_albums(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteAlbum>, AccessError> {
        ensure_active(cancel)?;
        let query = query.trim();
        if query.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search query must not be empty",
            ));
        }
        Ok(Page {
            items: vec![RemoteAlbum {
                provider: "remote_catalog".into(),
                album_id: "album-1".into(),
                title: format!("{query} Album"),
                artists: vec!["Fixture Artist".into()],
                track_count: Some(2),
                release_date: Some("2026".into()),
                artwork_url: None,
                kind: Some("Album".into()),
            }]
            .into_iter()
            .take(page.limit)
            .collect(),
            next_cursor: None,
        })
    }

    fn search_artists(
        &self,
        query: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteArtist>, AccessError> {
        ensure_active(cancel)?;
        let query = query.trim();
        if query.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "search query must not be empty",
            ));
        }
        Ok(Page {
            items: vec![RemoteArtist {
                provider: "remote_catalog".into(),
                artist_id: "artist-1".into(),
                name: format!("{query} Artist"),
                artwork_url: None,
                audience: Some(1_200_000),
            }]
            .into_iter()
            .take(page.limit)
            .collect(),
            next_cursor: None,
        })
    }

    fn album_tracks(
        &self,
        album_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        ensure_active(cancel)?;
        if album_id.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "album id must not be empty",
            ));
        }
        Ok(Page {
            items: vec![
                sample_track("track-1", "Fixture One"),
                sample_track("track-2", "Fixture Two"),
            ]
            .into_iter()
            .take(page.limit)
            .collect(),
            next_cursor: None,
        })
    }

    fn artist_page(
        &self,
        artist_id: &str,
        cancel: &CancellationToken,
    ) -> Result<RemoteArtistPage, AccessError> {
        ensure_active(cancel)?;
        let id = artist_id.trim();
        if id.is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "artist id must not be empty",
            ));
        }
        // The fixture artist: its own songs (and a cover by someone else, to rank down), an
        // album, a single and one artist like it.
        let hits = search_hits("Fixture");
        let album = |album_id: &str, title: &str, kind: &str| RemoteAlbum {
            provider: "remote_catalog".into(),
            album_id: album_id.into(),
            title: title.into(),
            artists: vec!["Fixture Artist".into()],
            release_date: Some("2026".into()),
            kind: Some(kind.into()),
            ..Default::default()
        };
        Ok(RemoteArtistPage {
            provider: "remote_catalog".into(),
            artist_id: id.into(),
            name: "Fixture Artist".into(),
            audience: Some(1_200_000),
            top_songs: hits[..3].to_vec(),
            songs: hits[3..].to_vec(),
            albums: vec![album("album-1", "Fixture Album", "Album")],
            singles: vec![album("album-single", "Fixture Single", "Single")],
            similar: vec![RemoteArtist {
                provider: "remote_catalog".into(),
                artist_id: "artist-2".into(),
                name: "Another Fixture".into(),
                audience: Some(300_000),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn playlist_tracks(
        &self,
        playlist_id: &str,
        page: &PageRequest,
        cancel: &CancellationToken,
    ) -> Result<Page<RemoteTrack>, AccessError> {
        ensure_active(cancel)?;
        if playlist_id.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "playlist id must not be empty",
            ));
        }
        self.album_tracks("album-1", page, cancel)
    }

    fn resolve_audio(
        &self,
        track_id: &str,
        cancel: &CancellationToken,
    ) -> Result<EphemeralAudio, AccessError> {
        ensure_active(cancel)?;
        if track_id.trim().is_empty() {
            return Err(AccessError::new(
                AccessErrorKind::InvalidInput,
                "track id must not be empty",
            ));
        }
        Ok(EphemeralAudio {
            track_id: track_id.to_owned(),
            provider: "remote_catalog".into(),
            url: self.base_url.clone(),
            mime_type: Some("audio/wav".into()),
            content_length: Some(wav_bytes().len() as u64),
            duration_ms: Some(200),
            headers: Vec::new(),
            via: "fixture",
            loudness_db: None,
        })
    }
}

fn ensure_active(cancel: &CancellationToken) -> Result<(), AccessError> {
    if cancel.is_cancelled() {
        Err(AccessError::new(
            AccessErrorKind::Cancelled,
            "request cancelled",
        ))
    } else {
        Ok(())
    }
}

fn sample_track(id: &str, title: &str) -> RemoteTrack {
    RemoteTrack {
        provider: "remote_catalog".into(),
        track_id: id.into(),
        title: title.into(),
        artists: vec!["Fixture Artist".into()],
        album: Some("Fixture Album".into()),
        album_id: Some("album-1".into()),
        duration_ms: Some(200),
        ..Default::default()
    }
}

/// Songs per page of fake search results.
const FAKE_PAGE: usize = 4;

/// What a search finds, shaped like the live catalog's mix so ranking can be tried offline:
/// the artist's own audio on an album (most plays), its official video, someone's cover (the
/// catalog's "top result", as happens live), and a karaoke upload with no album or count.
fn search_hits(query: &str) -> Vec<RemoteTrack> {
    vec![
        RemoteTrack {
            duration_ms: Some(215_000),
            video_type: Some("ATV".into()),
            plays: Some(12_000_000),
            shelf_rank: Some(1),
            ..sample_track("track-1", &format!("{query} (fixture)"))
        },
        RemoteTrack {
            album: None,
            album_id: None,
            duration_ms: Some(221_000),
            video_type: Some("OMV".into()),
            plays: Some(3_400_000),
            shelf_rank: Some(2),
            ..sample_track(
                "track-video",
                &format!("{query} (fixture) (Official Video)"),
            )
        },
        RemoteTrack {
            artists: vec!["A Fan".into()],
            album: None,
            album_id: None,
            duration_ms: Some(230_000),
            video_type: Some("UGC".into()),
            plays: Some(25_000),
            is_top: true,
            shelf_rank: Some(0),
            ..sample_track(
                "track-cover",
                &format!("{query} (fixture) (acoustic cover)"),
            )
        },
        RemoteTrack {
            artists: vec!["Karaoke Hits".into()],
            album: None,
            album_id: None,
            duration_ms: Some(218_000),
            video_type: Some("UGC".into()),
            ..sample_track(
                "track-karaoke",
                &format!("{query} (fixture) (Karaoke Version)"),
            )
        },
        // The second page: a live recording and a remix.
        RemoteTrack {
            album: Some("Fixture Live".into()),
            album_id: Some("album-live".into()),
            duration_ms: Some(260_000),
            video_type: Some("ATV".into()),
            plays: Some(800_000),
            ..sample_track("track-live", &format!("{query} (fixture) (Live)"))
        },
        RemoteTrack {
            artists: vec!["DJ Fixture".into()],
            album: Some("Club Fixtures".into()),
            album_id: Some("album-club".into()),
            duration_ms: Some(300_000),
            video_type: Some("ATV".into()),
            plays: Some(150_000),
            ..sample_track("track-remix", &format!("{query} (fixture) (Club Remix)"))
        },
    ]
}

fn wav_bytes() -> Vec<u8> {
    let sample_bytes = vec![0_u8; 1_600];
    let mut wav = Vec::with_capacity(44 + sample_bytes.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36_u32 + sample_bytes.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&8_000_u32.to_le_bytes());
    wav.extend_from_slice(&16_000_u32.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(sample_bytes.len() as u32).to_le_bytes());
    wav.extend_from_slice(&sample_bytes);
    wav
}

fn handle_client(mut stream: std::net::TcpStream) -> std::io::Result<()> {
    let mut request = [0_u8; 2048];
    let _ = stream.read(&mut request)?;
    let body = wav_bytes();
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_artist_page_has_songs_records_and_a_similar_artist() {
        let access = FakeMediaAccess::with_in_memory_fixture();
        let page = access
            .artist_page("artist-1", &CancellationToken::default())
            .unwrap();
        assert_eq!(page.name, "Fixture Artist");
        assert_eq!((page.top_songs.len(), page.songs.len()), (3, 3));
        assert_eq!(
            (page.albums.len(), page.singles.len(), page.similar.len()),
            (1, 1, 1)
        );
        assert!(
            access
                .artist_page(" ", &CancellationToken::default())
                .is_err()
        );
    }

    #[test]
    fn fake_search_and_resolve_redacts_url() {
        let access = FakeMediaAccess::with_in_memory_fixture();
        let page = access
            .search_tracks(
                "hello",
                &PageRequest::new(5, None),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(page.items.len(), 4);
        // A next page: two more songs, then nothing.
        let more = access
            .search_tracks(
                "hello",
                &PageRequest::new(20, page.next_cursor.clone()),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(more.items.len(), 2);
        assert!(more.items.iter().any(|t| t.title.contains("Live")));
        assert_eq!(more.next_cursor, None);
        assert!(
            access
                .search_tracks(
                    "hello",
                    &PageRequest::new(5, Some("nonsense".into())),
                    &CancellationToken::default()
                )
                .is_err()
        );
        // The artist's own audio on an album comes first; the cover is the catalog's top result.
        let first = &page.items[0];
        assert_eq!(first.track_id, "track-1");
        assert_eq!(
            (first.video_type.as_deref(), first.plays),
            (Some("ATV"), Some(12_000_000))
        );
        assert!(first.album_id.is_some());
        let cover = page.items.iter().find(|t| t.is_top).unwrap();
        assert_eq!(
            (cover.video_type.as_deref(), cover.album.as_deref()),
            (Some("UGC"), None)
        );
        assert!(cover.title.contains("cover"));
        assert!(
            page.items
                .iter()
                .any(|t| t.title.contains("Karaoke") && t.plays.is_none())
        );
        // A smaller page is the first rows.
        let two = access
            .search_tracks(
                "hello",
                &PageRequest::new(2, None),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(two.items.len(), 2);
        // A track's own fields round-trip, and old rows without the hints still read.
        let json = serde_json::to_string(first).unwrap();
        assert!(json.contains("\"video_type\":\"ATV\""));
        let old: RemoteTrack =
            serde_json::from_str(r#"{"provider":"remote_catalog","track_id":"x","title":"T"}"#)
                .unwrap();
        assert_eq!(
            (old.video_type, old.plays, old.is_top, old.shelf_rank),
            (None, None, false, None)
        );
        let audio = access
            .resolve_audio("track-1", &CancellationToken::default())
            .unwrap();
        let debug = format!("{audio:?}");
        assert!(!debug.contains("http://"));
        assert!(debug.contains("[redacted]"));
        assert_eq!(audio.mime_type.as_deref(), Some("audio/wav"));
    }
}
