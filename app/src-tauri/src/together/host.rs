//! Hosting a room: this Venuta is the room. The connections, the knock and everyone's messages
//! are the room's networking (`net_host`); this is the app around it: the room made from what's
//! playing, the host's files and covers, the host's own player following the room like
//! everyone's, and the window told of every change.

use super::follow::{self, Follower, Report};
use super::net::{self, News};
use super::net_host::{HostHooks, HostSession};
use super::sources::HostFiles;
use super::{emit, random_hex};
use crate::playback::{Playback, QueueItem};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, Weak};
use tauri::{AppHandle, Manager};
use together::invite::Invite;
use together::proto::{Action, Hearing, Role};
use together::room::{HOST, Room};

pub struct Host {
    session: Arc<HostSession>,
    glue: Arc<Glue>,
    /// The covers of the host's songs in the room (made here, served to the people in it).
    pub covers: Arc<super::covers::Covers>,
    root: String,
}

/// The room's connections, the room and its invite.
impl std::ops::Deref for Host {
    type Target = HostSession;
    fn deref(&self) -> &HostSession {
        &self.session
    }
}

/// What the room's networking asks of the app.
struct Glue {
    app: AppHandle,
    files: HostFiles,
    covers: Arc<super::covers::Covers>,
    follow: Mutex<Option<Sender<follow::In>>>,
}

impl HostHooks for Glue {
    fn changed(&self) {
        emit(&self.app);
    }
    fn knocked(&self, name: &str) {
        super::notify_knock(&self.app, name);
    }
    fn trace(&self, what: &str) {
        super::trace(what);
    }
    fn news(&self, news: News) {
        if let Some(tx) = self.follow.lock().unwrap().as_ref() {
            let _ = tx.send(follow::In::from(news));
        }
    }
    fn file(&self, key: &str) -> Option<(PathBuf, String)> {
        self.files
            .get(key)
            .map(|(p, format)| (p, super::sources::mime_of(&format).to_owned()))
    }
    fn cover(&self, key: &str) -> Option<PathBuf> {
        self.covers.get(key)
    }
}

impl Host {
    /// Open a room with what's playing: the host's queue becomes the room's.
    pub async fn start(
        app: AppHandle,
        root: String,
        items: Vec<QueueItem>,
        index: usize,
        position_ms: u64,
        playing: bool,
    ) -> Result<Arc<Host>, String> {
        let reach = net::Reach::from_settings();
        let ep = net::endpoint(&reach, true, None).await?;
        super::trace("room: endpoint open");
        let name = super::my_name();
        let lib = crate::load_lib(std::path::Path::new(&root))
            .ok()
            .map(|(_, l)| l);
        let files = HostFiles::default();
        let mut songs = Vec::new();
        let mut at = None;
        let mut covers_of = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if let Some(s) = super::sources::song_of_item(it, lib.as_deref()) {
                if s.host_file {
                    files.add(&s.key, std::path::Path::new(&root).join(&it.path));
                    covers_of.push((s.key.clone(), it.dir.clone()));
                }
                if i == index {
                    at = Some(songs.len());
                }
                songs.push(s);
            }
        }
        let secret = random_hex(16);
        let ticket = net::ticket(&ep, &reach).await;
        super::trace("room: invite ready");
        let until = super::unix_now() + 24 * 3600;
        let invite = Invite {
            to: ticket,
            secret: secret.clone(),
            until,
            host: name.clone(),
        };
        let room = Room::new(&name, &secret, songs);
        let covers = Arc::new(super::covers::Covers::default());
        let glue = Arc::new(Glue {
            app: app.clone(),
            files,
            covers: covers.clone(),
            follow: Mutex::default(),
        });
        let session = HostSession::new(ep, room, invite, glue.clone());
        let host = Arc::new(Host {
            session,
            glue,
            covers,
            root: root.clone(),
        });
        host.make_covers(covers_of);
        // The host's own player follows the room.
        let clock = host.clock;
        let catalog = app.state::<Playback>().catalog();
        let tx = follow::start(Follower {
            seat: Arc::new(follow::AppSeat(app.clone())),
            root: root.clone(),
            lib,
            catalog,
            local_only: reach.local_only,
            host_stream: None,
            hosting: true,
            to_local: Arc::new(move |us| Some(clock.instant(us))),
            report: Box::new(HostReport(Arc::downgrade(&host.session))),
            delay_ms: super::my_delay(&app),
            owner: 0,
        });
        *host.glue.follow.lock().unwrap() = Some(tx);
        super::trace("room: follower started");
        // What was playing goes on, everyone getting it ready where it is.
        if let Some(i) = at {
            let now = host.clock.now_us();
            let c = {
                let mut r = host.room.lock().unwrap();
                let c = r
                    .ask(HOST, Action::Jump { index: i }, now)
                    .unwrap_or_default();
                if position_ms > 0 {
                    r.ask(HOST, Action::Seek { position_ms }, now)
                        .unwrap_or_default()
                } else {
                    c
                }
            };
            host.apply(c);
            if !playing {
                let c = host
                    .room
                    .lock()
                    .unwrap()
                    .ask(HOST, Action::Pause, now)
                    .unwrap_or_default();
                host.apply(c);
            }
        }
        super::trace("room: playing on");
        // People come in, and the room's clock moves songs along.
        host.session.run();
        Ok(host)
    }

    pub fn to_follower(&self, m: follow::In) {
        if let Some(tx) = self.glue.follow.lock().unwrap().as_ref() {
            let _ = tx.send(m);
        }
    }

    /// Add the host's own songs (from its library or the catalog) to the room.
    pub fn add_items(&self, root: &str, items: &[QueueItem], next: bool) -> Result<(), String> {
        let lib = crate::load_lib(std::path::Path::new(root))
            .ok()
            .map(|(_, l)| l);
        let mut covers_of = Vec::new();
        let songs: Vec<_> = items
            .iter()
            .filter_map(|it| {
                let s = super::sources::song_of_item(it, lib.as_deref())?;
                if s.host_file {
                    self.glue
                        .files
                        .add(&s.key, std::path::Path::new(root).join(&it.path));
                    if self.covers.get(&s.key).is_none() {
                        covers_of.push((s.key.clone(), it.dir.clone()));
                    }
                }
                Some(s)
            })
            .collect();
        if songs.is_empty() {
            return Err("Those songs can't go in the room.".into());
        }
        self.make_covers(covers_of);
        self.ask(Action::Add { songs, next })
    }

    /// The covers of these songs of the host's (key, album folder), made off the async threads;
    /// the window shows them as they come.
    fn make_covers(&self, songs: Vec<(String, String)>) {
        if songs.is_empty() {
            return;
        }
        let root = std::path::PathBuf::from(&self.root);
        let weak = Arc::downgrade(&self.glue);
        tauri::async_runtime::spawn_blocking(move || {
            let made = super::covers::make(&root, &songs);
            if let Some(g) = weak.upgrade() {
                for (key, path) in made {
                    g.covers.put(&key, path);
                }
                emit(&g.app);
            }
        });
    }
}

impl From<News> for follow::In {
    fn from(n: News) -> Self {
        match n {
            News::Room(v) => follow::In::Room(v),
            News::Prepare {
                rev,
                index,
                position_ms,
            } => follow::In::Prepare {
                rev,
                index,
                position_ms,
            },
            News::Stop => follow::In::Stop,
        }
    }
}

/// The host's own player reports straight to the room.
struct HostReport(Weak<HostSession>);

impl Report for HostReport {
    fn ready(&self, rev: u64, index: usize) {
        if let Some(h) = self.0.upgrade() {
            let c = h
                .room
                .lock()
                .unwrap()
                .ready(HOST, rev, index, h.clock.now_us());
            h.apply(c);
        }
    }
    fn hearing(&self, rev: u64, index: usize, how: Hearing) {
        if let Some(h) = self.0.upgrade() {
            let c = h.room.lock().unwrap().hearing(HOST, rev, index, how);
            h.apply(c);
        }
    }
    fn sync(&self, error_ms: i32) {
        if let Some(h) = self.0.upgrade() {
            let c = h.room.lock().unwrap().sync(HOST, error_ms);
            h.apply(c);
        }
    }
    fn length(&self, ms: u64) {
        if let Some(h) = self.0.upgrade() {
            h.room.lock().unwrap().set_length(ms);
        }
    }
}

/// The roles the host can give, by name (from the window).
pub fn role_of(s: &str) -> Option<Role> {
    match s {
        "control" => Some(Role::Control),
        "add" => Some(Role::Add),
        "listen" => Some(Role::Listen),
        _ => None,
    }
}
