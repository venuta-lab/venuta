//! Every playlist of a real library written out and read back in: the same songs, in the same
//! order? Not run by default: it needs a library. Reads the library and its playlists; writes
//! only into a temporary folder.
//!
//! VENUTA_HOME=D:\copy\of\library cargo test --release -p engine --test round_trip -- --ignored --nocapture
//!
//! Today Venuta writes one format (M3U8, songs on disk only); each new writer joins the list
//! here and must bring every song back, in order, including songs you don't have yet.

use engine::library::Library;
use engine::{Index, imports, paths, playlists};
use std::path::PathBuf;

#[test]
#[ignore]
fn playlists_come_back_whole() {
    let home = PathBuf::from(std::env::var_os("VENUTA_HOME").expect("set VENUTA_HOME to a copy"));
    let root = paths::music_dir(&home);
    let index_file = paths::index_path_for(&root);
    assert!(
        index_file.exists(),
        "no index for {}: open the library in Venuta first",
        root.display()
    );
    let index = Index::open(&index_file, &paths::canonical_root_string(&root)).unwrap();
    let lib = Library::load(&index).unwrap();
    let out = tempfile::tempdir().unwrap();

    let (mut songs, mut written, mut back, mut wrong, mut lost) = (0, 0, 0, 0, 0);
    for p in playlists::list(&root, &lib) {
        let Ok((entries, _)) = playlists::read(&root, &lib, &p.name) else {
            continue;
        };
        let want: Vec<&str> = entries
            .iter()
            .filter(|e| !e.missing)
            .map(|e| e.path.as_str())
            .collect();
        let file = out.path().join("list.m3u8");
        let n = playlists::export(&root, &lib, &p.name, &file).unwrap();
        if n == 0 {
            // Nothing on disk: today's file comes out empty and can't be read back.
            eprintln!(
                "  {:<40} {:>5} songs, none on disk: an empty file",
                p.name,
                entries.len()
            );
            songs += entries.len();
            lost += entries.len();
            continue;
        }
        let read = imports::read_file(&file).unwrap();
        let preview = imports::preview(&root, &lib, &read);
        let got: Vec<&str> = preview
            .iter()
            .flat_map(|l| l.items.iter())
            .filter_map(|i| i.found.as_ref().map(|c| c.path.as_str()))
            .collect();
        let same = want.iter().zip(&got).filter(|(a, b)| a == b).count();
        songs += entries.len();
        written += n;
        back += same;
        wrong += got.len() - same;
        lost += entries.len() - n;
        if same != entries.len() {
            eprintln!(
                "  {:<40} {:>5} songs, {:>5} written, {:>5} back in place, {} left out (not on disk)",
                p.name,
                entries.len(),
                n,
                same,
                entries.len() - n
            );
        }
    }
    eprintln!(
        "\nM3U8: {songs} songs in your playlists, {written} written, {back} came back in place, \
         {wrong} came back different, {lost} left out (songs you don't have, online songs)"
    );
    assert_eq!(
        wrong, 0,
        "a song came back as a different file or out of order"
    );
}
