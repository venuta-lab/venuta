//! YouTube Music's taste builder ("Tell us which artists you like"): the artists it offers, each
//! with the two opaque form values its answer is made of. ytmusicapi `get_tasteprofile` /
//! `set_tasteprofile`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::metadata::{find_all, last_thumbnail, runs_text};

/// One artist the taste builder offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasteArtist {
    pub name: String,
    /// Sent among `selectedValues` when the person chose them.
    pub selection_value: String,
    /// Sent among `impressionValues` for every artist shown, chosen or not.
    pub impression_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Chosen already, when YouTube says so.
    #[serde(default)]
    pub selected: bool,
}

/// The artists of a `FEmusic_tastebuilder` answer, in its order, each once. One without a name
/// or either form value can't be offered and is left out.
pub fn parse_taste_profile(root: &Value) -> Vec<TasteArtist> {
    let mut seen = std::collections::HashSet::new();
    find_all(root, "tastebuilderItemRenderer")
        .into_iter()
        .filter_map(|item| {
            let name = runs_text(item.get("title"))?.trim().to_owned();
            let value = |key: &str| {
                item.get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
            };
            let artist = TasteArtist {
                selection_value: value("selectionFormValue")?,
                impression_value: value("impressionFormValue")?,
                thumbnail: item.get("thumbnail").and_then(last_thumbnail),
                selected: item
                    .get("isSelected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                name,
            };
            (!artist.name.is_empty()).then_some(artist)
        })
        .filter(|a| seen.insert(a.selection_value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tastebuilder.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    // Every artist across its lists, with both form values; one with no name is no artist.
    #[test]
    fn reads_the_artists_the_taste_builder_offers() {
        let artists = parse_taste_profile(&fixture());
        let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Arijit Singh", "Shreya Ghoshal", "A.R. Rahman"]);
        assert_eq!(artists[0].selection_value, "FIXTURE-SELECT-ARIJIT");
        assert_eq!(artists[0].impression_value, "FIXTURE-SEEN-ARIJIT");
        assert!(artists[0]
            .thumbnail
            .as_deref()
            .is_some_and(|t| t.contains("w226")));
        assert!(!artists[0].selected);
        assert!(artists[1].selected);
        assert!(parse_taste_profile(&serde_json::json!({})).is_empty());
    }
}
