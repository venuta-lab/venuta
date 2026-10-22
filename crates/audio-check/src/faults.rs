//! The faults every detector must catch, made on purpose (section 7.1).

/// One fault, injected at one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fault {
    /// One frame left out.
    DropFrame,
    /// One frame played twice.
    RepeatFrame,
    /// One sample out of line by `db` dBFS (-40: 0.01 of full scale), away from zero.
    Step { db: f64 },
    /// `ms` of silence cut in, with hard edges.
    Gap { ms: f64 },
    /// A jump `ms` further into the music.
    HardCut { ms: f64 },
    /// The level changed by `db` from that frame on, at once.
    GainStep { db: f64 },
}

impl Fault {
    /// A dropped or repeated frame: where the wave is nearly flat it changes almost nothing.
    pub fn is_single_frame(&self) -> bool {
        matches!(self, Fault::DropFrame | Fault::RepeatFrame)
    }
}

fn frames(ms: f64, rate: u32) -> usize {
    (ms / 1000.0 * f64::from(rate)).round() as usize
}

/// `x` with `f` at frame `at` (one channel, -1..1).
pub fn inject(x: &[f64], f: Fault, at: usize, rate: u32) -> Vec<f64> {
    let mut y = x.to_vec();
    match f {
        Fault::DropFrame => {
            y.remove(at);
        }
        Fault::RepeatFrame => y.insert(at, y[at]),
        Fault::Step { db } => {
            let d = 10f64.powf(db / 20.0);
            y[at] += if y[at] < 0.0 { -d } else { d };
        }
        Fault::Gap { ms } => {
            let end = (at + frames(ms, rate)).min(y.len());
            y[at..end].iter_mut().for_each(|v| *v = 0.0);
        }
        Fault::HardCut { ms } => {
            let end = (at + frames(ms, rate)).min(y.len());
            y.drain(at..end);
        }
        Fault::GainStep { db } => {
            let g = 10f64.powf(db / 20.0);
            y[at..].iter_mut().for_each(|v| *v *= g);
        }
    }
    y
}

/// `x` with `f` at frame `at` (one channel, 24-bit values).
pub fn inject_ints(x: &[i32], f: Fault, at: usize, rate: u32) -> Vec<i32> {
    let full = crate::music::FULL;
    let as_f: Vec<f64> = x.iter().map(|&v| f64::from(v) / full).collect();
    inject(&as_f, f, at, rate)
        .iter()
        .map(|v| (v * full).round() as i32)
        .collect()
}
