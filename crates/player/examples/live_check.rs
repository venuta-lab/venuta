//! Loopback capture with the click detectors, for the live test matrix (docs/qa/listen-together-live.md).
//!
//! ```text
//! cargo run -p player --example live_check -- devices
//! cargo run -p player --example live_check -- selftest
//! cargo run -p player --example live_check -- tone [--inject drop|repeat|step|gap|cut|gain] [--level-db -30] [--seconds 3] [--device ID]
//! cargo run -p player --example live_check -- capture [--out FILE.wav] [--seconds N] [--device ID | --input ID] [--solo SOLO.wav] [--log playback.log]
//! cargo run -p player --example live_check -- analyze FILE.wav [--solo SOLO.wav] [--log playback.log] [--report FILE.json]
//! cargo run -p player --example live_check -- compare ROOM.wav SOLO.wav
//! cargo run -p player --example live_check -- log playback.log [--from UNIX_MS] [--to UNIX_MS]
//! ```
//!
//! `capture` records what Windows sends to an output (the loopback: shared-mode streams only,
//! so Venuta must play in shared mode on that computer) or an input, until Enter or
//! `--seconds`, then runs detectors 3 (LPC residual) and 4 (dip and gap) of the design's
//! section 7.1. With `--solo`, a flag the same songs played solo also have is the music's own
//! and is not counted. With `--log`, a dip within 1.5 s of a shaped event the playback log
//! names (a pause, a seek, a skip, a stall, a start) is listed but not counted.
//!
//! `tone` plays the test music (quietly, for a few seconds, never anything else) while it
//! captures, to prove the chain: clean, it must find nothing; with `--inject`, it must find
//! the fault.

#[path = "live_kit/mod.rs"]
mod live_kit;

use live_kit::align::{self, Finder};
use live_kit::args::Args;
use live_kit::checks::Flag;
use live_kit::deadline::Histogram;
use live_kit::faults::{self, Fault};
use live_kit::timeline::{self, Timeline};
use live_kit::wav::{self, Wav};
use std::path::{Path, PathBuf};

fn main() {
    let args = Args::parse(&[]);
    let code = match args.command.as_str() {
        "devices" => devices(),
        "selftest" => selftest(),
        "tone" => tone(&args),
        "capture" => capture(&args),
        "analyze" => match args.positional.first() {
            Some(f) => analyze_file(Path::new(f), &args),
            None => usage(),
        },
        "compare" => match (args.positional.first(), args.positional.get(1)) {
            (Some(a), Some(b)) => compare(Path::new(a), Path::new(b)),
            _ => usage(),
        },
        "log" => match args.positional.first() {
            Some(f) => log_check(Path::new(f), &args),
            None => usage(),
        },
        _ => usage(),
    };
    std::process::exit(code);
}

fn usage() -> i32 {
    eprintln!(
        "live_check devices | selftest | tone | capture | analyze FILE.wav | compare A.wav B.wav | log playback.log\n\
         (see the top of crates/player/examples/live_check.rs, and docs/qa/listen-together-live.md)"
    );
    2
}

#[cfg(windows)]
fn devices() -> i32 {
    use live_kit::capture::{endpoints, mix_format};
    println!("Outputs (loopback and playback):");
    for (id, name, def) in endpoints(wasapi::Direction::Render) {
        let fmt = mix_format(Some(&id))
            .map(|(r, c)| format!("{r} Hz, {c} ch"))
            .unwrap_or_default();
        println!(
            "  {name}{}  [{fmt}]\n    {id}",
            if def { " (default)" } else { "" }
        );
    }
    println!("Inputs:");
    for (id, name, def) in endpoints(wasapi::Direction::Capture) {
        println!("  {name}{}\n    {id}", if def { " (default)" } else { "" });
    }
    0
}

#[cfg(not(windows))]
fn devices() -> i32 {
    eprintln!("Capture is Windows only for now.");
    1
}

/// Every detector against the injected faults and the shaped edits, offline (no sound).
fn selftest() -> i32 {
    let rate = 48_000;
    let mut ok = true;
    for gain in [1.0, 0.03] {
        println!(
            "Test music at {:.0} dBFS peak:",
            20.0 * (gain * 0.6 * 0.9f64).log10()
        );
        for fault in Fault::ALL {
            let mut x = faults::music(rate, 2.0, gain);
            let at = faults::inject(&mut x, rate, fault, 51_234, gain);
            let flags = faults::detect(&x, rate);
            let lpc = flags
                .iter()
                .any(|f| f.detector == "lpc residual" && f.at.abs_diff(at) < 400);
            let dip = flags
                .iter()
                .any(|f| f.detector == "dip" && f.at.abs_diff(at) < 480);
            let stray = flags.iter().filter(|f| f.at.abs_diff(at) >= 1200).count();
            // Detector 4 is for gaps; detector 3 must catch everything.
            let pass = lpc && stray == 0 && (fault != Fault::Gap5ms || dip);
            ok &= pass;
            println!(
                "  {:<40} detector 3 {}, detector 4 {}{}  {}",
                fault.name(),
                if lpc { "caught" } else { "MISSED" },
                if dip { "caught" } else { "-" },
                if stray > 0 {
                    format!(", {stray} stray flags")
                } else {
                    String::new()
                },
                if pass { "ok" } else { "FAIL" }
            );
        }
        let clean = faults::music(rate, 3.0, gain);
        let mut edits = vec![("clean music", clean)];
        edits.extend(faults::shaped(rate, gain));
        for (name, x) in edits {
            let n =
                faults::detect(&x, rate).len() + faults::detect_music_shaped(&x, rate, gain).len();
            ok &= n == 0;
            println!(
                "  {:<40} {} flags  {}",
                name,
                n,
                if n == 0 { "ok" } else { "FAIL" }
            );
        }
    }
    println!(
        "{}",
        if ok {
            "Self-test passed."
        } else {
            "Self-test FAILED."
        }
    );
    if ok { 0 } else { 1 }
}

/// The flags, and the long quiet stretches set aside.
type Analysis = (Vec<Found>, Vec<(usize, usize)>);

/// A flag with what the analysis made of it.
struct Found {
    flag: Flag,
    /// Why it is not counted, if it isn't.
    excused: Option<String>,
    /// With a solo run: this place was not found in it.
    note_missing: bool,
}

#[derive(serde::Serialize)]
struct ReportRow {
    seconds: f64,
    unix_ms: Option<f64>,
    detector: &'static str,
    db: f64,
    ms: f64,
    counted: bool,
    note: String,
}

/// The log's lines that declare a shaped event (a dip there is the event, not a fault).
fn declares_event(what: &str) -> bool {
    const WORDS: [&str; 22] = [
        "pause",
        "play",
        "seek",
        "lined up",
        "skip",
        "next",
        "previous",
        "jump",
        "prepare",
        "started",
        "waiting for the stream",
        "done waiting",
        "ran dry",
        "buffering",
        "suspend",
        "output",
        "switch",
        "realign",
        "stop",
        "fade",
        "join",
        "left",
    ];
    let w = what.to_lowercase();
    WORDS.iter().any(|k| w.contains(k))
}

struct LogLine {
    ms: f64,
    what: String,
    from: String,
}

fn read_log(path: &Path) -> Result<Vec<LogLine>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut old = String::new();
    // The log rolls over to playback.old.log at 256 KiB: read both, oldest first.
    if let Some(dir) = path.parent() {
        old = std::fs::read_to_string(dir.join("playback.old.log")).unwrap_or_default();
    }
    Ok(old
        .lines()
        .chain(text.lines())
        .filter_map(|l| {
            let (ms, rest) = l.split_once(' ')?;
            let ms: f64 = ms.parse().ok()?;
            let (what, from) = rest.rsplit_once(" from ").unwrap_or((rest, ""));
            Some(LogLine {
                ms,
                what: what.to_string(),
                from: from.to_string(),
            })
        })
        .collect())
}

fn analyze(w: &Wav, tl: Option<&Timeline>, args: &Args) -> Result<Analysis, String> {
    let rate = w.rate;
    let mut flags = Vec::new();
    let lpc = live_kit::checks::LpcParams::default();
    for c in 0..w.channels {
        flags.extend(live_kit::checks::lpc_residual(&w.channel(c), &lpc));
    }
    let (dips, quiet) = live_kit::checks::dip_gap(
        &w.data,
        w.channels,
        rate,
        &live_kit::checks::DipParams::default(),
    );
    flags.extend(dips);
    let flags = faults::merge(flags, (0.005 * f64::from(rate)) as usize);
    let mut found: Vec<Found> = flags
        .into_iter()
        .map(|flag| Found {
            flag,
            excused: None,
            note_missing: false,
        })
        .collect();
    // The capture's own losses.
    if let Some(tl) = tl {
        for f in &mut found {
            if tl.near_glitch(f.flag.at, rate as usize / 20) {
                f.excused = Some("next to a capture glitch (the recorder was late)".into());
            }
        }
    }
    // The same songs played solo.
    if let Some(solo) = args.get("solo") {
        let s = wav::read(Path::new(solo))?;
        if s.rate != rate {
            return Err(format!(
                "the solo capture is {} Hz, this one {rate} Hz",
                s.rate
            ));
        }
        let mut sflags = Vec::new();
        for c in 0..s.channels {
            sflags.extend(live_kit::checks::lpc_residual(&s.channel(c), &lpc));
        }
        sflags.extend(
            live_kit::checks::dip_gap(
                &s.data,
                s.channels,
                rate,
                &live_kit::checks::DipParams::default(),
            )
            .0,
        );
        let finder = Finder::new(s.mono(), rate);
        let mono = w.mono();
        let near = (0.005 * f64::from(rate)) as usize;
        let mut hint = None;
        for f in found.iter_mut().filter(|f| f.excused.is_none()) {
            match finder.locate(&mono, f.flag.at, hint) {
                Some(loc) => {
                    hint = Some(loc.at as i64 - f.flag.at as i64);
                    if sflags.iter().any(|s| {
                        s.detector == f.flag.detector
                            && loc.at + near + f.flag.len >= s.at
                            && s.at + s.len + near >= loc.at
                    }) {
                        f.excused = Some(format!(
                            "also in the solo run at {:.3} s (the music's own)",
                            loc.at as f64 / f64::from(rate)
                        ));
                    }
                }
                None => {
                    f.note_missing = true;
                }
            }
        }
    }
    // Shaped events the playback log declares.
    if let (Some(log), Some(tl)) = (args.get("log"), tl) {
        let lines = read_log(Path::new(log))?;
        for f in found
            .iter_mut()
            .filter(|f| f.excused.is_none() && f.flag.detector == "dip")
        {
            let Some(t) = tl.unix_ms_at(f.flag.at as f64) else {
                continue;
            };
            if let Some(l) = lines
                .iter()
                .find(|l| (l.ms - t).abs() <= 1500.0 && declares_event(&l.what))
            {
                f.excused = Some(format!("at a shaped event the log names: {:?}", l.what));
            }
        }
    }
    Ok((found, quiet))
}

fn report(
    w: &Wav,
    tl: Option<&Timeline>,
    found: &[Found],
    quiet: &[(usize, usize)],
    out: Option<&str>,
) -> i32 {
    let rate = f64::from(w.rate);
    let secs = w.frames() as f64 / rate;
    println!(
        "{:.1} s at {} Hz, {} ch{}",
        secs,
        w.rate,
        w.channels,
        tl.map(|t| format!(", {}", t.source)).unwrap_or_default()
    );
    if let Some(tl) = tl {
        if !tl.idle.is_empty() {
            let idle: u64 = tl.idle.iter().map(|i| i.1).sum();
            println!(
                "  the output was idle (nothing playing) {} times, {:.1} s in all",
                tl.idle.len(),
                idle as f64 / rate
            );
        }
        if !tl.capture_glitches.is_empty() {
            println!(
                "  WARNING: the recorder lost data {} times; flags beside those are set aside",
                tl.capture_glitches.len()
            );
        }
    }
    let peak = w.data.iter().fold(0f32, |m, v| m.max(v.abs()));
    if peak == 0.0 {
        println!(
            "NO RESULT: nothing was heard. Is Venuta playing in exclusive mode? The loopback hears shared mode only."
        );
        return 1;
    }
    println!(
        "  quiet stretches of 250 ms or more (pauses, stops, song boundaries): {}",
        quiet.len()
    );
    let mut rows = Vec::new();
    let mut counted = 0;
    for f in found {
        let s = f.flag.at as f64 / rate;
        let unix = tl.and_then(|t| t.unix_ms_at(f.flag.at as f64));
        let when = unix.map(timeline::clock).unwrap_or_default();
        let c = f.excused.is_none();
        counted += usize::from(c);
        println!(
            "  {} {:>9.3} s  {when}  {:<13} {:>5.1} dB  {:>6.1} ms  {}",
            if c { "CLICK" } else { "     " },
            s,
            f.flag.detector,
            f.flag.db,
            f.flag.len as f64 / rate * 1000.0,
            f.excused.as_deref().unwrap_or(if f.note_missing {
                "(this place is not in the solo run)"
            } else {
                ""
            })
        );
        rows.push(ReportRow {
            seconds: s,
            unix_ms: unix,
            detector: f.flag.detector,
            db: f.flag.db,
            ms: f.flag.len as f64 / rate * 1000.0,
            counted: c,
            note: f.excused.clone().unwrap_or_default(),
        });
    }
    if let Some(out) = out {
        let _ = std::fs::write(out, serde_json::to_string_pretty(&rows).unwrap_or_default());
    }
    if counted == 0 {
        println!(
            "PASS: no clicks or dips from detectors 3 and 4 ({} set aside).",
            found.len()
        );
        0
    } else {
        println!(
            "FAIL: {counted} flags counted (see the CLICK lines; the log's lines at those times say what happened)."
        );
        1
    }
}

fn analyze_file(path: &Path, args: &Args) -> i32 {
    let w = match wav::read(path) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let tl = Timeline::load(path);
    match analyze(&w, tl.as_ref(), args) {
        Ok((found, quiet)) => report(&w, tl.as_ref(), &found, &quiet, args.get("report")),
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

fn compare(a: &Path, b: &Path) -> i32 {
    let (wa, wb) = match (wav::read(a), wav::read(b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return 2;
        }
    };
    match align::compare(&wa, &wb) {
        Ok(c) => {
            let r = f64::from(wa.rate);
            println!(
                "lined up: the second is {:+.3} ms from the first; {:.1} s compared",
                c.offset as f64 / r * 1000.0,
                c.frames as f64 / r
            );
            if c.differing_frames == 0 {
                println!("PASS: bit-identical over the whole stretch.");
                0
            } else {
                println!(
                    "FAIL: {} frames differ (largest difference {:.1} dBFS)",
                    c.differing_frames,
                    20.0 * f64::from(c.max_diff).max(1e-12).log10()
                );
                for (s, l) in &c.stretches {
                    println!(
                        "  from {:.3} s for {:.1} ms",
                        *s as f64 / r,
                        *l as f64 / r * 1000.0
                    );
                }
                1
            }
        }
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

/// What the playback log must show after a run of the live matrix.
fn log_check(path: &Path, args: &Args) -> i32 {
    let lines = match read_log(path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let from = args.num("from", 0.0);
    let to = args.num("to", f64::MAX);
    let lines: Vec<&LogLine> = lines
        .iter()
        .filter(|l| l.ms >= from && l.ms <= to)
        .collect();
    let is_room = |l: &LogLine| {
        l.from == "listening together"
            || l.what.starts_with("room:")
            || l.what.starts_with("sync:")
            || l.what.starts_with("follow:")
    };
    let is_end = |l: &LogLine| {
        let w = l.what.as_str();
        w.starts_with("room: the host ended it")
            || w.starts_with("room: over for this guest")
            || w.starts_with("room: the player is on its own again")
            || w.starts_with("room: follower stopped")
    };
    let room = lines.iter().filter(|l| is_room(l)).count();
    let dry: Vec<&&LogLine> = lines
        .iter()
        .filter(|l| l.what.contains("ran dry"))
        .collect();
    let stalls = lines
        .iter()
        .filter(|l| l.what.starts_with("waiting for the stream"))
        .count();
    let rates = lines
        .iter()
        .filter(|l| l.what.starts_with("sync: rate"))
        .count();
    let lineups = lines
        .iter()
        .filter(|l| l.what.starts_with("sync: lined up"))
        .count();
    println!("{} lines, {room} about the room", lines.len());
    println!(
        "  sync: {rates} rate changes, {lineups} line-ups (seeks); {stalls} waits for a stream"
    );
    let mut ok = true;
    if dry.is_empty() {
        println!("  ran dry: never");
    } else {
        ok = false;
        println!("  FAIL: the output ran dry ({} lines):", dry.len());
        for l in dry.iter().take(10) {
            println!("    {} {}", timeline::clock(l.ms), l.what);
        }
    }
    let mut hist = Histogram::default();
    let mut hn = 0;
    for l in &lines {
        if let Some(h) = Histogram::from_log_line(&l.what) {
            hist.add(&h);
            hn += 1;
        }
    }
    if hn > 0 {
        println!("  render deadline ({hn} log lines):");
        for row in hist.readout().lines() {
            println!("    {row}");
        }
        ok &= hist.passes();
    }
    // After the last ending, nothing of the room.
    match lines.iter().rposition(|l| is_end(l)) {
        Some(e) => {
            // The ending writes a few lines of its own in the moment after.
            let end_ms = lines[e].ms;
            let first_end = lines[..=e]
                .iter()
                .position(|l| is_end(l) && end_ms - l.ms <= 3000.0)
                .unwrap_or(e);
            println!(
                "  the room ended at {} ({:?})",
                timeline::clock(lines[first_end].ms),
                lines[first_end].what
            );
            let after: Vec<&&LogLine> = lines[e + 1..].iter().filter(|l| is_room(l)).collect();
            if after.is_empty() {
                println!("  PASS: no room lines after the ending");
            } else {
                ok = false;
                println!("  FAIL: {} room lines after the ending:", after.len());
                for l in after.iter().take(10) {
                    println!("    {} {}", timeline::clock(l.ms), l.what);
                }
            }
        }
        None if room > 0 => println!("  the room has not ended in this stretch"),
        None => {}
    }
    if ok { 0 } else { 1 }
}

#[cfg(windows)]
fn capture(args: &Args) -> i32 {
    use live_kit::capture::{Recorder, Source};
    use std::sync::{Arc, Mutex};
    let source = match args.get("input") {
        Some(id) => Source::Input(Some(id.to_string()).filter(|s| s != "default")),
        None => Source::Loopback(args.get("device").map(str::to_string)),
    };
    let out: PathBuf = args.get("out").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("live-captures").join(format!(
            "capture-{}.wav",
            live_kit::capture::unix_ms() as u64
        ))
    });
    // The writer is made once the format is known: the first samples wait in memory.
    let writer: Arc<Mutex<Option<wav::WavWriter>>> = Arc::new(Mutex::new(None));
    let early: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let level: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));
    let (w2, e2, l2) = (writer.clone(), early.clone(), level.clone());
    let rec = match Recorder::start(
        source,
        Box::new(move |s: &[f32]| {
            let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
            if let Ok(mut l) = l2.lock() {
                *l = l.max(peak);
            }
            let mut w = w2.lock().unwrap();
            match w.as_mut() {
                Some(w) => {
                    let _ = w.write(s);
                }
                None => e2.lock().unwrap().extend_from_slice(s),
            }
        }),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    match wav::WavWriter::create(&out, rec.rate, rec.channels) {
        Ok(mut w) => {
            let _ = w.write(&early.lock().unwrap());
            *writer.lock().unwrap() = Some(w);
        }
        Err(e) => {
            eprintln!("{}: {e}", out.display());
            let _ = rec.stop();
            return 2;
        }
    }
    println!(
        "Recording {} at {} Hz, {} ch into {}",
        rec.name,
        rec.rate,
        rec.channels,
        out.display()
    );
    let seconds = args.get("seconds").and_then(|s| s.parse::<f64>().ok());
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if seconds.is_none() {
        println!("Press Enter to stop.");
        let s = stop.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            s.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }
    let t0 = std::time::Instant::now();
    let mut said = 0u64;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let el = t0.elapsed().as_secs_f64();
        if stop.load(std::sync::atomic::Ordering::SeqCst) || seconds.is_some_and(|s| el >= s) {
            break;
        }
        if el as u64 >= said + 10 {
            said = el as u64;
            let peak = std::mem::take(&mut *level.lock().unwrap());
            println!(
                "  {:>5} s  peak {:.1} dBFS",
                said,
                20.0 * f64::from(peak).max(1e-9).log10()
            );
        }
    }
    let tl = match rec.stop() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    if let Some(w) = writer.lock().unwrap().take() {
        let _ = w.finish(tl.rate);
    }
    let _ = tl.save(&out);
    println!(
        "Saved {} and {}",
        out.display(),
        Timeline::path_for(&out).display()
    );
    analyze_file(&out, args)
}

#[cfg(not(windows))]
fn capture(_: &Args) -> i32 {
    eprintln!("Capture is Windows only for now.");
    1
}

/// The test music through the default output, captured by its loopback.
#[cfg(windows)]
fn tone(args: &Args) -> i32 {
    use live_kit::capture::{Recorder, Source, mix_format};
    use live_kit::play::{PlayOpts, play};
    use std::sync::{Arc, Mutex};
    let device = args.get("device").map(str::to_string);
    let level_db = args.num("level-db", -30.0).min(-20.0);
    let seconds = args.num("seconds", 3.0).clamp(1.0, 5.0);
    let inject = match args.get("inject") {
        Some(k) => match Fault::parse(k) {
            Some(f) => Some(f),
            None => {
                eprintln!("--inject drop|repeat|step|gap|cut|gain");
                return 2;
            }
        },
        None => None,
    };
    let (rate, _) = match mix_format(device.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    // The piece's peaks are at about 0.54 of full scale.
    let gain = 10f64.powf(level_db / 20.0) / 0.54;
    let mut music = faults::music(rate, seconds, gain);
    let at = inject.map(|f| faults::inject(&mut music, rate, f, rate as usize * 3 / 2, gain));
    let got: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let g2 = got.clone();
    let rec = match Recorder::start(
        Source::Loopback(device.clone()),
        Box::new(move |s: &[f32]| g2.lock().unwrap().extend_from_slice(s)),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let ch = usize::from(rec.channels);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let before = got.lock().unwrap().iter().fold(0f32, |m, v| m.max(v.abs()));
    if before > 3e-4 {
        let _ = rec.stop();
        println!(
            "Something else is playing on this output ({:.0} dBFS): not measuring. Stop it and try again.",
            20.0 * f64::from(before).log10()
        );
        return 3;
    }
    let frames = music.len() / 2;
    let src = music.clone();
    let opts = PlayOpts {
        device,
        exclusive: false,
        rate,
        work_us: 0,
    };
    let played = play(&opts, frames, |out, c, first| {
        for (i, f) in out.chunks_exact_mut(c).enumerate() {
            let k = (first as usize + i) * 2;
            f[0] = src[k];
            if c > 1 {
                f[1] = src[k + 1];
            }
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    let tl = rec.stop();
    let played = match (played, tl) {
        (Ok(p), Ok(_)) => p,
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let all = std::mem::take(&mut *got.lock().unwrap());
    // The first two channels, as stereo.
    let cap: Vec<f32> = all
        .chunks_exact(ch)
        .flat_map(|f| [f[0], f[ch.min(2) - 1]])
        .collect();
    if let Some(keep) = args.get("keep") {
        let w = Wav {
            rate,
            channels: 2,
            data: cap.clone(),
        };
        match wav::write(Path::new(keep), &w) {
            Ok(()) => println!("Kept the capture in {keep}"),
            Err(e) => eprintln!("{keep}: {e}"),
        }
    }
    let peak = cap.iter().fold(0f32, |m, v| m.max(v.abs()));
    let sent = music.iter().fold(0f32, |m, v| m.max(v.abs()));
    println!(
        "Played {:.1} s of the test music at {:.0} dBFS on {} ({:?}, {} Hz); the loopback heard it at {:.1} dBFS",
        seconds,
        level_db,
        played.name,
        played.mode,
        played.format.rate,
        20.0 * f64::from(peak).max(1e-9).log10()
    );
    if peak < sent * 0.01 {
        println!("FAIL: the loopback did not hear the test music.");
        return 1;
    }
    // The loopback may be after the volume: scale the music-shaped detector's limits to it.
    let heard_gain = gain * f64::from(peak / sent);
    // Where the music starts in the capture, to place the injected fault.
    // (By its onset: it rises from silence, and the piece is too nearly periodic to be found by
    // its shape.)
    let onset = |x: &[f32], peak: f32| x.iter().position(|v| v.abs() > peak * 0.01).map(|i| i / 2);
    let start = match (onset(&cap, peak), onset(&music, sent)) {
        (Some(c), Some(m)) => Some(c as i64 - m as i64),
        _ => None,
    };
    let mut exact = false;
    let r = f64::from(rate);
    // Is what Windows sent to the output the music itself? Sound effects on the output
    // ("audio enhancements") change it on the way, and blunt the detectors.
    if let Some(s) = start {
        let span = rate as usize / 2..(rate as usize / 2 + rate as usize).min(frames);
        let fit = |d: i64| -> Option<(f64, f64)> {
            let (mut cm, mut mm, mut cc) = (0.0, 0.0, 0.0);
            for i in span.clone() {
                let j = i as i64 + s + d;
                if j < 0 || (j as usize) * 2 + 1 >= cap.len() {
                    return None;
                }
                for k in 0..2 {
                    let (c, m) = (
                        f64::from(cap[j as usize * 2 + k]),
                        f64::from(music[i * 2 + k]),
                    );
                    cm += c * m;
                    mm += m * m;
                    cc += c * c;
                }
            }
            let g = cm / mm.max(1e-30);
            // Residual power over the fitted music's.
            Some((
                g,
                (cc - 2.0 * g * cm + g * g * mm).max(1e-30) / (g * g * mm).max(1e-30),
            ))
        };
        if let Some((d, (g, res))) = (-480..=480)
            .filter_map(|d| fit(d).map(|f| (d, f)))
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
        {
            let res_db = 10.0 * res.log10();
            exact = res_db < -60.0;
            if exact {
                println!(
                    "  the loopback is the music itself (gain {:.3}, residual {res_db:.0} dB, {d:+} frames)",
                    g
                );
            } else {
                println!(
                    "  WARNING: Windows changes the sound on the way to this output (residual {res_db:.0} dB after the best gain): \
                     sound effects (\"audio enhancements\") are probably on. Turn them off for the live matrix; they blunt detector 4 \
                     and make captures differ from the file."
                );
            }
        }
    }
    let flags = faults::detect(&cap, rate);
    // The music-shaped detector knows the exact test music: only for an exact copy.
    let ms_flags = if exact {
        faults::detect_music_shaped(&cap, rate, heard_gain)
    } else {
        println!("  (the music-shaped detector is skipped: the sound is not the exact test music)");
        Vec::new()
    };
    for f in flags.iter().chain(&ms_flags) {
        println!(
            "  {:<13} at {:.4} s of the music, {:.1} dB",
            f.detector,
            start.map_or(0.0, |s| (f.at as f64 - s as f64) / r),
            f.db
        );
    }
    let total = flags.len() + ms_flags.len();
    match (inject, at, start) {
        (None, _, _) => {
            if total == 0 {
                println!(
                    "PASS: the capture chain is clean ({} found nothing).",
                    if exact {
                        "detectors 3, 4 and the music-shaped detector"
                    } else {
                        "detectors 3 and 4"
                    }
                );
                0
            } else {
                println!("FAIL: {total} flags on clean test music.");
                1
            }
        }
        (Some(fault), Some(at), Some(s)) => {
            let place = at as i64 + s;
            let near = |f: &Flag| (f.at as i64 - place).abs() < (r * 0.01) as i64;
            let lpc = flags
                .iter()
                .any(|f| f.detector == "lpc residual" && near(f));
            let stray = flags.iter().chain(&ms_flags).filter(|f| !near(f)).count();
            println!(
                "{} injected at {:.4} s: detector 3 {}{}",
                fault.name(),
                at as f64 / r,
                if lpc { "caught it" } else { "MISSED it" },
                if stray > 0 {
                    format!(", {stray} flags elsewhere")
                } else {
                    String::new()
                }
            );
            if lpc && stray == 0 { 0 } else { 1 }
        }
        _ => {
            println!("FAIL: the test music was not found in the capture.");
            1
        }
    }
}

#[cfg(not(windows))]
fn tone(_: &Args) -> i32 {
    eprintln!("Playing and capturing are Windows only for now.");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_in(lines: &[&str]) -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "venuta-kit-log-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("playback.log");
        std::fs::write(&p, lines.join("\n")).unwrap();
        p
    }

    const ROOM: [&str; 6] = [
        "1000 starting a room from listening together",
        "1100 room: endpoint open from listening together",
        "2000 room: 2 is in from listening together",
        "3000 sync: rate +40 ppm (one room, keeping in step) from the player",
        "9000 room: the host ended it from listening together",
        "9050 room: the player is on its own again from listening together",
    ];

    #[test]
    fn a_clean_ending_passes_the_log_check() {
        let mut l = ROOM.to_vec();
        l.push("12000 play or pause from the tray");
        let p = log_in(&l);
        assert_eq!(log_check(&p, &Args::from(vec!["log".into()], &[])), 0);
    }

    #[test]
    fn a_room_line_after_the_ending_fails_it() {
        let mut l = ROOM.to_vec();
        l.push("12000 sync: rate +12 ppm (one room, keeping in step) from the player");
        let p = log_in(&l);
        assert_eq!(log_check(&p, &Args::from(vec!["log".into()], &[])), 1);
    }

    #[test]
    fn running_dry_fails_it() {
        let mut l = ROOM.to_vec();
        l.insert(
            4,
            "5000 the output ran dry 1 more time(s), 1 in all (a gap: this thread was late) from the player",
        );
        let p = log_in(&l);
        assert_eq!(log_check(&p, &Args::from(vec!["log".into()], &[])), 1);
    }

    #[test]
    fn shaped_events_are_named_by_the_log() {
        assert!(declares_event(
            "sync: lined up from 1000 ms to 2000 ms (faded out and in)"
        ));
        assert!(declares_event("waiting for the stream pos_ms=1234"));
        assert!(!declares_event("room: 2 is in"));
    }
}
