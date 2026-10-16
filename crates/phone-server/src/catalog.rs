//! The library as phone apps see it: songs, albums and artists, with ids that stay the same
//! across scans (so a phone's offline downloads survive), built once per library snapshot.

use engine::library::{Library, Track};
use std::collections::HashMap;

pub struct Song {
    pub id: String,
    pub track_id: i64,
    /// Library-relative path.
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_id: String,
    pub artist_id: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub duration_ms: u64,
    pub size: u64,
    pub suffix: String,
    pub sample_rate: u32,
    pub bit_depth: u8,
    /// The embedded cover's hash, when the song has one (for its thumbnail).
    pub picture: Option<Vec<u8>>,
}

pub struct Album {
    pub id: String,
    /// The album's folder, library-relative.
    pub dir: String,
    pub name: String,
    pub artist: String,
    pub artist_id: String,
    pub year: Option<u32>,
    pub genre: Option<String>,
    /// Songs in disc and track order.
    pub songs: Vec<usize>,
    pub duration_ms: u64,
    /// Newest first sorts by this: the highest file number Venuta gave any of its songs.
    pub added: i64,
}

pub struct Artist {
    pub id: String,
    pub name: String,
    pub albums: Vec<usize>,
}

pub struct Catalog {
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    song_ix: HashMap<String, usize>,
    album_ix: HashMap<String, usize>,
    artist_ix: HashMap<String, usize>,
    path_ix: HashMap<String, usize>,
}

/// A stable id: a kind letter pair and 16 hex digits of the key's hash.
pub fn id_for(kind: &str, key: &str) -> String {
    let h = blake3::hash(key.to_lowercase().as_bytes()).to_hex();
    format!("{kind}{}", &h[..16])
}

fn first<'a>(t: &'a Track, key: &str) -> Option<&'a str> {
    t.tags
        .get(key)
        .and_then(|v| v.first())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
}

fn number(s: Option<&str>) -> Option<u32> {
    s?.split('/').next()?.trim().parse().ok().filter(|n| *n > 0)
}

fn year(t: &Track) -> Option<u32> {
    let y = first(t, "ORIGINALDATE").or_else(|| first(t, "DATE"))?;
    y.get(..4)?
        .parse()
        .ok()
        .filter(|y| (1000..3000).contains(y))
}

impl Catalog {
    pub fn build(lib: &Library) -> Self {
        let mut songs = Vec::new();
        let mut album_of: HashMap<String, usize> = HashMap::new();
        let mut albums: Vec<Album> = Vec::new();
        for t in lib
            .tracks
            .iter()
            .filter(|t| t.kind == "flac" || t.kind == "other_audio")
        {
            let file = t.path.rsplit('/').next().unwrap_or(&t.path);
            let stem = file.rsplit_once('.').map_or(file, |x| x.0);
            let artists: Vec<&str> = t
                .tags
                .get("ARTIST")
                .map(|v| v.iter().map(String::as_str).collect())
                .unwrap_or_default();
            let artist = if artists.is_empty() {
                "Unknown artist".to_owned()
            } else {
                artists.join(", ")
            };
            let album_artist = first(t, "ALBUMARTIST")
                .map(str::to_owned)
                .unwrap_or_else(|| artists.first().map_or(artist.clone(), |a| (*a).to_owned()));
            let album_name = first(t, "ALBUM")
                .map(str::to_owned)
                .unwrap_or_else(|| t.dir.rsplit('/').next().unwrap_or("").to_owned());
            let a = *album_of.entry(t.dir.to_lowercase()).or_insert_with(|| {
                albums.push(Album {
                    id: id_for("al", &t.dir),
                    dir: t.dir.clone(),
                    name: album_name.clone(),
                    artist: album_artist.clone(),
                    artist_id: id_for("ar", &engine::audit::name_key(&album_artist)),
                    year: year(t),
                    genre: first(t, "GENRE").map(str::to_owned),
                    songs: Vec::new(),
                    duration_ms: 0,
                    added: 0,
                });
                albums.len() - 1
            });
            let album = &mut albums[a];
            album.songs.push(songs.len());
            album.duration_ms += t.duration_ms;
            album.added = album.added.max(t.id);
            songs.push(Song {
                id: id_for("so", &t.path),
                track_id: t.id,
                path: t.path.clone(),
                title: first(t, "TITLE").unwrap_or(stem).to_owned(),
                artist,
                album: album.name.clone(),
                album_id: album.id.clone(),
                artist_id: album.artist_id.clone(),
                track: number(first(t, "TRACKNUMBER")),
                disc: number(first(t, "DISCNUMBER")),
                year: year(t),
                genre: first(t, "GENRE").map(str::to_owned),
                duration_ms: t.duration_ms,
                size: t.size,
                suffix: file
                    .rsplit_once('.')
                    .map(|x| x.1.to_lowercase())
                    .unwrap_or_default(),
                sample_rate: t.sample_rate,
                bit_depth: t.bit_depth,
                picture: t.pictures.first().map(|p| p.data_hash.clone()),
            });
        }
        for album in &mut albums {
            album.songs.sort_by_key(|&s| {
                let s = &songs[s];
                (
                    s.disc.unwrap_or(1),
                    s.track.unwrap_or(u32::MAX),
                    s.path.clone(),
                )
            });
        }
        albums.sort_by_key(|a| a.name.to_lowercase());
        let mut artists: Vec<Artist> = Vec::new();
        let mut artist_of: HashMap<String, usize> = HashMap::new();
        for (i, album) in albums.iter().enumerate() {
            let k = *artist_of.entry(album.artist_id.clone()).or_insert_with(|| {
                artists.push(Artist {
                    id: album.artist_id.clone(),
                    name: album.artist.clone(),
                    albums: Vec::new(),
                });
                artists.len() - 1
            });
            artists[k].albums.push(i);
        }
        artists.sort_by_key(|a| a.name.to_lowercase());
        Self {
            song_ix: songs
                .iter()
                .enumerate()
                .map(|(i, s)| (s.id.clone(), i))
                .collect(),
            path_ix: songs
                .iter()
                .enumerate()
                .map(|(i, s)| (s.path.to_lowercase(), i))
                .collect(),
            album_ix: albums
                .iter()
                .enumerate()
                .map(|(i, a)| (a.id.clone(), i))
                .collect(),
            artist_ix: artists
                .iter()
                .enumerate()
                .map(|(i, a)| (a.id.clone(), i))
                .collect(),
            songs,
            albums,
            artists,
        }
    }

    pub fn song(&self, id: &str) -> Option<&Song> {
        self.song_ix.get(id).map(|&i| &self.songs[i])
    }
    pub fn album(&self, id: &str) -> Option<&Album> {
        self.album_ix.get(id).map(|&i| &self.albums[i])
    }
    pub fn artist(&self, id: &str) -> Option<&Artist> {
        self.artist_ix.get(id).map(|&i| &self.artists[i])
    }
    pub fn by_path(&self, path: &str) -> Option<&Song> {
        self.path_ix
            .get(&path.to_lowercase())
            .map(|&i| &self.songs[i])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn track(id: i64, path: &str, tags: &[(&str, &str)]) -> Track {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in tags {
            map.entry((*k).to_owned())
                .or_default()
                .push((*v).to_owned());
        }
        Track {
            id,
            path: path.into(),
            dir: path.rsplit_once('/').map_or("", |x| x.0).into(),
            kind: "flac".into(),
            size: 30_000_000,
            duration_ms: 240_000,
            sample_rate: 96_000,
            bit_depth: 24,
            audio_md5: None,
            vendor: None,
            tags: map,
            raw_keys: Vec::new(),
            pictures: Vec::new(),
            quality: None,
        }
    }

    pub fn library(tracks: Vec<Track>) -> Library {
        Library::of_tracks(tracks)
    }

    pub fn sample() -> Library {
        library(vec![
            track(
                3,
                "Music/Jazz/Miles Davis/Kind of Blue (1959)/02 - Freddie Freeloader.flac",
                &[
                    ("TITLE", "Freddie Freeloader"),
                    ("ARTIST", "Miles Davis"),
                    ("ALBUM", "Kind of Blue"),
                    ("TRACKNUMBER", "2"),
                    ("DATE", "1959"),
                    ("GENRE", "Jazz"),
                ],
            ),
            track(
                2,
                "Music/Jazz/Miles Davis/Kind of Blue (1959)/01 - So What.flac",
                &[
                    ("TITLE", "So What"),
                    ("ARTIST", "Miles Davis"),
                    ("ALBUM", "Kind of Blue"),
                    ("TRACKNUMBER", "1/5"),
                    ("DATE", "1959-08-17"),
                ],
            ),
            track(
                7,
                "Music/Hindi/Lucky Ali/Sifar (1996)/01 - O Sanam.flac",
                &[
                    ("TITLE", "O Sanam"),
                    ("ARTIST", "Lucky Ali"),
                    ("ALBUM", "Sifar"),
                    ("ALBUMARTIST", "Lucky Ali"),
                ],
            ),
        ])
    }

    #[test]
    fn songs_gather_into_albums_in_track_order_with_ids_that_stay_put() {
        let c = Catalog::build(&sample());
        assert_eq!(c.albums.len(), 2);
        let kob = c.albums.iter().find(|a| a.name == "Kind of Blue").unwrap();
        let titles: Vec<&str> = kob
            .songs
            .iter()
            .map(|&s| c.songs[s].title.as_str())
            .collect();
        assert_eq!(titles, ["So What", "Freddie Freeloader"]);
        assert_eq!(kob.year, Some(1959));
        assert_eq!(kob.added, 3);
        assert_eq!(kob.duration_ms, 480_000);
        // Ids come from paths and names: the same library gives the same ids.
        let again = Catalog::build(&sample());
        assert_eq!(again.album(&kob.id).unwrap().name, "Kind of Blue");
        let so_what = c
            .by_path("music/jazz/miles davis/kind of blue (1959)/01 - so what.flac")
            .unwrap();
        assert_eq!(so_what.album_id, kob.id);
        assert_eq!(so_what.track, Some(1));
        assert_eq!(c.song(&so_what.id).unwrap().title, "So What");
        // Artists by album artist, each with their albums.
        assert_eq!(c.artists.len(), 2);
        let miles = c.artist(&kob.artist_id).unwrap();
        assert_eq!(
            (miles.name.as_str(), miles.albums.len()),
            ("Miles Davis", 1)
        );
    }
}
