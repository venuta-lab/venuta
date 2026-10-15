//! Full-length stream resolve via an external maintainer tool (`yt-dlp`).
//!
//! Catalog search stays on InnerTube. Stream unlock is delegated to a single
//! replaceable binary/module so future YouTube changes are fixed by updating
//! that tool (and optional local pot helper), not product layers.
//!
//! Discovery order (first hit wins):
//! 1. `VENUTA_STREAM_RESOLVER` — absolute path to a yt-dlp-compatible binary
//! 2. Next to the Venuta executable / `resources/` (release bundle)
//! 3. `%APPDATA%\Venuta\bin\yt-dlp.exe` (auto-downloaded if missing)
//! 4. `yt-dlp` on `PATH`
//! 5. `python` / `python3` / `py` with `-m yt_dlp`

use crate::access::{AccessError, AccessErrorKind};
use crate::types::{EphemeralAudio, MediaHeader};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const PROVIDER: &str = "remote_catalog";

#[derive(Debug, Deserialize)]
struct DumpJson {
    id: Option<String>,
    url: Option<String>,
    #[serde(default)]
    http_headers: serde_json::Map<String, serde_json::Value>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    duration: Option<f64>,
    #[serde(default)]
    acodec: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    format_id: Option<String>,
}

/// Resolve direct session audio for `track_id` using the configured stream tool.
pub fn resolve_with_stream_tool(track_id: &str) -> Result<EphemeralAudio, AccessError> {
    let track_id = track_id.trim();
    if track_id.is_empty() {
        return Err(AccessError::new(
            AccessErrorKind::InvalidInput,
            "track id must not be empty",
        ));
    }
    let launcher = find_stream_tool()
        .or_else(|| {
            ensure_stream_tool().ok().and_then(|_| find_stream_tool())
        })
        .ok_or_else(|| {
            AccessError::new(
                AccessErrorKind::Unsupported,
                "stream resolver is not installed (Venuta will try to download yt-dlp on first play; or set VENUTA_STREAM_RESOLVER)",
            )
        })?;

    let watch_url = format!("https://www.youtube.com/watch?v={track_id}");
    // Prefer AAC/MP4 audio; fall back to best audio.
    let format = "ba[ext=m4a]/ba[acodec^=mp4a]/ba/b";
    let output = run_dump_json(&launcher, &watch_url, format)?;
    let dump: DumpJson = serde_json::from_slice(&output).map_err(|_| {
        AccessError::new(
            AccessErrorKind::MalformedResponse,
            "stream resolver returned unreadable metadata",
        )
    })?;

    let url = dump.url.filter(|u| !u.is_empty()).ok_or_else(|| {
        AccessError::new(
            AccessErrorKind::Unsupported,
            "stream resolver did not return a playable URL",
        )
    })?;

    let mut headers: Vec<MediaHeader> = dump
        .http_headers
        .into_iter()
        .filter_map(|(name, value)| {
            let value = value.as_str()?.to_owned();
            if value.is_empty() {
                None
            } else {
                Some(MediaHeader { name, value })
            }
        })
        .collect();
    // Ensure Accept is present for ranged media GETs.
    if !headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("accept"))
    {
        headers.push(MediaHeader {
            name: "Accept".into(),
            value: "*/*".into(),
        });
    }

    let ext = dump.ext.as_deref().unwrap_or_default();
    let acodec = dump.acodec.as_deref().unwrap_or_default();
    let mime_type = if ext == "m4a" || acodec.starts_with("mp4a") {
        Some("audio/mp4; codecs=\"mp4a.40.2\"".into())
    } else if ext == "webm" || acodec.contains("opus") {
        Some("audio/webm; codecs=\"opus\"".into())
    } else if !ext.is_empty() {
        Some(format!("audio/{ext}"))
    } else {
        None
    };

    let content_length = dump.filesize.or(dump.filesize_approx);
    let duration_ms = dump.duration.map(|s| (s * 1000.0).round() as u64);

    let _ = dump.format_id; // kept for future diagnostics without logging URLs
    Ok(EphemeralAudio {
        track_id: dump.id.unwrap_or_else(|| track_id.to_owned()),
        provider: PROVIDER.into(),
        url,
        mime_type,
        content_length,
        duration_ms,
        headers,
    })
}

#[derive(Debug, Clone)]
enum StreamTool {
    Binary(PathBuf),
    PythonModule { python: PathBuf },
}

/// How the stream tool was found (for Settings health). Never includes media URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamToolStatus {
    pub available: bool,
    /// Short label: "yt-dlp", "python -m yt_dlp", env path basename, or empty if missing.
    pub kind: String,
    /// Safe display path (tool binary or python), empty if missing.
    pub detail: String,
}

/// Probe whether a stream resolver is installed (no network; does not download).
pub fn stream_tool_status() -> StreamToolStatus {
    match find_stream_tool() {
        Some(StreamTool::Binary(path)) => StreamToolStatus {
            available: true,
            kind: binary_tool_kind(&path),
            detail: path.display().to_string(),
        },
        Some(StreamTool::PythonModule { python }) => StreamToolStatus {
            available: true,
            kind: "python -m yt_dlp".into(),
            detail: python.display().to_string(),
        },
        None => StreamToolStatus {
            available: false,
            kind: String::new(),
            detail: String::new(),
        },
    }
}

fn find_stream_tool() -> Option<StreamTool> {
    if let Ok(path) = std::env::var("VENUTA_STREAM_RESOLVER") {
        let path = path.trim();
        if !path.is_empty() && Path::new(path).is_file() {
            return Some(StreamTool::Binary(PathBuf::from(path)));
        }
    }
    for path in bundled_and_managed_binaries() {
        if path.is_file() {
            return Some(StreamTool::Binary(path));
        }
    }
    if let Some(bin) = which("yt-dlp") {
        return Some(StreamTool::Binary(bin));
    }
    for py in ["python3", "python", "py"] {
        if let Some(python) = which(py)
            && python_module_available(&python, "yt_dlp")
        {
            return Some(StreamTool::PythonModule { python });
        }
    }
    // Common Windows install from the python.org store layout.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local)
            .join("Python")
            .join("bin")
            .join("python3.exe");
        if candidate.is_file() && python_module_available(&candidate, "yt_dlp") {
            return Some(StreamTool::PythonModule { python: candidate });
        }
    }
    None
}

/// Paths where a release or first-run install may place a standalone yt-dlp binary.
fn bundled_and_managed_binaries() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        // Sidecar next to Venuta.exe (NSIS / portable).
        out.push(dir.join(stream_tool_filename()));
        out.push(dir.join("bin").join(stream_tool_filename()));
        // Tauri `resources/` folder beside the executable.
        out.push(dir.join("resources").join(stream_tool_filename()));
        out.push(
            dir.join("resources")
                .join("bin")
                .join(stream_tool_filename()),
        );
    }
    if let Some(managed) = managed_tool_path() {
        out.push(managed);
    }
    out
}

fn stream_tool_filename() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

fn binary_tool_kind(path: &Path) -> String {
    if std::env::var_os("VENUTA_STREAM_RESOLVER").is_some() {
        return "VENUTA_STREAM_RESOLVER".into();
    }
    let path_s = path.to_string_lossy();
    if path_s.contains("Venuta") && path_s.contains("bin") {
        return "managed yt-dlp".into();
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && path.starts_with(dir)
    {
        return "bundled yt-dlp".into();
    }
    "yt-dlp".into()
}

/// Per-user managed install (works before a library exists; survives app updates).
fn managed_tool_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?
    };
    Some(base.join("Venuta").join("bin").join(stream_tool_filename()))
}

/// Official standalone binary (no Python required). Updated by re-running ensure.
#[cfg(windows)]
const YT_DLP_DOWNLOAD: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
#[cfg(not(windows))]
const YT_DLP_DOWNLOAD: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";

/// If no stream tool is found, download the official standalone binary into the user data dir.
/// Safe to call from a background thread; used automatically on first online play.
/// Returns the path to the binary (or the python used with `-m yt_dlp`).
pub fn ensure_stream_tool() -> Result<PathBuf, AccessError> {
    if let Some(existing) = find_stream_tool() {
        return Ok(match existing {
            StreamTool::Binary(p) => p,
            StreamTool::PythonModule { python } => python,
        });
    }
    let dest = managed_tool_path().ok_or_else(|| {
        AccessError::new(
            AccessErrorKind::Unsupported,
            "could not determine where to install the stream tool",
        )
    })?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|_| {
            AccessError::new(
                AccessErrorKind::Transport,
                "could not create the stream tool folder",
            )
        })?;
    }
    download_stream_tool(&dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
    }
    if dest.is_file() {
        Ok(dest)
    } else {
        Err(AccessError::new(
            AccessErrorKind::Transport,
            "stream tool download finished but the file is missing",
        ))
    }
}

fn download_stream_tool(dest: &Path) -> Result<(), AccessError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into();
    let response = agent.get(YT_DLP_DOWNLOAD).call().map_err(|_| {
        AccessError::new(
            AccessErrorKind::Transport,
            "could not download the stream tool (check your network)",
        )
    })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(AccessError::new(
            AccessErrorKind::Transport,
            format!("stream tool download returned HTTP {status}"),
        ));
    }
    let bytes = response.into_body().read_to_vec().map_err(|_| {
        AccessError::new(
            AccessErrorKind::Transport,
            "could not read the stream tool download",
        )
    })?;
    if bytes.len() < 1_000_000 {
        // Real yt-dlp.exe is several MB; reject HTML error pages.
        return Err(AccessError::new(
            AccessErrorKind::MalformedResponse,
            "stream tool download looked incomplete",
        ));
    }
    let tmp = dest.with_extension("download");
    fs::write(&tmp, &bytes).map_err(|_| {
        AccessError::new(AccessErrorKind::Transport, "could not save the stream tool")
    })?;
    fs::rename(&tmp, dest).map_err(|_| {
        AccessError::new(
            AccessErrorKind::Transport,
            "could not install the stream tool",
        )
    })?;
    Ok(())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
            let cmd = dir.join(format!("{name}.cmd"));
            if cmd.is_file() {
                return Some(cmd);
            }
        }
    }
    None
}

fn python_module_available(python: &Path, module: &str) -> bool {
    Command::new(python)
        .args(["-c", &format!("import {module}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_dump_json(tool: &StreamTool, watch_url: &str, format: &str) -> Result<Vec<u8>, AccessError> {
    let mut cmd = match tool {
        StreamTool::Binary(path) => {
            let mut c = Command::new(path);
            c.args([
                "-f",
                format,
                "--no-download",
                "--no-warnings",
                "--dump-single-json",
                watch_url,
            ]);
            c
        }
        StreamTool::PythonModule { python } => {
            let mut c = Command::new(python);
            c.args([
                "-m",
                "yt_dlp",
                "-f",
                format,
                "--no-download",
                "--no-warnings",
                "--dump-single-json",
                watch_url,
            ]);
            c
        }
    };
    // Give BotGuard/pot helpers time when present.
    cmd.stdin(Stdio::null());
    let output = cmd.output().map_err(|_| {
        AccessError::new(
            AccessErrorKind::Transport,
            "could not start the stream resolver",
        )
    })?;
    if !output.status.success() {
        return Err(AccessError::new(
            AccessErrorKind::Transport,
            "stream resolver failed to resolve audio",
        ));
    }
    if output.stdout.is_empty() {
        return Err(AccessError::new(
            AccessErrorKind::MalformedResponse,
            "stream resolver returned empty output",
        ));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_stream_tool_does_not_panic() {
        let _ = find_stream_tool();
    }
}
