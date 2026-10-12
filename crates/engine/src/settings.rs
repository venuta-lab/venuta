//! Small persistent settings: where the rules live, where the new library goes, and the
//! owner's review decisions per album. Plain JSON in the app data folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Folder holding the rules TOML files.
    pub rules_dir: Option<String>,
    /// Where the migrated library is built.
    pub new_library: Option<String>,
    /// The owner confirmed they have a backup; required before editing files in place.
    #[serde(default)]
    pub backup_confirmed: bool,
    /// Your Apple Music library file and its key (entered by you; never shipped with the app).
    #[serde(default)]
    pub apple_library: Option<String>,
    #[serde(default)]
    pub apple_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approved,
    Skipped,
}

fn settings_path() -> PathBuf {
    crate::paths::app_data_dir().join("settings.json")
}

fn decisions_path(root: &Path) -> PathBuf {
    crate::paths::index_path_for(root).with_file_name("decisions.json")
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.part");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

pub fn load() -> Settings {
    read_json(&settings_path())
}

pub fn save(s: &Settings) -> Result<(), String> {
    write_json(&settings_path(), s)
}

pub fn load_decisions(root: &Path) -> BTreeMap<String, Decision> {
    read_json(&decisions_path(root))
}

pub fn save_decisions(root: &Path, d: &BTreeMap<String, Decision>) -> Result<(), String> {
    write_json(&decisions_path(root), d)
}

fn answers_path(root: &Path) -> PathBuf {
    crate::paths::index_path_for(root).with_file_name("answers.json")
}

/// The owner's answers to decisions for one library (see `crate::review`).
pub fn load_answers(root: &Path) -> crate::review::Answers {
    read_json(&answers_path(root))
}

pub fn save_answers(root: &Path, a: &crate::review::Answers) -> Result<(), String> {
    write_json(&answers_path(root), a)
}
