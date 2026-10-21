//! Live check of what telling a play needs from the web players, signed out. NOT in the default
//! run, and nothing is told (no play or watch-time address is ever fetched):
//!   cargo test -p ytmusic --features integration-tests --test live_potoken -- --nocapture
#![cfg(feature = "integration-tests")]

use ytmusic::{Clients, InnerTube, Session, METADATA_CLIENT, WEB_PLAYS_CLIENT};

const VIDEO_ID: &str = "dQw4w9WgXcQ";

/// The current signature timestamp, off the signed-out home page, is what YouTube Music's
/// player (and the web client's) answers OK for with its tracking; without it both answer
/// UNPLAYABLE "Video unavailable", whatever the token. Measured 2026-10-19.
#[tokio::test]
async fn web_players_answer_with_the_current_signature_timestamp() {
    let sts = ytmusic::potoken::po_tokens()
        .signature_timestamp()
        .await
        .expect("the home page names the player's signature timestamp");
    println!("signature timestamp {sts}");
    let it = InnerTube::new(Session::default(), None).unwrap();
    it.set_visitor_data(Some(it.fetch_visitor_data().await.expect("visitor data")));
    let clients = Clients::bundled();
    for key in [METADATA_CLIENT, WEB_PLAYS_CLIENT] {
        let client = clients.get(key).unwrap();
        let without = it.player(client, VIDEO_ID, None, None, None).await.unwrap();
        let with = it
            .player(client, VIDEO_ID, None, Some(sts), None)
            .await
            .unwrap();
        let tracked = with
            .playback_tracking
            .as_ref()
            .and_then(|t| t.videostats_playback_url.as_ref())
            .is_some();
        println!(
            "{key}: without {} / with {} (tracking {tracked})",
            without.playability_status.status, with.playability_status.status
        );
        assert!(with.playability_status.is_ok(), "{key}");
        assert!(tracked, "{key} gives a play address");
    }
}
