//! FLAC container layout: where the metadata blocks end and the audio frames begin.
//!
//! This is the basis of the write-safety guarantee. A tag write may only change metadata
//! blocks; the STREAMINFO block and every byte of audio after the last metadata block must be
//! identical before and after. Comparing those bytes proves the audio is untouched without
//! decoding anything.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub const BLOCK_STREAMINFO: u8 = 0;
pub const BLOCK_PADDING: u8 = 1;
pub const BLOCK_VORBIS_COMMENT: u8 = 4;
pub const BLOCK_PICTURE: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    pub kind: u8,
    /// Offset of the block header within the file.
    pub offset: u64,
    /// Length of the block body (excluding the 4-byte header).
    pub len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlacLayout {
    /// Bytes of an ID3v2 tag before `fLaC` (discouraged by spec, but it happens).
    pub id3v2_len: u64,
    pub blocks: Vec<BlockInfo>,
    /// First byte of the first audio frame.
    pub audio_offset: u64,
    pub file_len: u64,
    /// The 34-byte STREAMINFO body (sample rate, channels, MD5…).
    pub streaminfo: Vec<u8>,
}

impl FlacLayout {
    pub fn padding_bytes(&self) -> u64 {
        self.blocks
            .iter()
            .filter(|b| b.kind == BLOCK_PADDING)
            .map(|b| u64::from(b.len))
            .sum()
    }

    pub fn audio_len(&self) -> u64 {
        self.file_len - self.audio_offset
    }
}

pub fn read_layout(path: &Path) -> io::Result<FlacLayout> {
    let mut f = BufReader::new(File::open(path)?);
    let file_len = f.get_ref().metadata()?.len();

    let mut id3v2_len = 0u64;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    if &magic[..3] == b"ID3" {
        // ID3v2 header: "ID3" ver(2) flags(1) size(4, syncsafe)
        let mut rest = [0u8; 6];
        f.read_exact(&mut rest)?;
        let size = rest[2..6]
            .iter()
            .fold(0u64, |acc, b| (acc << 7) | u64::from(b & 0x7F));
        let footer = if rest[1] & 0x10 != 0 { 10 } else { 0 };
        id3v2_len = 10 + size + footer;
        f.seek(SeekFrom::Start(id3v2_len))?;
        f.read_exact(&mut magic)?;
    }
    if &magic != b"fLaC" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a FLAC stream",
        ));
    }

    let mut blocks = Vec::new();
    let mut streaminfo = Vec::new();
    let mut pos = id3v2_len + 4;
    loop {
        let mut hdr = [0u8; 4];
        f.read_exact(&mut hdr)?;
        let last = hdr[0] & 0x80 != 0;
        let kind = hdr[0] & 0x7F;
        let len = u32::from_be_bytes([0, hdr[1], hdr[2], hdr[3]]);
        if pos + 4 + u64::from(len) > file_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "metadata block overruns file",
            ));
        }
        if kind == BLOCK_STREAMINFO {
            streaminfo = vec![0u8; len as usize];
            f.read_exact(&mut streaminfo)?;
        } else {
            f.seek_relative(i64::from(len))?;
        }
        blocks.push(BlockInfo {
            kind,
            offset: pos,
            len,
        });
        pos += 4 + u64::from(len);
        if last {
            break;
        }
    }
    if streaminfo.len() != 34 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing or malformed STREAMINFO",
        ));
    }
    Ok(FlacLayout {
        id3v2_len,
        blocks,
        audio_offset: pos,
        file_len,
        streaminfo,
    })
}

/// blake3 of every byte from the first audio frame to the end of the file.
pub fn audio_hash(path: &Path, layout: &FlacLayout) -> io::Result<[u8; 32]> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(layout.audio_offset))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Fingerprint of the parts of a FLAC file that a tag write must never change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFingerprint {
    pub streaminfo: Vec<u8>,
    pub audio_len: u64,
    pub audio_hash: [u8; 32],
}

pub fn audio_fingerprint(path: &Path) -> io::Result<AudioFingerprint> {
    let layout = read_layout(path)?;
    Ok(AudioFingerprint {
        audio_hash: audio_hash(path, &layout)?,
        audio_len: layout.audio_len(),
        streaminfo: layout.streaminfo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn block(last: bool, kind: u8, body: &[u8]) -> Vec<u8> {
        let len = (body.len() as u32).to_be_bytes();
        let mut v = vec![(u8::from(last) << 7) | kind, len[1], len[2], len[3]];
        v.extend_from_slice(body);
        v
    }

    fn synthetic(padding: usize, audio: &[u8]) -> Vec<u8> {
        let mut v = b"fLaC".to_vec();
        v.extend(block(false, BLOCK_STREAMINFO, &[7u8; 34]));
        v.extend(block(false, BLOCK_VORBIS_COMMENT, b"vendor-and-comments"));
        v.extend(block(true, BLOCK_PADDING, &vec![0u8; padding]));
        v.extend_from_slice(audio);
        v
    }

    #[test]
    fn layout_and_audio_hash() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.flac");
        std::fs::File::create(&p)
            .unwrap()
            .write_all(&synthetic(100, b"AUDIOFRAMES"))
            .unwrap();
        let l = read_layout(&p).unwrap();
        assert_eq!(l.blocks.len(), 3);
        assert_eq!(l.padding_bytes(), 100);
        assert_eq!(l.audio_len(), 11);
        assert_eq!(l.streaminfo, vec![7u8; 34]);

        // Same audio behind different metadata → same fingerprint.
        let q = dir.path().join("b.flac");
        std::fs::File::create(&q)
            .unwrap()
            .write_all(&synthetic(5000, b"AUDIOFRAMES"))
            .unwrap();
        assert_eq!(
            audio_fingerprint(&p).unwrap(),
            audio_fingerprint(&q).unwrap()
        );

        // One changed audio byte → different fingerprint.
        let r = dir.path().join("c.flac");
        std::fs::File::create(&r)
            .unwrap()
            .write_all(&synthetic(100, b"AUDIOFRAMEZ"))
            .unwrap();
        assert_ne!(
            audio_fingerprint(&p).unwrap(),
            audio_fingerprint(&r).unwrap()
        );
    }

    #[test]
    fn rejects_non_flac() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.flac");
        std::fs::write(&p, b"RIFF....WAVE").unwrap();
        assert!(read_layout(&p).is_err());
    }
}
