//! Words people spell their own way, looked up before the rules.
//!
//! Two lists per language, the first one found wins:
//! - Venuta's own short list (in [`super::words`]), for the house style of the commonest lyric
//!   words ("mein", "hain", "woh").
//! - The common-word table made from Google's Dakshina romanization lexicons
//!   (`data/dakshina/<lang>.tsv`, CC BY-SA 4.0, credited in `data/dakshina/README.md`): the
//!   most frequent human romanization of each word whose spelling by rule no person used.
//!
//! The tables are sorted by their native-script words and searched in place, with no set-up
//! and no allocation.

use super::Lang;

static HI: &str = include_str!("../../data/dakshina/hi.tsv");
static MR: &str = include_str!("../../data/dakshina/mr.tsv");
static BN: &str = include_str!("../../data/dakshina/bn.tsv");
static GU: &str = include_str!("../../data/dakshina/gu.tsv");
static PA: &str = include_str!("../../data/dakshina/pa.tsv");
static TA: &str = include_str!("../../data/dakshina/ta.tsv");
static TE: &str = include_str!("../../data/dakshina/te.tsv");
static KN: &str = include_str!("../../data/dakshina/kn.tsv");
static ML: &str = include_str!("../../data/dakshina/ml.tsv");

/// The Dakshina table of a language (empty for languages Dakshina does not cover).
pub fn table(lang: Lang) -> &'static str {
    match lang {
        Lang::Hi => HI,
        Lang::Mr => MR,
        Lang::Bn => BN,
        Lang::Gu => GU,
        Lang::Pa => PA,
        Lang::Ta => TA,
        Lang::Te => TE,
        Lang::Kn => KN,
        Lang::Ml => ML,
        Lang::Ne | Lang::Sa | Lang::Or => "",
    }
}

/// The spelling of `word` (NFC, no joiners) in the language's lists.
pub fn lookup(lang: Lang, word: &str) -> Option<&'static str> {
    super::words::house(lang, word).or_else(|| search(table(lang), word))
}

/// Binary search of a sorted `word\tspelling\n` table.
pub fn search(table: &'static str, key: &str) -> Option<&'static str> {
    let bytes = table.as_bytes();
    let key = key.as_bytes();
    // `lo` and `hi` are always at the start of a line.
    let (mut lo, mut hi) = (0usize, bytes.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let start = bytes[lo..mid]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(lo, |p| lo + p + 1);
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |p| start + p);
        let line = &bytes[start..end];
        let tab = line.iter().position(|&b| b == b'\t').unwrap_or(line.len());
        match key.cmp(&line[..tab]) {
            std::cmp::Ordering::Equal => {
                let value = table.get(start + tab + 1..end)?;
                return Some(value.trim_end_matches('\r'));
            }
            std::cmp::Ordering::Less => hi = start,
            std::cmp::Ordering::Greater => lo = end + 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_line_and_nothing_else() {
        let t = "a\tA\nb\tB\nbb\tBB\nc\tC\nख\tkh\n";
        for (k, v) in [
            ("a", "A"),
            ("b", "B"),
            ("bb", "BB"),
            ("c", "C"),
            ("ख", "kh"),
        ] {
            assert_eq!(search(t, k), Some(v), "{k}");
        }
        for k in ["", "0", "ba", "d", "क"] {
            assert_eq!(search(t, k), None, "{k}");
        }
        assert_eq!(search("", "a"), None);
        assert_eq!(search("a\tA", "a"), Some("A"));
    }

    #[test]
    fn the_tables_are_sorted_with_one_spelling_per_word() {
        for lang in Lang::ALL {
            let t = table(lang);
            let mut last = "";
            for line in t.lines() {
                let (k, v) = line
                    .split_once('\t')
                    .unwrap_or_else(|| panic!("{lang:?}: {line}"));
                assert!(k > last, "{lang:?}: {k} after {last}");
                assert!(!v.is_empty() && v.is_ascii(), "{lang:?}: {line}");
                last = k;
            }
            for line in t.lines().step_by(97) {
                let (k, v) = line.split_once('\t').unwrap();
                assert_eq!(search(t, k), Some(v));
            }
        }
    }
}
