//! Korean gate: Revised Romanization as the National Institute of the Korean Language writes
//! it, on every example in the rules (Ministry of Culture notice 2014-42), the regression set of
//! koroman (MIT) whose rules the converter ports, and common lyric phrases.

use lyrics_text::Word;
use lyrics_text::korean::{self, Options};

fn rr(s: &str) -> String {
    korean::romanize(s).text
}

/// Every example the rules give, chapter 2 and chapter 3, articles 1 to 3 and 6 (names of
/// people and places with a "-do", "-si" suffix need lexical knowledge and are left out).
/// Expected forms lowercased: the rules capitalise proper nouns, lyrics don't.
const NIKL: &[(&str, &str)] = &[
    // Chapter 2, article 1: vowels.
    ("광희문", "gwanghuimun"),
    // Article 2: consonants before a vowel, before a consonant and at the end.
    ("구미", "gumi"),
    ("영동", "yeongdong"),
    ("백암", "baegam"),
    ("옥천", "okcheon"),
    ("합덕", "hapdeok"),
    ("호법", "hobeop"),
    ("월곶", "wolgot"),
    ("벚꽃", "beotkkot"),
    ("한밭", "hanbat"),
    ("구리", "guri"),
    ("설악", "seorak"),
    ("칠곡", "chilgok"),
    ("임실", "imsil"),
    ("울릉", "ulleung"),
    ("대관령", "daegwallyeong"),
    // Chapter 3, article 1: sound changes.
    ("백마", "baengma"),
    ("신문로", "sinmunno"),
    ("종로", "jongno"),
    ("왕십리", "wangsimni"),
    ("별내", "byeollae"),
    ("신라", "silla"),
    ("학여울", "hangnyeoul"),
    ("알약", "allyak"),
    ("해돋이", "haedoji"),
    ("같이", "gachi"),
    ("굳히다", "guchida"),
    ("좋고", "joko"),
    ("놓다", "nota"),
    ("잡혀", "japyeo"),
    ("낳지", "nachi"),
    // ... but nouns keep the h after ㄱ, ㄷ, ㅂ.
    ("묵호", "mukho"),
    ("집현전", "jiphyeonjeon"),
    // Tensed consonants are not written.
    ("압구정", "apgujeong"),
    ("낙동강", "nakdonggang"),
    ("죽변", "jukbyeon"),
    ("낙성대", "nakseongdae"),
    ("합정", "hapjeong"),
    ("팔당", "paldang"),
    ("샛별", "saetbyeol"),
    ("울산", "ulsan"),
    // Article 3: proper nouns.
    ("부산", "busan"),
    ("세종", "sejong"),
    // Article 6: natural features, cultural properties, buildings.
    ("남산", "namsan"),
    ("속리산", "songnisan"),
    ("금강", "geumgang"),
    ("독도", "dokdo"),
    ("경복궁", "gyeongbokgung"),
    ("무량수전", "muryangsujeon"),
    ("연화교", "yeonhwagyo"),
    ("극락전", "geungnakjeon"),
    ("안압지", "anapji"),
    ("남한산성", "namhansanseong"),
    ("화랑대", "hwarangdae"),
    ("불국사", "bulguksa"),
    ("현충사", "hyeonchungsa"),
    ("독립문", "dongnimmun"),
    ("오죽헌", "ojukheon"),
    ("촉석루", "chokseongnu"),
    ("종묘", "jongmyo"),
    ("다보탑", "dabotap"),
];

/// Chapter 3, article 2: a hyphen where the syllables could be read two ways (optional).
const NIKL_HYPHEN: &[(&str, &str)] = &[
    ("중앙", "jung-ang"),
    ("반구대", "ban-gudae"),
    ("세운", "se-un"),
    ("해운대", "hae-undae"),
];

#[test]
fn every_example_of_the_rules() {
    let misses: Vec<String> = NIKL
        .iter()
        .filter(|(k, want)| rr(k) != *want)
        .map(|(k, want)| format!("{k}: got {}, want {want}", rr(k)))
        .collect();
    println!(
        "NIKL examples: {}/{} exact",
        NIKL.len() - misses.len(),
        NIKL.len()
    );
    assert!(misses.is_empty(), "{misses:#?}");
}

#[test]
fn hyphens_when_asked() {
    let opts = Options { hyphens: true };
    for (k, want) in NIKL_HYPHEN {
        assert_eq!(korean::romanize_with(k, opts).text, *want, "{k}");
    }
    // Without the option, as the lyrics are usually written.
    assert_eq!(rr("중앙"), "jungang");
    assert_eq!(rr("해운대"), "haeundae");
}

/// koroman's own regression set (its policy on nouns is replaced by the rules' own above).
const KOROMAN: &[(&str, &str)] = &[
    ("한글", "hangeul"),
    ("로마자", "romaja"),
    ("안녕하세요", "annyeonghaseyo"),
    ("테스트", "teseuteu"),
    ("문래역", "mullaeyeok"),
    ("선릉역", "seolleungyeok"),
    ("역량", "yeongnyang"),
    ("맞히다", "machida"),
    ("흙", "heuk"),
    ("닭", "dak"),
    ("삶", "sam"),
    ("값", "gap"),
    ("넋", "neok"),
    ("잃어", "ireo"),
    ("싫어", "sireo"),
    ("않아", "ana"),
    ("닭이", "dalgi"),
    ("삶이", "salmi"),
    ("값이", "gapsi"),
    ("넋이", "neoksi"),
    ("앉아", "anja"),
    ("핥아", "halta"),
    ("곬이", "golsi"),
    ("읊어", "eulpeo"),
    ("밟다", "bapda"),
    ("밟지", "bapji"),
    ("밟고", "bapgo"),
    ("밟는", "bamneun"),
    ("밟아", "balba"),
    ("닫히다", "dachida"),
    ("꽃잎", "kkonnip"),
    ("깻잎", "kkaennip"),
    ("감잎", "gamnip"),
    ("버섯잎", "beoseonnip"),
    ("많이", "mani"),
    ("받히다", "bachida"),
    ("묻히다", "muchida"),
    ("여기는 선릉역 입니다.", "yeogineun seolleungyeok imnida."),
    (
        "해돋이와 문래역 그리고 역량 개발.",
        "haedojiwa mullaeyeok geurigo yeongnyang gaebal.",
    ),
];

#[test]
fn koroman_regression_set() {
    for (k, want) in KOROMAN {
        assert_eq!(rr(k), *want, "{k}");
    }
}

/// Phrases songs use, as standard pronunciation gives them.
const LYRICS: &[(&str, &str)] = &[
    ("사랑해", "saranghae"),
    ("보고 싶어", "bogo sipeo"),
    ("괜찮아", "gwaenchana"),
    ("없어", "eopseo"),
    ("있어", "isseo"),
    ("몰라", "molla"),
    ("같이 가자", "gachi gaja"),
    ("좋아해", "joahae"),
    ("미안해", "mianhae"),
    ("고마워", "gomawo"),
    ("잊지 마", "itji ma"),
    ("읽어", "ilgeo"),
    ("밤하늘", "bamhaneul"),
    ("끝이", "kkeuchi"),
    ("빛나", "binna"),
    ("꽃을", "kkocheul"),
    ("국물", "gungmul"),
    ("십년", "simnyeon"),
    ("한국어", "hangugeo"),
    ("내 맘을", "nae mameul"),
    ("오빤 강남 스타일", "oppan gangnam seutail"),
    ("영원히", "yeongwonhi"),
    ("닿을", "daeul"),
    ("넣어", "neoeo"),
    ("끝나", "kkeunna"),
    ("달빛", "dalbit"),
    ("맛있어", "masisseo"),
    // ㅅ, ㅊ, ㅌ before ㅎ sound as ㅌ (standard pronunciation, article 12).
    ("못해", "motae"),
    ("따뜻해", "ttatteutae"),
    // Verbs aspirate: 하다, 히, 혀.
    ("생각해", "saenggakae"),
    ("행복해", "haengbokae"),
    ("막혀", "makyeo"),
    // Verb stems before 여 take no inserted ㄴ.
    ("죽여", "jugyeo"),
    ("녹여", "nogyeo"),
    ("속여", "sogyeo"),
];

#[test]
fn lyric_phrases() {
    for (k, want) in LYRICS {
        assert_eq!(rr(k), *want, "{k}");
    }
}

#[test]
fn other_writing_passes_through() {
    assert_eq!(rr("사랑해 baby"), "saranghae baby");
    assert_eq!(rr("I love you 사랑해!"), "I love you saranghae!");
    assert_eq!(rr("ㅋㅋㅋ"), "kkk");
    assert_eq!(rr("좋아? 응."), "joa? eung.");
}

fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
    Word {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

#[test]
fn each_syllable_is_its_own_piece() {
    // A consonant carried over to the next syllable is said, and so timed, with it.
    assert_eq!(korean::romanize("먹어").piece_texts(), ["meo", "geo"]);
    assert_eq!(
        korean::romanize("사랑해").piece_texts(),
        ["sa", "rang", "hae"]
    );
    assert_eq!(korean::romanize("신라").piece_texts(), ["sil", "la"]);
}

#[test]
fn word_timing_follows_the_syllables() {
    let line = "사랑해 너를";
    let words = [
        w(0, 100, "사"),
        w(100, 200, "랑"),
        w(200, 300, "해 "),
        w(400, 600, "너를"),
    ];
    let r = korean::romanize(line);
    assert_eq!(
        lyrics_text::pieces::timed_words(line, &words, &r),
        vec![
            w(0, 100, "sa"),
            w(100, 200, "rang"),
            w(200, 300, "hae "),
            w(400, 600, "neoreul"),
        ]
    );
    let second = r.to_second(line, &words, "ko-Latn");
    assert_eq!(second.text, "saranghae neoreul");
    assert_eq!(second.lang, "ko-Latn");
}

#[test]
fn speed_per_line() {
    let line = "보고 싶다 이렇게 말하니까 더 보고 싶다 너희 사진을 보고 있어도 보고 싶다";
    let t = std::time::Instant::now();
    let n = 2_000;
    for _ in 0..n {
        std::hint::black_box(korean::romanize(std::hint::black_box(line)));
    }
    let per = t.elapsed() / n;
    println!("korean: {per:?} per line");
    assert!(per < std::time::Duration::from_millis(1), "{per:?}");
}
