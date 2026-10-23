//! Each person's copy measured against the room's reference (design 4.5 and its WP11 notes in
//! 9.2): the host shelves the envelope of every song's reference copy for the `env:` stream; a
//! guest fetches it, makes the envelope of the copy it found (from the song's start, on a
//! thread of its own: the platform's `Envelopes`), aligns the two (`Aligner`, the player's
//! analysis) and gives the conductor the copy with its alignment.
//!
//! [`Measured`] is a `Copies` around the app's copy finder, so the conductor needs nothing new
//! to wait for a measure: it says `Looking` until the measure is in or the conductor's time for
//! it is up (`Copies::find_by`), then the copy, approximate when the measure isn't in. Through
//! the song, `Copies::refined` gives the copy's alignment as each later window found it.
//!
//! Pure and single-threaded like the sessions: nothing here blocks. A guest's fetches run on the
//! app's runtime when it gives one (`Spawn`), else they are polled each step (the simulation).

use crate::align::{
    Alignment, CopyClass, Envelope, MAX_ENV_BYTES, Reference, Track, Verdict, decode_reference,
    encode_reference,
};
use crate::conductor::{Copies, Copy, Lookup};
use crate::proto::Song;
use crate::proto::v3::Entry;
use crate::room_player::ListenMode;
use crate::transport::{BoxFuture, Get, Gone, PeerId, Transport};
use crate::view::CopyKind;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Waker};

/// A reference the host hasn't shelved yet is asked for again this long after, ns.
pub const ASK_AGAIN_NS: u64 = 250_000_000;
/// A reference of the start only is asked for again (for the whole song) this often, ns.
pub const WHOLE_AGAIN_NS: u64 = 5_000_000_000;
/// "The host has no copy" is asked again this often (its look-up may find one later), ns.
pub const NO_COPY_AGAIN_NS: u64 = 10_000_000_000;
/// An offset that moves at most this far is taken up by rate (a glide); further is never
/// jumped to for alignment alone, µs.
pub const GLIDE_US: i64 = 20_000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What making a copy's envelope has come to.
#[derive(Debug, Clone, PartialEq)]
pub enum Made {
    /// Under way: nothing yet.
    Making,
    /// The first window and the PCM hash (the first 30 s decoded); the rest under way.
    Start(Envelope),
    /// The whole song.
    Whole(Envelope),
    /// The copy couldn't be decoded from its start.
    Failed,
}

/// Makes the envelopes of a person's copies from their start: the platform's (the player's
/// `EnvelopeMaker`, a thread per song). Dropped: everything under way stops.
pub trait Envelopes<I>: Send {
    /// Start making the envelope of `item` (known by `key`), or say how far it has come.
    /// Never blocks.
    fn of(&mut self, key: &str, item: &I) -> Made;
}

/// Aligns two envelopes (the player's `analysis::align_track`).
pub trait Aligner: Send + Sync {
    fn track(&self, reference: &Envelope, mine: &Envelope) -> Track;
}

/// Runs a future to its end on the app's runtime (a guest's fetches); without one they are
/// polled on each step.
pub type Spawn = Arc<dyn Fn(BoxFuture<'static, ()>) + Send + Sync>;

// --- The host's shelf ------------------------------------------------------------------------

#[derive(Default)]
struct Shelved {
    bodies: HashMap<String, (Arc<[u8]>, bool)>,
    asked: HashSet<String>,
}

/// The host's references as made so far, as the `env:` stream serves them: shared by the
/// host's measure (which shelves) and its session's media (which serves, to members only).
#[derive(Clone, Default)]
pub struct Shelf(Arc<Mutex<Shelved>>);

impl Shelf {
    pub fn new() -> Shelf {
        Shelf::default()
    }

    /// Shelve `key`'s reference (a start never replaces a whole one).
    pub fn put(&self, key: &str, r: &Reference) {
        let whole = matches!(r, Reference::Whole(_) | Reference::NoCopy);
        let mut s = lock(&self.0);
        if !whole && s.bodies.get(key).is_some_and(|b| b.1) {
            return;
        }
        s.bodies
            .insert(key.to_owned(), (encode_reference(r).into(), whole));
        s.asked.remove(key);
    }

    /// The `env:` body for `key`, or nothing yet (the asker comes back); an ask for one not
    /// made is remembered, for the host to make it.
    pub fn serve(&self, key: &str) -> Option<Arc<[u8]>> {
        let mut s = lock(&self.0);
        match s.bodies.get(key) {
            Some(b) => Some(b.0.clone()),
            None => {
                s.asked.insert(key.to_owned());
                None
            }
        }
    }

    /// Whether `key` has a whole reference (or "no copy") shelved.
    pub fn done(&self, key: &str) -> bool {
        lock(&self.0).bodies.get(key).is_some_and(|b| b.1)
    }

    fn asked(&self) -> Vec<String> {
        lock(&self.0).asked.iter().cloned().collect()
    }
}

// --- A guest's fetches -------------------------------------------------------------------------

type Slot = Arc<Mutex<Option<Result<Reference, Gone>>>>;

enum Got {
    Asking {
        slot: Slot,
        /// Polled here when nothing runs it (no `Spawn`).
        poll: Option<BoxFuture<'static, ()>>,
        had: Option<Reference>,
    },
    Have {
        r: Reference,
        at_ns: u64,
    },
    /// Not shelved yet: asked again at `at_ns`.
    Again {
        at_ns: u64,
        had: Option<Reference>,
    },
}

/// A guest's fetches of the host's references over the transport (`env:<key>`).
pub struct References {
    t: Arc<dyn Transport>,
    host: PeerId,
    spawn: Option<Spawn>,
    got: HashMap<String, Got>,
}

async fn fetch(t: Arc<dyn Transport>, host: PeerId, key: String) -> Result<Reference, Gone> {
    let get = Get {
        get: format!("env:{key}"),
        from: 0,
        to: None,
    };
    let mut m = t.open_media(host, get).await?;
    let mut body = Vec::new();
    while let Some(c) = m.body.read_chunk().await? {
        body.extend_from_slice(&c);
        if body.len() > MAX_ENV_BYTES {
            return Ok(Reference::NoCopy);
        }
    }
    // A body that isn't a reference says nothing to measure against.
    Ok(decode_reference(&body).unwrap_or(Reference::NoCopy))
}

impl References {
    /// Fetches from `host` over `t`, run by `spawn` when given.
    pub fn new(t: Arc<dyn Transport>, host: PeerId, spawn: Option<Spawn>) -> References {
        References {
            t,
            host,
            spawn,
            got: HashMap::new(),
        }
    }

    fn ask(&self, key: &str, had: Option<Reference>) -> Got {
        let slot: Slot = Arc::default();
        let (s2, t, host, k) = (slot.clone(), self.t.clone(), self.host, key.to_owned());
        let f: BoxFuture<'static, ()> = Box::pin(async move {
            let r = fetch(t, host, k).await;
            *lock(&s2) = Some(r);
        });
        let poll = match &self.spawn {
            Some(spawn) => {
                spawn(f);
                None
            }
            None => Some(f),
        };
        Got::Asking { slot, poll, had }
    }

    /// The host's reference for `key` as fetched so far (asking, and asking again, as the rules
    /// say); `whole`: the whole song's is wanted.
    pub fn get(&mut self, key: &str, now_ns: u64, whole: bool) -> Option<Reference> {
        let state = match self.got.remove(key) {
            None => self.ask(key, None),
            Some(Got::Asking {
                slot,
                mut poll,
                had,
            }) => {
                if let Some(f) = poll.as_mut() {
                    let _ = f.as_mut().poll(&mut Context::from_waker(Waker::noop()));
                }
                let done = lock(&slot).take();
                match done {
                    Some(Ok(r)) => Got::Have { r, at_ns: now_ns },
                    Some(Err(Gone)) => Got::Again {
                        at_ns: now_ns + ASK_AGAIN_NS,
                        had,
                    },
                    None => Got::Asking { slot, poll, had },
                }
            }
            Some(Got::Have { r, at_ns }) => {
                let again = match &r {
                    Reference::Start(_) => whole && now_ns >= at_ns + WHOLE_AGAIN_NS,
                    Reference::NoCopy => now_ns >= at_ns + NO_COPY_AGAIN_NS,
                    Reference::Whole(_) => false,
                };
                if again {
                    self.ask(key, Some(r))
                } else {
                    Got::Have { r, at_ns }
                }
            }
            Some(Got::Again { at_ns, had }) if now_ns >= at_ns => self.ask(key, had),
            Some(g) => g,
        };
        let out = match &state {
            Got::Have { r, .. } => Some(r.clone()),
            Got::Asking { had, .. } | Got::Again { had, .. } => had.clone(),
        };
        self.got.insert(key.to_owned(), state);
        out
    }
}

// --- The measure ----------------------------------------------------------------------------

enum Side<I> {
    /// The host: its copies are the room's references; it shelves their envelopes.
    Host {
        shelf: Shelf,
        /// References under way: the copy whose envelope is being made, by key.
        making: HashMap<String, I>,
    },
    /// A guest: measures its copies against the host's references.
    Guest {
        refs: References,
        aligner: Arc<dyn Aligner>,
    },
}

/// One copy's measure (a guest's), by song and kind of copy.
struct Measure<I> {
    item: I,
    /// What `find` gave last: the alignment the player got.
    given: Option<Alignment>,
    /// It gave the copy before the measure was in.
    gave_up: bool,
    /// The start's alignment, once measured.
    start: Option<Alignment>,
    /// Nothing to measure against for now (the host has no copy, E6), or nothing of mine to
    /// measure (it can't be decoded from its start).
    nothing: bool,
    /// The whole song's comparison, once both envelopes are whole.
    track: Option<Track>,
    /// Nothing more to come: the whole song compared, or nothing to compare.
    over: bool,
}

/// A person's copies, measured (see the module's head).
pub struct Measured<I> {
    inner: Box<dyn Copies<I>>,
    side: Side<I>,
    envelopes: Box<dyn Envelopes<I>>,
    measures: HashMap<(String, CopyKind), Measure<I>>,
}

impl<I: Clone + Send + 'static> Measured<I> {
    /// The host's: every copy it finds is the room's reference, its envelope shelved on `shelf`
    /// (and made when a guest asks for one not made yet).
    pub fn host(inner: Box<dyn Copies<I>>, envelopes: Box<dyn Envelopes<I>>, shelf: Shelf) -> Self {
        Measured {
            inner,
            side: Side::Host {
                shelf,
                making: HashMap::new(),
            },
            envelopes,
            measures: HashMap::new(),
        }
    }

    /// A guest's: copies measured against the references fetched by `refs`, aligned by
    /// `aligner`.
    pub fn guest(
        inner: Box<dyn Copies<I>>,
        envelopes: Box<dyn Envelopes<I>>,
        refs: References,
        aligner: Arc<dyn Aligner>,
    ) -> Self {
        Measured {
            inner,
            side: Side::Guest { refs, aligner },
            envelopes,
            measures: HashMap::new(),
        }
    }

    /// The host shelves what making `key`'s envelope has come to; true once it is done.
    fn shelve(shelf: &Shelf, envelopes: &mut dyn Envelopes<I>, key: &str, item: &I) -> bool {
        match envelopes.of(key, item) {
            Made::Making => false,
            Made::Start(e) => {
                shelf.put(key, &Reference::Start(e));
                false
            }
            Made::Whole(e) => {
                shelf.put(key, &Reference::Whole(e));
                true
            }
            Made::Failed => {
                shelf.put(key, &Reference::NoCopy);
                true
            }
        }
    }

    /// The host found its copy of `key`: its envelope made and shelved.
    fn host_found(&mut self, key: &str, item: &I) {
        let Side::Host { shelf, making } = &mut self.side else {
            return;
        };
        if shelf.done(key) {
            return;
        }
        if Self::shelve(shelf, self.envelopes.as_mut(), key, item) {
            making.remove(key);
        } else {
            making.insert(key.to_owned(), item.clone());
        }
    }

    /// A guest's measure of `key` (copy `kind`), brought on as far as it can be now.
    fn advance(&mut self, key: &str, kind: CopyKind, now_ns: u64) {
        let Side::Guest { refs, aligner } = &mut self.side else {
            return;
        };
        let Some(m) = self.measures.get_mut(&(key.to_owned(), kind)) else {
            return;
        };
        if m.over {
            return;
        }
        let mine = self.envelopes.of(key, &m.item);
        let whole_mine = matches!(mine, Made::Whole(_));
        let reference = refs.get(key, now_ns, whole_mine);
        match (&reference, &mine) {
            // Nothing to measure against (the host has no copy, E6), or nothing of mine to
            // measure (it can't be decoded from its start): approximate.
            (Some(Reference::NoCopy), _) | (_, Made::Failed) => {
                m.nothing = true;
                m.over = matches!(mine, Made::Failed);
            }
            (
                Some(r @ (Reference::Start(e) | Reference::Whole(e))),
                Made::Start(me) | Made::Whole(me),
            ) => {
                m.nothing = false;
                if m.start.is_none() {
                    // The start's measure is the first window's (and the hash): an edit later in
                    // the song turns the copy approximate where it shows, not from the start.
                    m.start = Some(aligner.track(&first(e), &first(me)).alignment);
                }
                if whole_mine && matches!(r, Reference::Whole(_)) {
                    m.track = Some(aligner.track(e, me));
                    m.over = true;
                }
            }
            _ => {}
        }
    }
}

impl<I: Clone + Send + 'static> Copies<I> for Measured<I> {
    fn find(&mut self, song: &Song) -> Lookup<I> {
        self.find_by(song, 0, 0)
    }

    fn find_by(&mut self, song: &Song, now_ns: u64, by_ns: u64) -> Lookup<I> {
        let found = self.inner.find_by(song, now_ns, by_ns);
        let host_shelf = match &self.side {
            Side::Host { shelf, .. } => Some(shelf.clone()),
            Side::Guest { .. } => None,
        };
        if let Some(shelf) = host_shelf {
            return match found {
                Lookup::Found(c) => {
                    self.host_found(&song.key, &c.item);
                    // The host's copy is the room's reference, whatever it is.
                    Lookup::Found(Copy {
                        align: Alignment::EXACT,
                        ..c
                    })
                }
                Lookup::NoCopy => {
                    shelf.put(&song.key, &Reference::NoCopy);
                    Lookup::NoCopy
                }
                Lookup::Looking => Lookup::Looking,
            };
        }
        let Lookup::Found(c) = found else {
            return found;
        };
        if c.align.class == CopyClass::Exact {
            // The host's own bytes: nothing to measure.
            return Lookup::Found(c);
        }
        let k = (song.key.clone(), c.kind);
        self.measures.entry(k.clone()).or_insert_with(|| Measure {
            item: c.item.clone(),
            given: None,
            gave_up: false,
            start: None,
            nothing: false,
            track: None,
            over: false,
        });
        self.advance(&song.key, c.kind, now_ns);
        let m = self.measures.get_mut(&k).expect("made above");
        let align = match m.start {
            Some(s) => {
                m.gave_up = false;
                s
            }
            // Nothing to measure against, or the time for it up: approximate, at offset 0;
            // a measure that comes in later is taken within a glide (`refined`) or next time.
            None if m.nothing || now_ns >= by_ns => {
                m.gave_up = true;
                Alignment::APPROXIMATE
            }
            None => return Lookup::Looking,
        };
        m.given = Some(align);
        Lookup::Found(Copy { align, ..c })
    }

    fn lighter(&mut self) {
        self.inner.lighter();
    }

    fn prefer_host(&mut self, key: &str) {
        self.inner.prefer_host(key);
    }

    fn set_mode(&mut self, mode: ListenMode) {
        self.inner.set_mode(mode);
    }

    fn refined(&mut self, song: &Song, kind: CopyKind, room_us: i64) -> Option<Alignment> {
        at_place(self.measures.get(&(song.key.clone(), kind))?, room_us)
    }

    fn step(&mut self, queue: &[Entry], now_ns: u64) {
        self.inner.step(queue, now_ns);
        if let Side::Host { shelf, making } = &mut self.side {
            // References guests asked for that aren't under way: their copies looked for.
            for key in shelf.asked() {
                if making.contains_key(&key) {
                    continue;
                }
                let Some(song) = queue.iter().find(|e| e.song.key == key).map(|e| &e.song) else {
                    continue;
                };
                match self.inner.find(song) {
                    Lookup::Found(c) => {
                        making.insert(key, c.item);
                    }
                    Lookup::NoCopy => shelf.put(&key, &Reference::NoCopy),
                    Lookup::Looking => {}
                }
            }
            let under_way: Vec<(String, I)> =
                making.iter().map(|(k, i)| (k.clone(), i.clone())).collect();
            for (key, item) in under_way {
                if Self::shelve(shelf, self.envelopes.as_mut(), &key, &item) {
                    making.remove(&key);
                }
            }
            return;
        }
        // A guest's measures still coming in (the whole song's, a late start).
        let open: Vec<(String, CopyKind)> = self
            .measures
            .iter()
            .filter(|(_, m)| !m.over)
            .map(|(k, _)| k.clone())
            .collect();
        for (key, kind) in open {
            self.advance(&key, kind, now_ns);
        }
    }
}

/// A copy's alignment as known for the room's place `room_us`: what was given; a start
/// measured late, when within a glide of it; then each later window up to the place, in song
/// order: agreeing within a glide moves the offset, an edit makes the copy approximate from
/// there (the offset stays: aligned at the start as well as can be, never jumped).
fn at_place<I>(m: &Measure<I>, room_us: i64) -> Option<Alignment> {
    let mut a = m.given?;
    if m.gave_up
        && let Some(s) = m.start
        && s.class != CopyClass::Approximate
        && (s.offset_us - a.offset_us).abs() <= GLIDE_US
    {
        a = s;
    }
    let Some(t) = m
        .track
        .as_ref()
        .filter(|_| a.class != CopyClass::Approximate)
    else {
        return Some(a);
    };
    for w in &t.windows {
        if i64::from(w.start_ms) * 1_000 > room_us {
            break;
        }
        match w.verdict {
            Verdict::Agrees { offset_us }
                if a.class == CopyClass::Measured
                    && (offset_us - a.offset_us).abs() <= GLIDE_US =>
            {
                a.offset_us = offset_us;
            }
            Verdict::Moved { .. } | Verdict::Lost => {
                a.class = CopyClass::Approximate;
                a.confidence = 0.0;
                break;
            }
            _ => {}
        }
    }
    Some(a)
}

/// An envelope's first window and hash alone: what the start is measured on.
fn first(e: &Envelope) -> Envelope {
    Envelope {
        hop_ms: e.hop_ms,
        windows: e.windows.iter().filter(|w| w.0 == 0).cloned().collect(),
        pcm_hash: e.pcm_hash,
    }
}
