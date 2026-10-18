//! Where online timings go: how long finding a stream took and which way found it, how long
//! until the first audio arrived, how long a search took. The app points this at its playback
//! log, so a slow start can be told apart from a slow network afterwards. Never an address.

use std::sync::OnceLock;
use std::time::Instant;

type Sink = Box<dyn Fn(&str) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();

/// Send timing lines to `sink` from now on. Only the first call takes effect.
pub fn set_timing_log(sink: impl Fn(&str) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(sink));
}

/// Write one timing line, if anyone listens.
pub fn note_timing(line: &str) {
    if let Some(sink) = SINK.get() {
        sink(line);
    }
}

/// Milliseconds since `start`, for a timing line.
pub fn ms_since(start: Instant) -> u128 {
    start.elapsed().as_millis()
}
