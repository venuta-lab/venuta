//! Whole rooms in one process: the room itself (`together::room::Room`, as the host keeps it),
//! a follower for the host and one for each guest (the real `follow::Follower`), each driving a
//! real player on a device that takes sound at real-time pace (the `Seat` a check gives it), and
//! between them a network that delays every message, a little differently each time, in order
//! (as QUIC delivers on one stream), slower one way than the other if asked. Guests learn the
//! host's clock from pings over it, as `guest.rs` does.
//!
//! Same Wi-Fi stands as a millisecond or so each way; the internet as tens of ms with jitter and
//! a different delay each way. Songs are counting WAVs (each frame says which frame of which
//! song it is), so what each player's device heard shows every song played, every frame skipped
//! or repeated, and, with the device's own clock, when each was heard.

use super::follow::{self, Follower, In, Report, Seat};
use crate::playback::{QueueItem, RoomTick};
use media_access::{MediaAccess, PlayableId, UnconfiguredOnlineAccess};
use player::engine::{Cmd, OutputChoice, Player, Status};
use player::output::Captured;
use player::sync::SyncPlan;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use together::clock::Sample;
use together::proto::{Action, Hearing, RoomView, Song};
use together::room::{HOST, Knock, Room};

pub const RATE: u32 = 48_000;

/// A mono 24-bit WAV whose frame i is `base + i + 1`.
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

/// A room song: its catalog id (11 characters), its counting file, where its frames count from.
#[derive(Clone)]
pub struct TestSong {
    pub id: String,
    pub file: PathBuf,
    pub base: i32,
    pub frames: usize,
}

impl TestSong {
    pub fn song(&self) -> Song {
        Song {
            key: format!("online:{}", self.id),
            title: format!("Song {}", self.id),
            artists: vec!["Tester".into()],
            album: None,
            duration_ms: Some(self.frames as u64 * 1000 / u64::from(RATE)),
            isrc: None,
            mbid: None,
            online_id: Some(self.id.clone()),
            host_file: false,
            format: None,
            artwork_url: None,
            added_by: None,
        }
    }

    /// Whether a value heard is one of this song's frames (which one, from 0).
    pub fn frame_of(&self, v: i32) -> Option<usize> {
        let k = v - self.base - 1;
        (k >= 0 && (k as usize) < self.frames).then_some(k as usize)
    }
}

/// `n` songs of `secs` seconds each, in a folder of their own.
pub fn songs(name: &str, n: usize, secs: f64) -> Vec<TestSong> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "venuta-roomcheck-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    (0..n)
        .map(|k| {
            let frames = (f64::from(RATE) * secs) as usize;
            let base = 1_000_000 * (k as i32 + 1);
            let file = dir.join(format!("{k}.wav"));
            wav(&file, frames, base);
            TestSong {
                id: format!("song{k:07}"),
                file,
                base,
                frames,
            }
        })
        .collect()
}

// --- A player a room can take --------------------------------------------------------------

/// A real player on a paced memory device, for a follower to drive: it plays the counting file
/// for each room song, and gives back "your own" song when the room lets go.
pub struct TestSeat {
    player: Mutex<Option<Player<PathBuf>>>,
    pub captured: Arc<Mutex<Captured>>,
    files: HashMap<String, PathBuf>,
    tap: Arc<Mutex<Option<Sender<RoomTick>>>>,
    owner: AtomicU64,
    next: AtomicU64,
    pub notes: Mutex<Vec<String>>,
    pub released: AtomicBool,
    pub last: Arc<Mutex<Option<Status<PathBuf>>>>,
    /// Your own song, given back when the room lets go (paused, at 1 s).
    pub own: PathBuf,
    /// Plans the follower sent, with when.
    pub plans: Mutex<Vec<(Instant, Option<SyncPlan>)>>,
}

impl TestSeat {
    pub fn new(songs: &[TestSong], own: PathBuf, drift_ppm: f64) -> Arc<TestSeat> {
        let captured = Arc::new(Mutex::new(Captured {
            drift_ppm,
            ..Captured::default()
        }));
        let tap: Arc<Mutex<Option<Sender<RoomTick>>>> = Arc::default();
        let last: Arc<Mutex<Option<Status<PathBuf>>>> = Arc::default();
        let (t, l) = (tap.clone(), last.clone());
        let player = Player::spawn(
            OutputChoice::PacedMemory(captured.clone()),
            move |s: Status<PathBuf>| {
                if let Some(tx) = t.lock().unwrap().as_ref() {
                    let _ = tx.send(RoomTick {
                        index: s.index,
                        duration_ms: s.duration_ms,
                        sync: s.sync,
                        error: !s.error.is_empty(),
                        idle: false,
                    });
                }
                *l.lock().unwrap() = Some(s);
            },
        );
        Arc::new(TestSeat {
            player: Mutex::new(Some(player)),
            captured,
            files: songs
                .iter()
                .map(|s| (s.id.clone(), s.file.clone()))
                .collect(),
            tap,
            owner: AtomicU64::new(0),
            next: AtomicU64::new(1),
            notes: Mutex::default(),
            released: AtomicBool::new(false),
            last,
            own,
            plans: Mutex::default(),
        })
    }

    pub fn send(&self, c: Cmd<PathBuf>) {
        if let Some(p) = self.player.lock().unwrap().as_ref() {
            p.send(c);
        }
    }

    fn file_of(&self, item: &QueueItem) -> Option<PathBuf> {
        match &item.online {
            Some(PlayableId::Online { track_id, .. }) => self.files.get(track_id).cloned(),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<Status<PathBuf>> {
        self.last.lock().unwrap().clone()
    }

    /// Stop the player, and what its device heard.
    pub fn finish(&self) -> Captured {
        drop(self.player.lock().unwrap().take());
        std::thread::sleep(Duration::from_millis(30));
        std::mem::take(&mut *self.captured.lock().unwrap())
    }

    /// Everything heard so far, as 24-bit values.
    pub fn heard(&self) -> Vec<i32> {
        values(&self.captured.lock().unwrap().bytes)
    }
}

pub fn values(bytes: &[u8]) -> Vec<i32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b) >> 8)
        .collect()
}

impl Seat for TestSeat {
    fn claim(&self, tap: Sender<RoomTick>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.owner.store(id, Ordering::SeqCst);
        *self.tap.lock().unwrap() = Some(tap);
        id
    }
    fn set_aside(&self) {}
    fn load(&self, _root: &str, item: QueueItem, start_ms: u64, _: Option<Arc<dyn MediaAccess>>) {
        if let Some(f) = self.file_of(&item) {
            self.send(Cmd::Set {
                items: vec![f],
                index: 0,
                start_ms,
                play: false,
                from: String::new(),
            });
        }
    }
    fn queue_next(&self, _root: &str, item: QueueItem, _: Option<Arc<dyn MediaAccess>>) {
        if let Some(f) = self.file_of(&item) {
            self.send(Cmd::Insert {
                items: vec![f],
                next: false,
            });
        }
    }
    fn sync(&self, plan: Option<SyncPlan>) {
        self.plans.lock().unwrap().push((Instant::now(), plan));
        self.send(Cmd::Sync(plan));
    }
    fn pause(&self) {
        self.send(Cmd::Pause);
    }
    fn next(&self) {
        self.notes.lock().unwrap().push("seat: next".into());
        self.send(Cmd::Next);
    }
    fn release(&self, id: u64) {
        if self
            .owner
            .compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.send(Cmd::Sync(None));
            *self.tap.lock().unwrap() = None;
            self.send(Cmd::Set {
                items: vec![self.own.clone()],
                index: 0,
                start_ms: 1_000,
                play: false,
                from: "your own".into(),
            });
            self.released.store(true, Ordering::SeqCst);
        }
    }
    fn note(&self, what: &str) {
        self.notes.lock().unwrap().push(what.to_owned());
    }
}

// --- The network ---------------------------------------------------------------------------

/// How a way across the network behaves: a delay, jitter on it (never reordering: what's sent
/// later arrives later, as on one QUIC stream).
#[derive(Clone, Copy, Debug)]
pub struct Way {
    pub delay_ms: f64,
    pub jitter_ms: f64,
}

impl Way {
    pub const LAN: Way = Way {
        delay_ms: 1.0,
        jitter_ms: 0.4,
    };
}

type Job = Box<dyn FnOnce() + Send>;

/// One way across the network, in order, each message delayed as `Way` says. Cut, it delivers
/// nothing more.
pub struct Link {
    tx: Mutex<Option<Sender<(Instant, Job)>>>,
    cut: Arc<AtomicBool>,
}

impl Link {
    pub fn new(way: Way, seed: u64) -> Arc<Link> {
        let (tx, rx) = channel::<(Instant, Job)>();
        let cut = Arc::new(AtomicBool::new(false));
        let c = cut.clone();
        std::thread::Builder::new()
            .name("roomcheck-link".into())
            .spawn(move || {
                let mut seed = seed;
                let mut last_due = Instant::now();
                for (sent, job) in rx {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let r = (seed >> 33) as f64 / (1u64 << 31) as f64;
                    let ms = (way.delay_ms + way.jitter_ms * r).max(0.0);
                    let due = (sent + Duration::from_secs_f64(ms / 1000.0)).max(last_due);
                    last_due = due;
                    let now = Instant::now();
                    if due > now {
                        std::thread::sleep(due - now);
                    }
                    if !c.load(Ordering::SeqCst) {
                        job();
                    }
                }
            })
            .unwrap();
        Arc::new(Link {
            tx: Mutex::new(Some(tx)),
            cut,
        })
    }

    pub fn send(&self, job: impl FnOnce() + Send + 'static) {
        if self.cut.load(Ordering::SeqCst) {
            return;
        }
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send((Instant::now(), Box::new(job)));
        }
    }

    pub fn cut(&self) {
        self.cut.store(true, Ordering::SeqCst);
    }
}

// --- The room, as the host keeps it --------------------------------------------------------

pub struct Member {
    pub id: u32,
    pub seat: Arc<TestSeat>,
    pub follower: Sender<In>,
    up: Arc<Link>,
    down: Arc<Link>,
    clock: Arc<Mutex<together::clock::Clock>>,
    view: Arc<Mutex<Option<RoomView>>>,
    pub gone: Arc<AtomicBool>,
}

pub struct Hub {
    base: Instant,
    pub room: Mutex<Room>,
    host: Mutex<Option<Sender<In>>>,
    pub host_seat: Mutex<Option<Arc<TestSeat>>>,
    pub guests: Mutex<Vec<Arc<Member>>>,
    stop: AtomicBool,
    secret: String,
}

impl Hub {
    pub fn now_us(&self) -> i64 {
        self.base.elapsed().as_micros() as i64
    }

    /// Send what changed: the room to everyone, and a song to get ready.
    pub fn apply(&self, c: together::room::Changes) {
        if c.room {
            let (gv, hv) = {
                let r = self.room.lock().unwrap();
                (r.view(false), r.view(true))
            };
            if let Some(h) = self.host.lock().unwrap().as_ref() {
                let _ = h.send(In::Room(hv));
            }
            for g in self.guests.lock().unwrap().iter() {
                let (v, view, clock, f) = (
                    gv.clone(),
                    g.view.clone(),
                    g.clock.clone(),
                    g.follower.clone(),
                );
                g.down.send(move || {
                    *view.lock().unwrap() = Some(v.clone());
                    if clock.lock().unwrap().ready() {
                        let _ = f.send(In::Room(v));
                    }
                });
            }
        }
        if let Some((rev, index, position_ms)) = c.prepare {
            if let Some(h) = self.host.lock().unwrap().as_ref() {
                let _ = h.send(In::Prepare {
                    rev,
                    index,
                    position_ms,
                });
            }
            for g in self.guests.lock().unwrap().iter() {
                let f = g.follower.clone();
                g.down.send(move || {
                    let _ = f.send(In::Prepare {
                        rev,
                        index,
                        position_ms,
                    });
                });
            }
        }
    }

    pub fn ask(&self, a: Action) {
        let c = self.room.lock().unwrap().ask(HOST, a, self.now_us());
        if let Ok(c) = c {
            self.apply(c);
        }
    }
}

struct HostReport(Arc<Hub>);

impl Report for HostReport {
    fn ready(&self, rev: u64, index: usize) {
        let c = self
            .0
            .room
            .lock()
            .unwrap()
            .ready(HOST, rev, index, self.0.now_us());
        self.0.apply(c);
    }
    fn hearing(&self, rev: u64, index: usize, how: Hearing) {
        let c = self.0.room.lock().unwrap().hearing(HOST, rev, index, how);
        self.0.apply(c);
    }
    fn sync(&self, error_ms: i32) {
        let c = self.0.room.lock().unwrap().sync(HOST, error_ms);
        self.0.apply(c);
    }
    fn length(&self, ms: u64) {
        self.0.room.lock().unwrap().set_length(ms);
    }
}

struct GuestReport {
    hub: Arc<Hub>,
    id: u32,
    up: Arc<Link>,
}

impl GuestReport {
    fn up(&self, f: impl FnOnce(&Hub) -> together::room::Changes + Send + 'static) {
        let hub = self.hub.clone();
        self.up.send(move || {
            let c = f(&hub);
            hub.apply(c);
        });
    }
}

impl Report for GuestReport {
    fn ready(&self, rev: u64, index: usize) {
        let id = self.id;
        self.up(move |h| h.room.lock().unwrap().ready(id, rev, index, h.now_us()));
    }
    fn hearing(&self, rev: u64, index: usize, how: Hearing) {
        let id = self.id;
        self.up(move |h| h.room.lock().unwrap().hearing(id, rev, index, how));
    }
    fn sync(&self, error_ms: i32) {
        let id = self.id;
        self.up(move |h| h.room.lock().unwrap().sync(id, error_ms));
    }
}

/// A room with the host's player following it, playing `songs` from the first; `same_room` as
/// the host sets it.
pub fn host(songs: &[TestSong], own: PathBuf, same_room: bool, drift_ppm: f64) -> Arc<Hub> {
    let secret = "s3cret-for-the-check".to_owned();
    let mut room = Room::new("Host", &secret, songs.iter().map(TestSong::song).collect());
    room.set_let_in_without_asking(true);
    room.set_same_room(same_room);
    let hub = Arc::new(Hub {
        base: Instant::now(),
        room: Mutex::new(room),
        host: Mutex::default(),
        host_seat: Mutex::default(),
        guests: Mutex::default(),
        stop: AtomicBool::new(false),
        secret,
    });
    let seat = TestSeat::new(songs, own, drift_ppm);
    let base = hub.base;
    let tx = follow::start(Follower {
        seat: seat.clone(),
        root: String::new(),
        lib: None,
        catalog: Arc::new(UnconfiguredOnlineAccess),
        local_only: false,
        host_stream: None,
        hosting: true,
        to_local: Arc::new(move |us| Some(base + Duration::from_micros(us.max(0) as u64))),
        report: Box::new(HostReport(hub.clone())),
        delay_ms: 0,
        owner: 0,
    });
    *hub.host.lock().unwrap() = Some(tx);
    *hub.host_seat.lock().unwrap() = Some(seat);
    // The room's clock moves songs along and starts the ones everyone's ready for.
    let h = hub.clone();
    std::thread::Builder::new()
        .name("roomcheck-tick".into())
        .spawn(move || {
            while !h.stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(150));
                let c = h.room.lock().unwrap().tick(h.now_us());
                h.apply(c);
            }
        })
        .unwrap();
    hub.ask(Action::Jump { index: 0 });
    hub
}

/// Someone joins over `up` (to the host) and `down` (from it), their clock `skew_ms` apart
/// from the host's, their device drifting `drift_ppm`.
pub fn join(
    hub: &Arc<Hub>,
    songs: &[TestSong],
    own: PathBuf,
    up: Way,
    down: Way,
    drift_ppm: f64,
    skew_ms: u64,
) -> Arc<Member> {
    static SEED: AtomicU64 = AtomicU64::new(17);
    let s = SEED.fetch_add(101, Ordering::Relaxed);
    let (up, down) = (Link::new(up, s), Link::new(down, s + 7));
    let key = format!("guest-key-{s}");
    let proof = together::invite::proof(&hub.secret, &key);
    let id = match hub.room.lock().unwrap().knock(&key, "Guest", &proof) {
        Knock::In(id) => id,
        other => panic!("not let in: {other:?}"),
    };
    // This computer's own clock, started some while before the host's.
    let local = hub
        .base
        .checked_sub(Duration::from_millis(skew_ms))
        .unwrap_or(hub.base);
    let clock: Arc<Mutex<together::clock::Clock>> = Arc::default();
    let seat = TestSeat::new(songs, own, drift_ppm);
    let c = clock.clone();
    let tx = follow::start(Follower {
        seat: seat.clone(),
        root: String::new(),
        lib: None,
        catalog: Arc::new(UnconfiguredOnlineAccess),
        local_only: false,
        host_stream: None,
        hosting: false,
        to_local: Arc::new(move |host_us| {
            let l = c.lock().ok()?.local_time(host_us)?;
            Some(local + Duration::from_micros(l.max(0) as u64))
        }),
        report: Box::new(GuestReport {
            hub: hub.clone(),
            id,
            up: up.clone(),
        }),
        delay_ms: 0,
        owner: 0,
    });
    let m = Arc::new(Member {
        id,
        seat,
        follower: tx,
        up: up.clone(),
        down: down.clone(),
        clock: clock.clone(),
        view: Arc::default(),
        gone: Arc::new(AtomicBool::new(false)),
    });
    hub.guests.lock().unwrap().push(m.clone());
    // Welcome: the room, and what is getting ready.
    let (view, preparing) = {
        let r = hub.room.lock().unwrap();
        (r.view(false), r.preparing())
    };
    {
        let (v, f) = (m.view.clone(), m.follower.clone());
        down.send(move || {
            *v.lock().unwrap() = Some(view);
            if let Some((rev, index, position_ms)) = preparing {
                let _ = f.send(In::Prepare {
                    rev,
                    index,
                    position_ms,
                });
            }
        });
    }
    hub.apply(together::room::Changes {
        room: true,
        prepare: None,
    });
    // The host's clock, from pings: a burst, then now and then (as guest.rs).
    let (h, mm) = (hub.clone(), m.clone());
    std::thread::Builder::new()
        .name("roomcheck-ping".into())
        .spawn(move || {
            let mut n = 0u32;
            while !mm.gone.load(Ordering::SeqCst) && !h.stop.load(Ordering::SeqCst) {
                let t1 = local.elapsed().as_micros() as i64;
                let (h2, mm2) = (h.clone(), mm.clone());
                mm.up.send(move || {
                    let t2 = h2.now_us();
                    let t3 = h2.now_us();
                    let mm3 = mm2.clone();
                    mm2.down.send(move || {
                        let t4 = local.elapsed().as_micros() as i64;
                        let was = mm3.clock.lock().unwrap().ready();
                        mm3.clock.lock().unwrap().add(Sample { t1, t2, t3, t4 });
                        if !was
                            && mm3.clock.lock().unwrap().ready()
                            && let Some(v) = mm3.view.lock().unwrap().clone()
                        {
                            let _ = mm3.follower.send(In::Room(v));
                        }
                    });
                });
                n += 1;
                std::thread::sleep(Duration::from_millis(if n < 16 { 80 } else { 1_000 }));
            }
        })
        .unwrap();
    m
}

impl Hub {
    /// The host ends the room: every guest is told, every follower stops.
    pub fn end(&self) {
        for g in self.guests.lock().unwrap().iter() {
            let f = g.follower.clone();
            g.down.send(move || {
                let _ = f.send(In::Stop);
            });
            g.gone.store(true, Ordering::SeqCst);
        }
        if let Some(h) = self.host.lock().unwrap().as_ref() {
            let _ = h.send(In::Stop);
        }
        self.stop.store(true, Ordering::SeqCst);
    }

    /// A guest says goodbye and leaves.
    pub fn leave(self: &Arc<Self>, m: &Arc<Member>) {
        let _ = m.follower.send(In::Stop);
        m.gone.store(true, Ordering::SeqCst);
        let (h, id) = (self.clone(), m.id);
        m.up.send(move || {
            let c = h.room.lock().unwrap().left(id, h.now_us());
            h.apply(c);
        });
        self.guests.lock().unwrap().retain(|g| g.id != m.id);
    }

    /// The host takes a guest out of the room.
    pub fn remove(&self, m: &Arc<Member>) {
        let c = self.room.lock().unwrap().remove(m.id, false);
        self.guests.lock().unwrap().retain(|g| g.id != m.id);
        self.apply(c);
        let f = m.follower.clone();
        m.down.send(move || {
            let _ = f.send(In::Stop);
        });
        m.gone.store(true, Ordering::SeqCst);
    }

    /// A guest's connection is lost: nothing gets through either way; after a while they give
    /// up (guest.rs tries for a minute: shortened here) and the host marks them away.
    pub fn lose(&self, m: &Arc<Member>, give_up: Duration) {
        m.up.cut();
        m.down.cut();
        let c = self.room.lock().unwrap().away(m.id);
        self.apply(c);
        let f = m.follower.clone();
        let gone = m.gone.clone();
        std::thread::spawn(move || {
            std::thread::sleep(give_up);
            let _ = f.send(In::Stop);
            gone.store(true, Ordering::SeqCst);
        });
        self.guests.lock().unwrap().retain(|g| g.id != m.id);
    }
}

// --- What was heard ------------------------------------------------------------------------

/// For each song, how many of its frames were heard, and whether everything heard of it came
/// in order (no frame skipped or repeated inside, apart from fades at its edges).
pub fn songs_heard(h: &[i32], songs: &[TestSong]) -> Vec<(usize, bool)> {
    songs
        .iter()
        .map(|s| {
            let frames: Vec<usize> = h.iter().filter_map(|&v| s.frame_of(v)).collect();
            let in_order = frames.windows(2).filter(|w| w[1] != w[0] + 1).count() <= 2;
            (frames.len(), in_order)
        })
        .collect()
}

/// When frame `k` of `song` was heard on this capture (a paced device keeps when each write is
/// heard).
pub fn heard_at(c: &Captured, song: &TestSong, k: usize) -> Option<Instant> {
    let want = song.base + k as i32 + 1;
    let i = values(&c.bytes).iter().position(|&v| v == want)?;
    c.heard_at(i * 4, 4, RATE)
}

pub fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

pub fn own_song(name: &str) -> (PathBuf, TestSong) {
    let s = songs(&format!("own-{name}"), 1, 20.0).remove(0);
    let own = TestSong {
        base: -7_000_000,
        ..s.clone()
    };
    wav(&own.file, own.frames, own.base);
    (own.file.clone(), own)
}

/// After the room let go: your own song, paused at 1 s, then played: (it was given back, every
/// frame from the end of its fade-in on came in order from where it should, frames heard).
pub fn plays_your_own_after(seat: &TestSeat, own: &TestSong) -> (bool, bool, usize) {
    let t0 = Instant::now();
    while !seat.released.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(5) {
        sleep(20);
    }
    let released = seat.released.load(Ordering::SeqCst);
    sleep(300);
    let from = seat.captured.lock().unwrap().bytes.len();
    seat.send(Cmd::Play);
    sleep(1_500);
    let h = values(&seat.captured.lock().unwrap().bytes[from..]);
    let mine: Vec<usize> = h.iter().filter_map(|&v| own.frame_of(v)).collect();
    // From 1 s, after the 15 ms rise: frame 48,720 on, one after another.
    let start = RATE as usize + 720;
    let at = mine.iter().position(|&k| k == start);
    let straight = at.is_some_and(|a| mine[a..].windows(2).all(|w| w[1] == w[0] + 1));
    (released, straight, mine.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms_between(a: Instant, b: Instant) -> f64 {
        if a >= b {
            a.duration_since(b).as_secs_f64() * 1000.0
        } else {
            -(b.duration_since(a).as_secs_f64() * 1000.0)
        }
    }

    /// `n` songs of `secs` each, gapless, the host alone: each heard all the way, in order, and
    /// your own music after.
    fn chain(name: &str, n: usize, secs: f64) {
        let s = songs(name, n, secs);
        let (own, mine) = own_song(name);
        let hub = host(&s, own, false, 0.0);
        sleep((n as f64 * secs * 1000.0) as u64 + 2_000);
        let seat = hub.host_seat.lock().unwrap().clone().unwrap();
        let h = seat.heard();
        let heard = songs_heard(&h, &s);
        for (k, (got, in_order)) in heard.iter().enumerate() {
            assert!(
                *got as f64 >= s[k].frames as f64 * 0.97,
                "{name}: song {k} heard all the way ({got} of {} frames): {heard:?}, follower: {:?}",
                s[k].frames,
                seat.notes.lock().unwrap()
            );
            assert!(in_order, "{name}: song {k} in order");
        }
        hub.end();
        let (released, straight, _) = plays_your_own_after(&seat, &mine);
        assert!(released && straight, "{name}: your own music after");
        seat.finish();
    }

    #[test]
    fn a_room_plays_long_songs_through_one_after_another() {
        // Songs longer than the 10 s the next one is readied ahead by: the third used to stop
        // 0.4 s in (the follower looked for the player's second queue entry, never the third,
        // and skipped on).
        chain("long", 3, 11.0);
    }

    #[test]
    fn a_room_plays_short_songs_through_one_after_another() {
        // Songs shorter than that: the next is readied before the player has moved on to the
        // one before it (which used to replace the song playing, so one was never heard).
        chain("short", 4, 2.5);
    }

    /// Host and two guests over the network `way` (one way, and `back` the other), apart or in
    /// one room: every song heard by everyone, and how far apart the three heard each song's
    /// frame at 1 s (the worst, ms).
    fn three(name: &str, way: Way, back: Way, same_room: bool, drift: f64) -> f64 {
        let s = songs(name, 3, 4.0);
        let (own, _) = own_song(name);
        let hub = host(&s, own.clone(), same_room, 0.0);
        let a = join(&hub, &s, own.clone(), way, back, drift, 3_700);
        let b = join(&hub, &s, own, back, way, -drift, 900);
        sleep(14_000);
        let seats = [
            hub.host_seat.lock().unwrap().clone().unwrap(),
            a.seat.clone(),
            b.seat.clone(),
        ];
        hub.end();
        sleep(300);
        let caps: Vec<Captured> = seats.iter().map(|s| s.finish()).collect();
        let mut worst = 0.0f64;
        for (k, song) in s.iter().enumerate() {
            let at: Vec<Instant> = caps
                .iter()
                .enumerate()
                .map(|(who, c)| {
                    heard_at(c, song, RATE as usize).unwrap_or_else(|| {
                        panic!(
                            "{name}: song {k} heard by {who}: {:?}",
                            songs_heard(&values(&c.bytes), &s)
                        )
                    })
                })
                .collect();
            for x in &at[1..] {
                worst = worst.max(ms_between(*x, at[0]).abs());
            }
        }
        worst
    }

    #[test]
    fn on_one_wifi_everyone_hears_each_song_together() {
        let worst = three("wifi", Way::LAN, Way::LAN, true, 0.0);
        eprintln!("one Wi-Fi: the three heard each song within {worst:.2} ms");
        assert!(worst < 3.0, "within 3 ms on one Wi-Fi: {worst:.2} ms");
    }

    #[test]
    fn over_the_internet_everyone_hears_each_song_together() {
        // 30 ms one way and 50 the other, with 15 ms of jitter: the clock can't see the
        // difference between the two ways (half of it, 10 ms, is the least error).
        let worst = three(
            "internet",
            Way {
                delay_ms: 30.0,
                jitter_ms: 15.0,
            },
            Way {
                delay_ms: 50.0,
                jitter_ms: 15.0,
            },
            false,
            0.0,
        );
        eprintln!("internet: the three heard each song within {worst:.2} ms");
        assert!(
            worst < 40.0,
            "within 40 ms over the internet: {worst:.1} ms"
        );
    }

    #[test]
    fn every_way_a_room_ends_gives_each_player_back() {
        let s = songs("ends", 2, 30.0);
        let (own, mine) = own_song("ends");
        let hub = host(&s, own.clone(), false, 0.0);
        let net = Way {
            delay_ms: 20.0,
            jitter_ms: 5.0,
        };
        let leaves = join(&hub, &s, own.clone(), net, net, 0.0, 100);
        let removed = join(&hub, &s, own.clone(), net, net, 0.0, 200);
        let lost = join(&hub, &s, own, net, net, 0.0, 300);
        sleep(3_000);
        hub.leave(&leaves);
        hub.remove(&removed);
        hub.lose(&lost, Duration::from_secs(2));
        sleep(1_000);
        for (who, m) in [("left", &leaves), ("removed", &removed), ("lost", &lost)] {
            let (released, straight, n) = plays_your_own_after(&m.seat, &mine);
            assert!(released, "{who}: the room let go of the player");
            assert!(straight, "{who}: your own music as alone ({n} frames)");
            assert!(
                m.seat
                    .status()
                    .is_some_and(|s| s.sync.is_none() && s.ran_dry == 0),
                "{who}: no room plan left, never dry"
            );
            m.seat.finish();
        }
        // The host's own player played on, and when the host ends the room it's given back too.
        let seat = hub.host_seat.lock().unwrap().clone().unwrap();
        hub.end();
        let (released, straight, _) = plays_your_own_after(&seat, &mine);
        assert!(released && straight, "the host: your own music after");
        seat.finish();
    }
}
