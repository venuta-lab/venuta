//! Venuta's phone server: the library to phones and players on the same Wi-Fi.
//!
//! * OpenSubsonic under `/rest/`, so existing apps (Symfonium and Tempus on Android, Amperfy on
//!   iPhone) browse, stream, and download for offline.
//! * Only your own network may ask ([`lan`]); each phone signs in with a name and a password
//!   Venuta made for it ([`auth`]); nothing a phone asks can change the library.
//! * Off until you turn it on; it stops when you turn it off or close Venuta.

pub mod api;
pub mod auth;
pub mod catalog;
pub mod lan;
pub mod respond;

pub use auth::Phone;

use axum::Router;
use axum::routing::{any, get};
use catalog::Catalog;
use engine::library::Library;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

/// The default port, when it's free: easy to type, and not one another music server uses.
pub const DEFAULT_PORT: u16 = 4848;

/// Where the server reads the library from (the app keeps it fresh).
pub trait Source: Send + Sync + 'static {
    /// The library's folder (playlists and songs are read relative to it).
    fn root(&self) -> PathBuf;
    /// The library as it is now.
    fn library(&self) -> Result<Arc<Library>, String>;
    /// A phone played this song to the end.
    fn played(&self, _path: &str) {}
}

pub struct App {
    source: Arc<dyn Source>,
    phones: RwLock<Vec<Phone>>,
    /// The catalog of the last library snapshot, rebuilt only when the library changes.
    snapshot: Mutex<Option<(Arc<Library>, Arc<Catalog>)>>,
}

impl App {
    pub fn phones(&self) -> Vec<Phone> {
        self.phones.read().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn root(&self) -> PathBuf {
        self.source.root()
    }

    pub fn played(&self, path: &str) {
        self.source.played(path)
    }

    /// The library now, and its catalog.
    pub fn snapshot(&self) -> Result<(Arc<Library>, Arc<Catalog>), String> {
        let lib = self.source.library()?;
        let mut slot = self.snapshot.lock().map_err(|e| e.to_string())?;
        if let Some((l, c)) = slot.as_ref()
            && Arc::ptr_eq(l, &lib)
        {
            return Ok((l.clone(), c.clone()));
        }
        let cat = Arc::new(Catalog::build(&lib));
        *slot = Some((lib.clone(), cat.clone()));
        Ok((lib, cat))
    }
}

/// The server, running.
pub struct Running {
    pub addr: SocketAddr,
    app: Arc<App>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Running {
    /// Phones added or removed take effect on the next request.
    pub fn set_phones(&self, phones: Vec<Phone>) {
        if let Ok(mut p) = self.app.phones.write() {
            *p = phones;
        }
    }

    pub fn stop(mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/rest/{method}", any(api::rest))
        .route(
            "/",
            get(|| async { "Venuta is sharing music on this network." }),
        )
        .with_state(app)
}

/// Start listening on every network address of this PC, on `port` (0: any free port).
/// Requests from outside your own network are refused by [`lan::is_lan`].
pub async fn start(
    port: u16,
    source: Arc<dyn Source>,
    phones: Vec<Phone>,
) -> std::io::Result<Running> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    let addr = listener.local_addr()?;
    let app = Arc::new(App {
        source,
        phones: RwLock::new(phones),
        snapshot: Mutex::new(None),
    });
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let service = router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, service)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Ok(Running {
        addr,
        app,
        stop: Some(stop),
    })
}
