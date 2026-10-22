//! The engines that run with the listener's own key (DeepL, Microsoft, Google, Claude), each
//! against a local mock server: what is sent (the address, the key's header, the body), a
//! song's lines in as few requests as the service allows, one line back for every line sent,
//! waiting when asked to, and every failure in plain words. No real service is called.

mod mock;

use lyrics_translate::cache::SongCache;
use lyrics_translate::keyed::{self, Credentials, Keyed, Options};
use lyrics_translate::{Error, Translator, translate_song};
use mock::{Mock, Reply, busy, nowhere, reply};
use serde_json::json;

const HI: [&str; 3] = ["तुम ही हो", "अब तुम ही हो", "ज़िन्दगी अब तुम ही हो"];

fn engine(which: Keyed, key: &str, mock: &Mock) -> Box<dyn Translator> {
    keyed::engine(which, Credentials::new(key), Options::at(&mock.url))
}

/// DeepL's answer for `n` lines: "T0", "T1", ...
fn deepl_ok(texts: &[serde_json::Value]) -> Reply {
    reply(
        200,
        json!({ "translations": texts.iter().enumerate().map(|(i, _)| json!({"detected_source_language": "HI", "text": format!("T{i}")})).collect::<Vec<_>>() }),
    )
}

fn echo_deepl() -> Mock {
    Mock::answering(Box::new(|r| {
        let body = r.json();
        deepl_ok(body["text"].as_array().unwrap())
    }))
}

// --- The key's engines, by name ---------------------------------------------------------------

#[test]
fn the_engines_and_their_names() {
    let ids: Vec<&str> = Keyed::ALL.iter().map(|k| k.id()).collect();
    assert_eq!(ids, ["deepl", "microsoft", "google", "llm"]);
    assert_eq!(Keyed::from_id("deepl"), Some(Keyed::DeepL));
    assert_eq!(Keyed::from_id("bergamot"), None);
    assert_eq!(Keyed::Llm.name(), "Claude");
    assert_eq!(Keyed::Microsoft.name(), "Microsoft Translator");
    // The real addresses (never called by these tests).
    assert_eq!(
        keyed::deepl::endpoint_for("abc:fx"),
        "https://api-free.deepl.com"
    );
    assert_eq!(keyed::deepl::endpoint_for("abc"), "https://api.deepl.com");
    assert_eq!(keyed::claude::DEFAULT_MODEL, "claude-sonnet-5-5");
}

#[test]
fn a_key_never_shows_in_debug_output_or_errors() {
    let c = Credentials::new("secret-key-123:fx").with_region("westeurope");
    assert!(!format!("{c:?}").contains("secret-key-123"));
    let mock = Mock::new(vec![reply(
        403,
        json!({"message": "Wrong key secret-key-123:fx"}),
    )]);
    let e = engine(Keyed::DeepL, "secret-key-123:fx", &mock)
        .translate("hi", "en", &HI)
        .unwrap_err();
    assert!(!e.to_string().contains("secret-key-123"), "{e}");
    assert!(!format!("{e:?}").contains("secret-key-123"), "{e:?}");
}

/// What an error should be.
type Check = fn(&Error) -> bool;

// --- DeepL ------------------------------------------------------------------------------------

#[test]
fn deepl_sends_the_song_in_one_request_with_its_key_header() {
    let mock = echo_deepl();
    let out = engine(Keyed::DeepL, "k:fx", &mock)
        .translate("hi", "en", &HI)
        .unwrap();
    assert_eq!(out, ["T0", "T1", "T2"]);
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.target, "/v2/translate");
    assert_eq!(r.header("authorization"), Some("DeepL-Auth-Key k:fx"));
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert!(r.header("user-agent").unwrap().starts_with("Venuta"));
    let b = r.json();
    assert_eq!(b["text"], json!(HI));
    assert_eq!(b["source_lang"], "HI");
    assert_eq!(b["target_lang"], "EN-US");
    // One line is one sentence: never split or joined.
    assert_eq!(b["split_sentences"], "0");
    // The whole song as context (not translated, not counted against the quota).
    assert!(b["context"].as_str().unwrap().contains("अब तुम ही हो"));
}

#[test]
fn deepl_language_codes() {
    let mock = echo_deepl();
    let e = engine(Keyed::DeepL, "k", &mock);
    for (into, want) in [
        ("en-GB", "EN-GB"),
        ("pt", "PT-BR"),
        ("pt-PT", "PT-PT"),
        ("zh", "ZH-HANS"),
        ("zh-TW", "ZH-HANT"),
        ("hi", "HI"),
    ] {
        e.translate("es", into, &["Hola"]).unwrap();
        let b = mock.requests().last().unwrap().json();
        assert_eq!(b["target_lang"], want, "{into}");
        assert_eq!(b["source_lang"], "ES");
    }
}

#[test]
fn deepl_batches_fifty_lines_a_request_and_keeps_the_order() {
    let mock = Mock::answering(Box::new(|r| {
        let body = r.json();
        let texts = body["text"].as_array().unwrap();
        reply(
            200,
            json!({ "translations": texts.iter().map(|t| json!({"text": format!("EN {}", t.as_str().unwrap())})).collect::<Vec<_>>() }),
        )
    }));
    let song: Vec<String> = (0..120).map(|i| format!("línea {i}")).collect();
    let lines: Vec<&str> = song.iter().map(String::as_str).collect();
    let out = engine(Keyed::DeepL, "k", &mock)
        .translate("es", "en", &lines)
        .unwrap();
    assert_eq!(out.len(), 120);
    assert_eq!(out[0], "EN línea 0");
    assert_eq!(out[119], "EN línea 119");
    let sizes: Vec<usize> = mock
        .requests()
        .iter()
        .map(|r| r.json()["text"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [50, 50, 20]);
}

#[test]
fn deepl_failures_in_plain_words() {
    let cases: Vec<(Reply, Check)> = vec![
        (reply(403, json!({"message": "Forbidden"})), |e| {
            matches!(e, Error::BadKey { .. })
        }),
        (reply(456, json!({"message": "Quota exceeded"})), |e| {
            matches!(e, Error::Quota { .. })
        }),
        (
            reply(
                400,
                json!({"message": "Value for 'target_lang' not supported."}),
            ),
            |e| matches!(e, Error::Refused { detail, .. } if detail.contains("target_lang")),
        ),
        (reply(500, json!({})), |e| {
            matches!(e, Error::Refused { .. })
        }),
    ];
    for (r, ok) in cases {
        let mock = Mock::new(vec![r]);
        let e = engine(Keyed::DeepL, "k", &mock)
            .translate("hi", "en", &HI)
            .unwrap_err();
        assert!(ok(&e), "{e:?}");
        assert!(e.to_string().contains("DeepL"), "{e}");
    }
    let e = engine(Keyed::DeepL, "k", &Mock::new(vec![reply(403, json!({}))]))
        .translate("hi", "en", &HI)
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "DeepL didn't accept your key. Check it in Settings > Lyrics."
    );
}

#[test]
fn deepl_waits_when_asked_and_tries_again() {
    let mock = Mock::new(vec![
        busy(429, 0, json!({"message": "Too many requests"})),
        deepl_ok(&[json!(1), json!(2), json!(3)]),
    ]);
    let out = engine(Keyed::DeepL, "k", &mock)
        .translate("hi", "en", &HI)
        .unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(mock.requests().len(), 2);
    // Still busy after the retries: said so, not hammered.
    let mock = Mock::new(vec![busy(429, 0, json!({}))]);
    let e = engine(Keyed::DeepL, "k", &mock)
        .translate("hi", "en", &HI)
        .unwrap_err();
    assert!(matches!(e, Error::Busy { .. }), "{e:?}");
    assert_eq!(mock.requests().len(), 3);
}

#[test]
fn deepl_one_line_back_for_every_line_sent() {
    let mock = Mock::new(vec![deepl_ok(&[json!(1), json!(2)])]);
    let e = engine(Keyed::DeepL, "k", &mock)
        .translate("hi", "en", &HI)
        .unwrap_err();
    assert!(
        matches!(
            e,
            Error::Lines {
                expected: 3,
                got: 2
            }
        ),
        "{e:?}"
    );
}

#[test]
fn no_connection_is_said_plainly() {
    for which in Keyed::ALL {
        let e = keyed::engine(which, Credentials::new("k"), Options::at(&nowhere()))
            .translate("hi", "en", &HI)
            .unwrap_err();
        assert!(matches!(e, Error::Unreachable { .. }), "{which:?}: {e:?}");
        assert!(e.to_string().starts_with("Couldn't reach"), "{e}");
    }
}

#[test]
fn no_key_no_request() {
    let mock = echo_deepl();
    for which in Keyed::ALL {
        let e = engine(which, "  ", &mock)
            .translate("hi", "en", &HI)
            .unwrap_err();
        assert!(matches!(e, Error::NoKey { .. }), "{e:?}");
    }
    assert!(mock.requests().is_empty());
}

// --- Microsoft --------------------------------------------------------------------------------

fn echo_microsoft() -> Mock {
    Mock::answering(Box::new(|r| {
        let body = r.json();
        let items = body.as_array().unwrap();
        reply(
            200,
            json!(items.iter().map(|t| json!({"translations": [{"text": format!("EN {}", t["Text"].as_str().unwrap()), "to": "en"}]})).collect::<Vec<_>>()),
        )
    }))
}

#[test]
fn microsoft_sends_the_key_and_its_region() {
    let mock = echo_microsoft();
    let e = keyed::engine(
        Keyed::Microsoft,
        Credentials::new("mskey").with_region("centralindia"),
        Options::at(&mock.url),
    );
    let out = e.translate("hi", "en", &HI).unwrap();
    assert_eq!(out[0], "EN तुम ही हो");
    let r = &mock.requests()[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.target, "/translate?api-version=3.0&from=hi&to=en");
    assert_eq!(r.header("ocp-apim-subscription-key"), Some("mskey"));
    assert_eq!(
        r.header("ocp-apim-subscription-region"),
        Some("centralindia")
    );
    assert_eq!(
        r.json(),
        json!([{"Text": HI[0]}, {"Text": HI[1]}, {"Text": HI[2]}])
    );
    // A global resource sends no region; Chinese has its script.
    let e = keyed::engine(
        Keyed::Microsoft,
        Credentials::new("mskey").with_region("global"),
        Options::at(&mock.url),
    );
    e.translate("es", "zh-TW", &["Hola"]).unwrap();
    let r = mock.requests().last().unwrap().clone();
    assert_eq!(r.header("ocp-apim-subscription-region"), None);
    assert_eq!(r.target, "/translate?api-version=3.0&from=es&to=zh-Hant");
}

#[test]
fn microsoft_batches_by_lines_and_by_characters() {
    let mock = echo_microsoft();
    let song: Vec<String> = (0..250).map(|i| format!("línea {i}")).collect();
    let lines: Vec<&str> = song.iter().map(String::as_str).collect();
    let e = engine(Keyed::Microsoft, "k", &mock);
    assert_eq!(e.translate("es", "en", &lines).unwrap().len(), 250);
    let long = "a".repeat(4000);
    e.translate("es", "en", &[&long, &long, &long]).unwrap();
    let sizes: Vec<usize> = mock
        .requests()
        .iter()
        .map(|r| r.json().as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [100, 100, 50, 2, 1]);
}

#[test]
fn microsoft_failures_in_plain_words() {
    let err = |status, code| {
        reply(
            status,
            json!({"error": {"code": code, "message": "something"}}),
        )
    };
    let e = |r: Reply| {
        engine(Keyed::Microsoft, "k", &Mock::new(vec![r]))
            .translate("hi", "en", &HI)
            .unwrap_err()
    };
    assert!(matches!(e(err(401, 401000)), Error::BadKey { .. }));
    assert!(matches!(e(err(403, 403001)), Error::Quota { .. }));
    assert!(matches!(e(err(400, 400036)), Error::Refused { .. }));
    assert!(matches!(e(busy(429, 0, json!({}))), Error::Busy { .. }));
    let bad = e(err(401, 401000)).to_string();
    assert!(
        bad.contains("Microsoft Translator") && bad.contains("region"),
        "{bad}"
    );
}

// --- Google -----------------------------------------------------------------------------------

fn echo_google() -> Mock {
    Mock::answering(Box::new(|r| {
        let body = r.json();
        let q = body["q"].as_array().unwrap();
        reply(
            200,
            json!({"data": {"translations": q.iter().map(|t| json!({"translatedText": format!("EN {}", t.as_str().unwrap())})).collect::<Vec<_>>()}}),
        )
    }))
}

#[test]
fn google_sends_the_key_in_a_header_never_in_the_address() {
    let mock = echo_google();
    let out = engine(Keyed::Google, "gkey", &mock)
        .translate("hi", "en", &HI)
        .unwrap();
    assert_eq!(out[2], "EN ज़िन्दगी अब तुम ही हो");
    let r = &mock.requests()[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.target, "/language/translate/v2");
    assert!(!r.target.contains("gkey"));
    assert_eq!(r.header("x-goog-api-key"), Some("gkey"));
    let b = r.json();
    assert_eq!(b["q"], json!(HI));
    assert_eq!(b["source"], "hi");
    assert_eq!(b["target"], "en");
    // Plain text: no HTML entities back.
    assert_eq!(b["format"], "text");
    engine(Keyed::Google, "gkey", &mock)
        .translate("es", "zh-Hant", &["Hola"])
        .unwrap();
    assert_eq!(mock.requests().last().unwrap().json()["target"], "zh-TW");
}

#[test]
fn google_batches_128_lines_a_request() {
    let mock = echo_google();
    let song: Vec<String> = (0..200).map(|i| format!("línea {i}")).collect();
    let lines: Vec<&str> = song.iter().map(String::as_str).collect();
    let out = engine(Keyed::Google, "k", &mock)
        .translate("es", "en", &lines)
        .unwrap();
    assert_eq!(out[199], "EN línea 199");
    let sizes: Vec<usize> = mock
        .requests()
        .iter()
        .map(|r| r.json()["q"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [128, 72]);
}

#[test]
fn google_failures_in_plain_words() {
    let err = |status: u16, reason: &str, message: &str| {
        reply(
            status,
            json!({"error": {"code": status, "message": message, "errors": [{"reason": reason}], "status": "X", "details": [{"reason": reason}]}}),
        )
    };
    let e = |r: Reply| {
        engine(Keyed::Google, "k", &Mock::new(vec![r]))
            .translate("hi", "en", &HI)
            .unwrap_err()
    };
    assert!(matches!(
        e(err(
            400,
            "API_KEY_INVALID",
            "API key not valid. Please pass a valid API key."
        )),
        Error::BadKey { .. }
    ));
    assert!(matches!(
        e(err(403, "dailyLimitExceeded", "Daily Limit Exceeded")),
        Error::Quota { .. }
    ));
    let off = e(err(
        403,
        "accessNotConfigured",
        "Cloud Translation API has not been used",
    ));
    assert!(
        matches!(&off, Error::Refused { detail, .. } if detail.contains("turned on")),
        "{off:?}"
    );
    assert!(matches!(e(busy(429, 0, json!({}))), Error::Busy { .. }));
}

// --- Claude -----------------------------------------------------------------------------------

/// Claude's answer: `lines` as the JSON the prompt asks for.
fn claude_says(lines: &[&str]) -> Reply {
    reply(
        200,
        json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-sonnet-5-5",
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": json!({"lines": lines}).to_string()}],
        }),
    )
}

#[test]
fn claude_gets_the_whole_song_with_a_prompt_written_for_lyrics() {
    let mock = Mock::new(vec![claude_says(&[
        "You alone",
        "Now only you",
        "Life, now only you",
    ])]);
    let out = engine(Keyed::Llm, "sk-ant-x", &mock)
        .translate("hi", "en", &HI)
        .unwrap();
    assert_eq!(out, ["You alone", "Now only you", "Life, now only you"]);
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.target, "/v1/messages");
    assert_eq!(r.header("x-api-key"), Some("sk-ant-x"));
    assert_eq!(r.header("anthropic-version"), Some("2023-06-01"));
    let b = r.json();
    assert_eq!(b["model"], "claude-sonnet-5-5");
    assert!(b["max_tokens"].as_u64().unwrap() >= 1024);
    let system = b["system"].as_str().unwrap();
    for must in [
        "song lyrics",
        "exactly one",
        "tone",
        "Hinglish",
        "never add",
    ] {
        assert!(system.contains(must), "the prompt says {must:?}");
    }
    let user = b["messages"][0]["content"].as_str().unwrap();
    assert_eq!(b["messages"][0]["role"], "user");
    assert!(user.contains("Hindi") && user.contains("English"), "{user}");
    assert!(user.contains("3 lines"), "{user}");
    assert!(user.contains(&json!(HI).to_string()), "{user}");
    // The answer's shape is fixed by a schema.
    assert_eq!(b["output_config"]["format"]["type"], "json_schema");
    assert_eq!(
        b["output_config"]["format"]["schema"]["properties"]["lines"]["type"],
        "array"
    );
}

#[test]
fn claude_with_another_model_id() {
    let mock = Mock::new(vec![claude_says(&["a", "b", "c"])]);
    let e = keyed::engine(
        Keyed::Llm,
        Credentials::new("k"),
        Options {
            model: Some("claude-opus-5-5".into()),
            endpoint: Some(mock.url.clone()),
        },
    );
    assert_eq!(e.version("hi", "en").as_deref(), Some("claude-opus-5-5"));
    e.translate("hi", "en", &HI).unwrap();
    assert_eq!(mock.requests()[0].json()["model"], "claude-opus-5-5");
    // A blank model id is the default.
    let e = keyed::engine(
        Keyed::Llm,
        Credentials::new("k"),
        Options {
            model: Some("  ".into()),
            endpoint: Some(mock.url.clone()),
        },
    );
    assert_eq!(e.version("hi", "en").as_deref(), Some("claude-sonnet-5-5"));
}

#[test]
fn claude_asked_again_once_when_the_line_count_is_wrong() {
    let mock = Mock::new(vec![
        claude_says(&["a", "b"]),
        claude_says(&["a", "b", "c"]),
    ]);
    let out = engine(Keyed::Llm, "k", &mock)
        .translate("hi", "en", &HI)
        .unwrap();
    assert_eq!(out, ["a", "b", "c"]);
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 2);
    let again = reqs[1].json()["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(again.contains("exactly 3"), "{again}");
    // Wrong twice: an error, never misaligned lines.
    let mock = Mock::new(vec![claude_says(&["a", "b", "c", "d"])]);
    let e = engine(Keyed::Llm, "k", &mock)
        .translate("hi", "en", &HI)
        .unwrap_err();
    assert!(
        matches!(
            e,
            Error::Lines {
                expected: 3,
                got: 4
            }
        ),
        "{e:?}"
    );
    assert_eq!(mock.requests().len(), 2);
}

#[test]
fn claude_failures_in_plain_words() {
    let err = |status: u16, kind: &str, message: &str| {
        reply(
            status,
            json!({"type": "error", "error": {"type": kind, "message": message}}),
        )
    };
    let e = |r: Reply| {
        engine(Keyed::Llm, "k", &Mock::new(vec![r]))
            .translate("hi", "en", &HI)
            .unwrap_err()
    };
    assert!(matches!(
        e(err(401, "authentication_error", "invalid x-api-key")),
        Error::BadKey { .. }
    ));
    assert!(matches!(
        e(err(
            400,
            "invalid_request_error",
            "Your credit balance is too low to access the Anthropic API."
        )),
        Error::Quota { .. }
    ));
    assert!(matches!(
        e(err(402, "billing_error", "billing")),
        Error::Quota { .. }
    ));
    let missing = e(err(404, "not_found_error", "model: claude-nope"));
    assert!(
        matches!(&missing, Error::Refused { detail, .. } if detail.contains("claude-sonnet-5-5")),
        "{missing:?}"
    );
    assert!(matches!(
        e(busy(
            429,
            0,
            json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}})
        )),
        Error::Busy { .. }
    ));
    assert!(matches!(
        e(busy(
            529,
            0,
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})
        )),
        Error::Busy { .. }
    ));
    let refused = e(reply(200, json!({"stop_reason": "refusal", "content": []})));
    assert!(matches!(refused, Error::Refused { .. }), "{refused:?}");
    assert!(refused.to_string().contains("Claude"), "{refused}");
}

#[test]
fn claude_translates_mixed_and_romanized_lines_the_other_engines_leave() {
    // A Hindi song with a Hinglish line and an English one: Claude gets every line with words
    // (it reads Hindi in English letters); the translation services only the Devanagari ones.
    let song = ["तुम ही हो", "Tu hi meri jaan hai", "Baby I love you", "♪"];
    let dir = tempfile::tempdir().unwrap();
    let cache = SongCache::new(dir.path());
    let mock = Mock::new(vec![claude_says(&[
        "You alone",
        "You are my life",
        "Baby I love you",
    ])]);
    let t = translate_song(&*engine(Keyed::Llm, "k", &mock), &cache, "hi", "en", &song).unwrap();
    assert_eq!(t.engine, "llm");
    assert_eq!(t.version, "claude-sonnet-5-5");
    assert_eq!(
        t.lines,
        [
            Some("You alone".to_owned()),
            Some("You are my life".to_owned()),
            // Given back as it was: nothing to show under it.
            None,
            None
        ]
    );
    let mock = echo_deepl();
    let t = translate_song(
        &*engine(Keyed::DeepL, "k", &mock),
        &cache,
        "hi",
        "en",
        &song,
    )
    .unwrap();
    assert_eq!(t.engine, "deepl");
    assert_eq!(t.lines, [Some("T0".to_owned()), None, None, None]);
}

#[test]
fn every_engine_has_a_version_and_none_for_one_language() {
    let mock = echo_deepl();
    for which in Keyed::ALL {
        let e = engine(which, "k", &mock);
        assert_eq!(e.engine(), which.id());
        assert!(e.version("hi", "en").is_some());
        assert!(e.version("en-GB", "en").is_none());
    }
}
