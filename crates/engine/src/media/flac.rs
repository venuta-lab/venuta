use super::{AudioProps, MediaFacts, PictureFacts, picture};
use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::ogg::OggPictureStorage;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub(super) fn read(path: &Path) -> Result<MediaFacts, String> {
    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut reader = BufReader::new(file);
    // BestAttempt keeps going past minor damage; implicit conversions off so we see the
    // file exactly as written.
    let options = ParseOptions::new()
        .parsing_mode(ParsingMode::BestAttempt)
        .implicit_conversions(false);
    let flac = FlacFile::read_from(&mut reader, options).map_err(|e| format!("parse: {e}"))?;

    let props = flac.properties();
    let signature = props.signature();
    let audio = AudioProps {
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth(),
        channels: props.channels(),
        duration_ms: props.duration().as_millis() as u64,
        audio_md5: (signature != 0).then(|| signature.to_be_bytes()),
    };

    let (tags, vendor) = match flac.vorbis_comments() {
        Some(vc) => (
            vc.items()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            Some(vc.vendor().to_owned()).filter(|v| !v.is_empty()),
        ),
        None => (Vec::new(), None),
    };

    let pictures = flac
        .pictures()
        .iter()
        .map(|(pic, _info)| {
            let data = pic.data();
            PictureFacts {
                pic_type: pic.pic_type().as_u8(),
                mime: pic.mime_type().map(|m| m.as_str().to_owned()),
                bytes: data.len() as u64,
                image: picture::inspect(data),
                data_hash: *blake3::hash(data).as_bytes(),
            }
        })
        .collect();

    Ok(MediaFacts {
        audio: Some(audio),
        tags,
        vendor,
        pictures,
    })
}
