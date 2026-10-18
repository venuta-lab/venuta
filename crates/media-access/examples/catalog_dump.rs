//! Opt-in: save one catalog page's raw answer as a test fixture (network, nothing played):
//! `cargo run -p media-access --example catalog_dump --features live-provider -- <browse id> <out.json>`
//!
//! An artist (`UC…`), an album (`MPRE…`) or a playlist (`VL…`), in India English like the app.
//! Saved compact, without the tracking and logging fields no parser reads, so it stays small.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().expect("a browse id");
    let out = PathBuf::from(args.next().expect("where to save it"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let session = ytmusic::Session {
        locale: ytmusic::Locale {
            gl: "IN".into(),
            hl: "en".into(),
        },
        ..Default::default()
    };
    let it = ytmusic::InnerTube::new(session, None).expect("client");
    let clients = ytmusic::Clients::bundled();
    let client = clients.get("WEB_REMIX").expect("pages identity");
    let raw = runtime
        .block_on(it.browse_json(client, &id))
        .expect("the page");
    let mut raw = raw;
    prune(&mut raw);
    std::fs::write(&out, serde_json::to_string(&raw).expect("json")).expect("saved");
    println!(
        "saved {} bytes to {}",
        std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0),
        out.display()
    );
}

/// Fields of a catalog answer that only feed YouTube's own logging.
const NOISE: [&str; 11] = [
    "trackingParams",
    "clickTrackingParams",
    "loggingContext",
    "accessibility",
    "accessibilityData",
    "frameworkUpdates",
    "responseContext",
    "loggingDirectives",
    "serviceTrackingParams",
    "commandMetadata",
    "webCommandMetadata",
];

fn prune(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            map.retain(|k, _| !NOISE.contains(&k.as_str()));
            map.values_mut().for_each(prune);
        }
        serde_json::Value::Array(list) => list.iter_mut().for_each(prune),
        _ => {}
    }
}
