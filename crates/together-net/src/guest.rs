//! A guest's side of a room's connection: connect to the host's Venuta with the invite, knock
//! with its proof, then read the room's news. The host's clock is learned from pings
//! (lowest-delay answers trusted); the host's files are played through a small server on this
//! computer that asks the host for them. A dropped connection is tried again for a minute, and
//! the host knows it's the same person.
//!
//! What the app around it does (the window, the player that follows the room, covers kept on
//! disk) comes in through [`GuestHooks`].

use crate::host::Get;
use crate::net::{self, Clock, Lines, News, Reach};
use iroh::Endpoint;
use iroh::endpoint::Connection;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use together::clock::Sample;
use together::invite::Invite;
use together::proto::{self, Action, RoomView, ToGuest, ToHost};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Where joining is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Joining,
    Waiting,
    In,
    Reconnecting,
    Ended,
}

/// What a guest's session needs from the app around it.
pub trait GuestHooks: Send + Sync + 'static {
    /// Something the window shows changed.
    fn changed(&self);
    /// A step of the room, for the playback log.
    fn trace(&self, what: &str);
    /// A short note for the window.
    fn note(&self, what: &str);
    /// News for this computer's player, which follows the room.
    fn news(&self, news: News);
    /// A room view came (before it is followed): the app finds or asks for its covers.
    fn view(&self, guest: &Arc<GuestSession>, view: &RoomView);
    /// The name others see, as it is now (asked at each connect).
    fn name(&self) -> String;
}

pub struct GuestSession {
    pub invite: Invite,
    ep: Endpoint,
    conn: Mutex<Option<Connection>>,
    out: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    /// What was asked while the connection was down, to send once it's back.
    held: Mutex<Vec<Vec<u8>>>,
    /// This session's own clock (the host's is learned against it).
    pub local: Clock,
    pub clock: Arc<Mutex<together::clock::Clock>>,
    pub me: Mutex<Option<u32>>,
    pub view: Mutex<Option<RoomView>>,
    pub stage: Mutex<Stage>,
    /// Why it ended (refused, the host ended it, couldn't reconnect).
    pub why: Mutex<Option<String>>,
    /// The small server the host's files play through (stopped when the room ends).
    server: Mutex<Option<JoinHandle<()>>>,
    /// Where that server listens on this computer, and the token it asks for.
    proxy_port: u16,
    proxy_token: String,
    /// What this Venuta says it is, in its hello.
    app: String,
    hooks: Arc<dyn GuestHooks>,
    rt: Handle,
}

impl Drop for GuestSession {
    // A join that failed partway: its server goes too.
    fn drop(&mut self) {
        if let Some(h) = self.server.get_mut().ok().and_then(|s| s.take()) {
            h.abort();
        }
    }
}

impl GuestSession {
    /// Join the invite's room: this computer's key for that room kept in `key_dir`, `app`
    /// what this Venuta says it is.
    pub async fn join(
        invite: Invite,
        reach: &Reach,
        key_dir: &std::path::Path,
        app: String,
        hooks: Arc<dyn GuestHooks>,
    ) -> Result<Arc<GuestSession>, String> {
        if !invite.open_at(net::unix_now()) {
            return Err("This room's invite has ended. Ask for a new one.".into());
        }
        let ep = net::endpoint(reach, false, Some(net::guest_key(key_dir, &invite.secret))).await?;
        // The server the host's files are played through.
        let token = net::random_hex(16);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let g = Arc::new(GuestSession {
            invite,
            ep,
            conn: Mutex::default(),
            out: Mutex::default(),
            local: Clock::new(),
            clock: Arc::default(),
            me: Mutex::default(),
            view: Mutex::default(),
            stage: Mutex::new(Stage::Joining),
            why: Mutex::default(),
            server: Mutex::default(),
            held: Mutex::default(),
            proxy_port: port,
            proxy_token: token.clone(),
            app,
            hooks,
            rt: Handle::current(),
        });
        let weak = Arc::downgrade(&g);
        let rt = g.rt.clone();
        let server = g.rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let Some(g) = weak.upgrade() else { break };
                let token = token.clone();
                rt.spawn(async move { g.proxy(tcp, &token).await });
            }
        });
        *g.server.lock().unwrap() = Some(server);
        g.connect().await?;
        Ok(g)
    }

    /// The port on 127.0.0.1 the host's files play through, and the token it asks for.
    pub fn proxy_address(&self) -> (u16, String) {
        (self.proxy_port, self.proxy_token.clone())
    }

    /// Connect (again), knock, and start reading the room's news. (Boxed and Send: it is
    /// called again from the task it starts, when the connection drops.)
    fn connect(
        self: &Arc<Self>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(self.connect_once())
    }

    async fn connect_once(self: &Arc<Self>) -> Result<(), String> {
        let ticket: iroh_tickets::endpoint::EndpointTicket = self
            .invite
            .to
            .parse()
            .map_err(|_| "That invite isn't readable.".to_owned())?;
        let conn = tokio::time::timeout(
            Duration::from_secs(25),
            self.ep.connect(ticket.endpoint_addr().clone(), net::ALPN),
        )
        .await
        .map_err(|_| {
            format!(
                "Couldn't reach {}'s room. Check your internet, or ask for a new invite.",
                self.invite.host
            )
        })?
        .map_err(|_| {
            format!(
                "Couldn't reach {}'s room. Check your internet, or ask for a new invite.",
                self.invite.host
            )
        })?;
        let (mut send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        let _ = send.set_priority(net::priority::CONTROL);
        let hello = ToHost::Hello {
            protocol: together::PROTOCOL,
            name: self.hooks.name(),
            proof: self.invite.proof(&self.ep.id().to_string()),
            app: self.app.clone(),
        };
        net::send_line(&mut send, &proto::line(&hello)).await?;
        let (out, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        self.rt.spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if net::send_line(&mut send, &line).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        *self.conn.lock().unwrap() = Some(conn.clone());
        // What was asked while the connection was down goes first.
        for line in std::mem::take(&mut *self.held.lock().unwrap()) {
            let _ = out.send(line);
        }
        *self.out.lock().unwrap() = Some(out);
        // Its news.
        let g = self.clone();
        self.rt.spawn(async move {
            let mut lines = Lines::new(recv);
            while let Ok(Some(l)) = lines.next().await {
                let t4 = g.local.now_us();
                let Ok(m) = proto::read::<ToGuest>(&l) else {
                    continue;
                };
                g.on_message(m, t4);
            }
            g.dropped().await;
        });
        // The host's clock: a burst at first, then now and then.
        let g = self.clone();
        self.rt.spawn(async move {
            let mut n: u32 = 0;
            loop {
                if g.out.lock().unwrap().is_none() || *g.stage.lock().unwrap() == Stage::Ended {
                    break;
                }
                g.send(ToHost::Ping {
                    id: n,
                    t1: g.local.now_us(),
                });
                n += 1;
                tokio::time::sleep(Duration::from_millis(if n < 16 { 80 } else { 2_000 })).await;
            }
        });
        Ok(())
    }

    fn on_message(self: &Arc<Self>, m: ToGuest, t4: i64) {
        match m {
            ToGuest::Waiting => self.set_stage(Stage::Waiting),
            ToGuest::Welcome { you, room } => {
                *self.me.lock().unwrap() = Some(you);
                self.room(room);
                self.set_stage(Stage::In);
            }
            ToGuest::Room { room } => self.room(room),
            ToGuest::Prepare {
                rev,
                index,
                position_ms,
            } => self.hooks.news(News::Prepare {
                rev,
                index,
                position_ms,
            }),
            ToGuest::Pong { t1, t2, t3, .. } => {
                let ready_before = self.clock.lock().unwrap().ready();
                self.clock.lock().unwrap().add(Sample { t1, t2, t3, t4 });
                // The clock just became known: anything waiting on it can follow now.
                if !ready_before
                    && self.clock.lock().unwrap().ready()
                    && let Some(v) = self.view.lock().unwrap().clone()
                {
                    self.hooks.news(News::Room(v));
                }
            }
            ToGuest::Refused { why } | ToGuest::Ended { why } => {
                if self.finish(Some(why)) {
                    let g = self.clone();
                    self.rt.spawn(async move { g.ep.close().await });
                }
            }
        }
    }

    fn room(self: &Arc<Self>, v: RoomView) {
        *self.view.lock().unwrap() = Some(v.clone());
        self.hooks.view(self, &v);
        // Following needs the clock; until it's known, the view waits (sent again when it is).
        if self.clock.lock().unwrap().ready() {
            self.hooks.news(News::Room(v));
        }
        self.hooks.changed();
    }

    /// One cover's bytes from the host, at most `max` (none: it has none, or the room's over).
    pub async fn fetch_cover(&self, key: &str, max: u64) -> Option<Vec<u8>> {
        let conn = self.conn.lock().unwrap().clone()?;
        let (mut send, recv) = conn.open_bi().await.ok()?;
        let _ = send.set_priority(net::priority::COVER);
        let ask = Get {
            get: format!("cover:{key}"),
            from: 0,
            to: None,
        };
        net::send_line(&mut send, &proto::line(&ask)).await.ok()?;
        let _ = send.finish();
        let mut lines = Lines::new(recv);
        let meta: serde_json::Value = serde_json::from_slice(&lines.next().await.ok()??).ok()?;
        let size = meta.get("size")?.as_u64()?;
        if size == 0 || size > max {
            return None;
        }
        let mut bytes = Vec::with_capacity(size as usize);
        lines
            .into_inner()
            .take(max)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        Some(bytes)
    }

    fn set_stage(&self, s: Stage) {
        *self.stage.lock().unwrap() = s;
        self.hooks.changed();
    }

    /// The connection dropped: try again for a minute (the room plays on meanwhile).
    async fn dropped(self: Arc<Self>) {
        *self.out.lock().unwrap() = None;
        if *self.stage.lock().unwrap() == Stage::Ended {
            return;
        }
        self.set_stage(Stage::Reconnecting);
        self.hooks
            .trace("room: the connection to the host dropped; trying again");
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_secs(3)).await;
            if *self.stage.lock().unwrap() == Stage::Ended {
                return;
            }
            if self.connect().await.is_ok() {
                self.hooks.trace("room: back in touch with the host");
                return;
            }
        }
        let why = format!(
            "Lost {}'s room. Join again with the invite.",
            self.invite.host
        );
        if self.finish(Some(why)) {
            self.ep.close().await;
        }
    }

    /// The room is over for this computer (refused, ended by the host, lost, or left): the
    /// player is on its own again and the host's files are no longer served here. False when
    /// it was over already.
    fn finish(&self, why: Option<String>) -> bool {
        {
            let mut stage = self.stage.lock().unwrap();
            if *stage == Stage::Ended {
                return false;
            }
            *stage = Stage::Ended;
        }
        self.hooks.trace(&format!(
            "room: over for this guest ({})",
            why.as_deref().unwrap_or("left")
        ));
        if let Some(why) = why {
            *self.why.lock().unwrap() = Some(why);
        }
        self.hooks.news(News::Stop);
        if let Some(h) = self.server.lock().unwrap().take() {
            h.abort();
        }
        *self.out.lock().unwrap() = None;
        self.hooks.changed();
        true
    }

    pub fn send(&self, m: ToHost) {
        if let Some(out) = self.out.lock().unwrap().as_ref() {
            let _ = out.send(proto::line(&m));
        }
    }

    /// A press: to the host now, or as soon as the connection is back (never dropped quietly).
    pub fn ask(&self, action: Action) {
        let m = ToHost::Ask { action };
        if self.out.lock().unwrap().is_some() {
            self.send(m);
            return;
        }
        if *self.stage.lock().unwrap() == Stage::Reconnecting {
            let mut held = self.held.lock().unwrap();
            if held.len() < 20 {
                held.push(proto::line(&m));
            }
            self.hooks.note(
                "Reconnecting: your press will reach the room as soon as the connection is back.",
            );
        }
    }

    pub async fn leave(&self) {
        // The goodbye first, a moment to go out, then everything else.
        self.send(ToHost::Bye);
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.finish(None);
        self.ep.close().await;
    }

    /// Let go of the connection (a room that ended).
    pub async fn close(&self) {
        self.ep.close().await;
    }

    pub fn path(&self) -> Option<net::Path> {
        self.conn.lock().unwrap().as_ref().map(net::path_of)
    }

    /// One request from this computer's player for a host file: forwarded to the host, the
    /// bytes passed back as an HTTP answer (ranges too, for seeking).
    async fn proxy(&self, mut tcp: tokio::net::TcpStream, token: &str) {
        let mut head = Vec::new();
        let mut b = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match tcp.read(&mut b).await {
                Ok(0) | Err(_) => return,
                Ok(n) => head.extend_from_slice(&b[..n]),
            }
            if head.len() > 16 * 1024 {
                return;
            }
        }
        let text = String::from_utf8_lossy(&head).to_string();
        let mut first = text.lines().next().unwrap_or("").split_whitespace();
        let method = first.next().unwrap_or("");
        let target = first.next().unwrap_or("");
        let reply = |code: &str| {
            format!("HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        };
        let Some((path, query)) = target.split_once('?') else {
            let _ = tcp.write_all(reply("404 Not Found").as_bytes()).await;
            return;
        };
        let key = path.strip_prefix("/s/").unwrap_or("");
        let token_ok = query.strip_prefix("t=").is_some_and(|t| {
            t.len() == token.len()
                && t.bytes()
                    .zip(token.bytes())
                    .fold(0u8, |a, (x, y)| a | (x ^ y))
                    == 0
        });
        let key_ok = key
            .strip_prefix("host:")
            .is_some_and(|h| h.len() == 32 && h.bytes().all(|c| c.is_ascii_hexdigit()));
        if !token_ok || !key_ok || !(method == "GET" || method == "HEAD") {
            let _ = tcp.write_all(reply("403 Forbidden").as_bytes()).await;
            return;
        }
        // "Range: bytes=a-" or "bytes=a-b".
        let range = text.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if !k.trim().eq_ignore_ascii_case("range") {
                return None;
            }
            let (a, b) = v.trim().strip_prefix("bytes=")?.split_once('-')?;
            Some((a.trim().parse::<u64>().ok()?, b.trim().parse::<u64>().ok()))
        });
        let Some(conn) = self.conn.lock().unwrap().clone() else {
            let _ = tcp
                .write_all(reply("503 Service Unavailable").as_bytes())
                .await;
            return;
        };
        let Ok((mut send, recv)) = conn.open_bi().await else {
            let _ = tcp.write_all(reply("502 Bad Gateway").as_bytes()).await;
            return;
        };
        let _ = send.set_priority(net::priority::FILE);
        let (from, to) = range.unwrap_or((0, None));
        let ask = Get {
            get: key.to_owned(),
            from,
            to: if method == "HEAD" { Some(from) } else { to },
        };
        if net::send_line(&mut send, &proto::line(&ask)).await.is_err() {
            return;
        }
        let _ = send.finish();
        let mut lines = Lines::new(recv);
        let Ok(Some(meta)) = lines.next().await else {
            return;
        };
        let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&meta) else {
            return;
        };
        let Some(size) = meta.get("size").and_then(|s| s.as_u64()) else {
            let _ = tcp.write_all(reply("404 Not Found").as_bytes()).await;
            return;
        };
        let mime = meta
            .get("mime")
            .and_then(|m| m.as_str())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let end = to
            .unwrap_or(size.saturating_sub(1))
            .min(size.saturating_sub(1));
        let len = if size == 0 {
            0
        } else {
            end.saturating_sub(from) + 1
        };
        let status = if range.is_some() {
            format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{end}/{size}\r\n")
        } else {
            "HTTP/1.1 200 OK\r\n".to_owned()
        };
        let headers = format!(
            "{status}Content-Type: {mime}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
        );
        if tcp.write_all(headers.as_bytes()).await.is_err() || method == "HEAD" {
            return;
        }
        let mut body = lines.into_inner();
        let _ = tokio::io::copy(&mut body, &mut tcp).await;
    }
}
