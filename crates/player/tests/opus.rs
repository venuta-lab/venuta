//! Opus: the catalog's better stream (in WebM) and local .opus files (in Ogg) both decode.
//! The fixtures are one second of a 440 Hz tone at 48 kHz, made with ffmpeg; no music.

use player::decode::{DecodeState, Source};
use std::path::Path;

/// Decode a whole file; its format and how many frames came out.
fn decode_all(path: &Path) -> (player::decode::SourceFormat, usize) {
    let mut source = Source::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let format = source.format.clone();
    let channels = usize::from(format.channels);
    let mut out = Vec::new();
    let mut frames = 0;
    loop {
        match source.next(&mut out).expect("decodes") {
            DecodeState::Samples => frames += out.len() / channels,
            DecodeState::End => break,
            DecodeState::Pending => unreachable!("a local file never waits"),
        }
    }
    (format, frames)
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn opus_in_webm_decodes_as_the_catalog_sends_it() {
    let (format, frames) = decode_all(&fixture("tone-opus.webm"));
    assert_eq!(format.codec, "Opus");
    assert_eq!(format.rate, 48_000);
    assert_eq!(format.channels, 2);
    assert!(!format.lossless);
    // One second, give or take the encoder's padding.
    assert!((46_000..=50_000).contains(&frames), "{frames} frames");
}

#[test]
fn a_local_opus_file_plays_too() {
    let (format, frames) = decode_all(&fixture("tone.opus"));
    assert_eq!(format.codec, "Opus");
    assert!((46_000..=50_000).contains(&frames), "{frames} frames");
}

#[test]
fn seeking_into_an_opus_stream_lands_and_plays_on() {
    let mut source = Source::open(&fixture("tone-opus.webm")).unwrap();
    let frame = source.seek(500).expect("seeks");
    assert!(
        (20_000..=28_000).contains(&frame),
        "landed at frame {frame}"
    );
    let mut out = Vec::new();
    let mut frames = 0;
    while let DecodeState::Samples = source.next(&mut out).unwrap() {
        frames += out.len() / 2;
    }
    assert!(
        (20_000..=28_000).contains(&frames),
        "{frames} frames after the seek"
    );
}

/// A local .opus file states its length. (A WebM stream may not; online songs take theirs from
/// the catalog, which is what the player shows and seeks within.)
#[test]
fn a_local_opus_file_knows_its_length() {
    let source = Source::open(&fixture("tone.opus")).unwrap();
    let ms = source.duration_ms().expect("a length");
    assert!((950..=1050).contains(&ms), "{ms} ms");
}
