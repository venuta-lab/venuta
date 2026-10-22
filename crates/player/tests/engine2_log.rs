//! Engine 2 writes the render deadline line to the playback log once a minute and when its
//! output stops (sections 4.7 and 7.5), in WP14's format (`deadline_probe log` reads it): every
//! period counted, in 100 buckets of 1% of the period and late ones apart, with the output's
//! `Xrun`s of the stretch.

mod engine_kit;

use engine_kit::*;
use player::engine::Cmd;
use std::path::PathBuf;
use std::sync::Mutex;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A line's (seconds, periods, ran dry, late, buckets).
fn parse(line: &str) -> (u64, u64, u64, u64, Vec<u64>) {
    let rest = line
        .strip_prefix("render: deadline margin over ")
        .expect("the line's start");
    let (secs, rest) = rest.split_once(" s: ").unwrap();
    let (periods, rest) = rest.split_once(" periods, ran dry ").unwrap();
    let (dry, rest) = rest.split_once(", late ").unwrap();
    let (late, rest) = rest.split_once("; per 1% of the period from 0%: ").unwrap();
    let buckets: Vec<u64> = rest.split(' ').map(|c| c.parse().unwrap()).collect();
    (
        secs.parse().unwrap(),
        periods.parse().unwrap(),
        dry.parse().unwrap(),
        late.parse().unwrap(),
        buckets,
    )
}

#[test]
fn the_deadline_line_comes_once_a_minute_and_when_the_output_stops() {
    media_access::set_timing_log(|l| LINES.lock().unwrap().push(l.to_owned()));
    let long = song(0.0, 75.0, true, true);
    let mut r: Rig<PathBuf> = Rig::new();
    r.send(play(vec![long]));
    r.pass(65_000);
    r.send(Cmd::Pause);
    r.pass(1_000);
    r.finish();
    let lines: Vec<String> = LINES
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("render: "))
        .cloned()
        .collect();
    assert_eq!(lines.len(), 2, "a minute's line and the stop's: {lines:?}");
    let mut total = 0;
    for l in &lines {
        let (_, periods, dry, late, buckets) = parse(l);
        assert_eq!(buckets.len(), 100);
        assert_eq!(buckets.iter().sum::<u64>() + late, periods);
        assert_eq!(dry, 0, "the simulated DAC never runs dry");
        total += periods;
    }
    let (secs, periods, ..) = parse(&lines[0]);
    assert_eq!(secs, 60);
    // 10 ms periods: about 100 a second.
    assert!(
        (5_900..=6_100).contains(&periods),
        "{periods} periods a minute"
    );
    assert!((6_400..=6_700).contains(&total), "{total} periods in all");
}
