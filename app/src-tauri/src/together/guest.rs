//! Joining a room: the guest's session is `together`'s (`GuestSession`: the knock, the clock on
//! datagram pings, the conductor taking this player, reconnects and the 2-minute rule); the
//! connection is `together_net`'s `IrohTransport`, and the host's files play through
//! `together_net::proxy`. This is the app around them: this person's copies, the host's covers
//! kept on this computer, the session's thread, and the window told of every change.

use super::AppPlayer;
use super::sources::{AppCopies, HostStream};
use crate::playback::{Playback, QueueItem, RoomNow};
use player::measure::{Analysis, EnvelopeMaker, Making};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use together::clock_map;
use together::conductor::Conductor;
use together::invite::Invite;
use together::measure::{Measured, References, Spawn};
use together::proto::Song;
use together::proto::v3::Action;
use together::room_player::ListenMode;
use together::session::{GuestConfig, GuestSession, Stage, WindowExtras};
use together::time::RealClock;
use together::transport::{Get, Transport};
use together::view::View;
use together_net::proxy::Proxy;
use together_net::transport::{HOST, IrohTransport, Timing};

/// A guest's side, on its own thread.
pub struct Live {
    pub s: GuestSession<QueueItem>,
    t: IrohTransport,
    app: AppHandle,
    invite: Invite,
    /// The host's files, played through this computer's server (stops with the room).
    _proxy: Proxy,
    covers: Arc<super::covers::Covers>,
}

/// Join the invite's room: connect (25 s at most), then knock; the rest is the session's.
pub async fn join(
    app: AppHandle,
    root: String,
    invite: Invite,
) -> Result<Sender<super::GuestWork>, String> {
    if !invite.open_at(super::unix_now()) {
        return Err("This room's invite has ended. Ask for a new one.".into());
    }
    let reach = super::net::reach_from_settings();
    let key = super::net::guest_key(&engine::paths::app_data_dir(), &invite.secret);
    let ep = super::net::endpoint(&reach, false, Some(key)).await?;
    let me = ep.id().to_string();
    let host = super::net::host_addr(&invite.to)?;
    let unreachable = || {
        format!(
            "Couldn't reach {}'s room. Check your internet, or ask for a new invite.",
            invite.host
        )
    };
    let t = IrohTransport::guest(ep, host, Arc::new(RealClock), Timing::default())
        .await
        .map_err(|_| unreachable())?;
    super::trace("room: connected to the host");
    let shared: Arc<dyn Transport> = Arc::new(t.clone());
    let proxy = Proxy::start(shared.clone(), HOST).await?;
    let (port, token) = proxy.address();
    let hs = HostStream::new(port, token);
    let lib = crate::load_lib(std::path::Path::new(&root))
        .ok()
        .map(|(_, l)| l);
    let copies = AppCopies::new(
        root,
        lib,
        app.state::<Playback>().catalog(),
        reach.local_only,
        Some(hs),
        super::host::resolver(&app),
    );
    // Each copy measured against the host's reference (WP11): its envelope made on the
    // player's downloads, the host's fetched on the env: stream, on the app's runtime.
    let spawn: Spawn = Arc::new(|f| {
        tauri::async_runtime::spawn(f);
    });
    let copies = Measured::guest(
        Box::new(copies),
        Box::new(EnvelopeMaker::<QueueItem>::new(
            super::streams(&app),
            Making::Threads,
        )),
        References::new(shared.clone(), HOST, Some(spawn)),
        Arc::new(Analysis),
    );
    let mut conductor = Conductor::new(
        Arc::new(AppPlayer(app.clone())),
        clock_map::handle(),
        Box::new(copies),
        ListenMode::Apart,
    );
    conductor.set_extra_latency(super::my_delay(&app) * 1_000);
    let s = GuestSession::new(
        shared,
        HOST,
        Arc::new(RealClock),
        conductor,
        GuestConfig {
            name: super::my_name(),
            proof: invite.proof(&me),
            app: format!("Venuta {}", engine::VERSION),
            host_name: invite.host.clone(),
            // One speaker first on the host's network, unless chosen (A16, D15).
            mode: None,
        },
    );
    let live = Live {
        s,
        t,
        app: app.clone(),
        invite,
        _proxy: proxy,
        covers: Arc::default(),
    };
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("venuta-room".into())
        .spawn(move || run(live, rx))
        .map_err(|e| e.to_string())?;
    Ok(tx)
}

fn run(mut live: Live, rx: Receiver<super::GuestWork>) {
    let mut last_view = String::new();
    let mut last_tick = Instant::now();
    let mut over_at: Option<Instant> = None;
    let mut queue_version = (0u64, String::new());
    let mut stage = live.s.stage();
    loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(f) => f(&mut live),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => live.s.leave(),
        }
        live.s.step();
        if live.s.stage() != stage {
            stage = live.s.stage();
            super::trace(&format!("room: {stage:?}"));
        }
        for n in live.s.take_notes() {
            super::note(&live.app, &n);
        }
        live.fetch_covers();
        let playing = live.s.conductor().anchor().is_some_and(|a| a.playing);
        let v = live.view();
        let json = serde_json::to_string(&v).unwrap_or_default();
        let due = playing && last_tick.elapsed() >= Duration::from_millis(100);
        if json != last_view || due {
            last_view = json;
            last_tick = Instant::now();
            super::publish(&live.app, v);
            live.bar(&mut queue_version);
        }
        if live.s.stage() == Stage::Ended {
            let at = *over_at.get_or_insert_with(Instant::now);
            // The goodbye and the connection's close go first.
            if at.elapsed() >= Duration::from_millis(500) {
                break;
            }
        }
    }
    let queue: Vec<Song> = live
        .s
        .conductor()
        .queue()
        .iter()
        .map(|e| e.song.clone())
        .collect();
    let last = live.view();
    let t = live.t.clone();
    tauri::async_runtime::spawn(async move { t.close().await });
    super::trace(&format!(
        "room: over for this guest ({})",
        live.s.why().unwrap_or_else(|| "left".into())
    ));
    super::session_over(&live.app, queue, last);
}

impl Live {
    /// A press: to the room when the role allows it, else asked of whoever controls.
    pub fn press(&mut self, action: Action) {
        let me = self.s.me();
        let role = self
            .s
            .conductor()
            .view()
            .and_then(|v| v.members.iter().find(|m| Some(m.id) == me).map(|m| m.role));
        let may = match action {
            Action::Add { .. } => role.is_some_and(|r| r != together::proto::Role::Listen),
            _ => role == Some(together::proto::Role::Control),
        };
        if may || matches!(action, Action::Add { .. }) {
            self.s.ask(action);
        } else {
            let words = format!(
                "{} is in control: asked the room.",
                self.invite.host.clone()
            );
            self.s.request(action);
            super::note(&self.app, &words);
        }
    }

    fn view(&mut self) -> View {
        let extras = WindowExtras {
            name: super::my_name(),
            local_only: engine::settings::load().listening.local_only,
            let_in: false,
            visible: false,
            output: super::output_view(&self.app),
            invite: None,
            volume: None,
            message: None,
        };
        let mut v = self.s.window(&extras);
        if let Some(room) = v.room.as_mut() {
            self.covers.decorate(room);
        }
        v
    }

    /// The host's songs' covers not here yet: kept from before, or asked for (three at a time).
    fn fetch_covers(&mut self) {
        let keys: Vec<String> = self
            .s
            .conductor()
            .queue()
            .iter()
            .filter(|e| e.song.host_file && e.song.artwork_url.is_none())
            .map(|e| e.song.key.clone())
            .filter(|k| self.covers.first_ask(k))
            .collect();
        for key in keys {
            let Some(kept) = super::covers::kept_path(&key) else {
                continue;
            };
            if kept.exists() {
                self.covers.put(&key, kept);
                continue;
            }
            let (t, covers) = (self.t.clone(), self.covers.clone());
            tauri::async_runtime::spawn(async move {
                let get = Get {
                    get: format!("cover:{key}"),
                    from: 0,
                    to: Some(super::covers::MAX_BYTES),
                };
                let Ok(mut m) = t.open_media(HOST, get).await else {
                    return;
                };
                let mut bytes = Vec::new();
                while let Ok(Some(c)) = m.body.read_chunk().await {
                    bytes.extend_from_slice(&c);
                    if bytes.len() as u64 > super::covers::MAX_BYTES {
                        return;
                    }
                }
                let k = key.clone();
                let kept =
                    tauri::async_runtime::spawn_blocking(move || super::covers::keep(&k, &bytes))
                        .await
                        .ok()
                        .flatten();
                if let Some(path) = kept {
                    covers.put(&key, path);
                }
            });
        }
    }

    /// What the room plays, on the player's own bar.
    fn bar(&mut self, version: &mut (u64, String)) {
        if self.s.stage() == Stage::Ended {
            return;
        }
        let host_name = self.invite.host.clone();
        let c = self.s.conductor();
        let host_now =
            (**c.clock().load()).map(|m| m.host_of(together::time::MonoClock::now_ns(&RealClock)));
        let queue = c.queue().to_vec();
        let key = queue
            .iter()
            .map(|e| e.id.0.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let changed = key != version.1;
        if changed {
            version.0 += 1;
            version.1 = key;
        }
        let items: Vec<QueueItem> = queue
            .iter()
            .map(|e| super::display_item(&e.song, None))
            .collect();
        let (index, playing, position_ms, duration_ms) =
            match together::session::now_playing(c, host_now) {
                Some((entry, song, content, playing)) => (
                    queue
                        .iter()
                        .position(|e| e.id == entry)
                        .map_or(-1, |i| i as i64),
                    playing,
                    (content / 1_000) as u64,
                    song.duration_ms.unwrap_or(0),
                ),
                None => (-1, false, 0, 0),
            };
        let buffering = c.status().is_some_and(|s| {
            matches!(
                s.sub_state,
                together::room_player::SubState::Opening
                    | together::room_player::SubState::Suspended(_)
            )
        }) && playing;
        let pb = self.app.state::<Playback>();
        let first = !pb.in_room();
        pb.set_room_now_quiet(Some(RoomNow {
            items,
            index,
            playing,
            position_ms,
            duration_ms,
            buffering,
            from: format!("{host_name}'s room"),
        }));
        pb.room_tick(changed || first);
    }
}
