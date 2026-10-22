//! Every message between a guest's Venuta and the host's, as JSON lines (one message, one line).
//!
//! Peers are never trusted: each message has a size limit, unknown fields are refused, every
//! string and list is bounded, online ids must look like the catalog's, pictures must be https,
//! and no message ever carries a file path (the host's songs are known by a hash, and their
//! bytes are only ever streamed by the host on request).

/// Protocol 3, beside this one until the live sessions move to it (R2).
pub mod v3;

use crate::timeline::Timeline;

use serde::{Deserialize, Serialize};

/// The longest message line accepted, in bytes (a full room with a long queue fits easily).
pub const MAX_MESSAGE: usize = 512 * 1024;
const MAX_TEXT: usize = 300;
const MAX_ARTISTS: usize = 20;
/// The most songs one message adds.
pub const MAX_ADD: usize = 200;
/// The longest room queue.
pub const MAX_QUEUE: usize = 2_000;

/// A song as the room knows it: enough for each listener to find their best copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Song {
    /// Stable within the room: "online:<id>" for a catalog song, "host:<hash>" for the host's file.
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// The recording code, the surest way to find your own copy.
    #[serde(default)]
    pub isrc: Option<String>,
    /// The MusicBrainz recording id, the next surest.
    #[serde(default)]
    pub mbid: Option<String>,
    /// The catalog's id for it (a catalog song, or the host's file's online twin).
    #[serde(default)]
    pub online_id: Option<String>,
    /// The host has it as a file and can stream it to anyone without a copy.
    #[serde(default)]
    pub host_file: bool,
    /// The host's file's format ("flac", "mp3"…), for a guest's player to read its stream.
    #[serde(default)]
    pub format: Option<String>,
    /// Its cover picture (https only).
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// Who put it in the room's queue (their member id), when someone did.
    #[serde(default)]
    pub added_by: Option<u32>,
}

/// A catalog id as the catalog writes them (YouTube: 11 letters, digits, - and _).
pub fn is_online_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn text_ok(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT && !s.chars().any(|c| c.is_control())
}

impl Song {
    /// Whether this song can be taken from a peer as it is.
    pub fn check(&self) -> Result<(), String> {
        let key_ok = match self.key.split_once(':') {
            Some(("online", id)) => is_online_id(id),
            Some(("host", h)) => h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()),
            _ => false,
        };
        if !key_ok {
            return Err("a song key Venuta doesn't know".into());
        }
        if self.title.trim().is_empty() || !text_ok(&self.title) {
            return Err("a song without a proper title".into());
        }
        if self.artists.len() > MAX_ARTISTS || !self.artists.iter().all(|a| text_ok(a)) {
            return Err("too many or too long artist names".into());
        }
        if !self.album.as_deref().is_none_or(text_ok) {
            return Err("an album name that's too long".into());
        }
        if !self
            .isrc
            .as_deref()
            .is_none_or(|i| i.len() <= 15 && i.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err("a recording code that isn't one".into());
        }
        if !self
            .mbid
            .as_deref()
            .is_none_or(|m| m.len() == 36 && m.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        {
            return Err("a MusicBrainz id that isn't one".into());
        }
        if !self
            .format
            .as_deref()
            .is_none_or(|f| f.len() <= 8 && f.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err("a format that isn't one".into());
        }
        if !self.online_id.as_deref().is_none_or(is_online_id) {
            return Err("a catalog id that isn't one".into());
        }
        if !self.artwork_url.as_deref().is_none_or(|u| {
            u.starts_with("https://") && u.len() <= 1_000 && !u.contains(char::is_whitespace)
        }) {
            return Err("a picture address that isn't https".into());
        }
        Ok(())
    }

    /// The key a host's file gets: a hash of its place in the host's library, which tells the
    /// host which file is meant without telling anyone where it is.
    pub fn host_key(library_path: &str) -> String {
        let h = blake3::hash(library_path.as_bytes()).to_hex();
        format!("host:{}", &h[..32])
    }
}

/// What a person may do in the room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Play, pause, skip, seek, reorder and add.
    Control,
    /// Add songs to the queue.
    Add,
    /// Listen only.
    Listen,
}

/// How a person hears the song playing now (shown beside their name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hearing {
    /// Getting it ready.
    Opening,
    /// Their own file.
    Local,
    /// Streaming it from the catalog themselves.
    Online,
    /// The host's file, streamed from the host.
    FromHost,
    /// They can't play this one (and are back for the next).
    Cannot,
    /// They stepped out for a moment.
    SteppedOut,
}

/// Something a person asks the room to do (the host checks their role first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Play,
    Pause,
    Seek { position_ms: u64 },
    Next,
    Previous,
    Jump { index: usize },
    Add { songs: Vec<Song>, next: bool },
    Remove { index: usize },
    Move { from: usize, to: usize },
}

impl Action {
    /// The least a role must allow for this.
    pub fn needs(&self) -> Role {
        match self {
            Action::Add { .. } => Role::Add,
            _ => Role::Control,
        }
    }

    fn check(&self) -> Result<(), String> {
        if let Action::Add { songs, .. } = self {
            if songs.is_empty() || songs.len() > MAX_ADD {
                return Err("between 1 and 200 songs at a time".into());
            }
            for s in songs {
                s.check()?;
            }
        }
        Ok(())
    }
}

/// What a guest's Venuta sends the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToHost {
    /// The first message: who's asking, and proof they hold the invite's secret.
    Hello {
        protocol: u32,
        name: String,
        proof: String,
        app: String,
    },
    /// A clock ping, at the guest's time t1 (microseconds).
    Ping { id: u32, t1: i64 },
    /// Ready to play this queue entry (its revision) from the position asked.
    Ready { rev: u64, index: usize },
    /// How they hear the song now.
    Hearing {
        rev: u64,
        index: usize,
        how: Hearing,
    },
    /// How far from the room they are, in milliseconds (for the panel).
    Sync { error_ms: i32 },
    /// Something to do to the room.
    Ask { action: Action },
    /// Undo the last skip or queue change (host only; a guest's is refused).
    Undo,
    /// Leaving.
    Bye,
}

/// A person, as everyone in the room sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub id: u32,
    pub name: String,
    pub host: bool,
    pub role: Role,
    pub hearing: Hearing,
    /// Their last reported distance from the room, in milliseconds.
    #[serde(default)]
    pub error_ms: Option<i32>,
    /// Still connecting again after a drop.
    #[serde(default)]
    pub away: bool,
}

/// One thing that happened, for the activity line ("Meera skipped").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activity {
    pub who: u32,
    /// Their name then (so it still reads right after they leave).
    pub name: String,
    /// Plain words, made by the host ("skipped", "added 2 songs").
    pub what: String,
    /// The host's time it happened (microseconds).
    pub at_us: i64,
    /// The host can undo it.
    pub undoable: bool,
}

/// The whole room, as the host sends it to everyone after each change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomView {
    pub host_name: String,
    pub members: Vec<Member>,
    /// Who's asking to come in (names only; the host sees them, guests see a count).
    pub waiting: Vec<(u32, String)>,
    pub queue: Vec<Song>,
    pub rev: u64,
    pub timeline: Option<Timeline>,
    pub activity: Vec<Activity>,
    /// The role new people get.
    pub new_role: Role,
    /// Speakers in one room: tight sync (correcting by single samples).
    pub same_room: bool,
    /// A song getting ready to start: who it still waits for, by name (empty: about to go).
    #[serde(default)]
    pub starting: Option<Vec<String>>,
}

/// What the host sends a guest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToGuest {
    /// Waiting for the host to let them in.
    Waiting,
    /// In: who they are in this room, and the room.
    Welcome { you: u32, room: RoomView },
    /// Not let in, or removed (with why, in words).
    Refused { why: String },
    /// A clock ping's answer: the guest's t1, and the host's receive and send times.
    Pong { id: u32, t1: i64, t2: i64, t3: i64 },
    /// The room changed.
    Room { room: RoomView },
    /// Get this queue entry ready at this position (answer Ready).
    Prepare {
        rev: u64,
        index: usize,
        position_ms: u64,
    },
    /// The room ended.
    Ended { why: String },
}

/// Read one message line from a peer, checked.
pub fn read<T: for<'de> Deserialize<'de>>(line: &[u8]) -> Result<T, String> {
    if line.len() > MAX_MESSAGE {
        return Err("a message too large".into());
    }
    serde_json::from_slice(line).map_err(|e| format!("a message Venuta can't read: {e}"))
}

/// Check what a guest sent, beyond its shape.
pub fn check_to_host(m: &ToHost) -> Result<(), String> {
    match m {
        ToHost::Hello {
            name, proof, app, ..
        } => {
            if name.trim().is_empty()
                || name.chars().count() > 40
                || name.chars().any(char::is_control)
            {
                return Err("a name between 1 and 40 letters".into());
            }
            if proof.len() != 64 || !proof.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("an invite proof that isn't one".into());
            }
            if app.len() > 60 {
                return Err("an app name that's too long".into());
            }
            Ok(())
        }
        ToHost::Ask { action } => action.check(),
        _ => Ok(()),
    }
}

/// One message as a line to send.
pub fn line<T: Serialize>(m: &T) -> Vec<u8> {
    let mut v = serde_json::to_vec(m).expect("messages serialize");
    v.push(b'\n');
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn song(id: &str) -> Song {
        Song {
            key: format!("online:{id}"),
            title: "Kesariya".into(),
            artists: vec!["Arijit Singh".into()],
            album: Some("Brahmastra".into()),
            duration_ms: Some(268_000),
            isrc: Some("INZ032201234".into()),
            mbid: None,
            online_id: Some(id.into()),
            host_file: false,
            format: None,
            artwork_url: Some("https://i.ytimg.com/vi/x/hq.jpg".into()),
            added_by: None,
        }
    }

    #[test]
    fn messages_go_and_come_back_the_same() {
        let m = ToHost::Ask {
            action: Action::Add {
                songs: vec![song("abcdefghijk")],
                next: true,
            },
        };
        let back: ToHost = read(&line(&m)).unwrap();
        assert_eq!(back, m);
        assert!(check_to_host(&back).is_ok());
    }

    #[test]
    fn a_song_from_a_peer_is_checked() {
        assert!(song("abcdefghijk").check().is_ok());
        assert!(
            song("abc").check().is_err(),
            "an id that isn't the catalog's"
        );
        let mut s = song("abcdefghijk");
        s.key = "file:C:/Users/me/song.flac".into();
        assert!(s.check().is_err(), "no file paths, ever");
        let mut s = song("abcdefghijk");
        s.artwork_url = Some("file:///C:/secret.jpg".into());
        assert!(s.check().is_err());
        let mut s = song("abcdefghijk");
        s.title = "x".repeat(400);
        assert!(s.check().is_err());
        let host = Song {
            key: Song::host_key("Arijit/Brahmastra/01.flac"),
            host_file: true,
            online_id: None,
            ..song("abcdefghijk")
        };
        assert!(host.check().is_ok());
        assert!(
            !host.key.contains("Arijit"),
            "the key tells nobody where the file is"
        );
    }

    #[test]
    fn strange_messages_are_refused() {
        assert!(
            read::<ToHost>(br#"{"t":"ping","id":1,"t1":5,"extra":true}"#).is_err(),
            "unknown fields"
        );
        assert!(read::<ToHost>(br#"{"t":"format_disk"}"#).is_err());
        assert!(
            read::<ToHost>(&vec![b' '; MAX_MESSAGE + 1]).is_err(),
            "too large"
        );
        let hello = ToHost::Hello {
            protocol: 1,
            name: "".into(),
            proof: "00".into(),
            app: "Venuta".into(),
        };
        assert!(check_to_host(&hello).is_err());
        let many = ToHost::Ask {
            action: Action::Add {
                songs: vec![song("abcdefghijk"); MAX_ADD + 1],
                next: false,
            },
        };
        assert!(check_to_host(&many).is_err());
    }

    #[test]
    fn adding_needs_less_than_controlling() {
        assert_eq!(
            Action::Add {
                songs: vec![],
                next: false
            }
            .needs(),
            Role::Add
        );
        assert_eq!(Action::Next.needs(), Role::Control);
    }
}
