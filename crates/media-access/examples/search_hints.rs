//! Opt-in live check of search retrieval and its ranking hints (search only, no playback):
//! `cargo run -p media-access --example search_hints --features live-provider -- "wonderwall"`
//!
//! Prints each song with its kind of upload, plays, top-result mark and unfiltered place, then
//! albums (with year) and artists. Never prints addresses.

use media_access::{CancellationToken, LiveCatalogAccess, MediaAccess, PageRequest};

fn main() {
    let query = std::env::args().nth(1).unwrap_or_else(|| "kesariya".into());
    let access = LiveCatalogAccess::new().unwrap_or_else(|e| {
        eprintln!("init failed: {e}");
        std::process::exit(1);
    });
    let cancel = CancellationToken::default();
    match access.search_tracks(&query, &PageRequest::new(20, None), &cancel) {
        Ok(page) => {
            println!("songs for {query:?}: {}", page.items.len());
            for (i, t) in page.items.iter().enumerate() {
                println!(
                    "{:>2}. {:<4} {:>13} {}{:<5} {} — {} [{}]",
                    i + 1,
                    t.video_type.as_deref().unwrap_or("?"),
                    t.plays.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                    if t.is_top { "TOP " } else { "    " },
                    t.shelf_rank.map(|r| format!("#{r}")).unwrap_or_default(),
                    t.title,
                    t.artists.join(", "),
                    t.album.as_deref().unwrap_or("(no album)"),
                );
            }
            // The next page, as "More online songs" asks for it.
            if let Some(cursor) = page.next_cursor {
                match access.search_tracks(&query, &PageRequest::new(20, Some(cursor)), &cancel) {
                    Ok(more) => println!(
                        "next page: {} songs (first: {}), more after it: {}",
                        more.items.len(),
                        more.items.first().map(|t| t.title.as_str()).unwrap_or("-"),
                        more.next_cursor.is_some()
                    ),
                    Err(e) => eprintln!("next page failed: {e}"),
                }
            } else {
                println!("no next page");
            }
        }
        Err(e) => eprintln!("songs failed: {e}"),
    }
    match access.search_albums(&query, &PageRequest::new(5, None), &cancel) {
        Ok(page) => {
            for a in page.items {
                println!(
                    "album: {} — {} ({})",
                    a.title,
                    a.artists.join(", "),
                    a.release_date.as_deref().unwrap_or("?")
                );
            }
        }
        Err(e) => eprintln!("albums failed: {e}"),
    }
    match access.search_artists(&query, &PageRequest::new(5, None), &cancel) {
        Ok(page) => {
            for a in &page.items {
                println!("artist: {}", a.name);
            }
            // The first artist's page, as the online artist page opens it.
            if let Some(first) = page.items.first() {
                match access.artist_page(&first.artist_id, &cancel) {
                    Ok(p) => println!(
                        "artist page: {} ({:?} monthly), {} top songs, {} more songs, {} albums, {} singles, {} similar; first top song: {}",
                        p.name,
                        p.audience,
                        p.top_songs.len(),
                        p.songs.len(),
                        p.albums.len(),
                        p.singles.len(),
                        p.similar.len(),
                        p.top_songs.first().map(|t| t.title.as_str()).unwrap_or("-")
                    ),
                    Err(e) => eprintln!("artist page failed: {e}"),
                }
            }
        }
        Err(e) => eprintln!("artists failed: {e}"),
    }
}
