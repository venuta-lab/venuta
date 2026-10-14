//! Queue helpers for ephemeral remote-catalog audio.
//!
//! The stream URL is never kept on a queue item. A *deferred* item holds only a stable
//! provider id and its [`MediaAccess`] handle, and resolves [`EphemeralAudio`] the moment it is
//! opened — so snapshots and restored queues carry metadata and ids, never an address.

use crate::decode::Source;
use crate::engine::Item;
use media_access::{EphemeralAudio, MediaAccess};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// Where a [`RemoteItem`]'s session audio comes from.
#[derive(Clone)]
enum AudioSource {
    /// Already resolved by the caller (fixtures and callers that resolve up front).
    Resolved(EphemeralAudio),
    /// Resolved through [`MediaAccess`] on a worker at open time (no URL stored; no audio-thread block).
    Deferred {
        access: Arc<dyn MediaAccess>,
        duration_hint_ms: Option<u64>,
    },
}

/// A queue item that opens session-only direct HTTP audio for a remote-catalog track.
#[derive(Clone)]
pub struct RemoteItem {
    source: AudioSource,
    provider: String,
    track_id: String,
    title: String,
    album: Option<String>,
}

impl RemoteItem {
    /// From session audio already resolved by the caller.
    pub fn new(audio: EphemeralAudio, title: impl Into<String>, album: Option<String>) -> Self {
        Self {
            provider: audio.provider.clone(),
            track_id: audio.track_id.clone(),
            source: AudioSource::Resolved(audio),
            title: title.into(),
            album,
        }
    }

    /// Bound to a stable provider id; the stream is resolved through `access` at open time and
    /// never held on the item (so it cannot leak into a snapshot or a restored queue).
    pub fn deferred(
        access: Arc<dyn MediaAccess>,
        provider: impl Into<String>,
        track_id: impl Into<String>,
        title: impl Into<String>,
        album: Option<String>,
    ) -> Self {
        Self::deferred_with_duration(access, provider, track_id, title, album, None)
    }

    /// Like [`Self::deferred`], with a catalog duration so the bar can show length while opening.
    pub fn deferred_with_duration(
        access: Arc<dyn MediaAccess>,
        provider: impl Into<String>,
        track_id: impl Into<String>,
        title: impl Into<String>,
        album: Option<String>,
        duration_hint_ms: Option<u64>,
    ) -> Self {
        Self {
            source: AudioSource::Deferred {
                access,
                duration_hint_ms,
            },
            provider: provider.into(),
            track_id: track_id.into(),
            title: title.into(),
            album,
        }
    }

}

impl Item for RemoteItem {
    fn path(&self) -> Option<&Path> {
        None
    }

    fn open(&self) -> Result<Source, String> {
        match &self.source {
            // Already have session audio: download off-thread.
            AudioSource::Resolved(audio) => Source::open_remote(audio.clone()),
            // Resolve + download entirely off the audio thread (fast Opening…, no freeze).
            AudioSource::Deferred {
                access,
                duration_hint_ms,
            } => Source::open_remote_deferred(
                Arc::clone(access),
                self.track_id.clone(),
                *duration_hint_ms,
            ),
        }
    }

    fn label(&self) -> String {
        self.title.clone()
    }

    fn album(&self) -> Option<String> {
        self.album.clone()
    }

    fn reopen_when_stopped(&self) -> bool {
        false
    }
}

impl Serialize for RemoteItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Safe<'a> {
            title: &'a str,
            provider: &'a str,
            track_id: &'a str,
            album: &'a Option<String>,
        }
        Safe {
            title: &self.title,
            provider: &self.provider,
            track_id: &self.track_id,
            album: &self.album,
        }
        .serialize(serializer)
    }
}
