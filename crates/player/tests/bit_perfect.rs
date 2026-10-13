//! Real FLAC files, played into memory: the samples that come out must match the MD5 the
//! encoder stored in the file (STREAMINFO), which proves nothing changed them on the way.
//! Files come from VENUTA_TEST_FLAC (paths separated by `|`); run with
//! `cargo test -p player --test bit_perfect -- --ignored`.

use md5::{Digest, Md5};
use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// STREAMINFO: rate, channels, bits, total frames, MD5 of the unencoded samples.
fn streaminfo(path: &Path) -> (u32, u16, u16, u64, [u8; 16]) {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"fLaC");
    let si = &b[8..42];
    let rate = (u32::from(si[10]) << 12) | (u32::from(si[11]) << 4) | (u32::from(si[12]) >> 4);
    let channels = u16::from((si[12] >> 1) & 0x7) + 1;
    let bits = ((u16::from(si[12] & 1) << 4) | u16::from(si[13] >> 4)) + 1;
    let frames = (u64::from(si[13] & 0xF) << 32)
        | u64::from(u32::from_be_bytes([si[14], si[15], si[16], si[17]]));
    let mut md5 = [0u8; 16];
    md5.copy_from_slice(&si[18..34]);
    (rate, channels, bits, frames, md5)
}

#[test]
#[ignore = "needs real FLAC files (VENUTA_TEST_FLAC)"]
fn real_flac_files_come_out_bit_perfect() {
    let list = std::env::var("VENUTA_TEST_FLAC").expect("set VENUTA_TEST_FLAC");
    for file in list.split('|').filter(|s| !s.is_empty()) {
        let path = PathBuf::from(file);
        let (rate, channels, bits, frames, want) = streaminfo(&path);
        let captured = Arc::new(Mutex::new(Captured::default()));
        let (tx, rx) = channel::<Status<PathBuf>>();
        let player = Player::spawn(OutputChoice::Memory(captured.clone()), move |s| {
            let _ = tx.send(s);
        });
        player.send(Cmd::Set {
            items: vec![path.clone()],
            index: 0,
            start_ms: 0,
            play: true,
            from: String::new(),
        });
        let mut was = false;
        loop {
            let s = rx.recv_timeout(Duration::from_secs(60)).expect("status");
            assert!(s.error.is_empty(), "{}", s.error);
            was |= s.playing;
            if was && !s.playing {
                break;
            }
        }
        drop(player);
        let c = captured.lock().unwrap();
        assert_eq!(c.formats.len(), 1);
        assert_eq!(
            (
                c.formats[0].rate,
                c.formats[0].channels,
                c.formats[0].valid_bits
            ),
            (rate, channels, bits)
        );
        // Back to the source's own sample size, little-endian, as the MD5 was computed.
        let n = frames as usize * usize::from(channels);
        let mut h = Md5::new();
        let per = usize::from(bits.div_ceil(8));
        for b in c.bytes.as_chunks::<4>().0.iter().take(n) {
            let v = i32::from_le_bytes([b[0], b[1], b[2], b[3]]) >> (32 - bits);
            h.update(&v.to_le_bytes()[..per]);
        }
        let got: [u8; 16] = h.finalize().into();
        assert_eq!(
            got,
            want,
            "{} ({bits}/{rate}) is not bit-perfect",
            path.display()
        );
        println!(
            "bit-perfect: {} ({bits}-bit {rate} Hz, {frames} frames)",
            path.display()
        );
    }
}
