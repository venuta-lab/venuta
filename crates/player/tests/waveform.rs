//! A song's shape for the seek bar, from a file and from a stream.

use media_access::EphemeralAudio;
use player::waveform::{waveform, waveform_remote};
use std::io::{Read, Write};

/// A 16-bit mono WAV: the first half quiet, the second loud.
fn quiet_then_loud(path: &std::path::Path) {
    std::fs::write(path, quiet_then_loud_bytes()).unwrap();
}

fn quiet_then_loud_bytes() -> Vec<u8> {
    let rate = 8000u32;
    let frames = 16_000usize;
    let data_len = frames * 2;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        let amp: i16 = if i < frames / 2 { 300 } else { 20_000 };
        let s = if i % 2 == 0 { amp } else { -amp };
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

#[test]
fn a_file_shows_its_quiet_and_loud_parts() {
    let f = std::env::temp_dir().join(format!("venuta-wave-{}.wav", std::process::id()));
    quiet_then_loud(&f);
    let w = waveform(&f, 64).unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(w.len(), 64);
    let first = w[..32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    let second = w[32..].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    assert!(
        second > first + 60,
        "loud half {second} over quiet half {first}"
    );
}

#[test]
fn a_stream_shows_its_quiet_and_loud_parts_too() {
    let body = quiet_then_loud_bytes();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/song.wav", listener.local_addr().unwrap());
    let served = body.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK
Content-Type: audio/wav
Content-Length: {}
Connection: close

",
                served.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&served);
        }
    });
    let audio = EphemeralAudio {
        track_id: "t-1".into(),
        provider: "remote_catalog".into(),
        url,
        mime_type: Some("audio/wav".into()),
        content_length: Some(body.len() as u64),
        duration_ms: Some(2_000),
        headers: Vec::new(),
        via: "fixture",
        loudness_db: None,
    };
    let w = waveform_remote(audio, 64).unwrap();
    assert_eq!(w.len(), 64);
    let first = w[..32].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    let second = w[32..].iter().map(|&v| u32::from(v)).sum::<u32>() / 32;
    assert!(
        second > first + 60,
        "loud half {second} over quiet half {first}"
    );
}
