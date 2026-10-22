//! Every kind of output device, on engine 2 and a simulated clock (`docs/design/devices.md`):
//! Bluetooth's stereo and hands-free twins, a USB DAC with a few exclusive formats, an HDMI
//! output that vanishes and comes back, Windows changing a device's format while it plays,
//! sleep and wake, the default moving, virtual devices, and the delay per output in the place
//! reported. The devices are `SimDevices`; each output opened on one is a `SimOutput` whose
//! DAC's every run is judged by the detectors: it starts from silence (no burst), a run left
//! for another output ends faded, nothing stale plays, and the music inside is clean.

mod engine_kit;
mod render_kit;

use audio_check::Declared;
use engine_kit::{Rig, play};
use player::devices::{Endpoint, EndpointState, FormatTable, Kind, form_factor as ff};
use player::engine::{Cmd, OutputChoice, SimDevicesChoice, Status};
use player::output::{DeviceFormat, Mode, SampleKind};
use player::sim::SimConfig;
use player::sim_devices::{SimDevices, SimEndpoint};
use render_kit::{FULL, assert_clean, judge_music};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use together::time::{SimClock, Timebase};

// --- The devices ------------------------------------------------------------------------------

fn fmt(rate: u32, ch: u16, container: u16, valid: u16) -> DeviceFormat {
    DeviceFormat {
        rate,
        channels: ch,
        container_bits: container,
        valid_bits: valid,
        kind: SampleKind::Int,
    }
}

fn endpoint(id: &str, desc: &str, adapter: &str, en: &str, form: u32, container: &str) -> Endpoint {
    Endpoint {
        id: id.into(),
        description: desc.into(),
        adapter: adapter.into(),
        enumerator: en.into(),
        form_factor: form,
        container: Some(container.into()),
        state: EndpointState::Active,
    }
}

/// Exclusive formats: these rates, 16-bit and 24 in 32, stereo.
fn table(rates: &[u32]) -> FormatTable {
    let mut t = FormatTable::default();
    for &r in rates {
        t.formats.push((fmt(r, 2, 16, 16), 0));
        t.formats.push((fmt(r, 2, 32, 24), 0));
    }
    t
}

fn sim(ep: Endpoint, mix: DeviceFormat, exclusive: Option<FormatTable>) -> SimEndpoint {
    SimEndpoint {
        endpoint: ep,
        mix,
        exclusive,
        busy: false,
        fails: false,
        cfg: SimConfig::default(),
    }
}

const MACHINE: &str = "{00000000-0000-0000-ffff-ffffffffffff}";
const PODS: &str = "{eab42fbe-0000-0000-0000-000000000001}";

/// The laptop's speakers: 44.1 and 48 kHz in exclusive mode, 48 kHz shared.
fn speakers() -> SimEndpoint {
    sim(
        endpoint(
            "speakers",
            "Speakers",
            "Realtek(R) Audio",
            "INTELAUDIO",
            ff::SPEAKERS,
            MACHINE,
        ),
        fmt(48_000, 2, 32, 32),
        Some(table(&[44_100, 48_000])),
    )
}

/// A USB DAC: 44.1, 48 and 96 kHz in exclusive mode.
fn dac() -> SimEndpoint {
    sim(
        endpoint(
            "dac",
            "Headphones",
            "SNOWSKY DISC",
            "USB",
            ff::HEADPHONES,
            "{11eb762d-0000-0000-0000-000000000004}",
        ),
        fmt(48_000, 2, 32, 32),
        Some(table(&[44_100, 48_000, 96_000])),
    )
}

/// A monitor's HDMI sink.
fn hdmi() -> SimEndpoint {
    sim(
        endpoint(
            "hdmi",
            "XG32UCWG",
            "NVIDIA High Definition Audio",
            "HDAUDIO",
            ff::DISPLAY,
            "{1c809401-0000-0000-0000-000000000005}",
        ),
        fmt(48_000, 2, 32, 32),
        Some(table(&[48_000])),
    )
}

/// AirPods: the stereo (A2DP) endpoint, and the hands-free one (mono, 16 kHz).
fn pods() -> SimEndpoint {
    sim(
        endpoint(
            "pods",
            "Headphones",
            "AirPods Pro",
            "BTHENUM",
            ff::HEADPHONES,
            PODS,
        ),
        fmt(48_000, 2, 32, 32),
        Some(table(&[44_100, 48_000])),
    )
}

fn pods_hfp() -> SimEndpoint {
    sim(
        endpoint(
            "pods-hf",
            "Headset",
            "AirPods Pro Hands-Free",
            "BTHHFENUM",
            ff::HEADSET,
            PODS,
        ),
        fmt(16_000, 1, 16, 16),
        None,
    )
}

fn mirror() -> SimEndpoint {
    sim(
        endpoint(
            "mirror",
            "Speakers",
            "Nahimic mirroring device",
            "SWD",
            ff::SPEAKERS,
            MACHINE,
        ),
        fmt(48_000, 2, 32, 32),
        None,
    )
}

// --- The bench --------------------------------------------------------------------------------

struct Bench {
    rig: Rig<PathBuf>,
    devices: Arc<SimDevices>,
}

fn bench(eps: Vec<SimEndpoint>, default: Option<&str>, id: Option<&str>, exclusive: bool) -> Bench {
    let clock = SimClock::new(1_000_000_000);
    let devices = SimDevices::new(Arc::new(clock.clone()));
    for e in eps {
        devices.add(e);
    }
    devices.set_default(default);
    let rig = Rig::with_choice(
        clock.clone(),
        OutputChoice::SimDevices(SimDevicesChoice {
            clock,
            devices: devices.clone(),
            id: id.map(str::to_owned),
            exclusive,
        }),
    );
    Bench { rig, devices }
}

/// A song of `secs` of the test music from `from_s`, 24-bit stereo at `rate`.
fn song(rate: u32, from_s: f64, secs: f64, rise: bool, fall: bool) -> PathBuf {
    let piece = audio_check::music::Piece { rate, channels: 2 };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "venuta-devices-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    piece.write(
        &f,
        (from_s * f64::from(rate)) as usize,
        (secs * f64::from(rate)) as usize,
        rise,
        fall,
    );
    f
}

impl Bench {
    fn status(&mut self) -> Status<PathBuf> {
        self.rig.status().clone()
    }

    /// The output the music is on now (the last opened).
    fn on(&self) -> String {
        self.devices
            .opened()
            .last()
            .map(|o| o.id.clone())
            .unwrap_or_default()
    }

    /// Let time pass until the music sounds on `id` (at most `ms`).
    fn until_on(&mut self, id: &str, ms: u64) -> bool {
        for _ in 0..ms {
            self.rig.pass(1);
            if self.on() == id
                && self.rig.seen.last().is_some_and(|(_, s)| {
                    s.playing
                        && s.signal
                            .as_ref()
                            .is_some_and(|g| g.endpoint.as_deref() == Some(id))
                })
            {
                return true;
            }
        }
        false
    }

    /// Every status since the first play said playing, without an error.
    fn never_stopped(&self) {
        let first = self
            .rig
            .seen
            .iter()
            .position(|(_, s)| s.playing)
            .expect("it played");
        for (_, s) in &self.rig.seen[first..] {
            assert!(s.error.is_empty(), "no error: {}", s.error);
            assert!(s.playing, "never paused");
        }
    }

    /// Quit, and judge every run of every output opened: `lost` lists the openings whose
    /// device went away under them (their runs end where the device went, not faded);
    /// `exact` says the music was carried unchanged (no resampling) on that opening.
    fn finish(
        mut self,
        what: &str,
        lost: &[usize],
        exact: impl Fn(usize) -> bool,
    ) -> Arc<SimDevices> {
        if let Some(p) = self.rig.player.take() {
            drop(p);
        }
        self.rig.clock.pass(500_000_000);
        for (k, o) in self.devices.opened().iter().enumerate() {
            for (r, run) in o.handle.tape().runs.iter().enumerate() {
                let w = format!("{what}: output {k} ({}), run {r}", o.id);
                judge_run(&w, run, o.format.rate, !lost.contains(&k), exact(k));
            }
        }
        self.devices
    }
}

/// One run of a DAC: from silence, no burst; ended faded unless its device went; nothing stale;
/// clean inside by every detector (bit-exact only where nothing was resampled).
fn judge_run(what: &str, run: &player::sim::Run, rate: u32, ends_faded: bool, exact: bool) {
    assert_eq!(run.stale, 0, "{what}: nothing held from before");
    assert!(run.dry.is_empty(), "{what}: the DAC never ran dry");
    let ms = |n: u64| (u64::from(rate) * n / 1000) as usize;
    let reference = render_kit::Song::music(rate, 2, 0, u64::from(rate) * 24);
    for c in 0..2u16 {
        let heard = run.left_justified(c);
        let Some(first) = heard.iter().position(|&v| v != 0) else {
            continue;
        };
        // No burst: the first millisecond of sound is far under the music's level (a fade in, or
        // a song's own rise from silence).
        let burst = heard[first..(first + ms(1)).min(heard.len())]
            .iter()
            .map(|&v| (f64::from(v) / FULL).abs())
            .fold(0.0, f64::max);
        assert!(
            burst < 0.03,
            "{what}, channel {c}: starts from silence ({burst:.3} FS in its first ms)"
        );
        if ends_faded {
            let last = heard.iter().rposition(|&v| v != 0).unwrap();
            let tail = heard[last.saturating_sub(ms(1))..=last]
                .iter()
                .map(|&v| (f64::from(v) / FULL).abs())
                .fold(0.0, f64::max);
            assert!(
                tail < 0.03,
                "{what}, channel {c}: ends faded ({tail:.3} FS in its last ms)"
            );
        }
        let n = heard.len();
        let declared = Declared::new(vec![
            0..(first + ms(25)).min(n),
            n.saturating_sub(ms(45))..n,
        ]);
        let mut v = judge_music(
            &heard,
            &reference.channel(c, 0, reference.frames),
            rate,
            &declared,
        );
        if !exact {
            v.retain(|v| v.detector != "bit-exact");
        }
        assert_clean(&format!("{what}, channel {c}"), &v);
    }
}

fn steps(s: &Status<PathBuf>) -> Vec<String> {
    s.signal
        .as_ref()
        .map(|g| g.steps.clone())
        .unwrap_or_default()
}

// --- Bluetooth --------------------------------------------------------------------------------

#[test]
fn a_bluetooth_device_chosen_by_its_hands_free_endpoint_plays_on_its_stereo_one() {
    let mut b = bench(
        vec![speakers(), pods(), pods_hfp()],
        Some("speakers"),
        Some("pods-hf"),
        true,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 6.0, true, true)]));
    assert!(b.until_on("pods", 3_000), "on the stereo endpoint");
    b.rig.pass(500);
    let s = b.status();
    let g = s.signal.clone().unwrap();
    assert_eq!(g.kind, Some(Kind::Bluetooth));
    assert_eq!(
        b.devices.opened()[0].mode,
        Mode::Shared,
        "Bluetooth plays shared"
    );
    assert!(
        g.note
            .as_deref()
            .unwrap_or("")
            .contains("stereo connection"),
        "{:?}",
        g.note
    );
    assert!(!g.bit_perfect, "nothing is bit-perfect over Bluetooth");
    assert!(
        steps(&s)
            .iter()
            .any(|t| t.contains("Re-encoded for Bluetooth")),
        "{:?}",
        steps(&s)
    );
    assert_eq!((g.delay_ms, g.suggested_delay_ms), (None, Some(170)));
    assert!(!g.follows_default);
    b.rig.pass(1_000);
    b.never_stopped();
    let d = b.finish("hands-free chosen", &[], |_| true);
    assert!(
        d.opened().iter().all(|o| o.id == "pods"),
        "never the hands-free endpoint"
    );
}

#[test]
fn following_windows_to_a_hands_free_default_plays_its_stereo_twin() {
    let mut b = bench(
        vec![speakers(), pods(), pods_hfp()],
        Some("pods-hf"),
        None,
        false,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 4.0, true, true)]));
    assert!(b.until_on("pods", 3_000));
    let g = b.status().signal.unwrap();
    assert!(g.follows_default);
    assert!(
        g.note
            .as_deref()
            .unwrap_or("")
            .contains("stereo connection")
    );
    b.rig.pass(500);
    b.finish("hands-free default", &[], |_| true);
}

#[test]
fn a_device_with_only_its_hands_free_endpoint_plays_there_and_says_why_it_sounds_poor() {
    let mut a2dp = pods();
    a2dp.endpoint.state = EndpointState::Unplugged;
    let mut b = bench(
        vec![speakers(), a2dp, pods_hfp()],
        Some("speakers"),
        Some("pods"),
        true,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 4.0, true, true)]));
    assert!(b.until_on("pods-hf", 3_000));
    let s = b.status();
    let g = s.signal.clone().unwrap();
    assert!(
        g.note.as_deref().unwrap_or("").contains("calls only"),
        "{:?}",
        g.note
    );
    assert_eq!((g.output.rate, g.output.channels), (16_000, 1));
    assert!(
        steps(&s)
            .iter()
            .any(|t| t.contains("Hands-free connection"))
    );
    assert_eq!(g.suggested_delay_ms, None);
    b.rig.pass(500);
    // Call quality takes away the music's top: only the start is judged (no burst).
    let o = &b.devices.opened()[0];
    let run = &o.handle.tape().runs[0];
    let heard = run.left_justified(0);
    let first = heard.iter().position(|&v| v != 0).unwrap();
    let burst = heard[first..first + 16]
        .iter()
        .map(|&v| (f64::from(v) / FULL).abs())
        .fold(0.0, f64::max);
    assert!(burst < 0.03, "{burst}");
}

// --- USB DACs and exclusive mode ---------------------------------------------------------------

#[test]
fn a_usb_dac_plays_each_song_at_its_own_format_where_it_can_and_says_so_where_it_cant() {
    let mut b = bench(vec![speakers(), dac()], Some("speakers"), Some("dac"), true);
    // 96 kHz, then 192 kHz (more than it takes), then 48 kHz: the test music, one piece.
    let s96 = song(96_000, 0.0, 3.0, true, false);
    let s192 = song(192_000, 3.0, 3.0, false, false);
    let s48 = song(48_000, 6.0, 3.0, false, true);
    b.rig.send(play(vec![s96, s192, s48]));
    assert!(b.until_on("dac", 3_000));
    b.rig.pass(300);
    let g = b.status().signal.unwrap();
    assert_eq!(
        (g.mode, g.output.rate, g.output.valid_bits),
        (Mode::Exclusive, 96_000, 24)
    );
    assert!(
        g.bit_perfect,
        "its own format, exclusive: bit-perfect ({:?})",
        g.steps
    );
    // The 192 kHz song: on at 96 kHz, without a reopen (gapless), resampled and said.
    assert!(b.rig.until(5_000, |s| s.index == Some(1)), "on to 192 kHz");
    b.rig.pass(300);
    let s = b.status();
    let g = s.signal.clone().unwrap();
    assert_eq!((g.source.rate, g.output.rate), (192_000, 96_000));
    assert!(!g.bit_perfect);
    assert!(
        steps(&s).iter().any(|t| t == "Resampled to 96 kHz"),
        "{:?}",
        steps(&s)
    );
    assert_eq!(
        b.devices.opened().len(),
        1,
        "no reopen between songs that land on one format"
    );
    // The 48 kHz song: its own rate again (a reopen, from silence at its start).
    assert!(b.rig.until(5_000, |s| s.index == Some(2)), "on to 48 kHz");
    assert!(b.rig.until(2_000, |s| {
        s.signal.as_ref().is_some_and(|g| g.output.rate == 48_000)
    }));
    assert!(b.status().signal.unwrap().bit_perfect);
    assert!(b.rig.until(6_000, |s| !s.playing), "to the end");
    assert_eq!(
        b.devices.probed(),
        1,
        "the device was asked for its formats once"
    );
    let d = b.finish("a USB DAC", &[], |k| k != 0);
    let o = d.opened();
    assert_eq!(o.len(), 2);
    assert_eq!((o[1].format.rate, o[1].mode), (48_000, Mode::Exclusive));
}

#[test]
fn a_dac_another_app_holds_plays_shared_and_says_why() {
    let mut held = dac();
    held.busy = true;
    let mut b = bench(vec![speakers(), held], Some("speakers"), Some("dac"), true);
    b.rig.send(play(vec![song(48_000, 0.0, 3.0, true, true)]));
    assert!(b.until_on("dac", 3_000));
    let s = b.status();
    let g = s.signal.clone().unwrap();
    assert_eq!(g.mode, Mode::Shared);
    assert!(!g.bit_perfect);
    assert!(
        steps(&s)
            .iter()
            .any(|t| t.contains("another app is using it")),
        "{:?}",
        steps(&s)
    );
    b.rig.pass(500);
    b.finish("a DAC held", &[], |_| true);
}

// --- Devices going and coming -------------------------------------------------------------------

#[test]
fn an_hdmi_output_that_vanishes_falls_back_to_the_default_and_comes_back_when_it_does() {
    let mut b = bench(
        vec![speakers(), hdmi()],
        Some("speakers"),
        Some("hdmi"),
        false,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 20.0, true, true)]));
    assert!(b.until_on("hdmi", 3_000));
    b.rig.pass(2_000);
    let before = b.status().position_ms;
    // The monitor sleeps: its sink goes.
    b.devices.set_state("hdmi", EndpointState::NotPresent);
    assert!(b.until_on("speakers", 1_000), "on the fallback at once");
    let s = b.status();
    let g = s.signal.clone().unwrap();
    assert_eq!(
        g.note.as_deref(),
        Some("XG32UCWG isn't connected: playing on Speakers until it's back.")
    );
    let after = s.position_ms;
    assert!(
        after.abs_diff(before) < 300,
        "the place carries on: {before} then {after} ms"
    );
    b.rig.pass(2_000);
    // It wakes in a burst: there, gone, there again. Nothing moves until it has stayed.
    b.devices.set_state("hdmi", EndpointState::Active);
    b.rig.pass(500);
    b.devices.set_state("hdmi", EndpointState::NotPresent);
    b.rig.pass(300);
    b.devices.set_state("hdmi", EndpointState::Active);
    b.rig.pass(1_000);
    assert_eq!(b.on(), "speakers", "not before it has stayed 1.5 s");
    assert!(b.until_on("hdmi", 2_000), "back on the chosen output");
    assert_eq!(b.status().signal.unwrap().note, None);
    b.rig.pass(1_000);
    b.never_stopped();
    let d = b.finish("HDMI going and coming", &[0], |_| true);
    let ids: Vec<String> = d.opened().iter().map(|o| o.id.clone()).collect();
    assert_eq!(ids, ["hdmi", "speakers", "hdmi"]);
}

#[test]
fn windows_changing_the_format_while_playing_reopens_cleanly_at_the_new_one() {
    let mut b = bench(vec![speakers()], Some("speakers"), None, false);
    b.rig.send(play(vec![song(48_000, 0.0, 10.0, true, true)]));
    assert!(b.until_on("speakers", 3_000));
    b.rig.pass(2_000);
    b.devices.set_mix("speakers", fmt(44_100, 2, 32, 32));
    assert!(b.rig.until(1_000, |s| {
        s.signal.as_ref().is_some_and(|g| g.output.rate == 44_100)
    }));
    let s = b.status();
    assert!(steps(&s).iter().any(|t| t == "Resampled to 44.1 kHz"));
    b.rig.pass(2_000);
    b.never_stopped();
    let d = b.finish("a format change", &[0], |k| k == 0);
    assert_eq!(d.opened().len(), 2);
}

#[test]
fn sleep_and_wake_play_on_the_fallback_until_the_dac_is_back_without_anything_old() {
    let mut b = bench(vec![speakers(), dac()], Some("speakers"), Some("dac"), true);
    b.rig.send(play(vec![song(48_000, 0.0, 20.0, true, true)]));
    assert!(b.until_on("dac", 3_000));
    b.rig.pass(3_000);
    let before = b.status().position_ms;
    // The lid closes: every stream goes, the DAC powers down; an hour passes with nothing
    // running; the lid opens, the DAC not back yet.
    b.devices.sleep(&["dac"]);
    b.rig.clock.leap(3_600_000_000_000);
    b.devices.wake();
    assert!(b.until_on("speakers", 2_000), "on the fallback");
    let after = b.status().position_ms;
    assert!(
        after.abs_diff(before) < 300,
        "where it was: {before} then {after} ms"
    );
    b.rig.pass(1_000);
    b.devices.set_state("dac", EndpointState::Active);
    assert!(b.until_on("dac", 3_000), "back on the DAC");
    b.rig.pass(1_000);
    b.never_stopped();
    let d = b.finish("sleep and wake", &[0], |_| true);
    assert_eq!(d.probed(), 1, "the DAC's formats are known from before");
}

#[test]
fn the_default_moving_while_followed_moves_the_music_with_a_fade() {
    let mut b = bench(vec![speakers(), dac()], Some("speakers"), None, false);
    b.rig.send(play(vec![song(48_000, 0.0, 10.0, true, true)]));
    assert!(b.until_on("speakers", 3_000));
    b.rig.pass(2_000);
    b.devices.set_default(Some("dac"));
    assert!(
        b.until_on("dac", 1_000),
        "follows at once (it was there all along)"
    );
    assert!(b.status().signal.unwrap().follows_default);
    b.rig.pass(1_000);
    b.never_stopped();
    b.finish("the default moving", &[], |_| true);
}

#[test]
fn a_virtual_device_is_followed_when_windows_chose_it_and_never_picked_as_a_fallback() {
    let mut b = bench(
        vec![mirror(), speakers(), hdmi()],
        Some("mirror"),
        None,
        true,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 3.0, true, true)]));
    assert!(b.until_on("mirror", 3_000));
    let s = b.status();
    let g = s.signal.clone().unwrap();
    assert_eq!(
        (g.kind, g.mode, g.bit_perfect),
        (Some(Kind::Virtual), Mode::Shared, false)
    );
    assert!(steps(&s).iter().any(|t| t.contains("A virtual device")));
    b.rig.pass(300);
    b.finish("a virtual default", &[], |_| true);
    // Chosen HDMI away, the default virtual: the speakers.
    let mut away = hdmi();
    away.endpoint.state = EndpointState::NotPresent;
    let mut b = bench(
        vec![mirror(), speakers(), away],
        Some("mirror"),
        Some("hdmi"),
        true,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 3.0, true, true)]));
    assert!(b.until_on("speakers", 3_000));
    b.rig.pass(300);
    b.finish("a virtual default as fallback", &[], |_| true);
}

#[test]
fn a_chosen_device_that_never_opens_is_passed_over_for_the_fallback() {
    let mut broken = dac();
    broken.fails = true;
    let mut b = bench(
        vec![speakers(), broken],
        Some("speakers"),
        Some("dac"),
        true,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 4.0, true, true)]));
    assert!(b.until_on("speakers", 3_000));
    let g = b.status().signal.unwrap();
    assert_eq!(
        g.note.as_deref(),
        Some("SNOWSKY DISC isn't answering: playing on Speakers until it does.")
    );
    b.rig.pass(500);
    b.never_stopped();
    b.finish("a DAC that never opens", &[], |_| true);
}

// --- The delay per output ----------------------------------------------------------------------

#[test]
fn the_place_reported_is_what_reaches_the_ear_through_the_outputs_delay() {
    let mut b = bench(
        vec![speakers(), dac()],
        Some("speakers"),
        Some("speakers"),
        false,
    );
    b.rig.send(play(vec![song(48_000, 0.0, 20.0, true, true)]));
    assert!(b.until_on("speakers", 3_000));
    b.rig.pass(3_000);
    let p1 = b.status().position_ms;
    b.rig.send(Cmd::OutputDelays(BTreeMap::from([(
        "speakers".to_owned(),
        200,
    )])));
    b.rig.pass(1_000);
    let s = b.status();
    assert_eq!(s.signal.as_ref().unwrap().delay_ms, Some(200));
    let moved = s.position_ms as i64 - p1 as i64;
    assert!(
        (moved - 800).abs() <= 30,
        "a second on, heard 200 ms later: moved {moved} ms"
    );
    // Another output has its own (none set): picked up on the switch.
    b.rig.send(Cmd::Output(OutputChoice::Device {
        id: Some("dac".into()),
        exclusive: false,
    }));
    assert!(b.until_on("dac", 2_000));
    let g = b.status().signal.unwrap();
    assert_eq!((g.delay_ms, g.suggested_delay_ms), (None, None));
    b.rig.pass(500);
    b.never_stopped();
    b.finish("the delay", &[], |_| true);
}
