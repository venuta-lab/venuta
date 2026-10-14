//! Your themes: packages in the library's `.venuta/themes` (see `engine::themes`), for Settings'
//! gallery and the Theme Studio. The app checks what a theme says; these commands move files.

use engine::{paths, themes};
use std::path::{Path, PathBuf};
use tauri::ipc::Response;

type CmdResult<T> = Result<T, String>;

/// Where a removed theme goes: the library's Set aside.
fn set_aside_dir() -> CmdResult<PathBuf> {
    let home = paths::home().ok_or("Choose your library first.")?;
    Ok(paths::beside(
        &paths::music_dir(&home),
        paths::Helper::SetAside,
    ))
}

#[tauri::command]
pub fn themes_list() -> Vec<themes::Package> {
    themes::list(&themes::dir())
}

/// One file of a theme, as bytes (fonts and pictures become blob URLs in the page).
#[tauri::command]
pub fn theme_file(id: String, path: String) -> CmdResult<Response> {
    themes::read(&themes::dir(), &id, &path).map(Response::new)
}

#[tauri::command]
pub fn theme_save(id: String, json: String) -> CmdResult<()> {
    themes::save(&themes::dir(), &id, &json)
}

#[tauri::command]
pub fn theme_write_text(id: String, path: String, text: String) -> CmdResult<()> {
    themes::write_text(&themes::dir(), &id, &path, &text)
}

/// Copy a font or picture chosen on this computer into a theme.
#[tauri::command]
pub fn theme_add_file(id: String, path: String, from: String) -> CmdResult<()> {
    themes::add_file(
        &themes::dir(),
        &set_aside_dir()?,
        &id,
        &path,
        Path::new(&from),
    )
}

#[derive(serde::Serialize)]
pub struct Version {
    when: u64,
    json: String,
}

#[tauri::command]
pub fn theme_history(id: String) -> CmdResult<Vec<Version>> {
    Ok(themes::history(&themes::dir(), &id)?
        .into_iter()
        .map(|(when, json)| Version { when, json })
        .collect())
}

/// Remove a theme: it goes to Set aside, whole. Returns where.
#[tauri::command]
pub fn theme_set_aside(id: String) -> CmdResult<String> {
    themes::set_aside(&themes::dir(), &set_aside_dir()?, &id)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// Changes when anything in the themes folder does (a theme edited by hand reloads).
#[tauri::command]
pub fn themes_stamp() -> u64 {
    themes::stamp(&themes::dir())
}

#[tauri::command]
pub async fn theme_export(id: String, dest: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        themes::export(&themes::dir(), &id, Path::new(&dest))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn theme_inspect(path: String) -> CmdResult<themes::Inspected> {
    tauri::async_runtime::spawn_blocking(move || themes::inspect(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Install a shared theme as `id`. With `replace`, the theme already called `id` goes to Set
/// aside first.
#[tauri::command]
pub async fn theme_install(path: String, id: String, replace: bool) -> CmdResult<()> {
    let aside = set_aside_dir()?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = themes::dir();
        // Checked before anything moves, so a bad file never costs the theme it would replace.
        themes::inspect(Path::new(&path))?;
        if replace && root.join(&id).is_dir() {
            themes::set_aside(&root, &aside, &id)?;
        }
        themes::install(&root, Path::new(&path), &id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn theme_duplicate(from: String, to: String) -> CmdResult<()> {
    themes::duplicate(&themes::dir(), &from, &to)
}

/// Show a theme's folder in File Explorer, its theme.json selected.
#[tauri::command]
pub fn theme_show(app: tauri::AppHandle, id: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    if !themes::valid_id(&id) {
        return Err("That isn't one of your themes.".into());
    }
    let file = themes::dir().join(&id).join(themes::MANIFEST);
    app.opener()
        .reveal_item_in_dir(&file)
        .map_err(|e| e.to_string())
}
