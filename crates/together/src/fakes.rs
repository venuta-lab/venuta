//! Fakes for both sides of the room's hold on a player (contract 5.4), so each side is built and
//! tested before the other exists.
//!
//! - [`FakeRoomPlayer`] and its [`FakeRoomHandle`]: for the conductor and the sessions. Every
//!   call is recorded in order, with the time it came; readiness and the player's state answer
//!   from a script (ready a while after a prepare, entries it can't play, a state set by hand).
//! - [`RoomScript`]: for the player. Timed room calls (by host time) played against any
//!   `RoomPlayer`, with its status read as they go.

use crate::align::{Alignment, CopyClass};
use crate::anchor::{Anchor, EntryId, PrepId, Transition};
use crate::clock_map::ClockMapHandle;
use crate::room_player::{RoomHandle, RoomPlayStatus, RoomPlayer, SubState, Suspension, SyncMode};
use crate::time::{MonoClock, Timebase};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

/// One call the room made.
#[derive(Debug, Clone, PartialEq)]
pub enum Call<I> {
    Attach {
        mode: SyncMode,
    },
    Prepare {
        prep: PrepId,
        entry: EntryId,
        item: I,
        room_content_us: i64,
        align: Alignment,
    },
    ArmNext {
        entry: EntryId,
        item: I,
        boundary_host_us: i64,
        transition: Transition,
        align: Alignment,
    },
    Disarm,
    Commit(Anchor),
    Hold(Anchor),
    SetMode(SyncMode),
    SetExtraLatency(i32),
    StepOut,
    Rejoin,
    Suspend(Suspension),
    Resume,
    Detach,
}

/// How the fake player answers.
#[derive(Debug, Clone, Default)]
pub struct Script {
    /// A prepare is ready this long after it was asked (ns on the fake's clock).
    pub ready_after_ns: u64,
    /// The armed next is ready this long after it was armed.
    pub next_ready_after_ns: u64,
    /// Entries it has no copy of: never ready, `CannotPlay`.
    pub unable: HashSet<EntryId>,
    /// The copy class it reports for what it plays.
    pub copy: Option<CopyClass>,
    /// The error it reports while in step, in µs.
    pub error_us: Option<i64>,
    /// The length it reports for the entry it plays, in µs.
    pub length_us: Option<i64>,
}

struct Fake<I> {
    script: Script,
    calls: Vec<(u64, Call<I>)>,
    /// A state set by hand, over what the script says.
    forced: Option<SubState>,
    attached: bool,
}

/// The test's side of a fake player: what was called, and control over its answers.
pub struct FakeLog<I> {
    fake: Arc<Mutex<Fake<I>>>,
    clock: Arc<dyn MonoClock>,
}

impl<I> Clone for FakeLog<I> {
    fn clone(&self) -> Self {
        FakeLog {
            fake: self.fake.clone(),
            clock: self.clock.clone(),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<I: Clone> FakeLog<I> {
    /// Every call so far, with when (the fake's clock, ns).
    pub fn calls(&self) -> Vec<(u64, Call<I>)> {
        lock(&self.fake).calls.clone()
    }

    /// The calls alone.
    pub fn just_calls(&self) -> Vec<Call<I>> {
        lock(&self.fake).calls.iter().map(|c| c.1.clone()).collect()
    }

    /// Answer `sub_state` from now on, whatever the script says (`None`: the script again).
    pub fn force(&self, sub_state: Option<SubState>) {
        lock(&self.fake).forced = sub_state;
    }

    /// Change the script.
    pub fn script(&self, f: impl FnOnce(&mut Script)) {
        f(&mut lock(&self.fake).script);
    }

    /// Whether a handle is attached now.
    pub fn attached(&self) -> bool {
        lock(&self.fake).attached
    }
}

/// A player for the conductor's tests.
pub struct FakeRoomPlayer<I> {
    log: FakeLog<I>,
}

impl<I: Clone + Send + 'static> FakeRoomPlayer<I> {
    /// A fake answering from `script`, its time from `clock`.
    pub fn new(clock: Arc<dyn MonoClock>, script: Script) -> (FakeRoomPlayer<I>, FakeLog<I>) {
        let log = FakeLog {
            fake: Arc::new(Mutex::new(Fake {
                script,
                calls: Vec::new(),
                forced: None,
                attached: false,
            })),
            clock,
        };
        (FakeRoomPlayer { log: log.clone() }, log)
    }
}

impl<I: Clone + Send + 'static> RoomPlayer<I> for FakeRoomPlayer<I> {
    fn attach(&self, _clock: ClockMapHandle, mode: SyncMode) -> Box<dyn RoomHandle<I>> {
        let now = self.log.clock.now_ns();
        let mut f = lock(&self.log.fake);
        f.calls.push((now, Call::Attach { mode }));
        f.attached = true;
        drop(f);
        Box::new(FakeRoomHandle {
            log: self.log.clone(),
        })
    }
}

/// The handle a fake player gives.
pub struct FakeRoomHandle<I> {
    log: FakeLog<I>,
}

impl<I: Clone> FakeRoomHandle<I> {
    fn record(&self, c: Call<I>) {
        let now = self.log.clock.now_ns();
        lock(&self.log.fake).calls.push((now, c));
    }
}

impl<I: Clone + Send + 'static> RoomHandle<I> for FakeRoomHandle<I> {
    fn prepare(
        &self,
        prep: PrepId,
        entry: EntryId,
        item: I,
        room_content_us: i64,
        align: Alignment,
    ) {
        self.record(Call::Prepare {
            prep,
            entry,
            item,
            room_content_us,
            align,
        });
    }
    fn arm_next(
        &self,
        entry: EntryId,
        item: I,
        boundary_host_us: i64,
        transition: Transition,
        align: Alignment,
    ) {
        self.record(Call::ArmNext {
            entry,
            item,
            boundary_host_us,
            transition,
            align,
        });
    }
    fn disarm(&self) {
        self.record(Call::Disarm);
    }
    fn commit(&self, anchor: Anchor) {
        self.record(Call::Commit(anchor));
    }
    fn hold(&self, anchor: Anchor) {
        self.record(Call::Hold(anchor));
    }
    fn set_mode(&self, mode: SyncMode) {
        self.record(Call::SetMode(mode));
    }
    fn set_extra_latency(&self, us: i32) {
        self.record(Call::SetExtraLatency(us));
    }
    fn step_out(&self) {
        self.record(Call::StepOut);
    }
    fn rejoin(&self) {
        self.record(Call::Rejoin);
    }
    fn suspend(&self, why: Suspension) {
        self.record(Call::Suspend(why));
    }
    fn resume(&self) {
        self.record(Call::Resume);
    }

    fn status(&self) -> RoomPlayStatus {
        let now = self.log.clock.now_ns();
        let f = lock(&self.log.fake);
        let s = &f.script;
        let mut st = RoomPlayStatus {
            copy: s.copy,
            ..RoomPlayStatus::default()
        };
        let mut preparing: Option<(u64, PrepId, EntryId)> = None;
        let mut stepped_out = false;
        let mut away: Option<Suspension> = None;
        let mut playing = false;
        for (at, c) in &f.calls {
            match c {
                Call::Prepare { prep, entry, .. } => preparing = Some((*at, *prep, *entry)),
                Call::ArmNext { entry, .. } => {
                    st.next_ready = (now >= at + s.next_ready_after_ns
                        && !s.unable.contains(entry))
                    .then_some(*entry);
                }
                Call::Disarm => st.next_ready = None,
                Call::Commit(a) => {
                    st.entry = Some(a.entry);
                    playing = a.playing;
                    if st.next_ready == Some(a.entry) {
                        st.next_ready = None;
                    }
                }
                Call::Hold(_) => playing = false,
                Call::StepOut => stepped_out = true,
                Call::Rejoin => stepped_out = false,
                Call::Suspend(why) => away = Some(*why),
                Call::Resume => away = None,
                _ => {}
            }
        }
        let unable_now = st
            .entry
            .or(preparing.map(|p| p.2))
            .is_some_and(|e| s.unable.contains(&e));
        if let Some((at, prep, entry)) = preparing
            && !s.unable.contains(&entry)
            && now >= at + s.ready_after_ns
        {
            st.ready = Some(prep);
        }
        st.sub_state = if let Some(forced) = f.forced {
            forced
        } else if unable_now {
            SubState::CannotPlay
        } else if stepped_out {
            SubState::SteppedOut
        } else if let Some(why) = away {
            SubState::Suspended(why)
        } else if playing {
            SubState::InStep
        } else if st.ready.is_some() {
            SubState::Ready
        } else {
            SubState::Opening
        };
        if st.sub_state == SubState::InStep {
            st.error_us = s.error_us;
        }
        st.length_us = st.entry.and(s.length_us);
        st
    }

    fn detach(self: Box<Self>) {
        self.record(Call::Detach);
        lock(&self.log.fake).attached = false;
    }
}

/// One timed room call in a script.
#[derive(Debug, Clone)]
pub enum Step<I> {
    Prepare {
        prep: PrepId,
        entry: EntryId,
        item: I,
        room_content_us: i64,
        align: Alignment,
    },
    ArmNext {
        entry: EntryId,
        item: I,
        boundary_host_us: i64,
        transition: Transition,
        align: Alignment,
    },
    Disarm,
    Commit(Anchor),
    Hold(Anchor),
    SetMode(SyncMode),
    SetExtraLatency(i32),
    StepOut,
    Rejoin,
    Detach,
}

/// Room calls at host times, to play against a real player before the sessions exist.
pub struct RoomScript<I> {
    pub mode: SyncMode,
    /// (host µs, call), in time order.
    pub steps: Vec<(i64, Step<I>)>,
}

/// What the player said while a script played: (host µs, its status).
pub type Watched = Vec<(i64, RoomPlayStatus)>;

impl<I: Clone + Send + 'static> RoomScript<I> {
    /// Attach to `player` with `clock` (the map the player reads), then make each call when
    /// `host` reads its time, reading the player's status every `watch_us` meanwhile. Time
    /// passes on `tb` (on a `SimClock`, call this from the thread that runs the simulation).
    /// A script that doesn't end with `Detach` detaches at its end.
    pub fn play(
        self,
        player: &dyn RoomPlayer<I>,
        clock: ClockMapHandle,
        tb: &dyn Timebase,
        host: &dyn Fn() -> i64,
        watch_us: i64,
    ) -> Watched {
        let mut handle = Some(player.attach(clock, self.mode));
        let mut watched = Vec::new();
        for (at_us, step) in self.steps {
            while host() < at_us {
                let wait = (at_us - host()).min(watch_us.max(1));
                tb.pass(wait as u64 * 1_000);
                if let Some(h) = &handle {
                    watched.push((host(), h.status()));
                }
            }
            let Some(h) = handle.as_ref() else {
                break;
            };
            match step {
                Step::Prepare {
                    prep,
                    entry,
                    item,
                    room_content_us,
                    align,
                } => h.prepare(prep, entry, item, room_content_us, align),
                Step::ArmNext {
                    entry,
                    item,
                    boundary_host_us,
                    transition,
                    align,
                } => h.arm_next(entry, item, boundary_host_us, transition, align),
                Step::Disarm => h.disarm(),
                Step::Commit(a) => h.commit(a),
                Step::Hold(a) => h.hold(a),
                Step::SetMode(m) => h.set_mode(m),
                Step::SetExtraLatency(us) => h.set_extra_latency(us),
                Step::StepOut => h.step_out(),
                Step::Rejoin => h.rejoin(),
                Step::Detach => {
                    if let Some(h) = handle.take() {
                        h.detach();
                    }
                }
            }
        }
        if let Some(h) = handle.take() {
            h.detach();
        }
        watched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock_map;
    use crate::time::SimClock;

    const MS: u64 = 1_000_000;

    fn anchor(entry: u64, playing: bool) -> Anchor {
        Anchor {
            entry: EntryId(entry),
            content_us: 0,
            host_us: 1_000_000,
            playing,
            seq: 1,
        }
    }

    #[test]
    fn the_fake_records_every_call_and_answers_from_its_script() {
        let clock = SimClock::new(0);
        let (player, log) = FakeRoomPlayer::<&'static str>::new(
            Arc::new(clock.clone()),
            Script {
                ready_after_ns: 300 * MS,
                unable: [EntryId(9)].into(),
                copy: Some(CopyClass::Measured),
                error_us: Some(400),
                ..Script::default()
            },
        );
        let h = player.attach(clock_map::handle(), SyncMode::Apart);
        assert!(log.attached());
        h.prepare(
            PrepId(1),
            EntryId(4),
            "song four",
            95_000_000,
            Alignment::EXACT,
        );
        assert_eq!(h.status().sub_state, SubState::Opening);
        assert_eq!(h.status().ready, None);
        clock.pass(300 * MS);
        assert_eq!(h.status().ready, Some(PrepId(1)));
        assert_eq!(h.status().sub_state, SubState::Ready);
        h.commit(anchor(4, true));
        let s = h.status();
        assert_eq!(
            (s.sub_state, s.entry, s.error_us),
            (SubState::InStep, Some(EntryId(4)), Some(400))
        );
        h.step_out();
        assert_eq!(h.status().sub_state, SubState::SteppedOut);
        h.rejoin();
        h.prepare(PrepId(2), EntryId(9), "no copy", 0, Alignment::APPROXIMATE);
        clock.pass(S);
        assert_eq!(
            h.status().ready,
            None,
            "never ready for an entry it can't play"
        );
        log.force(Some(SubState::Suspended(
            crate::room_player::Suspension::Buffering,
        )));
        assert!(matches!(h.status().sub_state, SubState::Suspended(_)));
        h.detach();
        assert!(!log.attached());
        let calls = log.calls();
        assert_eq!(
            calls[0],
            (
                0,
                Call::Attach {
                    mode: SyncMode::Apart
                }
            )
        );
        assert_eq!(calls[2].0, 300 * MS, "when each came");
        assert_eq!(calls.last().map(|c| &c.1), Some(&Call::Detach));
        assert_eq!(calls.len(), 7);
    }

    const S: u64 = 1_000_000_000;

    #[test]
    fn a_disarmed_next_is_no_longer_ready() {
        let clock = SimClock::new(0);
        let (player, log) = FakeRoomPlayer::<u32>::new(Arc::new(clock.clone()), Script::default());
        let h = player.attach(clock_map::handle(), SyncMode::Apart);
        h.arm_next(
            EntryId(5),
            5,
            S as i64,
            Transition::Gapless,
            Alignment::EXACT,
        );
        assert_eq!(h.status().next_ready, Some(EntryId(5)));
        h.disarm();
        assert_eq!(h.status().next_ready, None);
        assert_eq!(log.just_calls()[2], Call::Disarm);
    }

    #[test]
    fn a_script_plays_its_calls_at_their_host_times_and_watches() {
        let clock = SimClock::new(0);
        let (player, log) = FakeRoomPlayer::<u32>::new(
            Arc::new(clock.clone()),
            Script {
                ready_after_ns: 200 * MS,
                ..Script::default()
            },
        );
        // The host's clock 5 s ahead of this one.
        let c2 = clock.clone();
        let host = move || (c2.now_ns() / 1_000) as i64 + 5_000_000;
        let script = RoomScript {
            mode: SyncMode::EveryDevice,
            steps: vec![
                (
                    5_100_000,
                    Step::Prepare {
                        prep: PrepId(1),
                        entry: EntryId(1),
                        item: 11,
                        room_content_us: 0,
                        align: Alignment::EXACT,
                    },
                ),
                (5_600_000, Step::Commit(anchor(1, true))),
                (6_000_000, Step::Hold(anchor(1, false))),
            ],
        };
        let watched = script.play(&player, clock_map::handle(), &clock, &host, 20_000);
        let calls = log.calls();
        let at_us: Vec<u64> = calls.iter().map(|c| c.0 / 1_000).collect();
        assert_eq!(at_us, [0, 100_000, 600_000, 1_000_000, 1_000_000]);
        assert_eq!(calls.last().map(|c| &c.1), Some(&Call::Detach));
        // Watched every 20 ms: opening, then ready 200 ms after the prepare, in step after the
        // commit.
        let at = |t: i64| watched.iter().find(|w| w.0 >= t).unwrap().1.sub_state;
        assert_eq!(at(5_200_000), SubState::Opening);
        assert_eq!(at(5_320_000), SubState::Ready);
        assert_eq!(at(5_700_000), SubState::InStep);
        assert!(watched.windows(2).all(|w| w[1].0 - w[0].0 <= 20_000));
    }
}
