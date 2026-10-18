//! Which of a song's audio streams to play.
//!
//! A player answer lists several audio-only streams: AAC in MP4, Opus in WebM, at a few
//! qualities, and on a dubbed video one set per language. Venuta takes the original language,
//! then the best quality YouTube marks, then the better codec it can decode, then the higher
//! bitrate. A stream that needs deciphering is passed over: the in-app resolver only takes plain
//! addresses, and leaves the rest to the stream tool.

use ytmusic::{Format, StreamingData};

/// Whether the player decodes Opus (it does, through libopus): the better codec at a quality.
pub(crate) const PLAYS_OPUS: bool = true;

/// The stream to play, or none when no plain, playable audio stream is offered.
#[cfg(test)]
pub(crate) fn pick_audio(data: &StreamingData, opus: bool) -> Option<&Format> {
    pick_audio_for(data, opus, false)
}

/// `pick_audio`, or with `saver` the smallest stream (data saver: the lowest bitrate offered).
pub(crate) fn pick_audio_for(data: &StreamingData, opus: bool, saver: bool) -> Option<&Format> {
    let playable = |f: &&Format| {
        f.is_audio()
            && f.direct_url().is_some()
            && ((f.mime_type.contains("mp4a") && !high_efficiency_aac(f))
                || (opus && f.mime_type.contains("opus")))
    };
    let mut audio: Vec<&Format> = data.adaptive_formats.iter().filter(playable).collect();
    if audio.is_empty() {
        audio = data.formats.iter().flatten().filter(playable).collect();
    }
    let original: Vec<&Format> = audio.iter().copied().filter(|f| f.is_original()).collect();
    let pool = if original.is_empty() { audio } else { original };
    if saver {
        return pool
            .into_iter()
            .min_by_key(|f| (f.bitrate, std::cmp::Reverse(codec_rank(f))));
    }
    pool.into_iter()
        .max_by_key(|f| (quality_rank(f), codec_rank(f), f.bitrate))
}

/// HE-AAC (`mp4a.40.5`, and v2 `mp4a.40.29`): the smallest stream YouTube offers (itag 139),
/// which the player's AAC decoder can't play.
fn high_efficiency_aac(f: &Format) -> bool {
    f.mime_type.contains("mp4a.40.5") || f.mime_type.contains("mp4a.40.29")
}

fn quality_rank(f: &Format) -> u8 {
    match f.audio_quality.as_deref() {
        Some("AUDIO_QUALITY_HIGH") => 3,
        Some("AUDIO_QUALITY_MEDIUM") => 2,
        Some("AUDIO_QUALITY_LOW") => 1,
        _ => 0,
    }
}

fn codec_rank(f: &Format) -> u8 {
    if f.mime_type.contains("opus") { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(formats: serde_json::Value) -> StreamingData {
        serde_json::from_value(json!({ "adaptiveFormats": formats })).unwrap()
    }

    fn aac(itag: i32, bitrate: i64, quality: &str) -> serde_json::Value {
        json!({ "itag": itag, "url": format!("https://x/{itag}"), "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                "bitrate": bitrate, "audioQuality": quality, "contentLength": "4000000" })
    }

    fn opus(itag: i32, bitrate: i64, quality: &str) -> serde_json::Value {
        json!({ "itag": itag, "url": format!("https://x/{itag}"), "mimeType": "audio/webm; codecs=\"opus\"",
                "bitrate": bitrate, "audioQuality": quality, "contentLength": "3500000" })
    }

    /// itag 139 as YouTube sends it: HE-AAC.
    fn he_aac(itag: i32, bitrate: i64) -> serde_json::Value {
        json!({ "itag": itag, "url": format!("https://x/{itag}"), "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"",
                "bitrate": bitrate, "audioQuality": "AUDIO_QUALITY_LOW", "contentLength": "1500000" })
    }

    #[test]
    fn data_saver_plays_the_smallest_stream() {
        let d = data(json!([
            opus(251, 160_000, "AUDIO_QUALITY_MEDIUM"),
            aac(139, 48_000, "AUDIO_QUALITY_LOW"),
            opus(249, 50_000, "AUDIO_QUALITY_LOW"),
            aac(140, 130_000, "AUDIO_QUALITY_MEDIUM"),
        ]));
        assert_eq!(pick_audio_for(&d, true, true).map(|f| f.itag), Some(139));
        assert_eq!(pick_audio_for(&d, true, false).map(|f| f.itag), Some(251));
    }

    #[test]
    fn he_aac_is_passed_over_it_does_not_decode() {
        let d = data(json!([
            opus(251, 160_000, "AUDIO_QUALITY_MEDIUM"),
            he_aac(139, 48_000),
            opus(249, 50_000, "AUDIO_QUALITY_LOW"),
            aac(140, 130_000, "AUDIO_QUALITY_MEDIUM"),
        ]));
        assert_eq!(
            pick_audio_for(&d, true, true).map(|f| f.itag),
            Some(249),
            "the smallest that plays"
        );
        assert_eq!(
            pick_audio_for(&d, false, true).map(|f| f.itag),
            Some(140),
            "without Opus, plain AAC"
        );
        assert_eq!(pick_audio_for(&d, true, false).map(|f| f.itag), Some(251));
    }

    #[test]
    fn without_opus_the_best_aac_is_played() {
        let d = data(json!([
            opus(251, 160_000, "AUDIO_QUALITY_MEDIUM"),
            aac(139, 48_000, "AUDIO_QUALITY_LOW"),
            aac(140, 130_000, "AUDIO_QUALITY_MEDIUM"),
        ]));
        assert_eq!(pick_audio(&d, false).map(|f| f.itag), Some(140));
    }

    #[test]
    fn with_opus_the_better_codec_wins_at_the_same_quality() {
        let d = data(json!([
            aac(140, 130_000, "AUDIO_QUALITY_MEDIUM"),
            opus(251, 160_000, "AUDIO_QUALITY_MEDIUM"),
        ]));
        assert_eq!(pick_audio(&d, true).map(|f| f.itag), Some(251));
    }

    #[test]
    fn a_higher_quality_beats_a_better_codec() {
        let d = data(json!([
            opus(251, 160_000, "AUDIO_QUALITY_MEDIUM"),
            aac(141, 256_000, "AUDIO_QUALITY_HIGH"),
        ]));
        assert_eq!(pick_audio(&d, true).map(|f| f.itag), Some(141));
    }

    #[test]
    fn video_streams_and_ciphered_streams_are_passed_over() {
        let d = data(json!([
            { "itag": 137, "url": "https://x/137", "mimeType": "video/mp4", "bitrate": 4_000_000, "width": 1920, "height": 1080 },
            { "itag": 140, "signatureCipher": "s=abc&url=https://x/140", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130_000 },
        ]));
        assert!(pick_audio(&d, true).is_none(), "nothing plain and playable");
    }

    #[test]
    fn the_original_language_beats_a_dub() {
        let mut dub = aac(140, 140_000, "AUDIO_QUALITY_MEDIUM");
        dub["audioTrack"] = json!({ "audioIsDefault": false, "isAutoDubbed": true });
        let mut original = aac(140, 120_000, "AUDIO_QUALITY_MEDIUM");
        original["url"] = json!("https://x/original");
        original["audioTrack"] = json!({ "audioIsDefault": true });
        let d = data(json!([dub, original]));
        assert_eq!(
            pick_audio(&d, false).and_then(|f| f.url.as_deref()),
            Some("https://x/original")
        );
    }
}
