//! The iroh transport against the Transport contract (design 4.1, 5.6), over real connections
//! on this computer: control lines in order and stamped, clock datagrams (and the clock stream
//! when a connection has none), media from disk, lost and back as the same peer, path changes.

mod common;
use common::*;

use std::sync::mpsc::Receiver;
use together::time::{MonoClock, RealClock};
use together::transport::{
    ClockDatagram, ControlMsg, Get as MediaGet, MediaInfo, MediaServer, MediaStream, PeerId,
    Transport, TransportEvent,
};
use together_net::transport::{HOST, IrohTransport, Timing};

fn clock() -> Arc<dyn MonoClock> {
    Arc::new(RealClock)
}

/// The next event that `f` picks, within `WAIT`.
async fn next<T>(
    rx: &Receiver<TransportEvent>,
    what: &str,
    f: impl Fn(TransportEvent) -> Option<T>,
) -> T {
    let end = Instant::now() + WAIT;
    loop {
        while let Ok(e) = rx.try_recv() {
            if let Some(t) = f(e) {
                return t;
            }
        }
        assert!(Instant::now() < end, "waited for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Whether `f` picks any event within `d`.
async fn any_within(
    rx: &Receiver<TransportEvent>,
    d: Duration,
    f: impl Fn(&TransportEvent) -> bool,
) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        while let Ok(e) = rx.try_recv() {
            if f(&e) {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    false
}

fn joined(e: TransportEvent) -> Option<PeerId> {
    match e {
        TransportEvent::Joined { peer } => Some(peer),
        _ => None,
    }
}

/// Files and bytes the host serves.
#[derive(Default)]
struct Shelf {
    files: HashMap<String, MediaInfo>,
    bytes: HashMap<String, Arc<[u8]>>,
}

impl MediaServer for Shelf {
    fn open(&self, _from: PeerId, get: &MediaGet) -> Option<Arc<[u8]>> {
        self.bytes.get(&get.get).cloned()
    }
    fn describe(&self, _from: PeerId, get: &MediaGet) -> Option<MediaInfo> {
        self.files.get(&get.get).cloned()
    }
}

fn quick() -> Timing {
    Timing {
        redial_first: Duration::from_millis(100),
        redial_most: Duration::from_millis(300),
        redial_for: Duration::from_secs(20),
    }
}

struct Pair {
    host: IrohTransport,
    guest: IrohTransport,
    hrx: Receiver<TransportEvent>,
    grx: Receiver<TransportEvent>,
}

async fn pair_with(shelf: Shelf, guest_cfg: iroh::endpoint::QuicTransportConfig) -> Pair {
    let hep = endpoint_on(false, true, None).await;
    let addr = hep.addr();
    let host = IrohTransport::host(hep, clock(), Arc::new(shelf));
    let hrx = host.events();
    let gep = endpoint_with(false, false, Some(key(1)), guest_cfg).await;
    let guest = IrohTransport::guest(gep, addr, clock(), quick())
        .await
        .unwrap();
    let grx = guest.events();
    Pair {
        host,
        guest,
        hrx,
        grx,
    }
}

async fn pair(shelf: Shelf) -> Pair {
    pair_with(shelf, net::transport_config(net::IDLE_TIMEOUT, true)).await
}

fn line(s: &str) -> ControlMsg {
    ControlMsg(format!("{s}\n").into_bytes())
}

/// The host on [::1] behind a link the guest (on 127.0.0.1) reaches it through, with this idle
/// timeout on both.
async fn behind_a_link(
    idle: Duration,
    timing: Timing,
) -> (
    IrohTransport,
    Receiver<TransportEvent>,
    IrohTransport,
    Receiver<TransportEvent>,
    Link,
) {
    let cfg = || net::transport_config(idle, true);
    let hep = endpoint_with(true, true, None, cfg()).await;
    let addr = hep.addr();
    let port = addr.ip_addrs().next().unwrap().port();
    let host = IrohTransport::host(hep, clock(), Arc::new(Shelf::default()));
    let hrx = host.events();
    let l = link(
        std::net::SocketAddr::new("::1".parse().unwrap(), port),
        1e9,
        1 << 20,
        Duration::ZERO,
    )
    .await;
    let through = iroh::EndpointAddr::new(addr.id).with_ip_addr(l.addr);
    let gep = endpoint_with(false, false, Some(key(1)), cfg()).await;
    let guest = IrohTransport::guest(gep, through, clock(), timing)
        .await
        .unwrap();
    let grx = guest.events();
    (host, hrx, guest, grx, l)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_lines_go_both_ways_in_order_stamped_as_they_are_read() {
    let p = pair(Shelf::default()).await;
    for i in 0..50 {
        p.guest.send(HOST, line(&format!("{{\"n\":{i}}}"))).unwrap();
    }
    let peer = next(&p.hrx, "joined", joined).await;
    for i in 0..50 {
        let (msg, at_ns) = next(&p.hrx, "a line", |e| match e {
            TransportEvent::Message { from, msg, at_ns } if from == peer => Some((msg, at_ns)),
            _ => None,
        })
        .await;
        assert_eq!(msg, line(&format!("{{\"n\":{i}}}")), "in order");
        assert!(at_ns <= RealClock.now_ns());
    }
    let before = RealClock.now_ns();
    p.host.send(peer, line("{\"back\":1}")).unwrap();
    let (msg, at_ns) = next(&p.grx, "the answer", |e| match e {
        TransportEvent::Message {
            from: HOST,
            msg,
            at_ns,
        } => Some((msg, at_ns)),
        _ => None,
    })
    .await;
    assert_eq!(msg, line("{\"back\":1}"));
    assert!(
        before <= at_ns && at_ns <= RealClock.now_ns(),
        "stamped on arrival"
    );
    assert!(p.host.path(peer).is_some_and(|path| path.direct));
    assert!(p.guest.path(HOST).is_some_and(|path| path.direct));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_line_over_the_limit_ends_the_connection() {
    let p = pair(Shelf::default()).await;
    p.guest.send(HOST, line("{\"hello\":1}")).unwrap();
    let peer = next(&p.hrx, "joined", joined).await;
    let mut big = vec![b'x'; together::proto::MAX_MESSAGE + 10];
    big.push(b'\n');
    p.guest.send(HOST, ControlMsg(big)).unwrap();
    next(&p.hrx, "lost", |e| {
        (e == TransportEvent::Lost { peer }).then_some(())
    })
    .await;
    next(&p.grx, "lost", |e| {
        (e == TransportEvent::Lost { peer: HOST }).then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clock_datagrams_go_both_ways_stamped_and_the_host_takes_fifty_a_second() {
    let p = pair(Shelf::default()).await;
    p.guest.send(HOST, line("{\"hello\":1}")).unwrap();
    let peer = next(&p.hrx, "joined", joined).await;
    let ping = ClockDatagram::Ping { id: 7, t1: 123 };
    let sent = RealClock.now_ns();
    p.guest.send_datagram(HOST, ping.encode()).unwrap();
    let (bytes, at_ns) = next(&p.hrx, "the ping", |e| match e {
        TransportEvent::Datagram { from, bytes, at_ns } if from == peer => Some((bytes, at_ns)),
        _ => None,
    })
    .await;
    assert_eq!(ClockDatagram::decode(&bytes), Some(ping));
    assert!(sent <= at_ns);
    let pong = ClockDatagram::Pong {
        id: 7,
        t1: 123,
        t2: 5,
        t3: 6,
    };
    p.host.send_datagram(peer, pong.encode()).unwrap();
    let bytes = next(&p.grx, "the pong", |e| match e {
        TransportEvent::Datagram {
            from: HOST, bytes, ..
        } => Some(bytes),
        _ => None,
    })
    .await;
    assert_eq!(ClockDatagram::decode(&bytes), Some(pong));
    // A flood: the host passes on at most 50 a second (a burst of 50, then 50 a second).
    let t = Instant::now();
    for id in 0..400 {
        let _ = p
            .guest
            .send_datagram(HOST, ClockDatagram::Ping { id, t1: 0 }.encode());
        if id % 50 == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let elapsed = t.elapsed().as_secs_f64();
    let got = p
        .hrx
        .try_iter()
        .filter(|e| matches!(e, TransportEvent::Datagram { .. }))
        .count();
    assert!(got >= 40, "{got}");
    assert!(
        got as f64 <= 50.0 + 50.0 * elapsed + 2.0,
        "{got} in {elapsed:.2} s"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_without_datagrams_carries_the_clock_on_a_stream_of_its_own() {
    // The guest takes no datagrams: the host's pongs go on the clock stream.
    let p = pair_with(
        Shelf::default(),
        net::transport_config(net::IDLE_TIMEOUT, false),
    )
    .await;
    p.guest.send(HOST, line("{\"hello\":1}")).unwrap();
    let peer = next(&p.hrx, "joined", joined).await;
    let ping = ClockDatagram::Ping { id: 1, t1: 9 };
    p.guest.send_datagram(HOST, ping.encode()).unwrap();
    let bytes = next(&p.hrx, "the ping", |e| match e {
        TransportEvent::Datagram { bytes, .. } => Some(bytes),
        _ => None,
    })
    .await;
    assert_eq!(ClockDatagram::decode(&bytes), Some(ping));
    let pong = ClockDatagram::Pong {
        id: 1,
        t1: 9,
        t2: 10,
        t3: 11,
    };
    // The clock stream may still be on its way when the first ping is in (an answer before it
    // is lost, as a datagram may be): the host answers each ping, as a session does.
    until("the clock stream", || {
        p.host.send_datagram(peer, pong.encode()).is_ok()
    })
    .await;
    let bytes = next(&p.grx, "the pong", |e| match e {
        TransportEvent::Datagram {
            from: HOST, bytes, ..
        } => Some(bytes),
        _ => None,
    })
    .await;
    assert_eq!(ClockDatagram::decode(&bytes), Some(pong));
}

async fn read_all(s: &mut MediaStream) -> Vec<u8> {
    let mut v = Vec::new();
    while let Some(c) = tokio::time::timeout(WAIT, s.body.read_chunk())
        .await
        .unwrap()
        .unwrap()
    {
        v.extend_from_slice(&c);
    }
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_come_from_the_hosts_disk_by_range_and_bytes_from_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.flac");
    let body = counting(500_000);
    std::fs::write(&path, &body).unwrap();
    let mut shelf = Shelf::default();
    shelf.files.insert(
        FILE_KEY.into(),
        MediaInfo {
            file: Some(path),
            mime: Some("audio/flac".into()),
            duration_ms: Some(1_000),
        },
    );
    shelf
        .bytes
        .insert(format!("env:{FILE_KEY}"), counting(5_000).into());
    let p = pair(shelf).await;
    let get = |get: &str, from, to| MediaGet {
        get: get.into(),
        from,
        to,
    };
    let mut s = p
        .guest
        .open_media(HOST, get(FILE_KEY, 0, None))
        .await
        .unwrap();
    assert_eq!(
        (s.total, s.mime.as_deref()),
        (Some(500_000), Some("audio/flac"))
    );
    assert_eq!(read_all(&mut s).await, body);
    // `to` is exclusive.
    let mut s = p
        .guest
        .open_media(HOST, get(FILE_KEY, 1_000, Some(3_000)))
        .await
        .unwrap();
    assert_eq!(s.total, Some(2_000));
    assert_eq!(read_all(&mut s).await, &body[1_000..3_000]);
    let mut s = p
        .guest
        .open_media(HOST, get(FILE_KEY, 499_000, Some(900_000)))
        .await
        .unwrap();
    assert_eq!(s.total, Some(1_000), "cut at the end");
    assert_eq!(read_all(&mut s).await, &body[499_000..]);
    let mut s = p
        .guest
        .open_media(HOST, get(FILE_KEY, 10, Some(10)))
        .await
        .unwrap();
    assert_eq!(s.total, Some(0));
    assert!(read_all(&mut s).await.is_empty());
    // Bytes the server holds in memory.
    let env = format!("env:{FILE_KEY}");
    let mut s = p
        .guest
        .open_media(HOST, get(&env, 100, None))
        .await
        .unwrap();
    assert_eq!(read_all(&mut s).await, &counting(5_000)[100..]);
    // Nothing of that name.
    assert!(
        p.guest
            .open_media(HOST, get(OTHER_KEY, 0, None))
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cut_is_lost_on_both_sides_then_the_guest_comes_back_as_the_same_peer() {
    let (host, hrx, guest, grx, l) = behind_a_link(Duration::from_secs(2), quick()).await;
    guest.send(HOST, line("{\"hello\":1}")).unwrap();
    let peer = next(&hrx, "joined", joined).await;
    l.cut();
    next(&grx, "lost", |e| {
        (e == TransportEvent::Lost { peer: HOST }).then_some(())
    })
    .await;
    next(&hrx, "lost", |e| {
        (e == TransportEvent::Lost { peer }).then_some(())
    })
    .await;
    assert!(
        guest.send(HOST, line("{\"x\":1}")).is_err(),
        "gone meanwhile"
    );
    l.restore();
    next(&grx, "joined again", |e| {
        (e == TransportEvent::Joined { peer: HOST }).then_some(())
    })
    .await;
    guest.send(HOST, line("{\"hello\":2}")).unwrap();
    assert_eq!(
        next(&hrx, "joined again", joined).await,
        peer,
        "the same peer"
    );
    let msg = next(&hrx, "its hello", |e| match e {
        TransportEvent::Message { from, msg, .. } if from == peer => Some(msg),
        _ => None,
    })
    .await;
    assert_eq!(msg, line("{\"hello\":2}"));
    // A guest with another key is another peer.
    let to = iroh::EndpointAddr::new(host.id()).with_ip_addr(l.addr);
    let gep = endpoint_with(
        false,
        false,
        Some(key(2)),
        net::transport_config(Duration::from_secs(2), true),
    )
    .await;
    let other = IrohTransport::guest(gep, to, clock(), quick())
        .await
        .unwrap();
    other.send(HOST, line("{\"hello\":3}")).unwrap();
    assert_ne!(next(&hrx, "another joined", joined).await, peer);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_gives_up_dialling_after_its_time() {
    let timing = Timing {
        redial_for: Duration::from_secs(3),
        ..quick()
    };
    let (_host, _hrx, guest, grx, l) = behind_a_link(Duration::from_secs(1), timing).await;
    guest.send(HOST, line("{\"hello\":1}")).unwrap();
    l.cut();
    next(&grx, "lost", |e| {
        (e == TransportEvent::Lost { peer: HOST }).then_some(())
    })
    .await;
    until("given up", || !guest.dialling()).await;
    l.restore();
    assert!(
        !any_within(&grx, Duration::from_secs(3), |e| *e
            == TransportEvent::Joined { peer: HOST })
        .await,
        "not back after giving up"
    );
}
