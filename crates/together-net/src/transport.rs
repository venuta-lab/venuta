//! The real `Transport` (contract 5.6, design 4.1): iroh QUIC, one connection per guest, guest
//! to host.
//!
//! - Control: the first bidirectional stream, opened by the guest, whose first line is the
//!   session's own (its hello: no preface, so a host of another version still reads it). JSON
//!   lines checked for length as they are read; priority 100 both ways.
//! - Clock: QUIC datagrams of 29 bytes, stamped with the `MonoClock` as they are read. A side
//!   that can't send datagrams on its connection (none, or refused by the other side) sends the
//!   same frames on the clock stream, which the guest opens on every connection. The host passes
//!   on at most 50 a second from one peer.
//! - Media: a stream per request, a `Get` line (on the wire `to` is inclusive), then a
//!   `{"size","mime"}` header and the bytes; the host serves them from its `MediaServer`, a
//!   song's file read from disk and paced (`pace::Downloads`).
//! - Peers: the host keeps one `PeerId` per remote key for the whole session, so a guest back
//!   with its per-room key is the same peer. A connection closed (the 15 s idle timeout, or
//!   closed by either side) is told `Lost`; the guest then dials again for a while and tells
//!   `Joined` when it is back. A change of the selected path is told `PathChanged`.

use crate::host::Get as WireGet;
use crate::net::{self, Lines};
use crate::pace::Downloads;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use n0_future::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use together::time::MonoClock;
use together::transport::{
    BoxFuture, ControlMsg, DATAGRAM_LEN, Get, Gone, MediaRead, MediaServer, MediaStream, PathInfo,
    PeerId, Transport, TransportEvent,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

/// How a guest's transport knows the host.
pub const HOST: PeerId = PeerId(0);

/// The line that opens a clock stream.
const CLOCK_STREAM: &str = "clock:";
/// Clock datagrams a host passes on from one peer: a burst of this many, then this many a second.
const DATAGRAMS_PER_S: f64 = 50.0;
/// How long one dial may take.
const DIAL: Duration = Duration::from_secs(25);
/// Media chunks are read in this size.
const CHUNK: usize = 64 * 1024;

/// How a guest dials again after its connection is lost.
#[derive(Debug, Clone)]
pub struct Timing {
    /// The first three waits.
    pub redial_first: Duration,
    /// Each wait after grows by half the first, up to this.
    pub redial_most: Duration,
    /// Dialling stops this long after the loss.
    pub redial_for: Duration,
}

impl Default for Timing {
    /// Every 2 s, then 3, 4 and 5 s apart, for 2 minutes (design 4.1, D17).
    fn default() -> Self {
        Timing {
            redial_first: Duration::from_secs(2),
            redial_most: Duration::from_secs(5),
            redial_for: Duration::from_secs(120),
        }
    }
}

impl Timing {
    /// The waits before each dial, in order (endless; the caller stops at `redial_for`).
    pub fn redial_waits(&self) -> impl Iterator<Item = Duration> + '_ {
        (0u32..).map(|k| {
            if k < 3 {
                self.redial_first
            } else {
                (self.redial_first + self.redial_first / 2 * (k - 2)).min(self.redial_most)
            }
        })
    }
}

/// One side of a room's connections.
#[derive(Clone)]
pub struct IrohTransport {
    inner: Arc<Inner>,
}

struct Inner {
    ep: Endpoint,
    clock: Arc<dyn MonoClock>,
    rt: Handle,
    events: Mutex<Option<Sender<TransportEvent>>>,
    peers: Mutex<HashMap<PeerId, Peer>>,
    /// The host's peers by their keys, for the whole session.
    keys: Mutex<HashMap<EndpointId, PeerId>>,
    next_peer: AtomicU64,
    server: Option<Arc<dyn MediaServer>>,
    downloads: Downloads,
    dialling: AtomicBool,
    closed: AtomicBool,
}

#[derive(Clone)]
struct Peer {
    conn: Connection,
    control: mpsc::UnboundedSender<Vec<u8>>,
    /// At the host: how many clock frames it passes on (datagrams and the clock stream).
    limit: Option<Arc<Mutex<Bucket>>>,
    /// The clock stream's writer, once there is one.
    clock: Arc<Mutex<Option<mpsc::UnboundedSender<[u8; DATAGRAM_LEN]>>>>,
}

impl IrohTransport {
    /// The host's side, on an endpoint made with `host: true`, serving media from `server`.
    /// Called on the async runtime its work runs on.
    pub fn host(ep: Endpoint, clock: Arc<dyn MonoClock>, server: Arc<dyn MediaServer>) -> Self {
        let t = IrohTransport::new(ep, clock, Some(server));
        let inner = t.inner.clone();
        t.inner.rt.spawn(async move {
            while let Some(incoming) = inner.ep.accept().await {
                let inner = inner.clone();
                inner.rt.clone().spawn(async move {
                    if let Ok(conn) = incoming.await {
                        inner.welcome(conn).await;
                    }
                });
            }
        });
        t
    }

    /// A guest's side: dial the host at `to` (the endpoint's key is who this guest is), once,
    /// then again after a loss as `timing` says. Called on the async runtime its work runs on.
    pub async fn guest(
        ep: Endpoint,
        to: EndpointAddr,
        clock: Arc<dyn MonoClock>,
        timing: Timing,
    ) -> Result<Self, String> {
        let t = IrohTransport::new(ep, clock, None);
        let conn = t.inner.dial(&to, DIAL).await?;
        t.inner.clone().attach(conn, to, timing).await?;
        Ok(t)
    }

    fn new(ep: Endpoint, clock: Arc<dyn MonoClock>, server: Option<Arc<dyn MediaServer>>) -> Self {
        IrohTransport {
            inner: Arc::new(Inner {
                ep,
                clock,
                rt: Handle::current(),
                events: Mutex::default(),
                peers: Mutex::default(),
                keys: Mutex::default(),
                next_peer: AtomicU64::new(1),
                server,
                downloads: Downloads::default(),
                dialling: AtomicBool::new(false),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// This side's key.
    pub fn id(&self) -> EndpointId {
        self.inner.ep.id()
    }

    /// How the connection to `peer` reaches it now.
    pub fn path(&self, peer: PeerId) -> Option<PathInfo> {
        let conn = self.inner.peers.lock().unwrap().get(&peer)?.conn.clone();
        selected(&conn).map(|(_, p)| p)
    }

    /// A guest dialling again after a loss.
    pub fn dialling(&self) -> bool {
        self.inner.dialling.load(Ordering::Relaxed)
    }

    /// Close everything: no more events, no dialling again.
    pub async fn close(&self) {
        self.inner.closed.store(true, Ordering::Relaxed);
        self.inner.ep.close().await;
    }
}

impl Transport for IrohTransport {
    fn send(&self, to: PeerId, msg: ControlMsg) -> Result<(), Gone> {
        let peers = self.inner.peers.lock().unwrap();
        let p = peers.get(&to).ok_or(Gone)?;
        p.control.send(msg.0).map_err(|_| Gone)
    }

    fn send_datagram(&self, to: PeerId, bytes: [u8; DATAGRAM_LEN]) -> Result<(), Gone> {
        let p = self
            .inner
            .peers
            .lock()
            .unwrap()
            .get(&to)
            .cloned()
            .ok_or(Gone)?;
        // A datagram when the connection takes them both ways (one side may have them off,
        // which the other learns only as its send is refused), else the clock stream.
        if p.conn
            .max_datagram_size()
            .is_some_and(|m| m >= DATAGRAM_LEN)
            && p.conn
                .send_datagram(bytes::Bytes::copy_from_slice(&bytes))
                .is_ok()
        {
            return Ok(());
        }
        let writer = p.clock.lock().unwrap().clone().ok_or(Gone)?;
        writer.send(bytes).map_err(|_| Gone)
    }

    fn open_media(&self, to: PeerId, get: Get) -> BoxFuture<'static, Result<MediaStream, Gone>> {
        let conn = self
            .inner
            .peers
            .lock()
            .unwrap()
            .get(&to)
            .map(|p| p.conn.clone());
        Box::pin(async move { open_media(conn.ok_or(Gone)?, get).await })
    }

    fn events(&self) -> Receiver<TransportEvent> {
        let (tx, rx) = channel();
        *self.inner.events.lock().unwrap() = Some(tx);
        rx
    }
}

impl Inner {
    fn emit(&self, e: TransportEvent) {
        if self.closed.load(Ordering::Relaxed) {
            return;
        }
        if let Some(tx) = self.events.lock().unwrap().as_ref() {
            let _ = tx.send(e);
        }
    }

    fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    async fn dial(&self, to: &EndpointAddr, limit: Duration) -> Result<Connection, String> {
        tokio::time::timeout(limit, self.ep.connect(to.clone(), net::ALPN))
            .await
            .map_err(|_| "no answer".to_owned())?
            .map_err(|e| e.to_string())
    }

    /// The host's side of a new connection: its peer, then its control stream.
    async fn welcome(self: Arc<Self>, conn: Connection) {
        let peer = *self
            .keys
            .lock()
            .unwrap()
            .entry(conn.remote_id())
            .or_insert_with(|| PeerId(self.next_peer.fetch_add(1, Ordering::Relaxed)));
        let Ok((send, recv)) = conn.accept_bi().await else {
            return;
        };
        let p = self.peer_of(conn.clone(), send);
        self.peers.lock().unwrap().insert(peer, p.clone());
        self.emit(TransportEvent::Joined { peer });
        self.watch(peer, conn.clone(), recv, p);
        // Its other streams: the clock stream, and media asked for.
        let inner = self.clone();
        self.rt.spawn(async move {
            while let Ok((send, recv)) = conn.accept_bi().await {
                let inner = inner.clone();
                inner
                    .rt
                    .clone()
                    .spawn(async move { inner.stream(peer, send, recv).await });
            }
        });
    }

    /// A guest's side of a connection to the host: its control and clock streams.
    fn attach(
        self: Arc<Self>,
        conn: Connection,
        to: EndpointAddr,
        timing: Timing,
    ) -> BoxFuture<'static, Result<(), String>> {
        Box::pin(self.attach_now(conn, to, timing))
    }

    async fn attach_now(
        self: Arc<Self>,
        conn: Connection,
        to: EndpointAddr,
        timing: Timing,
    ) -> Result<(), String> {
        let (send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        let p = self.peer_of(conn.clone(), send);
        // The clock stream, for when a side has no datagrams.
        let (mut csend, crecv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        let opening = WireGet {
            get: CLOCK_STREAM.into(),
            from: 0,
            to: None,
        };
        net::send_line(&mut csend, &together::proto::line(&opening)).await?;
        *p.clock.lock().unwrap() = Some(self.clock_writer(csend));
        self.clone().clock_reader(HOST, crecv, None);
        self.peers.lock().unwrap().insert(HOST, p.clone());
        self.watch(HOST, conn.clone(), recv, p);
        // Lost: dial again.
        let inner = self.clone();
        self.rt.spawn(async move {
            conn.closed().await;
            if inner.closed.load(Ordering::Relaxed) {
                return;
            }
            inner.redial(to, timing).await;
        });
        Ok(())
    }

    /// Dial the host again, as `timing` says, until it answers or the time is up.
    async fn redial(self: Arc<Self>, to: EndpointAddr, timing: Timing) {
        self.dialling.store(true, Ordering::Relaxed);
        let lost = Instant::now();
        for wait in timing.redial_waits() {
            if lost.elapsed() + wait >= timing.redial_for {
                break;
            }
            tokio::time::sleep(wait).await;
            if self.closed.load(Ordering::Relaxed) {
                break;
            }
            let left = timing.redial_for.saturating_sub(lost.elapsed());
            if let Ok(conn) = self.dial(&to, left.min(DIAL)).await
                && self
                    .clone()
                    .attach(conn, to.clone(), timing.clone())
                    .await
                    .is_ok()
            {
                self.dialling.store(false, Ordering::Relaxed);
                self.emit(TransportEvent::Joined { peer: HOST });
                return;
            }
        }
        self.dialling.store(false, Ordering::Relaxed);
    }

    /// A peer's control writer (everything to it goes through one task, ahead of media).
    fn peer_of(&self, conn: Connection, mut send: SendStream) -> Peer {
        let _ = send.set_priority(net::priority::CONTROL);
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        self.rt.spawn(async move {
            while let Some(line) = rx.recv().await {
                if net::send_line(&mut send, &line).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        Peer {
            conn,
            control: tx,
            clock: Arc::default(),
            limit: self
                .server
                .is_some()
                .then(|| Arc::new(Mutex::new(Bucket::new()))),
        }
    }

    /// Read a connection's control lines and datagrams, watch its paths, and tell when it ends.
    fn watch(self: &Arc<Self>, peer: PeerId, conn: Connection, recv: RecvStream, p: Peer) {
        // Control lines.
        let inner = self.clone();
        let c = conn.clone();
        self.rt.spawn(async move {
            let mut lines = Lines::new(recv);
            loop {
                match lines.next().await {
                    // A line over the limit ends it (`Lines` lets one through that ends
                    // in the chunk that passed the limit).
                    Ok(Some(line)) if line.len() > together::proto::MAX_MESSAGE => {
                        c.close(4u32.into(), b"line");
                        break;
                    }
                    Ok(Some(mut line)) => {
                        let at_ns = inner.now_ns();
                        line.push(b'\n');
                        inner.emit(TransportEvent::Message {
                            from: peer,
                            msg: ControlMsg(line),
                            at_ns,
                        });
                    }
                    Ok(None) => break,
                    Err(_) => {
                        // Too long, or broken: the connection is over.
                        c.close(4u32.into(), b"line");
                        break;
                    }
                }
            }
        });
        // Datagrams.
        let inner = self.clone();
        let c = conn.clone();
        let limit = p.limit.clone();
        self.rt.spawn(async move {
            while let Ok(d) = c.read_datagram().await {
                let at_ns = inner.now_ns();
                inner.datagram(peer, &d, at_ns, limit.as_deref());
            }
        });
        // Paths.
        let inner = self.clone();
        let c = conn.clone();
        self.rt.spawn(async move {
            let mut last = None;
            let mut paths = c.paths_stream();
            while let Some(list) = paths.next().await {
                let now = list
                    .iter()
                    .find(|p| p.is_selected())
                    .map(|p| (p.remote_addr().clone(), info_of(&p)));
                if let Some(path) = changed(&mut last, now) {
                    inner.emit(TransportEvent::PathChanged { peer, path });
                }
            }
        });
        // The end.
        let inner = self.clone();
        self.rt.spawn(async move {
            conn.closed().await;
            let ours = {
                let mut peers = inner.peers.lock().unwrap();
                let ours = peers
                    .get(&peer)
                    .is_some_and(|q| q.conn.stable_id() == conn.stable_id());
                if ours {
                    peers.remove(&peer);
                }
                ours
            };
            if ours {
                inner.downloads.forget(peer.0);
                inner.emit(TransportEvent::Lost { peer });
            }
        });
    }

    fn datagram(&self, peer: PeerId, d: &[u8], at_ns: u64, limit: Option<&Mutex<Bucket>>) {
        let Ok(bytes) = <[u8; DATAGRAM_LEN]>::try_from(d) else {
            return;
        };
        if let Some(b) = limit
            && !b.lock().unwrap().take(Instant::now())
        {
            return;
        }
        self.emit(TransportEvent::Datagram {
            from: peer,
            bytes,
            at_ns,
        });
    }

    /// A clock stream's writer: frames of 29 bytes.
    fn clock_writer(&self, mut send: SendStream) -> mpsc::UnboundedSender<[u8; DATAGRAM_LEN]> {
        let _ = send.set_priority(net::priority::CONTROL);
        let (tx, mut rx) = mpsc::unbounded_channel::<[u8; DATAGRAM_LEN]>();
        self.rt.spawn(async move {
            while let Some(f) = rx.recv().await {
                if net::send_line(&mut send, &f).await.is_err() {
                    break;
                }
            }
        });
        tx
    }

    /// Read a clock stream's frames as datagrams.
    fn clock_reader(
        self: Arc<Self>,
        peer: PeerId,
        recv: impl tokio::io::AsyncRead + Unpin + Send + 'static,
        limit: Option<Arc<Mutex<Bucket>>>,
    ) {
        let inner = self.clone();
        self.rt.spawn(async move {
            let mut recv = recv;
            let mut f = [0u8; DATAGRAM_LEN];
            while recv.read_exact(&mut f).await.is_ok() {
                let at_ns = inner.now_ns();
                inner.datagram(peer, &f, at_ns, limit.as_deref());
            }
        });
    }

    /// A stream a guest opened at the host: the clock stream, or media asked for.
    async fn stream(self: Arc<Self>, peer: PeerId, send: SendStream, recv: RecvStream) {
        let mut lines = Lines::new(recv);
        let Ok(Some(l)) = lines.next().await else {
            return;
        };
        let Ok(req) = serde_json::from_slice::<WireGet>(&l) else {
            return;
        };
        if req.get == CLOCK_STREAM {
            let writer = self.clock_writer(send);
            let limit = self.peers.lock().unwrap().get(&peer).and_then(|p| {
                *p.clock.lock().unwrap() = Some(writer);
                p.limit.clone()
            });
            self.clone().clock_reader(peer, lines.into_inner(), limit);
            return;
        }
        let _ = send.set_priority(priority_of(&req.get));
        let get = Get {
            get: req.get.clone(),
            from: req.from,
            to: req.to.map(|t| t.saturating_add(1)),
        };
        self.serve(peer, send, &get).await;
    }

    /// Serve one media request: the header, then the range, a song's file paced.
    async fn serve(&self, peer: PeerId, mut send: SendStream, get: &Get) {
        let Some(server) = &self.server else { return };
        let info = server.describe(peer, get);
        let mime = info
            .as_ref()
            .and_then(|i| i.mime.clone())
            .unwrap_or_else(|| "application/octet-stream".into());
        let duration_ms = info.as_ref().and_then(|i| i.duration_ms);
        enum Body {
            File(tokio::fs::File),
            Bytes(Arc<[u8]>),
        }
        let (body, size) = match info.and_then(|i| i.file) {
            Some(path) => match tokio::fs::File::open(&path).await {
                Ok(f) => {
                    let size = f.metadata().await.map(|m| m.len()).unwrap_or(0);
                    (Body::File(f), size)
                }
                Err(_) => {
                    let _ = net::send_line(&mut send, b"{\"error\":\"not there\"}\n").await;
                    let _ = send.finish();
                    return;
                }
            },
            None => match server.open(peer, get) {
                Some(b) => {
                    let size = b.len() as u64;
                    (Body::Bytes(b), size)
                }
                None => {
                    let _ = net::send_line(&mut send, b"{\"error\":\"not here\"}\n").await;
                    let _ = send.finish();
                    return;
                }
            },
        };
        let from = get.from.min(size);
        let end = get.to.unwrap_or(size).clamp(from, size);
        let head = serde_json::json!({ "size": size, "mime": mime });
        if net::send_line(&mut send, &together::proto::line(&head))
            .await
            .is_err()
        {
            return;
        }
        let paced = self
            .downloads
            .begin(peer.0, &get.get, from, size, duration_ms, Instant::now());
        let mut buf = vec![0u8; paced.unwrap_or(CHUNK)];
        let mut at = from;
        let mut body = body;
        if let Body::File(f) = &mut body
            && f.seek(std::io::SeekFrom::Start(from)).await.is_err()
        {
            return;
        }
        while at < end {
            let want = ((end - at) as usize).min(buf.len());
            let n = match &mut body {
                Body::File(f) => match f.read(&mut buf[..want]).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                },
                Body::Bytes(b) => {
                    buf[..want].copy_from_slice(&b[at as usize..at as usize + want]);
                    want
                }
            };
            if paced.is_some() {
                let wait = self
                    .downloads
                    .take(peer.0, &get.get, n, at + n as u64, Instant::now());
                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                }
            }
            if net::send_line(&mut send, &buf[..n]).await.is_err() {
                return;
            }
            at += n as u64;
        }
        let _ = send.finish();
    }
}

/// Ask the host for media on a stream of its own.
async fn open_media(conn: Connection, get: Get) -> Result<MediaStream, Gone> {
    if get.to.is_some_and(|t| t <= get.from) {
        return Ok(MediaStream {
            total: Some(0),
            mime: None,
            body: Box::new(Empty),
        });
    }
    let (mut send, recv) = conn.open_bi().await.map_err(|_| Gone)?;
    let _ = send.set_priority(priority_of(&get.get));
    let ask = WireGet {
        get: get.get.clone(),
        from: get.from,
        to: get.to.map(|t| t - 1),
    };
    net::send_line(&mut send, &together::proto::line(&ask))
        .await
        .map_err(|_| Gone)?;
    let _ = send.finish();
    let mut lines = Lines::new(recv);
    let head = lines.next().await.map_err(|_| Gone)?.ok_or(Gone)?;
    let head: serde_json::Value = serde_json::from_slice(&head).map_err(|_| Gone)?;
    let size = head.get("size").and_then(|s| s.as_u64()).ok_or(Gone)?;
    let from = get.from.min(size);
    let end = get.to.unwrap_or(size).clamp(from, size);
    Ok(MediaStream {
        total: Some(end - from),
        mime: head.get("mime").and_then(|m| m.as_str()).map(str::to_owned),
        body: Box::new(Body {
            r: lines.into_inner(),
        }),
    })
}

/// A media stream's bytes as they come.
struct Body {
    r: BufReader<RecvStream>,
}

impl MediaRead for Body {
    fn read_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, Gone>> {
        Box::pin(async move {
            let mut buf = vec![0u8; CHUNK];
            match self.r.read(&mut buf).await {
                Ok(0) => Ok(None),
                Ok(n) => {
                    buf.truncate(n);
                    Ok(Some(buf))
                }
                Err(_) => Err(Gone),
            }
        })
    }
}

/// An empty range.
struct Empty;

impl MediaRead for Empty {
    fn read_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, Gone>> {
        Box::pin(async { Ok(None) })
    }
}

/// The priority of a media stream by what it carries.
fn priority_of(get: &str) -> i32 {
    if get.starts_with("cover:") {
        net::priority::COVER
    } else if get.starts_with("env:") {
        net::priority::ENVELOPE
    } else {
        net::priority::FILE
    }
}

/// Whether the selected path is another than the last one seen (the first one is no change;
/// a moment with none selected changes nothing): the new path's state when it is.
fn changed(
    last: &mut Option<iroh::TransportAddr>,
    now: Option<(iroh::TransportAddr, PathInfo)>,
) -> Option<PathInfo> {
    let (addr, info) = now?;
    let moved = last.as_ref().is_some_and(|a| *a != addr);
    *last = Some(addr);
    moved.then_some(info)
}

/// The selected path of a connection: its address and how it goes.
fn selected(conn: &Connection) -> Option<(iroh::TransportAddr, PathInfo)> {
    let paths = conn.paths();
    let p = paths.iter().find(|p| p.is_selected())?;
    Some((p.remote_addr().clone(), info_of(&p)))
}

fn info_of(p: &iroh::endpoint::Path<'_>) -> PathInfo {
    PathInfo {
        direct: p.is_ip(),
        relay: p.is_relay(),
        rtt_us: u32::try_from(p.rtt().as_micros()).ok(),
    }
}

/// At most `DATAGRAMS_PER_S` a second, a burst of as many.
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new() -> Self {
        Bucket {
            tokens: DATAGRAMS_PER_S,
            last: Instant::now(),
        }
    }
    fn take(&mut self, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * DATAGRAMS_PER_S).min(DATAGRAMS_PER_S);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guest_dials_again_every_two_seconds_then_further_apart_up_to_five() {
        let t = Timing::default();
        let w: Vec<u64> = t.redial_waits().take(8).map(|d| d.as_secs()).collect();
        assert_eq!(w, [2, 2, 2, 3, 4, 5, 5, 5]);
        // Over two minutes that is 26 tries.
        let mut sum = Duration::ZERO;
        let tries = t
            .redial_waits()
            .take_while(|w| {
                sum += *w;
                sum < t.redial_for
            })
            .count();
        assert_eq!(tries, 26);
    }

    #[test]
    fn the_host_takes_a_burst_of_fifty_datagrams_then_fifty_a_second() {
        let t0 = Instant::now();
        let mut b = Bucket {
            tokens: DATAGRAMS_PER_S,
            last: t0,
        };
        assert_eq!((0..60).filter(|_| b.take(t0)).count(), 50);
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!((0..60).filter(|_| b.take(t1)).count(), 5);
    }

    #[test]
    fn media_streams_go_by_what_they_carry() {
        assert_eq!(priority_of("host:abc"), net::priority::FILE);
        assert_eq!(priority_of("cover:host:abc"), net::priority::COVER);
        assert_eq!(priority_of("env:host:abc"), net::priority::ENVELOPE);
    }

    #[test]
    fn a_change_of_path_is_another_selected_address() {
        let ip = |p: u16| iroh::TransportAddr::Ip(std::net::SocketAddr::from(([10, 0, 0, 1], p)));
        let info = |direct| PathInfo {
            direct,
            relay: !direct,
            rtt_us: Some(1_000),
        };
        let mut last = None;
        assert_eq!(
            changed(&mut last, Some((ip(1), info(true)))),
            None,
            "the first"
        );
        assert_eq!(
            changed(&mut last, Some((ip(1), info(true)))),
            None,
            "the same"
        );
        assert_eq!(changed(&mut last, None), None, "none selected a moment");
        assert_eq!(
            changed(&mut last, Some((ip(2), info(true)))),
            Some(info(true)),
            "another address"
        );
        let relay = iroh::TransportAddr::Relay("https://relay.example".parse().unwrap());
        assert_eq!(
            changed(&mut last, Some((relay, info(false)))),
            Some(info(false)),
            "to the relay"
        );
        assert_eq!(
            changed(&mut last, Some((ip(2), info(true)))),
            Some(info(true)),
            "and back"
        );
    }
}
