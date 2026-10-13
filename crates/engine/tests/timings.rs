//! How long each step of the heavy screens takes on a real library. Not run by default: it
//! needs a library, and it repairs that library's playlists as opening Playlists does, so point
//! it at a copy.
//!
//! VENUTA_HOME=D:\copy\of\library cargo test --release -p engine --test timings -- --ignored --nocapture

use engine::library::Library;
use engine::{Index, paths};
use std::path::PathBuf;
use std::time::Instant;

fn step<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    eprintln!("{name:<28} {:>7.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    out
}

#[test]
#[ignore]
fn playlists_screen() {
    let root = PathBuf::from(std::env::var_os("VENUTA_HOME").expect("set VENUTA_HOME to a copy"));
    let rules = paths::app_data_dir().join("rules");
    let mut index = step("open index", || {
        Index::open(
            &paths::index_path_for(&root),
            &paths::canonical_root_string(&root),
        )
        .unwrap()
    });
    let report = step("scan (nothing changed)", || {
        engine::scan(&mut index, &root).unwrap()
    });
    eprintln!("  {} files, {} changed", report.files_seen, report.changed);
    let lib = step("build library", || Library::load(&index).unwrap());
    eprintln!("  {} songs", lib.tracks.len());
    for round in 1..=2 {
        eprintln!("round {round}");
        step("ensure default smart", || {
            engine::playlists::ensure_default_smart(&rules).unwrap()
        });
        step("repair_all", || engine::playlists::repair_all(&root, &lib));
        step("refind_all", || engine::imports::refind_all(&root, &lib));
        step("refresh_smart", || {
            engine::playlists::refresh_smart(&root, &rules, &lib, &index).unwrap()
        });
        step("evaluate_all", || {
            engine::playlists::evaluate_all(&rules, &lib, &index).unwrap()
        });
        step("list", || engine::playlists::list(&root, &lib));
        step("wanted_now", || engine::playlists::wanted_now(&root, &lib));
    }
}

/// Reading Apple Music's library (decrypting it), with the key from the library's settings.
/// Only reads.
#[test]
#[ignore]
fn apple_library() {
    let s = engine::settings::load();
    let Some(key) = s.apple_key else {
        eprintln!("no Apple Music key in these settings");
        return;
    };
    let file = s
        .apple_library
        .map(PathBuf::from)
        .or_else(engine::apple::default_library_path)
        .expect("Apple Music library");
    for _ in 0..2 {
        let apple = step("decrypt + read", || {
            engine::apple::read(&file, &key).unwrap()
        });
        let (lists, _) = step("to playlists", || engine::imports::apple_library(&apple));
        eprintln!("  {} playlists, {} songs", lists.len(), apple.tracks.len());
    }
}
