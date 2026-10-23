//! Hosting a room: this Venuta is the room. The room and the host's session are `together`'s
//! (`HostSession`: the door, Room v2, everyone's messages and clock pings, the host's own
//! conductor); the connection is `together_net`'s `IrohTransport`. This is the app around them:
//! the room made from what's playing without touching it (A1), the host's files and covers,
//! the session's thread, and the window told of every change.

use super::AppPlayer;
use super::sources::{AppCopies, HostFiles};
use crate::playback::{Playback, QueueItem, RoomNow};
use player::measure::{EnvelopeMaker, Making};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use together::anchor::EntryId;
use together::clock_map::{self, ClockMap};
use together::conductor::Conductor;
use together::invite::Invite;
use together::measure::{Measured, Shelf};
use together::proto::Song;
use together::proto::v3::Action;
use together::room::v2::{HOST, HostNote, Opening, Room};
use together::room_player::{ListenMode, RoomPlayer, SyncMode};
use together::session::{HostConfig, HostSession, WindowExtras};
use together::time::RealClock;
use together::transport::{Get, MediaInfo, MediaServer, PeerId};
use together::view::{InviteView, SessionState, View};
use together_net::transport::IrohTransport;

/// The host's side, on its own thread.
pub struct Live {
    pub s: HostSession<QueueItem>,
    t: IrohTransport,
    app: AppHandle,
    invite: Invite,
    root: String,
    files: Arc<HostFiles>,
    covers: Arc<super::covers::Covers>,
    /// The host's own songs in the room, as its queue had them, by key.
    own: Arc<Mutex<HashMap<String, QueueItem>>>,
    /// Knocks the host was told of.
    knocks: HashSet<u32>,
    /// The host ended it (not the hour without an action).
    ended_by_host: bool,
}

/// The host's files and covers, as the transport serves them (the session lets members only).
struct HostMedia {
    files: Arc<HostFiles>,
    covers: Arc<super::covers::Covers>,
    lengths: Arc<Mutex<HashMap<String, u64>>>,
}

impl MediaServer for HostMedia {
    fn open(&self, _: PeerId, _: &Get) -> Option<Arc<[u8]>> {
        None
    }
    fn describe(&self, _: PeerId, get: &Get) -> Option<MediaInfo> {
        if let Some(key) = get.get.strip_prefix("cover:") {
            return Some(MediaInfo {
                file: Some(self.covers.get(key)?),
                mime: Some("image/jpeg".into()),
                duration_ms: None,
            });
        }
        let (path, format) = self.files.get(&get.get)?;
        Some(MediaInfo {
            file: Some(path),
            mime: Some(super::sources::mime_of(&format).into()),
            duration_ms: self.lengths.lock().ok()?.get(&get.get).copied(),
        })
    }
}

/// A server put in place once the session that gates it exists.
#[derive(Default)]
struct Later(OnceLock<Arc<dyn MediaServer>>);

impl MediaServer for Later {
    fn open(&self, from: PeerId, get: &Get) -> Option<Arc<[u8]>> {
        self.0.get()?.open(from, get)
    }
    fn describe(&self, from: PeerId, get: &Get) -> Option<MediaInfo> {
        self.0.get()?.describe(from, get)
    }
}

/// Open a room with what's playing: the host's queue becomes the room's, and the song playing
/// goes on as the room's first, untouched (A1).
pub async fn start(
    app: AppHandle,
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    position_ms: u64,
    playing: bool,
) -> Result<Sender<super::HostWork>, String> {
    let reach = super::net::reach_from_settings();
    let ep = super::net::endpoint(&reach, true, None).await?;
    super::trace("room: endpoint open");
    let name = super::my_name();
    let lib = crate::load_lib(std::path::Path::new(&root))
        .ok()
        .map(|(_, l)| l);
    let files = Arc::new(HostFiles::default());
    let own: Arc<Mutex<HashMap<String, QueueItem>>> = Arc::default();
    let lengths: Arc<Mutex<HashMap<String, u64>>> = Arc::default();
    let catalog = app.state::<Playback>().catalog();
    let resolved = app.state::<Playback>().resolve(&root, items);
    let mut songs = Vec::new();
    let mut at = None;
    let mut covers_of = Vec::new();
    for (i, it) in resolved.iter().enumerate() {
        if let Some(s) = super::sources::song_of_item(it, lib.as_deref()) {
            if s.host_file {
                files.add(&s.key, std::path::Path::new(&root).join(&it.path));
                covers_of.push((s.key.clone(), it.dir.clone()));
            }
            if let Some(d) = s.duration_ms {
                lengths.lock().unwrap().insert(s.key.clone(), d);
            }
            own.lock().unwrap().insert(s.key.clone(), it.clone());
            if i == index {
                at = Some(songs.len());
            }
            songs.push(s);
        }
    }
    let secret = super::random_hex(16);
    let ticket = super::net::ticket(&ep, &reach).await;
    super::trace("room: invite ready");
    let until = super::unix_now() + 24 * 3600;
    let invite = Invite {
        to: ticket,
        secret: secret.clone(),
        until,
        host: name.clone(),
    };
    // The host's player: the room made from what plays, in place, or taken as it is.
    let map = clock_map::handle();
    map.store(Arc::new(Some(ClockMap::identity())));
    let player: Arc<dyn RoomPlayer<QueueItem>> = Arc::new(AppPlayer(app.clone()));
    // Room v2 numbers its entries in the queue's order: the song playing's id, from a room made
    // the same way.
    let entry_at = |i: usize| -> EntryId {
        Room::new(&name, &secret, songs.clone(), Opening::Resting).queue()[i].id
    };
    let (handle, opening, playing_entry) = match at {
        Some(i) if playing => {
            let entry = entry_at(i);
            let handle = player.adopt(map.clone(), SyncMode::Apart, entry);
            // Where the music is, in host time, from the output's presentation.
            let t0 = Instant::now();
            let mut heard = None;
            while t0.elapsed() < Duration::from_millis(1_500) {
                if let Some(h) = handle.status().heard {
                    heard = Some(h);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            match heard {
                Some(h) => (
                    handle,
                    Opening::Playing {
                        index: i,
                        content_us: h.content_us,
                        host_us: h.host_us,
                    },
                    Some(entry),
                ),
                None => (
                    handle,
                    Opening::Held {
                        index: i,
                        content_us: position_ms as i64 * 1_000,
                    },
                    None,
                ),
            }
        }
        Some(i) => (
            player.attach(map.clone(), SyncMode::Apart),
            Opening::Held {
                index: i,
                content_us: position_ms as i64 * 1_000,
            },
            None,
        ),
        None => (
            player.attach(map.clone(), SyncMode::Apart),
            Opening::Resting,
            None,
        ),
    };
    super::trace(&format!("room: opened as {opening:?}"));
    let room = Room::new(&name, &secret, songs, opening);
    let mut copies = AppCopies::new(
        root.clone(),
        lib,
        catalog,
        reach.local_only,
        None,
        resolver(&app),
    );
    copies.own = own.clone();
    // The host's copies are the room's references: their envelopes made on the player's
    // downloads and served on the env: stream (WP11).
    let shelf = Shelf::new();
    let copies = Measured::host(
        Box::new(copies),
        Box::new(EnvelopeMaker::<QueueItem>::new(
            super::streams(&app),
            Making::Threads,
        )),
        shelf.clone(),
    );
    let mut conductor = Conductor::new(player, map, Box::new(copies), ListenMode::Apart)
        .with_handle(handle, playing_entry);
    conductor.set_extra_latency(super::my_delay(&app) * 1_000);
    let later = Arc::new(Later::default());
    let t = IrohTransport::host(ep, Arc::new(RealClock), later.clone());
    let open = invite.clone();
    let mut s = HostSession::new(
        Arc::new(t.clone()),
        Arc::new(RealClock),
        room,
        conductor,
        HostConfig {
            invite_open: Arc::new(move || open.open_at(super::unix_now())),
        },
    )
    .with_envelopes(shelf)
    .with_lyrics(Box::new(FileLyrics {
        files: files.clone(),
        found: HashMap::new(),
    }));
    let covers = Arc::new(super::covers::Covers::default());
    let _ = later.0.set(s.media(Arc::new(HostMedia {
        files: files.clone(),
        covers: covers.clone(),
        lengths,
    })));
    let live = Live {
        s,
        t,
        app: app.clone(),
        invite,
        root,
        files,
        covers,
        own,
        knocks: HashSet::new(),
        ended_by_host: false,
    };
    live.make_covers(covers_of);
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("venuta-room".into())
        .spawn(move || run(live, rx))
        .map_err(|e| e.to_string())?;
    super::trace("room: open");
    Ok(tx)
}

/// How a room's copies are bound to where they play from (`Playback::resolve`).
pub(super) fn resolver(app: &AppHandle) -> super::sources::Resolver {
    let app = app.clone();
    Arc::new(move |root: &str, it: QueueItem| {
        app.state::<Playback>()
            .resolve(root, vec![it])
            .pop()
            .expect("one item")
    })
}

/// The session's thread: the window's work, the session stepped at least every 10 ms, the view
/// and the player's bar told; it ends a moment after the room does.
fn run(mut live: Live, rx: Receiver<super::HostWork>) {
    let mut last_view = String::new();
    let mut last_tick = Instant::now();
    let mut over_at: Option<Instant> = None;
    let mut queue_version = (0u64, String::new());
    loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(f) => f(&mut live),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => live.end(),
        }
        live.s.step();
        live.after_step();
        let playing = live.s.room().anchor().is_some_and(|a| a.playing);
        let v = live.view();
        let json = serde_json::to_string(&v).unwrap_or_default();
        let due = playing && last_tick.elapsed() >= Duration::from_millis(100);
        if json != last_view || due {
            last_view = json;
            last_tick = Instant::now();
            super::publish(&live.app, v);
            live.bar(&mut queue_version);
        }
        if live.s.ended() {
            let at = *over_at.get_or_insert_with(Instant::now);
            if at.elapsed() >= Duration::from_millis(500) {
                break;
            }
        }
    }
    let queue: Vec<Song> = live
        .s
        .room()
        .queue()
        .iter()
        .map(|e| e.song.clone())
        .collect();
    let last = live.view();
    let t = live.t.clone();
    tauri::async_runtime::spawn(async move { t.close().await });
    super::session_over(&live.app, queue, last);
}

impl Live {
    /// End the room for everyone.
    pub fn end(&mut self) {
        if !self.s.ended() {
            super::trace("room: the host ended it");
            self.ended_by_host = true;
            self.s.end();
        }
    }

    fn after_step(&mut self) {
        // The speaker's volume, for remotes the host lets change it (B17).
        let v = self.app.state::<Playback>().volume_percent();
        self.s.set_volume(v);
        if let Some(v) = self.s.take_volume() {
            let _ = self
                .app
                .state::<Playback>()
                .with(|p| p.send(player::engine::Cmd::Volume(f32::from(v) / 100.0)));
        }
        for n in self.s.take_notes() {
            match n {
                HostNote::SameRoomOff { id } => {
                    let who = self.name_of(id);
                    super::note(
                        &self.app,
                        &format!("Same room turned off: {who}'s connection is too slow."),
                    );
                }
                HostNote::Slow { id } => {
                    super::trace(&format!("room: {} missed three starts", self.name_of(id)));
                }
            }
        }
        // A knock: the host is told (a notification when Venuta isn't in front).
        let waiting = self.s.room().view_for(HOST).waiting;
        for (id, name) in &waiting {
            if self.knocks.insert(*id) {
                super::notify_knock(&self.app, name);
            }
        }
    }

    fn name_of(&self, id: u32) -> String {
        self.s
            .room()
            .view_for(HOST)
            .members
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "Someone".into())
    }

    /// The window's view.
    fn view(&self) -> View {
        let link = self.invite.link();
        let extras = WindowExtras {
            name: super::my_name(),
            local_only: engine::settings::load().listening.local_only,
            let_in: self.s.room().lets_in_without_asking(),
            visible: self
                .app
                .state::<super::Together>()
                .visible
                .load(std::sync::atomic::Ordering::Relaxed),
            output: super::output_view(&self.app),
            invite: Some(InviteView {
                qr_svg: super::qr_svg(&link),
                link,
                code: self.invite.code(),
                host: self.invite.host.clone(),
                until: self.invite.until,
            }),
            volume: self.app.state::<Playback>().volume_percent(),
            message: None,
        };
        let mut v = self.s.window(&extras);
        if let Some(room) = v.room.as_mut() {
            self.covers.decorate(room);
        }
        if v.state == SessionState::Ended {
            v.message = Some(if self.ended_by_host {
                "You ended the room for everyone. Everyone's own music is back, paused.".into()
            } else {
                "The room ended itself after an hour with nothing playing.".into()
            });
        }
        v
    }

    /// What the room plays, on the player's own bar.
    fn bar(&self, version: &mut (u64, String)) {
        if self.s.ended() {
            return;
        }
        let room = self.s.room();
        let queue = room.queue();
        let key = queue
            .iter()
            .map(|e| e.id.0.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let changed = key != version.1;
        if changed {
            version.0 += 1;
            version.1 = key;
        }
        let items: Vec<QueueItem> = {
            let own = self.own.lock().unwrap();
            queue
                .iter()
                .map(|e| super::display_item(&e.song, own.get(&e.song.key)))
                .collect()
        };
        let now_us = self.s.now_us();
        let (index, playing, position_ms, duration_ms) = match room.anchor() {
            Some(a) => {
                let i = queue
                    .iter()
                    .position(|e| e.id == a.entry)
                    .map_or(-1, |i| i as i64);
                let d = queue
                    .iter()
                    .find(|e| e.id == a.entry)
                    .and_then(|e| e.song.duration_ms)
                    .unwrap_or(0);
                (
                    i,
                    a.playing,
                    (a.content_at(now_us).max(0) / 1_000) as u64,
                    d,
                )
            }
            None => (-1, false, 0, 0),
        };
        let pb = self.app.state::<Playback>();
        let first = !pb.in_room();
        pb.set_room_now_quiet(Some(RoomNow {
            items,
            index,
            playing,
            position_ms,
            duration_ms,
            buffering: false,
            from: "Your room".into(),
        }));
        pb.room_tick(changed || first);
    }

    /// Add the host's own songs (from its library or the catalog) to the room.
    pub fn add_items(&mut self, root: &str, items: &[QueueItem], next: bool) -> Result<(), String> {
        let lib = crate::load_lib(std::path::Path::new(root))
            .ok()
            .map(|(_, l)| l);
        let resolved = self.app.state::<Playback>().resolve(root, items.to_vec());
        let mut covers_of = Vec::new();
        let songs: Vec<Song> = resolved
            .iter()
            .filter_map(|it| {
                let s = super::sources::song_of_item(it, lib.as_deref())?;
                if s.host_file {
                    self.files
                        .add(&s.key, std::path::Path::new(root).join(&it.path));
                    if self.covers.get(&s.key).is_none() {
                        covers_of.push((s.key.clone(), it.dir.clone()));
                    }
                }
                self.own.lock().unwrap().insert(s.key.clone(), it.clone());
                Some(s)
            })
            .collect();
        if songs.is_empty() {
            return Err("Those songs can't go in the room.".into());
        }
        self.make_covers(covers_of);
        self.s.ask(Action::Add { songs, next })
    }

    /// The covers of these songs of the host's (key, album folder), made off this thread.
    fn make_covers(&self, songs: Vec<(String, String)>) {
        if songs.is_empty() {
            return;
        }
        let root = PathBuf::from(&self.root);
        let covers = self.covers.clone();
        tauri::async_runtime::spawn_blocking(move || {
            for (key, path) in super::covers::make(&root, &songs) {
                covers.put(&key, path);
            }
        });
    }
}

/// The words of the host's own songs for remotes (WP18): read from each file's `.lrc` or tags
/// on a thread of its own, once per song.
struct FileLyrics {
    files: Arc<HostFiles>,
    found: HashMap<String, Arc<Mutex<Option<Option<together::proto::v3::Lyrics>>>>>,
}

impl together::session::HostLyrics for FileLyrics {
    fn of(&mut self, song: &Song) -> Option<Option<together::proto::v3::Lyrics>> {
        if let Some(slot) = self.found.get(&song.key) {
            return slot.lock().ok()?.clone();
        }
        let slot: Arc<Mutex<Option<Option<together::proto::v3::Lyrics>>>> = Arc::default();
        let Some((path, _)) = self.files.get(&song.key) else {
            *slot.lock().ok()? = Some(None);
            self.found.insert(song.key.clone(), slot);
            return Some(None);
        };
        let s2 = slot.clone();
        let spawned = std::thread::Builder::new()
            .name("venuta-room-lyrics".into())
            .spawn(move || {
                let l = crate::playback::lyrics_of_file(&path);
                let words = together::proto::v3::Lyrics {
                    lines: l
                        .lines
                        .into_iter()
                        .take(together::proto::v3::MAX_LYRIC_LINES)
                        .map(|l| {
                            let line: String = l
                                .text
                                .chars()
                                .filter(|c| !c.is_control())
                                .take(500)
                                .collect();
                            (l.start_ms.min(u64::from(u32::MAX)) as u32, line)
                        })
                        .collect(),
                    text: l
                        .text
                        .chars()
                        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                        .take(60_000)
                        .collect(),
                };
                let some = !(words.lines.is_empty() && words.text.trim().is_empty());
                let ok = words.check().is_ok();
                if let Ok(mut s) = s2.lock() {
                    *s = Some((some && ok).then_some(words));
                }
            });
        if spawned.is_err() {
            *slot.lock().ok()? = Some(None);
        }
        self.found.insert(song.key.clone(), slot);
        None
    }
}
