//! Engine 2's clocks and its memory outputs (the WP4 notes in 9.2 of the Listen together
//! design).
//!
//! Every thread of engine 2 waits on one `Timebase`: real time for a sound device and the paced
//! memory output, a simulated clock for the fast memory output (moved by a driver thread here,
//! as fast as the threads' work allows and at most `SPEED` times real time) and for a test's
//! simulated output (moved by the test). The memory outputs are `SimOutput`s that fill
//! `output::Captured` as engine 1's memory device does: the formats opened, the frames per open,
//! and the bytes as the renderer made them, every start's run after the one before.

use crate::output::{Captured, Mode};
use crate::output_api::{
    OpenRequest, Output, OutputError, OutputEvent, OutputInfo, Presentation, Render,
};
use crate::sim::{SimConfig, SimOutput};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use together::time::{MonoClock, RealClock, SimClock, Sleeper, Timebase};

/// The fast memory output's clock runs at most this many times real time.
const SPEED: u64 = 32;
/// How often a thread on a simulated clock looks at its channels while it waits, ns.
const POLL_NS: u64 = 1_000_000;

/// How one of engine 2's threads waits: on real time (a channel's own timeout), or on a
/// simulated clock's sleeper (looking at the channel between short sleeps).
pub(super) enum Pacer {
    Real,
    Virtual(Sleeper),
}

impl Pacer {
    pub(super) fn now(&self) -> u64 {
        match self {
            Pacer::Real => RealClock.now_ns(),
            Pacer::Virtual(s) => s.now_ns(),
        }
    }

    /// The next message on `rx`, waiting until `until_ns` at most.
    pub(super) fn recv_until<M>(
        &self,
        rx: &Receiver<M>,
        until_ns: u64,
    ) -> Result<M, RecvTimeoutError> {
        match self {
            Pacer::Real => {
                let left = until_ns.saturating_sub(RealClock.now_ns());
                rx.recv_timeout(Duration::from_nanos(left))
            }
            Pacer::Virtual(s) => loop {
                match rx.try_recv() {
                    Ok(m) => return Ok(m),
                    Err(TryRecvError::Disconnected) => {
                        return Err(RecvTimeoutError::Disconnected);
                    }
                    Err(TryRecvError::Empty) => {}
                }
                let now = s.now_ns();
                if now >= until_ns {
                    return Err(RecvTimeoutError::Timeout);
                }
                s.sleep_until((now + POLL_NS).min(until_ns));
            },
        }
    }
}

/// Moves the fast memory output's simulated clock: as fast as the threads on it allow, and at
/// most `SPEED` times real time (so a test's waits in real time still mean something).
pub(super) struct Driver {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Driver {
    pub(super) fn start(clock: SimClock) -> Driver {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("venuta-sim-clock".into())
            .spawn(move || {
                let (t0, v0) = (Instant::now(), clock.now_ns());
                while !s.load(Ordering::Acquire) {
                    let target = v0 + t0.elapsed().as_nanos() as u64 * SPEED;
                    let now = clock.now_ns();
                    if now < target {
                        clock.run_until(target.min(now + 2 * POLL_NS));
                    } else {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
                // Long enough for every thread told to stop to see it and leave.
                for _ in 0..2_000 {
                    clock.pass(POLL_NS);
                }
            })
            .expect("start the simulated clock");
        Driver {
            stop,
            thread: Some(thread),
        }
    }

    /// Stop, once every thread of the engine that waits on the clock is gone or told to go.
    pub(super) fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A memory output: a simulated device (`SimOutput`) whose renders are kept in `Captured`.
pub(super) struct MemoryOutput {
    sim: SimOutput,
    captured: Arc<Mutex<Captured>>,
    /// Real time (the paced memory output): it takes the capture's open delay, and says when
    /// each render is heard.
    paced: bool,
    info: Option<OutputInfo>,
}

impl MemoryOutput {
    pub(super) fn new(tb: Arc<dyn Timebase>, captured: Arc<Mutex<Captured>>, paced: bool) -> Self {
        let drift = captured.lock().map(|c| c.drift_ppm).unwrap_or(0.0);
        let (sim, _handle) = SimOutput::new(
            tb,
            SimConfig {
                skew_ppm: drift,
                keep_tape: false,
                ..SimConfig::default()
            },
        );
        MemoryOutput {
            sim,
            captured,
            paced,
            info: None,
        }
    }
}

impl Output for MemoryOutput {
    fn name(&self) -> String {
        "Memory".into()
    }

    fn mode(&self) -> Mode {
        Mode::Memory
    }

    fn open(&mut self, want: &OpenRequest) -> Result<OutputInfo, OutputError> {
        let delay = self.captured.lock().ok().and_then(|c| c.open_delay);
        if self.paced
            && let Some(d) = delay
        {
            std::thread::sleep(d);
        }
        let info = self.sim.open(want)?;
        if let Ok(mut c) = self.captured.lock() {
            c.formats.push(info.format.clone());
            c.frames.push(0);
        }
        self.info = Some(info.clone());
        Ok(info)
    }

    fn start(&mut self, render: Box<dyn Render>) -> Result<(), OutputError> {
        let info = self
            .info
            .clone()
            .ok_or_else(|| OutputError::State("start before open".into()))?;
        self.sim.start(Box::new(Capture {
            inner: render,
            captured: self.captured.clone(),
            bpf: info.format.bytes_per_frame(),
            rate: info.format.rate.max(1),
            latency: info.buffer_frames,
            paced: self.paced,
            began: None,
        }))
    }

    fn stop(&mut self) -> Option<Box<dyn Render>> {
        self.sim.stop()
    }

    fn close(&mut self) {
        self.sim.close();
        self.info = None;
    }

    fn presentation(&self) -> Arc<Presentation> {
        self.sim.presentation()
    }

    fn events(&self) -> Receiver<OutputEvent> {
        self.sim.events()
    }

    fn latency(&self) -> Option<crate::output_api::Latency> {
        self.sim.latency()
    }
}

/// The render path of a memory output, kept: on the simulated device's own thread (a test's,
/// never a sound device's), so it may lock and grow the capture.
struct Capture {
    inner: Box<dyn Render>,
    captured: Arc<Mutex<Captured>>,
    bpf: usize,
    rate: u32,
    latency: u32,
    paced: bool,
    /// A paced output: when this start's first frame was rendered.
    began: Option<Instant>,
}

impl Render for Capture {
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64) {
        self.inner.render(out, frames, at);
        let n = (frames as usize * self.bpf).min(out.len());
        let Ok(mut c) = self.captured.lock() else {
            return;
        };
        let from = c.bytes.len();
        c.bytes.extend_from_slice(&out[..n]);
        if let Some(f) = c.frames.last_mut() {
            *f += frames as usize;
        }
        if self.paced {
            let began = *self.began.get_or_insert_with(Instant::now);
            c.started_at.get_or_insert(began);
            // Heard once the buffer before it has played.
            let frame = at + u64::from(self.latency);
            c.heard.push((
                from,
                began + Duration::from_secs_f64(frame as f64 / f64::from(self.rate)),
            ));
        }
    }
}
