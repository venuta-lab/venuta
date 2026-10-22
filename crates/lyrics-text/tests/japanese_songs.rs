//! Japanese reading gate, with the real dictionary: 65 lines of five public-domain songs
//! against the romaji people wrote for them (`fixtures/japanese-songs.md` for the sources).
//!
//! Needs the unpacked IPADIC folder in `LYRICS_TEXT_IPADIC` (the dictionary is never in the
//! repository); without it these tests say so and pass. To get it once:
//! `cargo run -p lyrics-text --example ipadic -- <folder>`.
#![cfg(feature = "japanese")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lyrics_text::japanese::Japanese;
use lyrics_text::{Line, Word, romanize};

fn dictionary() -> Option<Japanese> {
    let Some(dir) = std::env::var_os("LYRICS_TEXT_IPADIC").map(PathBuf::from) else {
        println!("LYRICS_TEXT_IPADIC is not set: the dictionary tests did not run");
        return None;
    };
    let t = Instant::now();
    let j = Japanese::with_dictionary(&dir).expect("the dictionary loads");
    println!("dictionary loaded in {:?}", t.elapsed());
    Some(j)
}

/// The sounds only: no spaces, marks or capitals, long vowels as the kana write them, "wo"
/// as "o" (both are を).
fn sounds(s: &str) -> String {
    let s = s
        .to_lowercase()
        .replace('ō', "ou")
        .replace('ū', "uu")
        .replace('ā', "aa")
        .replace('ē', "ee")
        .replace('ī', "ii");
    let s: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    s.replace("wo", "o")
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1)
                .min(row[j - 1] + 1)
                .min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = cur;
        }
    }
    row[b.len()]
}

#[test]
fn song_lines_read_as_people_read_them() {
    let Some(j) = dictionary() else { return };
    let lines: Vec<(&str, &str)> = include_str!("fixtures/japanese-songs.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[2], f[3])
        })
        .collect();
    assert_eq!(lines.len(), 65);
    let (mut exact, mut errors, mut letters) = (0, 0, 0);
    let mut misses = Vec::new();
    for (ja, human) in &lines {
        let ours = j.romanize(ja).unwrap();
        let (a, b) = (sounds(&ours), sounds(human));
        let d = distance(&a, &b);
        errors += d;
        letters += b.chars().count();
        if d == 0 {
            exact += 1;
        } else {
            misses.push(format!("{ja}\n    ours:   {ours}\n    people: {human}"));
        }
    }
    let accuracy = 100.0 * (1.0 - errors as f64 / letters as f64);
    println!(
        "japanese songs: {exact}/{} lines read exactly, letters {accuracy:.1}% right",
        lines.len()
    );
    for m in &misses {
        println!("  {m}");
    }
    // A floor under the measured result, so a change that reads worse fails.
    assert!(exact >= 38, "{exact} lines exact");
    assert!(accuracy >= 92.5, "{accuracy:.1}%");
}

#[test]
fn particles_endings_and_words() {
    let Some(j) = dictionary() else { return };
    let ro = |s: &str| j.romanize(s).unwrap();
    assert_eq!(ro("君の名は"), "kimi no na wa");
    assert_eq!(ro("私は学校へ行きます"), "watashi wa gakkou e ikimasu");
    assert_eq!(ro("本を読んでいる"), "hon o yonde iru");
    assert_eq!(ro("こんにちは"), "konnichiwa");
    assert_eq!(ro("東京に行った"), "toukyou ni itta");
    assert_eq!(ro("何も知らない"), "nani mo shiranai");
    assert_eq!(ro("よかったでしょう"), "yokatta deshou");
    assert_eq!(ro("ありがとう、Baby"), "arigatou, Baby");
}

fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
    Word {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

#[test]
fn a_reading_takes_the_time_of_its_kanji() {
    let Some(dir) = std::env::var_os("LYRICS_TEXT_IPADIC").map(PathBuf::from) else {
        return;
    };
    let timed = |text: &str, words: &[Word]| {
        romanize(&Line::new(text).words(words).japanese_dictionary(Some(&dir)))
            .expect("romanized")
            .words
    };
    // 走る: the kanji's reading with the kanji, the kana ending on its own.
    assert_eq!(
        timed("走る", &[w(0, 100, "走"), w(100, 200, "る")]),
        vec![w(0, 100, "Hashi"), w(100, 200, "ru")]
    );
    let words = [w(0, 100, "未"), w(100, 200, "来"), w(200, 300, "へ")];
    assert_eq!(
        timed("未来へ", &words),
        vec![w(0, 200, "Mirai "), w(200, 300, "e")]
    );
    // With the dictionary, a line with kanji is read whole.
    let r = romanize(&Line::new("君の名は").japanese_dictionary(Some(&dir))).unwrap();
    assert_eq!(
        (r.text.as_str(), r.lang.as_str()),
        ("Kimi no na wa", "ja-Latn")
    );
}

#[test]
fn speed_per_line() {
    let Some(j) = dictionary() else { return };
    let line = "兎追いし彼の山、小鮒釣りし彼の川、夢は今も巡りて";
    j.romanize(line).unwrap();
    let t = Instant::now();
    let n = 500;
    for _ in 0..n {
        std::hint::black_box(j.romanize(std::hint::black_box(line)).unwrap());
    }
    let per = t.elapsed() / n;
    println!("japanese: {per:?} per line");
    assert!(per < Duration::from_millis(5), "{per:?}");
}
