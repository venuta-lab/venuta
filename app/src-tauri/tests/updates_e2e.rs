//! The updater end to end: the real updater plugin, on Tauri's mock runtime, against a local
//! server, with a throwaway key: check, download and signature, as an installed copy does them.
//! Nothing is installed. (An integration test, so its binary gets the Common Controls manifest a
//! Tauri app needs on Windows: see build.rs.)

#[path = "../src/updates/mock.rs"]
mod mock;

use app_lib::updates::feed::{Channel, Source};
use app_lib::updates::{Kind, Pending, Plan, check_with, download_with};
use mock::{Reply, TestKey};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

const SETUP: &str = "Venuta_0.4.0_x64-setup.exe";
const AT: &str = "/v0.4.0/Venuta_0.4.0_x64-setup.exe";

fn app(pubkey: &str, version: &str) -> tauri::App<MockRuntime> {
    let mut ctx = mock_context(noop_assets());
    ctx.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({ "pubkey": pubkey, "windows": { "installMode": "passive" } }),
    );
    ctx.package_info_mut().version = version.parse().unwrap();
    mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Pending::default())
        .build(ctx)
        .unwrap()
}

fn plan(sources: Vec<Source>) -> Plan {
    Plan {
        sources,
        channel: Channel::Stable,
        waits: vec![Duration::ZERO; 2],
        timeout: Duration::from_secs(2),
        proxy: None,
    }
}

fn installer() -> Vec<u8> {
    b"MZ"
        .iter()
        .copied()
        .chain((0..200_000u32).map(|i| (i * 7) as u8))
        .collect()
}

/// Release 0.4.0 on the server: the installer as `served`, signed as `signed`, its manifest and
/// a releases list naming it.
fn publish(s: &mock::Server, key: &TestKey, served: Reply, signed: &[u8]) {
    let manifest = mock::manifest("0.4.0", &s.url(AT), &key.sign(signed, SETUP, "0.4.0"));
    let list = format!(
        r#"[{{"tag_name":"v0.4.0","draft":false,"prerelease":false,"assets":[{{"name":"latest.json","browser_download_url":"{}"}}]}}]"#,
        s.url("/v0.4.0/latest.json")
    );
    s.set(AT, vec![served]);
    s.set("/v0.4.0/latest.json", vec![mock::json(&manifest)]);
    s.set("/releases", vec![mock::json(&list)]);
}

#[test]
fn check_download_and_signature_end_to_end_with_the_primary_down() {
    let key = TestKey::new();
    let bytes = installer();
    let s = mock::serve(vec![("/latest.json", vec![Reply::Body(503, vec![])])]);
    publish(&s, &key, Reply::Body(200, bytes.clone()), &bytes);
    let app = app(&key.public(), "0.3.0");
    tauri::async_runtime::block_on(async {
        let p = plan(vec![
            Source::Manifest(s.url("/latest.json")),
            Source::Releases(s.url("/releases")),
        ]);
        let update = check_with(app.handle(), &p)
            .await
            .unwrap()
            .expect("an update");
        assert_eq!(update.version, "0.4.0");
        assert_eq!(update.current_version, "0.3.0");
        assert_eq!(
            s.hits("/latest.json"),
            3,
            "the primary was tried three times"
        );
        let seen = AtomicU64::new(0);
        let got = download_with(&update, Duration::from_secs(5), &p.waits, &|done, total| {
            seen.store(done, Ordering::Relaxed);
            assert_eq!(total, Some(bytes.len() as u64));
        })
        .await
        .unwrap();
        assert_eq!(got, bytes, "downloaded, and its signature accepted");
        assert_eq!(seen.load(Ordering::Relaxed), bytes.len() as u64);
    });
}

#[test]
fn a_changed_installer_is_refused_and_not_downloaded_again() {
    let key = TestKey::new();
    let bytes = installer();
    let mut changed = bytes.clone();
    changed[1000] ^= 1;
    let s = mock::serve(vec![]);
    publish(&s, &key, Reply::Body(200, changed), &bytes);
    let app = app(&key.public(), "0.3.0");
    tauri::async_runtime::block_on(async {
        let p = plan(vec![Source::Manifest(s.url("/v0.4.0/latest.json"))]);
        let update = check_with(app.handle(), &p).await.unwrap().unwrap();
        let e = download_with(&update, Duration::from_secs(5), &p.waits, &|_, _| {})
            .await
            .unwrap_err();
        assert_eq!(e.kind, Kind::Signature);
        assert_eq!(s.hits(AT), 1);
    });
}

#[test]
fn a_stalled_download_ends_with_a_clear_problem() {
    let key = TestKey::new();
    let s = mock::serve(vec![]);
    let stall = Reply::Stall {
        total: 1_000_000,
        send: vec![0; 1000],
    };
    publish(&s, &key, stall, b"the whole installer");
    let app = app(&key.public(), "0.3.0");
    tauri::async_runtime::block_on(async {
        let p = plan(vec![Source::Manifest(s.url("/v0.4.0/latest.json"))]);
        let update = check_with(app.handle(), &p).await.unwrap().unwrap();
        let t = Instant::now();
        let e = download_with(&update, Duration::from_secs(1), &p.waits, &|_, _| {})
            .await
            .unwrap_err();
        assert_eq!(e.kind, Kind::Stalled);
        assert_eq!(s.hits(AT), 3, "tried three times");
        assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    });
}

#[test]
fn old_versions_find_and_verify_the_new_manifest_as_they_are_configured() {
    // 0.1.0 to 0.3.0 call the plugin with the endpoint from tauri.conf.json and nothing else.
    let key = TestKey::new();
    let bytes = installer();
    let s = mock::serve(vec![]);
    publish(&s, &key, Reply::Body(200, bytes.clone()), &bytes);
    for old in ["0.1.0", "0.2.3", "0.3.0"] {
        let app = app(&key.public(), old);
        tauri::async_runtime::block_on(async {
            let updater = app
                .handle()
                .updater_builder()
                .endpoints(vec![s.url("/v0.4.0/latest.json").parse().unwrap()])
                .unwrap()
                .build()
                .unwrap();
            let update = updater.check().await.unwrap().expect(old);
            assert_eq!(update.version, "0.4.0");
            assert_eq!(update.download(|_, _| {}, || {}).await.unwrap(), bytes);
        });
    }
}

#[test]
fn a_server_that_never_answers_is_a_timeout_in_plain_words() {
    let key = TestKey::new();
    let s = mock::serve(vec![("/latest.json", vec![Reply::Hang])]);
    let app = app(&key.public(), "0.3.0");
    tauri::async_runtime::block_on(async {
        let mut p = plan(vec![Source::Manifest(s.url("/latest.json"))]);
        p.timeout = Duration::from_millis(500);
        let e = check_with(app.handle(), &p).await.err().expect("a problem");
        assert_eq!(e.kind, Kind::Timeout);
        assert!(e.message.contains("didn't answer"), "{}", e.message);
    });
}
