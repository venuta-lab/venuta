//! Engine 2's render path never allocates or frees (sections 4.7 and 7.5): a counting
//! allocator, switched on only inside the render calls the engine hands its output (the
//! renderer and the spectrum's ring after it), sees nothing over every solo action: a start,
//! gapless joins, a crossfade between songs, seeks while playing and paused, skips, a pause and
//! a resume, volume ramps, sound tools gliding on and off, the spectrum, the sleep timer.

mod engine_kit;

use engine_kit::*;
use player::dsp::Sound;
use player::engine::Cmd;
use player::output_api::Render;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
}

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);

fn counting() -> bool {
    ON.try_with(Cell::get).unwrap_or(false)
}

// SAFETY: every call is passed straight to the system allocator; only counted on the side.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if counting() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as the caller promised for `alloc`.
        unsafe { System.alloc(l) }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if counting() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as the caller promised for `alloc_zeroed`.
        unsafe { System.alloc_zeroed(l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        if counting() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as the caller promised for `realloc`.
        unsafe { System.realloc(p, l, size) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if counting() {
            FREES.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as the caller promised for `dealloc`.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The engine's render path, counted.
struct Counted(Box<dyn Render>);

impl Render for Counted {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        ON.with(|c| c.set(true));
        self.0.render(out, frames, at);
        ON.with(|c| c.set(false));
        CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn the_render_path_allocates_and_frees_nothing_through_every_solo_action() {
    let (a, b, c) = (
        song(0.0, 6.0, true, false),
        song(6.0, 3.0, false, true),
        song(30.0, 6.0, true, true),
    );
    let mut r: Rig<PathBuf> = Rig::with_wrap(Some(Box::new(|inner| Box::new(Counted(inner)))));
    r.send(Cmd::Spectrum(true));
    r.send(play(vec![a.clone(), b.clone(), c.clone(), a.clone()]));
    r.pass(1_000);
    r.send(Cmd::Seek(4_000));
    r.pass(500);
    r.send(Cmd::Volume(0.4));
    r.pass(300);
    r.send(Cmd::Sound(Sound {
        bass_db: 6.0,
        treble_db: -3.0,
        ..Sound::default()
    }));
    r.pass(300);
    r.send(Cmd::Sound(Sound::default()));
    r.pass(300);
    r.send(Cmd::Volume(1.0));
    // On through the gapless join into B.
    assert!(r.until(10_000, |s| s.index == Some(1)));
    r.pass(300);
    r.send(Cmd::Sound(Sound {
        crossfade_s: 1.0,
        ..Sound::default()
    }));
    // A crossfade from B into C.
    assert!(r.until(10_000, |s| s.index == Some(2)));
    r.pass(1_500);
    r.send(Cmd::Next);
    r.pass(500);
    r.send(Cmd::Previous);
    r.pass(500);
    r.send(Cmd::Pause);
    r.pass(300);
    r.send(Cmd::Seek(2_000));
    r.pass(100);
    r.send(Cmd::Play);
    r.pass(500);
    r.send(Cmd::Jump(0));
    r.pass(500);
    r.send(Cmd::SleepIn(Some(std::time::Duration::from_secs(2))));
    assert!(r.until(5_000, |s| !s.playing));
    r.finish();
    assert!(
        CALLS.load(Ordering::Relaxed) > 1_000,
        "the render path ran under the count"
    );
    assert_eq!(
        (
            ALLOCS.load(Ordering::Relaxed),
            FREES.load(Ordering::Relaxed)
        ),
        (0, 0),
        "allocations and frees on the render path"
    );
}
