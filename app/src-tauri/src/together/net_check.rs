//! What a room's networking does today, pinned: the knock and the door, the order messages
//! arrive in, pings, leaving, dropping and coming back as the same person, the host's files
//! streamed to people in the room (and through a guest's small server to its player), and the
//! room's end. Everything runs over real connections on this computer (local only: no relay,
//! nothing online), with a plain peer on the other side speaking the wire protocol, or the
//! other session itself.

use super::net::{self, News, Reach};
use super::net_guest::{GuestHooks, GuestSession, Stage};
use super::net_host::{Get, HostHooks, HostSession};
use iroh::endpoint::{ConnectionError, SendStream};
use iroh::{Endpoint, SecretKey};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use together::invite::Invite;
use together::proto::{self, Action, Role, RoomView, Song, ToGuest, ToHost};
use together::room::Room;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SECRET: &str = "00112233445566778899aabbccddeeff";
/// How long anything here may take before a test gives up (generous: the machine is busy).
const WAIT: Duration = Duration::from_secs(20);

fn local() -> Reach {
    Reach {
        local_only: true,
        ..Reach::default()
    }
}

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes(&[n; 32])
}

fn online(id: &str) -> Song {
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

fn host_song(hex: &str) -> Song {
    Song {
        key: format!("host:{hex}"),
        online_id: None,
        host_file: true,
        format: Some("flac".into()),
        ..online("aaaaaaaaaaa")
    }
}

const FILE_KEY: &str = "host:0123456789abcdef0123456789abcdef";
const OTHER_KEY: &str = "host:fedcba9876543210fedcba9876543210";

/// Bytes that say where they are (so a range shows which part came).
fn counting(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// What the app around a session was told, in order.
#[derive(Debug, Clone, PartialEq)]
enum Told {
    Changed,
    Knocked(String),
    Trace(String),
    News(News),
    Note(String),
    View(RoomView),
}

/// The app around a session, as a record of what it was told.
#[derive(Default)]
struct App {
    told: Mutex<Vec<Told>>,
    files: Mutex<HashMap<String, (PathBuf, String)>>,
    covers: Mutex<HashMap<String, PathBuf>>,
}

impl App {
    fn tell(&self, t: Told) {
        self.told.lock().unwrap().push(t);
    }
    fn told(&self) -> Vec<Told> {
        self.told.lock().unwrap().clone()
    }
    fn news(&self) -> Vec<News> {
        self.told()
            .into_iter()
            .filter_map(|t| match t {
                Told::News(n) => Some(n),
                _ => None,
            })
            .collect()
    }
    fn traces(&self) -> Vec<String> {
        self.told()
            .into_iter()
            .filter_map(|t| match t {
                Told::Trace(s) => Some(s),
                _ => None,
            })
            .collect()
    }
    fn notes(&self) -> Vec<String> {
        self.told()
            .into_iter()
            .filter_map(|t| match t {
                Told::Note(s) => Some(s),
                _ => None,
            })
            .collect()
    }
    async fn until(&self, what: &str, f: impl Fn(&[Told]) -> bool) {
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
async fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < end, "waited for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// --- A host, and a plain guest speaking the protocol to it -------------------------------

struct TestHost {
    s: Arc<HostSession>,
    app: Arc<App>,
    invite: Invite,
}

async fn host(songs: Vec<Song>) -> TestHost {
    host_until(songs, net::unix_now() + 3_600).await
}

async fn host_until(songs: Vec<Song>, until: u64) -> TestHost {
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

fn hello(invite: &Invite, key: &SecretKey, name: &str) -> ToHost {
    ToHost::Hello {
        protocol: together::PROTOCOL,
        name: name.into(),
        proof: invite.proof(&key.public().to_string()),
        app: "Venuta test".into(),
    }
}

/// A guest that speaks the wire protocol and nothing more.
struct Raw {
    ep: Endpoint,
    conn: iroh::endpoint::Connection,
    send: SendStream,
    lines: net::Lines,
}

impl Raw {
    async fn dial(invite: &Invite, key: SecretKey) -> Raw {
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
    async fn knock(invite: &Invite, n: u8, name: &str) -> Raw {
        let mut r = Raw::dial(invite, key(n)).await;
        r.say(&hello(invite, &key(n), name)).await;
        r
    }

    async fn say(&mut self, m: &ToHost) {
        net::send_line(&mut self.send, &proto::line(m))
            .await
            .unwrap();
    }

    /// The next message from the host (`None`: the host finished the stream).
    async fn hear(&mut self) -> Option<ToGuest> {
        tokio::time::timeout(WAIT, self.lines.next())
            .await
            .expect("a message in time")
            .ok()
            .flatten()
            .map(|l| proto::read::<ToGuest>(&l).expect("a message the guest can read"))
    }

    /// Everything the host sends before it answers this ping (it answers in order, so this is
    /// everything it sent before reading the ping).
    async fn until_pong(&mut self, id: u32) -> Vec<ToGuest> {
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
    async fn hear_past_rooms(&mut self) -> Option<ToGuest> {
        loop {
            match self.hear().await {
                Some(ToGuest::Room { .. }) => continue,
                m => return m,
            }
        }
    }

    /// How the host closed the connection: its code and reason.
    async fn closed(&self) -> (u64, Vec<u8>) {
        match tokio::time::timeout(WAIT, self.conn.closed())
            .await
            .expect("closed in time")
        {
            ConnectionError::ApplicationClosed(c) => (c.error_code.into_inner(), c.reason.to_vec()),
            other => panic!("closed otherwise: {other:?}"),
        }
    }

    /// Ask for a file (or a cover) on a stream of its own: the header line, then the bytes.
    async fn get(&self, get: &Get) -> (serde_json::Value, Vec<u8>) {
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
async fn member(h: &TestHost, n: u8, name: &str) -> Raw {
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

// --- The door ------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_knock_waits_at_the_door_until_the_host_lets_them_in() {
    let h = host(vec![online("aaaaaaaaaaa")]).await;
    let mut ali = Raw::knock(&h.invite, 1, "Ali").await;
    assert_eq!(ali.hear().await, Some(ToGuest::Waiting));
    h.app
        .until("the knock", |t| t.contains(&Told::Knocked("Ali".into())))
        .await;
    assert_eq!(
        h.app.told(),
        vec![Told::Knocked("Ali".into()), Told::Changed],
        "the knock, then the window"
    );
    assert_eq!(
        h.s.room.lock().unwrap().view(true).waiting,
        vec![(1, "Ali".to_owned())]
    );
    // Asked before being let in: nothing comes of it. Pings are answered all the same.
    ali.say(&ToHost::Ask {
        action: Action::Play,
    })
    .await;
    ali.say(&ToHost::Ready { rev: 1, index: 0 }).await;
    assert_eq!(ali.until_pong(5).await, vec![]);
    assert!(h.s.room.lock().unwrap().preparing().is_none());
    assert!(!h.s.room.lock().unwrap().is_member(1));

    h.s.admit(1);
    let Some(ToGuest::Welcome { you, room }) = ali.hear().await else {
        panic!("welcomed first")
    };
    assert_eq!(you, 1);
    assert!(room.members.iter().any(|m| m.id == 1 && m.name == "Ali"));
    assert!(room.waiting.is_empty());
    let Some(ToGuest::Room { room: after }) = ali.hear().await else {
        panic!("then the room")
    };
    assert_eq!(after, room, "the same room, sent to everyone");
    // The host's own player and the window hear of it too.
    let told = h.app.told();
    assert_eq!(told.len(), 4, "{told:#?}");
    assert!(matches!(&told[2], Told::News(News::Room(v)) if v.members.len() == 2));
    assert_eq!(told[3], Told::Changed);
    // In now: asking works.
    ali.say(&ToHost::Ask {
        action: Action::Play,
    })
    .await;
    let got = ali.until_pong(6).await;
    assert!(
        got.iter().any(|m| matches!(
            m,
            ToGuest::Prepare {
                index: 0,
                position_ms: 0,
                ..
            }
        )),
        "{got:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_host_can_let_people_in_without_asking_and_one_joining_mid_start_gets_the_start() {
    let h = host(vec![online("aaaaaaaaaaa"), online("bbbbbbbbbbb")]).await;
    h.s.room.lock().unwrap().set_let_in_without_asking(true);
    h.s.ask(Action::Play).unwrap();
    let preparing = h.s.room.lock().unwrap().preparing().unwrap();
    let mut ali = Raw::knock(&h.invite, 1, "Ali").await;
    let Some(ToGuest::Welcome { you: 1, .. }) = ali.hear().await else {
        panic!("straight in")
    };
    let (rev, index, position_ms) = preparing;
    assert_eq!(
        ali.hear().await,
        Some(ToGuest::Prepare {
            rev,
            index,
            position_ms
        }),
        "what's getting ready, right after the welcome"
    );
    assert!(matches!(ali.hear().await, Some(ToGuest::Room { .. })));
    assert!(h.app.traces().contains(&"room: 1 is in".to_owned()));
    assert!(
        !h.app.told().iter().any(|t| matches!(t, Told::Knocked(_))),
        "no knock to tell of"
    );
}

/// Refused at the door before a knock (another version, an ended invite). Today the host writes
/// its refusal and lets go of the connection at once, before the line is surely out: the
/// refusal is often lost, and the guest sees only its connection end.
async fn refused_before_the_knock(r: &mut Raw, why: &str) -> Option<ToGuest> {
    let heard = r.hear().await;
    assert!(
        heard.is_none() || heard == Some(ToGuest::Refused { why: why.into() }),
        "{heard:?}"
    );
    let closed = tokio::time::timeout(WAIT, r.conn.closed())
        .await
        .expect("closed in time");
    assert!(
        !matches!(closed, ConnectionError::ApplicationClosed(ref c) if c.error_code.into_inner() != 0),
        "let go of, not closed with a code: {closed:?}"
    );
    heard
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_protocol_version_is_refused_without_a_knock() {
    let h = host(vec![]).await;
    let mut r = Raw::dial(&h.invite, key(1)).await;
    let ToHost::Hello {
        name, proof, app, ..
    } = hello(&h.invite, &key(1), "Ali")
    else {
        unreachable!()
    };
    r.say(&ToHost::Hello {
        protocol: together::PROTOCOL + 1,
        name,
        proof,
        app,
    })
    .await;
    refused_before_the_knock(
        &mut r,
        "Your Venuta and the host's are different versions. Update both, then join again.",
    )
    .await;
    assert!(h.s.room.lock().unwrap().view(true).waiting.is_empty());
    assert!(h.app.told().is_empty(), "nobody knocked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ended_invite_turns_people_away_without_a_knock() {
    let h = host_until(vec![], 1).await;
    let mut r = Raw::knock(&h.invite, 1, "Ali").await;
    refused_before_the_knock(&mut r, "This room's invite has ended. Ask for a new one.").await;
    assert!(h.app.told().is_empty(), "nobody knocked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proof_for_another_room_is_refused_and_the_connection_closed() {
    let h = host(vec![]).await;
    let mut r = Raw::dial(&h.invite, key(1)).await;
    r.say(&ToHost::Hello {
        protocol: together::PROTOCOL,
        name: "Ali".into(),
        proof: "0".repeat(64),
        app: "Venuta test".into(),
    })
    .await;
    assert_eq!(
        r.hear().await,
        Some(ToGuest::Refused {
            why: "That invite isn't for this room.".into()
        })
    );
    assert_eq!(r.closed().await, (2, b"refused".to_vec()));
    assert!(
        h.app
            .traces()
            .contains(&"room: someone was refused (That invite isn't for this room.)".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_line_that_isnt_a_hello_closes_the_connection() {
    let h = host(vec![]).await;
    let mut r = Raw::dial(&h.invite, key(1)).await;
    net::send_line(&mut r.send, b"{\"t\":\"bye\"}\n")
        .await
        .unwrap();
    assert_eq!(r.closed().await, (1, b"hello".to_vec()));
    // A hello that fails its checks (no name) is no hello either.
    let mut r = Raw::dial(&h.invite, key(2)).await;
    r.say(&hello(&h.invite, &key(2), " ")).await;
    assert_eq!(r.closed().await, (1, b"hello".to_vec()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_hello_within_ten_seconds_closes_the_connection() {
    let h = host(vec![]).await;
    let mut r = Raw::dial(&h.invite, key(1)).await;
    // A stream is only seen once something is on it: a part of a line, never finished.
    net::send_line(&mut r.send, b"{").await.unwrap();
    let t = Instant::now();
    assert_eq!(r.closed().await, (1, b"hello".to_vec()));
    assert!(t.elapsed() >= Duration::from_secs(9), "{:?}", t.elapsed());
}

// --- In the room ------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pings_are_answered_on_the_hosts_clock() {
    let h = host(vec![]).await;
    let mut ali = Raw::knock(&h.invite, 1, "Ali").await;
    assert_eq!(ali.hear().await, Some(ToGuest::Waiting));
    let before = h.s.clock.now_us();
    ali.say(&ToHost::Ping { id: 7, t1: -123 }).await;
    let Some(ToGuest::Pong { id, t1, t2, t3 }) = ali.hear().await else {
        panic!("a pong")
    };
    let after = h.s.clock.now_us();
    assert_eq!((id, t1), (7, -123), "the guest's own id and time, back");
    assert!(
        before <= t2 && t2 <= t3 && t3 <= after,
        "{before} {t2} {t3} {after}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_room_reaches_everyone_in_the_order_it_changes() {
    let h = host(vec![online("aaaaaaaaaaa"), online("bbbbbbbbbbb")]).await;
    let mut ali = member(&h, 1, "Ali").await;
    let mut ben = member(&h, 2, "Ben").await;
    let _ = ali.until_pong(1).await;
    let _ = ben.until_pong(1).await;
    h.s.ask(Action::Play).unwrap();
    for i in 1..=30u64 {
        h.s.ask(Action::Seek {
            position_ms: i * 1_000,
        })
        .unwrap();
    }
    for r in [&mut ali, &mut ben] {
        let got = r.until_pong(2).await;
        let prepares: Vec<u64> = got
            .iter()
            .filter_map(|m| match m {
                ToGuest::Prepare { position_ms, .. } => Some(*position_ms),
                _ => None,
            })
            .collect();
        let want: Vec<u64> = std::iter::once(0)
            .chain((1..=30).map(|i| i * 1_000))
            .collect();
        assert_eq!(prepares, want, "every prepare, in order");
        // Each change sends the room first, then what to get ready.
        let first = got
            .iter()
            .position(|m| matches!(m, ToGuest::Prepare { .. }))
            .unwrap();
        assert!(matches!(got[first - 1], ToGuest::Room { .. }));
    }
    // The host's own player is told the same, in the same order.
    let mine: Vec<u64> = h
        .app
        .news()
        .iter()
        .filter_map(|n| match n {
            News::Prepare { position_ms, .. } => Some(*position_ms),
            _ => None,
        })
        .collect();
    assert_eq!(&mine[..1], &[0]);
    assert_eq!(mine.len(), 31);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_asking_beyond_their_role_gets_the_room_again_and_no_one_else_hears() {
    let h = host(vec![online("aaaaaaaaaaa")]).await;
    let mut ali = member(&h, 1, "Ali").await;
    let mut ben = member(&h, 2, "Ben").await;
    h.s.set(|room| room.set_role(1, Role::Listen));
    let _ = ali.until_pong(1).await;
    let _ = ben.until_pong(1).await;
    ali.say(&ToHost::Ask {
        action: Action::Play,
    })
    .await;
    let got = ali.until_pong(2).await;
    assert_eq!(got.len(), 1, "{got:?}");
    let ToGuest::Room { room } = &got[0] else {
        panic!("the room again: {got:?}")
    };
    assert_eq!(room, &h.s.room.lock().unwrap().view(false));
    assert_eq!(ben.until_pong(2).await, vec![], "only to the one who asked");
    assert!(h.s.room.lock().unwrap().preparing().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_members_reports_reach_the_room() {
    let h = host(vec![online("aaaaaaaaaaa")]).await;
    let mut ali = member(&h, 1, "Ali").await;
    ali.say(&ToHost::Sync { error_ms: 42 }).await;
    let _ = ali.until_pong(1).await;
    let me = |h: &TestHost| {
        h.s.room
            .lock()
            .unwrap()
            .view(true)
            .members
            .into_iter()
            .find(|m| m.id == 1)
            .unwrap()
    };
    assert_eq!(me(&h).error_ms, Some(42));
    ali.say(&ToHost::Hearing {
        rev: 1,
        index: 0,
        how: together::proto::Hearing::Online,
    })
    .await;
    let _ = ali.until_pong(2).await;
    // Hearing is for the song playing; with nothing playing it changes nothing.
    let _ = me(&h);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_goodbye_takes_them_out_at_once() {
    let h = host(vec![]).await;
    let mut ali = member(&h, 1, "Ali").await;
    let mut ben = member(&h, 2, "Ben").await;
    let _ = ben.until_pong(1).await;
    ali.say(&ToHost::Bye).await;
    until("Ali out", || !h.s.room.lock().unwrap().is_member(1)).await;
    let got = ben.until_pong(2).await;
    let Some(ToGuest::Room { room }) = got.last() else {
        panic!("the room without Ali: {got:?}")
    };
    assert!(!room.members.iter().any(|m| m.id == 1));
    assert!(h.app.traces().contains(&"room: 1 left".to_owned()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_connection_is_away_and_back_as_the_same_person() {
    let h = host(vec![]).await;
    let ali = member(&h, 1, "Ali").await;
    ali.conn.close(0u32.into(), b"gone");
    let away = |h: &TestHost| {
        h.s.room
            .lock()
            .unwrap()
            .view(true)
            .members
            .iter()
            .find(|m| m.id == 1)
            .map(|m| m.away)
    };
    until("Ali away", || away(&h) == Some(true)).await;
    assert!(
        h.app
            .traces()
            .contains(&"room: 1's connection dropped".to_owned())
    );
    // Back with the same key: in at once, as the same person.
    let mut back = Raw::knock(&h.invite, 1, "Ali").await;
    let Some(ToGuest::Welcome { you: 1, .. }) = back.hear().await else {
        panic!("straight back in")
    };
    assert_eq!(away(&h), Some(false));
    assert_eq!(h.s.room.lock().unwrap().view(true).members.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_old_connection_ending_leaves_the_new_one_alone() {
    let h = host(vec![]).await;
    let old = member(&h, 1, "Ali").await;
    let mut new = Raw::knock(&h.invite, 1, "Ali").await;
    let Some(ToGuest::Welcome { you: 1, .. }) = new.hear().await else {
        panic!("in on the new connection")
    };
    let _ = new.until_pong(1).await;
    old.conn.close(0u32.into(), b"old");
    old.ep.close().await;
    // The new connection is still theirs: not away, and the room still reaches them.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ali =
        h.s.room
            .lock()
            .unwrap()
            .view(true)
            .members
            .into_iter()
            .find(|m| m.id == 1)
            .unwrap();
    assert!(!ali.away);
    h.s.set(|room| room.set_role(1, Role::Add));
    let got = new.until_pong(2).await;
    assert!(matches!(got.last(), Some(ToGuest::Room { .. })), "{got:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn someone_waiting_who_drops_is_no_longer_at_the_door() {
    let h = host(vec![]).await;
    let mut ali = Raw::knock(&h.invite, 1, "Ali").await;
    assert_eq!(ali.hear().await, Some(ToGuest::Waiting));
    ali.conn.close(0u32.into(), b"gone");
    until("the door empty", || {
        h.s.room.lock().unwrap().view(true).waiting.is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_host_turning_someone_away_tells_them_closes_and_a_block_keeps_them_out() {
    let h = host(vec![]).await;
    let mut ali = Raw::knock(&h.invite, 1, "Ali").await;
    assert_eq!(ali.hear().await, Some(ToGuest::Waiting));
    h.s.turn_away(1, false, "Not this time.");
    assert_eq!(
        ali.hear().await,
        Some(ToGuest::Refused {
            why: "Not this time.".into()
        })
    );
    assert_eq!(ali.closed().await, (3, b"removed".to_vec()));
    assert!(
        h.app
            .traces()
            .contains(&"room: the host took 1 out (blocked: false)".to_owned())
    );
    // Taken out of the room and blocked: told, closed, and refused when they knock again.
    let mut ben = member(&h, 2, "Ben").await;
    h.s.turn_away(2, true, "Out.");
    assert_eq!(
        ben.hear_past_rooms().await,
        Some(ToGuest::Refused { why: "Out.".into() })
    );
    assert_eq!(ben.closed().await, (3, b"removed".to_vec()));
    assert!(!h.s.room.lock().unwrap().is_member(2));
    let mut again = Raw::knock(&h.invite, 2, "Ben").await;
    assert_eq!(
        again.hear().await,
        Some(ToGuest::Refused {
            why: "The host isn't letting you in.".into()
        })
    );
    assert_eq!(again.closed().await, (2, b"refused".to_vec()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ending_the_room_tells_everyone_then_closes() {
    let h = host(vec![]).await;
    let mut ali = member(&h, 1, "Ali").await;
    let mut ben = Raw::knock(&h.invite, 2, "Ben").await;
    assert_eq!(ben.hear().await, Some(ToGuest::Waiting));
    let _ = ali.until_pong(1).await;
    h.s.end().await;
    let ended = Some(ToGuest::Ended {
        why: "Asha ended the room.".into(),
    });
    assert_eq!(ali.hear_past_rooms().await, ended);
    assert_eq!(ben.hear().await, ended, "the door too");
    assert!(h.s.ep.is_closed());
    // The host's own player is stopped. (As the endpoint closes, members' connections end and
    // they are marked away: today those views still go out after the stop.)
    let news = h.app.news();
    let stop = news.iter().position(|n| *n == News::Stop).expect("stopped");
    assert!(
        news[stop + 1..]
            .iter()
            .all(|n| matches!(n, News::Room(v) if v.members.iter().all(|m| m.host || m.away))),
        "{news:#?}"
    );
    assert!(
        h.app
            .traces()
            .contains(&"room: the host ended it".to_owned())
    );
    tokio::time::timeout(WAIT, ali.conn.closed()).await.unwrap();
}

// --- The host's files -----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hosts_files_are_streamed_to_people_in_the_room_as_asked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.flac");
    let body = counting(300_000);
    std::fs::write(&path, &body).unwrap();
    let cover = dir.path().join("cover.jpg");
    std::fs::write(&cover, b"jpeg bytes").unwrap();
    let hex = &FILE_KEY["host:".len()..];
    let h = host(vec![host_song(hex), online("aaaaaaaaaaa")]).await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path.clone(), "audio/flac".into()));
    h.app
        .covers
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), cover.clone());
    let mut ben = Raw::knock(&h.invite, 2, "Ben").await;
    assert_eq!(ben.hear().await, Some(ToGuest::Waiting));
    let ali = member(&h, 1, "Ali").await;
    let get = |from, to| Get {
        get: FILE_KEY.into(),
        from,
        to,
    };
    let (head, bytes) = ali.get(&get(0, None)).await;
    assert_eq!(
        head,
        serde_json::json!({"size": 300_000, "mime": "audio/flac"})
    );
    assert_eq!(bytes, body, "the whole file, as it is");
    let (head, bytes) = ali.get(&get(1_000, Some(1_999))).await;
    assert_eq!(head["size"], 300_000);
    assert_eq!(bytes, &body[1_000..2_000], "a range, both ends in");
    let (_, bytes) = ali.get(&get(299_990, Some(400_000))).await;
    assert_eq!(
        bytes,
        &body[299_990..],
        "a range past the end stops at the end"
    );
    let (head, bytes) = ali.get(&get(400_000, None)).await;
    assert_eq!(head["size"], 300_000);
    assert!(bytes.is_empty(), "from past the end: the size, no bytes");
    // Its cover.
    let (head, bytes) = ali
        .get(&Get {
            get: format!("cover:{FILE_KEY}"),
            from: 0,
            to: None,
        })
        .await;
    assert_eq!(head, serde_json::json!({"size": 10, "mime": "image/jpeg"}));
    assert_eq!(bytes, b"jpeg bytes");
    let refused = serde_json::json!({"error": "not in the room"});
    // Only to people in the room, only songs of the room's that are the host's files.
    assert_eq!(ben.get(&get(0, None)).await, (refused.clone(), vec![]));
    let not_ours = Get {
        get: OTHER_KEY.into(),
        from: 0,
        to: None,
    };
    assert_eq!(ali.get(&not_ours).await, (refused.clone(), vec![]));
    let online_one = Get {
        get: "online:aaaaaaaaaaa".into(),
        from: 0,
        to: None,
    };
    assert_eq!(ali.get(&online_one).await, (refused, vec![]));
    // The file gone from the disk.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        ali.get(&get(0, None)).await,
        (serde_json::json!({"error": "not there"}), vec![])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_file_is_its_size_and_nothing_more() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.flac");
    std::fs::write(&path, b"").unwrap();
    let h = host(vec![host_song(&FILE_KEY["host:".len()..])]).await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path, "audio/flac".into()));
    let ali = member(&h, 1, "Ali").await;
    let (head, bytes) = ali
        .get(&Get {
            get: FILE_KEY.into(),
            from: 0,
            to: None,
        })
        .await;
    assert_eq!(head, serde_json::json!({"size": 0, "mime": "audio/flac"}));
    assert!(bytes.is_empty());
}

// --- A guest, and a plain host speaking the protocol to it ----------------------------------

/// A host that speaks the wire protocol and nothing more.
struct RawHost {
    ep: Endpoint,
    invite: Invite,
}

/// One connection to it: the guest's first stream.
struct Visit {
    conn: iroh::endpoint::Connection,
    send: SendStream,
    lines: net::Lines,
}

impl RawHost {
    async fn new(until: u64) -> RawHost {
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

    async fn visit(&self) -> Visit {
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
    async fn hear(&mut self) -> Option<ToHost> {
        tokio::time::timeout(WAIT, self.lines.next())
            .await
            .expect("a message in time")
            .ok()
            .flatten()
            .map(|l| proto::read::<ToHost>(&l).expect("a message the host can read"))
    }
    /// The next message that isn't a ping.
    async fn hear_past_pings(&mut self) -> Option<ToHost> {
        loop {
            match self.hear().await {
                Some(ToHost::Ping { .. }) => continue,
                m => return m,
            }
        }
    }
    async fn say(&mut self, m: &ToGuest) {
        net::send_line(&mut self.send, &proto::line(m))
            .await
            .unwrap();
    }
}

fn a_room() -> RoomView {
    Room::new("Asha", SECRET, vec![online("aaaaaaaaaaa")]).view(false)
}

async fn guest_of(invite: &Invite, keys: &std::path::Path) -> (Arc<GuestSession>, Arc<App>) {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_says_hello_with_its_proof_then_follows_the_door() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let visit = tokio::spawn({
        let ep = h.ep.clone();
        let invite = h.invite.clone();
        async move { RawHost { ep, invite }.visit().await }
    });
    let (g, app) = guest_of(&h.invite, keys.path()).await;
    let mut v = visit.await.unwrap();
    let who = v.conn.remote_id().to_string();
    assert_eq!(
        v.hear().await,
        Some(ToHost::Hello {
            protocol: together::PROTOCOL,
            name: "Ali".into(),
            proof: h.invite.proof(&who),
            app: "Venuta 9.9.9".into(),
        }),
        "the hello first, its proof bound to this connection's key"
    );
    assert_eq!(
        who,
        net::guest_key(keys.path(), SECRET).public().to_string(),
        "the key kept for this room"
    );
    assert_eq!(*g.stage.lock().unwrap(), Stage::Joining);
    // Pings from the start: a burst, numbered from 0, about 80 ms apart.
    let t = Instant::now();
    for want in 0..5u32 {
        let Some(ToHost::Ping { id, .. }) = v.hear().await else {
            panic!("a ping")
        };
        assert_eq!(id, want);
    }
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    v.say(&ToGuest::Waiting).await;
    until("at the door", || *g.stage.lock().unwrap() == Stage::Waiting).await;
    let room = a_room();
    v.say(&ToGuest::Welcome {
        you: 3,
        room: room.clone(),
    })
    .await;
    until("in", || *g.stage.lock().unwrap() == Stage::In).await;
    assert_eq!(*g.me.lock().unwrap(), Some(3));
    assert_eq!(g.view.lock().unwrap().as_ref(), Some(&room));
    assert!(app.told().contains(&Told::View(room.clone())));
    assert!(
        app.news().is_empty(),
        "nothing to follow before the clock is known"
    );
    // The clock: once the host's answers make it known, the room is followed.
    let mut answered = 0;
    while app.news().is_empty() {
        let Some(ToHost::Ping { id, t1 }) = v.hear().await else {
            panic!("pings go on")
        };
        let now = 5_000_000 + t1;
        v.say(&ToGuest::Pong {
            id,
            t1,
            t2: now,
            t3: now,
        })
        .await;
        answered += 1;
        assert!(answered < 20, "the clock never became known");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(g.clock.lock().unwrap().ready());
    assert_eq!(app.news(), vec![News::Room(room.clone())]);
    // What to get ready goes to the player as it comes.
    v.say(&ToGuest::Prepare {
        rev: 4,
        index: 0,
        position_ms: 1_500,
    })
    .await;
    app.until("the prepare", |t| {
        t.contains(&Told::News(News::Prepare {
            rev: 4,
            index: 0,
            position_ms: 1_500,
        }))
    })
    .await;
    // A room view, once the clock is known: to the player at once.
    let mut later = room.clone();
    later.rev = 9;
    v.say(&ToGuest::Room {
        room: later.clone(),
    })
    .await;
    app.until("the new room followed", |t| {
        t.contains(&Told::News(News::Room(later.clone())))
    })
    .await;
    assert_eq!(g.path().map(|p| p.direct), Some(true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_or_ended_by_the_host_ends_the_room_for_the_guest() {
    for (m, why) in [
        (ToGuest::Refused { why: "No.".into() }, "No."),
        (
            ToGuest::Ended {
                why: "Asha ended the room.".into(),
            },
            "Asha ended the room.",
        ),
    ] {
        let keys = tempfile::tempdir().unwrap();
        let h = RawHost::new(net::unix_now() + 3_600).await;
        let ep = h.ep.clone();
        let invite = h.invite.clone();
        let visit = tokio::spawn(async move { RawHost { ep, invite }.visit().await });
        let (g, app) = guest_of(&h.invite, keys.path()).await;
        let mut v = visit.await.unwrap();
        v.say(&m).await;
        until("ended", || *g.stage.lock().unwrap() == Stage::Ended).await;
        assert_eq!(g.why.lock().unwrap().as_deref(), Some(why));
        assert_eq!(app.news(), vec![News::Stop]);
        assert!(
            app.traces()
                .contains(&format!("room: over for this guest ({why})"))
        );
        // The guest lets go of the connection, and stops pinging.
        tokio::time::timeout(WAIT, v.conn.closed()).await.unwrap();
        g.ask(Action::Play);
        assert!(app.notes().is_empty(), "a press after the end goes nowhere");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_goes_at_once_or_waits_for_the_connection_and_the_guest_comes_back_as_itself() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let ep = h.ep.clone();
    let invite = h.invite.clone();
    let visit = tokio::spawn(async move { RawHost { ep, invite }.visit().await });
    let (g, app) = guest_of(&h.invite, keys.path()).await;
    let mut v = visit.await.unwrap();
    let who = v.conn.remote_id();
    assert!(matches!(v.hear().await, Some(ToHost::Hello { .. })));
    v.say(&ToGuest::Welcome {
        you: 1,
        room: a_room(),
    })
    .await;
    until("in", || *g.stage.lock().unwrap() == Stage::In).await;
    g.ask(Action::Pause);
    assert_eq!(
        v.hear_past_pings().await,
        Some(ToHost::Ask {
            action: Action::Pause
        })
    );
    // The host drops the connection: the guest tries again, and what's pressed meanwhile waits.
    let next = tokio::spawn({
        let ep = h.ep.clone();
        let invite = h.invite.clone();
        async move { RawHost { ep, invite }.visit().await }
    });
    v.conn.close(9u32.into(), b"blip");
    until("reconnecting", || {
        *g.stage.lock().unwrap() == Stage::Reconnecting
    })
    .await;
    assert!(
        app.traces()
            .contains(&"room: the connection to the host dropped; trying again".to_owned())
    );
    for i in 0..25u64 {
        g.ask(Action::Seek {
            position_ms: i * 1_000,
        });
    }
    assert_eq!(
        app.notes(),
        vec![
            "Reconnecting: your press will reach the room as soon as the connection is back."
                .to_owned();
            25
        ],
        "each press said to wait"
    );
    let t = Instant::now();
    let mut v2 = next.await.unwrap();
    assert!(
        t.elapsed() >= Duration::from_secs(2),
        "tried again after a few seconds ({:?})",
        t.elapsed()
    );
    assert_eq!(v2.conn.remote_id(), who, "the same key: the same person");
    assert!(matches!(v2.hear().await, Some(ToHost::Hello { .. })));
    // The held presses first, in order, at most 20 of them; then the pings.
    for i in 0..20u64 {
        assert_eq!(
            v2.hear().await,
            Some(ToHost::Ask {
                action: Action::Seek {
                    position_ms: i * 1_000
                }
            })
        );
    }
    assert!(matches!(v2.hear().await, Some(ToHost::Ping { id: 0, .. })));
    app.until("back", |t| {
        t.contains(&Told::Trace("room: back in touch with the host".into()))
    })
    .await;
    // Back, the stage stays where it was until the host says otherwise.
    assert_eq!(*g.stage.lock().unwrap(), Stage::Reconnecting);
    v2.say(&ToGuest::Welcome {
        you: 1,
        room: a_room(),
    })
    .await;
    until("in again", || *g.stage.lock().unwrap() == Stage::In).await;
    g.ask(Action::Play);
    assert_eq!(
        v2.hear_past_pings().await,
        Some(ToHost::Ask {
            action: Action::Play
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leaving_says_goodbye_then_lets_go() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let ep = h.ep.clone();
    let invite = h.invite.clone();
    let visit = tokio::spawn(async move { RawHost { ep, invite }.visit().await });
    let (g, app) = guest_of(&h.invite, keys.path()).await;
    let mut v = visit.await.unwrap();
    assert!(matches!(v.hear().await, Some(ToHost::Hello { .. })));
    g.leave().await;
    assert_eq!(v.hear_past_pings().await, Some(ToHost::Bye));
    assert_eq!(*g.stage.lock().unwrap(), Stage::Ended);
    assert_eq!(app.news(), vec![News::Stop]);
    assert_eq!(*g.why.lock().unwrap(), None);
    assert!(
        app.traces()
            .contains(&"room: over for this guest (left)".to_owned())
    );
    tokio::time::timeout(WAIT, v.conn.closed()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ended_invite_is_refused_before_connecting() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(1).await;
    let r = GuestSession::join(
        h.invite.clone(),
        &local(),
        keys.path(),
        "Venuta".into(),
        Arc::new(App::default()),
    )
    .await;
    assert_eq!(
        r.err(),
        Some("This room's invite has ended. Ask for a new one.".to_owned())
    );
    assert!(
        !keys.path().join("together.key").exists(),
        "nothing made for it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_guests_small_server_asks_the_host_as_its_player_asks() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let ep = h.ep.clone();
    let invite = h.invite.clone();
    let visit = tokio::spawn(async move { RawHost { ep, invite }.visit().await });
    let (g, _app) = guest_of(&h.invite, keys.path()).await;
    let v = visit.await.unwrap();
    let (port, token) = g.proxy_address();
    // The host's side: every file stream the guest opens, its request kept, then an answer.
    let asked: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let conn = v.conn.clone();
    let kept = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut send, recv)) = conn.accept_bi().await {
            let mut lines = net::Lines::new(recv);
            let req: serde_json::Value =
                serde_json::from_slice(&lines.next().await.unwrap().unwrap()).unwrap();
            kept.lock().unwrap().push(req.clone());
            if req["get"] == FILE_KEY {
                net::send_line(&mut send, b"{\"size\":100,\"mime\":\"audio/flac\"}\n")
                    .await
                    .unwrap();
                let from = req["from"].as_u64().unwrap() as usize;
                let to = req["to"].as_u64().map(|t| t as usize).unwrap_or(99);
                net::send_line(&mut send, &counting(100)[from..=to])
                    .await
                    .unwrap();
            } else {
                net::send_line(&mut send, b"{\"error\":\"not in the room\"}\n")
                    .await
                    .unwrap();
            }
            send.finish().unwrap();
        }
    });
    let http = |req: String| async move {
        let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        tcp.write_all(req.as_bytes()).await.unwrap();
        let mut got = Vec::new();
        tokio::time::timeout(WAIT, tcp.read_to_end(&mut got))
            .await
            .unwrap()
            .unwrap();
        got
    };
    let split = |got: &[u8]| {
        let at = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8(got[..at].to_vec()).unwrap(),
            got[at + 4..].to_vec(),
        )
    };
    let path = format!("/s/{FILE_KEY}?t={token}");
    let (head, body) = split(&http(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n")).await);
    assert_eq!(
        head,
        "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: 100\r\nAccept-Ranges: bytes\r\nConnection: close"
    );
    assert_eq!(body, counting(100));
    let (head, body) =
        split(&http(format!("GET {path} HTTP/1.1\r\nrange: bytes=10-19\r\n\r\n")).await);
    assert_eq!(
        head,
        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 10-19/100\r\nContent-Type: audio/flac\r\nContent-Length: 10\r\nAccept-Ranges: bytes\r\nConnection: close"
    );
    assert_eq!(body, &counting(100)[10..20]);
    let (head, body) =
        split(&http(format!("GET {path} HTTP/1.1\r\nRange: bytes=90-\r\n\r\n")).await);
    assert!(head.contains("Content-Range: bytes 90-99/100"), "{head}");
    assert_eq!(body, &counting(100)[90..]);
    let (head, body) =
        split(&http(format!("HEAD {path} HTTP/1.1\r\nRange: bytes=5-\r\n\r\n")).await);
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert!(head.contains("Content-Length: 95"), "{head}");
    assert!(body.is_empty(), "a HEAD is headers only");
    // What the host was asked, each on a stream of its own.
    assert_eq!(
        *asked.lock().unwrap(),
        vec![
            serde_json::json!({"get": FILE_KEY, "from": 0, "to": null}),
            serde_json::json!({"get": FILE_KEY, "from": 10, "to": 19}),
            serde_json::json!({"get": FILE_KEY, "from": 90, "to": null}),
            serde_json::json!({"get": FILE_KEY, "from": 5, "to": 5}),
        ]
    );
    // Refused here, never asked of the host.
    let status = |got: Vec<u8>| {
        String::from_utf8_lossy(&got)
            .lines()
            .next()
            .unwrap()
            .to_owned()
    };
    let bad_token = format!("/s/{FILE_KEY}?t=nope");
    assert_eq!(
        status(http(format!("GET {bad_token} HTTP/1.1\r\n\r\n")).await),
        "HTTP/1.1 403 Forbidden"
    );
    assert_eq!(
        status(
            http(format!(
                "GET /s/online:aaaaaaaaaaa?t={token} HTTP/1.1\r\n\r\n"
            ))
            .await
        ),
        "HTTP/1.1 403 Forbidden"
    );
    assert_eq!(
        status(http(format!("POST {path} HTTP/1.1\r\n\r\n")).await),
        "HTTP/1.1 403 Forbidden"
    );
    assert_eq!(
        status(http(format!("GET /s/{FILE_KEY} HTTP/1.1\r\n\r\n")).await),
        "HTTP/1.1 404 Not Found"
    );
    assert_eq!(asked.lock().unwrap().len(), 4);
    // The host says no: not found.
    assert_eq!(
        status(http(format!("GET /s/{OTHER_KEY}?t={token} HTTP/1.1\r\n\r\n")).await),
        "HTTP/1.1 404 Not Found"
    );
    assert_eq!(asked.lock().unwrap().len(), 5);
}

// --- Both sessions, over this network -------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_whole_room_host_and_guest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.flac");
    let body = counting(200_000);
    std::fs::write(&path, &body).unwrap();
    let cover = dir.path().join("cover.jpg");
    std::fs::write(&cover, counting(5_000)).unwrap();
    let h = host(vec![
        host_song(&FILE_KEY["host:".len()..]),
        online("aaaaaaaaaaa"),
    ])
    .await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path, "audio/flac".into()));
    h.app.covers.lock().unwrap().insert(FILE_KEY.into(), cover);
    let keys = tempfile::tempdir().unwrap();
    let (g, app) = guest_of(&h.invite, keys.path()).await;
    // The knock.
    until("at the door", || *g.stage.lock().unwrap() == Stage::Waiting).await;
    assert!(h.app.told().contains(&Told::Knocked("Ali".into())));
    h.s.admit(1);
    until("in", || *g.stage.lock().unwrap() == Stage::In).await;
    assert_eq!(*g.me.lock().unwrap(), Some(1));
    // The clock becomes known from the host's answers, and the room is followed.
    app.until("the room followed", |t| {
        t.iter().any(|t| matches!(t, Told::News(News::Room(_))))
    })
    .await;
    // The host starts a song: both players get it ready.
    h.s.ask(Action::Play).unwrap();
    let (rev, index, position_ms) = h.s.room.lock().unwrap().preparing().unwrap();
    let prepare = News::Prepare {
        rev,
        index,
        position_ms,
    };
    app.until("the prepare", |t| t.contains(&Told::News(prepare.clone())))
        .await;
    assert!(h.app.news().contains(&prepare));
    // The guest's press reaches the room.
    g.send(ToHost::Ready { rev, index });
    g.ask(Action::Pause);
    until("paused by the guest", || {
        h.s.room
            .lock()
            .unwrap()
            .timeline()
            .is_some_and(|t| !t.playing)
            && h.s.room.lock().unwrap().preparing().is_none()
    })
    .await;
    // The host's file, through the guest's small server, as its player would ask.
    let (port, token) = g.proxy_address();
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    tcp.write_all(format!("GET /s/{FILE_KEY}?t={token} HTTP/1.1\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut got = Vec::new();
    tokio::time::timeout(WAIT, tcp.read_to_end(&mut got))
        .await
        .unwrap()
        .unwrap();
    let at = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    assert_eq!(&got[at + 4..], &body[..]);
    // Its cover.
    assert_eq!(
        g.fetch_cover(FILE_KEY, 2 * 1024 * 1024).await,
        Some(counting(5_000))
    );
    assert_eq!(g.fetch_cover(FILE_KEY, 4_999).await, None, "too big");
    assert_eq!(g.fetch_cover(OTHER_KEY, 1 << 20).await, None, "none");
    assert_eq!(g.path().map(|p| p.direct), Some(true));
    assert!(h.s.all_on_one_network() || g.path().and_then(|p| p.rtt_ms).is_none_or(|r| r > 8));
    // The host ends it.
    h.s.end().await;
    until("ended", || *g.stage.lock().unwrap() == Stage::Ended).await;
    assert_eq!(
        g.why.lock().unwrap().as_deref(),
        Some("Asha ended the room.")
    );
    assert_eq!(app.news().last(), Some(&News::Stop));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_joining_again_with_its_key_is_the_same_person_and_after_a_goodbye_a_new_one() {
    let h = host(vec![]).await;
    h.s.room.lock().unwrap().set_let_in_without_asking(true);
    let keys = tempfile::tempdir().unwrap();
    let (g, _) = guest_of(&h.invite, keys.path()).await;
    until("in", || *g.me.lock().unwrap() == Some(1)).await;
    // Gone without a goodbye (the app closed): back as the same person.
    g.close().await;
    until("away", || {
        h.s.room
            .lock()
            .unwrap()
            .view(true)
            .members
            .iter()
            .any(|m| m.id == 1 && m.away)
    })
    .await;
    let (g, _) = guest_of(&h.invite, keys.path()).await;
    until("in again", || *g.me.lock().unwrap() == Some(1)).await;
    // After a goodbye: someone new to the room.
    g.leave().await;
    until("out", || !h.s.room.lock().unwrap().is_member(1)).await;
    let (g, _) = guest_of(&h.invite, keys.path()).await;
    until("in as new", || *g.me.lock().unwrap() == Some(2)).await;
    // Another computer (another key) is another person.
    let other = tempfile::tempdir().unwrap();
    let (g2, _) = guest_of(&h.invite, other.path()).await;
    until("the other in", || *g2.me.lock().unwrap() == Some(3)).await;
}
