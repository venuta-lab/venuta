//! WP15's gates for the life cycle (design 4.15, 6.1 I6, I7 and I10, the WP15 notes in 9.2):
//! whole rooms of sessions on the simulated network in simulated time (WP9's harness), with
//! the records kept in a folder of their own as the app keeps them.
//!
//! - I6: the host's app restarts; the room comes back held at its last place, the guests still
//!   reconnecting are in again as the same members, and the host's new player plays nothing
//!   until the room starts. After the guests' 2 minutes, they're offered to rejoin, and come
//!   back as themselves.
//! - I7: a guest's app crashes; under 2 minutes it rejoins by itself, after that it's asked;
//!   either way it is the same member, with its role.
//! - The records: what each ending keeps or removes.
//! - I10: every ending leaves the room's queue, in order, for the playlist.
//! - Rooms nearby (A15, D19): a person nearby always waits at the door.

mod session_sim;

use session_sim::*;
use together::anchor::Anchor;
use together::fakes::Call;
use together::invite::Invite;
use together::lifecycle::{GuestKeeper, HostKeeper, HostRecord, KEPT, Kept, Offer, offer};
use together::proto::Role;
use together::proto::v3::{Action, Phase};
use together::room::v2::{HOST, Place, RoomState};
use together::room_player::SyncMode;
use together::session::{Ending, Stage};
use together::view::Resume;

fn invite(w: &World) -> Invite {
    Invite {
        to: "endpoint-ticket".into(),
        secret: SECRET.into(),
        until: w.unix() + 24 * 3600,
        host: "Asha".into(),
    }
}

fn now_ms(w: &World) -> u64 {
    w.unix() * 1_000 + (w.sim_ns() / 1_000_000) % 1_000
}

/// The host's keeper, as the app makes it when the room opens.
fn host_keeper(w: &mut World, kept: Kept) -> HostKeeper {
    let room = w.people[0].host().state();
    let record = HostRecord {
        kept: KEPT,
        invite: invite(w),
        key: "11".repeat(32),
        port: Some(47_000),
        room,
        saved_at: w.unix(),
        app: serde_json::json!({ "root": "D:/Music" }),
    };
    HostKeeper::new(kept, record)
}

/// The world runs `ms`, the keepers stepped as the app's runners step them.
fn run_keeping(
    w: &mut World,
    host: Option<&mut HostKeeper>,
    guests: &mut [(usize, &mut GuestKeeper)],
    ms: u64,
) {
    let mut host = host;
    let until = w.sim_ns() + ms * NS_MS;
    while w.sim_ns() < until {
        w.run_ms(100);
        let now = now_ms(w);
        if let Some(h) = host.as_deref_mut()
            && w.people[0].side.is_some()
        {
            let hs = w.people[0].host();
            if hs.ended() {
                h.ended();
            } else {
                let playing = hs.room().anchor().is_some_and(|a| a.playing);
                h.step(hs.state(), playing, now);
            }
        }
        for (i, k) in guests.iter_mut() {
            if w.people[*i].side.is_none() {
                continue;
            }
            let g = w.people[*i].guest();
            k.step(g.stage(), g.ending(), now);
        }
    }
}

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a folder for the records")
}

fn place_of(s: &RoomState) -> (u64, i64) {
    match s.place {
        Place::At { entry, content_us } => (entry.0, content_us),
        ref p => panic!("not at a place: {p:?}"),
    }
}

// --- I6: the host restarts ---------------------------------------------------------------

#[test]
fn i6_a_host_restart_brings_the_room_back_held_and_its_guests_as_themselves() {
    let d = dir();
    let mut w = World::lan(&["Ravi", "Meera"]);
    let mut keeper = host_keeper(&mut w, Kept::new(d.path()));
    w.start();
    w.set_step_ms(10);
    run_keeping(&mut w, Some(&mut keeper), &mut [], 30_000);
    let ids = (w.member_id("Ravi").unwrap(), w.member_id("Meera").unwrap());
    let seq_before = w.room().anchor().unwrap().seq;
    let at_crash = place_of(&w.people[0].host().state());
    let t = w.sim_ns();
    w.host_goes();
    // The guests play on through it (rule 4): their players untouched, reconnecting.
    w.run_ms(20_000);
    for g in [1, 2] {
        assert_eq!(w.people[g].guest().stage(), Stage::Reconnecting);
        assert!(w.people[g].calls_since(t).is_empty(), "plays on, untouched");
    }
    // The app starts again: the record is there, reopen is offered (never by itself).
    let kept = Kept::new(d.path());
    let record = kept.host().expect("the room was kept");
    assert_eq!(
        offer(Some(&record), None, w.unix()),
        Some(Offer {
            resume: Resume::Reopen,
            auto: false
        })
    );
    let (entry, place) = place_of(&record.room);
    assert_eq!(entry, at_crash.0);
    assert!(
        (at_crash.1 - place).abs() <= 2 * S + 100 * MS,
        "the place is at most one write old: {} against {}",
        place,
        at_crash.1
    );
    let reopened = w.sim_ns();
    w.host_reopens(record.room.clone());
    // The host's new player: attached, the held place ready in silence, nothing committed.
    w.run_ms(1_000);
    let held = w.room().anchor().unwrap();
    assert!(!held.playing);
    assert_eq!((held.entry.0, held.content_us), (entry, place));
    assert!(held.seq > seq_before, "newer than anything a guest holds");
    assert_eq!(w.room().phase(), Phase::Held);
    let calls = w.people[0].calls();
    assert_eq!(
        calls.first(),
        Some(&Call::Attach {
            mode: SyncMode::Apart
        })
    );
    assert!(
        w.people[0]
            .prepares()
            .iter()
            .any(|p| p.1 == held.entry && p.2 == place),
        "the held place prepared: {calls:?}"
    );
    assert!(w.people[0].commits().is_empty(), "nothing plays");
    // The guests, still reconnecting, are in again as themselves, and held there.
    assert!(w.run_until(
        |w| {
            [1, 2]
                .iter()
                .all(|&g| w.people[g].guest().stage() == Stage::In)
        },
        10_000
    ));
    assert_eq!(
        (w.member_id("Ravi").unwrap(), w.member_id("Meera").unwrap()),
        ids
    );
    w.run_ms(2_000);
    for g in [1, 2] {
        let since = w.people[g].calls_since(reopened);
        assert!(
            since.contains(&Call::Hold(held)),
            "{} held at the kept place: {since:?}",
            w.people[g].name
        );
        assert!(
            !since.iter().any(|c| matches!(c, Call::Commit(_))),
            "nothing of the old room plays on"
        );
    }
    let activity = w.room().view_for(HOST).activity;
    assert!(
        activity.iter().any(|a| a.what == "reopened the room"),
        "{activity:?}"
    );
    // Five seconds on, still nothing for the host's player: the room starts only on a press.
    w.run_ms(5_000);
    assert!(w.people[0].commits().is_empty());
    let pressed = w.sim_ns();
    w.host_ask(Action::Play);
    assert!(w.run_until(
        |w| w.room().anchor().is_some_and(|a| a.playing)
            && (0..3).all(|i| {
                w.people[i]
                    .calls_since(pressed)
                    .iter()
                    .any(|c| matches!(c, Call::Commit(_)))
            }),
        4_000
    ));
    let a: Anchor = w.room().anchor().unwrap();
    assert_eq!((a.entry, a.content_us), (held.entry, held.content_us));
    for i in 0..3 {
        let c = w.people[i].calls_since(pressed);
        assert!(c.contains(&Call::Commit(a)), "{}: {c:?}", w.people[i].name);
    }
}

#[test]
fn i6_reopened_after_the_guests_gave_up_they_are_asked_and_come_back_as_themselves() {
    let d = dir();
    let mut w = World::lan(&["Ravi"]);
    w.set_step_ms(20);
    let mut host = host_keeper(&mut w, Kept::new(d.path().join("host")));
    let mut ravi = GuestKeeper::new(Kept::new(d.path().join("ravi")), invite(&w), "Asha".into());
    let rid = w.member_id("Ravi").unwrap();
    w.people[0]
        .host()
        .act(|room, now| room.set_role(rid, Some(Role::Add), now));
    w.start();
    run_keeping(&mut w, Some(&mut host), &mut [(1, &mut ravi)], 10_000);
    let id = w.member_id("Ravi").unwrap();
    w.host_goes();
    // Lost at the idle timeout, 2 minutes, then the song's end: Ravi's session ends.
    assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::Ended, 250_000));
    run_keeping(&mut w, None, &mut [(1, &mut ravi)], 200);
    assert_eq!(w.people[1].guest().ending(), Some(Ending::Lost));
    let rejoin = Kept::new(d.path().join("ravi"))
        .guest()
        .expect("kept: the host may come back");
    assert_eq!(
        offer(None, Some(&rejoin), w.unix()),
        Some(Offer {
            resume: Resume::Rejoin {
                host: "Asha".into()
            },
            auto: false
        }),
        "asked, not by itself: in the room over 2 minutes ago"
    );
    // The host reopens; nobody comes for 2 minutes, so the room lets Ravi go...
    let record = Kept::new(d.path().join("host")).host().unwrap();
    w.host_reopens(record.room);
    w.run_ms(130_000);
    assert!(!w.has_member("Ravi"), "dropped for silence");
    // ... and Ravi's rejoin still comes back as Ravi, with the role the host gave.
    w.guest_comes_back(1);
    assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::In, 5_000));
    assert_eq!(w.member_id("Ravi"), Some(id));
    assert_eq!(w.room().role_of(id), Some(Role::Add));
    let activity = w.room().view_for(HOST).activity;
    assert!(activity.iter().any(|a| a.what == "is back"), "{activity:?}");
}

// --- I7: a guest restarts ----------------------------------------------------------------

#[test]
fn i7_a_guest_crash_under_two_minutes_rejoins_by_itself_as_the_same_member() {
    let d = dir();
    let mut w = World::lan(&["Ravi", "Meera"]);
    let mut ravi = GuestKeeper::new(Kept::new(d.path()), invite(&w), "Asha".into());
    w.start();
    w.set_step_ms(10);
    run_keeping(&mut w, None, &mut [(1, &mut ravi)], 40_000);
    let id = w.member_id("Ravi").unwrap();
    let t = w.sim_ns();
    w.guest_goes(1);
    w.run_ms(30_000);
    let record = Kept::new(d.path()).guest().expect("kept through a crash");
    assert_eq!(
        offer(None, Some(&record), w.unix()),
        Some(Offer {
            resume: Resume::Rejoin {
                host: "Asha".into()
            },
            auto: true
        })
    );
    w.guest_comes_back(1);
    assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::In, 5_000));
    assert_eq!(w.member_id("Ravi"), Some(id), "the same member");
    // A late joiner: caught up at the room's place; nobody else hears a thing.
    assert!(w.run_until(|w| !w.people[1].commits().is_empty(), 3_000));
    let a = w.room().anchor().unwrap();
    assert!(a.playing);
    assert_eq!(w.people[1].prepares().last().unwrap().1, a.entry);
    for i in [0, 2] {
        assert!(
            w.people[i].calls_since(t).is_empty(),
            "{}",
            w.people[i].name
        );
    }
}

#[test]
fn i7_after_two_minutes_the_guest_is_asked_and_still_comes_back_as_themselves() {
    let d = dir();
    let mut w = World::lan(&["Ravi", "Meera"]);
    let mut ravi = GuestKeeper::new(Kept::new(d.path()), invite(&w), "Asha".into());
    let rid = w.member_id("Ravi").unwrap();
    w.people[0]
        .host()
        .act(|room, now| room.set_role(rid, Some(Role::Listen), now));
    w.start();
    w.set_step_ms(20);
    run_keeping(&mut w, None, &mut [(1, &mut ravi)], 10_000);
    w.guest_goes(1);
    w.run_ms(180_000);
    assert!(!w.has_member("Ravi"), "dropped 2 minutes after the loss");
    let record = Kept::new(d.path()).guest().unwrap();
    assert_eq!(
        offer(None, Some(&record), w.unix()).map(|o| o.auto),
        Some(false),
        "asked"
    );
    w.guest_comes_back(1);
    assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::In, 5_000));
    assert_eq!(w.member_id("Ravi"), Some(rid));
    assert_eq!(w.room().role_of(rid), Some(Role::Listen));
}

// --- What each ending keeps --------------------------------------------------------------

#[test]
fn a_guests_record_goes_with_a_leave_a_removal_or_the_end_and_stays_when_lost() {
    for how in ["leave", "removed", "ended", "lost"] {
        let d = dir();
        let mut w = World::lan(&["Ravi", "Meera"]);
        w.set_step_ms(20);
        let mut ravi = GuestKeeper::new(Kept::new(d.path()), invite(&w), "Asha".into());
        w.start();
        run_keeping(&mut w, None, &mut [(1, &mut ravi)], 3_000);
        assert!(
            Kept::new(d.path()).guest().is_some(),
            "{how}: written once in"
        );
        match how {
            "leave" => w.people[1].guest().leave(),
            "removed" => {
                let id = w.member_id("Ravi").unwrap();
                w.people[0].host().remove(id, false);
            }
            "ended" => w.people[0].host().end(),
            _ => w.cut(1),
        }
        let more = if how == "lost" { 250_000 } else { 2_000 };
        assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::Ended, more));
        run_keeping(&mut w, None, &mut [(1, &mut ravi)], 200);
        let want = match how {
            "leave" => Ending::Left,
            "removed" => Ending::Refused,
            "ended" => Ending::Ended,
            _ => Ending::Lost,
        };
        assert_eq!(w.people[1].guest().ending(), Some(want), "{how}");
        assert_eq!(
            Kept::new(d.path()).guest().is_some(),
            how == "lost",
            "{how}: kept only when the host went"
        );
    }
}

#[test]
fn the_hosts_record_follows_every_change_at_once_and_goes_when_the_room_ends() {
    let d = dir();
    let mut w = World::lan(&["Ravi"]);
    w.set_step_ms(10);
    let mut keeper = host_keeper(&mut w, Kept::new(d.path()));
    run_keeping(&mut w, Some(&mut keeper), &mut [], 300);
    let kept = Kept::new(d.path());
    assert_eq!(kept.host().unwrap().room.queue.len(), 5);
    // A change is written at once (the next step), not with the place's every 2 s.
    w.host_ask(Action::Add {
        songs: vec![song(9, 100)],
        next: false,
    });
    run_keeping(&mut w, Some(&mut keeper), &mut [], 100);
    assert_eq!(kept.host().unwrap().room.queue.len(), 6);
    // The place while playing: at most 2 s old.
    w.start();
    run_keeping(&mut w, Some(&mut keeper), &mut [], 9_000);
    let live = place_of(&w.people[0].host().state()).1;
    let written = place_of(&kept.host().unwrap().room).1;
    assert!(
        live - written <= 2 * S + 100 * MS,
        "{live} against {written}"
    );
    w.people[0].host().end();
    run_keeping(&mut w, Some(&mut keeper), &mut [], 200);
    assert!(kept.host().is_none(), "an ended room isn't offered again");
}

// --- I10: the room's queue after every ending --------------------------------------------

#[test]
fn i10_every_ending_leaves_the_rooms_queue_in_order() {
    for how in ["leave", "removed", "ended", "lost"] {
        let mut w = World::lan(&["Ravi", "Meera"]);
        w.set_step_ms(20);
        w.host_ask(Action::Add {
            songs: vec![song(7, 60)],
            next: false,
        });
        w.start();
        w.run_ms(2_000);
        let room: Vec<String> = w
            .room()
            .queue()
            .iter()
            .map(|e| e.song.key.clone())
            .collect();
        assert_eq!(room.len(), 6);
        match how {
            "leave" => w.people[1].guest().leave(),
            "removed" => {
                let id = w.member_id("Ravi").unwrap();
                w.people[0].host().remove(id, false);
            }
            "ended" => w.people[0].host().end(),
            _ => w.cut(1),
        }
        let more = if how == "lost" { 250_000 } else { 2_000 };
        assert!(w.run_until(|w| w.people[1].guest().stage() == Stage::Ended, more));
        let g = w.people[1].guest();
        let left: Vec<String> = g
            .conductor()
            .queue()
            .iter()
            .map(|e| e.song.key.clone())
            .collect();
        assert_eq!(left, room, "{how}: the guest's");
        assert_eq!(g.last_queue(), Some(6), "{how}");
        if how == "ended" {
            let h = w.people[0].host();
            assert!(h.ended());
            let host: Vec<String> = h
                .room()
                .queue()
                .iter()
                .map(|e| e.song.key.clone())
                .collect();
            assert_eq!(host, room, "the host's");
        }
    }
}

// --- Rooms nearby: the door ---------------------------------------------------------------

#[test]
fn a_person_nearby_always_waits_at_the_door_and_comes_in_as_a_member() {
    let mut w = World::lan(&["Ravi"]);
    assert!(
        w.room().lets_in_without_asking(),
        "the harness lets invites in"
    );
    let dev = w.arrive_nearby("Dev");
    assert!(w.run_until(|w| w.people[dev].guest().stage() == Stage::AtTheDoor, 2_000));
    w.run_ms(1_000);
    assert_eq!(
        w.people[dev].guest().stage(),
        Stage::AtTheDoor,
        "never let in by itself"
    );
    let waiting = w.room().view_for(HOST).waiting;
    assert_eq!(waiting.len(), 1);
    w.people[0].host().admit(waiting[0].0);
    assert!(w.run_until(|w| w.people[dev].guest().stage() == Stage::In, 2_000));
    let id = w.member_id("Dev").unwrap();
    // The room hidden from the network: nobody new nearby, but Dev, a member, is known.
    w.people[0].host().set_visible(false);
    let eli = w.arrive_nearby("Eli");
    assert!(w.run_until(|w| w.people[eli].guest().stage() == Stage::Ended, 2_000));
    assert_eq!(
        w.people[eli].guest().why().as_deref(),
        Some("This room isn't open to people nearby.")
    );
    w.cut(dev);
    w.run_ms(20_000);
    w.restore(dev);
    assert!(w.run_until(|w| w.people[dev].guest().stage() == Stage::In, 5_000));
    assert_eq!(w.member_id("Dev"), Some(id));
}

#[test]
fn a_door_code_is_not_an_invite() {
    // The door code lets a person knock; an invite's proof can't be made from it.
    let door = together::invite::nearby_secret(SECRET);
    assert_eq!(door.len(), 32);
    assert_ne!(door, SECRET);
    assert!(!together::invite::proof_ok(
        SECRET,
        "peer1",
        &together::invite::proof(&door, "peer1")
    ));
}

// --- The kept room itself ----------------------------------------------------------------

#[test]
fn a_kept_room_goes_through_its_record_and_comes_back_held_with_its_people() {
    use together::room::v2::{Knock, Opening, REOPEN_MARGIN, Room};
    let mut r = Room::new(
        "Asha",
        SECRET,
        songs(3),
        Opening::Playing {
            index: 1,
            content_us: 190 * S,
            host_us: 1_000 * S,
        },
    );
    r.set_let_in_without_asking(true);
    let key = |n: &str| format!("key-{n}");
    let knock = |r: &mut Room, n: &str| {
        let proof = together::invite::proof(SECRET, &key(n));
        r.knock(&key(n), n, &proof, 1_000 * S).0
    };
    let Knock::In(ravi) = knock(&mut r, "Ravi") else {
        panic!("Ravi in")
    };
    let Knock::In(ali) = knock(&mut r, "Ali") else {
        panic!("Ali in")
    };
    r.set_role(ravi, Some(Role::Listen), 1_000 * S);
    r.remove(ali, true, 1_000 * S);
    r.set_let_in_without_asking(false);
    let _ = knock(&mut r, "Dev");
    assert_eq!(r.view_for(HOST).waiting.len(), 1);
    // 30 s on, past the 200 s song's end: the place kept is under its length.
    let state = r.to_keep(1_030 * S);
    let back: RoomState = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(back, state);
    let entry = r.queue()[1].id;
    assert_eq!(
        state.place,
        Place::At {
            entry,
            content_us: 200 * S
        }
    );
    let mut o = Room::reopen(back, 5_000 * S);
    let a = o.anchor().unwrap();
    assert!(!a.playing);
    assert_eq!(
        (a.entry, a.content_us, a.host_us),
        (entry, 200 * S, 5_000 * S)
    );
    assert!(a.seq > r.anchor().unwrap().seq + REOPEN_MARGIN - 1);
    assert_eq!(o.queue(), r.queue());
    let v = o.view_for(HOST);
    assert!(v.waiting.is_empty(), "the door starts empty");
    let m = v.members.iter().find(|m| m.id == ravi).unwrap();
    assert!(m.away, "away until they reconnect");
    assert_eq!(m.role, Role::Listen);
    assert!(!o.lets_in_without_asking());
    // Ali stays out; Ravi is in at once as Ravi, even with the door asking.
    let proof = together::invite::proof(SECRET, &key("Ali"));
    assert!(matches!(
        o.knock(&key("Ali"), "Ali", &proof, 5_001 * S).0,
        Knock::Refused(_)
    ));
    assert_eq!(knock(&mut o, "Ravi"), Knock::In(ravi));
    // New people and entries get ids never given before.
    o.set_let_in_without_asking(true);
    let Knock::In(meera) = knock(&mut o, "Meera") else {
        panic!("Meera in")
    };
    assert!(meera > ali + REOPEN_MARGIN as u32 - 1);
}
