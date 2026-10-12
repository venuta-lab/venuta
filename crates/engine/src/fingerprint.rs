//! Acoustic fingerprints (Chromaprint, as used by AcoustID and MusicBrainz), computed on
//! this computer. Used to identify files that have no tags; only the fingerprint is ever sent.

use rusty_chromaprint::{Configuration, FingerprintCompressor, Fingerprinter};
use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::codecs::{FlacDecoder, MpaDecoder};
use symphonia::default::formats::{FlacReader, MpaReader};

/// The first two minutes are enough to identify a recording (what `fpcalc` uses).
const MAX_SECONDS: u64 = 120;

pub struct Print {
    /// Compressed and base64url-encoded, as the AcoustID web service expects.
    pub encoded: String,
    /// The whole file's length in seconds.
    pub duration_s: u64,
    /// The uncompressed fingerprint, for comparing files locally.
    pub raw: Vec<u32>,
}

/// Fingerprint a FLAC or MP3 file.
pub fn fingerprint(path: &Path) -> Result<Print, String> {
    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mut reader: Box<dyn FormatReader> = match ext.as_str() {
        "flac" => Box::new(
            FlacReader::try_new(mss, FormatOptions::default()).map_err(|e| format!("read: {e}"))?,
        ),
        "mp3" => Box::new(
            MpaReader::try_new(mss, FormatOptions::default()).map_err(|e| format!("read: {e}"))?,
        ),
        other => return Err(format!("can't fingerprint .{other} files")),
    };
    let track = reader.tracks().first().ok_or("no audio track")?;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err("not audio".into());
    };
    let track_id = track.id;
    let mut decoder: Box<dyn AudioDecoder> = if ext == "flac" {
        Box::new(
            FlacDecoder::try_new(&params, &AudioDecoderOptions::default())
                .map_err(|e| format!("decoder: {e}"))?,
        )
    } else {
        Box::new(
            MpaDecoder::try_new(&params, &AudioDecoderOptions::default())
                .map_err(|e| format!("decoder: {e}"))?,
        )
    };
    let rate = params.sample_rate.unwrap_or(44_100);
    let channels = params
        .channels
        .as_ref()
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1) as u32;
    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    printer
        .start(rate, channels)
        .map_err(|e| format!("fingerprint: {e:?}"))?;
    let mut buf: Vec<i16> = Vec::new();
    let (mut frames, mut printed) = (0u64, 0u64);
    let limit = MAX_SECONDS * u64::from(rate);
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(_) => break, // a damaged tail: what was read is enough
        };
        if packet.track_id != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        buf.clear();
        decoded.copy_to_vec_interleaved::<i16>(&mut buf);
        let n = (buf.len() / channels as usize) as u64;
        frames += n;
        if printed < limit {
            let take = ((limit - printed).min(n) * u64::from(channels)) as usize;
            printer.consume(&buf[..take]);
            printed += take as u64 / u64::from(channels);
        }
    }
    printer.finish();
    let raw = printer.fingerprint();
    if raw.is_empty() {
        return Err("too short to fingerprint".into());
    }
    let compressed = FingerprintCompressor::from(&config).compress(raw);
    Ok(Print {
        encoded: base64url(&compressed),
        duration_s: frames / u64::from(rate.max(1)),
        raw: raw.to_vec(),
    })
}

/// Base64, URL-safe alphabet, no padding (what AcoustID expects).
fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64url;

    #[test]
    fn base64url_without_padding() {
        assert_eq!(base64url(b"Ma"), "TWE");
        assert_eq!(base64url(b"Man"), "TWFu");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }
}
