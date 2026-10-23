//! Builds the common-word tables and the accuracy fixtures from Google's Dakshina dataset
//! (CC BY-SA 4.0, https://github.com/google-research-datasets/dakshina). The dataset is not
//! in the repository; download `dakshina_dataset_v1.0.tar` from that page and unpack its
//! `*/lexicons` and `*/romanized` folders.
//!
//! ```text
//! cargo run -p lyrics-text --example dakshina -- table   <dataset dir> [--splits train,dev] hi mr bn ...
//! cargo run -p lyrics-text --example dakshina -- fixture <dataset dir> <split> <max sentences> hi mr ...
//! cargo run -p lyrics-text --example dakshina -- misses  <dataset dir> <split> <lang> [lists]
//! cargo run -p lyrics-text --example dakshina -- rules   <dataset dir> <lexicon split> <lang> [misses file]
//! cargo run -p lyrics-text --example dakshina -- score   <split> [misses to show] [langs]
//! ```
//!
//! `table` writes `data/dakshina/<lang>.tsv`: for every lexicon word whose spelling by rule no
//! annotator used, the spelling most annotators used. Run it again after changing the rules.
//! It reads all three lexicon splits; `--splits train,dev` leaves the test lexicon out, to see
//! what the test split scores with none of its lexicon's words in the tables.
//! `fixture` writes `tests/fixtures/dakshina/<lang>.<split>.tsv`: every native word of the
//! split's hand-romanized sentences, how often it occurs, the romanizations the annotators gave
//! it in those sentences (with counts), and its romanizations in the lexicons.
//! `misses` prints the commonest words the engine spells differently from every annotator.
//! `rules <dataset dir> <lexicon split> <lang> [misses file]` scores the rules alone (no word
//! lists) on a lexicon split's words, each word counted once, and writes the misses.
//! `score <split> [misses to show] [langs]` scores a fixture made by `fixture` the way
//! `tests/dakshina.rs` scores the test split (for tuning on `dev`), with its commonest misses.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use lyrics_text::indic::{Lang, Options, lookup_key, word_to_latin, word_to_latin_with};
use unicode_normalization::UnicodeNormalization;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    match args.first().map(String::as_str) {
        Some("table") => {
            let dir = PathBuf::from(&args[1]);
            // `--splits train,dev` builds the tables from those lexicon splits only (to measure
            // the test split with no word of its lexicon in the tables).
            let (splits, codes) = match args.get(2).map(String::as_str) {
                Some("--splits") => (args[3].split(',').collect::<Vec<_>>(), &args[4..]),
                _ => (SPLITS.to_vec(), &args[2..]),
            };
            for code in codes {
                table(&dir, code, crate_dir, &splits);
            }
        }
        Some("fixture") => {
            let dir = PathBuf::from(&args[1]);
            let split = &args[2];
            let max: usize = args[3].parse().expect("max sentences");
            for code in &args[4..] {
                fixture(&dir, code, split, max, crate_dir);
            }
        }
        Some("misses") => {
            let dir = PathBuf::from(&args[1]);
            misses(
                &dir,
                &args[2],
                &args[3],
                args.get(4).is_some_and(|a| a == "lists"),
            );
        }
        Some("score") => {
            let split = &args[1];
            let show: usize = args
                .get(2)
                .map_or(0, |a| a.parse().expect("misses to show"));
            let all = ["hi", "mr", "bn", "gu", "pa", "ta", "te", "kn", "ml"].map(String::from);
            let codes = args.get(3..).filter(|c| !c.is_empty()).unwrap_or(&all);
            for code in codes {
                score(crate_dir, split, code, show);
            }
        }
        Some("rules") => rules(&PathBuf::from(&args[1]), &args[2], &args[3], args.get(4)),
        Some("vowels") => vowels(&PathBuf::from(&args[1]), &args[2]),
        Some("choose") => {
            for code in &args[3..] {
                choose(&PathBuf::from(&args[1]), &args[2], code);
            }
        }
        _ => eprintln!("usage: dakshina table|fixture|misses|rules|score|vowels|choose ..."),
    }
}

fn lang(code: &str) -> Lang {
    Lang::from_code(code).unwrap_or_else(|| panic!("unknown language {code}"))
}

fn canonical(word: &str) -> String {
    word.nfc()
        .filter(|c| !matches!(c, '\u{200C}' | '\u{200D}'))
        .collect()
}

const SPLITS: [&str; 3] = ["train", "dev", "test"];

/// Native word -> romanization -> attestations, from the three lexicon splits.
fn lexicon(dir: &Path, code: &str) -> BTreeMap<String, BTreeMap<String, u32>> {
    lexicon_of(dir, code, &SPLITS)
}

/// [`lexicon`] from some of the splits.
fn lexicon_of(dir: &Path, code: &str, splits: &[&str]) -> BTreeMap<String, BTreeMap<String, u32>> {
    let mut lex: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for split in splits {
        let path = dir
            .join(code)
            .join("lexicons")
            .join(format!("{code}.translit.sampled.{split}.tsv"));
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for line in text.lines() {
            let mut f = line.split('\t');
            let (Some(native), Some(roman), Some(count)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let roman = roman.trim().to_lowercase();
            if roman.is_empty() || !roman.bytes().all(|b| b.is_ascii_lowercase()) {
                continue;
            }
            *lex.entry(lookup_key(native.trim(), lang(code)))
                .or_default()
                .entry(roman)
                .or_default() += count.trim().parse::<u32>().unwrap_or(1);
        }
    }
    lex
}

/// Edits between two short strings.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut row = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            row[j] = (prev[j] + 1)
                .min(row[j - 1] + 1)
                .min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = row;
    }
    prev[b.len()]
}

fn table(dir: &Path, code: &str, crate_dir: &Path, splits: &[&str]) {
    let l = lang(code);
    let lex = lexicon_of(dir, code, splits);
    let mut out = String::new();
    let (mut kept, mut junk) = (0, 0);
    for (native, variants) in &lex {
        if native.chars().any(|c| c.is_ascii()) {
            continue;
        }
        // The rules' spelling for a word no list knows, and the house spelling (long a doubled).
        let ruled = word_to_latin(native, l, false);
        let house = word_to_latin_with(
            native,
            l,
            Options {
                lists: false,
                familiar: Some(true),
                medial: None,
            },
        );
        // Spellings at least half as common as the commonest one.
        let best = variants.values().copied().max().unwrap_or(0);
        let common = |r: &str| variants.get(r).is_some_and(|c| c * 2 >= best);
        let spelled = if house != ruled && common(&house) {
            Some(house.as_str())
        } else if variants.contains_key(&ruled) {
            None
        } else {
            Some("")
        };
        if let Some(h) = spelled
            && !h.is_empty()
        {
            out.push_str(native);
            out.push('\t');
            out.push_str(h);
            out.push('\n');
            kept += 1;
            continue;
        }
        if spelled.is_none() {
            continue;
        }
        let ruled = house;
        // Of the common spellings, the one nearest the house spelling (then the commonest).
        let Some(top) = variants
            .iter()
            .filter(|(r, _)| common(r))
            .min_by_key(|(r, c)| (distance(r, &ruled), std::cmp::Reverse(**c), r.len()))
            .map(|(r, _)| r)
        else {
            continue;
        };
        // A spelling that shares almost nothing with the word's letters is a misaligned row of
        // the lexicon, not a spelling.
        if distance(top, &ruled) * 10 > ruled.len().max(top.len()) * 7 {
            junk += 1;
            continue;
        }
        out.push_str(native);
        out.push('\t');
        out.push_str(top);
        out.push('\n');
        kept += 1;
    }
    let path = crate_dir
        .join("data")
        .join("dakshina")
        .join(format!("{code}.tsv"));
    fs::write(&path, out).expect("write table");
    println!(
        "{code}: {} words in the lexicons, {kept} spelled from the table, {junk} rows left out as misaligned",
        lex.len()
    );
}

/// The split's sentences, each as (native word, annotator's romanization) pairs.
fn sentences(dir: &Path, code: &str, split: &str) -> Vec<Vec<(String, String)>> {
    let rom = dir.join(code).join("romanized");
    let wanted: HashSet<String> =
        fs::read_to_string(rom.join(format!("{code}.romanized.rejoined.{split}.native.txt")))
            .expect("split")
            .lines()
            .map(|l| l.trim().to_string())
            .collect();
    let aligned = fs::read_to_string(rom.join(format!("{code}.romanized.rejoined.aligned.tsv")))
        .expect("aligned");
    let plain = fs::read_to_string(rom.join(format!(
        "{code}.romanized.rejoined.aligned.cased_nopunct.tsv"
    )))
    .expect("aligned nopunct");
    let mut all = Vec::new();
    let mut native_words: Vec<&str> = Vec::new();
    let mut pairs = Vec::new();
    for (a, p) in aligned.lines().zip(plain.lines()) {
        let native = a.split('\t').next().unwrap_or("");
        if native == "</s>" {
            if wanted.contains(native_words.join(" ").trim()) {
                all.push(std::mem::take(&mut pairs));
            }
            pairs.clear();
            native_words.clear();
            continue;
        }
        native_words.push(native);
        let mut f = p.split('\t');
        let (Some(n), Some(r)) = (f.next(), f.next()) else {
            continue;
        };
        pairs.push((n.trim().to_string(), r.trim().to_lowercase()));
    }
    all
}

/// Native words of the split we can score: one native word to one romanized word, in the script.
fn scorable(l: Lang, n: &str, r: &str) -> bool {
    !n.is_empty()
        && !n.contains(' ')
        && !r.contains(' ')
        && !r.is_empty()
        && n.chars().all(|c| {
            lyrics_text::Script::of(c) == l.script()
                || lyrics_text::Script::of(c) == lyrics_text::Script::Zinh
        })
        && r.bytes().all(|b| b.is_ascii_lowercase())
}

fn fixture(dir: &Path, code: &str, split: &str, max: usize, crate_dir: &Path) {
    let l = lang(code);
    let lex = lexicon(dir, code);
    let mut words: BTreeMap<String, (u32, BTreeMap<String, u32>)> = BTreeMap::new();
    let all = sentences(dir, code, split);
    let taken = all.len().min(max);
    for s in all.iter().take(max) {
        for (n, r) in s {
            if !scorable(l, n, r) {
                continue;
            }
            let e = words.entry(canonical(n)).or_default();
            e.0 += 1;
            *e.1.entry(r.clone()).or_default() += 1;
        }
    }
    let mut out = format!(
        "# Dakshina {code} {split}: {taken} hand-romanized sentences. word\\toccurrences\\tsentence romanizations\\tlexicon romanizations\n"
    );
    for (n, (count, seen)) in &words {
        let seen: Vec<String> = seen.iter().map(|(r, c)| format!("{r}:{c}")).collect();
        let lexed: Vec<&str> = lex
            .get(&lookup_key(n, l))
            .map(|v| v.keys().map(String::as_str).collect())
            .unwrap_or_default();
        out.push_str(&format!(
            "{n}\t{count}\t{}\t{}\n",
            seen.join(" "),
            lexed.join(" ")
        ));
    }
    let path = crate_dir
        .join("tests")
        .join("fixtures")
        .join("dakshina")
        .join(format!("{code}.{split}.tsv"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, out).expect("write fixture");
    println!("{code} {split}: {taken} sentences, {} words", words.len());
}

fn misses(dir: &Path, split: &str, code: &str, lists: bool) {
    let l = lang(code);
    let lex = lexicon(dir, code);
    let mut words: HashMap<String, (u32, BTreeMap<String, u32>)> = HashMap::new();
    for s in sentences(dir, code, split) {
        for (n, r) in s {
            if scorable(l, &n, &r) {
                let e = words.entry(canonical(&n)).or_default();
                e.0 += 1;
                *e.1.entry(r).or_default() += 1;
            }
        }
    }
    let (mut total, mut ok, mut folded, mut unseen, mut unseen_ok) = (0, 0, 0, 0, 0);
    let mut by_vowel = [0u32; 3];
    let fold = |r: &str| r.replace("aa", "a").replace("ee", "i").replace("oo", "u");
    let mut missed: Vec<(u32, String, String, String)> = Vec::new();
    for (n, (count, seen)) in &words {
        let got = word_to_latin(n, l, lists);
        total += count;
        let fine = seen.contains_key(&got)
            || lex
                .get(&lookup_key(n, l))
                .is_some_and(|v| v.contains_key(&got));
        if !lex.contains_key(&lookup_key(n, l)) {
            unseen += count;
            if fine {
                unseen_ok += count;
            }
        }
        if fine {
            ok += count;
        } else {
            let f = fold(&got);
            let human: Vec<&String> = seen
                .keys()
                .chain(
                    lex.get(&lookup_key(n, l))
                        .into_iter()
                        .flat_map(|v| v.keys()),
                )
                .collect();
            if human.iter().any(|r| fold(r) == f) {
                folded += count;
            }
            for (k, (a, b)) in [("aa", "a"), ("ee", "i"), ("oo", "u")].iter().enumerate() {
                if human.iter().any(|r| **r == got.replace(a, b)) {
                    by_vowel[k] += count;
                }
            }
            let mut all: Vec<String> = seen.iter().map(|(r, c)| format!("{r}:{c}")).collect();
            if let Some(v) = lex.get(&lookup_key(n, l)) {
                all.push(format!(
                    "| {}",
                    v.iter()
                        .map(|(r, c)| format!("{r}:{c}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            let mark = if lex.contains_key(&lookup_key(n, l)) {
                ""
            } else {
                "*"
            };
            missed.push((*count, format!("{mark}{n}"), got, all.join(" ")));
        }
    }
    missed.sort_by_key(|m| std::cmp::Reverse(m.0));
    println!(
        "{code} {split}: {ok}/{total} = {:.2}%",
        100.0 * ok as f64 / total as f64
    );
    println!(
        "words not in the lexicons: {unseen_ok}/{unseen}; misses that differ only in long vowels (aa/a, ee/i, oo/u): {folded}; fixed by a for aa {}, i for ee {}, u for oo {}",
        by_vowel[0], by_vowel[1], by_vowel[2]
    );
    for (c, n, got, all) in missed.iter().take(150) {
        println!("{c}\t{n}\t{got}\t{all}");
    }
}

/// How often annotators double a long a, by where it stands: for lexicon words whose rule
/// spelling has one "aa", the attestations that keep it doubled against those that write "a".
fn vowels(dir: &Path, code: &str) {
    let l = lang(code);
    let lex = lexicon(dir, code);
    let is_vowel = |b: u8| b"aeiou".contains(&b);
    let mut table: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for (native, variants) in &lex {
        let ruled = word_to_latin(native, l, false);
        if ruled.matches("aa").count() != 1 {
            continue;
        }
        let i = ruled.find("aa").unwrap();
        let single = format!("{}a{}", &ruled[..i], &ruled[i + 2..]);
        let (d, s) = (
            variants.get(&ruled).copied().unwrap_or(0),
            variants.get(&single).copied().unwrap_or(0),
        );
        if d + s == 0 {
            continue;
        }
        let b = ruled.as_bytes();
        let syllable = b[..i]
            .iter()
            .enumerate()
            .filter(|(k, c)| is_vowel(**c) && (*k == 0 || !is_vowel(b[k - 1])))
            .count();
        let syllables = b
            .iter()
            .enumerate()
            .filter(|(k, c)| is_vowel(**c) && (*k == 0 || !is_vowel(b[k - 1])))
            .count();
        let after = &b[i + 2..];
        let closed = after.is_empty()
            || after.iter().take_while(|c| !is_vowel(**c)).count() >= 2
            || after.iter().all(|c| !is_vowel(*c));
        let key = format!(
            "syllable {} of {}{}",
            syllable + 1,
            syllables.min(5),
            if closed { ", closed" } else { ", open" }
        );
        let e = table.entry(key).or_default();
        e.0 += d;
        e.1 += s;
    }
    for (k, (d, s)) in table {
        println!(
            "{k}: aa {d}, a {s} ({:.0}% aa)",
            100.0 * d as f64 / (d + s).max(1) as f64
        );
    }
}

/// Words no lexicon has, spelled each way the options allow: how many come out as the
/// sentence's annotators spelled them. The word lists decide every other word, so this is
/// what the options change.
fn choose(dir: &Path, split: &str, code: &str) {
    let l = lang(code);
    let lex = lexicon(dir, code);
    let mut words: HashMap<String, BTreeMap<String, u32>> = HashMap::new();
    for s in sentences(dir, code, split) {
        for (n, r) in s {
            if scorable(l, &n, &r) && !lex.contains_key(&lookup_key(&n, l)) {
                *words
                    .entry(canonical(&n))
                    .or_default()
                    .entry(r)
                    .or_default() += 1;
            }
        }
    }
    let total: u32 = words.values().flat_map(|v| v.values()).sum();
    let mut line = format!("{code} {split}, {total} words not in the lexicons:");
    for familiar in [true, false] {
        for medial in [true, false] {
            let opts = Options {
                lists: false,
                familiar: Some(familiar),
                medial: Some(medial),
            };
            let ok: u32 = words
                .iter()
                .filter_map(|(n, seen)| seen.get(&word_to_latin_with(n, l, opts)))
                .sum();
            line.push_str(&format!(
                " familiar={familiar} medial={medial}: {:.1}%",
                100.0 * ok as f64 / total.max(1) as f64
            ));
        }
    }
    println!("{line}");
}

/// Scores `tests/fixtures/dakshina/<lang>.<split>.tsv` as `tests/dakshina.rs` does, and prints
/// the `show` commonest misses (`*`: a word no lexicon has).
fn score(crate_dir: &Path, split: &str, code: &str, show: usize) {
    let l = lang(code);
    let path = crate_dir
        .join("tests")
        .join("fixtures")
        .join("dakshina")
        .join(format!("{code}.{split}.tsv"));
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("{}: not made yet (see `fixture`)", path.display());
        return;
    };
    let (mut words, mut any, mut unseen, mut unseen_ok) = (0u32, 0u32, 0u32, 0u32);
    let mut missed: Vec<(u32, String)> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split('\t');
        let (Some(word), Some(count), Some(seen)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let lexicon = f.next().unwrap_or("");
        let count: u32 = count.parse().expect("count");
        let got = word_to_latin(word, l, true);
        let known = seen
            .split(' ')
            .any(|p| p.rsplit_once(':').is_some_and(|(r, _)| r == got))
            || lexicon.split(' ').any(|r| r == got);
        words += count;
        if lexicon.is_empty() {
            unseen += count;
        }
        if known {
            any += count;
            if lexicon.is_empty() {
                unseen_ok += count;
            }
        } else {
            let mark = if lexicon.is_empty() { "*" } else { "" };
            missed.push((
                count,
                format!("{count}\t{mark}{word}\t{got}\t{seen} | {lexicon}"),
            ));
        }
    }
    println!(
        "{code} {split}: {words} words, {:.2}% as some annotator spells them; words no lexicon has: {:.2}% of {unseen}",
        100.0 * any as f64 / words.max(1) as f64,
        100.0 * unseen_ok as f64 / unseen.max(1) as f64
    );
    missed.sort_by_key(|m| std::cmp::Reverse(m.0));
    for (_, m) in missed.iter().take(show) {
        println!("{m}");
    }
}

/// The rules alone on the words of one lexicon split: how many come out spelled as an
/// annotator spelled them. Misses go to `out` as `word\tgot\tspelling:count ...`.
fn rules(dir: &Path, split: &str, code: &str, out: Option<&String>) {
    let l = lang(code);
    let lex = lexicon_of(dir, code, &[split]);
    let (mut ok, mut total) = (0u32, 0u32);
    let mut missed = String::new();
    for (native, variants) in &lex {
        if native.chars().any(|c| c.is_ascii()) {
            continue;
        }
        let got = word_to_latin(native, l, false);
        total += 1;
        if variants.contains_key(&got) {
            ok += 1;
        } else {
            let all: Vec<String> = variants.iter().map(|(r, c)| format!("{r}:{c}")).collect();
            missed.push_str(&format!("{native}\t{got}\t{}\n", all.join(" ")));
        }
    }
    println!(
        "{code} {split} lexicon, rules alone: {ok}/{total} = {:.2}%",
        100.0 * ok as f64 / total.max(1) as f64
    );
    if let Some(path) = out {
        fs::write(path, missed).expect("write misses");
    }
}
