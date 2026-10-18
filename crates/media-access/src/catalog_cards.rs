//! The catalog client's cards and rows, as Venuta's catalog types.
//!
//! Suggestions, playlist and podcast searches, videos and mood pages come back from the
//! `ytmusic` client as cards (a kind, an id, a title, a subtitle line, a picture) and song rows.
//! Here they become the same `RemoteTrack` / `RemoteAlbum` / `RemoteArtist` / `RemoteCollection`
//! the rest of Venuta already shows and plays.

use crate::art_url::sized_art;
use crate::catalog_json::{parse_audience, parse_count, parse_duration};

/// A play count as the client abbreviates it ("534M", "2.5B"), or as written ("534M plays").
pub(crate) fn plays(text: &str) -> Option<u64> {
    parse_count(text.trim()).or_else(|| parse_count(&format!("{} plays", text.trim())))
}

/// The audience at the end of a card line ("Artist • 646M monthly audience").
fn audience(line: &str) -> Option<u64> {
    parse_audience(line.rsplit('•').next().unwrap_or(line))
}
use crate::types::{
    RemoteAlbum, RemoteArtist, RemoteCollection, RemoteMore, RemoteShelf, RemoteSuggestion,
    RemoteTrack, SearchSuggestions,
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
                    .map(|k| k.trim().to_owned())
                    .filter(|k| is_release_kind(k)),
                release_date: subtitle.as_deref().and_then(|s| {
                    s.split('•')
                        .map(str::trim)
                        .rfind(|part| is_year(part))
                        .map(str::to_owned)
                }),
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

/// Words the catalog leads an album card's line with when it says what the release is.
fn is_release_kind(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    matches!(
        t.as_str(),
        "single" | "ep" | "album" | "compilation" | "mixtape" | "soundtrack"
    ) || t.ends_with(" album")
}

fn is_year(text: &str) -> bool {
    text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit())
}

/// A titled row of a catalog page as a shelf: the cards Venuta knows how to show, and its
/// "See all". None when nothing in it can be shown.
pub(crate) fn shelf_of(c: &ytmusic::ArtistCarousel) -> Option<RemoteShelf> {
    let items: Vec<RemoteSuggestion> = c.items.iter().filter_map(suggestion_of).collect();
    (!items.is_empty() && !c.title.trim().is_empty()).then(|| RemoteShelf {
        title: c.title.trim().to_owned(),
        items,
        more: c
            .more_browse_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| RemoteMore {
                browse_id: id.to_owned(),
                params: c.more_params.clone().filter(|p| !p.trim().is_empty()),
            }),
    })
}

/// What an artist's page says beyond their songs: who they are, their banner and radio, and
/// every row of the page in its order. `songs` is their song list's page, when it was read:
/// the releases by others they sing on become the last row, Appears on.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ArtistExtras {
    pub description: Option<String>,
    pub subscribers: Option<String>,
    pub banner_url: Option<String>,
    pub radio_id: Option<String>,
    pub shelves: Vec<RemoteShelf>,
}

/// Releases by others shown in Appears on (most played first): a row, not a discography.
const APPEARS_ON_SHOWN: usize = 40;

pub(crate) fn artist_extras(
    raw: &serde_json::Value,
    artist_id: &str,
    songs: Option<&serde_json::Value>,
) -> ArtistExtras {
    use ytmusic::models::browse::{appears_on, parse_artist, parse_playlist};
    let page = parse_artist(raw, artist_id);
    let mut shelves: Vec<RemoteShelf> = page.sections.iter().filter_map(shelf_of).collect();
    // A "See all" that leads back into their own page (Live performances) is a tab of it, not a
    // grid: it would open empty, so the row has none.
    for shelf in &mut shelves {
        if shelf
            .more
            .as_ref()
            .is_some_and(|m| m.browse_id == page.channel_id || m.browse_id == artist_id)
        {
            shelf.more = None;
        }
    }
    if let Some(songs) = songs {
        let mut cards = appears_on(&parse_playlist(songs).items, &page.channel_id);
        // Led by them under another of their channels is still their own release.
        let own = page.name.as_deref().map(|n| n.trim().to_lowercase());
        cards.retain(|c| {
            let lead = c.subtitle.as_deref().map(|s| s.trim().to_lowercase());
            own.is_none() || lead != own
        });
        cards.truncate(APPEARS_ON_SHOWN);
        if let Some(shelf) = shelf_of(&ytmusic::ArtistCarousel {
            title: "Appears on".into(),
            items: cards,
            more_browse_id: None,
            more_params: None,
        }) {
            shelves.push(shelf);
        }
    }
    let text = |s: Option<String>| s.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    ArtistExtras {
        description: text(page.description),
        subscribers: text(page.subscribers),
        // The banner is wide: asked for at its own width, never squared.
        banner_url: page.thumbnail.as_deref().and_then(|u| {
            let u = u.trim();
            u.starts_with("https://").then(|| u.to_owned())
        }),
        radio_id: text(page.radio_playlist_id),
        shelves,
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
        assert_eq!(album.release_date.as_deref(), Some("2023"));
    }

    #[test]
    fn an_album_card_with_only_a_year_is_not_a_kind_called_2026() {
        // An artist page's Albums row words its cards "2026"; Singles words them "Single • 2026".
        let Some(RemoteSuggestion::Album { album, .. }) =
            suggestion_of(&card("album", "MPREb_y", "Heartbeat Diaries", "2026"))
        else {
            panic!("an album")
        };
        assert_eq!(album.kind, None);
        assert_eq!(album.release_date.as_deref(), Some("2026"));
        let Some(RemoteSuggestion::Album { album, .. }) =
            suggestion_of(&card("album", "MPREb_z", "Ram Lala", "Single • 2026"))
        else {
            panic!("a single")
        };
        assert_eq!(album.kind.as_deref(), Some("Single"));
        assert_eq!(album.release_date.as_deref(), Some("2026"));
        // Appears on words a card by its lead artist: a name, not a kind of release.
        let Some(RemoteSuggestion::Album { album, .. }) =
            suggestion_of(&card("album", "MPREb_w", "Raees", "Arijit Singh"))
        else {
            panic!("an album")
        };
        assert_eq!(album.kind, None);
    }

    #[test]
    fn a_row_of_cards_keeps_its_title_cards_and_see_all() {
        let carousel = ytmusic::ArtistCarousel {
            title: "Singles & EPs".into(),
            items: vec![
                card("album", "MPREb_z", "Ram Lala", "Single • 2026"),
                card("episode", "x", "not shown", ""),
            ],
            more_browse_id: Some("MPADUC1".into()),
            more_params: Some("ggMI".into()),
        };
        let shelf = shelf_of(&carousel).expect("a shelf");
        assert_eq!(shelf.title, "Singles & EPs");
        assert_eq!(shelf.items.len(), 1);
        assert_eq!(
            shelf.more,
            Some(crate::types::RemoteMore {
                browse_id: "MPADUC1".into(),
                params: Some("ggMI".into()),
            })
        );
        let empty = ytmusic::ArtistCarousel {
            title: "Nothing".into(),
            items: vec![card("episode", "x", "y", "")],
            more_browse_id: None,
            more_params: None,
        };
        assert!(shelf_of(&empty).is_none());
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

    fn fixture(path: &str) -> serde_json::Value {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        serde_json::from_str(&std::fs::read_to_string(file).expect("fixture")).expect("json")
    }

    #[test]
    fn an_artist_page_keeps_who_they_are_and_every_row_in_order() {
        let raw = fixture("tests/fixtures/ytm/artist-arijit.json");
        let extras = artist_extras(&raw, "UCDxKh1gFWeYsqePvgVzmPoQ", None);
        assert_eq!(extras.subscribers.as_deref(), Some("6.52M subscribers"));
        assert!(
            extras
                .description
                .as_deref()
                .is_some_and(|d| d.starts_with("Arijit Singh is an Indian singer"))
        );
        assert!(
            extras
                .radio_id
                .as_deref()
                .is_some_and(|r| r.starts_with("RD"))
        );
        let banner = extras.banner_url.expect("a banner");
        assert!(banner.starts_with("https://lh3.googleusercontent.com/"));
        let titles: Vec<&str> = extras.shelves.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Albums",
                "Singles & EPs",
                "Videos",
                "Live performances",
                "Featured on",
                "Playlists by Arijit Singh",
                "Fans might also like",
            ]
        );
        let albums = &extras.shelves[0];
        assert!(albums.more.is_some(), "Albums has a See all");
        let live = &extras.shelves[3];
        assert_eq!(
            live.more, None,
            "a See all back into their own page opens nothing"
        );
        let Some(RemoteSuggestion::Album { album, .. }) = albums.items.first() else {
            panic!("an album first")
        };
        assert_eq!(album.kind, None);
        assert!(album.release_date.is_some());
        let fans = extras.shelves.last().expect("fans");
        assert!(fans.items.iter().all(|i| matches!(
            i,
            RemoteSuggestion::Artist { artist, .. } if artist.audience.is_some()
        )));
    }

    #[test]
    fn the_releases_by_others_they_sing_on_are_appears_on() {
        let raw = fixture("tests/fixtures/ytm/artist-arijit.json");
        let songs = fixture("../ytmusic/tests/fixtures/artist_top_songs_features.json");
        // Rows of their song list led by someone else are releases they sing on.
        let extras = artist_extras(&raw, "UCDxKh1gFWeYsqePvgVzmPoQ", Some(&songs));
        let last = extras.shelves.last().expect("a last row");
        assert_eq!(last.title, "Appears on");
        assert!(
            last.items
                .iter()
                .all(|i| matches!(i, RemoteSuggestion::Album { .. }))
        );
    }

    #[test]
    fn an_empty_answer_has_no_extras() {
        assert_eq!(
            artist_extras(&serde_json::json!({}), "UC1", None),
            ArtistExtras::default()
        );
    }
}
