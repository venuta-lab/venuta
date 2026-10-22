//! Every detector proves itself before it is trusted (section 7.1 of the Listen together
//! design): each fault it is built for, injected at many places, is caught every time; each
//! clean signal (the room's own shaping included, where declared) passes.
//!
//! The faults: one dropped frame, one repeated frame, a 1-sample step of -40 dBFS at a peak, a
//! 5 ms gap, a hard cut, a gain step. Which detector catches which:
//!
//! | Fault | music | bit-exact | sine lock | LPC | dip and gap |
//! |---|---|---|---|---|---|
//! | dropped frame | most | yes | yes | yes | |
//! | repeated frame | most | yes | yes | yes | |
//! | 1-sample step | yes | yes | yes | yes | |
//! | 5 ms gap | yes | yes | yes | yes | yes |
//! | hard cut | yes | yes | yes | yes | |
//! | gain step | yes | yes | yes | yes | |
//!
//! The dip detector is there for what the others can't judge on real music recorded through a
//! loopback (a level dropping out), not for single frames.

use audio_check::faults::{self, Fault};
use audio_check::music::{FULL, Piece};
use audio_check::{Declared, dip, exact, lpc, music, sine};

const RATE: u32 = 48_000;

/// 2 s of a -6 dBFS sine at `freq`, `ppm` off its rate (a resampler at work).
fn sine_wave(freq: f64, ppm: f64) -> Vec<f64> {
    let w = std::f64::consts::TAU * freq * (1.0 + ppm / 1e6) / f64::from(RATE);
    (0..2 * RATE as usize)
        .map(|n| 0.5 * (w * n as f64 + 0.3).sin())
        .collect()
}

/// 2 s of the test music (one channel), as -1..1.
fn music_wave() -> Vec<f64> {
    let p = Piece {
        rate: RATE,
        channels: 1,
    };
    (0..2 * RATE as usize)
        .map(|i| p.at(i as f64, 0) / FULL)
        .collect()
}

/// The same music as 24-bit values.
fn music_ints() -> Vec<i32> {
    music_wave()
        .iter()
        .map(|v| (v * FULL).round() as i32)
        .collect()
}

/// Places to inject at: spread over the signal, at moments of every steepness.
fn places() -> impl Iterator<Item = usize> {
    (0..12).map(|k| 20_000 + k * 4_129)
}

/// A place at a peak of `x` near `around` (where the wave is flattest).
fn peak_near(x: &[f64], around: usize) -> usize {
    (around..around + 2_000)
        .max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs()))
        .unwrap()
}

/// A place at a zero crossing of `x` near `around` (where it is steepest).
fn zero_near(x: &[f64], around: usize) -> usize {
    (around..around + 2_000)
        .find(|&i| x[i] <= 0.0 && x[i + 1] > 0.0)
        .unwrap()
}

/// Every fault of the list, by name.
fn all_faults() -> Vec<Fault> {
    vec![
        Fault::DropFrame,
        Fault::RepeatFrame,
        Fault::Step { db: -40.0 },
        Fault::Gap { ms: 5.0 },
        Fault::HardCut { ms: 300.0 },
        Fault::GainStep { db: -1.0 },
    ]
}

// --- Detector 0: the music-shaped detector (the fast first check) --------------------------

#[test]
fn the_music_detector_passes_clean_music_and_catches_each_fault() {
    let p = Piece {
        rate: RATE,
        channels: 1,
    };
    let (s1, s3) = p.limits();
    let clean = music_ints();
    assert_eq!(music::clicks(&clean, s1, s3), []);
    // It judges the line of the wave, not its level: a 1 dB step where the wave crosses zero
    // bends the line too little for it (the other detectors catch that one), so its gain step
    // is 6 dB, as it always was.
    let faults_here = all_faults().into_iter().map(|f| match f {
        Fault::GainStep { .. } => Fault::GainStep { db: -6.0 },
        f => f,
    });
    for f in faults_here {
        let mut missed = 0;
        for at in places() {
            let x = faults::inject_ints(&clean, f, at, RATE);
            if music::clicks(&x, s1, s3).is_empty() {
                missed += 1;
            }
        }
        // A frame dropped or repeated where the wave is nearly flat is heard as almost nothing.
        let allowed = if f.is_single_frame() { 3 } else { 0 };
        assert!(missed <= allowed, "{f:?}: missed {missed} of 12");
    }
}

/// The music at fractional frames: `frames` frames from `t0`, as 24-bit values (a resampler's
/// output is the same music at another phase).
fn music_from(t0: f64, frames: usize) -> Vec<i32> {
    let p = Piece {
        rate: RATE,
        channels: 1,
    };
    (0..frames)
        .map(|n| p.at(t0 + n as f64, 0).round() as i32)
        .collect()
}

#[test]
fn the_music_detector_reads_a_lone_zero_on_the_line_as_music() {
    // Resampled music puts a zero crossing exactly on a frame now and then: one exact zero
    // between two large values of opposite sign (as -217384, 0, 256673 heard in a room). It
    // lies on the music's line, so it is music, not silence cut in.
    let p = Piece {
        rate: RATE,
        channels: 1,
    };
    let (s1, s3) = p.limits();
    let at = |t: f64| p.at(t, 0);
    for k in places() {
        // The steepest crossing near `k`, its exact time by bisection.
        let i = (k..k + 2_000)
            .filter(|&i| at(i as f64) <= 0.0 && at(i as f64 + 1.0) > 0.0)
            .max_by(|&a, &b| {
                (at(a as f64 + 1.0) - at(a as f64)).total_cmp(&(at(b as f64 + 1.0) - at(b as f64)))
            })
            .unwrap();
        let (mut lo, mut hi) = (i as f64, i as f64 + 1.0);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if at(mid) <= 0.0 { lo = mid } else { hi = mid }
        }
        // 40,000 frames with the crossing on frame 20,000.
        let x = music_from(lo - 20_000.0, 40_000);
        assert_eq!(x[20_000], 0);
        assert!(
            f64::from(x[19_999].abs().min(x[20_001].abs())) > s1 * 0.5,
            "a steep crossing: {:?}",
            &x[19_998..20_003]
        );
        assert_eq!(music::clicks(&x, s1, s3), [], "the crossing near {k}");
    }
}

#[test]
fn the_music_detector_still_catches_a_frame_zeroed_off_the_line() {
    let p = Piece {
        rate: RATE,
        channels: 1,
    };
    let (s1, s3) = p.limits();
    let clean = music_ints();
    let mut tried = 0;
    for k in places() {
        // A frame where the music is well away from zero: zeroed, it leaves the line.
        let at = (k..k + 2_000)
            .find(|&i| f64::from(clean[i].abs()) > s1)
            .unwrap();
        let mut x = clean.clone();
        x[at] = 0;
        let found = music::clicks(&x, s1, s3);
        assert!(!found.is_empty(), "a frame zeroed at {at} ({})", clean[at]);
        assert!(found[0].at.abs_diff(at) < 4, "found at {}", found[0].at);
        tried += 1;
    }
    assert_eq!(tried, 12);
}

// --- Detector 1: bit-exact with declared edits ---------------------------------------------

#[test]
fn bit_exact_passes_the_source_and_declared_edits_only() {
    let src = music_ints();
    let r = exact::Samples(&src);
    // The source as it is, with silence before and after (a device before and after the music).
    let mut heard = vec![0; 1_000];
    heard.extend_from_slice(&src[5_000..60_000]);
    heard.extend(vec![0; 1_000]);
    assert_eq!(exact::check(&heard, &r, &Declared::none()), []);
    // A declared fade (samples changed) and a declared resampled span (the place moved).
    let mut edited = src[..60_000].to_vec();
    for (k, v) in edited[10_000..10_720].iter_mut().enumerate() {
        *v = (f64::from(*v) * k as f64 / 720.0) as i32;
    }
    edited.drain(30_000..30_003);
    let declared = Declared::new(vec![10_000..10_720, 29_990..30_010]);
    assert_eq!(exact::check(&edited, &r, &declared), []);
    // Undeclared, the same edits are caught.
    assert!(!exact::check(&edited, &r, &Declared::none()).is_empty());
}

#[test]
fn bit_exact_catches_every_fault_at_every_place() {
    let src = music_ints();
    let r = exact::Samples(&src);
    for f in all_faults() {
        for at in places() {
            let x = faults::inject_ints(&src, f, at, RATE);
            let found = exact::check(&x, &r, &Declared::none());
            assert!(!found.is_empty(), "{f:?} at {at}: missed");
            assert!(
                found[0].at.abs_diff(at) < 64,
                "{f:?} at {at}: found at {}",
                found[0].at
            );
        }
    }
}

#[test]
fn bit_exact_knows_counting_songs_by_their_values() {
    // A counting song: frame k holds base + k + 1.
    let r = exact::Counting {
        base: 2_000_000,
        frames: 50_000,
    };
    let x: Vec<i32> = (0..50_000).map(|k| 2_000_000 + k + 1).collect();
    assert_eq!(exact::check(&x, &r, &Declared::none()), []);
    let mut dropped = x.clone();
    dropped.remove(20_000);
    let found = exact::check(&dropped, &r, &Declared::none());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].what, "a frame dropped");
    let mut repeated = x.clone();
    repeated.insert(20_000, repeated[20_000]);
    assert_eq!(
        exact::check(&repeated, &r, &Declared::none())[0].what,
        "a frame repeated"
    );
}

// --- Detector 2: sine lock -----------------------------------------------------------------

#[test]
fn the_sine_lock_passes_clean_sines_and_the_resampler_at_its_limit() {
    for freq in [1_000.0, 15_000.0] {
        for ppm in [0.0, 500.0, -500.0] {
            let x = sine_wave(freq, ppm);
            let found = sine::check(&x, &sine::SineLock::new(freq, RATE, 0.5), &Declared::none());
            assert_eq!(found, [], "{freq} Hz at {ppm} ppm");
        }
    }
}

#[test]
fn the_sine_lock_catches_every_fault_at_every_place() {
    for freq in [1_000.0, 15_000.0] {
        let lock = sine::SineLock::new(freq, RATE, 0.5);
        let clean = sine_wave(freq, 0.0);
        // A cut of a whole number of periods lands on the same phase: no fault at all in a
        // pure sine, so its cut is a quarter period more.
        let faults_here = all_faults().into_iter().map(|f| match f {
            Fault::HardCut { .. } => Fault::HardCut { ms: 300.25 },
            f => f,
        });
        for f in faults_here {
            for at in places() {
                // Single frames at the hardest places too: at a peak and at a zero crossing.
                let ats = [at, peak_near(&clean, at), zero_near(&clean, at)];
                for at in ats {
                    let x = faults::inject(&clean, f, at, RATE);
                    assert!(
                        !sine::check(&x, &lock, &Declared::none()).is_empty(),
                        "{freq} Hz, {f:?} at {at}: missed"
                    );
                }
            }
        }
    }
}

#[test]
fn inside_a_declared_fade_only_the_envelope_counts() {
    let lock = sine::SineLock::new(1_000.0, RATE, 0.5);
    let fade = |x: &mut [f64], from: usize, len: usize, down: bool| {
        for k in 0..len {
            let g = 0.5 - 0.5 * (std::f64::consts::PI * k as f64 / len as f64).cos();
            x[from + k] *= if down { 1.0 - g } else { g };
        }
    };
    // A pause: 30 ms out, 200 ms of silence, 15 ms back in, declared as one span.
    let mut x = sine_wave(1_000.0, 0.0);
    fade(&mut x, 30_000, 1_440, true);
    x[31_440..41_040].iter_mut().for_each(|v| *v = 0.0);
    fade(&mut x, 41_040, 720, false);
    let pause = Declared::span(30_000..41_760);
    assert_eq!(sine::check(&x, &lock, &pause), [], "the pause as declared");
    assert!(
        !sine::check(&x, &lock, &Declared::none()).is_empty(),
        "undeclared, it is a gap"
    );
    // A fade too quick (5 ms: steeper than 1/240 of the level a sample) is a click even when
    // declared, and so is a jump in the level inside a declared span.
    let mut quick = sine_wave(1_000.0, 0.0);
    fade(&mut quick, 30_000, 240, true);
    quick[30_240..41_040].iter_mut().for_each(|v| *v = 0.0);
    fade(&mut quick, 41_040, 720, false);
    assert!(
        !sine::check(&quick, &lock, &pause).is_empty(),
        "a 5 ms fade"
    );
    let mut jump = x.clone();
    jump[41_400..41_760].iter_mut().for_each(|v| *v *= 0.5);
    assert!(
        !sine::check(&jump, &lock, &pause).is_empty(),
        "a level jump"
    );
}

// --- Detector 3: LPC residual on music -----------------------------------------------------

#[test]
fn the_lpc_detector_calibrated_on_solo_passes_clean_music() {
    let x = music_wave();
    let check = lpc::Lpc::calibrate(&x[..RATE as usize], RATE);
    assert_eq!(check.check(&x, &Declared::none()), []);
}

#[test]
fn the_lpc_detector_catches_every_fault_at_every_place() {
    let clean = music_wave();
    let check = lpc::Lpc::calibrate(&clean, RATE);
    for f in all_faults() {
        for at in places() {
            let x = faults::inject(&clean, f, at, RATE);
            let found = check.check(&x, &Declared::none());
            assert!(!found.is_empty(), "{f:?} at {at}: missed");
        }
    }
}

#[test]
fn the_lpc_detector_leaves_declared_spans_alone() {
    let clean = music_wave();
    let check = lpc::Lpc::calibrate(&clean, RATE);
    let x = faults::inject(&clean, Fault::Gap { ms: 5.0 }, 40_000, RATE);
    assert_eq!(check.check(&x, &Declared::span(39_900..40_400)), []);
}

// --- Detector 4: dip and gap ---------------------------------------------------------------

#[test]
fn the_dip_detector_passes_clean_signals_and_declared_pauses() {
    let d = dip::Dip::new(RATE);
    assert_eq!(d.check(&music_wave(), &Declared::none()), []);
    assert_eq!(d.check(&sine_wave(1_000.0, 0.0), &Declared::none()), []);
    // Music that starts and stops (silence around it, its own 50 ms rise and fall) is no dip.
    let p = Piece {
        rate: RATE,
        channels: 1,
    };
    let mut song = vec![0.0; 4_800];
    song.extend((0..48_000).map(|i| f64::from(p.sample(i, 0, 0, 48_000, true, true)) / FULL));
    song.extend(vec![0.0; 4_800]);
    assert_eq!(d.check(&song, &Declared::none()), []);
    // A declared pause.
    let mut x = music_wave();
    x[40_000..50_000].iter_mut().for_each(|v| *v = 0.0);
    assert_eq!(d.check(&x, &Declared::span(39_000..51_000)), []);
}

#[test]
fn the_dip_detector_catches_every_gap() {
    let d = dip::Dip::new(RATE);
    for wave in [
        music_wave(),
        sine_wave(1_000.0, 0.0),
        sine_wave(15_000.0, 0.0),
    ] {
        for at in places() {
            let x = faults::inject(&wave, Fault::Gap { ms: 5.0 }, at, RATE);
            assert!(
                !d.check(&x, &Declared::none()).is_empty(),
                "gap at {at}: missed"
            );
        }
    }
}

// --- The faults themselves -----------------------------------------------------------------

#[test]
fn the_faults_are_what_they_say() {
    let x: Vec<f64> = (0..10_000).map(|i| i as f64).collect();
    assert_eq!(faults::inject(&x, Fault::DropFrame, 100, RATE)[100], 101.0);
    let r = faults::inject(&x, Fault::RepeatFrame, 100, RATE);
    assert_eq!((r[100], r[101], r[102]), (100.0, 100.0, 101.0));
    let s = faults::inject(&x, Fault::Step { db: -40.0 }, 100, RATE);
    assert!(
        (s[100] - 100.0).abs() > 0.009 && s[101] == 101.0,
        "one sample moved"
    );
    let g = faults::inject(&x, Fault::Gap { ms: 5.0 }, 100, RATE);
    assert!(g[100..340].iter().all(|&v| v == 0.0) && g[340] == 340.0);
    let c = faults::inject(&x, Fault::HardCut { ms: 100.0 }, 100, RATE);
    assert_eq!((c[99], c[100]), (99.0, 4_900.0));
    let st = faults::inject(&x, Fault::GainStep { db: -6.0 }, 100, RATE);
    assert!(st[99] == 99.0 && (st[200] / 200.0 - 0.501).abs() < 0.001);
}
