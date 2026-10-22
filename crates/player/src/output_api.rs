//! Output v2 (contract 5.2 of the Listen together design): a callback-shaped output that owns
//! its real-time render thread (WASAPI now, AAudio later, `sim::SimOutput` in tests), with the
//! device's own report of which frame is at the DAC at which moment.
//!
//! Device frames count from 0 at every `start`: the period handed to `render` with `at = k`
//! carries device frames `k..k + frames`. The presentation pair says which of those frames was
//! at the DAC at which moment of the platform's monotonic clock (`together::time::MonoClock`).
//!
//! Nothing in the app uses this yet; today's engine still writes to `output::Device`.

use crate::output::{DeviceFormat, Mode};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

/// What the engine asks an output to open for: the music's own format. The output answers
/// with the format it runs at (`OutputInfo`): the same in exclusive mode where it can, the mix
/// format in shared mode (decision D2: our resampler converts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub rate: u32,
    pub channels: u16,
    /// Bits that carry the signal (16, 24, 32).
    pub bits: u16,
}

/// The format an output runs at, and its period and buffer, in frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    pub format: DeviceFormat,
    /// The most frames one `render` call is asked for.
    pub period_frames: u32,
    /// What the device holds before the DAC: the output's own latency.
    pub buffer_frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputError {
    /// Another app has it (exclusive mode).
    Busy,
    /// No such device, or it is gone.
    Lost,
    /// It can't take this format.
    Unsupported(String),
    /// Opened or started at the wrong moment (start before open, open twice).
    State(String),
    Other(String),
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputError::Busy => f.write_str("Another app is using the sound device."),
            OutputError::Lost => f.write_str("The sound device is gone."),
            OutputError::Unsupported(w) | OutputError::State(w) | OutputError::Other(w) => {
                f.write_str(w)
            }
        }
    }
}

impl std::error::Error for OutputError {}

/// What happened to an output, for the control thread (it fades, reopens, or moves on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    /// The device was lost (unplugged, invalidated): render is no longer called.
    Lost,
    /// The system's default output changed (when the output follows the default).
    DefaultChanged,
    /// The DAC ran out of frames before the period at device frame `at` arrived: a gap.
    Xrun { at: u64 },
}

/// The renderer the output calls from its own real-time thread.
pub trait Render: Send {
    /// Fill `out` (device format, interleaved, `frames` frames) for the period starting at
    /// device frame `at`. Never allocates, locks or blocks.
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64);
}

/// An output device.
pub trait Output: Send {
    fn name(&self) -> String;
    /// Exclusive, Shared, or Memory (the simulated output).
    fn mode(&self) -> Mode;
    fn open(&mut self, want: &OpenRequest) -> Result<OutputInfo, OutputError>;
    /// Start calling `render` from the output's own real-time thread. The device frame count
    /// starts again at 0, and nothing held from before plays.
    fn start(&mut self, render: Box<dyn Render>) -> Result<(), OutputError>;
    /// Stop calling `render` and hand the renderer back. What the output already holds plays
    /// out (the renderer's silence tail, written before the call), then the device stops.
    fn stop(&mut self) -> Option<Box<dyn Render>>;
    fn close(&mut self);
    /// The newest (device frame at the DAC, mono time) pair, lock-free.
    fn presentation(&self) -> Arc<Presentation>;
    /// Events from now on; each call gives a new receiver and the older one hears no more.
    fn events(&self) -> Receiver<OutputEvent>;
    /// The latency the output learned while running (since its last start), beside the
    /// driver's own figure in `OutputInfo::buffer_frames`. `None` before it knows.
    fn latency(&self) -> Option<Latency> {
        None
    }
}

/// One reading of the device: device frame `frame` was at the DAC at mono time `time_ns`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationPair {
    pub frame: u64,
    pub time_ns: u64,
}

/// The newest presentation pair, written by the output (once per buffer) and read by anyone,
/// lock-free (a sequence lock over atomics: a reader retries the rare read that overlaps a
/// write).
#[derive(Debug, Default)]
pub struct Presentation {
    seq: AtomicU64,
    frame: AtomicU64,
    time_ns: AtomicU64,
}

impl Presentation {
    /// Only the output writes (one writer).
    pub fn publish(&self, p: PresentationPair) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        std::sync::atomic::fence(Ordering::Release);
        self.frame.store(p.frame, Ordering::Relaxed);
        self.time_ns.store(p.time_ns, Ordering::Relaxed);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Forget the pair (a new start: none until the device reports again).
    pub fn clear(&self) {
        self.publish(PresentationPair {
            frame: u64::MAX,
            time_ns: 0,
        });
    }

    /// The newest pair (`None` before the first, or after `clear`).
    pub fn read(&self) -> Option<PresentationPair> {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let frame = self.frame.load(Ordering::Relaxed);
            let time_ns = self.time_ns.load(Ordering::Relaxed);
            std::sync::atomic::fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return (s1 != 0 && frame != u64::MAX)
                    .then_some(PresentationPair { frame, time_ns });
            }
        }
    }
}

/// The output's latency as it learned it while running (from its own presentation): how many
/// frames handed over to the device were not yet at the DAC, right after each fill. The
/// driver's own figure stays in `OutputInfo::buffer_frames`; many drivers say less than they
/// hold (the owner's Realtek laptop: 50 ms said, 74 ms held in shared mode; 30 ms said, about
/// 110 ms held in exclusive mode, a queue whose depth wobbles by about a buffer with the
/// driver's uneven events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Latency {
    /// The usual value: the median of the last `LATENCY_SAMPLES` fills, in frames.
    pub frames: u32,
    /// The most over those fills: the furthest the DAC is behind what was handed over.
    pub most: u32,
    /// How much it wobbles from fill to fill (the device's event jitter, in frames): the
    /// interquartile range of those fills, at least 1 ms.
    pub band: u32,
    /// Enough fills seen, and steady: the middle halves (interquartile ranges) of the older
    /// and of the newer half of them overlap, so the wobble has not moved (a ramp, a queue
    /// filling, has its halves apart).
    pub settled: bool,
}

/// Fills a learned latency is taken over.
pub const LATENCY_SAMPLES: usize = 64;

/// Learns the latency from one sample per fill (frames handed over less the DAC's frame
/// then). Fixed storage: usable on a real-time thread.
#[derive(Debug, Clone)]
pub struct LatencyTracker {
    rate: u32,
    v: [u32; LATENCY_SAMPLES],
    n: usize,
}

impl LatencyTracker {
    pub fn new(rate: u32) -> LatencyTracker {
        LatencyTracker {
            rate: rate.max(1),
            v: [0; LATENCY_SAMPLES],
            n: 0,
        }
    }

    /// A start: nothing known.
    pub fn restart(&mut self) {
        self.n = 0;
    }

    /// One fill's sample: the latency as it now stands.
    pub fn push(&mut self, ahead: u64) -> Latency {
        self.v[self.n % LATENCY_SAMPLES] = ahead.min(u64::from(LATENCY_MAX)) as u32;
        self.n += 1;
        let k = self.n.min(LATENCY_SAMPLES);
        // In the order they came, oldest first.
        let mut all = [0u32; LATENCY_SAMPLES];
        for (i, slot) in all.iter_mut().take(k).enumerate() {
            *slot = self.v[(self.n - k + i) % LATENCY_SAMPLES];
        }
        // (lower quartile, median, upper quartile) of a sorted copy.
        let quartiles = |s: &mut [u32]| {
            s.sort_unstable();
            (s[s.len() / 4], s[s.len() / 2], s[s.len() * 3 / 4])
        };
        let most = all[..k].iter().copied().max().unwrap_or(0);
        let settled = k == LATENCY_SAMPLES && {
            let (old, new) = all.split_at_mut(LATENCY_SAMPLES / 2);
            let ((lo_a, _, hi_a), (lo_b, _, hi_b)) = (quartiles(old), quartiles(new));
            lo_a <= hi_b && lo_b <= hi_a
        };
        let (lo, frames, hi) = quartiles(&mut all[..k]);
        Latency {
            frames,
            most,
            band: (hi - lo).max(self.rate / 1_000).min(LATENCY_BAND_MAX),
            settled,
        }
    }
}

/// The largest figures one word holds: about 43 s and 21 s at 48 kHz.
const LATENCY_MAX: u32 = (1 << 21) - 1;
const LATENCY_BAND_MAX: u32 = (1 << 20) - 1;
const LATENCY_KNOWN: u64 = 1 << 63;
const LATENCY_SETTLED: u64 = 1 << 62;

/// The newest learned latency, written by the output and read by anyone, lock-free (one
/// atomic word).
#[derive(Debug, Default)]
pub struct LatencyCell(AtomicU64);

impl LatencyCell {
    pub fn publish(&self, l: Latency) {
        let w = LATENCY_KNOWN
            | if l.settled { LATENCY_SETTLED } else { 0 }
            | (u64::from(l.band.min(LATENCY_BAND_MAX)) << 42)
            | (u64::from(l.most.min(LATENCY_MAX)) << 21)
            | u64::from(l.frames.min(LATENCY_MAX));
        self.0.store(w, Ordering::Release);
    }

    /// Nothing learned (a new start).
    pub fn clear(&self) {
        self.0.store(0, Ordering::Release);
    }

    pub fn read(&self) -> Option<Latency> {
        let w = self.0.load(Ordering::Acquire);
        (w & LATENCY_KNOWN != 0).then(|| Latency {
            frames: (w & u64::from(LATENCY_MAX)) as u32,
            most: ((w >> 21) & u64::from(LATENCY_MAX)) as u32,
            band: ((w >> 42) & u64::from(LATENCY_BAND_MAX)) as u32,
            settled: w & LATENCY_SETTLED != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_latency_is_learned_steady_from_its_fills_and_published_whole() {
        let mut t = LatencyTracker::new(48_000);
        // A ramp (a queue filling up): not steady.
        let mut l = Latency::default();
        for k in 0..64u64 {
            l = t.push(500 + 40 * k);
        }
        assert!(!l.settled, "{l:?}");
        // Then steady, a fill's sawtooth and a driver's hiccup now and then: settled once a
        // whole window of it is in, the usual value the median, the most the largest.
        for k in 0..64u64 {
            l = t.push(if k % 16 == 5 {
                1_100
            } else {
                4_850 + k % 3 * 20
            });
        }
        assert_eq!(
            l,
            Latency {
                frames: 4_870,
                most: 4_890,
                band: 48,
                settled: true
            }
        );
        // A queue whose depth wobbles by a buffer with uneven events: steady, its band the
        // wobble.
        for k in 0..64u64 {
            l = t.push(5_400 - (k * 7 % 5) * 120);
        }
        assert!(l.settled, "{l:?}");
        assert_eq!((l.most, l.band), (5_400, 240));
        t.restart();
        assert!(!t.push(10).settled);
        let c = LatencyCell::default();
        assert_eq!(c.read(), None);
        c.publish(l);
        assert_eq!(c.read(), Some(l));
        c.clear();
        assert_eq!(c.read(), None);
    }

    #[test]
    fn a_reader_never_sees_half_a_pair() {
        let p = Arc::new(Presentation::default());
        assert_eq!(p.read(), None);
        let w = p.clone();
        let writer = std::thread::spawn(move || {
            for k in 1..200_000u64 {
                w.publish(PresentationPair {
                    frame: k,
                    time_ns: k * 3,
                });
            }
        });
        let mut last = 0;
        while !writer.is_finished() {
            if let Some(r) = p.read() {
                assert_eq!(r.time_ns, r.frame * 3, "one pair, never two halves");
                assert!(r.frame >= last);
                last = r.frame;
            }
        }
        writer.join().unwrap();
        p.clear();
        assert_eq!(p.read(), None);
    }
}
