//! Apple-style TTML, as Apple Music writes it and as AMLL's lyrics library and Lyrics+ pass it
//! on: `<p>` lines, `<span>` words, and the second lines they carry.
//!
//! Where the second lines are:
//! - inside a line: `<span ttm:role="x-roman">` (its pronunciation) and
//!   `<span ttm:role="x-translation" xml:lang="zh-CN">` (its meaning), AMLL's way;
//! - in the head, Apple's way: `<iTunesMetadata><transliterations><transliteration
//!   xml:lang="ja-Latn"><text for="L1">…` and `<translations><translation xml:lang="en"><text
//!   for="L1">…`, keyed to the line's `itunes:key="L1"`. A transliteration may time its own
//!   words (`<span begin end>`), and that timing is kept.
//!
//! Background vocals (`<span ttm:role="x-bg">`) are sung words like any other: they join the
//! line. Nothing here reads files or the network.

use std::collections::HashMap;

use super::{
    LyricLine, Lyrics, Second, SecondSource, Word, finish, romaji, roman_lang, timed_by_chars,
    translation_lang,
};

/// The lyrics a TTML text holds, their second lines marked as `provider`'s, and its words as
/// text when no line is timed (Apple's `itunes:timing="None"`).
pub fn read(xml: &str, provider: &str) -> (Lyrics, String) {
    let Some(root) = parse(xml) else {
        return (Lyrics::default(), String::new());
    };
    let song_lang = root
        .attr("lang")
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "und")
        .map(str::to_owned);
    let side = Sidecar::of(&root);
    let mut lines = Vec::new();
    let mut plain = Vec::new();
    let body = root.find("body");
    for p in body.iter().flat_map(|b| b.all("p")) {
        let Some(raw) = read_p(p) else { continue };
        let Some(start_ms) = raw.start_ms else {
            plain.push(raw.text);
            continue;
        };
        let mut line = LyricLine {
            start_ms,
            end_ms: raw.end_ms,
            text: raw.text,
            words: raw.words,
            ..Default::default()
        };
        line.script = super::script_of(&line.text);
        let key = p.attr("key");
        let roman = key
            .and_then(|k| side.roman(k))
            .map(|r| (r.text.clone(), r.words.clone(), Some(r.lang.clone()), false))
            .or_else(|| {
                raw.roman
                    .map(|(text, words, lang)| (text, words, lang, true))
            });
        if let Some((text, words, lang, spaced)) = roman {
            line.roman = roman_of(
                &line,
                &text,
                words,
                lang,
                spaced,
                song_lang.as_deref(),
                provider,
            );
        }
        let translation = key
            .and_then(|k| side.translation(k))
            .map(|(text, lang)| (text.to_owned(), Some(lang.to_owned())))
            .or(raw.translation);
        if let Some((text, lang)) = translation.filter(|(t, _)| !t.is_empty() && *t != line.text) {
            line.translation = Some(Second {
                lang: lang
                    .filter(|l| !l.is_empty())
                    .unwrap_or_else(|| translation_lang(&text, provider)),
                text,
                words: Vec::new(),
                source: SecondSource::Lyrics(provider.to_owned()),
            });
        }
        lines.push(line);
    }
    lines.sort_by_key(|l| l.start_ms);
    finish(&mut lines);
    let text = if lines.is_empty() {
        plain.join("\n")
    } else {
        String::new()
    };
    (
        Lyrics {
            lines,
            lang: song_lang,
            version: 0,
        },
        text,
    )
}

/// A pronunciation the lyrics carried, as a second line: AMLL's Japanese and Korean, sent
/// syllable by syllable ("mu te ki no"), joined into words and timed by the line's words; any
/// other kept as sent (Apple's transliterations keep their own word timing).
fn roman_of(
    line: &LyricLine,
    text: &str,
    words: Vec<Word>,
    lang: Option<String>,
    spaced: bool,
    song_lang: Option<&str>,
    provider: &str,
) -> Option<Second> {
    let text = collapse(text);
    if text.is_empty() || text == line.text || line.script == "Latn" {
        return None;
    }
    let mut out = Second {
        text: text.clone(),
        words,
        lang: lang
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| roman_lang(song_lang, &line.script)),
        source: SecondSource::Lyrics(provider.to_owned()),
    };
    if spaced && out.words.is_empty() {
        let original: String = if line.words.is_empty() {
            line.text.clone()
        } else {
            line.words.iter().map(|w| w.text.as_str()).collect()
        };
        if let Some(joined) = romaji::join_syllables_at(&original, &text) {
            out.text = joined
                .iter()
                .map(|(w, _)| w.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            out.words = timed_by_chars(&line.words, &joined);
        }
    }
    Some(out)
}

/// Runs of spaces as one, ends trimmed.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A TTML time: "12.345", "1:02.5", "00:01:02.500", "12.3s" or "1234ms".
pub fn time_ms(t: &str) -> Option<u64> {
    let t = t.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms
            .trim()
            .parse::<f64>()
            .ok()
            .map(|x| x.max(0.0).round() as u64);
    }
    let t = t.strip_suffix('s').unwrap_or(t);
    let mut secs = 0.0;
    for part in t.split(':') {
        let part = part.trim();
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return None;
        }
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some((secs * 1000.0).round() as u64)
}

// --- A line ---------------------------------------------------------------------------------

/// A `<p>` as read: its times (None when untimed), words, text and the second lines inside it.
struct RawLine {
    start_ms: Option<u64>,
    end_ms: u64,
    text: String,
    words: Vec<Word>,
    /// Text, its own timed words, its language.
    roman: Option<(String, Vec<Word>, Option<String>)>,
    /// Text and language.
    translation: Option<(String, Option<String>)>,
}

/// The begin and end of an element: `begin` and `end`, or `begin` and `dur`.
fn span_of(n: &Node) -> (Option<u64>, Option<u64>) {
    let begin = n.attr("begin").and_then(time_ms);
    let end = n.attr("end").and_then(time_ms).or_else(|| {
        let d = n.attr("dur").and_then(time_ms)?;
        Some(begin? + d)
    });
    (begin, end)
}

fn read_p(p: &Node) -> Option<RawLine> {
    let mut words: Vec<Word> = Vec::new();
    let mut plain = String::new();
    let mut roman = None;
    let mut translation = None;
    sung(p, &mut words, &mut plain, &mut roman, &mut translation);
    let text = if words.is_empty() {
        collapse(&plain)
    } else {
        collapse(&words.iter().map(|w| w.text.as_str()).collect::<String>())
    };
    if text.is_empty() {
        return None;
    }
    let (begin, end) = span_of(p);
    let start_ms = begin.or_else(|| words.first().map(|w| w.start_ms));
    let end_ms = end
        .or_else(|| words.iter().map(|w| w.end_ms).max())
        .unwrap_or(0);
    Some(RawLine {
        start_ms,
        end_ms,
        text,
        words,
        roman,
        translation,
    })
}

/// The sung words of an element (a `<p>`, or a background `<span>` inside it), in order, and
/// the second lines it carries. A space after a word belongs to the word.
fn sung(
    el: &Node,
    words: &mut Vec<Word>,
    plain: &mut String,
    roman: &mut Option<(String, Vec<Word>, Option<String>)>,
    translation: &mut Option<(String, Option<String>)>,
) {
    for child in &el.children {
        match child {
            Child::Text(t) => match words.last_mut() {
                Some(last) if t.trim().is_empty() => {
                    if !last.text.ends_with(' ') {
                        last.text.push(' ');
                    }
                }
                Some(last) => last.text.push_str(t),
                None => plain.push_str(t),
            },
            Child::El(n) if n.name == "span" => match n.attr("role") {
                Some("x-translation") => {
                    if translation.is_none() {
                        *translation =
                            Some((collapse(&n.text()), n.attr("lang").map(str::to_owned)));
                    }
                }
                Some("x-roman") => {
                    if roman.is_none() {
                        *roman =
                            Some((n.text(), timed_words(n), n.attr("lang").map(str::to_owned)));
                    }
                }
                // Background vocals: sung, so they join the line (their own second lines
                // aren't kept: the line has one of each).
                Some("x-bg") => sung(n, words, plain, &mut None, &mut None),
                _ => match span_of(n) {
                    (Some(start_ms), end) => words.push(Word {
                        start_ms,
                        end_ms: end.unwrap_or(start_ms).max(start_ms),
                        text: n.text(),
                    }),
                    _ => match words.last_mut() {
                        Some(last) => last.text.push_str(&n.text()),
                        None => plain.push_str(&n.text()),
                    },
                },
            },
            Child::El(n) => plain.push_str(&n.text()),
        }
    }
}

/// The timed words of a second line written word by word (`<span begin end>`), each with the
/// space after it.
fn timed_words(el: &Node) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    for child in &el.children {
        match child {
            Child::El(n) if n.name == "span" => {
                if let (Some(start_ms), end) = span_of(n) {
                    words.push(Word {
                        start_ms,
                        end_ms: end.unwrap_or(start_ms).max(start_ms),
                        text: n.text(),
                    });
                }
            }
            Child::Text(t) if t.trim().is_empty() => {
                if let Some(last) = words.last_mut()
                    && !last.text.ends_with(' ')
                {
                    last.text.push(' ');
                }
            }
            Child::Text(t) => {
                if let Some(last) = words.last_mut() {
                    last.text.push_str(t);
                }
            }
            Child::El(_) => {}
        }
    }
    if let Some(last) = words.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    words
}

// --- Apple's sidecar ------------------------------------------------------------------------

struct Roman {
    text: String,
    words: Vec<Word>,
    lang: String,
}

/// The second lines in `<head><metadata><iTunesMetadata>`, by line key.
#[derive(Default)]
struct Sidecar {
    romans: HashMap<String, Roman>,
    /// By key: every language's translation, in file order.
    translations: HashMap<String, Vec<(String, String)>>,
}

impl Sidecar {
    fn of(root: &Node) -> Sidecar {
        let mut side = Sidecar::default();
        let Some(meta) = root.find("head").and_then(|h| h.find("iTunesMetadata")) else {
            return side;
        };
        for set in meta.all("transliteration") {
            let lang = set.attr("lang").unwrap_or("").trim().to_owned();
            for t in set.all("text") {
                let Some(key) = t.attr("for") else { continue };
                let words = timed_words(t);
                let text = if words.is_empty() {
                    collapse(&t.text())
                } else {
                    collapse(&words.iter().map(|w| w.text.as_str()).collect::<String>())
                };
                side.romans.entry(key.to_owned()).or_insert(Roman {
                    text,
                    words,
                    lang: lang.clone(),
                });
            }
        }
        for set in meta.all("translation") {
            let lang = set.attr("lang").unwrap_or("").trim().to_owned();
            for t in set.all("text") {
                let Some(key) = t.attr("for") else { continue };
                side.translations
                    .entry(key.to_owned())
                    .or_default()
                    .push((collapse(&t.text()), lang.clone()));
            }
        }
        side
    }

    fn roman(&self, key: &str) -> Option<&Roman> {
        self.romans.get(key).filter(|r| !r.text.is_empty())
    }

    /// A line's translation (text, language). The contract keeps one: English when the file
    /// has it (the language most listeners read, and the default), else the first.
    fn translation(&self, key: &str) -> Option<(&str, &str)> {
        let all = self.translations.get(key)?;
        let usable = || all.iter().filter(|(t, _)| !t.is_empty());
        usable()
            .find(|(_, l)| l == "en" || l.starts_with("en-"))
            .or_else(|| usable().next())
            .map(|(t, l)| (t.as_str(), l.as_str()))
    }
}

// --- A small XML reader ---------------------------------------------------------------------

/// An element: its local name ("span" for `tt:span`), its attributes by their written names
/// ("ttm:role"), and what is inside it.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
}

#[derive(Debug)]
enum Child {
    El(Node),
    Text(String),
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl Node {
    /// An attribute by its local name ("role" finds `ttm:role`, "lang" finds `xml:lang`).
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .or_else(|| self.attrs.iter().find(|(k, _)| local(k) == name))
            .map(|(_, v)| v.as_str())
    }

    /// The first element of that name inside, at any depth.
    fn find(&self, name: &str) -> Option<&Node> {
        self.children.iter().find_map(|c| match c {
            Child::El(n) if n.name == name => Some(n),
            Child::El(n) => n.find(name),
            Child::Text(_) => None,
        })
    }

    /// Every element of that name inside, at any depth, in document order (not inside one
    /// another).
    fn all<'a>(&'a self, name: &'a str) -> Vec<&'a Node> {
        let mut out = Vec::new();
        self.collect(name, &mut out);
        out
    }

    fn collect<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if let Child::El(n) = c {
                if n.name == name {
                    out.push(n);
                } else {
                    n.collect(name, out);
                }
            }
        }
    }

    /// All the text inside, as written.
    fn text(&self) -> String {
        let mut out = String::new();
        self.push_text(&mut out);
        out
    }

    fn push_text(&self, out: &mut String) {
        for c in &self.children {
            match c {
                Child::Text(t) => out.push_str(t),
                Child::El(n) => n.push_text(out),
            }
        }
    }
}

/// `&amp;`, `&#39;`, `&#x4E00;` and the other references, as their characters; an unknown one
/// is kept as written.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let resolved = after.find(';').filter(|&j| j <= 10).and_then(|j| {
            let name = &after[..j];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name.strip_prefix('#').and_then(|n| {
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            }?;
            Some((c, j))
        });
        match resolved {
            Some((c, j)) => {
                out.push(c);
                rest = &after[j + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A start tag's name and attributes (`p begin="1.0" itunes:key="L1"`).
fn tag_of(inner: &str) -> Node {
    let inner = inner.trim();
    let name_end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let mut node = Node {
        name: local(&inner[..name_end]).to_owned(),
        ..Default::default()
    };
    let mut rest = &inner[name_end..];
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_owned();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(quote) else {
            break;
        };
        node.attrs.push((key, unescape(&after[1..1 + close])));
        rest = &after[close + 2..];
    }
    node
}

/// The document's root element. Lenient: an end tag closes the nearest open element of its
/// name, and elements left open close at the end.
fn parse(xml: &str) -> Option<Node> {
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut rest = xml;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            push_text(&mut stack, rest);
            break;
        };
        if lt > 0 {
            push_text(&mut stack, &rest[..lt]);
        }
        let tag = &rest[lt..];
        let skip_to = |end: &str| tag.find(end).map(|i| i + end.len());
        if let Some(cdata) = tag.strip_prefix("<![CDATA[") {
            let end = cdata.find("]]>").unwrap_or(cdata.len());
            if let Some(top) = stack.last_mut() {
                top.children.push(Child::Text(cdata[..end].to_owned()));
            }
            rest = cdata.get(end + 3..).unwrap_or("");
            continue;
        }
        let skipped = if tag.starts_with("<!--") {
            skip_to("-->")
        } else if tag.starts_with("<?") {
            skip_to("?>")
        } else if tag.starts_with("<!") {
            skip_to(">")
        } else {
            None
        };
        if tag.starts_with("<!") || tag.starts_with("<?") {
            rest = skipped.map_or("", |i| &tag[i..]);
            continue;
        }
        let Some(gt) = tag.find('>') else { break };
        let inner = &tag[1..gt];
        rest = &tag[gt + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            let name = local(name.trim());
            if let Some(at) = stack.iter().rposition(|n| n.name == name)
                && at > 0
            {
                while stack.len() > at {
                    close(&mut stack);
                }
            }
        } else if let Some(inner) = inner.strip_suffix('/') {
            let node = tag_of(inner);
            if let Some(top) = stack.last_mut() {
                top.children.push(Child::El(node));
            }
        } else {
            stack.push(tag_of(inner));
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    let doc = stack.pop()?;
    doc.children.into_iter().find_map(|c| match c {
        Child::El(n) => Some(n),
        Child::Text(_) => None,
    })
}

fn push_text(stack: &mut [Node], text: &str) {
    if let Some(top) = stack.last_mut() {
        top.children.push(Child::Text(unescape(text)));
    }
}

fn close(stack: &mut Vec<Node>) {
    if let Some(done) = stack.pop()
        && let Some(parent) = stack.last_mut()
    {
        parent.children.push(Child::El(done));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/lyrics")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn timed(words: &[Word]) -> Vec<(u64, u64, &str)> {
        words
            .iter()
            .map(|w| (w.start_ms, w.end_ms, w.text.as_str()))
            .collect()
    }

    #[test]
    fn lines_words_and_plain_lines() {
        let xml = r#"<tt><body><div><p begin="00:01.000" end="00:03.000"><span begin="00:01.000" end="00:01.500">Hel</span><span begin="00:01.500" end="00:02.000">lo</span> <span begin="00:02.100" end="00:03.000">you</span></p><p begin="4.5" end="6">Plain &amp; line</p><p begin="7s" dur="1.5s">Dur &#x263A; &#39;n&#39;</p></div></body></tt>"#;
        let (l, text) = read(xml, "boidu");
        assert!(text.is_empty());
        let lines = &l.lines;
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].start_ms, lines[0].end_ms), (1000, 3000));
        assert_eq!(lines[0].text, "Hello you");
        assert_eq!(
            timed(&lines[0].words),
            [
                (1000, 1500, "Hel"),
                (1500, 2000, "lo "),
                (2100, 3000, "you")
            ]
        );
        assert_eq!(lines[1].text, "Plain & line");
        assert_eq!((lines[2].start_ms, lines[2].end_ms), (7000, 8500));
        assert_eq!(lines[2].text, "Dur \u{263A} 'n'");
        // Untimed lyrics (Apple's itunes:timing="None") are words, not lines.
        let (l, text) = read(
            r#"<tt itunes:timing="None"><body><div><p>One</p><p>Two</p></div></body></tt>"#,
            "boidu",
        );
        assert!(l.lines.is_empty());
        assert_eq!(text, "One\nTwo");
        assert_eq!(read("not xml at all", "x").0.lines, []);
    }

    #[test]
    fn apples_sidecar_pronunciation_keeps_its_word_timing() {
        // Apple's layout (as the AMLL guide and Lyrics+'s TTML writer show it): the
        // transliteration and the translations in the head, keyed to each line's itunes:key.
        let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric-ttml-internal" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" itunes:timing="Word" xml:lang="ja"><head><metadata><iTunesMetadata xmlns="http://music.apple.com/lyric-ttml-internal"><translations><translation type="subtitle" xml:lang="zh-Hans"><text for="L1">露出无敌笑容</text></translation><translation type="subtitle" xml:lang="en"><text for="L1">Taking the media by storm with an invincible smile</text></translation></translations><transliterations><transliteration xml:lang="ja-Latn"><text for="L1"><span begin="0.589" end="1.2">muteki</span> <span begin="1.2" end="1.4">no</span> <span begin="1.4" end="2.0">egao</span></text><text for="L2">shiritai sono himitsu</text></transliteration></transliterations></iTunesMetadata></metadata></head><body><div><p begin="0.589" end="2.5" itunes:key="L1"><span begin="0.589" end="1.2">無敵</span><span begin="1.2" end="1.4">の</span><span begin="1.4" end="2.0">笑顔</span></p><p begin="3.491" end="6" itunes:key="L2">知りたいその秘密</p></div></body></tt>"#;
        let (l, _) = read(xml, "lyricsplus");
        assert_eq!(l.lang.as_deref(), Some("ja"));
        let first = &l.lines[0];
        assert_eq!(first.text, "無敵の笑顔");
        let r = first.roman.as_ref().unwrap();
        assert_eq!(r.text, "muteki no egao");
        assert_eq!(r.lang, "ja-Latn");
        assert_eq!(r.source, SecondSource::Lyrics("lyricsplus".into()));
        assert_eq!(
            timed(&r.words),
            [
                (589, 1200, "muteki "),
                (1200, 1400, "no "),
                (1400, 2000, "egao")
            ],
            "Apple's own word timing"
        );
        let t = first.translation.as_ref().unwrap();
        assert_eq!(
            (t.text.as_str(), t.lang.as_str()),
            ("Taking the media by storm with an invincible smile", "en"),
            "English when the file has it"
        );
        let second = &l.lines[1];
        let r = second.roman.as_ref().unwrap();
        assert_eq!(
            (r.text.as_str(), r.words.len()),
            ("shiritai sono himitsu", 0)
        );
        assert_eq!(second.translation, None);
    }

    #[test]
    fn amll_lines_carry_their_pronunciation_and_translation_inside() {
        // Yoru ni Kakeru (YOASOBI) in AMLL's lyrics library, as it was on 2026-10-22: every line
        // has a romanization sent syllable by syllable and a Chinese translation.
        let (l, _) = read(&fixture("amll-yoru-ni-kakeru.ttml"), "amll");
        assert_eq!(l.lines.len(), 56);
        let first = &l.lines[0];
        assert!(first.text.starts_with("沈むように"), "{}", first.text);
        let r = first.roman.as_ref().unwrap();
        // Joined the way NetEase's syllables are (romaji::join_syllables), spelled as sent.
        assert_eq!(r.text, "sizumuyouni toketeyukuyouni");
        assert_eq!(r.source, SecondSource::Lyrics("amll".into()));
        assert_eq!(r.lang, "ja-Latn", "a Japanese line, by its kana");
        // Timed by the original's words: each romanized word from its first syllable's start
        // to its last one's end.
        assert_eq!(r.words.len(), 2);
        assert_eq!(r.words[0].start_ms, first.words[0].start_ms);
        assert_eq!(
            r.words.last().unwrap().end_ms,
            first.words.last().unwrap().end_ms
        );
        assert_eq!(
            r.words.iter().map(|w| w.text.as_str()).collect::<String>(),
            r.text,
            "the words put together are the line"
        );
        let t = first.translation.as_ref().unwrap();
        assert_eq!(
            (t.text.as_str(), t.lang.as_str()),
            ("像是沉溺溶化一般", "zh-CN"),
            "the language the file names"
        );
        assert!(
            !first.text.contains("像是"),
            "a translation is never sung text"
        );
        let quoted = &l.lines[2];
        assert_eq!(
            quoted.roman.as_ref().map(|r| r.text.as_str()),
            Some("「sa yo na ra」da ke da tta"),
            "syllables that don't line up with the line are kept as sent"
        );
        assert!(l.lines.iter().all(
            |l| l.roman.is_some() && l.translation.as_ref().is_some_and(|t| t.lang == "zh-CN")
        ));
    }

    #[test]
    fn background_vocals_are_sung_and_their_translation_is_not() {
        // Idol (YOASOBI) in AMLL's library: Chinese translations, no romanization, one line
        // with background vocals.
        let (l, _) = read(&fixture("amll-idol.ttml"), "amll");
        assert_eq!(l.lines[0].text, "無敵の笑顔で荒らすメディア");
        assert_eq!(l.lines[0].words.len(), 12);
        assert!(l.lines.iter().all(|l| l.roman.is_none()));
        assert_eq!(
            l.lines
                .iter()
                .filter(|l| l.translation.as_ref().is_some_and(|t| t.lang == "zh-CN"))
                .count(),
            l.lines.len()
        );
        assert!(
            l.lines.iter().all(|l| l
                .translation
                .as_ref()
                .is_none_or(|t| !l.text.contains(&t.text))),
            "no translation leaks into the sung line"
        );
    }

    #[test]
    fn inline_pronunciation_on_a_korean_line_and_a_line_already_in_latin() {
        let xml = r#"<tt><body><div><p begin="1" end="3"><span begin="1" end="2">사랑</span><span begin="2" end="3">해</span><span ttm:role="x-roman">sa rang hae</span></p><p begin="4" end="5">Hello<span ttm:role="x-roman">Hello</span><span ttm:role="x-translation">Ni hao</span></p></div></body></tt>"#;
        let (l, _) = read(xml, "amll");
        let r = l.lines[0].roman.as_ref().unwrap();
        assert_eq!(r.text, "saranghae");
        assert_eq!(r.lang, "ko-Latn");
        assert_eq!(timed(&r.words), [(1000, 3000, "saranghae")]);
        assert_eq!(l.lines[1].roman, None, "nothing to say for a Latin line");
        assert_eq!(
            l.lines[1].translation.as_ref().map(|t| t.lang.as_str()),
            Some("und-Latn"),
            "no language named: told by its letters"
        );
    }

    #[test]
    fn times_and_references() {
        assert_eq!(time_ms("12.345"), Some(12_345));
        assert_eq!(time_ms("1:02.5"), Some(62_500));
        assert_eq!(time_ms("00:01:02.500"), Some(62_500));
        assert_eq!(time_ms("1234ms"), Some(1234));
        assert_eq!(time_ms("3.5s"), Some(3500));
        assert_eq!(time_ms("soon"), None);
        assert_eq!(
            unescape("a &amp; b &lt;c&gt; &#20320; &bogus; &"),
            "a & b <c> 你 &bogus; &"
        );
    }
}
