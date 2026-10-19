//! Hosting a room: this Venuta is the room. People connect to it, knock with the invite's
//! proof, wait to be let in (or come straight in), and get the room's news; what they ask is
//! checked against their role here. The host's own player follows the room like everyone's.
//! The host's files are streamed, on request, only to people in the room.

use super::follow::{self, Follower, Report};
use super::net::{self, Clock, Lines};
use super::sources::HostFiles;
use super::{emit, random_hex};
use crate::playback::{Playback, QueueItem};
use iroh::Endpoint;
use iroh::endpoint::Connection;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use together::invite::Invite;
use together::proto::{self, Action, Hearing, Role, ToGuest, ToHost};
use together::room::{Changes, HOST, Knock, Room};
use tokio::sync::mpsc;

type Out = mpsc::UnboundedSender<Vec<u8>>;

pub struct Host {
    pub ep: Endpoint,
    pub clock: Clock,
    pub room: Mutex<Room>,
    /// Each person's way to send to them: in the room, or waiting at the door.
    conns: Mutex<HashMap<u32, (Out, Connection)>>,
    waiting: Mutex<HashMap<u32, (Out, Connection)>>,
    pub files: HostFiles,
    /// The covers of the host's songs in the room (made here, served to the people in it).
    pub covers: super::covers::Covers,
    root: String,
    pub invite: Invite,
    follow: Mutex<Option<Sender<follow::In>>>,
    app: AppHandle,
}

/// A file request on its own stream: which host song, from which byte (to which, inclusive).
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Get {
    pub get: String,
    pub from: u64,
    #[serde(default)]
    pub to: Option<u64>,
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
        let host = Arc::new(Host {
            ep,
            clock: Clock::new(),
            room: Mutex::new(room),
            conns: Mutex::default(),
            waiting: Mutex::default(),
            files,
            covers: Default::default(),
            root: root.clone(),
            invite,
            follow: Mutex::default(),
            app: app.clone(),
        });
        host.make_covers(covers_of);
        // The host's own player follows the room.
        let clock = host.clock;
        let catalog = app.state::<Playback>().catalog();
        let tx = follow::start(Follower {
            app: app.clone(),
            root: root.clone(),
            lib,
            catalog,
            local_only: reach.local_only,
            host_stream: None,
            hosting: true,
            to_local: Arc::new(move |us| Some(clock.instant(us))),
            report: Box::new(HostReport(Arc::downgrade(&host))),
            delay_ms: super::my_delay(&app),
            owner: 0,
        });
        *host.follow.lock().unwrap() = Some(tx);
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
        // People come in.
        let h = host.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(incoming) = h.ep.accept().await {
                let h2 = h.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(conn) = incoming.await {
                        h2.serve(conn).await;
                    }
                });
            }
        });
        // The room's clock moves songs along and starts the ones everyone's ready for.
        let weak = Arc::downgrade(&host);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                let Some(h) = weak.upgrade() else { break };
                if h.ep.is_closed() {
                    break;
                }
                let c = h.room.lock().unwrap().tick(h.clock.now_us());
                h.apply(c);
            }
        });
        Ok(host)
    }

    /// Send what changed: the room to everyone, and a song to get ready.
    pub fn apply(&self, c: Changes) {
        if c.room {
            let (guest_view, host_view) = {
                let r = self.room.lock().unwrap();
                (r.view(false), r.view(true))
            };
            let line = proto::line(&ToGuest::Room { room: guest_view });
            for (out, _) in self.conns.lock().unwrap().values() {
                let _ = out.send(line.clone());
            }
            self.to_follower(follow::In::Room(host_view));
            emit(&self.app);
        }
        if let Some((rev, index, position_ms)) = c.prepare {
            let line = proto::line(&ToGuest::Prepare {
                rev,
                index,
                position_ms,
            });
            for (out, _) in self.conns.lock().unwrap().values() {
                let _ = out.send(line.clone());
            }
            self.to_follower(follow::In::Prepare {
                rev,
                index,
                position_ms,
            });
        }
    }

    pub fn to_follower(&self, m: follow::In) {
        if let Some(tx) = self.follow.lock().unwrap().as_ref() {
            let _ = tx.send(m);
        }
    }

    /// Someone connected: the knock, then their messages until they go.
    async fn serve(self: Arc<Self>, conn: Connection) {
        let key = conn.remote_id().to_string();
        let Ok((mut send, recv)) = conn.accept_bi().await else {
            return;
        };
        let mut lines = Lines::new(recv);
        let hello = match tokio::time::timeout(Duration::from_secs(10), lines.next()).await {
            Ok(Ok(Some(l))) => proto::read::<ToHost>(&l).ok(),
            _ => None,
        };
        let Some(ToHost::Hello {
            protocol,
            name,
            proof,
            ..
        }) = hello.filter(|h| proto::check_to_host(h).is_ok())
        else {
            conn.close(1u32.into(), b"hello");
            return;
        };
        let refuse = |why: &str| {
            proto::line(&ToGuest::Refused {
                why: why.to_owned(),
            })
        };
        if protocol != together::PROTOCOL {
            let _ = net::send_line(&mut send, &refuse("Your Venuta and the host's are different versions. Update both, then join again.")).await;
            let _ = send.finish();
            return;
        }
        if !self.invite.open_at(super::unix_now()) {
            let _ = net::send_line(
                &mut send,
                &refuse("This room's invite has ended. Ask for a new one."),
            )
            .await;
            let _ = send.finish();
            return;
        }
        let knock = self.room.lock().unwrap().knock(&key, &name, &proof);
        let (out, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        // Everything for this person goes through one writer.
        tauri::async_runtime::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if net::send_line(&mut send, &line).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        let id = match knock {
            Knock::Refused(why) => {
                let _ = out.send(refuse(&why));
                tokio::time::sleep(Duration::from_millis(300)).await;
                conn.close(2u32.into(), b"refused");
                return;
            }
            Knock::Waiting(id) => {
                self.waiting
                    .lock()
                    .unwrap()
                    .insert(id, (out.clone(), conn.clone()));
                let _ = out.send(proto::line(&ToGuest::Waiting));
                super::notify_knock(&self.app, &name);
                emit(&self.app);
                id
            }
            Knock::In(id) => {
                self.conns
                    .lock()
                    .unwrap()
                    .insert(id, (out.clone(), conn.clone()));
                self.welcome(id, &out);
                id
            }
        };
        // Their files requests, on streams of their own.
        let h = self.clone();
        let c2 = conn.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok((s, r)) = c2.accept_bi().await {
                let h3 = h.clone();
                tauri::async_runtime::spawn(async move { h3.stream_file(id, s, r).await });
            }
        });
        // Their messages.
        let mut pings: u32 = 0;
        let mut said_bye = false;
        loop {
            let line = match lines.next().await {
                Ok(Some(l)) => l,
                _ => break,
            };
            let got_us = self.clock.now_us();
            let Ok(m) = proto::read::<ToHost>(&line) else {
                continue;
            };
            if proto::check_to_host(&m).is_err() {
                continue;
            }
            let in_room = self.room.lock().unwrap().is_member(id);
            match m {
                ToHost::Ping { id: pid, t1 } => {
                    let _ = out.send(proto::line(&ToGuest::Pong {
                        id: pid,
                        t1,
                        t2: got_us,
                        t3: self.clock.now_us(),
                    }));
                    pings += 1;
                    if pings.is_multiple_of(8)
                        && let Some(rtt) = net::path_of(&conn).rtt_ms
                    {
                        self.room.lock().unwrap().round_trip(i64::from(rtt) * 1_000);
                        // Someone farther than this network now: same room goes back to apart.
                        let same_room = self.room.lock().unwrap().view(true).same_room;
                        if same_room && !self.all_on_one_network() {
                            self.set(|room| room.set_same_room(false));
                        }
                    }
                }
                _ if !in_room => {}
                ToHost::Ready { rev, index } => {
                    let c = self
                        .room
                        .lock()
                        .unwrap()
                        .ready(id, rev, index, self.clock.now_us());
                    self.apply(c);
                }
                ToHost::Hearing { rev, index, how } => {
                    let c = self.room.lock().unwrap().hearing(id, rev, index, how);
                    self.apply(c);
                }
                ToHost::Sync { error_ms } => {
                    self.room.lock().unwrap().sync(id, error_ms);
                }
                ToHost::Ask { action } => {
                    let r = self
                        .room
                        .lock()
                        .unwrap()
                        .ask(id, action, self.clock.now_us());
                    match r {
                        Ok(c) => self.apply(c),
                        Err(why) => {
                            // Their view is refreshed so their buttons are right again.
                            let v = self.room.lock().unwrap().view(false);
                            let _ = out.send(proto::line(&ToGuest::Room { room: v }));
                            let _ = why;
                        }
                    }
                }
                ToHost::Undo => {}
                ToHost::Bye => {
                    said_bye = true;
                    break;
                }
                ToHost::Hello { .. } => {}
            }
        }
        // Back on a new connection already: that one is theirs now, nothing to tidy here.
        let this = conn.stable_id();
        let ours = |m: &mut std::collections::HashMap<u32, (Out, Connection)>| {
            if m.get(&id).is_some_and(|(_, c)| c.stable_id() == this) {
                m.remove(&id).is_some()
            } else {
                false
            }
        };
        let was_waiting = ours(&mut self.waiting.lock().unwrap());
        let was_conn = ours(&mut self.conns.lock().unwrap());
        if !was_waiting && !was_conn {
            return;
        }
        // A goodbye: out at once. Gone without one (dropped): away now, out after a minute
        // unless back.
        if was_conn && said_bye {
            let now = self.clock.now_us();
            let c = self.room.lock().unwrap().left(id, now);
            self.apply(c);
            return;
        }
        let c = if was_conn {
            self.room.lock().unwrap().away(id)
        } else {
            self.room.lock().unwrap().deny(id)
        };
        self.apply(c);
        let weak = Arc::downgrade(&self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if let Some(h) = weak.upgrade()
                && !h.conns.lock().unwrap().contains_key(&id)
            {
                let c = h.room.lock().unwrap().remove(id, false);
                h.apply(c);
            }
        });
    }

    fn welcome(&self, id: u32, out: &Out) {
        let (view, preparing) = {
            let r = self.room.lock().unwrap();
            (r.view(false), r.preparing())
        };
        let _ = out.send(proto::line(&ToGuest::Welcome {
            you: id,
            room: view,
        }));
        if let Some((rev, index, position_ms)) = preparing {
            let _ = out.send(proto::line(&ToGuest::Prepare {
                rev,
                index,
                position_ms,
            }));
        }
        self.apply(Changes {
            room: true,
            prepare: None,
        });
    }

    /// The host lets someone in.
    pub fn admit(&self, id: u32) {
        let c = self.room.lock().unwrap().admit(id);
        if c.is_none() {
            return;
        }
        if let Some((out, conn)) = self.waiting.lock().unwrap().remove(&id) {
            self.conns.lock().unwrap().insert(id, (out.clone(), conn));
            self.welcome(id, &out);
        }
    }

    /// The host says no, or removes someone (and maybe blocks them).
    pub fn turn_away(&self, id: u32, block: bool, why: &str) {
        let c = self.room.lock().unwrap().remove(id, block);
        let gone = self
            .waiting
            .lock()
            .unwrap()
            .remove(&id)
            .or_else(|| self.conns.lock().unwrap().remove(&id));
        if let Some((out, conn)) = gone {
            let _ = out.send(proto::line(&ToGuest::Refused {
                why: why.to_owned(),
            }));
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                conn.close(3u32.into(), b"removed");
            });
        }
        self.apply(c);
    }

    /// End the room for everyone.
    pub async fn end(&self) {
        let line = proto::line(&ToGuest::Ended {
            why: format!("{} ended the room.", self.invite.host),
        });
        for (out, _) in self
            .conns
            .lock()
            .unwrap()
            .values()
            .chain(self.waiting.lock().unwrap().values())
        {
            let _ = out.send(line.clone());
        }
        self.to_follower(follow::In::Stop);
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.ep.close().await;
    }

    pub fn ask(&self, action: Action) -> Result<(), String> {
        let c = self
            .room
            .lock()
            .unwrap()
            .ask(HOST, action, self.clock.now_us())?;
        self.apply(c);
        Ok(())
    }

    pub fn undo(&self) -> Result<(), String> {
        let c = self.room.lock().unwrap().undo(HOST, self.clock.now_us())?;
        self.apply(c);
        Ok(())
    }

    /// Whether everyone is on this network (same room can work).
    pub fn all_on_one_network(&self) -> bool {
        let paths: Vec<net::Path> = self
            .conns
            .lock()
            .unwrap()
            .values()
            .map(|(_, c)| net::path_of(c))
            .collect();
        net::same_room_possible(&paths)
    }

    pub fn set(&self, f: impl FnOnce(&mut Room) -> Changes) {
        let c = f(&mut self.room.lock().unwrap());
        self.apply(c);
    }

    /// Add the host's own songs (from its library or the catalog) to the room.
    pub fn add_items(
        self: &Arc<Self>,
        root: &str,
        items: &[QueueItem],
        next: bool,
    ) -> Result<(), String> {
        let lib = crate::load_lib(std::path::Path::new(root))
            .ok()
            .map(|(_, l)| l);
        let mut covers_of = Vec::new();
        let songs: Vec<_> = items
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
    fn make_covers(self: &Arc<Self>, songs: Vec<(String, String)>) {
        if songs.is_empty() {
            return;
        }
        let root = std::path::PathBuf::from(&self.root);
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn_blocking(move || {
            let made = super::covers::make(&root, &songs);
            if let Some(h) = weak.upgrade() {
                for (key, path) in made {
                    h.covers.put(&key, path);
                }
                emit(&h.app);
            }
        });
    }

    /// Stream one of the host's files to someone in the room: the bytes asked for, as they are.
    async fn stream_file(
        &self,
        id: u32,
        mut send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
    ) {
        let mut lines = Lines::new(recv);
        let Ok(Some(l)) = lines.next().await else {
            return;
        };
        let Ok(req) = serde_json::from_slice::<Get>(&l) else {
            return;
        };
        // A song's file, or (`cover:` and its key) its cover: only to people in the room, and only
        // for songs in its queue.
        let cover = req.get.strip_prefix("cover:");
        let key = cover.unwrap_or(&req.get);
        let allowed = {
            let r = self.room.lock().unwrap();
            r.is_member(id) && r.queue().iter().any(|s| s.key == key && s.host_file)
        };
        let found = if cover.is_some() {
            self.covers.get(key).map(|p| (p, "image/jpeg"))
        } else {
            self.files
                .get(key)
                .map(|(p, format)| (p, super::sources::mime_of(&format)))
        };
        let Some((path, mime)) = allowed.then_some(found).flatten() else {
            let _ = net::send_line(&mut send, b"{\"error\":\"not in the room\"}\n").await;
            let _ = send.finish();
            return;
        };
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let Ok(mut file) = tokio::fs::File::open(&path).await else {
            let _ = net::send_line(&mut send, b"{\"error\":\"not there\"}\n").await;
            let _ = send.finish();
            return;
        };
        let size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
        let from = req.from.min(size);
        let to = req
            .to
            .unwrap_or(size.saturating_sub(1))
            .min(size.saturating_sub(1));
        let head = serde_json::json!({ "size": size, "mime": mime });
        if net::send_line(&mut send, &proto::line(&head))
            .await
            .is_err()
        {
            return;
        }
        if size == 0 || from > to {
            let _ = send.finish();
            return;
        }
        if file.seek(std::io::SeekFrom::Start(from)).await.is_err() {
            return;
        }
        let mut left = to - from + 1;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let want = (left as usize).min(buf.len());
            let n = match file.read(&mut buf[..want]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if net::send_line(&mut send, &buf[..n]).await.is_err() {
                return;
            }
            left -= n as u64;
        }
        let _ = send.finish();
    }
}

/// The host's own player reports straight to the room.
struct HostReport(Weak<Host>);

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
            h.room.lock().unwrap().sync(HOST, error_ms);
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
