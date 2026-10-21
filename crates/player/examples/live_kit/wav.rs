//! WAV files for captures: written as 32-bit float while recording, read back as float
//! (16, 24 and 32-bit integer and 32-bit float files, plain or extensible).

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Interleaved samples, full scale 1.0.
#[derive(Debug, Clone)]
pub struct Wav {
    pub rate: u32,
    pub channels: u16,
    pub data: Vec<f32>,
}

impl Wav {
    pub fn frames(&self) -> usize {
        self.data.len() / usize::from(self.channels.max(1))
    }
    /// One channel.
    pub fn channel(&self, c: u16) -> Vec<f32> {
        let ch = usize::from(self.channels);
        self.data
            .iter()
            .skip(usize::from(c))
            .step_by(ch)
            .copied()
            .collect()
    }
    /// The channels averaged.
    pub fn mono(&self) -> Vec<f64> {
        let ch = usize::from(self.channels);
        self.data
            .chunks_exact(ch)
            .map(|f| f.iter().map(|&v| f64::from(v)).sum::<f64>() / ch as f64)
            .collect()
    }
}

/// A float WAV written as it comes; the sizes are filled in by `finish`.
pub struct WavWriter {
    out: BufWriter<File>,
    channels: u16,
    frames: u64,
}

impl WavWriter {
    pub fn create(path: &Path, rate: u32, channels: u16) -> std::io::Result<Self> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&header(rate, channels, 0))?;
        Ok(Self {
            out,
            channels,
            frames: 0,
        })
    }

    pub fn write(&mut self, interleaved: &[f32]) -> std::io::Result<()> {
        for v in interleaved {
            self.out.write_all(&v.to_le_bytes())?;
        }
        self.frames += (interleaved.len() / usize::from(self.channels.max(1))) as u64;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn finish(mut self, rate: u32) -> std::io::Result<()> {
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| e.into_error())?;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&header(rate, self.channels, self.frames))?;
        f.flush()
    }
}

fn header(rate: u32, channels: u16, frames: u64) -> Vec<u8> {
    let data_len = (frames * 4 * u64::from(channels)).min(u64::from(u32::MAX - 64)) as u32;
    let mut v = Vec::with_capacity(44);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 4 * u32::from(channels)).to_le_bytes());
    v.extend_from_slice(&(4 * channels).to_le_bytes());
    v.extend_from_slice(&32u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v
}

/// Write a whole float WAV at once.
pub fn write(path: &Path, w: &Wav) -> std::io::Result<()> {
    let mut o = WavWriter::create(path, w.rate, w.channels)?;
    o.write(&w.data)?;
    o.finish(w.rate)
}

pub fn read(path: &Path) -> Result<Wav, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(b: &[u8]) -> Result<Wav, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut i = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4) as usize;
        let body = i + 8;
        if id == b"fmt " {
            let mut tag = u16_at(body);
            let channels = u16_at(body + 2);
            let rate = u32_at(body + 4);
            let bits = u16_at(body + 14);
            if tag == 0xFFFE && len >= 40 {
                // Extensible: the sub-format's first two bytes are the plain tag.
                tag = u16_at(body + 24);
            }
            fmt = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = fmt.ok_or("data before fmt")?;
            // A capture cut short has a data length of 0 or too long: take what is there.
            let end = if len == 0 || body + len > b.len() {
                b.len()
            } else {
                body + len
            };
            let d = &b[body..end];
            let data: Vec<f32> = match (tag, bits) {
                (3, 32) => d
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes(*c))
                    .collect(),
                (1, 16) => d
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| f32::from(i16::from_le_bytes(*c)) / 32_768.0)
                    .collect(),
                (1, 24) => d
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
                    .collect(),
                (1, 32) => d
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| i32::from_le_bytes(*c) as f32 / 2_147_483_648.0)
                    .collect(),
                _ => return Err(format!("format {tag} with {bits} bits is not read here")),
            };
            let ch = usize::from(channels.max(1));
            let whole = data.len() / ch * ch;
            let mut data = data;
            data.truncate(whole);
            return Ok(Wav {
                rate,
                channels,
                data,
            });
        }
        i = body + len + (len & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_wav_round_trips() {
        let w = Wav {
            rate: 48_000,
            channels: 2,
            data: (0..200).map(|i| (i as f32 * 0.01).sin()).collect(),
        };
        let p = std::env::temp_dir().join(format!("venuta-kit-wav-{}.wav", std::process::id()));
        write(&p, &w).unwrap();
        let r = read(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(r.rate, 48_000);
        assert_eq!(r.channels, 2);
        assert_eq!(r.data, w.data);
    }
}
