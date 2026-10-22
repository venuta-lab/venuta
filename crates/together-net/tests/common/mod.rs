//! The harness the room networking tests share: an app that records what it is told, a host
//! and a guest session over this network (local only), and plain peers that speak the wire
//! protocol and nothing more.

#![allow(dead_code, unused_imports)]

pub use iroh::endpoint::{ConnectionError, SendStream};
pub use iroh::{Endpoint, SecretKey};
pub use std::collections::HashMap;
pub use std::path::PathBuf;
pub use std::sync::{Arc, Mutex};
pub use std::time::{Duration, Instant};
pub use together::invite::Invite;
pub use together::proto::{self, Action, Role, RoomView, Song, ToGuest, ToHost};
pub use together::room::Room;
pub use together_net::guest::{GuestHooks, GuestSession, Stage};
pub use together_net::host::{Get, HostHooks, HostSession};
pub use together_net::net::{self, News, Reach};
pub use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const SECRET: &str = "00112233445566778899aabbccddeeff";
/// How long anything here may take before a test gives up (generous: the machine is busy).
pub const WAIT: Duration = Duration::from_secs(20);

pub fn local() -> Reach {
    Reach {
        local_only: true,
        ..Reach::default()
    }
}

pub fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes(&[n; 32])
}

pub fn online(id: &str) -> Song {
    Song {
        key: format!("online:{id}"),
        title: format!("Song {id}"),
        artists: vec!["Tester".into()],
        album: None,
        duration_ms: Some(180_000),
        isrc: None,
        mbid: None,
        online_id: Some(id.into()),
        host_file: false,
        format: None,
        artwork_url: None,
        added_by: None,
    }
}

/// A host song short for its file's size: the whole file comes in the pacing's first burst.
pub fn short_host_song(hex: &str) -> Song {
    Song {
        duration_ms: Some(1_000),
        ..host_song(hex)
    }
}

pub fn host_song(hex: &str) -> Song {
    Song {
        key: format!("host:{hex}"),
        online_id: None,
        host_file: true,
        format: Some("flac".into()),
        ..online("aaaaaaaaaaa")
    }
}

pub const FILE_KEY: &str = "host:0123456789abcdef0123456789abcdef";
pub const OTHER_KEY: &str = "host:fedcba9876543210fedcba9876543210";

/// Bytes that say where they are (so a range shows which part came).
pub fn counting(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// What the app around a session was told, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Told {
    Changed,
    Knocked(String),
    Trace(String),
    News(News),
    Note(String),
    View(RoomView),
}

/// The app around a session, as a record of what it was told.
#[derive(Default)]
pub struct App {
    pub told: Mutex<Vec<Told>>,
    pub files: Mutex<HashMap<String, (PathBuf, String)>>,
    pub covers: Mutex<HashMap<String, PathBuf>>,
}

impl App {
    pub fn tell(&self, t: Told) {
        self.told.lock().unwrap().push(t);
    }
    pub fn told(&self) -> Vec<Told> {
        self.told.lock().unwrap().clone()
    }
    pub fn news(&self) -> Vec<News> {
        self.told()
            .into_iter()
            .filter_map(|t| match t {
                Told::News(n) => Some(n),
                _ => None,
            })
            .collect()
    }
    pub fn traces(&self) -> Vec<String> {
        self.told()
            .into_iter()
            .filter_map(|t| match t {
                Told::Trace(s) => Some(s),
                _ => None,
            })
            .collect()
    }
    pub fn notes(&self) -> Vec<String> {
        self.told()
            .into_iter()
            .filter_map(|t| match t {
                Told::Note(s) => Some(s),
                _ => None,
            })
            .collect()
    }
    pub async fn until(&self, what: &str, f: impl Fn(&[Told]) -> bool) {
        let end = Instant::now() + WAIT;
        while !f(&self.told()) {
            assert!(
                Instant::now() < end,
                "waited for {what}; told: {:#?}",
                self.told()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl HostHooks for App {
    fn changed(&self) {
        self.tell(Told::Changed);
    }
    fn knocked(&self, name: &str) {
        self.tell(Told::Knocked(name.into()));
    }
    fn trace(&self, what: &str) {
        self.tell(Told::Trace(what.into()));
    }
    fn news(&self, news: News) {
        self.tell(Told::News(news));
    }
    fn file(&self, key: &str) -> Option<(PathBuf, String)> {
        self.files.lock().unwrap().get(key).cloned()
    }
    fn cover(&self, key: &str) -> Option<PathBuf> {
        self.covers.lock().unwrap().get(key).cloned()
    }
}

impl GuestHooks for App {
    fn changed(&self) {
        self.tell(Told::Changed);
    }
    fn trace(&self, what: &str) {
        self.tell(Told::Trace(what.into()));
    }
    fn note(&self, what: &str) {
        self.tell(Told::Note(what.into()));
    }
    fn news(&self, news: News) {
        self.tell(Told::News(news));
    }
    fn view(&self, _guest: &Arc<GuestSession>, view: &RoomView) {
        self.tell(Told::View(view.clone()));
    }
    fn name(&self) -> String {
        "Ali".into()
    }
}

/// Wait for something to be so.
pub async fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < end, "waited for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// --- A host, and a plain guest speaking the protocol to it -------------------------------

pub struct TestHost {
    pub s: Arc<HostSession>,
    pub app: Arc<App>,
    pub invite: Invite,
}

pub async fn host(songs: Vec<Song>) -> TestHost {
    host_until(songs, net::unix_now() + 3_600).await
}

pub async fn host_until(songs: Vec<Song>, until: u64) -> TestHost {
    let reach = local();
    let ep = net::endpoint(&reach, true, None).await.unwrap();
    let ticket = net::ticket(&ep, &reach).await;
    let invite = Invite {
        to: ticket,
        secret: SECRET.into(),
        until,
        host: "Asha".into(),
    };
    let app = Arc::new(App::default());
    let s = HostSession::new(
        ep,
        Room::new("Asha", SECRET, songs),
        invite.clone(),
        app.clone(),
    );
    s.run();
    TestHost { s, app, invite }
}

pub fn hello(invite: &Invite, key: &SecretKey, name: &str) -> ToHost {
    ToHost::Hello {
        protocol: together::PROTOCOL,
        name: name.into(),
        proof: invite.proof(&key.public().to_string()),
        app: "Venuta test".into(),
    }
}

/// A guest that speaks the wire protocol and nothing more.
pub struct Raw {
    pub ep: Endpoint,
    pub conn: iroh::endpoint::Connection,
    pub send: SendStream,
    pub lines: net::Lines,
}

impl Raw {
    pub async fn dial(invite: &Invite, key: SecretKey) -> Raw {
        let ep = net::endpoint(&local(), false, Some(key)).await.unwrap();
        let t: iroh_tickets::endpoint::EndpointTicket = invite.to.parse().unwrap();
        let conn = tokio::time::timeout(WAIT, ep.connect(t.endpoint_addr().clone(), net::ALPN))
            .await
            .unwrap()
            .unwrap();
        let (send, recv) = conn.open_bi().await.unwrap();
        Raw {
            ep,
            conn,
            send,
            lines: net::Lines::new(recv),
        }
    }

    /// Connect and knock as person `n`, by `name`.
    pub async fn knock(invite: &Invite, n: u8, name: &str) -> Raw {
        let mut r = Raw::dial(invite, key(n)).await;
        r.say(&hello(invite, &key(n), name)).await;
        r
    }

    pub async fn say(&mut self, m: &ToHost) {
        net::send_line(&mut self.send, &proto::line(m))
            .await
            .unwrap();
    }

    /// The next message from the host (`None`: the host finished the stream).
    pub async fn hear(&mut self) -> Option<ToGuest> {
        tokio::time::timeout(WAIT, self.lines.next())
            .await
            .expect("a message in time")
            .ok()
            .flatten()
            .map(|l| proto::read::<ToGuest>(&l).expect("a message the guest can read"))
    }

    /// Everything the host sends before it answers this ping (it answers in order, so this is
    /// everything it sent before reading the ping).
    pub async fn until_pong(&mut self, id: u32) -> Vec<ToGuest> {
        self.say(&ToHost::Ping { id, t1: 0 }).await;
        let mut got = Vec::new();
        loop {
            match self.hear().await {
                Some(ToGuest::Pong { id: p, .. }) if p == id => return got,
                Some(m) => got.push(m),
                None => panic!("the stream ended before the pong; got {got:?}"),
            }
        }
    }

    /// The next message that isn't a room view (the room's tick sends those when it likes).
    pub async fn hear_past_rooms(&mut self) -> Option<ToGuest> {
        loop {
            match self.hear().await {
                Some(ToGuest::Room { .. }) => continue,
                m => return m,
            }
        }
    }

    /// How the host closed the connection: its code and reason.
    pub async fn closed(&self) -> (u64, Vec<u8>) {
        match tokio::time::timeout(WAIT, self.conn.closed())
            .await
            .expect("closed in time")
        {
            ConnectionError::ApplicationClosed(c) => (c.error_code.into_inner(), c.reason.to_vec()),
            other => panic!("closed otherwise: {other:?}"),
        }
    }

    /// Ask for a file (or a cover) on a stream of its own: the header line, then the bytes.
    pub async fn get(&self, get: &Get) -> (serde_json::Value, Vec<u8>) {
        let (mut send, recv) = self.conn.open_bi().await.unwrap();
        net::send_line(&mut send, &proto::line(get)).await.unwrap();
        send.finish().unwrap();
        let mut lines = net::Lines::new(recv);
        let head = tokio::time::timeout(WAIT, lines.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(WAIT, lines.into_inner().read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        (serde_json::from_slice(&head).unwrap(), bytes)
    }
}

/// A guest let in by the host: the welcome and the room after it read.
pub async fn member(h: &TestHost, n: u8, name: &str) -> Raw {
    let mut r = Raw::knock(&h.invite, n, name).await;
    match r.hear().await {
        Some(ToGuest::Waiting) => {}
        other => panic!("not waiting: {other:?}"),
    }
    let id = {
        let room = h.s.room.lock().unwrap();
        room.view(true)
            .waiting
            .iter()
            .find(|(_, w)| w == name)
            .map(|(id, _)| *id)
            .expect("at the door")
    };
    h.s.admit(id);
    match r.hear_past_rooms().await {
        Some(ToGuest::Welcome { you, .. }) => assert_eq!(you, id),
        other => panic!("not welcomed: {other:?}"),
    }
    r
}

// --- A guest, and a plain host speaking the protocol to it ----------------------------------

/// A host that speaks the wire protocol and nothing more.
pub struct RawHost {
    pub ep: Endpoint,
    pub invite: Invite,
}

/// One connection to it: the guest's first stream.
pub struct Visit {
    pub conn: iroh::endpoint::Connection,
    pub send: SendStream,
    pub lines: net::Lines,
}

impl RawHost {
    pub async fn new(until: u64) -> RawHost {
        let reach = local();
        let ep = net::endpoint(&reach, true, None).await.unwrap();
        let ticket = net::ticket(&ep, &reach).await;
        RawHost {
            ep,
            invite: Invite {
                to: ticket,
                secret: SECRET.into(),
                until,
                host: "Asha".into(),
            },
        }
    }

    pub async fn visit(&self) -> Visit {
        let conn = tokio::time::timeout(WAIT, async {
            self.ep.accept().await.unwrap().await.unwrap()
        })
        .await
        .expect("a guest in time");
        let (send, recv) = conn.accept_bi().await.unwrap();
        Visit {
            conn,
            send,
            lines: net::Lines::new(recv),
        }
    }
}

impl Visit {
    pub async fn hear(&mut self) -> Option<ToHost> {
        tokio::time::timeout(WAIT, self.lines.next())
            .await
            .expect("a message in time")
            .ok()
            .flatten()
            .map(|l| proto::read::<ToHost>(&l).expect("a message the host can read"))
    }
    /// The next message that isn't a ping.
    pub async fn hear_past_pings(&mut self) -> Option<ToHost> {
        loop {
            match self.hear().await {
                Some(ToHost::Ping { .. }) => continue,
                m => return m,
            }
        }
    }
    pub async fn say(&mut self, m: &ToGuest) {
        net::send_line(&mut self.send, &proto::line(m))
            .await
            .unwrap();
    }
}

pub fn a_room() -> RoomView {
    Room::new("Asha", SECRET, vec![online("aaaaaaaaaaa")]).view(false)
}

pub async fn guest_of(invite: &Invite, keys: &std::path::Path) -> (Arc<GuestSession>, Arc<App>) {
    let app = Arc::new(App::default());
    let g = GuestSession::join(
        invite.clone(),
        &local(),
        keys,
        "Venuta 9.9.9".into(),
        app.clone(),
    )
    .await
    .unwrap();
    (g, app)
}

// --- A slower link between two endpoints on this computer -----------------------------------

/// A link in the middle: datagrams from the guest's side go to `to` from a socket of the link's
/// own, and what comes back is passed on at `down_rate` bytes a second through a queue of at
/// most `queue` bytes (more is dropped, as a router does), after `delay`. `cut` stops it.
pub struct Link {
    pub addr: std::net::SocketAddr,
    pub cut: Arc<std::sync::atomic::AtomicBool>,
}

pub async fn link(to: std::net::SocketAddr, down_rate: f64, queue: usize, delay: Duration) -> Link {
    use std::sync::atomic::Ordering;
    let near = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let far_side = if to.is_ipv6() {
        "[::1]:0"
    } else {
        "127.0.0.1:0"
    };
    let far = Arc::new(tokio::net::UdpSocket::bind(far_side).await.unwrap());
    let addr = near.local_addr().unwrap();
    let cut = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guest: Arc<Mutex<Option<std::net::SocketAddr>>> = Arc::default();
    // Up: at once.
    tokio::spawn({
        let (near, far, guest, cut) = (near.clone(), far.clone(), guest.clone(), cut.clone());
        async move {
            let mut buf = vec![0u8; 65_536];
            while let Ok((n, from)) = near.recv_from(&mut buf).await {
                *guest.lock().unwrap() = Some(from);
                if !cut.load(Ordering::Relaxed) {
                    let _ = far.send_to(&buf[..n], to).await;
                }
            }
        }
    });
    // Down: queued, paced, delayed.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    let queued = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    tokio::spawn({
        let (far, queued, cut) = (far.clone(), queued.clone(), cut.clone());
        async move {
            let mut buf = vec![0u8; 65_536];
            while let Ok((n, _)) = far.recv_from(&mut buf).await {
                if cut.load(Ordering::Relaxed) || queued.load(Ordering::Relaxed) + n > queue {
                    continue;
                }
                queued.fetch_add(n, Ordering::Relaxed);
                let _ = tx.send((Instant::now(), buf[..n].to_vec()));
            }
        }
    });
    tokio::spawn(async move {
        let mut free_at = Instant::now();
        while let Some((came, d)) = rx.recv().await {
            // On the link's own schedule (a timer that wakes late sends the next ones at once).
            let start = free_at.max(came);
            free_at = start + Duration::from_secs_f64(d.len() as f64 / down_rate);
            let due = free_at.max(came + delay);
            if due > Instant::now() {
                tokio::time::sleep_until(due.into()).await;
            }
            queued.fetch_sub(d.len(), std::sync::atomic::Ordering::Relaxed);
            let g = *guest.lock().unwrap();
            if let Some(g) = g {
                let _ = near.send_to(&d, g).await;
            }
        }
    });
    Link { addr, cut }
}

/// The host's invite, reaching it only through these links.
pub fn invite_through(invite: &Invite, links: &[&Link]) -> Invite {
    let t: iroh_tickets::endpoint::EndpointTicket = invite.to.parse().unwrap();
    let mut addr = iroh::EndpointAddr::new(t.endpoint_addr().id);
    for l in links {
        addr = addr.with_ip_addr(l.addr);
    }
    Invite {
        to: iroh_tickets::endpoint::EndpointTicket::new(addr).to_string(),
        ..invite.clone()
    }
}

/// The host's own address on this computer (local only: its first IPv4 one).
pub fn host_addr(h: &TestHost) -> std::net::SocketAddr {
    if let Some(a) = h.s.ep.addr().ip_addrs().find(|a| a.is_ipv6()) {
        return std::net::SocketAddr::new("::1".parse().unwrap(), a.port());
    }
    let a =
        h.s.ep
            .addr()
            .ip_addrs()
            .find(|a| a.is_ipv4())
            .copied()
            .unwrap();
    std::net::SocketAddr::new("127.0.0.1".parse().unwrap(), a.port())
}

/// An endpoint on one address family of this computer only (`v6`: [::1], else 127.0.0.1), so
/// another on the other family can reach it only through a [`Link`].
pub async fn endpoint_on(v6: bool, host: bool, key: Option<SecretKey>) -> Endpoint {
    let mut b = Endpoint::builder(iroh::endpoint::presets::Minimal)
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports();
    b = if v6 {
        b.bind_addr("[::1]:0").unwrap()
    } else {
        b.bind_addr("127.0.0.1:0").unwrap()
    };
    if host {
        b = b.alpns(vec![net::ALPN.to_vec()]);
    }
    if let Some(k) = key {
        b = b.secret_key(k);
    }
    b.bind().await.unwrap()
}

/// A host on [::1] only: guests on 127.0.0.1 reach it through links.
pub async fn host_far(songs: Vec<Song>) -> TestHost {
    let ep = endpoint_on(true, true, None).await;
    let invite = Invite {
        to: iroh_tickets::endpoint::EndpointTicket::new(ep.addr()).to_string(),
        secret: SECRET.into(),
        until: net::unix_now() + 3_600,
        host: "Asha".into(),
    };
    let app = Arc::new(App::default());
    let s = HostSession::new(
        ep,
        Room::new("Asha", SECRET, songs),
        invite.clone(),
        app.clone(),
    );
    s.run();
    TestHost { s, app, invite }
}

impl Raw {
    /// Dial from 127.0.0.1 only (for a host reached through links).
    pub async fn dial_near(invite: &Invite, key: SecretKey) -> Raw {
        let ep = endpoint_on(false, false, Some(key)).await;
        let t: iroh_tickets::endpoint::EndpointTicket = invite.to.parse().unwrap();
        let conn = tokio::time::timeout(WAIT, ep.connect(t.endpoint_addr().clone(), net::ALPN))
            .await
            .unwrap()
            .unwrap();
        let (send, recv) = conn.open_bi().await.unwrap();
        Raw {
            ep,
            conn,
            send,
            lines: net::Lines::new(recv),
        }
    }
}
