//! Opt-in live timings for the online catalog (network, no sound, no stream addresses printed):
//! `cargo run -p media-access --example catalog_timing --features live-provider -- "query" ...`
//!
//! For each query: the songs search (with the unfiltered one beside it), albums and artists
//! asked side by side, then resolving the first song, with which way found its address.

use media_access::{CancellationToken, LiveCatalogAccess, MediaAccess, PageRequest};
use std::time::Instant;

fn main() {
    let mut queries: Vec<String> = std::env::args().skip(1).collect();
    if queries.is_empty() {
        queries = vec![
            "arijit singh".into(),
            "lokah chapter 1".into(),
            "blue train".into(),
        ];
    }
    let started = Instant::now();
    let access = LiveCatalogAccess::new().expect("catalog");
    println!("client ready in {} ms", started.elapsed().as_millis());
    let cancel = CancellationToken::default();
    for q in &queries {
        let t = Instant::now();
        let (songs, albums, artists) = std::thread::scope(|s| {
            let songs = s.spawn(|| access.search_tracks(q, &PageRequest::new(20, None), &cancel));
            let albums = s.spawn(|| access.search_albums(q, &PageRequest::new(12, None), &cancel));
            let artists =
                s.spawn(|| access.search_artists(q, &PageRequest::new(12, None), &cancel));
            (
                songs.join().unwrap(),
                albums.join().unwrap(),
                artists.join().unwrap(),
            )
        });
        let search_ms = t.elapsed().as_millis();
        let songs = songs.map(|p| p.items).unwrap_or_default();
        println!(
            "{q:?}: search {search_ms} ms ({} songs, {} albums, {} artists)",
            songs.len(),
            albums.map(|p| p.items.len()).unwrap_or(0),
            artists.map(|p| p.items.len()).unwrap_or(0)
        );
        if let Some(first) = songs.first() {
            let t = Instant::now();
            match access.resolve_audio(&first.track_id, &cancel) {
                Ok(audio) => println!(
                    "  resolve {:?}: {} ms via {} ({})",
                    first.title,
                    t.elapsed().as_millis(),
                    audio.via,
                    audio.mime_type.as_deref().unwrap_or("?")
                ),
                Err(e) => println!("  resolve failed in {} ms: {e}", t.elapsed().as_millis()),
            }
        }
    }
}
