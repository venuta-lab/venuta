//! Kana in Hepburn romaji: し "shi", ち "chi", つ "tsu", ふ "fu", じ "ji", small っ doubling
//! the next consonant ("matcha", "kitte"), ん "n" (with an apostrophe before a vowel or y:
//! "kin'en"), を "o". Long vowels are written as the kana write them ("toukyou", "haato" for
//! ハート), the way lyric romaji usually is, so every kana keeps a letter of its own.

use std::ops::Range;

use crate::pieces::Builder;

/// One kana of a word and the characters of the line it stands for (a kanji's reading: every
/// kana of it points at the kanji).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kana {
    pub c: char,
    pub src: Range<usize>,
}

/// A katakana as its hiragana (ヴ as ゔ); anything else as it is.
pub(crate) fn hiragana(c: char) -> char {
    match c as u32 {
        0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        0x30FD | 0x30FE => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

/// One kana's romaji.
fn single(c: char) -> Option<&'static str> {
    Some(match c {
        'あ' => "a",
        'い' => "i",
        'う' => "u",
        'え' => "e",
        'お' => "o",
        'か' => "ka",
        'き' => "ki",
        'く' => "ku",
        'け' => "ke",
        'こ' => "ko",
        'が' => "ga",
        'ぎ' => "gi",
        'ぐ' => "gu",
        'げ' => "ge",
        'ご' => "go",
        'さ' => "sa",
        'し' => "shi",
        'す' => "su",
        'せ' => "se",
        'そ' => "so",
        'ざ' => "za",
        'じ' => "ji",
        'ず' => "zu",
        'ぜ' => "ze",
        'ぞ' => "zo",
        'た' => "ta",
        'ち' => "chi",
        'つ' => "tsu",
        'て' => "te",
        'と' => "to",
        'だ' => "da",
        'ぢ' => "ji",
        'づ' => "zu",
        'で' => "de",
        'ど' => "do",
        'な' => "na",
        'に' => "ni",
        'ぬ' => "nu",
        'ね' => "ne",
        'の' => "no",
        'は' => "ha",
        'ひ' => "hi",
        'ふ' => "fu",
        'へ' => "he",
        'ほ' => "ho",
        'ば' => "ba",
        'び' => "bi",
        'ぶ' => "bu",
        'べ' => "be",
        'ぼ' => "bo",
        'ぱ' => "pa",
        'ぴ' => "pi",
        'ぷ' => "pu",
        'ぺ' => "pe",
        'ぽ' => "po",
        'ま' => "ma",
        'み' => "mi",
        'む' => "mu",
        'め' => "me",
        'も' => "mo",
        'や' => "ya",
        'ゆ' => "yu",
        'よ' => "yo",
        'ら' => "ra",
        'り' => "ri",
        'る' => "ru",
        'れ' => "re",
        'ろ' => "ro",
        'わ' => "wa",
        'ゐ' => "i",
        'ゑ' => "e",
        'を' => "o",
        'ん' => "n",
        'ゔ' => "vu",
        'ぁ' => "a",
        'ぃ' => "i",
        'ぅ' => "u",
        'ぇ' => "e",
        'ぉ' => "o",
        'ゃ' => "ya",
        'ゅ' => "yu",
        'ょ' => "yo",
        'ゎ' => "wa",
        'ゕ' => "ka",
        'ゖ' => "ke",
        // Katakana with no hiragana: ヷ ヸ ヹ ヺ.
        'ヷ' => "va",
        'ヸ' => "vi",
        'ヹ' => "ve",
        'ヺ' => "vo",
        _ => return None,
    })
}

/// Two kana read as one sound: きゃ "kya", しゃ "sha", ファ "fa", ティ "ti".
fn pair(a: char, b: char) -> Option<&'static str> {
    Some(match (a, b) {
        ('き', 'ゃ') => "kya",
        ('き', 'ゅ') => "kyu",
        ('き', 'ょ') => "kyo",
        ('ぎ', 'ゃ') => "gya",
        ('ぎ', 'ゅ') => "gyu",
        ('ぎ', 'ょ') => "gyo",
        ('し', 'ゃ') => "sha",
        ('し', 'ゅ') => "shu",
        ('し', 'ょ') => "sho",
        ('し', 'ぇ') => "she",
        ('じ', 'ゃ') | ('ぢ', 'ゃ') => "ja",
        ('じ', 'ゅ') | ('ぢ', 'ゅ') => "ju",
        ('じ', 'ょ') | ('ぢ', 'ょ') => "jo",
        ('じ', 'ぇ') => "je",
        ('ち', 'ゃ') => "cha",
        ('ち', 'ゅ') => "chu",
        ('ち', 'ょ') => "cho",
        ('ち', 'ぇ') => "che",
        ('に', 'ゃ') => "nya",
        ('に', 'ゅ') => "nyu",
        ('に', 'ょ') => "nyo",
        ('ひ', 'ゃ') => "hya",
        ('ひ', 'ゅ') => "hyu",
        ('ひ', 'ょ') => "hyo",
        ('び', 'ゃ') => "bya",
        ('び', 'ゅ') => "byu",
        ('び', 'ょ') => "byo",
        ('ぴ', 'ゃ') => "pya",
        ('ぴ', 'ゅ') => "pyu",
        ('ぴ', 'ょ') => "pyo",
        ('み', 'ゃ') => "mya",
        ('み', 'ゅ') => "myu",
        ('み', 'ょ') => "myo",
        ('り', 'ゃ') => "rya",
        ('り', 'ゅ') => "ryu",
        ('り', 'ょ') => "ryo",
        ('ふ', 'ぁ') => "fa",
        ('ふ', 'ぃ') => "fi",
        ('ふ', 'ぇ') => "fe",
        ('ふ', 'ぉ') => "fo",
        ('ふ', 'ゅ') => "fyu",
        ('て', 'ぃ') => "ti",
        ('て', 'ゅ') => "tyu",
        ('で', 'ぃ') => "di",
        ('で', 'ゅ') => "dyu",
        ('と', 'ぅ') => "tu",
        ('ど', 'ぅ') => "du",
        ('う', 'ぃ') => "wi",
        ('う', 'ぇ') => "we",
        ('う', 'ぉ') => "wo",
        ('い', 'ぇ') => "ye",
        ('つ', 'ぁ') => "tsa",
        ('つ', 'ぃ') => "tsi",
        ('つ', 'ぇ') => "tse",
        ('つ', 'ぉ') => "tso",
        ('く', 'ぁ') => "kwa",
        ('ぐ', 'ぁ') => "gwa",
        ('ゔ', 'ぁ') => "va",
        ('ゔ', 'ぃ') => "vi",
        ('ゔ', 'ぇ') => "ve",
        ('ゔ', 'ぉ') => "vo",
        ('ゔ', 'ゅ') => "vyu",
        _ => return None,
    })
}

/// The sound at `i` and how many kana it takes.
fn sound(kana: &[Kana], i: usize) -> Option<(&'static str, usize)> {
    let c = kana[i].c;
    if let Some(next) = kana.get(i + 1)
        && let Some(s) = pair(c, next.c)
    {
        return Some((s, 2));
    }
    single(c).map(|s| (s, 1))
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

/// Writes one word's kana into `b`, each sound as a piece pointing at its source.
pub(crate) fn write_word(kana: &[Kana], b: &mut Builder) {
    let mut last_vowel: Option<char> = None;
    let mut previous: Option<&'static str> = None;
    let mut i = 0;
    while i < kana.len() {
        let k = &kana[i];
        match k.c {
            'っ' => {
                // The next sound's first consonant, said twice ("tch" before "ch").
                let next = (i + 1 < kana.len()).then(|| sound(kana, i + 1)).flatten();
                if let Some((s, _)) = next
                    && let Some(f) = s.chars().next()
                    && !is_vowel(f)
                    && f != 'n'
                {
                    b.push(
                        if s.starts_with("ch") { "t" } else { &s[..1] },
                        k.src.clone(),
                    );
                }
                i += 1;
            }
            'ー' => {
                if let Some(v) = last_vowel {
                    b.push(v.encode_utf8(&mut [0; 4]), k.src.clone());
                }
                i += 1;
            }
            'ゝ' | 'ゞ' => {
                if let Some(s) = previous {
                    b.push(s, k.src.clone());
                }
                i += 1;
            }
            _ => match sound(kana, i) {
                Some((s, n)) => {
                    let src = k.src.start..kana[i + n - 1].src.end;
                    b.push(s, src);
                    if s == "n"
                        && let Some((next, _)) =
                            (i + n < kana.len()).then(|| sound(kana, i + n)).flatten()
                        && next.starts_with(['a', 'i', 'u', 'e', 'o', 'y'])
                    {
                        b.push("'", k.src.clone());
                    }
                    last_vowel = s.chars().last().filter(|&c| is_vowel(c));
                    previous = Some(s);
                    i += n;
                }
                None => {
                    // Not a kana this table knows: as written.
                    b.push(k.c.encode_utf8(&mut [0; 4]), k.src.clone());
                    i += 1;
                }
            },
        }
    }
}
