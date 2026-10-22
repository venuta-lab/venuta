//! Translation (`engine::translation`): on this PC (the models, listed, downloaded when the
//! listener clicks Download, removed when asked; nothing is sent anywhere, the only network use
//! is a model's download, from Mozilla), or with the listener's own key (DeepL, Microsoft,
//! Google, Claude), kept in Windows Credential Manager and used only when the listener asks.

use engine::accounts::SystemVault;
use engine::translation::{self, Ask, Credentials, KeyState, Keyed, Translations};
use lyrics_translate::store::ModelStatus;

/// Every translation model: what is on this PC, what is downloading (0 to 1), what failed, and
/// whether it is offered at all.
#[tauri::command]
pub fn translation_models() -> Vec<ModelStatus> {
    Translations::global().models()
}

/// Starts downloading a model and returns at once (its progress shows in
/// `translation_models`). Refused in Local only, and for a model not offered.
#[tauri::command]
pub fn translation_model_download(id: String) -> Result<(), String> {
    Translations::global().download(&id)
}

/// Takes a model off this PC (stopping its download).
#[tauri::command]
pub fn translation_model_remove(id: String) -> Result<(), String> {
    Translations::global().remove(&id)
}

fn keyed(engine: &str) -> Result<Keyed, String> {
    Keyed::from_id(engine).ok_or_else(|| format!("There is no translation engine called {engine}."))
}

/// Translates the song whose lyrics are open under `key` from `from` into `into`: on this PC
/// (`engine` none or "device"), or with the listener's own key for `engine` ("deepl",
/// "microsoft", "google", "llm"; `model` is the language model's id). When it returns,
/// reading the song's lyrics again gives its lines their translation. Asks for the same song
/// at once share one translation.
#[tauri::command]
pub async fn lyrics_translate(
    key: String,
    from: String,
    into: String,
    engine: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    let which = match engine.as_deref() {
        None | Some("") | Some("device") => None,
        Some(e) => Some(keyed(e)?),
    };
    tauri::async_runtime::spawn_blocking(move || match which {
        None => Translations::global().translate(&key, &from, &into),
        Some(engine) => Translations::global().translate_keyed(
            &SystemVault,
            Ask { engine, model },
            &key,
            &from,
            &into,
        ),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Every key-based engine, with whether a key is kept for it (never the key).
#[tauri::command]
pub fn translation_keys() -> Vec<KeyState> {
    translation::keys(&SystemVault)
}

/// Keeps the key the listener entered (and Microsoft's region) in the system's secret store.
#[tauri::command]
pub fn translation_key_save(
    engine: String,
    key: String,
    region: Option<String>,
) -> Result<(), String> {
    translation::save_key(&SystemVault, keyed(&engine)?, &Credentials { key, region })
}

#[tauri::command]
pub fn translation_key_forget(engine: String) -> Result<(), String> {
    translation::forget_key(&SystemVault, keyed(&engine)?)
}

/// Tries a key with one short line: the key as typed (`key`), or the one kept when none is
/// typed. Returns the line and what came back.
#[tauri::command]
pub async fn translation_key_test(
    engine: String,
    key: Option<String>,
    region: Option<String>,
    model: Option<String>,
    into: String,
) -> Result<(String, String), String> {
    let engine = keyed(&engine)?;
    let creds = key.map(|key| Credentials { key, region });
    tauri::async_runtime::spawn_blocking(move || {
        translation::test_key(&SystemVault, Ask { engine, model }, creds, &into)
    })
    .await
    .map_err(|e| e.to_string())?
}
