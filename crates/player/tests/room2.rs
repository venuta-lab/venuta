//! Rooms on engine 2 through `RoomHandle` (the WP5 notes in 9.2 of the Listen together
//! design): today's room scenarios (`room_clicks`, `room_flow`, `room_smooth`, `room_end`)
//! ported to the simulation, on one simulated clock the test moves, so they are fast and the
//! same on a busy machine. Every song carries the test music (left) and a counting ramp
//! (right): the music is judged by the detectors, the ramp says where every sound came from and
//! (with the simulated DAC's exact times) how far from the room it was heard.

mod room_kit;

use player::engine::Cmd;
use player::sim::SimConfig;
use player::voice_api::{FadeKind, RenderNote};
use room_kit::*;
use std::path::PathBuf;
use std::sync::Arc;
use together::align::Alignment;
use together::anchor::{Anchor, EntryId, PrepId, Transition};
use together::clock_map::{ClockMap, ClockMapHandle};
use together::room_player::{RoomHandle, RoomPlayer, SubState, SyncMode};
use together::time::{MonoClock, SimClock};

const EXACT: Alignment = Alignment::EXACT;

/// One person in a room the test plays.
struct Room {
    clock: SimClock,
    p: Person<PathBuf>,
    h: Option<Box<dyn RoomHandle<PathBuf>>>,
    host: HostTime,
    map: ClockMapHandle,
    tl: Timeline,
    prep: u64,
}

impl Room {
    fn with(cfg: SimConfig, host: HostTime, mode: SyncMode) -> Room {
        let clock = SimClock::new(1_000_000_000);
        let p = Person::new(&clock, cfg);
        let map = host.exact_map();
        let h = RoomPlayer::attach(p.player(), map.clone(), mode);
        Room {
            clock,
            p,
            h: Some(h),
            host,
            map,
            tl: Timeline::default(),
            prep: 0,
        }
    }

    fn new(mode: SyncMode) -> Room {
        Room::with(SimConfig::default(), HostTime::same(), mode)
    }

    fn h(&self) -> &dyn RoomHandle<PathBuf> {
        self.h.as_deref().expect("in the room")
    }

    fn pass(&mut self, ms: u64) {
        pass(&self.clock, ms, &mut []);
        self.p.collect();
    }

    fn host_now(&self) -> i64 {
        self.host.of(self.clock.now_ns() as f64) as i64
    }

    fn now_ns(&self) -> f64 {
        self.clock.now_ns() as f64
    }

    /// Prepare `entry` (its song `f`, ramp from `base`) at the room's `content_us`.
    fn prepare(&mut self, entry: u64, f: &std::path::Path, content_us: i64) -> PrepId {
        self.prep += 1;
        let prep = PrepId(self.prep);
        self.h()
            .prepare(prep, EntryId(entry), f.to_path_buf(), content_us, EXACT);
        prep
    }

    /// Commit the room playing `entry` with `a`, its ramp from `base`.
    fn commit(&mut self, a: Anchor, base: i32) {
        self.tl.set(a.host_us.min(self.host_now()), a, base);
        self.h().commit(a);
    }

    /// The room starts `entry` at its `content_us`, `lead_ms` from now, after getting it ready.
    fn start(&mut self, entry: u64, f: &std::path::Path, base: i32, content_us: i64, lead_ms: i64) {
        self.prepare(entry, f, content_us);
        self.pass(300);
        let a = anchor(entry, content_us, self.host_now() + lead_ms * 1_000);
        self.commit(a, base);
    }

    fn hold(&mut self, content_us: i64, entry: u64, base: i32) -> Anchor {
        let a = Anchor {
            entry: EntryId(entry),
            content_us,
            host_us: self.host_now() + 150_000,
            playing: false,
            seq: 0,
        };
        // The room plays on until the hold's moment.
        self.tl.set(a.host_us, a, base);
        self.h().hold(a);
        a
    }

    /// Where the room is now (content µs), by the newest anchor.
    fn room_place(&self) -> i64 {
        let (_, a, _) = self.tl.from.last().expect("an anchor");
        a.content_at(self.host_now())
    }

    fn finish(&mut self) -> Heard {
        if let Some(h) = self.h.take() {
            h.detach();
        }
        self.pass(1_500);
        self.p.finish()
    }

    /// The errors heard from `from_ns` (local) on, ms.
    fn errors(
        heard: &Heard,
        tl: &Timeline,
        host: &HostTime,
        from_ns: f64,
        to_ns: f64,
    ) -> Vec<(f64, f64)> {
        errors(heard, tl, host, from_ns, to_ns)
    }
}

fn crossfades_after(notes: &[RenderNote], at: u64) -> usize {
    count(
        notes,
        |n| matches!(n, RenderNote::Faded { kind: FadeKind::CrossIn, from, .. } if *from > at),
    )
}

fn started(notes: &[RenderNote]) -> Vec<(u64, u64, u32)> {
    notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Started {
                at, content, fade, ..
            } => Some((at, content, fade)),
            _ => None,
        })
        .collect()
}

// --- In step ------------------------------------------------------------------------------------

#[test]
fn a_room_in_step_is_clean_apart_and_in_one_room() {
    for mode in [SyncMode::Apart, SyncMode::EveryDevice] {
        let a = song(0.0, 12.0, BASES[0], true, true);
        let mut r = Room::new(mode);
        r.start(1, &a, BASES[0], 0, 400);
        let t0 = r.now_ns();
        r.pass(8_000);
        let st = r.h().status();
        assert_eq!(st.entry, Some(EntryId(1)), "{mode:?}: {st:?}");
        assert_eq!(st.sub_state, SubState::InStep, "{mode:?}: {st:?}");
        assert!(st.heard.is_some(), "{mode:?}: says what it hears");
        let (tl, host) = (r.tl.clone(), r.host);
        let heard = r.finish();
        assert_music_clean(&format!("{mode:?}"), &heard);
        let e = Room::errors(&heard, &tl, &host, t0 + 1e9, t0 + 8e9);
        assert!(e.len() > 500, "judged");
        assert!(worst(&e) < 0.1, "{mode:?}: in step: worst {} ms", worst(&e));
        assert_eq!(
            crossfades_after(&heard.notes, 0),
            0,
            "{mode:?}: nothing realigned"
        );
        // The song's very first frame on the anchor's frame, no fade.
        let s = started(&heard.notes);
        assert_eq!((s[0].1, s[0].2), (0, 0), "from its first frame: {s:?}");
    }
}

#[test]
fn a_sound_card_fast_or_slow_is_followed_by_rate_alone() {
    for (mode, drift) in [
        (SyncMode::EveryDevice, 300.0),
        (SyncMode::EveryDevice, -300.0),
        (SyncMode::Apart, 300.0),
    ] {
        let a = song(0.0, 30.0, BASES[0], true, true);
        let cfg = SimConfig {
            skew_ppm: drift,
            ..SimConfig::default()
        };
        let mut r = Room::with(cfg, HostTime::same(), mode);
        r.start(1, &a, BASES[0], 0, 400);
        let t0 = r.now_ns();
        r.pass(25_000);
        let st = r.h().status();
        assert!(
            (st.ppm + drift).abs() < 30.0,
            "{mode:?} {drift}: the trim takes the card's drift: {st:?}"
        );
        let (tl, host) = (r.tl.clone(), r.host);
        let heard = r.finish();
        let name = format!("{mode:?} {drift} ppm");
        assert_music_clean(&name, &heard);
        let e = Room::errors(&heard, &tl, &host, t0 + 5e9, t0 + 25e9);
        let (p95, max) = (quantile(&e, 0.95), worst(&e));
        let (p95_gate, max_gate) = match mode {
            SyncMode::EveryDevice => (1.0, 3.0),
            SyncMode::Apart => (5.0, 10.0),
        };
        assert!(
            p95 <= p95_gate && max <= max_gate,
            "{name}: p95 {p95} ms, max {max} ms"
        );
        assert_eq!(
            crossfades_after(&heard.notes, 0),
            0,
            "{name}: never a realign"
        );
    }
}

#[test]
fn a_clock_estimate_that_steps_is_taken_up_by_rate_not_heard() {
    // Apart, the map stepped by 150 ms (a quicker path found): under the realign line, so it is
    // closed by rate, never by a jump. One room, 8 ms: the same, quickly.
    for (mode, step_us) in [(SyncMode::Apart, 150_000), (SyncMode::EveryDevice, 8_000)] {
        let a = song(0.0, 30.0, BASES[0], true, true);
        let mut r = Room::new(mode);
        r.start(1, &a, BASES[0], 0, 400);
        r.pass(2_000);
        let m = (**r.map.load()).unwrap();
        r.map.store(Arc::new(Some(ClockMap {
            host_ref_us: m.host_ref_us + step_us,
            ..m
        })));
        let t_step = r.clock.now_ns();
        r.pass(2_000);
        let st = r.h().status();
        assert_eq!(st.sub_state, SubState::Correcting, "{mode:?}: {st:?}");
        assert!(
            st.ppm > 100.0 && st.ppm <= 500.0,
            "{mode:?}: catching up: {st:?}"
        );
        r.pass(16_000);
        let st = r.h().status();
        if mode == SyncMode::EveryDevice {
            assert!(
                st.error_us.unwrap().abs() < 1_000,
                "{mode:?}: back in step: {st:?}"
            );
        }
        let heard = r.finish();
        assert_music_clean(&format!("{mode:?} stepped"), &heard);
        let at = heard.tick_at(t_step as f64) as u64;
        assert_eq!(
            crossfades_after(&heard.notes, at),
            0,
            "{mode:?}: no realign"
        );
    }
}

// --- Far off: a realign, at a quiet moment ------------------------------------------------------

#[test]
fn far_off_apart_realigns_once_by_a_crossfade_at_a_quiet_moment() {
    // A quiet moment 10 s into the song (60 dB down for 120 ms).
    // The ramp crosses zero there too, so both channels are quiet.
    let base = -(10.06 * f64::from(R)) as i32;
    let a = song_with(0.0, 40.0, base, true, true, &[10.0..10.12]);
    let mut r = Room::new(SyncMode::Apart);
    r.start(1, &a, base, 0, 400);
    r.pass(2_000);
    // The room is suddenly 900 ms further on than this player thought.
    let m = (**r.map.load()).unwrap();
    r.map.store(Arc::new(Some(ClockMap {
        host_ref_us: m.host_ref_us + 900_000,
        ..m
    })));
    let t_step = r.now_ns();
    r.pass(14_000);
    let st = r.h().status();
    assert!(
        st.error_us.unwrap().abs() < 10_000,
        "lined up again: {st:?}"
    );
    let heard = r.finish();
    assert_music_clean_with("far off", &heard, &heard.playing_span(base, 10.0, 10.12));
    let at = heard.tick_at(t_step) as u64;
    let xf: Vec<(u64, u64)> = heard
        .notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Faded {
                from,
                to,
                kind: FadeKind::CrossOut,
                ..
            } if from > at => Some((from, to)),
            _ => None,
        })
        .collect();
    assert_eq!(xf.len(), 1, "once: {xf:?}");
    let (from, to) = xf[0];
    assert_eq!(to - from, ms(40), "a 40 ms crossfade");
    // It went out of the old place inside the quiet moment (the ramp says where it was).
    let before = heard.count(from as usize) - base - 1;
    let t = f64::from(before) / f64::from(R);
    assert!((10.0..10.12).contains(&t), "at the quiet moment: {t} s");
}

#[test]
fn far_off_in_one_room_with_no_quiet_moment_realigns_at_the_quietest_after_ten_seconds() {
    let a = song(0.0, 40.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::EveryDevice);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(2_000);
    let m = (**r.map.load()).unwrap();
    r.map.store(Arc::new(Some(ClockMap {
        host_ref_us: m.host_ref_us + 80_000,
        ..m
    })));
    let t_step = r.now_ns();
    r.pass(9_000);
    let st = r.h().status();
    assert!(st.error_us.unwrap().abs() > 50_000, "not yet: {st:?}");
    r.pass(8_000);
    let st = r.h().status();
    assert!(st.error_us.unwrap().abs() < 3_000, "lined up again: {st:?}");
    let heard = r.finish();
    assert_music_clean("far off, one room", &heard);
    let at = heard.tick_at(t_step) as u64;
    assert_eq!(crossfades_after(&heard.notes, at), 1, "once");
}

// --- Entrances ------------------------------------------------------------------------------------

#[test]
fn joining_partway_fades_in_over_300_ms_in_step() {
    let a = song(0.0, 20.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::EveryDevice);
    r.pass(100);
    // The room started this song 3 s ago; the joiner gets it ready a second on, and the anchor.
    let a0 = anchor(1, 0, r.host_now() - 3_000_000);
    let place = a0.content_at(r.host_now() + 1_000_000);
    r.prepare(1, &a, place);
    r.commit(a0, BASES[0]);
    let t0 = r.now_ns();
    r.pass(5_000);
    let (tl, host) = (r.tl.clone(), r.host);
    let heard = r.finish();
    assert_music_clean("joined", &heard);
    let s = started(&heard.notes);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].2 as u64, ms(300), "a musical entrance: {s:?}");
    let e = Room::errors(&heard, &tl, &host, t0 + 2e9, t0 + 5e9);
    assert!(worst(&e) < 0.1, "in step: {}", worst(&e));
}

#[test]
fn another_output_mid_song_comes_back_in_step_from_silence() {
    let a = song(0.0, 20.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::EveryDevice);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(3_000);
    let next = r.p.another_output();
    r.p.player().send(Cmd::Output(next));
    r.pass(400);
    assert_eq!(r.h().status().sub_state, SubState::Realigning);
    r.pass(3_000);
    let t1 = r.now_ns();
    r.pass(2_000);
    let st = r.h().status();
    assert_eq!(st.sub_state, SubState::InStep, "{st:?}");
    let (tl, host) = (r.tl.clone(), r.host);
    let heard = r.finish();
    assert_eq!(r.p.outputs.lock().unwrap().len(), 2, "on another output");
    assert_music_clean("output switch", &heard);
    let s = started(&heard.notes);
    assert_eq!(
        s.last().unwrap().2 as u64,
        ms(15),
        "from silence, 15 ms: {s:?}"
    );
    let e = Room::errors(&heard, &tl, &host, t1, t1 + 2e9);
    assert!(
        !e.is_empty() && worst(&e) < 0.1,
        "in step on the new one: {}",
        worst(&e)
    );
}

// --- Pause, resume, seek, skip ---------------------------------------------------------------------

#[test]
fn pause_resume_seek_and_skip_are_clean_and_land_where_the_room_is() {
    let a = song(0.0, 40.0, BASES[0], true, true);
    let b = song(50.0, 20.0, BASES[1], true, true);
    let mut r = Room::new(SyncMode::Apart);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(3_000);
    // Paused (scheduled 150 ms on), then resumed where it held.
    let held = r.room_place() + 150_000;
    r.hold(held, 1, BASES[0]);
    r.pass(1_000);
    r.start(1, &a, BASES[0], held, 300);
    r.pass(3_000);
    // A seek to 25 s: everyone plays on until the commit, then a 40 ms crossfade.
    let t_seek = r.now_ns();
    r.prepare(1, &a, 25_000_000);
    r.pass(300);
    let at = r.host_now() + 300_000;
    r.commit(anchor(1, 25_000_000, at), BASES[0]);
    r.pass(3_000);
    // Skipped to B: the old fades out over 30 ms ending where B starts, from its first frame.
    r.prepare(2, &b, 0);
    r.pass(300);
    let at = r.host_now() + 300_000;
    r.commit(anchor(2, 0, at), BASES[1]);
    let t_skip = r.now_ns();
    r.pass(3_000);
    let (tl, host) = (r.tl.clone(), r.host);
    let heard = r.finish();
    assert_music_clean("pause, resume, seek, skip", &heard);
    let st = runs(&heard);
    // A up to the pause; A from the pause (from silence); A from 25 s; B from its start.
    assert!(st.len() >= 4, "{st:?}");
    let resumed = f64::from(st[1].0 - BASES[0] - 1) * 1e6 / f64::from(R);
    assert!(
        (resumed - held as f64).abs() < 30_000.0,
        "resumed at the pause: {resumed} against {held}"
    );
    let sought = f64::from(st[2].0 - BASES[0] - 1) / f64::from(R);
    assert!((sought - 25.0).abs() < 0.05, "from 25 s: {sought}");
    assert_eq!(
        st.last().unwrap().0,
        BASES[1] + 1,
        "B from its very first frame"
    );
    let at = heard.tick_at(t_seek) as u64;
    assert_eq!(crossfades_after(&heard.notes, at), 1, "the seek crossfaded");
    for (from, label) in [
        (t_seek + 1.2e9, "after the seek"),
        (t_skip + 1e9, "after the skip"),
    ] {
        let e = Room::errors(&heard, &tl, &host, from, from + 1.5e9);
        assert!(!e.is_empty() && worst(&e) < 0.1, "{label}: {}", worst(&e));
    }
}

#[test]
fn a_newer_prepare_replaces_one_getting_ready() {
    // Play pressed on a paused room, and while it gets ready a seek to 30 s: the start is from
    // 30 s, straight on, never at the pause.
    let a = song(0.0, 40.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::Apart);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(2_500);
    let held = r.room_place() + 150_000;
    r.hold(held, 1, BASES[0]);
    r.pass(800);
    let p1 = r.prepare(1, &a, held);
    r.pass(150);
    let p2 = r.prepare(1, &a, 30_000_000);
    r.pass(300);
    let st = r.h().status();
    assert_eq!(
        st.ready,
        Some(p2),
        "the newer is the one ready (not {p1:?})"
    );
    let at = r.host_now() + 300_000;
    r.commit(anchor(1, 30_000_000, at), BASES[0]);
    r.pass(3_000);
    let heard = r.finish();
    assert_music_clean("a newer prepare", &heard);
    let st = runs(&heard);
    assert_eq!(st.len(), 2, "up to the pause, then from 30 s: {st:?}");
    let from = f64::from(st[1].0 - BASES[0] - 1) / f64::from(R);
    assert!((from - 30.0).abs() < 0.03, "from 30 s: {from}");
}

#[test]
fn waiting_silent_for_the_rooms_start_is_ready_not_a_stall() {
    let a = song(0.0, 10.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::Apart);
    let prep = r.prepare(1, &a, 0);
    r.pass(800);
    let st = r.h().status();
    assert_eq!(st.ready, Some(prep));
    assert_eq!(st.sub_state, SubState::Ready);
    let s = r.p.status().clone();
    assert!(
        !s.buffering && !s.stalled,
        "ready, not waiting for a stream"
    );
    assert!(s.sync.is_some_and(|y| y.ready));
    r.finish();
}

#[test]
fn stepping_out_and_back_in_catches_up_at_the_rooms_place() {
    let a = song(0.0, 30.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::EveryDevice);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(2_000);
    r.h().step_out();
    r.pass(1_000);
    assert_eq!(r.h().status().sub_state, SubState::SteppedOut);
    r.pass(2_000);
    r.h().rejoin();
    r.pass(2_000);
    let t1 = r.now_ns();
    r.pass(2_000);
    let (tl, host) = (r.tl.clone(), r.host);
    let heard = r.finish();
    assert_music_clean("stepped out", &heard);
    let s = started(&heard.notes);
    assert_eq!(
        s.last().unwrap().2 as u64,
        ms(300),
        "caught up with a 300 ms entrance"
    );
    let e = Room::errors(&heard, &tl, &host, t1, t1 + 2e9);
    assert!(!e.is_empty() && worst(&e) < 0.1, "in step: {}", worst(&e));
}

#[test]
fn a_sleep_or_lost_focus_suspends_alone_and_comes_back_in_at_the_rooms_place() {
    use together::room_player::Suspension;
    let a = song(0.0, 30.0, BASES[0], true, true);
    let mut r = Room::new(SyncMode::Apart);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(2_000);
    assert_eq!(
        r.h().status().length_us,
        Some(30_000_000),
        "the entry's length as decoded"
    );
    r.h().suspend(Suspension::Sleeping);
    r.pass(1_000);
    assert_eq!(
        r.h().status().sub_state,
        SubState::Suspended(Suspension::Sleeping)
    );
    r.pass(2_000);
    r.h().resume();
    r.pass(2_000);
    let t1 = r.now_ns();
    assert_eq!(r.h().status().sub_state, SubState::InStep);
    r.pass(2_000);
    let (tl, host) = (r.tl.clone(), r.host);
    let heard = r.finish();
    assert_music_clean("suspended for a sleep", &heard);
    let s = started(&heard.notes);
    assert_eq!(
        s.last().unwrap().2 as u64,
        ms(300),
        "back with a 300 ms entrance"
    );
    let e = Room::errors(&heard, &tl, &host, t1, t1 + 2e9);
    assert!(!e.is_empty() && worst(&e) < 0.1, "in step: {}", worst(&e));
}

// --- Songs changing at the armed boundary ------------------------------------------------------

/// The room plays A from its start, its reference `ref_s` long, then B, armed with `how`;
/// my copy of A is `mine_s` long. What was heard, and when B's anchor began (host µs).
fn boundary(how: Transition, mine_s: f64, ref_s: f64, seamless: bool) -> (Heard, Timeline, i64) {
    let a = song(0.0, mine_s, BASES[0], true, !seamless);
    let b = song(
        if seamless { mine_s } else { 50.0 },
        10.0,
        BASES[1],
        !seamless,
        true,
    );
    let mut r = Room::new(SyncMode::EveryDevice);
    r.start(1, &a, BASES[0], 0, 400);
    let start = r.tl.from[0].1.host_us;
    let fade_us = match how {
        Transition::Crossfade { ms } => i64::from(ms) * 1_000,
        _ => 0,
    };
    let boundary = start + (ref_s * 1e6) as i64 - fade_us;
    r.pass(1_000);
    r.h().arm_next(EntryId(2), b.clone(), boundary, how, EXACT);
    r.tl.set(boundary, anchor(2, 0, boundary), BASES[1]);
    r.pass(((ref_s + 1.0) * 1000.0) as u64);
    let st = r.h().status();
    assert_eq!(st.entry, Some(EntryId(2)), "{how:?}: B plays: {st:?}");
    // The host's commit after the boundary: a no-op.
    r.h().commit(anchor(2, 0, boundary));
    r.pass(3_000);
    let tl = r.tl.clone();
    (r.finish(), tl, boundary)
}

#[test]
fn exact_copies_join_gaplessly_at_the_boundary() {
    let (heard, tl, b) = boundary(Transition::Gapless, 6.0, 6.0, true);
    assert_music_clean("gapless", &heard);
    assert!(
        has(&heard.notes, |n| matches!(n, RenderNote::Joined { .. })),
        "joined"
    );
    let joined = heard
        .notes
        .iter()
        .find_map(|n| match *n {
            RenderNote::Joined { at, .. } => Some(at),
            _ => None,
        })
        .unwrap();
    assert!(
        !has(
            &heard.notes,
            |n| matches!(n, RenderNote::Faded { from, to, .. } if *from <= joined + 10 && *to + 10 >= joined)
        ),
        "no fade at the join"
    );
    // The ramp: A's last frame, then B's first, on consecutive ticks.
    let k = joined as usize;
    assert_eq!(
        heard.count(k),
        BASES[1] + 1,
        "B from its first frame at the join"
    );
    assert_eq!(
        heard.count(k - 1),
        BASES[0] + (6 * R) as i32,
        "after A's last"
    );
    let e = errors(
        &heard,
        &tl,
        &HostTime::same(),
        b as f64 * 1e3 + 5e8,
        b as f64 * 1e3 + 3e9,
    );
    assert!(!e.is_empty() && worst(&e) < 0.1, "B in step: {}", worst(&e));
}

#[test]
fn a_shorter_copy_waits_in_silence_and_the_next_starts_on_the_boundary() {
    let (heard, tl, b) = boundary(Transition::Gapless, 5.8, 6.0, false);
    assert_music_clean("shorter", &heard);
    let s = started(&heard.notes);
    let (at, content, fade) = *s.last().unwrap();
    assert_eq!((content, fade), (0, 0), "B from its start, no fade");
    let e = errors(
        &heard,
        &tl,
        &HostTime::same(),
        b as f64 * 1e3 + 3e8,
        b as f64 * 1e3 + 3e9,
    );
    assert!(!e.is_empty() && worst(&e) < 0.1, "B in step: {}", worst(&e));
    assert_eq!(heard.count(at as usize), BASES[1] + 1);
}

#[test]
fn a_longer_copy_fades_out_over_150_ms_ending_on_the_boundary() {
    let (heard, tl, b) = boundary(Transition::Gapless, 6.5, 6.0, false);
    assert_music_clean("longer", &heard);
    let s = started(&heard.notes);
    let (at, content, _) = *s.last().unwrap();
    assert_eq!(content, 0);
    assert!(
        has(
            &heard.notes,
            |n| matches!(n, RenderNote::Faded { from, to, kind: FadeKind::Out, .. }
            if *to == at && to - from == ms(150))
        ),
        "A faded out over 150 ms ending where B starts"
    );
    let e = errors(
        &heard,
        &tl,
        &HostTime::same(),
        b as f64 * 1e3 + 3e8,
        b as f64 * 1e3 + 3e9,
    );
    assert!(!e.is_empty() && worst(&e) < 0.1, "B in step: {}", worst(&e));
}

#[test]
fn the_rooms_crossfade_starts_on_the_boundary() {
    let (heard, tl, b) = boundary(Transition::Crossfade { ms: 2_000 }, 6.0, 6.0, false);
    assert_music_clean("crossfade", &heard);
    assert!(
        has(
            &heard.notes,
            |n| matches!(n, RenderNote::Faded { from, to, kind: FadeKind::CrossIn, .. }
            if to - from == ms(2_000))
        ),
        "a 2 s crossfade"
    );
    let e = errors(
        &heard,
        &tl,
        &HostTime::same(),
        b as f64 * 1e3 + 2.5e9,
        b as f64 * 1e3 + 4e9,
    );
    assert!(!e.is_empty() && worst(&e) < 0.1, "B in step: {}", worst(&e));
}

#[test]
fn a_disarmed_next_never_plays() {
    let a = song(0.0, 4.0, BASES[0], true, true);
    let b = song(50.0, 10.0, BASES[1], true, true);
    let mut r = Room::new(SyncMode::Apart);
    r.start(1, &a, BASES[0], 0, 400);
    let start = r.tl.from[0].1.host_us;
    r.pass(500);
    r.h()
        .arm_next(EntryId(2), b, start + 4_000_000, Transition::Gapless, EXACT);
    r.pass(500);
    r.h().disarm();
    r.pass(5_000);
    let heard = r.finish();
    assert!(
        !heard.right.iter().any(|&v| (v >> 8) > BASES[1]),
        "B never sounded"
    );
}

// --- The room ending, and your music after it ------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum When {
    WaitingForStart,
    PlayingApart,
    Correcting,
    Realigning,
    NextTookOver,
    Held,
}

/// Your own song playing; a room takes the player 2 s in and is `when`; it ends; you press
/// play. Against a player that paused there and never joined: bit-identical.
fn end_room(when: When) {
    let own = song(100.0, 30.0, BASES[3], true, true);
    let mut joined = Room::new(SyncMode::Apart);
    // Detach the room the constructor made: this test attaches at 2 s into your song.
    if let Some(h) = joined.h.take() {
        h.detach();
    }
    let clock = SimClock::new(1_000_000_000);
    let mut alone: Person<PathBuf> = Person::new(&clock, SimConfig::default());
    let set = |p: &Person<PathBuf>| {
        p.player().send(Cmd::Set {
            items: vec![own.clone()],
            index: 0,
            start_ms: 0,
            play: true,
            from: String::new(),
        })
    };
    set(&joined.p);
    set(&alone);
    joined.pass(2_000);
    pass(&clock, 2_000, &mut []);
    let mode = match when {
        When::Correcting => SyncMode::EveryDevice,
        _ => SyncMode::Apart,
    };
    joined.h = Some(RoomPlayer::attach(
        joined.p.player(),
        joined.map.clone(),
        mode,
    ));
    alone.player().send(Cmd::Pause);
    joined.pass(20);
    pass(&clock, 20, &mut []);
    let a = song(0.0, 8.0, BASES[0], true, false);
    let b = song(8.0, 8.0, BASES[1], false, true);
    match when {
        When::WaitingForStart => {
            joined.prepare(1, &a, 0);
            joined.pass(500);
        }
        When::PlayingApart => {
            joined.start(1, &a, BASES[0], 0, 300);
            joined.pass(1_500);
        }
        When::Correcting => {
            joined.start(1, &a, BASES[0], 0, 300);
            joined.pass(1_200);
            let m = (**joined.map.load()).unwrap();
            joined.map.store(Arc::new(Some(ClockMap {
                host_ref_us: m.host_ref_us + 15_000,
                ..m
            })));
            joined.pass(1_000);
            assert_eq!(joined.h().status().sub_state, SubState::Correcting);
        }
        When::Realigning => {
            let base = -(5.1 * f64::from(R)) as i32;
            let a = song_with(0.0, 30.0, base, true, true, &[5.0..5.2]);
            joined.start(1, &a, base, 0, 300);
            joined.pass(1_000);
            let m = (**joined.map.load()).unwrap();
            joined.map.store(Arc::new(Some(ClockMap {
                host_ref_us: m.host_ref_us + 900_000,
                ..m
            })));
            joined.pass(3_800);
            assert_eq!(joined.h().status().sub_state, SubState::Realigning);
        }
        When::NextTookOver => {
            joined.start(1, &a, BASES[0], 0, 300);
            let start = joined.tl.from[0].1.host_us;
            joined.pass(1_000);
            joined.h().arm_next(
                EntryId(2),
                b.clone(),
                start + 8_000_000,
                Transition::Gapless,
                EXACT,
            );
            joined.pass(7_330);
            assert_eq!(joined.h().status().entry, Some(EntryId(2)));
        }
        When::Held => {
            joined.start(1, &a, BASES[0], 0, 300);
            joined.pass(1_000);
            let at = joined.room_place() + 150_000;
            joined.hold(at, 1, BASES[0]);
            joined.pass(300);
        }
    }
    if let Some(h) = joined.h.take() {
        h.detach();
    }
    joined.pass(1_500);
    pass(&clock, 1_500, &mut []);
    let (j, al) = (joined.p.status().clone(), alone.status().clone());
    assert!(!j.playing, "{when:?}: your own queue back, paused");
    assert!(j.sync.is_none(), "{when:?}: nothing of the room left");
    assert_eq!(j.position_ms, al.position_ms, "{when:?}: at its place");
    joined.p.player().send(Cmd::Play);
    alone.player().send(Cmd::Play);
    joined.pass(2_000);
    pass(&clock, 2_000, &mut []);
    let (hj, ha) = (joined.p.finish(), alone.finish());
    let last = |h: &Heard| {
        h.notes
            .iter()
            .rev()
            .find_map(|n| match *n {
                RenderNote::Started { at, content, .. } => Some((at as usize, content)),
                _ => None,
            })
            .unwrap()
    };
    let ((sj, cj), (sa, ca)) = (last(&hj), last(&ha));
    assert_eq!(cj, ca, "{when:?}: from the same place");
    let n = ms(1_500) as usize;
    assert!(
        hj.left[sj..sj + n] == ha.left[sa..sa + n] && hj.right[sj..sj + n] == ha.right[sa..sa + n],
        "{when:?}: bit-identical to a player that never joined"
    );
    assert!(
        !has(&hj.notes[hj.notes.len() - 4..], |n| matches!(
            n,
            RenderNote::Resampling { on: true, .. }
        )),
        "{when:?}: not resampled after the room"
    );
}

#[test]
fn after_a_room_ends_your_music_is_bit_identical_whatever_it_was_doing() {
    for when in [
        When::WaitingForStart,
        When::PlayingApart,
        When::Correcting,
        When::Realigning,
        When::NextTookOver,
        When::Held,
    ] {
        end_room(when);
    }
}

// --- The host's room, made from its own music in place -------------------------------------------

#[test]
fn the_hosts_room_is_made_in_place_without_a_sample_changed() {
    let a = song(0.0, 8.0, BASES[0], true, false);
    let b = song(8.0, 8.0, BASES[1], false, true);
    let clock = SimClock::new(1_000_000_000);
    let mut host: Person<PathBuf> = Person::new(&clock, SimConfig::default());
    let mut solo: Person<PathBuf> = Person::new(&clock, SimConfig::default());
    host.player().send(Cmd::Set {
        items: vec![a.clone()],
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    solo.player().send(Cmd::Set {
        items: vec![a.clone(), b.clone()],
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    pass(&clock, 2_000, &mut []);
    // The host opens a room: the song playing is the room's entry 1, untouched.
    let h = host
        .player()
        .adopt(identity(), SyncMode::EveryDevice, EntryId(1));
    pass(&clock, 200, &mut []);
    let heard = h.status().heard.expect("says what it hears");
    assert_eq!(heard.entry, EntryId(1));
    let now = host_now(&clock);
    assert!((heard.host_us - now).abs() < 50_000, "now-ish: {heard:?}");
    // The room's opening anchor is the host's live place.
    let opening = Anchor { seq: 1, ..heard };
    h.commit(opening);
    let a_end = opening.host_us + (8_000_000 - opening.content_us);
    pass(&clock, 1_000, &mut []);
    h.arm_next(EntryId(2), b.clone(), a_end, Transition::Gapless, EXACT);
    pass(&clock, 8_000, &mut []);
    assert_eq!(h.status().entry, Some(EntryId(2)), "on to the next");
    assert_eq!(h.status().sub_state, SubState::InStep);
    let st = host.status().clone();
    assert!(!st.playing, "the host's own queue waits, paused");
    let (hh, hs) = (host.finish(), solo.finish());
    let n = ms(9_000) as usize;
    assert!(
        hh.left[..n] == hs.left[..n] && hh.right[..n] == hs.right[..n],
        "the host heard exactly what it hears alone, across the room's opening and the next song"
    );
    assert!(
        !has(
            &hh.notes,
            |x| matches!(x, RenderNote::Faded { from, .. } if (*from as usize) < n)
        ),
        "no fade: {:?}",
        hh.notes
    );
    assert_exact_where_undeclared("the host", &hh.upto(n), &hs.left[..n]);
}

// --- Every device: an output that can't hold ---------------------------------------------------

#[test]
fn an_output_that_cant_hold_in_every_device_mode_fades_out_alone_until_apart() {
    // The room's clock runs 1000 ppm faster than this one: more than any trim may take up, so
    // this player falls behind half a millisecond a second, over 5 ms after 10 s.
    let a = song(0.0, 80.0, BASES[0], true, true);
    let host = HostTime {
        t0_ns: 1_000_000_000,
        h0_us: 1_000_000.0,
        ppm: 1_000.0,
    };
    let mut r = Room::with(SimConfig::default(), host, SyncMode::EveryDevice);
    r.start(1, &a, BASES[0], 0, 400);
    r.pass(30_000);
    assert!(!r.h().status().cant_hold, "not yet: {:?}", r.h().status());
    r.pass(12_000);
    let st = r.h().status();
    assert!(st.cant_hold, "{st:?}");
    r.pass(1_500);
    assert_eq!(r.h().status().entry, None, "faded out");
    // The person chooses apart: back in at the room's place.
    r.h().set_mode(SyncMode::Apart);
    r.pass(2_000);
    let st = r.h().status();
    assert!(!st.cant_hold && st.entry == Some(EntryId(1)), "{st:?}");
    let heard = r.finish();
    assert_music_clean("can't hold", &heard);
    assert!(has(
        &heard.notes,
        |n| matches!(n, RenderNote::Faded { from, to, kind: FadeKind::Out, .. }
        if to - from == ms(1_000))
    ));
}
