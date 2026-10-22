//! Engine 2's status thread (sections 3.2 and 4.7 of the Listen together design): it builds the
//! app's `Status` from what the control thread publishes and the renderer's atomics, and calls
//! the app's callback, so the callback never runs on the render thread or the control thread.
//! It also writes the render deadline line to the playback log, once a minute and when an
//! output stops, and reads the spectrum's ring.

use super::{Item, Said, Signal, Status, SyncStatus};
use crate::dsp::Sound;
use crate::output::{DeviceFormat, SampleKind};
use crate::queue::Repeat;
use crate::render::{Deadline, RenderStatus};
use crate::sync::SyncNote;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};

use super::memory::Pacer;

/// A report every this long while anything changes, ns.
const REPORT_NS: u64 = 100_000_000;
/// The deadline line every this long, ns.
const LOG_NS: u64 = 60_000_000_000;
/// Frames the spectrum's ring keeps (mono).
pub(super) const TAP: usize = 4_096;

/// What the control thread publishes for the status thread: `Status` without what the status
/// thread adds (the band levels, how often the output ran dry).
#[derive(Clone)]
pub(super) struct View<T> {
    /// The whole queue, when it changed since the status thread last took it.
    pub queue: Option<Vec<T>>,
    pub unshuffled: Option<Vec<usize>>,
    pub index: Option<usize>,
    pub playing: bool,
    pub buffering: bool,
    pub stalled: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f32,
    pub signal: Option<Signal>,
    pub from: String,
    pub error: String,
    pub sound: Sound,
    pub stop_after_current: bool,
    pub sleep_in_s: Option<u64>,
    pub sync: Option<SyncStatus>,
    /// Band levels are asked for, of sound at this rate.
    pub spectrum: bool,
    pub rate: u32,
}

impl<T> Default for View<T> {
    fn default() -> Self {
        View {
            queue: None,
            unshuffled: None,
            index: None,
            playing: false,
            buffering: false,
            stalled: false,
            position_ms: 0,
            duration_ms: 0,
            shuffle: false,
            repeat: Repeat::Off,
            volume: 1.0,
            signal: None,
            from: String::new(),
            error: String::new(),
            sound: Sound::default(),
            stop_after_current: false,
            sleep_in_s: None,
            sync: None,
            spectrum: false,
            rate: 44_100,
        }
    }
}

/// The renderers whose deadline lines the status thread owes.
#[derive(Default)]
pub(super) struct Renders {
    /// The start running now.
    pub running: Option<Arc<RenderStatus>>,
    /// Starts that ended since the status thread last looked.
    pub ended: Vec<Arc<RenderStatus>>,
}

/// What the control thread and the status thread share (the render thread touches none of it
/// but the spectrum's ring and its switch, which are atomics).
pub(super) struct Shared<T> {
    pub view: Mutex<View<T>>,
    /// The view as each command left it, for its answer.
    pub answers: Mutex<Vec<View<T>>>,
    /// What was done to the music for a protocol 2 room, not yet reported.
    pub sync_notes: Mutex<Vec<SyncNote>>,
    pub common: Arc<Common>,
}

/// The part of what is shared that has nothing of the queue in it (the deck's).
pub(super) struct Common {
    /// The output's `Xrun` events since the player started.
    pub ran_dry: AtomicU64,
    pub tap: Arc<Tap>,
    pub renders: Mutex<Renders>,
    pub quit: AtomicBool,
}

impl<T> Shared<T> {
    pub(super) fn new() -> Self {
        Shared {
            view: Mutex::new(View::default()),
            answers: Mutex::new(Vec::new()),
            sync_notes: Mutex::new(Vec::new()),
            common: Arc::new(Common {
                ran_dry: AtomicU64::new(0),
                tap: Arc::new(Tap::new()),
                renders: Mutex::new(Renders::default()),
                quit: AtomicBool::new(false),
            }),
        }
    }

    pub(super) fn view(&self) -> MutexGuard<'_, View<T>> {
        self.view.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(super) fn answers(&self) -> MutexGuard<'_, Vec<View<T>>> {
        self.answers.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(super) fn sync_notes(&self) -> MutexGuard<'_, Vec<SyncNote>> {
        self.sync_notes.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Common {
    pub(super) fn renders(&self) -> MutexGuard<'_, Renders> {
        self.renders.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The spectrum's ring: the last `TAP` frames the render path made, mono, as `f32` bits, filled
/// (only while `on`) right after the renderer on the render thread, read by the status thread.
pub(super) struct Tap {
    pub on: AtomicBool,
    /// Frames written so far.
    at: AtomicUsize,
    buf: Box<[AtomicU32]>,
}

impl Tap {
    fn new() -> Tap {
        Tap {
            on: AtomicBool::new(false),
            at: AtomicUsize::new(0),
            buf: (0..TAP).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    /// From the render thread: the period just made, in the device's format. Nothing is
    /// allocated, locked or waited for.
    pub(super) fn write(&self, out: &[u8], frames: usize, fmt: &DeviceFormat) {
        if !self.on.load(Ordering::Relaxed) {
            return;
        }
        let ch = usize::from(fmt.channels.max(1));
        let size = usize::from(fmt.container_bits / 8).max(1);
        let mut at = self.at.load(Ordering::Relaxed);
        for f in 0..frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                let i = (f * ch + c) * size;
                let Some(s) = out.get(i..i + size) else {
                    return;
                };
                sum += match (fmt.kind, size) {
                    (SampleKind::Float, _) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    (_, 2) => f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0,
                    (_, 3) => i32::from_le_bytes([0, s[0], s[1], s[2]]) as f32 / 2_147_483_648.0,
                    _ => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                };
            }
            self.buf[at % TAP].store((sum / ch as f32).to_bits(), Ordering::Relaxed);
            at = at.wrapping_add(1);
        }
        self.at.store(at, Ordering::Release);
    }

    /// The newest `n` frames (at most `TAP / 2`), oldest first.
    fn last(&self, n: usize) -> Vec<f32> {
        let n = n.min(TAP / 2);
        loop {
            let end = self.at.load(Ordering::Acquire);
            let k = n.min(end);
            let v: Vec<f32> = (end - k..end)
                .map(|i| f32::from_bits(self.buf[i % TAP].load(Ordering::Relaxed)))
                .collect();
            // Read while it went round less than the rest of the ring: every value read is one
            // of these frames.
            if self.at.load(Ordering::Acquire).wrapping_sub(end) < TAP - n {
                return v;
            }
        }
    }
}

/// The deadline line's bookkeeping for one renderer.
struct Logged {
    render: Arc<RenderStatus>,
    since: Deadline,
    dry: u64,
    at_ns: u64,
}

/// The status thread: until told to quit.
pub(super) fn run<T: Item + Serialize>(
    shared: Arc<Shared<T>>,
    pacer: Pacer,
    wake: Receiver<()>,
    mut on_status: Box<dyn FnMut(Status<T>) + Send>,
) {
    let mut said: Option<Said> = None;
    let mut next_report = pacer.now();
    let mut logged: Option<Logged> = None;
    loop {
        let woken = match pacer.recv_until(&wake, next_report) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let now = pacer.now();
        deadline_lines(&shared.common, &mut logged, now);
        if shared.common.quit.load(Ordering::Acquire) {
            break;
        }
        // Every command's answer, one report each, in order (as engine 1).
        let answers: Vec<View<T>> = shared.answers().drain(..).collect();
        for mut v in answers {
            let mut status = build(&mut v, &shared.common);
            status.sync_notes = std::mem::take(&mut *shared.sync_notes());
            said = Some(Said::of(&status));
            on_status(status);
        }
        if !woken && now < next_report {
            continue;
        }
        next_report = now + REPORT_NS;
        let mut status = build(&mut shared.view(), &shared.common);
        status.sync_notes = std::mem::take(&mut *shared.sync_notes());
        // The regular report, when it would only repeat the last one, is not sent; a new
        // queue always is, and so does what was done to the music for a room.
        let s = Said::of(&status);
        if status.queue.is_none() && status.sync_notes.is_empty() && said.as_ref() == Some(&s) {
            continue;
        }
        said = Some(s);
        on_status(status);
    }
    // What the last renderers owe.
    deadline_lines(&shared.common, &mut logged, pacer.now());
    if let Some(l) = logged.take() {
        line(&l, pacer.now(), &shared.common);
    }
}

fn build<T: Item + Serialize>(v: &mut View<T>, common: &Common) -> Status<T> {
    let bands = (v.spectrum && v.playing)
        .then(|| crate::spectrum::levels(&common.tap.last(crate::spectrum::WINDOW), v.rate));
    Status {
        queue: v.queue.take(),
        index: v.index,
        playing: v.playing,
        buffering: v.buffering,
        stalled: v.stalled,
        position_ms: v.position_ms,
        duration_ms: v.duration_ms,
        shuffle: v.shuffle,
        repeat: v.repeat,
        volume: v.volume,
        signal: v.signal.clone(),
        unshuffled: v.unshuffled.take(),
        from: v.from.clone(),
        error: v.error.clone(),
        sound: v.sound.clone(),
        stop_after_current: v.stop_after_current,
        sleep_in_s: v.sleep_in_s,
        bands,
        sync: v.sync,
        sync_notes: Vec::new(),
        ran_dry: common.ran_dry.load(Ordering::Acquire),
    }
}

/// Write the line of a renderer that stopped, and of the one running when a minute has passed.
fn deadline_lines(shared: &Common, logged: &mut Option<Logged>, now: u64) {
    let (running, ended) = {
        let mut r = shared.renders();
        (r.running.clone(), std::mem::take(&mut r.ended))
    };
    for e in ended {
        if logged.as_ref().is_some_and(|l| Arc::ptr_eq(&l.render, &e)) {
            if let Some(l) = logged.take() {
                line(&l, now, shared);
            }
        } else {
            // Ended before this thread first looked: its whole run.
            line(
                &Logged {
                    render: e,
                    since: Deadline::default(),
                    dry: shared.ran_dry.load(Ordering::Acquire),
                    at_ns: now,
                },
                now,
                shared,
            );
        }
    }
    match (&running, logged.as_ref()) {
        (Some(r), l) if l.is_none_or(|l| !Arc::ptr_eq(&l.render, r)) => {
            *logged = Some(Logged {
                render: r.clone(),
                since: Deadline::default(),
                dry: shared.ran_dry.load(Ordering::Acquire),
                at_ns: now,
            });
        }
        _ => {}
    }
    if let Some(l) = logged.as_mut()
        && now.saturating_sub(l.at_ns) >= LOG_NS
    {
        line(l, now, shared);
        l.since = l.render.read().deadline;
        l.dry = shared.ran_dry.load(Ordering::Acquire);
        l.at_ns = now;
    }
}

/// One playback log line (WP14's format) for what `l`'s renderer did since its last line.
fn line(l: &Logged, now: u64, shared: &Common) {
    let d = l.render.read().deadline.since(&l.since);
    if d.periods() == 0 {
        return;
    }
    let seconds = (now.saturating_sub(l.at_ns) + 500_000_000) / 1_000_000_000;
    let dry = shared.ran_dry.load(Ordering::Acquire).saturating_sub(l.dry);
    media_access::note_timing(&d.to_log_line(seconds, dry));
}
