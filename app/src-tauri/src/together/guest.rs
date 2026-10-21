//! Joining a room: the connection to the host, the knock, the clock and the host's files are
//! the room's networking (`together_net::guest`); this is the app around it: this player
//! following the room on the host's clock, the host's covers kept on this computer, and the
//! window told of every change.

use super::emit;
use super::follow::{self, Follower, Report};
use super::net::{self, News};
use super::sources::HostStream;
use crate::playback::Playback;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use together::invite::Invite;
use together::proto::{Hearing, RoomView, ToHost};
pub use together_net::guest::Stage;
use together_net::guest::{GuestHooks, GuestSession};

pub struct Guest {
    session: Arc<GuestSession>,
    glue: Arc<Glue>,
}

/// The room's connection, its view and where joining is.
impl std::ops::Deref for Guest {
    type Target = GuestSession;
    fn deref(&self) -> &GuestSession {
        &self.session
    }
}

/// What the room's networking asks of the app.
struct Glue {
    app: AppHandle,
    follow: Mutex<Option<Sender<follow::In>>>,
    /// The covers of the host's songs, as they come (a few asked for at a time).
    covers: super::covers::Covers,
    asking_covers: Arc<tokio::sync::Semaphore>,
    /// Itself, for the covers asked for in tasks of their own.
    me: std::sync::Weak<Glue>,
}

impl Glue {
    fn to_follower(&self, m: follow::In) {
        if let Some(tx) = self.follow.lock().unwrap().as_ref() {
            let _ = tx.send(m);
        }
    }

    /// The room as this computer shows and plays it: the host's songs with their covers.
    fn with_covers(&self, mut v: RoomView) -> RoomView {
        self.covers.decorate(&mut v);
        v
    }
}

impl GuestHooks for Glue {
    fn changed(&self) {
        emit(&self.app);
    }
    fn trace(&self, what: &str) {
        super::trace(what);
    }
    fn note(&self, what: &str) {
        let _ = self.app.emit("together-note", what);
    }
    fn news(&self, news: News) {
        let m = match news {
            News::Room(v) => follow::In::Room(self.with_covers(v)),
            n => follow::In::from(n),
        };
        self.to_follower(m);
    }
    /// The covers of the host's songs not here yet: kept from before, or asked for.
    fn view(&self, g: &Arc<GuestSession>, v: &RoomView) {
        for s in &v.queue {
            if !s.host_file || s.artwork_url.is_some() || !self.covers.first_ask(&s.key) {
                continue;
            }
            let Some(kept) = super::covers::kept_path(&s.key) else {
                continue;
            };
            if kept.exists() {
                self.covers.put(&s.key, kept);
                continue;
            }
            let g = g.clone();
            let turns = self.asking_covers.clone();
            let Some(glue) = self.me.upgrade() else {
                continue;
            };
            let key = s.key.clone();
            tauri::async_runtime::spawn(async move {
                let Ok(_turn) = turns.acquire_owned().await else {
                    return;
                };
                if let Some(bytes) = g.fetch_cover(&key, super::covers::MAX_BYTES).await {
                    let k = key.clone();
                    let kept = tauri::async_runtime::spawn_blocking(move || {
                        super::covers::keep(&k, &bytes)
                    })
                    .await
                    .ok()
                    .flatten();
                    if let Some(path) = kept {
                        glue.covers.put(&key, path);
                        emit(&glue.app);
                    }
                }
            });
        }
    }
    fn name(&self) -> String {
        super::my_name()
    }
}

impl Guest {
    pub async fn join(app: AppHandle, root: String, invite: Invite) -> Result<Arc<Guest>, String> {
        let reach = net::reach_from_settings();
        let glue = Arc::new_cyclic(|me: &std::sync::Weak<Glue>| Glue {
            app: app.clone(),
            follow: Mutex::default(),
            covers: Default::default(),
            asking_covers: Arc::new(tokio::sync::Semaphore::new(3)),
            me: me.clone(),
        });
        let session = GuestSession::join(
            invite,
            &reach,
            &engine::paths::app_data_dir(),
            format!("Venuta {}", engine::VERSION),
            glue.clone(),
        )
        .await?;
        let (port, token) = session.proxy_address();
        let hs = HostStream::new(port, token);
        // This player follows the room, on the host's clock.
        let clock = session.clock.clone();
        let local = session.local;
        let lib = crate::load_lib(std::path::Path::new(&root))
            .ok()
            .map(|(_, l)| l);
        let tx = follow::start(Follower {
            seat: Arc::new(follow::AppSeat(app.clone())),
            root: root.clone(),
            lib,
            catalog: app.state::<Playback>().catalog(),
            local_only: reach.local_only,
            host_stream: Some(hs),
            hosting: false,
            to_local: Arc::new(move |host_us| {
                clock
                    .lock()
                    .ok()?
                    .local_time(host_us)
                    .map(|l| local.instant(l))
            }),
            report: Box::new(GuestReport(Arc::downgrade(&session))),
            delay_ms: super::my_delay(&app),
            owner: 0,
        });
        *glue.follow.lock().unwrap() = Some(tx);
        Ok(Arc::new(Guest { session, glue }))
    }

    /// The room as this computer shows and plays it: the host's songs with their covers.
    pub fn with_covers(&self, v: RoomView) -> RoomView {
        self.glue.with_covers(v)
    }

    pub fn to_follower(&self, m: follow::In) {
        self.glue.to_follower(m);
    }
}

struct GuestReport(std::sync::Weak<GuestSession>);

impl Report for GuestReport {
    fn ready(&self, rev: u64, index: usize) {
        if let Some(g) = self.0.upgrade() {
            g.send(ToHost::Ready { rev, index });
        }
    }
    fn hearing(&self, rev: u64, index: usize, how: Hearing) {
        if let Some(g) = self.0.upgrade() {
            g.send(ToHost::Hearing { rev, index, how });
        }
    }
    fn sync(&self, error_ms: i32) {
        if let Some(g) = self.0.upgrade() {
            g.send(ToHost::Sync { error_ms });
        }
    }
}
