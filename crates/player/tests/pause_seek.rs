//! A pause holds the device's buffer, so play goes on from exactly where it stopped. A move while
//! paused must let that buffer go first: otherwise the old place is heard for a moment (a blip,
//! a crackle) before the new one.

use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RATE: u32 = 48_000;

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
        v.extend_from_slice(&(((i % 2000) as i16) + 100).to_le_bytes());
    }
    std::fs::write(path, v).unwrap();
}

fn song(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("venuta-pause-seek-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    wav(&f, 20);
    f
}

/// A player at a real device's pace, playing `path` for a moment, then paused.
fn playing_then_paused(path: PathBuf) -> (Player<PathBuf>, Arc<Mutex<Captured>>) {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (tx, _rx) = channel::<Status<PathBuf>>();
    let player = Player::spawn(OutputChoice::PacedMemory(captured.clone()), move |s| {
        let _ = tx.send(s);
    });
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
    (player, captured)
}

fn flushes(c: &Arc<Mutex<Captured>>) -> usize {
    c.lock().unwrap().flushes
}

#[test]
fn a_move_while_paused_lets_the_old_sound_go_before_playing_on() {
    let (player, captured) = playing_then_paused(song("moved"));
    let before = flushes(&captured);
    player.send(Cmd::Seek(10_000));
    std::thread::sleep(Duration::from_millis(200));
    player.send(Cmd::Play);
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        flushes(&captured) > before,
        "the device still held the sound from before the move when play went on"
    );
}

#[test]
fn a_plain_pause_and_play_goes_on_from_what_the_device_holds() {
    let (player, captured) = playing_then_paused(song("plain"));
    let before = flushes(&captured);
    player.send(Cmd::Play);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        flushes(&captured),
        before,
        "nothing moved, so nothing held is let go"
    );
}
