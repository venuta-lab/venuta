//! The player, as the window sees it: commands in, a stream of status out.
//!
//! The engine (`player` crate) runs on its own thread. Status arrives about ten times a second
//! and goes straight to the window over a channel; the queue travels only when it changes.

use player::engine::{Cmd, Item, OutputChoice, Player, Signal, Status};
use player::queue::Repeat;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri::ipc::Channel;

/// A song in the queue, as the window sent it (library-relative path) plus where it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub path: String,
    pub dir: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artist: String,
    pub duration_ms: u64,
    pub format: String,
    pub bit_depth: u16,
    pub sample_rate: u32,
    pub lyrics: bool,
    /// The file on disk (the open folder joined with `path`).
    #[serde(skip)]
    pub file: PathBuf,
}

impl Item for QueueItem {
    fn path(&self) -> &Path {
        &self.file
    }
}

/// What the window receives (see `app/src/lib/player.ts`).
#[derive(Serialize)]
pub struct WindowStatus {
    queue: Option<Vec<QueueItem>>,
    index: i64,
    playing: bool,
    position_ms: u64,
    duration_ms: u64,
    shuffle: bool,
    repeat: Repeat,
    volume: f32,
    signal: Option<Signal>,
    from: String,
    error: String,
}

#[derive(Default)]
pub struct Playback {
    player: Mutex<Option<Player<QueueItem>>>,
    window: Arc<Mutex<Option<Channel<WindowStatus>>>>,
    /// Exclusive (bit-perfect) unless you choose shared; the device, or the default.
    output: Mutex<(Option<String>, bool)>,
}

impl Playback {
    fn with<R>(&self, f: impl FnOnce(&Player<QueueItem>) -> R) -> Result<R, String> {
        let mut slot = self.player.lock().map_err(|e| e.to_string())?;
        if slot.is_none() {
            let window = self.window.clone();
            let (id, exclusive) = self.output.lock().map_err(|e| e.to_string())?.clone();
            *slot = Some(Player::spawn(
                OutputChoice::Device { id, exclusive },
                move |s: Status<QueueItem>| {
                    if let Ok(w) = window.lock()
                        && let Some(ch) = w.as_ref()
                    {
                        let _ = ch.send(WindowStatus {
                            queue: s.queue,
                            index: s.index.map(|i| i as i64).unwrap_or(-1),
                            playing: s.playing,
                            position_ms: s.position_ms,
                            duration_ms: s.duration_ms,
                            shuffle: s.shuffle,
                            repeat: s.repeat,
                            volume: s.volume,
                            signal: s.signal,
                            from: s.from,
                            error: s.error,
                        });
                    }
                },
            ));
        }
        Ok(f(slot.as_ref().expect("just made")))
    }

    fn send(&self, cmd: Cmd<QueueItem>) -> Result<(), String> {
        self.with(|p| p.send(cmd))
    }
}

fn resolve(root: &str, items: Vec<QueueItem>) -> Vec<QueueItem> {
    let root = PathBuf::from(root);
    items
        .into_iter()
        .map(|mut it| {
            it.file = root.join(&it.path);
            it
        })
        .collect()
}

/// Start receiving the player's status (the window calls this once).
#[tauri::command]
pub fn player_subscribe(
    on_state: Channel<WindowStatus>,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    *pb.window.lock().map_err(|e| e.to_string())? = Some(on_state);
    // Send the current state at once (and the queue with it).
    pb.send(Cmd::Refresh)
}

#[tauri::command]
pub fn player_play(
    root: String,
    items: Vec<QueueItem>,
    index: usize,
    from: String,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    pb.send(Cmd::Set {
        items: resolve(&root, items),
        index,
        start_ms: 0,
        play: true,
        from,
    })
}

#[tauri::command]
pub fn player_toggle(pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Toggle)
}

#[tauri::command]
pub fn player_next(pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Next)
}

#[tauri::command]
pub fn player_previous(pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Previous)
}

#[tauri::command]
pub fn player_seek(ms: u64, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Seek(ms))
}

#[tauri::command]
pub fn player_volume(volume: f32, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Volume(volume))
}

#[tauri::command]
pub fn player_options(
    shuffle: bool,
    repeat: Repeat,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    pb.send(Cmd::Options { shuffle, repeat })
}

#[tauri::command]
pub fn player_enqueue(
    root: String,
    items: Vec<QueueItem>,
    next: bool,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    pb.send(Cmd::Insert {
        items: resolve(&root, items),
        next,
    })
}

#[tauri::command]
pub fn player_remove(index: usize, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Remove(index))
}

#[tauri::command]
pub fn player_move(from: usize, to: usize, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Move(from, to))
}

#[tauri::command]
pub fn player_jump(index: usize, pb: State<'_, Playback>) -> Result<(), String> {
    pb.send(Cmd::Jump(index))
}

#[cfg(windows)]
use player::wasapi_out::{OutputDevice, list_devices};

#[cfg(not(windows))]
#[derive(Serialize)]
pub struct OutputDevice {
    id: String,
    name: String,
    default: bool,
}

#[cfg(not(windows))]
fn list_devices() -> Result<Vec<OutputDevice>, String> {
    Ok(Vec::new())
}

#[derive(Serialize)]
pub struct OutputChoices {
    devices: Vec<OutputDevice>,
    /// The chosen device (`None`: follow the Windows default).
    device: Option<String>,
    exclusive: bool,
}

/// Sound outputs, and which one plays.
#[tauri::command]
pub async fn player_devices(pb: State<'_, Playback>) -> Result<OutputChoices, String> {
    let (device, exclusive) = pb.output.lock().map_err(|e| e.to_string())?.clone();
    let devices = tauri::async_runtime::spawn_blocking(list_devices)
        .await
        .map_err(|e| e.to_string())??;
    Ok(OutputChoices {
        devices,
        device,
        exclusive,
    })
}

/// Play through another device, or switch between exclusive (bit-perfect) and shared.
#[tauri::command]
pub fn player_output(
    device: Option<String>,
    exclusive: bool,
    pb: State<'_, Playback>,
) -> Result<(), String> {
    *pb.output.lock().map_err(|e| e.to_string())? = (device.clone(), exclusive);
    pb.send(Cmd::Output(OutputChoice::Device {
        id: device,
        exclusive,
    }))
}
