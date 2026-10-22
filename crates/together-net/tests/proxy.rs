//! The guest's player onto the host's files (design 4.1): an HTTP request on 127.0.0.1 is
//! asked of the host on a media stream, over real connections on this computer.

mod common;
use common::*;

use together::time::{MonoClock, RealClock};
use together::transport::{Get as MediaGet, MediaServer, PeerId, Transport};
use together_net::proxy::Proxy;
use together_net::transport::{HOST, IrohTransport, Timing};

const KEY: &str = "host:0123456789abcdef0123456789abcdef";

struct Shelf(Arc<[u8]>);

impl MediaServer for Shelf {
    fn open(&self, _: PeerId, get: &MediaGet) -> Option<Arc<[u8]>> {
        (get.get == KEY).then(|| self.0.clone())
    }
}

/// One request, raw: the answer's head and body.
async fn ask(port: u16, req: &str) -> (String, Vec<u8>) {
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut all = Vec::new();
    tcp.read_to_end(&mut all).await.unwrap();
    let at = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    (
        String::from_utf8_lossy(&all[..at]).to_string(),
        all[at..].to_vec(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_player_reads_the_hosts_file_by_ranges_through_the_transport() {
    let bytes: Arc<[u8]> = counting(300_000).into();
    let clock: Arc<dyn MonoClock> = Arc::new(RealClock);
    let hep = endpoint_on(false, true, None).await;
    let addr = hep.addr();
    let _host = IrohTransport::host(hep, clock.clone(), Arc::new(Shelf(bytes.clone())));
    let gep = endpoint_on(false, false, Some(key(1))).await;
    let guest = IrohTransport::guest(gep, addr, clock, Timing::default())
        .await
        .unwrap();
    let t: Arc<dyn Transport> = Arc::new(guest);
    let proxy = Proxy::start(t, HOST).await.unwrap();
    let (port, token) = proxy.address();

    let (head, body) = ask(
        port,
        &format!("GET /s/{KEY}?t={token} HTTP/1.1\r\nRange: bytes=1000-1999\r\n\r\n"),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert!(
        head.contains("Content-Range: bytes 1000-1999/300000"),
        "{head}"
    );
    assert_eq!(body, bytes[1000..2000].to_vec());

    let (head, body) = ask(
        port,
        &format!("GET /s/{KEY}?t={token} HTTP/1.1\r\nRange: bytes=299000-\r\n\r\n"),
    )
    .await;
    assert!(
        head.contains("Content-Range: bytes 299000-299999/300000"),
        "{head}"
    );
    assert_eq!(body, bytes[299_000..].to_vec());

    let (head, body) = ask(port, &format!("GET /s/{KEY}?t={token} HTTP/1.1\r\n\r\n")).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body.len(), 300_000);

    let (head, body) = ask(port, &format!("HEAD /s/{KEY}?t={token} HTTP/1.1\r\n\r\n")).await;
    assert!(head.contains("Content-Length: 300000"), "{head}");
    assert!(body.is_empty());

    let (head, _) = ask(
        port,
        &format!("GET /s/{KEY}?t={} HTTP/1.1\r\n\r\n", "0".repeat(32)),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 403"), "a wrong token: {head}");
    let other = "host:fedcba9876543210fedcba9876543210";
    let (head, _) = ask(port, &format!("GET /s/{other}?t={token} HTTP/1.1\r\n\r\n")).await;
    assert!(
        head.starts_with("HTTP/1.1 404"),
        "a song the host hasn't: {head}"
    );
}
