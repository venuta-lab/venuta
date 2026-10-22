//! The WASAPI output on real hardware (WP1's gates in section 9.2). Every test here is ignored:
//! each opens a real sound output; the two conformance runs play test tones (-6 dBFS sines
//! of 1 kHz, 600 Hz and 1.5 kHz, about 13 s in all), the others only silence. They are run by
//! hand, one at a time, after the owner has agreed, never by CI or by an agent on its own
//! (the steps are in section 4.8 of `docs/design/listen-together.md`, "Settled in WP1"):
//!
//! ```text
//! cargo test -p player --test wasapi_hw -- --ignored --test-threads 1 --nocapture <name>
//! ```
//!
//! `VENUTA_HW_OUTPUT` names the endpoint id to use (the `devices` command of `output_delay`
//! lists them); without it, the default output. For the loopback run the output's sound
//! effects must be off (Windows sound settings, the output's properties, "Audio
//! enhancements": off): the loopback is taken after them, and effects change the tone. Keep
//! the system volume low; the loopback is taken before the volume on most outputs, and the
//! check measures the level it gets.

#![cfg(windows)]

#[path = "../examples/live_kit/mod.rs"]
mod live_kit;

use live_kit::capture::{Recorder, Source};
use player::conformance;
use player::output::Mode;
use player::output_api::{OpenRequest, Output, OutputEvent, Render};
use player::wasapi_out::{WasapiOutput, wasapi_output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use together::time::{MonoClock, RealClock};

fn device() -> Option<String> {
    std::env::var("VENUTA_HW_OUTPUT")
        .ok()
        .filter(|s| !s.is_empty())
}

fn want() -> OpenRequest {
    OpenRequest {
        rate: 48_000,
        channels: 2,
        bits: 24,
    }
}

/// Bursts of sound in `x` (sample ranges), split by at least 20 ms of digital silence.
fn bursts(x: &[f32], rate: u32) -> Vec<std::ops::Range<usize>> {
    let quiet = (rate / 50) as usize;
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut i = 0;
    while i < x.len() {
        if x[i].abs() <= SILENT {
            i += 1;
            continue;
        }
        let start = i;
        let mut last = i;
        while i < x.len() && i - last < quiet {
            if x[i].abs() > SILENT {
                last = i;
            }
            i += 1;
        }
        out.push(start..last + 1);
    }
    out
}

/// Digital silence through the mixer and the loopback: -100 dBFS.
const SILENT: f32 = 1e-5;

/// The conformance suite (open; 10 s of 1 kHz with `at` contiguous; the presentation monotonic,
/// within 1 ms of the frames rendered and at the DAC's pace; the tone's fade, its silence and a
/// stop that lets the tail play; a restart from frame 0 at 600 Hz, another at 1.5 kHz straight
/// after a stop; close and open again) on WASAPI shared mode, with the output's loopback
/// recorded throughout: the three tones each clean by the sine lock, digital silence before
/// every stop and between the tones, nothing after the last. Also: the 40 to 60 ms buffer,
/// MMCSS, no xrun.
#[test]
#[ignore = "plays test tones on a real output; run by hand (see the file's notes)"]
fn wasapi_shared_passes_the_conformance_suite_with_a_clean_loopback() {
    let mut out: WasapiOutput = wasapi_output(device().as_deref(), false);
    let captured = Arc::new(Mutex::new(Vec::<f32>::new()));
    let c = captured.clone();
    let rec = Recorder::start(
        Source::Loopback(device()),
        Box::new(move |s: &[f32]| c.lock().unwrap().extend_from_slice(s)),
    )
    .expect("the loopback recorder");
    let report = conformance::run(&mut out, &RealClock, &want()).expect("conformance");
    std::thread::sleep(Duration::from_millis(500));
    let (rate, channels) = (rec.rate, rec.channels);
    rec.stop().expect("the recording");
    eprintln!("{report:?}");
    assert_eq!(out.mode(), Mode::Shared);
    assert_eq!(report.xruns, 0, "the device ran dry");
    let notes = out.notes().expect("what the device said");
    eprintln!("{notes:?}");
    assert!(notes.pro_audio, "MMCSS took the render thread");
    let rate_ms = |frames: u32| f64::from(frames) * 1000.0 / f64::from(report.info.format.rate);
    let buffer_ms = rate_ms(notes.device_buffer_frames);
    assert!(
        (40.0..=60.0).contains(&buffer_ms),
        "the shared buffer is {buffer_ms:.1} ms"
    );
    assert_eq!(
        rate, report.info.format.rate,
        "the output runs at the mix format"
    );

    // The loopback, first channel.
    let all = captured.lock().unwrap();
    let ch = usize::from(channels);
    let x: Vec<f32> = all.iter().step_by(ch).copied().collect();
    let found = bursts(&x, rate);
    assert_eq!(
        found.len(),
        3,
        "three tones and silence between them, found {found:?}"
    );
    let fade = (rate as usize) * 40 / 1000;
    for (b, &(freq, _, _)) in found.iter().zip(&report.tones) {
        let seg: Vec<f64> = x[b.clone()].iter().map(|&v| f64::from(v)).collect();
        let mid = &seg[seg.len() / 4..seg.len() / 2];
        let level = (mid.iter().map(|v| v * v).sum::<f64>() / mid.len() as f64).sqrt()
            * std::f64::consts::SQRT_2;
        let lock = audio_check::sine::SineLock::new(freq, rate, level);
        let end = audio_check::Declared::span(seg.len().saturating_sub(fade)..seg.len());
        let clicks = audio_check::sine::check(&seg, &lock, &end);
        assert_eq!(clicks, [], "{freq} Hz through the loopback");
    }
    // Digital silence between the tones and after the last (before each stop, the tone's fade
    // and then only silence reached the output).
    for w in found.windows(2) {
        assert!(x[w[0].end..w[1].start].iter().all(|v| v.abs() <= SILENT));
    }
    let last = found.last().unwrap();
    assert!(x[last.end..].iter().all(|v| v.abs() <= SILENT));
}

/// The suite in exclusive mode (no loopback: an exclusive stream bypasses the mixer).
#[test]
#[ignore = "plays test tones on a real output in exclusive mode; run by hand"]
fn wasapi_exclusive_passes_the_conformance_suite() {
    let mut out: WasapiOutput = wasapi_output(device().as_deref(), true);
    let report = conformance::run(&mut out, &RealClock, &want()).expect("conformance");
    eprintln!("{report:?} {:?}", out.notes());
    assert_eq!(out.mode(), Mode::Exclusive, "it fell back to shared");
    assert_eq!(report.xruns, 0);
    assert!(out.notes().is_some_and(|n| n.pro_audio));
}

/// Writes silence, and once, at call `stall_at`, takes `stall` before returning. Counts the
/// device frames it was asked for.
struct Stalling {
    calls: Arc<AtomicU64>,
    made: Arc<AtomicU64>,
    stall_at: u64,
    stall: Duration,
}

impl Stalling {
    fn new(stall_at: u64, stall: Duration) -> (Stalling, Arc<AtomicU64>, Arc<AtomicU64>) {
        let (calls, made) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        (
            Stalling {
                calls: calls.clone(),
                made: made.clone(),
                stall_at,
                stall,
            },
            calls,
            made,
        )
    }
}

impl Render for Stalling {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        out.fill(0);
        self.made.store(at + u64::from(frames), Ordering::SeqCst);
        if self.calls.fetch_add(1, Ordering::SeqCst) == self.stall_at {
            std::thread::sleep(self.stall);
        }
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

/// A render call that takes 150 ms (three shared buffers) runs the device dry: one `Xrun`, the
/// presentation never goes back, and it still says where the frames handed over are: how far
/// the frames made are ahead of the DAC (by the presentation) is the same before the gap and
/// after it. Only silence is written, so nothing is heard.
///
/// If that lead changes by about the gap, the device's position counted the gap's silence as
/// frames (in shared mode the fake and the design assume it stalls instead): then shared mode
/// needs its slips measured as exclusive mode's are. Report it to the integrator.
#[test]
#[ignore = "opens a real output (silence only); run by hand"]
fn wasapi_a_late_period_says_xrun_once_and_the_presentation_stays_true() {
    for exclusive in [false, true] {
        let mut out = wasapi_output(device().as_deref(), exclusive);
        let info = out.open(&want()).expect("open");
        let rate = f64::from(info.format.rate);
        let events = out.events();
        // Call 200 is about 2 s in.
        let (r, _calls, made) = Stalling::new(200, Duration::from_millis(150));
        out.start(Box::new(r)).expect("start");
        let t0 = Instant::now();
        let mut last = 0;
        let (mut early, mut late) = (Vec::new(), Vec::new());
        while t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
            let made_now = made.load(Ordering::SeqCst) as f64;
            let now = RealClock.now_ns();
            if let Some(p) = out.presentation().read() {
                assert!(p.frame >= last, "the presentation went back");
                last = p.frame;
                let at_dac = p.frame as f64 + now.saturating_sub(p.time_ns) as f64 * rate / 1e9;
                let lead_ms = (made_now - at_dac) * 1000.0 / rate;
                match t0.elapsed().as_secs_f64() {
                    s if (1.0..1.8).contains(&s) => early.push(lead_ms),
                    s if (3.0..4.8).contains(&s) => late.push(lead_ms),
                    _ => {}
                }
            }
        }
        out.stop();
        std::thread::sleep(Duration::from_millis(200));
        let mode = out.mode();
        out.close();
        let seen: Vec<OutputEvent> = events.try_iter().collect();
        let (before, after) = (median(early), median(late));
        eprintln!(
            "exclusive asked {exclusive}, ran {mode:?}: {seen:?}; lead {before:.2} ms before the gap, {after:.2} ms after"
        );
        assert_eq!(
            seen.iter()
                .filter(|e| matches!(e, OutputEvent::Xrun { .. }))
                .count(),
            1,
            "one xrun for the one late period"
        );
        assert!(!seen.contains(&OutputEvent::Lost));
        assert!(
            (after - before).abs() < 2.0,
            "the presentation moved by {:.1} ms over the gap",
            after - before
        );
    }
}

/// Device events, with the owner's hands: plays silence on the default output, asks for the
/// default output to be changed in Windows (DefaultChanged), then for the output that was the
/// default (the one open) to be disabled or unplugged (Lost). A minute for each.
#[test]
#[ignore = "needs a person to switch and unplug outputs; run by hand"]
fn wasapi_device_change_events_fire() {
    let mut out = wasapi_output(None, false);
    out.open(&want()).expect("open");
    let events = out.events();
    let (r, calls, _made) = Stalling::new(u64::MAX, Duration::ZERO);
    out.start(Box::new(r)).expect("start");
    eprintln!(
        "Open on {:?}. Now choose another default output in Windows (Sound settings), within 60 s.",
        out.name()
    );
    let first = events.recv_timeout(Duration::from_secs(60));
    assert_eq!(first, Ok(OutputEvent::DefaultChanged));
    eprintln!(
        "DefaultChanged seen. Now unplug or disable {:?} (the output that was the default), within 60 s.",
        out.name()
    );
    let until = Instant::now() + Duration::from_secs(60);
    let mut lost = false;
    while Instant::now() < until && !lost {
        if let Ok(e) = events.recv_timeout(Duration::from_millis(200)) {
            eprintln!("{e:?}");
            lost = e == OutputEvent::Lost;
        }
    }
    assert!(lost, "Lost after the output went away");
    let calls_then = calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        calls_then,
        "no render after Lost"
    );
    assert!(out.stop().is_some());
    out.close();
}
