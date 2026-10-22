//! The room's hold on a person's player (contract 5.4 of the Listen together design).
//!
//! The conductor (one per person, the host too) turns the room into calls on a `RoomHandle`;
//! the player implements `RoomPlayer` and keeps everything of the room in the room program it
//! makes at `attach`, dropped whole at `detach`. Nothing here blocks: every call is a command
//! the player takes up on its own thread, and the conductor learns how it went from `status`.
//!
//! The item type `I` is the player's own (`player::engine::Item`): what to open for an entry,
//! as the copy finder chose it. This crate never looks inside it.
//!
//! Fakes for both sides are in `fakes`, so each side is built and tested before the other.

use crate::align::{Alignment, CopyClass};
use crate::anchor::{Anchor, EntryId, PrepId, Transition};
use crate::clock_map::ClockMapHandle;
use serde::{Deserialize, Serialize};

/// How tightly a playing person follows the room (per person, decision D4). A remote (one
/// speaker mode) plays nothing, so it has no sync mode: it detaches instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Speakers heard together: 1 to 2 ms, the same-room column of the controller.
    EveryDevice,
    /// Each on their own: 10 to 20 ms, the apart column.
    Apart,
}

/// How a person listens, as the room and the window show it (section 4.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// A remote: the host's speaker plays; this person sees and controls.
    OneSpeaker,
    EveryDevice,
    Apart,
}

impl ListenMode {
    /// What the player follows in this mode (`None`: a remote attaches no player).
    pub fn sync(self) -> Option<SyncMode> {
        match self {
            ListenMode::OneSpeaker => None,
            ListenMode::EveryDevice => Some(SyncMode::EveryDevice),
            ListenMode::Apart => Some(SyncMode::Apart),
        }
    }
}

/// Why a person is faded out locally while the room goes on (section 4.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suspension {
    /// Their stream ran dry; back once 5 s past the room's place is in.
    Buffering,
    /// Their output was lost, switched or busy; back once it reopens.
    OutputLost,
    /// Another app took audio focus (Android: a call).
    Interrupted,
    /// The computer or phone is going to sleep.
    Sleeping,
}

/// A person's player in the room, as shown beside their name (section 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "why", rename_all = "snake_case")]
pub enum SubState {
    /// Getting this entry ready.
    Opening,
    /// Ready, waiting for the room's start.
    Ready,
    /// Playing, the error inside the band.
    InStep,
    /// Playing, the rate being changed.
    Correcting,
    /// A crossfade to the room's place is prepared or under way.
    Realigning,
    Suspended(Suspension),
    /// Paused just for them; the room goes on.
    SteppedOut,
    /// No copy of this entry; back for the next one.
    CannotPlay,
    /// One speaker mode: plays nothing.
    Remote,
}

/// What the player tells the conductor, read whenever it likes (cheap, never blocks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoomPlayStatus {
    /// The newest prepare that is ready (section 4.4's "ready means").
    pub ready: Option<PrepId>,
    /// The armed next entry, ready for its boundary.
    pub next_ready: Option<EntryId>,
    /// The entry heard now.
    pub entry: Option<EntryId>,
    pub sub_state: SubState,
    /// What is heard minus where the room is, in µs (positive: ahead), filtered.
    pub error_us: Option<i64>,
    /// The clock map's bound, in µs.
    pub bound_us: Option<u32>,
    /// How the copy heard now matches the reference.
    pub copy: Option<CopyClass>,
    /// The rate trim applied now, in ppm (0 outside rooms).
    pub ppm: f64,
}

impl Default for RoomPlayStatus {
    fn default() -> Self {
        RoomPlayStatus {
            ready: None,
            next_ready: None,
            entry: None,
            sub_state: SubState::Opening,
            error_us: None,
            bound_us: None,
            copy: None,
            ppm: 0.0,
        }
    }
}

/// A player a room can take.
pub trait RoomPlayer<I>: Send + Sync {
    /// The room takes the player: your own program is set aside (kept), a room program begins.
    /// The host attaches too, with an identity clock map.
    fn attach(&self, clock: ClockMapHandle, mode: SyncMode) -> Box<dyn RoomHandle<I>>;
}

/// The room's hold on the player, for as long as the person plays in the room.
pub trait RoomHandle<I>: Send {
    /// Get `entry` ready at the room's content time `room_content_us` (my copy plays at that
    /// plus `align.offset_us`), on a spare voice; whatever plays keeps playing meanwhile.
    fn prepare(
        &self,
        prep: PrepId,
        entry: EntryId,
        item: I,
        room_content_us: i64,
        align: Alignment,
    );
    /// The entry after the playing one, to switch to at `boundary_host_us` as `transition`
    /// says, by the player itself.
    fn arm_next(
        &self,
        entry: EntryId,
        item: I,
        boundary_host_us: i64,
        transition: Transition,
        align: Alignment,
    );
    /// The armed next is withdrawn (the queue changed and nothing follows the playing entry
    /// now): drop its voice if it hasn't started. Arming another entry replaces it instead.
    fn disarm(&self);
    /// Start, switch or resume so the anchor holds (a no-op when already there).
    fn commit(&self, anchor: Anchor);
    /// A scheduled pause: fade out from the frame heard at `anchor.host_us`, then hold at
    /// `anchor.content_us` (moving there in silence if the message came late).
    fn hold(&self, anchor: Anchor);
    /// Every device or apart (per person); a remote detaches instead.
    fn set_mode(&self, mode: SyncMode);
    /// This output's extra delay (Bluetooth, a receiver), in µs.
    fn set_extra_latency(&self, us: i32);
    /// Pause just for me; the room goes on.
    fn step_out(&self);
    /// Catch up at the room's place (300 ms fade-in).
    fn rejoin(&self);
    fn status(&self) -> RoomPlayStatus;
    /// The room lets go: the room program is dropped, your own program comes back paused.
    fn detach(self: Box<Self>);
}
