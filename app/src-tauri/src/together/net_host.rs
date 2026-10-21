//! The host's side of a room's connections: this Venuta is the room. People connect to it,
//! knock with the invite's proof, wait to be let in (or come straight in), and get the room's
//! news; what they ask is checked against their role here. The host's files and covers are
//! streamed, on request, only to people in the room.
//!
//! What the app around it does (the window, notifications, the host's own player, where the
//! host's files are) comes in through [`HostHooks`].

use super::net::{self, Clock, Lines, News};
use iroh::Endpoint;
use iroh::endpoint::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use together::invite::Invite;
use together::proto::{self, Action, ToGuest, ToHost};
use together::room::{Changes, HOST, Knock, Room};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

type Out = mpsc::UnboundedSender<Vec<u8>>;

/// What a hosted room needs from the app around it.
pub trait HostHooks: Send + Sync + 'static {
    /// Something the window shows changed.
    fn changed(&self);
    /// Someone knocked and waits at the door (by the name they gave).
    fn knocked(&self, name: &str);
    /// A step of the room, for the playback log.
    fn trace(&self, what: &str);
    /// News for the host's own player, which follows the room like everyone's.
    fn news(&self, news: News);
    /// One of the host's songs in the room: its file and its type, by key.
    fn file(&self, key: &str) -> Option<(PathBuf, String)>;
    /// The cover made for one of the host's songs, by key.
    fn cover(&self, key: &str) -> Option<PathBuf>;
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

pub struct HostSession {
    pub ep: Endpoint,
    pub clock: Clock,
    pub room: Mutex<Room>,
    /// Each person's way to send to them: in the room, or waiting at the door.
    conns: Mutex<HashMap<u32, (Out, Connection)>>,
    /// Each guest's last few round trips (ms), for whether same room can stay on.
    trips: Mutex<HashMap<u32, Vec<u32>>>,
    waiting: Mutex<HashMap<u32, (Out, Connection)>>,
    pub invite: Invite,
    hooks: Arc<dyn HostHooks>,
    rt: Handle,
}

impl HostSession {
    /// A room on this endpoint (made with `host: true`), its clock starting now. Called on the
    /// async runtime the room's work runs on. Nobody is let in until [`HostSession::run`].
    pub fn new(ep: Endpoint, room: Room, invite: Invite, hooks: Arc<dyn HostHooks>) -> Arc<Self> {
        Arc::new(HostSession {
            ep,
            clock: Clock::new(),
            room: Mutex::new(room),
            conns: Mutex::default(),
            waiting: Mutex::default(),
            trips: Mutex::default(),
            invite,
            hooks,
            rt: Handle::current(),
        })
    }

    /// Open the door, and start the room's clock moving songs along.
    pub fn run(self: &Arc<Self>) {
        // People come in.
        let h = self.clone();
        self.rt.spawn(async move {
            while let Some(incoming) = h.ep.accept().await {
                let h2 = h.clone();
                h.rt.spawn(async move {
                    if let Ok(conn) = incoming.await {
                        h2.serve(conn).await;
                    }
                });
            }
        });
        // The room's clock moves songs along and starts the ones everyone's ready for.
        let weak = Arc::downgrade(self);
        self.rt.spawn(async move {
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
            self.hooks.news(News::Room(host_view));
            self.hooks.changed();
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
            self.hooks.news(News::Prepare {
                rev,
                index,
                position_ms,
            });
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
        if !self.invite.open_at(net::unix_now()) {
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
        self.rt.spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if net::send_line(&mut send, &line).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        let id = match knock {
            Knock::Refused(why) => {
                self.hooks
                    .trace(&format!("room: someone was refused ({why})"));
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
                self.hooks.knocked(&name);
                self.hooks.changed();
                id
            }
            Knock::In(id) => {
                self.hooks.trace(&format!("room: {id} is in"));
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
        self.rt.spawn(async move {
            while let Ok((s, r)) = c2.accept_bi().await {
                let h3 = h.clone();
                h.rt.spawn(async move { h3.stream_file(id, s, r).await });
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
                    if pings.is_multiple_of(8) {
                        let path = net::path_of(&conn);
                        if let Some(rtt) = path.rtt_ms {
                            self.room.lock().unwrap().round_trip(i64::from(rtt) * 1_000);
                        }
                        // Someone farther than this network now (not one slow reading: their
                        // last few, mostly): same room goes back to apart.
                        let same_room = self.room.lock().unwrap().view(true).same_room;
                        if same_room && self.too_far_for_same_room(id, path) {
                            self.hooks.trace(&format!(
                                "room: same room off, {id} is too far now ({:?} ms)",
                                path.rtt_ms
                            ));
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
                    let c = self.room.lock().unwrap().sync(id, error_ms);
                    self.apply(c);
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
            self.hooks.trace(&format!("room: {id} left"));
            let now = self.clock.now_us();
            let c = self.room.lock().unwrap().left(id, now);
            self.apply(c);
            return;
        }
        self.hooks
            .trace(&format!("room: {id}'s connection dropped"));
        let c = if was_conn {
            self.room.lock().unwrap().away(id)
        } else {
            self.room.lock().unwrap().deny(id)
        };
        self.apply(c);
        let weak = Arc::downgrade(&self);
        self.rt.spawn(async move {
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
        self.hooks
            .trace(&format!("room: the host took {id} out (blocked: {block})"));
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
            self.rt.spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                conn.close(3u32.into(), b"removed");
            });
        }
        self.apply(c);
    }

    /// End the room for everyone.
    pub async fn end(&self) {
        self.hooks.trace("room: the host ended it");
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
        self.hooks.news(News::Stop);
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

    /// Whether this guest's connection has become too far for same room: not direct any more,
    /// or the middle one of their last three round trips over the line (one slow reading, as a
    /// busy moment gives, leaves it on).
    fn too_far_for_same_room(&self, id: u32, path: net::Path) -> bool {
        if !path.direct {
            return true;
        }
        let Some(rtt) = path.rtt_ms else {
            return false;
        };
        let mut trips = self.trips.lock().unwrap();
        let t = trips.entry(id).or_default();
        t.push(rtt);
        if t.len() > 3 {
            t.remove(0);
        }
        net::too_far(t)
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
            self.hooks.cover(key).map(|p| (p, "image/jpeg".to_owned()))
        } else {
            self.hooks.file(key)
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
