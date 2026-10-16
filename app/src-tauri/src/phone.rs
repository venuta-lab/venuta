//! The phone server as the app runs it: off until you turn it on, then on every time Venuta
//! starts, with the phones you added. Phones and their passwords are kept in the per-user
//! folder (`%APPDATA%\Venuta\phone-server.json`), never in the library, which may be copied
//! or shared.

use phone_server::{Phone, Running, Source};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;

#[derive(Default)]
pub struct PhoneServer(Mutex<Option<Running>>);

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    on: bool,
    #[serde(default)]
    phones: Vec<Phone>,
}

fn saved_path() -> PathBuf {
    engine::paths::user_dir().join("phone-server.json")
}

fn load() -> Saved {
    std::fs::read_to_string(saved_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(s: &Saved) -> Result<(), String> {
    let path = saved_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// The library the server shares: the app's own, kept fresh the same way.
struct Library {
    root: PathBuf,
}

impl Source for Library {
    fn root(&self) -> PathBuf {
        self.root.clone()
    }
    fn library(&self) -> Result<Arc<engine::library::Library>, String> {
        crate::libcache::fresh(&self.root).map(|(_, lib)| lib)
    }
    fn played(&self, path: &str) {
        crate::listening::record_play(&self.root.to_string_lossy(), path, 0);
    }
}

#[derive(Serialize)]
pub struct PhoneServerStatus {
    on: bool,
    port: u16,
    /// Where phones reach it: this PC's address on the network, and its name.
    address: Option<String>,
    host: Option<String>,
    phones: Vec<Phone>,
}

/// This PC's address on the network it reaches the internet through (nothing is sent: a
/// datagram socket only picks the route).
fn lan_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    phone_server::lan::is_lan(ip).then(|| ip.to_string())
}

fn status(server: &PhoneServer) -> PhoneServerStatus {
    let saved = load();
    let port = server
        .0
        .lock()
        .ok()
        .and_then(|r| r.as_ref().map(|r| r.addr.port()));
    PhoneServerStatus {
        on: port.is_some(),
        port: port.unwrap_or(phone_server::DEFAULT_PORT),
        address: lan_address(),
        host: std::env::var("COMPUTERNAME").ok().map(|n| n.to_lowercase()),
        phones: saved.phones,
    }
}

async fn start(root: &str, server: &PhoneServer) -> Result<(), String> {
    if server.0.lock().map_err(|e| e.to_string())?.is_some() {
        return Ok(());
    }
    let source = Arc::new(Library {
        root: PathBuf::from(root),
    });
    let phones = load().phones;
    // The usual port, or the next few if something else has it.
    let mut last = String::new();
    for port in phone_server::DEFAULT_PORT..phone_server::DEFAULT_PORT + 10 {
        match phone_server::start(port, source.clone(), phones.clone()).await {
            Ok(running) => {
                *server.0.lock().map_err(|e| e.to_string())? = Some(running);
                return Ok(());
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("The phone server couldn't start: {last}"))
}

/// Turn it on (and keep it on next time).
#[tauri::command]
pub async fn phone_server_start(
    root: String,
    server: State<'_, PhoneServer>,
) -> Result<PhoneServerStatus, String> {
    start(&root, &server).await?;
    let mut s = load();
    s.on = true;
    save(&s)?;
    Ok(status(&server))
}

/// When Venuta starts: on again if you left it on.
#[tauri::command]
pub async fn phone_server_resume(
    root: String,
    server: State<'_, PhoneServer>,
) -> Result<PhoneServerStatus, String> {
    if load().on {
        start(&root, &server).await?;
    }
    Ok(status(&server))
}

#[tauri::command]
pub fn phone_server_stop(server: State<'_, PhoneServer>) -> Result<PhoneServerStatus, String> {
    if let Some(r) = server.0.lock().map_err(|e| e.to_string())?.take() {
        r.stop();
    }
    let mut s = load();
    s.on = false;
    save(&s)?;
    Ok(status(&server))
}

#[tauri::command]
pub fn phone_server_status(server: State<'_, PhoneServer>) -> PhoneServerStatus {
    status(&server)
}

/// A phone, with a sign-in name from what you called it and a password made for it.
#[tauri::command]
pub fn phone_server_add(
    name: String,
    server: State<'_, PhoneServer>,
) -> Result<PhoneServerStatus, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the phone a name.".into());
    }
    let mut s = load();
    let base: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .take(16)
        .collect();
    let base = if base.is_empty() {
        "phone".into()
    } else {
        base
    };
    let mut user = base.clone();
    let mut n = 2;
    while s.phones.iter().any(|p| p.user == user) {
        user = format!("{base}{n}");
        n += 1;
    }
    s.phones.push(Phone {
        name: name.to_owned(),
        user,
        password: phone_server::auth::new_password(),
    });
    save(&s)?;
    tell_server(&server, s.phones);
    Ok(status(&server))
}

/// Cut a phone off at once.
#[tauri::command]
pub fn phone_server_remove(
    user: String,
    server: State<'_, PhoneServer>,
) -> Result<PhoneServerStatus, String> {
    let mut s = load();
    s.phones.retain(|p| p.user != user);
    save(&s)?;
    tell_server(&server, s.phones);
    Ok(status(&server))
}

fn tell_server(server: &PhoneServer, phones: Vec<Phone>) {
    if let Ok(r) = server.0.lock()
        && let Some(r) = r.as_ref()
    {
        r.set_phones(phones);
    }
}
