//! Every playlist of a real library written out in every format and read back in: the same
//! songs, in the same order? Not run by default: it needs a library. Reads the library and its
//! playlists; writes only into a temporary folder.
//!
//! VENUTA_HOME=D:\copy\of\library cargo test --release -p engine --test round_trip -- --ignored --nocapture
//!
//! A song comes back "in place" when the import finds the same file at the same place in the
//! list; a song you don't have, or an online one, when its title comes back at its place.

use engine::exports::{self, Format, Options, PathStyle};
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
    let lists: Vec<_> = playlists::list(&root, &lib)
        .into_iter()
        .filter_map(|p| {
            exports::songs_of(&root, &lib, &p.name)
                .ok()
                .map(|s| (p.name, s))
        })
        .filter(|(_, s)| !s.is_empty())
        .collect();
    let mut failed = false;
    for format in [
        Format::M3u8,
        Format::M3u,
        Format::Xspf,
        Format::Jspf,
        Format::Csv,
        Format::Txt,
        Format::Pls,
        Format::Wpl,
        Format::ItunesXml,
    ] {
        let opts = Options {
            format,
            style: PathStyle::ThisPc,
            include_missing: true,
        };
        let (mut songs, mut kept, mut back, mut wrong) = (0, 0, 0, 0);
        for (name, list) in &lists {
            let text = exports::render(name, list, &opts, &root, out.path());
            let file = out.path().join(format!("list.{}", format.extension()));
            exports::write(&file, &text).unwrap();
            // Formats that hold only files: the songs you have.
            let expect: Vec<_> = list
                .iter()
                .filter(|s| {
                    s.file.is_some()
                        || !format.has_paths()
                        || matches!(format, Format::M3u8 | Format::M3u | Format::Xspf)
                })
                .collect();
            songs += list.len();
            kept += expect.len();
            let read = match imports::read_file(&file) {
                Ok(r) => r,
                Err(_) if expect.is_empty() => continue,
                Err(e) => panic!("{format:?} {name}: {e}"),
            };
            let preview = imports::preview(&root, &lib, &read);
            let items: Vec<_> = preview.iter().flat_map(|l| l.items.iter()).collect();
            for (want, got) in expect.iter().zip(&items) {
                let same = match (&want.file, &got.found) {
                    (Some(f), Some(c)) => f == &root.join(&c.path),
                    (None, _) => got.foreign.title == want.title,
                    _ => false,
                };
                if same {
                    back += 1;
                } else {
                    wrong += 1;
                    if wrong <= 6 {
                        eprintln!(
                            "    {} / {} ({}) came back as {:?} ({})",
                            want.title,
                            want.artist,
                            want.file
                                .as_deref()
                                .map(|f| f.display().to_string())
                                .unwrap_or_default(),
                            got.found.as_ref().map(|c| &c.path),
                            got.why
                        );
                    }
                }
            }
            wrong += expect.len().abs_diff(items.len());
        }
        eprintln!(
            "{format:?}: {songs} songs, {kept} written, {back} came back in place, {wrong} didn't"
        );
        // A plain list of songs ("Artist - Title") can't say where an artist's own name has a dash
        // ("Avinash - Vishwajeet"): for a song you don't have, nothing settles it. Reported, not
        // failed; every other format must bring every song back.
        failed |= wrong > 0 && format != Format::Txt;
    }
    assert!(!failed, "a song didn't come back in place");
}
