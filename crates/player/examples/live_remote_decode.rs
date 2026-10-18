//! Opt-in live remote decode smoke (requires network + media-access live-provider):
//! `cargo run -p player --example live_remote_decode --features media-access/live-provider -- "blue train"`
//!
//! Never prints stream URLs. Exits 0 only if samples are decoded.

use media_access::{CancellationToken, LiveCatalogAccess, MediaAccess, PageRequest};
use player::decode::{DecodeState, Source};
use std::time::{Duration, Instant};

fn main() {
    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "blue train".into());
    let access = LiveCatalogAccess::new().unwrap_or_else(|e| {
        eprintln!("init failed: {e}");
        std::process::exit(1);
    });
    let cancel = CancellationToken::default();
    let page = access
        .search_tracks(&query, &PageRequest::new(3, None), &cancel)
        .unwrap_or_else(|e| {
            eprintln!("search failed: {e}");
            std::process::exit(2);
        });
    if page.items.is_empty() {
        eprintln!("no tracks");
        std::process::exit(3);
    }
    let track = &page.items[0];
    println!(
        "track={} — {} [{}]",
        track.title,
        track.artists.join(", "),
        track.track_id
    );
    let audio = access
        .resolve_audio(&track.track_id, &cancel)
        .unwrap_or_else(|e| {
            eprintln!("resolve failed: {e}");
            std::process::exit(4);
        });
    let expected_len = audio.content_length;
    println!(
        "resolved mime={:?} content_length={:?} duration_ms={:?} headers={}",
        audio.mime_type,
        expected_len,
        audio.duration_ms,
        audio.headers.len()
    );
    let mut source = Source::open_remote(audio).unwrap_or_else(|e| {
        eprintln!("open_remote failed: {e}");
        std::process::exit(5);
    });

    let started = Instant::now();
    let mut samples = Vec::new();
    let mut total_samples = 0usize;
    let mut ready = false;
    let mut first_audio_ms = None;
    // Prefer enough samples to prove more than a short preview (~30s at 44.1k stereo).
    let want_samples = 44_100usize * 2 * 30;
    loop {
        if started.elapsed() > Duration::from_secs(180) {
            eprintln!("timed out waiting for decoded samples (got {total_samples})");
            std::process::exit(6);
        }
        match source.next(&mut samples) {
            Ok(DecodeState::Pending) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(DecodeState::Samples) => {
                ready = true;
                first_audio_ms.get_or_insert(started.elapsed().as_millis());
                total_samples += samples.len();
                samples.clear();
                if total_samples >= want_samples {
                    break;
                }
            }
            Ok(DecodeState::End) => break,
            Err(e) => {
                eprintln!("decode error: {e}");
                std::process::exit(7);
            }
        }
    }
    println!("first_audio_ms={first_audio_ms:?} (from resolved address to first decoded samples)");
    let secs = total_samples as f64 / (source.format.rate as f64 * source.format.channels as f64);
    println!(
        "decode_ok ready={ready} samples={total_samples} approx_secs={secs:.1} format={:?} frames={:?} expected_len={:?} elapsed_ms={}",
        source.format,
        source.frames,
        expected_len,
        started.elapsed().as_millis()
    );
    if !ready || total_samples == 0 {
        eprintln!("no samples decoded");
        std::process::exit(8);
    }
    // Full-length tracks should expose long frame counts (minutes), not a ~60s window.
    if let Some(frames) = source.frames {
        let duration_secs = frames as f64 / source.format.rate as f64;
        println!("source_duration_secs={duration_secs:.1}");
        if duration_secs < 90.0 {
            eprintln!("source duration looks like a short preview window ({duration_secs:.1}s)");
            std::process::exit(9);
        }
    }
}
