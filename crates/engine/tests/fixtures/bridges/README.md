# Saved answers from the free bridges

What `crates/engine/src/bridges.rs` is tested against, so tests never go online. Asked once on
2026-10-14 about one Hindi film song (Kesariya, Arijit Singh, Brahmastra, ISRC INS172203702) and
one Western song (Bohemian Rhapsody, Queen), with Venuta's own User-Agent
(`Venuta/<version> (+https://github.com/venuta-lab/venuta)`), no key.

| File | Asked |
|---|---|
| `deezer-search.json` | `api.deezer.com/search?q=Kesariya Arijit Singh` (credited to the composer, Pritam) |
| `deezer-track.json`, `deezer-isrc.json` | `api.deezer.com/track/1828361287`, `/track/isrc:INS172203702` (every name credited) |
| `itunes-search.json` | `itunes.apple.com/search?term=Kesariya Arijit Singh&entity=song` (the single, the soundtrack, compilations, remixes) |
| `musicbrainz-isrc.json` | `musicbrainz.org/ws/2/isrc/INS172203702` |
| `lb-spotify.json`, `lb-apple.json`, `lb-spotify-mbid.json` | ListenBrainz Labs for Kesariya: no ids (Indian music barely covered) |
| `lb-spotify-western.json` | ListenBrainz Labs for Bohemian Rhapsody: 4 Spotify ids |
