//! Small persistent settings: where the rules live, which folder is open, your keys and look,
//! and the owner's review decisions per album. Plain JSON in the library's `.venuta` folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Folder holding the rules TOML files.
    pub rules_dir: Option<String>,
    /// Where the migrated library is built. Kept for older files; your library (see
    /// `paths::home`) is the one place it goes now.
    pub new_library: Option<String>,
    /// The folder showing in the app: your library, or a music folder being brought in.
    #[serde(default)]
    pub open_folder: Option<String>,
    /// Look and feel (theme, surface, accent…), as the app stores it.
    #[serde(default)]
    pub appearance: Option<serde_json::Value>,
    /// The owner confirmed they have a backup; required before editing files in place.
    #[serde(default)]
    pub backup_confirmed: bool,
    /// Your Apple Music library file and its key (entered by you; never shipped with the app).
    #[serde(default)]
    pub apple_library: Option<String>,
    #[serde(default)]
    pub apple_key: Option<String>,
    /// AcoustID application key: identifies files without tags by their sound (optional).
    #[serde(default)]
    pub acoustid_key: Option<String>,
    /// Discogs personal access token: credits and original years for releases MusicBrainz
    /// lacks (optional).
    #[serde(default)]
    pub discogs_token: Option<String>,
    /// Which free sources "Look up details online" asks.
    #[serde(default)]
    pub sources: Sources,
    /// What closing the window does: "tray" (keep playing there) or "quit". None until you
    /// are asked, the first time you close it.
    #[serde(default)]
    pub close_action: Option<String>,
    /// Your Spotify app's Client ID (not a secret), for signing in to read playlists.
    #[serde(default)]
    pub spotify_client_id: Option<String>,
    /// How listening goes: what plays next, what is never offered, what is remembered.
    #[serde(default)]
    pub listening: Listening,
    /// A proxy every online request goes through (`http://host:port`, `socks5://host:port`),
    /// for networks that only let traffic out that way. Used from the next start.
    #[serde(default)]
    pub proxy: Option<String>,
    /// Which new versions are offered: "stable" (releases), or "beta" (pre-releases too).
    #[serde(default)]
    pub update_channel: Option<String>,
}

/// A proxy address as it may be used: `http`, `https`, `socks5` or `socks5h`, then a host and a
/// port, with a user and password if the proxy wants them. Anything else is not used.
pub fn proxy_address(raw: &str) -> Option<String> {
    let s = raw.trim();
    let (scheme, rest) = s.split_once("://")?;
    if !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "socks5" | "socks5h"
    ) {
        return None;
    }
    let host_port = rest.rsplit('@').next()?.trim_end_matches('/');
    let (host, port) = host_port.rsplit_once(':')?;
    let port_ok = port.parse::<u16>().is_ok_and(|p| p > 0);
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'));
    (port_ok && host_ok && !rest.contains(char::is_whitespace)).then(|| {
        format!(
            "{}://{}",
            scheme.to_ascii_lowercase(),
            rest.trim_end_matches('/')
        )
    })
}

/// How listening goes. Each choice is yours; the defaults are a music app's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listening {
    /// When the queue runs out, keep playing songs like the last one (a radio).
    #[serde(default = "yes")]
    pub autoplay: bool,
    /// Open Now Playing when you press play on something.
    #[serde(default)]
    pub open_player_on_play: bool,
    /// Artists radio, autoplay and suggestions never offer.
    #[serde(default)]
    pub blocked_artists: Vec<BlockedArtist>,
    /// Online songs you disliked (`provider:id`): radio and autoplay skip them.
    #[serde(default)]
    pub disliked: Vec<String>,
    /// Leave music videos out of search and of what plays next.
    #[serde(default)]
    pub hide_videos: bool,
    /// Keep a history of what you play, from your files and online.
    #[serde(default = "yes")]
    pub history: bool,
    /// Online stream quality: "auto" (the best a song offers) or "low" (data saver).
    #[serde(default = "auto")]
    pub stream_quality: String,
    /// Which country the online catalog answers for (its charts, home and search); empty: the
    /// computer's own region.
    #[serde(default)]
    pub catalog_region: String,
    /// Say each new song in a Windows notification while Venuta is in the background.
    #[serde(default)]
    pub song_notifications: bool,
    /// Signed in to YouTube Music: liking, unliking or disliking an online song in Venuta does
    /// the same on the account. Off until you switch it on.
    #[serde(default)]
    pub sync_likes: bool,
    /// What you pinned to the top of Home (albums, artists, playlists; yours or online), as the
    /// app keeps them.
    #[serde(default)]
    pub shortcuts: Vec<serde_json::Value>,
    /// The lyrics sources in your order, "-id" for one switched off (see
    /// `lyrics_online::order`); empty: the default order, all on.
    #[serde(default)]
    pub lyrics_sources: Vec<String>,
}

/// An artist never to be offered: by their catalog id when known, and by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedArtist {
    pub name: String,
    #[serde(default)]
    pub artist_id: Option<String>,
}

fn auto() -> String {
    "auto".into()
}

impl Default for Listening {
    fn default() -> Self {
        Listening {
            autoplay: true,
            open_player_on_play: false,
            blocked_artists: Vec::new(),
            disliked: Vec::new(),
            hide_videos: false,
            history: true,
            stream_quality: auto(),
            catalog_region: String::new(),
            song_notifications: false,
            sync_likes: false,
            shortcuts: Vec::new(),
            lyrics_sources: Vec::new(),
        }
    }
}

/// The free online sources, each on unless switched off.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sources {
    #[serde(default = "yes")]
    pub musicbrainz: bool,
    #[serde(default = "yes")]
    pub apple: bool,
    #[serde(default = "yes")]
    pub deezer: bool,
    #[serde(default = "yes")]
    pub lrclib: bool,
}

fn yes() -> bool {
    true
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            musicbrainz: true,
            apple: true,
            deezer: true,
            lrclib: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approved,
    Skipped,
}

fn settings_path() -> PathBuf {
    crate::paths::app_data_dir().join("settings.json")
}

fn decisions_path(root: &Path) -> PathBuf {
    crate::paths::index_path_for(root).with_file_name("decisions.json")
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.part");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

pub fn load() -> Settings {
    read_json(&settings_path())
}

pub fn save(s: &Settings) -> Result<(), String> {
    write_json(&settings_path(), s)
}

pub fn load_decisions(root: &Path) -> BTreeMap<String, Decision> {
    read_json(&decisions_path(root))
}

pub fn save_decisions(root: &Path, d: &BTreeMap<String, Decision>) -> Result<(), String> {
    write_json(&decisions_path(root), d)
}

fn answers_path(root: &Path) -> PathBuf {
    crate::paths::index_path_for(root).with_file_name("answers.json")
}

/// The owner's answers to decisions for one library (see `crate::review`).
pub fn load_answers(root: &Path) -> crate::review::Answers {
    read_json(&answers_path(root))
}

pub fn save_answers(root: &Path, a: &crate::review::Answers) -> Result<(), String> {
    write_json(&answers_path(root), a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn likes_go_to_youtube_music_only_once_switched_on() {
        let old: Listening = serde_json::from_str(r#"{"autoplay": true}"#).unwrap();
        assert!(!old.sync_likes, "settings from before stay as they were");
        assert!(!Listening::default().sync_likes);
    }

    #[test]
    fn a_proxy_address_is_a_known_scheme_a_host_and_a_port() {
        assert_eq!(
            proxy_address(" http://10.0.0.2:3128/ ").as_deref(),
            Some("http://10.0.0.2:3128")
        );
        assert_eq!(
            proxy_address("SOCKS5://proxy.example.org:1080").as_deref(),
            Some("socks5://proxy.example.org:1080")
        );
        assert_eq!(
            proxy_address("http://me:secret@proxy:8080").as_deref(),
            Some("http://me:secret@proxy:8080")
        );
        assert_eq!(
            proxy_address("proxy:8080"),
            None,
            "a scheme says how to talk to it"
        );
        assert_eq!(proxy_address("ftp://proxy:21"), None);
        assert_eq!(proxy_address("http://proxy"), None, "a port is needed");
        assert_eq!(proxy_address("http://proxy:99999"), None);
        assert_eq!(proxy_address("http://pro xy:80"), None);
    }
}
