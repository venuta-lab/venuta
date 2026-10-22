//! Chinese gate: Mandarin pinyin for the common polyphonic characters (readings as the
//! Xiandai Hanyu Cidian gives them), Traditional characters, mixed lines and word timing.

use lyrics_text::chinese::{self, Options};
use lyrics_text::{Line, Word, romanize};

fn py(s: &str) -> String {
    chinese::romanize(s)
}

fn toned(s: &str) -> String {
    chinese::romanize_with(s, Options { tones: true })
}

/// Phrases with a polyphonic character, and their reading with tones (一 and 不 with their tone
/// changes, as pinyin is taught).
const POLYPHONES: &[(&str, &str)] = &[
    ("银行行长", "yín háng háng zhǎng"),
    ("我们一起长大", "wǒ men yì qǐ zhǎng dà"),
    ("长江", "cháng jiāng"),
    ("长发", "cháng fà"),
    ("头发", "tóu fa"),
    ("发现", "fā xiàn"),
    ("重新开始", "chóng xīn kāi shǐ"),
    ("重要的事", "zhòng yào de shì"),
    ("音乐让我快乐", "yīn yuè ràng wǒ kuài lè"),
    ("把心还给我", "bǎ xīn huán gěi wǒ"),
    ("还是你", "hái shì nǐ"),
    ("睡觉", "shuì jiào"),
    ("觉得", "jué de"),
    ("了解", "liǎo jiě"),
    ("走了", "zǒu le"),
    ("得到", "dé dào"),
    ("我得走了", "wǒ děi zǒu le"),
    ("记得", "jì de"),
    ("都是你", "dōu shì nǐ"),
    ("首都", "shǒu dū"),
    ("调皮", "tiáo pí"),
    ("单调", "dān diào"),
    ("朝代", "cháo dài"),
    ("慢慢地", "màn màn de"),
    ("默默地爱", "mò mò de ài"),
    ("目的地", "mù dì dì"),
    ("的确", "dí què"),
    ("一个人", "yí gè rén"),
    ("一样", "yí yàng"),
    ("不要走", "bú yào zǒu"),
    ("不会", "bú huì"),
    ("传说", "chuán shuō"),
    ("自传", "zì zhuàn"),
    ("差不多", "chà bu duō"),
    ("参差", "cēn cī"),
    ("背影", "bèi yǐng"),
    ("好奇", "hào qí"),
    ("爱好", "ài hào"),
    ("很好", "hěn hǎo"),
    ("为了你", "wèi le nǐ"),
    ("为什么", "wèi shén me"),
    ("种子", "zhǒng zi"),
    ("种花", "zhòng huā"),
    ("便宜", "pián yi"),
    ("方便", "fāng biàn"),
    ("处理", "chǔ lǐ"),
    ("到处", "dào chù"),
    ("空气", "kōng qì"),
    ("空闲", "kòng xián"),
    ("教室", "jiào shì"),
    ("教我", "jiāo wǒ"),
    ("曾经", "céng jīng"),
    ("似乎", "sì hū"),
    ("似的", "shì de"),
    ("数学", "shù xué"),
    ("数一数", "shǔ yi shǔ"),
    ("数不清", "shǔ bù qīng"),
    ("着急", "zháo jí"),
    ("看着我", "kàn zhe wǒ"),
    ("西藏", "xī zàng"),
    ("藏起来", "cáng qǐ lái"),
    ("落叶", "luò yè"),
    ("丢三落四", "diū sān là sì"),
    ("没有", "méi yǒu"),
    ("沉没", "chén mò"),
    ("一切", "yí qiè"),
    ("切菜", "qiē cài"),
    ("中奖", "zhòng jiǎng"),
    ("中间", "zhōng jiān"),
    ("只有", "zhǐ yǒu"),
    ("难过", "nán guò"),
    ("灾难", "zāi nàn"),
    ("相信", "xiāng xìn"),
    ("相机", "xiàng jī"),
    ("角色", "jué sè"),
    ("和你", "hé nǐ"),
    ("暖和", "nuǎn huo"),
    ("应该", "yīng gāi"),
    ("答应", "dā ying"),
    ("模样", "mú yàng"),
    ("模糊", "mó hu"),
    ("给你", "gěi nǐ"),
    ("弹琴", "tán qín"),
    ("子弹", "zǐ dàn"),
    ("转身", "zhuǎn shēn"),
    ("旋转", "xuán zhuǎn"),
    ("舍不得", "shě bu de"),
    ("宿舍", "sù shè"),
    ("会计", "kuài jì"),
    ("女孩", "nǚ hái"),
    ("绿色", "lǜ sè"),
];

fn strip(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ā' | 'á' | 'ǎ' | 'à' => 'a',
            'ē' | 'é' | 'ě' | 'è' => 'e',
            'ī' | 'í' | 'ǐ' | 'ì' => 'i',
            'ō' | 'ó' | 'ǒ' | 'ò' => 'o',
            'ū' | 'ú' | 'ǔ' | 'ù' => 'u',
            'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' => 'ü',
            c => c,
        })
        .collect()
}

#[test]
fn polyphones_without_tones() {
    let misses: Vec<String> = POLYPHONES
        .iter()
        .filter(|(k, want)| py(k) != strip(want))
        .map(|(k, want)| format!("{k}: got {}, want {}", py(k), strip(want)))
        .collect();
    println!(
        "polyphones: {}/{} exact without tones",
        POLYPHONES.len() - misses.len(),
        POLYPHONES.len()
    );
    assert!(misses.is_empty(), "{misses:#?}");
}

#[test]
fn polyphones_with_tones() {
    // Neutral tones are where dictionaries differ most: counted, the gate is above.
    let misses: Vec<String> = POLYPHONES
        .iter()
        .filter(|(k, want)| toned(k) != *want)
        .map(|(k, want)| format!("{k}: got {}, want {want}", toned(k)))
        .collect();
    println!(
        "polyphones: {}/{} exact with tones; {misses:#?}",
        POLYPHONES.len() - misses.len(),
        POLYPHONES.len()
    );
    assert!(misses.len() <= POLYPHONES.len() / 10, "{misses:#?}");
}

#[test]
fn traditional_reads_like_simplified() {
    assert_eq!(py("中國人"), "zhong guo ren");
    assert_eq!(py("我們一起長大"), "wo men yi qi zhang da");
    assert_eq!(py("還是你"), "hai shi ni");
    assert_eq!(py("頭髮"), "tou fa");
    assert_eq!(py("會不會"), "hui bu hui");
    assert_eq!(py("藏起來"), "cang qi lai");
    assert_eq!(py("傳說"), "chuan shuo");
}

#[test]
fn other_writing_and_punctuation() {
    assert_eq!(py("我是Jay"), "wo shi Jay");
    assert_eq!(py("Hello 世界"), "Hello shi jie");
    assert_eq!(py("你好，世界！"), "ni hao, shi jie!");
    assert_eq!(py("「愛你」3.14 次"), "\"ai ni\" 3.14 ci");
    assert_eq!(py("Ｉ　ｌｏｖｅ　你"), "I love ni");
}

fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
    Word {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

fn line(text: &str, words: &[Word]) -> lyrics_text::Roman {
    romanize(&Line::new(text).words(words)).expect("romanized")
}

#[test]
fn each_character_keeps_its_time() {
    let words = [
        w(0, 100, "我"),
        w(100, 200, "爱"),
        w(200, 300, "你 "),
        w(300, 500, "baby"),
    ];
    let r = line("我爱你 baby", &words);
    assert_eq!(r.text, "Wo ai ni baby");
    assert_eq!(r.lang, "zh-Latn");
    assert_eq!(
        r.words,
        vec![
            w(0, 100, "Wo "),
            w(100, 200, "ai "),
            w(200, 300, "ni "),
            w(300, 500, "baby")
        ]
    );
}

#[test]
fn whole_lines_mixed_with_other_writing() {
    // Words of other scripts keep a space from the pinyin around them.
    assert_eq!(line("我是Jay", &[]).text, "Wo shi Jay");
    assert_eq!(line("Hello世界", &[]).text, "Hello shi jie");
    assert_eq!(line("你好，世界！", &[]).text, "Ni hao, shi jie!");
    assert_eq!(line("「愛你」", &[]).text, "\"Ai ni\"");
}

#[test]
fn speed_per_line() {
    let line = "曾经的梦想还在心里，我们一起长大，慢慢地走过每一个地方";
    py(line);
    let t = std::time::Instant::now();
    let n = 2_000;
    for _ in 0..n {
        std::hint::black_box(chinese::romanize(std::hint::black_box(line)));
    }
    let per = t.elapsed() / n;
    println!("chinese: {per:?} per line");
    assert!(per < std::time::Duration::from_millis(2), "{per:?}");
}
