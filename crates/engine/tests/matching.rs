//! How well a playlist from elsewhere finds your files: every song of a real library, described
//! the ways other places describe it, matched back. Not run by default: it needs a library.
//! Only reads (the library index; nothing is written).
//!
//! VENUTA_HOME=D:\copy\of\library cargo test --release -p engine --test matching -- --ignored --nocapture
//!
//! "Right" is the same file, or another copy of the same song (same title key, a shared artist,
//! length within 3 s). "Wrong" is anything else, split by how sure the matcher was: a wrong
//! **found** (Exact) is the number that must stay under 1 in 200 (the plan's goal); a wrong
//! **close** match is shown to you to check, so it costs a click, not a wrong song.

use engine::library::{Library, Track};
use engine::playlists::{Foreign, Grade, Matcher, title_key};
use engine::{Index, paths};
use std::path::PathBuf;
use std::time::Instant;

/// How a song arrives from elsewhere.
#[derive(Clone, Copy)]
enum Kind {
    /// A streaming service or its export: title, first artist, album, length.
    Service,
    /// A pasted list: "Artist - Title", nothing else.
    Text,
    /// Written differently: a featured artist in the title, a remaster note, all artists joined,
    /// the length a second or two off.
    Noisy,
    /// Spelled differently, as Indian titles often are ("Kesariya" / "Kesariyaa").
    Spelling,
}

const KINDS: [(Kind, &str); 4] = [
    (Kind::Service, "service (title, artist, album, length)"),
    (Kind::Text, "text (artist and title only)"),
    (Kind::Noisy, "noisy (feat., remaster, length off)"),
    (Kind::Spelling, "spelling (a vowel doubled)"),
];

fn artists(t: &Track) -> Vec<String> {
    let mut a: Vec<String> = t.tags.get("ARTIST").cloned().unwrap_or_default();
    if a.is_empty() {
        a = t.tags.get("ALBUMARTIST").cloned().unwrap_or_default();
    }
    a
}

/// Double the last vowel of the first word that ends in one: Kesariya → Kesariyaa.
fn respell(title: &str) -> Option<String> {
    let mut words: Vec<String> = title.split(' ').map(str::to_owned).collect();
    for w in words.iter_mut() {
        if let Some(last) = w.chars().last()
            && "aeiou".contains(last.to_ascii_lowercase())
            && w.chars().count() >= 4
        {
            w.push(last);
            return Some(words.join(" "));
        }
    }
    None
}

fn describe(t: &Track, kind: Kind) -> Option<Foreign> {
    let title = t.first("TITLE")?.to_owned();
    let all = artists(t);
    let artist = all.first()?.clone();
    let album = t.first("ALBUM").unwrap_or_default().to_owned();
    let length = t.duration_ms as u64;
    Some(match kind {
        Kind::Service => Foreign {
            title,
            artist,
            album,
            duration_ms: Some(length),
            ..Default::default()
        },
        Kind::Text => Foreign {
            title,
            artist,
            ..Default::default()
        },
        Kind::Noisy => {
            let title = match all.get(1) {
                Some(featured) => format!("{title} (feat. {featured})"),
                None => format!("{title} - Remastered"),
            };
            Foreign {
                title,
                artist: all.join(", "),
                album,
                duration_ms: Some(length + 1_500),
                ..Default::default()
            }
        }
        Kind::Spelling => Foreign {
            title: respell(&title)?,
            artist,
            album,
            duration_ms: Some(length),
            ..Default::default()
        },
    })
}

fn same_song(a: &Track, b: &Track) -> bool {
    if a.path == b.path {
        return true;
    }
    let (Some(ta), Some(tb)) = (a.first("TITLE"), b.first("TITLE")) else {
        return false;
    };
    let shared = artists(a)
        .iter()
        .any(|x| artists(b).iter().any(|y| x.eq_ignore_ascii_case(y)));
    title_key(ta) == title_key(tb) && shared && a.duration_ms.abs_diff(b.duration_ms) <= 3_000
}

#[derive(Default)]
struct Tally {
    asked: usize,
    found_right: usize,
    close_right: usize,
    found_wrong: usize,
    close_wrong: usize,
    missing: usize,
    examples: Vec<String>,
}

fn pct(n: usize, of: usize) -> f64 {
    if of == 0 {
        0.0
    } else {
        n as f64 * 100.0 / of as f64
    }
}

#[test]
#[ignore]
fn songs_find_their_files() {
    let home = PathBuf::from(std::env::var_os("VENUTA_HOME").expect("set VENUTA_HOME to a copy"));
    // The index belongs to the library's music folder, as the app opens it.
    let root = paths::music_dir(&home);
    let index_file = paths::index_path_for(&root);
    assert!(
        index_file.exists(),
        "no index for {}: open the library in Venuta first",
        root.display()
    );
    let index = Index::open(&index_file, &paths::canonical_root_string(&root)).unwrap();
    let lib = Library::load(&index).unwrap();
    let t = Instant::now();
    let matcher = Matcher::new(&lib);
    eprintln!(
        "{} songs; matcher built in {:.1} ms",
        lib.tracks.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    for (kind, name) in KINDS {
        let mut tally = Tally::default();
        let t = Instant::now();
        for track in &lib.tracks {
            let Some(f) = describe(track, kind) else {
                continue;
            };
            tally.asked += 1;
            match matcher.grade(&f) {
                None => tally.missing += 1,
                Some(found) => {
                    let right = same_song(track, found.track);
                    match (found.grade, right) {
                        (Grade::Exact, true) => tally.found_right += 1,
                        (Grade::Close, true) => tally.close_right += 1,
                        (Grade::Exact, false) => {
                            tally.found_wrong += 1;
                            if tally.examples.len() < 8 {
                                tally.examples.push(format!(
                                    "    \"{} / {}\" ({}, {} s, {:?})
      found {} ({} s, {:?}): {}",
                                    f.title,
                                    f.artist,
                                    track.path,
                                    track.duration_ms / 1000,
                                    artists(track),
                                    found.track.path,
                                    found.track.duration_ms / 1000,
                                    artists(found.track),
                                    found.why
                                ));
                            }
                        }
                        (Grade::Close, false) => tally.close_wrong += 1,
                    }
                }
            }
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let a = tally.asked;
        eprintln!("\n{name}: {a} songs, {ms:.0} ms");
        eprintln!(
            "  found, right   {:>6}  {:>5.1}%",
            tally.found_right,
            pct(tally.found_right, a)
        );
        eprintln!(
            "  close, right   {:>6}  {:>5.1}%",
            tally.close_right,
            pct(tally.close_right, a)
        );
        eprintln!(
            "  not found      {:>6}  {:>5.1}%",
            tally.missing,
            pct(tally.missing, a)
        );
        eprintln!(
            "  close, wrong   {:>6}  {:>5.1}%",
            tally.close_wrong,
            pct(tally.close_wrong, a)
        );
        eprintln!(
            "  FOUND, WRONG   {:>6}  {:>5.2}%  (goal under 0.5%)",
            tally.found_wrong,
            pct(tally.found_wrong, a)
        );
        for e in &tally.examples {
            eprintln!("{e}");
        }
    }
}
