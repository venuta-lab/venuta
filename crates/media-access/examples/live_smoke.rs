//! Opt-in live catalog smoke:
//! `cargo run -p media-access --example live_smoke --features live-provider -- "query"`
//!
//! Prints search hits and attempts resolve. Never prints full stream URLs.
//! Set `VENUTA_LIVE_SMOKE_DIAG=1` for per-client player/media probes.

use innertube_rs::endpoints::player::resolve_stream_url_full;
use innertube_rs::models::video::GetVideoInfoOptions;
use innertube_rs::Innertube;
use media_access::{CancellationToken, LiveCatalogAccess, MediaAccess, PageRequest};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "blue train".into());
    let access = match LiveCatalogAccess::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("init failed: {e}");
            std::process::exit(1);
        }
    };
    let cancel = CancellationToken::default();
    let page = match access.search_tracks(&query, &PageRequest::new(5, None), &cancel) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("search failed: {e}");
            std::process::exit(2);
        }
    };
    if page.items.is_empty() {
        eprintln!("no tracks for {query:?}");
        std::process::exit(3);
    }
    for (i, t) in page.items.iter().enumerate() {
        println!(
            "{:>2}. {} — {} [{}]",
            i + 1,
            t.title,
            t.artists.join(", "),
            t.track_id
        );
    }
    let first = &page.items[0];

    if std::env::var("VENUTA_LIVE_SMOKE_DIAG").ok().as_deref() == Some("1") {
        if let Err(e) = run_diag(&first.track_id).await {
            eprintln!("diag failed: {e}");
        }
    }

    match access.resolve_audio(&first.track_id, &cancel) {
        Ok(audio) => {
            println!("resolve ok for {}", first.track_id);
            println!("debug={}", format!("{audio:?}"));
            let has_http = audio.url.starts_with("http://") || audio.url.starts_with("https://");
            println!("url_is_http={has_http}");
            println!("mime={:?}", audio.mime_type);
            println!("url_has_pot={}", audio.url.contains("pot="));
            println!("header_count={}", audio.headers.len());
            for mode in ["plain", "range"] {
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(15)))
                    .http_status_as_error(false)
                    .build()
                    .into();
                let mut req = agent.get(&audio.url);
                for h in &audio.headers {
                    req = req.header(&h.name, &h.value);
                }
                if mode == "range" {
                    req = req.header("Range", "bytes=0-2047");
                }
match req.call() {
                    Ok(mut resp) => {
                        let status = resp.status().as_u16();
                        let len = resp
                            .body_mut()
                            .read_to_vec()
                            .map(|b| b.len())
                            .unwrap_or(0);
                        println!("media_http_{mode}={status} bytes_read={len}");
                    }
                    Err(e) => println!("media_transport_error_{mode}={e}"),
                }
            }
        }
        Err(e) => {
            eprintln!("resolve failed: {e}");
            std::process::exit(4);
        }
    }
}

async fn run_diag(video_id: &str) -> Result<(), String> {
    let client = Innertube::new()
        .await
        .map_err(|e| format!("innertube init: {e}"))?;
    let visitor = client
        .session
        .context
        .client
        .visitor_data
        .clone()
        .unwrap_or_default();
    println!(
        "diag visitor_present={} visitor_len={}",
        !visitor.is_empty(),
        visitor.len()
    );

    let pot = fetch_pot(video_id);
    println!("diag pot_video_len={}", pot.as_ref().map(|s| s.len()).unwrap_or(0));
    let pot_v = if visitor.is_empty() {
        None
    } else {
        fetch_pot(&visitor)
    };
    println!(
        "diag pot_visitor_len={}",
        pot_v.as_ref().map(|s| s.len()).unwrap_or(0)
    );

    let clients = [
        ("ANDROID_VR", None),
        ("IOS", None),
        ("ANDROID", None),
        ("TVHTML5", None),
        ("WEB_EMBEDDED", None),
        ("WEB", pot_v.as_deref().or(pot.as_deref())),
        ("YTMUSIC", pot_v.as_deref().or(pot.as_deref())),
    ];

    for (name, pot_token) in clients {
        let options = GetVideoInfoOptions {
            client: Some(name.to_string()),
            po_token: pot_token.map(str::to_owned),
            playback_context: None,
        };
        match client.get_basic_info(video_id, Some(&options)).await {
            Ok(info) => {
                let status = info.player_response.playability_status.status.clone();
                let sd = info.player_response.streaming_data.as_ref();
                let adaptive = sd.map(|s| s.adaptive_formats.len()).unwrap_or(0);
                let progressive = sd.map(|s| s.formats.len()).unwrap_or(0);
                let hls = sd
                    .and_then(|s| s.hls_manifest_url.as_ref())
                    .is_some();
                println!(
                    "diag client={name} playability={status} adaptive={adaptive} progressive={progressive} hls={hls} pot={}",
                    pot_token.is_some()
                );
                if let Some(sd) = sd {
                    let format = sd
                        .adaptive_formats
                        .iter()
                        .chain(sd.formats.iter())
                        .filter(|f| f.is_audio_only())
                        .max_by_key(|f| f.bitrate);
                    if let Some(format) = format {
                        match resolve_stream_url_full(
                            format,
                            &client.player.decipherer,
                            pot_token,
                            None,
                        ) {
                            Ok(url) => {
                                let has_pot = url.contains("pot=");
                                let host = url
                                    .split('/')
                                    .nth(2)
                                    .unwrap_or("?")
                                    .to_string();
                                // Session client GET (cookies/UA preserved).
                                let sess = client
                                    .session
                                    .http_client
                                    .get(&url)
                                    .header("Range", "bytes=0-2047")
                                    .send()
                                    .await;
                                match sess {
                                    Ok(resp) => {
                                        let code = resp.status().as_u16();
                                        let bytes = resp.bytes().await.map(|b| b.len()).unwrap_or(0);
                                        println!(
                                            "diag client={name} itag={} mime={} host={host} has_pot={has_pot} session_http={code} bytes={bytes}",
                                            format.itag,
                                            format.mime_type
                                        );
                                    }
                                    Err(e) => println!(
                                        "diag client={name} session_transport={e} has_pot={has_pot} host={host}"
                                    ),
                                }
                            }
                            Err(e) => println!("diag client={name} resolve_url_err={e}"),
                        }
                    } else {
                        println!("diag client={name} no_audio_format");
                    }
                }
            }
            Err(e) => println!("diag client={name} player_err={e}"),
        }
    }
    Ok(())
}

fn fetch_pot(binding: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();
    let body = serde_json::json!({ "content_binding": binding });
    let mut response = agent
        .post("http://127.0.0.1:4416/get_pot")
        .header("Content-Type", "application/json")
        .send_json(&body)
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: serde_json::Value = response.body_mut().read_json().ok()?;
    value
        .get("poToken")
        .or_else(|| value.get("po_token"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .filter(|t| !t.trim().is_empty())
}
