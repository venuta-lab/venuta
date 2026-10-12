//! Is this the same audio? Decode two files and compare them sample by sample.
//!
//! An audio checksum only proves sameness when it matches. Two files of the same master can
//! still hash differently: one is 24-bit with padded zero bits, one stored no checksum, or a
//! handful of samples differ at a glitch. Decoding both (samples left-justified, so bit depth
//! is evened out) and comparing settles it, and says how close they are when they differ.

use crate::quality::FlacStream;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Every decoded sample is equal.
    Identical,
    /// The same length, and all but a tiny fraction of samples equal: the same master with a
    /// glitch, a different dither of the last bit, or an edited fade.
    Nearly,
    /// The same master, but many samples differ by a step or two in the last bit: the same
    /// recording rounded or dithered differently on the way to FLAC. Inaudible.
    LastBit,
    /// Different audio (or a different length or channel count).
    Different,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    pub verdict: Verdict,
    /// Samples compared (per channel × channels).
    pub samples: u64,
    pub differing: u64,
    /// Largest difference between two samples, in 16-bit steps.
    pub max_delta: u64,
}

impl Comparison {
    /// The same recording in practice: identical, or nearly.
    pub fn same(&self) -> bool {
        matches!(
            self.verdict,
            Verdict::Identical | Verdict::Nearly | Verdict::LastBit
        )
    }

    /// A plain sentence for the evidence panel.
    pub fn describe(&self) -> String {
        match self.verdict {
            Verdict::Identical => "decoded sound identical, sample for sample".into(),
            Verdict::Nearly => format!(
                "decoded sound equal except {} of {} samples",
                self.differing, self.samples
            ),
            Verdict::LastBit => format!(
                "decoded sound equal except for rounding in the last bit (at most {} step), which is inaudible",
                self.max_delta
            ),
            Verdict::Different => "different sound".into(),
        }
    }
}

/// At most this share of samples may differ for "nearly" (1 in 100,000).
const NEARLY_RATIO: f64 = 1e-5;

/// Differences no larger than this many 16-bit steps are rounding, not a different master.
const LAST_BIT_STEPS: u64 = 2;

/// Compare two FLAC files' decoded audio. Stops early once they are clearly different.
pub fn compare_files(a: &Path, b: &Path) -> Result<Comparison, String> {
    let mut sa = FlacStream::open(a, false)?;
    let mut sb = FlacStream::open(b, false)?;
    let different = |samples| Comparison {
        verdict: Verdict::Different,
        samples,
        differing: samples,
        max_delta: u64::MAX,
    };
    if sa.channels != sb.channels || sa.rate != sb.rate {
        return Ok(different(0));
    }
    if let (Some(fa), Some(fb)) = (sa.frames, sb.frames)
        && fa != fb
    {
        return Ok(different(0));
    }
    let limit = sa
        .frames
        .map(|f| ((f * sa.channels as u64) as f64 * NEARLY_RATIO).ceil() as u64)
        .unwrap_or(0);
    let (mut ba, mut bb) = (Vec::new(), Vec::new());
    let (mut ia, mut ib) = (0usize, 0usize);
    let (mut samples, mut differing, mut max_delta) = (0u64, 0u64, 0u64);
    loop {
        if ia == ba.len() {
            ia = 0;
            if !sa.next(&mut ba)? {
                ba.clear();
            }
        }
        if ib == bb.len() {
            ib = 0;
            if !sb.next(&mut bb)? {
                bb.clear();
            }
        }
        match (ba.is_empty(), bb.is_empty()) {
            (true, true) => break,
            (true, false) | (false, true) => return Ok(different(samples)),
            _ => {}
        }
        let n = (ba.len() - ia).min(bb.len() - ib);
        for (x, y) in ba[ia..ia + n].iter().zip(&bb[ib..ib + n]) {
            if x != y {
                differing += 1;
                // Samples are left-justified in 32 bits; express the gap in 16-bit steps.
                let d = (i64::from(*x) - i64::from(*y)).unsigned_abs() >> 16;
                max_delta = max_delta.max(d);
            }
        }
        samples += n as u64;
        ia += n;
        ib += n;
        // A step larger than rounding: a different master or a different recording.
        if max_delta > LAST_BIT_STEPS && differing > limit.max(1) {
            return Ok(Comparison {
                verdict: Verdict::Different,
                samples,
                differing,
                max_delta,
            });
        }
    }
    let verdict = if differing == 0 {
        Verdict::Identical
    } else if differing <= limit.max(1) {
        Verdict::Nearly
    } else if max_delta <= LAST_BIT_STEPS {
        Verdict::LastBit
    } else {
        Verdict::Different
    };
    Ok(Comparison {
        verdict,
        samples,
        differing,
        max_delta,
    })
}

/// Saved comparisons whose files haven't changed since, by (smaller id, larger id).
pub fn load(
    index: &crate::Index,
) -> crate::Result<std::collections::HashMap<(i64, i64), Comparison>> {
    let mut stmt = index.conn().prepare(
        "SELECT p.a, p.b, p.verdict, p.samples, p.differing, p.max_delta
         FROM audio_pairs p
         JOIN files fa ON fa.id = p.a AND fa.mtime_ns = p.a_mtime
         JOIN files fb ON fb.id = p.b AND fb.mtime_ns = p.b_mtime",
    )?;
    let rows = stmt.query_map([], |r| {
        let verdict = match r.get::<_, String>(2)?.as_str() {
            "identical" => Verdict::Identical,
            "nearly" => Verdict::Nearly,
            "last_bit" => Verdict::LastBit,
            _ => Verdict::Different,
        };
        Ok((
            (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?),
            Comparison {
                verdict,
                samples: r.get::<_, i64>(3)? as u64,
                differing: r.get::<_, i64>(4)? as u64,
                max_delta: r.get::<_, i64>(5)? as u64,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The key a pair is stored under.
pub fn key(a: i64, b: i64) -> (i64, i64) {
    (a.min(b), a.max(b))
}

/// Compare the given pairs of tracks (by file id) and save the results. Runs in parallel;
/// pairs whose files can't be read are skipped and tried again next time.
pub fn run(index: &crate::Index, root: &Path, pairs: &[(i64, i64)]) -> crate::Result<usize> {
    use rayon::prelude::*;
    let files: std::collections::HashMap<i64, (String, i64)> = {
        let mut stmt = index
            .conn()
            .prepare("SELECT id, path, mtime_ns FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get(1)?, r.get(2)?))))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let results: Vec<((i64, i64), i64, i64, Comparison)> = pairs
        .par_iter()
        .filter_map(|&(a, b)| {
            let (a, b) = key(a, b);
            let (pa, ma) = files.get(&a)?;
            let (pb, mb) = files.get(&b)?;
            let c = compare_files(&root.join(pa), &root.join(pb)).ok()?;
            Some(((a, b), *ma, *mb, c))
        })
        .collect();
    let mut stmt = index.conn().prepare(
        "INSERT OR REPLACE INTO audio_pairs (a, b, a_mtime, b_mtime, verdict, samples, differing, max_delta)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for ((a, b), ma, mb, c) in &results {
        let verdict = match c.verdict {
            Verdict::Identical => "identical",
            Verdict::Nearly => "nearly",
            Verdict::LastBit => "last_bit",
            Verdict::Different => "different",
        };
        stmt.execute(rusqlite::params![
            a,
            b,
            ma,
            mb,
            verdict,
            c.samples as i64,
            c.differing.min(i64::MAX as u64) as i64,
            c.max_delta.min(i64::MAX as u64) as i64
        ])?;
    }
    Ok(results.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A FLAC file of `samples` from a tiny PCM WAV, via the `flac` tool if it is installed.
    fn flac_from(dir: &Path, name: &str, pcm: &[i16]) -> Option<std::path::PathBuf> {
        let wav = dir.join(format!("{name}.wav"));
        let mut bytes = Vec::new();
        let data_len = (pcm.len() * 2) as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&(44_100u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for s in pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(&wav, bytes).ok()?;
        let out = dir.join(format!("{name}.flac"));
        let ok = Command::new("ffmpeg")
            .args(["-v", "quiet", "-y", "-i"])
            .arg(&wav)
            .arg(&out)
            .status()
            .ok()?
            .success();
        ok.then_some(out)
    }

    #[test]
    fn identical_nearly_and_different() {
        let dir = tempfile::tempdir().unwrap();
        let tone: Vec<i16> = (0..200_000)
            .map(|i| ((i as f32 * 0.05).sin() * 8000.0) as i16)
            .collect();
        let mut glitch = tone.clone();
        glitch[1000] += 3;
        let other: Vec<i16> = (0..200_000)
            .map(|i| ((i as f32 * 0.07).sin() * 8000.0) as i16)
            .collect();
        let (Some(a), Some(b), Some(c), Some(d)) = (
            flac_from(dir.path(), "a", &tone),
            flac_from(dir.path(), "b", &tone),
            flac_from(dir.path(), "c", &glitch),
            flac_from(dir.path(), "d", &other),
        ) else {
            eprintln!("ffmpeg not available; skipping");
            return;
        };
        assert_eq!(compare_files(&a, &b).unwrap().verdict, Verdict::Identical);
        let near = compare_files(&a, &c).unwrap();
        assert_eq!(near.verdict, Verdict::Nearly);
        assert_eq!(near.differing, 1);
        assert_eq!(compare_files(&a, &d).unwrap().verdict, Verdict::Different);
    }
}
