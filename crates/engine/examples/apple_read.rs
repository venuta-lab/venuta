//! cargo run --example apple_read -- <Library.musicdb> <key>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = engine::apple::read(std::path::Path::new(&a[1]), &a[2]).unwrap();
    println!(
        "tracks {} playlists {}",
        lib.tracks.len(),
        lib.playlists.len()
    );
    for p in &lib.playlists {
        println!("  {:30} {}", p.name, p.tracks.len());
    }
    let t = lib
        .tracks
        .values()
        .find(|t| t.title.contains("Kun Faya"))
        .cloned();
    println!("{t:?}");
}
