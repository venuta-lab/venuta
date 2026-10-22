//! The render deadline histogram of section 7.5: how much of each period is left when the
//! buffer goes back to the device.
//!
//! ```text
//! cargo run -p player --example deadline_probe -- probe [--seconds 60] [--exclusive] [--device ID] [--work-us 300] [--stress]
//! cargo run -p player --example deadline_probe -- log playback.log [--from UNIX_MS] [--to UNIX_MS]
//! ```
//!
//! `probe` runs a render loop on the app's own WASAPI output code (open, wait, write, as the
//! player does, scheduled as pro audio) that plays **silence**: nothing is heard. `--work-us`
//! spends that long per period as a stand-in for a renderer's cost; `--stress` adds the
//! section 7.5 stress set's machine load (two busy threads and a large file being written and
//! read in the temp folder) for the run. It prints the margin histogram, the device's
//! headroom (what the device still held when the loop woke), and how often the device ran dry.
//!
//! `log` reads the histograms the new engine writes to the playback log
//! (`render: deadline margin ...` lines, the format in `live_kit/deadline.rs`), adds up those in
//! the time range, and prints the same readout with the gate.

#[path = "live_kit/mod.rs"]
mod live_kit;

use live_kit::args::Args;
use live_kit::deadline::Histogram;

fn main() {
    let args = Args::parse(&["exclusive", "stress"]);
    let code = match args.command.as_str() {
        "probe" => probe(&args),
        "log" => match args.positional.first() {
            Some(p) => from_log(p, &args),
            None => usage(),
        },
        _ => usage(),
    };
    std::process::exit(code);
}

fn usage() -> i32 {
    eprintln!(
        "deadline_probe probe [--seconds 60] [--exclusive] [--device ID] [--work-us N] [--stress] | log playback.log [--from UNIX_MS] [--to UNIX_MS]"
    );
    2
}

fn from_log(path: &str, args: &Args) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 2;
        }
    };
    let (from, to) = (args.num("from", 0.0), args.num("to", f64::MAX));
    let mut h = Histogram::default();
    let mut n = 0;
    for l in text.lines() {
        let Some((ms, rest)) = l.split_once(' ') else {
            continue;
        };
        let Ok(ms) = ms.parse::<f64>() else { continue };
        if ms < from || ms > to {
            continue;
        }
        if let Some(x) = Histogram::from_log_line(rest) {
            h.add(&x);
            n += 1;
        }
    }
    if n == 0 {
        println!(
            "No render deadline lines in that stretch of the log (the new engine writes them; engine 1 does not)."
        );
        return 1;
    }
    println!("{n} histograms from the log:");
    print!("{}", h.readout());
    verdict(&h)
}

fn verdict(h: &Histogram) -> i32 {
    if h.passes() {
        println!("PASS: p99.99 of the margin over 30% of the period, never dry (section 7.5)");
        0
    } else {
        println!(
            "FAIL: section 7.5 wants p99.99 of the margin over 30% of the period and no dry period"
        );
        1
    }
}

/// Machine load for the stress set, until dropped.
struct Stress {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
    dir: std::path::PathBuf,
}

impl Stress {
    fn start() -> Self {
        use std::sync::atomic::Ordering;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dir = std::env::temp_dir().join(format!("venuta-stress-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut threads = Vec::new();
        for _ in 0..2 {
            let s = stop.clone();
            threads.push(std::thread::spawn(move || {
                let mut x = 1u64;
                while !s.load(Ordering::Relaxed) {
                    for _ in 0..10_000 {
                        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    }
                    std::hint::black_box(x);
                }
            }));
        }
        let (s, d) = (stop.clone(), dir.clone());
        threads.push(std::thread::spawn(move || {
            use std::io::{Read, Write};
            let chunk = vec![0x5au8; 8 << 20];
            let f = d.join("big.bin");
            while !s.load(Ordering::Relaxed) {
                // A large file written (512 MiB), then read back.
                if let Ok(mut o) = std::fs::File::create(&f) {
                    for _ in 0..64 {
                        if s.load(Ordering::Relaxed) || o.write_all(&chunk).is_err() {
                            break;
                        }
                    }
                    let _ = o.sync_all();
                }
                if let Ok(mut i) = std::fs::File::open(&f) {
                    let mut buf = vec![0u8; 8 << 20];
                    while !s.load(Ordering::Relaxed) && i.read(&mut buf).is_ok_and(|n| n > 0) {}
                }
            }
        }));
        Stress { stop, threads, dir }
    }
}

impl Drop for Stress {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(not(windows))]
fn probe(_: &Args) -> i32 {
    eprintln!("The probe runs on the Windows output; log works anywhere.");
    1
}

#[cfg(windows)]
fn probe(args: &Args) -> i32 {
    use live_kit::capture::mix_format;
    use live_kit::play::{PlayOpts, play};
    let device = args.get("device").map(str::to_string);
    let exclusive = args.flag("exclusive");
    let seconds = args.num("seconds", 60.0).max(1.0);
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
    let opts = PlayOpts {
        device,
        exclusive,
        rate,
        work_us: args.num("work-us", 0.0) as u64,
    };
    let stress = args.flag("stress").then(Stress::start);
    println!(
        "Playing silence for {seconds:.0} s{}{} ...",
        if opts.work_us > 0 {
            format!(", {} µs of work per period", opts.work_us)
        } else {
            String::new()
        },
        if stress.is_some() {
            ", under the stress set"
        } else {
            ""
        }
    );
    // Silence: the content is left as it comes, zeros.
    let played = play(&opts, (seconds * f64::from(rate)) as usize, |_, _, _| {});
    drop(stress);
    let p = match played {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let (h, head) = histogram(
        &p.periods,
        rate,
        p.buffer_frames,
        p.mode == player::output::Mode::Exclusive,
    );
    println!(
        "{} ({:?}, {} Hz, {} ch, buffer {:.1} ms)",
        p.name,
        p.mode,
        p.format.rate,
        p.format.channels,
        p.buffer_frames as f64 / f64::from(rate) * 1000.0
    );
    print!("{}", h.readout());
    if let Some((min, low)) = head {
        println!(
            "  headroom at the wake (what the device still held): worst {min:.2} ms, 0.01% under {low:.2} ms"
        );
    }
    verdict(&h)
}

/// The margin histogram of a run, and the headroom at the wake (worst, and the 0.01%
/// quantile) in ms: what the device still held, so how far it was from running dry.
///
/// Shared mode: the device ran dry when it held nothing at a wake (all of `buffer` free).
/// Exclusive: the device takes a whole buffer each wake, so no holding is reported; a wake
/// later than two periods after the one before is counted as dry instead.
fn histogram(
    periods: &[live_kit::play::Period],
    rate: u32,
    buffer: usize,
    exclusive: bool,
) -> (Histogram, Option<(f64, f64)>) {
    let mut h = Histogram::default();
    if periods.is_empty() {
        return (h, None);
    }
    // The period: what one wake asks for, typically.
    let sizes: Vec<f64> = periods.iter().map(|p| p.frames as f64).collect();
    let period_frames = live_kit::stats::median(&sizes).unwrap_or(1.0).max(1.0);
    let period_100ns = period_frames / f64::from(rate) * 1e7;
    // The first periods fill the device from cold; the loop settles within 10.
    let settled = &periods[periods.len().min(10)..];
    let mut head = Vec::new();
    let mut prev_wake: Option<u64> = None;
    for p in settled {
        let used = (p.done.saturating_sub(p.wake)) as f64;
        h.record(1.0 - used / period_100ns);
        if exclusive {
            if prev_wake.is_some_and(|w| (p.wake.saturating_sub(w)) as f64 > 2.0 * period_100ns) {
                h.ran_dry += 1;
            }
        } else {
            let held = buffer.saturating_sub(p.free);
            head.push(held as f64 / f64::from(rate) * 1000.0);
            if held == 0 {
                h.ran_dry += 1;
            }
        }
        prev_wake = Some(p.wake);
    }
    let first = settled.first().map_or(0, |p| p.wake);
    let last = settled.last().map_or(0, |p| p.done);
    h.seconds = last.saturating_sub(first) / 10_000_000;
    let head = (!head.is_empty()).then(|| {
        (
            head.iter().fold(f64::MAX, |m, &v| m.min(v)),
            live_kit::stats::quantile(&head, 0.0001).unwrap_or_default(),
        )
    });
    (h, head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use live_kit::play::Period;

    #[test]
    fn margins_come_from_the_wake_to_the_handing_back() {
        // 10 ms periods at 48 kHz, a 50 ms buffer; the loop takes 2 ms, then once 9 ms, then
        // once 12 ms (late); once the device held nothing.
        let mut ps = Vec::new();
        let mut t = 0u64;
        for i in 0..1000 {
            let used = match i {
                500 => 90_000,
                700 => 120_000,
                _ => 20_000,
            };
            ps.push(Period {
                wake: t,
                done: t + used,
                frames: 480,
                queued: None,
                free: if i == 800 { 2400 } else { 480 },
            });
            t += 100_000;
        }
        let (h, head) = histogram(&ps, 48_000, 2400, false);
        assert_eq!(h.periods(), 990);
        assert_eq!(h.late, 1);
        assert_eq!(h.ran_dry, 1);
        assert_eq!(h.low(0.5), Some(0.8));
        assert_eq!(head.unwrap().0, 0.0);
        assert!(!h.passes());
    }

    #[test]
    fn exclusive_mode_counts_a_missed_wake_as_dry() {
        let mut ps = Vec::new();
        let mut t = 0u64;
        for i in 0..100 {
            t += if i == 50 { 250_000 } else { 100_000 };
            ps.push(Period {
                wake: t,
                done: t + 10_000,
                frames: 480,
                queued: None,
                free: 480,
            });
        }
        let (h, head) = histogram(&ps, 48_000, 480, true);
        assert_eq!(h.ran_dry, 1);
        assert!(head.is_none());
    }
}
