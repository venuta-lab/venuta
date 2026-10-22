//! How one person's copy of a song lines up with the room's reference copy (contract 5.8 of the
//! Listen together design).
//!
//! The host makes an envelope of its reference copy and sends it (the `env:` stream); each
//! guest makes one of its own copy and correlates the two. This file declares the shapes every
//! package builds against; making envelopes and aligning them is the alignment package's
//! (`player::analysis`: `envelope(decoded, rate, ch) -> Envelope` and
//! `align(reference, mine) -> Alignment`).

use serde::{Deserialize, Serialize};

/// A song's onset envelope: one byte per hop (onset strength from the rise of log energy in
/// four bands), in windows (the first 30 s, then 10 s every 60 s), and the hash of the first
/// 10 s of decoded PCM, which tells an exact copy at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub hop_ms: u8,
    /// (where the window starts in the song, in ms; one byte per hop)
    pub windows: Vec<(u32, Vec<u8>)>,
    pub pcm_hash: Option<[u8; 16]>,
}

/// Where my copy is against the reference: `my content time = room content time + offset_us`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    pub offset_us: i64,
    pub class: CopyClass,
    /// How sure the measure is (the correlation peak over the noise, in standard deviations);
    /// 0 for an exact or approximate copy.
    pub confidence: f32,
}

impl Alignment {
    /// The same bytes as the reference (the host's own file): offset 0, nothing to measure.
    pub const EXACT: Alignment = Alignment {
        offset_us: 0,
        class: CopyClass::Exact,
        confidence: 0.0,
    };
    /// Nothing known (no reference envelope, or no confident peak): offset 0, best effort.
    pub const APPROXIMATE: Alignment = Alignment {
        offset_us: 0,
        class: CopyClass::Approximate,
        confidence: 0.0,
    };
}

/// How well a copy is known to match the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyClass {
    /// The host's own bytes, or a lossless copy whose decoded PCM hashes the same.
    Exact,
    /// Correlated with enough confidence; refined through the song.
    Measured,
    /// No reference, low confidence, or another edit.
    Approximate,
}

/// What one later stretch of envelope says about the offset known before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    /// The offset holds here, measured again (within 20 ms of it).
    Agrees { offset_us: i64 },
    /// A clear match at another offset, over 20 ms away: another edit.
    Moved { offset_us: i64 },
    /// Both copies have music here but not the same music at the offset, and no clear match
    /// within reach: an edit longer than the search, or another version.
    Lost,
    /// Nothing to say: quiet, or too little in common to tell.
    Unknown,
}

/// One later window's check of the offset.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowCheck {
    /// Where the window starts in the reference (the room's content time), ms.
    pub start_ms: u32,
    pub verdict: Verdict,
}

/// A whole comparison of two copies: the alignment, what each later window said (in song
/// order), and where an edit first showed (the copy is approximate from there).
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub alignment: Alignment,
    pub windows: Vec<WindowCheck>,
    pub edit_at_ms: Option<u32>,
}

/// The host's envelope of an entry's reference copy, as the `env:` stream carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// The host has no copy of this song: there is nothing to measure against (everyone is
    /// approximate for it).
    NoCopy,
    /// The first window and the PCM hash: what the host has made so far.
    Start(Envelope),
    /// Every window of the song.
    Whole(Envelope),
}

impl Reference {
    pub fn envelope(&self) -> Option<&Envelope> {
        match self {
            Reference::NoCopy => None,
            Reference::Start(e) | Reference::Whole(e) => Some(e),
        }
    }
}

/// The most an `env:` body may be on the wire (deflated), bytes.
pub const MAX_ENV_BYTES: usize = 256 * 1024;
/// The most it may be once inflated, bytes (an hour's song is about 63 KB).
const MAX_ENV_RAW: usize = 1024 * 1024;
/// The most windows an envelope may have (8 hours) and the most hops in one (the first
/// window's 30 s at a 1 ms hop).
const MAX_WINDOWS: usize = 480;
const MAX_HOPS: usize = 30_000;
const MAGIC: &[u8; 4] = b"VENV";
const FORM: u8 = 1;

/// The `env:` body of `r`: a small binary form (`VENV`, its form, the kind, the hop, the hash,
/// then each window's start, length and bytes, little-endian), deflated.
pub fn encode_reference(r: &Reference) -> Vec<u8> {
    use std::io::Write;
    let mut raw = Vec::new();
    raw.extend_from_slice(MAGIC);
    raw.push(FORM);
    let (kind, env) = match r {
        Reference::NoCopy => (0u8, None),
        Reference::Start(e) => (1, Some(e)),
        Reference::Whole(e) => (2, Some(e)),
    };
    raw.push(kind);
    if let Some(e) = env {
        raw.push(e.hop_ms);
        match &e.pcm_hash {
            Some(h) => {
                raw.push(1);
                raw.extend_from_slice(h);
            }
            None => raw.push(0),
        }
        let windows = &e.windows[..e.windows.len().min(MAX_WINDOWS)];
        raw.extend_from_slice(&(windows.len() as u16).to_le_bytes());
        for (start, w) in windows {
            let w = &w[..w.len().min(MAX_HOPS)];
            raw.extend_from_slice(&start.to_le_bytes());
            raw.extend_from_slice(&(w.len() as u32).to_le_bytes());
            raw.extend_from_slice(w);
        }
    }
    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).expect("deflate into memory");
    z.finish().expect("deflate into memory")
}

/// Read an `env:` body, checked: a body over the bounds or of another form is refused.
pub fn decode_reference(body: &[u8]) -> Result<Reference, String> {
    use std::io::Read;
    if body.len() > MAX_ENV_BYTES {
        return Err("an envelope that's too big".into());
    }
    let mut raw = Vec::new();
    flate2::read::DeflateDecoder::new(body)
        .take(MAX_ENV_RAW as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "an envelope that isn't one".to_owned())?;
    if raw.len() > MAX_ENV_RAW {
        return Err("an envelope that's too big".into());
    }
    let bad = || "an envelope that isn't one".to_owned();
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = raw.get(at..at + n).ok_or_else(bad)?;
        at += n;
        Ok(s)
    };
    if take(4)? != MAGIC || take(1)?[0] != FORM {
        return Err(bad());
    }
    let kind = take(1)?[0];
    if kind == 0 {
        return Ok(Reference::NoCopy);
    }
    let hop_ms = take(1)?[0];
    if hop_ms == 0 {
        return Err(bad());
    }
    let pcm_hash = match take(1)?[0] {
        0 => None,
        1 => Some(<[u8; 16]>::try_from(take(16)?).map_err(|_| bad())?),
        _ => return Err(bad()),
    };
    let n = u16::from_le_bytes(take(2)?.try_into().map_err(|_| bad())?) as usize;
    if n > MAX_WINDOWS {
        return Err(bad());
    }
    let mut windows = Vec::with_capacity(n);
    for _ in 0..n {
        let start = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
        let len = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?) as usize;
        if len > MAX_HOPS {
            return Err(bad());
        }
        windows.push((start, take(len)?.to_vec()));
    }
    if at != raw.len() {
        return Err(bad());
    }
    let e = Envelope {
        hop_ms,
        windows,
        pcm_hash,
    };
    match kind {
        1 => Ok(Reference::Start(e)),
        2 => Ok(Reference::Whole(e)),
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Envelope {
        Envelope {
            hop_ms: 10,
            windows: vec![
                (0, (0..3000u32).map(|i| (i * 7 % 251) as u8).collect()),
                (60_000, vec![3; 1000]),
            ],
            pcm_hash: Some([9; 16]),
        }
    }

    #[test]
    fn a_reference_goes_and_comes_back_the_same_and_small() {
        for r in [
            Reference::NoCopy,
            Reference::Start(env()),
            Reference::Whole(env()),
            Reference::Whole(Envelope {
                pcm_hash: None,
                ..env()
            }),
        ] {
            let b = encode_reference(&r);
            assert!(b.len() < 8 * 1024, "{} bytes", b.len());
            assert_eq!(decode_reference(&b), Ok(r));
        }
    }

    #[test]
    fn a_body_that_isnt_a_reference_is_refused() {
        assert!(decode_reference(b"not deflate at all").is_err());
        let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut z, b"VENV\x01\x02\x0a\x00\x01\x00\x00\x00\x00\x00\xff\xff")
            .unwrap();
        let cut = z.finish().unwrap();
        assert!(decode_reference(&cut).is_err(), "a window longer than its bytes");
        let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut z, &vec![0u8; MAX_ENV_RAW + 10]).unwrap();
        assert!(decode_reference(&z.finish().unwrap()).is_err(), "too big inflated");
        assert!(decode_reference(&vec![0u8; MAX_ENV_BYTES + 1]).is_err());
    }
}
