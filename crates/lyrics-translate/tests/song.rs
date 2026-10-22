//! A song's lines translated in one go: only the lines in the song's language, each different
//! line once, kept per song, and asked once however many windows ask together.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use lyrics_translate::cache::SongCache;
use lyrics_translate::lines::{tidy, wanted};
use lyrics_translate::{Error, Shared, Translator, translate_song};

/// Upper-cases each line, and remembers what it was given.
#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
    given: Mutex<Vec<Vec<String>>>,
    short: bool,
}

impl Translator for Fake {
    fn engine(&self) -> &str {
        "fake"
    }
    fn version(&self, from: &str, into: &str) -> Option<String> {
        (from == "hi" && into == "en").then(|| "hi-en 1".to_owned())
    }
    fn translate(&self, _from: &str, _into: &str, lines: &[&str]) -> Result<Vec<String>, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.given
            .lock()
            .unwrap()
            .push(lines.iter().map(|l| l.to_string()).collect());
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut out: Vec<String> = lines.iter().map(|l| format!("EN {l}")).collect();
        if self.short {
            out.pop();
        }
        Ok(out)
    }
}

const SONG: [&str; 6] = [
    "तुम ही हो",
    "",
    "Baby I love you",
    "तुम ही हो",
    "♪",
    "अब तुम ही हो",
];

#[test]
fn only_the_songs_own_lines_each_once() {
    let dir = tempfile::tempdir().unwrap();
    let cache = SongCache::new(dir.path());
    let fake = Fake::default();
    let t = translate_song(&fake, &cache, "hi", "en", &SONG).unwrap();
    assert_eq!(
        t.lines,
        vec![
            Some("EN तुम ही हो".to_owned()),
            None,
            None,
            Some("EN तुम ही हो".to_owned()),
            None,
            Some("EN अब तुम ही हो".to_owned()),
        ]
    );
    assert_eq!(t.engine, "fake");
    assert_eq!(t.version, "hi-en 1");
    assert_eq!((t.from.as_str(), t.into.as_str()), ("hi", "en"));
    assert_eq!(
        fake.given.lock().unwrap()[0],
        vec!["तुम ही हो".to_owned(), "अब तुम ही हो".to_owned()]
    );
}

#[test]
fn a_song_is_translated_once_then_kept() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let first = translate_song(&fake, &SongCache::new(dir.path()), "hi", "en", &SONG).unwrap();
    // Another cache over the same folder (the app started again).
    let cache = SongCache::new(dir.path());
    let again = translate_song(&fake, &cache, "hi", "en", &SONG).unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.lines, again.lines);
    assert_eq!(cache.latest(&SONG).unwrap().lines, first.lines);
    // Other lyrics for the song are another song as far as the cache knows.
    assert!(cache.latest(&SONG[..3]).is_none());
}

#[test]
fn the_latest_translation_of_a_song_is_the_one_shown() {
    let dir = tempfile::tempdir().unwrap();
    let cache = SongCache::new(dir.path());
    let mut e = translate_song(&Fake::default(), &cache, "hi", "en", &SONG).unwrap();
    e.into = "es".into();
    e.at_ms += 1;
    e.lines = vec![None, None, None, None, None, Some("ES".into())];
    cache.put(&SONG, &e).unwrap();
    assert_eq!(cache.latest(&SONG).unwrap().into, "es");
    assert!(cache.get(&SONG, "hi", "en", "fake", "hi-en 1").is_some());
    assert!(cache.get(&SONG, "hi", "en", "fake", "hi-en 2").is_none());
}

#[test]
fn a_pair_the_engine_cannot_do_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let err = translate_song(&fake, &SongCache::new(dir.path()), "pa", "en", &SONG).unwrap_err();
    assert!(matches!(err, Error::NoModel { .. }), "{err}");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn lines_lost_by_the_engine_are_an_error_not_a_shift() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake {
        short: true,
        ..Default::default()
    };
    let cache = SongCache::new(dir.path());
    let err = translate_song(&fake, &cache, "hi", "en", &SONG).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Lines {
                expected: 2,
                got: 1
            }
        ),
        "{err}"
    );
    assert!(cache.latest(&SONG).is_none());
}

#[test]
fn every_window_asking_at_once_gets_one_translation() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(SongCache::new(dir.path()));
    let fake = Arc::new(Fake::default());
    let shared: Arc<Shared<Result<Vec<Option<String>>, String>>> = Arc::default();
    let start = Arc::new(Barrier::new(4));
    let windows: Vec<_> = (0..4)
        .map(|_| {
            let (cache, fake, shared, start) =
                (cache.clone(), fake.clone(), shared.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                shared.run("song|hi|en", || {
                    translate_song(&*fake, &cache, "hi", "en", &SONG)
                        .map(|e| e.lines)
                        .map_err(|e| e.to_string())
                })
            })
        })
        .collect();
    let answers: Vec<_> = windows.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(answers.iter().all(|a| a == &answers[0] && a.is_ok()));
    // Asked again later, it runs again (the cache answers it then).
    let later = shared.run("song|hi|en", || Ok(vec![]));
    assert_eq!(later, Ok(vec![]));
}

#[test]
fn which_lines_are_the_songs_language() {
    assert!(wanted("hi", "तुम ही हो"));
    assert!(!wanted("hi", "Baby I love you"));
    assert!(!wanted("hi", "♪ ... 123"));
    assert!(!wanted("hi", ""));
    assert!(wanted("hi-IN", "तुम ही हो"));
    assert!(wanted("ja", "桜　桜"));
    assert!(wanted("ja", "匂いぞ　出ずる"));
    assert!(wanted("ja", "さくら"));
    assert!(!wanted("ja", "I love you"));
    assert!(wanted("ko", "도라지 도라지"));
    assert!(wanted("ko", "금일(今日)도 상봉(上峯)에"));
    assert!(wanted("ta", "யாதும் ஊரே"));
    assert!(!wanted("ta", "तुम ही हो"));
    assert!(wanted("es", "La cucaracha"));
    assert!(wanted("ru", "Привет"));
    assert!(!wanted("ru", "Hello"));
    // A language whose letters aren't known here: any line with letters.
    assert!(wanted("xx", "anything"));
}

#[test]
fn a_word_stuck_on_repeat_is_cut_to_the_songs_own_count() {
    // The Korean model repeated a word twelve times for a line that sings it three times.
    let stuck = ["Doraji"; 12].join(" ");
    assert_eq!(tidy("도라지 도라지 도라지", &stuck), "Doraji Doraji Doraji");
    assert_eq!(tidy("la la", "la la la la la la"), "la la");
    // A word said twice is fine even when the original says it once.
    assert_eq!(tidy("एक", "very very good"), "very very good");
    assert_eq!(tidy("एक", "no no no no"), "no no");
    assert_eq!(tidy("x", "  Hello there.  "), "Hello there.");
    assert_eq!(tidy("x", "Go, go, go, go, go!"), "Go, go!");
}
