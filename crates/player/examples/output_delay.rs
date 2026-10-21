//! Does Windows' `IAudioClock::GetPosition` include an output's whole delay (a Bluetooth
//! headset's codec and buffer, say)? Section 4.14 of docs/design/listen-together.md; the steps
//! for the owner are in docs/qa/output-delay.md.
//!
//! ```text
//! cargo run -p player --example output_delay -- devices
//! cargo run -p player --example output_delay -- measure [--device ID] [--mic ID|default] [--level-db -40] [--seconds 3] [--label NAME] [--save FILE.json] [--exclusive]
//! cargo run -p player --example output_delay -- compare WIRED.json BLUETOOTH.json
//! ```
//!
//! `measure` plays a few short chirps (quiet: -40 dBFS unless `--level-db` says otherwise)
//! through the output with the app's own WASAPI code, and keeps the device's reports of which
//! frame is at the speaker (`GetPosition` with its performance-counter stamp, as the player
//! uses it). So for each chirp it knows when Windows says it is heard. It then finds the chirp:
//!
//! - in the output's loopback (always, in shared mode): where Windows' mixer handed it on. The
//!   reported time minus this is how much delay after the mixer `GetPosition` counts.
//! - with `--mic`, in a microphone held to the speaker or the headphone's ear cup: when it was
//!   really heard. This minus the reported time is the delay `GetPosition` does not know of
//!   (plus the microphone's own input delay and the sound's flight, both a few ms; a wired run
//!   measures them, so `compare` subtracts them).

#[path = "live_kit/mod.rs"]
mod live_kit;

use live_kit::args::Args;
use serde::{Deserialize, Serialize};

/// What one run found, saved with `--save` for `compare`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Run {
    label: String,
    output: String,
    mode: String,
    rate: u32,
    buffer_ms: f64,
    chirps: usize,
    /// Reported time at the speaker minus the loopback's time (ms): the delay after the mixer
    /// that `GetPosition` counts.
    reported_after_mixer_ms: Option<f64>,
    reported_after_mixer_spread_ms: Option<f64>,
    /// Microphone time minus the reported time (ms): the delay `GetPosition` misses.
    heard_after_reported_ms: Option<f64>,
    heard_after_reported_spread_ms: Option<f64>,
    /// Microphone time minus the loopback's time (ms): the whole delay after the mixer.
    heard_after_mixer_ms: Option<f64>,
}

const INTERVAL_S: f64 = 0.5;
const FIRST_S: f64 = 0.3;
const CHIRP_S: f64 = 0.025;

/// The chirp: 25 ms sweeping 800 Hz to 5 kHz under a Hann window, peak 1.
fn chirp(rate: u32) -> Vec<f64> {
    let n = (CHIRP_S * f64::from(rate)) as usize;
    let (f0, f1) = (800.0, 5000.0);
    let tau = 2.0 * std::f64::consts::PI;
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let ph = tau * (f0 * t + (f1 - f0) / (2.0 * CHIRP_S) * t * t);
            let w = 0.5 - 0.5 * (tau * i as f64 / (n - 1) as f64).cos();
            w * ph.sin()
        })
        .collect()
}

/// Where the chirp is in `x` (mono): the frames (fractional) of each match over `min_score`,
/// at least 200 ms apart, in order.
fn find_chirps(x: &[f64], rate: u32, min_score: f64) -> Vec<(f64, f64)> {
    let t = chirp(rate);
    let c = live_kit::fft::find_in(&t, x);
    let gap = (0.2 * f64::from(rate)) as usize;
    let mut idx: Vec<usize> = (1..c.len().saturating_sub(1))
        .filter(|&i| c[i] >= min_score && c[i] >= c[i - 1] && c[i] >= c[i + 1])
        .collect();
    // Strongest first, then keep those not near a stronger one.
    idx.sort_by(|&a, &b| c[b].total_cmp(&c[a]));
    let mut kept: Vec<usize> = Vec::new();
    for i in idx {
        if kept.iter().all(|&k| k.abs_diff(i) > gap) {
            kept.push(i);
        }
    }
    kept.sort_unstable();
    kept.into_iter()
        .map(|i| {
            let f = live_kit::fft::parabolic(c[i - 1], c[i], c[i + 1]);
            (i as f64 + f, c[i])
        })
        .collect()
}

/// Pair found times (100 ns units) with the chirps' reported times, in order: the first found
/// is the first chirp (nothing else played), and each next one goes with the chirp nearest
/// its spacing.
fn pair(reported: &[f64], found: &[f64]) -> Vec<(usize, f64)> {
    let Some(&f0) = found.first() else {
        return Vec::new();
    };
    let step = INTERVAL_S * 1e7;
    let d0 = f0 - reported[0];
    found
        .iter()
        .filter_map(|&f| {
            let k = ((f - d0 - reported[0]) / step).round();
            (k >= 0.0 && (k as usize) < reported.len()).then_some((k as usize, f))
        })
        .collect()
}

fn summary(v: &[f64]) -> (Option<f64>, Option<f64>) {
    let med = live_kit::stats::median(v);
    let spread = (v.len() > 1).then(|| {
        v.iter().fold(f64::MIN, |m, &x| m.max(x)) - v.iter().fold(f64::MAX, |m, &x| m.min(x))
    });
    (med, spread)
}

fn main() {
    let args = Args::parse(&["exclusive"]);
    let code = match args.command.as_str() {
        "devices" => devices(),
        "measure" => measure(&args),
        "compare" => match (args.positional.first(), args.positional.get(1)) {
            (Some(a), Some(b)) => compare(a, b),
            _ => usage(),
        },
        _ => usage(),
    };
    std::process::exit(code);
}

fn usage() -> i32 {
    eprintln!(
        "output_delay devices | measure [--device ID] [--mic ID|default] [--level-db -40] [--seconds 3] [--label NAME] [--save FILE.json] | compare WIRED.json BLUETOOTH.json"
    );
    2
}

fn load(p: &str) -> Result<Run, String> {
    let s = std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
    serde_json::from_str(&s).map_err(|e| format!("{p}: {e}"))
}

fn compare(a: &str, b: &str) -> i32 {
    let (w, bt) = match (load(a), load(b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let ms = |v: Option<f64>| v.map_or("not measured".into(), |v| format!("{v:.1} ms"));
    println!(
        "                               {:<22} {:<22}",
        w.label, bt.label
    );
    println!(
        "  output                       {:<22} {:<22}",
        w.output, bt.output
    );
    println!(
        "  GetPosition counts after the mixer  {:<15} {:<15}",
        ms(w.reported_after_mixer_ms),
        ms(bt.reported_after_mixer_ms)
    );
    println!(
        "  heard after GetPosition says      {:<15} {:<15}",
        ms(w.heard_after_reported_ms),
        ms(bt.heard_after_reported_ms)
    );
    println!(
        "  heard after the mixer             {:<15} {:<15}",
        ms(w.heard_after_mixer_ms),
        ms(bt.heard_after_mixer_ms)
    );
    match (w.heard_after_reported_ms, bt.heard_after_reported_ms) {
        (Some(x), Some(y)) => {
            let missed = y - x;
            println!(
                "\nThe second output is heard {missed:.1} ms later than GetPosition says, beyond what the first (wired) shows."
            );
            if missed.abs() <= 15.0 {
                println!(
                    "Verdict: GetPosition INCLUDES this output's delay (within 15 ms). Every device mode can trust it; no extra delay needed."
                );
            } else if missed >= 30.0 {
                println!(
                    "Verdict: GetPosition does NOT include about {missed:.0} ms of this output's delay. It must be set as the output's extra delay (section 4.14)."
                );
            } else {
                println!(
                    "Verdict: unclear ({missed:.0} ms): repeat both runs with the microphone closer, or louder."
                );
            }
            0
        }
        _ => {
            if let (Some(x), Some(y)) = (w.reported_after_mixer_ms, bt.reported_after_mixer_ms) {
                println!(
                    "\nWithout a microphone run: GetPosition counts {:.1} ms more after the mixer on the second output.",
                    y - x
                );
                if y - x >= 80.0 {
                    println!(
                        "That is a Bluetooth-sized delay, so GetPosition includes at least that much of it. A microphone run says whether it is all of it."
                    );
                } else {
                    println!(
                        "That is no Bluetooth-sized delay: if the second output is Bluetooth, GetPosition most likely leaves its codec delay out. A microphone run proves it."
                    );
                }
            }
            0
        }
    }
}

#[cfg(windows)]
fn devices() -> i32 {
    use live_kit::capture::endpoints;
    println!("Outputs:");
    for (id, name, def) in endpoints(wasapi::Direction::Render) {
        println!("  {name}{}\n    {id}", if def { " (default)" } else { "" });
    }
    println!("Microphones and inputs:");
    for (id, name, def) in endpoints(wasapi::Direction::Capture) {
        println!("  {name}{}\n    {id}", if def { " (default)" } else { "" });
    }
    0
}

#[cfg(not(windows))]
fn devices() -> i32 {
    eprintln!("Windows only.");
    1
}

#[cfg(not(windows))]
fn measure(_: &Args) -> i32 {
    eprintln!("Windows only.");
    1
}

#[cfg(windows)]
fn measure(args: &Args) -> i32 {
    use live_kit::capture::{Recorder, Source, mix_format};
    use live_kit::play::{PlayOpts, play};
    use live_kit::timeline::Timeline;
    use std::sync::{Arc, Mutex};

    let device = args.get("device").map(str::to_string);
    let exclusive = args.flag("exclusive");
    let level_db = args.num("level-db", -40.0).min(-12.0);
    let seconds = args.num("seconds", 3.0).clamp(1.5, 10.0);
    let label = args.get("label").unwrap_or("run").to_string();
    let rate = if exclusive {
        48_000
    } else {
        match mix_format(device.as_deref()) {
            Ok((r, _)) => r,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    };
    type Got = Arc<Mutex<Vec<f32>>>;
    let start = |src: Source| -> Result<(Recorder, Got), String> {
        let got: Got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        let r = Recorder::start(
            src,
            Box::new(move |s: &[f32]| g.lock().unwrap().extend_from_slice(s)),
        )?;
        Ok((r, got))
    };
    let lb = if exclusive {
        None
    } else {
        match start(Source::Loopback(device.clone())) {
            Ok(x) => Some(x),
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    };
    let mic = match args.get("mic") {
        Some(id) => match start(Source::Input(
            Some(id.to_string()).filter(|s| s != "default"),
        )) {
            Ok(x) => Some(x),
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        },
        None => None,
    };
    std::thread::sleep(std::time::Duration::from_millis(400));
    if let Some((_, got)) = &lb {
        let before = got.lock().unwrap().iter().fold(0f32, |m, v| m.max(v.abs()));
        if before > 3e-4 {
            println!(
                "Something else is playing on this output: not measuring. Stop it and try again."
            );
            return 3;
        }
    }
    // The chirp train.
    let count = ((seconds - FIRST_S - 0.2) / INTERVAL_S).floor().max(1.0) as usize;
    let frames = (seconds * f64::from(rate)) as usize;
    let g = 10f64.powf(level_db / 20.0);
    let t = chirp(rate);
    let starts: Vec<usize> = (0..count)
        .map(|k| ((FIRST_S + k as f64 * INTERVAL_S) * f64::from(rate)) as usize)
        .collect();
    let mut train = vec![0f32; frames];
    for &s in &starts {
        for (i, v) in t.iter().enumerate() {
            if s + i < frames {
                train[s + i] = (v * g) as f32;
            }
        }
    }
    let opts = PlayOpts {
        device,
        exclusive,
        rate,
        work_us: 0,
    };
    let played = play(&opts, frames, |out, c, first| {
        for (i, f) in out.chunks_exact_mut(c).enumerate() {
            let v = train[first as usize + i];
            for s in f.iter_mut().take(2) {
                *s = v;
            }
        }
    });
    // Bluetooth may still be playing the last one.
    std::thread::sleep(std::time::Duration::from_millis(700));
    let stop = |x: Option<(Recorder, Got)>| -> Option<(Timeline, Vec<f32>)> {
        let (r, got) = x?;
        let tl = r.stop().ok()?;
        let data = std::mem::take(&mut *got.lock().unwrap());
        Some((tl, data))
    };
    let lb = stop(lb);
    let mic = stop(mic);
    let played = match played {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    // When Windows says each chirp is at the speaker: a line through the reported pairs (the
    // first few, from before the stream settled, left out).
    let pairs = &played.pairs[played.pairs.len().min(5)..];
    let (x, y): (Vec<f64>, Vec<f64>) = pairs.iter().map(|&(q, f)| (f, q as f64)).unzip();
    let Some((a, b)) = live_kit::stats::line(&x, &y) else {
        eprintln!("The device gave no positions.");
        return 1;
    };
    let reported: Vec<f64> = starts
        .iter()
        .map(|&s| a + b * (played.content_at as f64 + s as f64))
        .collect();
    // When each chirp was handed to the device (its period's buffer released): nothing can
    // hear it before that.
    let mut handed: Vec<f64> = Vec::new();
    {
        let mut w = played.content_at;
        let mut p = played.periods.iter();
        for &s in &starts {
            let f = played.content_at + s as u64;
            for per in p.by_ref() {
                let end = w + per.frames as u64;
                w = end;
                if f < end {
                    handed.push(per.done as f64);
                    break;
                }
            }
        }
    }
    let lead: Vec<f64> = reported
        .iter()
        .zip(&handed)
        .map(|(r, h)| (r - h) / 1e4)
        .collect();
    if let Some(l) = live_kit::stats::median(&lead) {
        println!(
            "  GetPosition says each chirp reaches the speaker {l:.1} ms after it is handed to the device"
        );
    }
    let jitter = pairs
        .iter()
        .map(|&(q, f)| (q as f64 - (a + b * f)).abs() / 1e4)
        .fold(0.0, f64::max);
    println!(
        "Played {count} chirps at {level_db:.0} dBFS on {} ({:?}, {} Hz, buffer {:.1} ms); positions fit a line within {jitter:.2} ms",
        played.name,
        played.mode,
        played.format.rate,
        played.buffer_frames as f64 / f64::from(rate) * 1000.0
    );
    let found_in = |cap: &(Timeline, Vec<f32>), min: f64| -> Vec<f64> {
        let (tl, data) = cap;
        let ch = usize::from(tl.channels.max(1));
        let mono: Vec<f64> = data
            .chunks_exact(ch)
            .map(|f| f.iter().map(|&v| f64::from(v)).sum::<f64>() / ch as f64)
            .collect();
        find_chirps(&mono, tl.rate, min)
            .into_iter()
            .filter_map(|(f, _)| tl.qpc_at(f))
            .collect()
    };
    let mut run = Run {
        label,
        output: played.name.clone(),
        mode: format!("{:?}", played.mode),
        rate,
        buffer_ms: played.buffer_frames as f64 / f64::from(rate) * 1000.0,
        chirps: count,
        ..Run::default()
    };
    let lb_t = lb.as_ref().map(|c| found_in(c, 0.6));
    let mic_t = mic.as_ref().map(|c| found_in(c, 0.25));
    let diffs = |found: &[f64]| -> Vec<(usize, f64)> {
        pair(&reported, found)
            .into_iter()
            .map(|(k, f)| (k, (f - reported[k]) / 1e4))
            .collect()
    };
    if let Some(found) = &lb_t {
        let d = diffs(found);
        let v: Vec<f64> = d.iter().map(|x| -x.1).collect();
        (
            run.reported_after_mixer_ms,
            run.reported_after_mixer_spread_ms,
        ) = summary(&v);
        let after_hand: Vec<f64> = d
            .iter()
            .filter_map(|&(k, x)| lead.get(k).map(|l| x + l))
            .collect();
        if let Some(m) = live_kit::stats::median(&after_hand) {
            println!("  the loopback stamps each chirp {m:.1} ms after it is handed to the device");
        }
        println!(
            "  loopback: {} of {count} chirps found; GetPosition puts the speaker {} after the mixer (spread {})",
            d.len(),
            run.reported_after_mixer_ms
                .map_or("-".into(), |v| format!("{v:.2} ms")),
            run.reported_after_mixer_spread_ms
                .map_or("-".into(), |v| format!("{v:.2} ms")),
        );
        if let Some(v) = run.reported_after_mixer_ms
            && v < 0.0
        {
            println!(
                "  NOTE: the loopback has the sound {:.1} ms after GetPosition says it reaches the speaker. The loopback is taken after\n  \
                 the output's sound effects, so they (or the driver) most likely add delay GetPosition leaves out, about this much\n  \
                 or more on this output. A microphone run (--mic) gives the whole figure.",
                -v
            );
        }
    }
    if let Some(found) = &mic_t {
        let d = diffs(found);
        let v: Vec<f64> = d.iter().map(|x| x.1).collect();
        (
            run.heard_after_reported_ms,
            run.heard_after_reported_spread_ms,
        ) = summary(&v);
        println!(
            "  microphone: {} of {count} chirps found; heard {} after GetPosition says (spread {})",
            d.len(),
            run.heard_after_reported_ms
                .map_or("-".into(), |v| format!("{v:.1} ms")),
            run.heard_after_reported_spread_ms
                .map_or("-".into(), |v| format!("{v:.1} ms")),
        );
        if d.is_empty() {
            println!("  (nothing heard: hold the microphone to the speaker, or raise --level-db)");
        }
    }
    if let (Some(a), Some(b)) = (run.reported_after_mixer_ms, run.heard_after_reported_ms) {
        run.heard_after_mixer_ms = Some(a + b);
    }
    if let Some(p) = args.get("save") {
        match std::fs::write(p, serde_json::to_string_pretty(&run).unwrap_or_default()) {
            Ok(()) => println!("Saved {p}"),
            Err(e) => eprintln!("{p}: {e}"),
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chirps_are_found_where_they_are() {
        let rate = 48_000;
        let t = chirp(rate);
        let mut x = vec![0.0f64; rate as usize * 3];
        // Soft noise, and chirps at -40 dB at known places, one at a fractional frame.
        let mut s = 1u64;
        for v in x.iter_mut() {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            *v = ((s >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 1e-3;
        }
        let at = [14_400usize, 38_400, 62_400, 86_400];
        for &a in &at {
            for (i, v) in t.iter().enumerate() {
                x[a + i] += 0.01 * v;
            }
        }
        let f = find_chirps(&x, rate, 0.5);
        assert_eq!(f.len(), 4, "{f:?}");
        for (got, want) in f.iter().zip(at) {
            assert!((got.0 - want as f64).abs() < 0.5, "{got:?} against {want}");
        }
    }

    #[test]
    fn found_chirps_pair_with_their_reports_in_order() {
        let step = INTERVAL_S * 1e7;
        let reported: Vec<f64> = (0..5).map(|k| 1e9 + k as f64 * step).collect();
        // Heard 180 ms late, the third one missed.
        let found: Vec<f64> = [0, 1, 3, 4].iter().map(|&k| reported[k] + 1.8e6).collect();
        let p = pair(&reported, &found);
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 1, 3, 4]);
    }
}
