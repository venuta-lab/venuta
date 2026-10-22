//! The room's measure on this player (design 4.5 and its WP11 notes): each copy's alignment
//! envelope made from the song's start, and two envelopes aligned.
//!
//! [`EnvelopeMaker`] is `together::measure::Envelopes` for any [`Item`]: a thread per song
//! ("venuta-room-envelope") opens the song from its start on the player's shared downloads (so a
//! stream's bytes are the ones its voices read: never a second download), decodes it once,
//! hands over the start's envelope (the first window and the hash) as soon as it is done and
//! the whole song's at the end. Its stop flag ends any wait for bytes; dropping the maker stops
//! every thread, so nothing of a room is left once its session goes. [`Analysis`] is
//! `together::measure::Aligner`: `analysis::align_track`.
//!
//! Platform neutral: Android reuses it as it is.

use crate::analysis::{Enveloper, align_track};
use crate::decode::{ByteWait, DecodeState, Streams, Waiter};
use crate::engine::Item;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use together::align::{Envelope, Track};
use together::measure::{Aligner, Envelopes, Made};

/// Aligns two envelopes by the player's analysis.
pub struct Analysis;

impl Aligner for Analysis {
    fn track(&self, reference: &Envelope, mine: &Envelope) -> Track {
        align_track(reference, mine)
    }
}

/// Where an envelope maker does its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Making {
    /// A thread per song (the app: the session's step never waits for a decode).
    Threads,
    /// At once, on the caller's thread (tests on a simulated clock, whose time must not pass
    /// while a real thread decodes).
    Inline,
}

struct Job {
    made: Arc<Mutex<Made>>,
    stop: Arc<AtomicBool>,
}

/// Makes the envelopes of a person's copies (see the module's head).
pub struct EnvelopeMaker<T> {
    streams: Option<Arc<Streams>>,
    how: Making,
    jobs: HashMap<String, Job>,
    item: PhantomData<fn(T)>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<T: Item> EnvelopeMaker<T> {
    /// Envelopes of songs opened on `streams` (the player's: `Player::streams`), made as `how`
    /// says.
    pub fn new(streams: Option<Arc<Streams>>, how: Making) -> Self {
        EnvelopeMaker {
            streams,
            how,
            jobs: HashMap::new(),
            item: PhantomData,
        }
    }
}

impl<T> Drop for EnvelopeMaker<T> {
    fn drop(&mut self) {
        for j in self.jobs.values() {
            j.stop.store(true, Ordering::Release);
        }
    }
}

/// Decode `item` from its start, once: the start's envelope into `made` as soon as it is done,
/// then the whole song's (a song shorter than its first window: only the whole).
fn make<T: Item>(item: &T, streams: Option<&Streams>, stop: &Arc<AtomicBool>, made: &Mutex<Made>) {
    let opened = match streams {
        Some(s) => item.open_on(s),
        None => item.open(),
    };
    let Ok(mut src) = opened else {
        *lock(made) = Made::Failed;
        return;
    };
    // Bytes not here yet are waited for (no packet cut), until stopped.
    src.set_byte_wait(ByteWait::Wait(Waiter::new(None, stop.clone(), |_| {})));
    let mut buf = Vec::new();
    let mut e: Option<Enveloper> = None;
    let mut said_start = false;
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match src.next(&mut buf) {
            Ok(DecodeState::Samples) => {
                let e = e.get_or_insert_with(|| {
                    Enveloper::new(src.format.rate, src.format.channels)
                });
                e.feed(&buf);
                if !said_start && e.start_done() {
                    said_start = true;
                    *lock(made) = Made::Start(e.so_far());
                }
            }
            Ok(DecodeState::Pending) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Ok(DecodeState::End) => break,
            Err(_) => {
                // Cut short: what was made is all there will be (none of it: it can't be
                // measured).
                *lock(made) = match e {
                    Some(e) if said_start => Made::Whole(e.so_far()),
                    _ => Made::Failed,
                };
                return;
            }
        }
    }
    *lock(made) = match e {
        Some(e) => Made::Whole(e.finish()),
        None => Made::Failed,
    };
}

impl<T: Item> Envelopes<T> for EnvelopeMaker<T> {
    fn of(&mut self, key: &str, item: &T) -> Made {
        if let Some(j) = self.jobs.get(key) {
            return lock(&j.made).clone();
        }
        let job = Job {
            made: Arc::new(Mutex::new(Made::Making)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let (made, stop) = (job.made.clone(), job.stop.clone());
        match self.how {
            Making::Inline => make(item, self.streams.as_deref(), &stop, &made),
            Making::Threads => {
                let (item, streams) = (item.clone(), self.streams.clone());
                let spawned = std::thread::Builder::new()
                    .name("venuta-room-envelope".into())
                    .spawn(move || make(&item, streams.as_deref(), &stop, &made));
                if spawned.is_err() {
                    *lock(&job.made) = Made::Failed;
                }
            }
        }
        let now = lock(&job.made).clone();
        self.jobs.insert(key.to_owned(), job);
        now
    }
}
