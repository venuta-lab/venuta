//! The music-shaped test songs, and an ear for clicks in them: the fast first check of section
//! 7.1 (moved here from the player's room checks, unchanged).
//!
//! The songs are a chord (near 110, 440 and 1500 Hz, none a whole number of cycles in any round time, so a cut shows), a little 8 kHz treble and a bed of soft noise,
//! so no two samples in a row repeat and silence never happens inside them, as in real music. The
//! ear knows how steep and how curved that music can ever be, and calls anything steeper or more
//! sharply bent a click: a frame dropped or repeated, a hard cut from one place to another, a
//! step in the volume, a gap of silence cut into the music, a sample out of line.

use std::path::Path;
/// Full scale for 24-bit samples.
pub const FULL: f64 = 8_388_607.0;

/// A song of the test music: frames `from..to` of one endless piece at `rate` (two songs cut from
/// it one after the other follow each other seamlessly), rising from silence over its first 50
/// ms (`rise`) and falling to it over its last (`fall`), as songs do.
pub struct Piece {
    pub rate: u32,
    pub channels: u16,
}

impl Piece {
    /// The music at frame `i` (any, fractional too), channel `c`, before any rise or fall, in
    /// 24-bit units.
    pub fn at(&self, i: f64, c: u16) -> f64 {
        let t = i / f64::from(self.rate);
        let tau = 2.0 * std::f64::consts::PI;
        // The right channel a little apart from the left.
        let p = f64::from(c) * 0.7;
        let x = 0.30 * (tau * 110.3 * t + p).sin()
            + 0.20 * (tau * 441.7 * t + 1.0 + p).sin()
            + 0.10 * (tau * 1503.1 * t + 2.0).sin()
            + 0.003 * (tau * 7993.0 * t + p).sin()
            // Soft noise: many slow, unrelated sines (no repeats, never silent, and smooth).
            + 0.004 * (tau * 37.3 * t * (1.0 + 0.1 * (tau * 0.31 * t).sin())).sin()
            + 0.003 * (tau * 2311.7 * t + 0.4 * (tau * 3.7 * t).sin()).sin();
        x * FULL * 0.9
    }

    /// The song's frame `i` (of `frames`), channel `c`, as written to its file.
    pub fn sample(
        &self,
        i: usize,
        c: u16,
        from: usize,
        frames: usize,
        rise: bool,
        fall: bool,
    ) -> i32 {
        let edge = (f64::from(self.rate) * 0.05) as usize;
        let mut g = 1.0;
        if rise && i < edge {
            g *= (i as f64 / edge as f64 * std::f64::consts::FRAC_PI_2).sin();
        }
        if fall && frames - i <= edge {
            g *= ((frames - i) as f64 / edge as f64 * std::f64::consts::FRAC_PI_2).sin();
        }
        (self.at((from + i) as f64, c) * g).round() as i32
    }

    /// Write frames `from..from + frames` as a 24-bit WAV.
    pub fn write(&self, path: &Path, from: usize, frames: usize, rise: bool, fall: bool) {
        let ch = usize::from(self.channels);
        let data_len = frames * 3 * ch;
        let mut v = Vec::with_capacity(44 + data_len);
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&self.channels.to_le_bytes());
        v.extend_from_slice(&self.rate.to_le_bytes());
        v.extend_from_slice(&(self.rate * 3 * u32::from(self.channels)).to_le_bytes());
        v.extend_from_slice(&(3 * self.channels).to_le_bytes());
        v.extend_from_slice(&24u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data_len as u32).to_le_bytes());
        for i in 0..frames {
            for c in 0..self.channels {
                let s = self.sample(i, c, from, frames, rise, fall);
                v.extend_from_slice(&s.to_le_bytes()[..3]);
            }
        }
        std::fs::write(path, v).unwrap();
    }

    /// The steepest step from one frame to the next this music ever has, and the most its third
    /// difference ever is (how sharply its bend changes: a cut, a drop or a spike changes it far
    /// more than music does), in 24-bit units.
    pub fn limits(&self) -> (f64, f64) {
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        // Several seconds of it, finely: the chord's worst moments come round within them.
        let n = self.rate as usize * 4;
        for c in 0..self.channels {
            let x: Vec<f64> = (0..n).map(|i| self.at(i as f64, c)).collect();
            for w in x.windows(4) {
                s1 = s1.max((w[1] - w[0]).abs());
                s2 = s2.max((w[3] - 3.0 * w[2] + 3.0 * w[1] - w[0]).abs());
            }
        }
        (s1, s2)
    }
}

/// A click the ear heard: where (frame), and what.
#[derive(Debug, Clone, PartialEq)]
pub struct Click {
    pub at: usize,
    pub what: &'static str,
    /// How far past the music's own limit, times.
    pub over: f64,
}

/// The clicks in one channel of what was heard, for music whose steepest step is `s1` and whose
/// third difference is at most `s3`. Silence before and after the music is no click; silence inside it is,
/// unless the music faded down to it and rises back from it (a correction done gently).
pub fn clicks(x: &[i32], s1: f64, s3: f64) -> Vec<Click> {
    let quiet = FULL * 1e-4;
    let Some(first) = x.iter().position(|&v| f64::from(v).abs() > quiet) else {
        return Vec::new();
    };
    let last = x.iter().rposition(|&v| f64::from(v).abs() > quiet).unwrap();
    let mut found: Vec<Click> = Vec::new();
    let mut add = |at: usize, what: &'static str, over: f64| {
        // One event, however many frames it touches.
        if found.last().is_some_and(|c| at < c.at + 64) {
            return;
        }
        found.push(Click { at, what, over });
    };
    // How far from silence an edge of a gap may be: the music fades into a gap and out of one
    // over many frames, so its last and first frames there are tiny.
    let edge = s1 * 0.5;
    let mut i = first;
    while i <= last {
        if x[i] == 0 && on_the_line(x, i, s1, s3) {
            // One exact zero where the music crosses zero (resampled music puts a crossing on a
            // frame now and then): music, judged as any other frame.
            i += 1;
            continue;
        }
        if x[i] == 0 {
            // A gap inside the music.
            let start = i;
            while i <= last && x[i] == 0 {
                i += 1;
            }
            let before = f64::from(x[start.saturating_sub(1)]).abs();
            let after = f64::from(x[i.min(x.len() - 1)]).abs();
            if before > edge || after > edge {
                add(
                    start,
                    "silence cut into the music",
                    before.max(after) / edge,
                );
            }
            continue;
        }
        if i >= 1 && i + 2 < x.len() && x[i - 1..=i + 2].iter().all(|&v| v != 0) {
            let [a, b, c, d] = [x[i - 1], x[i], x[i + 1], x[i + 2]].map(f64::from);
            let d1 = (c - b).abs();
            let d3 = (d - 3.0 * c + 3.0 * b - a).abs();
            if d1 > s1 * 1.25 {
                add(i, "a step steeper than the music", d1 / s1);
            } else if d3 > s3 * 1.25 {
                add(i, "a break in the music's line", d3 / s3);
            }
        }
        i += 1;
    }
    found
}

/// Frame `i` is a lone zero whose neighbours are music (three each side, none zero), and taken as
/// a value it keeps the music's line: no step past the music's steepest and no third difference
/// past its most, in every window that holds it. A frame zeroed where the music is away from
/// zero breaks the line (by about three times its value in the third difference), so it is
/// still a cut.
fn on_the_line(x: &[i32], i: usize, s1: f64, s3: f64) -> bool {
    if i < 3 || i + 3 >= x.len() {
        return false;
    }
    let w = &x[i - 3..=i + 3];
    if w.iter().enumerate().any(|(k, &v)| k != 3 && v == 0) {
        return false;
    }
    let v: Vec<f64> = w.iter().map(|&s| f64::from(s)).collect();
    let steps_ok = (2..4).all(|k| (v[k + 1] - v[k]).abs() <= s1 * 1.25);
    let bends_ok =
        (0..4).all(|k| (v[k + 3] - 3.0 * v[k + 2] + 3.0 * v[k + 1] - v[k]).abs() <= s3 * 1.25);
    steps_ok && bends_ok
}
