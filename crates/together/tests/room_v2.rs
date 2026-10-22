//! Room v2's gates (design 9.2, WP7): a test for every row of section 6.1's groups B (playback
//! control), C (the queue), D (from one song to the next) and G (buffering and stalls), room
//! side, plus the rules they rest on (who waits for whom, the away rule, roles, endings, the
//! door between protocol versions). Whole rooms on the simulated network in simulated time
//! (`SimClock` and `SimNet`), so they run in a moment and the same way on a busy machine.
//!
//! Rows with no room side say so where they are: B9 (the seek bar sends only the release: the
//! window's; a burst of seeks is B10's case), B15 (media keys: the app shell turns them into the
//! host's own asks, which every test here makes), D6 (your own crossfade is set aside: the
//! player's; the room's transition is one for everyone, D5), D7 (another sample rate: the
//! output's and the renderer's; nothing changes in the room's boundary).

mod room_sim;

use room_sim::*;
use together::anchor::{Purpose, Transition};
use together::proto::Song;
use together::proto::v3::{Action, CopyReport, Phase, ToGuest, ToHost, UnableWhy};
use together::align::CopyClass;
use together::room::v2::{HOST, HostNote, Opening};
use together::room_player::{ListenMode, SubState, Suspension};
use together::sim_net::Way;
use together::view::CopyKind;

fn two() -> Sim {
    Sim::lan(songs(5), Opening::Resting, &["Ravi", "Meera"])
}

/// A room of 30 s songs (their ends come quickly), playing from the first.
fn short_songs(n: u32) -> (Sim, together::anchor::Anchor) {
    let list = (1..=n).map(|i| song(i, 30)).collect();
    let mut s = Sim::lan(list, Opening::Resting, &["Ravi", "Meera"]);
    let a = s.start();
    (s, a)
}

fn since_commits(s: &Sim, i: usize, t: i64) -> Vec<together::anchor::Anchor> {
    s.commits(i)
        .into_iter()
        .filter(|(at, _)| *at >= t)
        .map(|(_, a)| a)
        .collect()
}

// --- B. Playback control ----------------------------------------------------------------

#[test]
fn b1_play_waits_for_everyone_then_starts_them_on_one_moment() {
    let mut s = two();
    s.people[2].player.ready_ms = Some(800);
    let t0 = s.now();
    s.press(1, Action::Play);
    s.run_ms(500);
    let p = s.prepares(0);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].2, p[0].3, p[0].4), (s.entry(0), 0, Purpose::Start));
    for i in 1..3 {
        assert_eq!(s.prepares(i)[0].1, p[0].1, "the same prepare for everyone");
    }
    assert!(s.commits(0).is_empty(), "waiting for Meera");
    assert_eq!(
        s.room.view_for(HOST).starting,
        Some(vec!["Meera".to_owned()]),
        "Starting... waiting for Meera"
    );
    assert!(s.run_until(|s| !s.commits(0).is_empty(), 3_000));
    let (at_host, a) = s.commits(0)[0];
    assert_eq!(a.host_us - at_host, 300 * MS, "one Wi-Fi: committed 300 ms ahead");
    assert!(a.host_us - t0 >= 800 * MS + 300 * MS, "after Meera was ready");
    assert_eq!((a.content_us, a.playing), (0, true));
    s.run_ms(400);
    for i in 0..3 {
        let c = s.commits(i);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].1, a, "one anchor for everyone");
        assert!(c[0].0 < a.host_us, "everyone has it before the moment");
    }
}

#[test]
fn b2_a_pause_is_scheduled_so_everyone_falls_silent_from_one_moment() {
    let mut s = two();
    let a = s.start();
    s.run_ms(10_000);
    s.press(1, Action::Pause);
    assert!(s.run_until(|s| !s.holds(0).is_empty(), 1_000));
    let (at_host, h) = s.holds(0)[0];
    assert_eq!(s.room.phase(), Phase::Held, "held from the press");
    assert_eq!(h.host_us - at_host, 150 * MS, "one Wi-Fi: 150 ms ahead");
    assert_eq!(h.content_us, a.content_at(h.host_us), "where the room is then");
    assert!(!h.playing);
    s.run_ms(300);
    for i in 1..3 {
        let g = s.holds(i);
        assert_eq!(g[0].1, h);
        assert!(g[0].0 < h.host_us, "before the moment, so the fades start together");
    }
    assert!(s.activity().iter().any(|l| l == "Ravi paused"));

    // Over the internet the lead is twice the round trip (here 240 ms), 400 ms at most.
    let mut s = Sim::new(
        songs(3),
        Opening::Resting,
        &[("Ali", Way::internet(60.0, 0.0), Way::internet(60.0, 0.0))],
    );
    s.start();
    s.run_ms(2_000);
    s.press(0, Action::Pause);
    let (at_host, h) = s.holds(0)[0];
    assert_eq!(h.host_us - at_host, 240 * MS);
}

#[test]
fn b3_resume_gets_everyone_ready_at_the_held_place_and_starts_together() {
    let mut s = two();
    s.start();
    s.run_ms(10_000);
    s.press(0, Action::Pause);
    let h = s.holds(0)[0].1;
    s.run_ms(2_000);
    let t = s.now();
    s.press(2, Action::Play);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let p = s.prepares(0).last().copied().unwrap();
    assert_eq!((p.2, p.3, p.4), (h.entry, h.content_us, Purpose::Resume));
    let (at_host, a) = *s.commits(0).last().unwrap();
    assert_eq!((a.entry, a.content_us, a.playing), (h.entry, h.content_us, true));
    assert_eq!(a.host_us - at_host, 300 * MS);
    assert!(a.host_us - t < 1_000 * MS, "heard within a second on one Wi-Fi");
}

#[test]
fn b4_a_seek_plays_on_until_the_commit_and_is_heard_within_a_second() {
    let mut s = two();
    let a = s.start();
    s.run_ms(30_000);
    let t = s.now();
    s.press(1, Action::Seek { content_us: 95 * S });
    s.run_ms(100);
    assert_eq!(s.room.anchor(), Some(a), "the music plays on meanwhile (D7)");
    assert!(s.holds(0).is_empty(), "never held for a seek");
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.3, p.4), (a.entry, 95 * S, Purpose::Seek));
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let c = s.commits(0)[1].1;
    assert_eq!((c.entry, c.content_us, c.playing), (a.entry, 95 * S, true));
    assert!(c.host_us - t <= 1_000 * MS, "{} ms", (c.host_us - t) / MS);

    // Over the internet: within 1.5 s.
    let mut s = Sim::new(
        songs(3),
        Opening::Resting,
        &[("Ali", Way::internet(40.0, 5.0), Way::internet(40.0, 5.0))],
    );
    s.start();
    s.run_ms(5_000);
    let t = s.now();
    s.press(1, Action::Seek { content_us: 60 * S });
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let c = s.commits(0)[1].1;
    assert!(c.host_us - t <= 1_500 * MS, "{} ms", (c.host_us - t) / MS);
}

#[test]
fn b5_a_seek_while_paused_moves_the_held_place_in_silence() {
    let mut s = two();
    s.start();
    s.run_ms(10_000);
    s.press(0, Action::Pause);
    let h = s.holds(0)[0].1;
    s.run_ms(1_000);
    let t = s.now();
    s.press(1, Action::Seek { content_us: 30 * S });
    s.run_ms(1_000);
    let moved = s.holds(0).last().unwrap().1;
    assert_eq!((moved.entry, moved.content_us, moved.playing), (h.entry, 30 * S, false));
    assert!(moved.seq > h.seq);
    assert!(s.prepares(0).len() == 1 && s.commits(0).len() == 1, "nothing starts");
    assert_eq!(s.room.phase(), Phase::Held);
    assert_eq!(s.playback_since(2, t).len(), 1, "only the moved hold");
}

#[test]
fn b6_next_plays_on_until_the_commit_then_switches() {
    let mut s = two();
    let a = s.start();
    s.run_ms(10_000);
    let t = s.now();
    s.press(2, Action::Next);
    s.run_ms(100);
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.3, p.4), (s.entry(1), 0, Purpose::Skip));
    assert_eq!(s.room.anchor(), Some(a), "song 1 plays on");
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let c = s.commits(0)[1].1;
    assert_eq!((c.entry, c.content_us), (s.entry(1), 0));
    assert!(c.host_us - t <= 1_000 * MS);
    assert!(s.activity().iter().any(|l| l == "Meera skipped"));
}

#[test]
fn b7_previous_restarts_late_in_a_song_and_goes_back_early_in_one() {
    let mut s = two();
    s.start();
    s.run_ms(10_000);
    s.press(1, Action::Previous);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.3, p.4), (s.entry(0), 0, Purpose::Seek), "the first song, from its start");
    s.press(1, Action::Next);
    assert!(s.run_until(|s| s.commits(0).len() == 3, 3_000));
    s.run_ms(1_000);
    s.press(1, Action::Previous);
    assert!(s.run_until(|s| s.commits(0).len() == 4, 3_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.3, p.4), (s.entry(0), 0, Purpose::Skip), "a second in: the song before");
    s.run_ms(1_000);
    s.press(1, Action::Previous);
    assert!(s.run_until(|s| s.commits(0).len() == 5, 3_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.3), (s.entry(0), 0), "the first song early: its start again");
}

#[test]
fn b8_a_jump_is_a_skip_to_the_entry_chosen() {
    let mut s = two();
    s.start();
    s.run_ms(5_000);
    let four = s.entry(3);
    s.press(2, Action::Jump { entry: four });
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.3, p.4), (four, 0, Purpose::Jump));
    assert_eq!(s.commits(0)[1].1.entry, four);
    s.press(2, Action::Jump {
        entry: together::anchor::EntryId(999),
    });
    s.run_ms(50);
    assert!(s.refused.iter().any(|(_, w)| w.contains("isn't in the queue")));
}

#[test]
fn b9_b10_presses_in_a_row_replace_each_other_and_one_switch_is_heard() {
    let mut s = two();
    for p in &mut s.people {
        p.player.ready_ms = Some(400);
    }
    s.start();
    s.run_ms(5_000);
    let t = s.now();
    for _ in 0..3 {
        s.press(1, Action::Next);
        s.run_ms(100);
    }
    assert!(s.run_until(|s| since_commits(s, 0, t).len() == 1, 4_000));
    s.run_ms(3_000);
    let p: Vec<_> = s.prepares(0).into_iter().filter(|p| p.0 >= t).collect();
    assert_eq!(
        p.iter().map(|p| p.2).collect::<Vec<_>>(),
        vec![s.entry(1), s.entry(2), s.entry(3)],
        "each skip from the one before"
    );
    let c = since_commits(&s, 0, t);
    assert_eq!(c.len(), 1, "one commit for three presses");
    assert_eq!(c[0].entry, s.entry(3));
    // A seek bar dragged and let go sends the release; a few seeks close together end in one.
    let t = s.now();
    for at in [40, 50, 60] {
        s.press(2, Action::Seek { content_us: at * S });
        s.run_ms(50);
    }
    assert!(s.run_until(|s| since_commits(s, 0, t).len() == 1, 4_000));
    s.run_ms(3_000);
    let c = since_commits(&s, 0, t);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].content_us, 60 * S);
}

#[test]
fn b11_two_presses_at_once_are_taken_in_order_and_the_later_wins() {
    // A skip, then a pause: paused at the start of the next song; both said.
    let mut s = two();
    s.start();
    s.run_ms(10_000);
    s.press(2, Action::Next);
    s.run_ms(2);
    s.press(1, Action::Pause);
    s.run_ms(500);
    let h = s.holds(0).last().unwrap().1;
    assert_eq!((h.entry, h.content_us, h.playing), (s.entry(1), 0, false));
    assert_eq!(s.room.phase(), Phase::Held);
    assert!(s.room.preparing().is_none());
    let said = s.activity();
    assert!(said.contains(&"Meera skipped".to_owned()) && said.contains(&"Ravi paused".to_owned()));
    // A pause, then a skip: the next song starts.
    let mut s = two();
    s.start();
    s.run_ms(10_000);
    s.press(1, Action::Pause);
    s.run_ms(2);
    s.press(2, Action::Next);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let c = s.commits(0)[1].1;
    assert_eq!((c.entry, c.content_us, c.playing), (s.entry(1), 0, true));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!(p.4, Purpose::Start, "from silence");
}

#[test]
fn b12_a_press_held_through_a_reconnect_is_taken_when_back() {
    let mut s = two();
    let a = s.start();
    s.run_ms(5_000);
    let ravi = s.id(1);
    s.cut(1);
    s.run_ms(16_000);
    assert!(s.member(ravi).away, "lost after the idle timeout");
    assert_eq!(s.room.anchor(), Some(a), "nobody paused for Ravi's drop");
    s.restore(1);
    s.run_ms(50);
    assert_eq!(s.id(1), ravi, "the same member, welcomed again");
    assert!(!s.member(ravi).away);
    // The press held while reconnecting (under 10 s old: the guest's rule), sent now.
    s.press(1, Action::Pause);
    assert!(s.run_until(|s| !s.holds(0).is_empty(), 1_000));
    assert!(s.activity().iter().any(|l| l == "Ravi paused"));
}

#[test]
fn b13_a_press_without_the_role_is_a_request_whoever_controls_accepts() {
    // Five people: guests add but don't control (D20).
    let mut s = Sim::lan(songs(4), Opening::Resting, &["Ravi", "Meera", "Ali", "Sam"]);
    s.start();
    s.run_ms(5_000);
    s.press(1, Action::Next);
    s.run_ms(50);
    assert_eq!(s.refused.last().map(|r| r.1.as_str()), Some("The host is in control."));
    s.say(1, ToHost::Request { action: Action::Next });
    s.run_ms(50);
    let r = s.room.view_for(s.id(2)).requests;
    assert_eq!(r.len(), 1, "everyone sees the request");
    assert_eq!((r[0].from, &r[0].action), (s.id(1), &Action::Next));
    // Someone without control can't accept it.
    s.press(2, Action::Accept { request: r[0].id });
    s.run_ms(50);
    assert_eq!(s.refused.last().map(|r| r.0), Some(s.id(2)));
    // The host accepts: done, as Ravi's.
    let n = s.commits(0).len();
    s.press(0, Action::Accept { request: r[0].id });
    assert!(s.run_until(|s| s.commits(0).len() == n + 1, 3_000));
    assert_eq!(s.commits(0).last().unwrap().1.entry, s.entry(1));
    assert!(s.activity().iter().any(|l| l == "Ravi skipped (Asha said yes)"));
    assert!(s.room.view_for(HOST).requests.is_empty());
    // "Ask the room to wait": a request; declined, it's gone and nothing happened.
    s.say(3, ToHost::Request { action: Action::Pause });
    s.run_ms(50);
    let id = s.room.view_for(HOST).requests[0].id;
    s.press(0, Action::Decline { request: id });
    s.run_ms(50);
    assert!(s.room.view_for(HOST).requests.is_empty());
    assert_eq!(s.room.phase(), Phase::Playing);
    // A request the room has moved past is out of date.
    s.say(1, ToHost::Request { action: Action::Next });
    s.run_ms(50);
    let id = s.room.view_for(HOST).requests[0].id;
    let n = s.commits(0).len();
    s.press(0, Action::Next);
    assert!(s.run_until(|s| s.commits(0).len() == n + 1, 3_000));
    s.press(0, Action::Accept { request: id });
    s.run_ms(50);
    assert_eq!(
        s.refused.last().map(|r| r.1.as_str()),
        Some("That request is out of date.")
    );
    assert!(s.room.view_for(HOST).requests.is_empty());
    // Adding is never a request: someone who may add just adds; someone who may not is told.
    s.room.set_role(s.id(4), Some(together::proto::Role::Listen), s.now());
    s.say(
        4,
        ToHost::Request {
            action: Action::Add {
                songs: vec![song(9, 100)],
                next: false,
            },
        },
    );
    s.run_ms(50);
    assert_eq!(
        s.refused.last().map(|r| r.1.as_str()),
        Some("The host hasn't let you add songs.")
    );
}

#[test]
fn b14_the_host_undoes_a_skip_back_to_where_the_song_would_be_now() {
    let mut s = two();
    let a = s.start();
    s.run_ms(10_000);
    s.press(1, Action::Next);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    s.run_ms(1_000);
    s.say(1, ToHost::Undo);
    s.run_ms(50);
    assert_eq!(s.refused.last().map(|r| r.1.as_str()), Some("Only the host can undo."));
    let t = s.now();
    s.say(0, ToHost::Undo);
    assert!(s.run_until(|s| s.commits(0).len() == 3, 3_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!(p.2, a.entry, "back to song 1");
    assert!((p.3 - a.content_at(t)).abs() < 50 * MS, "where it would be now");
    // Undoing an addition leaves the playing song alone.
    let n = s.prepares(0).len();
    s.press(0, Action::Add {
        songs: vec![song(9, 100)],
        next: false,
    });
    s.run_ms(50);
    assert_eq!(s.room.queue().len(), 6);
    s.say(0, ToHost::Undo);
    s.run_ms(500);
    assert_eq!(s.room.queue().len(), 5);
    assert_eq!(s.prepares(0).len(), n, "nothing to prepare");
}

#[test]
fn b15_the_hosts_own_presses_are_asks_like_anyone_elses() {
    // Media keys, the tray and the mini player go to the room as the host's asks.
    let mut s = two();
    s.start();
    s.run_ms(3_000);
    s.press(0, Action::Next);
    assert!(s.run_until(|s| s.commits(1).len() == 2, 3_000));
    assert_eq!(s.commits(1)[1].1.entry, s.entry(1));
    assert!(s.activity().iter().any(|l| l == "Asha skipped"));
}

#[test]
fn b16_pause_for_me_steps_out_alone_and_ask_to_wait_is_a_request() {
    let mut s = two();
    let a = s.start();
    s.run_ms(5_000);
    let t = s.now();
    s.report(1, SubState::SteppedOut);
    s.run_ms(5_000);
    assert_eq!(s.room.anchor(), Some(a), "the room goes on");
    assert!(s.playback_since(2, t).is_empty(), "nobody else hears a thing");
    // Not waited for at a prepare while stepped out.
    s.people[1].player.ready_ms = None;
    let t = s.now();
    s.press(2, Action::Seek { content_us: 60 * S });
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    assert!(s.commits(0)[1].0 - t < 1_000 * MS, "only the others were waited for");
    // "Ask the room to wait": a request, never a pause.
    s.say(1, ToHost::Request { action: Action::Pause });
    s.run_ms(50);
    assert_eq!(s.room.view_for(HOST).requests.len(), 1);
    assert_eq!(s.room.phase(), Phase::Playing);
}

#[test]
fn b17_only_a_remote_the_host_allows_changes_the_speakers_volume() {
    let mut s = two();
    s.set_mode(1, ListenMode::OneSpeaker);
    s.run_ms(50);
    s.press(1, Action::Volume { percent: 40 });
    s.run_ms(50);
    assert_eq!(s.volume, None);
    assert!(s.refused.last().is_some_and(|r| r.0 == s.id(1)));
    let now = s.now();
    let c = s.room.set_guests_volume(true, now);
    s.apply(c);
    s.press(1, Action::Volume { percent: 40 });
    s.run_ms(50);
    assert_eq!(s.volume, Some(40));
    s.press(2, Action::Volume { percent: 90 });
    s.run_ms(50);
    assert_eq!(s.volume, Some(40), "Meera plays on her own device: not hers to change");
    s.press(0, Action::Volume { percent: 70 });
    assert_eq!(s.volume, Some(70), "the host's own");
}

// --- C. The queue ------------------------------------------------------------------------

#[test]
fn c1_added_songs_get_new_ids_and_nothing_playing_is_touched() {
    let mut s = two();
    let a = s.start();
    s.run_ms(2_000);
    let t = s.now();
    s.press(1, Action::Add {
        songs: vec![song(7, 100), song(8, 100)],
        next: false,
    });
    s.run_ms(100);
    let q = s.room.queue();
    assert_eq!(q.len(), 7);
    assert_eq!(q[5].song.title, "Song 7");
    assert_eq!(q[5].song.added_by, Some(s.id(1)));
    let mut ids: Vec<_> = q.iter().map(|e| e.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), 7, "every entry its own id");
    s.press(2, Action::Add {
        songs: vec![song(9, 100)],
        next: true,
    });
    s.run_ms(100);
    assert_eq!(s.room.queue()[1].song.title, "Song 9", "right after the playing one");
    assert_eq!(s.room.anchor(), Some(a));
    assert!(s.playback_since(1, t).is_empty());
    let v = s.people[1].view.clone().unwrap();
    assert_eq!(v.queue.len(), 8, "everyone's view has them");
}

#[test]
fn c2_adding_to_an_empty_or_resting_room_starts_it() {
    let mut s = Sim::lan(vec![], Opening::Resting, &["Ravi"]);
    assert_eq!(s.room.phase(), Phase::Resting);
    s.press(1, Action::Add {
        songs: vec![song(1, 10)],
        next: false,
    });
    assert!(s.run_until(|s| !s.commits(0).is_empty(), 3_000));
    let p = s.prepares(0)[0];
    assert_eq!((p.2, p.4), (s.entry(0), Purpose::Start));
    // The queue ends; an addition starts again.
    assert!(s.run_until(|s| s.room.phase() == Phase::QueueEnded, 12_000));
    s.press(1, Action::Add {
        songs: vec![song(2, 10)],
        next: false,
    });
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    assert_eq!(s.commits(0)[1].1.entry, s.entry_of(2));
}

#[test]
fn c3_a_song_not_playing_can_be_taken_out_the_playing_one_cant() {
    let mut s = two();
    let a = s.start();
    let t = s.now();
    let three = s.entry(2);
    s.press(1, Action::Remove { entry: three });
    s.run_ms(50);
    assert_eq!(s.room.queue().len(), 4);
    assert!(s.activity().iter().any(|l| l == "Ravi took out “Song 3”"));
    s.press(1, Action::Remove { entry: a.entry });
    s.run_ms(50);
    assert_eq!(
        s.refused.last().map(|r| r.1.as_str()),
        Some("The song playing can't be taken out; skip it instead.")
    );
    // Nor the song a skip is getting ready.
    s.people[2].player.ready_ms = None;
    s.press(1, Action::Next);
    s.run_ms(100);
    let target = s.room.preparing().unwrap().1;
    s.press(1, Action::Remove { entry: target });
    s.run_ms(50);
    assert!(s.room.queue().iter().any(|e| e.id == target));
    assert!(s.playback_since(2, t).iter().all(|m| matches!(m, ToGuest::Prepare { .. })));
}

#[test]
fn c4_moving_songs_the_playing_one_too_keeps_it_playing() {
    let mut s = two();
    let a = s.start();
    let t = s.now();
    s.press(2, Action::Move {
        entry: a.entry,
        to: 4,
    });
    s.run_ms(100);
    assert_eq!(s.room.queue()[4].id, a.entry);
    assert_eq!(s.room.anchor(), Some(a));
    assert!(s.playback_since(1, t).is_empty(), "nothing heard");
    assert!(s.activity().iter().any(|l| l == "Meera moved a song"));
}

#[test]
fn c5_a_change_that_makes_another_song_next_arms_it_or_withdraws_the_arm() {
    let (mut s, a) = short_songs(3);
    s.run_to(a.host_us + 11 * S);
    let first = s.arms(1);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].2.entry, s.entry(1));
    let three = s.entry(2);
    s.press(1, Action::Move { entry: three, to: 1 });
    s.run_ms(100);
    let arms = s.arms(1);
    assert_eq!(arms.len(), 2);
    assert_eq!(arms[1].2.entry, three, "song 3 is next now");
    assert!(arms[1].1 != arms[0].1 && arms[1].2.seq > arms[0].2.seq, "a new prep and seq");
    assert_eq!(arms[1].2.boundary_host_us, arms[0].2.boundary_host_us);
    s.press(1, Action::Remove { entry: three });
    s.run_ms(100);
    assert_eq!(s.arms(1).last().unwrap().2.entry, s.entry(1));
    let two_id = s.entry(1);
    s.press(1, Action::Remove { entry: two_id });
    s.run_ms(100);
    assert_eq!(s.disarms(1), 1, "nothing follows now: withdrawn");
    assert!(s.room.next().is_none());
}

#[test]
fn c6_a_change_within_3_s_of_the_boundary_applies_to_the_one_after() {
    let (mut s, a) = short_songs(5);
    let end = a.host_us + 30 * S;
    s.run_to(end - 2 * S);
    let armed = s.arms(1);
    assert_eq!(armed.len(), 1);
    let two_id = s.entry(1);
    let five = s.entry(4);
    // Song 5 to play next, 2 s before the boundary: song 2 stays armed, song 5 comes after it.
    s.press(1, Action::Move { entry: five, to: 1 });
    s.run_ms(50);
    assert_eq!(s.arms(1).len(), 1, "the armed next stays for this boundary");
    let order: Vec<_> = s.room.queue().iter().map(|e| e.id).collect();
    assert_eq!(&order[..3], &[a.entry, two_id, five]);
    // The armed song can't be taken out or moved away now.
    s.press(2, Action::Remove { entry: two_id });
    s.run_ms(20);
    assert!(s.refused.last().is_some_and(|r| r.1.contains("about to start")));
    s.press(2, Action::Move { entry: two_id, to: 4 });
    s.run_ms(20);
    assert!(s.refused.last().is_some_and(|r| r.1.contains("about to start")));
    s.run_to(end + 100 * MS);
    assert_eq!(s.commits(0).last().unwrap().1.entry, two_id);
    s.run_to(end + 11 * S);
    assert_eq!(s.arms(1).last().unwrap().2.entry, five, "the change, for the one after");
}

#[test]
fn c7_the_end_of_the_queue_rests_the_room() {
    let (mut s, a) = short_songs(1);
    let end = a.host_us + 30 * S;
    s.run_to(end - 1 * S);
    assert!(s.holds(1).is_empty());
    s.run_to(end + 50 * MS);
    let (got, h) = s.holds(1)[0];
    assert!(got < end, "said ahead of the end");
    assert_eq!((h.entry, h.content_us, h.host_us, h.playing), (a.entry, 30 * S, end, false));
    assert_eq!(s.room.phase(), Phase::QueueEnded);
    assert_eq!(s.people[1].view.as_ref().unwrap().phase, Phase::QueueEnded);
    assert!(s.room.anchor().is_none());
    s.run_ms(10_000);
    assert_eq!(s.commits(1).len(), 1, "nothing more plays");
}

#[test]
fn c8_songs_added_near_the_end_follow_on() {
    // Added 5 s before the end (autoplay): armed at once, joined at the end.
    let (mut s, a) = short_songs(1);
    let end = a.host_us + 30 * S;
    s.run_to(end - 5 * S);
    s.press(0, Action::Add {
        songs: vec![song(2, 30)],
        next: false,
    });
    s.run_ms(50);
    let arm = s.arms(1).last().unwrap().2;
    assert_eq!((arm.entry, arm.boundary_host_us), (s.entry(1), end));
    s.run_to(end + 50 * MS);
    let c = s.commits(1).last().unwrap().1;
    assert_eq!((c.entry, c.host_us, c.content_us), (s.entry(1), end, 0));
    // Added after the end was said (a pause lead before it): the queue ends, then it starts.
    let (mut s, a) = short_songs(1);
    let end = a.host_us + 30 * S;
    s.run_to(end - 100 * MS);
    assert_eq!(s.holds(0).len(), 1);
    s.press(0, Action::Add {
        songs: vec![song(2, 30)],
        next: false,
    });
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.4), (s.entry(1), Purpose::Start));
    assert!(s.commits(0)[1].1.host_us > end);
}

// --- D. From one song to the next --------------------------------------------------------

#[test]
fn d1_the_next_song_is_armed_20_s_ahead_and_joins_at_the_boundary() {
    let (mut s, a) = short_songs(3);
    let end = a.host_us + 30 * S;
    s.run_to(end + 50 * MS);
    let arm0 = s.arms(0)[0];
    assert!(
        (end - 20 * S..end - 20 * S + 10 * MS).contains(&arm0.0),
        "armed 20 s before the boundary (on the step after)"
    );
    let n = arm0.2;
    assert_eq!((n.entry, n.boundary_host_us, n.transition), (s.entry(1), end, Transition::Gapless));
    for i in 1..3 {
        let g = s.arms(i)[0];
        assert_eq!(g.2, n, "the same for everyone");
        assert!(g.0 < end - 19 * S);
    }
    let (got, c) = *s.commits(0).last().unwrap();
    assert_eq!((c.entry, c.content_us, c.host_us), (n.entry, 0, end));
    assert!(got >= end, "no message is needed at the moment: the commit only confirms");
}

#[test]
fn d2_d3_d4_everyones_copy_follows_the_reference_boundary() {
    let (mut s, a) = short_songs(3);
    // Ravi's copy is 12 s longer, Meera's 5 s shorter: the boundary is the reference's.
    for (i, d) in [(1, 12), (2, -5)] {
        s.say(
            i,
            ToHost::State {
                entry: Some(a.entry),
                sub_state: SubState::InStep,
                error_us: Some(0),
                bound_us: Some(500),
                copy: Some(CopyReport {
                    kind: CopyKind::Catalog,
                    class: CopyClass::Approximate,
                    length_diff_s: Some(d),
                }),
            },
        );
    }
    let end = a.host_us + 30 * S;
    s.run_to(end + 50 * MS);
    assert_eq!(s.arms(1)[0].2.boundary_host_us, end);
    assert_eq!(s.commits(0).last().unwrap().1.host_us, end);
    let v = s.room.view_for(s.id(1));
    let me = v.members.iter().find(|m| m.id == s.id(1)).unwrap();
    assert_eq!(me.copy.unwrap().length_diff_s, Some(12), "Ravi sees his own copy");
    let other = v.members.iter().find(|m| m.id == s.id(2)).unwrap();
    assert_eq!(other.copy, None, "but not Meera's (D14)");
    let host_view = s.room.view_for(HOST);
    assert!(host_view.members.iter().all(|m| m.host || m.copy.is_some()), "the host sees all");
}

#[test]
fn d5_d6_the_rooms_crossfade_is_one_moment_for_all_and_never_within_an_album() {
    let mut list: Vec<_> = (1..=3).map(|i| song(i, 30)).collect();
    list[1].album = Some("Brahmastra".into());
    list[2].album = Some("Brahmastra".into());
    let mut s = Sim::lan(list, Opening::Resting, &["Ravi", "Meera"]);
    let now = s.now();
    assert!(s.room.set_transition(Transition::Crossfade { ms: 3_000 }, now).is_err());
    let c = s.room.set_transition(Transition::Crossfade { ms: 4_000 }, now).unwrap();
    s.apply(c);
    let a = s.start();
    let end1 = a.host_us + 30 * S;
    s.run_to(end1 - 4 * S + 50 * MS);
    let n = s.arms(0)[0].2;
    assert_eq!(n.transition, Transition::Crossfade { ms: 4_000 });
    assert_eq!(n.boundary_host_us, end1 - 4 * S, "the crossfade begins 4 s before the end");
    for i in 1..3 {
        assert_eq!(s.arms(i)[0].2, n, "one transition for everyone");
    }
    let c = s.commits(0).last().unwrap().1;
    assert_eq!((c.entry, c.host_us, c.content_us), (n.entry, n.boundary_host_us, 0));
    // Songs 2 and 3 are on one album: gapless between them.
    s.run_to(c.host_us + 11 * S);
    assert_eq!(s.arms(0).last().unwrap().2.transition, Transition::Gapless);
    assert_eq!(s.room.view_for(HOST).settings.transition, Transition::Crossfade { ms: 4_000 });
}

#[test]
fn d8_very_short_songs_are_armed_at_once_and_still_joined() {
    let list = vec![song(1, 15), song(2, 30)];
    let mut s = Sim::lan(list, Opening::Resting, &["Ravi"]);
    let a = s.start();
    let (armed_at, _, n) = s.arms(0)[0];
    assert_eq!(armed_at, s.commits(0)[0].0, "armed with the commit");
    assert_eq!(n.boundary_host_us, a.host_us + 15 * S);
    // Shorter than the lead: it still plays; the next is armed at once with its boundary.
    let list = vec![
        Song {
            duration_ms: Some(200),
            ..song(1, 1)
        },
        song(2, 30),
    ];
    let mut s = Sim::lan(list, Opening::Resting, &["Ravi"]);
    let a = s.start();
    let n = s.arms(0)[0].2;
    assert_eq!(n.boundary_host_us, a.host_us + 200 * MS);
    s.run_to(n.boundary_host_us + 50 * MS);
    assert_eq!(s.commits(1).last().unwrap().1.entry, s.entry(1));
}

#[test]
fn d9_d10_someone_not_ready_for_the_next_or_without_it_never_holds_the_boundary() {
    let (mut s, a) = short_songs(3);
    s.people[1].player.next_ready_ms = None;
    let two_key = s.room.queue()[1].song.key.clone();
    s.people[2].player.unable.insert(two_key);
    let end = a.host_us + 30 * S;
    s.run_to(end + 50 * MS);
    assert_eq!(s.commits(0).last().unwrap().1.host_us, end, "on time for those ready");
    // Meera, without song 2, is asked about song 3 when it's armed.
    s.run_to(end + 11 * S);
    let arm = s.arms(2).last().unwrap().2;
    assert_eq!(arm.entry, s.entry(2));
    s.run_ms(1_000);
    let meera = s.id(2);
    assert!(
        s.room.view_for(HOST).members.iter().any(|m| m.id == meera && !m.away),
        "still in the room"
    );
}

#[test]
fn d11_an_entry_nobody_can_play_is_skipped_after_5_s() {
    let (mut s, a) = short_songs(3);
    let two_key = s.room.queue()[1].song.key.clone();
    for p in &mut s.people {
        p.player.unable.insert(two_key.clone());
    }
    let end = a.host_us + 30 * S;
    s.run_to(end + 50 * MS);
    assert_eq!(s.commits(0).last().unwrap().1.entry, s.entry(1), "its boundary comes");
    s.run_to(end + 4_900 * MS);
    assert_eq!(s.prepares(0).len(), 1, "not yet");
    s.run_to(end + 5_100 * MS + 600 * MS);
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.2, p.4), (s.entry(2), Purpose::Skip));
    assert!(
        s.activity().iter().any(|l| l == "Asha skipped “Song 2”: no one could play it"),
        "{:?}",
        s.activity()
    );
    // One person able to play it: not skipped.
    let (mut s, a) = short_songs(3);
    let two_key = s.room.queue()[1].song.key.clone();
    s.people[0].player.unable.insert(two_key.clone());
    s.people[1].player.unable.insert(two_key);
    let end = a.host_us + 30 * S;
    s.run_to(end + 10 * S);
    assert_eq!(s.prepares(0).len(), 1);
    assert_eq!(s.room.anchor().unwrap().entry, s.entry(1));
}

// --- G. Buffering and stalls -------------------------------------------------------------

#[test]
fn g1_a_guest_stalling_mid_song_drops_out_alone() {
    let mut s = two();
    let a = s.start();
    s.run_ms(10_000);
    let t = s.now();
    s.report(1, SubState::Suspended(Suspension::Buffering));
    s.run_ms(5_000);
    assert_eq!(s.room.anchor(), Some(a), "the room goes on");
    assert!(s.playback_since(2, t).is_empty() && s.playback_since(1, t).is_empty());
    assert_eq!(
        s.member(s.id(1)).sub_state,
        Some(SubState::Suspended(Suspension::Buffering)),
        "shown as buffering"
    );
    // And isn't waited for at a prepare while suspended.
    s.people[1].player.ready_ms = None;
    let t = s.now();
    s.press(0, Action::Seek { content_us: 90 * S });
    assert!(s.run_until(|s| s.commits(0).len() == 2, 3_000));
    assert!(s.commits(0)[1].0 - t < 1_000 * MS);
}

#[test]
fn g2_the_hosts_own_stream_stalling_stops_nobody() {
    let mut s = two();
    let a = s.start();
    s.run_ms(10_000);
    let t = s.now();
    s.report(0, SubState::Suspended(Suspension::Buffering));
    s.run_ms(10_000);
    assert_eq!(s.room.anchor(), Some(a));
    assert!(s.playback_since(1, t).is_empty());
    assert_eq!(s.room.phase(), Phase::Playing);
}

#[test]
fn g3_when_everyone_stalls_for_3_s_the_room_holds_and_resumes_together() {
    let mut s = two();
    let a = s.start();
    s.run_ms(10_000);
    // One person stalling, however long: never held.
    s.report(1, SubState::Suspended(Suspension::Buffering));
    s.run_ms(10_000);
    assert_eq!(s.room.phase(), Phase::Playing);
    // Everyone, the host included.
    s.report(0, SubState::Suspended(Suspension::Buffering));
    s.report(2, SubState::Suspended(Suspension::Buffering));
    let t = s.now();
    s.run_ms(2_900);
    assert!(s.holds(0).is_empty());
    s.run_ms(200);
    let (got, h) = s.holds(0)[0];
    assert!(got - t >= 3 * S);
    assert_eq!(s.room.phase(), Phase::Held);
    assert!((h.content_us - a.content_at(got)).abs() <= 5 * MS, "held where the room was");
    // Someone has the music again: ready at the held place, everyone together.
    s.report(2, SubState::Ready);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 4_000));
    let p = *s.prepares(0).last().unwrap();
    assert_eq!((p.3, p.4), (h.content_us, Purpose::Resume));
    assert_eq!(s.commits(0)[1].1.content_us, h.content_us);
}

#[test]
fn g4_g5_a_slow_guest_is_waited_for_2_5_s_at_most() {
    // A start: Meera never ready; it goes after 2.5 s, she joins late alone.
    let mut s = two();
    s.people[2].player.ready_ms = None;
    s.press(0, Action::Play);
    s.run_ms(10);
    let prepared = s.prepares(0)[0].0;
    assert!(s.run_until(|s| !s.commits(0).is_empty(), 4_000));
    let (got, c) = s.commits(0)[0];
    assert!((got - prepared - 2_500 * MS).abs() <= 10 * MS, "{} ms", (got - prepared) / MS);
    assert_eq!(c.host_us - got, 300 * MS);
    // A seek past what's downloaded: waited for while it comes within the 2.5 s...
    s.people[2].player.ready_ms = Some(2_000);
    s.run_ms(5_000);
    s.press(1, Action::Seek { content_us: 120 * S });
    s.run_ms(10);
    let prepared = s.prepares(0).last().unwrap().0;
    assert!(s.run_until(|s| s.commits(0).len() == 2, 4_000));
    let got = s.commits(0)[1].0;
    assert!(got - prepared < 2_100 * MS && got - prepared >= 2_000 * MS);
    // ... and not past them.
    s.people[2].player.ready_ms = Some(4_000);
    s.run_ms(1_000);
    s.press(1, Action::Seek { content_us: 150 * S });
    s.run_ms(10);
    let prepared = s.prepares(0).last().unwrap().0;
    assert!(s.run_until(|s| s.commits(0).len() == 3, 4_000));
    let got = s.commits(0)[2].0;
    assert!((got - prepared - 2_500 * MS).abs() <= 10 * MS);
}

#[test]
fn g6_a_stream_that_never_returns_is_moved_on_at_the_next_song() {
    let mut s = two();
    let one_key = s.room.queue()[0].song.key.clone();
    s.people[1].player.unable.insert(one_key);
    s.people[1].player.ready_ms = Some(1_500);
    let t = s.now();
    s.start();
    assert!(s.commits(0)[0].0 - t < 1_000 * MS, "not waited for on song 1");
    let ravi = s.id(1);
    s.say(
        1,
        ToHost::Unable {
            prep: s.prepares(1)[0].1,
            entry: s.entry(0),
            why: UnableWhy::Failed,
        },
    );
    s.run_ms(5_000);
    let t = s.now();
    s.press(0, Action::Next);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 4_000));
    assert!(
        s.commits(0)[1].0 - t >= 1_500 * MS,
        "waited for again on song 2"
    );
    assert!(s.room.is_member(ravi));
}

#[test]
fn g7_three_prepares_missed_in_a_row_tell_the_host_once() {
    let mut s = two();
    s.start();
    s.people[1].player.ready_ms = None;
    for k in 0..4 {
        s.run_ms(1_000);
        s.press(0, Action::Seek { content_us: (20 + 10 * k) * S });
        assert!(s.run_until(|s| s.commits(0).len() == 2 + k as usize, 4_000));
        let slow = s.notes.iter().filter(|n| **n == HostNote::Slow { id: s.id(1) }).count();
        assert_eq!(slow, usize::from(k >= 2), "after the third, once");
    }
    // Ready in time again: the count starts over.
    s.people[1].player.ready_ms = Some(100);
    s.run_ms(1_000);
    s.press(0, Action::Seek { content_us: 90 * S });
    assert!(s.run_until(|s| s.commits(0).len() == 6, 4_000));
    s.people[1].player.ready_ms = None;
    for k in 0..3 {
        s.run_ms(1_000);
        s.press(0, Action::Seek { content_us: (100 + 10 * k) * S });
        assert!(s.run_until(|s| s.commits(0).len() == 7 + k as usize, 4_000));
    }
    let slow = s.notes.iter().filter(|n| **n == HostNote::Slow { id: s.id(1) }).count();
    assert_eq!(slow, 2);
}

// --- Who waits for whom, presence, roles, endings, versions ------------------------------

#[test]
fn a_prepare_waits_for_players_only_and_a_joiner_mid_prepare_too() {
    let mut s = two();
    // A remote plays nothing: never waited for.
    s.set_mode(1, ListenMode::OneSpeaker);
    s.people[1].player.ready_ms = None;
    s.people[2].player.ready_ms = Some(1_000);
    s.run_ms(50);
    assert_eq!(s.member(s.id(1)).mode, ListenMode::OneSpeaker);
    s.press(0, Action::Play);
    s.run_ms(200);
    // Someone let in while it gets ready joins the prepare (A11), within the same 2.5 s.
    let never = Player {
        ready_ms: None,
        ..Player::default()
    };
    let ali = s.join("Ali", Way::LAN, Way::LAN, never);
    assert!(
        !s.prepares(ali).is_empty(),
        "the joiner is told the prepare with their welcome"
    );
    assert!(s.run_until(|s| !s.commits(0).is_empty(), 4_000));
    let prepared = s.prepares(0)[0].0;
    assert!(
        (s.commits(0)[0].0 - prepared - 2_500 * MS).abs() <= 10 * MS,
        "waited for Ali to the end of the 2.5 s"
    );
}

#[test]
fn someone_silent_for_5_s_shows_away_and_is_back_when_heard() {
    let mut s = two();
    let ravi = s.id(1);
    // The last ping was up to a second ago.
    s.people[1].player.pings = false;
    s.run_ms(3_900);
    assert!(!s.member(ravi).away);
    s.run_ms(1_200);
    assert!(s.member(ravi).away, "no ping for 5 s");
    assert_eq!(s.member(ravi).sub_state, None, "no state while away");
    s.people[1].player.pings = true;
    s.run_ms(1_100);
    assert!(!s.member(ravi).away, "back with no knock");
    assert_eq!(s.id(1), ravi);
}

#[test]
fn a_lost_member_is_kept_2_minutes_not_waited_for_then_gone() {
    let mut s = two();
    s.start();
    let ravi = s.id(1);
    let cut = s.now();
    s.cut(1);
    s.run_ms(16_000);
    assert!(s.member(ravi).away);
    // Not waited for meanwhile.
    let t = s.now();
    s.press(0, Action::Next);
    assert!(s.run_until(|s| s.commits(0).len() == 2, 4_000));
    assert!(s.commits(0)[1].0 - t < 1_000 * MS);
    s.set_step_ms(50);
    assert!(s.run_until(|s| !s.room.is_member(ravi), 130_000));
    // Lost 15 s after the last word from them (up to a second before the cut), out 2 minutes
    // after that.
    let gone = s.now() - cut;
    assert!((134 * S..135_200 * MS).contains(&gone), "{} ms", gone / MS);
    assert!(s.gone.contains(&ravi));
    assert!(s.activity().iter().any(|l| l == "Ravi lost connection"));
}

#[test]
fn back_within_2_minutes_is_the_same_member_with_the_rooms_plans_again() {
    let (mut s, a) = short_songs(3);
    let ravi = s.id(1);
    s.run_to(a.host_us + 5 * S);
    s.cut(1);
    s.set_step_ms(20);
    s.run_to(a.host_us + 25 * S);
    let before = s.people[1].got.len();
    s.restore(1);
    s.run_ms(200);
    assert_eq!(s.id(1), ravi);
    let got: Vec<_> = s.people[1].got[before..].iter().map(|g| &g.1).collect();
    assert!(matches!(got[0], ToGuest::Welcome { .. }), "welcomed again");
    assert!(
        got.iter().any(|m| matches!(m, ToGuest::ArmNext { .. })),
        "with the armed next"
    );
}

#[test]
fn guests_control_by_default_up_to_four_people() {
    let mut s = Sim::lan(songs(3), Opening::Resting, &["Ravi", "Meera", "Ali"]);
    let ravi = s.id(1);
    assert_eq!(s.room.role_of(ravi), Some(together::proto::Role::Control));
    let sam = s.join("Sam", Way::LAN, Way::LAN, Player::default());
    assert_eq!(s.room.role_of(ravi), Some(together::proto::Role::Add), "five: add only");
    assert_eq!(s.room.role_of(s.id(sam)), Some(together::proto::Role::Add));
    s.say(sam, ToHost::Bye);
    s.run_ms(50);
    assert_eq!(s.room.role_of(ravi), Some(together::proto::Role::Control), "four again");
    let now = s.now();
    let c = s.room.set_new_role(Some(together::proto::Role::Listen), now);
    s.apply(c);
    assert_eq!(s.room.role_of(ravi), Some(together::proto::Role::Listen));
    assert!(!s.room.view_for(HOST).settings.auto_roles);
}

#[test]
fn every_device_needs_the_hosts_network_and_is_left_when_it_goes() {
    let mut s = Sim::new(
        songs(3),
        Opening::Resting,
        &[
            ("Ravi", Way::LAN, Way::LAN),
            ("Ali", Way::internet(20.0, 1.0), Way::internet(20.0, 1.0)),
        ],
    );
    s.set_mode(1, ListenMode::EveryDevice);
    s.set_mode(2, ListenMode::EveryDevice);
    s.run_ms(100);
    assert_eq!(s.member(s.id(1)).mode, ListenMode::EveryDevice);
    assert_eq!(s.member(s.id(2)).mode, ListenMode::Apart, "not on the host's network");
    // Ravi's Wi-Fi turns slow (20 ms round trips) for 30 s: moved to apart, the host told.
    s.set_ways(1, Way::internet(10.0, 0.0), Way::internet(10.0, 0.0));
    s.set_step_ms(20);
    s.run_ms(25_000);
    assert_eq!(s.member(s.id(1)).mode, ListenMode::EveryDevice, "not yet");
    s.run_ms(40_000);
    assert_eq!(s.member(s.id(1)).mode, ListenMode::Apart);
    assert!(s.notes.contains(&HostNote::SameRoomOff { id: s.id(1) }));
}

#[test]
fn leaving_and_being_removed_touch_nobody_else() {
    let mut s = two();
    let a = s.start();
    s.run_ms(2_000);
    let t = s.now();
    let ravi = s.id(1);
    s.say(1, ToHost::Bye);
    s.run_ms(100);
    assert!(!s.room.is_member(ravi) && s.gone.contains(&ravi));
    assert!(s.activity().iter().any(|l| l == "Ravi left"));
    let meera = s.id(2);
    let now = s.now();
    let c = s.room.remove(meera, true, now);
    assert_eq!(
        c.to(meera),
        vec![&ToGuest::Refused {
            why: "Asha took you out of the room.".into()
        }]
    );
    s.apply(c);
    s.run_ms(100);
    assert!(s.people[2].got.iter().any(|g| matches!(&g.1, ToGuest::Refused { .. })));
    assert_eq!(s.room.anchor(), Some(a));
    assert!(s.playback_since(0, t).is_empty(), "the host hears nothing of it");
}

#[test]
fn ending_for_everyone_tells_everyone_the_host_too() {
    let mut s = two();
    s.start();
    s.run_ms(2_000);
    let now = s.now();
    let c = s.room.end(now);
    s.apply(c);
    s.run_ms(100);
    assert!(s.ended);
    for i in 0..3 {
        assert!(
            s.people[i]
                .got
                .iter()
                .any(|g| g.1 == ToGuest::Ended { why: "Asha ended the room.".into() }),
            "person {i}"
        );
    }
    s.press(0, Action::Play);
    assert!(s.refused.last().is_some_and(|r| r.0 == HOST));
}

#[test]
fn a_held_room_ends_itself_after_an_hour_without_an_action() {
    let mut s = two();
    s.start();
    s.run_ms(2_000);
    s.press(0, Action::Pause);
    s.set_step_ms(200);
    s.run_ms(59 * 60_000);
    assert!(!s.ended);
    s.press(1, Action::Seek { content_us: 10 * S });
    s.run_ms(59 * 60_000);
    assert!(!s.ended, "an action starts the hour again");
    s.run_ms(2 * 60_000);
    assert!(s.ended);
    assert!(s.people[1].got.iter().any(|g| matches!(&g.1, ToGuest::Ended { .. })));
    assert!(s.people[0].got.iter().any(|g| matches!(&g.1, ToGuest::Ended { .. })));
}

#[test]
fn a_0_3_guest_at_a_protocol_3_host_is_told_different_versions() {
    let mut s = Sim::lan(songs(1), Opening::Resting, &[]);
    let old = s.net.peer(std::sync::Arc::new(s.clock.clone()));
    let rx = together::transport::Transport::events(&old);
    s.net.connect(&old, &s.host_t(), Way::LAN, Way::LAN);
    let hello = format!(
        "{{\"t\":\"hello\",\"protocol\":2,\"name\":\"Ravi\",\"proof\":\"{}\",\"app\":\"Venuta 0.3.0\"}}\n",
        together::invite::proof(SECRET, "x")
    );
    together::transport::Transport::send(
        &old,
        s.host_t().id(),
        together::transport::ControlMsg(hello.into_bytes()),
    )
    .unwrap();
    s.run_ms(50);
    let got: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            together::transport::TransportEvent::Message { msg, .. } => {
                msg.read::<together::proto::ToGuest>().ok()
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        got,
        vec![together::proto::ToGuest::Refused {
            why: together::proto::v3::DIFFERENT_VERSIONS.into()
        }],
        "read by the 0.3 guest as its own refusal"
    );
    assert_eq!(s.room.member_ids(), vec![HOST]);
}
