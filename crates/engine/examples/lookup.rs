//! Live provider check: cargo run --example lookup -- <barcode>
use engine::providers::*;
fn main() {
    let upc = std::env::args().nth(1).expect("barcode");
    let http = engine::net::Http::open().unwrap();
    for (name, r) in [
        ("MusicBrainz", musicbrainz_by_barcode(&http, &upc)),
        ("Apple", apple_by_upc(&http, &upc)),
        ("Deezer", deezer_by_upc(&http, &upc)),
    ] {
        match r {
            Ok(Some(r)) => {
                println!(
                    "{name}: {} | artist {:?} | first {:?} date {:?} | label {:?} | genres {:?} | types {:?} | art {:?}",
                    r.title,
                    r.album_artist,
                    r.first_release,
                    r.release_date,
                    r.label,
                    r.genres,
                    r.release_types,
                    r.cover_px
                );
                if let Some(t) = r.tracks.first() {
                    println!(
                        "   track 1: {} isrc {:?} artists {:?} composers {:?} lyricists {:?}",
                        t.title, t.isrc, t.artists, t.composers, t.lyricists
                    );
                }
            }
            Ok(None) => println!("{name}: no match"),
            Err(e) => println!("{name}: error {e}"),
        }
    }
    match lrclib_find(
        &http,
        "Phir Se Ud Chala",
        "Mohit Chauhan",
        "Rockstar",
        271,
        preferred_script(Some("hin")),
    ) {
        Ok(Some(l)) => println!(
            "LRCLIB: id {} script {:?} first line {:?}",
            l.id,
            l.script,
            l.synced.lines().next()
        ),
        other => println!("LRCLIB: {other:?}"),
    }
}
