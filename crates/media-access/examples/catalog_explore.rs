//! Opt-in live look at the browse side of the catalog (network, nothing played):
//! `cargo run -p media-access --example catalog_explore --features live-provider -- "words"`
//!
//! Suggestions for the words, its playlists, podcasts and videos, and the moods and genres.

use media_access::{
    CancellationToken, LiveCatalogAccess, MediaAccess, PageRequest, RemoteSuggestion,
};
use std::time::Instant;

fn main() {
    let q = std::env::args().nth(1).unwrap_or_else(|| "arijit".into());
    let access = LiveCatalogAccess::new().expect("catalog");
    let cancel = CancellationToken::default();
    let page = PageRequest::new(5, None);

    let t = Instant::now();
    match access.suggest(&q, &cancel) {
        Ok(s) => {
            println!("suggestions in {} ms", t.elapsed().as_millis());
            for item in &s.items {
                match item {
                    RemoteSuggestion::Song { track } => {
                        println!("  song   {} | {}", track.title, track.artists.join(", "))
                    }
                    RemoteSuggestion::Album { album, subtitle } => {
                        println!(
                            "  album  {} | {}",
                            album.title,
                            subtitle.as_deref().unwrap_or("")
                        )
                    }
                    RemoteSuggestion::Artist { artist, subtitle } => println!(
                        "  artist {} | {} | {:?}",
                        artist.name,
                        subtitle.as_deref().unwrap_or(""),
                        artist.audience
                    ),
                    RemoteSuggestion::Playlist { collection } => println!(
                        "  list   {} | {}",
                        collection.title,
                        collection.subtitle.as_deref().unwrap_or("")
                    ),
                }
            }
            for text in &s.queries {
                println!("  query  {text}");
            }
        }
        Err(e) => println!("suggestions failed: {e}"),
    }
    for kind in ["playlists", "podcasts"] {
        let t = Instant::now();
        match access.search_collections(&q, kind, &page, &cancel) {
            Ok(p) => {
                println!("{kind} in {} ms", t.elapsed().as_millis());
                for c in &p.items {
                    println!(
                        "  {} | {} | {}",
                        c.title,
                        c.subtitle.as_deref().unwrap_or(""),
                        c.collection_id
                    );
                }
            }
            Err(e) => println!("{kind} failed: {e}"),
        }
    }
    let t = Instant::now();
    match access.search_videos(&q, &page, &cancel) {
        Ok(p) => {
            println!("videos in {} ms", t.elapsed().as_millis());
            for v in &p.items {
                println!(
                    "  {} | {} | {:?}",
                    v.title,
                    v.artists.join(", "),
                    v.duration_ms
                );
            }
        }
        Err(e) => println!("videos failed: {e}"),
    }
    let t = Instant::now();
    match access.moods(&cancel) {
        Ok(sections) => {
            println!("moods in {} ms", t.elapsed().as_millis());
            for s in &sections {
                let names: Vec<_> = s.tiles.iter().map(|m| m.title.as_str()).collect();
                println!("  {}: {}", s.title, names.join(", "));
            }
            if let Some(first) = sections.first().and_then(|s| s.tiles.first()) {
                match access.mood_collections(&first.params, &cancel) {
                    Ok(lists) => {
                        println!(
                            "  {} has {} playlists, first: {:?}",
                            first.title,
                            lists.len(),
                            lists.first().map(|l| &l.title)
                        )
                    }
                    Err(e) => println!("  mood page failed: {e}"),
                }
            }
        }
        Err(e) => println!("moods failed: {e}"),
    }
}
