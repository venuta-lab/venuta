use super::*;
use crate::test_server::MockServer;
use std::sync::Mutex;

/// A home page shaped like the real one: the config first, decoy `ytAtN` mentions before the
/// call, and the attestation answer as a single-quoted literal of `\x` escapes.
fn home_page(with_challenge: bool) -> String {
    let answer = serde_json::json!({
        "responseContext": {},
        "bgChallenge": {
            "interpreterUrl": {
                "privateDoNotAccessOrElseTrustedResourceUrlWrappedValue":
                    "//www.google.com/js/th/Abc_12-x.js"
            },
            "interpreterHash": "Abc_12-x",
            "program": "pr/og+ram==",
            "globalName": "trayride",
            "clientExperimentsStateBlob": "[null,[],[]]"
        }
    })
    .to_string();
    // As the page writes it: braces and quotes as `\x..`, slashes as `\/`.
    let escaped = answer
        .replace('{', "\\x7b")
        .replace('}', "\\x7d")
        .replace('"', "\\x22")
        .replace('[', "\\x5b")
        .replace(']', "\\x5d")
        .replace('/', "\\/");
    let call = if with_challenge {
        format!(
            "<script>(function() {{if (window.ytAtN) {{window.ytAtN({{'R': '{escaped}','T': 'AQ\\x3d'}});}}}})();</script>"
        )
    } else {
        String::new()
    };
    format!(
        r#"<html><script nonce="n">ytcfg.set({{"EVENT_ID":"evt-1","STS":20740,"INNERTUBE_CLIENT_VERSION":"2.20261019.01.00","TRICKY":"a}});b"}});</script>
<script nonce="n">ytcfg.set({{"CSI_SERVICE_NAME": 'youtube'}})</script>
<script nonce="n">window.ytAtP = new Promise(res => window.ytAtN = res); window.addEventListener('DOMContentLoaded', () => {{window.ytAtN(); delete window.ytAtN;}});</script>
{call}</html>"#
    )
}

#[test]
fn js_escapes_are_undone() {
    // `\u` escapes built up, so nothing on the way can mistake them for characters.
    let u = |hex: &str| ["\\", "u", hex].concat();
    let input = format!(r"\x7b\x22a\x22:\/\/x\\y\'z{}\n", u("00e9"));
    assert_eq!(
        unescape_js(&input).as_deref(),
        Some("{\"a\"://x\\y'z\u{e9}\n")
    );
    let pair = format!("{}{}", u("d83c"), u("dfb5"));
    assert_eq!(unescape_js(&pair).as_deref(), Some("\u{1f3b5}"));
    assert_eq!(unescape_js(&u("d83c")), None, "half a pair");
    assert_eq!(unescape_js(r"\x7"), None, "a cut-off escape");
    assert_eq!(unescape_js("tail\\"), None);
}

// The home page's own challenge, with the config it was made with.
#[test]
fn the_home_page_challenge_is_read_with_its_config() {
    let c = challenge_from_home(&home_page(true)).unwrap();
    assert_eq!(c.interpreter_url, "//www.google.com/js/th/Abc_12-x.js");
    assert_eq!(c.interpreter_hash, "Abc_12-x");
    assert_eq!(c.program, "pr/og+ram==");
    assert_eq!(c.global_name, "trayride");
    let config = c.page_config.expect("the page config");
    assert_eq!(config["EVENT_ID"], "evt-1");
    assert_eq!(
        config["TRICKY"], "a});b",
        "a closing brace inside a string ends nothing"
    );
}

#[test]
fn a_page_without_a_challenge_says_so() {
    assert_eq!(
        challenge_from_home(&home_page(false)).unwrap_err(),
        PotError::new("the YouTube page carried no security check")
    );
    assert!(page_config(&home_page(false)).is_some());
    assert!(page_config("<html></html>").is_none());
}

// Only Google's own interpreter scripts are ever run.
#[test]
fn only_googles_interpreter_scripts_are_taken() {
    assert_eq!(
        interpreter_path("//www.google.com/js/th/UuzLx1YTO8P6u56my.js"),
        Some("/js/th/UuzLx1YTO8P6u56my.js")
    );
    assert_eq!(
        interpreter_path("https://www.google.com/js/th/a-b_c.js"),
        Some("/js/th/a-b_c.js")
    );
    for bad in [
        "//evil.example/js/th/a.js",
        "//www.google.com.evil.example/js/th/a.js",
        "//www.google.com/js/th/../x.js",
        "//www.google.com/js/th/a.js?x=1",
        "//www.google.com/js/other/a.js",
        "//www.google.com/js/th/.js",
    ] {
        assert_eq!(interpreter_path(bad), None, "{bad}");
    }
    let foreign = serde_json::json!({ "bgChallenge": {
        "interpreterUrl": { "privateDoNotAccessOrElseTrustedResourceUrlWrappedValue": "//evil.example/js/th/a.js" },
        "program": "p", "globalName": "g"
    }});
    assert!(challenge_from_attestation(&foreign, None).is_err());
    let odd_global = serde_json::json!({ "bgChallenge": {
        "interpreterUrl": { "privateDoNotAccessOrElseTrustedResourceUrlWrappedValue": "//www.google.com/js/th/a.js" },
        "program": "p", "globalName": "x;alert(1)"
    }});
    assert!(challenge_from_attestation(&odd_global, None).is_err());
}

#[test]
fn the_integrity_token_answer_is_read() {
    let ok = serde_json::json!(["dG9rZW4=", 43200, 300, "fallback"]);
    let it = IntegrityToken::from_answer(&ok).unwrap();
    assert_eq!(it.token, "dG9rZW4=");
    assert_eq!(it.ttl, Some(Duration::from_secs(43200)));
    assert_eq!(it.refresh, Some(Duration::from_secs(300)));
    assert_eq!(it.keep_for(), Duration::from_secs(43200 - 300));
    // An error answer has a code where the token goes.
    assert!(IntegrityToken::from_answer(&serde_json::json!([3, "no"])).is_err());
    assert!(IntegrityToken::from_answer(&serde_json::json!([""])).is_err());
}

#[test]
fn the_integrity_token_is_let_go_before_it_runs_out() {
    let it = |ttl: Option<u64>, refresh: Option<u64>| IntegrityToken {
        token: "t".into(),
        ttl: ttl.map(Duration::from_secs),
        refresh: refresh.map(Duration::from_secs),
    };
    // A long threshold wins over the margin.
    assert_eq!(
        it(Some(43200), Some(3600)).keep_for(),
        Duration::from_secs(39600)
    );
    // No threshold: the margin.
    assert_eq!(
        it(Some(43200), None).keep_for(),
        Duration::from_secs(43200 - 300)
    );
    // Nothing said: the default lifetime.
    assert_eq!(it(None, None).keep_for(), DEFAULT_LIFETIME - REFRESH_MARGIN);
    // Very short: never less than the floor.
    assert_eq!(it(Some(60), Some(30)).keep_for(), MIN_LIFETIME);
}

#[test]
fn the_minter_is_kept_until_it_runs_out_and_a_failure_waits() {
    let now = Instant::now();
    let later = now + Duration::from_secs(60);
    assert_eq!(Minter::None.next_step(now), Step::Make);
    assert_eq!(Minter::Ready(later).next_step(now), Step::Use);
    assert_eq!(Minter::Ready(later).next_step(later), Step::Make);
    let why = PotError::new("nope");
    assert_eq!(
        Minter::Failed(later, why.clone()).next_step(now),
        Step::Refuse(why.clone())
    );
    assert_eq!(Minter::Failed(later, why).next_step(later), Step::Make);
}

#[test]
fn request_bodies_are_the_pages_own() {
    assert_eq!(
        generate_it_body("snap\"shot"),
        r#"["O43z0dpjhgX20SCx4KAo","snap\"shot"]"#
    );
    let att: Value = serde_json::from_str(&att_get_body("2.1")).unwrap();
    assert_eq!(att["context"]["client"]["clientName"], "WEB");
    assert_eq!(att["context"]["client"]["clientVersion"], "2.1");
    assert_eq!(att["engagementType"], "ENGAGEMENT_TYPE_UNBOUND");
}

#[test]
fn a_token_looks_like_one() {
    assert!(looks_like_token(&"Mn-_Az09".repeat(20)));
    assert!(looks_like_token(&format!("{}==", "a".repeat(40))));
    assert!(!looks_like_token("short"));
    assert!(!looks_like_token(&"a+b/".repeat(20)), "not web-safe");
    assert!(!looks_like_token(&format!("{} ", "a".repeat(40))));
}

#[test]
fn engine_messages_are_tidied_for_the_log() {
    let e = PotError::with_detail(
        "BotGuard didn't run",
        "TypeError: x is <b>not</b> a function\n\u{1}",
    );
    assert_eq!(
        e.0,
        "BotGuard didn't run (TypeError: x is bnotb a function)"
    );
    assert_eq!(PotError::with_detail("w", "<>").0, "w");
    assert!(PotError::with_detail("w", &"y".repeat(500)).0.len() < 100);
}

/// A VM that answers as BotGuard would, keeping what it was given.
#[derive(Default)]
struct FakeVm {
    calls: Mutex<Vec<String>>,
    fail_start: bool,
    fail_mint: Mutex<bool>,
    bad_token: bool,
}

impl FakeVm {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl BotGuardVm for FakeVm {
    fn start<'a>(&'a self, script: &'a str, c: &'a Challenge) -> VmFuture<'a, String> {
        Box::pin(async move {
            let event = c
                .page_config
                .as_ref()
                .and_then(|p| p.get("EVENT_ID"))
                .and_then(Value::as_str)
                .unwrap_or("-")
                .to_owned();
            self.calls.lock().unwrap().push(format!(
                "start {script} {} {} {event}",
                c.global_name, c.program
            ));
            if self.fail_start {
                Err("ReferenceError: trayride is not defined".into())
            } else {
                Ok("snapshot-1".into())
            }
        })
    }
    fn ready_minter<'a>(&'a self, it: &'a str) -> VmFuture<'a, ()> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("minter {it}"));
            Ok(())
        })
    }
    fn mint<'a>(&'a self, binding: &'a str) -> VmFuture<'a, String> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("mint {binding}"));
            if std::mem::take(&mut *self.fail_mint.lock().unwrap()) {
                return Err("mint timed out".into());
            }
            Ok(if self.bad_token {
                "x".into()
            } else {
                format!("{binding}-{}", "T".repeat(40))
            })
        })
    }
}

/// A local stand-in for YouTube and Google: the home page (with or without its challenge),
/// `/att/get`, the interpreter and `GenerateIT`.
fn attestation_server(home_has_challenge: bool) -> (MockServer, Urls) {
    let server = MockServer::start(move |req| {
        let p = req.path.as_str();
        if p.starts_with("/home") {
            (200, home_page(home_has_challenge))
        } else if p.starts_with("/att") {
            let att = serde_json::json!({ "bgChallenge": {
                "interpreterUrl": { "privateDoNotAccessOrElseTrustedResourceUrlWrappedValue": "//www.google.com/js/th/Att.js" },
                "interpreterHash": "Att", "program": "att-program", "globalName": "attvm"
            }});
            (200, att.to_string())
        } else if p.starts_with("/js/th/") {
            (
                200,
                format!("script-of-{}", p.trim_start_matches("/js/th/")),
            )
        } else if p.starts_with("/it") {
            (200, r#"["aW50ZWdyaXR5",43200,300,"fb"]"#.into())
        } else {
            (404, "{}".into())
        }
    });
    let origin = server.base_url.trim_end_matches("/youtubei/v1/").to_owned();
    let urls = Urls {
        home: format!("{origin}/home"),
        att_get: format!("{origin}/att"),
        generate_it: format!("{origin}/it"),
        interpreter_origin: origin,
    };
    (server, urls)
}

// The whole of it: the page's challenge and config, Google's script, the snapshot sent for an
// integrity token, the minter made from it, then a token per binding with nothing asked again.
#[tokio::test]
async fn a_minter_is_made_once_and_mints_each_binding() {
    let (server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    let vm = Arc::new(FakeVm::default());
    tokens.install(vm.clone());
    assert!(tokens.can_mint());

    let first = tokens.mint("video-1").await.unwrap();
    let second = tokens.mint("video-2").await.unwrap();
    assert!(first.starts_with("video-1-") && second.starts_with("video-2-"));
    assert_eq!(
        vm.calls(),
        [
            "start script-of-Abc_12-x.js trayride pr/og+ram== evt-1",
            "minter aW50ZWdyaXR5",
            "mint video-1",
            "mint video-2",
        ]
    );

    let seen = server.requests();
    let paths: Vec<_> = seen.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["/home", "/js/th/Abc_12-x.js", "/it"], "asked once");
    let it = &seen[2];
    assert_eq!(it.body, r#"["O43z0dpjhgX20SCx4KAo","snapshot-1"]"#);
    assert_eq!(it.header("content-type"), Some("application/json+protobuf"));
    assert_eq!(it.header("x-user-agent"), Some("grpc-web-javascript/0.1"));
    assert!(it.header("x-goog-api-key").is_some());
    for r in &seen {
        assert!(r.header("cookie").is_none(), "signed out: {}", r.path);
        assert!(r.header("authorization").is_none());
    }
}

// A page without its own challenge: `/att/get`, with the page's config still set.
#[tokio::test]
async fn without_a_page_challenge_att_get_is_asked() {
    let (server, urls) = attestation_server(false);
    let tokens = PoTokens::with_urls(urls);
    let vm = Arc::new(FakeVm::default());
    tokens.install(vm.clone());
    tokens.mint("video-1").await.unwrap();
    assert_eq!(
        vm.calls()[0],
        "start script-of-Att.js attvm att-program evt-1"
    );
    let att = server
        .requests()
        .into_iter()
        .find(|r| r.path.starts_with("/att"))
        .expect("att/get asked");
    assert!(att.body.contains("ENGAGEMENT_TYPE_UNBOUND"));
    assert!(att.body.contains("2.20261019.01.00"), "the page's version");
}

// A failure is told at once, and the next song doesn't try again until the wait is over.
#[tokio::test]
async fn a_failed_check_is_not_retried_for_every_song() {
    let (server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    let vm = Arc::new(FakeVm {
        fail_start: true,
        ..Default::default()
    });
    tokens.install(vm.clone());
    let e = tokens.mint("video-1").await.unwrap_err();
    assert_eq!(
        e.0,
        "BotGuard didn't run (ReferenceError: trayride is not defined)"
    );
    let asked = server.requests().len();
    assert_eq!(tokens.mint("video-2").await.unwrap_err(), e);
    assert_eq!(server.requests().len(), asked, "nothing asked again");
    assert_eq!(vm.calls().len(), 1);
}

// A mint that fails lets the minter go: the next one starts afresh.
#[tokio::test]
async fn a_failed_mint_starts_afresh_next_time() {
    let (server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    let vm = Arc::new(FakeVm::default());
    *vm.fail_mint.lock().unwrap() = true;
    tokens.install(vm.clone());
    let e = tokens.mint("video-1").await.unwrap_err();
    assert_eq!(e.0, "couldn't make a token (mint timed out)");
    tokens.mint("video-1").await.unwrap();
    let starts = vm.calls().iter().filter(|c| c.starts_with("start")).count();
    assert_eq!(starts, 2);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path.starts_with("/it"))
            .count(),
        2
    );
}

#[tokio::test]
async fn an_unusable_token_is_refused() {
    let (_server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    tokens.install(Arc::new(FakeVm {
        bad_token: true,
        ..Default::default()
    }));
    assert_eq!(
        tokens.mint("video-1").await.unwrap_err().0,
        "the security check made an unusable token"
    );
}

#[tokio::test]
async fn nothing_to_run_it_in_is_said() {
    let tokens = PoTokens::with_urls(Urls::default());
    assert!(!tokens.can_mint());
    assert_eq!(
        tokens.mint("video-1").await.unwrap_err().0,
        "the security check can't run here"
    );
}

// `forget` (a token was refused) makes the next one start afresh.
#[tokio::test]
async fn a_forgotten_minter_is_made_again() {
    let (_server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    let vm = Arc::new(FakeVm::default());
    tokens.install(vm.clone());
    tokens.mint("a").await.unwrap();
    tokens.forget().await;
    tokens.mint("b").await.unwrap();
    let starts = vm.calls().iter().filter(|c| c.starts_with("start")).count();
    assert_eq!(starts, 2);
}

// The current player's signature timestamp, off the page's config.
#[test]
fn the_signature_timestamp_is_read_off_the_page() {
    assert_eq!(page_sts(&home_page(true)), Some(20740));
    assert_eq!(page_sts(r#"ytcfg.set({"EVENT_ID":"e"});"#), None);
    assert_eq!(page_sts(r#"ytcfg.set({"STS":"x"});"#), None);
    assert_eq!(page_sts("<html></html>"), None);
}

// The page the challenge came from says the signature timestamp too: nothing more is asked.
#[tokio::test]
async fn the_signature_timestamp_comes_with_the_challenge() {
    let (server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    tokens.install(Arc::new(FakeVm::default()));
    tokens.mint("video-1").await.unwrap();
    let asked = server.requests().len();
    assert_eq!(tokens.signature_timestamp().await, Some(20740));
    assert_eq!(server.requests().len(), asked);
}

// With no token made (nothing to run BotGuard in), the page is asked for it alone, once.
#[tokio::test]
async fn the_signature_timestamp_is_asked_for_alone_and_kept() {
    let (server, urls) = attestation_server(true);
    let tokens = PoTokens::with_urls(urls);
    assert_eq!(tokens.signature_timestamp().await, Some(20740));
    assert_eq!(tokens.signature_timestamp().await, Some(20740));
    let paths: Vec<_> = server.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/home"]);
}

// A page that can't be had isn't asked for again for every song.
#[tokio::test]
async fn a_missing_signature_timestamp_is_not_asked_for_every_song() {
    let server = MockServer::start(|_| (500, "{}".into()));
    let origin = server.base_url.trim_end_matches("/youtubei/v1/").to_owned();
    let tokens = PoTokens::with_urls(Urls {
        home: format!("{origin}/home"),
        att_get: format!("{origin}/att"),
        generate_it: format!("{origin}/it"),
        interpreter_origin: origin,
    });
    assert_eq!(tokens.signature_timestamp().await, None);
    assert_eq!(tokens.signature_timestamp().await, None);
    assert_eq!(server.requests().len(), 1);
}
