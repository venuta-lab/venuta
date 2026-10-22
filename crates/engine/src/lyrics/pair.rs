//! Pairing two copies of a song: one in its own script (Devanagari from LRCLIB) and one in
//! Latin letters (Apple's romanized Hindi, through LyricsPlus). The Latin copy's lines become
//! the original's pronunciation, line by line, matched by time.
//!
//! The Latin copy has to earn it, so a translation or another song is never taken for a
//! pronunciation:
//! - it is mostly in Latin letters, and has about as many lines (Apple splits some lines in
//!   two, so up to three times as many, and at least half as many);
//! - its lines say what the original says: each is compared with the pronunciation Venuta
//!   makes of the original line ([`similarity`]: their consonants, in order, since the vowels
//!   are what people spell every which way). A translation shares almost none;
//! - the times agree: the two copies' offset is read from the lines that match best, and each
//!   line then pairs only with lines starting within 1.5 s of where it starts;
//! - and the song as a whole agrees: at least three lines and half of the lines compared pair,
//!   and the typical pair is close.
//!
//! A line may pair with up to three of the other copy's lines in a row (Apple's halves of one
//! line). A line with no made pronunciation to compare (a script Venuta can't read yet) pairs
//! by time alone, but only in a song the other lines have shown to be the same. Nothing here
//! reads the network.

use super::{LyricLine, Second, SecondSource, Word, roman_lang};

/// How a pairing went: lines paired, of the lines in another script that could take one, and
/// the typical (median) closeness of the pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct Pairing {
    pub paired: usize,
    pub of: usize,
    pub similarity: f32,
}

/// A line pairs with lines starting at most this far from it (after the offset).
const WITHIN_MS: i64 = 1_500;
/// The offset between the copies is looked for this far.
const OFFSET_MS: i64 = 5_000;
/// A pair at least this close counts.
const LINE_MIN: f32 = 0.6;
/// Lines this close tell the offset.
const ANCHOR_MIN: f32 = 0.75;
/// The typical pair at least this close, for the song to pair at all.
const SONG_MIN: f32 = 0.75;
/// At least this many lines pair, and at least half of the lines compared.
const LINES_MIN: usize = 3;

/// Gives each line of `lines` in another script (with no pronunciation, or only one made on
/// the device) the line(s) of `other` that say it in Latin letters, as its pronunciation from
/// `provider`, when the two copies are the same song (see the module notes). Nothing changes
/// when they aren't.
pub fn pair_roman(
    lines: &mut [LyricLine],
    song_lang: Option<&str>,
    other: &[LyricLine],
    provider: &str,
) -> Option<Pairing> {
    let wanted: Vec<usize> = (0..lines.len())
        .filter(|&i| {
            let l = &lines[i];
            !matches!(l.script.as_str(), "Latn" | "Zyyy" | "")
                && !l.text.trim().is_empty()
                && l.roman
                    .as_ref()
                    .is_none_or(|r| r.source == SecondSource::Made)
        })
        .collect();
    if wanted.len() < LINES_MIN {
        return None;
    }
    // The other copy: mostly Latin letters, about as many lines.
    let lettered = |ls: &[LyricLine]| {
        ls.iter()
            .filter(|l| !matches!(l.script.as_str(), "Zyyy" | ""))
            .count()
    };
    let latin_lines: Vec<&LyricLine> = other.iter().filter(|l| l.script == "Latn").collect();
    let latin = latin_lines.as_slice();
    let (n, m) = (lettered(lines), latin.len());
    if m < LINES_MIN || m * 10 < lettered(other) * 6 || m * 2 < n || m > n * 3 {
        return None;
    }
    // What each wanted line says, as Venuta would spell it.
    let made: Vec<Option<String>> = wanted
        .iter()
        .map(|&i| {
            let l = &lines[i];
            let text = match &l.roman {
                Some(r) => Some(r.text.clone()),
                None => lyrics_text::romanize(&lyrics_text::Line::new(&l.text).lang(song_lang))
                    .map(|r| r.text),
            };
            text.filter(|t| skeleton(t).len() >= 3)
        })
        .collect();

    // The offset: where the closest lines are, the median of their distances.
    let mut shifts: Vec<i64> = Vec::new();
    for (k, &i) in wanted.iter().enumerate() {
        let Some(said) = &made[k] else { continue };
        let at = lines[i].start_ms as i64;
        let best = latin
            .iter()
            .filter(|o| (o.start_ms as i64 - at).abs() <= OFFSET_MS)
            .map(|o| (similarity(said, &o.text), o.start_ms as i64 - at))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((s, d)) = best.filter(|(s, _)| *s >= ANCHOR_MIN) {
            shifts.push(d);
        }
    }
    if shifts.len() < LINES_MIN {
        return None;
    }
    shifts.sort_unstable();
    let shift = shifts[shifts.len() / 2];

    // Each line in order, with the best run of up to three lines near it, in order.
    let mut pairs: Vec<(usize, usize, usize, Option<f32>)> = Vec::new();
    let mut next = 0;
    for (k, &i) in wanted.iter().enumerate() {
        let l = &lines[i];
        let at = l.start_ms as i64 + shift;
        let ends = l.end_ms.max(l.start_ms + 1) as i64 + shift;
        let starts: Vec<usize> = (next..latin.len())
            .filter(|&j| (latin[j].start_ms as i64 - at).abs() <= WITHIN_MS)
            .collect();
        let pick = match &made[k] {
            Some(said) => starts
                .iter()
                .flat_map(|&j| {
                    (j..latin.len().min(j + 3))
                        .take_while(move |&e| e == j || (latin[e].start_ms as i64) < ends)
                        .map(move |e| (j, e))
                })
                .map(|(j, e)| {
                    let joined = latin[j..=e]
                        .iter()
                        .map(|o| o.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    (j, e, similarity(said, &joined))
                })
                // The closest; of equals, the fewest lines.
                .max_by(|a, b| a.2.total_cmp(&b.2).then(b.1.cmp(&a.1)))
                .filter(|(.., s)| *s >= LINE_MIN)
                .map(|(j, e, s)| (j, e, Some(s))),
            // Nothing to compare: the nearest line, close in time.
            None => starts
                .iter()
                .copied()
                .filter(|&j| (latin[j].start_ms as i64 - at).abs() <= WITHIN_MS / 2)
                .min_by_key(|&j| (latin[j].start_ms as i64 - at).abs())
                .map(|j| (j, j, None)),
        };
        if let Some((j, e, s)) = pick {
            pairs.push((i, j, e, s));
            next = e + 1;
        }
    }

    // The song as a whole.
    let mut close: Vec<f32> = pairs.iter().filter_map(|p| p.3).collect();
    let compared = made.iter().filter(|m| m.is_some()).count();
    if close.len() < LINES_MIN || close.len() * 2 < compared {
        return None;
    }
    close.sort_by(f32::total_cmp);
    let typical = close[close.len() / 2];
    if typical < SONG_MIN {
        return None;
    }

    for &(i, j, e, _) in &pairs {
        let made_lang = lines[i]
            .roman
            .as_ref()
            .filter(|r| r.source == SecondSource::Made)
            .map(|r| r.lang.clone());
        let l = &lines[i];
        let lang = made_lang.unwrap_or_else(|| roman_lang(song_lang, &l.script));
        let parts = &latin[j..=e];
        let text = parts
            .iter()
            .map(|o| o.text.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let words = moved_words(parts, l);
        lines[i].roman = Some(Second {
            text,
            words,
            lang,
            source: SecondSource::Paired(provider.to_owned()),
        });
    }
    Some(Pairing {
        paired: pairs.len(),
        of: wanted.len(),
        similarity: typical,
    })
}

/// The other copy's words for a line, moved to where this copy starts it, within the line;
/// none unless every part has its words.
fn moved_words(parts: &[&LyricLine], line: &LyricLine) -> Vec<Word> {
    if parts.iter().any(|p| p.words.is_empty()) {
        return Vec::new();
    }
    let shift = line.start_ms as i64 - parts[0].start_ms as i64;
    let end = line.end_ms.max(line.start_ms);
    let moved = |ms: u64| ((ms as i64 + shift).max(0) as u64).clamp(line.start_ms, end);
    let mut out: Vec<Word> = Vec::new();
    for (k, p) in parts.iter().enumerate() {
        let n = p.words.len();
        for (w_at, w) in p.words.iter().enumerate() {
            let mut text = w.text.clone();
            if w_at + 1 == n {
                text = text.trim_end().to_owned();
                if k + 1 < parts.len() {
                    text.push(' ');
                }
            }
            if k == 0 && w_at == 0 {
                text = text.trim_start().to_owned();
            }
            out.push(Word {
                start_ms: moved(w.start_ms),
                end_ms: moved(w.end_ms.max(w.start_ms)),
                text,
            });
        }
    }
    out
}

/// A Latin-letter line reduced to how it sounds, as different spellings of the same words
/// agree on it: lower case, its consonants in order (vowels, y and h dropped: the schwa, the
/// long vowels, the glides and the aspiration are written every which way: "waade" and
/// "vaade", "kiye" and "kie", "reh" and "rah"), w as v, z as j, q as k, f as p, and a doubled
/// letter once.
fn skeleton(s: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let b = match c {
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' => continue,
            'w' => b'v',
            'z' => b'j',
            'q' => b'k',
            'f' => b'p',
            'x' => b'k',
            c if c.is_ascii_lowercase() => c as u8,
            _ => continue,
        };
        if out.last() != Some(&b) {
            out.push(b);
        }
    }
    out
}

/// How alike two Latin-letter lines sound, 0 to 1: one less the edit distance between their
/// [`skeleton`]s over the longer one.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (skeleton(a), skeleton(b));
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 0.0;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = (diag + usize::from(x != y)).min(row[j] + 1).min(up + 1);
            diag = up;
        }
    }
    1.0 - row[b.len()] as f32 / longest as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_the_same_words_are_alike_and_a_translation_is_not() {
        // Venuta's spelling and Apple's.
        assert!(similarity("Mujhko itna bataae koi", "Mujhko itna bataye koyi") > 0.95);
        assert!(
            similarity(
                "Jo vaade kie saare bas baaton mein rah gae",
                "Jo waade kiye saare bas baaton mein reh gaye"
            ) > 0.95
        );
        assert!(
            similarity(
                "kar di hain husn ki khaali tijoriyaan",
                "Kardi hain husn ki khaali tijoriyan"
            ) > 0.9
        );
        // A translation of the same line, and another line of the song.
        assert!(similarity("Mujhko itna bataae koi", "Somebody just tell me this much") < 0.4);
        assert!(similarity("Mujhko itna bataae koi", "Kaise tujhse dil na lagaye koyi") < 0.5);
        assert_eq!(similarity("", "anything"), 0.0);
        assert_eq!(skeleton("Achchha Kuchh"), b"ckc".to_vec());
    }
}
