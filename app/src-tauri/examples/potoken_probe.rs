//! A check of YouTube's security check, run by hand: makes PO tokens in the same hidden window
//! the app uses, signed out, with its browser data in a folder of its own. Nothing is sent as an
//! account and no play is told.
//!
//! ```text
//! cargo run -p app --example potoken_probe -- <empty folder> <binding>...
//! cargo run -p app --example potoken_probe -- <empty folder> --serve <seconds>
//! ```
//!
//! The first writes a token per binding (a video id, a visitor data) to `tokens.json` there. A
//! signed-out WEB_REMIX `/player` takes the video's as `serviceIntegrityDimensions.poToken`.
//!
//! `--serve` answers `POST http://127.0.0.1:4416/get_pot` (`{"content_binding": …}` →
//! `{"poToken": …}`) for that long, the local provider the stream resolver and the media-access
//! `live_smoke` diagnostics already ask: a stream address with the visitor's token as `pot` is
//! the check that googlevideo itself takes the token.

use app_lib::ytm_botguard::WebviewBotGuard;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("a folder for the browser data"));
    let rest: Vec<String> = args.collect();
    assert!(
        !rest.is_empty(),
        "a binding to make a token for, or --serve"
    );
    std::fs::create_dir_all(&dir).expect("the folder");
    tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime");
                let tokens = media_access::potoken::po_tokens();
                tokens.install(Arc::new(
                    WebviewBotGuard::new(handle.clone()).with_data_directory(dir.join("webview")),
                ));
                if rest[0] == "--serve" {
                    let secs = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(120);
                    serve(&runtime, Duration::from_secs(secs));
                } else {
                    runtime.block_on(write_tokens(&dir, &rest));
                }
                handle.exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the probe app");
}

async fn write_tokens(dir: &std::path::Path, bindings: &[String]) {
    let tokens = media_access::potoken::po_tokens();
    let mut made = serde_json::Map::new();
    for (i, binding) in bindings.iter().enumerate() {
        let started = Instant::now();
        match tokens.mint(binding).await {
            Ok(token) => {
                println!(
                    "token for binding {} made in {} ms ({} characters)",
                    i + 1,
                    started.elapsed().as_millis(),
                    token.len()
                );
                made.insert(binding.clone(), token.into());
            }
            Err(e) => println!("no token for binding {}: {e}", i + 1),
        }
    }
    let out = dir.join("tokens.json");
    std::fs::write(&out, serde_json::Value::Object(made).to_string()).expect("tokens.json");
    println!("written to {}", out.display());
}

/// The local provider's one call, for `for_how_long`.
fn serve(runtime: &tokio::runtime::Runtime, for_how_long: Duration) {
    let listener = std::net::TcpListener::bind("127.0.0.1:4416").expect("port 4416");
    listener.set_nonblocking(true).expect("nonblocking");
    let until = Instant::now() + for_how_long;
    println!("serving PO tokens on 127.0.0.1:4416");
    while Instant::now() < until {
        let Ok((stream, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let mut reader = BufReader::new(stream.try_clone().expect("stream"));
        let mut len = 0;
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 && line.trim() != "" {
            if let Some((k, v)) = line.split_once(':')
                && k.trim().eq_ignore_ascii_case("content-length")
            {
                len = v.trim().parse().unwrap_or(0);
            }
            line.clear();
        }
        let mut body = vec![0; len];
        let _ = reader.read_exact(&mut body);
        let binding = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["content_binding"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let started = Instant::now();
        let token = runtime.block_on(media_access::potoken::po_tokens().mint(&binding));
        let (status, reply) = match token {
            Ok(t) => {
                println!(
                    "minted for a {}-character binding in {} ms",
                    binding.len(),
                    started.elapsed().as_millis()
                );
                ("200 OK", serde_json::json!({ "poToken": t }).to_string())
            }
            Err(e) => {
                println!("no token: {e}");
                ("500 X", "{}".to_owned())
            }
        };
        let mut stream = stream;
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
    }
}
