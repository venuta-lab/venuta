#![allow(dead_code)]
//! A small HTTP server on 127.0.0.1 for the update tests: each address answers from a list of
//! replies (the last one repeats), so a test can say "busy twice, then the file".

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub enum Reply {
    /// A status and a body.
    Body(u16, Vec<u8>),
    /// Accept, then never answer.
    Hang,
    /// Promise `total` bytes, send the first `send`, then go quiet.
    Stall { total: usize, send: Vec<u8> },
}

pub fn json(text: &str) -> Reply {
    Reply::Body(200, text.as_bytes().to_vec())
}

pub struct Server {
    pub base: String,
    hits: Arc<Mutex<Vec<String>>>,
    routes: Arc<Mutex<HashMap<String, Vec<Reply>>>>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
    /// Answer this address from now on (its full URL is known once the server listens).
    pub fn set(&self, path: &str, replies: Vec<Reply>) {
        self.routes.lock().unwrap().insert(path.to_owned(), replies);
    }
    /// How many times an address was asked.
    pub fn hits(&self, path: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }
}

pub fn serve(routes: Vec<(&str, Vec<Reply>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes: Arc<Mutex<HashMap<String, Vec<Reply>>>> = Arc::new(Mutex::new(
        routes.into_iter().map(|(p, r)| (p.to_owned(), r)).collect(),
    ));
    let hits = Arc::new(Mutex::new(Vec::new()));
    let (r, h) = (routes.clone(), hits.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (routes, hits) = (r.clone(), h.clone());
            std::thread::spawn(move || answer(stream, &routes, &hits));
        }
    });
    Server { base, hits, routes }
}

fn answer(
    mut s: TcpStream,
    routes: &Mutex<HashMap<String, Vec<Reply>>>,
    hits: &Mutex<Vec<String>>,
) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match s.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head);
    let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
    hits.lock().unwrap().push(path.clone());
    let reply = {
        let mut r = routes.lock().unwrap();
        match r.get_mut(&path) {
            Some(list) if list.len() > 1 => list.remove(0),
            Some(list) => list[0].clone(),
            None => Reply::Body(404, b"not found".to_vec()),
        }
    };
    match reply {
        Reply::Body(status, body) => {
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(&body);
        }
        Reply::Hang => std::thread::sleep(Duration::from_secs(120)),
        Reply::Stall { total, send } => {
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
            );
            let _ = s.write_all(&send);
            let _ = s.flush();
            std::thread::sleep(Duration::from_secs(120));
        }
    }
}

/// A throwaway signing key, written the way Tauri writes keys and signatures (base64 of
/// minisign's text). It signs in minisign's legacy mode ("Ed", the bytes as they are), which the
/// updater accepts like the prehashed mode the release uses.
pub struct TestKey {
    key: ed25519_dalek::SigningKey,
    id: [u8; 8],
}

#[allow(clippy::new_without_default)]
impl TestKey {
    pub fn new() -> Self {
        let mut seed = [0u8; 32];
        let mut id = [0u8; 8];
        getrandom::fill(&mut seed).unwrap();
        getrandom::fill(&mut id).unwrap();
        Self {
            key: ed25519_dalek::SigningKey::from_bytes(&seed),
            id,
        }
    }

    /// The `pubkey` for tauri.conf.json.
    pub fn public(&self) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut bin = b"Ed".to_vec();
        bin.extend(self.id);
        bin.extend(self.key.verifying_key().to_bytes());
        b64.encode(format!(
            "untrusted comment: minisign public key: TEST\n{}\n",
            b64.encode(bin)
        ))
    }

    /// The `signature` for latest.json, for this file and version.
    pub fn sign(&self, data: &[u8], file: &str, version: &str) -> String {
        use base64::Engine;
        use ed25519_dalek::Signer;
        let b64 = base64::engine::general_purpose::STANDARD;
        let sig = self.key.sign(data).to_bytes();
        let trusted = format!("timestamp:1792365701\tfile:{file}\tversion:{version}");
        let mut global_msg = sig.to_vec();
        global_msg.extend(trusted.as_bytes());
        let global = self.key.sign(&global_msg).to_bytes();
        let mut bin = b"Ed".to_vec();
        bin.extend(self.id);
        bin.extend(sig);
        b64.encode(format!(
            "untrusted comment: signature from tauri secret key\n{}\ntrusted comment: {trusted}\n{}\n",
            b64.encode(bin),
            b64.encode(global)
        ))
    }
}

/// A manifest as the release writes it, for one version and installer address.
pub fn manifest(version: &str, url: &str, signature: &str) -> String {
    serde_json::json!({
        "version": version,
        "notes": "What is new.",
        "pub_date": "2026-10-22T00:00:00.000Z",
        "platforms": {
            "windows-x86_64-nsis": { "signature": signature, "url": url },
            "windows-x86_64": { "signature": signature, "url": url },
        }
    })
    .to_string()
}
