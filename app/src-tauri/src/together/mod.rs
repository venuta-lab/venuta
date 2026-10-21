//! Listening together: host a room from this Venuta, or join one, and everyone hears the same
//! song at the same moment, each from their best copy. The room's logic is the `together`
//! crate; this is the app around it: the connection (net), each player following the room
//! (follow), finding each person's copy (sources), and the host's and a guest's sessions.
//!
//! The window sees one view (`View`), sent as the "together" event after every change.

mod covers;
mod follow;
mod guest;
mod host;
mod net;
#[cfg(test)]
mod net_check;
mod net_guest;
mod net_host;
#[cfg(test)]
mod room_check;
mod sources;

use crate::playback::{Playback, QueueItem};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use together::invite::Invite;
use together::proto::{Action, Role, RoomView};

/// A room hosted here, or joined from here.
#[derive(Clone)]
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
    /// An invite link opened from outside (a chat, the browser), for the window to offer.
    invite_link: Mutex<Option<String>>,
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
    /// The host lets people with the link in without asking.
    pub let_in: bool,
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

/// The music folder songs are under: a Venuta home's Music folder when it has one with a library
/// (the window may pass the home or the music folder), else the folder given.
pub fn music_root(root: &str) -> String {
    let music = engine::paths::music_dir(std::path::Path::new(root));
    if music.is_dir() && crate::load_lib(&music).is_ok() {
        return music.to_string_lossy().replace('\\', "/");
    }
    root.to_owned()
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

/// A step of listening together, in the playback log (to see where a room got to).
pub fn trace(what: &str) {
    crate::playback::note_play(what, "listening together");
}

/// A player button pressed in a room (the tray, media keys, the mini player): it asks the room,
/// or the main window says why not.
fn button(app: &AppHandle, b: crate::playback::Button) {
    use crate::playback::Button;
    let Some(s) = current(&app.state::<Together>()) else {
        return;
    };
    let (room, may) = match &s {
        Session::Host(h) => (Some(h.room.lock().unwrap().view(true)), true),
        Session::Guest(g) => {
            let room = g.view.lock().unwrap().clone();
            let me = *g.me.lock().unwrap();
            let may = room
                .as_ref()
                .and_then(|r| r.members.iter().find(|m| Some(m.id) == me))
                .is_some_and(|m| m.role == Role::Control);
            (room, may)
        }
    };
    if !may {
        let host = room
            .map(|r| r.host_name)
            .unwrap_or_else(|| "The host".into());
        let _ = app.emit("together-note", format!("{host} is in control."));
        return;
    }
    let playing = room.and_then(|r| r.timeline).is_some_and(|t| t.playing);
    let action = match b {
        Button::Toggle if playing => Action::Pause,
        Button::Toggle | Button::Play => Action::Play,
        Button::Pause => Action::Pause,
        Button::Next => Action::Next,
        Button::Previous => Action::Previous,
        Button::Seek(ms) => Action::Seek { position_ms: ms },
    };
    match &s {
        Session::Host(h) => {
            if let Err(e) = h.ask(action) {
                let _ = app.emit("together-note", e);
            }
        }
        Session::Guest(g) => g.ask(action),
    }
}

/// The room takes the player's buttons while this session lasts.
pub(crate) fn take_buttons(app: &AppHandle) {
    let a = app.clone();
    app.state::<Playback>()
        .set_room_buttons(Some(Arc::new(move |b| button(&a, b))));
}

/// Invite links clicked anywhere (venuta://join/…) bring Venuta forward with the room to join.
pub fn watch_invite_links(app: &AppHandle) {
    use tauri_plugin_deep_link::DeepLinkExt;
    // Opened by a link: it's here from the start.
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        offer_invite(app, &urls);
    }
    // Open already: the link comes on (single instance passes it).
    let a = app.clone();
    app.deep_link()
        .on_open_url(move |e| offer_invite(&a, &e.urls()));
}

fn offer_invite<U: AsRef<str>>(app: &AppHandle, urls: &[U]) {
    let Some(link) = urls
        .iter()
        .map(|u| u.as_ref())
        .find(|u| u.starts_with(together::invite::LINK_PREFIX))
    else {
        return;
    };
    *app.state::<Together>().invite_link.lock().unwrap() = Some(link.to_owned());
    crate::desktop::show_main(app);
    let _ = app.emit("together-invite", link);
}

/// The invite link Venuta was opened with, once (the window asks as it starts).
#[tauri::command]
pub fn together_take_invite(t: State<'_, Together>) -> Option<String> {
    t.invite_link.lock().unwrap().take()
}

/// Tell the window what changed.
fn emit(app: &AppHandle) {
    let mut v = view(&app.state::<Together>());
    v.delay_ms = my_delay(app);
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
    match current(t).as_ref() {
        Some(Session::Host(h)) => {
            v.state = "hosting".into();
            v.host = true;
            v.me = Some(together::room::HOST);
            {
                let room = h.room.lock().unwrap();
                let mut view = room.view(true);
                h.covers.decorate(&mut view);
                v.room = Some(view);
                v.let_in = room.lets_in_without_asking();
            }
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
            v.room = g.view.lock().unwrap().clone().map(|r| g.with_covers(r));
            v.path = g.path();
            if let Some(why) = g.why.lock().unwrap().clone() {
                v.message = Some(why);
            }
        }
        None => {}
    }
    v
}

/// The session, out of its lock: a room's work tells the window, which reads the session
/// again, so it's never done while the lock is held.
fn current(t: &Together) -> Option<Session> {
    t.session.lock().unwrap().clone()
}

/// A room that ended (the host ended it, you were taken out, it was lost) is over: starting or
/// joining another one clears it.
fn clear_ended(t: &Together) {
    let mut s = t.session.lock().unwrap();
    if let Some(Session::Guest(g)) = s.as_ref()
        && *g.stage.lock().unwrap() == guest::Stage::Ended
    {
        let g = g.clone();
        *s = None;
        tauri::async_runtime::spawn(async move { g.close().await });
    }
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
        clear_ended(&t);
        if t.session.lock().unwrap().is_some() {
            return Err("You're in a room already.".into());
        }
        *t.busy.lock().unwrap() = Some("starting");
        *t.error.lock().unwrap() = None;
    }
    emit(&app);
    trace("starting a room");
    let r = host::Host::start(
        app.clone(),
        music_root(&root),
        items,
        index,
        position_ms,
        playing,
    )
    .await;
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
        clear_ended(&t);
        if t.session.lock().unwrap().is_some() {
            return Err("You're in a room already. Leave it first.".into());
        }
        *t.busy.lock().unwrap() = Some("joining");
        *t.error.lock().unwrap() = None;
    }
    emit(&app);
    let r = guest::Guest::join(app.clone(), music_root(&root), invite).await;
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
    match current(&t).as_ref() {
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
    match current(&t).as_ref() {
        Some(Session::Host(h)) => h.add_items(&music_root(&root), &items, next),
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
    match current(t).as_ref() {
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
    with_host(&t, |h| {
        if on && !h.all_on_one_network() {
            return Err(
                "Same room is for speakers that hear each other: everyone on this Wi-Fi. Over the internet each of you hears it smoothly on your own."
                    .to_owned(),
            );
        }
        h.set(|room| room.set_same_room(on));
        Ok(())
    })?
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
    match current(&t).as_ref() {
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
    match current(&t).as_ref() {
        Some(Session::Host(h)) => h.to_follower(follow::In::Delay(ms)),
        Some(Session::Guest(g)) => g.to_follower(follow::In::Delay(ms)),
        None => {}
    }
    emit(&app);
    Ok(())
}
