//! Reading small text files (lyrics) whatever their encoding.

use std::path::Path;

/// Read a text file as UTF-8, UTF-8 with a byte-order mark, or UTF-16 (either byte order,
/// with its mark), as editors and downloaders write them. Anything else falls back to
/// Windows-1252, the usual older Windows encoding, so a file is never refused for its encoding.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    Ok(decode(&std::fs::read(path)?))
}

pub fn decode(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => b.iter().map(|&c| windows_1252(c)).collect(),
    }
}

fn utf16(b: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = b.as_chunks::<2>().0.iter().map(|&c| unit(c));
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn windows_1252(c: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match c {
        0x80..=0x9F => HIGH[(c - 0x80) as usize],
        _ => c as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_common_encoding() {
        let text = "[00:01.25] जाए न जाए";
        assert_eq!(decode(text.as_bytes()), text);
        let mut bom8 = vec![0xEF, 0xBB, 0xBF];
        bom8.extend_from_slice(text.as_bytes());
        assert_eq!(decode(&bom8), text);
        let mut le = vec![0xFF, 0xFE];
        le.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode(&le), text);
        let mut be = vec![0xFE, 0xFF];
        be.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode(&be), text);
        // Windows-1252: curly quotes and é.
        assert_eq!(decode(&[0x93, b'C', b'a', b'f', 0xE9, 0x94]), "“Café”");
    }
}
