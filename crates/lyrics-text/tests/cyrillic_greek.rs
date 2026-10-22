//! Cyrillic and Greek gate: readable conventions, each the country's own where there is one
//! (Ukraine's 2010 national system, Bulgaria's 2009 Streamlined System, Serbian Latin as
//! Serbs write it, Greece's ELOT 743), Russian in a simplified BGN/PCGN style.

use lyrics_text::Word;
use lyrics_text::cyrillic::{self, Lang};
use lyrics_text::greek;

fn cy(s: &str, lang: Lang) -> String {
    cyrillic::romanize(s, lang).text
}

#[test]
fn russian() {
    let r = |s| cy(s, Lang::Russian);
    assert_eq!(r("Я тебя люблю"), "Ya tebya lyublyu");
    assert_eq!(r("Привет, мир!"), "Privet, mir!");
    assert_eq!(r("Звезда по имени Солнце"), "Zvezda po imeni Solntse");
    assert_eq!(r("Группа крови на рукаве"), "Gruppa krovi na rukave");
    assert_eq!(r("Хорошо"), "Khorosho");
    assert_eq!(r("Цой жив"), "Tsoy zhiv");
    assert_eq!(r("ещё моё жёлтое"), "yeshcho moyo zholtoye");
    assert_eq!(r("объятия"), "obyatiya");
    assert_eq!(r("новый синий"), "novy siniy");
    assert_eq!(r("Война и мир"), "Voyna i mir");
    assert_eq!(r("ЩИ и Щука"), "SHCHI i Shchuka");
    assert_eq!(r("это мы"), "eto my");
}

#[test]
fn ukrainian() {
    let u = |s| cy(s, Lang::Ukrainian);
    assert_eq!(u("Я тебе кохаю"), "Ya tebe kokhaiu");
    assert_eq!(u("Червона рута"), "Chervona ruta");
    assert_eq!(u("Київ"), "Kyiv");
    assert_eq!(u("Україна"), "Ukraina");
    assert_eq!(u("Їжак Євген"), "Yizhak Yevhen");
    assert_eq!(u("Згорани"), "Zghorany");
    assert_eq!(u("щастя"), "shchastia");
    assert_eq!(u("Ґанок"), "Ganok");
    assert_eq!(u("м'ята"), "miata");
    assert_eq!(u("Йосип і Юлія"), "Yosyp i Yuliia");
}

#[test]
fn bulgarian() {
    let b = |s| cy(s, Lang::Bulgarian);
    assert_eq!(b("Обичам те"), "Obicham te");
    assert_eq!(b("България"), "Balgaria");
    assert_eq!(b("щастие"), "shtastie");
    assert_eq!(b("Път"), "Pat");
    assert_eq!(b("Юлия"), "Yulia");
    assert_eq!(b("Хубаво"), "Hubavo");
}

#[test]
fn serbian() {
    let s = |t| cy(t, Lang::Serbian);
    assert_eq!(s("Волим те"), "Volim te");
    assert_eq!(s("Ђорђе"), "Đorđe");
    assert_eq!(s("Љубав и Њега"), "Ljubav i Njega");
    assert_eq!(s("ЉУБАВ"), "LJUBAV");
    assert_eq!(s("Џеп"), "Džep");
    assert_eq!(s("Ћао, чаша, шта"), "Ćao, čaša, šta");
    assert_eq!(
        s("Јелена, жена, хвала, цвеће"),
        "Jelena, žena, hvala, cveće"
    );
}

#[test]
fn language_from_the_letters() {
    assert_eq!(
        cyrillic::detect("Я тебе кохаю, моя Україно"),
        Lang::Ukrainian
    );
    assert_eq!(cyrillic::detect("Волим те, љубави моја"), Lang::Serbian);
    assert_eq!(
        cyrillic::detect("Обичам те, ти си в сърцето ми"),
        Lang::Bulgarian
    );
    assert_eq!(cyrillic::detect("Ты моя любовь"), Lang::Russian);
    assert_eq!(cyrillic::detect("Привет"), Lang::Russian);
    assert_eq!(Lang::from_tag("uk-UA"), Some(Lang::Ukrainian));
    assert_eq!(Lang::from_tag("sr"), Some(Lang::Serbian));
    assert_eq!(Lang::from_tag("en"), None);
    assert_eq!(Lang::Bulgarian.tag(), "bg-Latn");
}

#[test]
fn greek_elot_743() {
    let g = |s| greek::romanize(s).text;
    assert_eq!(g("Σ' αγαπώ"), "S' agapo");
    assert_eq!(g("Καλημέρα"), "Kalimera");
    assert_eq!(g("Ευχαριστώ"), "Efcharisto");
    assert_eq!(g("Αύριο"), "Avrio");
    assert_eq!(g("Εύα"), "Eva");
    assert_eq!(g("Μπαμπάς"), "Bampas");
    assert_eq!(g("Ντομάτα"), "Domata");
    assert_eq!(g("Άγγελος"), "Angelos");
    assert_eq!(g("Θάλασσα"), "Thalassa");
    assert_eq!(g("Ψυχή"), "Psychi");
    assert_eq!(g("Ουρανός"), "Ouranos");
    assert_eq!(g("Είναι οι γκρίζοι"), "Einai oi grizoi");
    assert_eq!(g("Τι κάνεις;"), "Ti kaneis?");
    assert_eq!(g("Γεια σου"), "Geia sou");
    assert_eq!(g("ΕΛΛΑΔΑ"), "ELLADA");
    assert_eq!(g("Χαΐδεψέ με"), "Chaidepse me");
}

fn w(start_ms: u64, end_ms: u64, text: &str) -> Word {
    Word {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

#[test]
fn word_timing() {
    let line = "Я тебя люблю";
    let words = [w(0, 100, "Я "), w(100, 300, "тебя "), w(300, 600, "люблю")];
    let r = cyrillic::romanize(line, Lang::Russian);
    assert_eq!(
        lyrics_text::pieces::timed_words(line, &words, &r),
        vec![
            w(0, 100, "Ya "),
            w(100, 300, "tebya "),
            w(300, 600, "lyublyu")
        ]
    );
    let line = "Σ' αγαπώ";
    let words = [w(0, 100, "Σ' "), w(100, 300, "αγαπώ")];
    let r = greek::romanize(line);
    assert_eq!(
        lyrics_text::pieces::timed_words(line, &words, &r),
        vec![w(0, 100, "S' "), w(100, 300, "agapo")]
    );
}

#[test]
fn speed_per_line() {
    let t = std::time::Instant::now();
    let n = 2_000;
    for _ in 0..n {
        std::hint::black_box(cyrillic::romanize(
            std::hint::black_box("Звезда по имени Солнце, и мы ещё живы"),
            Lang::Russian,
        ));
        std::hint::black_box(greek::romanize(std::hint::black_box(
            "Σ' αγαπώ, σε θέλω, ευχαριστώ για όλα",
        )));
    }
    let per = t.elapsed() / (2 * n);
    println!("cyrillic and greek: {per:?} per line");
    assert!(per < std::time::Duration::from_millis(1), "{per:?}");
}
