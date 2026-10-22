//! Protocol 3 (section 5.7 of the Listen together design): the messages of Room v2, as JSON
//! lines, checked as strictly as protocol 2 (unknown fields refused, every string and list
//! bounded, songs checked, no file paths ever).
//!
//! What changed from protocol 2: queue entries have ids that never change (`EntryId`), so
//! moving, adding and removing songs never touches what plays; times are microseconds; a song
//! change is prepared while the music plays on and committed at one moment (`Prepare`, `Ready`,
//! `Commit`); a pause is scheduled (`Hold`); the next song is armed ahead with its boundary
//! (`ArmNext`, `Disarm`); each person reports their player's state and copy (`State`) and their
//! mode (`Mode`); a guest without the role can ask whoever controls (`Request`). Clock pings are
//! not control messages any more: they are the transport's datagrams.
//!
//! `Hello` and `Refused` keep protocol 2's exact shape, so a Venuta of either version reads the
//! other's first exchange and says "different versions" in words (`door`).

use super::{Activity, MAX_ADD, MAX_QUEUE, Role, Song, read, text_ok};
use crate::MAX_PEOPLE;
use crate::align::CopyClass;
use crate::anchor::{Anchor, EntryId, Next, PrepId, Purpose, Transition};
use crate::room_player::{ListenMode, SubState};
use crate::view::{CopyKind, CopyView};
use serde::{Deserialize, Serialize};

/// The protocol both sides of a Room v2 speak.
pub const PROTOCOL: u32 = 3;

/// What a host says to a Venuta speaking another protocol (today's words, so a 0.3 guest shows
/// the same).
pub const DIFFERENT_VERSIONS: &str =
    "Your Venuta and the host's are different versions. Update both, then join again.";

/// The longest name a person gives.
pub const MAX_NAME: usize = 40;
/// How many activity lines a view carries at most.
pub const MAX_ACTIVITY: usize = 20;
/// How many open requests a view carries at most.
pub const MAX_REQUESTS: usize = 10;
/// How many people may wait at the door at once.
pub const MAX_WAITING: usize = 50;
/// The longest place in a song a message may name (a day, in µs).
pub const MAX_CONTENT_US: i64 = 24 * 3_600 * 1_000_000;
const MAX_TICKET: usize = 2_000;
const MAX_ERROR_US: i64 = 3_600 * 1_000_000;
const MAX_LENGTH_DIFF_S: i32 = 86_400;

/// One song in the room's queue, with the id the host gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: EntryId,
    pub song: Song,
}

/// Something a person asks the room to do (the host checks their role first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Play,
    Pause,
    /// To this place of the entry playing (or being prepared), in µs of the reference copy.
    Seek {
        content_us: i64,
    },
    Next,
    /// Over 3 s in, this entry from its start; else the entry before.
    Previous,
    Jump {
        entry: EntryId,
    },
    /// At the end of the queue, or right after the entry playing (`next`).
    Add {
        songs: Vec<Song>,
        next: bool,
    },
    Remove {
        entry: EntryId,
    },
    /// The entry to this place in the queue (0 is the first).
    Move {
        entry: EntryId,
        to: usize,
    },
    /// The host's speaker's volume (one speaker mode), 0 to 100: the host's, or a remote's
    /// when the host lets guests change it.
    Volume {
        percent: u8,
    },
    /// Do what someone without the role asked (`Request`).
    Accept {
        request: u32,
    },
    Decline {
        request: u32,
    },
}

impl Action {
    /// The least role this needs (volume has its own permission, see `Room v2`).
    pub fn needs(&self) -> Role {
        match self {
            Action::Add { .. } => Role::Add,
            _ => Role::Control,
        }
    }

    /// Whether this can be taken from a peer as it is.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Action::Add { songs, .. } => {
                if songs.is_empty() || songs.len() > MAX_ADD {
                    return Err("between 1 and 200 songs at a time".into());
                }
                songs.iter().try_for_each(Song::check)
            }
            Action::Seek { content_us } if !(0..=MAX_CONTENT_US).contains(content_us) => {
                Err("a place in a song that isn't one".into())
            }
            Action::Move { to, .. } if *to >= MAX_QUEUE => Err("a place past the queue".into()),
            Action::Volume { percent } if *percent > 100 => {
                Err("a volume between 0 and 100".into())
            }
            _ => Ok(()),
        }
    }
}

/// Why someone can't play an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnableWhy {
    /// No copy anywhere for them.
    NoCopy,
    /// Their copy failed (a stream that never came back, a file that won't open).
    Failed,
}

/// The copy someone hears, as they report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyReport {
    pub kind: CopyKind,
    pub class: CopyClass,
    /// Another version: how much longer (positive) or shorter than the reference, in seconds.
    #[serde(default)]
    pub length_diff_s: Option<i32>,
}

impl From<CopyReport> for CopyView {
    fn from(c: CopyReport) -> CopyView {
        CopyView {
            kind: c.kind,
            class: c.class,
            length_diff_s: c.length_diff_s,
        }
    }
}

/// What a guest's Venuta sends the host (the host's own conductor reports the same, locally).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToHost {
    /// The first message: who's asking, and proof they hold the invite's secret (protocol 2's
    /// shape exactly).
    Hello {
        protocol: u32,
        name: String,
        proof: String,
        app: String,
    },
    /// Ready for this prepare (or this armed next: its prep).
    Ready { prep: PrepId, entry: EntryId },
    /// Can't play this entry (a prepare's or an armed next's).
    Unable {
        prep: PrepId,
        entry: EntryId,
        why: UnableWhy,
    },
    /// How their player is now (on a change, at most every 2 s).
    State {
        #[serde(default)]
        entry: Option<EntryId>,
        sub_state: SubState,
        #[serde(default)]
        error_us: Option<i64>,
        #[serde(default)]
        bound_us: Option<u32>,
        #[serde(default)]
        copy: Option<CopyReport>,
    },
    /// How they listen (decision D4: per person).
    Mode { mode: ListenMode },
    /// Something to do to the room (their role is checked).
    Ask { action: Action },
    /// Something to ask whoever controls (no role needed).
    Request { action: Action },
    /// Undo the last skip or queue change (host only).
    Undo,
    /// Leaving.
    Bye,
}

/// A person, as the room shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub id: u32,
    pub name: String,
    pub host: bool,
    pub role: Role,
    pub mode: ListenMode,
    /// Their player's state (`None` while away, or before they said).
    #[serde(default)]
    pub sub_state: Option<SubState>,
    /// Connection lost (or silent), kept for 2 minutes.
    #[serde(default)]
    pub away: bool,
    /// The copy they hear: only in their own view and the host's (decision D14).
    #[serde(default)]
    pub copy: Option<CopyReport>,
    #[serde(default)]
    pub error_us: Option<i64>,
    #[serde(default)]
    pub bound_us: Option<u32>,
}

/// Where the room's playback is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nothing has played yet ("Add songs to start").
    Resting,
    /// The last song ended ("The room's queue has ended").
    QueueEnded,
    Playing,
    Held,
}

/// The host's settings, as everyone sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Between songs (the room's, not anyone's own crossfade).
    pub transition: Transition,
    /// The role a guest has unless the host gave them one.
    pub new_role: Role,
    /// The host hasn't chosen: control up to 4 people, add above (decision D20).
    pub auto_roles: bool,
    /// Remotes may change the host's speaker's volume.
    pub guests_volume: bool,
}

/// Something asked of whoever controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestView {
    pub id: u32,
    pub from: u32,
    pub action: Action,
    /// The host's time it was asked.
    pub at_us: i64,
}

/// The room, as the host sends it to one person after each change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomView {
    pub host_name: String,
    pub members: Vec<Member>,
    /// Who's asking to come in (names for the host only; guests see a count).
    pub waiting: Vec<(u32, String)>,
    pub queue: Vec<Entry>,
    pub phase: Phase,
    pub anchor: Option<Anchor>,
    pub next: Option<Next>,
    /// A prepare under way: who it still waits for, by name (empty: about to go).
    #[serde(default)]
    pub starting: Option<Vec<String>>,
    pub activity: Vec<Activity>,
    pub settings: Settings,
    #[serde(default)]
    pub requests: Vec<RequestView>,
}

/// The old host's clock in the new host's time, for a hand-over (WP20):
/// `to = to_ref_us + (from - from_ref_us) x rate`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockLine {
    pub from_ref_us: i64,
    pub to_ref_us: i64,
    pub rate: f64,
}

/// What the host sends a guest (and its own conductor, locally).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToGuest {
    /// Waiting for the host to let them in.
    Waiting,
    /// In: who they are in this room, and the room.
    Welcome { you: u32, room: RoomView },
    /// Not let in, or removed, with why in words (protocol 2's shape exactly).
    Refused { why: String },
    /// The room ended, with why in words.
    Ended { why: String },
    /// The room changed.
    Room { room: RoomView },
    /// Get this entry ready at this place (answer `Ready`, or `Unable`); whatever plays keeps
    /// playing until the commit.
    Prepare {
        prep: PrepId,
        entry: EntryId,
        content_us: i64,
        purpose: Purpose,
    },
    /// Start, switch or resume so the anchor holds.
    Commit { anchor: Anchor },
    /// A scheduled pause: playing until `anchor.host_us`, then held at `anchor.content_us`.
    Hold { anchor: Anchor },
    /// The entry after the playing one, to switch to at its boundary by itself.
    ArmNext { prep: PrepId, next: Next },
    /// The armed next is withdrawn (nothing follows the playing entry now).
    Disarm { seq: u64 },
    /// The room moves to another host (WP20): reconnect there as the same member.
    HandOver {
        to: u32,
        ticket: String,
        #[serde(default)]
        anchor: Option<Anchor>,
        #[serde(default)]
        next: Option<Next>,
        line: ClockLine,
    },
    /// The host suggests something to this person (G7, for someone who missed three starts).
    Advise { advice: Advice },
}

/// What the host can suggest to one person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Advice {
    /// Play a lighter copy from now on (the catalog's stream before the host's file).
    LighterCopy,
    /// Stop catching up and start cleanly with the next song (A17).
    StartWithNext,
}

fn name_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= MAX_NAME && !s.chars().any(char::is_control)
}

fn anchor_ok(a: &Anchor) -> bool {
    (0..=MAX_CONTENT_US).contains(&a.content_us)
}

/// Check what a guest sent, beyond its shape.
pub fn check_to_host(m: &ToHost) -> Result<(), String> {
    match m {
        ToHost::Hello {
            name, proof, app, ..
        } => {
            if !name_ok(name) {
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
        ToHost::Ask { action } | ToHost::Request { action } => action.check(),
        ToHost::State { error_us, copy, .. } => {
            if error_us.is_some_and(|e| e.abs() > MAX_ERROR_US) {
                return Err("an error that isn't one".into());
            }
            if copy
                .and_then(|c| c.length_diff_s)
                .is_some_and(|d| d.abs() > MAX_LENGTH_DIFF_S)
            {
                return Err("a length that isn't one".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

impl RoomView {
    /// Whether a view from the host can be taken as it is.
    pub fn check(&self) -> Result<(), String> {
        if !name_ok(&self.host_name) {
            return Err("a host name that isn't one".into());
        }
        if self.members.len() > MAX_PEOPLE || !self.members.iter().all(|m| name_ok(&m.name)) {
            return Err("too many people, or a name that isn't one".into());
        }
        if self.waiting.len() > MAX_WAITING
            || !self.waiting.iter().all(|(_, n)| n.is_empty() || name_ok(n))
        {
            return Err("too many waiting, or a name that isn't one".into());
        }
        if self.queue.len() > MAX_QUEUE {
            return Err("a queue that's too long".into());
        }
        self.queue.iter().try_for_each(|e| e.song.check())?;
        if self.activity.len() > MAX_ACTIVITY
            || !self
                .activity
                .iter()
                .all(|a| text_ok(&a.name) && text_ok(&a.what))
        {
            return Err("too much activity, or too long".into());
        }
        if self.requests.len() > MAX_REQUESTS {
            return Err("too many requests".into());
        }
        self.requests.iter().try_for_each(|r| r.action.check())?;
        if !self.anchor.as_ref().is_none_or(anchor_ok)
            || self
                .starting
                .as_ref()
                .is_some_and(|s| s.len() > MAX_PEOPLE || !s.iter().all(|n| name_ok(n)))
        {
            return Err("a room that isn't one".into());
        }
        Ok(())
    }
}

/// Check what the host sent, beyond its shape (a guest trusts the host no more than the host
/// trusts guests).
pub fn check_to_guest(m: &ToGuest) -> Result<(), String> {
    match m {
        ToGuest::Welcome { room, .. } | ToGuest::Room { room } => room.check(),
        ToGuest::Refused { why } | ToGuest::Ended { why } if !text_ok(why) => {
            Err("words that are too long".into())
        }
        ToGuest::Prepare { content_us, .. } if !(0..=MAX_CONTENT_US).contains(content_us) => {
            Err("a place in a song that isn't one".into())
        }
        ToGuest::Commit { anchor } | ToGuest::Hold { anchor } if !anchor_ok(anchor) => {
            Err("a place in a song that isn't one".into())
        }
        ToGuest::HandOver { ticket, line, .. }
            if ticket.len() > MAX_TICKET
                || !ticket.bytes().all(|b| b.is_ascii_graphic())
                || !line.rate.is_finite() =>
        {
            Err("a hand-over that isn't one".into())
        }
        _ => Ok(()),
    }
}

/// A guest's hello, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub name: String,
    pub proof: String,
    pub app: String,
}

/// Why a first line didn't get someone to the door.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotIn {
    /// A hello of another protocol: send `answer()` ("different versions"), then close.
    OtherVersion,
    /// Not a hello, or not a proper one: close without a word.
    NotAHello,
}

impl NotIn {
    /// What to send before closing, if anything.
    pub fn answer(self) -> Option<ToGuest> {
        match self {
            NotIn::OtherVersion => Some(ToGuest::Refused {
                why: DIFFERENT_VERSIONS.into(),
            }),
            NotIn::NotAHello => None,
        }
    }
}

/// The first line from a guest: their hello, checked, or why not. The version is read before
/// anything else, leniently, so a hello of any version (a later one with more fields too) is
/// told "different versions" in words.
pub fn door(line: &[u8]) -> Result<Hello, NotIn> {
    #[derive(Deserialize)]
    struct AnyHello {
        t: String,
        protocol: u32,
    }
    let any: AnyHello = read(line).map_err(|_| NotIn::NotAHello)?;
    if any.t != "hello" {
        return Err(NotIn::NotAHello);
    }
    if any.protocol != PROTOCOL {
        return Err(NotIn::OtherVersion);
    }
    let m = read::<ToHost>(line).map_err(|_| NotIn::NotAHello)?;
    check_to_host(&m).map_err(|_| NotIn::NotAHello)?;
    match m {
        ToHost::Hello {
            name, proof, app, ..
        } => Ok(Hello {
            name: name.trim().to_owned(),
            proof,
            app,
        }),
        _ => Err(NotIn::NotAHello),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::line;
    use crate::room_player::Suspension;

    fn song(id: &str) -> Song {
        Song {
            key: format!("online:{id}"),
            title: "Kesariya".into(),
            artists: vec!["Arijit Singh".into()],
            album: Some("Brahmastra".into()),
            duration_ms: Some(268_000),
            isrc: None,
            mbid: None,
            online_id: Some(id.into()),
            host_file: false,
            format: None,
            artwork_url: None,
            added_by: None,
        }
    }

    const PROOF: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn hello(protocol: u32) -> ToHost {
        ToHost::Hello {
            protocol,
            name: "Ravi".into(),
            proof: PROOF.into(),
            app: "Venuta 0.4.0".into(),
        }
    }

    fn anchor() -> Anchor {
        Anchor {
            entry: EntryId(4),
            content_us: 95_000_000,
            host_us: 7_000_000,
            playing: true,
            seq: 3,
        }
    }

    fn view() -> RoomView {
        RoomView {
            host_name: "Asha".into(),
            members: vec![Member {
                id: 0,
                name: "Asha".into(),
                host: true,
                role: Role::Control,
                mode: ListenMode::Apart,
                sub_state: Some(SubState::InStep),
                away: false,
                copy: Some(CopyReport {
                    kind: CopyKind::OwnFile,
                    class: CopyClass::Exact,
                    length_diff_s: None,
                }),
                error_us: Some(-300),
                bound_us: Some(900),
            }],
            waiting: vec![(3, "Meera".into())],
            queue: vec![Entry {
                id: EntryId(4),
                song: song("abcdefghijk"),
            }],
            phase: Phase::Playing,
            anchor: Some(anchor()),
            next: None,
            starting: None,
            activity: vec![],
            settings: Settings {
                transition: Transition::Crossfade { ms: 4_000 },
                new_role: Role::Control,
                auto_roles: true,
                guests_volume: false,
            },
            requests: vec![RequestView {
                id: 1,
                from: 2,
                action: Action::Next,
                at_us: 5,
            }],
        }
    }

    fn every_to_host() -> Vec<ToHost> {
        vec![
            hello(3),
            ToHost::Ready {
                prep: PrepId(17),
                entry: EntryId(4),
            },
            ToHost::Unable {
                prep: PrepId(17),
                entry: EntryId(4),
                why: UnableWhy::NoCopy,
            },
            ToHost::State {
                entry: Some(EntryId(4)),
                sub_state: SubState::Suspended(Suspension::Buffering),
                error_us: Some(1_200),
                bound_us: Some(800),
                copy: Some(CopyReport {
                    kind: CopyKind::Catalog,
                    class: CopyClass::Approximate,
                    length_diff_s: Some(12),
                }),
            },
            ToHost::Mode {
                mode: ListenMode::OneSpeaker,
            },
            ToHost::Ask {
                action: Action::Add {
                    songs: vec![song("abcdefghijk")],
                    next: true,
                },
            },
            ToHost::Ask {
                action: Action::Move {
                    entry: EntryId(9),
                    to: 0,
                },
            },
            ToHost::Request {
                action: Action::Seek {
                    content_us: 95_000_000,
                },
            },
            ToHost::Undo,
            ToHost::Bye,
        ]
    }

    fn every_to_guest() -> Vec<ToGuest> {
        vec![
            ToGuest::Waiting,
            ToGuest::Welcome {
                you: 2,
                room: view(),
            },
            ToGuest::Refused {
                why: "The room is full (10 people).".into(),
            },
            ToGuest::Ended {
                why: "Asha ended the room.".into(),
            },
            ToGuest::Room { room: view() },
            ToGuest::Prepare {
                prep: PrepId(17),
                entry: EntryId(4),
                content_us: 95_000_000,
                purpose: Purpose::Seek,
            },
            ToGuest::Commit { anchor: anchor() },
            ToGuest::Hold { anchor: anchor() },
            ToGuest::ArmNext {
                prep: PrepId(18),
                next: Next {
                    entry: EntryId(5),
                    boundary_host_us: 9,
                    transition: Transition::Gapless,
                    seq: 4,
                },
            },
            ToGuest::Disarm { seq: 5 },
            ToGuest::HandOver {
                to: 2,
                ticket: "endpointabc".into(),
                anchor: Some(anchor()),
                next: None,
                line: ClockLine {
                    from_ref_us: 1,
                    to_ref_us: 2,
                    rate: 1.000_01,
                },
            },
        ]
    }

    #[test]
    fn every_message_goes_and_comes_back_the_same() {
        for m in every_to_host() {
            let back: ToHost = read(&line(&m)).unwrap();
            assert_eq!(back, m);
            assert!(check_to_host(&back).is_ok(), "{back:?}");
        }
        for m in every_to_guest() {
            let back: ToGuest = read(&line(&m)).unwrap();
            assert_eq!(back, m);
            assert!(check_to_guest(&back).is_ok(), "{back:?}");
        }
    }

    #[test]
    fn the_wire_shapes_stay_as_agreed() {
        let l = |m: &ToGuest| String::from_utf8(line(m)).unwrap();
        assert_eq!(
            l(&ToGuest::Prepare {
                prep: PrepId(17),
                entry: EntryId(4),
                content_us: 95_000_000,
                purpose: Purpose::Seek,
            }),
            "{\"t\":\"prepare\",\"prep\":17,\"entry\":4,\"content_us\":95000000,\"purpose\":\"seek\"}\n"
        );
        assert_eq!(
            l(&ToGuest::Disarm { seq: 5 }),
            "{\"t\":\"disarm\",\"seq\":5}\n"
        );
        assert_eq!(
            l(&ToGuest::Advise {
                advice: Advice::StartWithNext
            }),
            "{\"t\":\"advise\",\"advice\":\"start_with_next\"}\n"
        );
        assert!(
            read::<ToGuest>(b"{\"t\":\"advise\",\"advice\":\"louder\"}").is_err(),
            "only the advice there is"
        );
        let a = String::from_utf8(line(&ToHost::Ask {
            action: Action::Jump { entry: EntryId(9) },
        }))
        .unwrap();
        assert_eq!(
            a,
            "{\"t\":\"ask\",\"action\":{\"do\":\"jump\",\"entry\":9}}\n"
        );
        let s = String::from_utf8(line(&ToHost::State {
            entry: None,
            sub_state: SubState::Suspended(Suspension::Sleeping),
            error_us: None,
            bound_us: None,
            copy: None,
        }))
        .unwrap();
        assert!(
            s.contains("\"sub_state\":{\"state\":\"suspended\",\"why\":\"sleeping\"}"),
            "{s}"
        );
    }

    #[test]
    fn strange_messages_are_refused() {
        assert!(
            read::<ToHost>(br#"{"t":"ready","prep":1,"entry":2,"extra":true}"#).is_err(),
            "unknown fields"
        );
        assert!(
            read::<ToHost>(br#"{"t":"ping","id":1,"t1":5}"#).is_err(),
            "no pings on the control stream in protocol 3"
        );
        assert!(read::<ToHost>(br#"{"t":"ready","rev":1,"index":0}"#).is_err());
        assert!(
            read::<ToGuest>(br#"{"t":"prepare","rev":1,"index":0,"position_ms":0}"#).is_err(),
            "protocol 2's prepare"
        );
        let big = ToHost::Ask {
            action: Action::Add {
                songs: vec![song("abcdefghijk"); MAX_ADD + 1],
                next: false,
            },
        };
        assert!(check_to_host(&big).is_err());
        let path = ToHost::Request {
            action: Action::Add {
                songs: vec![Song {
                    key: "file:C:/Users/me/song.flac".into(),
                    ..song("abcdefghijk")
                }],
                next: false,
            },
        };
        assert!(check_to_host(&path).is_err(), "no file paths, ever");
        for a in [
            Action::Seek { content_us: -1 },
            Action::Seek {
                content_us: MAX_CONTENT_US + 1,
            },
            Action::Move {
                entry: EntryId(1),
                to: MAX_QUEUE,
            },
            Action::Volume { percent: 101 },
        ] {
            assert!(
                check_to_host(&ToHost::Ask { action: a.clone() }).is_err(),
                "{a:?}"
            );
        }
        let far = ToHost::State {
            entry: None,
            sub_state: SubState::InStep,
            error_us: Some(MAX_ERROR_US + 1),
            bound_us: None,
            copy: None,
        };
        assert!(check_to_host(&far).is_err());
        let nameless = ToHost::Hello {
            protocol: 3,
            name: "  ".into(),
            proof: PROOF.into(),
            app: "Venuta".into(),
        };
        assert!(check_to_host(&nameless).is_err());
    }

    #[test]
    fn a_guest_checks_what_the_host_sends() {
        let mut v = view();
        v.queue[0].song.key = "file:C:/secret.flac".into();
        assert!(check_to_guest(&ToGuest::Room { room: v }).is_err());
        let mut v = view();
        v.host_name = "x".repeat(80);
        assert!(check_to_guest(&ToGuest::Room { room: v }).is_err());
        let mut v = view();
        v.activity = vec![
            Activity {
                who: 0,
                name: "Asha".into(),
                what: "skipped".into(),
                at_us: 0,
                undoable: false,
            };
            MAX_ACTIVITY + 1
        ];
        assert!(check_to_guest(&ToGuest::Welcome { you: 1, room: v }).is_err());
        let mut a = anchor();
        a.content_us = -5;
        assert!(check_to_guest(&ToGuest::Commit { anchor: a }).is_err());
        assert!(
            check_to_guest(&ToGuest::Ended {
                why: "x".repeat(400)
            })
            .is_err()
        );
        assert!(
            check_to_guest(&ToGuest::HandOver {
                to: 1,
                ticket: "with space".into(),
                anchor: None,
                next: None,
                line: ClockLine {
                    from_ref_us: 0,
                    to_ref_us: 0,
                    rate: 1.0
                },
            })
            .is_err()
        );
    }

    #[test]
    fn a_hello_of_another_version_is_told_in_words_both_ways() {
        // A 0.3 guest (protocol 2) at a protocol 3 host: its hello, as 0.3 writes it.
        let v2_hello = br#"{"t":"hello","protocol":2,"name":"Ravi","proof":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","app":"Venuta 0.3.0"}"#;
        let refusal = door(v2_hello)
            .unwrap_err()
            .answer()
            .expect("told, not just closed");
        assert_eq!(
            refusal,
            ToGuest::Refused {
                why: DIFFERENT_VERSIONS.into()
            }
        );
        // ... and the 0.3 guest reads that refusal as its own message.
        let back: crate::proto::ToGuest = read(&line(&refusal)).unwrap();
        assert_eq!(
            back,
            crate::proto::ToGuest::Refused {
                why: DIFFERENT_VERSIONS.into()
            }
        );
        // A protocol 3 guest at a 0.3 host: the 0.3 host reads the hello as its own, sees the
        // protocol, and refuses with the same words, which this guest reads.
        let v3_hello = line(&hello(3));
        let as_v2: crate::proto::ToHost = read(&v3_hello[..v3_hello.len() - 1]).unwrap();
        let crate::proto::ToHost::Hello { protocol, .. } = as_v2 else {
            panic!("a 0.3 host reads a protocol 3 hello")
        };
        assert_eq!(protocol, 3);
        assert!(crate::proto::check_to_host(&as_v2).is_ok());
        let from_v2 = crate::proto::line(&crate::proto::ToGuest::Refused {
            why: DIFFERENT_VERSIONS.into(),
        });
        assert_eq!(
            read::<ToGuest>(&from_v2[..from_v2.len() - 1]).unwrap(),
            ToGuest::Refused {
                why: DIFFERENT_VERSIONS.into()
            }
        );
        // A later version, with fields this one doesn't know: told in words too.
        let v4 = br#"{"t":"hello","protocol":4,"name":"Ravi","proof":"x","app":"Venuta 0.9","caps":[1]}"#;
        assert_eq!(door(v4), Err(NotIn::OtherVersion));
        // Our own version: in, checked.
        let me = door(&v3_hello[..v3_hello.len() - 1]).unwrap();
        assert_eq!(me.name, "Ravi");
        // Not a hello, or a bad one: closed without a word.
        assert_eq!(door(br#"{"t":"bye"}"#), Err(NotIn::NotAHello));
        assert_eq!(door(b"not json"), Err(NotIn::NotAHello));
        let bad = line(&ToHost::Hello {
            protocol: 3,
            name: "Ravi".into(),
            proof: "00".into(),
            app: "Venuta".into(),
        });
        assert_eq!(door(&bad[..bad.len() - 1]), Err(NotIn::NotAHello));
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
        assert_eq!(Action::Accept { request: 1 }.needs(), Role::Control);
    }
}
