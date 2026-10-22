//! The simulated output keeps time exactly as its model says (contract 5.2): the frame at the
//! DAC at any moment is within 1 µs of `start + frame / (rate x (1 + skew))`, whatever the wake
//! jitter; skew, coarse positions, missed periods, a held buffer and a lost device all behave
//! as a real device would. And the output conformance suite passes on it, and catches an output
//! that plays stale frames after a restart.

use player::conformance::{self, ToneRender};
use player::output_api::{OpenRequest, Output, OutputEvent, Render};
use player::sim::{SimConfig, SimOutput};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use together::time::{MonoClock, RealClock, SimClock, Timebase};

const RATE: u32 = 48_000;
const MS: u64 = 1_000_000;

fn want() -> OpenRequest {
    OpenRequest {
        rate: RATE,
        channels: 2,
        bits: 24,
    }
}

/// A renderer that writes each frame's own device frame number (low 24 bits, both channels):
/// what was heard then says exactly which frame it was.
struct Counter {
    calls: Arc<AtomicU64>,
    next: Arc<AtomicU64>,
    broken: Arc<AtomicU64>,
}

impl Render for Counter {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        if at != self.next.load(Ordering::SeqCst) {
            self.broken.fetch_add(1, Ordering::SeqCst);
        }
        for (k, f) in out
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .take(frames as usize)
            .enumerate()
        {
            let v = (((at + k as u64 + 1) & 0x7F_FFFF) as i32) << 8;
            f[..4].copy_from_slice(&v.to_le_bytes());
            f[4..].copy_from_slice(&v.to_le_bytes());
        }
        self.next.store(at + u64::from(frames), Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn counter() -> (Box<Counter>, Arc<AtomicU64>, Arc<AtomicU64>, Arc<AtomicU64>) {
    let (calls, next, broken) = (Arc::default(), Arc::default(), Arc::default());
    (
        Box::new(Counter {
            calls: Arc::clone(&calls),
            next: Arc::clone(&next),
            broken: Arc::clone(&broken),
        }),
        calls,
        next,
        broken,
    )
}

/// When the model says device frame `frame` is at the DAC.
fn model_ns(start_ns: u64, frame: u64, skew_ppm: f64) -> f64 {
    start_ns as f64 + frame as f64 * 1e9 / (f64::from(RATE) * (1.0 + skew_ppm / 1e6))
}

#[test]
fn presentation_is_within_a_microsecond_of_the_model_with_skew_and_jitter() {
    for (skew, jitter_us) in [(0.0, 0), (100.0, 3_000), (-300.0, 1_000), (480.0, 2_500)] {
        let clock = SimClock::new(5_000 * MS);
        let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
        let (mut out, sim) = SimOutput::new(
            tb,
            SimConfig {
                skew_ppm: skew,
                wake_jitter_us: jitter_us,
                ..SimConfig::default()
            },
        );
        out.open(&want()).unwrap();
        let (r, _, next, broken) = counter();
        out.start(r).unwrap();
        let start_ns = clock.now_ns();
        let p = out.presentation();
        let mut worst = 0.0f64;
        let mut readings = 0;
        for _ in 0..1_000 {
            clock.pass(10 * MS);
            let pair = p.read().expect("a pair once playing");
            worst = worst.max((pair.time_ns as f64 - model_ns(start_ns, pair.frame, skew)).abs());
            readings += 1;
        }
        assert!(
            worst < 1_000.0,
            "skew {skew}, jitter {jitter_us} µs: {worst:.1} ns off"
        );
        assert_eq!(readings, 1_000);
        assert_eq!(broken.load(Ordering::SeqCst), 0, "render's `at` contiguous");
        // The skew is applied: 10 s of this DAC's clock is 10 s x (1 + skew) of frames.
        let played = sim.tape().played_frames();
        let expected = 10.0 * f64::from(RATE) * (1.0 + skew / 1e6);
        assert!(
            (played as f64 - expected).abs() <= 2.0,
            "skew {skew}: played {played}, expected {expected:.1}"
        );
        // It renders ahead by its buffer, never further.
        let ahead = next.load(Ordering::SeqCst) - played;
        assert!(
            ahead <= u64::from(sim.info().unwrap().buffer_frames),
            "{ahead} ahead"
        );
        out.stop();
        out.close();
    }
}

#[test]
fn what_was_heard_is_what_was_rendered_in_order_with_when() {
    let clock = SimClock::new(0);
    let (mut out, sim) = SimOutput::new(Arc::new(clock.clone()), SimConfig::default());
    out.open(&want()).unwrap();
    let (r, ..) = counter();
    out.start(r).unwrap();
    let start_ns = clock.now_ns();
    clock.pass(2_000 * MS);
    let tape = sim.tape();
    let run = &tape.runs[0];
    let left = run.channel(0);
    assert!(left.len() >= 95_000);
    for (k, v) in left.iter().enumerate() {
        assert_eq!(*v, ((k as u64 + 1) & 0x7F_FFFF) as i32, "frame {k}");
    }
    assert!(run.dry.is_empty());
    assert!((run.heard_ns(48_000) - model_ns(start_ns, 48_000, 0.0)).abs() < 1.0);
}

#[test]
fn coarse_positions_step_and_lag_by_at_most_one_step() {
    let clock = SimClock::new(0);
    let (mut out, _) = SimOutput::new(
        Arc::new(clock.clone()),
        SimConfig {
            position_step_us: 10_000,
            ..SimConfig::default()
        },
    );
    out.open(&want()).unwrap();
    let (r, ..) = counter();
    out.start(r).unwrap();
    let start_ns = clock.now_ns();
    let p = out.presentation();
    for _ in 0..300 {
        clock.pass(7 * MS);
        let pair = p.read().unwrap();
        assert_eq!(pair.frame % 480, 0, "a 10 ms step");
        let truth = (pair.time_ns - start_ns) as f64 * f64::from(RATE) / 1e9;
        let behind = truth - pair.frame as f64;
        assert!((0.0..=481.0).contains(&behind), "{behind} frames behind");
    }
}

#[test]
fn a_missed_period_runs_the_dac_dry_and_says_so() {
    let clock = SimClock::new(0);
    let (mut out, sim) = SimOutput::new(
        Arc::new(clock.clone()),
        SimConfig {
            // Two wakes in a row missed, with only two periods of buffer: the DAC runs out.
            buffer_periods: 2,
            missed: vec![50, 51],
            ..SimConfig::default()
        },
    );
    out.open(&want()).unwrap();
    let events = out.events();
    let (r, _, _, broken) = counter();
    out.start(r).unwrap();
    clock.pass(1_000 * MS);
    let tape = sim.tape();
    let run = &tape.runs[0];
    assert!(!run.dry.is_empty(), "the DAC ran dry");
    let gap: u64 = run.dry.iter().map(|d| d.end - d.start).sum();
    assert!(gap > 0 && gap <= 480 * 2, "{gap} frames of silence");
    assert!(
        events
            .try_iter()
            .any(|e| matches!(e, OutputEvent::Xrun { .. })),
        "an xrun event"
    );
    // Nothing was lost: every frame rendered still played, in order, after the gap.
    let heard: Vec<i32> = run.channel(0).into_iter().filter(|&v| v != 0).collect();
    assert!(heard.windows(2).all(|w| w[1] == w[0] + 1));
    assert_eq!(broken.load(Ordering::SeqCst), 0);
}

#[test]
fn a_lost_device_stops_rendering_and_says_so() {
    let clock = SimClock::new(0);
    let (mut out, sim) = SimOutput::new(Arc::new(clock.clone()), SimConfig::default());
    out.open(&want()).unwrap();
    let events = out.events();
    let (r, calls, ..) = counter();
    out.start(r).unwrap();
    clock.pass(200 * MS);
    sim.lose();
    clock.pass(20 * MS);
    let n = calls.load(Ordering::SeqCst);
    clock.pass(500 * MS);
    assert_eq!(calls.load(Ordering::SeqCst), n, "render no longer called");
    assert!(events.try_iter().any(|e| e == OutputEvent::Lost));
    assert!(out.start(counter().0).is_err(), "can't start a lost device");
}

#[test]
fn stop_plays_out_what_it_holds_and_a_held_buffer_plays_on_after_a_restart() {
    for held in [false, true] {
        let clock = SimClock::new(0);
        let (mut out, sim) = SimOutput::new(
            Arc::new(clock.clone()),
            SimConfig {
                held_buffer: held,
                ..SimConfig::default()
            },
        );
        out.open(&want()).unwrap();
        let (r, ..) = counter();
        out.start(r).unwrap();
        clock.pass(500 * MS);
        let rendered = sim.rendered_frames();
        assert!(out.stop().is_some(), "the renderer comes back");
        // Start again at once: frames 0.. again, and nothing of before, unless the buffer is
        // held (the fault the conformance suite must catch).
        let (r, ..) = counter();
        out.start(r).unwrap();
        clock.pass(500 * MS);
        let tape = sim.tape();
        let first = tape.runs[0].channel(0);
        let second = tape.runs[1].channel(0);
        if held {
            assert!(second[0] > 1_000, "stale frames first: {}", second[0]);
        } else {
            assert_eq!(second[0], 1, "the new start's frame 0 first");
            assert!(first.len() as u64 <= rendered);
        }
    }
    // Stopped for good: what it held plays out, then the DAC stops.
    let clock = SimClock::new(0);
    let (mut out, sim) = SimOutput::new(Arc::new(clock.clone()), SimConfig::default());
    out.open(&want()).unwrap();
    out.start(counter().0).unwrap();
    clock.pass(300 * MS);
    let rendered = sim.rendered_frames();
    out.stop();
    clock.pass(300 * MS);
    assert_eq!(
        sim.tape().played_frames(),
        rendered,
        "every frame rendered was played"
    );
}

#[test]
fn it_runs_in_real_time_too() {
    let tb: Arc<dyn Timebase> = Arc::new(RealClock);
    let (mut out, sim) = SimOutput::new(tb, SimConfig::default());
    out.open(&want()).unwrap();
    out.start(counter().0).unwrap();
    let start = RealClock.now_ns();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let p = out.presentation().read().unwrap();
    // Real time on a busy machine may run the DAC dry: the model counts each underrun.
    let run = &sim.tape().runs[0];
    let off = p.time_ns as f64 - run.heard_ns(run.tick_of(p.frame));
    assert!(off.abs() < 1_000.0, "{off} ns off the model");
    assert!(p.time_ns >= start);
    out.stop();
    out.close();
}

#[test]
fn the_conformance_suite_passes_on_the_simulated_output() {
    let clock = SimClock::new(1_000 * MS);
    let (mut out, sim) = SimOutput::new(
        Arc::new(clock.clone()),
        SimConfig {
            skew_ppm: 80.0,
            wake_jitter_us: 2_000,
            ..SimConfig::default()
        },
    );
    let report = conformance::run(&mut out, &clock, &want()).expect("conformance");
    assert!(report.rendered >= 10 * u64::from(RATE));
    // What it played: the tone clean by the sine lock, silence before the stop, and each
    // restart from its own frame 0 (nothing stale).
    let tape = sim.tape();
    conformance::check_tape(&tape, &report).expect("the tape");
    for (run, &(freq, tail_from, _)) in tape.runs.iter().zip(&report.tones) {
        let x = run.channel_f64(0);
        let lock = audio_check::sine::SineLock::new(freq, RATE, 0.5);
        let fade = audio_check::Declared::span(tail_from as usize..x.len());
        assert_eq!(audio_check::sine::check(&x, &lock, &fade), [], "{freq} Hz");
    }
}

#[test]
fn the_conformance_suite_catches_stale_frames_after_a_restart() {
    let clock = SimClock::new(0);
    let (mut out, sim) = SimOutput::new(
        Arc::new(clock.clone()),
        SimConfig {
            held_buffer: true,
            ..SimConfig::default()
        },
    );
    // Its position counts the stale frames, so the DAC is seen past what this start rendered;
    // and what it played begins with them.
    let err = match conformance::run(&mut out, &clock, &want()) {
        Err(e) => e,
        Ok(report) => conformance::check_tape(&sim.tape(), &report).unwrap_err(),
    };
    assert!(err.contains("stale") || err.contains("past the"), "{err}");
    let tape = sim.tape();
    let restarted = tape
        .runs
        .iter()
        .skip(1)
        .find(|r| r.stale > 0)
        .expect("a start with stale frames");
    assert!(restarted.stale > 0);
}

#[test]
fn the_conformance_tone_is_the_sine_it_says() {
    let fmt = player::output::DeviceFormat {
        rate: RATE,
        channels: 2,
        container_bits: 32,
        valid_bits: 24,
        kind: player::output::SampleKind::Int,
    };
    let tone = ToneRender::new(1_000.0, fmt);
    let mut r = tone.render_box();
    let mut buf = vec![0u8; 480 * 8];
    r.render(&mut buf, 480, 0);
    let s = i32::from_le_bytes(buf[8 * 12..8 * 12 + 4].try_into().unwrap());
    let want = 0.5 * (std::f64::consts::TAU * 1_000.0 * 12.0 / 48_000.0 + 0.3).sin();
    assert!((f64::from(s) / 2_147_483_648.0 - want).abs() < 1e-6);
}
