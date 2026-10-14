//! In-process fake catalog + HTTP fixture audio for product tests.

use crate::access::{AccessError, AccessErrorKind, MediaAccess};
use crate::types::{
    CancellationToken, EphemeralAudio, Page, PageRequest, RemoteAlbum, RemoteArtist, RemoteTrack,
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
        let track = sample_track("track-1", &format!("{query} (fixture)"));
        Ok(Page {
            items: vec![track].into_iter().take(page.limit).collect(),
            next_cursor: None,
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
        artwork_url: None,
        isrc: None,
    }
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
    fn fake_search_and_resolve_redacts_url() {
        let access = FakeMediaAccess::with_in_memory_fixture();
        let page = access
            .search_tracks(
                "hello",
                &PageRequest::new(5, None),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let audio = access
            .resolve_audio("track-1", &CancellationToken::default())
            .unwrap();
        let debug = format!("{audio:?}");
        assert!(!debug.contains("http://"));
        assert!(debug.contains("[redacted]"));
        assert_eq!(audio.mime_type.as_deref(), Some("audio/wav"));
    }
}
