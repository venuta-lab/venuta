//! Today's room tests (`room.rs`), ported to Room v2: the same cases, with entry ids, µs, and
//! the new rules where they differ (the music plays on while a seek or skip gets ready; the
//! next song is armed with its boundary instead of prepared). The time is the host's, passed
//! in, so these are simulated time already; the whole-room cases on the simulated network are
//! in `tests/room_v2.rs`.

use super::*;
use crate::invite::proof;
use crate::proto::v3::Phase;
use crate::room_player::Suspension;
use crate::transport::PathInfo;

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const S: i64 = 1_000_000;

fn song(n: u8) -> Song {
    Song {
        key: format!("online:song{n:07}"),
        title: format!("Song {n}"),
        artists: vec!["Someone".into()],
        album: None,
        duration_ms: Some(200_000),
        isrc: None,
        mbid: None,
        online_id: Some(format!("song{n:07}")),
        host_file: false,
        format: None,
        artwork_url: None,
        added_by: None,
    }
}

fn room() -> Room {
    Room::new(
        "Aakash",
        SECRET,
        vec![song(1), song(2), song(3)],
        Opening::Resting,
    )
}

fn join(r: &mut Room, key: &str, name: &str) -> u32 {
    match r.knock(key, name, &proof(SECRET, key), 0).0 {
        Knock::Waiting(id) => {
            r.admit(id, 0).unwrap();
            id
        }
        Knock::In(id) => id,
        Knock::Refused(w) => panic!("refused: {w}"),
    }
}

fn entry(r: &Room, i: usize) -> EntryId {
    r.queue()[i].id
}

/// The first prepare the host's own conductor is told of.
fn prep(c: &Changes) -> Option<(PrepId, EntryId, i64, Purpose)> {
    c.to(HOST).into_iter().find_map(|m| match m {
        ToGuest::Prepare {
            prep,
            entry,
            content_us,
            purpose,
        } => Some((*prep, *entry, *content_us, *purpose)),
        _ => None,
    })
}

fn commit(c: &Changes) -> Option<Anchor> {
    c.to(HOST).into_iter().find_map(|m| match m {
        ToGuest::Commit { anchor } => Some(*anchor),
        _ => None,
    })
}

fn hold(c: &Changes) -> Option<Anchor> {
    c.to(HOST).into_iter().find_map(|m| match m {
        ToGuest::Hold { anchor } => Some(*anchor),
        _ => None,
    })
}

fn arm(c: &Changes) -> Option<(PrepId, Next)> {
    c.to(HOST).into_iter().find_map(|m| match m {
        ToGuest::ArmNext { prep, next } => Some((*prep, *next)),
        _ => None,
    })
}

/// Start the room's first song with everyone ready at `at`; returns its anchor.
fn start(r: &mut Room, people: &[u32], at: i64) -> Anchor {
    let c = r.ask(HOST, Action::Play, at).unwrap();
    let (p, e, _, _) = prep(&c).expect("a prepare");
    let mut last = Changes::default();
    for id in people {
        last = r.ready(*id, p, e, at);
    }
    commit(&last).expect("started once all were ready")
}

fn view_member(r: &Room, id: u32) -> crate::proto::v3::Member {
    r.view_for(HOST)
        .members
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

#[test]
fn people_knock_wait_and_come_in() {
    let mut r = room();
    assert!(
        matches!(r.knock("k1", "Ravi", "00", 0).0, Knock::Refused(_)),
        "no proof, no knock"
    );
    let (k, c) = r.knock("k1", "Ravi", &proof(SECRET, "k1"), 0);
    let Knock::Waiting(id) = k else { panic!() };
    assert_eq!(c.to(id), vec![&ToGuest::Waiting]);
    assert_eq!(r.view_for(HOST).waiting, vec![(id, "Ravi".to_owned())]);
    let c = r.admit(id, 0).unwrap();
    assert!(
        matches!(c.to(id).first(), Some(ToGuest::Welcome { you, .. }) if *you == id),
        "their welcome first"
    );
    assert!(r.is_member(id));
    assert_eq!(
        r.role_of(id),
        Some(Role::Control),
        "full control by default (2 people)"
    );
    assert_eq!(
        r.view_for(id).waiting.len(),
        0,
        "nobody waits now; guests never see names"
    );
    // Back after a drop, at once and the same person.
    r.lost(id, S);
    let (k, c) = r.knock("k1", "Ravi", &proof(SECRET, "k1"), 2 * S);
    assert_eq!(k, Knock::In(id));
    assert!(matches!(c.to(id).first(), Some(ToGuest::Welcome { .. })));
}

#[test]
fn blocked_people_stay_out_and_rooms_fill_up() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let c = r.remove(ravi, true, 0);
    assert_eq!(c.gone, vec![ravi]);
    assert!(matches!(
        r.knock("k1", "Ravi", &proof(SECRET, "k1"), 0).0,
        Knock::Refused(_)
    ));
    for i in 2..crate::MAX_PEOPLE + 1 {
        join(&mut r, &format!("k{i}"), "Friend");
    }
    assert_eq!(r.member_ids().len(), crate::MAX_PEOPLE);
    assert!(matches!(
        r.knock("late", "Late", &proof(SECRET, "late"), 0).0,
        Knock::Refused(_)
    ));
}

#[test]
fn a_song_starts_for_everyone_once_all_are_ready() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let c = r.ask(HOST, Action::Play, 0).unwrap();
    let (p, e, content, purpose) = prep(&c).unwrap();
    assert_eq!((e, content, purpose), (entry(&r, 0), 0, Purpose::Start));
    assert_eq!(c.to(ravi).len(), c.to(HOST).len(), "everyone is told");
    assert!(r.anchor().is_none(), "not until ready");
    assert!(
        commit(&r.ready(HOST, p, e, 100_000)).is_none(),
        "still waiting for Ravi"
    );
    let a = commit(&r.ready(ravi, p, e, 300_000)).unwrap();
    assert!(a.playing);
    assert_eq!(
        a.host_us, 800_000,
        "half a second ahead of the last ready (Ravi's path not known to be one network)"
    );
    assert_eq!(r.phase(), Phase::Playing);
}

#[test]
fn play_while_a_song_starts_changes_nothing() {
    let mut r = room();
    let _ravi = join(&mut r, "k1", "Ravi");
    r.ask(HOST, Action::Play, 0).unwrap();
    r.tick(READY_WAIT_US);
    r.ask(HOST, Action::Next, 10 * S).unwrap();
    // Pressed again while the next song gets ready (the button looked like nothing happened):
    // it stays on its way, never back to the song before.
    let c = r.ask(HOST, Action::Play, 10 * S + 200_000).unwrap();
    assert!(prep(&c).is_none());
    assert_eq!(r.preparing().map(|p| p.1), Some(entry(&r, 1)));
}

#[test]
fn a_seek_while_a_song_starts_moves_its_start() {
    let mut r = room();
    start(&mut r, &[HOST], 0);
    r.ask(HOST, Action::Next, 5 * S).unwrap();
    let c = r
        .ask(HOST, Action::Seek { content_us: 30 * S }, 5 * S + 100_000)
        .unwrap();
    let (_, e, content, purpose) = prep(&c).unwrap();
    assert_eq!(
        (e, content, purpose),
        (entry(&r, 1), 30 * S, Purpose::Skip),
        "still the skip, starting further in"
    );
}

#[test]
fn a_seek_while_a_resume_gets_ready_moves_where_it_will_start() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let a = start(&mut r, &[HOST, ravi], 0);
    let at = a.host_us + 10 * S;
    let held = hold(&r.ask(HOST, Action::Pause, at).unwrap()).unwrap();
    assert!(!held.playing);
    assert!(
        (10 * S..10 * S + 400_000).contains(&held.content_us),
        "where it is a pause lead later: {}",
        held.content_us
    );
    // Play: ready where it paused.
    let c = r.ask(HOST, Action::Play, at + S).unwrap();
    let (_, _, content, purpose) = prep(&c).unwrap();
    assert_eq!((content, purpose), (held.content_us, Purpose::Resume));
    assert_eq!(
        r.anchor(),
        Some(held),
        "held exactly as it paused meanwhile"
    );
    // A seek before it starts: ready there instead.
    let c = r
        .ask(HOST, Action::Seek { content_us: 30 * S }, at + S + 100_000)
        .unwrap();
    let (p, e, content, purpose) = prep(&c).unwrap();
    assert_eq!((content, purpose), (30 * S, Purpose::Resume));
    r.ready(HOST, p, e, at + S + 200_000);
    let a = commit(&r.ready(ravi, p, e, at + S + 300_000)).unwrap();
    assert_eq!((a.content_us, a.playing), (30 * S, true));
}

#[test]
fn a_seek_while_the_next_song_gets_ready_leaves_the_last_one_playing() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let a = start(&mut r, &[HOST, ravi], 0);
    r.ask(HOST, Action::Next, 10 * S).unwrap();
    assert_eq!(
        r.anchor(),
        Some(a),
        "the song before plays on until the commit (D7)"
    );
    let c = r
        .ask(HOST, Action::Seek { content_us: 30 * S }, 10 * S + 200_000)
        .unwrap();
    assert_eq!(prep(&c).map(|p| (p.1, p.2)), Some((entry(&r, 1), 30 * S)));
    assert!(hold(&c).is_none() && commit(&c).is_none());
    assert_eq!(r.anchor(), Some(a));
}

#[test]
fn play_while_playing_leaves_the_rooms_anchor_alone() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let a = start(&mut r, &[HOST, ravi], 0);
    let c = r.ask(ravi, Action::Play, 6 * S).unwrap();
    assert!(c.is_empty());
    assert_eq!(r.anchor(), Some(a), "the same start, the same place");
}

#[test]
fn how_someone_plays_reaches_everyone_when_what_they_see_changes() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let report = |e: i64| Report {
        entry: None,
        sub_state: SubState::InStep,
        error_us: Some(e),
        bound_us: Some(1_000),
        copy: None,
    };
    let views = |c: &Changes| {
        c.out
            .iter()
            .filter(|o| matches!(o.msg, ToGuest::Room { .. }))
            .count()
    };
    // The first word of it: everyone's panel shows it.
    assert_eq!(views(&r.state(ravi, report(300), 0)), 2);
    // A few µs either way: the line ("In step, about ±2 ms") doesn't change.
    assert_eq!(views(&r.state(ravi, report(500), 0)), 0);
    // A millisecond more: the figure moves.
    assert_eq!(views(&r.state(ravi, report(2_500), 0)), 2);
    // Buffering: shown at once.
    let mut b = report(0);
    b.sub_state = SubState::Suspended(Suspension::Buffering);
    assert_eq!(views(&r.state(ravi, b, 0)), 2);
    // Whatever is said, the room keeps the latest.
    r.state(ravi, report(2_400), 0);
    assert_eq!(view_member(&r, ravi).error_us, Some(2_400));
}

#[test]
fn ready_still_counts_after_the_queue_changes_mid_start() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let (p, e, _, _) = prep(&r.ask(HOST, Action::Play, 0).unwrap()).unwrap();
    // Someone adds a song while it gets ready: the start isn't held up by it.
    r.ask(
        ravi,
        Action::Add {
            songs: vec![song(9)],
            next: false,
        },
        100_000,
    )
    .unwrap();
    r.ready(HOST, p, e, 200_000);
    assert!(
        commit(&r.ready(ravi, p, e, 300_000)).is_some(),
        "started at once"
    );
}

#[test]
fn a_slow_guest_holds_a_start_two_and_a_half_seconds_at_most() {
    assert_eq!(READY_WAIT_US, 2_500_000);
}

#[test]
fn one_slow_round_trip_doesnt_slow_every_start_after() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let path = |rtt| PathInfo {
        direct: true,
        relay: false,
        rtt_us: Some(rtt),
    };
    for i in 0..30 {
        r.path(ravi, path(40_000), i * S / 2);
    }
    r.path(ravi, path(900_000), 15 * S);
    let a = start(&mut r, &[HOST, ravi], 16 * S);
    assert_eq!(
        a.host_us - 16 * S,
        500_000,
        "one spike in 31: the p95 is 40 ms"
    );
    // Slow for real (many samples): starts are set further ahead.
    for i in 0..10 {
        r.path(ravi, path(400_000), 17 * S + i * S / 10);
    }
    let c = r.ask(HOST, Action::Next, 20 * S).unwrap();
    let (p, e, _, _) = prep(&c).unwrap();
    r.ready(HOST, p, e, 20 * S);
    let a = commit(&r.ready(ravi, p, e, 20 * S)).unwrap();
    assert_eq!(a.host_us - 20 * S, 850_000, "2 x 400 ms + 50 ms");
}

#[test]
fn the_room_says_a_song_is_starting_and_who_it_waits_for() {
    let mut r = room();
    let _ravi = join(&mut r, "k1", "Ravi");
    let (p, e, _, _) = prep(&r.ask(HOST, Action::Play, 0).unwrap()).unwrap();
    r.ready(HOST, p, e, 0);
    assert_eq!(r.view_for(HOST).starting, Some(vec!["Ravi".to_owned()]));
    r.tick(READY_WAIT_US);
    assert_eq!(r.view_for(HOST).starting, None, "playing");
}

#[test]
fn a_slow_guest_is_not_waited_for_forever() {
    let mut r = room();
    let _ravi = join(&mut r, "k1", "Ravi");
    let (p, e, _, _) = prep(&r.ask(HOST, Action::Play, 0).unwrap()).unwrap();
    r.ready(HOST, p, e, 0);
    assert!(commit(&r.tick(READY_WAIT_US - 1)).is_none());
    assert!(r.anchor().is_none());
    assert!(
        commit(&r.tick(READY_WAIT_US)).is_some(),
        "it goes on; Ravi catches up"
    );
}

#[test]
fn someone_who_cant_play_it_isnt_waited_for() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let (p, e, _, _) = prep(&r.ask(HOST, Action::Play, 0).unwrap()).unwrap();
    r.ready(HOST, p, e, 0);
    assert!(commit(&r.unable(ravi, p, e, 10)).is_some());
}

#[test]
fn roles_decide_what_guests_may_do() {
    let mut r = room();
    let meera = join(&mut r, "k1", "Meera");
    r.set_role(meera, Some(Role::Add), 0);
    assert!(r.ask(meera, Action::Next, 0).is_err());
    let add = |n| Action::Add {
        songs: vec![song(n)],
        next: false,
    };
    assert!(r.ask(meera, add(9), 0).is_ok());
    r.set_role(meera, Some(Role::Listen), 0);
    assert!(r.ask(meera, add(8), 0).is_err());
    assert!(r.ask(99, Action::Play, 0).is_err(), "not in the room");
    r.set_new_role(Some(Role::Listen), 0);
    let sam = join(&mut r, "k2", "Sam");
    assert_eq!(r.role_of(sam), Some(Role::Listen));
    assert!(!r.view_for(HOST).settings.auto_roles);
}

#[test]
fn everyone_sees_who_did_what_and_the_host_can_undo_it() {
    let mut r = room();
    let meera = join(&mut r, "k1", "Meera");
    let a = start(&mut r, &[HOST, meera], 0);
    assert_eq!(a.entry, entry(&r, 0));
    let c = r.ask(meera, Action::Next, 10 * S).unwrap();
    let (p, e, _, _) = prep(&c).unwrap();
    r.ready(HOST, p, e, 10 * S);
    r.ready(meera, p, e, 10 * S);
    let last = r.view_for(meera).activity.last().cloned().unwrap();
    assert_eq!(
        (last.who, last.what.as_str(), last.undoable),
        (meera, "skipped", true)
    );
    assert!(r.undo(meera, 11 * S).is_err(), "only the host undoes");
    let c = r.undo(HOST, 11 * S).unwrap();
    let (_, e, content, _) = prep(&c).unwrap();
    assert_eq!(e, entry(&r, 0), "back to the song before");
    assert!(
        (a.content_at(11 * S) - 500_000..=a.content_at(11 * S)).contains(&content),
        "where it would be now: {content}"
    );
}

#[test]
fn added_songs_carry_who_added_them_and_keep_the_playing_one() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    let two = entry(&r, 1);
    let c = r.ask(HOST, Action::Jump { entry: two }, 0).unwrap();
    let (p, e, _, purpose) = prep(&c).unwrap();
    assert_eq!(purpose, Purpose::Start, "from rest, a jump is a start");
    r.ready(HOST, p, e, 0);
    r.ready(ravi, p, e, 0);
    r.ask(
        ravi,
        Action::Add {
            songs: vec![song(7), song(8)],
            next: true,
        },
        1_000,
    )
    .unwrap();
    let v = r.view_for(ravi);
    assert_eq!(
        v.queue[2].song.title, "Song 7",
        "next: right after the playing one"
    );
    assert_eq!(v.queue[2].song.added_by, Some(ravi));
    assert_eq!(v.anchor.unwrap().entry, two, "still playing Song 2");
    assert_eq!(v.activity.last().unwrap().what, "added 2 songs");
    // Taking out a song before the playing one changes nothing for it.
    let one = entry(&r, 0);
    r.ask(HOST, Action::Remove { entry: one }, 2_000).unwrap();
    assert_eq!(r.anchor().unwrap().entry, two);
    assert!(r.ask(HOST, Action::Remove { entry: two }, 3_000).is_err());
    // Moving the playing song keeps it playing.
    let a = r.anchor();
    r.ask(HOST, Action::Move { entry: two, to: 3 }, 4_000)
        .unwrap();
    assert_eq!(r.anchor(), a);
    assert_eq!(r.queue()[3].id, two);
}

#[test]
fn songs_follow_each_other_on_the_rooms_clock() {
    let mut r = room();
    let a = start(&mut r, &[HOST], 0);
    let ends = a.host_us + 200 * S;
    assert!(arm(&r.tick(ends - 20 * S - 1)).is_none());
    let (_, n) = arm(&r.tick(ends - 20 * S)).expect("armed 20 s ahead");
    assert_eq!(
        (n.entry, n.boundary_host_us, n.transition),
        (entry(&r, 1), ends, Transition::Gapless)
    );
    assert_eq!(r.anchor(), Some(a), "this one plays on meanwhile");
    assert!(commit(&r.tick(ends - 1)).is_none());
    let next = commit(&r.tick(ends)).expect("committed at the boundary");
    assert_eq!(
        (next.entry, next.host_us, next.content_us, next.playing),
        (entry(&r, 1), ends, 0, true),
        "starts exactly at the end"
    );
}

#[test]
fn pausing_seeking_and_the_end_of_the_queue() {
    let mut r = Room::new("Aakash", SECRET, vec![song(1)], Opening::Resting);
    let a = start(&mut r, &[HOST], 0);
    let p = hold(&r.ask(HOST, Action::Pause, a.host_us + 10 * S).unwrap()).unwrap();
    assert!(!p.playing);
    let c = r
        .ask(
            HOST,
            Action::Seek { content_us: 50 * S },
            a.host_us + 11 * S,
        )
        .unwrap();
    let moved = hold(&c).expect("paused, it just moves");
    assert_eq!(moved.content_us, 50 * S);
    assert!(prep(&c).is_none());
    let c = r.ask(HOST, Action::Play, a.host_us + 12 * S).unwrap();
    let (pp, e, content, _) = prep(&c).unwrap();
    assert_eq!(content, 50 * S, "resuming gets everyone ready there");
    let a = commit(&r.ready(HOST, pp, e, a.host_us + 12 * S)).unwrap();
    let ends = a.host_us + 150 * S;
    let end = r.tick(ends - 100_000);
    assert_eq!(
        hold(&end).map(|h| (h.content_us, h.host_us)),
        Some((200 * S, ends)),
        "the end, a pause lead ahead"
    );
    r.tick(ends);
    assert_eq!(
        r.phase(),
        Phase::QueueEnded,
        "nothing after: the room rests"
    );
    assert!(r.anchor().is_none());
    assert!(r.ask(HOST, Action::Next, ends).unwrap().is_empty());
}

#[test]
fn a_goodbye_takes_them_out_at_once_and_back_is_the_same_person() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    // A dropped connection: away, still in the room, the same person when back.
    r.lost(ravi, S);
    assert!(r.is_member(ravi));
    assert!(view_member(&r, ravi).away);
    assert_eq!(
        r.knock("k1", "Ravi", &proof(SECRET, "k1"), 2 * S).0,
        Knock::In(ravi)
    );
    // A goodbye: out at once, and the room says so.
    let c = r.left(ravi, 5 * S);
    assert_eq!(c.gone, vec![ravi]);
    assert!(!r.is_member(ravi));
    assert_eq!(r.view_for(HOST).members.len(), 1);
    let said = r.view_for(HOST).activity.last().cloned().unwrap();
    assert_eq!((said.name.as_str(), said.what.as_str()), ("Ravi", "left"));
    // Back with the same invite: asks again (a new arrival, waiting for the host).
    assert!(matches!(
        r.knock("k1", "Ravi", &proof(SECRET, "k1"), 6 * S).0,
        Knock::Waiting(_)
    ));
}

#[test]
fn pausing_a_song_getting_ready_keeps_it_paused_there() {
    let mut r = room();
    let two = entry(&r, 1);
    r.ask(HOST, Action::Jump { entry: two }, 1_000).unwrap();
    let held =
        hold(&r.ask(HOST, Action::Pause, 2_000).unwrap()).expect("held where it was to start");
    assert_eq!((held.entry, held.content_us, held.playing), (two, 0, false));
    assert!(r.preparing().is_none());
    r.tick(3_000 + READY_WAIT_US * 2);
    assert_eq!(r.phase(), Phase::Held, "still paused after the wait");
    // Playing it gets everyone ready again, then starts.
    let c = r.ask(HOST, Action::Play, 50 * S).unwrap();
    assert_eq!(
        prep(&c).map(|p| (p.1, p.2, p.3)),
        Some((two, 0, Purpose::Resume))
    );
}

#[test]
fn pausing_near_the_end_pauses_this_song_not_the_next() {
    let mut r = room();
    let a = start(&mut r, &[HOST], 0);
    let ends = a.host_us + 200 * S;
    assert!(
        arm(&r.tick(ends - 20 * S)).is_some(),
        "the next is armed ahead"
    );
    let c = r.ask(HOST, Action::Pause, ends - 5 * S).unwrap();
    let p = hold(&c).unwrap();
    assert_eq!((p.entry, p.playing), (a.entry, false), "this song, paused");
    assert!(
        (195 * S..195 * S + 400_000).contains(&p.content_us),
        "where it was: {}",
        p.content_us
    );
    assert!(
        c.to(HOST)
            .iter()
            .any(|m| matches!(m, ToGuest::Disarm { .. })),
        "the armed next withdrawn"
    );
    r.tick(ends + READY_WAIT_US);
    assert_eq!(
        r.anchor().map(|a| a.entry),
        Some(a.entry),
        "the next never starts by itself"
    );
}

#[test]
fn someone_shown_away_for_silence_is_back_when_heard_again() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    r.heard(ravi, 0);
    r.tick(AWAY_AFTER_US - 1);
    assert!(!view_member(&r, ravi).away);
    r.tick(AWAY_AFTER_US);
    assert!(view_member(&r, ravi).away, "no ping for 5 s");
    let c = r.heard(ravi, AWAY_AFTER_US + 100_000);
    assert!(!c.is_empty(), "everyone sees them back");
    assert!(!view_member(&r, ravi).away);
}

#[test]
fn a_room_opened_while_the_host_plays_takes_its_place_untouched() {
    let r = Room::new(
        "Aakash",
        SECRET,
        vec![song(1), song(2)],
        Opening::Playing {
            index: 1,
            content_us: 42 * S,
            host_us: 1_000 * S,
        },
    );
    let a = r.anchor().unwrap();
    assert_eq!(
        (a.entry, a.content_us, a.host_us, a.playing),
        (r.queue()[1].id, 42 * S, 1_000 * S, true)
    );
    assert!(
        r.preparing().is_none(),
        "nothing prepared: the host's audio isn't touched"
    );
}

#[test]
fn an_entry_asked_about_again_asks_everyone_again() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    start(&mut r, &[HOST, ravi], 0);
    let two = entry(&r, 1);
    // Ravi couldn't play song 2 once (his stream failed): not waited for it then.
    let (p, e, _, _) = prep(&r.ask(HOST, Action::Next, 5 * S).unwrap()).unwrap();
    assert!(commit(&r.unable(ravi, p, e, 5 * S)).is_none());
    r.ready(HOST, p, e, 5 * S);
    // Later the room comes back to it: he's asked again, and waited for.
    r.ask(HOST, Action::Previous, 20 * S).unwrap();
    // (his pings keep coming)
    r.heard(ravi, 24 * S);
    r.tick(25 * S);
    r.heard(ravi, 29 * S);
    let c = r.ask(HOST, Action::Jump { entry: two }, 30 * S).unwrap();
    assert!(prep(&c).is_some());
    assert_eq!(
        r.view_for(HOST).starting,
        Some(vec!["Aakash".to_owned(), "Ravi".to_owned()])
    );
}

#[test]
fn every_view_the_room_makes_passes_a_guests_checks() {
    let long = "A host with a name far longer than forty letters allows";
    let mut r = Room::new(long, SECRET, vec![song(1)], Opening::Resting);
    let ravi = join(&mut r, "k1", "Ravi");
    let c = r.ask(HOST, Action::Play, 0).unwrap();
    for o in &c.out {
        assert!(
            crate::proto::v3::check_to_guest(&o.msg).is_ok(),
            "{:?}",
            o.msg
        );
    }
    assert!(r.view_for(ravi).check().is_ok());
}

#[test]
fn undo_keeps_a_pending_seek_and_never_takes_out_the_song_playing() {
    let mut r = room();
    let ravi = join(&mut r, "k1", "Ravi");
    start(&mut r, &[HOST, ravi], 0);
    // A seek getting ready (Ravi slow), then an addition, then undo: the seek stays on its way.
    r.ask(HOST, Action::Seek { content_us: 60 * S }, 5 * S)
        .unwrap();
    r.ask(
        HOST,
        Action::Add {
            songs: vec![song(9)],
            next: false,
        },
        5 * S + 100_000,
    )
    .unwrap();
    r.undo(HOST, 5 * S + 200_000).unwrap();
    assert_eq!(r.queue().len(), 3);
    assert_eq!(
        r.preparing().map(|p| p.2),
        Some(60 * S),
        "the seek is still on its way"
    );
    // A room that started with an added song: undoing the addition would take out the song
    // playing, so it isn't done.
    let mut r = Room::new("Aakash", SECRET, vec![], Opening::Resting);
    let c = r
        .ask(
            HOST,
            Action::Add {
                songs: vec![song(1)],
                next: false,
            },
            0,
        )
        .unwrap();
    let (p, e, _, _) = prep(&c).unwrap();
    commit(&r.ready(HOST, p, e, 0)).unwrap();
    assert_eq!(
        r.undo(HOST, S).unwrap_err(),
        "The song playing can't be taken out; skip it instead."
    );
    assert_eq!(r.queue().len(), 1);
    assert_eq!(r.anchor().map(|a| a.entry), Some(e));
}

#[test]
fn a_knocker_whose_connection_drops_leaves_the_door() {
    let mut r = room();
    let (k, _) = r.knock("k1", "Ravi", &proof(SECRET, "k1"), 0);
    let Knock::Waiting(id) = k else { panic!() };
    let c = r.lost(id, S);
    assert_eq!(c.gone, vec![id]);
    assert!(r.view_for(HOST).waiting.is_empty());
}

#[test]
fn the_host_is_never_a_remote_in_its_own_room() {
    let mut r = room();
    r.mode(HOST, ListenMode::OneSpeaker, 0);
    assert_eq!(r.mode_of(HOST), Some(ListenMode::Apart));
}
