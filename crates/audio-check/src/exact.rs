//! Detector 1: bit-exact with declared edits. Every frame heard equals the source's next frame,
//! except inside the spans the renderer declared (fades, crossfades, resampled spans), after
//! which the place in the source is found again. Silence before the first sound and after the
//! last is the device's, not the music's.

use crate::{Click, Declared, Flags};

/// How many frames in a row identify a place in the source.
const LOCATE: usize = 8;

/// Where the heard samples come from.
pub trait Reference {
    /// The source's frame `pos`, if it has one.
    fn at(&self, pos: usize) -> Option<i32>;
    /// Where in the source `heard` (at least `LOCATE` frames) starts, looking near `hint` first.
    fn locate(&self, heard: &[i32], hint: Option<usize>) -> Option<usize>;
}

/// A source given as its samples (one channel).
pub struct Samples<'a>(pub &'a [i32]);

impl Reference for Samples<'_> {
    fn at(&self, pos: usize) -> Option<i32> {
        self.0.get(pos).copied()
    }

    fn locate(&self, heard: &[i32], hint: Option<usize>) -> Option<usize> {
        let n = heard.len();
        let fits = |p: usize| self.0.get(p..p + n) == Some(heard);
        // Near where it was first (a declared edit moves the place a little), else anywhere,
        // where the first sample matches before the rest is compared.
        let near = hint.map(|h| h.saturating_sub(4_800)..(h + 4_800).min(self.0.len()));
        if let Some(p) = near.and_then(|mut r| r.find(|&p| fits(p))) {
            return Some(p);
        }
        self.0
            .iter()
            .enumerate()
            .filter(|&(_, &v)| v == heard[0])
            .map(|(p, _)| p)
            .find(|&p| fits(p))
    }
}

/// A counting song: its frame k holds `base + k + 1` (each frame says which it is).
pub struct Counting {
    pub base: i32,
    pub frames: usize,
}

impl Reference for Counting {
    fn at(&self, pos: usize) -> Option<i32> {
        (pos < self.frames).then(|| self.base + pos as i32 + 1)
    }

    fn locate(&self, heard: &[i32], _hint: Option<usize>) -> Option<usize> {
        let k = heard[0].checked_sub(self.base + 1)?;
        let p = usize::try_from(k).ok()?;
        heard
            .iter()
            .enumerate()
            .all(|(i, &v)| self.at(p + i) == Some(v))
            .then_some(p)
    }
}

/// Every place `heard` (one channel) isn't the source, outside the declared spans.
pub fn check(heard: &[i32], r: &dyn Reference, declared: &Declared) -> Vec<Click> {
    let mut flags = Flags::new(64);
    let Some(first) = heard.iter().position(|&v| v != 0) else {
        return Vec::new();
    };
    let last = heard.iter().rposition(|&v| v != 0).unwrap();
    // Where in the source the next frame heard should be (`None`: to be found).
    let mut pos: Option<usize> = None;
    let mut hint = None;
    let mut i = first;
    while i <= last {
        if declared.covers(i) {
            pos = None;
            i += 1;
            continue;
        }
        let Some(p) = pos else {
            let end = (i + LOCATE).min(last + 1);
            let found = if end - i == LOCATE {
                r.locate(&heard[i..end], hint)
            } else {
                None
            };
            match found {
                Some(p) => pos = Some(p),
                None => {
                    // Lost: one event, looked for again a little on (not at every frame).
                    flags.add(i, "a frame not from the source", 1.0);
                    i += LOCATE;
                }
            }
            continue;
        };
        let v = heard[i];
        if r.at(p) == Some(v) {
            pos = Some(p + 1);
            hint = Some(p + 1);
            i += 1;
            continue;
        }
        let next = heard.get(i + 1).copied();
        if r.at(p + 1) == Some(v) && r.at(p + 2) == next {
            flags.add(i, "a frame dropped", 1.0);
            pos = Some(p + 2);
        } else if p > 0 && r.at(p - 1) == Some(v) && r.at(p) == next {
            flags.add(i, "a frame repeated", 1.0);
        } else if r.at(p + 1) == next {
            flags.add(i, "a sample changed", 1.0);
            pos = Some(p + 1);
        } else {
            flags.add(i, "a jump to another place", 1.0);
            pos = None;
            hint = Some(p);
        }
        i += 1;
    }
    flags.done()
}
