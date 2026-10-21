//! The acoustic measure of section 7.2: two computers' outputs into one stereo input (machine A
//! into the left channel, machine B into the right, by cable from their line or headphone
//! outs, or two microphones), cross-correlated in 1 s windows. The offset is how much later B
//! is than A; the gate is on its size.
//!
//! ```text
//! cargo run -p player --example acoustic_offset -- devices
//! cargo run -p player --example acoustic_offset -- record --input ID [--seconds N] [--out FILE.wav] [--gate n1|n1b|apart|direct|relay]
//! cargo run -p player --example acoustic_offset -- analyze FILE.wav [--gate n1] [--skip 5] [--window 1] [--max-lag-ms 100] [--csv FILE.csv]
//! cargo run -p player --example acoustic_offset -- selftest
//! ```
//!
//! Each window is lined up by GCC-PHAT (the cross-spectrum's phase alone, 150 Hz to 6 kHz),
//! which gives a sharp peak on music and reads to a fraction of a sample. A window counts when
//! both channels have sound and its peak stands out (1.5 times the next one a millisecond or
//! more away); the first `--skip` seconds (5, as the gate says) are left out.

#[path = "live_kit/mod.rs"]
mod live_kit;

use live_kit::args::Args;
use live_kit::fft::gcc_phat;
use live_kit::stats::{line, mean, quantile};
use live_kit::wav::{self, Wav};
use std::path::Path;

/// One window's measure.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Window {
    /// Its start, seconds into the recording.
    t: f64,
    /// B later than A, ms.
    offset_ms: f64,
    confidence: f64,
    /// The quieter channel's RMS, dBFS.
    level_db: f64,
}

#[derive(Debug, Clone, Copy)]
struct Params {
    window_s: f64,
    max_lag_ms: f64,
    min_conf: f64,
    min_level_db: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            window_s: 1.0,
            max_lag_ms: 100.0,
            min_conf: 1.5,
            min_level_db: -60.0,
        }
    }
}

fn rms_db(x: &[f64]) -> f64 {
    let p = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    10.0 * p.max(1e-30).log10()
}

fn measure(a: &[f64], b: &[f64], rate: u32, p: &Params) -> Vec<Window> {
    let w = (p.window_s * f64::from(rate)) as usize;
    let lag = (p.max_lag_ms / 1000.0 * f64::from(rate)) as usize;
    let n = a.len().min(b.len());
    let mut out = Vec::new();
    let mut s = 0;
    while s + w <= n {
        let (wa, wb) = (&a[s..s + w], &b[s..s + w]);
        let level = rms_db(wa).min(rms_db(wb));
        if level >= p.min_level_db {
            let (d, conf) = gcc_phat(wa, wb, f64::from(rate), lag, 150.0, 6000.0);
            out.push(Window {
                t: s as f64 / f64::from(rate),
                offset_ms: d / f64::from(rate) * 1000.0,
                confidence: conf,
                level_db: level,
            });
        }
        s += w;
    }
    out
}

/// Section 7.2's acoustic gates: (name, p95 limit ms, max limit ms).
fn gate(name: &str) -> Option<(&'static str, f64, Option<f64>)> {
    Some(match name {
        "n1" => (
            "every device, one Wi-Fi, exact copies: p95 within 2 ms, max 5 ms",
            2.0,
            Some(5.0),
        ),
        "n1b" => (
            "every device with a Bluetooth output lined up by ear: p95 within 5 ms",
            5.0,
            None,
        ),
        "apart" => ("apart, one Wi-Fi: p95 within 10 ms", 10.0, None),
        "direct" => ("internet, direct: p95 within 20 ms", 20.0, None),
        "relay" => (
            "internet, relay: p95 within 30 ms, max 60 ms",
            30.0,
            Some(60.0),
        ),
        _ => return None,
    })
}

/// The readout, and whether the gate (if any) passed.
fn report(
    ws: &[Window],
    skip_s: f64,
    p: &Params,
    gate_name: Option<&str>,
    csv: Option<&str>,
) -> bool {
    let all = ws.len();
    let counted: Vec<&Window> = ws
        .iter()
        .filter(|w| w.t >= skip_s && w.confidence >= p.min_conf)
        .collect();
    let unsure = ws
        .iter()
        .filter(|w| w.t >= skip_s && w.confidence < p.min_conf)
        .count();
    if let Some(path) = csv {
        let mut s = String::from("seconds,offset_ms,confidence,level_dbfs,counted\n");
        for w in ws {
            s += &format!(
                "{:.3},{:.4},{:.2},{:.1},{}\n",
                w.t,
                w.offset_ms,
                w.confidence,
                w.level_db,
                w.t >= skip_s && w.confidence >= p.min_conf
            );
        }
        if let Err(e) = std::fs::write(path, s) {
            eprintln!("{path}: {e}");
        }
    }
    println!(
        "{all} windows with sound in both channels; {} counted after the first {skip_s} s, {unsure} set aside as unsure",
        counted.len()
    );
    if counted.is_empty() {
        println!(
            "Nothing to measure: is each computer's output in its own channel, and both playing?"
        );
        return false;
    }
    let off: Vec<f64> = counted.iter().map(|w| w.offset_ms).collect();
    let abs: Vec<f64> = off.iter().map(|v| v.abs()).collect();
    let t: Vec<f64> = counted.iter().map(|w| w.t).collect();
    let p50 = quantile(&abs, 0.5).unwrap_or_default();
    let p95 = quantile(&abs, 0.95).unwrap_or_default();
    let max = quantile(&abs, 1.0).unwrap_or_default();
    println!(
        "B against A: mean {:+.2} ms; |offset| p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms",
        mean(&off).unwrap_or_default()
    );
    if let Some((_, slope)) = line(&t, &off) {
        println!(
            "  drift {:+.3} ms per minute ({:+.0} ppm)",
            slope * 60.0,
            slope * 1000.0
        );
    }
    let worst = counted
        .iter()
        .max_by(|a, b| a.offset_ms.abs().total_cmp(&b.offset_ms.abs()))
        .unwrap();
    println!("  worst at {:.0} s: {:+.2} ms", worst.t, worst.offset_ms);
    match gate_name.and_then(|g| gate(g).map(|x| (g, x))) {
        Some((_, (what, p95_max, max_max))) => {
            let ok = p95 <= p95_max && max_max.is_none_or(|m| max <= m);
            println!("{}: {what}", if ok { "PASS" } else { "FAIL" });
            ok
        }
        None => true,
    }
}

fn params(args: &Args) -> Params {
    Params {
        window_s: args.num("window", 1.0),
        max_lag_ms: args.num("max-lag-ms", 100.0),
        ..Params::default()
    }
}

fn analyze(w: &Wav, args: &Args) -> i32 {
    if w.channels < 2 {
        eprintln!("The recording is mono: A must be in the left channel and B in the right.");
        return 2;
    }
    let a: Vec<f64> = w.channel(0).into_iter().map(f64::from).collect();
    let b: Vec<f64> = w.channel(1).into_iter().map(f64::from).collect();
    let p = params(args);
    let ws = measure(&a, &b, w.rate, &p);
    let ok = report(
        &ws,
        args.num("skip", 5.0),
        &p,
        args.get("gate"),
        args.get("csv"),
    );
    if ok { 0 } else { 1 }
}

fn main() {
    let args = Args::parse(&[]);
    if let Some(g) = args.get("gate")
        && gate(g).is_none()
    {
        eprintln!("--gate n1|n1b|apart|direct|relay");
        std::process::exit(2);
    }
    let code = match args.command.as_str() {
        "devices" => devices(),
        "record" => record(&args),
        "analyze" => match args.positional.first() {
            Some(f) => match wav::read(Path::new(f)) {
                Ok(w) => analyze(&w, &args),
                Err(e) => {
                    eprintln!("{e}");
                    2
                }
            },
            None => usage(),
        },
        "selftest" => selftest(),
        _ => usage(),
    };
    std::process::exit(code);
}

fn usage() -> i32 {
    eprintln!(
        "acoustic_offset devices | record --input ID [--seconds N] [--out FILE.wav] | analyze FILE.wav [--gate n1|n1b|apart|direct|relay] | selftest"
    );
    2
}

/// Music-like sound at any moment (so it can be delayed by any amount): partials that swell
/// and fade on their own cycles.
fn music_at(t: f64) -> f64 {
    let tau = 2.0 * std::f64::consts::PI;
    let mut x = 0.0;
    for k in 0..90 {
        let u = ((k * 7919 + 17) % 997) as f64 / 997.0;
        let f = 70.0 * 80f64.powf(u);
        let env = 0.5 + 0.5 * (tau * (0.07 + 0.3 * u) * t + k as f64).sin();
        x += env * (tau * f * t + k as f64 * 2.1).sin() / (f / 70.0).sqrt();
    }
    x * 0.03
}

/// A known offset (1.37 ms, drifting 0.6 ms over the recording, one 2 s stretch at 9 ms) is
/// found window by window.
fn selftest() -> i32 {
    let rate = 48_000u32;
    let secs = 10.0;
    let n = (secs * f64::from(rate)) as usize;
    let truth = |t: f64| {
        if (6.0..8.0).contains(&t) {
            9.0
        } else {
            1.37 + 0.6 * t / secs
        }
    };
    let a: Vec<f64> = (0..n)
        .map(|i| music_at(i as f64 / f64::from(rate)))
        .collect();
    let b: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            music_at(t - truth(t) / 1000.0)
        })
        .collect();
    let ws = measure(&a, &b, rate, &Params::default());
    let mut worst: f64 = 0.0;
    for w in &ws {
        let want = truth(w.t + 0.5);
        // Windows straddling the 9 ms stretch's edges have two answers; skip them.
        if (5.0..6.0).contains(&w.t) || (7.0..8.0).contains(&w.t) {
            continue;
        }
        worst = worst.max((w.offset_ms - want).abs());
    }
    println!(
        "{} windows; the largest error against the known offset is {worst:.3} ms",
        ws.len()
    );
    report(&ws, 0.0, &Params::default(), None, None);
    if worst < 0.05 {
        println!("Self-test passed.");
        0
    } else {
        println!("Self-test FAILED.");
        1
    }
}

#[cfg(windows)]
fn devices() -> i32 {
    use live_kit::capture::endpoints;
    println!("Inputs (A into the left channel, B into the right):");
    for (id, name, def) in endpoints(wasapi::Direction::Capture) {
        println!("  {name}{}\n    {id}", if def { " (default)" } else { "" });
    }
    0
}

#[cfg(not(windows))]
fn devices() -> i32 {
    eprintln!("Recording is Windows only for now; analyze works anywhere.");
    1
}

#[cfg(windows)]
fn record(args: &Args) -> i32 {
    use live_kit::capture::{Recorder, Source, unix_ms};
    use std::sync::{Arc, Mutex};
    let Some(id) = args.get("input") else {
        eprintln!("--input ID (see devices): the stereo input both outputs are wired into");
        return 2;
    };
    let out = args
        .get("out")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| format!("acoustic-{}.wav", unix_ms() as u64).into());
    let got: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    let rec = match Recorder::start(
        Source::Input(Some(id.to_string()).filter(|s| s != "default")),
        Box::new(move |s: &[f32]| g.lock().unwrap().extend_from_slice(s)),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    if rec.channels < 2 {
        eprintln!("{} is mono: the measure needs a stereo input.", rec.name);
        let _ = rec.stop();
        return 2;
    }
    println!(
        "Recording {} ({} Hz, {} ch). A in the left channel, B in the right.",
        rec.name, rec.rate, rec.channels
    );
    let seconds = args.get("seconds").and_then(|s| s.parse::<f64>().ok());
    match seconds {
        Some(s) => std::thread::sleep(std::time::Duration::from_secs_f64(s)),
        None => {
            println!("Press Enter to stop.");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
        }
    }
    let rate = rec.rate;
    let ch = usize::from(rec.channels);
    if let Err(e) = rec.stop() {
        eprintln!("{e}");
        return 2;
    }
    let all = std::mem::take(&mut *got.lock().unwrap());
    // The first two channels.
    let data: Vec<f32> = all.chunks_exact(ch).flat_map(|f| [f[0], f[1]]).collect();
    let w = Wav {
        rate,
        channels: 2,
        data,
    };
    if let Err(e) = wav::write(&out, &w) {
        eprintln!("{}: {e}", out.display());
    } else {
        println!("Saved {}", out.display());
    }
    analyze(&w, args)
}

#[cfg(not(windows))]
fn record(_: &Args) -> i32 {
    eprintln!("Recording is Windows only for now; analyze works anywhere.");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_self_test_passes() {
        assert_eq!(selftest(), 0);
    }

    #[test]
    fn a_silent_channel_gives_no_windows() {
        let a: Vec<f64> = (0..96_000).map(|i| music_at(i as f64 / 48_000.0)).collect();
        let b = vec![0.0; 96_000];
        assert!(measure(&a, &b, 48_000, &Params::default()).is_empty());
    }

    #[test]
    fn the_gates_read_as_section_7_2() {
        let w = |o: f64| Window {
            t: 10.0,
            offset_ms: o,
            confidence: 10.0,
            level_db: -20.0,
        };
        let p = Params::default();
        let mut ws: Vec<Window> = (0..100).map(|_| w(1.0)).collect();
        assert!(report(&ws, 5.0, &p, Some("n1"), None));
        ws[0] = w(6.0);
        assert!(
            !report(&ws, 5.0, &p, Some("n1"), None),
            "a 6 ms window breaks max 5 ms"
        );
        assert!(report(&ws, 5.0, &p, Some("n1b"), None));
    }
}
