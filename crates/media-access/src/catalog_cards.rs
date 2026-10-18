//! The catalog client's cards and rows, as Venuta's catalog types.
//!
//! Suggestions, playlist and podcast searches, videos and mood pages come back from the
//! `ytmusic` client as cards (a kind, an id, a title, a subtitle line, a picture) and song rows.
//! Here they become the same `RemoteTrack` / `RemoteAlbum` / `RemoteArtist` / `RemoteCollection`
//! the rest of Venuta already shows and plays.

use crate::art_url::sized_art;
use crate::catalog_json::{parse_audience, parse_count, parse_duration};

/// A play count as the client abbreviates it ("534M", "2.5B").
fn plays(text: &str) -> Option<u64> {
    parse_count(&format!("{} plays", text.trim()))
}

/// The audience at the end of a card line ("Artist • 646M monthly audience").
fn audience(line: &str) -> Option<u64> {
    parse_audience(line.rsplit('•').next().unwrap_or(line))
}
use crate::types::{
    RemoteAlbum, RemoteArtist, RemoteCollection, RemoteSuggestion, RemoteTrack, SearchSuggestions,
};
use ytmusic::{ArtistRun, BrowseItem, SongItem};

pub(crate) const PROVIDER: &str = "remote_catalog";

/// A cover address made https and asked for at a size that looks sharp anywhere it is shown.
fn art(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    let url = if let Some(rest) = s.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = s.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        s.to_owned()
    };
    url.starts_with("https://").then(|| sized_art(&url, 544))
}

/// The artist names in a credit line, without the words and marks between them.
fn names(runs: &[ArtistRun], fallback: Option<&str>) -> Vec<String> {
    let from_runs: Vec<String> = runs
        .iter()
        .map(|r| r.text.trim().to_owned())
        .filter(|t| !t.is_empty() && !is_joiner(t))
        .collect();
    if !from_runs.is_empty() {
        return from_runs;
    }
    fallback
        .map(|line| {
            line.split([',', '&'])
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn is_joiner(text: &str) -> bool {
    matches!(text, "&" | "," | "and" | "•" | "x" | "X" | "feat." | "ft.")
}

/// A song card or row as a playable catalog track.
pub(crate) fn track_of_card(item: &BrowseItem) -> Option<RemoteTrack> {
    (item.kind == "song" && !item.id.trim().is_empty()).then(|| RemoteTrack {
        provider: PROVIDER.into(),
        track_id: item.id.trim().to_owned(),
        title: item.title.clone(),
        artists: names(&item.artist_runs, item.subtitle.as_deref()),
        album_id: item.album_id.clone(),
        duration_ms: item.duration.as_deref().and_then(parse_duration),
        artwork_url: art(item.thumbnail.as_deref()).or_else(|| {
            Some(format!(
                "https://i.ytimg.com/vi/{}/hqdefault.jpg",
                item.id.trim()
            ))
        }),
        plays: item.play_count.as_deref().and_then(plays),
        video_type: item.is_video.then(|| "OMV".to_owned()),
        ..Default::default()
    })
}

/// A song row (videos search) as a playable catalog track.
pub(crate) fn track_of_row(s: &SongItem) -> Option<RemoteTrack> {
    (!s.video_id.trim().is_empty()).then(|| RemoteTrack {
        provider: PROVIDER.into(),
        track_id: s.video_id.trim().to_owned(),
        title: s.title.clone(),
        artists: names(&s.artist_runs, Some(&s.artists)),
        album: s.album.clone(),
        album_id: s.album_id.clone(),
        duration_ms: s.duration.as_deref().and_then(parse_duration),
        artwork_url: art(s.thumbnail.as_deref()).or_else(|| {
            Some(format!(
                "https://i.ytimg.com/vi/{}/hqdefault.jpg",
                s.video_id.trim()
            ))
        }),
        plays: s.play_count.as_deref().and_then(plays),
        video_type: s.is_video.then(|| "OMV".to_owned()),
        ..Default::default()
    })
}

/// A playlist or podcast card as a collection to open.
pub(crate) fn collection_of(item: &BrowseItem, kind: &str) -> Option<RemoteCollection> {
    (!item.id.trim().is_empty()).then(|| RemoteCollection {
        provider: PROVIDER.into(),
        collection_id: item.id.trim().to_owned(),
        title: item.title.clone(),
        subtitle: item.subtitle.clone().filter(|s| !s.trim().is_empty()),
        artwork_url: art(item.thumbnail.as_deref()),
        kind: kind.to_owned(),
    })
}

/// One suggestion card as something to play or open; None for kinds Venuta doesn't show.
pub(crate) fn suggestion_of(item: &BrowseItem) -> Option<RemoteSuggestion> {
    let id = item.id.trim();
    if id.is_empty() {
        return None;
    }
    let subtitle = item.subtitle.clone().filter(|s| !s.trim().is_empty());
    match item.kind {
        "song" => track_of_card(item).map(|track| RemoteSuggestion::Song { track }),
        "album" => Some(RemoteSuggestion::Album {
            album: RemoteAlbum {
                provider: PROVIDER.into(),
                album_id: id.to_owned(),
                title: item.title.clone(),
                artists: names(&item.artist_runs, None),
                artwork_url: art(item.thumbnail.as_deref()),
                kind: subtitle
                    .as_deref()
                    .and_then(|s| s.split('•').next())
                    .map(|k| k.trim().to_owned()),
                ..Default::default()
            },
            subtitle,
        }),
        "artist" => Some(RemoteSuggestion::Artist {
            artist: RemoteArtist {
                provider: PROVIDER.into(),
                artist_id: id.to_owned(),
                name: item.title.clone(),
                artwork_url: art(item.thumbnail.as_deref()),
                audience: subtitle.as_deref().and_then(audience),
            },
            subtitle,
        }),
        "playlist" => collection_of(item, "playlist")
            .map(|collection| RemoteSuggestion::Playlist { collection }),
        _ => None,
    }
}

/// The client's suggestions as Venuta's: completions, then the cards it knows how to show.
pub(crate) fn suggestions_of(s: &ytmusic::SearchSuggestions) -> SearchSuggestions {
    SearchSuggestions {
        queries: s
            .queries
            .iter()
            .map(|q| q.text.clone())
            .filter(|q| !q.trim().is_empty())
            .collect(),
        items: s.items.iter().filter_map(suggestion_of).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(kind: &'static str, id: &str, title: &str, subtitle: &str) -> BrowseItem {
        BrowseItem {
            kind,
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            thumbnail: Some("https://lh3.googleusercontent.com/x=w60-h60-l90-rj".into()),
            duration: None,
            album_id: None,
            artist_runs: Vec::new(),
            play_count: None,
            is_video: false,
            is_upload: false,
            explicit: false,
        }
    }

    #[test]
    fn a_song_suggestion_plays_with_its_artists_and_a_sharp_cover() {
        let mut c = card("song", "fsiPzT50ZiM", "Tum Hi Ho", "Mithoon, Arijit Singh");
        c.artist_runs = vec![
            ArtistRun {
                text: "Mithoon".into(),
                id: Some("UC1".into()),
            },
            ArtistRun {
                text: " & ".into(),
                id: None,
            },
            ArtistRun {
                text: "Arijit Singh".into(),
                id: Some("UC2".into()),
            },
        ];
        c.play_count = Some("534M".into());
        let Some(RemoteSuggestion::Song { track }) = suggestion_of(&c) else {
            panic!("a song")
        };
        assert_eq!(track.track_id, "fsiPzT50ZiM");
        assert_eq!(track.artists, ["Mithoon", "Arijit Singh"]);
        assert_eq!(track.plays, Some(534_000_000));
        assert_eq!(
            track.artwork_url.as_deref(),
            Some("https://lh3.googleusercontent.com/x=w544-h544-l90-rj")
        );
    }

    #[test]
    fn an_artist_suggestion_keeps_its_line_and_reads_the_audience() {
        let c = card(
            "artist",
            "UCDx",
            "Arijit Singh",
            "Artist • 646M monthly audience",
        );
        let Some(RemoteSuggestion::Artist { artist, subtitle }) = suggestion_of(&c) else {
            panic!("an artist")
        };
        assert_eq!(artist.name, "Arijit Singh");
        assert_eq!(artist.audience, Some(646_000_000));
        assert_eq!(subtitle.as_deref(), Some("Artist • 646M monthly audience"));
    }

    #[test]
    fn an_album_suggestion_knows_its_kind() {
        let c = card(
            "album",
            "MPREb_x",
            "Arijit Singh (All Time Hits)",
            "Album • Arijit Singh • 2023",
        );
        let Some(RemoteSuggestion::Album { album, .. }) = suggestion_of(&c) else {
            panic!("an album")
        };
        assert_eq!(album.album_id, "MPREb_x");
        assert_eq!(album.kind.as_deref(), Some("Album"));
    }

    #[test]
    fn playlists_open_as_collections_and_unknown_kinds_are_left_out() {
        let c = card(
            "playlist",
            "VLPL123",
            "Spotlight: Arijit Singh",
            "Playlist • YouTube Music • 50 songs",
        );
        assert!(matches!(
            suggestion_of(&c),
            Some(RemoteSuggestion::Playlist { .. })
        ));
        assert!(suggestion_of(&card("episode", "x", "y", "z")).is_none());
        assert!(suggestion_of(&card("song", " ", "y", "z")).is_none());
    }
}
