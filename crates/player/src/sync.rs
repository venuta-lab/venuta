//! Playing in step with others (listening together): the room says where the song should be
//! at each moment, and the engine keeps what comes out of the speaker there.
//!
//! Two ways to stay together:
//! - **Apart** (friends in different places, the default): nobody can hear the others' speakers,
//!   so smooth sound comes first. Each song lines up from silence as it starts; a drift past
//!   10 ms is taken up by playing a hair faster or slower (never heard), and only one past 250
//!   ms that lasts is lined up again, faded. Bit-perfect whenever within 3 ms.
//! - **Same room** (speakers that hear each other, on one network): within a few milliseconds.
//!   Drift past 2 ms is taken out the same way, down to half a millisecond.
//!
//! The rate change goes through a high-quality resampler (`Resampler`) whose rate glides, at
//! most 500 per million (under a cent of pitch), asked for by one controller (`Controller`)
//! on an error smoothed over a second so clock noise isn't chased.
//!
//! The resampler is one loop with two faces: `Resampler`, today's engine's (it keeps its own
//! history), and `Converter`, the renderer's (`render.rs`), which works on a window it is lent,
//! allocates nothing, and takes any base ratio (the source's rate over the device's), with a
//! kernel cut at the device's rate for sources faster than it (`Kernels`).

use std::time::{Duration, Instant};

/// What the room asks of this player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncPlan {
    /// When the song is at `position_ms` (this computer's clock); `None` while it waits for the
    /// room to start: the song is made ready there, silent.
    pub anchor: Option<Instant>,
    pub position_ms: u64,
    pub playing: bool,
    /// Speakers in one room: correct by single frames.
    pub same_room: bool,
    /// How much later than the device says the sound is heard (Bluetooth), milliseconds.
    pub extra_latency_ms: i32,
}

impl SyncPlan {
    /// Where the song should be heard at `now` (milliseconds), once started.
    pub fn expected_ms(&self, now: Instant) -> Option<f64> {
        let anchor = self.anchor?;
        if !self.playing {
            return Some(self.position_ms as f64);
        }
        let elapsed = if now >= anchor {
            now.duration_since(anchor).as_secs_f64() * 1_000.0
        } else {
            -(anchor.duration_since(now).as_secs_f64() * 1_000.0)
        };
        Some(self.position_ms as f64 + elapsed)
    }
}

/// Something the player did to the music to stay with the room, said in its status (for the
/// playback log, and for checks to count).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncNote {
    /// The room's song started on its very frame, from silence, at this place in it.
    Started { position_ms: u64 },
    /// Moved from one place in the song to another (milliseconds): faded out and back in when
    /// music was coming out, at once from silence.
    Seek {
        from_ms: u64,
        to_ms: u64,
        faded: bool,
    },
    /// Just after a seek, its last few frames closed exactly: skipped (positive), or silence
    /// first (negative).
    Fine { frames: i64 },
    /// One room: the music plays this many parts per million faster (positive, catching up) or
    /// slower (negative); 0 is the song's own rate again.
    Rate { ppm: i32 },
}

/// What to do about an error (heard minus expected, milliseconds; positive is ahead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Correction {
    /// Close enough.
    None,
    /// Move to where the room is (a hard resync, faded).
    Seek,
    /// Take out (behind) or add (ahead) single frames at this many per million.
    Trim { ppm: f64, ahead: bool },
}

/// A new plan this close to the old one is the same plan, its start known better (same room).
pub const NUDGE_MS: f64 = 20.0;
/// Apart, a start known better by up to this much is still the same plan: the music plays on.
pub const APART_NUDGE_MS: f64 = 300.0;

/// The same plan with its start known a little better (the host's clock estimated better), not
/// a change of plan: the player keeps on, its usual corrections taking up the difference.
pub fn is_nudge(old: &SyncPlan, new: &SyncPlan, now: Instant) -> bool {
    old.playing
        && new.playing
        && old.same_room == new.same_room
        && old.extra_latency_ms == new.extra_latency_ms
        && match (
            old.anchor,
            new.anchor,
            old.expected_ms(now),
            new.expected_ms(now),
        ) {
            (Some(_), Some(_), Some(a), Some(b)) => {
                (a - b).abs()
                    <= if new.same_room {
                        NUDGE_MS
                    } else {
                        APART_NUDGE_MS
                    }
            }
            _ => false,
        }
}

/// Apart: further than this for a while (`far_for`), lined up again (a seek, faded).
pub const APART_SEEK_MS: f64 = 250.0;
/// Apart: under this (after smoothing), nothing to do (the clock over the internet can't be
/// known much better); a correction under way goes on until `APART_DONE_MS`.
pub const APART_DEAD_MS: f64 = 10.0;
pub const APART_DONE_MS: f64 = 3.0;
/// Apart: how much faster or slower per millisecond off (gentle: a slow close is fine when no
/// one hears the other's speaker).
pub const APART_PPM_PER_MS: f64 = 25.0;
/// Same room: under this (after smoothing), nothing to do; clock and position noise is a
/// millisecond or two.
pub const SAME_ROOM_DEAD_MS: f64 = 2.0;
/// Same room: a correction under way goes on until this close.
pub const SAME_ROOM_DONE_MS: f64 = 0.5;
/// Same room: how much faster or slower per millisecond off (a small gap closes in a few
/// seconds, without overshooting the second the error is smoothed over).
pub const PPM_PER_MS: f64 = 250.0;
/// The most the slow part of the controller (which learns a sound card's own drift) adds.
pub const MAX_LEARNED_PPM: f64 = 200.0;
/// How fast the rate glides, in parts per million per second of music.
pub const GLIDE_PPM_PER_S: f64 = 1_000.0;
/// Same room: further than this, a seek.
pub const SAME_ROOM_SEEK_MS: f64 = 50.0;
/// The most frames per million taken out or added.
pub const MAX_PPM: f64 = 500.0;
/// How long the error is smoothed over before it's acted on.
pub const SMOOTHING: Duration = Duration::from_secs(1);
/// How long music takes to fade in from silence: a start partway into a song, a resume, the
/// far side of a seek.
pub const FADE: Duration = Duration::from_millis(15);
/// How long music takes to fade out to silence: before a pause, a stop, a seek, another song.
pub const FADE_OUT: Duration = Duration::from_millis(30);

/// How long an error past the seek line has to last before it is lined up: a stall or a burst
/// of late clock readings passes; a real drift doesn't.
pub fn far_for(same_room: bool) -> Duration {
    Duration::from_secs(if same_room { 2 } else { 3 })
}

/// The correction for an error (smoothed). `settling` is true just after a start or a seek,
/// while the device fills (no correction then).
pub fn correction(error_ms: f64, same_room: bool, settling: bool) -> Correction {
    if settling {
        return Correction::None;
    }
    let e = error_ms.abs();
    let (seek, dead) = if same_room {
        (SAME_ROOM_SEEK_MS, SAME_ROOM_DEAD_MS)
    } else {
        (APART_SEEK_MS, APART_DEAD_MS)
    };
    if e > seek {
        Correction::Seek
    } else if e > dead {
        Correction::Trim {
            ppm: Controller::default().rate(error_ms, 0.0, same_room).abs(),
            ahead: error_ms > 0.0,
        }
    } else {
        Correction::None
    }
}

/// Asks for the rate that keeps the music with the room: faster (positive) when behind,
/// slower when ahead, from the smoothed error.
///
/// - A correction starts past the dead zone and goes on until well within it (so it doesn't
///   flicker on noise): `per_ms` ppm for each millisecond off, at most 500.
/// - All along it measures, over 5 s at a time, how fast the error moves against the rate it
///   asked for: what is left over is the sound card's own drift against the room's clock. It
///   takes half of each measure into what it has learned, and plays that much the other way all
///   along (a card 80 ppm fast is played 80 ppm slower), so the error closes all the way and then
///   stays where it is, instead of hanging where the correction balances the drift, or drifting
///   back to the line and being corrected over and over.
/// - A card within 5 ppm of the room needs nothing: the song's own rate, bit-perfect.
#[derive(Debug, Clone, Default)]
pub struct Controller {
    correcting: bool,
    /// The card's drift against the room, learned (parts per million to play faster).
    learned: f64,
    /// The measure under way: for how long (s), the error when it began, and the rate asked
    /// for over it (ppm × s).
    window: Option<(f64, f64, f64)>,
}

/// How long each measure of the card's drift is (s).
const WINDOW_S: f64 = 5.0;

impl Controller {
    /// The rate (parts per million faster) for `error_ms` (positive: ahead), `dt_s` seconds
    /// after the last ask.
    pub fn rate(&mut self, error_ms: f64, dt_s: f64, same_room: bool) -> f64 {
        let (dead, done, per_ms) = if same_room {
            (SAME_ROOM_DEAD_MS, SAME_ROOM_DONE_MS, PPM_PER_MS)
        } else {
            (APART_DEAD_MS, APART_DONE_MS, APART_PPM_PER_MS)
        };
        let e = error_ms.abs();
        if self.correcting && e <= done {
            self.correcting = false;
        } else if !self.correcting && e > dead {
            self.correcting = true;
        }
        let steady = if self.learned.abs() < 5.0 {
            0.0
        } else {
            self.learned
        };
        let rate = if self.correcting {
            (-error_ms * per_ms + steady).clamp(-MAX_PPM, MAX_PPM)
        } else {
            steady
        };
        // What the error does, against the rate asked for, is the card's drift not yet learned.
        let (t, e0, asked) = self.window.get_or_insert((0.0, error_ms, 0.0));
        *t += dt_s;
        *asked += rate * dt_s;
        if *t >= WINDOW_S {
            let moved_ppm = (error_ms - *e0) / *t * 1_000.0;
            let drift = moved_ppm - *asked / *t;
            // A card is never that far off: a measure like that saw something else (a stall,
            // a jump), not drift.
            if drift.abs() <= MAX_LEARNED_PPM {
                self.learned = (self.learned + (-drift - self.learned) * 0.5)
                    .clamp(-MAX_LEARNED_PPM, MAX_LEARNED_PPM);
            }
            self.window = Some((0.0, error_ms, 0.0));
        }
        rate
    }

    /// Start over (a seek, a new plan, a stall): what was learned of the card stays (it is the
    /// card's, not the plan's).
    pub fn reset(&mut self) {
        self.correcting = false;
        self.window = None;
    }

    /// The room's place was known better (a new estimate of the host's clock): the error moved
    /// by that, not by the card, so the measure under way starts again.
    pub fn plan_moved(&mut self) {
        self.window = None;
    }

    /// Forget the card too (another output).
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    pub fn correcting(&self) -> bool {
        self.correcting
    }

    /// The card's drift learned, parts per million to play faster.
    pub fn learned(&self) -> f64 {
        self.learned
    }
}

/// Half the 64-tap kernel's taps: 32 either side (flat to 20 kHz at 44.1 kHz, images 80 dB down).
const HALF: usize = 32;
/// Fractions of a frame the 64-tap kernel is worked out for (in between, interpolated).
const PHASES: usize = 512;
/// Kaiser's window: about 87 dB down in the stop band.
const BETA: f64 = 8.6;
/// A kernel for a source faster than the device cuts here, as a share of the device's rate. Its
/// transition band (0.087 of the device rate wide: 64 taps' worth at the device's rate) then
/// ends at the device's Nyquist, so nothing above it folds back, and it is flat to 0.413 of the
/// device's rate (19.8 kHz at 48 kHz).
const CUTOFF: f64 = 0.4565;
/// The most taps any kernel has: the one for 384 kHz on a 44.1 kHz device, 279 a side.
pub const MAX_TAPS: usize = 560;
/// The source rates a renderer has a kernel for. A source between two takes the kernel of the
/// next one above it (cut a little lower, never folding back).
pub const STANDARD_RATES: [u32; 16] = [
    8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 64_000, 88_200, 96_000,
    176_400, 192_000, 352_800, 384_000,
];

/// The resampler's filter: a windowed sinc (Kaiser), for each of `phases + 1` fractions of a
/// frame, `2 * half` taps (tap t weighs the input frame `t + 1 - half` from the one at or before
/// the point), each row summing to one.
///
/// The unity kernel (64 taps, 512 phases, cut at the source's own Nyquist) serves every ratio up
/// to 1: the song at the device's rate (with a room's trim) and every source slower than the
/// device. A source faster than the device needs one cut lower, at the device's rate: `scaled`
/// stretches the same window over `ratio` times the frames, with `1 / ratio` the phases, so it
/// is as exact and holds about as many coefficients.
#[derive(Debug, Clone)]
pub struct Kernel {
    half: usize,
    phases: usize,
    /// Cut at the source's own Nyquist: its fraction 0 is the input frame itself, so a whole
    /// frame passes untouched.
    unity: bool,
    table: Vec<f64>,
}

impl Kernel {
    fn build(half: usize, phases: usize, scale: f64, unity: bool) -> Kernel {
        // The zeroth modified Bessel function, for Kaiser's window.
        fn i0(x: f64) -> f64 {
            let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
            while term > sum * 1e-17 {
                term *= (x / (2.0 * k)).powi(2);
                sum += term;
                k += 1.0;
            }
            sum
        }
        let taps = 2 * half;
        let mut t = Vec::with_capacity((phases + 1) * taps);
        for p in 0..=phases {
            let frac = p as f64 / phases as f64;
            let row: Vec<f64> = (0..taps)
                .map(|i| {
                    let x = i as f64 + 1.0 - half as f64 - frac;
                    let w = x / half as f64;
                    if w.abs() >= 1.0 {
                        return 0.0;
                    }
                    let xs = x * scale;
                    let sinc = if xs.abs() < 1e-12 {
                        1.0
                    } else {
                        (std::f64::consts::PI * xs).sin() / (std::f64::consts::PI * xs)
                    };
                    sinc * i0(BETA * (1.0 - w * w).sqrt()) / i0(BETA)
                })
                .collect();
            let sum: f64 = row.iter().sum();
            t.extend(row.iter().map(|v| v / sum));
        }
        Kernel {
            half,
            phases,
            unity,
            table: t,
        }
    }

    /// The tested 64-tap kernel, for every ratio up to 1 (worked out once, at the player's
    /// start: call it there).
    pub fn unity() -> &'static Kernel {
        static K: std::sync::OnceLock<Kernel> = std::sync::OnceLock::new();
        K.get_or_init(|| Kernel::build(HALF, PHASES, 1.0, true))
    }

    /// The kernel for a source `ratio` times the device's rate (above 1): cut at 0.4565 of the
    /// device's rate, `ceil(32 x ratio)` taps a side, `ceil(512 / ratio)` phases.
    pub fn scaled(ratio: f64) -> Kernel {
        let r = ratio.max(1.0);
        let half = (HALF as f64 * r).ceil() as usize;
        let phases = ((PHASES as f64 / r).ceil() as usize).max(1);
        Kernel::build(half, phases, 2.0 * CUTOFF / r, false)
    }

    /// Taps either side.
    pub fn half(&self) -> usize {
        self.half
    }

    pub fn taps(&self) -> usize {
        2 * self.half
    }
}

/// A renderer's kernels: the unity one, and one for each standard rate above the device's, all
/// worked out when the renderer is made (never on the render thread).
#[derive(Debug, Clone)]
pub struct Kernels {
    device: u32,
    scaled: Vec<(u32, Kernel)>,
}

impl Kernels {
    pub fn for_device(rate: u32) -> Kernels {
        let _ = Kernel::unity();
        let d = rate.max(1);
        Kernels {
            device: d,
            scaled: STANDARD_RATES
                .iter()
                .filter(|&&s| s > d)
                .map(|&s| (s, Kernel::scaled(f64::from(s) / f64::from(d))))
                .collect(),
        }
    }

    /// The kernel for a source at `source` Hz (`None`: too far above the device's rate, past the
    /// highest standard rate).
    pub fn pick(&self, source: u32) -> Option<&Kernel> {
        self.index(source).map(|i| self.get(i))
    }

    /// `pick`'s kernel as a number to keep beside a voice (0: the unity kernel).
    pub fn index(&self, source: u32) -> Option<usize> {
        if source <= self.device {
            return Some(0);
        }
        self.scaled
            .iter()
            .position(|(s, _)| *s >= source)
            .map(|i| i + 1)
    }

    /// The kernel `index` gave.
    pub fn get(&self, index: usize) -> &Kernel {
        match index {
            0 => Kernel::unity(),
            i => &self.scaled[i - 1].1,
        }
    }

    /// The most input frames per device frame any kernel here serves (1 when none is above the
    /// device's rate).
    pub fn max_ratio(&self) -> f64 {
        self.scaled
            .last()
            .map_or(1.0, |(s, _)| f64::from(*s) / f64::from(self.device))
    }

    /// The widest kernel's taps a side.
    pub fn max_half(&self) -> usize {
        self.scaled
            .iter()
            .map(|(_, k)| k.half)
            .fold(HALF, usize::max)
    }
}

/// Where the resampler is: how far past the first input frame not yet played past the next
/// frame out is (0..1), the rate now (ppm faster), and, coming back to the song's own rate, the
/// whole frame it stops on.
#[derive(Debug, Clone, Copy, Default)]
struct Glide {
    phase: f64,
    ppm: f64,
    home: Option<f64>,
}

/// The resampler's one loop, for `Resampler` and `Converter` alike: `want` frames into `out`
/// (interleaved, `ch` channels) from `win`, whose frame `at` is the first not yet played past
/// (the frames before it are the filter's left side, and the ones after it must reach as far as
/// the filter reads). Each frame out is the music at a point between input frames, worked out
/// by the kernel; the point moves `base x (1 + ppm / 1e6)` input frames per frame out, the rate
/// gliding toward `target_ppm` by at most `glide` ppm a frame. A source at the device's rate
/// (`exact`) whose target is 0 comes to a standstill on a whole frame, and from there gives the
/// input exactly. Returns how many input frames were played past. `places` gets each frame's
/// point (for checks).
#[allow(clippy::too_many_arguments)]
fn run(
    g: &mut Glide,
    k: &Kernel,
    base: f64,
    exact: bool,
    glide: f64,
    win: &[i32],
    ch: usize,
    at: usize,
    want: usize,
    target_ppm: f64,
    out: &mut [i32],
    mut places: Option<&mut Vec<f64>>,
) -> usize {
    let taps = k.taps();
    let half = k.half;
    let phases = k.phases;
    let table = &k.table[..];
    let mut h = [0.0f64; MAX_TAPS];
    let mut pos = g.phase;
    let mut ppm = g.ppm;
    let mut home = g.home;
    for j in 0..want {
        let whole = pos.floor();
        let frac = pos - whole;
        let i0 = whole as usize;
        if let Some(a) = places.as_deref_mut() {
            a.push(pos);
        }
        let o = &mut out[j * ch..(j + 1) * ch];
        if frac == 0.0 && k.unity {
            let s = (at + i0) * ch;
            o.copy_from_slice(&win[s..s + ch]);
        } else {
            let f = frac * phases as f64;
            let p = (f.floor() as usize).min(phases - 1);
            let t = f - p as f64;
            let (r0, r1) = (&table[p * taps..][..taps], &table[(p + 1) * taps..][..taps]);
            for (hk, (a, b)) in h[..taps].iter_mut().zip(r0.iter().zip(r1)) {
                *hk = a + (b - a) * t;
            }
            let first = (at + i0 + 1 - half) * ch;
            let span = &win[first..first + taps * ch];
            for (c, oc) in o.iter_mut().enumerate() {
                let mut acc = 0.0;
                for (kk, hk) in h[..taps].iter().enumerate() {
                    acc += hk * f64::from(span[kk * ch + c]);
                }
                *oc = acc.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
            }
        }
        // The rate glides toward the target. At the song's own rate with a fraction of a frame
        // left (or still moving), it comes to a standstill on a whole frame: the first it can
        // stop at, chosen once, slowing so it arrives with no speed left. (`d` is how far the
        // music is from where it would be at its own rate: a whole number of frames means on a
        // frame.)
        let homing = exact && target_ppm == 0.0 && (frac != 0.0 || ppm != 0.0);
        let a = glide * 1e-6;
        let d = pos - j as f64;
        let goal = if homing {
            let to = *home.get_or_insert_with(|| {
                let v = ppm * 1e-6;
                let stop = v * v.abs() / (2.0 * a);
                if v > 0.0 {
                    (d + stop).ceil()
                } else if v < 0.0 {
                    (d + stop).floor()
                } else {
                    d.round()
                }
            });
            let left = to - d;
            left.signum() * (2.0 * a * left.abs()).sqrt().min(100e-6) * 1e6
        } else {
            home = None;
            target_ppm
        };
        ppm += (goal - ppm).clamp(-glide, glide);
        let step = ppm * 1e-6;
        match home {
            Some(to)
                if homing && {
                    let left = to - d;
                    left.abs() < 1e-12
                        || (left > 0.0 && step >= left)
                        || (left < 0.0 && step <= left)
                } =>
            {
                // There: on a whole frame, at the song's own rate.
                pos = (j + 1) as f64 + to;
                ppm = 0.0;
                home = None;
            }
            _ => pos += base * (1.0 + step),
        }
    }
    g.ppm = ppm;
    let used = pos.floor().max(0.0) as usize;
    g.phase = pos - used as f64;
    // The frame to land on, from where the next call counts.
    g.home = home.map(|to| to - (used as f64 - want as f64));
    used
}

/// Plays music a hair faster or slower without a step in it: each frame out is the music at a
/// point between two input frames, worked out by a long windowed-sinc filter (not a frame
/// repeated or dropped, heard as grit, nor a straight line drawn between two samples, which dulls
/// and roughens the treble), the rate gliding to where it is asked. At the song's own rate,
/// landed on a whole frame, it gives exactly the input: bit-perfect.
///
/// Today's engine's resampler (it keeps its own history, so it allocates); the renderer's is
/// `Converter`, the same loop on a window it is lent.
#[derive(Debug, Clone, Default)]
pub struct Resampler {
    /// The last `HALF` input frames already played past, interleaved (the filter's left side).
    history: Vec<i32>,
    /// How far past the first input frame not yet played past the next frame out is (0..1).
    phase: f64,
    /// The rate now, parts per million faster.
    ppm: f64,
    /// Coming back to the song's own rate: the whole frame it stops on (see `process`).
    home: Option<f64>,
}

impl Resampler {
    /// At the song's own rate, on a whole frame: the music plays as it is.
    pub fn idle(&self) -> bool {
        self.ppm == 0.0 && self.phase == 0.0
    }

    /// The rate now, parts per million faster.
    pub fn ppm(&self) -> f64 {
        self.ppm
    }

    /// Start over (a seek, another song: nothing before it to carry on from).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The song couldn't be decoded far enough for the filter this time (a stream behind): the
    /// music comes out as it is, back on a whole frame, the frames before it kept for the
    /// filter's left side (not zeros, which would be a transient when it carries on) and the
    /// rate kept.
    pub fn as_is_for_now(&mut self, input: &[i32], ch: usize) {
        self.phase = 0.0;
        self.home = None;
        self.played_as_is(input, ch);
    }

    /// Work the filter out now (at the player's start), not on the sound's thread mid-song.
    pub fn warm() {
        let _ = Kernel::unity();
    }

    /// The music at `input` (interleaved, `ch` channels) came out as it is: its last frames are
    /// kept, so a correction starting next has the filter's left side.
    pub fn played_as_is(&mut self, input: &[i32], ch: usize) {
        let keep = HALF * ch.max(1);
        if self.history.len() != keep {
            self.history = vec![0; keep];
        }
        self.history.extend_from_slice(input);
        let cut = self.history.len() - keep;
        self.history.drain(..cut);
    }

    /// `want` frames out of `input` (interleaved, `ch` channels at `rate`: the next frames of the
    /// song, `ended` when there are none after them), the rate gliding toward `target_ppm`; once
    /// that is 0, the last fraction of a frame is closed so the music is exact again. Returns how
    /// many input frames were played past, or `None` when `input` is too short for the filter
    /// (the song not decoded far enough: nothing done). `at` gets each frame's place in the
    /// input (for checks).
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        input: &[i32],
        ch: usize,
        rate: u32,
        ended: bool,
        want: usize,
        target_ppm: f64,
        out: &mut Vec<i32>,
        at: Option<&mut Vec<f64>>,
    ) -> Option<usize> {
        let ch = ch.max(1);
        let frames = input.len() / ch;
        // Enough ahead for the last frame's filter (past a song's end, silence).
        let reach = (want as f64 * (1.0 + MAX_PPM / 1e6) + self.phase).ceil() as usize + HALF;
        if frames < reach && !ended {
            return None;
        }
        if self.history.len() != HALF * ch {
            self.history = vec![0; HALF * ch];
        }
        // The filter's window: its left side, the input, and silence past a song's end.
        let len = HALF + frames.max(reach) + HALF + 1;
        let mut win = Vec::with_capacity(len * ch);
        win.extend_from_slice(&self.history);
        win.extend_from_slice(&input[..frames * ch]);
        win.resize(len * ch, 0);
        let start = out.len();
        out.resize(start + want * ch, 0);
        let mut g = Glide {
            phase: self.phase,
            ppm: self.ppm,
            home: self.home,
        };
        let glide = GLIDE_PPM_PER_S / f64::from(rate.max(1));
        let used = run(
            &mut g,
            Kernel::unity(),
            1.0,
            true,
            glide,
            &win,
            ch,
            HALF,
            want,
            target_ppm,
            &mut out[start..],
            at,
        );
        self.phase = g.phase;
        self.ppm = g.ppm;
        self.home = g.home;
        // What was played past becomes the filter's left side.
        self.history
            .copy_from_slice(&win[used * ch..(used + HALF) * ch]);
        Some(used.min(frames))
    }
}

/// The renderer's resampler for one voice: the same filter, glide and landing as `Resampler`, at
/// any base ratio (the source's rate over the device's), on a window of the voice's frames that
/// it is lent. The frames in the window before the read point are the filter's left side, so it
/// keeps no history of its own and allocates nothing.
#[derive(Debug, Clone)]
pub struct Converter {
    base: f64,
    /// The source runs at the device's rate: it can land on whole frames, bit-perfect.
    exact: bool,
    /// The most the rate moves per frame out, ppm.
    glide: f64,
    g: Glide,
}

impl Converter {
    /// A voice's resampler: a source at `source_rate` played at `device_rate`, starting at a
    /// trim of `ppm` (a new voice starts at the trim the room plays at: it starts from silence
    /// or under a crossfade, so it has no rate to glide from).
    pub fn new(source_rate: u32, device_rate: u32, ppm: f64) -> Converter {
        let d = device_rate.max(1);
        Converter {
            base: f64::from(source_rate.max(1)) / f64::from(d),
            exact: source_rate == d,
            glide: GLIDE_PPM_PER_S / f64::from(d),
            g: Glide {
                phase: 0.0,
                ppm: ppm.clamp(-MAX_PPM, MAX_PPM),
                home: None,
            },
        }
    }

    /// The trim now, ppm faster.
    pub fn ppm(&self) -> f64 {
        self.g.ppm
    }

    /// How far past the next whole input frame the next frame out is (0..1).
    pub fn phase(&self) -> f64 {
        self.g.phase
    }

    /// The source's rate over the device's.
    pub fn base(&self) -> f64 {
        self.base
    }

    /// Input frames per frame out now.
    pub fn ratio(&self) -> f64 {
        self.base * (1.0 + self.g.ppm * 1e-6)
    }

    /// A source at the device's rate, untrimmed, on a whole frame: its frames pass as they are.
    pub fn idle(&self) -> bool {
        self.exact && self.g.ppm == 0.0 && self.g.phase == 0.0
    }

    /// The most frames of the window, from the read point on, that the next `want` frames out
    /// may read (the filter's right side included).
    pub fn reach(&self, k: &Kernel, want: usize) -> usize {
        (self.g.phase + want as f64 * self.base * (1.0 + MAX_PPM / 1e6)).ceil() as usize
            + k.half
            + 1
    }

    /// `want` frames out (interleaved, `ch` channels) into `out`, reading `win` from frame `at`
    /// (at least `k.half()`, the left side before it) to `at + reach(k, want)`, the trim gliding
    /// toward `target_ppm` (at most 500 ppm either way). Returns the frames played past: the next
    /// read point is `at` plus that.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        k: &Kernel,
        win: &[i32],
        ch: usize,
        at: usize,
        want: usize,
        target_ppm: f64,
        out: &mut [i32],
    ) -> usize {
        run(
            &mut self.g,
            k,
            self.base,
            self.exact,
            self.glide,
            win,
            ch,
            at,
            want,
            target_ppm.clamp(-MAX_PPM, MAX_PPM),
            out,
            None,
        )
    }

    /// As `process`, with each frame's point in the window (relative to `at`) for checks.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn process_with_places(
        &mut self,
        k: &Kernel,
        win: &[i32],
        ch: usize,
        at: usize,
        want: usize,
        target_ppm: f64,
        out: &mut [i32],
        places: &mut Vec<f64>,
    ) -> usize {
        run(
            &mut self.g,
            k,
            self.base,
            self.exact,
            self.glide,
            win,
            ch,
            at,
            want,
            target_ppm.clamp(-MAX_PPM, MAX_PPM),
            out,
            Some(places),
        )
    }
}

/// Takes out or adds single frames in interleaved audio, spread evenly: a running fraction
/// carries over between device periods so the rate is exact over time.
#[derive(Debug, Default, Clone)]
pub struct Trimmer {
    owed: f64,
}

impl Trimmer {
    /// How many frames to change in a period of `frames` at `ppm`.
    pub fn due(&mut self, frames: usize, ppm: f64) -> usize {
        self.owed += frames as f64 * ppm / 1_000_000.0;
        // A hair over, so sums like 100 x 0.24 come to 24, not 23.
        let n = (self.owed + 1e-9).floor();
        self.owed -= n;
        n as usize
    }

    pub fn reset(&mut self) {
        self.owed = 0.0;
    }
}

/// Interleaved audio (`ch` channels) stretched or squeezed to `to` frames by
/// linear interpolation, its first and last frames kept exactly: a tiny local resample, so a
/// frame more or fewer is spread over the whole stretch instead of showing as a step.
fn resample_into(src: &[i32], ch: usize, to: usize, out: &mut Vec<i32>) {
    let n = src.len() / ch;
    if n < 2 || to < 2 {
        out.extend_from_slice(src);
        return;
    }
    let scale = (n - 1) as f64 / (to - 1) as f64;
    for j in 0..to {
        let pos = j as f64 * scale;
        let i = (pos.floor() as usize).min(n - 2);
        let t = pos - i as f64;
        for c in 0..ch {
            let x = f64::from(src[i * ch + c]);
            let y = f64::from(src[(i + 1) * ch + c]);
            out.push((x + (y - x) * t).round() as i32);
        }
    }
}

/// Change `samples` (interleaved, `ch` channels) by `n` frames, longer (`longer`) or shorter,
/// in `n` even stretches each a frame longer or shorter than it was.
fn respace(samples: &mut Vec<i32>, ch: usize, n: usize, longer: bool) {
    let ch = ch.max(1);
    let frames = samples.len() / ch;
    // Each stretch at least a few dozen frames, so the change is a fraction of a sample a frame.
    if n == 0 || frames < 32 * n {
        return;
    }
    let len = frames / n;
    let mut out = Vec::with_capacity(samples.len() + n * ch);
    for k in 0..n {
        let from = k * len;
        let to = if k + 1 == n { frames } else { from + len };
        let piece = &samples[from * ch..to * ch];
        let count = to - from;
        resample_into(
            piece,
            ch,
            if longer { count + 1 } else { count - 1 },
            &mut out,
        );
    }
    *samples = out;
}

/// Add `n` frames to `samples` (interleaved, `ch` channels), spread through it: the audio gets
/// `n` frames longer (we were ahead). Stretched, not repeated: a repeated sample is heard as grit
/// on clean music.
pub fn repeat_frames(samples: &mut Vec<i32>, ch: usize, n: usize) {
    respace(samples, ch, n, true);
}

/// Take `n` frames out of `samples` (interleaved, `ch` channels), spread through it: the audio
/// gets `n` frames shorter (we were behind). Squeezed, not cut.
pub fn drop_frames(samples: &mut Vec<i32>, ch: usize, n: usize) {
    respace(samples, ch, n, false);
}

/// How loud each frame of a fade is: up from silence (`rising`) or down to it, `done` frames of
/// `len` already past.
pub fn fade_gain(done: usize, len: usize, rising: bool) -> f32 {
    let t = (done as f32 / len.max(1) as f32).clamp(0.0, 1.0);
    // An equal-power curve: no dip or bump in loudness on the way.
    let g = (t * std::f32::consts::FRAC_PI_2).sin();
    if rising { g } else { 1.0 - g }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_known_a_little_better_is_a_nudge_not_a_new_plan() {
        let t0 = Instant::now();
        let p = SyncPlan {
            anchor: Some(t0),
            position_ms: 10_000,
            playing: true,
            same_room: true,
            extra_latency_ms: 0,
        };
        let later = |ms| SyncPlan {
            anchor: Some(t0 + Duration::from_millis(ms)),
            ..p
        };
        assert!(is_nudge(&p, &later(3), t0), "a clock estimate moving 3 ms");
        assert!(!is_nudge(&p, &later(40), t0), "in one room, 40 ms is heard");
        assert!(!is_nudge(&p, &later(500), t0), "a seek in the room");
        let apart = SyncPlan {
            same_room: false,
            ..p
        };
        let apart_later = |ms| SyncPlan {
            anchor: Some(t0 + Duration::from_millis(ms)),
            ..apart
        };
        assert!(
            is_nudge(&apart, &apart_later(150), t0),
            "apart, 150 ms isn't heard"
        );
        assert!(!is_nudge(&apart, &apart_later(500), t0));
        assert!(
            !is_nudge(
                &p,
                &SyncPlan {
                    playing: false,
                    ..p
                },
                t0
            ),
            "a pause"
        );
        assert!(
            !is_nudge(
                &p,
                &SyncPlan {
                    same_room: false,
                    ..p
                },
                t0
            ),
            "the way changed"
        );
        assert!(
            !is_nudge(&p, &SyncPlan { anchor: None, ..p }, t0),
            "a new song getting ready"
        );
    }

    #[test]
    fn the_expected_place_moves_with_the_clock() {
        let t0 = Instant::now();
        let p = SyncPlan {
            anchor: Some(t0),
            position_ms: 10_000,
            playing: true,
            same_room: false,
            extra_latency_ms: 0,
        };
        let at = p.expected_ms(t0 + Duration::from_millis(1_500)).unwrap();
        assert!((at - 11_500.0).abs() < 0.01);
        assert!(p.expected_ms(t0).unwrap() >= 10_000.0 - 0.01);
        let waiting = SyncPlan { anchor: None, ..p };
        assert_eq!(waiting.expected_ms(t0), None, "not started");
        let paused = SyncPlan {
            playing: false,
            ..p
        };
        assert_eq!(
            paused.expected_ms(t0 + Duration::from_secs(9)),
            Some(10_000.0)
        );
    }

    #[test]
    fn apart_listeners_stay_bit_perfect_until_far_off() {
        assert_eq!(correction(5.0, false, false), Correction::None);
        assert_eq!(
            correction(-40.0, false, false),
            Correction::Trim {
                ppm: 500.0,
                ahead: false
            },
            "apart, 40 ms off: as fast as it goes, gently"
        );
        assert_eq!(
            correction(12.0, false, false),
            Correction::Trim {
                ppm: 300.0,
                ahead: true
            }
        );
        assert_eq!(correction(251.0, false, false), Correction::Seek);
        assert_eq!(
            correction(900.0, false, true),
            Correction::None,
            "not while settling"
        );
        assert!(far_for(false) > far_for(true), "apart waits longer");
    }

    #[test]
    fn same_room_trims_gently_and_seeks_when_far() {
        assert_eq!(correction(0.3, true, false), Correction::None);
        assert_eq!(correction(1.9, true, false), Correction::None, "noise");
        assert_eq!(
            correction(3.0, true, false),
            Correction::Trim {
                ppm: 500.0,
                ahead: true
            }
        );
        // Once under way, on down to half a millisecond, slower as it closes; and not started
        // for noise within the dead zone.
        let mut c = Controller::default();
        assert_eq!(c.rate(1.0, 0.0, true), 0.0, "not started for noise");
        assert_eq!(c.rate(3.0, 0.0, true), -500.0, "ahead: slower");
        assert_eq!(c.rate(1.0, 0.0, true), -250.0, "still on, closing");
        assert_eq!(c.rate(-1.2, 0.0, true), 300.0, "behind: faster");
        assert_eq!(c.rate(0.4, 0.0, true), 0.0, "close enough: its own rate");
        assert!(!c.correcting());
    }

    #[test]
    fn same_room_seeks_only_when_far() {
        assert_eq!(
            correction(-20.0, true, false),
            Correction::Trim {
                ppm: 500.0,
                ahead: false
            }
        );
        assert_eq!(correction(60.0, true, false), Correction::Seek);
    }

    /// `secs` of a card drifting `drift` ppm, the controller asked every 10 ms: the error at the
    /// end and the largest rate it asked for.
    fn drifting(c: &mut Controller, err: &mut f64, drift: f64, secs: f64, same_room: bool) -> f64 {
        let dt = 0.01;
        let mut most = 0.0f64;
        for _ in 0..(secs / dt) as usize {
            let rate = c.rate(*err, dt, same_room);
            most = most.max(rate.abs());
            // The error grows with the drift and shrinks with the rate asked for.
            *err += (drift + rate) * dt / 1000.0;
        }
        most
    }

    #[test]
    fn the_controller_learns_a_sound_cards_drift_and_stays_put() {
        for (same_room, drift) in [(false, 100.0), (false, -60.0), (true, 80.0)] {
            let mut c = Controller::default();
            let mut err = if same_room { 3.0 } else { 12.0 };
            let most = drifting(&mut c, &mut err, drift, 600.0, same_room);
            assert!(most <= MAX_PPM, "never past 500 ppm: {most}");
            assert!(
                (c.learned() + drift).abs() < drift.abs() * 0.15,
                "{drift} ppm learned: {:.0}",
                c.learned()
            );
            // Then it holds: another five minutes, never a correction, the error where it was.
            let before = err;
            drifting(&mut c, &mut err, drift, 300.0, same_room);
            let line = if same_room {
                SAME_ROOM_DEAD_MS
            } else {
                APART_DEAD_MS
            };
            assert!(
                err.abs() < line && (err - before).abs() < line / 2.0,
                "{drift} ppm, same room {same_room}: held at {err:.2} ms (was {before:.2})"
            );
        }
        // A card as the room's: nothing learned, the song's own rate once in step.
        let mut c = Controller::default();
        let mut err = 12.0;
        drifting(&mut c, &mut err, 0.0, 300.0, false);
        assert_eq!(c.rate(err, 0.01, false), 0.0, "bit-perfect");
    }

    #[test]
    fn trimming_is_exact_over_time() {
        let mut t = Trimmer::default();
        // 500 ppm over 48,000 frames a second, in 480-frame periods: 24 frames a second.
        let per_second: usize = (0..100).map(|_| t.due(480, 500.0)).sum();
        assert_eq!(per_second, 24);
    }

    #[test]
    fn frames_go_in_and_out_without_a_step_in_the_wave() {
        // A 2 kHz sine at 48 kHz: what it changes from one sample to the next is its own.
        let amp = 1_000_000.0;
        let sine: Vec<i32> = (0..480)
            .map(|i| {
                (amp * (2.0 * std::f64::consts::PI * 2000.0 * i as f64 / 48_000.0).sin()) as i32
            })
            .collect();
        let own = sine.windows(2).map(|w| (w[1] - w[0]).abs()).max().unwrap();
        let changes: [fn(&mut Vec<i32>, usize, usize); 2] = [repeat_frames, drop_frames];
        for change in changes {
            let mut s = sine.clone();
            change(&mut s, 1, 6);
            let worst = s.windows(2).map(|w| (w[1] - w[0]).abs()).max().unwrap();
            // A frame spread over 80: a step at most a couple of percent past the wave's own (a
            // repeated or dropped sample doubles it, or worse).
            assert!(
                f64::from(worst) <= f64::from(own) * 1.03,
                "steps no bigger than the wave's own: {worst} > {own}"
            );
        }
        // Too short to spread a frame over: left as it is.
        let mut short = sine[..40].to_vec();
        repeat_frames(&mut short, 1, 2);
        assert_eq!(short, sine[..40]);
    }

    #[test]
    fn frames_are_added_or_taken_out_whole() {
        // Stereo: frame i is [i, -i].
        let base: Vec<i32> = (0..200).flat_map(|i| [i * 2, -i * 2]).collect();
        let mut more = base.clone();
        repeat_frames(&mut more, 2, 2);
        assert_eq!(more.len(), base.len() + 4);
        assert!(
            more.as_chunks::<2>().0.iter().all(|f| f[0] == -f[1]),
            "channels stay together"
        );
        let mut fewer = base.clone();
        drop_frames(&mut fewer, 2, 2);
        assert_eq!(fewer.len(), base.len() - 4);
        assert!(fewer.as_chunks::<2>().0.iter().all(|f| f[0] == -f[1]));
        let mut none = base.clone();
        drop_frames(&mut none, 2, 0);
        assert_eq!(none, base);
    }

    #[test]
    fn a_fade_goes_all_the_way_smoothly() {
        assert_eq!(fade_gain(0, 100, true), 0.0);
        assert!((fade_gain(100, 100, true) - 1.0).abs() < 1e-6);
        assert!((fade_gain(0, 100, false) - 1.0).abs() < 1e-6);
        assert!(fade_gain(100, 100, false).abs() < 1e-6);
        let rising: Vec<f32> = (0..=100).map(|i| fade_gain(i, 100, true)).collect();
        assert!(rising.windows(2).all(|w| w[1] >= w[0]), "only up");
    }

    /// A sine of `hz` at 48 kHz, nearly full scale, at any (fractional) frame.
    fn sine_at(hz: f64, pos: f64) -> f64 {
        f64::from(1 << 30) * (2.0 * std::f64::consts::PI * hz * pos / 48_000.0).sin()
    }

    /// Plays `secs` of a sine through the resampler in 480-frame periods, asking `target(t)` ppm
    /// at each period's start: (the worst error against the true sine at each frame's place, in
    /// dB below its peak; every place; the frames out).
    fn through(hz: f64, secs: f64, target: impl Fn(f64) -> f64) -> (f64, Vec<f64>, Vec<i32>) {
        let n = (48_000.0 * (secs + 1.0)) as usize;
        let input: Vec<i32> = (0..n).map(|i| sine_at(hz, i as f64) as i32).collect();
        let mut r = Resampler::default();
        let (mut used, mut places, mut out) = (0usize, Vec::new(), Vec::new());
        while out.len() < (48_000.0 * secs) as usize {
            let mut at = Vec::new();
            let t = out.len() as f64 / 48_000.0;
            let k = r
                .process(
                    &input[used..],
                    1,
                    48_000,
                    false,
                    480,
                    target(t),
                    &mut out,
                    Some(&mut at),
                )
                .expect("enough decoded");
            places.extend(at.iter().map(|p| p + used as f64));
            used += k;
        }
        // Past the filter's start (silence before the song, by its left side).
        let worst = out
            .iter()
            .zip(&places)
            .skip(64)
            .map(|(&y, &p)| (f64::from(y) - sine_at(hz, p)).abs())
            .fold(0.0, f64::max);
        let db = 20.0 * (worst / f64::from(1 << 30)).log10();
        (db, places, out)
    }

    #[test]
    fn at_its_own_rate_the_resampler_gives_the_music_exactly() {
        let input: Vec<i32> = (0..4_000).map(|i| i * 7 - 9_000).collect();
        let mut r = Resampler::default();
        let mut out = Vec::new();
        let used = r.process(&input, 2, 48_000, false, 960, 0.0, &mut out, None);
        assert_eq!(used, Some(960));
        assert_eq!(out, input[..1_920]);
        assert!(r.idle());
    }

    #[test]
    fn faster_or_slower_the_music_stays_clean_to_the_top_of_hearing() {
        // Treble (15 kHz) and the middle (1 kHz), as fast and as slow as a room ever asks: every
        // frame within -90 dB of the true sine at its place (a frame repeated or dropped is a
        // step near 0 dB on a 15 kHz sine; a straight line between samples, -15 dB).
        for hz in [1_000.0, 15_000.0] {
            for ppm in [MAX_PPM, -MAX_PPM] {
                let (db, places, _) = through(hz, 3.0, |_| ppm);
                assert!(db < -90.0, "{hz} Hz at {ppm} ppm: {db:.1} dB");
                let rate = (places[places.len() - 1] - places[places.len() - 481]) / 480.0;
                assert!(
                    ((rate - 1.0) * 1e6 - ppm).abs() < 1.0,
                    "{hz} Hz: {ppm} ppm reached ({rate})"
                );
            }
        }
    }

    #[test]
    fn the_rate_glides_and_lands_back_on_whole_frames() {
        // 500 ppm faster for a second, then the song's own rate.
        let (db, places, out) = through(15_000.0, 4.0, |t| if t < 1.0 { 500.0 } else { 0.0 });
        assert!(db < -90.0, "clean throughout: {db:.1} dB");
        // No step in the rate: from one frame to the next it changes by a hair.
        let steps: Vec<f64> = places.windows(2).map(|w| w[1] - w[0]).collect();
        let jolt = steps
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max);
        let at = steps
            .windows(2)
            .position(|w| (w[1] - w[0]).abs() == jolt)
            .unwrap();
        assert!(
            jolt < 2e-6,
            "the rate glides ({jolt} at {at}: {:?})",
            &steps[at.saturating_sub(2)..at + 3]
        );
        // Then exact: the last second is the input itself, on whole frames.
        let tail = &places[places.len() - 48_000..];
        let off = places.iter().rposition(|p| p.fract() != 0.0);
        assert!(
            tail.iter().all(|p| p.fract() == 0.0),
            "on whole frames (last off at {off:?} of {}: {:?})",
            places.len(),
            off.map(|i| &places[i - 3..i + 1])
        );
        let first = tail[0] as usize;
        let exact: Vec<i32> = (first..first + 48_000)
            .map(|i| sine_at(15_000.0, i as f64) as i32)
            .collect();
        assert_eq!(out[out.len() - 48_000..], exact[..], "bit for bit");
        // A second at +500 ppm took up about half a millisecond, glide included.
        let gained = tail[0] - (places.len() - 48_000) as f64;
        assert!((gained - 24.0).abs() < 2.0, "{gained} frames gained");
    }

    // --- The resampler kept, and made general (WP3) -------------------------------------------

    /// FNV-1a over the samples: a fingerprint of a run.
    fn fingerprint(v: &[i32]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for x in v {
            for b in x.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// The resampler before WP3, run on `golden_input` with `golden_target` in periods of 480,
    /// 441, 512 and 97 frames for 4 s: (tone, channels, fingerprint, samples). Taken from the
    /// code as it was (`Resampler::process` as of fa8817a) before it was made general.
    const GOLDEN: [(f64, usize, u64, usize); 2] = [
        (15_000.0, 1, 0x69e5_b6e8_96c0_0bee, 192_171),
        (1_000.0, 2, 0xc0a0_4051_c4b0_4f78, 384_342),
    ];
    const GOLDEN_PERIODS: [usize; 4] = [480, 441, 512, 97];

    fn golden_input(hz: f64, ch: usize) -> Vec<i32> {
        (0..48_000 * 5 * ch)
            .map(|i| {
                (f64::from(1 << 30)
                    * (2.0 * std::f64::consts::PI * hz * (i / ch) as f64 / 48_000.0
                        + (i % ch) as f64)
                        .sin()) as i32
            })
            .collect()
    }

    /// 500 ppm faster for a second, 320 slower for a second, then the song's own rate.
    fn golden_target(t: f64) -> f64 {
        if t < 1.0 {
            500.0
        } else if t < 2.0 {
            -320.0
        } else {
            0.0
        }
    }

    #[test]
    fn the_resampler_is_bit_for_bit_what_it_was() {
        for (hz, ch, print, len) in GOLDEN {
            let input = golden_input(hz, ch);
            let mut r = Resampler::default();
            let (mut used, mut out, mut k) = (0usize, Vec::new(), 0);
            while out.len() < 48_000 * 4 * ch {
                let t = (out.len() / ch) as f64 / 48_000.0;
                let want = GOLDEN_PERIODS[k % 4];
                k += 1;
                used += r
                    .process(
                        &input[used * ch..],
                        ch,
                        48_000,
                        false,
                        want,
                        golden_target(t),
                        &mut out,
                        None,
                    )
                    .unwrap();
            }
            assert_eq!((fingerprint(&out), out.len()), (print, len), "{hz} Hz");
        }
    }

    /// A voice's window as the renderer keeps it: the filter's left side, then the frames in
    /// hand, refilled from `input` before each call and moved down after it.
    struct Window {
        win: Vec<i32>,
        /// Frames in it.
        len: usize,
        /// The read point.
        at: usize,
        /// Input frames copied in so far.
        fed: usize,
    }

    impl Window {
        fn new(half: usize, ch: usize, cap: usize) -> Window {
            // Silence before the song: the filter's left side at the start.
            Window {
                win: vec![0; cap * ch],
                len: half,
                at: half,
                fed: 0,
            }
        }

        /// Room for `reach` frames from the read point, from `input` (or silence past its end).
        fn fill(&mut self, input: &[i32], ch: usize, reach: usize) {
            while self.len < self.at + reach {
                let i = self.fed * ch;
                let f = &mut self.win[self.len * ch..(self.len + 1) * ch];
                match input.get(i..i + ch) {
                    Some(s) => f.copy_from_slice(s),
                    None => f.fill(0),
                }
                self.fed += 1;
                self.len += 1;
            }
        }

        /// Keep only `half` frames before the read point.
        fn compact(&mut self, half: usize, ch: usize) {
            let cut = self.at - half;
            self.win.copy_within(cut * ch..self.len * ch, 0);
            self.len -= cut;
            self.at -= cut;
        }
    }

    /// `secs` of `input` (interleaved, `ch`) through a converter from `from` to `to` Hz in
    /// `periods`, asking `target(t)` ppm: (the frames out, each one's point in the input).
    fn convert(
        input: &[i32],
        ch: usize,
        from: u32,
        to: u32,
        secs: f64,
        periods: &[usize],
        target: impl Fn(f64) -> f64,
    ) -> (Vec<i32>, Vec<f64>) {
        let bank = Kernels::for_device(to);
        let k = bank.pick(from).unwrap();
        let mut c = Converter::new(from, to, 0.0);
        let mut w = Window::new(k.half(), ch, 8_192);
        let (mut out, mut places) = (Vec::new(), Vec::new());
        let mut n = 0;
        while out.len() < (f64::from(to) * secs) as usize * ch {
            let want = periods[n % periods.len()];
            n += 1;
            let t = (out.len() / ch) as f64 / f64::from(to);
            w.fill(input, ch, c.reach(k, want));
            let mut o = vec![0; want * ch];
            let mut at = Vec::new();
            // The window's read point is input frame `fed - (len - at)`.
            let first = (w.fed - (w.len - w.at)) as f64;
            let used = c.process_with_places(k, &w.win, ch, w.at, want, target(t), &mut o, &mut at);
            places.extend(at.iter().map(|p| p + first));
            out.extend_from_slice(&o);
            w.at += used;
            w.compact(k.half(), ch);
        }
        (out, places)
    }

    #[test]
    fn the_converter_on_a_lent_window_is_the_same_resampler() {
        for (hz, ch, print, len) in GOLDEN {
            let input = golden_input(hz, ch);
            let (out, _) = convert(&input, ch, 48_000, 48_000, 4.0, &GOLDEN_PERIODS, |t| {
                golden_target(t)
            });
            assert_eq!(
                (fingerprint(&out), out.len()),
                (print, len),
                "{hz} Hz, {ch} channels"
            );
        }
    }

    #[test]
    fn a_converter_at_its_own_rate_passes_every_frame_untouched() {
        let input: Vec<i32> = (0..96_000).map(|i| i * 7_919 - 300_000_000).collect();
        let (out, _) = convert(&input, 2, 44_100, 44_100, 1.0, &[441, 512, 1], |_| 0.0);
        assert_eq!(out[..], input[..out.len()]);
        let c = Converter::new(44_100, 44_100, 0.0);
        assert!(c.idle());
        assert!(!Converter::new(44_100, 48_000, 0.0).idle());
        assert!(!Converter::new(48_000, 48_000, 20.0).idle());
        // A new voice starts at the trim the room plays at, never past 500 ppm.
        assert_eq!(Converter::new(48_000, 48_000, 300.0).ppm(), 300.0);
        assert_eq!(Converter::new(48_000, 48_000, -900.0).ppm(), -MAX_PPM);
    }

    /// A sine of `hz` at `rate`, nearly full scale, at any (fractional) frame.
    fn tone_at(hz: f64, rate: u32, pos: f64) -> f64 {
        f64::from(1 << 30) * (2.0 * std::f64::consts::PI * hz * pos / f64::from(rate)).sin()
    }

    /// The worst error of a converted sine against the true one at each frame's point, in dB
    /// below its peak, past the first 0.1 s (the filter starting from silence).
    fn conversion_error(hz: f64, from: u32, to: u32, ppm: f64) -> f64 {
        let input: Vec<i32> = (0..from as usize * 2)
            .map(|i| tone_at(hz, from, i as f64) as i32)
            .collect();
        let (out, places) = convert(&input, 1, from, to, 1.0, &[480], |_| ppm);
        let worst = out
            .iter()
            .zip(&places)
            .skip(to as usize / 10)
            .map(|(&y, &p)| (f64::from(y) - tone_at(hz, from, p)).abs())
            .fold(0.0, f64::max);
        20.0 * (worst / f64::from(1 << 30)).log10()
    }

    #[test]
    fn rate_conversion_either_way_is_clean_to_the_top_of_hearing() {
        // Up (a CD on a 48 kHz mixer), down (hi-res on a 48 kHz mixer, 384 kHz on a CD-rate
        // device), each with a room's trim too: every frame within -90 dB of the true sine.
        for (from, to) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (88_200, 48_000),
            (96_000, 48_000),
            (192_000, 48_000),
            (384_000, 44_100),
        ] {
            for hz in [1_000.0, 15_000.0] {
                for ppm in [0.0, 500.0, -500.0] {
                    let db = conversion_error(hz, from, to, ppm);
                    assert!(db < -90.0, "{from} to {to}, {hz} Hz, {ppm} ppm: {db:.1} dB");
                }
            }
        }
    }

    #[test]
    fn nothing_above_the_devices_nyquist_folds_back() {
        // 30 kHz in a 96 kHz file played at 48 kHz would fold to 18 kHz: it must be gone.
        let input: Vec<i32> = (0..192_000)
            .map(|i| (tone_at(30_000.0, 96_000, i as f64) * 0.5) as i32)
            .collect();
        let (out, _) = convert(&input, 1, 96_000, 48_000, 1.0, &[480], |_| 0.0);
        let peak = out[4_800..]
            .iter()
            .map(|&v| f64::from(v).abs())
            .fold(0.0, f64::max);
        let db = 20.0 * (peak / 2_147_483_648.0).log10();
        assert!(db < -80.0, "30 kHz folded back at {db:.1} dBFS");
    }

    #[test]
    fn a_trimmed_conversion_reaches_its_rate() {
        let input: Vec<i32> = (0..88_200)
            .map(|i| tone_at(1_000.0, 44_100, i as f64) as i32)
            .collect();
        let (_, places) = convert(&input, 1, 44_100, 48_000, 1.5, &[480], |_| 500.0);
        let n = places.len();
        let ratio = (places[n - 1] - places[n - 481]) / 480.0;
        let want = 44_100.0 / 48_000.0 * (1.0 + 500e-6);
        assert!(
            ((ratio / want) - 1.0).abs() < 1e-6,
            "{ratio} against {want}"
        );
    }

    #[test]
    fn a_renderer_has_a_kernel_for_every_standard_rate() {
        let bank = Kernels::for_device(48_000);
        assert!(std::ptr::eq(bank.pick(44_100).unwrap(), Kernel::unity()));
        assert!(std::ptr::eq(bank.pick(48_000).unwrap(), Kernel::unity()));
        assert_eq!(bank.pick(96_000).unwrap().half(), 64);
        assert_eq!(bank.pick(192_000).unwrap().half(), 128);
        // Between standard rates: the next one above (cut a little lower).
        assert_eq!(
            bank.pick(50_000).unwrap().half(),
            bank.pick(64_000).unwrap().half()
        );
        assert!(bank.pick(400_000).is_none(), "past 384 kHz");
        let cd = Kernels::for_device(44_100);
        assert!(cd.pick(384_000).unwrap().taps() <= MAX_TAPS);
        // Every kernel about the same size: the phases fall as the taps grow.
        for (_, k) in &cd.scaled {
            let n = k.table.len();
            assert!((30_000..40_000).contains(&n), "{} taps: {n}", k.taps());
        }
    }
}
