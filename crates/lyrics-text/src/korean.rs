//! Korean in Revised Romanization (Ministry of Culture notice 2014-42), with the sound changes
//! between syllables that the rules ask for: liaison, nasal assimilation, ㄹ assimilation,
//! palatalization, aspiration and consonant-cluster simplification. Tensing is not written,
//! as the rules say.
//!
//! The rules are ported from koroman (MIT, (c) 2025 Donghe Youn,
//! <https://github.com/gerosyab/koroman>, `python/koroman/core.py`), applied in its order to
//! the jamo of each word. Unlike koroman each jamo remembers the syllable it was written in,
//! so every syllable comes out as its own piece and keeps its own timing (먹어 is "meo" "geo":
//! the ㄱ is said with the second syllable). Three changes from koroman, each tested:
//!
//! - ㄱ, ㄷ, ㅂ (and ㅅ, ㅊ, ㅌ, which sound as ㄷ) before ㅎ aspirate only when the ㅎ starts
//!   a verb ending or suffix (하, 해, 했, 히, 혀, ...): 생각해 "saenggakae", 잡혀 "japyeo". In
//!   nouns the h is kept, as the rules ask (묵호 "mukho", 집현전 "jiphyeonjeon"). koroman
//!   aspirates everywhere because it has no part of speech; the verb endings are the cheap
//!   part of that.
//! - ㅅ, ㅊ, ㅌ before ㅎ sound as ㅌ (standard pronunciation, article 12): 못해 "motae".
//! - A few verb stems take no inserted ㄴ before 여 (죽여 "jugyeo", not "jungnyeo").
//!
//! Words are romanized one by one: nothing changes across a space, as in the rules. Anything
//! that isn't Hangul passes through as written.

use std::ops::Range;

use crate::pieces::{Piece, Romanized};

/// How to write the line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// A hyphen where the syllables could be read two ways ("jung-ang"): the rules allow it,
    /// lyrics rarely use it. Off by default.
    pub hyphens: bool,
}

/// The BCP 47 tag of the pronunciation line.
pub const LANG: &str = "ko-Latn";

/// A Hangul syllable block (가 to 힣).
pub fn is_syllable(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

/// The line in Revised Romanization, without hyphens.
pub fn romanize(line: &str) -> Romanized {
    romanize_with(line, Options::default())
}

/// The line in Revised Romanization.
pub fn romanize_with(line: &str, opts: Options) -> Romanized {
    let mut out = Romanized::default();
    let mut chars = line.char_indices().peekable();
    while let Some(&(at, c)) = chars.peek() {
        if is_syllable(c) {
            // A word: the syllables in a row.
            let mut syllables = Vec::new();
            while let Some(&(at, c)) = chars.peek() {
                if !is_syllable(c) {
                    break;
                }
                syllables.push((c, at..at + c.len_utf8()));
                chars.next();
            }
            word(&syllables, opts, &mut out);
        } else if c.is_whitespace() {
            out.text.push(c);
            chars.next();
        } else {
            // Anything else up to the next space or syllable, as written (ㅋ as "k").
            let start = out.text.len();
            let mut end = at;
            while let Some(&(at, c)) = chars.peek() {
                if is_syllable(c) || c.is_whitespace() {
                    break;
                }
                match compat_jamo(c) {
                    Some(r) => out.text.push_str(r),
                    None => out.text.push(c),
                }
                end = at + c.len_utf8();
                chars.next();
            }
            out.pieces.push(Piece {
                out: start..out.text.len(),
                src: at..end,
            });
        }
    }
    out
}

// Initial consonants (choseong), U+1100 + index.
const G: char = '\u{1100}';
const GG: char = '\u{1101}';
const N: char = '\u{1102}';
const D: char = '\u{1103}';
const R: char = '\u{1105}';
const M: char = '\u{1106}';
const B: char = '\u{1107}';
const S: char = '\u{1109}';
const SS: char = '\u{110A}';
const O: char = '\u{110B}';
const J: char = '\u{110C}';
const CH: char = '\u{110E}';
const K: char = '\u{110F}';
const T: char = '\u{1110}';
const P: char = '\u{1111}';
const H: char = '\u{1112}';
// Vowels (jungseong), U+1161 + index.
const V_A: char = '\u{1161}';
const V_YA: char = '\u{1163}';
const V_YAE: char = '\u{1164}';
const V_YEO: char = '\u{1167}';
const V_YE: char = '\u{1168}';
const V_O: char = '\u{1169}';
const V_YO: char = '\u{116D}';
const V_YU: char = '\u{1172}';
const V_I: char = '\u{1175}';
// Final consonants (jongseong), U+11A7 + index.
const JG: char = '\u{11A8}';
const JGG: char = '\u{11A9}';
const JGS: char = '\u{11AA}';
const JN: char = '\u{11AB}';
const JNJ: char = '\u{11AC}';
const JNH: char = '\u{11AD}';
const JD: char = '\u{11AE}';
const JL: char = '\u{11AF}';
const JLG: char = '\u{11B0}';
const JLM: char = '\u{11B1}';
const JLB: char = '\u{11B2}';
const JLS: char = '\u{11B3}';
const JLT: char = '\u{11B4}';
const JLP: char = '\u{11B5}';
const JLH: char = '\u{11B6}';
const JM: char = '\u{11B7}';
const JB: char = '\u{11B8}';
const JBS: char = '\u{11B9}';
const JS: char = '\u{11BA}';
const JSS: char = '\u{11BB}';
const JNG: char = '\u{11BC}';
const JJ: char = '\u{11BD}';
const JCH: char = '\u{11BE}';
const JK: char = '\u{11BF}';
const JT: char = '\u{11C0}';
const JP: char = '\u{11C1}';
const JH: char = '\u{11C2}';
/// The two l of ㄹㄹ, written as letters.
const LL: char = 'L';
const HYPHEN: char = '-';

fn is_cho(c: char) -> bool {
    ('\u{1100}'..='\u{1112}').contains(&c)
}
fn is_jong(c: char) -> bool {
    ('\u{11A8}'..='\u{11C2}').contains(&c)
}

/// A jamo and the syllable (index in the word) it was written in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Jamo {
    c: char,
    of: usize,
}

/// What a rule looks for at one position.
#[derive(Clone, Copy)]
enum Is {
    One(char),
    Any(&'static [char]),
}

impl Is {
    fn matches(self, c: char) -> bool {
        match self {
            Is::One(x) => x == c,
            Is::Any(xs) => xs.contains(&c),
        }
    }
}

/// One rewrite: `find` (then `ahead`, not replaced) becomes `put`.
struct Rule {
    find: &'static [Is],
    ahead: Option<Is>,
    put: &'static [char],
}

const fn rule(find: &'static [Is], put: &'static [char]) -> Rule {
    Rule {
        find,
        ahead: None,
        put,
    }
}

const fn rule_ahead(find: &'static [Is], ahead: Is, put: &'static [char]) -> Rule {
    Rule {
        find,
        ahead: Some(ahead),
        put,
    }
}

use Is::{Any, One};

const Y_VOWELS: Is = Any(&[V_YA, V_YAE, V_YEO, V_YE, V_YO, V_YU]);
const NASAL: Is = Any(&[N, M]);
const NOT_SILENT_CHO: Is = Any(&[
    '\u{1100}', '\u{1101}', '\u{1102}', '\u{1103}', '\u{1104}', '\u{1105}', '\u{1106}', '\u{1107}',
    '\u{1108}', '\u{1109}', '\u{110A}', '\u{110C}', '\u{110D}', '\u{110E}', '\u{110F}', '\u{1110}',
    '\u{1111}', '\u{1112}',
]);

/// koroman's rules 2 to 8, in its order: nasal assimilation before ㄴ, ㅁ ...
const NASAL_RULES: &[Rule] = &[
    rule_ahead(&[Any(&[JB, JP, JBS, JLB, JLP])], NASAL, &[JM]),
    rule_ahead(&[Any(&[JD, JT, JJ, JCH, JS, JSS, JH])], NASAL, &[JN]),
    rule_ahead(&[Any(&[JG, JGG, JK, JGS, JLG])], NASAL, &[JNG]),
];

/// ... ㄴ or ㄹ inserted before a y-vowel (학여울 "hangnyeoul", 알약 "allyak") ...
const N_INSERTION: &[Rule] = &[
    rule_ahead(&[One(JG), One(O)], Y_VOWELS, &[JNG, N]),
    rule_ahead(&[One(JL), One(O)], Y_VOWELS, &[JL, R]),
];

/// ... and the rest: ㄹ assimilation, clusters, palatalization, liaison.
const BEFORE_ASPIRATION: &[Rule] = &[
    rule(&[Any(&[JG, JNG]), One(R)], &[JNG, N]),
    rule_ahead(&[One(JN), One(R)], One(V_O), &[JN, N]),
    rule(&[One(JL), One(N)], &[JL, R]),
    rule(&[One(JN), One(R)], &[JL, R]),
    rule(&[Any(&[JM, JB]), One(R)], &[JM, N]),
    rule(&[One(JLG), One(R)], &[JG, R]),
    // The same consonant twice across syllables.
    rule(&[One(JG), One(K)], &[JG, HYPHEN, K]),
    rule(&[One(JB), One(P)], &[JB, HYPHEN, P]),
    rule(&[One(JD), One(T)], &[JD, HYPHEN, T]),
    // A cluster before a vowel: its second consonant moves on (standard pronunciation 14).
    rule(&[One(JGS), One(O)], &[JG, S]),
    rule(&[One(JNJ), One(O)], &[JN, J]),
    rule(&[One(JNH), One(O)], &[JN, O]),
    rule(&[One(JLG), One(O)], &[JL, G]),
    rule(&[One(JLM), One(O)], &[JL, M]),
    rule(&[One(JLB), One(O)], &[JL, B]),
    rule(&[One(JLS), One(O)], &[JL, S]),
    rule(&[One(JLT), One(O)], &[JL, T]),
    rule(&[One(JLP), One(O)], &[JL, P]),
    rule(&[One(JLH), One(O)], &[JL, O]),
    rule(&[One(JBS), One(O)], &[JB, S]),
    // 밟- sounds ㅂ before a consonant (standard pronunciation 10).
    rule_ahead(&[One(B), One(V_A), One(JLB)], NOT_SILENT_CHO, &[B, V_A, JB]),
    // Clusters: the consonant that is said first.
    rule(&[One(JGS)], &[JG, JS]),
    rule(&[One(JNJ)], &[JN, JJ]),
    rule(&[One(JNH)], &[JN, JH]),
    rule(&[One(JLG)], &[JG, JL]),
    rule(&[One(JLM)], &[JM, JL]),
    rule(&[One(JLB)], &[JL, JB]),
    rule(&[One(JLS)], &[JL, JS]),
    rule(&[One(JLT)], &[JL, JT]),
    rule(&[One(JLP)], &[JP, JL]),
    rule(&[One(JLH)], &[JL, JH]),
    rule(&[One(JBS)], &[JB, JS]),
    // Palatalization.
    rule(&[One(JD), One(O), One(V_I)], &[J, V_I]),
    rule(&[One(JT), One(O), One(V_I)], &[CH, V_I]),
    rule(&[One(JD), One(H), One(V_I)], &[CH, V_I]),
    // Liaison: a final consonant before a vowel starts the next syllable.
    rule(&[One(JG), One(O)], &[G]),
    rule(&[One(JGG), One(O)], &[GG]),
    rule(&[One(JD), One(O)], &[D]),
    rule(&[One(JL), One(O)], &[R]),
    rule(&[One(JB), One(O)], &[B]),
    rule(&[One(JS), One(O)], &[S]),
    rule(&[One(JSS), One(O)], &[SS]),
    rule(&[One(JJ), One(O)], &[J]),
    rule(&[One(JCH), One(O)], &[CH]),
    rule(&[One(JH), One(O)], &[]),
];

/// ㅎ then a consonant: always aspirated (좋고 "joko").
const H_FIRST: &[Rule] = &[
    rule(&[One(JH), One(G)], &[K]),
    rule(&[One(JH), One(D)], &[T]),
    rule(&[One(JH), One(J)], &[CH]),
    rule(&[One(JH), One(B)], &[B]),
];

/// A consonant then ㅎ: aspirated in verbs (see [`verb_h`]); ㅈ always.
const H_SECOND: &[Rule] = &[
    rule(&[One(JG), One(H)], &[K]),
    rule(&[One(JD), One(H)], &[T]),
    rule(&[Any(&[JS, JCH, JT]), One(H)], &[T]),
    rule(&[One(JB), One(H)], &[P]),
];
const JJ_H: Rule = rule(&[One(JJ), One(H)], &[CH]);

/// Syllables starting with ㅎ that begin a verb ending or suffix: after ㄱ, ㄷ, ㅂ these
/// aspirate (생각해), elsewhere the ㅎ is a noun's and is kept (묵호).
fn verb_h(syllable: char) -> bool {
    matches!(
        syllable,
        '하' | '해' | '했' | '한' | '할' | '함' | '합' | '히' | '혀' | '혔' | '힌' | '힐'
    )
}

/// Verb stems that take 여 (a contracted 이어) with no ㄴ inserted.
fn no_n_insertion(stem: char, next: char) -> bool {
    matches!(stem, '죽' | '속' | '녹' | '먹') && matches!(next, '여' | '였')
}

fn decompose(c: char) -> [Option<char>; 3] {
    let n = c as u32 - 0xAC00;
    let cho = char::from_u32(0x1100 + n / (21 * 28));
    let jung = char::from_u32(0x1161 + (n % (21 * 28)) / 28);
    let jong = n % 28;
    let jong = (jong != 0).then(|| char::from_u32(0x11A7 + jong)).flatten();
    [cho, jung, jong]
}

/// Applies a rule left to right, without overlaps, as `re.sub` does. A final consonant put
/// back belongs to the syllable the match started in; anything else to where it ended.
fn apply(rule: &Rule, seq: &mut Vec<Jamo>, skip: &dyn Fn(&[Jamo]) -> bool) {
    let n = rule.find.len();
    let mut i = 0;
    while i + n <= seq.len() {
        let hit = rule
            .find
            .iter()
            .zip(&seq[i..i + n])
            .all(|(is, j)| is.matches(j.c))
            && rule
                .ahead
                .is_none_or(|a| seq.get(i + n).is_some_and(|j| a.matches(j.c)))
            && !skip(&seq[i..]);
        if !hit {
            i += 1;
            continue;
        }
        let first = seq[i].of;
        let last = seq[i + n - 1].of;
        let put: Vec<Jamo> = rule
            .put
            .iter()
            .enumerate()
            .map(|(k, &c)| Jamo {
                c,
                of: if is_jong(c) || (c == LL && k == 0) {
                    first
                } else {
                    last
                },
            })
            .collect();
        let len = put.len();
        seq.splice(i..i + n, put);
        i += len;
    }
}

fn word(syllables: &[(char, Range<usize>)], opts: Options, out: &mut Romanized) {
    let mut seq: Vec<Jamo> = Vec::with_capacity(syllables.len() * 3);
    for (of, &(c, _)) in syllables.iter().enumerate() {
        let mut parts = decompose(c);
        // ㄴ inserted before 잎 (꽃잎 "kkonnip", koroman's one compound trigger).
        if c == '잎' && of > 0 && decompose(syllables[of - 1].0)[2].is_some() {
            parts[0] = Some(N);
        }
        seq.extend(parts.into_iter().flatten().map(|c| Jamo { c, of }));
    }
    let syllable = |of: usize| syllables[of].0;
    let none = |_: &[Jamo]| false;
    for r in NASAL_RULES {
        apply(r, &mut seq, &none);
    }
    for r in N_INSERTION {
        // Not after the verb stems that refuse it.
        apply(r, &mut seq, &|at: &[Jamo]| {
            at[0].of + 1 < syllables.len()
                && no_n_insertion(syllable(at[0].of), syllable(at[0].of + 1))
        });
    }
    for r in BEFORE_ASPIRATION {
        apply(r, &mut seq, &none);
    }
    for r in H_FIRST {
        apply(r, &mut seq, &none);
    }
    for r in H_SECOND {
        apply(r, &mut seq, &|at: &[Jamo]| !verb_h(syllable(at[1].of)));
    }
    apply(&JJ_H, &mut seq, &none);
    apply(&rule(&[One(JL), One(R)], &[LL, LL]), &mut seq, &none);
    // A ㅎ left before a consonant is not said; at the end of a word it is "t".
    let mut i = 0;
    while i < seq.len() {
        if seq[i].c == JH && seq.get(i + 1).is_some_and(|j| is_cho(j.c)) {
            seq.remove(i);
        } else {
            i += 1;
        }
    }
    // Of two final consonants left together, the first is said.
    let mut i = 1;
    while i < seq.len() {
        if is_jong(seq[i].c) && is_jong(seq[i - 1].c) {
            seq.remove(i);
        } else {
            i += 1;
        }
    }

    // Each syllable's letters as one piece.
    let mut at = 0;
    for of in 0..syllables.len() {
        let start = out.text.len();
        while at < seq.len() && seq[at].of == of {
            out.text.push_str(roman(seq[at].c));
            at += 1;
        }
        if opts.hyphens && of + 1 < syllables.len() && needs_hyphen(&seq, at) {
            out.text.push('-');
        }
        if out.text.len() > start {
            out.pieces.push(Piece {
                out: start..out.text.len(),
                src: syllables[of].1.clone(),
            });
        }
    }
}

/// koroman's three places where a hyphen keeps two readings apart: ㅇ then a vowel
/// (jung-ang), ㄴ then ㄱ (ban-gudae), a vowel then a vowel (hae-undae). `at` is where the next
/// syllable starts in `seq`.
fn needs_hyphen(seq: &[Jamo], at: usize) -> bool {
    let (Some(prev), Some(next)) = (at.checked_sub(1).map(|i| seq[i].c), seq.get(at)) else {
        return false;
    };
    if !is_cho(next.c) {
        return false;
    }
    let vowel = ('\u{1161}'..='\u{1175}').contains(&prev);
    (prev == JNG && next.c == O) || (prev == JN && next.c == G) || (vowel && next.c == O)
}

fn roman(c: char) -> &'static str {
    match c {
        '\u{1100}' => "g",
        '\u{1101}' => "kk",
        '\u{1102}' => "n",
        '\u{1103}' => "d",
        '\u{1104}' => "tt",
        '\u{1105}' => "r",
        '\u{1106}' => "m",
        '\u{1107}' => "b",
        '\u{1108}' => "pp",
        '\u{1109}' => "s",
        '\u{110A}' => "ss",
        '\u{110B}' => "",
        '\u{110C}' => "j",
        '\u{110D}' => "jj",
        '\u{110E}' => "ch",
        '\u{110F}' => "k",
        '\u{1110}' => "t",
        '\u{1111}' => "p",
        '\u{1112}' => "h",
        '\u{1161}' => "a",
        '\u{1162}' => "ae",
        '\u{1163}' => "ya",
        '\u{1164}' => "yae",
        '\u{1165}' => "eo",
        '\u{1166}' => "e",
        '\u{1167}' => "yeo",
        '\u{1168}' => "ye",
        '\u{1169}' => "o",
        '\u{116A}' => "wa",
        '\u{116B}' => "wae",
        '\u{116C}' => "oe",
        '\u{116D}' => "yo",
        '\u{116E}' => "u",
        '\u{116F}' => "wo",
        '\u{1170}' => "we",
        '\u{1171}' => "wi",
        '\u{1172}' => "yu",
        '\u{1173}' => "eu",
        '\u{1174}' => "ui",
        '\u{1175}' => "i",
        JG | JGG | JGS | JLG | JK => "k",
        JN | JNJ | JNH => "n",
        JD | JLS | JLT | JS | JSS | JJ | JCH | JT | JH => "t",
        JL | JLH => "l",
        JLM | JM => "m",
        JLB | JLP | JB | JBS | JP => "p",
        JNG => "ng",
        LL => "l",
        HYPHEN => "-",
        _ => "",
    }
}

/// A Hangul letter written on its own (ㅋㅋ, ㅠㅠ) as its sound.
fn compat_jamo(c: char) -> Option<&'static str> {
    Some(match c {
        'ㄱ' => "g",
        'ㄲ' => "kk",
        'ㄴ' => "n",
        'ㄷ' => "d",
        'ㄸ' => "tt",
        'ㄹ' => "r",
        'ㅁ' => "m",
        'ㅂ' => "b",
        'ㅃ' => "pp",
        'ㅅ' => "s",
        'ㅆ' => "ss",
        'ㅇ' => "ng",
        'ㅈ' => "j",
        'ㅉ' => "jj",
        'ㅊ' => "ch",
        'ㅋ' => "k",
        'ㅌ' => "t",
        'ㅍ' => "p",
        'ㅎ' => "h",
        'ㅏ' => "a",
        'ㅐ' => "ae",
        'ㅑ' => "ya",
        'ㅒ' => "yae",
        'ㅓ' => "eo",
        'ㅔ' => "e",
        'ㅕ' => "yeo",
        'ㅖ' => "ye",
        'ㅗ' => "o",
        'ㅘ' => "wa",
        'ㅙ' => "wae",
        'ㅚ' => "oe",
        'ㅛ' => "yo",
        'ㅜ' => "u",
        'ㅝ' => "wo",
        'ㅞ' => "we",
        'ㅟ' => "wi",
        'ㅠ' => "yu",
        'ㅡ' => "eu",
        'ㅢ' => "ui",
        'ㅣ' => "i",
        _ => return None,
    })
}
