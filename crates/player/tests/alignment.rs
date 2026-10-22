//! Alignment of two copies of a song (design 4.5 and 5.8, the WP10 gates): known offsets are
//! found within 2 ms, codec priming is measured, an edit is caught, repetitive music is turned
//! down rather than placed wrong, and the PCM hash matches only identical audio.
//!
//! Everything here is synthetic: a small synthesizer (`align_kit`) writes music-shaped signals (drums,
//! notes, chords, a tempo, a little human timing) straight from event lists, so a shift, a cut
//! or an inserted section is exact to the sample and can be rendered at any rate. The three
//! lossy fixtures are that synthesizer's output encoded by ffmpeg (see `make_fixtures`); no
//! real recording is used anywhere. The synthesizer and the fixtures are `align_kit`'s, shared
//! with the room's measure tests.

mod align_kit;

use align_kit::*;
use player::analysis::{self, Verdict};
use std::time::Instant;
use together::align::{Alignment, CopyClass, Envelope};

// ---------------------------------------------------------------------------------------
// The envelope itself
// ---------------------------------------------------------------------------------------

#[test]
fn the_envelope_has_the_windows_of_the_design() {
    let secs = 131.0;
    let e = env(&song(1, secs), secs, REF);
    assert_eq!(e.hop_ms, 10);
    let shape: Vec<(u32, usize)> = e.windows.iter().map(|(s, w)| (*s, w.len())).collect();
    // The first 30 s, then 10 s every 60 s.
    assert_eq!(shape, vec![(0, 3_000), (60_000, 1_000), (120_000, 1_000)]);
    assert!(e.pcm_hash.is_some());
    // Onsets show: the envelope is not flat.
    let busy = e.windows[0].1.iter().filter(|&&b| b >= 24).count();
    assert!(busy > 100, "only {busy} strong hops in 30 s");

    // A short song: one window, cut where the song ends.
    let short = env(&song(1, 21.0), 21.0, REF);
    assert_eq!(short.windows.len(), 1);
    let len = short.windows[0].1.len();
    assert!((2_090..=2_100).contains(&len), "{len} hops for 21 s");
}

#[test]
fn the_envelope_does_not_depend_on_how_the_audio_is_cut_into_blocks() {
    let pcm = render(&song(3, 70.0), 70.0, REF);
    let whole = envelope_of(&pcm, REF.rate, REF.ch);
    for block in [1usize, 7, 441, 4_095, 100_000] {
        // Blocks of any size, even ones that split a frame between channels.
        let e = analysis::envelope(&mut pcm.chunks(block), REF.rate, REF.ch);
        assert_eq!(e, whole, "blocks of {block} samples");
    }
}

#[test]
fn an_empty_or_silent_song_gives_nothing_to_align() {
    let e = analysis::envelope(&mut std::iter::empty::<&[i32]>(), 44_100, 2);
    assert!(e.windows.is_empty());
    assert_eq!(e.pcm_hash, None);
    let silence = vec![0i32; 44_100 * 2 * 40];
    let s = envelope_of(&silence, 44_100, 2);
    let a = analysis::align(&s, &s.clone_without_hash());
    assert_eq!(a.class, CopyClass::Approximate);
    assert_eq!(a.offset_us, 0);
}

trait NoHash {
    fn clone_without_hash(&self) -> Envelope;
}
impl NoHash for Envelope {
    fn clone_without_hash(&self) -> Envelope {
        Envelope {
            pcm_hash: None,
            ..self.clone()
        }
    }
}

// ---------------------------------------------------------------------------------------
// Gate: known offsets within 2 ms
// ---------------------------------------------------------------------------------------

#[test]
fn known_offsets_up_to_3_s_are_found_within_2_ms() {
    let secs = 40.0;
    let events = song(1, secs + 4.0);
    let reference = env(&events, secs, REF);
    let mut worst: f64 = 0.0;
    for offset in [
        3.0, -3.0, 2.987_6, -1.234_5, 0.0, 0.037_1, -0.004_9, 1.000_02,
    ] {
        let mine = env(&offset_copy(&events, offset, secs), secs, OTHER);
        let a = analysis::align(&reference, &mine);
        let e = err_ms(a.offset_us, offset);
        println!(
            "offset {offset:+.4} s: measured {:+.4} s, error {e:.3} ms, {:?}, confidence {:.2}",
            a.offset_us as f64 / 1e6,
            a.class,
            a.confidence
        );
        assert_eq!(a.class, CopyClass::Measured, "offset {offset}");
        assert!(e <= 2.0, "offset {offset}: off by {e:.3} ms");
        assert!(a.confidence >= 8.0, "confidence {}", a.confidence);
        worst = worst.max(e);
    }
    println!("worst error {worst:.3} ms");
}

#[test]
fn offsets_are_found_on_other_songs_and_rates_too() {
    let secs = 35.0;
    let mut worst: f64 = 0.0;
    for (seed, offset, m) in [
        (11, 2.5, OTHER),
        (12, -2.75, OTHER),
        (
            13,
            0.333,
            Master {
                rate: 22_050,
                ch: 1,
                ..OTHER
            },
        ),
        (
            14,
            -0.9,
            Master {
                rate: 96_000,
                ch: 2,
                gain: 1.0,
                ..OTHER
            },
        ),
        (
            15,
            1.75,
            Master {
                rate: 44_100,
                ch: 1,
                gain: 0.3,
                ..OTHER
            },
        ),
    ] {
        let events = song(seed, secs + 4.0);
        let reference = env(&events, secs, REF);
        let mine = env(&offset_copy(&events, offset, secs), secs, m);
        let a = analysis::align(&reference, &mine);
        let e = err_ms(a.offset_us, offset);
        println!(
            "seed {seed} offset {offset:+} at {} Hz: error {e:.3} ms, {:?}",
            m.rate, a.class
        );
        assert_eq!(a.class, CopyClass::Measured, "seed {seed}");
        assert!(e <= 2.0, "seed {seed}: off by {e:.3} ms");
        worst = worst.max(e);
    }
    println!("worst error {worst:.3} ms");
}

#[test]
fn later_windows_agree_through_a_whole_song() {
    let secs = 190.0;
    let events = song(21, secs + 4.0);
    let reference = env(&events, secs, REF);
    let offset = -2.2;
    let mine = env(&offset_copy(&events, offset, secs), secs, OTHER);
    let track = analysis::align_track(&reference, &mine);
    assert_eq!(track.alignment.class, CopyClass::Measured);
    assert!(err_ms(track.alignment.offset_us, offset) <= 2.0);
    assert_eq!(track.edit_at_ms, None);
    let starts: Vec<u32> = track.windows.iter().map(|w| w.start_ms).collect();
    assert_eq!(starts, vec![60_000, 120_000, 180_000]);
    for w in &track.windows {
        match w.verdict {
            Verdict::Agrees { offset_us } => {
                assert!(err_ms(offset_us, offset) <= 2.0, "window {}", w.start_ms)
            }
            other => panic!("window {}: {other:?}", w.start_ms),
        }
    }
}

#[test]
fn unrelated_songs_are_not_aligned() {
    let secs = 40.0;
    for (a, b) in [(1, 2), (5, 6), (7, 8)] {
        let x = env(&song(a, secs), secs, REF);
        let y = env(&song(b, secs), secs, OTHER);
        let r = analysis::align(&x, &y);
        assert_eq!(r.class, CopyClass::Approximate, "songs {a} and {b}");
        assert_eq!(r.offset_us, 0);
    }
}

#[test]
fn an_intro_longer_than_the_search_is_approximate() {
    // A video intro of 8 s: beyond the 5 s the first window searches.
    let secs = 40.0;
    let events = song(31, secs);
    let reference = env(&events, secs, REF);
    let mine = env(&offset_copy(&events, 8.0, secs), secs, OTHER);
    let a = analysis::align(&reference, &mine);
    assert_eq!(a.class, CopyClass::Approximate);
}

// ---------------------------------------------------------------------------------------
// Gate: encoder delays (MP3, AAC and Opus priming)
// ---------------------------------------------------------------------------------------

#[test]
fn codec_priming_is_measured_within_2_ms() {
    let reference_pcm = render(&song(FIXTURE_SEED, FIXTURE_SECS), FIXTURE_SECS, REF);
    let reference = envelope_of(&reference_pcm, REF.rate, REF.ch);
    for name in [
        "align-music.mp3",
        "align-music-raw.mp3",
        "align-music.m4a",
        "align-music.opus",
    ] {
        let (rate, ch, pcm) = decode_all(&fixture(name));
        let truth = true_delay(&pcm, ch, rate);
        let truth_s = truth as f64 / rate as f64;
        let mine = envelope_of(&pcm, rate, ch);
        let a = analysis::align(&reference, &mine);
        let e = err_ms(a.offset_us, truth_s);
        println!(
            "{name}: {rate} Hz, true delay {truth} samples ({:.2} ms), measured {:.2} ms, error {e:.3} ms, {:?}",
            truth_s * 1e3,
            a.offset_us as f64 / 1e3,
            a.class
        );
        assert_eq!(a.class, CopyClass::Measured, "{name}");
        assert!(e <= 2.0, "{name}: off by {e:.3} ms");
    }
}

/// Writes the lossy fixtures: the synthesizer's song as a WAV, encoded by ffmpeg as MP3 (LAME,
/// with and without its gapless header),
/// AAC in MP4 and Opus in Ogg, mono at low bitrates to keep them small. Run by hand:
/// `FFMPEG=path/to/ffmpeg cargo test -p player --test alignment make_fixtures -- --ignored`.
#[test]
#[ignore = "writes the fixtures; needs ffmpeg"]
fn make_fixtures() {
    let ffmpeg = std::env::var("FFMPEG").expect("FFMPEG names ffmpeg");
    let dir = std::env::temp_dir().join("venuta-align-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("align-music.wav");
    let m = Master { ch: 1, ..REF };
    write_wav(
        &wav,
        &render(&song(FIXTURE_SEED, FIXTURE_SECS), FIXTURE_SECS, m),
        m.rate,
        m.ch,
    );
    for (name, args) in [
        (
            "align-music.mp3",
            &["-c:a", "libmp3lame", "-b:a", "48k", "-ar", "44100"][..],
        ),
        // Without the LAME header the decoder cannot trim the priming: it plays as a delay.
        (
            "align-music-raw.mp3",
            &[
                "-c:a",
                "libmp3lame",
                "-b:a",
                "48k",
                "-ar",
                "44100",
                "-write_xing",
                "0",
            ][..],
        ),
        (
            "align-music.m4a",
            &["-c:a", "aac", "-b:a", "48k", "-ar", "44100"][..],
        ),
        ("align-music.opus", &["-c:a", "libopus", "-b:a", "32k"][..]),
    ] {
        let out = fixture(name);
        let status = std::process::Command::new(&ffmpeg)
            .args(["-y", "-loglevel", "error", "-i"])
            .arg(&wav)
            .args(["-map_metadata", "-1"])
            .args(args)
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success(), "{name}");
    }
}


// ---------------------------------------------------------------------------------------
// Gate: an edit is detected
// ---------------------------------------------------------------------------------------

#[test]
fn a_cut_after_the_first_window_is_detected_where_it_shows() {
    let secs = 130.0;
    let events = song(41, secs + 4.0);
    let reference = env(&events, secs, REF);
    // A radio edit: 2.5 s cut at 75 s; the copy also starts 0.4 s later.
    let mine_events = offset_copy(&cut(&events, 75.0, 2.5), 0.4, secs);
    let track = analysis::align_track(&reference, &env(&mine_events, secs, OTHER));
    // Aligned at the start as well as can be, but not trusted through the song.
    assert_eq!(track.alignment.class, CopyClass::Approximate);
    assert!(err_ms(track.alignment.offset_us, 0.4) <= 2.0);
    assert_eq!(track.edit_at_ms, Some(120_000));
    assert!(matches!(track.windows[0].verdict, Verdict::Agrees { .. }));
    match track.windows[1].verdict {
        // Where the edit shows, the new offset is measured too.
        Verdict::Moved { offset_us } => assert!(err_ms(offset_us, 0.4 - 2.5) <= 2.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_inserted_section_is_detected_short_or_long() {
    let secs = 130.0;
    let events = song(42, secs + 4.0);
    let reference = env(&events, secs, REF);
    for len in [3.0, 12.0] {
        let mine = env(&insert(&events, 40.0, len), secs, OTHER);
        let track = analysis::align_track(&reference, &mine);
        println!("insert {len} s: {:?}", track.windows);
        assert_eq!(
            track.alignment.class,
            CopyClass::Approximate,
            "insert of {len} s"
        );
        assert!(err_ms(track.alignment.offset_us, 0.0) <= 2.0);
        assert_eq!(track.edit_at_ms, Some(60_000), "insert of {len} s");
    }
}

#[test]
fn a_cut_inside_the_first_window_is_detected() {
    let secs = 40.0;
    let events = song(43, secs + 4.0);
    let reference = env(&events, secs, REF);
    for (at, len, start) in [
        (15.0, 2.0, 0.0),
        (12.0, 0.6, 0.0),
        (20.0, 4.0, 0.0),
        (15.0, 2.0, 0.4),
        (18.0, 1.5, -0.7),
    ] {
        let mine = env(
            &offset_copy(&cut(&events, at, len), start, secs),
            secs,
            OTHER,
        );
        let a = analysis::align(&reference, &mine);
        println!("cut {len} s at {at} s: {a:?}");
        assert_eq!(a.class, CopyClass::Approximate, "cut of {len} s at {at} s");
        // Aligned at the start as well as can be: the copy is the same up to the cut.
        assert!(
            err_ms(a.offset_us, start) <= 2.0,
            "cut of {len} s at {at} s: {a:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Gate: repetitive music is turned down, never placed wrong
// ---------------------------------------------------------------------------------------

#[test]
fn repetitive_music_is_rejected_rather_than_misaligned() {
    let secs = 130.0;
    let mut cases = 0;
    let mut rejected = 0;
    for (seed, bars, wobble) in [
        (61, 1, 0.0),
        (62, 2, 0.0),
        (63, 1, 0.02),
        (64, 2, 0.1),
        (65, 1, 0.3),
    ] {
        let events = looped(seed, bars, secs + 4.0, wobble);
        let reference = env(&events, secs, REF);
        for offset in [3.0, -1.0, 0.5] {
            let mine = env(&offset_copy(&events, offset, secs), secs, OTHER);
            let a = analysis::align(&reference, &mine);
            println!(
                "loop seed {seed} of {bars} bar(s), wobble {wobble}, offset {offset:+}: {:?} at {:+.4} s",
                a.class,
                a.offset_us as f64 / 1e6
            );
            cases += 1;
            if a.class == CopyClass::Measured {
                // Allowed only when it is right.
                assert!(
                    err_ms(a.offset_us, offset) <= 2.0,
                    "loop {seed} offset {offset}: placed at {} us",
                    a.offset_us
                );
            } else {
                rejected += 1;
            }
            // An exact loop can never be placed: every bar is the same.
            if wobble == 0.0 {
                assert_eq!(
                    a.class,
                    CopyClass::Approximate,
                    "exact loop {seed} offset {offset}"
                );
            }
        }
    }
    println!("{rejected} of {cases} loop cases rejected, the rest measured right");
}

// ---------------------------------------------------------------------------------------
// Gate: the PCM hash matches only identical audio
// ---------------------------------------------------------------------------------------

#[test]
fn the_pcm_hash_matches_only_identical_audio() {
    let pcm = render(&song(71, 15.0), 15.0, REF);
    let base = envelope_of(&pcm, REF.rate, REF.ch).pcm_hash.unwrap();
    let hash = |p: &[i32], rate: u32, ch: u16| envelope_of(p, rate, ch).pcm_hash.unwrap();

    // The same audio, cut into blocks any way: the same hash.
    assert_eq!(
        analysis::envelope(&mut pcm.chunks(3), REF.rate, REF.ch).pcm_hash,
        Some(base)
    );

    // The same audio through a file and the decoder: the same hash.
    let dir = std::env::temp_dir().join(format!("venuta-align-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("same.wav");
    write_wav(&wav, &pcm, REF.rate, REF.ch);
    let (rate, ch, decoded) = decode_all(&wav);
    assert_eq!(hash(&decoded, rate, ch), base, "a WAV of the same samples");
    let _ = std::fs::remove_dir_all(&dir);

    // Anything else: another hash.
    let mut one_lsb = pcm.clone();
    one_lsb[REF.rate as usize * 2 * 5] += 1 << 16;
    assert_ne!(
        hash(&one_lsb, REF.rate, REF.ch),
        base,
        "one sample one step off at 5 s"
    );
    let mut late = pcm.clone();
    late.insert(0, 0);
    late.insert(0, 0);
    assert_ne!(hash(&late, REF.rate, REF.ch), base, "one frame later");
    assert_ne!(
        hash(&pcm, 48_000, REF.ch),
        base,
        "the same numbers at another rate"
    );
    assert_ne!(hash(&pcm, REF.rate, 1), base, "the same numbers as mono");
    let quieter: Vec<i32> = pcm.iter().map(|&s| (s >> 17) << 16).collect();
    assert_ne!(hash(&quieter, REF.rate, REF.ch), base, "6 dB quieter");
    let mut swapped = pcm.clone();
    for f in swapped.chunks_mut(2) {
        f.swap(0, 1);
    }
    assert_ne!(hash(&swapped, REF.rate, REF.ch), base, "channels swapped");
    let short = &pcm[..REF.rate as usize * 2 * 8];
    assert_ne!(hash(short, REF.rate, REF.ch), base, "the first 8 s only");
}

#[test]
fn identical_audio_is_exact_unless_a_later_window_shows_an_edit() {
    let secs = 130.0;
    let events = song(72, secs + 4.0);
    let pcm = render(&events, secs, REF);
    let reference = envelope_of(&pcm, REF.rate, REF.ch);
    let same = envelope_of(&pcm, REF.rate, REF.ch);
    let a = analysis::align(&reference, &same);
    assert_eq!(a.class, CopyClass::Exact);
    assert_eq!(a.offset_us, 0);
    assert_eq!(a, Alignment::EXACT);

    // A radio edit whose first 10 s are the same samples: its hash matches, but the cut
    // at 70 s shows in the window at 120 s, so it is not exact.
    let edit = render(&cut(&events, 70.0, 3.0), secs, REF);
    let edit_env = envelope_of(&edit, REF.rate, REF.ch);
    assert_eq!(edit_env.pcm_hash, reference.pcm_hash);
    let a = analysis::align(&reference, &edit_env);
    assert_eq!(a.class, CopyClass::Approximate);
    assert_eq!(a.offset_us, 0);

    // Without the hash, the same audio is measured at 0.
    let a = analysis::align(&reference, &same.clone_without_hash());
    assert_eq!(a.class, CopyClass::Measured);
    assert!(a.offset_us.abs() <= 2_000);
}

// ---------------------------------------------------------------------------------------
// Cost: cheap enough for a low-end handheld
// ---------------------------------------------------------------------------------------

#[test]
fn cost_per_minute_of_audio() {
    // Five minutes of 48 kHz stereo (content does not change the cost: noise-like music).
    let rate = 48_000u32;
    let minute = render(&song(81, 60.0), 60.0, Master { rate, ..REF });
    let five: Vec<i32> = (0..5).flat_map(|_| minute.iter().copied()).collect();
    let runs = 3;
    let mut best_env = f64::MAX;
    let mut env5 = None;
    for _ in 0..runs {
        let t = Instant::now();
        let e = analysis::envelope(&mut five.chunks(4_096), rate, 2);
        best_env = best_env.min(t.elapsed().as_secs_f64());
        env5 = Some(e);
    }
    let env5 = env5.unwrap();
    let analysed_s: f64 = env5
        .windows
        .iter()
        .map(|(_, w)| w.len() as f64 / 100.0)
        .sum();
    let mut best_align = f64::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        let _ = analysis::align_track(&env5, &env5.clone_without_hash());
        best_align = best_align.min(t.elapsed().as_secs_f64());
    }
    println!(
        "envelope: {:.1} ms for 5 min fed ({:.2} ms per minute fed, {:.1} s analysed, {:.2} ms per analysed minute); align: {:.1} ms",
        best_env * 1e3,
        best_env * 1e3 / 5.0,
        analysed_s,
        best_env * 1e3 / (analysed_s / 60.0),
        best_align * 1e3
    );
    // Loose bounds that hold on a busy machine; the real numbers are printed above. A
    // handheld several times slower still does a song in well under a second.
    assert!(best_env < 0.5, "envelope of 5 minutes took {best_env:.3} s");
    assert!(best_align < 0.25, "align took {best_align:.3} s");
}
