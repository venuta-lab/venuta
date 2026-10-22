//! A bench for rooms on engine 2, on one simulated clock the test moves (section 7.2 of the
//! Listen together design): people (each a player on its own simulated output), the room's
//! clock as each guest estimates it (WP6's `Clock`, fed by simulated pings over a simulated
//! path), and the truth: every song has the test music on its left channel and a counting
//! ramp on its right (frame k of a song holds `base + k + 1`), so what each simulated DAC
//! played says, at the exact time it was heard, which place of which song it was. Measured
//! against the room's timeline at the host's true time, that is the error a listener has.

#![allow(dead_code)]

use audio_check::Declared;
use player::engine::{EngineKind, Item, OutputChoice, Player, SimChoice, Status, Watch};
use player::output_api::Output;
use player::sim::{SimConfig, SimHandle, SimOutput};
use player::voice_api::{FadeKind, RenderNote};
use serde::Serialize;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use together::anchor::{Anchor, EntryId};
use together::clock::{Clock, Sample, Steps};
use together::clock_map::{self, ClockMap, ClockMapHandle};
use together::time::{MonoClock, SimClock, Timebase};

pub const R: u32 = 48_000;
/// Full scale of a left-justified sample.
pub const FULL: f64 = 2_147_483_648.0;

/// Device frames (and song frames) for `ms` at 48 kHz.
pub fn ms(ms: u64) -> u64 {
    ms * u64::from(R) / 1000
}

// --- Songs ---------------------------------------------------------------------------------------

/// Where each song's ramp counts from: A, B, C, D (all within 24 bits for 80 s songs).
pub const BASES: [i32; 4] = [0, 4_000_000, -4_000_000, -8_000_000];

/// A song in a folder of its own: `secs` of the test music from `from_s` (left), the counting
/// ramp from `base` (right), 24-bit stereo at 48 kHz, rising from silence over its first 50 ms
/// and falling to it over its last when asked. `quiet`: spans (seconds into the song) where
/// the music is 60 dB down (a quiet moment).
pub fn song(from_s: f64, secs: f64, base: i32, rise: bool, fall: bool) -> PathBuf {
    song_with(from_s, secs, base, rise, fall, &[])
}

pub fn song_with(
    from_s: f64,
    secs: f64,
    base: i32,
    rise: bool,
    fall: bool,
    quiet: &[Range<f64>],
) -> PathBuf {
    let piece = audio_check::music::Piece {
        rate: R,
        channels: 1,
    };
    let frames = (secs * f64::from(R)) as usize;
    let from = (from_s * f64::from(R)) as usize;
    let mut data = Vec::with_capacity(frames * 6);
    for i in 0..frames {
        let mut m = f64::from(piece.sample(i, 0, from, frames, rise, fall));
        let t = i as f64 / f64::from(R);
        for q in quiet {
            // 60 dB down inside, with a 20 ms raised-cosine slope each side.
            let edge = 0.02;
            let g = if t < q.start - edge || t > q.end + edge {
                1.0
            } else if t >= q.start && t <= q.end {
                1e-3
            } else {
                let d = if t < q.start { q.start - t } else { t - q.end } / edge;
                1e-3 + (1.0 - 1e-3) * (0.5 - 0.5 * (std::f64::consts::PI * d).cos())
            };
            m *= g;
        }
        let m = m.round() as i32;
        let c = base + i as i32 + 1;
        data.extend_from_slice(&m.to_le_bytes()[..3]);
        data.extend_from_slice(&c.to_le_bytes()[..3]);
    }
    write_wav(&data, 2)
}

/// A song of a sine at `hz` (-6 dBFS, both channels), for the sine lock.
pub fn sine_song(hz: f64, secs: f64) -> PathBuf {
    let frames = (secs * f64::from(R)) as usize;
    let mut data = Vec::with_capacity(frames * 6);
    for i in 0..frames {
        let v = (0.5 * 8_388_607.0 * (std::f64::consts::TAU * hz * i as f64 / f64::from(R)).sin())
            .round() as i32;
        data.extend_from_slice(&v.to_le_bytes()[..3]);
        data.extend_from_slice(&v.to_le_bytes()[..3]);
    }
    write_wav(&data, 2)
}

fn write_wav(data: &[u8], ch: u16) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "venuta-room2-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    let mut v = Vec::with_capacity(44 + data.len());
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&ch.to_le_bytes());
    v.extend_from_slice(&R.to_le_bytes());
    v.extend_from_slice(&(R * 3 * u32::from(ch)).to_le_bytes());
    v.extend_from_slice(&(3 * ch).to_le_bytes());
    v.extend_from_slice(&24u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v.extend_from_slice(data);
    std::fs::write(&f, v).unwrap();
    f
}

/// The test music's left channel from `from_s`, as left-justified samples (the reference the
/// LPC detector is calibrated on).
pub fn music_reference(from_s: f64, secs: f64) -> Vec<i32> {
    let piece = audio_check::music::Piece {
        rate: R,
        channels: 1,
    };
    let from = (from_s * f64::from(R)) as usize;
    let n = (secs * f64::from(R)) as usize;
    (0..n)
        .map(|i| (piece.at((from + i) as f64, 0).round() as i32) << 8)
        .collect()
}

// --- People ----------------------------------------------------------------------------------------

/// One person's player on the shared clock, and what it said and played.
pub struct Person<T: Item + Serialize> {
    pub player: Option<Player<T>>,
    statuses: Receiver<Status<T>>,
    notes: Receiver<RenderNote>,
    pub outputs: Arc<Mutex<Vec<SimHandle>>>,
    pub cfg: Arc<Mutex<SimConfig>>,
    pub seen: Vec<Status<T>>,
    pub heard_notes: Vec<RenderNote>,
    clock: SimClock,
}

/// A simulated output on `clock` for every output the engine asks for, made with `cfg` as it is
/// then, kept in `outputs`.
pub fn choice(
    clock: &SimClock,
    cfg: &Arc<Mutex<SimConfig>>,
    outputs: &Arc<Mutex<Vec<SimHandle>>>,
) -> OutputChoice {
    let (c, o, k) = (clock.clone(), outputs.clone(), cfg.clone());
    OutputChoice::Sim(SimChoice {
        clock: clock.clone(),
        output: Arc::new(move || {
            let cfg = k.lock().unwrap().clone();
            let (out, handle) = SimOutput::new(Arc::new(c.clone()), cfg);
            o.lock().unwrap().push(handle);
            Box::new(out) as Box<dyn Output>
        }),
    })
}

impl<T: Item + Serialize> Person<T> {
    pub fn new(clock: &SimClock, cfg: SimConfig) -> Person<T> {
        let outputs = Arc::new(Mutex::new(Vec::new()));
        let cfg = Arc::new(Mutex::new(cfg));
        let (tx, statuses) = channel();
        let (notes_tx, notes) = channel();
        let player = Player::spawn_watched(
            EngineKind::Two,
            choice(clock, &cfg, &outputs),
            move |s| {
                let _ = tx.send(s);
            },
            Watch {
                notes: Some(notes_tx),
                wrap: None,
            },
        );
        Person {
            player: Some(player),
            statuses,
            notes,
            outputs,
            cfg,
            seen: Vec::new(),
            heard_notes: Vec::new(),
            clock: clock.clone(),
        }
    }

    pub fn player(&self) -> &Player<T> {
        self.player.as_ref().expect("playing")
    }

    pub fn collect(&mut self) {
        self.seen.extend(self.statuses.try_iter());
        self.heard_notes.extend(self.notes.try_iter());
    }

    pub fn status(&mut self) -> &Status<T> {
        self.collect();
        self.seen.last().expect("a status")
    }

    /// Another output, made as `cfg` says.
    pub fn another_output(&self) -> OutputChoice {
        choice(&self.clock, &self.cfg, &self.outputs)
    }

    /// Quit (its last fade plays out) and give what it played, every output's runs one after
    /// another.
    pub fn finish(&mut self) -> Heard {
        if let Some(p) = self.player.take() {
            drop(p);
        }
        self.clock.pass(500_000_000);
        self.collect();
        let outs = self.outputs.lock().unwrap();
        let mut h = Heard::default();
        for o in outs.iter() {
            for run in o.tape().runs {
                assert!(run.dry.is_empty(), "the simulated DAC never ran dry");
                for k in 0..run.ticks() {
                    h.at_ns.push(run.heard_ns(k));
                }
                h.left.extend(run.left_justified(0));
                h.right.extend(run.left_justified(1));
            }
        }
        h.notes = self.heard_notes.clone();
        h
    }
}

/// What one person's DACs played: each tick's sample, left and right, and when it was heard.
#[derive(Default)]
pub struct Heard {
    pub left: Vec<i32>,
    pub right: Vec<i32>,
    pub at_ns: Vec<f64>,
    pub notes: Vec<RenderNote>,
}

impl Heard {
    /// The right channel as the song's counting values (24-bit).
    pub fn count(&self, k: usize) -> i32 {
        self.right[k] >> 8
    }

    /// The first `n` ticks.
    pub fn upto(&self, n: usize) -> Heard {
        Heard {
            left: self.left[..n].to_vec(),
            right: self.right[..n].to_vec(),
            at_ns: self.at_ns[..n].to_vec(),
            notes: self.notes.clone(),
        }
    }

    /// The ticks where the ramp says the song with `base` played its seconds `from_s..to_s`
    /// (a quiet passage of the song itself, which the dip detector would rightly call a dip),
    /// widened by 50 ms each side.
    pub fn playing_span(&self, base: i32, from_s: f64, to_s: f64) -> Vec<Range<u64>> {
        let (lo, hi) = (
            base + 1 + ((from_s - 0.05) * f64::from(R)) as i32,
            base + 1 + ((to_s + 0.05) * f64::from(R)) as i32,
        );
        let mut out: Vec<Range<u64>> = Vec::new();
        for k in 0..self.right.len() {
            let v = self.count(k);
            if v >= lo && v <= hi {
                match out.last_mut() {
                    Some(r) if r.end == k as u64 => r.end += 1,
                    _ => out.push(k as u64..k as u64 + 1),
                }
            }
        }
        out
    }

    /// The first tick at or after `t_ns`.
    pub fn tick_at(&self, t_ns: f64) -> usize {
        self.at_ns.partition_point(|&t| t < t_ns)
    }
}

// --- The room's clock ------------------------------------------------------------------------------

/// The host's true clock against the simulation's (every player's mono clock): `host_us =
/// h0_us + (local - t0) x (1 + ppm)`.
#[derive(Clone, Copy, Debug)]
pub struct HostTime {
    pub t0_ns: u64,
    pub h0_us: f64,
    pub ppm: f64,
}

impl HostTime {
    /// The host is this computer (the identity, as the host's own map).
    pub fn same() -> HostTime {
        HostTime {
            t0_ns: 0,
            h0_us: 0.0,
            ppm: 0.0,
        }
    }

    pub fn of(&self, local_ns: f64) -> f64 {
        self.h0_us + (local_ns - self.t0_ns as f64) / 1_000.0 * (1.0 + self.ppm * 1e-6)
    }

    /// The map that is exactly true (no estimate).
    pub fn exact_map(&self) -> ClockMapHandle {
        let h = clock_map::handle();
        h.store(Arc::new(Some(ClockMap {
            epoch: 0,
            local_ref_ns: self.t0_ns,
            host_ref_us: self.h0_us.round() as i64,
            rate: 1.0 + self.ppm * 1e-6,
            bound_us: 0,
        })));
        h
    }
}

/// A small deterministic random source (xorshift64*).
pub struct Rng(pub u64);

impl Rng {
    pub fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn exp(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.unit()).ln()
    }
}

/// One way through the network, in ms: a base delay, an exponential jitter, and a share of
/// packets that are much later.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    pub base_ms: f64,
    pub jitter_ms: f64,
    pub spikes: f64,
    pub spike_ms: f64,
}

impl Leg {
    pub fn delay_us(&self, rng: &mut Rng) -> f64 {
        let mut d = self.base_ms + rng.exp(self.jitter_ms);
        if rng.unit() < self.spikes {
            d += rng.unit() * self.spike_ms;
        }
        d * 1_000.0
    }
}

/// A path to the host: up, down, the share of pings lost.
#[derive(Clone, Copy, Debug)]
pub struct Path {
    pub up: Leg,
    pub down: Leg,
    pub loss: f64,
}

/// One Wi-Fi: about 2 ms each way, a little jitter, retries now and then.
pub fn wifi() -> Path {
    let leg = Leg {
        base_ms: 1.5,
        jitter_ms: 0.4,
        spikes: 0.1,
        spike_ms: 30.0,
    };
    Path {
        up: leg,
        down: leg,
        loss: 0.01,
    }
}

/// The internet, direct: 20 ms up, 40 ms down, 10 ms jitter, 1% loss (7.2).
pub fn internet() -> Path {
    Path {
        up: Leg {
            base_ms: 20.0,
            jitter_ms: 10.0,
            spikes: 0.05,
            spike_ms: 40.0,
        },
        down: Leg {
            base_ms: 40.0,
            jitter_ms: 10.0,
            spikes: 0.05,
            spike_ms: 40.0,
        },
        loss: 0.01,
    }
}

/// A guest's clock service in the simulation: pings over `path` (a burst of 20, 50 ms apart,
/// then one a second), answered by the host at its true time, each answer fed to WP6's `Clock`
/// as it arrives and the map published where the player reads it.
pub struct Pinger {
    pub clock: Clock,
    pub handle: ClockMapHandle,
    host: HostTime,
    path: Path,
    rng: Rng,
    next_ns: u64,
    sent: u32,
    /// Answers on their way: (arrival, sample).
    flying: Vec<(u64, Sample)>,
}

impl Pinger {
    pub fn new(host: HostTime, path: Path, steps: Steps, start_ns: u64, seed: u64) -> Pinger {
        Pinger {
            clock: Clock::with_steps(steps),
            handle: clock_map::handle(),
            host,
            path,
            rng: Rng(seed),
            next_ns: start_ns,
            sent: 0,
            flying: Vec::new(),
        }
    }

    /// Everything due by `now_ns`.
    pub fn tick(&mut self, now_ns: u64) {
        while self.next_ns <= now_ns {
            let t1 = self.next_ns as f64 / 1_000.0;
            if self.rng.unit() >= self.path.loss {
                let up = self.path.up.delay_us(&mut self.rng);
                let down = self.path.down.delay_us(&mut self.rng);
                let t2 = self.host.of((t1 + up) * 1_000.0);
                let t3 = t2 + 50.0;
                let t4 = t1 + up + 50.0 + down;
                self.flying.push((
                    (t4 * 1_000.0) as u64,
                    Sample {
                        t1: t1 as i64,
                        t2: t2 as i64,
                        t3: t3 as i64,
                        t4: t4 as i64,
                    },
                ));
            }
            self.sent += 1;
            self.next_ns += if self.sent < 20 {
                50_000_000
            } else {
                1_000_000_000
            };
        }
        self.flying.sort_by_key(|(t, _)| *t);
        while self.flying.first().is_some_and(|(t, _)| *t <= now_ns) {
            let (_, s) = self.flying.remove(0);
            self.clock.add(s);
            self.clock.publish(&self.handle);
        }
    }
}

/// Let `ms` pass on `clock` a millisecond at a time, running the pingers as it goes.
pub fn pass(clock: &SimClock, ms: u64, pingers: &mut [&mut Pinger]) {
    for _ in 0..ms {
        clock.pass(1_000_000);
        let now = clock.now_ns();
        for p in pingers.iter_mut() {
            p.tick(now);
        }
    }
}

// --- The room's timeline, and the truth ---------------------------------------------------------

/// What the room plays, as the test made it: from each host time on, an anchor and the base of
/// that entry's song's ramp.
#[derive(Default, Clone)]
pub struct Timeline {
    pub from: Vec<(i64, Anchor, i32)>,
}

impl Timeline {
    pub fn set(&mut self, from_host_us: i64, a: Anchor, base: i32) {
        self.from.push((from_host_us, a, base));
    }

    /// The room's (base, content in µs) at host time `h`, while it plays.
    pub fn at(&self, h: f64) -> Option<(i32, f64)> {
        let (_, a, base) = self.from.iter().rev().find(|(t, _, _)| (*t as f64) <= h)?;
        if !a.playing || h < a.host_us as f64 {
            return None;
        }
        Some((*base, a.content_us as f64 + (h - a.host_us as f64)))
    }
}

pub fn anchor(entry: u64, content_us: i64, host_us: i64) -> Anchor {
    Anchor {
        entry: EntryId(entry),
        content_us,
        host_us,
        playing: true,
        seq: 0,
    }
}

/// The error heard at each 10 ms of `heard` from `from_ns` to `to_ns` (true local times):
/// (heard time, ms ahead of the room). Ticks where the ramp is not played straight (a fade, a
/// crossfade, silence, another song than the room's) are left out; `ramps_seen` says how many
/// were judged.
pub fn errors(
    heard: &Heard,
    room: &Timeline,
    host: &HostTime,
    from_ns: f64,
    to_ns: f64,
) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let step = 480usize;
    let mut k = heard.tick_at(from_ns);
    while k + 48 < heard.right.len() && heard.at_ns[k] < to_ns {
        let (v, w) = (heard.count(k), heard.count(k + 48));
        let straight = v != 0 && (w - v - 48).abs() <= 1;
        if straight && let Some((base, expect_us)) = room.at(host.of(heard.at_ns[k])) {
            let content = f64::from(v - base - 1);
            if (0.0..f64::from(R) * 200.0).contains(&content) {
                let got_us = content * 1e6 / f64::from(R);
                out.push((heard.at_ns[k], (got_us - expect_us) / 1_000.0));
            }
        }
        k += step;
    }
    out
}

/// The `p` quantile of the errors' sizes, ms.
pub fn quantile(e: &[(f64, f64)], p: f64) -> f64 {
    let mut v: Vec<f64> = e.iter().map(|(_, x)| x.abs()).collect();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return f64::NAN;
    }
    v[((v.len() - 1) as f64 * p).round() as usize]
}

pub fn worst(e: &[(f64, f64)]) -> f64 {
    e.iter().map(|(_, x)| x.abs()).fold(0.0, f64::max)
}

// --- The detectors on a room's music ----------------------------------------------------------

/// The spans the renderer's notes shape (fades, crossfades, gain ramps, pauses) and each song's
/// own rise and fall from silence; not the resampled spans (in a room everything is resampled,
/// and the detectors other than bit-exact judge resampled music as music).
pub fn shaped(notes: &[RenderNote], end: u64) -> Vec<Range<u64>> {
    let mut out = Vec::new();
    let joins: Vec<u64> = notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Joined { at, .. } => Some(at),
            _ => None,
        })
        .collect();
    let mut pause: Option<u64> = None;
    for n in notes {
        match *n {
            RenderNote::Faded { from, to, .. } => out.push(from..to),
            RenderNote::Gain { from, to, .. } => out.push(from..to),
            RenderNote::Silent { at, .. } => pause = pause.or(Some(at)),
            RenderNote::Started {
                at, content, fade, ..
            } => {
                if let Some(p) = pause.take() {
                    out.push(p..at.max(p));
                }
                if content == 0 && fade == 0 && !joins.contains(&at) {
                    out.push(at..at + ms(50));
                }
            }
            RenderNote::Resumed { at, .. } => {
                if let Some(p) = pause.take() {
                    out.push(p..at.max(p));
                }
            }
            RenderNote::Ended { at, .. } if !joins.contains(&at) => {
                // A song's own fall, and the silence after it until the next sound (a song
                // boundary).
                out.push(at.saturating_sub(ms(50))..at + 1);
                pause = pause.or(Some(at));
            }
            _ => {}
        }
    }
    out.extend(pause.map(|p| p..end));
    out
}

/// The crossfades' spans (where two places of the music sound together).
pub fn crossfades(notes: &[RenderNote]) -> Vec<Range<u64>> {
    notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Faded {
                from,
                to,
                kind: FadeKind::CrossIn | FadeKind::CrossOut,
                ..
            } => Some(from..to),
            _ => None,
        })
        .collect()
}

fn declared(spans: &[Range<u64>]) -> Declared {
    Declared::new(
        spans
            .iter()
            .map(|r| r.start as usize..r.end as usize)
            .collect(),
    )
}

/// Every detector on the music (left) channel heard: the music-shaped check (but where two
/// places sound together in a crossfade: its limits are one piece's), bit-exact against the
/// source with every resampled span declared too, LPC calibrated on the music played solo, and
/// dip and gap. A flag is a failure.
pub fn assert_music_clean(what: &str, heard: &Heard) {
    assert_music_clean_with(what, heard, &[]);
}

/// `assert_music_clean`, with `extra` spans declared too (the song's own quiet passages).
pub fn assert_music_clean_with(what: &str, heard: &Heard, extra: &[Range<u64>]) {
    let x = &heard.left;
    assert!(x.iter().any(|&v| v != 0), "{what}: something played");
    let end = x.len() as u64;
    let mut spans = shaped(&heard.notes, end);
    spans.extend_from_slice(extra);
    let d = declared(&spans);
    let piece = audio_check::music::Piece {
        rate: R,
        channels: 1,
    };
    let (s1, s3) = piece.limits();
    let x24: Vec<i32> = x.iter().map(|v| v >> 8).collect();
    let xf = declared(&crossfades(&heard.notes));
    let music: Vec<_> = audio_check::music::clicks(&x24, s1, s3)
        .into_iter()
        .filter(|c| !xf.covers(c.at))
        .collect();
    assert!(
        music.is_empty(),
        "{what}: music-shaped: {:?}",
        &music[..music.len().min(5)]
    );
    let rf: Vec<f64> = music_reference(0.0, 30.0)
        .iter()
        .map(|&v| f64::from(v) / FULL)
        .collect();
    let xf64: Vec<f64> = x.iter().map(|&v| f64::from(v) / FULL).collect();
    let lpc = audio_check::lpc::Lpc::calibrate(&rf, R).check(&xf64, &d);
    assert!(
        lpc.is_empty(),
        "{what}: LPC: {:?}",
        &lpc[..lpc.len().min(5)]
    );
    let dip = audio_check::dip::Dip::new(R).check(&xf64, &d);
    assert!(
        dip.is_empty(),
        "{what}: dip and gap: {:?}",
        &dip[..dip.len().min(5)]
    );
}

/// Bit-exact against `source` (left-justified) with every span the notes declare, resampling
/// included: what the renderer did not say it changed is the source, frame for frame.
pub fn assert_exact_where_undeclared(what: &str, heard: &Heard, source: &[i32]) {
    let end = heard.left.len() as u64;
    let mut spans = shaped(&heard.notes, end);
    let mut open: Vec<(player::voice_api::VoiceId, u64)> = Vec::new();
    for n in &heard.notes {
        if let RenderNote::Resampling { voice, at, on } = *n {
            if on {
                open.push((voice, at));
            } else if let Some(k) = open.iter().position(|(v, _)| *v == voice) {
                let (_, from) = open.remove(k);
                spans.push(from..at);
            }
        }
    }
    spans.extend(open.into_iter().map(|(_, from)| from..end));
    let flags = audio_check::exact::check(
        &heard.left,
        &audio_check::exact::Samples(source),
        &declared(&spans),
    );
    assert!(
        flags.is_empty(),
        "{what}: bit-exact: {:?}",
        &flags[..flags.len().min(5)]
    );
}

/// The sine lock on both channels of a run of a sine song.
pub fn assert_sine_clean(what: &str, heard: &Heard, hz: f64) {
    let end = heard.left.len() as u64;
    let d = declared(&shaped(&heard.notes, end));
    let lock = audio_check::sine::SineLock::new(hz, R, 0.5);
    for (c, x) in [&heard.left, &heard.right].into_iter().enumerate() {
        let xf: Vec<f64> = x.iter().map(|&v| f64::from(v) / FULL).collect();
        let f = audio_check::sine::check(&xf, &lock, &d);
        assert!(
            f.is_empty(),
            "{what}, channel {c}: sine lock: {:?}",
            &f[..f.len().min(5)]
        );
    }
}

pub fn has(notes: &[RenderNote], f: impl Fn(&RenderNote) -> bool) -> bool {
    notes.iter().any(f)
}

pub fn count(notes: &[RenderNote], f: impl Fn(&RenderNote) -> bool) -> usize {
    notes.iter().filter(|n| f(n)).count()
}

/// Simulated now on `clock`, as the host's time when the host is this computer.
pub fn host_now(clock: &SimClock) -> i64 {
    (clock.now_ns() / 1_000) as i64
}

/// The right channel's stretches played straight: (first value, frames) for each stretch of at
/// least 200 frames where each value is the one before plus one (fades and silence between).
pub fn runs(h: &Heard) -> Vec<(i32, usize)> {
    let mut out = Vec::new();
    let x: Vec<i32> = h.right.iter().map(|v| v >> 8).collect();
    let mut i = 0;
    while i < x.len() {
        let mut j = i + 1;
        while j < x.len() && x[i] != 0 && (x[j] - x[j - 1] - 1).abs() <= 1 {
            j += 1;
        }
        if x[i] != 0 && j - i >= 200 {
            out.push((x[i], j - i));
        }
        i = j;
    }
    out
}

pub fn identity() -> ClockMapHandle {
    HostTime::same().exact_map()
}

pub fn timebase(clock: &SimClock) -> &dyn Timebase {
    clock
}
