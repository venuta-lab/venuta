//! The library engine. Everything the app and the CLI do goes through here; it has no UI.
//!
//! Safety rule for the whole crate: nothing writes to a user's music file except the
//! apply step (not yet built), and that step always writes a temp copy, verifies the audio
//! is untouched, then renames.

pub mod accounts;
pub mod apple;
pub mod apply;
pub mod art;
pub mod audit;
pub mod bridges;
pub mod cache;
pub mod compare;
pub mod convert;
pub mod device;
pub mod edit;
pub mod enrich;
pub mod error;
pub mod exports;
pub mod fingerprint;
pub mod formats;
pub mod imports;
pub mod index;
pub mod intake;
pub mod library;
pub mod links;
pub mod lrc;
pub mod lyrics;
pub mod lyrics_online;
pub mod media;
pub mod net;
pub mod palette;
pub mod paths;
pub mod plan;
pub mod playlists;
pub mod providers;
pub mod quality;
pub mod review;
pub mod rules;
pub mod scan;
pub mod settings;
pub mod songid;
pub mod spectrogram;
pub mod sync;
pub mod text;
pub mod themes;
pub mod tidy;
pub mod volumes;
pub mod ytm_links;

pub use error::{Error, Result};
pub use index::Index;
pub use scan::{ScanProgress, ScanReport, ScanStage, scan, scan_with};

/// Engine version, shown by the CLI and the app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
