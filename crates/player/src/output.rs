//! Where the sound goes: a sound device (WASAPI exclusive or shared on Windows), or memory
//! (for checks). Samples arrive left-justified `i32`; each device format gets them packed its
//! way. Integer formats at least as deep as the source carry the exact values.

use crate::decode::SourceFormat;
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    Int,
    Float,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceFormat {
    pub rate: u32,
    pub channels: u16,
    /// Bytes per sample × 8: 16, 24 or 32.
    pub container_bits: u16,
    /// Bits that carry the signal (24 in a 32-bit container…).
    pub valid_bits: u16,
    pub kind: SampleKind,
}

impl DeviceFormat {
    pub fn bytes_per_frame(&self) -> usize {
        usize::from(self.container_bits / 8) * usize::from(self.channels)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Straight to the device, at the song's own format; nothing else can play meanwhile.
    Exclusive,
    /// Through Windows' mixer, which converts to its own rate.
    Shared,
    /// Into memory, for checks.
    Memory,
}

/// A place to send sound.
pub trait Device {
    fn name(&self) -> String;
    fn mode(&self) -> Mode;
    /// Get ready for songs of this format. Returns the format the device will run at.
    fn open(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String>;
    /// Frames the device can take right now, without waiting.
    fn available(&mut self) -> Result<usize, String>;
    /// Wait until the device wants more, then say how many frames.
    fn wait(&mut self) -> Result<usize, String>;
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<(), String>;
    /// Frames the device holds before you hear them (its buffer): the output latency.
    fn latency_frames(&self) -> usize {
        0
    }
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self) -> Result<(), String>;
    /// Let go of the device (other apps can use it).
    fn close(&mut self);
}

/// Pack left-justified samples for the device, applying `gain` when it isn't 1 (which makes
/// the output no longer bit-perfect).
pub fn encode(samples: &[i32], fmt: &DeviceFormat, gain: f32, out: &mut Vec<u8>) {
    out.clear();
    let scaled = |s: i32| -> i32 {
        if gain >= 0.999_999 {
            s
        } else {
            (f64::from(s) * f64::from(gain))
                .round()
                .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
        }
    };
    match (fmt.kind, fmt.container_bits) {
        (SampleKind::Float, _) => {
            out.reserve(samples.len() * 4);
            for &s in samples {
                let v = s as f32 / 2_147_483_648.0 * gain.min(1.0);
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        (SampleKind::Int, 16) => {
            out.reserve(samples.len() * 2);
            for &s in samples {
                out.extend_from_slice(&((scaled(s) >> 16) as i16).to_le_bytes());
            }
        }
        (SampleKind::Int, 24) => {
            out.reserve(samples.len() * 3);
            for &s in samples {
                out.extend_from_slice(&(scaled(s) >> 8).to_le_bytes()[..3]);
            }
        }
        (SampleKind::Int, _) => {
            out.reserve(samples.len() * 4);
            // A 32-bit container with fewer valid bits keeps the value left-justified, as
            // the source already is: the low bits are zero for a 16- or 24-bit song.
            let mask = if fmt.valid_bits >= 32 {
                -1i32
            } else {
                !((1i32 << (32 - fmt.valid_bits)) - 1)
            };
            for &s in samples {
                out.extend_from_slice(&(scaled(s) & mask).to_le_bytes());
            }
        }
    }
}

/// Fit `samples` (interleaved, `from` channels) to `to` channels: mono doubles to stereo;
/// extra channels are dropped; missing ones are silent.
pub fn remap(samples: &[i32], from: u16, to: u16, out: &mut Vec<i32>) {
    out.clear();
    let (f, t) = (usize::from(from.max(1)), usize::from(to.max(1)));
    if f == t {
        out.extend_from_slice(samples);
        return;
    }
    for frame in samples.chunks_exact(f) {
        for c in 0..t {
            out.push(if f == 1 {
                frame[0]
            } else {
                frame.get(c).copied().unwrap_or(0)
            });
        }
    }
}

/// Nothing changes the samples between the file and the device.
pub fn bit_perfect(src: &SourceFormat, dev: &DeviceFormat, mode: Mode, gain: f32) -> bool {
    src.lossless
        && mode != Mode::Shared
        && dev.kind == SampleKind::Int
        && dev.valid_bits >= src.bits
        && dev.rate == src.rate
        && dev.channels == src.channels
        && gain >= 0.999_999
}

// --- Memory -------------------------------------------------------------------------------

/// What a memory device received: each format it was opened with, and every byte written.
#[derive(Debug, Default)]
pub struct Captured {
    pub formats: Vec<DeviceFormat>,
    pub bytes: Vec<u8>,
    /// Frames written while each format was open (same order as `formats`).
    pub frames: Vec<usize>,
    /// A paced device: when its first frame played (frame k plays k/rate after it).
    pub started_at: Option<std::time::Instant>,
}

pub struct MemoryDevice {
    pub captured: Arc<Mutex<Captured>>,
    /// Frames it takes per wait (like a device period).
    pub period: usize,
    /// Taking frames no faster than real time, holding a device's worth of buffer.
    pub paced: Option<Pace>,
}

/// A memory device's clock: when it opened and how many frames it has taken, so a wait lasts
/// until that many frames would have played (keeping `BUFFER_FRAMES` ahead, like a device).
#[derive(Debug, Default)]
pub struct Pace {
    since: Option<std::time::Instant>,
    taken: u64,
    rate: u32,
}

impl Pace {
    /// How far ahead of what's been played the device holds, in frames.
    pub const BUFFER_FRAMES: u64 = 960;
}

impl Device for MemoryDevice {
    fn name(&self) -> String {
        "Memory".into()
    }
    fn mode(&self) -> Mode {
        Mode::Memory
    }
    fn open(&mut self, src: &SourceFormat) -> Result<DeviceFormat, String> {
        // Like an ideal DAC: takes every song at its own rate and depth.
        let f = DeviceFormat {
            rate: src.rate,
            channels: src.channels,
            container_bits: 32,
            valid_bits: src.bits.min(32),
            kind: SampleKind::Int,
        };
        if let Ok(mut c) = self.captured.lock() {
            c.formats.push(f.clone());
            c.frames.push(0);
        }
        if let Some(p) = &mut self.paced {
            *p = Pace {
                since: None,
                taken: 0,
                rate: f.rate,
            };
        }
        Ok(f)
    }
    fn available(&mut self) -> Result<usize, String> {
        Ok(self.period)
    }
    fn wait(&mut self) -> Result<usize, String> {
        if let Some(p) = &mut self.paced {
            let since = *p.since.get_or_insert_with(std::time::Instant::now);
            // Until there's room for another period after what has played.
            let played = (since.elapsed().as_secs_f64() * f64::from(p.rate.max(1))) as u64;
            let ahead = p.taken.saturating_sub(played);
            if ahead + self.period as u64 > Pace::BUFFER_FRAMES {
                let over = ahead + self.period as u64 - Pace::BUFFER_FRAMES;
                std::thread::sleep(std::time::Duration::from_secs_f64(
                    over as f64 / f64::from(p.rate.max(1)),
                ));
            }
        }
        Ok(self.period)
    }
    fn latency_frames(&self) -> usize {
        if self.paced.is_some() {
            Pace::BUFFER_FRAMES as usize
        } else {
            0
        }
    }
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<(), String> {
        if let Some(p) = &mut self.paced {
            let since = *p.since.get_or_insert_with(std::time::Instant::now);
            p.taken += frames as u64;
            if let Ok(mut c) = self.captured.lock() {
                c.started_at.get_or_insert(since);
            }
        }
        if let Ok(mut c) = self.captured.lock() {
            c.bytes.extend_from_slice(bytes);
            if let Some(f) = c.frames.last_mut() {
                *f += frames;
            }
        }
        Ok(())
    }
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(container: u16, valid: u16, kind: SampleKind) -> DeviceFormat {
        DeviceFormat {
            rate: 96_000,
            channels: 2,
            container_bits: container,
            valid_bits: valid,
            kind,
        }
    }

    #[test]
    fn integer_formats_carry_the_exact_values() {
        // 24-bit samples, left-justified.
        let src: Vec<i32> = [0x7F_FFFF, -0x80_0000, 0x12_3456, -1, 0]
            .iter()
            .map(|v| v << 8)
            .collect();
        let mut out = Vec::new();
        encode(&src, &fmt(24, 24, SampleKind::Int), 1.0, &mut out);
        let back: Vec<i32> = out
            .as_chunks::<3>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes([0, b[0], b[1], b[2]]))
            .collect();
        assert_eq!(back, src, "packed 24-bit");
        encode(&src, &fmt(32, 24, SampleKind::Int), 1.0, &mut out);
        let back: Vec<i32> = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(back, src, "24 in a 32-bit container");
        let s16: Vec<i32> = [32767, -32768, 1234].iter().map(|v| v << 16).collect();
        encode(&s16, &fmt(16, 16, SampleKind::Int), 1.0, &mut out);
        assert_eq!(out, [0xFF, 0x7F, 0x00, 0x80, 0xD2, 0x04]);
    }

    #[test]
    fn mono_becomes_stereo() {
        let mut out = Vec::new();
        remap(&[1, 2, 3], 1, 2, &mut out);
        assert_eq!(out, [1, 1, 2, 2, 3, 3]);
    }
}
