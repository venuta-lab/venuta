//! Golden lines: whole lyric lines in every Indian script this crate reads, as people write
//! them in English letters.

use lyrics_text::{Line, romanize};

fn roman(text: &str, lang: Option<&str>) -> String {
    romanize(&Line::new(text).lang(lang))
        .map(|r| r.text)
        .unwrap_or_default()
}

#[track_caller]
fn golden(lang: Option<&str>, cases: &[(&str, &str)]) {
    let mut wrong = Vec::new();
    for (native, want) in cases {
        let got = roman(native, lang);
        if got != *want {
            wrong.push(format!("{native}\n   want {want}\n    got {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} lines differ:\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_apple_music_example_line() {
    assert_eq!(
        roman("जो वादे किये सारे बस बातों में रह गये", None),
        "Jo waade kiye saare bas baaton mein reh gaye"
    );
    assert_eq!(
        roman("जो वादे किए सारे बस बातों में रह गए", None),
        "Jo waade kiye saare bas baaton mein reh gaye"
    );
}

#[test]
fn hindi() {
    golden(
        Some("hi"),
        &[
            ("तुम ही हो, अब तुम ही हो", "Tum hi ho, ab tum hi ho"),
            ("ज़िंदगी अब तुम ही हो", "Zindagi ab tum hi ho"),
            ("केसरिया तेरा इश्क़ है पिया", "Kesariya tera ishq hai piya"),
            ("मैंने तुझे देखा", "Maine tujhe dekha"),
            ("कहना है तुमसे", "Kahna hai tumse"),
            ("पहली नज़र में", "Pehli nazar mein"),
            ("आँखों में तेरी", "Aankhon mein teri"),
            ("दिल धड़कने लगा", "Dil dhadakne laga"),
            ("मुझको इतना बताए कोई", "Mujhko itna bataye koi"),
            (
                "कर दी हैं हुस्न की ख़ाली तिजोरियाँ",
                "Kar di hain husn ki khaali tijoriyan",
            ),
            (
                "ये रातें ये मौसम नदी का किनारा",
                "Ye raatein ye mausam nadi ka kinara",
            ),
            ("वो कौन थी", "Woh kaun thi"),
            ("मैं यहाँ हूँ", "Main yahan hoon"),
            ("प्यार हुआ इक़रार हुआ", "Pyar hua iqrar hua"),
        ],
    );
}

#[test]
fn marathi() {
    golden(
        Some("mr"),
        &[
            ("मला तुझी आठवण येते", "Mala tujhi aathvan yete"),
            ("ज्ञान", "Dnyan"),
            ("मन उधाण वाऱ्याचे", "Man udhaan varyache"),
        ],
    );
}

#[test]
fn nepali() {
    golden(
        Some("ne"),
        &[
            ("म तिमीलाई माया गर्छु", "Ma timilai maya garchhu"),
            ("मेरो मन", "Mero man"),
        ],
    );
}

#[test]
fn sanskrit() {
    golden(
        Some("sa"),
        &[
            ("ॐ नमः शिवाय", "Om namah shivaya"),
            ("वक्रतुण्ड महाकाय", "Vakratunda mahakaya"),
        ],
    );
}

#[test]
fn bengali() {
    golden(
        None,
        &[
            ("আমার সোনার বাংলা", "Amar sonar bangla"),
            ("আমি তোমাকে ভালোবাসি", "Ami tomake bhalobasi"),
            ("কথা হবে", "Kotha hobe"),
            ("এবং তুমি", "Ebong tumi"),
        ],
    );
}

#[test]
fn gujarati() {
    golden(
        None,
        &[("તારી આંખનો અફીણી", "Taari aankhano afeeni"), ("છે", "Chhe")],
    );
}

#[test]
fn punjabi() {
    golden(
        None,
        &[
            ("ਤੇਰੇ ਬਿਨਾ", "Tere bina"),
            ("ਮੈਨੂੰ ਤੇਰੇ ਨਾਲ ਪਿਆਰ ਹੋ ਗਿਆ", "Mainoon tere naal piyaar ho giya"),
            ("ਇੱਕ ਦਿਲ", "Ikk dil"),
        ],
    );
}

#[test]
fn odia() {
    golden(None, &[("ମୋ ମନ ତୁମକୁ ଖୋଜେ", "Mo mana tumaku khoje")]);
}

#[test]
fn tamil() {
    golden(
        None,
        &[
            ("நான் உன்னை காதலிக்கிறேன்", "Naan unnai kaathalikkiren"),
            ("கண்ணே கலைமானே", "Kanne kalaimaane"),
            ("என்றும் அன்புடன்", "Endrum anbudan"),
        ],
    );
}

#[test]
fn telugu() {
    golden(
        None,
        &[
            ("నీ కళ్ళు నీలి సముద్రం", "Nee kallu neeli samudram"),
            ("నేను నిన్ను ప్రేమిస్తున్నాను", "Nenu ninnu premistunnaanu"),
        ],
    );
}

#[test]
fn kannada() {
    golden(
        None,
        &[(
            "ನಿನ್ನ ಕಣ್ಣುಗಳು ತುಂಬಾ ಸುಂದರವಾಗಿವೆ",
            "Ninna kannugalu tumbaa sundaravaagive",
        )],
    );
}

#[test]
fn malayalam() {
    golden(
        None,
        &[
            ("എന്റെ ഖൽബിലെ വെണ്ണിലാവു നീ", "Ente khalbile vennilaavu nee"),
            ("ഞാൻ നിന്നെ സ്നേഹിക്കുന്നു", "Njaan ninne snehikkunnu"),
        ],
    );
}

#[test]
fn mixed_lines_keep_their_english_words() {
    golden(
        None,
        &[
            ("तेरा मेरा, Kesariya!", "Tera mera, Kesariya!"),
            ("I love you, मेरी जान", "I love you, meri jaan"),
            ("Baby तू मेरी है", "Baby tu meri hai"),
            ("२०२४ में", "2024 mein"),
            ("दिल।", "Dil."),
        ],
    );
}

#[test]
fn lines_with_nothing_to_romanize() {
    for line in [
        "",
        "   ",
        "Kesariya (From \"Brahmastra\")",
        "123 !",
        "دل دیاں گلاں",
    ] {
        assert_eq!(romanize(&Line::new(line)), None, "{line}");
    }
    // Urdu stays as it is beside the Hindi words that are read.
    assert_eq!(roman("दिल دل", None), "Dil دل");
}

#[test]
fn the_pronunciation_says_its_language() {
    let tag = |t, l| romanize(&Line::new(t).lang(l)).map(|r| r.lang);
    assert_eq!(tag("दिल", None).as_deref(), Some("hi-Latn"));
    assert_eq!(tag("मन", Some("mr")).as_deref(), Some("mr-Latn"));
    assert_eq!(
        tag("ਦਿਲ", Some("hi")).as_deref(),
        Some("pa-Latn"),
        "a Hindi song's Gurmukhi line is still Punjabi"
    );
    assert_eq!(tag("Love you दिल", None).as_deref(), Some("hi-Latn"));
}
