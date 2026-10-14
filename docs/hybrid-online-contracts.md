# Hybrid online/local contracts (Stage 0)

Frozen interfaces for product work. Implement against these types; do not add service-specific clients in UI/engine layers.

## Crate: `media-access`

- `MediaAccess` — search + resolve trait
- `EphemeralAudio` — session-only direct HTTP audio (`Debug` redacts URL/headers; not serializable)
- `Playable` / `PlayableId` — stable local path or online `{provider, track_id}`
- `Availability` — `Local` | `Online` | `LocalPreferred`
- `FakeMediaAccess` — local WAV fixture server for tests
- `default_media_access()` — app factory (currently fake until a real provider is wired)

## Player

- `Source::open(path)` — local files
- `Source::open_remote(EphemeralAudio)` — non-blocking remote open
- `Item` supports `path() -> Option<&Path>` and default `open()` via path

## Playlist persistence tag

Line format for remote-bound entries (`engine::playlists::OnlineEntry`, optional `isrc`):

```text
#VENUTA-ONLINE:{"provider":"remote_catalog","track_id":"...","title":"...","artists":["..."],"album":"...","duration_ms":123}
```

- `encode_online` / `decode_online` write and read the line; ids that are empty or contain
  `://` are rejected (written as a `#VENUTA-WANT` line instead), and unknown JSON fields are dropped.
- Local first: `read` binds an online entry to a library file on an exact match (ISRC, or
  title + artist); `PlaylistEntry::playable` / `playables` return the local file when it is on
  disk (`LocalPreferred`), else the online `Playable`. Unbound songs to get are skipped.
- Online entries are not on the want-list (`want` is None); `PlaylistSummary.online` counts them,
  separately from `missing`.

Never store stream URLs in playlist files or restored queues.

## App surface

- `online_search(query)` / `online_album_tracks(album_id)` (app `online.rs`) go through the
  playback's own `MediaAccess`; a newer search cancels the one before it.
- `playlist_queue(root, name)` returns `engine::playlists::playables`, the only path the window
  uses to play a playlist (mixed local + online, in order).
- `playlist_add(root, name, items)` takes playlist items (`{path}` or `{online}`) and dedupes by
  path or stable online id (`engine::playlists::add`).
