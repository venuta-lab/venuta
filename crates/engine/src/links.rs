//! Playlists from a share link, free: no account, no key. The page a link opens is read as
//! anyone's browser would see it, once, when you bring the playlist in (or check it for
//! changes), with Venuta's own name (`net::USER_AGENT`). Readers are kept apart from fetching
//! and tested against saved pages (`tests/fixtures/pages`); a page whose shape has changed gives
//! a plain message and the other ways in, never wrong songs.
//!
//! Spotify (its embed page, up to 100 songs), Apple Music (the playlist page), TIDAL (its embed
//! page), JioSaavn (the site's own page data), Deezer (playlists and a profile's favourites,
//! through its public API, in `imports`), ListenBrainz (JSPF). YouTube Music links go through
//! Venuta's catalogue (in the app). SoundCloud and Amazon need the "Copy for Venuta" bookmark.

use crate::imports::ForeignPlaylist;
use crate::net::Http;
use crate::playlists::Foreign;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// A link Venuta can read a playlist from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Spotify(String),
    AppleMusic(String),
    Tidal(String),
    JioSaavn(String),
    Deezer(String),
    DeezerFavourites(String),
    ListenBrainz(String),
    /// A YouTube or YouTube Music playlist id (read through the catalogue).
    YouTube(String),
    /// A link to a service whose pages can't be read free; says what to do instead.
    Other(&'static str),
}

/// A kind of link, and how to make it a [`Link`] from what the pattern caught.
type Pattern = (Regex, fn(&str) -> Link);

/// What a pasted link is. `None`: not a link to a playlist at all.
pub fn parse(link: &str) -> Option<Link> {
    static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
        let r = |p: &str| Regex::new(p).expect("valid");
        vec![
            (
                r(r"open\.spotify\.com/(?:intl-[a-z-]+/)?(?:embed/)?playlist/([A-Za-z0-9]{22})"),
                |id| Link::Spotify(id.into()),
            ),
            (r(r"^spotify:playlist:([A-Za-z0-9]{22})"), |id| {
                Link::Spotify(id.into())
            }),
            (
                r(r"(https://music\.apple\.com/[a-z]{2}/playlist/[^?#\s]*pl\.[A-Za-z0-9.-]+)"),
                |url| Link::AppleMusic(url.into()),
            ),
            (
                r(
                    r"tidal\.com/(?:browse/)?(?:playlists?)/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})",
                ),
                |id| Link::Tidal(id.into()),
            ),
            (
                r(r"jiosaavn\.com/(?:featured|s/playlist)/[^/\s]+/([A-Za-z0-9_,-]+)"),
                |token| Link::JioSaavn(token.into()),
            ),
            (r(r"deezer\.com/(?:[a-z]{2}/)?playlist/(\d+)"), |id| {
                Link::Deezer(id.into())
            }),
            (r(r"deezer\.com/(?:[a-z]{2}/)?profile/(\d+)"), |id| {
                Link::DeezerFavourites(id.into())
            }),
            (
                r(
                    r"listenbrainz\.org/playlist/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})",
                ),
                |id| Link::ListenBrainz(id.into()),
            ),
            (
                r(r"(?:youtube\.com|youtu\.be)/.*[?&]list=([A-Za-z0-9_-]{10,})"),
                |id| Link::YouTube(id.into()),
            ),
            (r(r"soundcloud\.com/"), |_| {
                Link::Other(
                    "SoundCloud doesn't show a whole playlist on its page. Use the Copy for Venuta bookmark on it instead.",
                )
            }),
            (r(r"music\.amazon\.[a-z.]+/"), |_| {
                Link::Other(
                    "Amazon Music doesn't show a playlist to apps. Use the Copy for Venuta bookmark on its web player instead.",
                )
            }),
        ]
    });
    let link = link.trim();
    PATTERNS.iter().find_map(|(re, make)| {
        re.captures(link)
            .map(|c| make(c.get(1).map(|m| m.as_str()).unwrap_or_default()))
    })
}

/// Read the playlist a link points at. YouTube links are read by the app's catalogue, not here.
/// The second value: a note to show with it ("Spotify's page shows the first 100 songs…").
pub fn read(http: &Http, link: &Link) -> Result<(ForeignPlaylist, Option<String>), String> {
    let fetch = |url: &str| {
        http.get_text_fresh(url)
            .map_err(|e| format!("That page didn't open ({e}). Is the playlist public?"))
    };
    let changed = |service: &str| {
        format!(
            "{service}'s page has changed, so Venuta can't read this playlist from it now. Copy the songs from the app, or use the Copy for Venuta bookmark."
        )
    };
    match link {
        Link::Spotify(id) => {
            let page = fetch(&format!("https://open.spotify.com/embed/playlist/{id}"))?;
            let (name, tracks) = spotify_embed(&page).ok_or_else(|| changed("Spotify"))?;
            let note = (tracks.len() >= 100).then(|| {
                "Spotify's page shows the first 100 songs. For the rest, select all the songs in Spotify's app (Ctrl+A), copy them (Ctrl+C) and paste here.".to_owned()
            });
            Ok((list(name, "Spotify", format!("spotify:{id}"), tracks), note))
        }
        Link::AppleMusic(url) => {
            let page = fetch(url)?;
            let (name, tracks, total) =
                apple_music_page(&page).ok_or_else(|| changed("Apple Music"))?;
            let note = total.filter(|t| *t > tracks.len()).map(|t| {
                format!(
                    "Apple Music's page shows {} of the {t} songs. For all of them, use the Copy for Venuta bookmark on music.apple.com.",
                    tracks.len()
                )
            });
            let id = url.rsplit('/').next().unwrap_or(url);
            Ok((
                list(name, "Apple Music", format!("applemusic:{id}"), tracks),
                note,
            ))
        }
        Link::Tidal(id) => {
            let page = fetch(&format!("https://embed.tidal.com/playlists/{id}"))?;
            let (name, tracks) = tidal_embed(&page).ok_or_else(|| changed("TIDAL"))?;
            Ok((list(name, "TIDAL", format!("tidal:{id}"), tracks), None))
        }
        Link::JioSaavn(token) => {
            let mut tracks = Vec::new();
            let mut name = String::new();
            for page in 1..=40 {
                let text = fetch(&format!(
                    "https://www.jiosaavn.com/api.php?__call=webapi.get&token={token}&type=playlist&p={page}&n=50&ctx=web6dot0&api_version=4&_format=json&_marker=0"
                ))?;
                let v: Value = serde_json::from_str(&text).map_err(|_| changed("JioSaavn"))?;
                let (n, mut songs, total) = jiosaavn(&v).ok_or_else(|| changed("JioSaavn"))?;
                name = n;
                let got = songs.len();
                tracks.append(&mut songs);
                if got == 0 || tracks.len() >= total {
                    break;
                }
            }
            Ok((
                list(name, "JioSaavn", format!("jiosaavn:{token}"), tracks),
                None,
            ))
        }
        Link::Deezer(id) => Ok((crate::imports::deezer(http, id)?, None)),
        Link::DeezerFavourites(user) => Ok((crate::imports::deezer_favourites(http, user)?, None)),
        Link::ListenBrainz(id) => {
            let text = fetch(&format!("https://api.listenbrainz.org/1/playlist/{id}"))?;
            let (name, listed) = crate::formats::jspf(&text);
            let tracks = listed
                .into_iter()
                .map(|l| l.song)
                .filter(|s| !s.title.is_empty())
                .collect();
            Ok((
                list(
                    name.unwrap_or_else(|| "ListenBrainz playlist".into()),
                    "ListenBrainz",
                    format!("listenbrainz:{id}"),
                    tracks,
                ),
                None,
            ))
        }
        Link::YouTube(_) => Err("YouTube playlists are read through Venuta's catalogue.".into()),
        Link::Other(what) => Err((*what).into()),
    }
}

fn list(name: String, source: &str, id: String, tracks: Vec<Foreign>) -> ForeignPlaylist {
    ForeignPlaylist {
        name,
        source: source.into(),
        id: Some(id),
        tracks,
    }
}

/// The JSON inside `<script id="…">` of a page.
fn script_json(page: &str, id: &str) -> Option<Value> {
    let at = page.find(&format!("id=\"{id}\""))?;
    let start = at + page[at..].find('>')? + 1;
    let end = start + page[start..].find("</script>")?;
    serde_json::from_str(&page[start..end]).ok()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_owned()
}

/// Spotify's embed page: the playlist's name and its songs (at most 100).
pub fn spotify_embed(page: &str) -> Option<(String, Vec<Foreign>)> {
    let v = script_json(page, "__NEXT_DATA__")?;
    let e = &v["props"]["pageProps"]["state"]["data"]["entity"];
    let tracks = e["trackList"]
        .as_array()?
        .iter()
        .map(|t| Foreign {
            title: s(&t["title"]),
            // The artists, as one line ("A, B").
            artist: s(&t["subtitle"]),
            duration_ms: t["duration"].as_u64().filter(|d| *d > 0),
            ..Default::default()
        })
        .filter(|f| !f.title.is_empty())
        .collect();
    Some((s(&e["name"]), tracks))
}

/// Apple Music's playlist page: name, songs, and how many the playlist has in all.
pub fn apple_music_page(page: &str) -> Option<(String, Vec<Foreign>, Option<usize>)> {
    let v = script_json(page, "serialized-server-data")?;
    let data = &v.get("data").unwrap_or(&v)[0]["data"];
    let sections = data["sections"].as_array()?;
    let header = sections
        .iter()
        .find(|x| s(&x["itemKind"]) == "containerDetailHeaderLockup")
        .and_then(|x| x["items"].get(0));
    let tracks: Vec<Foreign> = sections
        .iter()
        .filter(|x| s(&x["itemKind"]) == "trackLockup")
        .flat_map(|x| x["items"].as_array().cloned().unwrap_or_default())
        .map(|t| Foreign {
            title: s(&t["title"]),
            artist: s(&t["artistName"]),
            album: t["tertiaryLinks"]
                .get(0)
                .map(|l| s(&l["title"]))
                .unwrap_or_default(),
            duration_ms: t["duration"].as_u64().filter(|d| *d > 0),
            ..Default::default()
        })
        .filter(|f| !f.title.is_empty())
        .collect();
    let name = header
        .map(|h| s(&h["title"]))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Apple Music playlist".into());
    let total = header
        .and_then(|h| h["trackCount"].as_u64())
        .map(|n| n as usize);
    Some((name, tracks, total))
}

/// TIDAL's embed page: name and songs.
pub fn tidal_embed(page: &str) -> Option<(String, Vec<Foreign>)> {
    static ITEM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<list-item\b[^>]*product-type="track"[^>]*>(.*?)</list-item>"#)
            .expect("valid")
    });
    static SLOT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?s)<(?:span|time) slot="(title|artist|duration)"[^>]*>(.*?)</(?:span|time)>"#,
        )
        .expect("valid")
    });
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid"));
    static NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<h1 class="media-album"[^>]*>(.*?)</h1>"#).expect("valid")
    });
    let text = |h: &str| html_text(&TAG.replace_all(h, ""));
    let tracks: Vec<Foreign> = ITEM
        .captures_iter(page)
        .map(|item| {
            let mut f = Foreign::default();
            for slot in SLOT.captures_iter(&item[1]) {
                let value = text(&slot[2]);
                match &slot[1] {
                    "title" => f.title = value,
                    "artist" => f.artist = value,
                    _ => f.duration_ms = clock(&value),
                }
            }
            f
        })
        .filter(|f| !f.title.is_empty())
        .collect();
    if tracks.is_empty() {
        return None;
    }
    let name = NAME
        .captures(page)
        .map(|c| text(&c[1]))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "TIDAL playlist".into());
    Some((name, tracks))
}

/// One page of JioSaavn's playlist data: name, its songs, and how many in all.
pub fn jiosaavn(v: &Value) -> Option<(String, Vec<Foreign>, usize)> {
    let list = v["list"].as_array()?;
    let tracks = list
        .iter()
        .map(|t| {
            let artists: Vec<String> = t["more_info"]["artistMap"]["primary_artists"]
                .as_array()
                .map(|a| a.iter().map(|x| html_text(&s(&x["name"]))).collect())
                .unwrap_or_default();
            Foreign {
                title: html_text(&s(&t["title"])),
                artist: artists.join(", "),
                album: html_text(&s(&t["more_info"]["album"])),
                duration_ms: s(&t["more_info"]["duration"])
                    .parse::<u64>()
                    .ok()
                    .filter(|d| *d > 0)
                    .map(|d| d * 1000),
                ..Default::default()
            }
        })
        .filter(|f| !f.title.is_empty())
        .collect();
    let total = s(&v["list_count"]).parse().unwrap_or(list.len());
    Some((html_text(&s(&v["title"])), tracks, total))
}

/// "&amp;" and "&quot;" as the characters they stand for.
fn html_text(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

fn clock(s: &str) -> Option<u64> {
    let parts: Vec<u64> = s
        .trim()
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [m, sec] => Some((m * 60 + sec) * 1000),
        [h, m, sec] => Some((h * 3600 + m * 60 + sec) * 1000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(name: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/pages")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn links_say_where_they_go() {
        assert_eq!(
            parse("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc"),
            Some(Link::Spotify("37i9dQZF1DXcBWIGoYBM5M".into()))
        );
        assert_eq!(
            parse("https://open.spotify.com/intl-de/playlist/37i9dQZF1DXcBWIGoYBM5M"),
            Some(Link::Spotify("37i9dQZF1DXcBWIGoYBM5M".into()))
        );
        assert!(matches!(
            parse("https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb?l=en"),
            Some(Link::AppleMusic(u)) if u.ends_with("pl.f4d106fed2bd41149aaacabb233eb5eb")
        ));
        assert_eq!(
            parse("https://tidal.com/browse/playlist/36ea71a8-445e-41a4-82ab-6628c581535d"),
            Some(Link::Tidal("36ea71a8-445e-41a4-82ab-6628c581535d".into()))
        );
        assert_eq!(
            parse("https://www.jiosaavn.com/featured/trending-today/I3kvhipIy73uCJW60TJk1Q__"),
            Some(Link::JioSaavn("I3kvhipIy73uCJW60TJk1Q__".into()))
        );
        assert_eq!(
            parse("https://www.deezer.com/en/playlist/3155776842"),
            Some(Link::Deezer("3155776842".into()))
        );
        assert_eq!(
            parse("https://www.deezer.com/us/profile/12345"),
            Some(Link::DeezerFavourites("12345".into()))
        );
        assert_eq!(
            parse(
                "https://music.youtube.com/playlist?list=RDCLAK5uy_kmPRjHDECIcuVwnKsx2Ng7fyNgFKWNJFs"
            ),
            Some(Link::YouTube(
                "RDCLAK5uy_kmPRjHDECIcuVwnKsx2Ng7fyNgFKWNJFs".into()
            ))
        );
        assert!(matches!(
            parse("https://soundcloud.com/a/sets/b"),
            Some(Link::Other(_))
        ));
        assert_eq!(parse("Arijit Singh - Kesariya"), None);
    }

    #[test]
    fn spotify_embed_page() {
        let (name, songs) = spotify_embed(&saved("spotify-embed-playlist.html")).unwrap();
        assert_eq!(name, "Today’s Top Hits");
        assert_eq!(songs.len(), 50);
        assert_eq!(songs[0].title, "Stop The Wedding!");
        assert_eq!(songs[0].artist, "Ashe");
        assert!(songs[0].duration_ms.is_some());
    }

    #[test]
    fn apple_music_playlist_page() {
        let (name, songs, total) = apple_music_page(&saved("apple-music-playlist.html")).unwrap();
        assert_eq!(name, "Today’s Hits");
        assert_eq!(songs.len(), 50);
        assert_eq!(total, Some(50));
        assert_eq!(
            (
                songs[0].title.as_str(),
                songs[0].artist.as_str(),
                songs[0].album.as_str()
            ),
            ("Nicole Kidman", "ADÉLA", "PRIMA")
        );
        assert_eq!(songs[0].duration_ms, Some(181_270));
    }

    #[test]
    fn tidal_embed_page() {
        let (name, songs) = tidal_embed(&saved("tidal-embed-playlist.html")).unwrap();
        assert_eq!(name, "Pop Hits");
        assert_eq!(songs.len(), 49);
        assert_eq!(
            (
                songs[0].title.as_str(),
                songs[0].artist.as_str(),
                songs[0].duration_ms
            ),
            ("Patient Zero", "Taylor Swift", Some(226_000))
        );
    }

    #[test]
    fn jiosaavn_page_data() {
        let v: Value = serde_json::from_str(&saved("jiosaavn-playlist.json")).unwrap();
        let (name, songs, total) = jiosaavn(&v).unwrap();
        assert_eq!(name, "Trending Today");
        assert_eq!((songs.len(), total), (43, 43));
        assert_eq!(songs[0].title, "Hanuman Chalisa");
        assert!(!songs[0].artist.is_empty());
        assert!(songs[0].duration_ms.is_some());
    }

    #[test]
    fn a_changed_page_reads_as_nothing_not_as_wrong_songs() {
        assert!(spotify_embed("<html>new design</html>").is_none());
        assert!(apple_music_page("<html></html>").is_none());
        assert!(tidal_embed("<html></html>").is_none());
    }
}
