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

#[cfg(test)]
mod tests {
    use super::*;

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
