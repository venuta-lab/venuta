//! The owner's rules: plain TOML files that are the authority over how the library should
//! look. Online sources and heuristics only ever *propose*; these files decide.
//!
//! Files (all optional, each has defaults):
//! * `aliases.toml`    one canonical spelling per person, with known variants
//! * `categories.toml` which source folders are what (film / independent / scores…), their
//!   language and default genre
//! * `layout.toml`     path template and limits
//! * `strip.toml`      tag keys and watermarks to remove
//! * `titles.toml`     suffixes to drop from album titles

use crate::audit::name_key;
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Aliases {
    #[serde(default, rename = "artist")]
    pub artists: Vec<Alias>,
    /// Pairs of names confirmed to be *different* people, so they are never suggested again.
    #[serde(default)]
    pub distinct: Vec<[String; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alias {
    pub name: String,
    #[serde(default)]
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRule {
    /// Glob over the album folder path relative to the library root, e.g. `01_Indian_Film_Music/01_Hindi/**`.
    pub pattern: String,
    pub category: String,
    /// ISO 639-3 code, e.g. `hin`.
    #[serde(default)]
    pub language: Option<String>,
    /// Genre used when the tracks have none.
    #[serde(default)]
    pub genre: Option<String>,
    /// Film/series soundtrack: the album artist is the composer, taken from the folder above
    /// the album folder.
    #[serde(default)]
    pub soundtrack: bool,
    /// Optional sub-folder under the category (e.g. `Games` under `Scores`).
    #[serde(default)]
    pub subfolder: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Categories {
    #[serde(default, rename = "rule")]
    pub rules: Vec<CategoryRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    /// Fields: {category} {subfolder} {language} {albumartist} {year} {album} {disc} {track} {title}
    pub template: String,
    pub max_component: usize,
    pub max_path: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            template: "{category}/{subfolder}/{language}/{albumartist}/{year} - {album}/{disc}{track} - {title}"
                .into(),
            max_component: 120,
            max_path: 180,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strip {
    /// Tag keys removed from every file (case-insensitive).
    pub keys: Vec<String>,
    /// Substrings that mark a tag value as a download-site watermark; such values are removed.
    pub watermarks: Vec<String>,
}

impl Default for Strip {
    fn default() -> Self {
        Self {
            keys: [
                "MAJOR_BRAND",
                "MINOR_VERSION",
                "COMPATIBLE_BRANDS",
                "ENCODER",
                "LENGTH",
                "TIDAL_DATA",
                "TIDAL_TRACK_ID",
                "TIDAL_ALBUM_ID",
                "TIDAL_TRACK_URL",
                "TIDAL_ALBUM_URL",
                "ALBUM_RELEASE_DATE",
                "ITUNESADVISORY",
            ]
            .map(String::from)
            .to_vec(),
            watermarks: ["t.me/", "telegram", "samperals", "desihires", "raag.fm"]
                .map(String::from)
                .to_vec(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Titles {
    /// Removed from the end of album titles (case-insensitive).
    pub album_suffixes: Vec<String>,
}

impl Default for Titles {
    fn default() -> Self {
        Self {
            album_suffixes: [
                " (Original Motion Picture Soundtrack)",
                " (Original Soundtrack)",
                " (Original Television Soundtrack)",
                " (Original Series Soundtrack)",
                " (Original Game Soundtrack)",
                " (Original Score)",
                " (Soundtrack)",
                " (OST)",
                " - Original Motion Picture Soundtrack",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Genres {
    /// Provider or legacy genre → your genre. Map to "" to drop a genre.
    #[serde(default)]
    pub map: std::collections::BTreeMap<String, String>,
}

/// All rules, loaded from a folder.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    pub aliases: Aliases,
    pub categories: Categories,
    pub layout: Layout,
    pub strip: Strip,
    pub titles: Titles,
    pub genres: Genres,
    alias_index: HashMap<String, String>,
    category_matchers: Vec<GlobMatcher>,
}

impl Rules {
    pub fn load(dir: &Path) -> Result<Self, String> {
        fn read<T: for<'de> Deserialize<'de> + Default>(
            dir: &Path,
            name: &str,
        ) -> Result<T, String> {
            let p = dir.join(name);
            if !p.exists() {
                return Ok(T::default());
            }
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{name}: {e}"))?;
            toml::from_str(&text).map_err(|e| format!("{name}: {e}"))
        }
        let mut r = Rules {
            aliases: read(dir, "aliases.toml")?,
            categories: read(dir, "categories.toml")?,
            layout: read(dir, "layout.toml")?,
            strip: read(dir, "strip.toml")?,
            titles: read(dir, "titles.toml")?,
            genres: read(dir, "genres.toml")?,
            ..Default::default()
        };
        r.reindex()?;
        Ok(r)
    }

    /// Rebuild lookup structures after the rules change.
    pub fn reindex(&mut self) -> Result<(), String> {
        self.alias_index.clear();
        for a in &self.aliases.artists {
            self.alias_index.insert(name_key(&a.name), a.name.clone());
            for v in &a.variants {
                self.alias_index.insert(name_key(v), a.name.clone());
            }
        }
        self.category_matchers = self
            .categories
            .rules
            .iter()
            .map(|r| {
                Glob::new(&r.pattern)
                    .map(|g| g.compile_matcher())
                    .map_err(|e| format!("categories.toml: bad pattern {:?}: {e}", r.pattern))
            })
            .collect::<Result<_, _>>()?;
        Ok(())
    }

    /// Canonical spelling of a person's name, if the aliases know them.
    pub fn canonical(&self, name: &str) -> Option<&str> {
        self.alias_index.get(&name_key(name)).map(String::as_str)
    }

    /// First category rule whose pattern matches the album folder.
    pub fn category_for(&self, album_dir: &str) -> Option<&CategoryRule> {
        self.category_matchers
            .iter()
            .position(|m| m.is_match(album_dir))
            .map(|i| &self.categories.rules[i])
    }

    /// Your genre for a raw genre value: mapped, dropped (None) or kept as is.
    pub fn map_genre(&self, raw: &str) -> Option<String> {
        match self
            .genres
            .map
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(raw.trim()))
        {
            Some((_, v)) if v.is_empty() => None,
            Some((_, v)) => Some(v.clone()),
            None => Some(raw.trim().to_owned()),
        }
    }

    pub fn is_distinct(&self, a: &str, b: &str) -> bool {
        let (ka, kb) = (name_key(a), name_key(b));
        self.aliases.distinct.iter().any(|[x, y]| {
            (name_key(x) == ka && name_key(y) == kb) || (name_key(x) == kb && name_key(y) == ka)
        })
    }

    /// Write every rules file (used by `init` and by the UI when the owner answers a question).
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let write = |name: &str, header: &str, body: String| -> Result<(), String> {
            let tmp = dir.join(format!("{name}.part"));
            std::fs::write(&tmp, format!("{header}\n\n{body}")).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
        };
        let ser = |v: &dyn erased::Ser| v.to_toml();
        write(
            "aliases.toml",
            "# One canonical spelling per person. Variants are matched ignoring case, spaces and punctuation.",
            ser(&self.aliases)?,
        )?;
        write(
            "categories.toml",
            "# First matching rule wins. Patterns are globs over the album folder, relative to the library root.",
            ser(&self.categories)?,
        )?;
        write(
            "layout.toml",
            "# Where each track lives. Empty fields collapse, so {subfolder} disappears when unset.",
            ser(&self.layout)?,
        )?;
        write(
            "strip.toml",
            "# Tags removed from every file.",
            ser(&self.strip)?,
        )?;
        write(
            "titles.toml",
            "# Cleanup of album titles.",
            ser(&self.titles)?,
        )?;
        write(
            "genres.toml",
            "# Map provider or legacy genres onto your own. Map to \"\" to drop one.",
            ser(&self.genres)?,
        )?;
        Ok(())
    }
}

/// Tiny helper so `save` can serialise different types through one closure.
mod erased {
    pub trait Ser {
        fn to_toml(&self) -> Result<String, String>;
    }
    impl<T: serde::Serialize> Ser for T {
        fn to_toml(&self) -> Result<String, String> {
            toml::to_string_pretty(self).map_err(|e| e.to_string())
        }
    }
}

/// Record the owner's answer to "are A and B the same artist?" in `aliases.toml`.
/// `keep` is the spelling to use when they are the same; `None` means they are different people.
pub fn answer_same_artist(dir: &Path, a: &str, b: &str, keep: Option<&str>) -> Result<(), String> {
    let mut rules = Rules::load(dir)?;
    match keep {
        None => rules.aliases.distinct.push([a.to_owned(), b.to_owned()]),
        Some(keep) => {
            let other = if name_key(keep) == name_key(a) { b } else { a };
            // Gather both entries' spellings into the kept one.
            let mut spellings: Vec<String> = vec![other.to_owned()];
            rules.aliases.artists.retain(|al| {
                let hit = name_key(&al.name) == name_key(other)
                    || (name_key(&al.name) != name_key(keep)
                        && al.variants.iter().any(|v| name_key(v) == name_key(other)));
                if hit {
                    spellings.push(al.name.clone());
                    spellings.extend(al.variants.iter().cloned());
                }
                !hit
            });
            let entry = match rules
                .aliases
                .artists
                .iter_mut()
                .find(|al| name_key(&al.name) == name_key(keep))
            {
                Some(e) => e,
                None => {
                    rules.aliases.artists.push(Alias {
                        name: keep.to_owned(),
                        variants: Vec::new(),
                    });
                    rules.aliases.artists.last_mut().expect("just pushed")
                }
            };
            for s in spellings {
                if name_key(&s) != name_key(&entry.name)
                    && !entry.variants.iter().any(|v| name_key(v) == name_key(&s))
                {
                    entry.variants.push(s);
                }
            }
        }
    }
    rules.save(dir)
}

/// Starter aliases from the library itself: every spelling of every credited name (album
/// artist, composer, lyricist, and the artist folders above album folders), grouped by
/// `name_key`. The canonical spelling is the most common one in tags, never ALL CAPS.
pub fn bootstrap_aliases(lib: &crate::library::Library) -> Aliases {
    use crate::plan::split_names;
    // spelling → (times seen in tags, seen as a folder name)
    let mut groups: HashMap<String, HashMap<String, (usize, bool)>> = HashMap::new();
    let mut add = |name: &str, from_tags: bool| {
        let name = name
            .replace('\u{a0}', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if name.is_empty() {
            return;
        }
        let e = groups
            .entry(name_key(&name))
            .or_default()
            .entry(name)
            .or_insert((0, false));
        if from_tags {
            e.0 += 1;
        } else {
            e.1 = true;
        }
    };
    for t in &lib.tracks {
        for key in ["ALBUMARTIST", "COMPOSER", "LYRICIST"] {
            for v in t.tags.get(key).into_iter().flatten() {
                for n in split_names(v) {
                    add(&n, true);
                }
            }
        }
    }
    let folders: std::collections::BTreeSet<String> = lib
        .tracks
        .iter()
        .map(|t| {
            crate::library::parent_dir(&t.dir)
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .collect();
    for f in folders {
        let pretty = f.replace('_', " ");
        if pretty
            .split(' ')
            .next()
            .is_some_and(|w| w.len() == 2 && w.chars().all(|c| c.is_ascii_digit()))
        {
            continue; // "01 Hindi"-style grouping folders aren't people
        }
        add(&pretty, false);
    }
    let mut artists: Vec<Alias> = groups
        .into_values()
        .filter(|g| g.len() > 1 || g.values().any(|(_, folder)| *folder))
        .map(|g| {
            let mut spellings: Vec<(String, (usize, bool))> = g.into_iter().collect();
            // Prefer: not ALL CAPS, then most used in tags, then alphabetical.
            spellings.sort_by(|(a, (na, _)), (b, (nb, _))| {
                let shouting = |s: &str| {
                    s.chars()
                        .filter(|c| c.is_alphabetic())
                        .all(char::is_uppercase)
                };
                (shouting(a), std::cmp::Reverse(*na), a.clone()).cmp(&(
                    shouting(b),
                    std::cmp::Reverse(*nb),
                    b.clone(),
                ))
            });
            let name = spellings[0].0.clone();
            let variants = spellings.into_iter().skip(1).map(|(s, _)| s).collect();
            Alias { name, variants }
        })
        .collect();
    artists.sort_by_key(|a| a.name.to_lowercase());
    Aliases {
        artists,
        distinct: Vec::new(),
    }
}

/// ISO 639-3 code → display name used in folder names.
pub fn language_name(code: &str) -> &str {
    match code {
        "hin" => "Hindi",
        "mar" => "Marathi",
        "tam" => "Tamil",
        "mal" => "Malayalam",
        "tel" => "Telugu",
        "kan" => "Kannada",
        "ben" => "Bengali",
        "urd" => "Urdu",
        "pan" => "Punjabi",
        "guj" => "Gujarati",
        "eng" => "English",
        "jpn" => "Japanese",
        "kor" => "Korean",
        "zho" => "Chinese",
        "fil" => "Filipino",
        "zxx" => "Instrumental",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_categories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("aliases.toml"),
            r#"
[[artist]]
name = "A.R. Rahman"
variants = ["A. R. Rahman", "A_R_Rahman"]
"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("categories.toml"),
            r#"
[[rule]]
pattern = "01_Indian_Film_Music/01_Hindi/**"
category = "Film & Series"
language = "hin"
soundtrack = true
"#,
        )
        .unwrap();
        let r = Rules::load(dir.path()).unwrap();
        assert_eq!(r.canonical("A.  R.  Rahman"), Some("A.R. Rahman"));
        assert_eq!(r.canonical("A R Rahman"), Some("A.R. Rahman"));
        assert_eq!(r.canonical("Pritam"), None);
        let c = r
            .category_for("01_Indian_Film_Music/01_Hindi/A_R_Rahman/2004_Swades")
            .unwrap();
        assert!(c.soundtrack);
        assert!(r.category_for("03_English/Pink_Floyd/1975_Wish").is_none());

        // "Same artist?" answers edit aliases.toml.
        answer_same_artist(dir.path(), "Pritam", "Pritam Chakraborty", Some("Pritam")).unwrap();
        let merged = Rules::load(dir.path()).unwrap();
        assert_eq!(merged.canonical("Pritam Chakraborty"), Some("Pritam"));
        answer_same_artist(dir.path(), "The Local Train", "The Local Music Train", None).unwrap();
        let distinct = Rules::load(dir.path()).unwrap();
        assert!(distinct.is_distinct("The Local Music Train", "The Local Train"));
        assert_eq!(distinct.canonical("A. R. Rahman"), Some("A.R. Rahman")); // untouched

        // Round trip through save/load keeps everything.
        r.save(dir.path()).unwrap();
        let again = Rules::load(dir.path()).unwrap();
        assert_eq!(again.aliases.artists.len(), 1);
        assert_eq!(again.layout.max_path, 180);
    }
}
