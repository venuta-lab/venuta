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

    refresh_managed_tool_when_old();
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
        via: "stream tool",
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
        refresh_managed_tool_when_old();
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

/// A console tool started without a window of its own. Venuta has no console to lend it, so
/// without this Windows opens a terminal window every time an online song starts.
fn quiet(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn python_module_available(python: &Path, module: &str) -> bool {
    quiet(Command::new(python))
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
            let mut c = quiet(Command::new(path));
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
            let mut c = quiet(Command::new(python));
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
    cmd.stdin(Stdio::null());
    let output = output_within(cmd, TOOL_TIME_LIMIT)?;
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

/// A copy of the stream tool that Venuta installed is replaced with the newest after this long:
/// the catalog changes often, and an old copy is the usual reason the tool stops working.
const TOOL_FRESH_FOR: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Replace Venuta's own copy of the stream tool when it is old, in the background and at most once
/// per run. A copy the user installed (on PATH, a Python module, `VENUTA_STREAM_RESOLVER`) is
/// theirs to update and is left alone. The new copy goes in only once fully downloaded.
fn refresh_managed_tool_when_old() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CHECKED: AtomicBool = AtomicBool::new(false);
    if CHECKED.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(path) = managed_tool_path() else {
        return;
    };
    let old = fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > TOOL_FRESH_FOR);
    if old {
        let _ = std::thread::Builder::new()
            .name("venuta-stream-tool-update".into())
            .spawn(move || {
                let _ = download_stream_tool(&path);
            });
    }
}

/// How long the stream tool may take for one song before it is stopped.
const TOOL_TIME_LIMIT: Duration = Duration::from_secs(30);

/// Run `cmd` to the end, or stop it after `limit`: a stuck tool must not hold a song (and the
/// thread waiting on it) forever.
fn output_within(mut cmd: Command, limit: Duration) -> Result<std::process::Output, AccessError> {
    use std::io::Read;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            AccessError::new(
                AccessErrorKind::Transport,
                "could not start the stream resolver",
            )
        })?;
    // Read its output beside the wait, so a full pipe can never stall it.
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut out);
        }
        out
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AccessError::new(
                    AccessErrorKind::Transport,
                    "the stream resolver took too long",
                ));
            }
        }
    };
    let stdout = reader.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_that_takes_too_long_is_stopped_at_the_limit() {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "6", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("5");
            c
        };
        cmd.stdin(Stdio::null());
        let started = std::time::Instant::now();
        let result = output_within(cmd, Duration::from_millis(400));
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "stopped, not waited out"
        );
    }

    #[test]
    fn a_quick_tool_gives_its_output() {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", "echo hello"]);
            c
        } else {
            let mut c = Command::new("echo");
            c.arg("hello");
            c
        };
        cmd.stdin(Stdio::null());
        let out = output_within(cmd, Duration::from_secs(10)).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }

    #[test]
    fn find_stream_tool_does_not_panic() {
        let _ = find_stream_tool();
    }
}
