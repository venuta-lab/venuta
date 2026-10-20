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
    MenuFeedback, MenuToggle, PastSearch, RadioChip, RadioEndpoint, RemoteAlbum, RemoteArtist,
    RemoteCollection, RemoteMore, RemoteRadio, RemoteRelatedShelf, RemoteShelf, RemoteSuggestion,
    RemoteTrack, SearchSuggestions, TrackMenu,
};
use ytmusic::{ArtistRun, BrowseItem, RowMenu, SongItem};

/// A row's own account menu as a track's: what each press sends. None when it offers nothing (a
/// row read signed out, or one Venuta built).
pub(crate) fn menu_of(m: &RowMenu) -> Option<TrackMenu> {
    let menu = TrackMenu {
        library: m.library.as_ref().map(|l| MenuToggle {
            on: l.in_library,
            on_token: l.add_token.clone(),
            off_token: l.remove_token.clone(),
        }),
        pin: m.pin.as_ref().map(|p| MenuToggle {
            on: p.pinned,
            on_token: p.pin_token.clone(),
            off_token: p.unpin_token.clone(),
        }),
        feedback: m
            .feedback
            .iter()
            .map(|f| MenuFeedback {
                icon: f.icon.clone(),
                label: f.label.clone(),
                token: f.token.clone(),
            })
            .collect(),
    };
    (!menu.is_empty()).then_some(menu)
}

/// Tracks read by another parser (a search or a playlist page read raw) given their rows' own
/// menus from the raw answer, by video id. A track that has one already keeps it.
pub(crate) fn with_menus(tracks: &mut [RemoteTrack], raw: &serde_json::Value) {
    if tracks.iter().all(|t| t.menu.is_some()) {
        return;
    }
    let mut menus = std::collections::HashMap::new();
    for row in ytmusic::models::browse::song_rows(raw) {
        if let Some(m) = menu_of(&row.menu()) {
            menus.entry(row.video_id.trim().to_owned()).or_insert(m);
        }
    }
    for t in tracks.iter_mut().filter(|t| t.menu.is_none()) {
        t.menu = menus.get(t.track_id.as_str()).cloned();
    }
}

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
        menu: menu_of(&item.menu),
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
        menu: menu_of(&s.menu()),
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
        play: item.play.as_ref().map(endpoint_of),
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

/// A song's Related page as the Related tab shows it: every shelf in the catalog's order, songs
/// as tracks, cards as what they open, the words about the artist as words. A shelf with nothing
/// Venuta can show is left out.
pub(crate) fn related_of(shelves: &[ytmusic::RelatedShelf]) -> Vec<RemoteRelatedShelf> {
    shelves
        .iter()
        .filter_map(|s| {
            let shelf = RemoteRelatedShelf {
                title: s.title.trim().to_owned(),
                tracks: s.songs.iter().filter_map(track_of_row).collect(),
                items: s.cards.iter().filter_map(suggestion_of).collect(),
                about: s
                    .description
                    .as_deref()
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned),
            };
            (!shelf.tracks.is_empty() || !shelf.items.is_empty() || shelf.about.is_some())
                .then_some(shelf)
        })
        .collect()
}

/// Mixed for you's page as rows of mixes, each mix keeping what its play button plays. A row
/// with no heading keeps its mixes (the page may send one grid without a title).
pub(crate) fn mixes_of(sections: &[ytmusic::Section]) -> Vec<RemoteShelf> {
    sections
        .iter()
        .filter_map(|s| {
            let items: Vec<RemoteSuggestion> = s.items.iter().filter_map(suggestion_of).collect();
            (!items.is_empty()).then(|| RemoteShelf {
                title: s.title.trim().to_owned(),
                items,
                more: None,
            })
        })
        .collect()
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
    pub radio: Option<RadioEndpoint>,
    pub shuffle: Option<RadioEndpoint>,
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
        radio: page.radio.as_ref().map(endpoint_of),
        shuffle: page.shuffle.as_ref().map(endpoint_of),
        shelves,
    }
}

/// The client's home feed as Venuta's: chips, and the rows it can show.
pub(crate) fn home_of(page: &ytmusic::HomePage) -> crate::types::RemoteHome {
    crate::types::RemoteHome {
        chips: page
            .chips
            .iter()
            .filter(|c| !c.title.trim().is_empty() && !c.params.trim().is_empty())
            .map(|c| crate::types::RemoteChip {
                title: c.title.trim().to_owned(),
                params: c.params.clone(),
            })
            .collect(),
        shelves: page
            .sections
            .iter()
            .filter_map(|s| {
                let shelf = shelf_of(&ytmusic::ArtistCarousel {
                    title: s.title.clone(),
                    items: s.items.clone(),
                    more_browse_id: s.more_browse_id.clone(),
                    more_params: s.more_params.clone(),
                })?;
                Some(crate::types::RemoteHomeShelf {
                    shelf,
                    kind: s.kind.map(str::to_owned),
                })
            })
            .collect(),
        continuation: page.continuation.clone(),
    }
}

/// A radio's songs as catalog tracks, in the catalog's order, a song coming round again and all.
/// `seed` is the song playing: an answer that opens with it (its audio row, or the row whose
/// music-video twin it is) has that first row left out, since it is already there. Nothing else
/// is left out.
pub(crate) fn radio_of(next: &ytmusic::NextResult, seed: Option<&str>) -> Vec<RemoteTrack> {
    let seed = seed.map(str::trim).filter(|s| !s.is_empty());
    let mut rows = next
        .items
        .iter()
        .filter(|s| !s.video_id.trim().is_empty())
        .peekable();
    if let (Some(seed), Some(first)) = (seed, rows.peek())
        && (first.video_id.trim() == seed || first.counterpart.as_deref() == Some(seed))
    {
        rows.next();
    }
    rows.filter_map(track_of_row).collect()
}

/// An endpoint as Venuta keeps it.
pub(crate) fn endpoint_of(e: &ytmusic::WatchEndpoint) -> RadioEndpoint {
    RadioEndpoint {
        video_id: e.video_id.clone(),
        playlist_id: e.playlist_id.clone(),
        params: e.params.clone(),
        index: e.index,
        playlist_set_video_id: e.playlist_set_video_id.clone(),
    }
}

/// An endpoint as the client sends it again: exactly what came.
pub(crate) fn watch_of(e: &RadioEndpoint) -> ytmusic::WatchEndpoint {
    let s = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    ytmusic::WatchEndpoint {
        video_id: s(&e.video_id),
        playlist_id: s(&e.playlist_id),
        params: s(&e.params),
        index: e.index,
        playlist_set_video_id: s(&e.playlist_set_video_id),
    }
}

/// A `next` answer as a radio page: its songs (see [`radio_of`]) and its session, `asked` being
/// the endpoint it answered.
pub(crate) fn radio_page_of(
    next: &ytmusic::NextResult,
    asked: &ytmusic::WatchEndpoint,
    seed: Option<&str>,
) -> RemoteRadio {
    RemoteRadio {
        playlist_id: next
            .playlist_id
            .clone()
            .or_else(|| asked.playlist_id.clone()),
        tracks: radio_of(next, seed),
        title: next.title.clone(),
        endpoint: Some(endpoint_of(asked)),
        continuation: next.continuation.clone(),
        chips: next
            .chips
            .iter()
            .filter(|c| !c.text.trim().is_empty())
            .map(|c| RadioChip {
                text: c.text.trim().to_owned(),
                selected: c.selected,
                endpoint: endpoint_of(&c.endpoint),
            })
            .collect(),
        continue_with: next.automix.as_ref().map(endpoint_of),
        related_id: next.related_browse_id.clone(),
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
        past: s
            .queries
            .iter()
            .filter(|q| q.history && !q.text.trim().is_empty())
            .map(|q| PastSearch {
                text: q.text.clone(),
                remove_token: q.remove_token.clone(),
            })
            .collect(),
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
            play: None,
            menu: Default::default(),
        }
    }

    // A mix card on Home keeps the endpoint its play button names, so playing it replays that
    // (a mix's own `params` and all) instead of a radio rebuilt from its id.
    #[test]
    fn a_mix_card_carries_its_play_endpoint() {
        let mut c = card("playlist", "VLRDTMAK5uy_super", "My Supermix", "Mix");
        c.play = Some(ytmusic::WatchEndpoint {
            playlist_id: Some("RDTMAK5uy_super".into()),
            params: Some("wAEB8gECKAE%3D".into()),
            ..Default::default()
        });
        let Some(RemoteSuggestion::Playlist { collection }) = suggestion_of(&c) else {
            panic!("a playlist")
        };
        assert_eq!(
            collection.play,
            Some(RadioEndpoint {
                playlist_id: Some("RDTMAK5uy_super".into()),
                params: Some("wAEB8gECKAE%3D".into()),
                ..Default::default()
            })
        );
        let plain = card("playlist", "VLPL1", "Hits", "Playlist");
        let Some(RemoteSuggestion::Playlist { collection }) = suggestion_of(&plain) else {
            panic!("a playlist")
        };
        assert_eq!(collection.play, None);
        let json = serde_json::to_value(&collection).unwrap();
        assert!(
            json.get("play").is_none(),
            "nothing said when there is none"
        );
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

    fn row_menu_fixture() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ytmusic/tests/fixtures/library_song_row_menu.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn saved_menu() -> TrackMenu {
        TrackMenu {
            library: Some(MenuToggle {
                on: true,
                on_token: Some("FIXTURE-LIBRARY-ADD".into()),
                off_token: Some("FIXTURE-LIBRARY-REMOVE".into()),
            }),
            pin: Some(MenuToggle {
                on: false,
                on_token: Some("FIXTURE-PIN".into()),
                off_token: Some("FIXTURE-UNPIN".into()),
            }),
            feedback: Vec::new(),
        }
    }

    // A song row read as the account brings its own menu to the track: in the library, its pin,
    // and the tokens each press sends. A row with nothing on it brings nothing.
    #[test]
    fn a_song_row_brings_its_account_menu_to_the_track() {
        let raw = row_menu_fixture();
        let song = ytmusic::models::browse::song_rows(&raw);
        let t = track_of_row(&song[0]).unwrap();
        assert_eq!(t.menu, Some(saved_menu()));

        let mut feedback = row("x", "X");
        feedback.feedback = vec![ytmusic::MenuFeedback {
            icon: "NOT_INTERESTED".into(),
            label: "Not interested".into(),
            token: "NI".into(),
        }];
        let menu = track_of_row(&feedback).unwrap().menu.unwrap();
        assert_eq!(
            menu.feedback,
            vec![MenuFeedback {
                icon: "NOT_INTERESTED".into(),
                label: "Not interested".into(),
                token: "NI".into(),
            }]
        );
        assert_eq!(track_of_row(&row("y", "Y")).unwrap().menu, None);
    }

    // A song card on Home carries the same menu its row had.
    #[test]
    fn a_song_card_brings_its_account_menu_to_the_track() {
        let raw = row_menu_fixture();
        let song = &ytmusic::models::browse::song_rows(&raw)[0];
        let mut c = card("song", "BddP6PYo2gs", "Kesariya", "Pritam");
        c.menu = song.menu();
        assert_eq!(track_of_card(&c).unwrap().menu, Some(saved_menu()));
        assert_eq!(
            track_of_card(&card("song", "a", "A", "B")).unwrap().menu,
            None
        );
    }

    // Tracks read by another parser (a search or a playlist page read raw) get their rows' menus
    // by video id; a track the raw page doesn't list, or one that has a menu, is left as it is.
    #[test]
    fn tracks_read_raw_get_their_rows_menus() {
        let raw = serde_json::json!({ "contents": [row_menu_fixture()] });
        let plain = |id: &str| RemoteTrack {
            provider: "youtube_music".into(),
            track_id: id.into(),
            title: id.into(),
            ..Default::default()
        };
        let mut tracks = vec![plain("BddP6PYo2gs"), plain("other")];
        with_menus(&mut tracks, &raw);
        assert_eq!(tracks[0].menu, Some(saved_menu()));
        assert_eq!(tracks[1].menu, None);
    }

    // Past searches come with the suggestions as the completions they are, and on their own
    // with the token that takes each out; a completion is never one.
    #[test]
    fn past_searches_come_with_their_remove_tokens() {
        let s = ytmusic::SearchSuggestions {
            queries: vec![
                ytmusic::QuerySuggestion {
                    text: "arijit singh".into(),
                    history: true,
                    remove_token: Some("PAST".into()),
                },
                ytmusic::QuerySuggestion {
                    text: "arijit singh songs".into(),
                    history: false,
                    remove_token: None,
                },
            ],
            items: Vec::new(),
        };
        let out = suggestions_of(&s);
        assert_eq!(out.queries, ["arijit singh", "arijit singh songs"]);
        assert_eq!(
            out.past,
            vec![PastSearch {
                text: "arijit singh".into(),
                remove_token: Some("PAST".into()),
            }]
        );
    }

    fn row(id: &str, title: &str) -> SongItem {
        SongItem {
            video_id: id.into(),
            title: title.into(),
            artists: "Someone".into(),
            ..Default::default()
        }
    }

    // The catalog's queue as it sent it: a song coming round again stays, and the seed leading
    // the answer (the song playing) is the only row left out. Later on, the seed is a repeat
    // like any other and stays too.
    #[test]
    fn a_radio_leaves_out_only_the_seed_it_opens_with() {
        let next = ytmusic::NextResult {
            items: vec![
                row("seed", "Seed"),
                row("a", "A"),
                row("b", "B"),
                row("a", "A again"),
                row("seed", "Seed again"),
                row(" ", "No id"),
            ],
            ..Default::default()
        };
        let ids: Vec<String> = radio_of(&next, Some("seed"))
            .into_iter()
            .map(|t| t.track_id)
            .collect();
        assert_eq!(ids, ["a", "b", "a", "seed"]);
        // A radio of a mix, or a later page, has no seed: every song is in it.
        assert_eq!(radio_of(&next, None).len(), 5);
        // Opened on the seed's music video, the audio row is still the seed.
        let mut wrapped = next.clone();
        wrapped.items[0].video_id = "seedAudio".into();
        wrapped.items[0].counterpart = Some("seed".into());
        assert_eq!(radio_of(&wrapped, Some("seed")).len(), 4);
    }

    // Everything a radio session needs comes through: the endpoint asked, the next page's token,
    // the chips with theirs, what it goes on with, and the Related page.
    #[test]
    fn a_radio_page_carries_its_session() {
        let next = ytmusic::NextResult {
            items: vec![row("seed", "Seed"), row("a", "A")],
            continuation: Some("TOKEN2".into()),
            playlist_id: Some("RDAMVMseed".into()),
            title: Some("Seed Radio".into()),
            chips: vec![ytmusic::QueueChip {
                text: "Familiar".into(),
                selected: false,
                endpoint: ytmusic::WatchEndpoint {
                    playlist_id: Some("RDATiYvseed".into()),
                    params: Some("P".into()),
                    ..Default::default()
                },
            }],
            automix: Some(ytmusic::WatchEndpoint {
                playlist_id: Some("RDAMPLmix".into()),
                ..Default::default()
            }),
            related_browse_id: Some("MPTRt_seed".into()),
            ..Default::default()
        };
        let asked = ytmusic::WatchEndpoint::song_radio("seed");
        let radio = radio_page_of(&next, &asked, Some("seed"));
        assert_eq!(radio.tracks.len(), 1);
        assert_eq!(radio.playlist_id.as_deref(), Some("RDAMVMseed"));
        assert_eq!(radio.title.as_deref(), Some("Seed Radio"));
        assert_eq!(radio.continuation.as_deref(), Some("TOKEN2"));
        assert_eq!(
            radio.endpoint,
            Some(crate::types::RadioEndpoint {
                video_id: Some("seed".into()),
                playlist_id: Some("RDAMVMseed".into()),
                params: Some("wAEB".into()),
                ..Default::default()
            })
        );
        assert_eq!(radio.chips.len(), 1);
        assert_eq!(radio.chips[0].text, "Familiar");
        assert_eq!(
            radio.chips[0].endpoint.playlist_id.as_deref(),
            Some("RDATiYvseed")
        );
        assert_eq!(radio.chips[0].endpoint.params.as_deref(), Some("P"));
        assert_eq!(
            radio
                .continue_with
                .as_ref()
                .and_then(|e| e.playlist_id.as_deref()),
            Some("RDAMPLmix")
        );
        assert_eq!(radio.related_id.as_deref(), Some("MPTRt_seed"));
        // And back: what is sent again is exactly what came.
        assert_eq!(
            watch_of(radio.endpoint.as_ref().unwrap()),
            ytmusic::WatchEndpoint::song_radio("seed")
        );
    }

    #[test]
    fn the_home_feed_keeps_its_chips_rows_and_their_kinds() {
        let page = ytmusic::HomePage {
            chips: vec![
                ytmusic::models::browse::HomeChip {
                    title: "Romance".into(),
                    params: "p1".into(),
                },
                ytmusic::models::browse::HomeChip {
                    title: " ".into(),
                    params: "p2".into(),
                },
            ],
            sections: vec![
                ytmusic::Section {
                    title: "Quick picks".into(),
                    kind: Some("quick_picks"),
                    items: vec![card("song", "s1", "Tum Hi Ho", "Arijit Singh")],
                    more_browse_id: None,
                    more_params: None,
                },
                ytmusic::Section {
                    title: "Nothing to show".into(),
                    kind: None,
                    items: vec![card("episode", "e", "x", "")],
                    more_browse_id: None,
                    more_params: None,
                },
            ],
            continuation: Some("next".into()),
        };
        let home = home_of(&page);
        assert_eq!(home.chips.len(), 1, "a chip with no name is left out");
        assert_eq!(
            home.shelves.len(),
            1,
            "a row with nothing to show is left out"
        );
        assert_eq!(home.shelves[0].kind.as_deref(), Some("quick_picks"));
        assert_eq!(home.shelves[0].shelf.title, "Quick picks");
        assert_eq!(home.continuation.as_deref(), Some("next"));
    }

    #[test]
    fn an_empty_answer_has_no_extras() {
        assert_eq!(
            artist_extras(&serde_json::json!({}), "UC1", None),
            ArtistExtras::default()
        );
    }

    // A song YouTube queued names its album: the album's id comes through with it, so the queue
    // can go to that album as it can for a file of yours.
    #[test]
    fn a_queued_song_keeps_its_album() {
        let mut r = row("a", "A");
        r.album = Some("Album A".into());
        r.album_id = Some("MPREalbumA".into());
        let next = ytmusic::NextResult {
            items: vec![r],
            ..Default::default()
        };
        let tracks = radio_of(&next, None);
        assert_eq!(tracks[0].album.as_deref(), Some("Album A"));
        assert_eq!(tracks[0].album_id.as_deref(), Some("MPREalbumA"));
    }

    // A Related page as the Related tab shows it: each shelf in YouTube's order, its songs as
    // tracks, its cards as the things they open, the words about the artist as words.
    #[test]
    fn a_related_page_keeps_every_shelf_in_order() {
        let shelves = [
            ytmusic::RelatedShelf {
                title: "You might also like".into(),
                songs: vec![row("a", "A"), row(" ", "No id")],
                cards: Vec::new(),
                description: None,
            },
            ytmusic::RelatedShelf {
                title: "Similar artists".into(),
                songs: Vec::new(),
                cards: vec![
                    card("artist", "UC3", "Artist Three", "1.2M monthly audience"),
                    card("radio", "X", "Not a kind Venuta shows", ""),
                ],
                description: None,
            },
            ytmusic::RelatedShelf {
                title: "About the artist".into(),
                songs: Vec::new(),
                cards: Vec::new(),
                description: Some("Words.".into()),
            },
            // Nothing Venuta can show in it: left out, not a bare heading.
            ytmusic::RelatedShelf {
                title: "Unknown".into(),
                songs: vec![row("", "")],
                cards: vec![card("radio", "Y", "Y", "")],
                description: None,
            },
        ];
        let out = related_of(&shelves);
        let titles: Vec<&str> = out.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["You might also like", "Similar artists", "About the artist"]
        );
        let ids: Vec<&str> = out[0].tracks.iter().map(|t| t.track_id.as_str()).collect();
        assert_eq!(ids, ["a"]);
        assert!(matches!(
            &out[1].items[..],
            [RemoteSuggestion::Artist { artist, .. }] if artist.artist_id == "UC3"
        ));
        assert_eq!(out[2].about.as_deref(), Some("Words."));
        assert!(out[2].tracks.is_empty() && out[2].items.is_empty());
    }

    // Mixed for you's page: its rows of mixes, each mix keeping what its play button plays.
    #[test]
    fn the_mixes_page_is_its_rows_of_mixes() {
        let mut mix = card("playlist", "VLRDTMAK5uy_super", "My Supermix", "Mix");
        mix.play = Some(ytmusic::WatchEndpoint {
            playlist_id: Some("RDTMAK5uy_super".into()),
            params: Some("wAEB8gECKAE%3D".into()),
            ..Default::default()
        });
        let sections = [
            ytmusic::Section {
                title: "Your mixes".into(),
                kind: None,
                items: vec![mix],
                more_browse_id: None,
                more_params: None,
            },
            ytmusic::Section {
                title: "Nothing to show".into(),
                kind: None,
                items: vec![card("radio", "Y", "Y", "")],
                more_browse_id: None,
                more_params: None,
            },
        ];
        let shelves = mixes_of(&sections);
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].title, "Your mixes");
        assert_eq!(shelves[0].more, None);
        let [RemoteSuggestion::Playlist { collection }] = &shelves[0].items[..] else {
            panic!("a mix")
        };
        assert_eq!(
            collection.play.as_ref().and_then(|p| p.params.as_deref()),
            Some("wAEB8gECKAE%3D")
        );
    }
}
