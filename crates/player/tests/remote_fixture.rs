//! Remote direct-HTTP audio through the engine using FakeMediaAccess only, on both engines.

mod engines;

use media_access::{CancellationToken, FakeMediaAccess, MediaAccess};
use player::engine::{Cmd, EngineKind, Item, OutputChoice, Player, Status};
use player::output::Captured;
use player::queue::Repeat;
use player::remote::RemoteItem;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn play_remote(engine: EngineKind, item: RemoteItem) -> (Captured, Status<RemoteItem>) {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, rx) = channel::<Status<RemoteItem>>();
    let player = Player::spawn_with(engine, OutputChoice::Memory(captured.clone()), move |s| {
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
    let mut last: Option<Status<RemoteItem>> = None;
    let last = loop {
        // A player that has nothing more to say (the short fixture played to its end and
        // every command was answered) is done; one that never answers is not.
        let s = match rx.recv_timeout(Duration::from_secs(if saw_pause { 2 } else { 10 })) {
            Ok(s) => s,
            Err(_) if saw_pause && commands_sent => break last.expect("a status"),
            Err(_) => panic!("status from player"),
        };
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
        last = Some(s);
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

on_both_engines! {
fn fake_remote_audio_plays_through_engine_controls(engine) {
    let access = FakeMediaAccess::with_in_memory_fixture();
    let audio = access
        .resolve_audio("track-1", &CancellationToken::default())
        .expect("fixture resolve");
    assert!(
        format!("{audio:?}").contains("[redacted]"),
        "ephemeral audio debug must redact URLs"
    );

    let item = RemoteItem::new(audio, "Fixture Track", Some("Fixture Album".into()));
    let (captured, status) = play_remote(engine, item);

    assert!(
        !captured.bytes.is_empty() || status.duration_ms > 0 || status.position_ms > 0,
        "expected remote fixture audio activity"
    );
    assert!(status.error.is_empty());
}
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

on_both_engines! {
fn deferred_remote_item_resolves_through_media_access_at_open(engine) {
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
    let (captured, status) = play_remote(engine, item);
    assert!(
        !captured.bytes.is_empty() || status.duration_ms > 0 || status.position_ms > 0,
        "expected deferred remote fixture audio activity"
    );
    assert!(status.error.is_empty());
}
}

/// A 16-bit stereo 44.1 kHz WAV of `secs` seconds (a quiet ramp, never silent).
fn wav_bytes(secs: usize) -> Vec<u8> {
    let frames = secs * 44_100;
    let data_len = frames * 4;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&44_100u32.to_le_bytes());
    v.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        let s = ((i % 400) as i16 - 200) * 8;
        v.extend_from_slice(&s.to_le_bytes());
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// A media host that sends `body` slowly, in small pieces with pauses, like a slow network.
fn slow_host(body: Vec<u8>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = Arc::new(body);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let body = body.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 && line.trim() != "" {
                    line.clear();
                }
                let mut s = stream;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                if s.write_all(head.as_bytes()).is_err() {
                    return;
                }
                for piece in body.chunks(32 * 1024) {
                    if s.write_all(piece).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
            });
        }
    });
    format!("http://{addr}/song.wav")
}

on_both_engines! {
fn seeking_past_what_has_arrived_waits_for_it_and_lands_there(engine) {
    let wav = wav_bytes(6);
    let audio = media_access::EphemeralAudio {
        track_id: "slow".into(),
        provider: "test".into(),
        url: slow_host(wav.clone()),
        mime_type: Some("audio/wav".into()),
        content_length: Some(wav.len() as u64),
        duration_ms: Some(6_000),
        headers: Vec::new(),
        via: "test",
        loudness_db: None,
    };
    let item = RemoteItem::new(audio, "Slow song", None);
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, rx) = channel::<Status<RemoteItem>>();
    let player = Player::spawn_with(engine, OutputChoice::Memory(captured), move |s| {
        let _ = tx.send(s);
    });
    player.send(Cmd::Set {
        items: vec![item],
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    let started = Instant::now();
    let mut sought = false;
    let mut landed = false;
    while started.elapsed() < Duration::from_secs(15) {
        let Ok(s) = rx.recv_timeout(Duration::from_secs(5)) else {
            break;
        };
        assert!(
            s.error.is_empty(),
            "a seek past the download is not an error: {}",
            s.error
        );
        if !sought && s.playing && s.position_ms > 100 {
            player.send(Cmd::Seek(5_000));
            sought = true;
        } else if sought && s.position_ms >= 5_000 {
            landed = true;
            break;
        }
    }
    assert!(sought, "it started playing");
    assert!(landed, "and it reached 5 s once the bytes arrived");
}
}
