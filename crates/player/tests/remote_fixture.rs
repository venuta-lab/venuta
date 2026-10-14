//! Remote direct-HTTP audio through the engine using FakeMediaAccess only.

use media_access::{CancellationToken, FakeMediaAccess, MediaAccess};
use player::engine::{Cmd, Item, OutputChoice, Player, Status};
use player::output::Captured;
use player::queue::Repeat;
use player::remote::RemoteItem;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn play_remote(item: RemoteItem) -> (Captured, Status<RemoteItem>) {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, rx) = channel::<Status<RemoteItem>>();
    let player = Player::spawn(OutputChoice::Memory(captured.clone()), move |s| {
        let _ = tx.send(s);
    });
    player.send(Cmd::Set {
        items: vec![item],
        index: 0,
        start_ms: 0,
        play: true,
        from: "remote-fixture".into(),
    });

    let started = Instant::now();
    let mut was_playing = false;
    let mut saw_pause = false;
    let mut commands_sent = false;
    let last = loop {
        let s = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("status from player");
        assert!(s.error.is_empty(), "player error: {}", s.error);
        if s.playing && !commands_sent {
            // Exercise pause/resume/stop controls on remote audio.
            player.send(Cmd::Pause);
            player.send(Cmd::Play);
            player.send(Cmd::Options {
                shuffle: false,
                repeat: Repeat::Off,
            });
            commands_sent = true;
        }
        if was_playing && !s.playing {
            saw_pause = true;
        }
        was_playing |= s.playing;
        if was_playing && commands_sent {
            // Allow some audio to land, then stop.
            if started.elapsed() > Duration::from_millis(200) {
                player.send(Cmd::Pause);
            }
        }
        if saw_pause && commands_sent && started.elapsed() > Duration::from_millis(300) {
            break s;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "remote fixture playback timed out"
        );
    };

    drop(player);
    let captured = Arc::try_unwrap(captured)
        .ok()
        .unwrap()
        .into_inner()
        .unwrap();
    (captured, last)
}

#[test]
fn fake_remote_audio_plays_through_engine_controls() {
    let access = FakeMediaAccess::with_in_memory_fixture();
    let audio = access
        .resolve_audio("track-1", &CancellationToken::default())
        .expect("fixture resolve");
    assert!(
        format!("{audio:?}").contains("[redacted]"),
        "ephemeral audio debug must redact URLs"
    );

    let item = RemoteItem::new(audio, "Fixture Track", Some("Fixture Album".into()));
    let (captured, status) = play_remote(item);

    assert!(
        !captured.bytes.is_empty() || status.duration_ms > 0 || status.position_ms > 0,
        "expected remote fixture audio activity"
    );
    assert!(status.error.is_empty());
}

#[test]
fn remote_item_serialize_omits_url() {
    let access = FakeMediaAccess::with_in_memory_fixture();
    let audio = access
        .resolve_audio("track-2", &CancellationToken::default())
        .unwrap();
    let item = RemoteItem::new(audio, "Serialized", None);
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("track-2"));
    assert!(json.contains("Serialized"));
    assert!(!json.contains("http://"));
    assert!(!json.contains("fixture.wav"));
}

#[test]
fn deferred_remote_item_resolves_through_media_access_at_open() {
    // The item carries only a stable id + its MediaAccess handle; the URL is fetched at open.
    let access: Arc<dyn MediaAccess> = Arc::new(FakeMediaAccess::with_in_memory_fixture());
    let item = RemoteItem::deferred(
        access,
        "remote_catalog",
        "track-1",
        "Deferred Track",
        Some("Fixture Album".into()),
    );

    // Nothing resolved yet: the serialized snapshot is id + metadata only, never a URL.
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("track-1"));
    assert!(json.contains("Deferred Track"));
    assert!(
        !json.contains("http://"),
        "snapshot leaked a stream URL: {json}"
    );
    assert!(
        !json.contains("fixture.wav"),
        "snapshot leaked a stream URL: {json}"
    );

    // open() resolves through FakeMediaAccess and yields a playable remote source.
    item.open().expect("deferred item resolves and opens");

    // Playing it through the engine exercises the same resolve-then-open path.
    let (captured, status) = play_remote(item);
    assert!(
        !captured.bytes.is_empty() || status.duration_ms > 0 || status.position_ms > 0,
        "expected deferred remote fixture audio activity"
    );
    assert!(status.error.is_empty());
}
