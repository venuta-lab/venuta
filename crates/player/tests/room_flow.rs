//! The room's whole flow as a follower drives the player: a song plays, is skipped, paused,
//! resumed, sought while it gets ready. Each step sends the player exactly what the follower
//! sends (`app/src-tauri/src/together/follow.rs`): the room's paused timeline first, then the
//! song made ready silent (no start yet), then, once everyone is ready, the start. Songs are
//! counting WAVs (each frame says which frame of which song it is), so the device's bytes show
//! where every sound came from.

mod engines;
use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::SyncPlan;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
/// Song B's frames count from here, song C's from `C` (song A's from 0); all within 24 bits.
const B: i32 = 4_000_000;
const C: i32 = -6_000_000;

fn wav(path: &Path, frames: usize, base: i32) {
    let data_len = frames * 3;
    let mut v = Vec::with_capacity(44 + data_len);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 3).to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    v.extend_from_slice(&24u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        v.extend_from_slice(&(base + i as i32 + 1).to_le_bytes()[..3]);
    }
    std::fs::write(path, v).unwrap();
}

/// A counting song of `secs` seconds whose frame i is `base + i + 1`.
fn counting(name: &str, secs: u32, base: i32) -> PathBuf {
    let d = std::env::temp_dir().join(format!("venuta-flow-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    wav(&f, (RATE * secs) as usize, base);
    f
}

fn heard(c: &Captured) -> Vec<i32> {
    c.bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 8)
        .collect()
}

/// What was heard as stretches of music played straight on: (first frame value, frames), for
/// every stretch of at least 200 frames (silence, and the frames a fade scales, between them).
/// A song played through once is one stretch; every seek starts another.
fn runs(h: &[i32]) -> Vec<(i32, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < h.len() {
        let mut j = i + 1;
        while j < h.len() && h[i] != 0 && h[j] == h[j - 1] + 1 {
            j += 1;
        }
        if h[i] != 0 && j - i >= 200 {
            out.push((h[i], j - i));
        }
        i = j;
    }
    out
}

fn ms(frames: i32) -> f64 {
    f64::from(frames) * 1_000.0 / f64::from(RATE)
}

/// The room, as one listener's follower sees it: a timeline (position at a moment) that is
/// playing or not.
#[derive(Clone, Copy)]
struct Timeline {
    position_ms: u64,
    at: Instant,
    playing: bool,
}

impl Timeline {
    fn position_at(&self, now: Instant) -> u64 {
        if !self.playing || now <= self.at {
            return self.position_ms;
        }
        self.position_ms + now.duration_since(self.at).as_millis() as u64
    }
    fn plan(&self) -> SyncPlan {
        SyncPlan {
            anchor: Some(self.at),
            position_ms: self.position_ms,
            playing: self.playing,
            same_room: false,
            extra_latency_ms: 0,
        }
    }
}

/// The plan a follower sends with a song made ready: silent at `position_ms`, no start yet.
fn ready_at(position_ms: u64) -> SyncPlan {
    SyncPlan {
        anchor: None,
        position_ms,
        playing: true,
        same_room: false,
        extra_latency_ms: 0,
    }
}

struct Run {
    player: Player<PathBuf>,
    captured: Arc<Mutex<Captured>>,
    errors: Arc<Mutex<Vec<(Instant, f32)>>>,
}

impl Run {
    fn new() -> Self {
        let captured = Arc::new(Mutex::new(Captured::default()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let seen = errors.clone();
        let player = Player::spawn(
            OutputChoice::PacedMemory(captured.clone()),
            move |s: Status<PathBuf>| {
                if let Some(e) = s.sync.and_then(|y| y.error_ms) {
                    seen.lock().unwrap().push((Instant::now(), e));
                }
            },
        );
        Run {
            player,
            captured,
            errors,
        }
    }

    fn send(&self, c: Cmd<PathBuf>) {
        self.player.send(c);
    }

    /// The follower opens a song for the room: the player has it alone, ready at `ms`.
    fn load(&self, file: &Path, ms: u64) {
        self.send(Cmd::Set {
            items: vec![file.to_path_buf()],
            index: 0,
            start_ms: ms,
            play: false,
            from: String::new(),
        });
        self.send(Cmd::Sync(Some(ready_at(ms))));
    }

    /// The room starts what is ready, `lead_ms` from now.
    fn start(&self, position_ms: u64, lead_ms: u64) -> Timeline {
        let t = Timeline {
            position_ms,
            at: Instant::now() + Duration::from_millis(lead_ms),
            playing: true,
        };
        self.send(Cmd::Sync(Some(t.plan())));
        t
    }

    /// The room pauses `t` where it was `late_ms` ago (the press reaches this player a moment
    /// after the host took the room's place): the player is told the paused timeline.
    fn pause(&self, t: &Timeline, late_ms: u64) -> Timeline {
        let at = Instant::now() - Duration::from_millis(late_ms);
        let p = Timeline {
            position_ms: t.position_at(at),
            at,
            playing: false,
        };
        self.send(Cmd::Sync(Some(p.plan())));
        p
    }

    /// The last error the player reported at or before `at`.
    /// How far off the player says it is, as it says so after `after` (waiting for that up to a
    /// few seconds: a busy computer makes reports late, never the music wrong).
    fn error_after(&self, after: Instant) -> Option<f32> {
        let t0 = Instant::now();
        loop {
            let e = self
                .errors
                .lock()
                .unwrap()
                .iter()
                .filter(|(t, _)| *t >= after)
                .map(|(_, e)| *e)
                .next_back();
            if e.is_some() || t0.elapsed() > Duration::from_secs(5) {
                return e;
            }
            sleep(20);
        }
    }

    fn error_at(&self, at: Instant) -> Option<f32> {
        self.errors
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| *t <= at)
            .map(|(_, e)| *e)
            .next_back()
    }

    fn finish(self) -> Vec<i32> {
        drop(self.player);
        std::thread::sleep(Duration::from_millis(50));
        heard(&std::mem::take(&mut *self.captured.lock().unwrap()))
    }
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// Play pressed on a paused room, as the follower sees it: the room holds the paused song again
/// (a new moment, the same place), the song is made ready there, the follower is told the held
/// timeline once more as it says ready, and the room starts it for everyone. `held` is the
/// place the room holds while it starts (where it paused).
fn resume(r: &Run, held: &Timeline, from_ms: u64) -> Timeline {
    let again = Timeline {
        at: Instant::now(),
        ..*held
    };
    r.send(Cmd::Sync(Some(again.plan())));
    r.send(Cmd::Sync(Some(ready_at(from_ms))));
    sleep(300);
    r.send(Cmd::Sync(Some(again.plan())));
    r.start(from_ms, 500)
}

/// Between two stretches heard: at most the music before fading out where it was (a pause, a
/// hold), silence, then at most the music fading in to the stretch that follows (a start partway
/// into a song rises from silence over a moment): each frame the true one scaled, softer each
/// time on the way out and louder each time on the way in. Nothing else: no music moved.
fn silence_then_fade_in(gap: &[i32], last: i32, next: i32) -> bool {
    let scaled = |x: i32, own: i32| f64::from(x) / f64::from(own);
    let out = gap.iter().position(|&x| x == 0).unwrap_or(gap.len());
    let fading_out = gap[..out]
        .iter()
        .enumerate()
        .map(|(j, &x)| scaled(x, last + 1 + j as i32))
        .try_fold(1.0 + 1e-3, |was, g| {
            (g <= was + 1e-3 && g >= -1e-3).then_some(g)
        })
        .is_some();
    let rest = &gap[out..];
    let k = rest.iter().position(|&x| x != 0).unwrap_or(rest.len());
    let tail = &rest[k..];
    let n = tail.len() as i32;
    out <= 2_000
        && fading_out
        && tail.len() <= 1_200
        && tail
            .iter()
            .enumerate()
            .map(|(j, &x)| scaled(x, next - (n - j as i32)))
            .try_fold(0.0, |was, g| {
                (g >= was - 1e-3 && g <= 1.0 + 1e-3).then_some(g)
            })
            .is_some()
}

/// Nothing but silence between the stretches heard (and a fade in from it): no music faded out
/// and moved after a start.
fn only_silence_between(h: &[i32]) -> bool {
    let mut at = 0;
    let mut gaps_clean = true;
    let mut i = 0;
    while i < h.len() {
        let mut j = i + 1;
        while j < h.len() && h[i] != 0 && h[j] == h[j - 1] + 1 {
            j += 1;
        }
        if h[i] != 0 && j - i >= 200 {
            if at > 0 {
                gaps_clean &= silence_then_fade_in(&h[at..i], h[at - 1], h[i]);
            }
            at = j;
        }
        i = j;
    }
    gaps_clean
}

#[test]
fn a_skip_starts_the_next_song_from_its_start_not_where_the_last_was_paused() {
    let _turn = engines::real_time();
    let a = counting("skip-a", 10, 0);
    let b = counting("skip-b", 10, B);
    let r = Run::new();
    sleep(100);
    r.load(&a, 0);
    sleep(300);
    let ta = r.start(0, 300);
    sleep(3_000);
    // Skip: the room holds where it is (the follower gets the paused timeline first), then the
    // next song is made ready from its start, then started.
    let pa = r.pause(&ta, 60);
    r.load(&b, 0);
    sleep(400);
    r.start(0, 500);
    sleep(2_500);
    let late = r.error_at(Instant::now());
    let h = r.finish();
    let first_b = h.iter().position(|&x| x > B).expect("song B played");
    let heard_b = runs(&h[first_b..]);
    assert_eq!(
        heard_b.len(),
        1,
        "song B straight through, not moved to where A paused ({} ms): {heard_b:?}",
        pa.position_ms
    );
    assert_eq!(heard_b[0].0, B + 1, "song B from its first frame");
    let e = late.expect("an error reported");
    assert!(e.abs() < 40.0, "in step with the room: {e} ms off");
}

#[test]
fn a_resume_plays_on_from_the_pause_without_a_jump() {
    let _turn = engines::real_time();
    let a = counting("resume", 12, 0);
    let r = Run::new();
    sleep(100);
    r.load(&a, 0);
    sleep(300);
    let t = r.start(0, 300);
    sleep(2_500);
    let p = r.pause(&t, 60);
    sleep(1_000);
    resume(&r, &p, p.position_ms);
    sleep(2_500);
    let late = r.error_at(Instant::now());
    let h = r.finish();
    let heard = runs(&h);
    // Up to the pause, then on from where the room paused, once: nothing played twice.
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert!(
        only_silence_between(&h),
        "the resume starts from silence, not from a moment faded out and moved"
    );
    let from = (p.position_ms * u64::from(RATE) / 1000) as i32;
    let off = ms(heard[1].0 - from);
    assert!(off.abs() < 30.0, "resumed {off:.0} ms from the pause");
    assert!(
        heard[1].1 > RATE as usize * 2,
        "and on from there: {heard:?}"
    );
    let e = late.expect("an error reported");
    assert!(e.abs() < 40.0, "in step with the room: {e} ms off");
}

/// Play on a paused room, then a seek to 30 s while it gets ready: the room still holds the
/// song where it paused, and the follower, once ready, is told that held place again before the
/// start. The song starts from 30 s, straight on, in step.
#[test]
fn a_seek_while_a_resume_gets_ready_starts_there_and_stays_there() {
    let _turn = engines::real_time();
    let a = counting("seek-resume", 40, 0);
    let r = Run::new();
    sleep(100);
    r.load(&a, 0);
    sleep(300);
    let t = r.start(0, 300);
    sleep(2_500);
    let p = r.pause(&t, 60);
    sleep(800);
    // Play: held, made ready where it paused...
    let held = Timeline {
        at: Instant::now(),
        ..p
    };
    r.send(Cmd::Sync(Some(held.plan())));
    r.send(Cmd::Sync(Some(ready_at(p.position_ms))));
    sleep(150);
    // ...and a seek to 30 s before it starts: held again, made ready at 30 s, the held place told
    // once more as it says ready, then the start from 30 s.
    let tc = resume(&r, &held, 30_000);
    sleep(4_500);
    let late = r.error_at(Instant::now());
    let h = r.finish();
    let heard = runs(&h);
    assert_eq!(heard.len(), 2, "up to the pause, then from 30 s: {heard:?}");
    assert!(
        only_silence_between(&h),
        "the start is from silence, not faded out and moved back to the pause"
    );
    assert!(
        ms(heard[1].0 - 30 * RATE as i32).abs() < 30.0,
        "from 30 s: {heard:?}"
    );
    assert!(heard[1].1 > RATE as usize * 4, "and on from there");
    let e = late.expect("an error reported");
    assert!(e.abs() < 40.0, "in step with the room: {e} ms off");
    let _ = tc;
}

/// The run the room test went through, end to end: a song, a skip, a skip with a seek while the
/// next one gets ready (the follower opening the held song again for the room's re-held
/// timeline, then the next one where the seek put it), a pause and a resume. In step all along.
#[test]
fn in_step_after_skips_a_seek_while_starting_a_pause_and_a_resume() {
    let _turn = engines::real_time();
    let a = counting("all-a", 8, 0);
    let b = counting("all-b", 8, B);
    let c = counting("all-c", 40, C);
    let r = Run::new();
    sleep(100);
    r.load(&a, 0);
    sleep(300);
    let ta = r.start(0, 300);
    sleep(2_500);
    // Next: B.
    r.pause(&ta, 60);
    r.load(&b, 0);
    sleep(400);
    let tb = r.start(0, 500);
    sleep(2_500);
    // Next: C, and while it gets ready, a seek to 30 s.
    let pb = r.pause(&tb, 60);
    r.load(&c, 0);
    sleep(200);
    // The room holds B again (a new moment): not in the player any more, so the follower opens
    // it there, paused; then C where the seek put it.
    let pb2 = Timeline {
        at: Instant::now(),
        ..pb
    };
    r.load(&b, pb2.position_ms);
    r.send(Cmd::Sync(Some(pb2.plan())));
    r.load(&c, 30_000);
    sleep(400);
    let tc = r.start(30_000, 500);
    sleep(3_000);
    let in_c = r.error_at(Instant::now());
    // Pause, and Play again.
    let pc = r.pause(&tc, 60);
    sleep(1_000);
    resume(&r, &pc, pc.position_ms);
    sleep(3_000);
    let after = r.error_at(Instant::now());
    let h = r.finish();
    let first_c = h.iter().position(|&x| x < 0).expect("song C played");
    let heard_c = runs(&h[first_c..]);
    assert_eq!(
        heard_c.len(),
        2,
        "C from 30 s, then on from its pause: {heard_c:?}"
    );
    assert!(only_silence_between(&h[first_c..]));
    assert!(
        ms(heard_c[0].0 - C - 30 * RATE as i32).abs() < 30.0,
        "C from 30 s, not from where B paused ({} ms): {heard_c:?}",
        pb.position_ms
    );
    let from = (pc.position_ms * u64::from(RATE) / 1000) as i32 + C;
    assert!(
        ms(heard_c[1].0 - from).abs() < 30.0,
        "resumed where it paused"
    );
    for e in [in_c, after] {
        let e = e.expect("an error reported");
        assert!(e.abs() < 40.0, "in step with the room: {e} ms off");
    }
}

/// Another output chosen in the middle of a song (headphones plugged in, Bluetooth, a DAC): the
/// new device takes a moment to open while the room plays on. The player lines up again from
/// silence at once, instead of coming back behind and waiting for the drift rule.
#[test]
fn another_output_mid_song_lines_up_with_the_room_at_once() {
    let _turn = engines::real_time();
    let a = counting("switch-a", 30, 0);
    let r = Run::new();
    sleep(100);
    r.load(&a, 0);
    sleep(300);
    let _t = r.start(0, 300);
    sleep(1_500);
    let next = Arc::new(Mutex::new(Captured {
        open_delay: Some(Duration::from_millis(400)),
        ..Captured::default()
    }));
    r.send(Cmd::Output(OutputChoice::PacedMemory(next.clone())));
    sleep(1_500);
    // Settled on the new output: what it says from here on.
    let e = r.error_after(Instant::now()).expect("an error reported");
    assert!(e.abs() < 40.0, "in step after the switch: {e} ms off");
    // On the new device: one stretch of music, from where the room was (no jump after it).
    drop(r);
    sleep(50);
    let h = heard(&std::mem::take(&mut *next.lock().unwrap()));
    let stretches = runs(&h);
    assert_eq!(
        stretches.len(),
        1,
        "one stretch on the new output: {stretches:?}"
    );
}
