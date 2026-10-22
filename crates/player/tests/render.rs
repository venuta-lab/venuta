//! The renderer's gates (WP3, sections 4.7 and 7.1 of the Listen together design): every row
//! of the splice table clean by the detectors; gapless joins sample-exact; a start on its exact
//! device frame; a song at the device's rate untouched (bit-perfect) at gain 1; a stream that
//! runs dry fades out over frames it holds (the guard) and back in when refilled.
//!
//! Everything runs on the bench in `render_kit` (no threads, no clock), so the runs are the
//! same however busy the machine is.

mod render_kit;

#[path = "../examples/live_kit/deadline.rs"]
#[allow(dead_code)]
mod deadline;

use player::dsp::{Chain, Gain as Replay, Sound};
use player::output::{DeviceFormat, SampleKind, encode};
use player::render::{Deadline, RenderState, VoiceState};
use player::voice_api::{FadeKind, Refusal, RenderCmd, RenderNote, Returned, Splice, VoiceId};
use render_kit::*;

const R: u32 = 48_000;
const P: u32 = 480;

fn s(secs: f64) -> u64 {
    (secs * f64::from(R)) as u64
}

/// Frames of the fades, at 48 kHz.
const F15: u64 = 720;
const F30: u64 = 1_440;

fn started(rig: &Rig, v: VoiceId) -> (u64, u64, u32) {
    rig.notes
        .iter()
        .find_map(|n| match *n {
            RenderNote::Started {
                voice,
                at,
                content,
                fade,
            } if voice == v => Some((at, content, fade)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{v:?} never started: {:?}", rig.notes))
}

// --- A song at the device's rate passes untouched -------------------------------------------

#[test]
fn a_song_at_the_devices_rate_plays_bit_for_bit() {
    let song = Song::music(R, 2, 0, s(20.0) + 123);
    let mut rig = Rig::new(R, 2, P);
    let v = rig.start(&song, 0, 0, 0);
    rig.run(song.frames + s(0.5));
    for c in 0..2 {
        let heard = rig.channel(c);
        assert_eq!(
            heard[..song.frames as usize],
            song.channel(c, 0, song.frames)[..],
            "channel {c}"
        );
        assert!(
            heard[song.frames as usize..].iter().all(|&v| v == 0),
            "silence after its end"
        );
    }
    assert_eq!(started(&rig, v), (0, 0, 0));
    let shaped = rig.notes_of(|n| {
        matches!(
            n,
            RenderNote::Faded { .. } | RenderNote::Resampling { .. } | RenderNote::Gain { .. }
        )
    });
    assert!(shaped.is_empty(), "nothing shaped: {shaped:?}");
    assert!(
        rig.notes.contains(&RenderNote::Ended {
            voice: v,
            at: song.frames
        }),
        "{:?}",
        rig.notes
    );
}

#[test]
fn every_device_format_gets_the_bytes_the_encoder_gives() {
    let formats = [
        (16, 16, SampleKind::Int),
        (24, 24, SampleKind::Int),
        (32, 24, SampleKind::Int),
        (32, 32, SampleKind::Int),
        (32, 32, SampleKind::Float),
    ];
    for (container_bits, valid_bits, kind) in formats {
        let fmt = DeviceFormat {
            rate: R,
            channels: 2,
            container_bits,
            valid_bits,
            kind,
        };
        let song = Song::music(R, 2, 0, s(1.0));
        let mut rig = Rig::with_format(fmt.clone(), P);
        rig.keep_bytes = true;
        rig.start(&song, 0, 0, 0);
        rig.run(song.frames);
        let src: Vec<i32> = (0..song.frames)
            .flat_map(|i| [(song.sample)(i, 0), (song.sample)(i, 1)])
            .collect();
        let mut want = Vec::new();
        encode(&src, &fmt, 1.0, &mut want);
        assert!(
            rig.bytes[..want.len()] == want[..],
            "{container_bits}/{valid_bits} {kind:?}"
        );
    }
}

#[test]
fn a_mono_song_on_a_stereo_device_doubles_and_stays_exact() {
    let song = Song::music(R, 1, 0, s(2.0));
    let mut rig = Rig::new(R, 2, P);
    rig.start(&song, 0, 0, 0);
    rig.run(song.frames);
    let want = song.channel(0, 0, song.frames);
    assert_eq!(rig.channel(0), want);
    assert_eq!(rig.channel(1), want);
}

#[test]
fn after_a_volume_change_and_back_the_music_is_exact_again() {
    let song = Song::music(R, 2, 0, s(6.0));
    let mut rig = Rig::new(R, 2, P);
    rig.start(&song, 0, 0, 0);
    rig.run(s(1.0));
    rig.send(RenderCmd::Gain { target: 0.5 });
    rig.run(s(2.0));
    rig.send(RenderCmd::Gain { target: 1.0 });
    rig.run(s(3.0));
    let ramps: Vec<(u64, u64, f32)> = rig
        .notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Gain { from, to, target } => Some((from, to, target)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ramps,
        vec![(s(1.0), s(1.0) + 960, 0.5), (s(3.0), s(3.0) + 960, 1.0)],
        "20 ms ramps from the frame each was asked at"
    );
    let src = song.channel(0, 0, song.frames);
    let heard = rig.channel(0);
    for i in 0..song.frames as usize {
        let f = i as u64;
        if (s(1.0)..s(1.0) + 960).contains(&f) || (s(3.0)..s(3.0) + 960).contains(&f) {
            continue;
        }
        let want = if (s(1.0) + 960..s(3.0)).contains(&f) {
            (f64::from(src[i]) * 0.5).round() as i32
        } else {
            src[i]
        };
        assert_eq!(heard[i], want, "frame {i}");
    }
    // The ramp is a raised cosine from the gain it was at.
    for k in 0..960u64 {
        let g = 1.0 - 0.5 * raised(k as f64 / 960.0);
        let i = (s(1.0) + k) as usize;
        assert_eq!(
            heard[i],
            (f64::from(src[i]) * g).round() as i32,
            "ramp frame {k}"
        );
    }
}

// --- A start on its exact device frame -------------------------------------------------------

#[test]
fn a_start_lands_on_its_exact_device_frame() {
    for at in [0u64, 1, 479, 480, 12_345, 48_007] {
        let song = Song::music(R, 2, 0, s(1.0));
        let mut rig = Rig::new(R, 2, P);
        let v = rig.start(&song, 0, at, 0);
        rig.run(at + song.frames + u64::from(P));
        let heard = rig.channel(0);
        let at_ = at as usize;
        assert!(heard[..at_].iter().all(|&x| x == 0), "{at}: silence before");
        assert_eq!(
            heard[at_..at_ + song.frames as usize],
            song.channel(0, 0, song.frames)[..],
            "{at}: from its first frame, on frame {at}"
        );
        assert_eq!(started(&rig, v), (at, 0, 0));
        let m = rig.markers.iter().find(|m| m.voice == v).unwrap();
        assert_eq!((m.frame, m.content), (at, 0.0), "{at}: the first marker");
    }
}

#[test]
fn a_start_partway_fades_in_from_its_frame() {
    // Asked for 15 ms; asked for none (partway, so 15 ms anyway); a join's 300 ms.
    for (asked, len) in [(F15, F15), (0, F15), (s(0.3), s(0.3))] {
        let song = Song::music(R, 2, 0, s(5.0));
        let (at, content) = (10_007u64, s(2.0));
        let mut rig = Rig::new(R, 2, P);
        let v = rig.start(&song, content, at, asked);
        rig.run(at + s(2.0));
        assert_eq!(started(&rig, v), (at, content, len as u32));
        assert!(rig.notes.contains(&RenderNote::Faded {
            voice: v,
            from: at,
            to: at + len,
            kind: FadeKind::In
        }));
        let heard = rig.channel(1);
        let src = song.channel(1, 0, song.frames);
        for k in 0..s(1.5) {
            let i = (at + k) as usize;
            let c = (content + k) as usize;
            let want = if k < len {
                (f64::from(src[c]) * raised(k as f64 / len as f64)).round() as i32
            } else {
                src[c]
            };
            assert_eq!(heard[i], want, "asked {asked}: frame {k} of the fade-in");
        }
    }
}

#[test]
fn a_late_start_keeps_the_place_it_was_given() {
    let song = Song::music(R, 2, 0, s(5.0));
    let mut rig = Rig::new(R, 2, P);
    rig.run(4_800);
    // Meant for frame 2,400, which has passed: it starts at 4,800, 2,400 frames further in.
    let v = rig.start(&song, s(1.0), 2_400, 0);
    rig.run(s(1.0));
    assert_eq!(started(&rig, v), (4_800, s(1.0) + 2_400, F15 as u32));
    assert!(rig.notes.contains(&RenderNote::Late {
        voice: v,
        at: 4_800,
        by: 2_400
    }));
    let src = song.channel(0, 0, song.frames);
    let heard = rig.channel(0);
    for k in F15..s(0.5) {
        assert_eq!(
            heard[(4_800 + k) as usize],
            src[(s(1.0) + 2_400 + k) as usize]
        );
    }
}

// --- Gapless ---------------------------------------------------------------------------------

#[test]
fn songs_join_gapless_sample_exact() {
    // Two songs cut one after the other from one piece, the seam mid-period.
    let n = 100_003u64;
    let a = Song::music(R, 2, 0, n);
    let b = Song::music(R, 2, n, s(3.0));
    let whole = Song::music(R, 2, 0, n + b.frames);
    let mut rig = Rig::new(R, 2, P);
    let va = rig.start(&a, 0, 0, 0);
    let (vb, tap) = rig.voice(&b, 0);
    rig.send(RenderCmd::SwitchAt {
        from: va,
        to: vb,
        tap,
        at: n,
        to_content: 0,
        rate: R,
        how: Splice::Gapless,
    });
    rig.run(n + b.frames);
    for c in 0..2 {
        assert_eq!(
            rig.channel(c)[..whole.frames as usize],
            whole.channel(c, 0, whole.frames)[..],
            "channel {c}: one seamless piece"
        );
    }
    assert!(rig.notes.contains(&RenderNote::Joined {
        from: va,
        to: vb,
        at: n
    }));
    let m = rig.markers.iter().find(|m| m.voice == vb).unwrap();
    assert_eq!((m.frame, m.content), (n, 0.0));
}

#[test]
fn a_gapless_join_through_the_rate_converter_is_one_signal() {
    // CD-rate songs on a 48 kHz device (shared mode, our converter): the join is the same as
    // one long song converted, bit for bit.
    let cd = 44_100;
    let n = 97_001u64;
    let a = Song::music(cd, 2, 0, n);
    let b = Song::music(cd, 2, n, 88_200);
    let whole = Song::music(cd, 2, 0, n + 88_200);
    let mut joined = Rig::new(R, 2, P);
    let va = joined.start(&a, 0, 0, 0);
    let (vb, tap) = joined.voice(&b, 0);
    joined.send(RenderCmd::SwitchAt {
        from: va,
        to: vb,
        tap,
        at: 0,
        to_content: 0,
        rate: cd,
        how: Splice::Gapless,
    });
    let mut one = Rig::new(R, 2, P);
    one.start(&whole, 0, 0, 0);
    joined.run(s(3.8));
    one.run(s(3.8));
    assert!(joined.heard == one.heard, "the join is one signal");
    assert!(
        joined
            .notes
            .iter()
            .any(|x| matches!(x, RenderNote::Joined { from, to, .. } if *from == va && *to == vb))
    );
}

#[test]
fn a_join_between_rates_starts_the_next_song_on_the_frame_after() {
    let a = Song::counting(R, 2, 50_000);
    let b = Song::music(96_000, 2, 0, 96_000);
    let mut rig = Rig::new(R, 2, P);
    let va = rig.start(&a, 0, 0, 0);
    let (vb, tap) = rig.voice(&b, 0);
    rig.send(RenderCmd::SwitchAt {
        from: va,
        to: vb,
        tap,
        at: 50_000,
        to_content: 0,
        rate: 96_000,
        how: Splice::Gapless,
    });
    rig.run(s(2.0));
    let heard = rig.channel(0);
    assert_eq!(heard[49_999], 50_000 << 8, "the first song's last frame");
    assert!(rig.notes.contains(&RenderNote::Joined {
        from: va,
        to: vb,
        at: 50_000
    }));
    assert_eq!(
        started(&rig, vb),
        (50_000, 0, 0),
        "on the frame after, no fade"
    );
}

// --- The splice table, every row, by the detectors --------------------------------------------

/// What a row is played on.
#[derive(Clone, Copy, Debug)]
enum Signal {
    Music,
    Sine(f64),
}

impl Signal {
    /// A song cut from the endless source every song of a row comes from.
    fn piece(self, from: u64, frames: u64) -> Song {
        match self {
            Signal::Music => Song::music(R, 2, from, frames),
            Signal::Sine(hz) => Song::sine(hz, R, 2, from, frames),
        }
    }
}

/// The 50 ms a song rises over, from `at`: the song's own start (a song boundary), so declared.
fn rise(at: u64) -> std::ops::Range<u64> {
    at..at + ms(R, 50)
}

/// What a row's script says about its run.
struct Ran {
    /// It sums two places of music (a crossfade), where the music-shaped check (whose slope
    /// limit is one piece's own) does not apply.
    sums: bool,
    /// Spans to declare beyond the notes': a song's own rise from silence.
    extra: Vec<std::ops::Range<u64>>,
}

/// Plays a row's script on each signal and judges every channel by the detectors that apply.
fn row(name: &str, script: impl Fn(Signal, &mut Rig) -> Ran) {
    for signal in [Signal::Music, Signal::Sine(1_000.0), Signal::Sine(15_000.0)] {
        let mut rig = Rig::new(R, 2, P);
        let ran = script(signal, &mut rig);
        rig.run(u64::from(P) * 10);
        let declared = rig.declared(&ran.extra);
        let reference = signal.piece(0, s(60.0));
        for c in 0..2 {
            let heard = rig.channel(c);
            let r = reference.channel(c, 0, reference.frames);
            let mut verdicts = match signal {
                Signal::Music => judge_music(&heard, &r, R, &declared),
                Signal::Sine(hz) => judge_sine(&heard, &r, hz, R, &declared),
            };
            if ran.sums {
                verdicts.retain(|v| v.detector != "music-shaped");
            }
            assert_clean(&format!("{name}, {signal:?}, channel {c}"), &verdicts);
        }
        assert!(
            rig.heard.iter().any(|&v| v != 0),
            "{name}, {signal:?}: something played"
        );
    }
}

fn plain() -> Ran {
    Ran {
        sums: false,
        extra: Vec::new(),
    }
}

#[test]
fn row_pause_or_stop_fades_out_over_30_ms_then_silence() {
    row("pause", |sig, rig| {
        let v = rig.start(&sig.piece(0, s(30.0)), s(1.0), 0, F15);
        rig.run(s(2.0));
        let at = s(2.0) + 123;
        rig.send(RenderCmd::StopAt {
            voice: v,
            at,
            fade: F30 as u32,
        });
        rig.run(s(1.0));
        assert!(rig.notes.contains(&RenderNote::Faded {
            voice: v,
            from: at,
            to: at + F30,
            kind: FadeKind::Out
        }));
        assert!(rig.notes.contains(&RenderNote::Silent {
            voice: v,
            at: at + F30
        }));
        assert!(
            rig.channel(0)[(at + F30) as usize..]
                .iter()
                .all(|&x| x == 0)
        );
        plain()
    });
}

#[test]
fn row_leaving_fades_out_over_a_second() {
    row("leave", |sig, rig| {
        let v = rig.start(&sig.piece(0, s(30.0)), s(3.0), 0, s(0.3));
        rig.run(s(2.0));
        rig.send(RenderCmd::StopAt {
            voice: v,
            at: s(2.0),
            fade: R,
        });
        rig.run(s(1.5));
        assert!(rig.notes.contains(&RenderNote::Silent {
            voice: v,
            at: s(3.0)
        }));
        plain()
    });
}

#[test]
fn row_a_stop_asked_without_a_fade_still_fades() {
    row("stop, no fade asked", |sig, rig| {
        let v = rig.start(&sig.piece(0, s(30.0)), s(2.0), 0, F15);
        rig.run(s(1.0));
        rig.send(RenderCmd::StopAt {
            voice: v,
            at: s(1.0) + 7,
            fade: 0,
        });
        rig.run(s(0.5));
        assert!(rig.notes.contains(&RenderNote::Silent {
            voice: v,
            at: s(1.0) + 7 + F30
        }));
        plain()
    });
}

#[test]
fn row_start_or_resume_partway_fades_in_over_15_ms() {
    row("resume", |sig, rig| {
        rig.start(&sig.piece(0, s(30.0)), s(4.0), 1_003, F15);
        rig.run(s(2.0));
        plain()
    });
}

#[test]
fn row_join_partway_fades_in_over_300_ms() {
    row("join", |sig, rig| {
        rig.start(&sig.piece(0, s(30.0)), s(9.5), 2_222, s(0.3));
        rig.run(s(2.0));
        plain()
    });
}

#[test]
fn row_start_at_the_songs_first_frame_has_no_fade() {
    row("first frame", |sig, rig| {
        let v = rig.start(&sig.piece(0, s(30.0)).rising(), 0, 777, 0);
        rig.run(s(2.0));
        assert_eq!(started(rig, v), (777, 0, 0));
        Ran {
            sums: false,
            extra: vec![rise(777)],
        }
    });
}

/// The switch frame of `switch`.
const SWITCH: u64 = 96_061;

/// A song of the piece from its start (rising) at frame 0; at `SWITCH` it moves to
/// `to_content` of a song cut from the piece at `to_from` (rising too: its start is a song's),
/// shaped as `how`.
fn switch(
    sig: Signal,
    rig: &mut Rig,
    to_from: u64,
    to_content: u64,
    how: Splice,
) -> (VoiceId, VoiceId, Vec<std::ops::Range<u64>>) {
    let a = rig.start(&sig.piece(0, s(30.0)).rising(), 0, 0, 0);
    rig.run(s(1.0));
    let song = sig.piece(to_from, s(20.0)).rising();
    let (b, tap) = rig.voice(&song, to_content);
    rig.send(RenderCmd::SwitchAt {
        from: a,
        to: b,
        tap,
        at: SWITCH,
        to_content,
        rate: R,
        how,
    });
    rig.run(s(3.0));
    let mut extra = vec![rise(0)];
    if to_content == 0 {
        extra.push(rise(SWITCH));
    }
    (a, b, extra)
}

#[test]
fn row_seek_or_realign_crossfades_over_40_ms() {
    row("seek", |sig, rig| {
        // The same song, 7.3 s on.
        let (a, b, extra) = switch(
            sig,
            rig,
            0,
            s(7.3) + 11,
            Splice::Crossfade { frames: 1_920 },
        );
        assert!(rig.notes.contains(&RenderNote::Faded {
            voice: a,
            from: SWITCH,
            to: SWITCH + 1_920,
            kind: FadeKind::CrossOut
        }));
        assert!(rig.notes.contains(&RenderNote::Faded {
            voice: b,
            from: SWITCH,
            to: SWITCH + 1_920,
            kind: FadeKind::CrossIn
        }));
        Ran { sums: true, extra }
    });
}

#[test]
fn row_skip_fades_the_old_out_ending_at_the_switch_and_starts_the_new_there() {
    row("skip", |sig, rig| {
        let (a, b, extra) = switch(
            sig,
            rig,
            s(20.0),
            0,
            Splice::FadeOutThenIn {
                out_frames: F30 as u32,
            },
        );
        assert!(rig.notes.contains(&RenderNote::Faded {
            voice: a,
            from: SWITCH - F30,
            to: SWITCH,
            kind: FadeKind::Out
        }));
        assert_eq!(
            started(rig, b),
            (SWITCH, 0, 0),
            "another song from its start: no fade"
        );
        Ran { sums: false, extra }
    });
}

#[test]
fn row_a_longer_copy_fades_out_over_150_ms_ending_at_the_boundary() {
    row("longer copy", |sig, rig| {
        let (a, _, extra) = switch(sig, rig, s(20.0), 0, Splice::EndFade { frames: 7_200 });
        assert!(rig.notes.contains(&RenderNote::Faded {
            voice: a,
            from: SWITCH - 7_200,
            to: SWITCH,
            kind: FadeKind::Out
        }));
        Ran { sums: false, extra }
    });
}

#[test]
fn row_a_boundary_crossfade_is_equal_power_over_its_length() {
    row("crossfade", |sig, rig| {
        let (_, _, extra) = switch(
            sig,
            rig,
            s(20.0),
            0,
            Splice::Crossfade {
                frames: s(4.0) as u32,
            },
        );
        rig.run(s(2.0));
        Ran { sums: true, extra }
    });
}

#[test]
fn row_volume_ramps_over_20_ms() {
    row("volume", |sig, rig| {
        rig.start(&sig.piece(0, s(30.0)).rising(), 0, 0, 0);
        rig.run(s(1.0));
        rig.send(RenderCmd::Gain { target: 0.25 });
        rig.run(s(1.0));
        rig.send(RenderCmd::Gain { target: 1.0 });
        rig.run(s(1.0));
        Ran {
            sums: false,
            extra: vec![rise(0)],
        }
    });
}

#[test]
fn row_a_stream_running_dry_fades_out_and_comes_back_faded_in() {
    row("running dry", |sig, rig| {
        let v = rig.start(&sig.piece(0, s(30.0)), s(1.0), 0, F15);
        // Decoded only to 2.5 s of the song.
        rig.feed(v).limit = s(2.5);
        rig.run(s(3.0));
        assert!(
            rig.notes
                .iter()
                .any(|n| matches!(n, RenderNote::Starved { voice, .. } if *voice == v))
        );
        rig.feed(v).limit = u64::MAX;
        rig.run(s(3.0));
        assert!(
            rig.notes
                .iter()
                .any(|n| matches!(n, RenderNote::Resumed { voice, .. } if *voice == v))
        );
        plain()
    });
}

// --- The guard ---------------------------------------------------------------------------------

/// The starvation fade, frame by frame, for a song decoded only to `limit`, played in periods of
/// `period`: it begins where the guard says, it is applied to frames really decoded (never to
/// silence past what was in hand), and after it nothing sounds until the song is refilled; then
/// it comes back with a 15 ms fade-in at the place it faded at.
fn dry_run(period: u32, limit: u64) {
    let song = Song::music(R, 2, 0, s(30.0));
    let mut rig = Rig::new(R, 2, period);
    let v = rig.start(&song, 0, 0, 0);
    rig.feed(v).limit = limit;
    rig.run(limit + s(1.0));
    let at = rig
        .notes
        .iter()
        .find_map(|n| match *n {
            RenderNote::Starved { voice, at } if voice == v => Some(at),
            _ => None,
        })
        .unwrap_or_else(|| panic!("period {period}, limit {limit}: no starvation"));
    let what = format!("period {period}, limit {limit}");
    assert!(rig.notes.contains(&RenderNote::Faded {
        voice: v,
        from: at,
        to: at + F30,
        kind: FadeKind::Dry
    }));
    // In hand when it began: the fade, the filter's reach, and a period more (the guard).
    assert!(
        limit - at >= F30 + 33 + u64::from(period),
        "{what}: began at {at} with {} in hand",
        limit - at
    );
    // And not much earlier than the guard needs (one period of slack, plus a block).
    assert!(
        limit - at <= F30 + 33 + 3 * u64::from(period) + 1_024,
        "{what}: began at {at}, {} early",
        limit - at
    );
    let src = song.channel(0, 0, song.frames);
    let heard = rig.channel(0);
    for k in 0..F30 {
        let i = (at + k) as usize;
        let want = (f64::from(src[i]) * (1.0 - raised(k as f64 / F30 as f64))).round() as i32;
        assert_eq!(
            heard[i], want,
            "{what}: frame {k} of the fade is the music, faded"
        );
    }
    assert!(
        heard[(at + F30) as usize..].iter().all(|&x| x == 0),
        "{what}: silent after"
    );
    // Refilled: back from the place it faded at.
    rig.feed(v).limit = u64::MAX;
    let back = rig.at;
    rig.run(s(2.0));
    let (r, content) = rig
        .notes
        .iter()
        .find_map(|n| match *n {
            RenderNote::Resumed { voice, at, content } if voice == v => Some((at, content)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{what}: never back"));
    assert!(r >= back);
    assert_eq!(content, at + F30, "{what}: on from where it faded");
    let heard = rig.channel(0);
    for k in 0..s(1.0) {
        let i = (r + k) as usize;
        let c = (content + k) as usize;
        let want = if k < F15 {
            (f64::from(src[c]) * raised(k as f64 / F15 as f64)).round() as i32
        } else {
            src[c]
        };
        assert_eq!(heard[i], want, "{what}: frame {k} after coming back");
    }
}

#[test]
fn a_stream_running_dry_fades_out_over_frames_it_holds() {
    dry_run(P, s(2.0) + 1_000);
}

#[test]
fn the_guard_holds_at_any_period_and_any_place() {
    for period in [64, 441, 1_024, 4_096] {
        for limit in [s(1.0), s(1.0) + 1, s(1.3) + 517] {
            dry_run(period, limit);
        }
    }
}

#[test]
fn a_voice_that_is_not_ready_does_not_start_until_it_is() {
    // Nothing decoded yet at its frame: it starts late, at the place the mapping says, faded.
    let song = Song::music(R, 2, 0, s(10.0));
    let mut rig = Rig::new(R, 2, P);
    let (v, tap) = rig.voice(&song, 0);
    rig.feed(v).limit = 0;
    rig.send(RenderCmd::StartAt {
        voice: v,
        tap,
        at: 960,
        content: 0,
        rate: R,
        fade_in: 0,
    });
    rig.run(s(0.5));
    assert!(rig.heard.iter().all(|&x| x == 0));
    rig.feed(v).limit = u64::MAX;
    rig.run(s(1.0));
    let (at, content, fade) = started(&rig, v);
    assert_eq!(content, at - 960, "the place the mapping says");
    assert_eq!(fade, F15 as u32, "partway: faded in");
}

// --- Commands, slots and what comes back ------------------------------------------------------

#[test]
fn a_drop_of_a_sounding_voice_fades_it_first_and_then_returns_the_tap() {
    let song = Song::music(R, 2, 0, s(10.0));
    let mut rig = Rig::new(R, 2, P);
    let v = rig.start(&song, 0, 0, 0);
    rig.run(s(1.0));
    rig.send(RenderCmd::Drop { voice: v });
    rig.step();
    assert!(rig.returned.is_empty(), "not while it fades");
    rig.run(u64::from(P) * 4);
    assert!(matches!(rig.returned[..], [Returned::Tap(ref t)] if t.voice == v));
    assert!(rig.notes.contains(&RenderNote::Faded {
        voice: v,
        from: s(1.0),
        to: s(1.0) + F30,
        kind: FadeKind::Out
    }));
    assert!(
        rig.channel(0)[(s(1.0) + F30) as usize..]
            .iter()
            .all(|&x| x == 0)
    );
}

#[test]
fn a_start_without_a_free_slot_or_too_fast_a_source_is_refused() {
    let song = Song::music(R, 2, 0, s(10.0));
    let mut rig = Rig::new(R, 2, P);
    let mut voices = Vec::new();
    for _ in 0..4 {
        voices.push(rig.start(&song, 0, 0, 0));
    }
    let extra = rig.start(&song, 0, 0, 0);
    rig.step();
    assert!(rig.notes.contains(&RenderNote::Refused {
        voice: extra,
        why: Refusal::NoSlot
    }));
    assert!(
        rig.returned
            .iter()
            .any(|r| matches!(r, Returned::Tap(t) if t.voice == extra))
    );
    let fast = Song::music(500_000, 2, 0, 500_000);
    let mut rig = Rig::new(R, 2, P);
    let v = rig.start(&fast, 0, 0, 0);
    rig.step();
    assert!(rig.notes.contains(&RenderNote::Refused {
        voice: v,
        why: Refusal::Rate
    }));
    assert!(
        rig.returned
            .iter()
            .any(|r| matches!(r, Returned::Tap(t) if t.voice == v))
    );
    // Commands for nobody.
    rig.send(RenderCmd::StopAt {
        voice: VoiceId(99),
        at: 0,
        fade: 0,
    });
    rig.step();
    assert!(rig.notes.contains(&RenderNote::Refused {
        voice: VoiceId(99),
        why: Refusal::NoVoice
    }));
}

#[test]
fn a_new_chain_takes_over_and_the_old_one_comes_back() {
    let song = Song::music(R, 2, 0, s(4.0));
    let mut rig = Rig::new(R, 2, P);
    let v = rig.start(&song, 0, 0, 0);
    rig.run(s(1.0));
    let loud = Sound {
        preamp_db: -6.0,
        ..Sound::default()
    };
    rig.send(RenderCmd::Chain {
        voice: v,
        chain: Box::new(Chain::new(&loud, &Replay::default(), R, 2, 32)),
    });
    rig.run(s(1.0));
    rig.send(RenderCmd::Chain {
        voice: v,
        chain: Box::new(Chain::new(&Sound::default(), &Replay::default(), R, 2, 32)),
    });
    rig.run(s(1.0));
    // The first chain replaced nothing (a voice starts with none), the second gave the first
    // back.
    assert_eq!(
        rig.returned
            .iter()
            .filter(|r| matches!(r, Returned::Chain(_)))
            .count(),
        1
    );
    let src = song.channel(0, 0, song.frames);
    let heard = rig.channel(0);
    let i = (s(1.5)) as usize;
    let quieter = f64::from(heard[i]) / f64::from(src[i]);
    assert!((quieter - 0.501).abs() < 0.01, "-6 dB: {quieter}");
    // Declared while the tools change the music, exact again once they don't.
    assert!(rig.notes.contains(&RenderNote::Resampling {
        voice: v,
        at: s(1.0),
        on: true
    }));
    assert_eq!(heard[s(2.5) as usize], src[s(2.5) as usize]);
}

/// Sound tools changed while a song sounds glide over 50 ms (engine 2 makes every such chain
/// with `Chain::gliding`): clean by every detector, on music and on sines, the old chain kept
/// inside the new one until that one comes back. The same change made at once steps, and the
/// sine lock hears it (so a chain that didn't glide would be caught).
#[test]
fn a_change_of_the_sound_tools_glides_and_is_clean() {
    let quiet = || Sound {
        preamp_db: -6.0,
        limiter: false,
        ..Sound::default()
    };
    let chain = |sound: &Sound, glide: bool| {
        let c = Chain::new(sound, &Replay::default(), R, 2, 32);
        Box::new(if glide {
            c.gliding(ms(R, 50) as u32)
        } else {
            c
        })
    };
    row("sound tools", |sig, rig| {
        let v = rig.start(&sig.piece(0, s(30.0)), s(1.0), 0, F15);
        rig.run(s(1.0));
        rig.send(RenderCmd::Chain {
            voice: v,
            chain: chain(&quiet(), true),
        });
        rig.run(s(1.0));
        rig.send(RenderCmd::Chain {
            voice: v,
            chain: chain(&Sound::default(), true),
        });
        rig.run(s(1.0));
        // The first glided from no chain at all, the second keeps the first inside it: nothing
        // came back yet.
        assert!(
            !rig.returned
                .iter()
                .any(|r| matches!(r, Returned::Chain(_)))
        );
        plain()
    });
    let song = Song::sine(1_000.0, R, 2, 0, s(30.0));
    let mut rig = Rig::new(R, 2, P);
    let v = rig.start(&song, s(1.0), 0, F15);
    rig.run(s(1.0));
    rig.send(RenderCmd::Chain {
        voice: v,
        chain: chain(&quiet(), false),
    });
    rig.run(s(1.0));
    let declared = rig.declared(&[]);
    let heard = rig.channel(0);
    let r = song.channel(0, 0, song.frames);
    let flagged = judge_sine(&heard, &r, 1_000.0, R, &declared)
        .iter()
        .any(|v| !v.flags.is_empty());
    assert!(flagged, "a chain swapped at once steps, and the detectors hear it");
}

#[test]
fn a_newer_epoch_while_sounding_dips_out_and_back_at_the_new_place() {
    let song = Song::music(R, 2, 0, s(30.0));
    let mut rig = Rig::new(R, 2, P);
    let v = rig.start(&song, 0, 0, 0);
    rig.run(s(1.0));
    // The decode thread moved to 10 s (a new epoch) and stopped feeding the old place.
    rig.feed(v).seek_to(s(10.0));
    rig.run(s(2.0));
    let dipped = rig
        .notes
        .iter()
        .any(|n| matches!(n, RenderNote::Faded { voice, kind: FadeKind::Dry, .. } if *voice == v));
    let back = rig.notes.iter().find_map(|n| match *n {
        RenderNote::Resumed { voice, content, .. } if voice == v => Some(content),
        _ => None,
    });
    assert!(dipped, "{:?}", rig.notes);
    assert_eq!(rig.status.read().starved, 0, "a move, not running dry");
    assert_eq!(back, Some(s(10.0)), "on at the new place");
    let declared = rig.declared(&[]);
    let reference = Song::music(R, 2, 0, s(30.0)).channel(0, 0, s(30.0));
    assert_clean(
        "a new epoch",
        &judge_music(&rig.channel(0), &reference, R, &declared),
    );
}

// --- Status by atomics ---------------------------------------------------------------------------

#[test]
fn the_status_says_what_plays() {
    let song = Song::music(R, 2, 0, s(10.0));
    let mut rig = Rig::new(R, 2, P);
    let st = rig.status.read();
    assert_eq!(st.state, RenderState::Silent);
    let v = rig.start(&song, s(1.0), 0, F15);
    rig.feed(v).limit = s(4.0);
    rig.run(s(1.0));
    let st = rig.status.read();
    assert_eq!(st.frames, s(1.0));
    assert_eq!(st.state, RenderState::Playing);
    let one = st.voices.iter().flatten().find(|x| x.voice == v).unwrap();
    assert_eq!(one.state, VoiceState::Sounding);
    assert_eq!(one.content, (s(2.0)) as f64, "where it is in the song");
    assert_eq!(one.ratio, 1.0);
    rig.trim.set_ppm(250.0);
    rig.run(s(1.0));
    let st = rig.status.read();
    assert!(
        (st.ppm - 250.0).abs() < 1e-9,
        "the trim reached: {}",
        st.ppm
    );
    rig.run(s(1.5));
    let st = rig.status.read();
    assert_eq!(st.state, RenderState::Starved);
    assert_eq!(st.starved, 1);
    assert_eq!(st.deadline.periods(), rig.at / u64::from(P));
}

#[test]
fn the_deadline_line_is_the_one_the_hardware_kit_reads() {
    let mut d = Deadline::default();
    d.buckets[97] = 5_000;
    d.buckets[3] = 2;
    d.late = 1;
    let line = d.to_log_line(60, 4);
    let h = deadline::Histogram::from_log_line(&line).expect("WP14 reads it");
    assert_eq!(h.buckets, d.buckets);
    assert_eq!((h.late, h.ran_dry, h.seconds), (1, 4, 60));
    assert_eq!(
        line,
        h.to_log_line(),
        "the same line, character for character"
    );
    // A renderer's own: every call counted.
    let song = Song::music(R, 2, 0, s(2.0));
    let mut rig = Rig::new(R, 2, P);
    rig.start(&song, 0, 0, 0);
    rig.run(s(1.0));
    let d = rig.status.read().deadline;
    assert_eq!(d.periods(), 100);
    let earlier = d.clone();
    rig.run(s(1.0));
    assert_eq!(rig.status.read().deadline.since(&earlier).periods(), 100);
}

// --- Through the simulated output ----------------------------------------------------------------

#[test]
fn through_the_simulated_output_each_marker_says_what_the_dac_played() {
    use player::output_api::{OpenRequest, Output};
    use player::render::{RenderSetup, Renderer};
    use player::sim::{SimConfig, SimOutput};
    use player::voice_api::{BLOCK_FRAMES, RateTarget, render_link, voice_ring};
    use std::sync::Arc;
    use together::time::{SimClock, Timebase};

    const MS: u64 = 1_000_000;
    let clock = SimClock::new(1_000 * MS);
    let tb: Arc<dyn Timebase> = Arc::new(clock.clone());
    let (mut out, sim) = SimOutput::new(
        tb,
        player::sim::SimConfig {
            skew_ppm: 80.0,
            wake_jitter_us: 1_500,
            ..SimConfig::default()
        },
    );
    let info = out
        .open(&OpenRequest {
            rate: R,
            channels: 2,
            bits: 32,
        })
        .unwrap();
    let (link, ends) = render_link(64, 4_096, 4_096);
    let (r, status) = Renderer::new(RenderSetup {
        format: info.format.clone(),
        max_period: info.period_frames,
        ends,
        rate: Arc::new(RateTarget::default()),
        clock: Arc::new(clock.clone()),
    });
    // The whole song decoded ahead (its ring holds it), so nothing has to feed it meanwhile.
    let song = Song::music(R, 2, 0, s(3.0)).rising();
    let (feed, tap) = voice_ring(VoiceId(1), (song.frames as usize).div_ceil(BLOCK_FRAMES));
    fill_ring(&feed, VoiceId(1), &song);
    link.commands
        .try_send(RenderCmd::StartAt {
            voice: VoiceId(1),
            tap,
            at: 4_801,
            content: 0,
            rate: R,
            fade_in: 0,
        })
        .unwrap_or_else(|_| panic!("room for the command"));
    out.start(Box::new(r)).unwrap();
    clock.pass(4_000 * MS);
    let tape = sim.tape();
    let run = &tape.runs[0];
    let mut markers = Vec::new();
    while let Ok(m) = link.markers.try_recv() {
        markers.push(m);
    }
    assert!(markers.len() > 250, "a marker a period: {}", markers.len());
    for m in &markers {
        let tick = run.tick_of(m.frame) as usize;
        assert_eq!(
            run.samples[tick * 2],
            (song.sample)(m.content as u64, 0),
            "device frame {} carried content frame {}",
            m.frame,
            m.content
        );
        assert_eq!(m.ratio, 1.0);
    }
    assert_eq!(markers[0].frame, 4_801, "on its frame");
    // The DAC played the song whole, from the frame asked, at its own pace.
    let left = run.left_justified(0);
    let at = run.tick_of(4_801) as usize;
    assert!(left[..at].iter().all(|&v| v == 0));
    assert_eq!(
        left[at..at + song.frames as usize],
        song.channel(0, 0, song.frames)[..]
    );
    assert!(run.dry.is_empty(), "never ran dry");
    assert_eq!(status.read().state, player::render::RenderState::Silent);
    drop(feed);
}
