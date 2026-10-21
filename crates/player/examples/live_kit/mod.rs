//! The hardware test kit's shared parts (work package 14 of docs/design/listen-together.md).
//!
//! Each tool in `crates/player/examples` takes this folder in with
//! `#[path = "live_kit/mod.rs"] mod live_kit;`. Cargo builds no example from it (it has no
//! `main.rs`). The analysis (detectors, alignment, WAV files, statistics) is platform neutral;
//! only `capture` and `play` speak WASAPI, and only on Windows.
//!
//! The click detectors are reached through `checks` alone, so moving them to
//! `crates/audio-check` at merge is a change to that one file.

// Every tool uses only some of the kit.
#![allow(dead_code)]

pub mod align;
pub mod args;
pub mod checks;
pub mod deadline;
pub mod detect34;
pub mod faults;
pub mod fft;
pub mod stats;
pub mod timeline;
pub mod wav;

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod play;
