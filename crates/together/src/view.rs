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
}

/// Which copy a person plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        assert_eq!(pressing(Press::Seek { to_ms: 95_400 }), "Moving to 1:35...");
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

    fn member(
        id: u32,
        name: &str,
        mode: ListenMode,
        s: Option<SubState>,
        away: bool,
    ) -> MemberView {
        let (e, b) = match mode {
            ListenMode::EveryDevice => (Some(0.3), Some(0.9)),
            _ => (Some(-4.0), Some(9.0)),
        };
        let copy = Some(match id % 4 {
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
        });
        MemberView {
            id,
            name: name.into(),
            host: id == 0,
            role: if id == 0 { Role::Control } else { Role::Add },
            mode,
            sub_state: if away { None } else { s },
            away,
            line: line(if away { None } else { s }, mode, away, e, b, "Asha"),
            copy,
            error_ms: e,
            bound_ms: b,
        }
    }

    fn room(members: Vec<MemberView>, transition: Transition, playing: bool) -> RoomView {
        RoomView {
            host_name: "Asha".into(),
            members,
            waiting: vec![(7, "Kabir".into())],
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
                who: 2,
                name: "Ravi".into(),
                what: "added 2 songs".into(),
                at_us: 3_590_000_000,
                undoable: true,
            }],
            new_role: Role::Add,
            starting: None,
            host_away: false,
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
        }
    }

    /// Every sub-state, mode, copy and absence, among the members of one room.
    fn everyone() -> Vec<MemberView> {
        use ListenMode::*;
        use SubState::*;
        let states = [
            (Apart, Some(InStep)),
            (EveryDevice, Some(InStep)),
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
        m
    }

    /// A small room: the host, you and one more.
    fn few() -> Vec<MemberView> {
        vec![
            member(
                0,
                "Asha",
                ListenMode::EveryDevice,
                Some(SubState::InStep),
                false,
            ),
            member(2, "Meera", ListenMode::Apart, Some(SubState::InStep), false),
            member(
                3,
                "Ali",
                ListenMode::Apart,
                Some(SubState::Suspended(Suspension::Buffering)),
                false,
            ),
        ]
    }

    fn fixtures() -> Vec<(&'static str, View)> {
        use ListenMode::*;
        let mut out = Vec::new();
        out.push(("off", base(SessionState::Off)));
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
        let you = |mode, s: SubState, pressing: Option<String>| You {
            mode,
            sub_state: s,
            line: line(Some(s), mode, false, Some(0.5), Some(8.0), "Asha"),
            pressing,
            joining_pct: None,
        };
        out.push((
            "hosting, everyone in every state",
            View {
                host: true,
                me: Some(0),
                name: "Asha".into(),
                let_in: true,
                invite: Some(invite.clone()),
                room: Some(room(everyone(), Transition::Gapless, true)),
                you: Some(you(EveryDevice, SubState::InStep, None)),
                ..base(SessionState::Hosting)
            },
        ));
        out.push((
            "hosting alone, nothing playing",
            View {
                host: true,
                me: Some(0),
                name: "Asha".into(),
                invite: Some(invite),
                room: Some(RoomView {
                    members: vec![member(0, "Asha", EveryDevice, Some(SubState::Ready), false)],
                    queue: vec![],
                    anchor: None,
                    next: None,
                    activity: vec![],
                    waiting: vec![],
                    ..room(vec![], Transition::Gapless, false)
                }),
                you: Some(you(EveryDevice, SubState::Ready, None)),
                ..base(SessionState::Hosting)
            },
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
        out.push(("waiting at the door", base(SessionState::Waiting)));
        let guest = |s: SubState, mode, pressing: Option<String>, t| View {
            me: Some(2),
            path: Some(PathView {
                direct: false,
                relay: true,
                rtt_ms: Some(118),
            }),
            room: Some(room(few(), t, true)),
            you: Some(you(mode, s, pressing)),
            ..base(SessionState::In)
        };
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
            guest(SubState::Remote, OneSpeaker, None, Transition::Gapless),
        ));
        let mut paused = guest(SubState::Ready, Apart, None, Transition::Gapless);
        if let Some(r) = paused.room.as_mut() {
            r.anchor = r.anchor.map(|a| Anchor {
                playing: false,
                ..a
            });
        }
        out.push(("in, the room paused", paused));
        let mut starting = guest(SubState::Ready, Apart, None, Transition::Gapless);
        if let Some(r) = starting.room.as_mut() {
            r.starting = Some(vec!["Ali".into()]);
        }
        out.push(("in, a start waiting for someone", starting));
        let mut host_away = guest(SubState::InStep, Apart, None, Transition::Gapless);
        if let Some(r) = host_away.room.as_mut() {
            r.host_away = true;
        }
        out.push(("in, the host reconnecting", host_away));
        let mut bluetooth = guest(SubState::InStep, Apart, None, Transition::Gapless);
        bluetooth.output = OutputView {
            name: "WH-1000XM5".into(),
            delay_ms: 200,
            bluetooth: true,
            delay_known: true,
        };
        out.push(("in, on Bluetooth with its delay set", bluetooth));
        let mut unmeasured = guest(SubState::InStep, EveryDevice, None, Transition::Gapless);
        unmeasured.output = OutputView {
            name: "JBL Flip".into(),
            delay_ms: 0,
            bluetooth: true,
            delay_known: false,
        };
        out.push(("in, on Bluetooth with its delay not known", unmeasured));
        out.push((
            "reconnecting",
            View {
                me: Some(2),
                room: Some(room(few(), Transition::Gapless, true)),
                you: Some(you(Apart, SubState::InStep, None)),
                ..base(SessionState::Reconnecting)
            },
        ));
        out.push((
            "ended",
            View {
                message: Some("Asha ended the room".into()),
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
