//! The clock map (design 5.5): a line that turns this computer's monotonic time into the
//! host's, published by the clock service and read directly by the player's sync tick.

use arc_swap::ArcSwap;
use std::sync::Arc;

/// The host's time as a line through this computer's: at local `local_ref_ns` the host is at
/// `host_ref_us`, and from there the host's clock runs `rate` host microseconds per local
/// microsecond (about 1.0). `bound_us` is how far the line may be from the truth; `epoch`
/// changes when the old line no longer describes the clocks (a sleep, a clock reset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockMap {
    pub epoch: u32,
    pub local_ref_ns: u64,
    pub host_ref_us: i64,
    pub rate: f64,
    pub bound_us: u32,
}

/// Where the clock service publishes its map; `None` until the clock is ready.
pub type ClockMapHandle = Arc<ArcSwap<Option<ClockMap>>>;
