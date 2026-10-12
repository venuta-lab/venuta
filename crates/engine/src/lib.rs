//! The library engine. Everything the app and the CLI do goes through here; it has no UI.
//!
//! Safety rule for the whole crate: nothing writes to a user's music file except the
//! apply step (not yet built), and that step always writes a temp copy, verifies the audio
//! is untouched, then renames.

pub mod apple;
pub mod apply;
pub mod art;
pub mod audit;
pub mod compare;
pub mod edit;
pub mod enrich;
pub mod error;
pub mod index;
pub mod intake;
pub mod library;
pub mod media;
pub mod net;
pub mod paths;
pub mod plan;
pub mod playlists;
pub mod providers;
pub mod quality;
pub mod review;
pub mod rules;
pub mod scan;
pub mod settings;
pub mod spectrogram;
pub mod sync;
pub mod volumes;

pub use error::{Error, Result};
pub use index::Index;
pub use scan::{ScanReport, scan};

/// Engine version, shown by the CLI and the app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
