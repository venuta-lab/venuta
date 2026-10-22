//! Script detection and splitting a line by script.

use lyrics_text::Script;
use lyrics_text::script::{line_script, runs};

#[test]
fn characters_by_script() {
    for (c, s) in [
        ('a', Script::Latn),
        ('é', Script::Latn),
        ('क', Script::Deva),
        ('।', Script::Deva),
        ('ক', Script::Beng),
        ('ਕ', Script::Guru),
        ('ક', Script::Gujr),
        ('କ', Script::Orya),
        ('க', Script::Taml),
        ('క', Script::Telu),
        ('ಕ', Script::Knda),
        ('ക', Script::Mlym),
        ('ب', Script::Arab),
        ('한', Script::Hang),
        ('か', Script::Hira),
        ('カ', Script::Kana),
        ('ー', Script::Kana),
        ('漢', Script::Hani),
        ('я', Script::Cyrl),
        ('λ', Script::Grek),
        ('ש', Script::Hebr),
        ('ก', Script::Thai),
        (' ', Script::Zyyy),
        ('1', Script::Zyyy),
        ('!', Script::Zyyy),
        ('、', Script::Zyyy),
        ('\u{200D}', Script::Zinh),
        ('\u{301}', Script::Zinh),
    ] {
        assert_eq!(Script::of(c), s, "{c:?}");
    }
}

#[test]
fn a_line_split_by_script() {
    let line = "तेरा मेरा, Kesariya!";
    let got: Vec<(&str, &str)> = runs(line)
        .map(|r| (r.script.code(), &line[r.range]))
        .collect();
    assert_eq!(
        got,
        [
            ("Deva", "तेरा"),
            ("Zyyy", " "),
            ("Deva", "मेरा"),
            ("Zyyy", ", "),
            ("Latn", "Kesariya"),
            ("Zyyy", "!")
        ]
    );
    // Joiners and combining marks stay with their letter.
    let got: Vec<&str> = runs("क्\u{200D}ष a\u{301}")
        .map(|r| &"क्\u{200D}ष a\u{301}"[r.range])
        .collect();
    assert_eq!(got, ["क्\u{200D}ष", " ", "a\u{301}"]);
}

#[test]
fn a_line_has_one_script() {
    assert_eq!(line_script("जो वादे किये"), Script::Deva);
    assert_eq!(
        line_script("Baby तू मेरी है"),
        Script::Deva,
        "a Latin word does not make a Hindi line Latin"
    );
    assert_eq!(line_script("Hello world"), Script::Latn);
    assert_eq!(line_script("君の名は"), Script::Jpan);
    assert_eq!(line_script("사랑해"), Script::Hang);
    assert_eq!(line_script("123 ..."), Script::Zyyy);
    assert_eq!(line_script("天才的なアイドル様"), Script::Jpan);
    assert_eq!(line_script("露出无敌笑容"), Script::Hani);
    assert_eq!(line_script("तेरा मेरा, Kesariya!"), Script::Deva);
    assert_eq!(line_script("ab कख"), Script::Deva, "Latin loses a tie");
    assert_eq!(Script::Jpan.code(), "Jpan");
}
