//! Japanese without the dictionary (kana in Hepburn romaji), and the dictionary's installer
//! against a fake download. With the real dictionary: `tests/japanese_songs.rs`.
#![cfg(feature = "japanese")]

use std::cell::Cell;
use std::io::{Cursor, Write};
use std::path::Path;

use lyrics_text::japanese::dictionary::{self, Error, Fetch, Progress, Spec};
use lyrics_text::japanese::{Japanese, NeedsDictionary, needs_dictionary};
use lyrics_text::{Line, Word, romanize};
use sha2::{Digest, Sha256};

fn ro(s: &str) -> String {
    Japanese::kana_only().romanize(s).expect("kana only")
}

#[test]
fn hepburn_kana() {
    assert_eq!(ro("さくら"), "sakura");
    assert_eq!(ro("しちつふじ"), "shichitsufuji");
    assert_eq!(ro("きゃりーぱみゅぱみゅ"), "kyariipamyupamyu");
    assert_eq!(ro("まっちゃ"), "matcha");
    assert_eq!(ro("きって"), "kitte");
    assert_eq!(ro("きんえん"), "kin'en");
    assert_eq!(ro("こんや"), "kon'ya");
    assert_eq!(ro("しんぶん"), "shinbun");
    assert_eq!(ro("ぢゃ づ を"), "ja zu o");
    assert_eq!(ro("ありがとう"), "arigatou");
}

#[test]
fn katakana_and_loanwords() {
    assert_eq!(ro("ハート"), "haato");
    assert_eq!(ro("ファイル"), "fairu");
    assert_eq!(ro("ティッシュ"), "tisshu");
    assert_eq!(ro("ヴァイオリン"), "vaiorin");
    assert_eq!(ro("ウィンドウ"), "windou");
    assert_eq!(ro("チェック"), "chekku");
}

#[test]
fn words_cut_at_spaces_marks_and_script_changes() {
    assert_eq!(ro("ラブソングを うたう"), "rabusongu o utau");
    assert_eq!(ro("さくら、さくら。"), "sakura, sakura.");
    assert_eq!(ro("「すき」って"), "\"suki\" tte");
    assert_eq!(ro("Hello さくら"), "Hello sakura");
    assert_eq!(ro("ありがとうbaby"), "arigatou baby");
    assert_eq!(ro("ＯＫ　さよなら"), "OK sayonara");
}

#[test]
fn kanji_wait_for_the_dictionary() {
    assert!(needs_dictionary("君の名は"));
    assert!(!needs_dictionary("きみのなは"));
    let j = Japanese::kana_only();
    assert!(!j.has_dictionary());
    assert_eq!(j.romanize("君の名は"), Err(NeedsDictionary));
}

fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
    Word {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

fn line(text: &str, words: &[Word]) -> Option<lyrics_text::Roman> {
    romanize(&Line::new(text).words(words))
}

#[test]
fn each_sound_keeps_its_time() {
    let words = [w(0, 100, "ま"), w(100, 200, "っ"), w(200, 300, "ちゃ")];
    let r = line("まっちゃ", &words).unwrap();
    assert_eq!(r.lang, "ja-Latn");
    assert_eq!(
        r.words,
        vec![w(0, 100, "Ma"), w(100, 200, "t"), w(200, 300, "cha")]
    );
    let words = [
        w(0, 100, "さ"),
        w(100, 200, "く"),
        w(200, 300, "ら "),
        w(300, 900, "さくら"),
    ];
    assert_eq!(
        line("さくら さくら", &words).unwrap().words,
        vec![
            w(0, 100, "Sa"),
            w(100, 200, "ku"),
            w(200, 300, "ra "),
            w(300, 900, "sakura"),
        ]
    );
}

#[test]
fn a_line_with_kanji_waits_for_the_dictionary() {
    assert_eq!(line("君の名は", &[]), None);
    // Kanji alone in a song known to be Japanese are not read as Chinese.
    let ja = romanize(&Line::new("未来").lang(Some("ja")));
    assert_eq!(ja, None);
    assert_eq!(line("さよならBaby", &[]).unwrap().text, "Sayonara Baby");
}

// The installer, against a fake download.

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut out);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap();
    }
    out.into_inner()
}

fn spec_for(bytes: &[u8]) -> Spec {
    let hash: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Spec {
        name: "test-dict-1",
        url: "https://example.invalid/test-dict-1.zip",
        sha256: Box::leak(hash.into_boxed_str()),
        size: bytes.len() as u64,
        folder: "lindera-ipadic",
    }
}

struct Fake {
    bytes: Vec<u8>,
    calls: Cell<u32>,
    fail: bool,
}

impl Fake {
    fn new(bytes: Vec<u8>) -> Fake {
        Fake {
            bytes,
            calls: Cell::new(0),
            fail: false,
        }
    }
}

impl Fetch for Fake {
    fn fetch(
        &self,
        _url: &str,
        into: &mut dyn Write,
        more: &mut dyn FnMut(u64) -> bool,
    ) -> Result<(), Error> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            return Err(Error::Network("offline".into()));
        }
        dictionary::copy_all(&mut Cursor::new(&self.bytes), into, more)
    }
}

fn good_zip() -> Vec<u8> {
    zip_of(&[
        ("lindera-ipadic/metadata.json", b"{}"),
        ("lindera-ipadic/NOTICE.txt", b"notice"),
        ("lindera-ipadic/dict.words", &[7u8; 200_000]),
    ])
}

fn leftovers(app: &Path) -> usize {
    std::fs::read_dir(app.join("dictionaries"))
        .map(|d| {
            d.filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".part"))
            })
            .count()
        })
        .unwrap_or(0)
}

#[test]
fn installs_checks_and_reports_progress() {
    let app = tempfile::tempdir().unwrap();
    let bytes = good_zip();
    let spec = spec_for(&bytes);
    let fake = Fake::new(bytes);
    assert!(dictionary::installed(app.path(), &spec).is_none());
    let mut seen = Vec::new();
    let dir = dictionary::install(app.path(), &spec, &fake, &mut |p| {
        seen.push(p);
        true
    })
    .unwrap();
    assert_eq!(dir, dictionary::folder(app.path(), &spec));
    assert!(dir.join("metadata.json").is_file());
    assert_eq!(std::fs::read(dir.join("NOTICE.txt")).unwrap(), b"notice");
    assert_eq!(dictionary::installed(app.path(), &spec), Some(dir));
    assert!(seen.contains(&Progress::Downloading {
        done: spec.size,
        total: spec.size
    }));
    assert!(seen.ends_with(&[Progress::Checking, Progress::Unpacking]));
    assert_eq!(leftovers(app.path()), 0);
    // Installed: no second download.
    dictionary::install(app.path(), &spec, &fake, &mut |_| true).unwrap();
    assert_eq!(fake.calls.get(), 1);
}

#[test]
fn a_wrong_checksum_leaves_nothing() {
    let app = tempfile::tempdir().unwrap();
    let bytes = good_zip();
    let mut spec = spec_for(&bytes);
    spec.sha256 = "00".repeat(32).leak();
    let err = dictionary::install(app.path(), &spec, &Fake::new(bytes), &mut |_| true);
    assert!(matches!(err, Err(Error::Checksum { .. })), "{err:?}");
    assert!(dictionary::installed(app.path(), &spec).is_none());
    assert!(!dictionary::folder(app.path(), &spec).exists());
    assert_eq!(leftovers(app.path()), 0);
}

#[test]
fn a_larger_download_is_refused() {
    let app = tempfile::tempdir().unwrap();
    let bytes = good_zip();
    let mut spec = spec_for(&bytes);
    spec.size -= 10;
    let err = dictionary::install(app.path(), &spec, &Fake::new(bytes), &mut |_| true);
    assert!(matches!(err, Err(Error::Size { .. })), "{err:?}");
    assert!(dictionary::installed(app.path(), &spec).is_none());
}

#[test]
fn it_can_be_stopped_and_fails_cleanly_offline() {
    let app = tempfile::tempdir().unwrap();
    let bytes = good_zip();
    let spec = spec_for(&bytes);
    let err = dictionary::install(
        app.path(),
        &spec,
        &Fake::new(bytes.clone()),
        &mut |p| !matches!(p, Progress::Downloading { done, .. } if done > 0),
    );
    assert!(matches!(err, Err(Error::Stopped)), "{err:?}");
    let mut offline = Fake::new(bytes);
    offline.fail = true;
    let err = dictionary::install(app.path(), &spec, &offline, &mut |_| true);
    assert!(matches!(err, Err(Error::Network(_))), "{err:?}");
    assert!(dictionary::installed(app.path(), &spec).is_none());
    assert_eq!(leftovers(app.path()), 0);
}

#[test]
fn paths_that_leave_the_folder_are_refused() {
    let app = tempfile::tempdir().unwrap();
    let bytes = zip_of(&[
        ("lindera-ipadic/metadata.json", b"{}"),
        ("lindera-ipadic/../../evil.txt", b"x"),
    ]);
    let spec = spec_for(&bytes);
    let err = dictionary::install(app.path(), &spec, &Fake::new(bytes), &mut |_| true);
    assert!(matches!(err, Err(Error::Unpack(_))), "{err:?}");
    assert!(!app.path().join("evil.txt").exists());
    assert!(dictionary::installed(app.path(), &spec).is_none());
}

#[test]
fn the_pinned_release() {
    let s = dictionary::IPADIC;
    assert!(
        s.url
            .starts_with("https://github.com/lindera/lindera/releases/download/v6.2.0/")
    );
    assert_eq!(s.sha256.len(), 64);
    assert_eq!(s.size, 10_519_550);
}
