//! Engine 2's gates (the WP4 notes in 9.2 of the Listen together design), on a simulated clock
//! the test moves:
//!
//! - every solo action is clean by the detectors (section 7.1), with the spans the renderer's
//!   notes declared and each song's own rise and fall from silence;
//! - the app's callback runs on the status thread, never the render or control thread;
//! - a room takes the player and lets go, and your own queue comes back paused at its place
//!   and plays bit-identical to a player that never joined (section 7.6);
//! - a stream that runs dry fades out and back in, said as buffering.

mod engine_kit;
mod render_kit;

use audio_check::Declared;
use engine_kit::*;
use player::decode::{Download, Feeder, OpenInfo, Source, Streams};
use player::dsp::Sound;
use player::engine::{Cmd, Item};
use player::voice_api::RenderNote;
use render_kit::{assert_clean, declared_spans, judge_music};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use together::align::Alignment;
use together::anchor::{Anchor, EntryId, PrepId};
use together::clock_map::{self, ClockMap};
use together::room_player::{RoomPlayer, SyncMode};
use together::time::MonoClock;

/// The test music from 0 to 60 s, channel `c`: every song here is cut from it.
fn reference(c: u16) -> Vec<i32> {
    let s = render_kit::Song::music(R, 2, 0, u64::from(R) * 60);
    s.channel(c, 0, s.frames)
}

/// The spans the notes declare, and each song's own rise from silence (a start at its first
/// frame, not a join) and its fall at its end (not into a join).
fn declared(notes: &[RenderNote], end: u64) -> Declared {
    let joins: Vec<u64> = notes
        .iter()
        .filter_map(|n| match *n {
            RenderNote::Joined { at, .. } => Some(at),
            _ => None,
        })
        .collect();
    let mut spans = declared_spans(notes, end);
    for n in notes {
        match *n {
            RenderNote::Started {
                at,
                content: 0,
                fade: 0,
                ..
            } if !joins.contains(&at) => spans.push(at..at + ms(50)),
            RenderNote::Ended { at, .. } if !joins.contains(&at) => {
                spans.push(at.saturating_sub(ms(50))..at)
            }
            _ => {}
        }
    }
    Declared::new(
        spans
            .into_iter()
            .map(|r| r.start as usize..r.end as usize)
            .collect(),
    )
}

/// Every detector on both channels of what was played. `sums`: it crossfades two places of
/// music somewhere, where the music-shaped check (whose limit is one piece's own) doesn't apply.
fn judge(what: &str, heard: &[Vec<i32>; 2], notes: &[RenderNote], sums: bool) {
    let d = declared(notes, heard[0].len() as u64);
    for (c, h) in heard.iter().enumerate() {
        assert!(h.iter().any(|&v| v != 0), "{what}: something played");
        let mut v = judge_music(h, &reference(c as u16), R, &d);
        if sums {
            v.retain(|v| v.detector != "music-shaped");
        }
        assert_clean(&format!("{what}, channel {c}"), &v);
    }
}

/// The songs: A (0 to 10 s) and B (10 to 20 s) follow each other seamlessly; C is from 30 s.
struct Songs {
    a: PathBuf,
    b: PathBuf,
    c: PathBuf,
}

fn songs() -> Songs {
    Songs {
        a: song(0.0, 10.0, true, false),
        b: song(10.0, 10.0, false, true),
        c: song(30.0, 10.0, true, true),
    }
}

/// Runs a script on a fresh engine 2, then judges what it played, and checks every report came
/// from the status thread.
fn solo(what: &str, sums: bool, script: impl FnOnce(&mut Rig<PathBuf>)) -> Rig<PathBuf> {
    let mut rig = Rig::new();
    script(&mut rig);
    let heard = rig.finish();
    judge(what, &heard, &rig.heard_notes, sums);
    assert!(!rig.seen.is_empty());
    for (name, _) in &rig.seen {
        assert_eq!(
            name.as_deref(),
            Some("venuta-status"),
            "{what}: reported there"
        );
    }
    assert!(
        rig.seen.iter().all(|(_, s)| s.error.is_empty()),
        "{what}: no error"
    );
    rig
}

fn has(notes: &[RenderNote], f: impl Fn(&RenderNote) -> bool) -> bool {
    notes.iter().any(f)
}

#[test]
fn songs_join_gaplessly_and_the_queue_ends_clean() {
    let s = songs();
    let rig = solo("gapless", false, |r| {
        r.send(play(vec![s.a.clone(), s.b.clone()]));
        assert!(r.until(30_000, |s| s.index == Some(1)), "on to the next");
        assert!(r.until(30_000, |s| !s.playing), "to the end");
        r.pass(500);
    });
    assert!(has(&rig.heard_notes, |n| matches!(
        n,
        RenderNote::Joined { .. }
    )));
}

#[test]
fn pause_and_play_fade_out_and_in() {
    let s = songs();
    solo("pause", false, |r| {
        r.send(play(vec![s.a.clone()]));
        r.pass(2_000);
        r.send(Cmd::Pause);
        r.pass(500);
        assert!(!r.status().playing);
        r.send(Cmd::Play);
        r.pass(2_000);
    });
}

#[test]
fn seeks_crossfade_while_playing_and_move_in_silence_while_paused() {
    let s = songs();
    solo("seek", true, |r| {
        r.send(play(vec![s.a.clone()]));
        r.pass(1_500);
        r.send(Cmd::Seek(6_000));
        r.pass(1_500);
        let p = r.status().position_ms;
        assert!((7_000..7_700).contains(&p), "at 6 s and on: {p}");
        r.send(Cmd::Seek(1_000));
        r.pass(1_500);
        r.send(Cmd::Pause);
        r.pass(300);
        r.send(Cmd::Seek(4_000));
        r.pass(300);
        assert_eq!(r.status().position_ms, 4_000);
        r.send(Cmd::Play);
        r.pass(1_000);
    });
}

#[test]
fn next_previous_and_jump_fade_the_old_song_out_as_the_new_starts() {
    let s = songs();
    solo("skips", false, |r| {
        // Songs that start as songs do (B is the seamless continuation of A, with no rise).
        let d = song(45.0, 10.0, true, true);
        r.send(play(vec![s.a.clone(), s.c.clone(), d]));
        r.pass(1_500);
        r.send(Cmd::Next);
        r.pass(1_500);
        assert_eq!(r.status().index, Some(1));
        r.send(Cmd::Previous);
        r.pass(1_000);
        assert_eq!(r.status().index, Some(0));
        r.send(Cmd::Jump(1));
        r.pass(1_000);
        assert_eq!(r.status().index, Some(1));
        r.send(Cmd::Next);
        r.pass(1_000);
        r.send(Cmd::Next);
        r.pass(500);
        assert!(!r.status().playing, "past the last song: paused");
    });
}

#[test]
fn volume_and_sound_tools_glide() {
    let s = songs();
    let rig = solo("volume and tools", false, |r| {
        r.send(play(vec![s.a.clone()]));
        r.pass(1_000);
        r.send(Cmd::Volume(0.5));
        r.pass(1_000);
        r.send(Cmd::Volume(1.0));
        r.pass(1_000);
        r.send(Cmd::Sound(Sound {
            bass_db: 6.0,
            ..Sound::default()
        }));
        r.pass(1_000);
        assert!(!r.status().signal.as_ref().unwrap().bit_perfect);
        r.send(Cmd::Sound(Sound::default()));
        r.pass(1_000);
        assert!(
            r.status().signal.as_ref().unwrap().bit_perfect,
            "exact again"
        );
    });
    assert!(has(&rig.heard_notes, |n| matches!(
        n,
        RenderNote::Gain { target, .. } if *target == 0.5
    )));
}

#[test]
fn a_crossfade_between_songs_and_a_stop_after_the_song() {
    let s = songs();
    solo("crossfade", true, |r| {
        r.send(Cmd::Sound(Sound {
            crossfade_s: 2.0,
            ..Sound::default()
        }));
        r.send(play(vec![s.a.clone(), s.c.clone(), s.b.clone()]));
        assert!(r.until(30_000, |s| s.index == Some(1)), "crossfaded on");
        r.send(Cmd::StopAfterCurrent(true));
        assert!(r.until(30_000, |s| !s.playing), "stopped after it");
        assert_eq!(r.status().index, Some(2), "with the next one ready");
    });
}

#[test]
fn the_sleep_timer_fades_out_and_pauses() {
    let s = songs();
    solo("sleep timer", false, |r| {
        r.send(play(vec![s.a.clone()]));
        r.pass(1_000);
        r.send(Cmd::SleepIn(Some(std::time::Duration::from_secs(3))));
        assert!(r.until(5_000, |s| !s.playing), "paused at its end");
    });
}

#[test]
fn an_output_switch_fades_out_and_comes_back_at_the_place() {
    let s = songs();
    let rig = solo("output switch", false, |r| {
        r.send(play(vec![s.a.clone()]));
        r.pass(2_000);
        let again = choice(&r.clock, &r.outputs);
        r.send(Cmd::Output(again));
        r.pass(2_000);
        assert!(r.status().playing);
    });
    assert!(rig.outputs.lock().unwrap().len() >= 2, "on another output");
}

// --- A stream that runs dry ----------------------------------------------------------------------

/// A song whose bytes the test hands over (a download fed by hand).
#[derive(Clone)]
struct Fed {
    download: Arc<Download>,
}

impl Serialize for Fed {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("fed")
    }
}

impl Item for Fed {
    fn open_on(&self, _: &Streams) -> Result<Source, String> {
        Ok(Source::on_download(self.download.clone(), Some(10_000)))
    }
    fn reopen_when_stopped(&self) -> bool {
        false
    }
}

fn fed(path: &Path) -> (Fed, Feeder, Vec<u8>) {
    let (download, feeder) = Download::fed(OpenInfo {
        mime_type: Some("audio/wav".into()),
        url_hint: "song.wav".into(),
        ..OpenInfo::default()
    });
    (Fed { download }, feeder, std::fs::read(path).unwrap())
}

#[test]
fn a_stream_that_runs_dry_fades_out_says_so_and_comes_back() {
    let s = songs();
    let (item, mut feeder, bytes) = fed(&s.a);
    let mut rig: Rig<Fed> = Rig::new();
    // 2 s of it, then nothing for 3 s, then the rest.
    let two_s = 44 + 2 * 48_000 * 6;
    feeder.append(&bytes[..two_s]);
    rig.send(play(vec![item]));
    assert!(
        rig.until(5_000, |s| s.playing && s.position_ms > 2_000 && s.buffering),
        "it says it waits for the stream"
    );
    assert!(rig.status().stalled, "partway, not opening");
    rig.pass(1_000);
    feeder.append(&bytes[two_s..]);
    feeder.finish();
    assert!(
        rig.until(5_000, |s| !s.buffering && s.position_ms > 3_000),
        "and plays on"
    );
    rig.pass(1_000);
    let heard = rig.finish();
    judge("stream", &heard, &rig.heard_notes, false);
    assert!(has(&rig.heard_notes, |n| matches!(
        n,
        RenderNote::Starved { .. }
    )));
    assert!(has(&rig.heard_notes, |n| matches!(
        n,
        RenderNote::Resumed { .. }
    )));
}

// --- Programs ----------------------------------------------------------------------------------

/// The host's own map: host time is this computer's.
fn identity() -> together::clock_map::ClockMapHandle {
    let h = clock_map::handle();
    h.store(Arc::new(Some(ClockMap {
        epoch: 0,
        local_ref_ns: 0,
        host_ref_us: 0,
        rate: 1.0,
        bound_us: 0,
    })));
    h
}

/// The frames of the last start: where it began and its content place there.
fn last_start(notes: &[RenderNote]) -> (u64, u64) {
    notes
        .iter()
        .rev()
        .find_map(|n| match *n {
            RenderNote::Started { at, content, .. } => Some((at, content)),
            _ => None,
        })
        .expect("a start")
}

#[test]
fn a_room_takes_the_player_and_your_queue_comes_back_exactly() {
    let s = songs();
    // One player joins a room 2 s into its song; the other pauses there instead.
    let mut joined: Rig<PathBuf> = Rig::new();
    let mut alone: Rig<PathBuf> = Rig::new();
    for r in [&mut joined, &mut alone] {
        r.send(play(vec![s.a.clone(), s.b.clone()]));
        r.pass(2_000);
    }
    let room = {
        let player = joined.player.as_ref().unwrap();
        RoomPlayer::attach(player, identity(), SyncMode::Apart)
    };
    alone.send(Cmd::Pause);
    joined.pass(20);
    alone.pass(20);
    // The room plays C from its start, 300 ms from now.
    room.prepare(PrepId(1), EntryId(7), s.c.clone(), 0, Alignment::EXACT);
    let host_now = (joined.clock.now_ns() / 1_000) as i64;
    room.commit(Anchor {
        entry: EntryId(7),
        content_us: 0,
        host_us: host_now + 300_000,
        playing: true,
        seq: 1,
    });
    joined.pass(3_000);
    alone.pass(3_020);
    let st = room.status();
    assert_eq!(st.entry, Some(EntryId(7)), "the room's song plays: {st:?}");
    assert!(joined.status().sync.is_some());
    room.detach();
    joined.pass(1_500);
    alone.pass(1_500);
    let (j, a) = (joined.status().clone(), alone.status().clone());
    assert!(!j.playing, "your own queue back, paused");
    assert_eq!(j.index, Some(0));
    assert!(j.sync.is_none(), "nothing of the room left");
    assert_eq!(j.position_ms, a.position_ms, "at its place");
    for r in [&mut joined, &mut alone] {
        r.send(Cmd::Play);
        r.pass(2_000);
    }
    let (hj, ha) = (joined.finish(), alone.finish());
    let (sj, cj) = last_start(&joined.heard_notes);
    let (sa, ca) = last_start(&alone.heard_notes);
    assert_eq!(cj, ca, "from the same place");
    let n = ms(1_500) as usize;
    for c in 0..2 {
        assert!(
            hj[c][sj as usize..sj as usize + n] == ha[c][sa as usize..sa as usize + n],
            "channel {c}: bit-identical to a player that never joined"
        );
    }
    judge("after a room", &hj, &joined.heard_notes, false);
    // The room's song started from silence at its first frame, and faded out over 1 s.
    assert!(has(&joined.heard_notes, |n| matches!(
        n,
        RenderNote::Faded { from, to, kind: player::voice_api::FadeKind::Out, .. }
            if to - from == ms(1_000)
    )));
}

// --- The song decoded ahead is the one Next and Jump play -------------------------------------

/// A song that counts its opens.
#[derive(Clone)]
struct Counted {
    path: PathBuf,
    opens: Arc<std::sync::atomic::AtomicUsize>,
}

impl Serialize for Counted {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.path.display().to_string())
    }
}

impl Item for Counted {
    fn path(&self) -> Option<&Path> {
        Some(&self.path)
    }
    fn open(&self) -> Result<Source, String> {
        self.opens.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Source::open(&self.path)
    }
}

fn counted(n: usize) -> Vec<Counted> {
    (0..n)
        .map(|i| Counted {
            path: song(10.0 * i as f64, 5.0, true, true),
            opens: Arc::default(),
        })
        .collect()
}

fn opens(items: &[Counted], i: usize) -> usize {
    items[i].opens.load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn next_plays_the_song_already_decoded_ahead_without_opening_it_again() {
    let items = counted(3);
    let mut r: Rig<Counted> = Rig::new();
    r.send(play(items.clone()));
    r.pass(500);
    assert_eq!(opens(&items, 1), 1, "the next song was opened ahead");
    r.send(Cmd::Next);
    r.pass(500);
    assert_eq!(r.status().index, Some(1));
    assert!(r.status().playing);
    assert_eq!(opens(&items, 1), 1, "and it was not opened a second time");
    r.finish();
}

#[test]
fn jumping_to_the_song_decoded_ahead_reuses_it_and_any_other_opens_fresh() {
    let items = counted(3);
    let mut r: Rig<Counted> = Rig::new();
    r.send(play(items.clone()));
    r.pass(500);
    r.send(Cmd::Jump(1));
    r.pass(300);
    assert_eq!(opens(&items, 1), 1, "song 2 reused");
    r.finish();
    let items = counted(3);
    let mut r: Rig<Counted> = Rig::new();
    r.send(play(items.clone()));
    r.pass(500);
    assert_eq!(opens(&items, 2), 0, "only the next one is opened ahead");
    r.send(Cmd::Jump(2));
    r.pass(300);
    assert_eq!(r.status().index, Some(2));
    assert_eq!(opens(&items, 2), 1, "song 3 opened once, now");
    r.finish();
}
