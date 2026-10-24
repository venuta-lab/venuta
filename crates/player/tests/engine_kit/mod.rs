//! A bench for engine 2 on a simulated clock the test moves: the engine's every thread, its
//! voices and its outputs (`SimOutput`s) wait on that clock, so a run is the same every time
//! however busy the machine is, and an hour of music plays in seconds. It keeps what each
//! output's DAC played, every renderer note (in frames of all the runs end to end), every
//! status and the thread that reported it.

#![allow(dead_code)]

use player::engine::{Cmd, EngineKind, Item, OutputChoice, Player, SimChoice, Status, Watch};
use player::output_api::{Output, Render};
use player::sim::{SimConfig, SimHandle, SimOutput};
use player::voice_api::RenderNote;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use together::time::{SimClock, Timebase};

pub const R: u32 = 48_000;

/// Device frames for `ms` at 48 kHz.
pub fn ms(ms: u64) -> u64 {
    ms * u64::from(R) / 1000
}

/// What one test hears.
pub struct Rig<T: Item + Serialize> {
    pub clock: SimClock,
    pub player: Option<Player<T>>,
    statuses: Receiver<(Option<String>, Status<T>)>,
    notes: Receiver<RenderNote>,
    pub outputs: Arc<Mutex<Vec<SimHandle>>>,
    pub seen: Vec<(Option<String>, Status<T>)>,
    pub heard_notes: Vec<RenderNote>,
}

/// A simulated output on `clock` for every output the engine asks for, kept in `outputs`.
pub fn choice(clock: &SimClock, outputs: &Arc<Mutex<Vec<SimHandle>>>) -> OutputChoice {
    let (c, o) = (clock.clone(), outputs.clone());
    OutputChoice::Sim(SimChoice {
        clock: clock.clone(),
        output: Arc::new(move || {
            let (out, handle) = SimOutput::new(Arc::new(c.clone()), SimConfig::default());
            o.lock().unwrap().push(handle);
            Box::new(out) as Box<dyn Output>
        }),
    })
}

impl<T: Item + Serialize> Rig<T> {
    pub fn new() -> Rig<T> {
        Rig::with_wrap(None)
    }

    /// The render path handed to every output wrapped by `wrap` (a counting allocator's
    /// switch).
    #[allow(clippy::type_complexity)]
    pub fn with_wrap(
        wrap: Option<Box<dyn Fn(Box<dyn Render>) -> Box<dyn Render> + Send>>,
    ) -> Rig<T> {
        let clock = SimClock::new(1_000_000_000);
        let outputs = Arc::new(Mutex::new(Vec::new()));
        let (tx, statuses) = channel();
        let (notes_tx, notes) = channel();
        let player = Player::spawn_watched(
            EngineKind::Two,
            choice(&clock, &outputs),
            move |s| {
                let name = std::thread::current().name().map(str::to_owned);
                let _ = tx.send((name, s));
            },
            Watch {
                notes: Some(notes_tx),
                wrap,
            },
        );
        Rig {
            clock,
            player: Some(player),
            statuses,
            notes,
            outputs,
            seen: Vec::new(),
            heard_notes: Vec::new(),
        }
    }

    /// A rig on `clock` playing to `choice` (simulated sound devices, say): what it played is
    /// read from the choice's own outputs, not `outputs`.
    pub fn with_choice(clock: SimClock, choice: OutputChoice) -> Rig<T> {
        let (tx, statuses) = channel();
        let (notes_tx, notes) = channel();
        let player = Player::spawn_watched(
            EngineKind::Two,
            choice,
            move |s| {
                let name = std::thread::current().name().map(str::to_owned);
                let _ = tx.send((name, s));
            },
            Watch {
                notes: Some(notes_tx),
                wrap: None,
            },
        );
        Rig {
            clock,
            player: Some(player),
            statuses,
            notes,
            outputs: Arc::new(Mutex::new(Vec::new())),
            seen: Vec::new(),
            heard_notes: Vec::new(),
        }
    }

    pub fn send(&self, cmd: Cmd<T>) {
        self.player.as_ref().expect("playing").send(cmd);
    }

    /// Let `ms` of simulated time pass, keeping what was said meanwhile.
    pub fn pass(&mut self, ms: u64) {
        for _ in 0..ms {
            self.clock.pass(1_000_000);
        }
        self.collect();
    }

    fn collect(&mut self) {
        self.seen.extend(self.statuses.try_iter());
        self.heard_notes.extend(self.notes.try_iter());
    }

    /// The newest status.
    pub fn status(&mut self) -> &Status<T> {
        self.collect();
        &self.seen.last().expect("a status").1
    }

    /// Let time pass until `f` holds of a status (at most `ms`).
    pub fn until(&mut self, ms: u64, f: impl Fn(&Status<T>) -> bool) -> bool {
        for _ in 0..ms {
            self.pass(1);
            if self.seen.last().is_some_and(|(_, s)| f(s)) {
                return true;
            }
        }
        false
    }

    /// Quit the player (its last fade plays out) and give what it played: channel `c` of
    /// every output's runs, one after the other, as left-justified samples.
    pub fn finish(&mut self) -> [Vec<i32>; 2] {
        if let Some(p) = self.player.take() {
            drop(p);
        }
        self.clock.pass(500_000_000);
        self.collect();
        let outs = self.outputs.lock().unwrap();
        let mut heard = [Vec::new(), Vec::new()];
        for o in outs.iter() {
            for run in o.tape().runs {
                assert!(run.dry.is_empty(), "the simulated DAC never ran dry");
                for (c, h) in heard.iter_mut().enumerate() {
                    h.extend(run.left_justified(c as u16));
                }
            }
        }
        heard
    }
}

/// A test song in a folder of its own: `secs` of the test music from `from_s` seconds of the
/// endless piece, 24-bit stereo at 48 kHz, rising from silence over its first 50 ms (`rise`)
/// and falling to it over its last (`fall`), as songs do.
pub fn song(from_s: f64, secs: f64, rise: bool, fall: bool) -> PathBuf {
    let piece = audio_check::music::Piece {
        rate: R,
        channels: 2,
    };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = player::test_support::temp_dir().join(format!(
        "venuta-engine2-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("song.wav");
    piece.write(
        &f,
        (from_s * f64::from(R)) as usize,
        (secs * f64::from(R)) as usize,
        rise,
        fall,
    );
    f
}

pub fn play<T>(items: Vec<T>) -> Cmd<T> {
    Cmd::Set {
        items,
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    }
}
