//! The sync accuracy gates of section 7.2 (simulation), for room playback in engine 2 (WP5):
//! a guest whose clock map is WP6's `Clock` fed by simulated pings over one Wi-Fi or the
//! internet, with the host's clock and the guest's sound card each running their own rate. The
//! truth is the counting ramp each song carries, read off the simulated DAC at the exact moment
//! it was heard and set against the room's timeline at the host's true time.
//!
//! Gates: every device on one Wi-Fi p95 within 1 ms and never over 3 ms after the first 5 s;
//! apart on one Wi-Fi p95 within 5 ms; the internet direct (20 ms up, 40 ms down, 10 ms jitter,
//! 1% loss) p95 within the path asymmetry / 2 + 5 ms; sound cards ±300 ppm; in step again after
//! a join, a seek, a skip and a song change; zero clicks by every detector; a stall suspends
//! one person only.

mod room_kit;

use player::decode::{Download, Feeder, OpenInfo, Source, Streams};
use player::engine::Item;
use player::sim::SimConfig;
use player::voice_api::RenderNote;
use room_kit::*;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use together::align::Alignment;
use together::anchor::{EntryId, PrepId, Transition};
use together::clock::Steps;
use together::room_player::{RoomPlayer, SubState, Suspension, SyncMode};
use together::time::{MonoClock, SimClock};

const EXACT: Alignment = Alignment::EXACT;

/// A guest on `path`: the host's clock `host_ppm` fast and 7,000 s ahead of the guest's, the
/// guest's card `dac_ppm` fast. The room plays A from its start, then (at its armed boundary)
/// B, gapless. What the guest heard, the room's timeline, the host's clock, and when A began.
struct Run {
    heard: Heard,
    tl: Timeline,
    host: HostTime,
    start_ns: f64,
    notes_ok: bool,
}

fn guest_run(path: Path, mode: SyncMode, host_ppm: f64, dac_ppm: f64, secs: u64, seed: u64) -> Run {
    let clock = SimClock::new(1_000_000_000);
    let host = HostTime {
        t0_ns: 1_000_000_000,
        h0_us: 7_000_000_000.0,
        ppm: host_ppm,
    };
    let steps = match mode {
        SyncMode::EveryDevice => Steps::SameRoom,
        SyncMode::Apart => Steps::Apart,
    };
    let mut pinger = Pinger::new(host, path, steps, clock.now_ns(), seed);
    let mut p: Person<PathBuf> = Person::new(
        &clock,
        SimConfig {
            skew_ppm: dac_ppm,
            ..SimConfig::default()
        },
    );
    let h = RoomPlayer::attach(p.player(), pinger.handle.clone(), mode);
    let a_len = secs as f64 * 0.6;
    let a = song(0.0, a_len, BASES[0], true, false);
    let b = song(a_len, secs as f64, BASES[1], false, true);
    // The clock gets ready (a burst), then the room prepares A and starts it.
    pass(&clock, 1_500, &mut [&mut pinger]);
    h.prepare(PrepId(1), EntryId(1), a, 0, EXACT);
    pass(&clock, 500, &mut [&mut pinger]);
    let start = host.of(clock.now_ns() as f64) as i64 + 500_000;
    let mut tl = Timeline::default();
    let a0 = anchor(1, 0, start);
    tl.set(start - 500_000, a0, BASES[0]);
    h.commit(a0);
    let boundary = start + (a_len * 1e6) as i64;
    pass(&clock, 3_000, &mut [&mut pinger]);
    h.arm_next(EntryId(2), b, boundary, Transition::Gapless, EXACT);
    tl.set(boundary, anchor(2, 0, boundary), BASES[1]);
    let start_ns = clock.now_ns() as f64 - 3e9 + 5e8;
    pass(&clock, secs * 1_000, &mut [&mut pinger]);
    let st = h.status();
    let notes_ok = st.entry == Some(EntryId(2)) && st.bound_us.is_some();
    h.detach();
    pass(&clock, 1_500, &mut [&mut pinger]);
    Run {
        heard: p.finish(),
        tl,
        host,
        start_ns,
        notes_ok,
    }
}

/// The errors from 5 s after the start to the end: (p95, max) in ms.
fn judged(name: &str, r: &Run) -> (f64, f64) {
    assert!(
        r.notes_ok,
        "{name}: on to the next song, with a clock bound"
    );
    assert_music_clean(name, &r.heard);
    let e = errors(&r.heard, &r.tl, &r.host, r.start_ns + 5e9, f64::MAX);
    assert!(e.len() > 1_000, "{name}: judged {}", e.len());
    let (p95, max) = (quantile(&e, 0.95), worst(&e));
    println!(
        "{name}: p95 {p95:.3} ms, max {max:.3} ms over {} points",
        e.len()
    );
    (p95, max)
}

#[test]
fn every_device_on_one_wifi_is_within_a_millisecond() {
    for (seed, host_ppm, dac_ppm) in [(1, 40.0, -60.0), (2, -80.0, 300.0), (3, 0.0, -300.0)] {
        let r = guest_run(wifi(), SyncMode::EveryDevice, host_ppm, dac_ppm, 60, seed);
        let name = format!("every device, Wi-Fi, host {host_ppm} ppm, card {dac_ppm} ppm");
        let (p95, max) = judged(&name, &r);
        assert!(p95 <= 1.0, "{name}: p95 {p95} ms");
        assert!(max <= 3.0, "{name}: max {max} ms");
        assert_eq!(
            count(&r.heard.notes, |n| matches!(
                n,
                RenderNote::Faded {
                    kind: player::voice_api::FadeKind::CrossIn,
                    ..
                }
            )),
            0,
            "{name}: never a realign"
        );
    }
}

#[test]
fn apart_on_one_wifi_is_within_five_milliseconds() {
    for (seed, host_ppm, dac_ppm) in [(4, 60.0, 100.0), (5, -40.0, -300.0)] {
        let r = guest_run(wifi(), SyncMode::Apart, host_ppm, dac_ppm, 60, seed);
        let name = format!("apart, Wi-Fi, host {host_ppm} ppm, card {dac_ppm} ppm");
        let (p95, _) = judged(&name, &r);
        assert!(p95 <= 5.0, "{name}: p95 {p95} ms");
    }
}

#[test]
fn apart_over_the_internet_is_within_half_the_asymmetry_and_five_milliseconds() {
    // 20 ms up and 40 ms down: half the asymmetry is 10 ms.
    for (seed, host_ppm, dac_ppm) in [(6, 50.0, -100.0), (7, -100.0, 300.0)] {
        let r = guest_run(internet(), SyncMode::Apart, host_ppm, dac_ppm, 60, seed);
        let name = format!("apart, internet, host {host_ppm} ppm, card {dac_ppm} ppm");
        let (p95, _) = judged(&name, &r);
        assert!(p95 <= 15.0, "{name}: p95 {p95} ms");
    }
}

// --- In step after every change ---------------------------------------------------------------

#[test]
fn joins_seeks_skips_and_song_changes_are_in_step_after_each() {
    let clock = SimClock::new(1_000_000_000);
    let host = HostTime {
        t0_ns: 1_000_000_000,
        h0_us: 3_000_000_000.0,
        ppm: 70.0,
    };
    let mut pinger = Pinger::new(host, wifi(), Steps::SameRoom, clock.now_ns(), 11);
    let mut p: Person<PathBuf> = Person::new(
        &clock,
        SimConfig {
            skew_ppm: -120.0,
            ..SimConfig::default()
        },
    );
    let h = RoomPlayer::attach(p.player(), pinger.handle.clone(), SyncMode::EveryDevice);
    let a = song(0.0, 70.0, BASES[0], true, true);
    let b = song(50.0, 20.0, BASES[1], true, false);
    let c = song(70.0, 30.0, BASES[2], false, true);
    let mut tl = Timeline::default();
    let now_h = |clock: &SimClock| host.of(clock.now_ns() as f64) as i64;
    pass(&clock, 1_500, &mut [&mut pinger]);
    // A join: the room has played A for 20 s.
    let a0 = anchor(1, 0, now_h(&clock) - 20_000_000);
    tl.set(now_h(&clock), a0, BASES[0]);
    h.prepare(
        PrepId(1),
        EntryId(1),
        a.clone(),
        a0.content_at(now_h(&clock) + 1_000_000),
        EXACT,
    );
    h.commit(a0);
    let mut checks = vec![("join", clock.now_ns() as f64 + 3e9)];
    pass(&clock, 6_000, &mut [&mut pinger]);
    // A seek to 50 s.
    h.prepare(PrepId(2), EntryId(1), a.clone(), 50_000_000, EXACT);
    pass(&clock, 400, &mut [&mut pinger]);
    let s = anchor(1, 50_000_000, now_h(&clock) + 300_000);
    tl.set(now_h(&clock), s, BASES[0]);
    h.commit(s);
    checks.push(("seek", clock.now_ns() as f64 + 1.5e9));
    pass(&clock, 5_000, &mut [&mut pinger]);
    // A skip to B.
    h.prepare(PrepId(3), EntryId(2), b.clone(), 0, EXACT);
    pass(&clock, 400, &mut [&mut pinger]);
    let k = anchor(2, 0, now_h(&clock) + 300_000);
    tl.set(now_h(&clock), k, BASES[1]);
    h.commit(k);
    checks.push(("skip", clock.now_ns() as f64 + 1.5e9));
    pass(&clock, 3_000, &mut [&mut pinger]);
    // B into C at B's end (B and C follow each other seamlessly; both exact).
    let boundary = k.host_us + 20_000_000;
    h.arm_next(EntryId(3), c, boundary, Transition::Gapless, EXACT);
    tl.set(boundary, anchor(3, 0, boundary), BASES[2]);
    let to_boundary = ((boundary - now_h(&clock)) / 1_000) as u64;
    pass(&clock, to_boundary, &mut [&mut pinger]);
    checks.push(("song change", clock.now_ns() as f64 + 1.5e9));
    pass(&clock, 5_000, &mut [&mut pinger]);
    assert_eq!(h.status().entry, Some(EntryId(3)));
    h.detach();
    pass(&clock, 1_500, &mut [&mut pinger]);
    let heard = p.finish();
    assert_music_clean("joins, seeks, skips, song changes", &heard);
    for (what, from) in checks {
        let e = errors(&heard, &tl, &host, from, from + 2e9);
        let (p95, max) = (quantile(&e, 0.95), worst(&e));
        println!("after the {what}: p95 {p95:.3} ms, max {max:.3} ms");
        assert!(e.len() > 100, "{what}: judged");
        assert!(
            p95 <= 1.0 && max <= 3.0,
            "after the {what}: p95 {p95} ms, max {max} ms"
        );
    }
}

// --- The sine lock -----------------------------------------------------------------------------

#[test]
fn the_sine_lock_hears_nothing_through_drift_seeks_skips_and_song_changes() {
    for hz in [1_000.0, 15_000.0] {
        let clock = SimClock::new(1_000_000_000);
        let host = HostTime {
            t0_ns: 1_000_000_000,
            h0_us: 9_000_000_000.0,
            ppm: -50.0,
        };
        let mut pinger = Pinger::new(host, wifi(), Steps::SameRoom, clock.now_ns(), 21);
        let mut p: Person<PathBuf> = Person::new(
            &clock,
            SimConfig {
                skew_ppm: 300.0,
                ..SimConfig::default()
            },
        );
        let h = RoomPlayer::attach(p.player(), pinger.handle.clone(), SyncMode::EveryDevice);
        let a = sine_song(hz, 40.0);
        let b = sine_song(hz, 20.0);
        let now_h = |clock: &SimClock| host.of(clock.now_ns() as f64) as i64;
        pass(&clock, 1_500, &mut [&mut pinger]);
        h.prepare(PrepId(1), EntryId(1), a.clone(), 3_000_000, EXACT);
        pass(&clock, 400, &mut [&mut pinger]);
        h.commit(anchor(1, 3_000_000, now_h(&clock) + 300_000));
        pass(&clock, 8_000, &mut [&mut pinger]);
        h.prepare(PrepId(2), EntryId(1), a.clone(), 20_000_000, EXACT);
        pass(&clock, 400, &mut [&mut pinger]);
        h.commit(anchor(1, 20_000_000, now_h(&clock) + 300_000));
        pass(&clock, 4_000, &mut [&mut pinger]);
        h.prepare(PrepId(3), EntryId(2), b.clone(), 2_000_000, EXACT);
        pass(&clock, 400, &mut [&mut pinger]);
        let k = anchor(2, 2_000_000, now_h(&clock) + 300_000);
        h.commit(k);
        let boundary = k.host_us + 6_000_000;
        h.arm_next(
            EntryId(3),
            a,
            boundary,
            Transition::Crossfade { ms: 2_000 },
            EXACT,
        );
        pass(&clock, 12_000, &mut [&mut pinger]);
        h.detach();
        pass(&clock, 1_500, &mut [&mut pinger]);
        let heard = p.finish();
        assert_sine_clean(&format!("{hz} Hz"), &heard, hz);
    }
}

// --- A stall suspends one person only -----------------------------------------------------------

/// A song whose bytes the test hands over (a download fed by hand).
#[derive(Clone)]
struct Fed {
    download: Arc<Download>,
}

impl Serialize for Fed {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("fed")
    }
}

impl Item for Fed {
    fn open_on(&self, _: &Streams) -> Result<Source, String> {
        Ok(Source::on_download(self.download.clone(), Some(40_000)))
    }
    fn reopen_when_stopped(&self) -> bool {
        false
    }
}

fn fed(path: &std::path::Path) -> (Fed, Feeder, Vec<u8>) {
    let (download, feeder) = Download::fed(OpenInfo {
        mime_type: Some("audio/wav".into()),
        url_hint: "song.wav".into(),
        ..OpenInfo::default()
    });
    (Fed { download }, feeder, std::fs::read(path).unwrap())
}

#[test]
fn a_stall_suspends_the_one_who_stalled_and_nobody_else() {
    // Host and guest on one clock (the host's clock is this computer's for both: the guest's
    // map is exact), both in every device mode. The guest streams the host's file: 12 s of it
    // come at once, then nothing for 6 s, then the rest.
    let clock = SimClock::new(1_000_000_000);
    let a = song(0.0, 40.0, BASES[0], true, true);
    let (item, mut feeder, bytes) = fed(&a);
    let twelve_s = 44 + 12 * 48_000 * 6;
    feeder.append(&bytes[..twelve_s]);
    let mut host: Person<PathBuf> = Person::new(&clock, SimConfig::default());
    let mut guest: Person<Fed> = Person::new(&clock, SimConfig::default());
    let hh = RoomPlayer::attach(host.player(), identity(), SyncMode::EveryDevice);
    let gh = RoomPlayer::attach(guest.player(), identity(), SyncMode::EveryDevice);
    hh.prepare(PrepId(1), EntryId(1), a.clone(), 0, EXACT);
    gh.prepare(PrepId(1), EntryId(1), item, 0, EXACT);
    pass(&clock, 500, &mut []);
    assert_eq!(
        gh.status().ready,
        Some(PrepId(1)),
        "the guest is ready: 3 s decoded, 10 s of bytes"
    );
    let a0 = anchor(1, 0, host_now(&clock) + 300_000);
    let mut tl = Timeline::default();
    tl.set(a0.host_us, a0, BASES[0]);
    hh.commit(a0);
    gh.commit(a0);
    // Past the 12 s the guest has: it runs dry, alone.
    let mut suspended_at = None;
    for _ in 0..14_000 {
        pass(&clock, 1, &mut []);
        if gh.status().sub_state == SubState::Suspended(Suspension::Buffering) {
            suspended_at.get_or_insert(clock.now_ns());
        }
        if host_now(&clock) > a0.host_us + 1_000_000 {
            assert_eq!(hh.status().sub_state, SubState::InStep, "the host plays on");
        }
    }
    let suspended_at = suspended_at.expect("the guest suspended");
    pass(&clock, 2_000, &mut []);
    // The rest comes: the guest is back within a second of 5 s refilled, in step.
    let back_from = clock.now_ns();
    feeder.append(&bytes[twelve_s..]);
    feeder.finish();
    let mut back_at = None;
    for _ in 0..4_000 {
        pass(&clock, 1, &mut []);
        if gh.status().sub_state == SubState::InStep {
            back_at.get_or_insert(clock.now_ns());
        }
        assert_eq!(hh.status().sub_state, SubState::InStep, "the host plays on");
    }
    let back_at = back_at.expect("the guest came back");
    assert!(back_at > suspended_at);
    assert!(
        back_at - back_from < 2_000_000_000,
        "back soon after the bytes came: {} ms",
        (back_at - back_from) / 1_000_000
    );
    pass(&clock, 3_000, &mut []);
    hh.detach();
    gh.detach();
    pass(&clock, 1_500, &mut []);
    let (hh_heard, gh_heard) = (host.finish(), guest.finish());
    assert_music_clean("host", &hh_heard);
    assert_music_clean("guest", &gh_heard);
    // The host: in step throughout; one start, no other fade before the end.
    let e = errors(
        &hh_heard,
        &tl,
        &HostTime::same(),
        a0.host_us as f64 * 1e3 + 1e9,
        back_at as f64 + 3e9,
    );
    assert!(worst(&e) < 0.1, "the host never moved: {}", worst(&e));
    assert!(
        !has(&hh_heard.notes, |n| matches!(n, RenderNote::Starved { .. })),
        "the host never ran dry"
    );
    // The guest: it ran dry once, faded in again over 300 ms, then in step.
    assert!(has(&gh_heard.notes, |n| matches!(
        n,
        RenderNote::Starved { .. }
    )));
    let s: Vec<u32> = gh_heard
        .notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Started { fade, .. } => Some(fade),
            _ => None,
        })
        .collect();
    assert_eq!(
        s.last().copied(),
        Some(ms(300) as u32),
        "a 300 ms re-entry: {s:?}"
    );
    let e = errors(
        &gh_heard,
        &tl,
        &HostTime::same(),
        back_at as f64 + 5e8,
        back_at as f64 + 3e9,
    );
    assert!(
        !e.is_empty() && worst(&e) < 0.1,
        "the guest in step again: {}",
        worst(&e)
    );
    let _ = guest.status();
}
