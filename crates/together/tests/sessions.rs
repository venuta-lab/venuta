//! WP9's gates for the sessions and the conductor (design 9.2): whole rooms of 3 to 5 people,
//! each person's session on the simulated network in simulated time, each player a fake that
//! records every call. For every row of 6.1 where a session or a conductor acts, what each
//! person's player is told, what the room hears back, and what each person sees.
//!
//! Where the other rows are tested:
//! - the room's own rules (B1 to B17, C1 to C8, D1 to D11, G1 to G7, who waits for whom,
//!   roles, the away rule): `room_v2.rs`, Room v2 on the same network with scripted players;
//! - what a player does with the calls (splices, fades, the boundary table, stalls, output
//!   switches, H1 to H11): `crates/player/tests/room2.rs` and `room2_sync.rs`, and whole rooms
//!   with real players in `crates/player/tests/room_whole.rs` (7.3, 7.4, 7.6, clicks);
//! - copies and alignment: E1 to E6 and E9 here (WP11, with fake envelopes), and on real encodes
//!   in `crates/player/tests/room_align.rs`; one speaker mode (A16, B17, J7) here (WP18) and
//!   with a real host in `crates/player/tests/room_one_speaker.rs`; output delays (H3, H3b,
//!   J8): WP19; hand-over (I3b): WP20; reopen and rejoin (I6, I7): WP15, in `lifecycle.rs`,
//!   with the door for rooms nearby (A15);
//! - the window's (B9, the panel's words and greying): `app/tests` and WP12's model tests;
//!   the app shell's (A14, B15, C8, J4, H13): the app.

mod session_sim;

use session_sim::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use together::align::{Alignment, CopyClass};
use together::anchor::{Anchor, EntryId, Transition};
use together::fakes::Call;
use together::proto::Role;
use together::proto::Song;
use together::proto::v3::{Action, Advice, DIFFERENT_VERSIONS, Lyrics, Phase, ToGuest};
use together::room::v2::{HOST, HostNote, Opening};
use together::room_player::{ListenMode, SubState, Suspension, SyncMode};
use together::session::HostLyrics;
use together::session::Stage;
use together::sim_net::Way;
use together::time::MonoClock;
use together::transport::{ControlMsg, Transport, TransportEvent};
use together::view::{CopyKind, CopyView};

fn internet() -> (Way, Way) {
    (Way::internet(25.0, 5.0), Way::internet(35.0, 5.0))
}

fn relay() -> (Way, Way) {
    let w = Way {
        relay: true,
        ..Way::internet(60.0, 10.0)
    };
    (w, w)
}

/// The room's place now (host µs) by its anchor.
fn place(w: &mut World) -> i64 {
    let now = w.now();
    w.room().anchor().unwrap().content_at(now)
}

// --- A. Opening, joining and the door -----------------------------------------------------

#[test]
fn a1_the_hosts_music_goes_on_untouched_and_a_guest_joins_its_place() {
    let mut w = World::open(
        songs(3),
        Opening::Playing {
            index: 1,
            content_us: 30 * S,
            host_us: 0,
        },
    );
    let a = w.room().anchor().unwrap();
    assert_eq!(a.entry, w.entry(1));
    w.run_ms(500);
    // The host's player: made in place, then its own anchor, a no-op. No prepare, no fade;
    // alone in its room, it leads (A19: exactly solo playback).
    assert_eq!(
        w.people[0].calls(),
        vec![
            Call::Attach {
                mode: SyncMode::Apart
            },
            Call::Commit(a),
            Call::Lead(true)
        ]
    );
    let t = w.sim_ns();
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.run_ms(2_000);
    let p = w.people[r].prepares();
    assert_eq!(p.len(), 1, "Ravi caught up");
    assert_eq!(p[0].1, a.entry);
    assert_eq!(w.people[r].commits(), vec![a]);
    // Someone plays now: the host's player follows the room's clock (no longer leading), and
    // nothing else: no prepare, no commit, no fade.
    assert_eq!(
        w.people[0].calls_since(t),
        vec![Call::Lead(false)],
        "the host's music is never touched by a joiner"
    );
}

#[test]
fn a2_a_resting_room_starts_with_the_first_songs_added() {
    let mut w = World::open(vec![], Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    assert!(w.people[r].prepares().is_empty() && w.people[0].prepares().is_empty());
    assert_eq!(w.room().phase(), Phase::Resting);
    w.host_ask(Action::Add {
        songs: songs(2),
        next: false,
    });
    assert!(w.run_until(|w| w.room().phase() == Phase::Playing, 4_000));
    let e = w.entry(0);
    for i in [0, r] {
        assert_eq!(w.people[i].prepares()[0].1, e);
        assert!(w.run_until(|w| !w.people[i].commits().is_empty(), 2_000));
    }
}

/// A guest joining a room playing for a while over `ways`: when the clock was ready after
/// connecting, and when the catch-up's first sound is due (host time), from the join.
fn join_to_first_sound(ways: (Way, Way)) -> (i64, i64) {
    let mut w = World::open(songs(3), Opening::Resting);
    w.start();
    w.run_ms(20_000);
    let t0 = w.now();
    let i = w.arrive("Meera", ways.0, ways.1, ListenMode::Apart);
    assert!(w.run_until(|w| !w.people[i].commits().is_empty(), 10_000));
    let ready = w.people[i].guest().clock_ready_at().expect("clock ready");
    let a = w.people[i].commits()[0];
    let (_, _, content) = *w.people[i].prepares().last().unwrap();
    // The prepared place is heard when the room reaches it.
    let sound = a.host_us + (content - a.content_us);
    (ready - t0, sound - t0)
}

#[test]
fn a3_a4_f1_f2_f3_join_to_first_sound_and_the_clock_ready_within_their_gates() {
    // 7.3: first sound under 3 s on one Wi-Fi, 5 s direct, 8 s through a relay (a copy at
    // hand); the clock ready 1, 1.5 and 2 s after connecting.
    for (what, ways, sound_max, clock_max) in [
        ("one Wi-Fi", (Way::LAN, Way::LAN), 3 * S, S),
        ("internet, direct", internet(), 5 * S, 3 * S / 2),
        ("relay", relay(), 8 * S, 2 * S),
    ] {
        let (clock, sound) = join_to_first_sound(ways);
        eprintln!(
            "{what}: clock ready {} ms, first sound {} ms after joining",
            clock / MS,
            sound / MS
        );
        assert!(
            clock <= clock_max,
            "{what}: clock ready in {} ms",
            clock / MS
        );
        assert!(
            sound <= sound_max,
            "{what}: first sound in {} ms",
            sound / MS
        );
    }
}

#[test]
fn a7_a8_a_knock_waits_until_the_host_lets_them_in_or_says_no() {
    let mut w = World::open(songs(2), Opening::Resting);
    w.people[0].host().set_let_in(false);
    let r = w.arrive("Ravi", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[r].guest().stage() == Stage::AtTheDoor, 2_000));
    assert!(w.people[r].calls().is_empty(), "nothing taken at the door");
    let id = w.room().view_for(HOST).waiting[0].0;
    w.people[0].host().admit(id);
    assert!(w.run_until(|w| w.people[r].guest().stage() == Stage::In, 1_000));
    assert_eq!(
        w.people[r].calls()[0],
        Call::Attach {
            mode: SyncMode::Apart
        }
    );
    let m = w.arrive("Meera", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[m].guest().stage() == Stage::AtTheDoor, 2_000));
    let id = w.room().view_for(HOST).waiting[0].0;
    w.people[0].host().deny(id);
    assert!(w.run_until(|w| w.people[m].guest().stage() == Stage::Ended, 1_000));
    assert_eq!(
        w.people[m].guest().why().as_deref(),
        Some("The host didn't let you in this time.")
    );
    assert!(w.people[m].calls().is_empty(), "never took Meera's player");
}

#[test]
fn a9_a_full_room_and_an_ended_invite_are_refused_in_words() {
    let mut w = World::open(songs(2), Opening::Resting);
    for n in 1..10 {
        w.join(&format!("Guest {n}"), Way::LAN, Way::LAN);
    }
    let x = w.arrive("Eleventh", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[x].guest().stage() == Stage::Ended, 2_000));
    assert_eq!(
        w.people[x].guest().why().as_deref(),
        Some("The room is full (10 people).")
    );
    let mut w = World::open(songs(2), Opening::Resting);
    w.invite_open
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let x = w.arrive("Late", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[x].guest().stage() == Stage::Ended, 2_000));
    assert_eq!(
        w.people[x].guest().why().as_deref(),
        Some("This room's invite has ended. Ask for a new one.")
    );
}

#[test]
fn a9_a_0_3_guest_is_told_different_versions_and_closed() {
    let mut w = World::open(songs(2), Opening::Resting);
    // A protocol 2 guest: its hello, word for word.
    let raw = w.net.peer(std::sync::Arc::new(w.clock.clone()));
    let rx = raw.events();
    w.net.connect(&raw, &w.host_t(), Way::LAN, Way::LAN);
    let hello = format!(
        "{{\"t\":\"hello\",\"protocol\":2,\"name\":\"Old\",\"proof\":\"{}\",\"app\":\"Venuta 0.3.0\"}}\n",
        "ab".repeat(32)
    );
    raw.send(w.peer(0), ControlMsg(hello.into_bytes())).unwrap();
    w.run_ms(1_000);
    let got: Vec<TransportEvent> = rx.try_iter().collect();
    let refused = got.iter().find_map(|e| match e {
        TransportEvent::Message { msg, .. } => Some(String::from_utf8(msg.0.clone()).unwrap()),
        _ => None,
    });
    assert_eq!(
        refused.as_deref(),
        Some(format!("{{\"t\":\"refused\",\"why\":\"{DIFFERENT_VERSIONS}\"}}\n").as_str()),
        "protocol 2's own shape, today's words"
    );
    assert!(
        got.iter().any(|e| matches!(e, TransportEvent::Lost { .. })),
        "then closed"
    );
    assert!(!w.has_member("Old"));
}

#[test]
fn a9_a_0_3_host_says_different_versions_and_the_guest_shows_its_words() {
    // A protocol 2 host answers our hello with its refusal (the same shape).
    let mut w = World::open(songs(1), Opening::Resting);
    let old_host = w.net.peer(std::sync::Arc::new(w.clock.clone()));
    let hrx = old_host.events();
    let t = w.net.peer(std::sync::Arc::new(w.clock.clone()));
    let (player, log) = together::fakes::FakeRoomPlayer::<String>::new(
        std::sync::Arc::new(w.clock.clone()),
        Default::default(),
    );
    let conductor = together::conductor::Conductor::new(
        std::sync::Arc::new(player),
        together::clock_map::handle(),
        Box::new(TestCopies(Default::default())),
        ListenMode::Apart,
    );
    let mut s = together::session::GuestSession::new(
        std::sync::Arc::new(t.clone()),
        old_host.id(),
        std::sync::Arc::new(w.clock.clone()),
        conductor,
        together::session::GuestConfig {
            name: "Ravi".into(),
            proof: "cd".repeat(32),
            app: "Venuta test".into(),
            host_name: "Asha".into(),
            mode: Some(ListenMode::Apart),
        },
    );
    w.net.connect(&t, &old_host, Way::LAN, Way::LAN);
    for _ in 0..100 {
        w.run_ms(5);
        s.step();
        for e in hrx.try_iter() {
            if let TransportEvent::Message { from, msg, .. } = e {
                let line = String::from_utf8(msg.0).unwrap();
                assert!(line.contains("\"protocol\":3"), "our hello: {line}");
                let old = "{\"t\":\"refused\",\"why\":\"Your Venuta and the host's are different versions. Update both, then join again.\"}\n";
                old_host
                    .send(from, ControlMsg(old.as_bytes().to_vec()))
                    .unwrap();
            }
        }
    }
    assert_eq!(s.stage(), Stage::Ended);
    assert_eq!(s.why().as_deref(), Some(DIFFERENT_VERSIONS));
    assert!(log.just_calls().is_empty());
}

#[test]
fn a10_a_joiner_mid_song_catches_up_and_disturbs_nobody() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    let a = w.start();
    w.run_ms(20_000);
    let t = w.sim_ns();
    let before = place(&mut w);
    let k = w.join("Kabir", Way::LAN, Way::LAN);
    w.run_ms(3_000);
    for i in 0..3 {
        assert!(
            w.people[i].calls_since(t).is_empty(),
            "{} hears nothing of a joiner",
            w.people[i].name
        );
    }
    let p = w.people[k].prepares();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].1, a.entry);
    // Where the room will be once Kabir is ready: a second or two on from the join.
    let ahead = p[0].2 - before;
    assert!(
        (S..=3 * S).contains(&ahead),
        "caught up {} ms ahead",
        ahead / MS
    );
    assert_eq!(w.people[k].commits(), vec![a]);
    assert_eq!(w.room().anchor(), Some(a), "the room never moved");
}

#[test]
fn a11_joining_while_a_start_gets_ready_is_waited_for() {
    let mut w = World::lan(&["Ravi"]);
    for p in &w.people {
        p.log.script(|s| s.ready_after_ns = 1_500 * NS_MS);
    }
    w.host_ask(Action::Play);
    w.run_ms(300);
    let m = w.arrive("Meera", Way::LAN, Way::LAN, ListenMode::Apart);
    w.people[m].log.script(|s| s.ready_after_ns = 1_500 * NS_MS);
    assert!(w.run_until(|w| w.room().anchor().is_some_and(|a| a.playing), 5_000));
    let prep = w.room().anchor().unwrap();
    let p = w.people[m].prepares();
    assert_eq!(p.len(), 1, "the room's prepare, not a catch-up");
    assert_eq!(p[0].1, prep.entry);
    assert_eq!(p[0].0, w.people[1].prepares()[0].0, "the same prepare");
    assert!(w.run_until(|w| !w.people[m].commits().is_empty(), 1_000));
}

#[test]
fn a12_a_joiner_into_a_paused_room_gets_the_held_place_ready_in_silence() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.run_ms(10_000);
    w.host_ask(Action::Pause);
    w.run_ms(1_000);
    let held = w.room().anchor().unwrap();
    assert!(!held.playing);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    w.run_ms(2_000);
    let p = w.people[m].prepares();
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].1, p[0].2), (held.entry, held.content_us));
    assert_eq!(w.people[m].holds(), vec![held]);
    assert!(w.people[m].commits().is_empty(), "nothing plays");
}

#[test]
fn a13_a_joiner_into_a_room_whose_queue_ended_hears_nothing() {
    let mut w = World::open(vec![song(1, 30)], Opening::Resting);
    w.start();
    w.run_ms(32_000);
    assert_eq!(w.room().phase(), Phase::QueueEnded);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    w.run_ms(2_000);
    assert_eq!(
        w.people[m].calls(),
        vec![Call::Attach {
            mode: SyncMode::Apart
        }]
    );
}

#[test]
fn a17_start_with_the_next_song_waits_for_the_boundary() {
    let mut w = World::open(vec![song(1, 40), song(2, 40)], Opening::Resting);
    w.join("Ravi", Way::LAN, Way::LAN);
    let a = w.start();
    w.run_ms(10_000);
    let m = w.arrive("Meera", Way::LAN, Way::LAN, ListenMode::Apart);
    w.people[m].guest().conductor().start_with_next();
    w.run_ms(3_000);
    assert!(w.people[m].prepares().is_empty(), "no catch-up");
    assert_eq!(w.people[m].commits(), vec![a], "the room's anchor, kept");
    assert!(w.run_until(|w| !w.people[m].arms().is_empty(), 30_000));
    assert_eq!(w.people[m].arms()[0].0, w.entry(1), "in with the next song");
}

#[test]
fn a18_a_joiner_with_no_copy_stays_and_is_back_for_the_next() {
    let mut w = World::open(vec![song(1, 40), song(2, 40)], Opening::Resting);
    w.join("Ravi", Way::LAN, Way::LAN);
    w.start();
    w.run_ms(10_000);
    let m = w.arrive("Meera", Way::LAN, Way::LAN, ListenMode::Apart);
    w.people[m]
        .rules
        .lock()
        .unwrap()
        .none
        .insert(song(1, 40).key);
    assert!(w.run_until(|w| w.people[m].guest().stage() == Stage::In, 2_000));
    let id = w.people[m].guest().me().unwrap();
    assert!(w.run_until(
        |w| {
            w.room()
                .view_for(HOST)
                .members
                .iter()
                .any(|p| p.id == id && p.sub_state == Some(SubState::CannotPlay))
        },
        3_000
    ));
    assert!(w.people[m].prepares().is_empty());
    assert!(w.run_until(|w| !w.people[m].arms().is_empty(), 30_000));
    assert_eq!(w.people[m].arms()[0].0, w.entry(1));
    assert_eq!(w.room().phase(), Phase::Playing, "never stalled the room");
}

// --- B. Playback control ------------------------------------------------------------------

#[test]
fn b_every_press_from_a_guest_reaches_the_room_and_every_player_follows() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.run_ms(10_000);
    let presses = [
        Action::Pause,
        Action::Play,
        Action::Seek { content_us: 60 * S },
        Action::Next,
        Action::Previous,
        Action::Jump { entry: EntryId(4) },
    ];
    for a in presses {
        w.people[1].guest().ask(a.clone());
        w.run_ms(3_000);
        let room = w.room().anchor().unwrap();
        for i in 0..3 {
            let last = w.people[i]
                .calls()
                .into_iter()
                .rev()
                .find_map(|c| match c {
                    Call::Commit(x) | Call::Hold(x) => Some(x),
                    _ => None,
                })
                .unwrap();
            assert_eq!(last, room, "{} after {a:?}", w.people[i].name);
        }
    }
    assert_eq!(w.room().anchor().unwrap().entry, EntryId(4));
}

#[test]
fn b12_a_press_while_reconnecting_goes_when_back_only_while_fresh() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.run_ms(5_000);
    w.cut(1);
    assert!(w.run_until(
        |w| w.people[1].guest().stage() == Stage::Reconnecting,
        20_000
    ));
    w.people[1].guest().ask(Action::Pause);
    w.run_ms(4_000);
    w.restore(1);
    assert!(
        w.run_until(|w| w.room().phase() == Phase::Held, 3_000),
        "the fresh press went"
    );
    w.host_ask(Action::Play);
    w.run_ms(3_000);
    w.cut(1);
    assert!(w.run_until(
        |w| w.people[1].guest().stage() == Stage::Reconnecting,
        20_000
    ));
    w.people[1].guest().ask(Action::Next);
    let entry = w.room().anchor().unwrap().entry;
    w.run_ms(12_000);
    w.restore(1);
    w.run_ms(3_000);
    assert_eq!(
        w.room().anchor().unwrap().entry,
        entry,
        "the old press never went"
    );
    assert!(
        w.people[1]
            .guest()
            .take_notes()
            .iter()
            .any(|n| n == "Your skip was too late to send."),
        "and was said"
    );
}

#[test]
fn b13_a_press_without_the_role_is_asked_of_whoever_controls() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.people[0]
        .host()
        .act(|r, now| r.set_new_role(Some(Role::Listen), now));
    w.run_ms(500);
    w.people[1].guest().request(Action::Next);
    w.run_ms(500);
    let v = w.room().view_for(HOST);
    assert_eq!(v.requests.len(), 1, "the host sees Ravi's request");
    let id = v.requests[0].id;
    let entry = w.room().anchor().unwrap().entry;
    w.host_ask(Action::Accept { request: id });
    assert!(w.run_until(|w| w.room().anchor().unwrap().entry != entry, 3_000));
}

#[test]
fn b16_j3_pause_for_me_steps_out_alone_and_catches_up() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.run_ms(5_000);
    let t = w.sim_ns();
    w.people[1].guest().conductor().step_out();
    w.run_ms(3_000);
    assert!(w.people[1].calls().contains(&Call::StepOut));
    let id = w.people[1].guest().me().unwrap();
    assert!(
        w.room()
            .view_for(HOST)
            .members
            .iter()
            .any(|m| m.id == id && m.sub_state == Some(SubState::SteppedOut))
    );
    for i in [0, 2] {
        assert!(w.people[i].calls_since(t).is_empty());
    }
    assert_eq!(w.room().phase(), Phase::Playing);
    w.people[1].guest().conductor().rejoin();
    w.run_ms(500);
    assert_eq!(w.people[1].calls().last(), Some(&Call::Rejoin));
}

// --- C and D. The queue, and from one song to the next ----------------------------------------

#[test]
fn c5_a_new_next_is_armed_again_and_a_withdrawn_one_disarmed() {
    let list = (1..=4).map(|i| song(i, 30)).collect();
    let mut w = World::open(list, Opening::Resting);
    w.join("Ravi", Way::LAN, Way::LAN);
    w.start();
    assert!(w.run_until(|w| !w.people[1].arms().is_empty(), 15_000));
    assert_eq!(w.people[1].arms()[0].0, w.entry(1));
    let third = w.entry(2);
    w.host_ask(Action::Move {
        entry: third,
        to: 1,
    });
    assert!(w.run_until(|w| w.people[1].arms().last().unwrap().0 == third, 1_000));
    let after: Vec<EntryId> = w.room().queue()[1..].iter().map(|e| e.id).collect();
    for e in after {
        let _ = w.people[0].host().ask(Action::Remove { entry: e });
    }
    assert!(w.run_until(|w| w.people[1].calls().last() == Some(&Call::Disarm), 1_000));
}

#[test]
fn d1_the_next_is_armed_at_the_hosts_reference_length() {
    let list = vec![song(1, 30), song(2, 30)];
    let mut w = World::open(list, Opening::Resting);
    // The host's player decodes the first song at 29.5 s, not the tag's 30.
    w.people[0].log.script(|s| s.length_us = Some(29_500_000));
    w.join("Ravi", Way::LAN, Way::LAN);
    let a = w.start();
    assert!(w.run_until(|w| !w.people[1].arms().is_empty(), 15_000));
    let (entry, boundary) = w.people[1].arms()[0];
    assert_eq!(entry, w.entry(1));
    assert_eq!(boundary, a.host_us + 29_500_000 - a.content_us);
}

#[test]
fn d5_the_rooms_crossfade_goes_to_every_player() {
    let list = vec![song(1, 30), song(2, 30)];
    let mut w = World::open(list, Opening::Resting);
    w.join("Ravi", Way::LAN, Way::LAN);
    w.people[0].host().act(|r, now| {
        r.set_transition(Transition::Crossfade { ms: 4_000 }, now)
            .unwrap()
    });
    w.start();
    assert!(w.run_until(|w| !w.people[1].arms().is_empty(), 15_000));
    let t = w.people[1].calls().into_iter().find_map(|c| match c {
        Call::ArmNext { transition, .. } => Some(transition),
        _ => None,
    });
    assert_eq!(t, Some(Transition::Crossfade { ms: 4_000 }));
}

#[test]
fn d10_someone_without_the_next_says_so_and_is_back_for_the_one_after() {
    let list = (1..=3).map(|i| song(i, 30)).collect();
    let mut w = World::open(list, Opening::Resting);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    w.people[m]
        .rules
        .lock()
        .unwrap()
        .none
        .insert(song(2, 30).key);
    w.start();
    w.run_ms(40_000);
    let second = w.entry(1);
    assert!(
        !w.people[m].arms().iter().any(|a| a.0 == second),
        "nothing armed without a copy"
    );
    assert!(w.run_until(|w| w.people[m].arms().iter().any(|a| a.0 != second), 30_000));
}

#[test]
fn d11_an_entry_nobody_can_play_is_skipped() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    for p in &w.people {
        p.rules.lock().unwrap().none.insert(song(1, 200).key);
    }
    w.host_ask(Action::Play);
    assert!(w.run_until(
        |w| w.room().anchor().is_some_and(|a| a.entry != EntryId(1)),
        10_000
    ));
    assert!(
        w.room()
            .view_for(HOST)
            .activity
            .iter()
            .any(|a| a.what.contains("no one could play it"))
    );
}

// --- F. Network -------------------------------------------------------------------------------

#[test]
fn f4_f9_a_change_of_path_measures_the_clock_again_with_a_burst() {
    let (up, down) = relay();
    let mut w = World::open(songs(2), Opening::Resting);
    let r = w.join("Ravi", up, down);
    w.run_ms(5_000);
    let from = w.sim_ns();
    w.set_ways(r, Way::LAN, Way::LAN);
    w.run_ms(1_200);
    let to = w.sim_ns();
    let pings = w
        .net
        .sent()
        .iter()
        .filter(|s| {
            s.from == w.peer(r)
                && s.kind == together::sim_net::SentKind::Datagram
                && (from..to).contains(&s.at_ns)
        })
        .count();
    assert!(pings >= 20, "a burst on the new path: {pings} pings");
    assert!(w.people[r].guest().clock_ready());
}

#[test]
fn f7_a_cut_under_two_minutes_plays_on_and_comes_back_quietly() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.run_ms(5_000);
    let t = w.sim_ns();
    w.cut(1);
    w.run_ms(60_000);
    assert_eq!(w.people[1].guest().stage(), Stage::Reconnecting);
    assert!(w.people[1].calls_since(t).is_empty(), "plays on, untouched");
    let back = w.sim_ns();
    w.restore(1);
    assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::In, 2_000));
    assert!(
        (w.sim_ns() - back) <= 2_000 * NS_MS,
        "back within 2 s of the connection"
    );
    w.run_ms(1_000);
    assert!(
        w.people[1].calls_since(t).is_empty(),
        "nothing changed: nothing heard"
    );
    let id = w.people[1].guest().me().unwrap();
    assert!(
        w.room()
            .view_for(HOST)
            .members
            .iter()
            .any(|m| m.id == id && !m.away)
    );
}

#[test]
fn f8_i4_lost_for_good_finishes_the_song_playing_then_lets_go() {
    let mut w = World::lan(&["Ravi"]);
    let a = w.start();
    w.run_ms(5_000);
    w.cut(1);
    w.set_step_ms(20);
    // Lost at the 15 s idle timeout, then 2 minutes: 140 s on; the song (200 s) still plays.
    w.run_ms(145_000);
    assert!(!w.people[1].detached(), "never mid-song");
    assert!(
        w.people[1].calls().contains(&Call::Disarm),
        "nothing new starts"
    );
    let end_host = a.host_us + 200 * S - a.content_us;
    assert!(w.run_until(|w| w.people[1].detached(), 60_000));
    let at = w.now();
    assert!(
        at >= end_host && at <= end_host + S,
        "at the song's end ({} ms after it)",
        (at - end_host) / MS
    );
    assert_eq!(w.people[1].guest().stage(), Stage::Ended);
    assert_eq!(
        w.people[1].guest().why().as_deref(),
        Some("Lost the connection to Asha's room.")
    );
}

#[test]
fn f12_every_device_is_turned_to_apart_when_the_path_slows() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.people[1]
        .guest()
        .conductor()
        .set_mode(ListenMode::EveryDevice);
    w.run_ms(1_000);
    assert!(
        w.people[1]
            .calls()
            .contains(&Call::SetMode(SyncMode::EveryDevice))
    );
    w.set_ways(1, Way::internet(12.0, 1.0), Way::internet(12.0, 1.0));
    w.set_step_ms(20);
    w.run_ms(45_000);
    assert!(
        w.people[0]
            .host()
            .take_notes()
            .iter()
            .any(|n| matches!(n, HostNote::SameRoomOff { .. }))
    );
    assert_eq!(
        w.people[1].calls().last(),
        Some(&Call::SetMode(SyncMode::Apart)),
        "the player follows the room's word"
    );
}

#[test]
fn f13_the_hosts_blip_greys_the_room_and_the_music_goes_on() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.cut(1);
    w.run_ms(20_000);
    let v = w.people[1].guest().window(&Default::default());
    assert!(v.room.as_ref().is_some_and(|r| r.host_away));
    assert_eq!(v.state, together::view::SessionState::Reconnecting);
}

// --- G. Buffering and stalls --------------------------------------------------------------

#[test]
fn g3_when_everyone_stalls_the_room_holds_and_resumes_together() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.run_ms(5_000);
    for p in &w.people {
        p.log
            .force(Some(SubState::Suspended(Suspension::Buffering)));
    }
    assert!(w.run_until(|w| w.room().phase() == Phase::Held, 6_000));
    w.run_ms(200);
    for i in 0..3 {
        assert!(
            !w.people[i].holds().is_empty(),
            "held for {}: {:?}",
            w.people[i].name,
            w.people[i].calls()
        );
    }
    for p in &w.people {
        p.log.force(None);
    }
    assert!(w.run_until(|w| w.room().phase() == Phase::Playing, 6_000));
}

#[test]
fn g7_the_host_tells_a_slow_guest_to_start_with_the_next_song_or_take_a_lighter_copy() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.people[2]
        .log
        .script(|s| s.ready_after_ns = 3_600_000 * NS_MS);
    w.start();
    for _ in 0..3 {
        w.host_ask(Action::Next);
        w.run_ms(4_000);
    }
    let notes = w.people[0].host().take_notes();
    let slow = notes
        .iter()
        .find_map(|n| match n {
            HostNote::Slow { id } => Some(*id),
            _ => None,
        })
        .expect("told Meera is slow");
    w.people[0].host().advise(slow, Advice::LighterCopy);
    w.people[0].host().advise(slow, Advice::StartWithNext);
    w.run_ms(500);
    assert!(w.people[2].rules.lock().unwrap().lighter);
    assert!(w.people[2].guest().conductor().starting_with_next());
}

// --- H and I. Devices, leaving and ending -------------------------------------------------

#[test]
fn h12_i8_a_guest_that_slept_measures_again_and_comes_back_in() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.run_ms(5_000);
    let epoch = w.people[1].guest().clock_epoch();
    // Ravi's computer sleeps 40 s: nothing of it runs, its link is down.
    w.cut(1);
    w.asleep(1, true);
    w.run_ms(40_000);
    w.restore(1);
    w.asleep(1, false);
    w.run_ms(5);
    assert!(
        w.people[1]
            .calls()
            .contains(&Call::Suspend(Suspension::Sleeping)),
        "everything buffered dropped"
    );
    assert!(w.people[1].guest().clock_epoch() > epoch, "a new epoch");
    assert!(w.run_until(|w| w.people[1].calls().last() == Some(&Call::Resume), 3_000));
    assert!(w.people[1].guest().clock_ready());
    assert_eq!(w.people[1].guest().stage(), Stage::In);
}

#[test]
fn i1_a_guest_leaves_at_once_and_nobody_else_hears_it() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.run_ms(3_000);
    let t = w.sim_ns();
    w.people[1].guest().leave();
    assert!(w.people[1].detached(), "let go in the same moment");
    w.run_ms(1_000);
    assert!(!w.has_member("Ravi"));
    for i in [0, 2] {
        assert!(w.people[i].calls_since(t).is_empty());
    }
    assert_eq!(w.people[1].guest().stage(), Stage::Ended);
}

#[test]
fn i2_a_guest_taken_out_lets_go_with_neutral_words() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    let id = w.people[1].guest().me().unwrap();
    w.people[0].host().remove(id, true);
    let t = w.sim_ns();
    assert!(w.run_until(|w| w.people[1].detached(), 1_000));
    assert!(w.sim_ns() - t <= 100 * NS_MS, "within 100 ms on one Wi-Fi");
    assert_eq!(
        w.people[1].guest().why().as_deref(),
        Some("Asha took you out of the room.")
    );
    assert!(!w.people[2].detached());
}

#[test]
fn i3_ending_for_everyone_lets_everyones_player_go_the_hosts_too() {
    let mut w = World::lan(&["Ravi", "Meera", "Kabir"]);
    w.start();
    w.run_ms(3_000);
    w.people[0].host().end();
    assert!(w.people[0].detached(), "the host at once");
    assert!(w.run_until(|w| (1..4).all(|i| w.people[i].detached()), 100));
    for i in 1..4 {
        assert_eq!(
            w.people[i].guest().why().as_deref(),
            Some("Asha ended the room.")
        );
    }
    assert!(w.people[0].host().ended());
}

#[test]
fn i5_the_host_sleeping_a_while_comes_back_and_everyone_with_it() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.run_ms(3_000);
    w.cut(1);
    w.asleep(0, true);
    w.run_ms(30_000);
    w.restore(1);
    w.asleep(0, false);
    assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::In, 5_000));
    assert!(
        w.people[0]
            .calls()
            .contains(&Call::Suspend(Suspension::Sleeping))
    );
    assert!(w.run_until(|w| w.people[0].calls().last() == Some(&Call::Resume), 2_000));
    assert!(!w.people[1].detached());
}

#[test]
fn i9_an_invite_that_ends_turns_new_people_away_and_the_room_goes_on() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.invite_open
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let x = w.arrive("Late", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[x].guest().stage() == Stage::Ended, 2_000));
    w.run_ms(5_000);
    assert_eq!(w.people[1].guest().stage(), Stage::In);
    assert_eq!(w.room().phase(), Phase::Playing);
}

#[test]
fn i11_a_held_room_idle_for_an_hour_ends_and_lets_everyone_go() {
    let mut w = World::lan(&["Ravi"]);
    w.start();
    w.host_ask(Action::Pause);
    w.set_step_ms(200);
    w.run_ms(3_601_000);
    assert!(w.people[0].detached() && w.people[1].detached());
    assert!(w.people[0].host().ended());
}

// --- E. Copies and alignment (WP11) ------------------------------------------------------

/// A song only the host has (its file, streamed to whoever asks).
fn host_song(n: u32, secs: u64) -> Song {
    Song {
        key: Song::host_key(&format!("Album/{n:02}.flac")),
        online_id: None,
        host_file: true,
        format: Some("flac".into()),
        ..song(n, secs)
    }
}

/// The copy someone's row shows in `viewer`'s window.
fn copy_seen(w: &mut World, viewer: usize, of: usize) -> Option<CopyView> {
    let id = if of == 0 {
        HOST
    } else {
        w.people[of].guest().me().unwrap()
    };
    let v = match viewer {
        0 => w.people[0].host().window(&Default::default()),
        i => w.people[i].guest().window(&Default::default()),
    };
    v.room?.members.into_iter().find(|m| m.id == id)?.copy
}

/// Someone's first prepare: when it was made (the fake's clock) and its alignment.
fn first_prepare(w: &mut World, i: usize) -> (u64, Alignment) {
    assert!(w.run_until(|w| !w.people[i].prepares().is_empty(), 4_000));
    let (at, c) = w.people[i]
        .log
        .calls()
        .into_iter()
        .find(|(_, c)| matches!(c, Call::Prepare { .. }))
        .unwrap();
    match c {
        Call::Prepare { align, .. } => (at, align),
        _ => unreachable!(),
    }
}

#[test]
fn e1_d5_the_hosts_file_is_exact_with_nothing_to_measure_and_first_in_every_device() {
    let mut w = World::open_measured(vec![host_song(1, 200), host_song(2, 200)], Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    let key = host_song(1, 200).key;
    w.people[r].rules.lock().unwrap().host_copy.insert(key);
    // Every device puts the host's file first (D5), asked for or not.
    w.people[m]
        .guest()
        .conductor()
        .set_mode(ListenMode::EveryDevice);
    w.run_ms(1_500);
    w.start();
    for i in [r, m] {
        let (_, a) = first_prepare(&mut w, i);
        assert_eq!(a, Alignment::EXACT, "{}", w.people[i].name);
    }
    w.run_ms(2_000);
    let seen = copy_seen(&mut w, 0, r).expect("the host sees Ravi's copy");
    assert_eq!(
        (seen.kind, seen.class),
        (CopyKind::HostStream, CopyClass::Exact)
    );
    assert_eq!(seen.words(), "your file, streamed, matched");
    assert_eq!(
        copy_seen(&mut w, r, r).map(|c| c.class),
        Some(CopyClass::Exact)
    );
    assert_eq!(
        copy_seen(&mut w, m, m).map(|c| c.kind),
        Some(CopyKind::HostStream)
    );
}

#[test]
fn e2_an_own_copy_that_decodes_the_same_is_exact() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.start();
    let (_, a) = first_prepare(&mut w, r);
    assert_eq!(a.class, CopyClass::Exact);
    assert_eq!(a.offset_us, 0);
    w.run_ms(1_000);
    assert_eq!(
        copy_seen(&mut w, 0, r).map(|c| c.words()).as_deref(),
        Some("YouTube Music, matched")
    );
}

#[test]
fn e3_another_master_is_measured_then_refined_through_the_song_by_glides() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.people[r].truth(
        &song(1, 200).key,
        Truth {
            offsets: vec![12_000, 14_000, 17_000, 21_000],
            same_pcm: false,
            ..Truth::default()
        },
    );
    w.start();
    let (_, a) = first_prepare(&mut w, r);
    assert_eq!(a.class, CopyClass::Measured);
    assert_eq!(a.offset_us, 12_000);
    let e = w.entry(0);
    // Each later window moves the offset when the room reaches it, never before.
    for (window_s, offset) in [(60, 14_000), (120, 17_000), (180, 21_000)] {
        assert!(w.run_until(|w| place(w) >= (window_s - 1) * S, 200_000));
        assert!(
            w.people[r]
                .refines()
                .iter()
                .all(|x| x.2.offset_us != offset),
            "not before {window_s} s"
        );
        assert!(w.run_until(|w| place(w) >= window_s * S + 100 * MS, 3_000));
        let last = w.people[r].refines().last().cloned().expect("refined");
        assert_eq!((last.1, last.2.offset_us), (e, offset));
        assert_eq!(last.2.class, CopyClass::Measured);
    }
    // Every move a glide: never 20 ms or more at once.
    let mut was = 12_000;
    for (_, _, x) in w.people[r].refines() {
        assert!(
            (x.offset_us - was).abs() <= 20_000,
            "{} to {}",
            was,
            x.offset_us
        );
        was = x.offset_us;
    }
}

#[test]
fn e4_a_catalog_copy_is_measured_with_its_encoder_delay() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.people[r].truth(&song(1, 200).key, Truth::at(-25_000));
    w.people[r].truth(&song(2, 200).key, Truth::at(-26_000));
    w.start();
    let (_, a) = first_prepare(&mut w, r);
    assert_eq!((a.class, a.offset_us), (CopyClass::Measured, -25_000));
    w.run_ms(1_000);
    assert_eq!(
        copy_seen(&mut w, r, r).map(|c| c.words()).as_deref(),
        Some("YouTube Music, matched")
    );
    // The next song, armed ahead, measured too.
    assert!(w.run_until(|w| !w.people[r].arms().is_empty(), 200_000));
    let next = w.entry(1);
    let arm = w.people[r]
        .aligns()
        .into_iter()
        .find(|(e, _)| *e == next)
        .expect("armed");
    assert_eq!(
        (arm.1.class, arm.1.offset_us),
        (CopyClass::Measured, -26_000)
    );
}

#[test]
fn e5_d14_another_edit_turns_approximate_where_it_shows_for_its_listener_and_the_host_only() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    w.people[r].truth(
        &song(1, 200).key,
        Truth {
            edit_ms: Some(120_000),
            ..Truth::at(3_000)
        },
    );
    w.start();
    let (_, a) = first_prepare(&mut w, r);
    assert_eq!((a.class, a.offset_us), (CopyClass::Measured, 3_000));
    assert!(w.run_until(|w| place(w) >= 119 * S, 200_000));
    assert!(
        w.people[r].refines().is_empty(),
        "the same offset: nothing to refine"
    );
    assert!(w.run_until(|w| place(w) >= 122 * S, 5_000));
    let x = w.people[r].refines();
    assert_eq!(x.len(), 1);
    assert_eq!(
        (x[0].2.class, x[0].2.offset_us),
        (CopyClass::Approximate, 3_000),
        "approximate from the edit, the offset kept (never a jump)"
    );
    w.run_ms(1_000);
    assert_eq!(
        copy_seen(&mut w, 0, r).map(|c| c.words()).as_deref(),
        Some("YouTube Music, another version")
    );
    assert_eq!(
        copy_seen(&mut w, r, r).map(|c| c.class),
        Some(CopyClass::Approximate)
    );
    assert_eq!(copy_seen(&mut w, m, r), None, "nobody else sees it (D14)");
}

#[test]
fn e6_a_song_the_host_cant_play_is_approximate_for_everyone_at_once() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    w.people[0]
        .rules
        .lock()
        .unwrap()
        .none
        .insert(song(1, 200).key);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.people[r].truth(&song(1, 200).key, Truth::at(40_000));
    let t0 = w.sim_ns();
    w.host_ask(Action::Play);
    let (at, a) = first_prepare(&mut w, r);
    assert_eq!(a, Alignment::APPROXIMATE);
    assert!(
        at - t0 < 400 * NS_MS,
        "no waiting for a measure that can't come: {} ms",
        (at - t0) / NS_MS
    );
}

#[test]
fn a_measure_too_slow_is_given_up_at_one_second_and_taken_later_only_within_a_glide() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    let key = song(1, 200).key;
    let slow = |offset| Truth {
        start_ms: 3_000,
        whole_ms: 3_000,
        ..Truth::at(offset)
    };
    w.people[r].truth(&key, slow(8_000));
    w.people[m].truth(&key, slow(300_000));
    let t0 = w.sim_ns();
    w.host_ask(Action::Play);
    for i in [r, m] {
        let (at, a) = first_prepare(&mut w, i);
        assert_eq!(a, Alignment::APPROXIMATE, "{}", w.people[i].name);
        let ms = (at - t0) / NS_MS;
        assert!((900..1_500).contains(&ms), "given up at 1 s: {ms} ms");
    }
    w.run_ms(4_000);
    let x = w.people[r].refines();
    assert_eq!(x.len(), 1, "within a glide: taken");
    assert_eq!(
        (x[0].2.class, x[0].2.offset_us),
        (CopyClass::Measured, 8_000)
    );
    assert!(
        w.people[m].refines().is_empty(),
        "300 ms off: never jumped to for alignment alone"
    );
    // The next voice of the song opens with it.
    w.host_ask(Action::Seek { content_us: 30 * S });
    assert!(w.run_until(|w| w.people[m].prepares().len() == 2, 3_000));
    let last = w.people[m]
        .aligns()
        .into_iter()
        .rfind(|(_, a)| a.class == CopyClass::Measured)
        .expect("measured");
    assert_eq!(last.1.offset_us, 300_000);
}

#[test]
fn e9_every_device_on_another_version_is_offered_the_hosts_copy_one_speaker_or_apart() {
    let mut w = World::open_measured(songs(2), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.people[r].truth(
        &song(1, 200).key,
        Truth {
            edit_ms: Some(10_000),
            ..Truth::at(0)
        },
    );
    w.people[r]
        .guest()
        .conductor()
        .set_mode(ListenMode::EveryDevice);
    w.run_ms(1_500);
    w.start();
    let (_, a) = first_prepare(&mut w, r);
    assert_eq!(a.class, CopyClass::Approximate);
    w.run_ms(1_500);
    let v = w.people[r].guest().window(&Default::default());
    let choices = v.you.unwrap().choices;
    let every = choices
        .iter()
        .find(|c| c.mode == ListenMode::EveryDevice)
        .unwrap();
    assert_eq!(
        every.why_not.as_deref(),
        Some("Your copy is another version: use Asha's copy, or listen apart")
    );
    for mode in [ListenMode::OneSpeaker, ListenMode::Apart] {
        let c = choices.iter().find(|c| c.mode == mode).unwrap();
        assert_eq!(c.why_not, None);
    }
}

/// `key`'s reference as person `i` gets it on the `env:` stream (`None`: nothing served).
fn fetch_reference(w: &mut World, i: usize, key: &str) -> Option<together::align::Reference> {
    use std::task::Poll;
    use together::sim_net::poll_once;
    use together::transport::Get;
    let mut f = w.people[i].t.open_media(
        w.peer(0),
        Get {
            get: format!("env:{key}"),
            from: 0,
            to: None,
        },
    );
    let mut s = loop {
        if let Poll::Ready(s) = poll_once(&mut f) {
            break s.ok()?;
        }
        w.run_ms(5);
    };
    let mut body = Vec::new();
    loop {
        let got = loop {
            let mut c = s.body.read_chunk();
            if let Poll::Ready(x) = poll_once(&mut c) {
                break x;
            }
            drop(c);
            w.run_ms(5);
        };
        match got.ok()? {
            Some(b) => body.extend_from_slice(&b),
            None => return together::align::decode_reference(&body).ok(),
        }
    }
}

#[test]
fn references_go_to_members_only_and_are_made_when_first_asked_for() {
    use together::align::Reference;
    let mut w = World::open_measured(songs(3), Opening::Resting);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.run_ms(200);
    // Never prepared yet: the first ask finds nothing, and the host makes it.
    let key = song(3, 200).key;
    assert_eq!(fetch_reference(&mut w, r, &key), None);
    w.run_ms(100);
    assert!(matches!(
        fetch_reference(&mut w, r, &key),
        Some(Reference::Whole(_))
    ));
    assert_eq!(
        fetch_reference(&mut w, r, "online:songzzzzzzz"),
        None,
        "never a song not in the room"
    );
    w.people[0].host().set_let_in(false);
    let x = w.arrive("Outsider", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[x].guest().stage() == Stage::AtTheDoor, 2_000));
    assert_eq!(
        fetch_reference(&mut w, x, &key),
        None,
        "not to someone at the door"
    );
}

// --- J. Modes -----------------------------------------------------------------------------

#[test]
fn j1_one_speaker_lets_go_of_the_player_and_playing_again_catches_up() {
    let mut w = World::lan(&["Ravi"]);
    let a = w.start();
    w.run_ms(3_000);
    w.people[1]
        .guest()
        .conductor()
        .set_mode(ListenMode::OneSpeaker);
    w.run_ms(1_000);
    assert!(w.people[1].detached(), "a remote plays nothing");
    let id = w.people[1].guest().me().unwrap();
    assert_eq!(w.room().mode_of(id), Some(ListenMode::OneSpeaker));
    let n = w.people[1].prepares().len();
    w.people[1].guest().conductor().set_mode(ListenMode::Apart);
    w.run_ms(2_000);
    let calls = w.people[1].calls();
    assert!(calls.contains(&Call::Attach {
        mode: SyncMode::Apart
    }));
    assert_eq!(w.people[1].prepares().len(), n + 1, "a late join");
    assert_eq!(w.people[1].commits().last(), Some(&a));
}

// --- One speaker (WP18) -------------------------------------------------------------------

#[test]
fn a16_on_the_hosts_network_a_guest_comes_in_on_the_hosts_speaker_unless_they_chose() {
    let mut w = World::open(songs(3), Opening::Resting);
    let r = w.arrive_by_network("Ravi", Way::LAN, Way::LAN);
    assert!(w.run_until(|w| w.people[r].guest().stage() == Stage::In, 3_000));
    w.run_ms(500);
    assert_eq!(
        w.people[r].guest().conductor().mode(),
        ListenMode::OneSpeaker
    );
    assert!(w.people[r].calls().is_empty(), "nothing of Ravi's plays");
    let id = w.people[r].guest().me().unwrap();
    assert_eq!(w.room().mode_of(id), Some(ListenMode::OneSpeaker));
    let y = w.people[r].guest().window(&Default::default()).you.unwrap();
    assert_eq!(
        y.choices.first().map(|c| c.mode),
        Some(ListenMode::OneSpeaker),
        "the host's speaker offered first"
    );
    assert!(y.choices.iter().any(|c| c.mode != ListenMode::OneSpeaker));
    assert_eq!(y.line, "Listening on Asha's speaker");
    // Over the internet: apart, and no question.
    let (up, down) = internet();
    let m = w.arrive_by_network("Meera", up, down);
    assert!(w.run_until(|w| w.people[m].guest().stage() == Stage::In, 5_000));
    w.run_ms(500);
    assert_eq!(w.people[m].guest().conductor().mode(), ListenMode::Apart);
    assert_eq!(
        w.people[m].calls().first(),
        Some(&Call::Attach {
            mode: SyncMode::Apart
        })
    );
    let y = w.people[m].guest().window(&Default::default()).you.unwrap();
    assert!(y.choices.is_empty());
    // Chosen while knocking: kept, and the room hears it.
    let d = w.arrive_by_network("Dev", Way::LAN, Way::LAN);
    w.people[d]
        .guest()
        .conductor()
        .set_mode(ListenMode::EveryDevice);
    assert!(w.run_until(|w| w.people[d].guest().stage() == Stage::In, 3_000));
    w.run_ms(4_000);
    let id = w.people[d].guest().me().unwrap();
    assert_eq!(w.room().mode_of(id), Some(ListenMode::EveryDevice));
    assert_eq!(
        w.people[d].calls().first(),
        Some(&Call::Attach {
            mode: SyncMode::EveryDevice
        })
    );
}

#[test]
fn j7_a_one_speaker_room_the_host_leads_and_remotes_see_and_control_by_role() {
    let mut w = World::open(songs(3), Opening::Resting);
    let r = w.join_as("Ravi", Way::LAN, Way::LAN, ListenMode::OneSpeaker);
    let m = w.join_as("Meera", Way::LAN, Way::LAN, ListenMode::OneSpeaker);
    w.run_ms(500);
    assert_eq!(
        w.people[0]
            .calls()
            .into_iter()
            .filter(|c| matches!(c, Call::Lead(_)))
            .collect::<Vec<_>>(),
        vec![Call::Lead(true)],
        "the host leads: every other member a remote"
    );
    w.host_ask(Action::Play);
    assert!(w.run_until(|w| w.room().phase() == Phase::Playing, 4_000));
    w.run_ms(10_000);
    for i in [r, m] {
        assert!(
            w.people[i].calls().is_empty(),
            "a remote attaches no player"
        );
    }
    let v = w.people[r].guest().window(&Default::default());
    let you = v.you.clone().unwrap();
    assert_eq!(you.sub_state, SubState::Remote);
    let room = v.room.clone().unwrap();
    assert_eq!(room.queue.len(), 3);
    let progress = room.anchor.unwrap().content_at(v.host_now_us.unwrap());
    let truth = place(&mut w);
    assert!(
        (progress - truth).abs() < 100 * MS,
        "progress from the anchor and the clock line: off by {} µs",
        progress - truth
    );
    // Controls by role: listening only, a skip is a request; with control, a press.
    let mid = w.people[m].guest().me().unwrap();
    w.people[0]
        .host()
        .act(|room, now| room.set_role(mid, Some(Role::Listen), now));
    w.run_ms(300);
    w.people[m].guest().request(Action::Next);
    w.run_ms(300);
    let hv = w.people[0].host().window(&Default::default());
    let words: Vec<String> = hv
        .room
        .unwrap()
        .requests
        .into_iter()
        .map(|q| q.words)
        .collect();
    assert_eq!(words, vec!["Meera asks to skip".to_owned()]);
    w.people[r].guest().ask(Action::Pause);
    assert!(w.run_until(|w| w.room().phase() == Phase::Held, 2_000));
    // Someone plays: the host's player follows the room's clock again.
    w.people[m].guest().conductor().set_mode(ListenMode::Apart);
    w.run_ms(1_000);
    let leads: Vec<Call<String>> = w.people[0]
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::Lead(_)))
        .collect();
    assert_eq!(leads.last(), Some(&Call::Lead(false)));
    assert_eq!(
        w.people[m].calls().first(),
        Some(&Call::Attach {
            mode: SyncMode::Apart
        })
    );
}

#[test]
fn b17_a_remote_changes_the_speakers_volume_only_when_the_host_lets_guests() {
    let mut w = World::open(songs(2), Opening::Resting);
    let r = w.join_as("Ravi", Way::LAN, Way::LAN, ListenMode::OneSpeaker);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    w.people[0].host().set_volume(Some(70));
    let volume_seen = |w: &mut World, i: usize| {
        w.people[i]
            .guest()
            .window(&Default::default())
            .room
            .and_then(|r| r.volume)
    };
    w.people[r].guest().ask(Action::Volume { percent: 30 });
    w.run_ms(300);
    assert_eq!(w.people[0].host().take_volume(), None, "not allowed yet");
    assert_eq!(volume_seen(&mut w, r), None);
    w.people[0]
        .host()
        .act(|room, now| room.set_guests_volume(true, now));
    w.run_ms(300);
    assert_eq!(volume_seen(&mut w, r), Some(70), "the speaker's volume");
    assert_eq!(volume_seen(&mut w, m), None, "for remotes only");
    w.people[r].guest().ask(Action::Volume { percent: 30 });
    w.run_ms(300);
    assert_eq!(
        w.people[0].host().take_volume(),
        Some(30),
        "the host's player told"
    );
    assert_eq!(volume_seen(&mut w, r), Some(30));
    w.people[m].guest().ask(Action::Volume { percent: 20 });
    w.run_ms(300);
    assert_eq!(
        w.people[0].host().take_volume(),
        None,
        "someone who plays their own copy can't"
    );
}

/// The songs whose words were asked for, and the words the host has, by song.
type Asked = Arc<Mutex<(Vec<String>, HashMap<String, Lyrics>)>>;

/// The host's words for its own songs, and how often they were asked for.
struct Words(Asked);

impl HostLyrics for Words {
    fn of(&mut self, song: &Song) -> Option<Option<Lyrics>> {
        let mut w = self.0.lock().unwrap();
        w.0.push(song.key.clone());
        Some(w.1.get(&song.key).cloned())
    }
}

#[test]
fn j7_the_words_of_a_song_only_the_host_has_reach_remotes_once() {
    let mine = host_song(1, 200);
    let words = Lyrics {
        lines: vec![(1_000, "First line".into()), (5_000, "Second line".into())],
        text: String::new(),
    };
    let asked: Asked = Arc::new(Mutex::new((
        Vec::new(),
        [(mine.key.clone(), words.clone())].into_iter().collect(),
    )));
    let mut w = World::open(vec![mine.clone(), song(2, 200)], Opening::Resting);
    w.people[0]
        .host()
        .set_lyrics(Box::new(Words(asked.clone())));
    let r = w.join_as("Ravi", Way::LAN, Way::LAN, ListenMode::OneSpeaker);
    let m = w.join("Meera", Way::LAN, Way::LAN);
    w.start_with_remotes();
    w.run_ms(3_000);
    let seen = |w: &mut World, i: usize| {
        w.people[i]
            .guest()
            .window(&Default::default())
            .room
            .and_then(|r| r.lyrics)
    };
    let l = seen(&mut w, r).expect("the remote has the words");
    assert_eq!((l.key, l.lines), (mine.key.clone(), words.lines));
    assert_eq!(
        seen(&mut w, m),
        None,
        "who plays their own copy finds their own"
    );
    assert_eq!(asked.lock().unwrap().0, vec![mine.key.clone()], "once");
    // A catalog song next: no words from the host (the window finds them).
    w.host_ask(Action::Next);
    w.run_ms(3_000);
    assert_eq!(seen(&mut w, r), None);
}

#[test]
fn j7_a_remotes_progress_follows_the_hosts_own_sound_card_within_100_ms_for_ten_minutes() {
    let mut w = World::open(vec![song(1, 700)], Opening::Resting);
    // The host's sound card runs 300 ppm fast: free running, 18 ms a minute ahead of a clock.
    let hc: Arc<dyn MonoClock> = w.people[0].clock.clone();
    w.people[0].log.heard_from(hc, 300.0);
    let r = w.join_as("Ravi", Way::LAN, Way::LAN, ListenMode::OneSpeaker);
    w.set_step_ms(10);
    w.host_ask(Action::Play);
    assert!(w.run_until(|w| w.room().phase() == Phase::Playing, 4_000));
    let start = w.people[0].commits()[0];
    let truth = |h: i64| start.content_us + ((h - start.host_us) as f64 * 1.000_3) as i64;
    let mut worst = 0i64;
    let mut anchors: Vec<Anchor> = Vec::new();
    for _ in 0..6_000 {
        w.run_ms(100);
        let h = w.now();
        if h < start.host_us + S {
            continue;
        }
        let v = w.people[r].guest().window(&Default::default());
        let a = v.room.unwrap().anchor.unwrap();
        let progress = a.content_at(v.host_now_us.unwrap());
        worst = worst.max((progress - truth(h)).abs());
        let ra = w.room().anchor().unwrap();
        if anchors.last() != Some(&ra) {
            anchors.push(ra);
        }
    }
    eprintln!(
        "ten minutes: the remote's progress at most {} ms from the host's heard place, {} re-anchors",
        worst / MS,
        anchors.len() - 1
    );
    assert!(worst < 100 * MS, "{} µs", worst);
    assert!(
        anchors.len() >= 9,
        "re-anchored as the card drifts: {}",
        anchors.len()
    );
    for p in anchors.windows(2) {
        let moved = p[1].content_us - p[0].content_at(p[1].host_us);
        assert!(moved > 20 * MS && moved < 25 * MS, "moved {moved} µs");
    }
    // The host's own player: never told anything but its first start (commits are no-ops for
    // the entry it plays) and to lead.
    let host_calls: Vec<Call<String>> = w.people[0]
        .calls()
        .into_iter()
        .filter(|c| !matches!(c, Call::Commit(_)))
        .collect();
    assert!(
        host_calls.iter().all(|c| matches!(
            c,
            Call::Attach { .. } | Call::Lead(true) | Call::Prepare { .. }
        )),
        "{host_calls:?}"
    );
}

// --- The window ---------------------------------------------------------------------------

#[test]
fn the_window_reads_lines_for_you_and_for_others_and_the_press_under_way() {
    let mut w = World::lan(&["Ravi", "Meera"]);
    w.start();
    w.run_ms(3_000);
    w.people[2]
        .rules
        .lock()
        .unwrap()
        .none
        .insert(song(2, 200).key);
    w.people[2]
        .log
        .script(|s| s.unable = [EntryId(2)].into_iter().collect());
    w.host_ask(Action::Next);
    w.run_ms(4_000);
    let mv = w.people[2].guest().window(&Default::default());
    let me = mv.me.unwrap();
    let mine = mv
        .room
        .as_ref()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id == me);
    assert_eq!(
        mine.unwrap().line,
        "Not available for you, back with the next song"
    );
    let rv = w.people[1].guest().window(&Default::default());
    let meera = rv
        .room
        .as_ref()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id == me);
    assert_eq!(
        meera.unwrap().line,
        "Can't play this one, back with the next song"
    );
    w.people[1].guest().ask(Action::Pause);
    let v = w.people[1].guest().window(&Default::default());
    assert_eq!(
        v.you.as_ref().and_then(|y| y.pressing.clone()).as_deref(),
        Some("Pausing...")
    );
    assert!(v.host_now_us.is_some());
    w.run_ms(1_000);
    let v = w.people[1].guest().window(&Default::default());
    assert_eq!(
        v.you.and_then(|y| y.pressing),
        None,
        "until the room has it"
    );
    let _ = Anchor {
        entry: EntryId(0),
        content_us: 0,
        host_us: 0,
        playing: false,
        seq: 0,
    };
    let _ = ToGuest::Waiting;
}

#[test]
fn the_hosts_files_go_only_to_people_in_the_room() {
    use together::sim_net::poll_once;
    use together::transport::{Get, MediaServer, PeerId};
    struct Shelf;
    impl MediaServer for Shelf {
        fn open(&self, _: PeerId, get: &Get) -> Option<std::sync::Arc<[u8]>> {
            Some(get.get.as_bytes().to_vec().into())
        }
    }
    let mut host_song = song(1, 200);
    host_song.key = "host:0123456789abcdef0123456789abcdef".into();
    host_song.online_id = None;
    host_song.host_file = true;
    let mut w = World::open(vec![host_song.clone(), song(2, 200)], Opening::Resting);
    let gate = w.people[0].host().media(std::sync::Arc::new(Shelf));
    let ht = w.host_t();
    w.net.serve(&ht, gate);
    let r = w.join("Ravi", Way::LAN, Way::LAN);
    w.run_ms(500);
    let open = |w: &mut World, i: usize, key: &str| {
        let mut f = w.people[i].t.open_media(
            w.peer(0),
            Get {
                get: key.into(),
                from: 0,
                to: None,
            },
        );
        for _ in 0..200 {
            if let std::task::Poll::Ready(r) = poll_once(&mut f) {
                return r.is_ok();
            }
            w.run_ms(5);
        }
        false
    };
    assert!(open(&mut w, r, &host_song.key), "a member gets a host song");
    assert!(
        open(&mut w, r, &format!("cover:{}", host_song.key)),
        "and its cover"
    );
    assert!(
        !open(&mut w, r, "host:fedcba9876543210fedcba9876543210"),
        "never a file not in the room"
    );
    w.people[0].host().set_let_in(false);
    let x = w.arrive("Outsider", Way::LAN, Way::LAN, ListenMode::Apart);
    assert!(w.run_until(|w| w.people[x].guest().stage() == Stage::AtTheDoor, 2_000));
    assert!(
        !open(&mut w, x, &host_song.key),
        "not to someone at the door"
    );
}
