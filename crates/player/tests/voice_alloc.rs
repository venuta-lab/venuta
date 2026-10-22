//! The renderer's side of a voice never allocates or frees (design 4.7; WP2's gate). A counting
//! allocator counts every allocation and free made on the thread that plays the renderer's part:
//! taking blocks, reading them, giving them back, for a whole song while its decode thread
//! decodes beside it.

use player::decode::Source;
use player::voice::{Pace, Voice, VoiceSpec};
use player::voice_api::{END_OF_SOURCE, VoiceId};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

fn count() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn the_counter_counts() {
    COUNTING.with(|on| on.set(true));
    let b = std::hint::black_box(Box::new(7u64));
    drop(b);
    COUNTING.with(|on| on.set(false));
    assert_eq!(COUNT.with(Cell::get), 2, "one allocation, one free");
}

#[test]
fn the_renderer_s_side_of_a_voice_never_allocates() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("seek-music.ogg");
    let voice = Voice::start(VoiceSpec {
        id: VoiceId(1),
        open: Box::new(move || Source::open(&path)),
        start: 0,
        // A small ring, so the decode thread works the whole time the renderer plays.
        depth: Some(Duration::from_millis(100)),
        pace: Pace::Real,
    });
    let until = Instant::now() + Duration::from_secs(20);
    let tap = loop {
        if let Some(t) = voice.take_tap() {
            break t;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    };
    let (counted, frames) = std::thread::spawn(move || {
        let mut frames = 0u64;
        let mut sum = 0i64;
        COUNTING.with(|on| on.set(true));
        loop {
            match tap.pop() {
                Some(b) => {
                    frames += u64::from(b.frames);
                    sum = sum.wrapping_add(b.data().iter().map(|&s| i64::from(s)).sum::<i64>());
                    let end = b.flags & END_OF_SOURCE != 0;
                    tap.give_back(b);
                    if end {
                        break;
                    }
                }
                None => std::thread::yield_now(),
            }
        }
        COUNTING.with(|on| on.set(false));
        std::hint::black_box(sum);
        let counted = COUNT.with(Cell::get);
        drop(tap);
        (counted, frames)
    })
    .join()
    .unwrap();
    assert!(
        frames > 200_000,
        "a whole song went through ({frames} frames)"
    );
    assert_eq!(counted, 0, "allocations or frees on the renderer's side");
}
