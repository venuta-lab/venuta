//! The output's render thread allocates and frees nothing while it streams (sections 3.2 and
//! 7.5, WP1): a counting allocator, switched on by the fake device once the stream runs and off
//! inside the fake's own calls, sees nothing of the loop's own work over steady periods, a
//! late wake that runs the DAC dry (the `Xrun` queued and its forwarder woken), skipped
//! readings, and a stop with its tail drained, in shared and exclusive mode.

mod pull_kit;

use player::output::PullOutput;
use player::output_api::{Output, OutputEvent};
use pull_kit::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use together::time::{SimClock, Timebase};

struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);
/// Allocations by the fake itself while watched: proof the watch was on the render thread.
static FAKE_ALLOCS: AtomicU64 = AtomicU64::new(0);

fn count(on: &AtomicU64) {
    if watched() {
        on.fetch_add(1, Ordering::Relaxed);
    } else if watching() {
        FAKE_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every call is passed straight to the system allocator; only counted on the side.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(&ALLOCS);
        // SAFETY: as the caller promised for `alloc`.
        unsafe { System.alloc(l) }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count(&ALLOCS);
        // SAFETY: as the caller promised for `alloc_zeroed`.
        unsafe { System.alloc_zeroed(l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        count(&ALLOCS);
        // SAFETY: as the caller promised for `realloc`.
        unsafe { System.realloc(p, l, size) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count(&FREES);
        // SAFETY: as the caller promised for `dealloc`.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn the_render_thread_allocates_nothing_while_it_streams() {
    for exclusive in [false, true] {
        let clock = SimClock::new(1_000 * MS);
        let opener = FakeOpener::new(
            FakeCfg {
                exclusive,
                latency: 48,
                late: vec![(150, if exclusive { 15 } else { 80 } * MS)],
                no_reading_every: 5,
                watch_from: Some(3),
                ..FakeCfg::default()
            },
            SimOrReal::Sim(clock.clone()),
        );
        let fake = opener.fake.clone();
        let mut out = PullOutput::new(opener, Arc::new(clock.clone()));
        out.open(&want()).unwrap();
        let events = out.events();
        let c = Counts::default();
        out.start(c.render()).unwrap();
        clock.pass(3_000 * MS);
        out.stop().unwrap();
        clock.pass(500 * MS);
        let f = fake.lock().unwrap();
        assert!(f.gap_at.is_some(), "the late wake ran the DAC dry");
        assert!(f.halts.len() >= 2, "the tail drained and the device halted");
        drop(f);
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(5)),
            Ok(OutputEvent::Xrun { .. })
        ));
        out.close();
        assert_eq!(
            (
                ALLOCS.load(Ordering::Relaxed),
                FREES.load(Ordering::Relaxed)
            ),
            (0, 0),
            "allocations and frees on the render thread (exclusive {exclusive})"
        );
        assert!(
            FAKE_ALLOCS.load(Ordering::Relaxed) > 0,
            "the watch was on the render thread (the fake allocates there)"
        );
        FAKE_ALLOCS.store(0, Ordering::Relaxed);
    }
}
