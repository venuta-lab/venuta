//! Translation on this PC (`engine::translation`): the models (listed, downloaded when the
//! listener clicks Download, removed when asked) and a song's translation. Nothing is sent
//! anywhere; the only network use is a model's download, from Mozilla.

use engine::translation::Translations;
use lyrics_translate::store::ModelStatus;

/// Every translation model: what is on this PC, what is downloading (0 to 1) and what failed.
#[tauri::command]
pub fn translation_models() -> Vec<ModelStatus> {
    Translations::global().models()
}

/// Starts downloading a model and returns at once (its progress shows in
/// `translation_models`). Refused in Local only.
#[tauri::command]
pub fn translation_model_download(id: String) -> Result<(), String> {
    Translations::global().download(&id)
}

/// Takes a model off this PC (stopping its download).
#[tauri::command]
pub fn translation_model_remove(id: String) -> Result<(), String> {
    Translations::global().remove(&id)
}

/// Translates the song whose lyrics are open under `key` from `from` into `into`, on this PC.
/// When it returns, reading the song's lyrics again gives its lines their translation. Every
/// window asks for its song; asks for the same song at once share one translation.
#[tauri::command]
pub async fn lyrics_translate(key: String, from: String, into: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        Translations::global().translate(&key, &from, &into)
    })
    .await
    .map_err(|e| e.to_string())?
}
