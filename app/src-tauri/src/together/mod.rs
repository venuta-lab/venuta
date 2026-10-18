//! Listening together: host a room from this Venuta, or join one, and everyone hears the same
//! song at the same moment, each from their best copy. The room's logic is the `together`
//! crate; this is the app around it: the connection (net), each player following the room
//! (follow), finding each person's copy (sources), and the host's and a guest's sessions.
//!
//! The window sees one view (`View`), sent as the "together" event after every change.

mod follow;
mod guest;
mod host;
mod net;
mod sources;

use crate::playback::{Playback, QueueItem};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use together::invite::Invite;
use together::proto::{Action, Role, RoomView};

/// A room hosted here, or joined from here.
enum Session {
    Host(Arc<host::Host>),
    Guest(Arc<guest::Guest>),
}

#[derive(Default)]
pub struct Together {
    session: Mutex<Option<Session>>,
    /// A start or a join under way (for the window to say so).
    busy: Mutex<Option<&'static str>>,
    error: Mutex<Option<String>>,
}

/// What the window shows.
#[derive(serde::Serialize, Clone, Default)]
pub struct View {
    /// "off", "starting", "joining", "hosting", "waiting", "in", "reconnecting", "ended".
    pub state: String,
    pub host: bool,
    /// Your id in the room.
    pub me: Option<u32>,
    pub room: Option<RoomView>,
    /// The host's invite: the link, the code, and a QR picture of the link (SVG).
    pub invite: Option<InviteView>,
    /// How the connection to the host goes (a guest's).
    pub path: Option<net::Path>,
    /// Why it ended, or what went wrong.
    pub message: Option<String>,
    /// Your name, as others see it.
    pub name: String,
    /// Your output's delay setting (Bluetooth), milliseconds.
    pub delay_ms: i32,
    pub local_only: bool,
}

#[derive(serde::Serialize, Clone)]
pub struct InviteView {
    pub link: String,
    pub code: String,
    pub qr_svg: String,
    pub host: String,
    pub until: u64,
}

/// The name others see: yours from Settings, or this computer's user name.
pub fn my_name() -> String {
    let s = engine::settings::load().listening.together_name;
    let n = if s.trim().is_empty() {
        std::env::var("USERNAME").unwrap_or_else(|_| "Someone".into())
    } else {
        s
    };
    n.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect()
}

/// This output's delay setting.
pub fn my_delay(app: &AppHandle) -> i32 {
    let out = app
        .state::<Playback>()
        .output_id()
        .unwrap_or_else(|| "default".into());
    engine::settings::load()
        .listening
        .together_delay
        .get(&out)
        .copied()
        .unwrap_or(0)
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `n` random bytes as hex, from the operating system's randomness.
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("the system's randomness");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn qr_svg(text: &str) -> String {
    qrcode::QrCode::new(text.as_bytes())
        .map(|c| {
            c.render::<qrcode::render::svg::Color>()
                .min_dimensions(200, 200)
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_default()
}

/// Someone knocks: a Windows notification when Venuta isn't in front.
fn notify_knock(app: &AppHandle, name: &str) {
    let front = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if !front {
        use tauri_plugin_notification::NotificationExt;
        let _ = app
            .notification()
            .builder()
            .title("Listen together")
            .body(format!("{name} wants to join your room"))
            .show();
    }
}

/// Tell the window what changed.
fn emit(app: &AppHandle) {
    let v = view(&app.state::<Together>());
    let _ = app.emit("together", v);
}

fn view(t: &Together) -> View {
    let s = engine::settings::load();
    let mut v = View {
        state: "off".into(),
        name: my_name(),
        delay_ms: 0,
        local_only: s.listening.local_only,
        message: t.error.lock().unwrap().clone(),
        ..View::default()
    };
    if let Some(b) = *t.busy.lock().unwrap() {
        v.state = b.into();
    }
    match t.session.lock().unwrap().as_ref() {
        Some(Session::Host(h)) => {
            v.state = "hosting".into();
            v.host = true;
            v.me = Some(together::room::HOST);
            v.room = Some(h.room.lock().unwrap().view(true));
            let link = h.invite.link();
            v.invite = Some(InviteView {
                qr_svg: qr_svg(&link),
                link,
                code: h.invite.code(),
                host: h.invite.host.clone(),
                until: h.invite.until,
            });
        }
        Some(Session::Guest(g)) => {
            v.state = match *g.stage.lock().unwrap() {
                guest::Stage::Joining => "joining",
                guest::Stage::Waiting => "waiting",
                guest::Stage::In => "in",
                guest::Stage::Reconnecting => "reconnecting",
                guest::Stage::Ended => "ended",
            }
            .into();
            v.me = *g.me.lock().unwrap();
            v.room = g.view.lock().unwrap().clone();
            v.path = g.path();
            if let Some(why) = g.why.lock().unwrap().clone() {
                v.message = Some(why);
            }
        }
        None => {}
    }
    v
}

fn role(s: &str) -> Result<Role, String> {
    host::role_of(s).ok_or_else(|| format!("no role called {s}"))
}

// --- Commands ---------------------------------------------------------------------------

#[tauri::command]
pub fn together_view(t: State<'_, Together>, app: AppHandle) -> View {
    let mut v = view(&t);
    v.delay_ms = my_delay(&app);
    v
}

/// Open a room with what's playing.
#[tauri::command]
pub async fn together_host(
    app: AppHandle,
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    position_ms: u64,
    playing: bool,
) -> Result<View, String> {
    {
        let t = app.state::<Together>();
        if t.session.lock().unwrap().is_some() {
            return Err("You're in a room already.".into());
        }
        *t.busy.lock().unwrap() = Some("starting");
        *t.error.lock().unwrap() = None;
    }
    emit(&app);
    let r = host::Host::start(app.clone(), root, items, index, position_ms, playing).await;
    let t = app.state::<Together>();
    *t.busy.lock().unwrap() = None;
    match r {
        Ok(h) => *t.session.lock().unwrap() = Some(Session::Host(h)),
        Err(e) => *t.error.lock().unwrap() = Some(e.clone()),
    }
    emit(&app);
    Ok(view(&t))
}

/// What an invite says, before joining (whose room, whether it still works).
#[tauri::command]
pub fn together_read_invite(code: String) -> Result<serde_json::Value, String> {
    let i = Invite::parse(&code)?;
    Ok(serde_json::json!({ "host": i.host, "open": i.open_at(unix_now()), "until": i.until }))
}

/// Join a room with its invite.
#[tauri::command]
pub async fn together_join(app: AppHandle, root: String, code: String) -> Result<View, String> {
    let invite = Invite::parse(&code)?;
    {
        let t = app.state::<Together>();
        if t.session.lock().unwrap().is_some() {
            return Err("You're in a room already. Leave it first.".into());
        }
        *t.busy.lock().unwrap() = Some("joining");
        *t.error.lock().unwrap() = None;
    }
    emit(&app);
    let r = guest::Guest::join(app.clone(), root, invite).await;
    let t = app.state::<Together>();
    *t.busy.lock().unwrap() = None;
    match r {
        Ok(g) => *t.session.lock().unwrap() = Some(Session::Guest(g)),
        Err(e) => *t.error.lock().unwrap() = Some(e.clone()),
    }
    emit(&app);
    Ok(view(&t))
}

/// Leave the room (the host: end it for everyone).
#[tauri::command]
pub async fn together_leave(app: AppHandle) -> Result<(), String> {
    let s = app.state::<Together>().session.lock().unwrap().take();
    match s {
        Some(Session::Host(h)) => h.end().await,
        Some(Session::Guest(g)) => g.leave().await,
        None => {}
    }
    *app.state::<Together>().error.lock().unwrap() = None;
    emit(&app);
    Ok(())
}

/// Something to do to the room (anyone, within their role).
#[tauri::command]
pub fn together_ask(t: State<'_, Together>, action: Action) -> Result<(), String> {
    match t.session.lock().unwrap().as_ref() {
        Some(Session::Host(h)) => h.ask(action),
        Some(Session::Guest(g)) => {
            g.ask(action);
            Ok(())
        }
        None => Err("You're not in a room.".into()),
    }
}

/// Add songs to the room: the host's from its library or the catalog; a guest's online songs.
#[tauri::command]
pub fn together_add(
    t: State<'_, Together>,
    root: String,
    items: Vec<QueueItem>,
    next: bool,
) -> Result<(), String> {
    match t.session.lock().unwrap().as_ref() {
        Some(Session::Host(h)) => h.add_items(&root, &items, next),
        Some(Session::Guest(g)) => {
            let songs: Vec<_> = items
                .iter()
                .filter(|it| it.path.is_empty())
                .filter_map(|it| sources::song_of_item(it, None))
                .collect();
            if songs.is_empty() {
                return Err("Only online songs can go in someone else's room (your own files stay with you).".into());
            }
            g.ask(Action::Add { songs, next });
            Ok(())
        }
        None => Err("You're not in a room.".into()),
    }
}

fn with_host<R>(t: &Together, f: impl FnOnce(&host::Host) -> R) -> Result<R, String> {
    match t.session.lock().unwrap().as_ref() {
        Some(Session::Host(h)) => Ok(f(h)),
        _ => Err("Only the host can do that.".into()),
    }
}

#[tauri::command]
pub fn together_undo(t: State<'_, Together>) -> Result<(), String> {
    with_host(&t, |h| h.undo())?
}

#[tauri::command]
pub fn together_admit(t: State<'_, Together>, id: u32) -> Result<(), String> {
    with_host(&t, |h| h.admit(id))
}

#[tauri::command]
pub fn together_deny(t: State<'_, Together>, id: u32) -> Result<(), String> {
    with_host(&t, |h| {
        h.turn_away(id, false, "The host didn't let you in this time.")
    })
}

#[tauri::command]
pub fn together_remove(t: State<'_, Together>, id: u32, block: bool) -> Result<(), String> {
    with_host(&t, |h| {
        h.turn_away(
            id,
            block,
            if block {
                "The host removed you from the room."
            } else {
                "The host took you out of the room."
            },
        )
    })
}

#[tauri::command]
pub fn together_role(t: State<'_, Together>, id: u32, role_name: String) -> Result<(), String> {
    let r = role(&role_name)?;
    with_host(&t, |h| h.set(|room| room.set_role(id, r)))
}

#[tauri::command]
pub fn together_new_role(t: State<'_, Together>, role_name: String) -> Result<(), String> {
    let r = role(&role_name)?;
    with_host(&t, |h| h.set(|room| room.set_new_role(r)))
}

#[tauri::command]
pub fn together_same_room(t: State<'_, Together>, on: bool) -> Result<(), String> {
    with_host(&t, |h| h.set(|room| room.set_same_room(on)))
}

#[tauri::command]
pub fn together_let_in_without_asking(t: State<'_, Together>, on: bool) -> Result<(), String> {
    with_host(&t, |h| {
        h.set(|room| {
            room.set_let_in_without_asking(on);
            together::room::Changes::default()
        })
    })
}

/// Pause just for you (the room goes on), or come back to where the room is.
#[tauri::command]
pub fn together_step_out(t: State<'_, Together>, out: bool) -> Result<(), String> {
    let m = || {
        if out {
            follow::In::StepOut
        } else {
            follow::In::CatchUp
        }
    };
    match t.session.lock().unwrap().as_ref() {
        Some(Session::Host(h)) => h.to_follower(m()),
        Some(Session::Guest(g)) => g.to_follower(m()),
        None => return Err("You're not in a room.".into()),
    }
    Ok(())
}

/// Your name, as others see it.
#[tauri::command]
pub fn together_set_name(app: AppHandle, name: String) -> Result<(), String> {
    let mut s = engine::settings::load();
    s.listening.together_name = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    engine::settings::save(&s)?;
    emit(&app);
    Ok(())
}

/// Your output's delay (Bluetooth): how much later than the device says you hear it.
#[tauri::command]
pub fn together_set_delay(app: AppHandle, t: State<'_, Together>, ms: i32) -> Result<(), String> {
    let ms = ms.clamp(-500, 1_000);
    let out = app
        .state::<Playback>()
        .output_id()
        .unwrap_or_else(|| "default".into());
    let mut s = engine::settings::load();
    s.listening.together_delay.insert(out, ms);
    engine::settings::save(&s)?;
    match t.session.lock().unwrap().as_ref() {
        Some(Session::Host(h)) => h.to_follower(follow::In::Delay(ms)),
        Some(Session::Guest(g)) => g.to_follower(follow::In::Delay(ms)),
        None => {}
    }
    Ok(())
}
