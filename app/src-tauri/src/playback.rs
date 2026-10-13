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

/// What the Windows media overlay (and media keys) show.
struct NowPlaying {
    title: String,
    artist: String,
    album: String,
    cover: Option<String>,
    duration_ms: u64,
    playing: bool,
    position_ms: u64,
}

#[derive(Default)]
pub struct Playback {
    player: Arc<Mutex<Option<Player<QueueItem>>>>,
    /// Updates for the media overlay thread, once it runs.
    media: Arc<Mutex<Option<std::sync::mpsc::Sender<NowPlaying>>>>,
    window: Arc<Mutex<Option<Channel<WindowStatus>>>>,
    /// Exclusive (bit-perfect) unless you choose shared; the device, or the default.
    output: Mutex<(Option<String>, bool)>,
}

impl Playback {
    fn with<R>(&self, f: impl FnOnce(&Player<QueueItem>) -> R) -> Result<R, String> {
        let mut slot = self.player.lock().map_err(|e| e.to_string())?;
        if slot.is_none() {
            let window = self.window.clone();
            let media = self.media.clone();
            let (id, exclusive) = self.output.lock().map_err(|e| e.to_string())?.clone();
            // The current song and play state, to tell the media overlay only when they change.
            let mut last: (Option<String>, bool, u64) = (None, false, 0);
            let mut queue: Vec<QueueItem> = Vec::new();
            *slot = Some(Player::spawn(
                OutputChoice::Device { id, exclusive },
                move |s: Status<QueueItem>| {
                    if let Some(q) = &s.queue {
                        queue = q.clone();
                    }
                    let item = s.index.and_then(|i| queue.get(i));
                    let key = item.map(|i| i.file.to_string_lossy().into_owned());
                    let tick = s.position_ms / 5000;
                    if (key.clone(), s.playing, tick) != last
                        && let Some(item) = item
                        && let Ok(m) = media.lock()
                        && let Some(tx) = m.as_ref()
                    {
                        let cover = item
                            .file
                            .parent()
                            .map(|d| d.join("cover.jpg"))
                            .filter(|c| c.is_file());
                        let _ = tx.send(NowPlaying {
                            title: item.title.clone(),
                            artist: item.artists.join(", "),
                            album: item.album.clone(),
                            cover: cover.map(|c| {
                                format!("file:///{}", c.to_string_lossy().replace('\\', "/"))
                            }),
                            duration_ms: s.duration_ms,
                            playing: s.playing,
                            position_ms: s.position_ms,
                        });
                    }
                    last = (key, s.playing, tick);
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

/// Media keys and the Windows media overlay: a thread of their own, for the window `hwnd`.
#[cfg(windows)]
pub fn start_media_controls(pb: &Playback, hwnd: isize) {
    use souvlaki::{
        MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition,
        PlatformConfig,
    };
    let (tx, rx) = std::sync::mpsc::channel::<NowPlaying>();
    if let Ok(mut m) = pb.media.lock() {
        *m = Some(tx);
    }
    let player = pb.player.clone();
    std::thread::Builder::new()
        .name("venuta-media-keys".into())
        .spawn(move || {
            let Ok(mut controls) = MediaControls::new(PlatformConfig {
                display_name: "Venuta",
                dbus_name: "venuta",
                hwnd: Some(hwnd as *mut std::ffi::c_void),
            }) else {
                return;
            };
            let send = move |cmd: Cmd<QueueItem>| {
                if let Ok(p) = player.lock()
                    && let Some(p) = p.as_ref()
                {
                    p.send(cmd);
                }
            };
            let _ = controls.attach(move |e| match e {
                MediaControlEvent::Play => send(Cmd::Play),
                MediaControlEvent::Pause => send(Cmd::Pause),
                MediaControlEvent::Toggle => send(Cmd::Toggle),
                MediaControlEvent::Next => send(Cmd::Next),
                MediaControlEvent::Previous => send(Cmd::Previous),
                MediaControlEvent::Stop => send(Cmd::Pause),
                MediaControlEvent::SetPosition(MediaPosition(d)) => {
                    send(Cmd::Seek(d.as_millis() as u64))
                }
                _ => {}
            });
            for now in rx {
                let _ = controls.set_metadata(MediaMetadata {
                    title: Some(&now.title),
                    artist: Some(&now.artist),
                    album: Some(&now.album),
                    cover_url: now.cover.as_deref(),
                    duration: Some(std::time::Duration::from_millis(now.duration_ms)),
                });
                let progress = Some(MediaPosition(std::time::Duration::from_millis(
                    now.position_ms,
                )));
                let _ = controls.set_playback(if now.playing {
                    MediaPlayback::Playing { progress }
                } else {
                    MediaPlayback::Paused { progress }
                });
            }
        })
        .ok();
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
