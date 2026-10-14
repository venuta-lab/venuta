//! Theme packages: folders in `<library>/.venuta/themes/<id>/`, each a `theme.json` and the
//! files it names (fonts, background art, an expert stylesheet). The app checks what a theme
//! *says* (docs/THEMES.md); this module keeps the files safe: names inside the package only,
//! known kinds only, sizes capped, nothing ever destroyed (a removed theme goes to Set aside,
//! and every save keeps the version before it, for good).

use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// The folder of theme packages, inside Venuta's data folder.
pub fn dir() -> PathBuf {
    crate::paths::app_data_dir().join("themes")
}

pub const MANIFEST: &str = "theme.json";
const HISTORY: &str = ".history";

/// A whole package, unpacked.
pub const MAX_PACKAGE: u64 = 25 * 1024 * 1024;
pub const MAX_FONT: u64 = 2 * 1024 * 1024;
pub const MAX_IMAGE: u64 = 12 * 1024 * 1024;
pub const MAX_TEXT: u64 = 512 * 1024;
/// Files in one package.
const MAX_FILES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Manifest,
    Css,
    Font,
    Image,
    Note,
}

/// What a file in a package may be, by its name; `None`: not allowed.
fn kind(rel: &str) -> Option<Kind> {
    if rel == MANIFEST {
        return Some(Kind::Manifest);
    }
    let ext = rel.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "css" => Kind::Css,
        "woff2" | "woff" | "ttf" | "otf" => Kind::Font,
        "png" | "jpg" | "jpeg" | "webp" => Kind::Image,
        "md" | "txt" => Kind::Note,
        _ => return None,
    })
}

fn limit(k: Kind) -> u64 {
    match k {
        Kind::Font => MAX_FONT,
        Kind::Image => MAX_IMAGE,
        Kind::Manifest | Kind::Css | Kind::Note => MAX_TEXT,
    }
}

/// A theme id: what its folder is called and how the app names it. Lowercase letters, digits
/// and dashes, so it is the same on every system and safe in a CSS selector.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A file name inside a package: relative, forward slashes, no `..`, nothing hidden (the
/// history is reached only through [`history`]).
fn valid_rel(rel: &str) -> bool {
    if rel.is_empty() || rel.len() > 200 || rel.contains('\\') || rel.contains(':') {
        return false;
    }
    let p = Path::new(rel);
    p.components().all(|c| matches!(c, Component::Normal(_)))
        && rel
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.'))
}

fn package(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err(format!("\"{id}\" isn't a theme name Venuta can use."));
    }
    Ok(root.join(id))
}

fn file_in(root: &Path, id: &str, rel: &str) -> Result<(PathBuf, Kind), String> {
    let k = kind(rel).filter(|_| valid_rel(rel));
    let Some(k) = k else {
        return Err(format!("A theme can't have a file called \"{rel}\"."));
    };
    Ok((package(root, id)?.join(rel), k))
}

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Package {
    pub id: String,
    /// `theme.json` as written (the app checks it); empty when it couldn't be read.
    pub json: String,
    pub files: Vec<FileInfo>,
    /// Why the package couldn't be read, if it couldn't.
    pub error: Option<String>,
}

/// The files of a package (not its history), with their sizes; refuses what a package can't hold.
fn files_of(pkg: &Path) -> Result<Vec<FileInfo>, String> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(pkg).min_depth(1).max_depth(4) {
        let e = e.map_err(|e| e.to_string())?;
        let rel = e
            .path()
            .strip_prefix(pkg)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if rel == HISTORY || rel.starts_with(".history/") || !e.file_type().is_file() {
            continue;
        }
        let size = e.metadata().map_err(|e| e.to_string())?.len();
        out.push(FileInfo { path: rel, size });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Every package in `root`, sorted by id. A package that can't be read is listed with why.
pub fn list(root: &Path) -> Vec<Package> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Package> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            if !valid_id(&id) {
                return None;
            }
            let pkg = e.path();
            let read = || -> Result<(String, Vec<FileInfo>), String> {
                let m = pkg.join(MANIFEST);
                let size = std::fs::metadata(&m)
                    .map_err(|_| "It has no theme.json.".to_string())?
                    .len();
                if size > MAX_TEXT {
                    return Err("Its theme.json is too big to be a theme.".into());
                }
                let json = std::fs::read_to_string(&m).map_err(|e| e.to_string())?;
                Ok((json, files_of(&pkg)?))
            };
            Some(match read() {
                Ok((json, files)) => Package {
                    id,
                    json,
                    files,
                    error: None,
                },
                Err(e) => Package {
                    id,
                    json: String::new(),
                    files: Vec::new(),
                    error: Some(e),
                },
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// One file of a package.
pub fn read(root: &Path, id: &str, rel: &str) -> Result<Vec<u8>, String> {
    let (path, k) = file_in(root, id, rel)?;
    let size = std::fs::metadata(&path)
        .map_err(|_| format!("The theme has no file \"{rel}\"."))?
        .len();
    if size > limit(k) {
        return Err(format!("\"{rel}\" is too big for a theme."));
    }
    std::fs::read(&path).map_err(|e| e.to_string())
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// Write a file whole or not at all (a crash mid-save never leaves half a theme).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Save a theme's `theme.json` (making the package if it's new). The version it replaces is
/// kept in the package's history first.
pub fn save(root: &Path, id: &str, json: &str) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(json).map_err(|e| format!("Not valid JSON: {e}"))?;
    if json.len() as u64 > MAX_TEXT {
        return Err("That theme.json is too big.".into());
    }
    let pkg = package(root, id)?;
    let m = pkg.join(MANIFEST);
    if let Ok(old) = std::fs::read(&m) {
        if old == json.as_bytes() {
            return Ok(());
        }
        let h = pkg.join(HISTORY);
        std::fs::create_dir_all(&h).map_err(|e| e.to_string())?;
        std::fs::write(h.join(format!("{}.json", now_ms())), &old).map_err(|e| e.to_string())?;
    }
    write_atomic(&m, json.as_bytes())
}

/// The kept versions of a theme's `theme.json`, newest first: (when, in ms since 1970; the JSON).
pub fn history(root: &Path, id: &str) -> Result<Vec<(u64, String)>, String> {
    let h = package(root, id)?.join(HISTORY);
    let mut v: Vec<(u64, String)> = std::fs::read_dir(&h)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stamp = name.strip_suffix(".json")?.parse::<u64>().ok()?;
            let text = std::fs::read_to_string(e.path()).ok()?;
            Some((stamp, text))
        })
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.0));
    Ok(v)
}

/// Write a text file into a package (its expert stylesheet, a note).
pub fn write_text(root: &Path, id: &str, rel: &str, text: &str) -> Result<(), String> {
    let (path, k) = file_in(root, id, rel)?;
    if !matches!(k, Kind::Css | Kind::Note) {
        return Err(format!("\"{rel}\" isn't a text file a theme keeps."));
    }
    if text.len() as u64 > MAX_TEXT {
        return Err(format!("\"{rel}\" is too big for a theme."));
    }
    if !package(root, id)?.join(MANIFEST).is_file() {
        return Err("Save the theme first.".into());
    }
    write_atomic(&path, text.as_bytes())
}

/// Copy a font or picture from anywhere into a package, as `rel`. The file it replaces, if
/// any, goes to Set aside.
pub fn add_file(
    root: &Path,
    set_aside: &Path,
    id: &str,
    rel: &str,
    from: &Path,
) -> Result<(), String> {
    let (path, k) = file_in(root, id, rel)?;
    if !matches!(k, Kind::Font | Kind::Image) {
        return Err(format!("\"{rel}\" isn't a font or a picture."));
    }
    let size = std::fs::metadata(from).map_err(|e| e.to_string())?.len();
    if size > limit(k) {
        let mb = limit(k) / (1024 * 1024);
        return Err(format!(
            "That file is bigger than a theme can hold ({mb} MB)."
        ));
    }
    let pkg = package(root, id)?;
    if !pkg.join(MANIFEST).is_file() {
        return Err("Save the theme first.".into());
    }
    let total: u64 = files_of(&pkg)?.iter().map(|f| f.size).sum();
    if total + size > MAX_PACKAGE {
        return Err("The theme would be bigger than 25 MB.".into());
    }
    if path.is_file() {
        aside(set_aside, &path, &format!("{id}-{}", rel.replace('/', "-")))?;
    }
    let bytes = std::fs::read(from).map_err(|e| e.to_string())?;
    write_atomic(&path, &bytes)
}

/// Move a file or folder into `<Set aside>/themes` under a name never taken; returns where.
fn aside(set_aside: &Path, src: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = set_aside.join("themes");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = format!("{}-{name}", now_ms());
    let mut to = dir.join(&base);
    for n in 2.. {
        if !to.exists() {
            break;
        }
        to = dir.join(format!("{base}-{n}"));
    }
    std::fs::rename(src, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

/// Copy a theme as a new one called `to` (its files, not its history); `to` must be free.
pub fn duplicate(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let src = package(root, from)?;
    let dst = package(root, to)?;
    if dst.exists() {
        return Err("A theme with that name is already there.".into());
    }
    let files = files_of(&src)?;
    let tmp = root.join(format!(".copying-{}", now_ms()));
    let r = (|| {
        for f in files
            .iter()
            .filter(|f| kind(&f.path).is_some() && valid_rel(&f.path))
        {
            let to = tmp.join(&f.path);
            if let Some(d) = to.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::copy(src.join(&f.path), &to).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &dst).map_err(|e| e.to_string())
    })();
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    r
}

/// Remove a theme: its whole folder goes to Set aside. Returns where.
pub fn set_aside(root: &Path, set_aside_dir: &Path, id: &str) -> Result<PathBuf, String> {
    let pkg = package(root, id)?;
    if !pkg.is_dir() {
        return Err("That theme isn't there any more.".into());
    }
    aside(set_aside_dir, &pkg, id)
}

/// Something changed in the themes folder since `stamp()` last said this: the newest change
/// time of any file, and how many there are (a removed file changes the count).
pub fn stamp(root: &Path) -> u64 {
    let mut newest = 0u64;
    let mut count = 0u64;
    for e in walkdir::WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .flatten()
    {
        count += 1;
        if let Ok(m) = e
            .metadata()
            .map_err(std::io::Error::other)
            .and_then(|m| m.modified())
            && let Ok(d) = m.duration_since(std::time::UNIX_EPOCH)
        {
            newest = newest.max(d.as_millis() as u64);
        }
    }
    newest.wrapping_mul(31).wrapping_add(count)
}

/// Write a package as one shareable file (a zip of its files, not its history).
pub fn export(root: &Path, id: &str, dest: &Path) -> Result<(), String> {
    let pkg = package(root, id)?;
    let files = files_of(&pkg)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for f in files
            .iter()
            .filter(|f| kind(&f.path).is_some() && valid_rel(&f.path))
        {
            z.start_file(f.path.as_str(), opts)
                .map_err(|e| e.to_string())?;
            let bytes = std::fs::read(pkg.join(&f.path)).map_err(|e| e.to_string())?;
            z.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    write_atomic(dest, buf.get_ref())
}

#[derive(Debug, Clone, Serialize)]
pub struct Inspected {
    pub json: String,
    pub files: Vec<FileInfo>,
    /// Files that were left out, and why.
    pub skipped: Vec<String>,
}

/// Read a shared theme file without installing anything: its `theme.json` and what else it
/// carries. Refuses anything that isn't a theme, or is too big.
pub fn inspect(src: &Path) -> Result<Inspected, String> {
    unpack(src, None)
}

/// Install a shared theme file as `id` (which must not exist yet: to replace, set the old one
/// aside first).
pub fn install(root: &Path, src: &Path, id: &str) -> Result<(), String> {
    let pkg = package(root, id)?;
    if pkg.exists() {
        return Err("A theme with that name is already there.".into());
    }
    // Unpacked beside, then moved into place whole.
    let tmp = root.join(format!(".installing-{}", now_ms()));
    let r = unpack(src, Some(&tmp))
        .and_then(|_| std::fs::rename(&tmp, &pkg).map_err(|e| e.to_string()));
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    r.map(|_| ())
}

fn unpack(src: &Path, into: Option<&Path>) -> Result<Inspected, String> {
    let meta = std::fs::metadata(src).map_err(|e| e.to_string())?;
    if meta.len() > MAX_PACKAGE {
        return Err("That file is bigger than a theme can be (25 MB).".into());
    }
    // A theme.json on its own (written by hand, or by an AI assistant) is a theme too.
    if src
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        if meta.len() > MAX_TEXT {
            return Err("That theme.json is too big.".into());
        }
        let json = std::fs::read_to_string(src).map_err(|_| "That file isn't text.".to_string())?;
        serde_json::from_str::<serde_json::Value>(&json)
            .map_err(|e| format!("That theme.json isn't valid: {e}"))?;
        if let Some(dir) = into {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join(MANIFEST), &json).map_err(|e| e.to_string())?;
        }
        let size = json.len() as u64;
        return Ok(Inspected {
            json,
            files: vec![FileInfo {
                path: MANIFEST.into(),
                size,
            }],
            skipped: Vec::new(),
        });
    }
    let f = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let mut z =
        zip::ZipArchive::new(f).map_err(|_| "That isn't a Venuta theme file.".to_string())?;
    if z.len() > MAX_FILES * 2 {
        return Err("That theme has too many files.".into());
    }
    let mut out = Inspected {
        json: String::new(),
        files: Vec::new(),
        skipped: Vec::new(),
    };
    let names: Vec<String> = z.file_names().map(|n| n.replace('\\', "/")).collect();
    let top = top_folder(&names);
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        if e.is_dir() {
            continue;
        }
        let raw = e.name().replace('\\', "/");
        let name = match &top {
            Some(t) => raw.strip_prefix(t.as_str()).unwrap_or(&raw).to_string(),
            None => raw.clone(),
        };
        let ok = kind(&name).filter(|_| valid_rel(&name));
        let Some(k) = ok else {
            out.skipped
                .push(format!("{raw} (not something a theme can carry)"));
            continue;
        };
        if out.files.len() >= MAX_FILES {
            out.skipped.push(format!("{raw} (too many files)"));
            continue;
        }
        if e.size() > limit(k) {
            out.skipped.push(format!("{raw} (too big)"));
            continue;
        }
        let mut bytes = Vec::new();
        // Never trust the size the zip claims: read at most one byte past the limit.
        (&mut e)
            .take(limit(k) + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > limit(k) {
            out.skipped.push(format!("{raw} (too big)"));
            continue;
        }
        total += bytes.len() as u64;
        if total > MAX_PACKAGE {
            return Err("That theme unpacks to more than 25 MB.".into());
        }
        if k == Kind::Manifest {
            out.json = String::from_utf8(bytes.clone())
                .map_err(|_| "Its theme.json isn't text.".to_string())?;
            serde_json::from_str::<serde_json::Value>(&out.json)
                .map_err(|e| format!("Its theme.json isn't valid: {e}"))?;
        }
        if let Some(dir) = into {
            let to = dir.join(&name);
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::write(&to, &bytes).map_err(|e| e.to_string())?;
        }
        out.files.push(FileInfo {
            path: name,
            size: bytes.len() as u64,
        });
    }
    if out.json.is_empty() {
        return Err("That file has no theme.json, so it isn't a Venuta theme.".into());
    }
    Ok(out)
}

/// A theme zipped by hand often sits in one folder (`dusk/theme.json`, `dusk/art/…`): that
/// folder, with its slash, when everything is inside it and `theme.json` isn't at the top.
fn top_folder(names: &[String]) -> Option<String> {
    if names.iter().any(|n| n == MANIFEST) {
        return None;
    }
    let first = names
        .iter()
        .find(|n| n.ends_with(&format!("/{MANIFEST}")))?;
    let top = format!("{}/", first.strip_suffix(MANIFEST)?.trim_end_matches('/'));
    (top.matches('/').count() == 1 && names.iter().all(|n| n.starts_with(&top))).then_some(top)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn ids_and_names_stay_inside_the_package() {
        assert!(valid_id("midnight-sakura"));
        assert!(!valid_id("Midnight"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("-x"));
        assert!(!valid_id(""));
        assert!(valid_rel("art/blossom.jpg"));
        assert!(valid_rel("theme.json"));
        assert!(!valid_rel("../x.png"));
        assert!(!valid_rel("art/../../x.png"));
        assert!(!valid_rel("/abs.png"));
        assert!(!valid_rel(r"C:\x.png"));
        assert!(!valid_rel(r"art\x.png"));
        assert!(!valid_rel(".history/1.json"));
        assert!(!valid_rel("art//x.png"));
        assert_eq!(kind("x.exe"), None);
        assert_eq!(kind("x.js"), None);
        assert_eq!(kind("fonts/A.WOFF2"), Some(Kind::Font));
    }

    #[test]
    fn save_keeps_the_version_before_and_lists_it() {
        let t = temp();
        let root = t.path();
        save(root, "dusk", r#"{"format":1,"name":"One"}"#).unwrap();
        save(root, "dusk", r#"{"format":1,"name":"Two"}"#).unwrap();
        // The same again changes nothing and keeps no copy.
        save(root, "dusk", r#"{"format":1,"name":"Two"}"#).unwrap();
        let l = list(root);
        assert_eq!(l.len(), 1);
        assert!(l[0].json.contains("Two"));
        assert_eq!(l[0].files.len(), 1, "history is not a file of the theme");
        let h = history(root, "dusk").unwrap();
        assert_eq!(h.len(), 1);
        assert!(h[0].1.contains("One"));
        assert!(save(root, "dusk", "not json").is_err());
        assert!(save(root, "../dusk", "{}").is_err());
    }

    #[test]
    fn files_are_checked_on_the_way_in_and_out() {
        let t = temp();
        let root = t.path().join("themes");
        let aside_dir = t.path().join("Set aside");
        save(&root, "dusk", "{}").unwrap();
        write_text(&root, "dusk", "extra.css", ":root{}").unwrap();
        assert!(write_text(&root, "dusk", "x.js", "alert(1)").is_err());
        assert!(write_text(&root, "dusk", "../escape.css", "").is_err());
        assert!(
            write_text(&root, "nope", "extra.css", "").is_err(),
            "no theme, no files"
        );
        let img = t.path().join("pic.png");
        std::fs::write(&img, b"png").unwrap();
        add_file(&root, &aside_dir, "dusk", "art/bg.png", &img).unwrap();
        std::fs::write(&img, b"png2").unwrap();
        add_file(&root, &aside_dir, "dusk", "art/bg.png", &img).unwrap();
        assert_eq!(read(&root, "dusk", "art/bg.png").unwrap(), b"png2");
        assert_eq!(
            std::fs::read_dir(aside_dir.join("themes")).unwrap().count(),
            1,
            "the replaced picture was kept"
        );
        assert!(read(&root, "dusk", "../../pic.png").is_err());
        let big = t.path().join("big.woff2");
        std::fs::write(&big, vec![0u8; (MAX_FONT + 1) as usize]).unwrap();
        assert!(add_file(&root, &aside_dir, "dusk", "fonts/big.woff2", &big).is_err());
    }

    #[test]
    fn export_then_install_round_trips_and_set_aside_keeps_it() {
        let t = temp();
        let root = t.path().join("themes");
        let aside_dir = t.path().join("Set aside");
        save(&root, "dusk", r#"{"format":1,"id":"dusk"}"#).unwrap();
        save(&root, "dusk", r#"{"format":1,"id":"dusk","name":"Dusk"}"#).unwrap();
        write_text(&root, "dusk", "extra.css", ".x{}").unwrap();
        let file = t.path().join("Dusk.venuta-theme");
        export(&root, "dusk", &file).unwrap();
        let i = inspect(&file).unwrap();
        assert!(i.json.contains("Dusk"));
        assert_eq!(
            i.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["extra.css", "theme.json"]
        );
        assert!(
            install(&root, &file, "dusk").is_err(),
            "never over an existing theme"
        );
        install(&root, &file, "dusk-2").unwrap();
        assert_eq!(read(&root, "dusk-2", "extra.css").unwrap(), b".x{}");
        assert!(
            history(&root, "dusk-2").unwrap().is_empty(),
            "history isn't shared"
        );
        duplicate(&root, "dusk", "dusk-copy").unwrap();
        assert!(duplicate(&root, "dusk", "dusk-copy").is_err());
        assert_eq!(read(&root, "dusk-copy", "extra.css").unwrap(), b".x{}");
        assert!(history(&root, "dusk-copy").unwrap().is_empty());
        let went = set_aside(&root, &aside_dir, "dusk").unwrap();
        assert!(went.join("theme.json").is_file());
        assert_eq!(list(&root).len(), 2);
    }

    #[test]
    fn a_hostile_theme_file_is_refused_or_trimmed() {
        let t = temp();
        let file = t.path().join("bad.venuta-theme");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("theme.json", b"{\"format\":1}".as_slice()),
            ("../../evil.css", b"x"),
            ("run.exe", b"MZ"),
            ("script.js", b"x"),
            ("ok.css", b"a{}"),
        ] {
            z.start_file(name, o).unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap();
        let i = inspect(&file).unwrap();
        assert_eq!(
            i.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["theme.json", "ok.css"]
        );
        assert_eq!(i.skipped.len(), 3);
        let root = t.path().join("themes");
        install(&root, &file, "bad").unwrap();
        assert!(!t.path().join("evil.css").exists());
        assert!(!root.join("evil.css").exists());

        let bare = t.path().join("theme.json");
        std::fs::write(&bare, r#"{"format":1,"name":"Bare"}"#).unwrap();
        assert_eq!(inspect(&bare).unwrap().files.len(), 1);
        install(&root, &bare, "bare").unwrap();
        assert!(read(&root, "bare", "theme.json").is_ok());
        std::fs::write(&bare, "{oops").unwrap();
        assert!(inspect(&bare).is_err());

        let not_zip = t.path().join("x.venuta-theme");
        std::fs::write(&not_zip, b"hello").unwrap();
        assert!(inspect(&not_zip).is_err());

        let no_manifest = t.path().join("y.venuta-theme");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&no_manifest).unwrap());
        z.start_file("a.css", o).unwrap();
        z.finish().unwrap();
        assert!(inspect(&no_manifest).is_err());
    }
}

#[cfg(test)]
mod top_folder_tests {
    use super::top_folder;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_theme_in_one_top_folder_is_read_from_inside_it() {
        assert_eq!(
            top_folder(&v(&["dusk/", "dusk/theme.json", "dusk/art/a.png"])),
            Some("dusk/".into())
        );
        // At the top already, or spread over folders: names stay as they are.
        assert_eq!(top_folder(&v(&["theme.json", "art/a.png"])), None);
        assert_eq!(top_folder(&v(&["dusk/theme.json", "other/a.png"])), None);
        assert_eq!(top_folder(&v(&["a/b/theme.json", "a/b/x.css"])), None);
    }
}
