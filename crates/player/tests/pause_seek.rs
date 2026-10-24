//! A pause holds the device's buffer, so play goes on from exactly where it stopped. A move while
//! paused must let that buffer go first: otherwise the old place is heard for a moment (a blip,
//! a crackle) before the new one.
//!
//! On engine 1 that is the device's flush (its memory device keeps whatever it was given, so the
//! flush is the evidence). On engine 2 a pause stops the output after its fade, and every start
//! is a new renderer on an output that never plays anything held from before (contract 5.2):
//! the evidence is the sound itself, every frame of it after the pause, placed by the renderer's
//! notes.

mod engines;

use player::engine::{Cmd, EngineKind, OutputChoice, Player, Status, Watch};
use player::output::Captured;
use player::voice_api::{FadeKind, RenderNote};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RATE: u32 = 48_000;
/// Each frame says where it is: frame k holds `k % PLACES + 100` (never silent). `PLACES` is no
/// divisor of any round time, so places seconds apart hold different values.
const PLACES: u64 = 29_989;

/// A mono 16-bit WAV of `secs` seconds that is never silent.
fn wav(path: &Path, secs: u32) {
    let frames = (RATE * secs) as usize;
    let data_len = frames * 2;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        v.extend_from_slice(&(((i as u64 % PLACES) + 100) as i16).to_le_bytes());
    }
    std::fs::write(path, v).unwrap();
}

fn song(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = player::test_support::temp_dir().join(format!(
        "venuta-pause-seek-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    wav(&f, 20);
    f
}

struct Paused {
    player: Player<PathBuf>,
    captured: Arc<Mutex<Captured>>,
    notes: Receiver<RenderNote>,
}

/// A player at a real device's pace, playing `path` for a moment, then paused.
fn playing_then_paused(engine: EngineKind, path: PathBuf) -> Paused {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, _rx) = channel::<Status<PathBuf>>();
    let (notes_tx, notes) = channel();
    let player = Player::spawn_watched(
        engine,
        OutputChoice::PacedMemory(captured.clone()),
        move |s| {
            let _ = tx.send(s);
        },
        Watch {
            notes: Some(notes_tx),
            ..Watch::default()
        },
    );
    player.send(Cmd::Set {
        items: vec![path],
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    std::thread::sleep(Duration::from_millis(600));
    player.send(Cmd::Pause);
    std::thread::sleep(Duration::from_millis(300));
    Paused {
        player,
        captured,
        notes,
    }
}

fn flushes(c: &Arc<Mutex<Captured>>) -> usize {
    c.lock().unwrap().flushes
}

/// Engine 2, once the player is gone: the pause faded the song out from frame `f` of what was
/// rendered; after it, nothing sounds until the one start that comes next, which rises (15 ms)
/// from the song's place `place(f)` and from then on plays every frame of the song, one after
/// the other, until the last fade (the player let go of): nothing from anywhere else.
fn after_the_pause_only(p: Paused, place: impl Fn(u64) -> u64) {
    let Paused {
        player,
        captured,
        notes,
    } = p;
    drop(player);
    let notes: Vec<RenderNote> = notes.try_iter().collect();
    let x: Vec<i32> = captured
        .lock()
        .unwrap()
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 16)
        .collect();
    let outs: Vec<(u64, u64)> = notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Faded {
                from,
                to,
                kind: FadeKind::Out,
                ..
            } => Some((from, to)),
            _ => None,
        })
        .collect();
    assert_eq!(
        outs.len(),
        2,
        "the pause's fade and the last one: {notes:?}"
    );
    let (paused_from, paused_to) = outs[0];
    let starts: Vec<(u64, u64, u32)> = notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Started {
                at, content, fade, ..
            } => Some((at, content, fade)),
            _ => None,
        })
        .collect();
    assert_eq!(
        starts.len(),
        2,
        "the first start and the one after the pause"
    );
    let (at, content, fade) = starts[1];
    assert!(at >= paused_to, "it starts again after the pause's fade");
    assert_eq!(
        content,
        place(paused_from),
        "from the place it was meant to"
    );
    assert_eq!(fade, 720, "rising over 15 ms");
    assert!(
        x[paused_to as usize..at as usize].iter().all(|&v| v == 0),
        "silence between the pause and the start"
    );
    let value = |k: u64| (k % PLACES + 100) as i32;
    let last = outs[1].0;
    let from = at + u64::from(fade);
    assert!(last > from + 4_800, "and it played a while");
    for f in from..last {
        assert_eq!(
            x[f as usize],
            value(content + (f - at)),
            "frame {f}: the song's own, in order from its place"
        );
    }
}

on_both_engines! {
    fn a_move_while_paused_lets_the_old_sound_go_before_playing_on(engine) {
        let p = playing_then_paused(engine, song("moved"));
        let before = flushes(&p.captured);
        p.player.send(Cmd::Seek(10_000));
        std::thread::sleep(Duration::from_millis(200));
        p.player.send(Cmd::Play);
        std::thread::sleep(Duration::from_millis(400));
        match engine {
            EngineKind::One => assert!(
                flushes(&p.captured) > before,
                "the device still held the sound from before the move when play went on"
            ),
            // From 10 s exactly, and nothing of the old place.
            EngineKind::Two => after_the_pause_only(p, |_| 10 * u64::from(RATE)),
        }
    }

    fn a_plain_pause_and_play_goes_on_from_what_the_device_holds(engine) {
        let p = playing_then_paused(engine, song("plain"));
        let before = flushes(&p.captured);
        p.player.send(Cmd::Play);
        std::thread::sleep(Duration::from_millis(400));
        match engine {
            EngineKind::One => assert_eq!(
                flushes(&p.captured),
                before,
                "nothing moved, so nothing held is let go"
            ),
            // It goes on from where its fade-out began (the song started at frame 0, so that is
            // the frame the fade began on).
            EngineKind::Two => after_the_pause_only(p, |faded_from| faded_from),
        }
    }
}
