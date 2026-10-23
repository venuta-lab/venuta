//! Listening together: host a room from this Venuta, or join one, and everyone hears the same
//! song at the same moment, each from their best copy. The room, the sessions and each person's
//! conductor are the `together` crate's (protocol 3, design 4.3 and its WP9 notes); the
//! connection is `together_net`'s (iroh). This is the app around them: each session on a
//! thread of its own, the player it takes (engine 2, through `RoomHandle`), each person's
//! copies (sources), the host's files and covers, and the window's commands and view.
//!
//! The window sees one view (`together::view::View`, v2), sent as the "together" event after
//! every change and ten times a second while the room plays.

mod covers;
mod guest;
mod host;
mod net;
mod sources;

use crate::playback::{Button, Playback, QueueItem};
use player::drift_memory::DriftMemory;
use std::sync::mpsc::{Sender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use together::align::Alignment;
use together::anchor::{Anchor, EntryId, PrepId, Transition};
use together::invite::Invite;
use together::lifecycle::{Kept, Offer, offer};
use together::proto::v3::{Action, Advice};
use together::proto::{Role, Song};
use together::room_player::{
    ListenMode, RoomHandle, RoomPlayStatus, RoomPlayer, SubState, SyncMode,
};
use together::view::{OutputView, Resume, SessionState, View};
use together_net::nearby::{self, Beacon, Listener};

/// The catalog's provider name, for the player's plays.
pub use sources::CATALOG as CATALOG_PROVIDER;

/// Work for a session's thread.
type HostWork = Box<dyn FnOnce(&mut host::Live) + Send>;
type GuestWork = Box<dyn FnOnce(&mut guest::Live) + Send>;

/// A room hosted here, or joined from here: its thread's queue.
#[derive(Clone)]
enum Session {
    Host(Sender<HostWork>),
    Guest(Sender<GuestWork>),
}

pub struct Together {
    session: Mutex<Option<Session>>,
    /// The view last published (the session's, or the last one after it ended).
    view: Mutex<Option<View>>,
    /// A start or a join under way (for the window to say so).
    busy: Mutex<Option<SessionState>>,
    error: Mutex<Option<String>>,
    /// An invite link opened from outside (a chat, the browser), for the window to offer.
    invite_link: Mutex<Option<String>>,
    /// The songs of the room just left, for "Save the room's queue as a playlist".
    last_queue: Mutex<Vec<Song>>,
    /// The host lets the room be seen on this network (D19: on by default).
    visible: std::sync::atomic::AtomicBool,
    /// What the records on disk offer ("Reopen your room?", "Rejoin Asha's room?"), read once
    /// at start and again after a room; `None` inside while not read yet.
    offer: Mutex<Option<Option<Offer>>>,
    /// An automatic rejoin was tried (once per start).
    auto_tried: std::sync::atomic::AtomicBool,
    /// Rooms nearby, heard while in no room (A15).
    nearby: Mutex<Option<Listener>>,
    rooms: Arc<Mutex<Vec<(u32, Beacon)>>>,
}

impl Default for Together {
    fn default() -> Self {
        Together {
            session: Mutex::default(),
            view: Mutex::default(),
            busy: Mutex::default(),
            error: Mutex::default(),
            invite_link: Mutex::default(),
            last_queue: Mutex::default(),
            visible: std::sync::atomic::AtomicBool::new(true),
            offer: Mutex::default(),
            auto_tried: std::sync::atomic::AtomicBool::new(false),
            nearby: Mutex::default(),
            rooms: Arc::default(),
        }
    }
}

/// The records kept for reopening and rejoining, in the app's data folder (WP15).
pub fn kept() -> Kept {
    Kept::new(engine::paths::app_data_dir())
}

/// What rooms learn of each output's drift, kept in the data folder (WP15), for the player.
pub fn drift_memory() -> Arc<DriftMemory> {
    static MEMORY: std::sync::OnceLock<Arc<DriftMemory>> = std::sync::OnceLock::new();
    MEMORY
        .get_or_init(|| {
            Arc::new(DriftMemory::open(
                &engine::paths::app_data_dir().join("together-drift.json"),
            ))
        })
        .clone()
}

pub fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// What the records offer now (read from disk the first time, and after each room).
fn the_offer(t: &Together) -> Option<Offer> {
    let mut o = t.offer.lock().unwrap();
    if o.is_none() {
        let k = kept();
        *o = Some(offer(k.host().as_ref(), k.guest().as_ref(), unix_now()));
    }
    o.clone().flatten()
}

/// Rooms nearby are listened for while in no room, and the window is told of each change.
fn listen_nearby(app: &AppHandle) {
    let t = app.state::<Together>();
    let mut n = t.nearby.lock().unwrap();
    if n.is_some() || t.session.lock().unwrap().is_some() || t.busy.lock().unwrap().is_some() {
        return;
    }
    let (a, rooms) = (app.clone(), t.rooms.clone());
    let at = std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, nearby::PORT));
    match Listener::start(at, Some(nearby::GROUP), move |heard| {
        *rooms.lock().unwrap() = heard;
        let t = a.state::<Together>();
        if current(&t).is_none() && t.busy.lock().unwrap().is_none() {
            emit_idle(&a);
        }
    }) {
        Ok(l) => *n = Some(l),
        Err(e) => trace(&format!("rooms nearby: not listening ({e})")),
    }
}

/// A room begins: rooms nearby aren't listened for meanwhile.
fn stop_nearby(t: &Together) {
    let l = t.nearby.lock().unwrap().take();
    drop(l);
    t.rooms.lock().unwrap().clear();
}

/// The rooms nearby, for the window: those whose invite still works.
fn nearby_views(t: &Together) -> Vec<together::view::NearbyView> {
    let now = unix_now();
    t.rooms
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, b)| b.until > now)
        .map(|(id, b)| together::view::NearbyView {
            id: *id,
            host: b.host.clone(),
            listening: b.listening,
        })
        .collect()
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

/// This output's delay setting, ms.
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

/// The output the room is heard on, for the window.
fn output_view(app: &AppHandle) -> OutputView {
    let id = app.state::<Playback>().output_id();
    OutputView {
        name: id.unwrap_or_else(|| "Default output".into()),
        delay_ms: my_delay(app),
        // Which outputs are Bluetooth, and whose delay is measured, is WP19's.
        bluetooth: false,
        delay_known: true,
    }
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

/// A short note for the window.
fn note(app: &AppHandle, what: &str) {
    let _ = app.emit("together-note", what);
}

// --- The player a room takes ------------------------------------------------------------------

/// The app's player, as a room takes it (engine 2's `RoomPlayer`).
struct AppPlayer(AppHandle);

impl RoomPlayer<QueueItem> for AppPlayer {
    fn attach(
        &self,
        clock: together::clock_map::ClockMapHandle,
        mode: SyncMode,
    ) -> Box<dyn RoomHandle<QueueItem>> {
        self.0
            .state::<Playback>()
            .with(|p| RoomPlayer::attach(p, clock, mode))
            .unwrap_or_else(|_| Box::new(NoPlayer))
    }
    fn adopt(
        &self,
        clock: together::clock_map::ClockMapHandle,
        mode: SyncMode,
        entry: EntryId,
    ) -> Box<dyn RoomHandle<QueueItem>> {
        self.0
            .state::<Playback>()
            .with(|p| RoomPlayer::adopt(p, clock, mode, entry))
            .unwrap_or_else(|_| Box::new(NoPlayer))
    }
}

/// A player that couldn't be had (its thread failed to start): it can't play anything.
struct NoPlayer;

impl RoomHandle<QueueItem> for NoPlayer {
    fn prepare(&self, _: PrepId, _: EntryId, _: QueueItem, _: i64, _: Alignment) {}
    fn arm_next(&self, _: EntryId, _: QueueItem, _: i64, _: Transition, _: Alignment) {}
    fn disarm(&self) {}
    fn commit(&self, _: Anchor) {}
    fn hold(&self, _: Anchor) {}
    fn set_mode(&self, _: SyncMode) {}
    fn set_extra_latency(&self, _: i32) {}
    fn step_out(&self) {}
    fn rejoin(&self) {}
    fn status(&self) -> RoomPlayStatus {
        RoomPlayStatus {
            sub_state: SubState::CannotPlay,
            ..RoomPlayStatus::default()
        }
    }
    fn detach(self: Box<Self>) {}
}

/// Rooms play only on engine 2: on engine 1, nobody sits in a room hearing nothing.
fn needs_engine_two(app: &AppHandle) -> Result<(), String> {
    if app.state::<Playback>().engine_two() {
        Ok(())
    } else {
        Err("Listen together plays on Venuta's new sound engine, which is turned off in its settings (player.engine is 1). Remove that setting to listen together.".into())
    }
}

/// A room song as the player's bar shows songs.
fn display_item(song: &Song, own: Option<&QueueItem>) -> QueueItem {
    if let Some(it) = own {
        return it.clone();
    }
    let id = song.online_id.clone().unwrap_or_else(|| song.key.clone());
    sources::item_of_online(song, &id)
}

// --- The window ----------------------------------------------------------------------------

/// Tell the window: the session's view (made on its thread), stored for `together_view`.
fn publish(app: &AppHandle, v: View) {
    *app.state::<Together>().view.lock().unwrap() = Some(v.clone());
    let _ = app.emit("together", v);
}

/// The view with no session: off, starting or joining, or what the last room left.
fn idle_view(app: &AppHandle) -> View {
    let t = app.state::<Together>();
    let s = engine::settings::load();
    let busy = *t.busy.lock().unwrap();
    let last = t.view.lock().unwrap().clone();
    let error = t.error.lock().unwrap().clone();
    let last_queue = {
        let q = t.last_queue.lock().unwrap();
        (!q.is_empty()).then_some(q.len() as u32)
    };
    let ended = last
        .as_ref()
        .filter(|v| v.state == SessionState::Ended)
        .cloned();
    View {
        state: busy.unwrap_or(if ended.is_some() {
            SessionState::Ended
        } else {
            SessionState::Off
        }),
        host: false,
        me: None,
        room: None,
        invite: None,
        path: None,
        message: error.or(ended.and_then(|v| v.message)),
        name: my_name(),
        local_only: s.listening.local_only,
        let_in: false,
        you: None,
        output: output_view(app),
        host_now_us: None,
        nearby: if busy.is_none() {
            nearby_views(&t)
        } else {
            Vec::new()
        },
        visible: t.visible.load(std::sync::atomic::Ordering::Relaxed),
        resume: if busy.is_none() {
            the_offer(&t).map(|o| o.resume)
        } else {
            None
        },
        last_queue,
    }
}

fn emit_idle(app: &AppHandle) {
    let v = idle_view(app);
    let _ = app.emit("together", v);
}

/// The session, out of its lock.
fn current(t: &Together) -> Option<Session> {
    t.session.lock().unwrap().clone()
}

/// Run `host` or `guest` on the session's thread and wait (2 s at most) for what it says.
fn on_session<R: Send + 'static>(
    t: &Together,
    host: impl FnOnce(&mut host::Live) -> Result<R, String> + Send + 'static,
    guest: impl FnOnce(&mut guest::Live) -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    let (tx, rx) = sync_channel(1);
    let sent = match current(t) {
        Some(Session::Host(s)) => s
            .send(Box::new(move |l: &mut host::Live| {
                let _ = tx.send(host(l));
            }))
            .is_ok(),
        Some(Session::Guest(s)) => s
            .send(Box::new(move |l: &mut guest::Live| {
                let _ = tx.send(guest(l));
            }))
            .is_ok(),
        None => return Err("You're not in a room.".into()),
    };
    if !sent {
        return Err("You're not in a room.".into());
    }
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "The room didn't answer.".to_owned())?
}

fn host_only<R: Send + 'static>(
    t: &Together,
    f: impl FnOnce(&mut host::Live) -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    on_session(t, f, |_| Err("Only the host can do that.".into()))
}

fn guest_only<R: Send + 'static>(
    t: &Together,
    f: impl FnOnce(&mut guest::Live) -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    on_session(t, |_| Err("Only a guest can do that.".into()), f)
}

/// A press of the player's buttons in a room (the tray, media keys, the mini player, the bar):
/// the room's, as your role allows (a request otherwise).
fn button(app: &AppHandle, b: Button) {
    let t = app.state::<Together>();
    let action = move |playing: bool| match b {
        Button::Toggle if playing => Action::Pause,
        Button::Toggle | Button::Play => Action::Play,
        Button::Pause => Action::Pause,
        Button::Next => Action::Next,
        Button::Previous => Action::Previous,
        Button::Seek(ms) => Action::Seek {
            content_us: (ms as i64).saturating_mul(1_000),
        },
    };
    let r = on_session(
        &t,
        move |l| {
            let playing = l.s.room().anchor().is_some_and(|a| a.playing);
            l.s.ask(action(playing))
        },
        move |l| {
            let playing = l.s.conductor().anchor().is_some_and(|a| a.playing);
            l.press(action(playing));
            Ok(())
        },
    );
    if let Err(e) = r {
        note(app, &e);
    }
}

/// The room takes the player's buttons while this session lasts (`false`: they're given back).
pub(crate) fn take_buttons(app: &AppHandle, on: bool) {
    let pb = app.state::<Playback>();
    if on {
        let a = app.clone();
        pb.set_room_buttons(Some(Arc::new(move |b| button(&a, b))));
    } else {
        pb.set_room_buttons(None);
    }
}

/// Invite links clicked anywhere (venuta://join/…) bring Venuta forward with the room to join.
pub fn watch_invite_links(app: &AppHandle) {
    use tauri_plugin_deep_link::DeepLinkExt;
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        offer_invite(app, &urls);
    }
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

/// A session's thread let go: what it leaves for the window, and the player as it was.
fn session_over(app: &AppHandle, queue: Vec<Song>, last: View) {
    let t = app.state::<Together>();
    *t.session.lock().unwrap() = None;
    let n = queue.len() as u32;
    *t.last_queue.lock().unwrap() = queue;
    take_buttons(app, false);
    app.state::<Playback>().set_room_now(None);
    // What the room learned of this output's drift, kept for the next room (WP15).
    if let Err(e) = drift_memory().save() {
        trace(&format!("room: the drift not kept ({e})"));
    }
    // The records as the room left them: a guest whose host went is offered a rejoin.
    *t.offer.lock().unwrap() = None;
    let resume = the_offer(&t).map(|o| o.resume);
    publish(
        app,
        View {
            last_queue: (n > 0).then_some(n),
            resume,
            nearby: Vec::new(),
            ..last
        },
    );
    trace("room: the player is on its own again");
}

fn role(s: &str) -> Result<Option<Role>, String> {
    match s {
        "control" => Ok(Some(Role::Control)),
        "add" => Ok(Some(Role::Add)),
        "listen" => Ok(Some(Role::Listen)),
        "auto" => Ok(None),
        _ => Err(format!("no role called {s}")),
    }
}

// --- Commands ---------------------------------------------------------------------------

/// The invite link Venuta was opened with, once (the window asks as it starts).
#[tauri::command]
pub fn together_take_invite(t: State<'_, Together>) -> Option<String> {
    t.invite_link.lock().unwrap().take()
}

#[tauri::command]
pub fn together_view(t: State<'_, Together>, app: AppHandle) -> View {
    if current(&t).is_some()
        && let Some(v) = t.view.lock().unwrap().clone()
    {
        return v;
    }
    // A guest whose app went mid-room under 2 minutes ago rejoins by itself (I7), once.
    if t.busy.lock().unwrap().is_none()
        && the_offer(&t).is_some_and(|o| o.auto)
        && !t.auto_tried.swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        let a = app.clone();
        tauri::async_runtime::spawn(async move {
            // The library folder, once the window has opened it (a few seconds at most).
            for _ in 0..20 {
                if !crate::playback::current_root(&a).is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            trace("room: rejoining by itself after a restart");
            if let Err(e) = resume(&a).await {
                note(&a, &e);
            }
        });
    }
    idle_view(&app)
}

/// Rooms nearby are listened for while the panel's start screen shows (`on`), and not after
/// (A15): nothing is bound for them otherwise.
#[tauri::command]
pub fn together_nearby(app: AppHandle, on: bool) {
    if on {
        listen_nearby(&app);
    } else {
        stop_nearby(&app.state::<Together>());
    }
}

/// Start a room or join one (`fresh`: not a reopen or rejoin, so what was kept goes: one room
/// at a time).
fn begin(app: &AppHandle, as_: SessionState) -> Result<(), String> {
    begin_with(app, as_, true)
}

fn begin_with(app: &AppHandle, as_: SessionState, fresh: bool) -> Result<(), String> {
    needs_engine_two(app)?;
    let t = app.state::<Together>();
    if t.session.lock().unwrap().is_some() {
        return Err("You're in a room already. Leave it first.".into());
    }
    if t.busy.lock().unwrap().is_some() {
        return Err("A room is starting already.".into());
    }
    if fresh {
        let k = kept();
        k.clear_host();
        k.clear_guest();
        *t.offer.lock().unwrap() = Some(None);
    }
    stop_nearby(&t);
    *t.busy.lock().unwrap() = Some(as_);
    *t.error.lock().unwrap() = None;
    *t.view.lock().unwrap() = None;
    emit_idle(app);
    Ok(())
}

fn begun(app: &AppHandle, r: Result<Session, String>) -> View {
    let t = app.state::<Together>();
    *t.busy.lock().unwrap() = None;
    match r {
        Ok(s) => {
            *t.session.lock().unwrap() = Some(s);
            take_buttons(app, true);
        }
        Err(e) => {
            trace(&format!("room: couldn't begin ({e})"));
            *t.error.lock().unwrap() = Some(e);
            emit_idle(app);
        }
    }
    together_view(t, app.clone())
}

/// Open a room with what's playing (it goes on, untouched: A1).
#[tauri::command]
pub async fn together_host(
    app: AppHandle,
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    position_ms: u64,
    playing: bool,
) -> Result<View, String> {
    begin(&app, SessionState::Starting)?;
    trace("starting a room");
    let r = host::start(
        app.clone(),
        music_root(&root),
        items,
        index,
        position_ms,
        playing,
    )
    .await;
    Ok(begun(&app, r.map(Session::Host)))
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
    begin(&app, SessionState::Joining)?;
    trace("joining a room");
    let r = guest::join(app.clone(), music_root(&root), invite).await;
    Ok(begun(&app, r.map(Session::Guest)))
}

/// Leave the room (the host without a hand-over: end it for everyone); stop joining or waiting.
#[tauri::command]
pub fn together_leave(t: State<'_, Together>) -> Result<(), String> {
    if current(&t).is_none() {
        *t.error.lock().unwrap() = None;
        return Ok(());
    }
    on_session(
        &t,
        |l| {
            l.end();
            Ok(())
        },
        |l| {
            l.s.leave();
            Ok(())
        },
    )
}

/// End the room for everyone (the host's, confirmed in the window).
#[tauri::command]
pub fn together_end(t: State<'_, Together>) -> Result<(), String> {
    host_only(&t, |l| {
        l.end();
        Ok(())
    })
}

/// Something to do to the room (within your role).
#[tauri::command]
pub fn together_ask(t: State<'_, Together>, action: Action) -> Result<(), String> {
    action.check()?;
    let a = action.clone();
    on_session(
        &t,
        move |l| l.s.ask(a),
        move |l| {
            l.press(action);
            Ok(())
        },
    )
}

/// Something asked of whoever controls (no role needed).
#[tauri::command]
pub fn together_request(t: State<'_, Together>, action: Action) -> Result<(), String> {
    action.check()?;
    let a = action.clone();
    on_session(
        &t,
        move |l| l.s.request(a),
        move |l| {
            l.s.request(action);
            Ok(())
        },
    )
}

/// Add songs to the room: the host's from its library or the catalog; a guest's online songs.
#[tauri::command]
pub fn together_add(
    t: State<'_, Together>,
    root: String,
    items: Vec<QueueItem>,
    next: bool,
) -> Result<(), String> {
    let root = music_root(&root);
    let them = items.clone();
    on_session(
        &t,
        move |l| l.add_items(&root, &them, next),
        move |l| {
            let songs: Vec<Song> = items
                .iter()
                .filter(|it| it.path.is_empty())
                .filter_map(|it| sources::song_of_item(it, None))
                .collect();
            if songs.is_empty() {
                return Err("Only online songs can go in someone else's room (your own files stay with you).".into());
            }
            l.press(Action::Add { songs, next });
            Ok(())
        },
    )
}

#[tauri::command]
pub fn together_undo(t: State<'_, Together>) -> Result<(), String> {
    host_only(&t, |l| l.s.undo())
}

#[tauri::command]
pub fn together_admit(t: State<'_, Together>, id: u32) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.admit(id);
        Ok(())
    })
}

#[tauri::command]
pub fn together_deny(t: State<'_, Together>, id: u32) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.deny(id);
        Ok(())
    })
}

#[tauri::command]
pub fn together_remove(t: State<'_, Together>, id: u32, block: bool) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.remove(id, block);
        Ok(())
    })
}

#[tauri::command]
pub fn together_role(t: State<'_, Together>, id: u32, role_name: String) -> Result<(), String> {
    let r = role(&role_name)?;
    host_only(&t, move |l| {
        l.s.act(|room, now| room.set_role(id, r, now));
        Ok(())
    })
}

#[tauri::command]
pub fn together_new_role(t: State<'_, Together>, role_name: String) -> Result<(), String> {
    let r = role(&role_name)?;
    host_only(&t, move |l| {
        l.s.act(|room, now| room.set_new_role(r, now));
        Ok(())
    })
}

/// Between songs: gapless, a cut, or a crossfade of 2, 4, 6 or 8 s.
#[tauri::command]
pub fn together_transition(t: State<'_, Together>, transition: Transition) -> Result<(), String> {
    host_only(&t, move |l| {
        let mut r = Ok(());
        l.s.act(|room, now| match room.set_transition(transition, now) {
            Ok(c) => c,
            Err(e) => {
                r = Err(e);
                Default::default()
            }
        });
        r
    })
}

/// Remotes may change the host's speaker's volume (B17).
#[tauri::command]
pub fn together_guests_volume(t: State<'_, Together>, on: bool) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.act(|room, now| room.set_guests_volume(on, now));
        Ok(())
    })
}

#[tauri::command]
pub fn together_let_in_without_asking(t: State<'_, Together>, on: bool) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.set_let_in(on);
        Ok(())
    })
}

/// Today's room panel's switch: each person chooses how they listen now (protocol 3).
#[tauri::command]
pub fn together_same_room(_on: bool) -> Result<(), String> {
    Err("Each person chooses how they listen now: Play here too, or on the host's speaker.".into())
}

/// The room seen on this network (D19): the host's beacon and its door for people nearby
/// follow it within a quarter second.
#[tauri::command]
pub fn together_visible(t: State<'_, Together>, on: bool) -> Result<(), String> {
    t.visible.store(on, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// How you listen: on the host's speaker (a remote), every device, or apart (D4).
#[tauri::command]
pub fn together_mode(t: State<'_, Together>, mode: ListenMode) -> Result<(), String> {
    on_session(
        &t,
        move |l| {
            if mode == ListenMode::OneSpeaker {
                return Err("The host plays the room's speaker.".into());
            }
            l.s.conductor().set_mode(mode);
            Ok(())
        },
        move |l| {
            l.s.conductor().set_mode(mode);
            Ok(())
        },
    )
}

/// Pause just for you (the room goes on), or come back to where the room is.
#[tauri::command]
pub fn together_step_out(t: State<'_, Together>, out: bool) -> Result<(), String> {
    let f = move |c: &mut together::conductor::Conductor<QueueItem>| {
        if out { c.step_out() } else { c.rejoin() }
    };
    on_session(
        &t,
        move |l| {
            f(l.s.conductor());
            Ok(())
        },
        move |l| {
            f(l.s.conductor());
            Ok(())
        },
    )
}

/// Start cleanly with the next song instead of catching up (A17).
#[tauri::command]
pub fn together_start_with_next(t: State<'_, Together>) -> Result<(), String> {
    guest_only(&t, |l| {
        l.s.conductor().start_with_next();
        Ok(())
    })
}

/// Play the host's copy of the song playing (E10: another version).
#[tauri::command]
pub fn together_use_host_copy(t: State<'_, Together>) -> Result<(), String> {
    guest_only(&t, |l| {
        l.s.conductor().use_host_copy();
        Ok(())
    })
}

/// The host suggests a lighter copy to someone whose connection is slow (G7).
#[tauri::command]
pub fn together_lighter(t: State<'_, Together>, id: u32) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.advise(id, Advice::LighterCopy);
        Ok(())
    })
}

/// The host suggests someone slow start with the next song (G7).
#[tauri::command]
pub fn together_next_song_for(t: State<'_, Together>, id: u32) -> Result<(), String> {
    host_only(&t, move |l| {
        l.s.advise(id, Advice::StartWithNext);
        Ok(())
    })
}

/// Hand the room to someone else (4.15): WP20's.
#[tauri::command]
pub fn together_hand_over(_id: u32) -> Result<(), String> {
    Err("Handing the room over comes in a later version. For now, ending the room gives everyone their own music back.".into())
}

/// "Line up by ear": WP19's clicks.
#[tauri::command]
pub fn together_line_up(_on: bool) -> Result<(), String> {
    Err(
        "Lining up by ear comes in a later version. Set the delay with the slider meanwhile."
            .into(),
    )
}

/// Ask to join a room heard on this network (A15): its beacon made into an invite whose secret
/// is the room's door code, so the host always lets you in by hand (D19).
#[tauri::command]
pub async fn together_join_nearby(app: AppHandle, root: String, id: u32) -> Result<View, String> {
    let b = app
        .state::<Together>()
        .rooms
        .lock()
        .unwrap()
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, b)| b.clone())
        .ok_or_else(|| "That room isn't on this network any more.".to_owned())?;
    let invite = Invite {
        to: b.to,
        secret: b.door,
        until: b.until,
        host: b.host,
    };
    if !invite.open_at(unix_now()) {
        return Err("That room's invite has ended.".into());
    }
    begin(&app, SessionState::Joining)?;
    trace("joining a room nearby");
    let r = guest::join(app.clone(), music_root(&root), invite).await;
    Ok(begun(&app, r.map(Session::Guest)))
}

/// "Reopen your room?" or "Rejoin Asha's room?" (I6, I7): yes goes back to it; no forgets it.
#[tauri::command]
pub async fn together_resume(app: AppHandle, yes: bool) -> Result<View, String> {
    if yes {
        return resume(&app).await;
    }
    let t = app.state::<Together>();
    let k = kept();
    k.clear_host();
    k.clear_guest();
    *t.offer.lock().unwrap() = Some(None);
    emit_idle(&app);
    Ok(idle_view(&app))
}

/// Back to the room the records offer: the host's reopened, or a guest's joined again with the
/// same invite (so the same key, so the same member).
async fn resume(app: &AppHandle) -> Result<View, String> {
    let (what, k) = (the_offer(&app.state::<Together>()), kept());
    match what.map(|o| o.resume) {
        Some(Resume::Reopen) => {
            let record = k
                .host()
                .ok_or_else(|| "Your room isn't kept any more.".to_owned())?;
            begin_with(app, SessionState::Starting, false)?;
            trace("reopening the room");
            let r = host::reopen(app.clone(), record).await;
            Ok(begun(app, r.map(Session::Host)))
        }
        Some(Resume::Rejoin { .. }) => {
            let record = k
                .guest()
                .ok_or_else(|| "That room isn't kept any more.".to_owned())?;
            begin_with(app, SessionState::Joining, false)?;
            trace("rejoining a room");
            let root = music_root(&crate::playback::current_root(app));
            let r = guest::join(app.clone(), root, record.invite).await;
            Ok(begun(app, r.map(Session::Guest)))
        }
        None => Err("There's no room to go back to.".into()),
    }
}

/// The room just left, as a playlist: its online songs, and your own files of its songs.
#[tauri::command]
pub async fn together_save_queue(app: AppHandle) -> Result<String, String> {
    let songs = app.state::<Together>().last_queue.lock().unwrap().clone();
    if songs.is_empty() {
        return Err("There's no room's queue to save.".into());
    }
    let root = crate::playback::current_root(&app);
    if root.is_empty() {
        return Err("Open your library first.".into());
    }
    let root = music_root(&root);
    tauri::async_runtime::spawn_blocking(move || {
        let rootp = std::path::PathBuf::from(&root);
        let (_, lib) = crate::load_lib(&rootp).map_err(|e| e.to_string())?;
        save_room_queue(&rootp, &lib, &songs, &date_of(unix_now()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The room's songs as a playlist in the library at `root` (I10), in the room's order: the
/// catalog's songs by their id, your own files of the others; songs only the host had are left
/// out. Named for the day ("Listened together 2026-10-22", then "(2)" for a second room).
fn save_room_queue(
    root: &std::path::Path,
    lib: &engine::library::Library,
    songs: &[Song],
    date: &str,
) -> Result<String, String> {
    let items: Vec<engine::playlists::Item> = songs
        .iter()
        .filter_map(|s| sources::playlist_item(s, lib))
        .collect();
    if items.is_empty() {
        return Err("None of the room's songs can go in a playlist here.".into());
    }
    let taken: std::collections::HashSet<String> = engine::playlists::list(root, lib)
        .into_iter()
        .map(|p| p.name.to_lowercase())
        .collect();
    let base = format!("Listened together {date}");
    let name = (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base} ({n})")
            }
        })
        .find(|n| !taken.contains(&n.to_lowercase()))
        .expect("a free name");
    engine::playlists::create(root, lib, &name)?;
    engine::playlists::add_with(root, lib, &name, &items, false)?;
    Ok(name)
}

/// A date as a playlist's name says it ("2026-10-22").
fn date_of(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    // The civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
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
    if current(&app.state::<Together>()).is_none() {
        emit_idle(&app);
    }
    Ok(())
}

/// Your output's delay (Bluetooth, a receiver): how much later than the device says you hear.
#[tauri::command]
pub fn together_set_delay(app: AppHandle, t: State<'_, Together>, ms: i32) -> Result<(), String> {
    let ms = ms.clamp(-500, 500);
    let out = app
        .state::<Playback>()
        .output_id()
        .unwrap_or_else(|| "default".into());
    let mut s = engine::settings::load();
    s.listening.together_delay.insert(out, ms);
    engine::settings::save(&s)?;
    if current(&t).is_some() {
        let f = move |c: &mut together::conductor::Conductor<QueueItem>| {
            c.set_extra_latency(ms * 1_000)
        };
        let _ = on_session(
            &t,
            move |l| {
                f(l.s.conductor());
                Ok(())
            },
            move |l| {
                f(l.s.conductor());
                Ok(())
            },
        );
    } else {
        emit_idle(&app);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{date_of, save_room_queue};
    use together::proto::Song;

    #[test]
    fn a_date_is_said_as_a_playlist_name_says_it() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_792_627_200), "2026-10-22");
        assert_eq!(date_of(951_782_400), "2000-02-29");
    }

    fn song(n: u32, online: bool) -> Song {
        Song {
            key: format!("k{n}"),
            title: format!("Song {n}"),
            artists: vec!["Someone".into()],
            album: None,
            duration_ms: Some(200_000),
            isrc: None,
            mbid: None,
            online_id: online.then(|| format!("vid{n:07}")),
            host_file: !online,
            format: None,
            artwork_url: None,
            added_by: None,
        }
    }

    #[test]
    fn i10_the_rooms_queue_becomes_a_playlist_in_its_order_and_a_second_room_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let lib = engine::library::Library::of_tracks(Vec::new());
        // A song only the host had can't be one: left out; the others in the room's order.
        let songs = vec![song(3, true), song(1, false), song(2, true)];
        let name = save_room_queue(dir.path(), &lib, &songs, "2026-10-22").unwrap();
        assert_eq!(name, "Listened together 2026-10-22");
        let (entries, _) = engine::playlists::read(dir.path(), &lib, &name).unwrap();
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Song 3", "Song 2"]);
        let again = save_room_queue(dir.path(), &lib, &songs[..1], "2026-10-22").unwrap();
        assert_eq!(again, "Listened together 2026-10-22 (2)");
        assert!(save_room_queue(dir.path(), &lib, &[song(9, false)], "2026-10-22").is_err());
    }
}
