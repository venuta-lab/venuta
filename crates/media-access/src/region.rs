//! Which country and language the online catalog answers for.
//!
//! The catalog picks its charts, home feed and search ranking by country (`gl`), and writes its
//! own labels in a language (`hl`). Left alone it fell back to the United States, so an Indian
//! listener got American charts. The country now comes from Windows' own region setting, the
//! one in Settings > Time & language > Region, so it is right without asking.

/// Country and language for catalog requests: an ISO 3166 two-letter country, a language tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRegion {
    pub country: String,
    pub language: String,
}

impl Default for CatalogRegion {
    fn default() -> Self {
        Self {
            country: "US".into(),
            language: "en".into(),
        }
    }
}

/// The region to use: `VENUTA_REGION` (for testing another country), else the computer's own
/// region, else the United States. The language stays English: song titles and artist names
/// come as the catalog has them either way, and the app's own words are English.
pub fn catalog_region() -> CatalogRegion {
    let country = std::env::var("VENUTA_REGION")
        .ok()
        .and_then(|v| country_code(&v))
        .or_else(system_country)
        .unwrap_or_else(|| "US".into());
    CatalogRegion {
        country,
        language: "en".into(),
    }
}

/// A two-letter country code, upper-cased, or nothing for anything else (a numeric UN M.49
/// region such as "419", an empty answer, junk).
pub(crate) fn country_code(raw: &str) -> Option<String> {
    let code = raw.trim();
    (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| code.to_ascii_uppercase())
}

#[cfg(windows)]
fn system_country() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultGeoName;
    let mut buf = [0u16; 16];
    // SAFETY: the buffer and its length are passed together; the call writes at most that many
    // UTF-16 units, including the terminating zero, and returns how many it wrote.
    let n = unsafe { GetUserDefaultGeoName(buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 1 {
        return None;
    }
    let text = String::from_utf16_lossy(&buf[..(n as usize - 1)]);
    country_code(&text)
}

#[cfg(not(windows))]
fn system_country() -> Option<String> {
    // LANG=en_IN.UTF-8 and the like.
    let lang = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()?;
    let region = lang.split(['.', '@']).next()?.split('_').nth(1)?;
    country_code(region)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_country_is_two_letters_and_anything_else_is_ignored() {
        assert_eq!(country_code("in").as_deref(), Some("IN"));
        assert_eq!(country_code(" GB ").as_deref(), Some("GB"));
        assert_eq!(country_code("419"), None, "a UN region is not a country");
        assert_eq!(country_code(""), None);
        assert_eq!(country_code("IND"), None);
        assert_eq!(country_code("I1"), None);
    }

    #[test]
    fn the_language_stays_english_and_there_is_always_a_country() {
        let region = catalog_region();
        assert_eq!(region.language, "en");
        assert_eq!(region.country.len(), 2);
    }
}
