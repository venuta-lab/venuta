//! Proof that playback is bit-perfect: play a FLAC file through the player into memory and
//! compare what comes out with the MD5 the encoder stored in the file (STREAMINFO), which is
//! a fingerprint of the original samples.

use crate::engine::{Cmd, OutputChoice, Player, Status};
use crate::output::Captured;
use md5::{Digest, Md5};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct Verification {
    /// The output matches the file's fingerprint.
    pub bit_perfect: bool,
    pub bits: u16,
    pub rate: u32,
    pub frames: u64,
    pub file_md5: String,
    pub output_md5: String,
}

/// STREAMINFO: rate, channels, bits, total frames, MD5 of the unencoded samples.
fn streaminfo(path: &Path) -> Result<(u32, u16, u16, u64, [u8; 16]), String> {
    use std::io::Read;
    let mut head = [0u8; 42];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| e.to_string())?;
    if &head[..4] != b"fLaC" {
        return Err("Only FLAC files carry a fingerprint of their samples.".into());
    }
    let si = &head[8..42];
    let rate = (u32::from(si[10]) << 12) | (u32::from(si[11]) << 4) | (u32::from(si[12]) >> 4);
    let channels = u16::from((si[12] >> 1) & 0x7) + 1;
    let bits = ((u16::from(si[12] & 1) << 4) | u16::from(si[13] >> 4)) + 1;
    let frames = (u64::from(si[13] & 0xF) << 32)
        | u64::from(u32::from_be_bytes([si[14], si[15], si[16], si[17]]));
    let mut md5 = [0u8; 16];
    md5.copy_from_slice(&si[18..34]);
    if md5 == [0; 16] {
        return Err("This file was encoded without a fingerprint, so it can't be checked.".into());
    }
    Ok((rate, channels, bits, frames, md5))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Play `path` into memory (as fast as it decodes) and compare with its fingerprint.
pub fn verify_bit_perfect(path: &Path) -> Result<Verification, String> {
    let (rate, channels, bits, frames, want) = streaminfo(path)?;
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, rx) = channel::<Status<PathBuf>>();
    let player = Player::spawn(OutputChoice::Memory(captured.clone()), move |s| {
        let _ = tx.send(s);
    });
    player.send(Cmd::Set {
        items: vec![path.to_path_buf()],
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    let mut was = false;
    loop {
        let s = rx
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| "The check took too long.".to_string())?;
        if !s.error.is_empty() {
            return Err(s.error);
        }
        was |= s.playing;
        if was && !s.playing {
            break;
        }
    }
    drop(player);
    let c = captured.lock().map_err(|e| e.to_string())?;
    let n = frames as usize * usize::from(channels);
    let per = usize::from(bits.div_ceil(8));
    let mut h = Md5::new();
    let mut buf = Vec::with_capacity(per * 4096);
    for b in c.bytes.as_chunks::<4>().0.iter().take(n) {
        let v = i32::from_le_bytes(*b) >> (32 - bits);
        buf.extend_from_slice(&v.to_le_bytes()[..per]);
        if buf.len() >= per * 4096 {
            h.update(&buf);
            buf.clear();
        }
    }
    h.update(&buf);
    let got: [u8; 16] = h.finalize().into();
    Ok(Verification {
        bit_perfect: got == want && c.bytes.len() / 4 >= n,
        bits,
        rate,
        frames,
        file_md5: hex(&want),
        output_md5: hex(&got),
    })
}
