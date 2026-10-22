//! The render thread never allocates or frees (sections 4.7 and 7.5): a counting allocator,
//! switched on only inside `render` calls, sees nothing over a run that goes through every path
//! of the renderer: starts on their frame, late and partway; every splice; gapless joins in one
//! window and between rates; conversion up and down; a trim gliding on and landing back on whole
//! frames; volume ramps; sound tools swapped; a stream running dry and coming back; a newer
//! epoch; stops and drops; refused commands; and full queues.

mod render_kit;

use player::dsp::{Chain, Gain, Sound};
use player::voice_api::{RenderCmd, Splice, VoiceId};
use render_kit::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
}

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);

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

fn switch(on: bool) {
    ON.with(|c| c.set(on));
}

const R: u32 = 48_000;

fn s(secs: f64) -> u64 {
    (secs * f64::from(R)) as u64
}

/// Allocations and frees inside `render` so far.
fn counted() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::Relaxed),
        FREES.load(Ordering::Relaxed),
    )
}

#[test]
fn the_renderer_allocates_nothing_on_any_path() {
    // The counter itself works: an allocation while switched on is seen.
    switch(true);
    let v = std::hint::black_box(vec![1u8; 64]);
    switch(false);
    drop(v);
    assert_eq!(counted(), (1, 0), "the counting allocator counts");
    ALLOCS.store(0, Ordering::Relaxed);

    let mut rig = Rig::new(R, 2, 480);
    rig.around = Some(switch);

    // Start on a frame mid-period, at the song's first frame; a trim on and off again.
    let a = rig.start(&Song::music(R, 2, 0, s(30.0)).rising(), 0, 1_001, 0);
    rig.run(s(1.0));
    rig.trim.set_ppm(300.0);
    rig.run(s(1.0));
    rig.trim.set_ppm(0.0);
    rig.run(s(1.5));
    // Volume down and up; sound tools on and off.
    rig.send(RenderCmd::Gain { target: 0.3 });
    rig.run(s(0.2));
    rig.send(RenderCmd::Gain { target: 1.0 });
    let mut bands = [0.0; 10];
    bands[3] = 4.0;
    bands[8] = -3.0;
    let preamp = Sound {
        eq: true,
        bands,
        preamp_db: -3.0,
        bass_db: 2.0,
        treble_db: -1.5,
        ..Sound::default()
    };
    rig.send(RenderCmd::Chain {
        voice: a,
        chain: Box::new(Chain::new(&preamp, &Gain::default(), R, 2, 32)),
    });
    rig.run(s(0.2));
    rig.send(RenderCmd::Chain {
        voice: a,
        chain: Box::new(Chain::new(&Sound::default(), &Gain::default(), R, 2, 32)),
    });
    rig.run(s(0.2));
    // A seek (crossfade) to another place, then a skip to a CD-rate song (converted up).
    let song = Song::music(R, 2, 0, s(30.0));
    let (b, tap) = rig.voice(&song, s(9.0));
    let at = rig.at + 2_000;
    rig.send(RenderCmd::SwitchAt {
        from: a,
        to: b,
        tap,
        at,
        to_content: s(9.0),
        rate: R,
        how: Splice::Crossfade { frames: 1_920 },
    });
    rig.run(s(0.5));
    rig.send(RenderCmd::Drop { voice: a });
    let cd = Song::music(44_100, 2, 0, 44_100 * 20).rising();
    let (c, tap) = rig.voice(&cd, 0);
    let at = rig.at + 3_000;
    rig.send(RenderCmd::SwitchAt {
        from: b,
        to: c,
        tap,
        at,
        to_content: 0,
        rate: 44_100,
        how: Splice::FadeOutThenIn { out_frames: 1_440 },
    });
    rig.run(s(0.5));
    rig.send(RenderCmd::Drop { voice: b });
    rig.trim.set_ppm(-250.0);
    rig.run(s(0.5));
    // Gapless into the next CD-rate song in the same window, then into a 96 kHz one (converted
    // down, joined on the frame after).
    let cd2 = Song::music(44_100, 2, 44_100 * 20, 44_100 * 2);
    let (d, tap) = rig.voice(&cd2, 0);
    rig.send(RenderCmd::SwitchAt {
        from: c,
        to: d,
        tap,
        at: 0,
        to_content: 0,
        rate: 44_100,
        how: Splice::Gapless,
    });
    let hi = Song::music(96_000, 2, 0, 96_000 * 20).rising();
    let (e, tap) = rig.voice(&hi, 0);
    rig.send(RenderCmd::SwitchAt {
        from: d,
        to: e,
        tap,
        at: 0,
        to_content: 0,
        rate: 96_000,
        how: Splice::Gapless,
    });
    rig.run(s(19.0) + s(2.0) + s(1.0));
    rig.send(RenderCmd::Drop { voice: c });
    rig.send(RenderCmd::Drop { voice: d });
    rig.trim.set_ppm(0.0);
    // It runs dry and comes back.
    // (Its decode thread stops: what the ring holds plays out first.)
    rig.feed(e).limit = rig.feed(e).next;
    rig.run(s(5.0));
    rig.feed(e).limit = u64::MAX;
    rig.run(s(2.0));
    // A newer epoch while it sounds.
    rig.feed(e).seek_to(96_000 * 12);
    rig.run(s(2.0));
    // A late start partway, a longer copy's end fade, a crossfade boundary, stops.
    let f = rig.start(&song, s(3.0), rig.at - 960, 0);
    rig.run(s(0.5));
    let (g, tap) = rig.voice(&song, s(12.0));
    let at = rig.at + 9_000;
    rig.send(RenderCmd::SwitchAt {
        from: f,
        to: g,
        tap,
        at,
        to_content: s(12.0),
        rate: R,
        how: Splice::EndFade { frames: 7_200 },
    });
    rig.run(s(0.5));
    rig.send(RenderCmd::StopAt {
        voice: g,
        at: rig.at + 100,
        fade: R,
    });
    rig.send(RenderCmd::StopAt {
        voice: e,
        at: rig.at,
        fade: 0,
    });
    rig.run(s(1.5));
    // Refused: no such voice, a source too fast, no free slot.
    rig.send(RenderCmd::Drop { voice: VoiceId(77) });
    let fast = Song::music(700_000, 2, 0, 700_000);
    rig.start(&fast, 0, rig.at, 0);
    for _ in 0..5 {
        rig.start(&song, s(1.0), rig.at, 0);
    }
    rig.run(s(0.5));
    let (allocs, frees) = counted();
    assert_eq!(
        (allocs, frees),
        (0, 0),
        "allocations and frees inside render"
    );
    // The run really went through those paths.
    let saw = |f: &dyn Fn(&player::voice_api::RenderNote) -> bool| rig.notes.iter().any(f);
    use player::voice_api::{FadeKind, RenderNote as N};
    assert!(saw(&|n| matches!(n, N::Joined { .. })));
    assert!(saw(&|n| matches!(n, N::Starved { .. })));
    assert!(saw(&|n| matches!(n, N::Resumed { .. })));
    assert!(saw(&|n| matches!(n, N::Late { .. })));
    assert!(saw(&|n| matches!(n, N::Refused { .. })));
    assert!(saw(&|n| matches!(n, N::Resampling { on: false, .. })));
    assert!(saw(&|n| matches!(
        n,
        N::Faded {
            kind: FadeKind::CrossIn,
            ..
        }
    )));
    assert!(saw(&|n| matches!(n, N::Gain { .. })));
}

#[test]
fn full_queues_lose_notes_and_markers_without_allocating() {
    let mut rig = Rig::with_queues(int32(R, 2), 480, 8, 2, 2);
    rig.around = Some(switch);
    rig.drain_each_step = false;
    let song = Song::music(R, 2, 0, s(10.0));
    let a = rig.start(&song, s(1.0), 0, 720);
    rig.run(s(0.5));
    rig.send(RenderCmd::StopAt {
        voice: a,
        at: rig.at,
        fade: 1_440,
    });
    rig.run(s(0.5));
    let before = counted();
    rig.run(s(0.5));
    assert_eq!(counted(), before, "nothing allocated with the queues full");
    assert!(rig.status.read().lost > 0, "what was lost is counted");
    // Once drained, the next note says what was lost.
    rig.drain();
    rig.send(RenderCmd::Gain { target: 0.5 });
    rig.step();
    rig.drain();
    assert!(
        rig.notes
            .iter()
            .any(|n| matches!(n, player::voice_api::RenderNote::Dropped { .. })),
        "{:?}",
        rig.notes
    );
}
