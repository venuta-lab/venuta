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

/// `VENUTA_HW_TRACE`: a path stem. A test that traces writes `<stem>-<mode>.csv` (one row per
/// device event, the columns `output::TRACE_COLUMNS`) and `<stem>-<mode>.txt` (what the
/// device said, the events, the leads, what the flags mean).
fn trace_path() -> Option<String> {
    std::env::var("VENUTA_HW_TRACE")
        .ok()
        .filter(|s| !s.is_empty())
}

fn write_trace(
    t: &player::output::Trace,
    name: &str,
    about: &str,
    notes: Option<player::output::DeviceNotes>,
    events: &[OutputEvent],
) {
    let stem = trace_path().expect("a trace path");
    let csv = format!("{stem}-{name}.csv");
    std::fs::write(&csv, t.csv()).expect("write the trace");
    let about = format!(
        "{about}\n{notes:?}\nrows {} (dropped {})\nevents {events:?}\n\
         flags: near 1, passed 2, suspect 4, certain 8, settled 16, counted 32, dry 64, watching 128, queued 256\n\
         state: 1 running, 2 draining; a start is where `before` goes back to one buffer\n\
         value: exclusive lead / shared pipeline (or the shift when settled)\n\
         delta: exclusive frames later than usual (a queue: its lead) / shared lag behind time; -1: none\n",
        t.rows().len(),
        t.dropped(),
    );
    std::fs::write(format!("{stem}-{name}.txt"), about).expect("write the notes");
    eprintln!("trace written to {csv}");
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
    let trace = trace_path().map(|_| out.trace(40_000));
    let run = conformance::run(&mut out, &RealClock, &want());
    if let Some(t) = &trace {
        write_trace(
            t,
            "conformance-shared",
            &format!("{run:?}"),
            out.notes(),
            &[],
        );
    }
    let report = run.expect("conformance");
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

    // The loopback, first channel, against what was rendered.
    let all = captured.lock().unwrap();
    let ch = usize::from(channels);
    let x: Vec<f32> = all.iter().step_by(ch).copied().collect();
    let fits = judge_loopback(&x, &report.info.format, &report.tones)
        .unwrap_or_else(|e| panic!("the loopback: {e}"));
    eprintln!("loopback against what was rendered: {fits:?}");
    if let Some(why) = processed(&fits) {
        panic!(
            "effects in the path ({why}): the loopback is taken after this output's sound effects, so it can't judge clicks or silence here. Turn the output's audio enhancements off and run again."
        );
    }
    for (f, &(freq, tail_from, _)) in fits.iter().zip(&report.tones) {
        let seg: Vec<f64> = x[f.onset..f.end].iter().map(|&v| f64::from(v)).collect();
        let lock = audio_check::sine::SineLock::new(freq, rate, 0.5 * f.gain);
        let fade = audio_check::Declared::span(tail_from as usize..seg.len());
        let clicks = audio_check::sine::check(&seg, &lock, &fade);
        assert_eq!(clicks, [], "{freq} Hz through the loopback");
    }
    // Digital silence between the tones and after the last (before each stop, the tone's fade
    // and then only silence reached the output).
    for w in fits.windows(2) {
        assert!(
            x[w[0].end..w[1].onset].iter().all(|v| v.abs() <= SILENT),
            "sound between tones"
        );
    }
    let last = fits.last().unwrap();
    assert!(
        x[last.end..].iter().all(|v| v.abs() <= SILENT),
        "sound after the last stop"
    );
}

/// One tone as the loopback had it, against what was rendered.
#[derive(Debug, Clone, PartialEq)]
struct ToneFit {
    /// Where it begins in the capture (its first sound) and where its fade ends.
    onset: usize,
    end: usize,
    /// The loopback's level against the rendered tone's (the endpoint's volume, where the
    /// loopback is taken after it).
    gain: f64,
    /// What differs from the rendered tone at that gain, over the tone, in dB under the tone.
    /// Far under -60 on a path with nothing after the mixer; an output's effects (filters,
    /// delays) make it large. Sound after the fade on a clean path is a fault, not an effect.
    residual_db: f64,
    /// Sound in the 60 ms after the fade's end (from 1 ms on), dBFS.
    after_db: f64,
}

/// Finds each tone of a conformance run (`tones`: frequency, fade start, silence start) in a
/// capture, in order, each beginning at the first sound after the last one's end, and compares
/// it with what was rendered. On a path with nothing after the mixer the capture is the
/// rendered tone at one gain (`residual_db` far under -60) with digital silence after each
/// fade; an output's sound effects (filters, delays, ringing) show as a large residual over
/// the tone itself. Sound after a fade with the tone itself clean is a fault, for the caller.
fn judge_loopback(
    x: &[f32],
    fmt: &player::output::DeviceFormat,
    tones: &[(f64, u64, u64)],
) -> Result<Vec<ToneFit>, String> {
    let rate = fmt.rate as usize;
    let after = rate * 60 / 1000;
    let mut from = 0;
    let mut fits = Vec::new();
    for &(freq, tail_from, silent_from) in tones {
        let onset = from
            + x[from..]
                .iter()
                .position(|v| v.abs() > SILENT)
                .ok_or(format!("no {freq} Hz tone after sample {from}"))?;
        let len = silent_from as usize;
        if onset + len + after > x.len() {
            return Err(format!("the capture ends inside the {freq} Hz tone"));
        }
        let tone = conformance::ToneRender::faded(freq, fmt.clone(), tail_from);
        let r: Vec<f64> = (0..len as u64).map(|k| tone.value(k)).collect();
        let steady = rate / 20..(tail_from as usize).saturating_sub(rate / 20);
        let (xr, rr) = steady.clone().fold((0.0, 0.0), |(a, b), k| {
            (a + f64::from(x[onset + k]) * r[k], b + r[k] * r[k])
        });
        let gain = if rr > 0.0 { xr / rr } else { 0.0 };
        let (mut err, mut sig) = (0.0, 0.0);
        for k in 0..len {
            let want = gain * r[k];
            let d = f64::from(x[onset + k]) - want;
            err += d * d;
            sig += want * want;
        }
        let tail = &x[onset + len + rate / 1000..onset + len + after];
        let rms =
            (tail.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        fits.push(ToneFit {
            onset,
            end: onset + len,
            gain,
            residual_db: 10.0 * (err.max(1e-300) / sig.max(1e-300)).log10(),
            after_db: 20.0 * rms.max(1e-15).log10(),
        });
        from = onset + len;
    }
    Ok(fits)
}

/// Why the path has effects in it, if it has.
fn processed(fits: &[ToneFit]) -> Option<String> {
    fits.iter().find_map(|f| {
        (f.residual_db > -60.0 || f.gain < 0.01).then(|| {
            format!(
                "at sample {} the loopback differs from what was rendered by {:.1} dB at gain {:.3}, with {:.1} dBFS of sound after the fade",
                f.onset, f.residual_db, f.gain, f.after_db
            )
        })
    })
}

/// The loopback judge on made-up captures (CI): a clean path is judged, an output's effects
/// are told, not judged.
#[cfg(test)]
mod loopback_judge {
    use super::*;

    fn fmt() -> player::output::DeviceFormat {
        player::output::DeviceFormat {
            rate: 48_000,
            channels: 2,
            container_bits: 32,
            valid_bits: 32,
            kind: player::output::SampleKind::Float,
        }
    }

    /// Three tones as a run renders them (shorter), with gaps, at a gain.
    fn capture(gain: f64, gaps: [usize; 3]) -> (Vec<f32>, Vec<(f64, u64, u64)>) {
        let tones = vec![
            (1_000.0, 24_000, 24_000 + 1_440),
            (600.0, 12_000, 12_000 + 1_440),
            (1_500.0, 12_000, 12_000 + 1_440),
        ];
        let mut x = Vec::new();
        for (&(freq, tail, silent), gap) in tones.iter().zip(gaps) {
            x.extend(std::iter::repeat_n(0.0f32, gap));
            let t = conformance::ToneRender::faded(freq, fmt(), tail);
            x.extend((0..silent).map(|k| (gain * t.value(k)) as f32));
        }
        x.extend(std::iter::repeat_n(0.0f32, 9_600));
        (x, tones)
    }

    #[test]
    fn a_clean_path_is_the_rendered_tones_at_one_gain_with_silence_after() {
        let (x, tones) = capture(0.7, [6_353, 13_174, 3_600]);
        let fits = judge_loopback(&x, &fmt(), &tones).unwrap();
        assert_eq!(processed(&fits), None, "{fits:?}");
        assert_eq!(
            fits.iter().map(|f| f.onset).collect::<Vec<_>>(),
            [
                6_353,
                6_353 + 25_440 + 13_174,
                6_353 + 25_440 + 13_174 + 13_440 + 3_600
            ]
        );
        assert!(fits.iter().all(|f| (f.gain - 0.7).abs() < 1e-6));
    }

    #[test]
    fn sound_after_a_fade_on_a_clean_path_is_a_fault_not_an_effect() {
        let (mut x, tones) = capture(0.7, [6_353, 13_174, 3_600]);
        // Something plays on after the last stop (a stale buffer).
        let end = x.len() - 9_600;
        for (k, v) in x[end + 480..end + 960].iter_mut().enumerate() {
            *v = 0.2 * (k as f32 * 0.3).sin();
        }
        let fits = judge_loopback(&x, &fmt(), &tones).unwrap();
        assert_eq!(processed(&fits), None, "the tones themselves are clean");
        assert!(fits[2].after_db > -40.0, "{fits:?}");
        assert!(x[fits[2].end..].iter().any(|v| v.abs() > SILENT));
    }

    #[test]
    fn an_output_with_effects_is_told_and_not_judged() {
        // What an output's effects do after the mixer: a delay and a filter that rings on
        // (a one-pole low-pass, 50 ms to decay), so the 75 ms between the last two tones is
        // never silent: the merged run seen on the owner's laptop.
        let (clean, tones) = capture(0.7, [6_353, 13_174, 3_600]);
        let mut x = vec![0.0f32; 4_800];
        let (mut y, a) = (0.0f64, (-1.0 / (0.05 * 48_000.0f64)).exp());
        for v in clean {
            y = a * y + (1.0 - a) * f64::from(v) * 8.0;
            x.push(y as f32);
        }
        let fits = judge_loopback(&x, &fmt(), &tones).unwrap();
        let why = processed(&fits).expect("effects told");
        assert!(why.contains("differs from what was rendered"), "{why}");
    }
}

/// The suite in exclusive mode (no loopback: an exclusive stream bypasses the mixer).
#[test]
#[ignore = "plays test tones on a real output in exclusive mode; run by hand"]
fn wasapi_exclusive_passes_the_conformance_suite() {
    let mut out: WasapiOutput = wasapi_output(device().as_deref(), true);
    let trace = trace_path().map(|_| out.trace(40_000));
    let run = conformance::run(&mut out, &RealClock, &want());
    if let Some(t) = &trace {
        write_trace(
            t,
            "conformance-exclusive",
            &format!("{run:?}"),
            out.notes(),
            &[],
        );
    }
    let report = run.expect("conformance");
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
/// Shared mode takes off silence the position counted (the pipeline between the engine's
/// read and the DAC is steady): the first run of WP1, without that, saw the lead move 9.8 ms.
/// Whether the corrected presentation is the sound's truth is the loopback test's to say.
#[test]
#[ignore = "opens a real output (silence only); run by hand"]
fn wasapi_a_late_period_says_xrun_once_and_the_presentation_stays_true() {
    for exclusive in [false, true] {
        let mut out = wasapi_output(device().as_deref(), exclusive);
        let trace = trace_path().map(|_| out.trace(20_000));
        let info = out.open(&want()).expect("open");
        let rate = f64::from(info.format.rate);
        let events = out.events();
        // Call 200 is about 2 s in.
        let (r, _calls, made) = Stalling::new(200, Duration::from_millis(150));
        out.start(Box::new(r)).expect("start");
        let t0 = Instant::now();
        let mut last = 0;
        let mut went_back = false;
        let (mut early, mut late) = (Vec::new(), Vec::new());
        while t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
            let made_now = made.load(Ordering::SeqCst) as f64;
            let now = RealClock.now_ns();
            if let Some(p) = out.presentation().read() {
                went_back |= p.frame < last;
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
        let (most_early, most_late) = (
            early.iter().copied().fold(f64::MIN, f64::max),
            late.iter().copied().fold(f64::MIN, f64::max),
        );
        let (before, after) = (median(early), median(late));
        eprintln!(
            "exclusive asked {exclusive}, ran {mode:?}: {seen:?}; lead {before:.2} ms before the gap, {after:.2} ms after; most {most_early:.2} ms before, {most_late:.2} ms after"
        );
        if let Some(t) = &trace {
            let name = format!("{mode:?}").to_lowercase();
            let about =
                format!("{info:?}, lead before the gap {before:.2} ms, after {after:.2} ms");
            write_trace(t, &name, &about, out.notes(), &seen);
        }
        assert!(!went_back, "the presentation went back");
        assert_eq!(
            seen.iter()
                .filter(|e| matches!(e, OutputEvent::Xrun { .. }))
                .count(),
            1,
            "one xrun for the one late period"
        );
        assert!(!seen.contains(&OutputEvent::Lost));
        // Shared mode: the engine's pipeline is steady, so is the usual lead. A queue (exclusive
        // mode on the laptop): its usual depth sinks when the driver's events come more unevenly
        // after a gap, but what it refills to does not: compare the most.
        let (b, a, what) = if mode == Mode::Exclusive {
            (most_early, most_late, "most")
        } else {
            (before, after, "usual")
        };
        // A queue's depth wobbles from fill to fill by its learned band (about 5 ms on the
        // laptop's driver): no reading of it can be finer than that, so that is the allowance
        // there, never less than 2 ms.
        let allowed = if mode == Mode::Exclusive {
            out.latency().map_or(2.0, |l| {
                (f64::from(l.band) * 1000.0 / f64::from(info.format.rate)).max(2.0)
            })
        } else {
            2.0
        };
        assert!(
            (a - b).abs() < allowed,
            "the presentation moved by {:.1} ms over the gap (the {what} lead, {allowed:.1} ms allowed)",
            a - b
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

/// Short tone bursts (5 ms of 1 kHz under a Hann window, -20 dBFS) every 250 ms, and once,
/// in the silence between two of them, a render call 150 ms late. Counts its calls.
struct Bursts {
    fmt: player::output::DeviceFormat,
    every: u64,
    len: u64,
    stall_after: u64,
    stalled: bool,
    scratch: Vec<i32>,
    made: Arc<AtomicU64>,
}

impl Bursts {
    fn value(&self, frame: u64) -> f64 {
        let k = frame % self.every;
        if k >= self.len {
            return 0.0;
        }
        let x = k as f64 / self.len as f64;
        let w = (std::f64::consts::PI * x).sin().powi(2);
        0.1 * w * (std::f64::consts::TAU * 1_000.0 * k as f64 / f64::from(self.fmt.rate)).sin()
    }
}

impl Render for Bursts {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        let ch = usize::from(self.fmt.channels);
        self.scratch.clear();
        for k in 0..u64::from(frames) {
            let v = (self.value(at + k) * 2_147_483_647.0) as i32;
            self.scratch.extend(std::iter::repeat_n(v, ch));
        }
        player::render::encode_into(&self.scratch, &self.fmt, out);
        self.made.store(at + u64::from(frames), Ordering::SeqCst);
        if !self.stalled && at >= self.stall_after {
            self.stalled = true;
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

/// The energy centroid of each burst in `x` (sample index, fractional).
fn centroids(x: &[f32], rate: u32) -> Vec<f64> {
    let mut out = Vec::new();
    for b in bursts(x, rate) {
        let (mut num, mut den) = (0.0, 0.0);
        for i in b.clone() {
            let e = f64::from(x[i]) * f64::from(x[i]);
            num += i as f64 * e;
            den += e;
        }
        if den > 0.0 {
            out.push(num / den);
        }
    }
    out
}

/// The truth of the presentation across a gap, by the loopback: for every burst, when the
/// presentation said its frames would be at the DAC against when the loopback had them. The
/// difference is the path between the two (constant for an output); it must not change across
/// the late period. This is what settles how a shared stream's position behaves in a gap: if
/// the presentation were off after it, the bursts after the gap would be off by the same
/// amount. Plays 18 quiet beeps over 4.5 s; the late period falls in silence.
#[test]
#[ignore = "plays quiet beeps on a real output; run by hand"]
fn wasapi_the_presentation_matches_the_loopback_across_a_gap() {
    let mut out = wasapi_output(device().as_deref(), false);
    let info = out.open(&want()).expect("open");
    assert_eq!(out.mode(), Mode::Shared);
    let rate = info.format.rate;
    let captured = Arc::new(Mutex::new(Vec::<f32>::new()));
    let c = captured.clone();
    let rec = Recorder::start(
        Source::Loopback(device()),
        Box::new(move |s: &[f32]| c.lock().unwrap().extend_from_slice(s)),
    )
    .expect("the loopback recorder");
    assert_eq!(rec.rate, rate, "the output runs at the mix format");
    let channels = usize::from(rec.channels);
    let events = out.events();
    let every = u64::from(rate) / 4;
    let made = Arc::new(AtomicU64::new(0));
    out.start(Box::new(Bursts {
        fmt: info.format.clone(),
        every,
        len: u64::from(rate) / 200,
        // In the silence after the ninth burst (2 s in).
        stall_after: 8 * every + u64::from(rate) / 20,
        stalled: false,
        scratch: Vec::with_capacity(16_384 * usize::from(info.format.channels)),
        made: made.clone(),
    }))
    .expect("start");
    let mut pairs = Vec::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(4_600) {
        std::thread::sleep(Duration::from_millis(5));
        if let Some(p) = out.presentation().read()
            && pairs.last() != Some(&p)
        {
            pairs.push(p);
        }
    }
    out.stop();
    std::thread::sleep(Duration::from_millis(300));
    out.close();
    let timeline = rec.stop().expect("the recording");
    let seen: Vec<OutputEvent> = events.try_iter().collect();
    eprintln!("events {seen:?}; {} pairs", pairs.len());

    // When the loopback had each burst's centre, by the capture's anchors (performance
    // counter, 100 ns units).
    let all = captured.lock().unwrap();
    let x: Vec<f32> = all.iter().step_by(channels).copied().collect();
    let heard: Vec<f64> = centroids(&x, rate)
        .into_iter()
        .map(|i| {
            let (f, q) = timeline
                .anchors
                .iter()
                .rev()
                .find(|(f, _)| (*f as f64) <= i)
                .copied()
                .unwrap_or(timeline.anchors[0]);
            (q as f64 + (i - f as f64) * 1e7 / f64::from(rate)) * 100.0
        })
        .collect();
    // When the presentation said each burst's centre is at the DAC, from the pair nearest to it.
    let centre = (u64::from(rate) / 400) as f64;
    let said: Vec<f64> = (0..heard.len() as u64)
        .map(|k| {
            let f = (k * every) as f64 + centre;
            let p = pairs
                .iter()
                .min_by(|a, b| {
                    (a.frame as f64 - f)
                        .abs()
                        .total_cmp(&(b.frame as f64 - f).abs())
                })
                .expect("pairs");
            p.time_ns as f64 + (f - p.frame as f64) * 1e9 / f64::from(rate)
        })
        .collect();
    let offsets: Vec<f64> = heard
        .iter()
        .zip(&said)
        .map(|(h, s)| (h - s) / 1e6)
        .collect();
    eprintln!("loopback minus presentation, ms, per burst: {offsets:.2?}");
    assert!(heard.len() >= 17, "found {} bursts", heard.len());
    assert_eq!(
        seen.iter()
            .filter(|e| matches!(e, OutputEvent::Xrun { .. }))
            .count(),
        1
    );
    let (lo, hi) = offsets
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &o| (lo.min(o), hi.max(o)));
    assert!(
        hi - lo < 1.0,
        "the presentation moved against the loopback by {:.2} ms",
        hi - lo
    );
}
