//! The learned drift remembered per output (design 4.11 point 4, the WP15 notes in 9.2): a
//! room measures its output's sound card against the monotonic clock; when it lets the output
//! go, the rate is remembered by the output's key, and the next room on that output starts
//! from it instead of from nothing while its own measure is still short.

mod room_kit;

use player::drift_memory::DriftMemory;
use player::sim::SimConfig;
use room_kit::*;
use std::path::PathBuf;
use std::sync::Arc;
use together::align::Alignment;
use together::anchor::{EntryId, PrepId};
use together::clock_map::{self, ClockMap};
use together::room_player::{RoomHandle, RoomPlayer, SyncMode};
use together::time::{MonoClock, SimClock};

/// The simulated outputs have no endpoint id: they are known by their name.
const KEY: &str = "name:Simulated output";

/// A room on `p`'s output for `secs`: a song from its start, then let go. The rate the room
/// asked 1 s after the start.
fn room(clock: &SimClock, p: &Person<PathBuf>, secs: u64) -> f64 {
    let map = clock_map::handle();
    map.store(Arc::new(Some(ClockMap::identity())));
    let h: Box<dyn RoomHandle<PathBuf>> = RoomPlayer::attach(p.player(), map, SyncMode::Apart);
    let file = song(0.0, secs as f64 + 5.0, BASES[0], true, true);
    h.prepare(PrepId(1), EntryId(1), file, 0, Alignment::EXACT);
    pass(clock, 1_000, &mut []);
    let start = (clock.now_ns() / 1_000) as i64 + 500_000;
    h.commit(anchor(1, 0, start));
    pass(clock, 1_500, &mut []);
    let early = h.status().ppm;
    pass(clock, secs * 1_000, &mut []);
    h.detach();
    pass(clock, 1_500, &mut []);
    early
}

#[test]
fn a_rooms_output_drift_is_remembered_and_the_next_room_starts_from_it() {
    let clock = SimClock::new(1_000_000_000);
    let mut p: Person<PathBuf> = Person::new(
        &clock,
        SimConfig {
            skew_ppm: 120.0,
            ..SimConfig::default()
        },
    );
    let mem = Arc::new(DriftMemory::in_memory());
    p.player().set_drift_memory(mem.clone());
    let first = room(&clock, &p, 30);
    assert!(first.abs() < 20.0, "nothing known yet: {first} ppm");
    let learned = mem.recall(KEY).expect("remembered when the room let go");
    assert!(
        (learned - 120.0).abs() < 3.0,
        "the card's 120 ppm: {learned}"
    );
    let second = room(&clock, &p, 10);
    println!("first room {first:.1} ppm, learned {learned:.2} ppm, next room {second:.1} ppm");
    assert!(
        (second + 120.0).abs() < 10.0,
        "the next room plays 120 ppm slower from its first second: {second}"
    );
    let _ = p.finish();
}

#[test]
fn a_room_too_short_to_measure_remembers_nothing() {
    let clock = SimClock::new(1_000_000_000);
    let mut p: Person<PathBuf> = Person::new(
        &clock,
        SimConfig {
            skew_ppm: -80.0,
            ..SimConfig::default()
        },
    );
    let mem = Arc::new(DriftMemory::in_memory());
    p.player().set_drift_memory(mem.clone());
    let _ = room(&clock, &p, 3);
    assert_eq!(mem.recall(KEY), None, "under 8 s of pairs is no measure");
    let _ = p.finish();
}
