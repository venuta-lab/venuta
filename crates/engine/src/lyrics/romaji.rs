//! Roman letters that a lyrics source sends syllable by syllable ("ka n pe ki de u so tsu ki
//! na ki mi wa"), joined into words ("kanpeki de usotsukina kimi wa").
//!
//! Japanese and Korean only: their syllables are lined up with the original line (each kana
//! is one syllable, a run of kanji takes one or more, each Hangul block is one), and words are
//! then cut where people cut them: at spaces, where the writing changes from kana to kanji or
//! katakana, around を, and around the particles (の, は, が, に, で, と, も, へ) after a kanji or
//! katakana word. Chinese romanization is written syllable by syllable anyway, so it is kept as
//! sent. This is a rule of thumb, not a dictionary: some Japanese words stay joined
//! ("kyounanitabeta"), but no syllable is lost or changed.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Kanji,
    Hira,
    Kata,
    Hangul,
    Latin,
    /// Spaces and punctuation: a word ends here.
    Gap,
}

/// A piece of the original line and, once lined up, its Roman letters.
#[derive(Debug, Clone)]
struct Piece {
    kind: Kind,
    text: String,
    /// What a kana reads as (any one syllable when empty); unused for the other kinds.
    readings: Vec<String>,
    /// A small っ or a ー on its own: may take a syllable or none.
    optional: bool,
    roman: String,
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
        || matches!(c, '々' | '〆' | 'ヶ' | 'ヵ')
}
fn is_hira(c: char) -> bool {
    matches!(c as u32, 0x3041..=0x3096 | 0x309D..=0x309F)
}
fn is_kata(c: char) -> bool {
    matches!(c as u32, 0x30A1..=0x30FA | 0x30FD..=0x30FF | 0x31F0..=0x31FF)
}
fn is_hangul(c: char) -> bool {
    matches!(c as u32, 0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F)
}

/// A katakana as its hiragana.
fn hira(c: char) -> char {
    if matches!(c as u32, 0x30A1..=0x30F6) {
        char::from_u32(c as u32 - 0x60).unwrap_or(c)
    } else {
        c
    }
}

/// A small kana's sound: ゃ "ya", ぁ "a".
fn small(c: char) -> Option<&'static str> {
    Some(match hira(c) {
        'ゃ' => "ya",
        'ゅ' => "yu",
        'ょ' => "yo",
        'ぁ' => "a",
        'ぃ' => "i",
        'ぅ' => "u",
        'ぇ' => "e",
        'ぉ' => "o",
        'ゎ' => "wa",
        _ => return None,
    })
}

/// How a kana is written in Roman letters (Hepburn first, then the other ways sources write
/// it).
fn base(c: char) -> &'static [&'static str] {
    match hira(c) {
        'あ' => &["a"],
        'い' => &["i"],
        'う' => &["u"],
        'え' => &["e"],
        'お' => &["o"],
        'か' | 'ゕ' => &["ka"],
        'き' => &["ki"],
        'く' => &["ku"],
        'け' | 'ゖ' => &["ke"],
        'こ' => &["ko"],
        'が' => &["ga"],
        'ぎ' => &["gi"],
        'ぐ' => &["gu"],
        'げ' => &["ge"],
        'ご' => &["go"],
        'さ' => &["sa"],
        'し' => &["shi", "si"],
        'す' => &["su"],
        'せ' => &["se"],
        'そ' => &["so"],
        'ざ' => &["za"],
        'じ' => &["ji", "zi"],
        'ず' => &["zu"],
        'ぜ' => &["ze"],
        'ぞ' => &["zo"],
        'た' => &["ta"],
        'ち' => &["chi", "ti"],
        'つ' => &["tsu", "tu"],
        'て' => &["te"],
        'と' => &["to"],
        'だ' => &["da"],
        'ぢ' => &["ji", "di"],
        'づ' => &["zu", "du"],
        'で' => &["de"],
        'ど' => &["do"],
        'な' => &["na"],
        'に' => &["ni"],
        'ぬ' => &["nu"],
        'ね' => &["ne"],
        'の' => &["no"],
        'は' => &["ha", "wa"],
        'ひ' => &["hi"],
        'ふ' => &["fu", "hu"],
        'へ' => &["he", "e"],
        'ほ' => &["ho"],
        'ば' => &["ba"],
        'び' => &["bi"],
        'ぶ' => &["bu"],
        'べ' => &["be"],
        'ぼ' => &["bo"],
        'ぱ' => &["pa"],
        'ぴ' => &["pi"],
        'ぷ' => &["pu"],
        'ぺ' => &["pe"],
        'ぽ' => &["po"],
        'ま' => &["ma"],
        'み' => &["mi"],
        'む' => &["mu"],
        'め' => &["me"],
        'も' => &["mo"],
        'や' => &["ya"],
        'ゆ' => &["yu"],
        'よ' => &["yo"],
        'ら' => &["ra"],
        'り' => &["ri"],
        'る' => &["ru"],
        'れ' => &["re"],
        'ろ' => &["ro"],
        'わ' => &["wa"],
        'ゐ' => &["wi", "i"],
        'ゑ' => &["we", "e"],
        'を' => &["wo", "o"],
        'ん' => &["n", "nn", "m"],
        'ゔ' => &["vu"],
        'ぁ' => &["a"],
        'ぃ' => &["i"],
        'ぅ' => &["u"],
        'ぇ' => &["e"],
        'ぉ' => &["o"],
        'ゃ' => &["ya"],
        'ゅ' => &["yu"],
        'ょ' => &["yo"],
        'ゎ' => &["wa"],
        _ => &[],
    }
}

const VOWELS: &[char] = &['a', 'i', 'u', 'e', 'o'];

/// A kana with its small kana after it: き + ょ "kyo", デ + ィ "di", フ + ァ "fa".
fn with_small(readings: Vec<String>, s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for r in readings {
        let stem = r.strip_suffix(VOWELS).unwrap_or(&r);
        if let Some(v) = s.strip_prefix('y') {
            if r.ends_with('i') {
                out.push(format!("{stem}y{v}"));
                if stem.ends_with('h') || stem == "j" {
                    out.push(format!("{stem}{v}"));
                }
            }
        } else if let Some(v) = s.strip_prefix('w') {
            out.push(format!("{stem}w{v}"));
        } else {
            out.push(format!("{stem}{s}"));
            if stem.is_empty() {
                out.push(format!("w{s}"));
            }
        }
    }
    out
}

/// After a small っ: the next sound's consonant doubled ("te" "tte", "chi" "tchi").
fn doubled(readings: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for r in readings {
        if let Some(c) = r
            .chars()
            .next()
            .filter(|c| !VOWELS.contains(c) && *c != 'n')
        {
            out.push(format!("{c}{r}"));
            if r.starts_with("ch") {
                out.push(format!("t{r}"));
            }
        }
    }
    out
}

/// With a ー after it: the vowel held ("bi" "bii"), or not written.
fn held(readings: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for r in readings {
        if let Some(v) = r.chars().last().filter(|c| VOWELS.contains(c)) {
            out.push(format!("{r}{v}"));
        }
        out.push(r);
    }
    out
}

/// Punctuation as it is written among Roman letters.
fn ascii_punct(c: char) -> char {
    match c {
        '？' => '?',
        '！' => '!',
        '、' | '，' => ',',
        '。' | '．' => '.',
        '～' | '〜' => '~',
        '：' => ':',
        '；' => ';',
        '（' => '(',
        '）' => ')',
        c => c,
    }
}

/// The original line in pieces: kana one by one (with their small kana and ー), kanji in runs,
/// Hangul blocks one by one, Latin words whole, and the gaps between.
fn pieces(text: &str) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut i = 0;
    let piece = |kind, text: String| Piece {
        kind,
        text,
        readings: Vec::new(),
        optional: false,
        roman: String::new(),
    };
    while i < chars.len() {
        let c = chars[i];
        if is_han(c) {
            let start = i;
            while i < chars.len() && is_han(chars[i]) {
                i += 1;
            }
            out.push(piece(Kind::Kanji, chars[start..i].iter().collect()));
        } else if is_hira(c) || is_kata(c) || c == 'ー' {
            let start = i;
            let kind = if is_hira(c) { Kind::Hira } else { Kind::Kata };
            let sokuon = matches!(c, 'っ' | 'ッ');
            if sokuon || c == 'ー' {
                i += 1;
            }
            let has_base = i < chars.len()
                && (is_hira(chars[i]) || is_kata(chars[i]))
                && !matches!(chars[i], 'っ' | 'ッ')
                && (start == i || sokuon);
            let mut readings: Vec<String> = Vec::new();
            if has_base {
                readings = base(chars[i]).iter().map(|s| s.to_string()).collect();
                i += 1;
                while i < chars.len()
                    && let Some(s) = small(chars[i])
                {
                    readings = with_small(readings, s);
                    i += 1;
                }
                if sokuon {
                    readings = doubled(readings);
                }
                while i < chars.len() && chars[i] == 'ー' {
                    readings = held(readings);
                    i += 1;
                }
            }
            let mut p = piece(kind, chars[start..i].iter().collect());
            if has_base {
                p.readings = readings;
            } else {
                p.optional = true;
            }
            out.push(p);
        } else if is_hangul(c) {
            out.push(piece(Kind::Hangul, c.to_string()));
            i += 1;
        } else if c.is_alphanumeric() {
            let start = i;
            while i < chars.len()
                && chars[i].is_alphanumeric()
                && !is_han(chars[i])
                && !is_hira(chars[i])
                && !is_kata(chars[i])
                && !is_hangul(chars[i])
                || (i > start && i < chars.len() && chars[i] == '\'')
            {
                i += 1;
            }
            out.push(piece(Kind::Latin, chars[start..i].iter().collect()));
        } else {
            out.push(piece(Kind::Gap, c.to_string()));
            i += 1;
        }
    }
    out
}

/// A syllable as sent, reduced to its letters ("ta?" "ta", "「da" "da", "Bde" "de": Latin
/// from the original glued on is the original's, not the syllable's).
fn letters(token: &str) -> String {
    token.chars().filter(|c| c.is_ascii_lowercase()).collect()
}

/// Line the syllables up with the pieces; false when they can't be.
fn line_up(ps: &mut [Piece], spaced: &str) -> bool {
    let toks: Vec<String> = spaced.split_whitespace().map(letters).collect();
    let (n, m) = (ps.len(), toks.len());
    // from[i][j]: how pieces 0..i took syllables 0..j (the step before), when they can.
    let mut from: Vec<Vec<Option<(usize, usize)>>> = vec![vec![None; m + 1]; n + 1];
    let mut reach = vec![vec![false; m + 1]; n + 1];
    reach[0][0] = true;
    for i in 0..=n {
        for j in 0..=m {
            if !reach[i][j] {
                continue;
            }
            let mut go = |a: usize, b: usize, reach: &mut Vec<Vec<bool>>| {
                if !reach[a][b] {
                    reach[a][b] = true;
                    from[a][b] = Some((i, j));
                }
            };
            // Punctuation the source wrote as a syllable of its own.
            if j < m && toks[j].is_empty() {
                go(i, j + 1, &mut reach);
            }
            if i == n {
                continue;
            }
            let p = &ps[i];
            match p.kind {
                Kind::Gap => go(i + 1, j, &mut reach),
                Kind::Latin => {
                    go(i + 1, j, &mut reach);
                    let want: String = p
                        .text
                        .to_lowercase()
                        .chars()
                        .filter(|c| c.is_ascii_lowercase())
                        .collect();
                    let mut have = String::new();
                    for (k, tok) in toks.iter().enumerate().skip(j) {
                        have.push_str(tok);
                        if have == want {
                            go(i + 1, k + 1, &mut reach);
                        }
                        if have.len() >= want.len() {
                            break;
                        }
                    }
                }
                Kind::Hangul => {
                    if j < m && !toks[j].is_empty() {
                        go(i + 1, j + 1, &mut reach);
                    }
                }
                Kind::Hira | Kind::Kata if p.optional => {
                    go(i + 1, j, &mut reach);
                    if j < m && !toks[j].is_empty() {
                        go(i + 1, j + 1, &mut reach);
                    }
                }
                Kind::Hira | Kind::Kata => {
                    if j < m
                        && !toks[j].is_empty()
                        && (p.readings.is_empty() || p.readings.contains(&toks[j]))
                    {
                        go(i + 1, j + 1, &mut reach);
                    }
                }
                Kind::Kanji => {
                    let most = (p.text.chars().count() * 4).min(m - j);
                    for k in 1..=most {
                        go(i + 1, j + k, &mut reach);
                    }
                }
            }
        }
    }
    if !reach[n][m] {
        return false;
    }
    let (mut i, mut j) = (n, m);
    while let Some((pi, pj)) = from[i][j] {
        if pi < i {
            let p = &mut ps[pi];
            p.roman = match p.kind {
                Kind::Latin => p.text.clone(),
                Kind::Gap => String::new(),
                _ => toks[pj..j].concat(),
            };
        }
        (i, j) = (pi, pj);
    }
    true
}

const PARTICLES: &[&str] = &["の", "は", "が", "を", "に", "で", "と", "も", "へ"];

/// The pieces' Roman letters as words: each word's text and the pieces it came from.
fn words(ps: &[Piece]) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    // Punctuation waiting for the word after it ("「", or a "-" between spaces).
    let mut open = String::new();
    let mut spaced = true;
    let mut prev: Option<Kind> = None;
    let mut after_particle = false;
    for (i, p) in ps.iter().enumerate() {
        if p.kind == Kind::Gap {
            prev = None;
            after_particle = false;
            for c in p.text.chars() {
                let c = ascii_punct(c);
                if c.is_whitespace() {
                    // Punctuation with spaces on both sides stands alone.
                    if !open.is_empty() {
                        out.push((std::mem::take(&mut open), i, i));
                    }
                    spaced = true;
                } else if spaced || matches!(c, '(' | '[' | '「' | '『' | '“' | '¿' | '¡') {
                    open.push(c);
                } else if let Some(last) = out.last_mut() {
                    last.0.push(c);
                } else {
                    open.push(c);
                }
            }
            continue;
        }
        spaced = false;
        if p.roman.is_empty() {
            continue;
        }
        // を is never part of a word; the others are particles after a kanji or katakana word.
        let particle = p.kind == Kind::Hira
            && (p.text == "を"
                || (PARTICLES.contains(&p.text.as_str())
                    && matches!(prev, Some(Kind::Kanji | Kind::Kata))));
        let new_word = match (prev, p.kind) {
            (None, _) => true,
            _ if after_particle || particle => true,
            (Some(Kind::Latin), _) | (_, Kind::Latin) => true,
            (Some(a), b) if a == b => false,
            (Some(Kind::Kanji), Kind::Hira) => false,
            (Some(Kind::Hangul), _) | (_, Kind::Hangul) => true,
            _ => true,
        };
        if new_word {
            out.push((std::mem::take(&mut open), i, i));
        }
        let last = out.last_mut().expect("a word was started");
        last.0.push_str(&p.roman);
        last.2 = i;
        prev = Some(p.kind);
        after_particle = particle;
    }
    if !open.is_empty()
        && let Some(last) = out.last_mut()
    {
        last.0.push_str(&open);
    }
    out
}

/// Whether a line is one this joins: Japanese (any kana) or Korean.
fn joinable(original: &str) -> bool {
    original
        .chars()
        .any(|c| is_hira(c) || is_kata(c) || is_hangul(c))
}

/// A romanized line a source sends syllable by syllable, joined into words by the original
/// line. None when the line isn't Japanese or Korean, or the syllables don't line up with it
/// (then the line is best kept as sent).
pub fn join_syllables(original: &str, spaced: &str) -> Option<String> {
    if !joinable(original) || spaced.trim().is_empty() {
        return None;
    }
    let mut ps = pieces(original);
    if !line_up(&mut ps, spaced) {
        return None;
    }
    let out: Vec<String> = words(&ps).into_iter().map(|(w, ..)| w).collect();
    (!out.is_empty()).then(|| out.join(" "))
}

/// One word of a joined line: its text and the parts (as given) it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    pub text: String,
    pub first: usize,
    pub last: usize,
}

/// A line given part by part, each with its Roman letters (Kugou's way: "様" "yo u"), joined
/// into words. Parts with no Roman letters are left out; Latin parts are kept as written.
pub fn join_parts(parts: &[(&str, &str)]) -> Vec<Joined> {
    let mut ps: Vec<Piece> = Vec::new();
    let mut part_of: Vec<usize> = Vec::new();
    for (k, (orig, roman)) in parts.iter().enumerate() {
        let core = orig.trim_matches(|c: char| !c.is_alphanumeric());
        let lead = &orig[..orig.find(core).unwrap_or(0)];
        let trail = &orig[lead.len() + core.len()..];
        let mut push = |p: Piece| {
            ps.push(p);
            part_of.push(k);
        };
        let gap = |t: &str| Piece {
            kind: Kind::Gap,
            text: t.to_owned(),
            readings: Vec::new(),
            optional: false,
            roman: String::new(),
        };
        if !lead.is_empty() {
            push(gap(lead));
        }
        if let Some(first) = core.chars().next() {
            let kind = if is_han(first) {
                Kind::Kanji
            } else if is_hira(first) {
                Kind::Hira
            } else if is_kata(first) || first == 'ー' {
                Kind::Kata
            } else if is_hangul(first) {
                Kind::Hangul
            } else {
                Kind::Latin
            };
            let roman = if kind == Kind::Latin {
                core.to_owned()
            } else {
                roman
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_owned()
            };
            push(Piece {
                kind,
                text: core.to_owned(),
                readings: Vec::new(),
                optional: false,
                roman,
            });
        }
        if !trail.is_empty() {
            push(gap(trail));
        }
    }
    words(&ps)
        .into_iter()
        .map(|(text, a, b)| Joined {
            text,
            first: part_of[a],
            last: part_of[b],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netease_syllables_become_words() {
        let j = |o: &str, s: &str| join_syllables(o, s);
        assert_eq!(
            j(
                "無敵の笑顔で荒らすメディア",
                "mu te ki no e ga o de a ra su me di a"
            )
            .as_deref(),
            Some("muteki no egao de arasu media")
        );
        assert_eq!(
            j(
                "君は完璧で究極のアイドル",
                "ki mi wa ka n pe ki de kyu u kyo ku no a i do ru"
            )
            .as_deref(),
            Some("kimi wa kanpeki de kyuukyoku no aidoru")
        );
        assert_eq!(
            j("好きな本は？", "su ki na ho n wa?").as_deref(),
            Some("sukina hon wa?")
        );
        // っ doubles the next sound; ー holds the vowel; Latin from the line stays as written.
        assert_eq!(
            j("やっと言えた", "ya tto i e ta").as_deref(),
            Some("yatto ieta")
        );
        assert_eq!(
            j(
                "ルビーを隠したこの瞼",
                "ru bii wo ka ku shi ta ko no ma bu ta"
            )
            .as_deref(),
            Some("rubii wo kakushitakono mabuta")
        );
        assert_eq!(
            j(
                "お星様の引き立て役Bです",
                "o ho shi sa ma no hi ki ta te ya ku Bde su"
            )
            .as_deref(),
            Some("o hoshisama no hiki tate yaku B desu")
        );
        assert_eq!(
            j(
                "「誰かを好きになることなんて私分からなくてさ」",
                "「da re ka wo su ki ni na ru ko to na n te wa ta shi wa ka ra na ku te sa」"
            )
            .as_deref(),
            Some("「dareka wo sukininarukotonante watashiwakaranakutesa」")
        );
        // Korean: each block one syllable, words where the line has spaces.
        assert_eq!(
            j("사랑해 너를", "sa rang hae neo reul").as_deref(),
            Some("saranghae neoreul")
        );
    }

    #[test]
    fn what_cant_be_joined_is_left_as_sent() {
        // Chinese pinyin is written syllable by syllable.
        assert_eq!(join_syllables("我爱你", "wo ai ni"), None);
        // Syllables that don't fit the line.
        assert_eq!(join_syllables("あいう", "ka ki"), None);
        assert_eq!(join_syllables("あいう", ""), None);
    }

    #[test]
    fn kugou_parts_become_words_with_their_parts() {
        let parts = [
            ("嗚", "a "),
            ("呼 ", "a  "),
            ("い", "i "),
            ("つ", "tsu "),
            ("も", "mo "),
            ("の", "no "),
            ("様", "yo u "),
            ("に", "ni "),
        ];
        let j = join_parts(&parts);
        assert_eq!(
            j.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            ["aa", "itsumono", "you", "ni"]
        );
        assert_eq!((j[0].first, j[0].last), (0, 1));
        assert_eq!((j[1].first, j[1].last), (2, 5));
        let latin = join_parts(&[("YOASOBI ", "YOASOBI "), ("- ", "- "), ("群", "gu n ")]);
        assert_eq!(
            latin.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            ["YOASOBI", "-", "gun"]
        );
    }
}
