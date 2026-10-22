//! The clock map: the line that turns this computer's monotonic time into the host's (contract
//! 5.5 of the Listen together design).
//!
//! The clock service publishes one into a shared handle each time its estimate moves (slewed,
//! so at most 50 µs a second); the player's sync tick reads it every 20 ms and never re-plans
//! anything when it changes. The host's own map is the identity.
//!
//! This file only declares the shape every package builds against. The mapping itself
//! (`host_of`, `local_of`) is the clock's, written with the clock service (`clock.rs`).

use arc_swap::ArcSwap;
use std::sync::Arc;

/// A line from local time to host time.
///
/// - `epoch`: which run of the estimate this belongs to. A new epoch starts when the window is
///   cleared (a machine that slept, a clock reset, a sample over 500 ms off the map): anything
///   planned on an older epoch is measured again.
/// - `local_ref_ns`: a point on this computer's monotonic clock (`time::MonoClock`), in ns.
/// - `host_ref_us`: the host's monotonic time at that point, in µs.
/// - `rate`: host µs per local µs, the slope of the line (about 1.0; 1.0001 when the host's
///   clock runs 100 ppm fast against this one).
/// - `bound_us`: how far off the map may be, honestly (least delay / 2 plus the 90th percentile
///   of the fit's residuals); shown in the panel and used to widen the controller's band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockMap {
    pub epoch: u32,
    pub local_ref_ns: u64,
    pub host_ref_us: i64,
    pub rate: f64,
    pub bound_us: u32,
}

/// Where the clock service publishes its map and the player reads it: `None` until the clock is
/// ready (8 samples and 1 s of burst). Lock-free on both sides.
pub type ClockMapHandle = Arc<ArcSwap<Option<ClockMap>>>;

/// A handle with nothing published yet.
pub fn handle() -> ClockMapHandle {
    Arc::new(ArcSwap::from_pointee(None))
}
