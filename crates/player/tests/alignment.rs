//! Alignment of two copies of a song (design 4.5 and 5.8, the WP10 gates): known offsets are
//! found within 2 ms, codec priming is measured, an edit is caught, repetitive music is turned
//! down rather than placed wrong, and the PCM hash matches only identical audio.
//!
//! Everything here is synthetic: a small synthesizer below writes music-shaped signals (drums,
//! notes, chords, a tempo, a little human timing) straight from event lists, so a shift, a cut
//! or an inserted section is exact to the sample and can be rendered at any rate. The three
//! lossy fixtures are that synthesizer's output encoded by ffmpeg (see `make_fixtures`); no
//! real recording is used anywhere.

use player::analysis::{self, Verdict};
use player::decode::{DecodeState, Source};
use std::path::{Path, PathBuf};
use std::time::Instant;
use together::align::{Alignment, CopyClass, Envelope};

// ---------------------------------------------------------------------------------------
// The synthesizer
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Kind {
    Kick,
    Snare,
    Hat,
    Note { freq: f64, len: f64 },
    Pad { freq: f64, len: f64 },
}

#[derive(Clone, Copy, Debug)]
struct Event {
    t: f64,
    kind: Kind,
    amp: f64,
    id: u64,
}

/// A small deterministic generator (splitmix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn hash_noise(id: u64, i: u64) -> f64 {
    let mut r = Rng(id.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ i);
    r.f() * 2.0 - 1.0
}

/// A song: a tempo between 100 and 130 BPM, drums with a little human timing, a melody on a
/// scale, a chord per bar. Varied enough that no two bars are the same.
fn song(seed: u64, secs: f64) -> Vec<Event> {
    let mut rng = Rng(seed);
    let bpm = 100.0 + rng.f() * 30.0;
    let beat = 60.0 / bpm;
    let scale = [0, 2, 3, 5, 7, 8, 10, 12, 14, 15];
    let root = 110.0 * 2f64.powf(rng.f());
    let mut events = Vec::new();
    let mut id = seed << 32;
    let mut push = |events: &mut Vec<Event>, t: f64, kind: Kind, amp: f64| {
        id += 1;
        events.push(Event { t, kind, amp, id });
    };
    let mut b = 0u64;
    loop {
        let t0 = b as f64 * beat;
        if t0 >= secs {
            break;
        }
        let in_bar = b % 4;
        let jit = |rng: &mut Rng| (rng.f() - 0.5) * 0.012;
        if in_bar == 0 || in_bar == 2 || rng.f() < 0.12 {
            let t = t0 + jit(&mut rng);
            push(&mut events, t, Kind::Kick, 0.6 + 0.3 * rng.f());
        }
        if (in_bar == 1 || in_bar == 3) && rng.f() < 0.95 {
            let t = t0 + jit(&mut rng);
            push(&mut events, t, Kind::Snare, 0.4 + 0.3 * rng.f());
        }
        for e in 0..2 {
            if rng.f() < 0.8 {
                let t = t0 + e as f64 * beat / 2.0 + jit(&mut rng);
                push(&mut events, t, Kind::Hat, 0.1 + 0.35 * rng.f());
            }
        }
        for s in 0..4 {
            if rng.f() < 0.3 {
                let step = scale[(rng.next() % scale.len() as u64) as usize];
                let freq = root * 2f64.powf(1.0 + step as f64 / 12.0);
                let len = 0.08 + rng.f() * 0.5;
                let t = t0 + s as f64 * beat / 4.0 + jit(&mut rng);
                push(
                    &mut events,
                    t,
                    Kind::Note { freq, len },
                    0.15 + 0.3 * rng.f(),
                );
            }
        }
        if in_bar == 0 {
            for _ in 0..3 {
                let step = scale[(rng.next() % 7) as usize];
                let freq = root * 2f64.powf(step as f64 / 12.0);
                push(
                    &mut events,
                    t0,
                    Kind::Pad {
                        freq,
                        len: 4.0 * beat,
                    },
                    0.05,
                );
            }
        }
        b += 1;
    }
    events
}

/// One bar of `song(seed)`, repeated exactly for `secs`: a loop.
fn looped(seed: u64, bars: usize, secs: f64, wobble: f64) -> Vec<Event> {
    let mut rng = Rng(seed ^ 0xABCD);
    let bpm = 100.0 + Rng(seed).f() * 30.0;
    let bar = 4.0 * 60.0 / bpm;
    let one: Vec<Event> = song(seed, bar * bars as f64)
        .into_iter()
        .filter(|e| e.t >= 0.0 && e.t < bar * bars as f64)
        .collect();
    let period = bar * bars as f64;
    let mut out = Vec::new();
    let mut k = 0u64;
    while (k as f64) * period < secs {
        for e in &one {
            let amp = e.amp * (1.0 + wobble * (rng.f() - 0.5));
            out.push(Event {
                t: e.t + k as f64 * period,
                amp,
                id: e.id ^ (k << 20),
                ..*e
            });
        }
        k += 1;
    }
    out
}

fn shifted(events: &[Event], by: f64) -> Vec<Event> {
    events.iter().map(|e| Event { t: e.t + by, ..*e }).collect()
}

/// My copy, `offset` seconds later than the reference: a different intro first when it is
/// later, the first `-offset` seconds missing when it is earlier.
fn offset_copy(events: &[Event], offset: f64, secs: f64) -> Vec<Event> {
    let mut out = shifted(events, offset);
    if offset > 0.0 {
        out.extend(
            song(777, offset)
                .into_iter()
                .filter(|e| e.t < offset - 0.05),
        );
    }
    out.retain(|e| e.t < secs);
    out
}

/// Cut `len` seconds at `at`.
fn cut(events: &[Event], at: f64, len: f64) -> Vec<Event> {
    events
        .iter()
        .filter(|e| e.t < at - 0.6 || e.t >= at + len)
        .map(|e| {
            if e.t >= at {
                Event { t: e.t - len, ..*e }
            } else {
                *e
            }
        })
        .collect()
}

/// Insert `len` seconds of other music at `at`.
fn insert(events: &[Event], at: f64, len: f64) -> Vec<Event> {
    let mut out: Vec<Event> = events
        .iter()
        .map(|e| {
            if e.t >= at {
                Event { t: e.t + len, ..*e }
            } else {
                *e
            }
        })
        .filter(|e| !(e.t >= at - 0.6 && e.t < at))
        .collect();
    out.extend(shifted(
        &song(4242, len)
            .into_iter()
            .filter(|e| e.t < len - 0.6)
            .collect::<Vec<_>>(),
        at,
    ));
    out
}

/// How a copy is made: its rate, channels, gain, and a "master" (a gentle low-pass and a
/// little noise) so it is not the same samples.
#[derive(Clone, Copy)]
struct Master {
    rate: u32,
    ch: u16,
    gain: f64,
    lowpass_hz: Option<f64>,
    noise_db: Option<f64>,
}

const REF: Master = Master {
    rate: 44_100,
    ch: 2,
    gain: 1.0,
    lowpass_hz: None,
    noise_db: None,
};

const OTHER: Master = Master {
    rate: 48_000,
    ch: 2,
    gain: 0.7,
    lowpass_hz: Some(9_000.0),
    noise_db: Some(-62.0),
};

fn voice(e: &Event, tau: f64, i: u64) -> f64 {
    use std::f64::consts::TAU;
    match e.kind {
        Kind::Kick => {
            let ph = TAU * (50.0 * tau + 100.0 * 0.04 * (1.0 - (-tau / 0.04).exp()));
            ph.sin() * (-tau / 0.25).exp()
        }
        Kind::Snare => {
            hash_noise(e.id, i) * 0.6 * (-tau / 0.07).exp()
                + (TAU * 190.0 * tau).sin() * 0.4 * (-tau / 0.05).exp()
        }
        Kind::Hat => hash_noise(e.id, i) * (TAU * 8_000.0 * tau).sin() * (-tau / 0.02).exp(),
        Kind::Note { freq, len } => {
            let env = (tau / 0.005).min(1.0) * (-tau / (len * 0.6)).exp();
            let mut s = 0.0;
            for h in 1..=4 {
                s += (TAU * freq * h as f64 * tau).sin() / h as f64;
            }
            s * env
        }
        Kind::Pad { freq, len } => {
            let env = (tau / 0.3).min(1.0) * ((len - tau) / 0.3).clamp(0.0, 1.0);
            (TAU * freq * tau).sin() * env
        }
    }
}

fn duration(kind: Kind) -> f64 {
    match kind {
        Kind::Kick => 0.8,
        Kind::Snare => 0.4,
        Kind::Hat => 0.15,
        Kind::Note { len, .. } => len * 4.0 + 0.05,
        Kind::Pad { len, .. } => len,
    }
}

/// Mono samples in [-1, 1] at `rate`, `secs` long.
fn render_mono(events: &[Event], secs: f64, rate: u32) -> Vec<f64> {
    let n = (secs * rate as f64).round() as usize;
    let mut out = vec![0.0f64; n];
    for e in events {
        let start = (e.t * rate as f64).ceil().max(0.0) as usize;
        let end = (((e.t + duration(e.kind)) * rate as f64).ceil() as usize).min(n);
        for (i, o) in out.iter_mut().enumerate().take(end).skip(start) {
            let tau = i as f64 / rate as f64 - e.t;
            *o += e.amp * voice(e, tau, i as u64);
        }
    }
    for o in &mut out {
        *o = (*o * 0.5).tanh();
    }
    out
}

/// Interleaved left-justified samples (16-bit values shifted up), as the decoder gives them.
fn render(events: &[Event], secs: f64, m: Master) -> Vec<i32> {
    let mut mono = render_mono(events, secs, m.rate);
    if let Some(hz) = m.lowpass_hz {
        let a = (-std::f64::consts::TAU * hz / m.rate as f64).exp();
        let mut y = 0.0;
        for s in &mut mono {
            y = (1.0 - a) * *s + a * y;
            *s = y;
        }
    }
    let mut rng = Rng(9);
    let noise = m.noise_db.map(|db| 10f64.powf(db / 20.0)).unwrap_or(0.0);
    let mut out = Vec::with_capacity(mono.len() * m.ch as usize);
    for s in mono {
        for c in 0..m.ch {
            let v = s * m.gain * if c == 1 { 0.9 } else { 1.0 } + noise * (rng.f() * 2.0 - 1.0);
            let q = (v.clamp(-1.0, 1.0) * 32_767.0).round() as i32;
            out.push(q << 16);
        }
    }
    out
}

fn envelope_of(samples: &[i32], rate: u32, ch: u16) -> Envelope {
    let block = 4_096 * ch as usize;
    analysis::envelope(&mut samples.chunks(block), rate, ch)
}

fn env(events: &[Event], secs: f64, m: Master) -> Envelope {
    envelope_of(&render(events, secs, m), m.rate, m.ch)
}

fn err_ms(offset_us: i64, truth_s: f64) -> f64 {
    (offset_us as f64 / 1_000.0 - truth_s * 1_000.0).abs()
}

// ---------------------------------------------------------------------------------------
// The envelope itself
// ---------------------------------------------------------------------------------------

#[test]
fn the_envelope_has_the_windows_of_the_design() {
    let secs = 131.0;
    let e = env(&song(1, secs), secs, REF);
    assert_eq!(e.hop_ms, 10);
    let shape: Vec<(u32, usize)> = e.windows.iter().map(|(s, w)| (*s, w.len())).collect();
    // The first 30 s, then 10 s every 60 s.
    assert_eq!(shape, vec![(0, 3_000), (60_000, 1_000), (120_000, 1_000)]);
    assert!(e.pcm_hash.is_some());
    // Onsets show: the envelope is not flat.
    let busy = e.windows[0].1.iter().filter(|&&b| b >= 24).count();
    assert!(busy > 100, "only {busy} strong hops in 30 s");

    // A short song: one window, cut where the song ends.
    let short = env(&song(1, 21.0), 21.0, REF);
    assert_eq!(short.windows.len(), 1);
    let len = short.windows[0].1.len();
    assert!((2_090..=2_100).contains(&len), "{len} hops for 21 s");
}

#[test]
fn the_envelope_does_not_depend_on_how_the_audio_is_cut_into_blocks() {
    let pcm = render(&song(3, 70.0), 70.0, REF);
    let whole = envelope_of(&pcm, REF.rate, REF.ch);
    for block in [1usize, 7, 441, 4_095, 100_000] {
        // Blocks of any size, even ones that split a frame between channels.
        let e = analysis::envelope(&mut pcm.chunks(block), REF.rate, REF.ch);
        assert_eq!(e, whole, "blocks of {block} samples");
    }
}

#[test]
fn an_empty_or_silent_song_gives_nothing_to_align() {
    let e = analysis::envelope(&mut std::iter::empty::<&[i32]>(), 44_100, 2);
    assert!(e.windows.is_empty());
    assert_eq!(e.pcm_hash, None);
    let silence = vec![0i32; 44_100 * 2 * 40];
    let s = envelope_of(&silence, 44_100, 2);
    let a = analysis::align(&s, &s.clone_without_hash());
    assert_eq!(a.class, CopyClass::Approximate);
    assert_eq!(a.offset_us, 0);
}

trait NoHash {
    fn clone_without_hash(&self) -> Envelope;
}
impl NoHash for Envelope {
    fn clone_without_hash(&self) -> Envelope {
        Envelope {
            pcm_hash: None,
            ..self.clone()
        }
    }
}

// ---------------------------------------------------------------------------------------
// Gate: known offsets within 2 ms
// ---------------------------------------------------------------------------------------

#[test]
fn known_offsets_up_to_3_s_are_found_within_2_ms() {
    let secs = 40.0;
    let events = song(1, secs + 4.0);
    let reference = env(&events, secs, REF);
    let mut worst: f64 = 0.0;
    for offset in [
        3.0, -3.0, 2.987_6, -1.234_5, 0.0, 0.037_1, -0.004_9, 1.000_02,
    ] {
        let mine = env(&offset_copy(&events, offset, secs), secs, OTHER);
        let a = analysis::align(&reference, &mine);
        let e = err_ms(a.offset_us, offset);
        println!(
            "offset {offset:+.4} s: measured {:+.4} s, error {e:.3} ms, {:?}, confidence {:.2}",
            a.offset_us as f64 / 1e6,
            a.class,
            a.confidence
        );
        assert_eq!(a.class, CopyClass::Measured, "offset {offset}");
        assert!(e <= 2.0, "offset {offset}: off by {e:.3} ms");
        assert!(a.confidence >= 8.0, "confidence {}", a.confidence);
        worst = worst.max(e);
    }
    println!("worst error {worst:.3} ms");
}

#[test]
fn offsets_are_found_on_other_songs_and_rates_too() {
    let secs = 35.0;
    let mut worst: f64 = 0.0;
    for (seed, offset, m) in [
        (11, 2.5, OTHER),
        (12, -2.75, OTHER),
        (
            13,
            0.333,
            Master {
                rate: 22_050,
                ch: 1,
                ..OTHER
            },
        ),
        (
            14,
            -0.9,
            Master {
                rate: 96_000,
                ch: 2,
                gain: 1.0,
                ..OTHER
            },
        ),
        (
            15,
            1.75,
            Master {
                rate: 44_100,
                ch: 1,
                gain: 0.3,
                ..OTHER
            },
        ),
    ] {
        let events = song(seed, secs + 4.0);
        let reference = env(&events, secs, REF);
        let mine = env(&offset_copy(&events, offset, secs), secs, m);
        let a = analysis::align(&reference, &mine);
        let e = err_ms(a.offset_us, offset);
        println!(
            "seed {seed} offset {offset:+} at {} Hz: error {e:.3} ms, {:?}",
            m.rate, a.class
        );
        assert_eq!(a.class, CopyClass::Measured, "seed {seed}");
        assert!(e <= 2.0, "seed {seed}: off by {e:.3} ms");
        worst = worst.max(e);
    }
    println!("worst error {worst:.3} ms");
}

#[test]
fn later_windows_agree_through_a_whole_song() {
    let secs = 190.0;
    let events = song(21, secs + 4.0);
    let reference = env(&events, secs, REF);
    let offset = -2.2;
    let mine = env(&offset_copy(&events, offset, secs), secs, OTHER);
    let track = analysis::align_track(&reference, &mine);
    assert_eq!(track.alignment.class, CopyClass::Measured);
    assert!(err_ms(track.alignment.offset_us, offset) <= 2.0);
    assert_eq!(track.edit_at_ms, None);
    let starts: Vec<u32> = track.windows.iter().map(|w| w.start_ms).collect();
    assert_eq!(starts, vec![60_000, 120_000, 180_000]);
    for w in &track.windows {
        match w.verdict {
            Verdict::Agrees { offset_us } => {
                assert!(err_ms(offset_us, offset) <= 2.0, "window {}", w.start_ms)
            }
            other => panic!("window {}: {other:?}", w.start_ms),
        }
    }
}

#[test]
fn unrelated_songs_are_not_aligned() {
    let secs = 40.0;
    for (a, b) in [(1, 2), (5, 6), (7, 8)] {
        let x = env(&song(a, secs), secs, REF);
        let y = env(&song(b, secs), secs, OTHER);
        let r = analysis::align(&x, &y);
        assert_eq!(r.class, CopyClass::Approximate, "songs {a} and {b}");
        assert_eq!(r.offset_us, 0);
    }
}

#[test]
fn an_intro_longer_than_the_search_is_approximate() {
    // A video intro of 8 s: beyond the 5 s the first window searches.
    let secs = 40.0;
    let events = song(31, secs);
    let reference = env(&events, secs, REF);
    let mine = env(&offset_copy(&events, 8.0, secs), secs, OTHER);
    let a = analysis::align(&reference, &mine);
    assert_eq!(a.class, CopyClass::Approximate);
}

// ---------------------------------------------------------------------------------------
// Gate: encoder delays (MP3, AAC and Opus priming)
// ---------------------------------------------------------------------------------------

const FIXTURE_SEED: u64 = 51;
const FIXTURE_SECS: f64 = 20.0;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn decode_all(path: &Path) -> (u32, u16, Vec<i32>) {
    let mut source = Source::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (rate, ch) = (source.format.rate, source.format.channels);
    let mut all = Vec::new();
    let mut out = Vec::new();
    loop {
        match source.next(&mut out).expect("decodes") {
            DecodeState::Samples => all.extend_from_slice(&out),
            DecodeState::End => break,
            DecodeState::Pending => unreachable!("a local file never waits"),
        }
    }
    (rate, ch, all)
}

/// The true delay of a decoded copy against the synthesizer's own samples, in samples, by
/// brute-force cross-correlation at the sample level (independent of the envelope).
fn true_delay(decoded: &[i32], ch: u16, rate: u32) -> i64 {
    let ch = ch as usize;
    let mono: Vec<f64> = decoded
        .chunks(ch)
        .map(|f| f.iter().map(|&s| s as f64).sum::<f64>() / ch as f64)
        .collect();
    let reference = render_mono(&song(FIXTURE_SEED, FIXTURE_SECS), FIXTURE_SECS, rate);
    let (from, len) = (2 * rate as usize, rate as usize / 2);
    let mut best = (f64::MIN, 0i64);
    for lag in -2_048i64..=6_000 {
        let mut dot = 0.0;
        let mut e = 0.0;
        let start = (from as i64 + lag) as usize;
        for (r, m) in reference[from..from + len]
            .iter()
            .zip(&mono[start..start + len])
        {
            dot += r * m;
            e += m * m;
        }
        let score = dot / e.sqrt().max(1e-9);
        if score > best.0 {
            best = (score, lag);
        }
    }
    best.1
}

#[test]
fn codec_priming_is_measured_within_2_ms() {
    let reference_pcm = render(&song(FIXTURE_SEED, FIXTURE_SECS), FIXTURE_SECS, REF);
    let reference = envelope_of(&reference_pcm, REF.rate, REF.ch);
    for name in [
        "align-music.mp3",
        "align-music-raw.mp3",
        "align-music.m4a",
        "align-music.opus",
    ] {
        let (rate, ch, pcm) = decode_all(&fixture(name));
        let truth = true_delay(&pcm, ch, rate);
        let truth_s = truth as f64 / rate as f64;
        let mine = envelope_of(&pcm, rate, ch);
        let a = analysis::align(&reference, &mine);
        let e = err_ms(a.offset_us, truth_s);
        println!(
            "{name}: {rate} Hz, true delay {truth} samples ({:.2} ms), measured {:.2} ms, error {e:.3} ms, {:?}",
            truth_s * 1e3,
            a.offset_us as f64 / 1e3,
            a.class
        );
        assert_eq!(a.class, CopyClass::Measured, "{name}");
        assert!(e <= 2.0, "{name}: off by {e:.3} ms");
    }
}

/// Writes the lossy fixtures: the synthesizer's song as a WAV, encoded by ffmpeg as MP3 (LAME,
/// with and without its gapless header),
/// AAC in MP4 and Opus in Ogg, mono at low bitrates to keep them small. Run by hand:
/// `FFMPEG=path/to/ffmpeg cargo test -p player --test alignment make_fixtures -- --ignored`.
#[test]
#[ignore = "writes the fixtures; needs ffmpeg"]
fn make_fixtures() {
    let ffmpeg = std::env::var("FFMPEG").expect("FFMPEG names ffmpeg");
    let dir = std::env::temp_dir().join("venuta-align-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("align-music.wav");
    let m = Master { ch: 1, ..REF };
    write_wav(
        &wav,
        &render(&song(FIXTURE_SEED, FIXTURE_SECS), FIXTURE_SECS, m),
        m.rate,
        m.ch,
    );
    for (name, args) in [
        (
            "align-music.mp3",
            &["-c:a", "libmp3lame", "-b:a", "48k", "-ar", "44100"][..],
        ),
        // Without the LAME header the decoder cannot trim the priming: it plays as a delay.
        (
            "align-music-raw.mp3",
            &[
                "-c:a",
                "libmp3lame",
                "-b:a",
                "48k",
                "-ar",
                "44100",
                "-write_xing",
                "0",
            ][..],
        ),
        (
            "align-music.m4a",
            &["-c:a", "aac", "-b:a", "48k", "-ar", "44100"][..],
        ),
        ("align-music.opus", &["-c:a", "libopus", "-b:a", "32k"][..]),
    ] {
        let out = fixture(name);
        let status = std::process::Command::new(&ffmpeg)
            .args(["-y", "-loglevel", "error", "-i"])
            .arg(&wav)
            .args(["-map_metadata", "-1"])
            .args(args)
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success(), "{name}");
    }
}

fn write_wav(path: &Path, samples: &[i32], rate: u32, ch: u16) {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|&s| ((s >> 16) as i16).to_le_bytes())
        .collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&ch.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(ch * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    std::fs::write(path, w).unwrap();
}

// ---------------------------------------------------------------------------------------
// Gate: an edit is detected
// ---------------------------------------------------------------------------------------

#[test]
fn a_cut_after_the_first_window_is_detected_where_it_shows() {
    let secs = 130.0;
    let events = song(41, secs + 4.0);
    let reference = env(&events, secs, REF);
    // A radio edit: 2.5 s cut at 75 s; the copy also starts 0.4 s later.
    let mine_events = offset_copy(&cut(&events, 75.0, 2.5), 0.4, secs);
    let track = analysis::align_track(&reference, &env(&mine_events, secs, OTHER));
    // Aligned at the start as well as can be, but not trusted through the song.
    assert_eq!(track.alignment.class, CopyClass::Approximate);
    assert!(err_ms(track.alignment.offset_us, 0.4) <= 2.0);
    assert_eq!(track.edit_at_ms, Some(120_000));
    assert!(matches!(track.windows[0].verdict, Verdict::Agrees { .. }));
    match track.windows[1].verdict {
        // Where the edit shows, the new offset is measured too.
        Verdict::Moved { offset_us } => assert!(err_ms(offset_us, 0.4 - 2.5) <= 2.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_inserted_section_is_detected_short_or_long() {
    let secs = 130.0;
    let events = song(42, secs + 4.0);
    let reference = env(&events, secs, REF);
    for len in [3.0, 12.0] {
        let mine = env(&insert(&events, 40.0, len), secs, OTHER);
        let track = analysis::align_track(&reference, &mine);
        println!("insert {len} s: {:?}", track.windows);
        assert_eq!(
            track.alignment.class,
            CopyClass::Approximate,
            "insert of {len} s"
        );
        assert!(err_ms(track.alignment.offset_us, 0.0) <= 2.0);
        assert_eq!(track.edit_at_ms, Some(60_000), "insert of {len} s");
    }
}

#[test]
fn a_cut_inside_the_first_window_is_detected() {
    let secs = 40.0;
    let events = song(43, secs + 4.0);
    let reference = env(&events, secs, REF);
    for (at, len, start) in [
        (15.0, 2.0, 0.0),
        (12.0, 0.6, 0.0),
        (20.0, 4.0, 0.0),
        (15.0, 2.0, 0.4),
        (18.0, 1.5, -0.7),
    ] {
        let mine = env(
            &offset_copy(&cut(&events, at, len), start, secs),
            secs,
            OTHER,
        );
        let a = analysis::align(&reference, &mine);
        println!("cut {len} s at {at} s: {a:?}");
        assert_eq!(a.class, CopyClass::Approximate, "cut of {len} s at {at} s");
        // Aligned at the start as well as can be: the copy is the same up to the cut.
        assert!(
            err_ms(a.offset_us, start) <= 2.0,
            "cut of {len} s at {at} s: {a:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Gate: repetitive music is turned down, never placed wrong
// ---------------------------------------------------------------------------------------

#[test]
fn repetitive_music_is_rejected_rather_than_misaligned() {
    let secs = 130.0;
    let mut cases = 0;
    let mut rejected = 0;
    for (seed, bars, wobble) in [
        (61, 1, 0.0),
        (62, 2, 0.0),
        (63, 1, 0.02),
        (64, 2, 0.1),
        (65, 1, 0.3),
    ] {
        let events = looped(seed, bars, secs + 4.0, wobble);
        let reference = env(&events, secs, REF);
        for offset in [3.0, -1.0, 0.5] {
            let mine = env(&offset_copy(&events, offset, secs), secs, OTHER);
            let a = analysis::align(&reference, &mine);
            println!(
                "loop seed {seed} of {bars} bar(s), wobble {wobble}, offset {offset:+}: {:?} at {:+.4} s",
                a.class,
                a.offset_us as f64 / 1e6
            );
            cases += 1;
            if a.class == CopyClass::Measured {
                // Allowed only when it is right.
                assert!(
                    err_ms(a.offset_us, offset) <= 2.0,
                    "loop {seed} offset {offset}: placed at {} us",
                    a.offset_us
                );
            } else {
                rejected += 1;
            }
            // An exact loop can never be placed: every bar is the same.
            if wobble == 0.0 {
                assert_eq!(
                    a.class,
                    CopyClass::Approximate,
                    "exact loop {seed} offset {offset}"
                );
            }
        }
    }
    println!("{rejected} of {cases} loop cases rejected, the rest measured right");
}

// ---------------------------------------------------------------------------------------
// Gate: the PCM hash matches only identical audio
// ---------------------------------------------------------------------------------------

#[test]
fn the_pcm_hash_matches_only_identical_audio() {
    let pcm = render(&song(71, 15.0), 15.0, REF);
    let base = envelope_of(&pcm, REF.rate, REF.ch).pcm_hash.unwrap();
    let hash = |p: &[i32], rate: u32, ch: u16| envelope_of(p, rate, ch).pcm_hash.unwrap();

    // The same audio, cut into blocks any way: the same hash.
    assert_eq!(
        analysis::envelope(&mut pcm.chunks(3), REF.rate, REF.ch).pcm_hash,
        Some(base)
    );

    // The same audio through a file and the decoder: the same hash.
    let dir = std::env::temp_dir().join(format!("venuta-align-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("same.wav");
    write_wav(&wav, &pcm, REF.rate, REF.ch);
    let (rate, ch, decoded) = decode_all(&wav);
    assert_eq!(hash(&decoded, rate, ch), base, "a WAV of the same samples");
    let _ = std::fs::remove_dir_all(&dir);

    // Anything else: another hash.
    let mut one_lsb = pcm.clone();
    one_lsb[REF.rate as usize * 2 * 5] += 1 << 16;
    assert_ne!(
        hash(&one_lsb, REF.rate, REF.ch),
        base,
        "one sample one step off at 5 s"
    );
    let mut late = pcm.clone();
    late.insert(0, 0);
    late.insert(0, 0);
    assert_ne!(hash(&late, REF.rate, REF.ch), base, "one frame later");
    assert_ne!(
        hash(&pcm, 48_000, REF.ch),
        base,
        "the same numbers at another rate"
    );
    assert_ne!(hash(&pcm, REF.rate, 1), base, "the same numbers as mono");
    let quieter: Vec<i32> = pcm.iter().map(|&s| (s >> 17) << 16).collect();
    assert_ne!(hash(&quieter, REF.rate, REF.ch), base, "6 dB quieter");
    let mut swapped = pcm.clone();
    for f in swapped.chunks_mut(2) {
        f.swap(0, 1);
    }
    assert_ne!(hash(&swapped, REF.rate, REF.ch), base, "channels swapped");
    let short = &pcm[..REF.rate as usize * 2 * 8];
    assert_ne!(hash(short, REF.rate, REF.ch), base, "the first 8 s only");
}

#[test]
fn identical_audio_is_exact_unless_a_later_window_shows_an_edit() {
    let secs = 130.0;
    let events = song(72, secs + 4.0);
    let pcm = render(&events, secs, REF);
    let reference = envelope_of(&pcm, REF.rate, REF.ch);
    let same = envelope_of(&pcm, REF.rate, REF.ch);
    let a = analysis::align(&reference, &same);
    assert_eq!(a.class, CopyClass::Exact);
    assert_eq!(a.offset_us, 0);
    assert_eq!(a, Alignment::EXACT);

    // A radio edit whose first 10 s are the same samples: its hash matches, but the cut
    // at 70 s shows in the window at 120 s, so it is not exact.
    let edit = render(&cut(&events, 70.0, 3.0), secs, REF);
    let edit_env = envelope_of(&edit, REF.rate, REF.ch);
    assert_eq!(edit_env.pcm_hash, reference.pcm_hash);
    let a = analysis::align(&reference, &edit_env);
    assert_eq!(a.class, CopyClass::Approximate);
    assert_eq!(a.offset_us, 0);

    // Without the hash, the same audio is measured at 0.
    let a = analysis::align(&reference, &same.clone_without_hash());
    assert_eq!(a.class, CopyClass::Measured);
    assert!(a.offset_us.abs() <= 2_000);
}

// ---------------------------------------------------------------------------------------
// Cost: cheap enough for a low-end handheld
// ---------------------------------------------------------------------------------------

#[test]
fn cost_per_minute_of_audio() {
    // Five minutes of 48 kHz stereo (content does not change the cost: noise-like music).
    let rate = 48_000u32;
    let minute = render(&song(81, 60.0), 60.0, Master { rate, ..REF });
    let five: Vec<i32> = (0..5).flat_map(|_| minute.iter().copied()).collect();
    let runs = 3;
    let mut best_env = f64::MAX;
    let mut env5 = None;
    for _ in 0..runs {
        let t = Instant::now();
        let e = analysis::envelope(&mut five.chunks(4_096), rate, 2);
        best_env = best_env.min(t.elapsed().as_secs_f64());
        env5 = Some(e);
    }
    let env5 = env5.unwrap();
    let analysed_s: f64 = env5
        .windows
        .iter()
        .map(|(_, w)| w.len() as f64 / 100.0)
        .sum();
    let mut best_align = f64::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        let _ = analysis::align_track(&env5, &env5.clone_without_hash());
        best_align = best_align.min(t.elapsed().as_secs_f64());
    }
    println!(
        "envelope: {:.1} ms for 5 min fed ({:.2} ms per minute fed, {:.1} s analysed, {:.2} ms per analysed minute); align: {:.1} ms",
        best_env * 1e3,
        best_env * 1e3 / 5.0,
        analysed_s,
        best_env * 1e3 / (analysed_s / 60.0),
        best_align * 1e3
    );
    // Loose bounds that hold on a busy machine; the real numbers are printed above. A
    // handheld several times slower still does a song in well under a second.
    assert!(best_env < 0.5, "envelope of 5 minutes took {best_env:.3} s");
    assert!(best_align < 0.25, "align took {best_align:.3} s");
}
