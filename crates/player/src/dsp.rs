//! Sound tools: equalizer, bass and treble, preamp, ReplayGain, and a limiter. All off by
//! default; when nothing is on, samples pass straight through (bit-perfect). When something is
//! on, each step is named in the signal path.

use serde::{Deserialize, Serialize};

/// The equalizer's bands, in Hz.
pub const BANDS: [f64; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Leveling {
    #[default]
    Off,
    /// Every song at the same loudness.
    Track,
    /// Each album at the same loudness, keeping the differences within it.
    Album,
}

/// What you set. dB values; 0 means untouched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    pub eq: bool,
    pub bands: [f32; 10],
    /// Shown in the signal path ("Bass boost").
    pub preset: String,
    pub preamp_db: f32,
    pub bass_db: f32,
    pub treble_db: f32,
    pub leveling: Leveling,
    /// Added to ReplayGain (most music is mastered louder than ReplayGain's reference).
    pub leveling_preamp_db: f32,
    /// Keep boosts from clipping.
    pub limiter: bool,
    /// Blend the end of one song into the next over this many seconds (0: off). Songs from
    /// the same album are never blended (live albums and mixes stay seamless).
    #[serde(default)]
    pub crossfade_s: f32,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            eq: false,
            bands: [0.0; 10],
            preset: "Flat".into(),
            preamp_db: 0.0,
            bass_db: 0.0,
            treble_db: 0.0,
            leveling: Leveling::Off,
            leveling_preamp_db: 0.0,
            limiter: true,
            crossfade_s: 0.0,
        }
    }
}

/// The gap between the catalog's loudness reference (about -14 LUFS) and ReplayGain's (-18).
const CATALOG_TO_REPLAYGAIN_DB: f64 = -4.0;

impl Gain {
    /// This song's gain, or, when its tags give none (an online song), the gain from the
    /// catalog's loudness for it, as ReplayGain would have measured it.
    pub fn or_loudness(self, loudness_db: Option<f64>) -> Gain {
        if self.track_db.is_some() || self.album_db.is_some() {
            return self;
        }
        let Some(l) = loudness_db.filter(|l| l.is_finite()) else {
            return self;
        };
        let db = (-l + CATALOG_TO_REPLAYGAIN_DB) as f32;
        Gain {
            track_db: Some(db),
            album_db: Some(db),
            ..self
        }
    }
}

/// ReplayGain for one song, from its tags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Gain {
    pub track_db: Option<f32>,
    pub album_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_peak: Option<f32>,
}

/// One biquad filter (RBJ cookbook), per channel.
#[derive(Debug, Clone)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z: Vec<[f64; 2]>,
}

impl Biquad {
    fn new(kind: Kind, f0: f64, gain_db: f64, q: f64, rate: f64, channels: usize) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = std::f64::consts::TAU * (f0 / rate).min(0.49);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            Kind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            Kind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            Kind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z: vec![[0.0; 2]; channels],
        }
    }

    #[inline]
    fn run(&mut self, c: usize, x: f64) -> f64 {
        // Transposed direct form II.
        let z = &mut self.z[c];
        let y = self.b0 * x + z[0];
        z[0] = self.b1 * x - self.a1 * y + z[1];
        z[1] = self.b2 * x - self.a2 * y;
        y
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Peak,
    LowShelf,
    HighShelf,
}

/// The tools, ready for one sample rate and channel count.
pub struct Chain {
    filters: Vec<Biquad>,
    gain: f64,
    limiter: bool,
    /// Bits of the output, for dither (16-bit output gets TPDF dither).
    out_bits: u16,
    rng: u64,
    pub active: bool,
    pub steps: Vec<String>,
    /// While gliding: the chain this one took over from (none: the sound untouched). It stays
    /// here after the glide, until this chain is handed back, so nothing is freed where the
    /// chain runs (the render thread).
    prev: Option<Box<Chain>>,
    /// Frames of the glide done, and its length (0: no glide).
    glide_done: u64,
    glide_len: u64,
    /// The old chain's work during a glide (made with the chain, never where it runs).
    scratch: Box<[i32]>,
}

/// The most samples a glide works on at once (more are taken in pieces).
const GLIDE_PIECE: usize = 4096;

fn db(x: f32) -> f64 {
    10f64.powf(f64::from(x) / 20.0)
}

impl Chain {
    pub fn new(sound: &Sound, gain: &Gain, rate: u32, channels: u16, out_bits: u16) -> Self {
        let rate = f64::from(rate.max(1));
        let ch = usize::from(channels.max(1));
        let mut filters = Vec::new();
        let mut steps = Vec::new();
        if sound.eq && sound.bands.iter().any(|b| b.abs() > 0.05) {
            for (f, g) in BANDS.iter().zip(sound.bands) {
                if g.abs() > 0.05 && *f < rate / 2.0 {
                    filters.push(Biquad::new(Kind::Peak, *f, f64::from(g), 1.41, rate, ch));
                }
            }
            steps.push(format!("Equalizer ({})", sound.preset));
        }
        if sound.bass_db.abs() > 0.05 {
            filters.push(Biquad::new(
                Kind::LowShelf,
                100.0,
                f64::from(sound.bass_db),
                0.707,
                rate,
                ch,
            ));
            steps.push(format!("Bass {:+.1} dB", sound.bass_db));
        }
        if sound.treble_db.abs() > 0.05 {
            filters.push(Biquad::new(
                Kind::HighShelf,
                10_000.0,
                f64::from(sound.treble_db),
                0.707,
                rate,
                ch,
            ));
            steps.push(format!("Treble {:+.1} dB", sound.treble_db));
        }
        let mut g = 1.0;
        if sound.preamp_db.abs() > 0.05 {
            g *= db(sound.preamp_db);
            steps.push(format!("Preamp {:+.1} dB", sound.preamp_db));
        }
        let (rg, peak) = match sound.leveling {
            Leveling::Off => (None, None),
            Leveling::Track => (
                gain.track_db.or(gain.album_db),
                gain.track_peak.or(gain.album_peak),
            ),
            Leveling::Album => (
                gain.album_db.or(gain.track_db),
                gain.album_peak.or(gain.track_peak),
            ),
        };
        if let Some(r) = rg {
            let total = r + sound.leveling_preamp_db;
            let mut lg = db(total);
            // Peak protection: never push the loudest sample past full scale.
            if let Some(p) = peak.filter(|p| *p > 0.0) {
                lg = lg.min(1.0 / f64::from(p));
            }
            g *= lg;
            steps.push(format!(
                "Volume levelled ({}, {:+.1} dB)",
                if sound.leveling == Leveling::Album {
                    "album"
                } else {
                    "song"
                },
                20.0 * lg.log10()
            ));
        } else if sound.leveling != Leveling::Off {
            steps.push("Volume levelling on, but this song has no ReplayGain values".into());
        }
        let active = !filters.is_empty() || (g - 1.0).abs() > 1e-6;
        if active && sound.limiter {
            steps.push("Limiter".into());
        }
        Self {
            filters,
            gain: g,
            limiter: sound.limiter,
            out_bits,
            rng: 0x9E37_79B9_7F4A_7C15,
            active,
            steps,
            prev: None,
            glide_done: 0,
            glide_len: 0,
            scratch: Box::default(),
        }
    }

    /// This chain, to take over from the one a voice has over `frames` frames instead of at once
    /// (a change of the tools while the voice sounds: a step in them would be heard). Its room
    /// for the glide is made here (on the control thread).
    pub fn gliding(mut self, frames: u32) -> Self {
        self.glide_len = u64::from(frames);
        self.glide_done = 0;
        if frames > 0 {
            self.scratch = vec![0; GLIDE_PIECE].into_boxed_slice();
        }
        self
    }

    /// Take over from `old`, the chain the voice had (`None`: no tools). Without a glide `old`
    /// comes back at once; with one it is kept to glide from, and what it was itself gliding
    /// from comes back instead. Only moves boxes (the renderer calls it on its own thread), and
    /// whatever comes back is for the caller to hand off that thread.
    pub fn take_over(&mut self, old: Option<Box<Chain>>) -> Option<Box<Chain>> {
        if self.glide_len == 0 {
            return old;
        }
        let mut old = old;
        let older = old.as_mut().and_then(|o| o.prev.take());
        self.prev = old;
        self.glide_done = 0;
        older
    }

    /// The chain changes the sound: a tool is on, or it is gliding from the chain before.
    pub fn touches(&self) -> bool {
        self.active || self.glide_done < self.glide_len
    }

    fn noise(&mut self) -> f64 {
        // xorshift: fast, good enough for dither.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Process left-justified interleaved samples in place. Does nothing (not one bit) when
    /// no tool is on and it isn't gliding. Never allocates: the renderer runs it on the render
    /// thread, after the resampler, so a chain for the renderer is made for the device's rate
    /// and the voice's channels.
    ///
    /// Gliding, it runs the chain before it and itself on the same frames and goes from the
    /// first's output to its own on a raised cosine over the glide, with equal gains (both are
    /// the same music, so the sum keeps its level).
    pub fn process(&mut self, samples: &mut [i32], channels: u16) {
        if self.glide_done >= self.glide_len {
            self.run(samples, channels);
            return;
        }
        let ch = usize::from(channels.max(1));
        let piece = (GLIDE_PIECE / ch).max(1) * ch;
        for part in samples.chunks_mut(piece) {
            let n = part.len();
            if n > self.scratch.len() {
                // Never reached: `gliding` made the room.
                self.run(part, channels);
                continue;
            }
            let mut scratch = std::mem::take(&mut self.scratch);
            scratch[..n].copy_from_slice(part);
            if let Some(p) = self.prev.as_mut() {
                p.process(&mut scratch[..n], channels);
            }
            self.run(part, channels);
            for (k, (new, old)) in part
                .chunks_exact_mut(ch)
                .zip(scratch[..n].chunks_exact(ch))
                .enumerate()
            {
                let done = self.glide_done + k as u64;
                if done >= self.glide_len {
                    break;
                }
                let t = (done + 1) as f64 / self.glide_len as f64;
                let w = (1.0 - (std::f64::consts::PI * t).cos()) / 2.0;
                for (y, &x) in new.iter_mut().zip(old) {
                    let v = f64::from(x) * (1.0 - w) + f64::from(*y) * w;
                    *y = v.round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                }
            }
            self.scratch = scratch;
            self.glide_done = (self.glide_done + (n / ch) as u64).min(self.glide_len);
        }
    }

    /// The tools themselves.
    fn run(&mut self, samples: &mut [i32], channels: u16) {
        if !self.active {
            return;
        }
        let ch = usize::from(channels.max(1));
        let full = 2_147_483_648.0;
        // One output step, for dither at 16 bits.
        let lsb = if self.out_bits <= 16 {
            full / 32_768.0
        } else {
            0.0
        };
        for (i, s) in samples.iter_mut().enumerate() {
            let c = i % ch;
            let mut x = f64::from(*s) / full;
            for f in &mut self.filters {
                x = f.run(c, x);
            }
            x *= self.gain;
            if self.limiter {
                // Soft knee above -1 dBFS: smooth rather than clipped.
                let t = 0.891;
                let a = x.abs();
                if a > t {
                    x = x.signum() * (t + (1.0 - t) * ((a - t) / (1.0 - t)).tanh());
                }
            }
            let mut y = x * full;
            if lsb > 0.0 {
                y += (self.noise() - self.noise()) * lsb;
            }
            *s = y.round().clamp(-full, full - 1.0) as i32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_online_song_is_levelled_by_the_catalogs_loudness_and_tags_win() {
        // 6 dB louder than the catalog's reference: turned down 10 dB at ReplayGain's.
        let g = Gain::default().or_loudness(Some(6.0));
        assert_eq!(g.track_db, Some(-10.0));
        assert_eq!(g.album_db, Some(-10.0));
        let tagged = Gain {
            track_db: Some(-7.5),
            ..Gain::default()
        };
        assert_eq!(
            tagged.or_loudness(Some(6.0)),
            tagged,
            "a file's own gain is kept"
        );
        assert_eq!(Gain::default().or_loudness(None), Gain::default());
    }

    fn tone(n: usize) -> Vec<i32> {
        (0..n)
            .flat_map(|i| {
                let v = ((i as f64 * 0.05).sin() * 0.25 * 2_147_483_648.0) as i32;
                [v, -v]
            })
            .collect()
    }

    #[test]
    fn nothing_on_changes_nothing() {
        let mut c = Chain::new(&Sound::default(), &Gain::default(), 48_000, 2, 24);
        assert!(!c.active);
        assert!(c.steps.is_empty());
        let a = tone(4800);
        let mut b = a.clone();
        c.process(&mut b, 2);
        assert_eq!(a, b, "bit-exact when off");
        // An equalizer switched on but flat is still nothing.
        let flat = Sound {
            eq: true,
            ..Sound::default()
        };
        assert!(!Chain::new(&flat, &Gain::default(), 48_000, 2, 24).active);
    }

    #[test]
    fn a_boost_is_named_and_the_limiter_keeps_it_from_clipping() {
        let s = Sound {
            preamp_db: 12.0,
            ..Sound::default()
        };
        let mut c = Chain::new(&s, &Gain::default(), 44_100, 2, 24);
        assert!(c.active);
        assert_eq!(
            c.steps,
            vec!["Preamp +12.0 dB".to_string(), "Limiter".to_string()]
        );
        let mut x = tone(2000);
        c.process(&mut x, 2);
        let peak = x.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak < i32::MAX as u32, "no clipping");
        assert!(peak > (0.8 * 2_147_483_648.0) as u32, "but louder");
    }

    #[test]
    fn replaygain_respects_the_peak() {
        let s = Sound {
            leveling: Leveling::Track,
            ..Sound::default()
        };
        let g = Gain {
            track_db: Some(6.0),
            track_peak: Some(0.9),
            ..Gain::default()
        };
        let c = Chain::new(&s, &g, 44_100, 2, 24);
        // +6 dB would push a 0.9 peak past full scale; it stops at 1/0.9.
        assert!((c.gain - 1.0 / 0.9).abs() < 1e-6);
        assert!(
            c.steps[0].starts_with("Volume levelled (song, +0.9 dB)"),
            "{:?}",
            c.steps
        );
    }

    /// A steady signal through a chain gliding from `old` (`None`: no tools) to `new`, in calls
    /// of 100 frames: the samples of the left channel.
    fn glide(old: Option<Chain>, new: Chain, frames: usize) -> Vec<i32> {
        let mut new = new;
        assert!(new.take_over(old.map(Box::new)).is_none());
        let mut out = Vec::new();
        for _ in 0..frames / 100 {
            let mut x = vec![(0.25 * 2_147_483_648.0) as i32; 200];
            new.process(&mut x, 2);
            out.extend(x.iter().step_by(2));
        }
        out
    }

    #[test]
    fn a_change_of_the_tools_glides_from_the_old_sound_to_the_new() {
        let loud = || {
            Chain::new(
                &Sound {
                    preamp_db: 6.0,
                    limiter: false,
                    ..Sound::default()
                },
                &Gain::default(),
                48_000,
                2,
                24,
            )
        };
        let plain = Chain::new(&Sound::default(), &Gain::default(), 48_000, 2, 24);
        // From +6 dB to nothing over 480 frames: from the old level to the new, never a step
        // bigger than the raised cosine's steepest, and the source itself after it.
        let x = 0.25 * 2_147_483_648.0;
        let out = glide(Some(loud()), plain.gliding(480), 1_000);
        let (hi, lo) = (x * 10f64.powf(0.3), x);
        assert!((f64::from(out[0]) - hi).abs() < hi * 1e-4, "{}", out[0]);
        let most = (hi - lo) * std::f64::consts::PI / 2.0 / 480.0 + 2.0;
        for w in out.windows(2) {
            assert!(f64::from(w[0] - w[1]).abs() <= most, "a step: {w:?}");
            assert!(w[1] <= w[0], "only down");
        }
        assert!(
            out[480..].iter().all(|&v| v == lo as i32),
            "untouched after"
        );
        // And up again, from no tools at all.
        let out = glide(None, loud().gliding(480), 1_000);
        assert!((f64::from(out[0]) - lo).abs() < 1e-3 * lo);
        assert!((f64::from(out[999]) - hi).abs() < 1.0);
        for w in out.windows(2) {
            assert!(f64::from(w[1] - w[0]).abs() <= most, "a step: {w:?}");
        }
    }

    #[test]
    fn a_glide_keeps_what_it_glides_from_and_hands_back_what_that_glided_from() {
        let plain = || Chain::new(&Sound::default(), &Gain::default(), 48_000, 2, 24);
        let mut a = plain().gliding(10);
        assert!(a.take_over(Some(Box::new(plain()))).is_none());
        assert!(a.touches(), "gliding");
        let mut x = vec![7; 40];
        a.process(&mut x, 2);
        assert!(!a.touches(), "the glide is over, and nothing is on");
        assert!(
            a.prev.is_some(),
            "kept until the chain itself is handed back"
        );
        let mut b = plain().gliding(10);
        let back = b.take_over(Some(Box::new(a)));
        assert!(back.is_some(), "what the old chain glided from comes back");
        let mut c = plain();
        assert!(
            c.take_over(Some(Box::new(b))).is_some(),
            "no glide: the old one back"
        );
    }
}
