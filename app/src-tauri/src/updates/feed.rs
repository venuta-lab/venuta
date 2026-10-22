//! Where a new version is announced and whether it can be used: the manifest (latest.json) read
//! before the updater reads it, with timeouts, retries and a second address, so a failure says
//! what went wrong in plain words. No Tauri here; tests run it against a local server.

use semver::Version;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// What every installed copy asks: the manifest of the release GitHub calls latest.
pub const PRIMARY: &str =
    "https://github.com/venuta-lab/venuta/releases/latest/download/latest.json";
/// The second way to the same file: GitHub's list of releases, read for their latest.json.
pub const RELEASES_API: &str =
    "https://api.github.com/repos/venuta-lab/venuta/releases?per_page=30";
/// Where the installer can be downloaded by hand.
pub const RELEASES_PAGE: &str = "https://github.com/venuta-lab/venuta/releases/latest";

/// Waits between tries of one address, for network trouble and busy servers.
pub const WAITS: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(5)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    /// Pre-releases too (Settings › System › Pre-releases).
    Beta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A manifest's address.
    Manifest(String),
    /// GitHub's releases list, to find the newest release's manifest.
    Releases(String),
}

/// The addresses to try, in order.
pub fn sources(channel: Channel, primary: &str, releases: &str) -> Vec<Source> {
    let (m, r) = (
        Source::Manifest(primary.to_owned()),
        Source::Releases(releases.to_owned()),
    );
    match channel {
        Channel::Stable => vec![m, r],
        Channel::Beta => vec![r, m],
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    LocalOnly,
    UpToDate,
    Network,
    Timeout,
    Busy,
    NotPublished,
    Broken,
    NoWindows,
    WrongKey,
    Stalled,
    Signature,
    Folder,
    Installer,
}

/// Why there is no update to offer or install, in words for the person using Venuta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub kind: Kind,
    pub message: String,
}

impl Problem {
    pub fn new(kind: Kind, detail: impl AsRef<str>) -> Self {
        let d = detail.as_ref();
        let message = match kind {
            Kind::LocalOnly => {
                "Local only is on, so Venuta doesn't go online to check for updates.".to_owned()
            }
            Kind::UpToDate => "Venuta is already up to date.".to_owned(),
            Kind::Network => "Couldn't reach GitHub to check for updates. Check the internet \
                              connection, or the proxy in Settings, and try again."
                .to_owned(),
            Kind::Timeout => "GitHub didn't answer in time. Try again in a moment.".to_owned(),
            Kind::Busy => format!("GitHub is busy right now ({d}). Try again in a few minutes."),
            Kind::NotPublished => "GitHub has no update file for the newest release yet. Try \
                                   again later, or download the installer."
                .to_owned(),
            Kind::Broken => format!(
                "The newest release's update file can't be used ({d}). Download the installer \
                 instead."
            ),
            Kind::NoWindows => "The newest release has no update for Windows. Download the \
                                installer instead."
                .to_owned(),
            Kind::WrongKey => "The newest release isn't signed with Venuta's key, so it won't be \
                               installed. Download the installer from GitHub instead."
                .to_owned(),
            Kind::Stalled => {
                "The download stopped. Check the internet connection and try again.".to_owned()
            }
            Kind::Signature => "The download didn't match Venuta's signature, so it was not \
                                installed. Try again, or download the installer."
                .to_owned(),
            Kind::Folder => format!(
                "Venuta can't update itself in {d} without administrator rights. Download the \
                 installer and run it."
            ),
            Kind::Installer => format!(
                "Windows couldn't start the installer ({d}). Download the installer and run it."
            ),
        };
        Self { kind, message }
    }

    /// Worth asking again in a moment.
    pub fn transient(&self) -> bool {
        matches!(
            self.kind,
            Kind::Network | Kind::Timeout | Kind::Busy | Kind::Stalled
        )
    }

    /// How much a problem says: when every address fails, the most telling one is shown (a wrong
    /// key matters more than a busy server).
    fn rank(&self) -> u8 {
        match self.kind {
            Kind::Stalled | Kind::Folder | Kind::Installer | Kind::LocalOnly | Kind::UpToDate => 0,
            Kind::Network => 1,
            Kind::Timeout => 2,
            Kind::Busy => 3,
            Kind::NotPublished => 4,
            Kind::Broken | Kind::NoWindows => 5,
            Kind::WrongKey | Kind::Signature => 6,
        }
    }
}

/// A newer version, as the manifest announces it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Offer {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
    /// The installer's address.
    pub url: String,
}

/// An HTTP client for the manifest: its own timeout, Venuta's User-Agent, the proxy if any.
pub fn agent(timeout: Duration, proxy: Option<&str>) -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(engine::net::USER_AGENT)
        .http_status_as_error(false);
    // Without one, ureq uses the proxy variables Venuta sets at start.
    if let Some(p) = proxy.and_then(|p| ureq::Proxy::new(p).ok()) {
        config = config.proxy(Some(p));
    }
    config.build().into()
}

/// One GET, its answer sorted into the problems above.
pub fn get(agent: &ureq::Agent, url: &str) -> Result<String, Problem> {
    let mut r = agent
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| from_ureq(&e))?;
    match r.status().as_u16() {
        200..=299 => r.body_mut().read_to_string().map_err(|e| from_ureq(&e)),
        404 => Err(Problem::new(Kind::NotPublished, "")),
        // GitHub's rate limit answers 403 or 429.
        s @ (403 | 429 | 500..=599) => Err(Problem::new(Kind::Busy, format!("HTTP {s}"))),
        s => Err(Problem::new(Kind::Broken, format!("HTTP {s}"))),
    }
}

fn from_ureq(e: &ureq::Error) -> Problem {
    match e {
        ureq::Error::Timeout(_) => Problem::new(Kind::Timeout, ""),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            Problem::new(Kind::Timeout, "")
        }
        e => Problem::new(Kind::Network, e.to_string()),
    }
}

/// Run `f` again after each wait while it fails with a problem worth retrying.
pub fn with_retries<T>(
    waits: &[Duration],
    sleep: &dyn Fn(Duration),
    mut f: impl FnMut() -> Result<T, Problem>,
) -> Result<T, Problem> {
    let mut tries = 0;
    loop {
        match f() {
            Err(p) if p.transient() && tries < waits.len() => {
                sleep(waits[tries]);
                tries += 1;
            }
            r => return r,
        }
    }
}

/// Of GitHub's releases list, the newest published release's latest.json (pre-releases only on
/// the beta channel), by version rather than by list order.
pub fn pick_release(list: &str, channel: Channel) -> Result<String, Problem> {
    let broken = || Problem::new(Kind::Broken, "GitHub's list of releases");
    let releases: serde_json::Value = serde_json::from_str(list).map_err(|_| broken())?;
    let releases = releases.as_array().ok_or_else(broken)?;
    releases
        .iter()
        .filter_map(|r| {
            let flag = |k: &str| r.get(k).and_then(serde_json::Value::as_bool) == Some(true);
            let tag = r.get("tag_name")?.as_str()?;
            let version: Version = tag.trim_start_matches('v').parse().ok()?;
            let pre = flag("prerelease") || !version.pre.is_empty();
            if flag("draft") || (channel == Channel::Stable && pre) {
                return None;
            }
            let url = r.get("assets")?.as_array()?.iter().find_map(|a| {
                (a.get("name")?.as_str()? == "latest.json")
                    .then(|| a.get("browser_download_url")?.as_str().map(str::to_owned))
                    .flatten()
            })?;
            Some((version, url))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, url)| url)
        .ok_or_else(|| Problem::new(Kind::NotPublished, ""))
}

/// What the updater looks for in `platforms`, in its order.
pub fn platform_keys() -> Vec<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let arch = std::env::consts::ARCH;
    let mut keys = Vec::new();
    if os == "windows" {
        keys.push(format!("{os}-{arch}-nsis"));
    }
    keys.push(format!("{os}-{arch}"));
    keys
}

/// The key id (8 bytes) in a public key or a signature as Tauri writes them: base64 of
/// minisign's text, whose second line is base64 of the algorithm (2 bytes), the key id and more.
fn key_id(tauri_b64: &str) -> Option<[u8; 8]> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let text = String::from_utf8(b64.decode(tauri_b64.trim()).ok()?).ok()?;
    let bin = b64.decode(text.lines().nth(1)?.trim()).ok()?;
    bin.get(2..10)?.try_into().ok()
}

/// A newer version, if the manifest announces one that this copy may install: newer, on this
/// channel, with this platform, signed by the key built into Venuta.
pub fn read_manifest(
    json: &str,
    current: &Version,
    channel: Channel,
    pubkey: &str,
) -> Result<Option<Offer>, Problem> {
    let m: serde_json::Value =
        serde_json::from_str(json).map_err(|_| Problem::new(Kind::Broken, "it isn't JSON"))?;
    let version: Version = m
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(|v| v.trim_start_matches('v').parse().ok())
        .ok_or_else(|| Problem::new(Kind::Broken, "its version"))?;
    if version <= *current || (channel == Channel::Stable && !version.pre.is_empty()) {
        return Ok(None);
    }
    let platforms = m.get("platforms");
    let (url, signature) = platform_keys()
        .iter()
        .find_map(|k| {
            let p = platforms?.get(k)?;
            Some((p.get("url")?.as_str()?, p.get("signature")?.as_str()?))
        })
        .ok_or_else(|| Problem::new(Kind::NoWindows, ""))?;
    let local = url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:");
    if !url.starts_with("https://") && !local {
        return Err(Problem::new(Kind::Broken, "its address"));
    }
    let signed_by = key_id(signature).ok_or_else(|| Problem::new(Kind::Broken, "its signature"))?;
    if key_id(pubkey) != Some(signed_by) {
        return Err(Problem::new(Kind::WrongKey, ""));
    }
    let text = |k: &str| {
        m.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Ok(Some(Offer {
        version: version.to_string(),
        notes: text("notes"),
        date: text("pub_date"),
        url: url.to_owned(),
    }))
}

/// Each source in turn until one answers; the most telling problem if none does.
pub fn find(
    sources: &[Source],
    fetch: &dyn Fn(&str) -> Result<String, Problem>,
    current: &Version,
    channel: Channel,
    pubkey: &str,
) -> Result<Option<(String, Offer)>, Problem> {
    let mut worst: Option<Problem> = None;
    for source in sources {
        let found = (|| -> Result<Option<(String, Offer)>, Problem> {
            let url = match source {
                Source::Manifest(u) => u.clone(),
                Source::Releases(list) => pick_release(&fetch(list)?, channel)?,
            };
            let offer = read_manifest(&fetch(&url)?, current, channel, pubkey)?;
            Ok(offer.map(|o| (url, o)))
        })();
        match found {
            Ok(answer) => return Ok(answer),
            Err(p) => {
                if worst.as_ref().is_none_or(|w| p.rank() > w.rank()) {
                    worst = Some(p);
                }
            }
        }
    }
    Err(worst.unwrap_or_else(|| Problem::new(Kind::NotPublished, "")))
}

/// The installer's `/D=` for the folder Venuta runs from, when that folder is an installed
/// Venuta (it holds the uninstaller). NSIS wants it last and unquoted.
pub fn install_dir_arg(exe: &Path) -> Option<String> {
    let dir = exe.parent()?;
    dir.join("uninstall.exe")
        .is_file()
        .then(|| format!("/D={}", dir.display()))
}

/// Whether Venuta can write in its own folder (the installer runs without administrator rights).
pub fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".venuta-update-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Remove installers earlier updates left in the temp folder (`Venuta-<version>-updater-*`),
/// once they are older than `age` (the newest may still be running).
pub fn clean_old_installers(temp: &Path, age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("Venuta-") && name.contains("-updater-")
        })
        .filter(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|d| d >= age))
        })
        .filter(|e| std::fs::remove_dir_all(e.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::mock::{self, Reply, TestKey};
    use std::cell::RefCell;

    const SETUP: &str = "Venuta_0.4.0_x64-setup.exe";

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    fn offer_json(key: &TestKey, version: &str) -> String {
        let url = format!(
            "https://github.com/venuta-lab/venuta/releases/download/v{version}/Venuta_{version}_x64-setup.exe"
        );
        mock::manifest(
            version,
            &url,
            &key.sign(
                b"installer",
                &format!("Venuta_{version}_x64-setup.exe"),
                version,
            ),
        )
    }

    #[test]
    fn stable_asks_the_release_address_first_then_the_list_beta_the_other_way() {
        assert_eq!(
            sources(Channel::Stable, "p", "r"),
            vec![Source::Manifest("p".into()), Source::Releases("r".into())]
        );
        assert_eq!(
            sources(Channel::Beta, "p", "r"),
            vec![Source::Releases("r".into()), Source::Manifest("p".into())]
        );
    }

    #[test]
    fn a_newer_version_is_offered_with_its_installer() {
        let key = TestKey::new();
        let o = read_manifest(
            &offer_json(&key, "0.4.0"),
            &v("0.3.0"),
            Channel::Stable,
            &key.public(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(o.version, "0.4.0");
        assert!(o.url.ends_with(SETUP));
        assert_eq!(o.notes.as_deref(), Some("What is new."));
    }

    #[test]
    fn the_same_or_an_older_version_is_up_to_date() {
        let key = TestKey::new();
        for current in ["0.4.0", "0.4.1", "1.0.0"] {
            let r = read_manifest(
                &offer_json(&key, "0.4.0"),
                &v(current),
                Channel::Stable,
                &key.public(),
            );
            assert_eq!(r, Ok(None), "{current}");
        }
    }

    #[test]
    fn versions_compare_as_semver_not_as_text() {
        let key = TestKey::new();
        let r = read_manifest(
            &offer_json(&key, "0.10.0"),
            &v("0.9.0"),
            Channel::Stable,
            &key.public(),
        );
        assert!(matches!(r, Ok(Some(_))));
        // A release is newer than its own pre-releases.
        let r = read_manifest(
            &offer_json(&key, "0.4.0"),
            &v("0.4.0-beta.2"),
            Channel::Beta,
            &key.public(),
        );
        assert!(matches!(r, Ok(Some(_))));
    }

    #[test]
    fn a_pre_release_version_is_offered_only_on_the_beta_channel() {
        let key = TestKey::new();
        let json = offer_json(&key, "0.4.0-beta.1");
        assert_eq!(
            read_manifest(&json, &v("0.3.0"), Channel::Stable, &key.public()),
            Ok(None)
        );
        assert!(matches!(
            read_manifest(&json, &v("0.3.0"), Channel::Beta, &key.public()),
            Ok(Some(_))
        ));
    }

    #[test]
    fn the_nsis_platform_is_used_first_and_the_plain_one_is_enough() {
        assert_eq!(
            platform_keys()[0],
            format!("windows-{}-nsis", std::env::consts::ARCH)
        );
        let key = TestKey::new();
        let mut m: serde_json::Value = serde_json::from_str(&offer_json(&key, "0.4.0")).unwrap();
        m["platforms"]
            .as_object_mut()
            .unwrap()
            .remove("windows-x86_64-nsis");
        let r = read_manifest(&m.to_string(), &v("0.3.0"), Channel::Stable, &key.public());
        assert!(matches!(r, Ok(Some(_))));
    }

    #[test]
    fn a_manifest_that_cannot_be_used_says_why() {
        let key = TestKey::new();
        let pk = key.public();
        let cur = v("0.3.0");
        let kind = |json: &str| {
            read_manifest(json, &cur, Channel::Stable, &pk)
                .unwrap_err()
                .kind
        };
        assert_eq!(kind("<html>"), Kind::Broken);
        assert_eq!(kind(r#"{"version":"next","platforms":{}}"#), Kind::Broken);
        assert_eq!(
            kind(
                r#"{"version":"0.4.0","platforms":{"darwin-aarch64":{"url":"https://x","signature":"y"}}}"#
            ),
            Kind::NoWindows
        );
        let other = TestKey::new();
        assert_eq!(kind(&offer_json(&other, "0.4.0")), Kind::WrongKey);
        let mut m: serde_json::Value = serde_json::from_str(&offer_json(&key, "0.4.0")).unwrap();
        for p in ["windows-x86_64-nsis", "windows-x86_64"] {
            m["platforms"][p]["signature"] = "not base64 at all".into();
        }
        assert_eq!(kind(&m.to_string()), Kind::Broken);
        for p in ["windows-x86_64-nsis", "windows-x86_64"] {
            m["platforms"][p]["url"] = "ftp://example.com/x.exe".into();
            m["platforms"][p]["signature"] = key.sign(b"x", SETUP, "0.4.0").into();
        }
        assert_eq!(kind(&m.to_string()), Kind::Broken);
    }

    #[test]
    fn the_list_gives_the_newest_stable_release_with_a_manifest() {
        let list = r#"[
            {"tag_name": "v0.5.0", "draft": true, "prerelease": false, "assets": [{"name": "latest.json", "browser_download_url": "https://x/draft"}]},
            {"tag_name": "v0.4.1", "draft": false, "prerelease": true, "assets": [{"name": "latest.json", "browser_download_url": "https://x/beta"}]},
            {"tag_name": "v0.2.3", "draft": false, "prerelease": false, "assets": [{"name": "latest.json", "browser_download_url": "https://x/023"}]},
            {"tag_name": "v0.4.0", "draft": false, "prerelease": false, "assets": [{"name": "Venuta_0.4.0_x64-setup.exe", "browser_download_url": "https://x/setup"}]},
            {"tag_name": "v0.3.0", "draft": false, "prerelease": false, "assets": [{"name": "latest.json", "browser_download_url": "https://x/030"}]},
            {"tag_name": "v0.4.2-rc.1", "draft": false, "prerelease": false, "assets": [{"name": "latest.json", "browser_download_url": "https://x/rc"}]}
        ]"#;
        assert_eq!(
            pick_release(list, Channel::Stable).as_deref(),
            Ok("https://x/030")
        );
        assert_eq!(
            pick_release(list, Channel::Beta).as_deref(),
            Ok("https://x/rc")
        );
        assert_eq!(
            pick_release("[]", Channel::Stable).unwrap_err().kind,
            Kind::NotPublished
        );
        assert_eq!(
            pick_release("{\"message\":\"x\"}", Channel::Stable)
                .unwrap_err()
                .kind,
            Kind::Broken
        );
    }

    #[test]
    fn only_network_trouble_and_busy_servers_are_tried_again() {
        let waited = RefCell::new(Vec::new());
        let sleep = |d: Duration| waited.borrow_mut().push(d);
        let mut n = 0;
        let r = with_retries(&WAITS, &sleep, || {
            n += 1;
            if n < 3 {
                Err(Problem::new(Kind::Busy, "HTTP 503"))
            } else {
                Ok(n)
            }
        });
        assert_eq!(r, Ok(3));
        assert_eq!(*waited.borrow(), WAITS.to_vec());
        let mut n = 0;
        let r: Result<(), _> = with_retries(&WAITS, &sleep, || {
            n += 1;
            Err(Problem::new(Kind::Timeout, ""))
        });
        assert_eq!((r.unwrap_err().kind, n), (Kind::Timeout, 3));
        let mut n = 0;
        let r: Result<(), _> = with_retries(&WAITS, &sleep, || {
            n += 1;
            Err(Problem::new(Kind::WrongKey, ""))
        });
        assert_eq!((r.unwrap_err().kind, n), (Kind::WrongKey, 1));
    }

    #[test]
    fn every_problem_has_plain_words() {
        for k in [
            Kind::LocalOnly,
            Kind::UpToDate,
            Kind::Network,
            Kind::Timeout,
            Kind::Busy,
            Kind::NotPublished,
            Kind::Broken,
            Kind::NoWindows,
            Kind::WrongKey,
            Kind::Stalled,
            Kind::Signature,
            Kind::Folder,
            Kind::Installer,
        ] {
            let p = Problem::new(k.clone(), "detail");
            assert!(p.message.ends_with('.'), "{k:?}: {}", p.message);
            assert!(!p.message.contains('\u{2014}'), "no em dash: {}", p.message);
            assert!(p.message.len() > 20, "{k:?}");
        }
        assert!(
            Problem::new(Kind::Busy, "HTTP 503")
                .message
                .contains("HTTP 503")
        );
        assert!(
            Problem::new(Kind::LocalOnly, "")
                .message
                .contains("Local only")
        );
    }

    // Against a local server: what the network does, and what the check makes of it.

    fn quick() -> ureq::Agent {
        agent(Duration::from_millis(800), None)
    }

    #[test]
    fn statuses_become_problems() {
        let s = mock::serve(vec![
            ("/ok", vec![mock::json("{}")]),
            ("/busy", vec![Reply::Body(503, vec![])]),
            ("/limited", vec![Reply::Body(429, vec![])]),
            ("/forbidden", vec![Reply::Body(403, vec![])]),
            ("/gone", vec![Reply::Body(404, vec![])]),
            ("/odd", vec![Reply::Body(418, vec![])]),
            ("/hang", vec![Reply::Hang]),
        ]);
        let a = quick();
        assert_eq!(get(&a, &s.url("/ok")).as_deref(), Ok("{}"));
        let kind = |p: &str| get(&a, &s.url(p)).unwrap_err().kind;
        assert_eq!(kind("/busy"), Kind::Busy);
        assert_eq!(kind("/limited"), Kind::Busy);
        assert_eq!(kind("/forbidden"), Kind::Busy);
        assert_eq!(kind("/gone"), Kind::NotPublished);
        assert_eq!(kind("/odd"), Kind::Broken);
        let t = std::time::Instant::now();
        assert_eq!(kind("/hang"), Kind::Timeout);
        assert!(t.elapsed() < Duration::from_secs(5));
        // Nothing listening at all (Windows takes about two seconds to give up on a refused
        // connection, so this one waits longer).
        let a = agent(Duration::from_secs(10), None);
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        assert_eq!(
            get(&a, &format!("http://127.0.0.1:{port}/x"))
                .unwrap_err()
                .kind,
            Kind::Network
        );
    }

    fn check(
        s: &mock::Server,
        sources: &[Source],
        key: &TestKey,
    ) -> Result<Option<(String, Offer)>, Problem> {
        let a = quick();
        let fetch =
            |u: &str| with_retries(&[Duration::ZERO, Duration::ZERO], &|_| {}, || get(&a, u));
        let _ = s;
        find(sources, &fetch, &v("0.3.0"), Channel::Stable, &key.public())
    }

    #[test]
    fn a_busy_primary_is_asked_again_and_then_answers() {
        let key = TestKey::new();
        let s = mock::serve(vec![(
            "/latest.json",
            vec![
                Reply::Body(503, vec![]),
                Reply::Body(502, vec![]),
                mock::json(&offer_json(&key, "0.4.0")),
            ],
        )]);
        let r = check(&s, &[Source::Manifest(s.url("/latest.json"))], &key)
            .unwrap()
            .unwrap();
        assert_eq!(r.1.version, "0.4.0");
        assert_eq!(s.hits("/latest.json"), 3);
    }

    #[test]
    fn when_the_primary_fails_the_releases_list_finds_the_same_manifest() {
        let key = TestKey::new();
        let s = mock::serve(vec![
            ("/latest.json", vec![Reply::Hang]),
            (
                "/v0.4.0/latest.json",
                vec![mock::json(&offer_json(&key, "0.4.0"))],
            ),
        ]);
        let list = format!(
            r#"[{{"tag_name":"v0.4.0","draft":false,"prerelease":false,"assets":[{{"name":"latest.json","browser_download_url":"{}"}}]}}]"#,
            s.url("/v0.4.0/latest.json")
        );
        let s2 = mock::serve(vec![("/releases", vec![mock::json(&list)])]);
        let srcs = [
            Source::Manifest(s.url("/latest.json")),
            Source::Releases(s2.url("/releases")),
        ];
        let (url, offer) = check(&s, &srcs, &key).unwrap().unwrap();
        assert_eq!(url, s.url("/v0.4.0/latest.json"));
        assert_eq!(offer.version, "0.4.0");
        assert_eq!(
            s.hits("/latest.json"),
            3,
            "the primary is tried three times first"
        );
    }

    #[test]
    fn up_to_date_is_an_answer_not_a_failure() {
        let key = TestKey::new();
        let s = mock::serve(vec![(
            "/latest.json",
            vec![mock::json(&offer_json(&key, "0.3.0"))],
        )]);
        let srcs = [
            Source::Manifest(s.url("/latest.json")),
            Source::Releases(s.url("/releases")),
        ];
        assert_eq!(check(&s, &srcs, &key), Ok(None));
        assert_eq!(s.hits("/releases"), 0);
    }

    #[test]
    fn when_nothing_answers_the_most_telling_problem_is_given() {
        let key = TestKey::new();
        let other = TestKey::new();
        // The primary is down; the list's manifest is signed by someone else: that is the news.
        let s = mock::serve(vec![
            ("/latest.json", vec![Reply::Body(503, vec![])]),
            (
                "/v0.4.0/latest.json",
                vec![mock::json(&offer_json(&other, "0.4.0"))],
            ),
        ]);
        let list = format!(
            r#"[{{"tag_name":"v0.4.0","draft":false,"prerelease":false,"assets":[{{"name":"latest.json","browser_download_url":"{}"}}]}}]"#,
            s.url("/v0.4.0/latest.json")
        );
        let s2 = mock::serve(vec![("/releases", vec![mock::json(&list)])]);
        let srcs = [
            Source::Manifest(s.url("/latest.json")),
            Source::Releases(s2.url("/releases")),
        ];
        assert_eq!(check(&s, &srcs, &key).unwrap_err().kind, Kind::WrongKey);
        // Both down: busy.
        let s3 = mock::serve(vec![("/releases", vec![Reply::Body(503, vec![])])]);
        let srcs = [
            Source::Manifest(s.url("/latest.json")),
            Source::Releases(s3.url("/releases")),
        ];
        assert_eq!(check(&s, &srcs, &key).unwrap_err().kind, Kind::Busy);
        // No release has a manifest: not published.
        let s4 = mock::serve(vec![("/releases", vec![mock::json("[]")])]);
        let s5 = mock::serve(vec![]);
        let srcs = [
            Source::Manifest(s5.url("/latest.json")),
            Source::Releases(s4.url("/releases")),
        ];
        assert_eq!(check(&s, &srcs, &key).unwrap_err().kind, Kind::NotPublished);
    }

    #[test]
    fn the_update_goes_into_the_installed_folder() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        assert_eq!(
            install_dir_arg(&exe),
            None,
            "not an installed copy: the installer decides"
        );
        std::fs::write(dir.path().join("uninstall.exe"), b"").unwrap();
        assert_eq!(
            install_dir_arg(&exe),
            Some(format!("/D={}", dir.path().display()))
        );
        assert!(writable(dir.path()));
        assert!(!writable(&dir.path().join("missing")));
    }

    #[test]
    fn old_installers_are_cleared_but_not_other_files() {
        let temp = tempfile::tempdir().unwrap();
        for d in [
            "Venuta-0.2.3-updater-AbCdEf",
            "Venuta-0.3.0-updater-x1",
            "Other-0.1-updater-z",
            "Venuta-notes",
        ] {
            std::fs::create_dir(temp.path().join(d)).unwrap();
            std::fs::write(temp.path().join(d).join("f.exe"), b"x").unwrap();
        }
        assert_eq!(
            clean_old_installers(temp.path(), Duration::from_secs(3600)),
            0,
            "too new"
        );
        assert_eq!(clean_old_installers(temp.path(), Duration::ZERO), 2);
        let mut left: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["Other-0.1-updater-z", "Venuta-notes"]);
    }
}
