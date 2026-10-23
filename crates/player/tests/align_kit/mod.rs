//! The alignment tests' synthesizer and fixtures (WP10's), shared with the room's measure
//! tests (WP11): music-shaped signals written straight from event lists, so a shift, a cut or
//! an inserted section is exact to the sample and can be rendered at any rate; and the lossy
//! fixtures that are its output encoded by ffmpeg.

#![allow(dead_code)]

use player::analysis;
use player::decode::{DecodeState, Source};
use std::path::{Path, PathBuf};
use together::align::Envelope;

// ---------------------------------------------------------------------------------------
// The synthesizer
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Kick,
    Snare,
    Hat,
    Note { freq: f64, len: f64 },
    Pad { freq: f64, len: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub t: f64,
    pub kind: Kind,
    pub amp: f64,
    pub id: u64,
}

/// A small deterministic generator (splitmix64).
pub struct Rng(pub u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn hash_noise(id: u64, i: u64) -> f64 {
    let mut r = Rng(id.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ i);
    r.f() * 2.0 - 1.0
}

/// A song: a tempo between 100 and 130 BPM, drums with a little human timing, a melody on a
/// scale, a chord per bar. Varied enough that no two bars are the same.
pub fn song(seed: u64, secs: f64) -> Vec<Event> {
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
pub fn looped(seed: u64, bars: usize, secs: f64, wobble: f64) -> Vec<Event> {
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

pub fn shifted(events: &[Event], by: f64) -> Vec<Event> {
    events.iter().map(|e| Event { t: e.t + by, ..*e }).collect()
}

/// My copy, `offset` seconds later than the reference: a different intro first when it is
/// later, the first `-offset` seconds missing when it is earlier.
pub fn offset_copy(events: &[Event], offset: f64, secs: f64) -> Vec<Event> {
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
pub fn cut(events: &[Event], at: f64, len: f64) -> Vec<Event> {
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
pub fn insert(events: &[Event], at: f64, len: f64) -> Vec<Event> {
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
pub struct Master {
    pub rate: u32,
    pub ch: u16,
    pub gain: f64,
    pub lowpass_hz: Option<f64>,
    pub noise_db: Option<f64>,
}

pub const REF: Master = Master {
    rate: 44_100,
    ch: 2,
    gain: 1.0,
    lowpass_hz: None,
    noise_db: None,
};

pub const OTHER: Master = Master {
    rate: 48_000,
    ch: 2,
    gain: 0.7,
    lowpass_hz: Some(9_000.0),
    noise_db: Some(-62.0),
};

pub fn voice(e: &Event, tau: f64, i: u64) -> f64 {
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

pub fn duration(kind: Kind) -> f64 {
    match kind {
        Kind::Kick => 0.8,
        Kind::Snare => 0.4,
        Kind::Hat => 0.15,
        Kind::Note { len, .. } => len * 4.0 + 0.05,
        Kind::Pad { len, .. } => len,
    }
}

/// Mono samples in [-1, 1] at `rate`, `secs` long.
pub fn render_mono(events: &[Event], secs: f64, rate: u32) -> Vec<f64> {
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
pub fn render(events: &[Event], secs: f64, m: Master) -> Vec<i32> {
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

pub fn envelope_of(samples: &[i32], rate: u32, ch: u16) -> Envelope {
    let block = 4_096 * ch as usize;
    analysis::envelope(&mut samples.chunks(block), rate, ch)
}

pub fn env(events: &[Event], secs: f64, m: Master) -> Envelope {
    envelope_of(&render(events, secs, m), m.rate, m.ch)
}

pub fn err_ms(offset_us: i64, truth_s: f64) -> f64 {
    (offset_us as f64 / 1_000.0 - truth_s * 1_000.0).abs()
}

// ---------------------------------------------------------------------------------------
// The fixtures
// ---------------------------------------------------------------------------------------

pub const FIXTURE_SEED: u64 = 51;
pub const FIXTURE_SECS: f64 = 20.0;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn decode_all(path: &Path) -> (u32, u16, Vec<i32>) {
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
pub fn true_delay(decoded: &[i32], ch: u16, rate: u32) -> i64 {
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

pub fn write_wav(path: &Path, samples: &[i32], rate: u32, ch: u16) {
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
