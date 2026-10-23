//! What the window shows of a room (contract 5.9 of the Listen together design): today's view,
//! with per person their mode, their player's state in one plain line and the copy they hear;
//! for you, your state and the press under way; the room's transition; and this output's delay.
//!
//! The lines are made here (section 4.12), so every platform's window says the same. The fixture
//! of every state the window must draw (`app/tests/fixtures/together-views.json`) is made from
//! these types by this file's tests: run them with `UPDATE_FIXTURES=1` after changing a shape or
//! a word, and the window's tests read the new file.

use crate::align::CopyClass;
use crate::anchor::{Anchor, EntryId, Next, Transition};
use crate::proto::{Activity, Role, Song};
use crate::room_player::{ListenMode, SubState, Suspension};
use serde::{Deserialize, Serialize};

/// Where this Venuta is with rooms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Off,
    Starting,
    Joining,
    Hosting,
    Waiting,
    In,
    Reconnecting,
    Ended,
}

/// The whole view, sent to the window after every change and 10 times a second while playing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub state: SessionState,
    pub host: bool,
    /// Your id in the room.
    pub me: Option<u32>,
    pub room: Option<RoomView>,
    pub invite: Option<InviteView>,
    /// How the connection to the host goes (a guest's).
    pub path: Option<PathView>,
    /// Why it ended, or what went wrong.
    pub message: Option<String>,
    /// Your name, as others see it.
    pub name: String,
    pub local_only: bool,
    /// The host lets people with the link in without asking.
    pub let_in: bool,
    /// You, while in a room.
    pub you: Option<You>,
    /// The output you hear the room on, and its delay.
    pub output: OutputView,
    /// The host's time when this view was made, from your clock line (`None` before it is
    /// ready): a remote's progress is the anchor's place at this time, moved on since.
    pub host_now_us: Option<i64>,
    /// Rooms open on this network (the host lets them be seen; joining one still knocks).
    pub nearby: Vec<NearbyView>,
    /// The host's room can be seen on this network.
    pub visible: bool,
    /// A room to go back to: after a crash or restart, while its invite is valid.
    pub resume: Option<Resume>,
    /// After a room: how many songs its queue had, for "Save the room's queue as a playlist".
    pub last_queue: Option<u32>,
}

/// A room open on this network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NearbyView {
    pub id: u32,
    pub host: String,
    pub listening: u32,
}

/// A room to go back to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resume {
    /// A guest's: "Rejoin Asha's room?"
    Rejoin { host: String },
    /// A host's: "Reopen your room?"
    Reopen,
}

/// A mode you may take now, in the order offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeChoice {
    pub mode: ListenMode,
    /// Why it can't be had now, in words ("Hide my IP is on"); `None`: it can.
    pub why_not: Option<String>,
}

/// Why you stepped out of the room's sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOut {
    /// "Pause for me".
    Chosen,
    /// The sleep timer ran out.
    SleepTimer,
    /// Headphones unplugged or a Bluetooth output gone.
    Unplugged,
}

/// Where the room's playback is (the same words as protocol 3's).
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

/// Something asked of whoever controls ("Ravi asks to skip").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestView {
    pub id: u32,
    pub from: u32,
    pub words: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InviteView {
    pub link: String,
    pub code: String,
    pub qr_svg: String,
    pub host: String,
    /// When it stops letting people in (seconds since 1970).
    pub until: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathView {
    pub direct: bool,
    pub relay: bool,
    pub rtt_ms: Option<u32>,
}

/// You, in the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct You {
    pub mode: ListenMode,
    pub sub_state: SubState,
    /// Your line, as beside your name.
    pub line: String,
    /// A press on its way to the room ("Pausing...", "Moving to 1:35...", "Skipping..."), until
    /// the room commits it.
    pub pressing: Option<String>,
    /// While joining: how much of the first song is in, 0 to 100.
    pub joining_pct: Option<u8>,
    /// The modes you may take now, in the order offered (one speaker first on the host's
    /// network; empty over the internet).
    pub choices: Vec<ModeChoice>,
    /// Why you stepped out, while you are.
    pub stepped_out: Option<StepOut>,
}

/// The output you hear the room on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputView {
    pub name: String,
    /// Its extra delay setting (Bluetooth, a receiver), -500 to 500 ms.
    pub delay_ms: i32,
    pub bluetooth: bool,
    /// The delay is known (measured, reported by the platform, or set by the person); every
    /// device mode needs it.
    pub delay_known: bool,
}

/// The room, as the window shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomView {
    pub host_name: String,
    pub members: Vec<MemberView>,
    /// Who's asking to come in (names only; guests see a count).
    pub waiting: Vec<(u32, String)>,
    pub queue: Vec<EntryView>,
    pub anchor: Option<Anchor>,
    pub next: Option<Next>,
    pub transition: Transition,
    pub activity: Vec<Activity>,
    /// The role new people get.
    pub new_role: Role,
    /// A start getting ready: who it still waits for, by name (empty: about to go).
    pub starting: Option<Vec<String>>,
    /// The host is reconnecting: room controls are greyed.
    pub host_away: bool,
    pub phase: Phase,
    /// What people without the role asked (shown to whoever controls).
    pub requests: Vec<RequestView>,
    /// The host hasn't chosen the role for new people: control up to 4 people, add above.
    pub auto_roles: bool,
    /// Remotes may change the host's speaker's volume.
    pub guests_volume: bool,
    /// The host's speaker's volume, 0 to 100 (for the host, and remotes allowed to change it).
    pub volume: Option<u8>,
    /// The words of the song playing when only the host has it (sent to remotes by the host,
    /// WP18); a catalog song's words the window finds itself.
    #[serde(default)]
    pub lyrics: Option<RoomLyrics>,
}

/// A song's words, for the remote view: timed lines (ms, words) or plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomLyrics {
    /// The song they are the words of (`Song::key`).
    pub key: String,
    pub lines: Vec<(u32, String)>,
    pub text: String,
}

/// One song in the room's queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryView {
    pub id: EntryId,
    pub song: Song,
}

/// A person, as the room panel shows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberView {
    pub id: u32,
    pub name: String,
    pub host: bool,
    pub role: Role,
    pub mode: ListenMode,
    /// Their player's state (`None` while away).
    pub sub_state: Option<SubState>,
    /// Connection lost, kept for 2 minutes.
    pub away: bool,
    /// One short line in plain words (section 4.12).
    pub line: String,
    /// The copy they hear (the host sees everyone's; a guest sees their own).
    pub copy: Option<CopyView>,
    /// The milliseconds behind the line, for the disclosure: their error and the clock's bound.
    pub error_ms: Option<f32>,
    pub bound_ms: Option<f32>,
    /// Missed three prepares in a row (the host's view).
    pub slow: bool,
    /// `None`: can't take the room over; else how many people would lose songs only the host
    /// has if they did (the host's view).
    pub hand_over: Option<u32>,
}

/// Which copy a person plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyKind {
    /// Their own file.
    OwnFile,
    /// The catalog's stream (YouTube Music).
    Catalog,
    /// The host's file, streamed from the host.
    HostStream,
    /// A close catalog match (another upload of the song).
    CloseMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CopyView {
    pub kind: CopyKind,
    pub class: CopyClass,
    /// Another version: how much longer (positive) or shorter, in seconds.
    pub length_diff_s: Option<i32>,
}

impl CopyView {
    /// "own file, matched", "YouTube Music, another version (+12 s)".
    pub fn words(&self) -> String {
        let kind = match self.kind {
            CopyKind::OwnFile => "own file",
            CopyKind::Catalog => "YouTube Music",
            CopyKind::HostStream => "your file, streamed",
            CopyKind::CloseMatch => "a close match",
        };
        let how = match (self.class, self.length_diff_s) {
            (CopyClass::Exact | CopyClass::Measured, _) => "matched".to_owned(),
            (CopyClass::Approximate, Some(d)) if d != 0 => format!("another version ({d:+} s)"),
            (CopyClass::Approximate, _) => "another version".to_owned(),
        };
        format!("{kind}, {how}")
    }
}

/// The figure after "In step": the clock's bound plus the error, never better than the clock
/// allows, in whole ms (at least 1).
fn figure(error_ms: Option<f32>, bound_ms: Option<f32>) -> u32 {
    let e = error_ms.map_or(0.0, f32::abs) + bound_ms.unwrap_or(0.0);
    (e.ceil() as u32).max(1)
}

/// A person's line (section 4.12). `host_name` names the speaker a remote listens on.
pub fn line(
    sub_state: Option<SubState>,
    mode: ListenMode,
    away: bool,
    error_ms: Option<f32>,
    bound_ms: Option<f32>,
    host_name: &str,
) -> String {
    if away {
        return "Reconnecting".into();
    }
    let Some(s) = sub_state else {
        return "Getting ready".into();
    };
    match s {
        SubState::Opening => "Getting ready".into(),
        SubState::Ready => "Ready".into(),
        SubState::InStep => match mode {
            ListenMode::EveryDevice => format!("In step, ±{} ms", figure(error_ms, bound_ms)),
            _ => format!("In step, about ±{} ms", figure(error_ms, bound_ms)),
        },
        SubState::Correcting | SubState::Realigning => "Catching up".into(),
        SubState::Suspended(Suspension::Buffering) => "Buffering".into(),
        SubState::Suspended(Suspension::OutputLost) => "Sound output unavailable".into(),
        SubState::Suspended(Suspension::Interrupted) => "Interrupted".into(),
        SubState::Suspended(Suspension::Sleeping) => "Sleeping".into(),
        SubState::SteppedOut => "Stepped out".into(),
        SubState::CannotPlay => "Not available for you, back with the next song".into(),
        SubState::Remote => format!("Listening on {host_name}'s speaker"),
    }
}

/// Someone else's line, as beside their name: as line, except that a person with no copy of
/// the song reads "Can't play this one, back with the next song" ("Not available for you" is
/// said to the person it is about).
pub fn others_line(
    sub_state: Option<SubState>,
    mode: ListenMode,
    away: bool,
    error_ms: Option<f32>,
    bound_ms: Option<f32>,
    host_name: &str,
) -> String {
    if !away && sub_state == Some(SubState::CannotPlay) {
        return "Can't play this one, back with the next song".into();
    }
    line(sub_state, mode, away, error_ms, bound_ms, host_name)
}

/// Your line while stepped out (others read "Stepped out").
pub fn stepped_out_line(why: StepOut) -> String {
    match why {
        StepOut::Chosen => "Stepped out".into(),
        StepOut::SleepTimer => "Stepped out (sleep timer)".into(),
        StepOut::Unplugged => "Paused: headphones disconnected".into(),
    }
}

/// Your line while joining: "Joining... buffering 60%".
pub fn joining_line(pct: u8) -> String {
    format!("Joining... buffering {}%", pct.min(100))
}

/// What a press shows until the room commits it: "Pausing...", "Moving to 1:35...",
/// "Skipping...".
pub fn pressing(what: Press) -> String {
    match what {
        Press::Play => "Starting...".into(),
        Press::Pause => "Pausing...".into(),
        Press::Seek { to_ms } => {
            let s = to_ms / 1_000;
            format!("Moving to {}:{:02}...", s / 60, s % 60)
        }
        Press::Skip => "Skipping...".into(),
        Press::Previous => "Going back...".into(),
    }
}

/// A press under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Play,
    Pause,
    Seek { to_ms: u64 },
    Skip,
    Previous,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn every_state_has_its_line() {
        let l = |s, m| line(Some(s), m, false, Some(0.4), Some(1.2), "Asha");
        assert_eq!(l(SubState::Opening, ListenMode::Apart), "Getting ready");
        assert_eq!(l(SubState::Ready, ListenMode::Apart), "Ready");
        assert_eq!(
            l(SubState::InStep, ListenMode::EveryDevice),
            "In step, ±2 ms"
        );
        assert_eq!(
            line(
                Some(SubState::InStep),
                ListenMode::Apart,
                false,
                Some(-3.0),
                Some(11.5),
                "Asha"
            ),
            "In step, about ±15 ms"
        );
        assert_eq!(l(SubState::Correcting, ListenMode::Apart), "Catching up");
        assert_eq!(l(SubState::Realigning, ListenMode::Apart), "Catching up");
        assert_eq!(
            l(
                SubState::Suspended(Suspension::Buffering),
                ListenMode::Apart
            ),
            "Buffering"
        );
        assert_eq!(
            l(
                SubState::Suspended(Suspension::OutputLost),
                ListenMode::Apart
            ),
            "Sound output unavailable"
        );
        assert_eq!(
            l(
                SubState::Suspended(Suspension::Interrupted),
                ListenMode::Apart
            ),
            "Interrupted"
        );
        assert_eq!(l(SubState::SteppedOut, ListenMode::Apart), "Stepped out");
        assert_eq!(
            l(SubState::CannotPlay, ListenMode::Apart),
            "Not available for you, back with the next song"
        );
        assert_eq!(
            l(SubState::Remote, ListenMode::OneSpeaker),
            "Listening on Asha's speaker"
        );
        assert_eq!(
            line(None, ListenMode::Apart, true, None, None, "Asha"),
            "Reconnecting"
        );
        assert_eq!(joining_line(60), "Joining... buffering 60%");
        assert_eq!(
            others_line(
                Some(SubState::CannotPlay),
                ListenMode::Apart,
                false,
                None,
                None,
                "Asha"
            ),
            "Can't play this one, back with the next song"
        );
        assert_eq!(
            others_line(
                Some(SubState::Ready),
                ListenMode::Apart,
                false,
                None,
                None,
                "Asha"
            ),
            "Ready"
        );
        assert_eq!(pressing(Press::Seek { to_ms: 95_400 }), "Moving to 1:35...");
        assert_eq!(stepped_out_line(StepOut::Chosen), "Stepped out");
        assert_eq!(
            stepped_out_line(StepOut::SleepTimer),
            "Stepped out (sleep timer)"
        );
        assert_eq!(
            stepped_out_line(StepOut::Unplugged),
            "Paused: headphones disconnected"
        );
        let c = CopyView {
            kind: CopyKind::Catalog,
            class: CopyClass::Approximate,
            length_diff_s: Some(12),
        };
        assert_eq!(c.words(), "YouTube Music, another version (+12 s)");
    }

    // --- The fixture of every state, for the window --------------------------------------

    fn song(n: u32, host_file: bool) -> Song {
        let id = format!("song{n:07}");
        Song {
            key: if host_file {
                Song::host_key(&format!("Album/{n:02}.flac"))
            } else {
                format!("online:{id}")
            },
            title: format!("Song {n}"),
            artists: vec!["Test Artist".into()],
            album: Some("Test Album".into()),
            duration_ms: Some(200_000 + u64::from(n) * 1_000),
            isrc: None,
            mbid: None,
            online_id: (!host_file).then_some(id),
            host_file,
            format: host_file.then(|| "flac".into()),
            artwork_url: None,
            added_by: Some(n % 3),
        }
    }

    fn copy_of(id: u32) -> CopyView {
        match id % 4 {
            0 => CopyView {
                kind: CopyKind::HostStream,
                class: CopyClass::Exact,
                length_diff_s: None,
            },
            1 => CopyView {
                kind: CopyKind::Catalog,
                class: CopyClass::Approximate,
                length_diff_s: Some(12),
            },
            2 => CopyView {
                kind: CopyKind::OwnFile,
                class: CopyClass::Measured,
                length_diff_s: None,
            },
            _ => CopyView {
                kind: CopyKind::CloseMatch,
                class: CopyClass::Approximate,
                length_diff_s: None,
            },
        }
    }

    /// A person's error and the clock's bound, by mode (a remote has neither).
    fn error_bound(mode: ListenMode) -> (Option<f32>, Option<f32>) {
        match mode {
            ListenMode::EveryDevice => (Some(0.3), Some(0.9)),
            ListenMode::OneSpeaker => (None, None),
            ListenMode::Apart => (Some(-4.0), Some(9.0)),
        }
    }

    fn member(
        id: u32,
        name: &str,
        mode: ListenMode,
        s: Option<SubState>,
        away: bool,
    ) -> MemberView {
        let (e, b) = error_bound(mode);
        MemberView {
            id,
            name: name.into(),
            host: id == 0,
            role: if id == 0 { Role::Control } else { Role::Add },
            mode,
            sub_state: if away { None } else { s },
            away,
            // Asha (the host) and Meera (the guest) are "you" in the views here; the rest are others.
            line: if id == 0 || id == 2 {
                line(if away { None } else { s }, mode, away, e, b, "Asha")
            } else {
                others_line(if away { None } else { s }, mode, away, e, b, "Asha")
            },
            copy: (mode != ListenMode::OneSpeaker).then(|| copy_of(id)),
            error_ms: e,
            bound_ms: b,
            slow: false,
            hand_over: None,
        }
    }

    fn room(members: Vec<MemberView>, transition: Transition, playing: bool) -> RoomView {
        RoomView {
            host_name: "Asha".into(),
            members,
            waiting: vec![],
            queue: (1..=4)
                .map(|n| EntryView {
                    id: EntryId(u64::from(n) + 40),
                    song: song(n, n % 2 == 0),
                })
                .collect(),
            anchor: Some(Anchor {
                entry: EntryId(41),
                content_us: 95_000_000,
                host_us: 3_600_000_000,
                playing,
                seq: 12,
            }),
            next: Some(Next {
                entry: EntryId(42),
                boundary_host_us: 3_706_000_000,
                transition,
                seq: 13,
            }),
            transition,
            activity: vec![Activity {
                who: 1,
                name: "Ravi".into(),
                what: "added 2 songs".into(),
                at_us: 3_590_000_000,
                undoable: true,
            }],
            new_role: Role::Add,
            starting: None,
            host_away: false,
            phase: if playing { Phase::Playing } else { Phase::Held },
            requests: vec![],
            auto_roles: false,
            guests_volume: false,
            volume: None,
            lyrics: None,
        }
    }

    fn base(state: SessionState) -> View {
        View {
            state,
            host: false,
            me: None,
            room: None,
            invite: None,
            path: None,
            message: None,
            name: "Meera".into(),
            local_only: false,
            let_in: false,
            you: None,
            output: OutputView {
                name: "Speakers".into(),
                delay_ms: 0,
                bluetooth: false,
                delay_known: true,
            },
            host_now_us: None,
            nearby: vec![],
            visible: false,
            resume: None,
            last_queue: None,
        }
    }

    /// Every sub-state, mode, copy and absence, among the members of one room (the host's
    /// view: everyone's copy, who is slow, who could take the room over).
    fn everyone() -> Vec<MemberView> {
        use ListenMode::*;
        use SubState::*;
        let states = [
            (EveryDevice, Some(InStep)),
            (Apart, Some(InStep)),
            (Apart, Some(Opening)),
            (Apart, Some(Ready)),
            (EveryDevice, Some(Correcting)),
            (Apart, Some(Realigning)),
            (Apart, Some(Suspended(Suspension::Buffering))),
            (Apart, Some(Suspended(Suspension::OutputLost))),
            (Apart, Some(Suspended(Suspension::Interrupted))),
            (Apart, Some(Suspended(Suspension::Sleeping))),
            (Apart, Some(SteppedOut)),
            (Apart, Some(CannotPlay)),
            (OneSpeaker, Some(Remote)),
        ];
        let names = [
            "Asha", "Ravi", "Meera", "Ali", "Kabir", "Sara", "Dev", "Ira", "Noor", "Zoya", "Om",
            "Tara", "Veer",
        ];
        let mut m: Vec<MemberView> = states
            .iter()
            .zip(names)
            .enumerate()
            .map(|(i, (&(mode, s), name))| member(i as u32, name, mode, s, false))
            .collect();
        m.push(member(13, "Ishan", Apart, Some(InStep), true));
        // Dev missed three starts; Ravi and Meera could host (Meera without two people's
        // songs only Asha has).
        m[6].slow = true;
        m[1].hand_over = Some(0);
        m[2].hand_over = Some(2);
        m
    }

    /// A small room as Meera (id 2) sees it: the host, Meera listening as given, and Ali
    /// buffering. Only Meera's own copy is in her view (D14).
    fn few(mode: ListenMode, s: SubState) -> Vec<MemberView> {
        let mut out = vec![
            member(
                0,
                "Asha",
                ListenMode::EveryDevice,
                Some(SubState::InStep),
                false,
            ),
            member(2, "Meera", mode, Some(s), false),
            member(
                3,
                "Ali",
                ListenMode::Apart,
                Some(SubState::Suspended(Suspension::Buffering)),
                false,
            ),
        ];
        for m in &mut out {
            if m.id != 2 {
                m.copy = None;
            }
        }
        out
    }

    fn you(mode: ListenMode, s: SubState, pressing: Option<String>) -> You {
        let (e, b) = error_bound(mode);
        You {
            mode,
            sub_state: s,
            line: line(Some(s), mode, false, e, b, "Asha"),
            pressing,
            joining_pct: None,
            choices: vec![],
            stepped_out: None,
        }
    }

    fn choice(mode: ListenMode) -> ModeChoice {
        ModeChoice {
            mode,
            why_not: None,
        }
    }

    /// Meera, a guest over the internet (through a relay), in a playing room.
    fn guest(s: SubState, mode: ListenMode, pressing: Option<String>, t: Transition) -> View {
        View {
            me: Some(2),
            path: Some(PathView {
                direct: false,
                relay: true,
                rtt_ms: Some(118),
            }),
            room: Some(room(few(mode, s), t, true)),
            you: Some(you(mode, s, pressing)),
            host_now_us: Some(3_612_500_000),
            ..base(SessionState::In)
        }
    }

    /// Meera on the host's network (direct, a few ms), offered one speaker first.
    fn neighbour(s: SubState, mode: ListenMode) -> View {
        let mut v = guest(s, mode, None, Transition::Gapless);
        v.path = Some(PathView {
            direct: true,
            relay: false,
            rtt_ms: Some(3),
        });
        if let Some(y) = v.you.as_mut() {
            y.choices = vec![
                choice(ListenMode::OneSpeaker),
                choice(ListenMode::EveryDevice),
            ];
        }
        v
    }

    fn edit(mut v: View, f: impl FnOnce(&mut View)) -> View {
        f(&mut v);
        v
    }

    fn room_of(v: &mut View) -> &mut RoomView {
        v.room.as_mut().expect("a room")
    }

    fn you_of(v: &mut View) -> &mut You {
        v.you.as_mut().expect("you")
    }

    fn me_of(v: &mut View) -> &mut MemberView {
        let me = v.me;
        room_of(v)
            .members
            .iter_mut()
            .find(|m| Some(m.id) == me)
            .expect("me")
    }

    fn fixtures() -> Vec<(&'static str, View)> {
        use ListenMode::*;
        let mut out = vec![("off", base(SessionState::Off))];
        out.push((
            "off, rooms nearby and one to rejoin",
            View {
                nearby: vec![
                    NearbyView {
                        id: 1,
                        host: "Asha".into(),
                        listening: 3,
                    },
                    NearbyView {
                        id: 2,
                        host: "Kabir".into(),
                        listening: 1,
                    },
                ],
                resume: Some(Resume::Rejoin {
                    host: "Asha".into(),
                }),
                ..base(SessionState::Off)
            },
        ));
        out.push((
            "off, your room to reopen",
            View {
                name: "Asha".into(),
                resume: Some(Resume::Reopen),
                ..base(SessionState::Off)
            },
        ));
        out.push((
            "starting",
            View {
                host: true,
                ..base(SessionState::Starting)
            },
        ));
        let invite = InviteView {
            link: "https://venuta.app/join#v1.example".into(),
            code: "ABCD-EFGH".into(),
            qr_svg: "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into(),
            host: "Asha".into(),
            until: 1_790_000_000,
        };
        let hosting = |members: Vec<MemberView>, mode, t| View {
            host: true,
            me: Some(0),
            name: "Asha".into(),
            invite: Some(invite.clone()),
            room: Some(RoomView {
                volume: Some(80),
                ..room(members, t, true)
            }),
            you: Some(you(mode, SubState::InStep, None)),
            host_now_us: Some(3_612_500_000),
            visible: true,
            ..base(SessionState::Hosting)
        };
        out.push((
            "hosting, everyone in every state",
            edit(hosting(everyone(), EveryDevice, Transition::Gapless), |v| {
                v.let_in = true;
                let r = room_of(v);
                r.waiting = vec![(20, "Kabir".into())];
                r.auto_roles = true;
                r.requests = vec![RequestView {
                    id: 4,
                    from: 1,
                    words: "Ravi asks to skip".into(),
                }];
            }),
        ));
        out.push((
            "hosting alone, nothing playing",
            edit(
                hosting(
                    vec![member(0, "Asha", EveryDevice, Some(SubState::Ready), false)],
                    EveryDevice,
                    Transition::Gapless,
                ),
                |v| {
                    you_of(v).sub_state = SubState::Ready;
                    you_of(v).line = "Ready".into();
                    let r = room_of(v);
                    r.queue = vec![];
                    r.anchor = None;
                    r.next = None;
                    r.activity = vec![];
                    r.phase = Phase::Resting;
                },
            ),
        ));
        out.push((
            "hosting, a one speaker room",
            edit(
                hosting(
                    vec![
                        member(0, "Asha", EveryDevice, Some(SubState::InStep), false),
                        member(1, "Ravi", OneSpeaker, Some(SubState::Remote), false),
                        member(2, "Meera", OneSpeaker, Some(SubState::Remote), false),
                    ],
                    EveryDevice,
                    Transition::Gapless,
                ),
                |v| {
                    let r = room_of(v);
                    r.guests_volume = true;
                    r.members[1].hand_over = Some(0);
                },
            ),
        ));
        out.push((
            "hosting, the queue ended",
            edit(
                hosting(
                    vec![
                        member(0, "Asha", EveryDevice, Some(SubState::Ready), false),
                        member(2, "Meera", Apart, Some(SubState::Ready), false),
                    ],
                    EveryDevice,
                    Transition::Gapless,
                ),
                |v| {
                    you_of(v).sub_state = SubState::Ready;
                    you_of(v).line = "Ready".into();
                    let r = room_of(v);
                    r.anchor = None;
                    r.next = None;
                    r.phase = Phase::QueueEnded;
                },
            ),
        ));
        out.push((
            "joining, buffering",
            View {
                me: Some(2),
                path: Some(PathView {
                    direct: true,
                    relay: false,
                    rtt_ms: Some(42),
                }),
                you: Some(You {
                    line: joining_line(60),
                    joining_pct: Some(60),
                    ..you(Apart, SubState::Opening, None)
                }),
                ..base(SessionState::Joining)
            },
        ));
        out.push((
            "joining on the host's network, choosing",
            edit(neighbour(SubState::Opening, OneSpeaker), |v| {
                v.state = SessionState::Joining;
                let y = you_of(v);
                y.line = "Getting ready".into();
                me_of(v).sub_state = Some(SubState::Opening);
                me_of(v).line = "Getting ready".into();
            }),
        ));
        out.push((
            "joining on the host's network, on headphones",
            edit(neighbour(SubState::Opening, OneSpeaker), |v| {
                v.state = SessionState::Joining;
                v.output = OutputView {
                    name: "WH-1000XM5".into(),
                    delay_ms: 0,
                    bluetooth: true,
                    delay_known: false,
                };
                let y = you_of(v);
                y.line = "Getting ready".into();
                y.choices = vec![choice(OneSpeaker), choice(Apart)];
            }),
        ));
        out.push(("waiting at the door", base(SessionState::Waiting)));
        for (name, p) in [
            ("in, pausing", Press::Pause),
            ("in, moving", Press::Seek { to_ms: 95_000 }),
            ("in, skipping", Press::Skip),
            ("in, starting", Press::Play),
            ("in, going back", Press::Previous),
        ] {
            out.push((
                name,
                guest(
                    SubState::InStep,
                    Apart,
                    Some(pressing(p)),
                    Transition::Gapless,
                ),
            ));
        }
        out.push((
            "in, crossfading between songs",
            guest(
                SubState::InStep,
                Apart,
                None,
                Transition::Crossfade { ms: 4_000 },
            ),
        ));
        out.push((
            "in, cut between songs",
            guest(SubState::InStep, Apart, None, Transition::Cut),
        ));
        out.push((
            "in, a remote on the host's speaker",
            neighbour(SubState::Remote, OneSpeaker),
        ));
        out.push((
            "in, a remote who controls and may change the volume",
            edit(neighbour(SubState::Remote, OneSpeaker), |v| {
                me_of(v).role = Role::Control;
                let r = room_of(v);
                r.guests_volume = true;
                r.volume = Some(60);
            }),
        ));
        out.push((
            "in, a remote, the words of a song only the host has",
            edit(neighbour(SubState::Remote, OneSpeaker), |v| {
                let r = room_of(v);
                let key = r.queue[0].song.key.clone();
                r.lyrics = Some(RoomLyrics {
                    key,
                    lines: vec![
                        (90_000, "The line before".into()),
                        (94_500, "The line being sung".into()),
                        (99_000, "The next line".into()),
                    ],
                    text: String::new(),
                });
            }),
        ));
        out.push((
            "in, every device, on the host's network",
            neighbour(SubState::InStep, EveryDevice),
        ));
        out.push((
            "in, every device not available",
            edit(
                guest(SubState::InStep, Apart, None, Transition::Gapless),
                |v| {
                    you_of(v).choices = vec![
                        ModeChoice {
                            mode: EveryDevice,
                            why_not: Some("Not while Hide my IP is on".into()),
                        },
                        choice(Apart),
                    ];
                },
            ),
        ));
        out.push((
            "in, the room paused",
            edit(
                guest(SubState::Ready, Apart, None, Transition::Gapless),
                |v| {
                    let r = room_of(v);
                    r.anchor = r.anchor.map(|a| Anchor {
                        playing: false,
                        ..a
                    });
                    r.phase = Phase::Held;
                },
            ),
        ));
        out.push((
            "in, a start waiting for someone",
            edit(
                guest(SubState::Ready, Apart, None, Transition::Gapless),
                |v| {
                    room_of(v).starting = Some(vec!["Ali".into()]);
                },
            ),
        ));
        out.push((
            "in, a skip waiting for two",
            edit(
                guest(
                    SubState::InStep,
                    Apart,
                    Some(pressing(Press::Skip)),
                    Transition::Gapless,
                ),
                |v| {
                    room_of(v).starting = Some(vec!["Ali".into(), "Dev".into()]);
                },
            ),
        ));
        out.push((
            "in, the host reconnecting",
            edit(
                guest(SubState::InStep, Apart, None, Transition::Gapless),
                |v| {
                    let r = room_of(v);
                    r.host_away = true;
                    let host = &mut r.members[0];
                    host.away = true;
                    host.sub_state = None;
                    host.line = line(None, host.mode, true, None, None, "Asha");
                },
            ),
        ));
        out.push((
            "in, on Bluetooth with its delay set",
            edit(
                guest(SubState::InStep, Apart, None, Transition::Gapless),
                |v| {
                    v.output = OutputView {
                        name: "WH-1000XM5".into(),
                        delay_ms: 200,
                        bluetooth: true,
                        delay_known: true,
                    };
                },
            ),
        ));
        out.push((
            "in, on Bluetooth with its delay not known",
            edit(neighbour(SubState::InStep, EveryDevice), |v| {
                v.output = OutputView {
                    name: "JBL Flip".into(),
                    delay_ms: 0,
                    bluetooth: true,
                    delay_known: false,
                };
            }),
        ));
        for (name, why) in [
            ("in, stepped out", StepOut::Chosen),
            ("in, stepped out by the sleep timer", StepOut::SleepTimer),
            ("in, headphones disconnected", StepOut::Unplugged),
        ] {
            out.push((
                name,
                edit(
                    guest(SubState::SteppedOut, Apart, None, Transition::Gapless),
                    |v| {
                        let y = you_of(v);
                        y.stepped_out = Some(why);
                        y.line = stepped_out_line(why);
                    },
                ),
            ));
        }
        for (name, why) in [
            ("in, buffering", Suspension::Buffering),
            ("in, sound output unavailable", Suspension::OutputLost),
            ("in, interrupted", Suspension::Interrupted),
        ] {
            out.push((
                name,
                guest(SubState::Suspended(why), Apart, None, Transition::Gapless),
            ));
        }
        out.push((
            "in, not available for you",
            guest(SubState::CannotPlay, Apart, None, Transition::Gapless),
        ));
        out.push((
            "in, catching up",
            guest(SubState::Correcting, Apart, None, Transition::Gapless),
        ));
        out.push((
            "in, on another version",
            edit(
                guest(SubState::InStep, Apart, None, Transition::Gapless),
                |v| {
                    me_of(v).copy = Some(CopyView {
                        kind: CopyKind::Catalog,
                        class: CopyClass::Approximate,
                        length_diff_s: Some(12),
                    });
                },
            ),
        ));
        out.push((
            "in, just listening",
            edit(
                guest(SubState::InStep, Apart, None, Transition::Gapless),
                |v| {
                    me_of(v).role = Role::Listen;
                },
            ),
        ));
        out.push((
            "in, in control with a request to answer",
            edit(
                guest(SubState::InStep, Apart, None, Transition::Gapless),
                |v| {
                    me_of(v).role = Role::Control;
                    room_of(v).requests = vec![RequestView {
                        id: 9,
                        from: 3,
                        words: "Ali asks to skip".into(),
                    }];
                },
            ),
        ));
        out.push((
            "in, the room's queue ended",
            edit(
                guest(SubState::Ready, Apart, None, Transition::Gapless),
                |v| {
                    let r = room_of(v);
                    r.anchor = None;
                    r.next = None;
                    r.phase = Phase::QueueEnded;
                },
            ),
        ));
        out.push((
            "reconnecting",
            View {
                state: SessionState::Reconnecting,
                ..guest(SubState::InStep, Apart, None, Transition::Gapless)
            },
        ));
        out.push((
            "ended",
            View {
                message: Some("Asha ended the room".into()),
                last_queue: Some(4),
                ..base(SessionState::Ended)
            },
        ));
        out.push((
            "ended, not let in",
            View {
                message: Some("Asha didn't let you in.".into()),
                ..base(SessionState::Ended)
            },
        ));
        out
    }

    #[derive(Serialize)]
    struct Case {
        name: &'static str,
        view: View,
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../app/tests/fixtures/together-views.json")
    }

    #[test]
    fn the_windows_fixture_holds_every_state_as_these_types_make_it() {
        let cases: Vec<Case> = fixtures()
            .into_iter()
            .map(|(name, view)| Case { name, view })
            .collect();
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "about": "Every state the room panel draws, made by crates/together/src/view.rs (run its tests with UPDATE_FIXTURES=1 to remake it).",
            "cases": cases,
        }))
        .unwrap()
            + "\n";
        // Every view reads back as itself.
        for c in &cases {
            let back: View =
                serde_json::from_value(serde_json::to_value(&c.view).unwrap()).unwrap();
            assert_eq!(back, c.view, "{}", c.name);
        }
        // No em dashes in anything a person reads.
        assert!(!json.contains('\u{2014}'));
        let path = fixture_path();
        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            std::fs::write(&path, &json).unwrap();
        }
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            on_disk == json,
            "app/tests/fixtures/together-views.json is out of date: run this test with UPDATE_FIXTURES=1"
        );
    }
}
