//! Finding a place of one capture in another: a room run against a solo run of the same
//! songs, so a flag the solo run also has (the music's own) is told from one the room made,
//! and the bit-for-bit comparison of the one speaker mode.

use super::fft::find_in;
use super::wav::Wav;

/// Decimation for the coarse search (48 kHz becomes 3 kHz).
const COARSE: usize = 16;

pub struct Finder {
    full: Vec<f64>,
    coarse: Vec<f64>,
    rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// The frame in the other capture.
    pub at: usize,
    /// Normalised correlation there (1 is the same shape).
    pub score: f64,
}

fn decimate(x: &[f64], f: usize) -> Vec<f64> {
    x.chunks(f)
        .map(|c| c.iter().sum::<f64>() / f as f64)
        .collect()
}

impl Finder {
    pub fn new(other_mono: Vec<f64>, rate: u32) -> Self {
        let coarse = decimate(&other_mono, COARSE);
        Self {
            full: other_mono,
            coarse,
            rate,
        }
    }

    /// Where frame `at` of `mine` (mono) is in the other capture, if it is there. Where the
    /// music repeats (a loop, a chorus) several places fit; `hint` (the other capture's frame
    /// minus this one's, from the last place found) picks the nearest of them.
    pub fn locate(&self, mine: &[f64], at: usize, hint: Option<i64>) -> Option<Located> {
        let r = self.rate as usize;
        // Coarse: a second around the place.
        let half = r / 2;
        let from = at.saturating_sub(half);
        let to = (at + half).min(mine.len());
        if to <= from + r / 10 {
            return None;
        }
        let needle = decimate(&mine[from..to], COARSE);
        if energy(&needle) < 1e-12 {
            return None;
        }
        let c = find_in(&needle, &self.coarse);
        let (top, score) = best(&c)?;
        if score < 0.8 {
            return None;
        }
        // Every place nearly as good as the best (local peaks), nearest the hint first.
        let k = match hint {
            Some(h) => {
                let want = (from as i64 + h) as f64 / COARSE as f64;
                (1..c.len().saturating_sub(1))
                    .filter(|&i| c[i] >= 0.98 * score && c[i] >= c[i - 1] && c[i] >= c[i + 1])
                    .min_by(|&a, &b| (a as f64 - want).abs().total_cmp(&(b as f64 - want).abs()))
                    .unwrap_or(top)
            }
            None => top,
        };
        // Fine: 4096 frames around the place, within two coarse steps of the coarse answer.
        let guess = k * COARSE + (at - from);
        let w = 2048usize;
        let nf = at.saturating_sub(w);
        let nt = (at + w).min(mine.len());
        let needle = &mine[nf..nt];
        let reach = 2 * COARSE + 8;
        let lo = (guess + nf).saturating_sub(at + reach);
        let hi = (guess + (nt - at) + reach).min(self.full.len());
        if hi <= lo + needle.len() {
            return None;
        }
        let c = find_in(needle, &self.full[lo..hi]);
        let (k, score) = best(&c)?;
        (score >= 0.9).then_some(Located {
            at: lo + k + (at - nf),
            score,
        })
    }
}

fn energy(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum::<f64>()
}

fn best(c: &[f64]) -> Option<(usize, f64)> {
    c.iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// How two captures of the same playback compare, sample for sample.
#[derive(Debug, Clone)]
pub struct Comparison {
    /// `b`'s frame = `a`'s frame + offset.
    pub offset: i64,
    /// Frames compared (where both have sound or the stretch between).
    pub frames: usize,
    pub differing_frames: usize,
    pub max_diff: f32,
    /// The first differing frame of `a`, and up to 20 differing stretches (frame, frames).
    pub stretches: Vec<(usize, usize)>,
}

/// `a` against `b`, lined up on the first second of sound in `a`.
pub fn compare(a: &Wav, b: &Wav) -> Result<Comparison, String> {
    if a.rate != b.rate || a.channels != b.channels {
        return Err(format!(
            "different formats: {} Hz {} ch against {} Hz {} ch",
            a.rate, a.channels, b.rate, b.channels
        ));
    }
    let am = a.mono();
    let bm = b.mono();
    let first = am
        .iter()
        .position(|v| v.abs() > 1e-4)
        .ok_or("the first capture is silent")?;
    let finder = Finder::new(bm, b.rate);
    let probe = (first + a.rate as usize / 2).min(am.len().saturating_sub(1));
    let loc = finder
        .locate(&am, probe, None)
        .ok_or("the first capture's start is not found in the second")?;
    let offset = loc.at as i64 - probe as i64;
    let ch = usize::from(a.channels);
    let (fa, fb) = (a.frames(), b.frames());
    let start = first.max((-offset).max(0) as usize);
    let end = fa.min((fb as i64 - offset).max(0) as usize);
    let mut c = Comparison {
        offset,
        frames: end.saturating_sub(start),
        differing_frames: 0,
        max_diff: 0.0,
        stretches: Vec::new(),
    };
    let mut open: Option<(usize, usize)> = None;
    for i in start..end {
        let j = (i as i64 + offset) as usize;
        let mut differs = false;
        for k in 0..ch {
            let d = (a.data[i * ch + k] - b.data[j * ch + k]).abs();
            if d != 0.0 {
                differs = true;
                c.max_diff = c.max_diff.max(d);
            }
        }
        if differs {
            c.differing_frames += 1;
            open = match open {
                Some((s, l)) if s + l + 64 >= i => Some((s, i + 1 - s)),
                Some(done) => {
                    if c.stretches.len() < 20 {
                        c.stretches.push(done);
                    }
                    Some((i, 1))
                }
                None => Some((i, 1)),
            };
        }
    }
    if let Some(done) = open
        && c.stretches.len() < 20
    {
        c.stretches.push(done);
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_kit::faults;

    #[test]
    fn a_place_is_found_in_another_capture() {
        let rate = 48_000;
        let a = faults::dense(rate, 6.0, 1);
        let am: Vec<f64> = a.chunks(2).map(|f| f64::from(f[0] + f[1]) / 2.0).collect();
        // The other capture starts 0.7311 s later in the music.
        let shift = 35_093usize;
        let bm = am[shift..].to_vec();
        let f = Finder::new(bm, rate);
        let loc = f.locate(&am, 150_000, None).unwrap();
        assert_eq!(loc.at, 150_000 - shift);
        assert!(loc.score > 0.99);
    }

    #[test]
    fn identical_captures_compare_equal_and_a_change_shows() {
        let rate = 48_000;
        let data = faults::dense(rate, 3.0, 2);
        let mut lead = vec![0.0f32; 2 * 1000];
        lead.extend_from_slice(&data);
        let a = Wav {
            rate,
            channels: 2,
            data: data.clone(),
        };
        let b = Wav {
            rate,
            channels: 2,
            data: lead,
        };
        let c = compare(&a, &b).unwrap();
        assert_eq!(c.offset, 1000);
        assert_eq!(c.differing_frames, 0);
        let mut d = b.clone();
        d.data[2 * 50_000] += 1e-6;
        let c = compare(&a, &d).unwrap();
        assert_eq!(c.differing_frames, 1);
        assert_eq!(c.stretches, vec![(49_000, 1)]);
    }

    #[test]
    fn a_repeated_passage_is_placed_by_the_hint() {
        let rate = 48_000;
        let one = faults::dense(rate, 3.0, 3);
        // The same 3 s twice (a loop), then something else.
        let mut b: Vec<f32> = one.clone();
        b.extend_from_slice(&one);
        b.extend_from_slice(&faults::dense(rate, 2.0, 4));
        let bm: Vec<f64> = b.chunks(2).map(|f| f64::from(f[0] + f[1]) / 2.0).collect();
        let f = Finder::new(bm.clone(), rate);
        // Mine is the second time through, 0.25 s later in the file.
        let shift = 12_000i64;
        let mine: Vec<f64> = bm[..bm.len() - shift as usize].to_vec();
        let mut lead = vec![0.0; shift as usize];
        lead.extend_from_slice(&mine);
        let at = 3 * 48_000 + 72_000 + shift as usize;
        let near_second = f.locate(&lead, at, Some(-shift)).unwrap();
        assert_eq!(near_second.at as i64, at as i64 - shift);
        let near_first = f.locate(&lead, at, Some(-shift - 144_000)).unwrap();
        assert_eq!(near_first.at as i64, at as i64 - shift - 144_000);
    }
}
