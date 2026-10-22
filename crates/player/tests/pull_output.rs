//! The output's own loop (`output::PullOutput`, what the WASAPI output runs) on the fake pull
//! device of `pull_kit`, in virtual time: the conformance suite of contract 5.2 passes on it in
//! shared and exclusive mode, and every fault the design names behaves as section 4.8 says
//! (WP1): stop returns at once and the tail plays out, a start resets, a late wake runs the DAC
//! dry once with one `Xrun` and true pairs after it, a lost or silent device is `Lost` once.

mod pull_kit;

use player::conformance;
use player::output::PullOutput;
use player::output_api::{Output, OutputError, OutputEvent};
use pull_kit::*;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use together::time::{MonoClock, RealClock, SimClock, Timebase};

// --- The conformance suite ------------------------------------------------------------------

fn conformance_on(cfg: FakeCfg) {
    let (mut out, fake, clock) = sim_output(cfg);
    let report = conformance::run(&mut out, &clock, &want()).expect("conformance");
    assert!(report.rendered >= 10 * u64::from(RATE));
    assert_eq!(report.xruns, 0, "no gap in a healthy run");
    let tape = tape(&fake, &clock);
    conformance::check_tape(&tape, &report).expect("the tape");
    for (run, &(freq, tail_from, _)) in tape.runs.iter().zip(&report.tones) {
        assert_eq!(run.stale, 0, "nothing held from before a start");
        let x = run.channel_f64(0);
        let lock = audio_check::sine::SineLock::new(freq, RATE, 0.5);
        let fade = audio_check::Declared::span(tail_from as usize..x.len());
        assert_eq!(audio_check::sine::check(&x, &lock, &fade), [], "{freq} Hz");
    }
    let f = fake.lock().unwrap();
    assert_eq!(
        f.errors,
        Vec::<String>::new(),
        "the device was used as it must be"
    );
    // Every stop let the tail reach the DAC before the device stopped (the second start comes
    // straight after a stop, so it resets at once instead).
    assert!(f.halts.len() >= 3, "{:?}", f.halts);
}

#[test]
fn the_conformance_suite_passes_on_the_pull_loop_in_shared_mode() {
    conformance_on(FakeCfg::default());
}

#[test]
fn the_conformance_suite_passes_on_the_pull_loop_in_exclusive_mode() {
    conformance_on(FakeCfg {
        exclusive: true,
        latency: 48,
        ..FakeCfg::default()
    });
}

#[test]
fn the_conformance_suite_passes_with_skew_late_wakes_inside_the_buffer_and_missing_readings() {
    conformance_on(FakeCfg {
        skew_ppm: 120.0,
        // Up to 30 ms late, every so often: the 50 ms buffer rides it out.
        late: (0..1_200).step_by(7).map(|k| (k, 30 * MS)).collect(),
        no_reading_every: 3,
        ..FakeCfg::default()
    });
}

// --- Stop, the tail, and a start ------------------------------------------------------------

#[test]
fn stop_returns_at_once_render_is_never_called_after_and_the_tail_plays_out() {
    for exclusive in [false, true] {
        let (mut out, fake, clock) = sim_output(FakeCfg {
            exclusive,
            ..FakeCfg::default()
        });
        out.open(&want()).unwrap();
        let c = Counts::default();
        out.start(c.render()).unwrap();
        clock.pass(1_000 * MS);
        let t = clock.now_ns();
        assert!(out.stop().is_some(), "stop hands the renderer back");
        assert_eq!(clock.now_ns(), t, "stop waits for nothing");
        let (calls, made) = (c.calls(), c.made());
        clock.pass(500 * MS);
        assert_eq!(
            c.calls(),
            calls,
            "render is never called after stop returned"
        );
        let f = fake.lock().unwrap();
        let h = f
            .halts
            .last()
            .expect("the device stopped once the tail played");
        assert!(
            h.raw >= made,
            "the device stopped at {} before the last frame made ({made}) reached the DAC",
            h.raw
        );
        // Every frame made was heard, in order, and the device stopped soon after the last.
        let heard: Vec<i32> = f.runs[0].left_justified(0);
        assert!(heard.len() as u64 >= made);
        for (k, &v) in heard.iter().enumerate().take(made as usize) {
            assert_eq!(v, counter_value(k as u64), "frame {k}");
        }
        let last = f.heard_frame_ns(made - 1).unwrap();
        assert!(
            (h.at_ns as f64) - last < 30.0 * MS as f64,
            "stopped {} ms after the tail",
            (h.at_ns as f64 - last) / 1e6
        );
        drop(f);
        assert_eq!(
            out.stop().map(|_| ()),
            None,
            "a second stop has nothing to give"
        );
        out.close();
    }
}

#[test]
fn a_start_right_after_a_stop_plays_nothing_held_from_before() {
    for exclusive in [false, true] {
        let (mut out, fake, clock) = sim_output(FakeCfg {
            exclusive,
            ..FakeCfg::default()
        });
        out.open(&want()).unwrap();
        out.start(Counts::default().render()).unwrap();
        clock.pass(300 * MS);
        out.stop().unwrap();
        let c = Counts::default();
        out.start(c.render()).unwrap();
        clock.pass(300 * MS);
        out.stop();
        clock.pass(200 * MS);
        let f = fake.lock().unwrap();
        assert_eq!(f.runs.len(), 2);
        assert_eq!(f.runs[1].stale, 0, "the start reset what the device held");
        let heard = f.runs[1].left_justified(0);
        assert_eq!(
            heard[0],
            counter_value(0),
            "the new start begins with its frame 0"
        );
        assert_eq!(f.errors, Vec::<String>::new());
        drop(f);
        assert!(out.presentation().read().is_some());
        out.close();
    }
}

#[test]
fn the_wrong_order_is_refused() {
    let (mut out, _fake, _clock) = sim_output(FakeCfg::default());
    assert!(matches!(
        out.start(Counts::default().render()),
        Err(OutputError::State(_))
    ));
    assert!(out.stop().is_none());
    out.open(&want()).unwrap();
    assert!(matches!(out.open(&want()), Err(OutputError::State(_))));
    out.start(Counts::default().render()).unwrap();
    assert!(matches!(
        out.start(Counts::default().render()),
        Err(OutputError::State(_))
    ));
    out.stop().unwrap();
    out.close();
    out.close();
    out.open(&want()).unwrap();
    out.close();
}

// --- Running dry ----------------------------------------------------------------------------

/// For `steps` periods of 10 ms: every published pair says when its frame was heard (within one
/// frame), the presentation never goes back, and it keeps moving (pairs are held back while a
/// suspected gap settles, about a quarter of a second).
fn pairs_are_true(
    out: &PullOutput<FakeOpener>,
    fake: &Arc<Mutex<Fake>>,
    clock: &SimClock,
    steps: u32,
) {
    let mut last = None;
    let mut fresh = 0;
    for _ in 0..steps {
        clock.pass(10 * MS);
        let Some(p) = out.presentation().read() else {
            continue;
        };
        let f = fake.lock().unwrap();
        let heard = f
            .heard_frame_ns(p.frame)
            .unwrap_or_else(|| panic!("frame {} at the DAC was never one of ours", p.frame));
        let off = p.time_ns as f64 - heard;
        assert!(
            off.abs() <= 1e9 / f64::from(RATE) + 1.0,
            "frame {} said at {} but heard at {heard:.0} ({off:.0} ns)",
            p.frame,
            p.time_ns
        );
        if let Some(l) = last {
            assert!(p.frame >= l, "the presentation went back");
            if p.frame > l {
                fresh += 1;
            }
        }
        last = Some(p.frame);
    }
    assert!(
        fresh * 10 > steps * 8,
        "only {fresh} new pairs in {steps} periods"
    );
}

#[test]
fn a_late_wake_in_shared_mode_runs_the_dac_dry_says_xrun_once_and_keeps_the_pairs_true() {
    let (mut out, fake, clock) = sim_output(FakeCfg {
        // 80 ms late for one event, 1 s in: the 50 ms buffer runs out.
        late: vec![(100, 80 * MS)],
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    pairs_are_true(&out, &fake, &clock, 300);
    let gap_at = fake.lock().unwrap().gap_at.expect("the fake ran dry");
    let got = events
        .recv_timeout(Duration::from_secs(5))
        .expect("an xrun");
    assert_eq!(got, OutputEvent::Xrun { at: gap_at });
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(events.try_iter().count(), 0, "one xrun for one dry stretch");
    out.stop();
    out.close();
}

#[test]
fn a_late_wake_in_exclusive_mode_replays_a_buffer_says_xrun_and_the_slip_is_taken_off() {
    let (mut out, fake, clock) = sim_output(FakeCfg {
        exclusive: true,
        latency: 48,
        // 15 ms late with a 10 ms buffer, 1 s in: the hardware plays one half again.
        late: vec![(100, 15 * MS)],
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    pairs_are_true(&out, &fake, &clock, 300);
    {
        let f = fake.lock().unwrap();
        assert_eq!(
            f.gaps.iter().map(|g| g.end - g.start).sum::<u64>(),
            480,
            "one stale buffer"
        );
    }
    let gap_at = fake.lock().unwrap().gap_at.unwrap();
    let got = events
        .recv_timeout(Duration::from_secs(5))
        .expect("an xrun");
    assert_eq!(got, OutputEvent::Xrun { at: gap_at });
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(events.try_iter().count(), 0);
    out.stop();
    out.close();
}

// --- Lost devices ---------------------------------------------------------------------------

#[test]
fn a_lost_device_says_lost_once_stops_rendering_and_refuses_a_start() {
    let (mut out, _fake, clock) = sim_output(FakeCfg {
        lost_after: Some(500 * MS),
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    let c = Counts::default();
    out.start(c.render()).unwrap();
    clock.pass(1_000 * MS);
    let calls = c.calls();
    clock.pass(500 * MS);
    assert_eq!(c.calls(), calls, "render is no longer called");
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)),
        Ok(OutputEvent::Lost)
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(events.try_iter().count(), 0, "lost is said once");
    assert!(out.stop().is_some(), "the renderer comes back");
    assert_eq!(
        out.start(Counts::default().render()),
        Err(OutputError::Lost)
    );
    out.close();
    // A new open is a new device.
    out.open(&want()).unwrap();
    let c = Counts::default();
    out.start(c.render()).unwrap();
    clock.pass(200 * MS);
    assert!(c.calls() > 0);
    out.stop();
    out.close();
}

#[test]
fn a_device_that_stops_signalling_is_lost_after_2_s() {
    let (mut out, _fake, clock) = sim_output(FakeCfg {
        silent_after: Some(300 * MS),
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    clock.pass(2_200 * MS);
    assert!(events.try_recv().is_err(), "not before 2 s of silence");
    clock.pass(300 * MS);
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)),
        Ok(OutputEvent::Lost)
    );
    out.stop();
    out.close();
}

#[test]
fn events_go_to_the_newest_receiver() {
    let (mut out, _fake, clock) = sim_output(FakeCfg {
        lost_after: Some(100 * MS),
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let old = out.events();
    let new = out.events();
    out.start(Counts::default().render()).unwrap();
    clock.pass(400 * MS);
    assert_eq!(
        new.recv_timeout(Duration::from_secs(5)),
        Ok(OutputEvent::Lost)
    );
    assert!(old.try_recv().is_err());
    out.close();
}

// --- The loop as the renderer sees it -------------------------------------------------------

#[test]
fn render_is_asked_for_at_most_a_period_with_at_contiguous_from_0() {
    for exclusive in [false, true] {
        let (mut out, _fake, clock) = sim_output(FakeCfg {
            exclusive,
            late: vec![(20, 25 * MS)],
            ..FakeCfg::default()
        });
        let info = out.open(&want()).unwrap();
        let c = Counts::default();
        out.start(c.render()).unwrap();
        clock.pass(1_000 * MS);
        out.stop();
        assert_eq!(c.broken.load(Ordering::SeqCst), 0);
        assert!(c.longest.load(Ordering::SeqCst) <= u64::from(info.period_frames));
        assert!(c.made() >= u64::from(RATE));
        out.close();
    }
}

#[test]
fn it_runs_in_real_time_too() {
    let opener = FakeOpener::new(FakeCfg::default(), SimOrReal::Real);
    let fake = opener.fake.clone();
    let mut out = PullOutput::new(opener, Arc::new(RealClock));
    out.open(&want()).unwrap();
    let c = Counts::default();
    out.start(c.render()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(out.stop().is_some());
    assert!(c.made() > 0);
    std::thread::sleep(Duration::from_millis(200));
    out.close();
    assert!(!fake.lock().unwrap().halts.is_empty());
}

#[test]
fn exclusive_mode_counts_two_buffers_played_again_and_a_second_gap() {
    let (mut out, fake, clock) = sim_output(FakeCfg {
        exclusive: true,
        latency: 48,
        // 25 ms late (two halves played again), then 12 ms late (one).
        late: vec![(100, 25 * MS), (200, 12 * MS)],
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    pairs_are_true(&out, &fake, &clock, 400);
    assert_eq!(
        fake.lock()
            .unwrap()
            .gaps
            .iter()
            .map(|g| g.end - g.start)
            .sum::<u64>(),
        3 * 480
    );
    std::thread::sleep(Duration::from_millis(300));
    let xruns = events
        .try_iter()
        .filter(|e| matches!(e, OutputEvent::Xrun { .. }))
        .count();
    assert_eq!(xruns, 2, "one xrun for each gap");
    out.stop();
    out.close();
}

/// What the owner's laptop showed (shared mode, Realtek with Nahimic): the engine reads a burst
/// at a start, filling its pipeline, so at the first wake the device holds less than a period
/// with no gap at all. That is no xrun; the one late period is one.
#[test]
fn a_start_whose_engine_reads_a_burst_is_no_xrun_and_a_late_period_is_one() {
    let (mut out, fake, clock) = sim_output(FakeCfg {
        // 2400 handed over before the start; the engine takes 1500 at once and 480 at its
        // first pass: 420 held at the first wake. Then a render call 150 ms late, 2 s in.
        prime: 1_500,
        late: vec![(200, 150 * MS)],
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    pairs_are_true(&out, &fake, &clock, 400);
    let f = fake.lock().unwrap();
    assert_eq!(
        f.gaps.len(),
        1,
        "one gap, the late period's, none at the start"
    );
    let gap_at = f.gap_at.expect("the late period ran the DAC dry");
    drop(f);
    std::thread::sleep(Duration::from_millis(300));
    let seen: Vec<OutputEvent> = events.try_iter().collect();
    assert_eq!(
        seen,
        [OutputEvent::Xrun { at: gap_at }],
        "no xrun at the start"
    );
    out.stop();
    out.close();
}

/// A position that counts the silence of a pass that found part of a period as the stream's
/// frames (the other way a shared stream's position can behave in a gap): the slip is measured
/// from the engine's pipeline and taken off, so the pairs stay true after the gap.
#[test]
fn silence_the_position_counts_in_a_gap_is_taken_off() {
    let (mut out, fake, clock) = sim_output(FakeCfg {
        // A buffer that isn't whole periods, so the last pass before the gap finds part of one.
        buffer: 2_200,
        counts_partial: true,
        late: vec![(100, 80 * MS)],
        ..FakeCfg::default()
    });
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    pairs_are_true(&out, &fake, &clock, 400);
    {
        let f = fake.lock().unwrap();
        let gap: u64 = f.gaps.iter().map(|g| g.end - g.start).sum();
        let uncounted: u64 = f.uncounted.iter().map(|g| g.end - g.start).sum();
        assert!(gap > uncounted, "the position counted some silence");
    }
    std::thread::sleep(Duration::from_millis(300));
    let xruns = events
        .try_iter()
        .filter(|e| matches!(e, OutputEvent::Xrun { .. }))
        .count();
    assert_eq!(xruns, 1);
    out.stop();
    out.close();
}

#[test]
fn a_trace_has_a_row_for_every_device_event_with_the_decisions() {
    let (mut out, fake, clock) = sim_output(FakeCfg {
        late: vec![(100, 80 * MS)],
        ..FakeCfg::default()
    });
    let trace = out.trace(1_000);
    out.open(&want()).unwrap();
    let events = out.events();
    out.start(Counts::default().render()).unwrap();
    clock.pass(3_000 * MS);
    out.stop();
    clock.pass(200 * MS);
    out.close();
    let rows = trace.rows();
    let col = |name: &str| {
        player::output::TRACE_COLUMNS
            .iter()
            .position(|c| *c == name)
            .unwrap()
    };
    // About one row per 10 ms pass; times rising; frames handed over never going back.
    assert!(rows.len() > 250 && rows.len() < 320, "{} rows", rows.len());
    for w in rows.windows(2) {
        assert!(w[1][col("wake_ns")] > w[0][col("wake_ns")]);
        assert!(w[1][col("before")] >= w[0][col("before")]);
    }
    assert!(
        rows.iter()
            .all(|r| r[col("held")] >= 0 && r[col("pos_ok")] == 1)
    );
    // The one xrun is in the trace, where the event says, and the gap where the fake had it.
    let xruns: Vec<i64> = rows
        .iter()
        .map(|r| r[col("xrun_at")])
        .filter(|&a| a >= 0)
        .collect();
    std::thread::sleep(Duration::from_millis(200));
    let said: Vec<i64> = events
        .try_iter()
        .filter_map(|e| match e {
            OutputEvent::Xrun { at } => Some(at as i64),
            _ => None,
        })
        .collect();
    assert_eq!(xruns, said);
    assert_eq!(xruns, [fake.lock().unwrap().gap_at.unwrap() as i64]);
    let csv = trace.csv();
    assert_eq!(csv.lines().count(), rows.len() + 1);
    assert!(csv.starts_with("wake_ns,filled_ns,free,held,padding,"));
}
