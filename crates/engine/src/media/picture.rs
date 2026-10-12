//! Cover-art inspection. FLAC PICTURE blocks often record width/height as 0, and nothing
//! records whether a JPEG is progressive (which many players cannot show), so we read the
//! image header ourselves.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Jpeg,
    Png,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    /// True for progressive (SOF2) or other multi-scan JPEGs.
    pub progressive: bool,
}

/// Inspect image bytes. Returns `None` if the header cannot be understood.
pub fn inspect(data: &[u8]) -> Option<ImageInfo> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return inspect_png(data);
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        return inspect_jpeg(data);
    }
    None
}

fn inspect_png(data: &[u8]) -> Option<ImageInfo> {
    // Signature (8) + IHDR length (4) + "IHDR" (4) + width (4) + height (4)
    if data.len() < 24 || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    Some(ImageInfo {
        format: ImageFormat::Png,
        width,
        height,
        progressive: false,
    })
}

fn inspect_jpeg(data: &[u8]) -> Option<ImageInfo> {
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // Fill bytes and standalone markers carry no length.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        // SOFn markers (C0..CF except C4 DHT, C8 JPG, CC DAC) carry the frame size.
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            if i + 9 > data.len() {
                return None;
            }
            let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            // C2/C6/CA/CE are progressive variants.
            let progressive = matches!(marker, 0xC2 | 0xC6 | 0xCA | 0xCE);
            return Some(ImageInfo {
                format: ImageFormat::Jpeg,
                width,
                height,
                progressive,
            });
        }
        if marker == 0xDA {
            // Start of scan before any SOF: malformed.
            return None;
        }
        i += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_with_sof(sof: u8, w: u16, h: u16) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        // APP0 segment of length 16 (content irrelevant)
        v.extend([0xFF, 0xE0, 0x00, 0x10]);
        v.extend([0u8; 14]);
        // SOF: length 17, precision 8, height, width, 3 components
        v.extend([0xFF, sof, 0x00, 0x11, 0x08]);
        v.extend(h.to_be_bytes());
        v.extend(w.to_be_bytes());
        v.extend([0x03, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        v.extend([0xFF, 0xD9]);
        v
    }

    #[test]
    fn baseline_jpeg() {
        let info = inspect(&jpeg_with_sof(0xC0, 1400, 1200)).unwrap();
        assert_eq!(
            (info.width, info.height, info.progressive),
            (1400, 1200, false)
        );
        assert_eq!(info.format, ImageFormat::Jpeg);
    }

    #[test]
    fn progressive_jpeg() {
        let info = inspect(&jpeg_with_sof(0xC2, 1000, 1000)).unwrap();
        assert!(info.progressive);
    }

    #[test]
    fn png_header() {
        let mut v = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        v.extend(3000u32.to_be_bytes());
        v.extend(2999u32.to_be_bytes());
        let info = inspect(&v).unwrap();
        assert_eq!(
            (info.format, info.width, info.height),
            (ImageFormat::Png, 3000, 2999)
        );
    }

    #[test]
    fn garbage_is_none() {
        assert!(inspect(b"not an image").is_none());
        assert!(inspect(&[0xFF, 0xD8, 0xFF]).is_none());
    }
}
