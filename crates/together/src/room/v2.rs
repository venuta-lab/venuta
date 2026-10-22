//! Room v2 (section 4.3 and 4.4 of the Listen together design): the room the host keeps, on
//! protocol 3. Pure: the host's time is passed in (µs of its monotonic clock), and every call
//! returns what to send, addressed and in order.
//!
//! Skeleton: the bodies come after the tests.

#![allow(unused_variables, dead_code)]

use crate::anchor::{Anchor, EntryId, Next, PrepId, Purpose, Transition};
use crate::proto::Role;
use crate::proto::Song;
use crate::proto::v3::{Action, CopyReport, Entry, Phase, RoomView, ToGuest, ToHost};
use crate::room_player::{ListenMode, SubState};
use crate::transport::PathInfo;

/// The host's member id.
pub const HOST: u32 = 0;
/// A prepare waits this long at most for everyone to be ready (rule 3).
pub const READY_WAIT_US: i64 = 2_500_000;
/// A member not heard from this long is shown away (pings come once a second).
pub const AWAY_AFTER_US: i64 = 5_000_000;
/// A member not heard from this long is taken as lost (QUIC's idle timeout).
pub const SILENT_LOST_US: i64 = 15_000_000;
/// A lost member is kept this long for their reconnect (decision D17).
pub const KEEP_AWAY_US: i64 = 120_000_000;
/// Every playing member suspended this long holds the room (decision D11).
pub const AUTO_HOLD_US: i64 = 3_000_000;
/// Nobody able to play the entry this long skips it.
pub const AUTO_SKIP_US: i64 = 5_000_000;
/// A held or resting room with no action this long ends itself (decision D18).
pub const IDLE_END_US: i64 = 3_600_000_000;
/// Within this of a boundary, a queue change leaves the armed next as it is (case C6).
pub const FREEZE_US: i64 = 3_000_000;
/// Previous over this far into a song starts it again; under it, the song before.
pub const PREVIOUS_RESTARTS_US: i64 = 3_000_000;
/// A request is kept this long.
pub const REQUEST_KEEP_US: i64 = 60_000_000;
/// This many prepares missed in a row tells the host.
pub const SLOW_AFTER: u32 = 3;
/// On the host's network: direct, and a median round trip under this.
pub const SAME_NETWORK_RTT_US: u32 = 8_000;
/// Every device is turned off for someone whose median round trip is over this...
pub const SAME_NETWORK_OFF_RTT_US: u32 = 15_000;
/// ... for this long.
pub const SAME_NETWORK_OFF_US: i64 = 30_000_000;
/// Round trips are kept over this long (and 64 at most).
pub const RTT_WINDOW_US: i64 = 60_000_000;
/// Guests control playback by default up to this many people (decision D20).
pub const AUTO_CONTROL_UP_TO: usize = 4;
/// How many activity lines are kept.
pub const ACTIVITY: usize = 6;

/// What a knock at the door comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Knock {
    /// In (let in without asking, or back after a drop): their welcome is in the changes.
    In(u32),
    /// Waiting for the host.
    Waiting(u32),
    /// Not let in, with why: send `Refused` and close.
    Refused(String),
}

/// One message to send to one member (the host's own, `HOST`, go to the host's conductor and
/// window).
#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub to: u32,
    pub msg: ToGuest,
}

/// Something for the host's window alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostNote {
    /// Missed three prepares in a row: "Ravi's connection is slow".
    Slow { id: u32 },
    /// Moved from every device to apart: "Same room turned off: Ali's connection is too slow".
    SameRoomOff { id: u32 },
}

/// What a call changed, for the session to carry out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    /// Messages, in the order to send them.
    pub out: Vec<Out>,
    pub notes: Vec<HostNote>,
    /// The host's speaker's volume to set (0 to 100).
    pub volume: Option<u8>,
    /// Out of the room now (left, removed, gone after the grace): close and forget them.
    pub gone: Vec<u32>,
    /// The room ended: send what's in `out`, then close everything.
    pub ended: bool,
}

impl Changes {
    /// The messages for one member, in order.
    pub fn to(&self, id: u32) -> Vec<&ToGuest> {
        self.out
            .iter()
            .filter(|o| o.to == id)
            .map(|o| &o.msg)
            .collect()
    }

    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        *self == Changes::default()
    }

    /// `o` after these.
    pub fn and(mut self, o: Changes) -> Changes {
        self.out.extend(o.out);
        self.notes.extend(o.notes);
        self.volume = o.volume.or(self.volume);
        self.gone.extend(o.gone);
        self.ended |= o.ended;
        self
    }
}

/// How the room starts, from the host's player at the moment it opens (case A1: the host's
/// audio is never touched).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Nothing playing.
    Resting,
    /// Paused at this place of the song at `index`.
    Held { index: usize, content_us: i64 },
    /// Playing: the song at `index`, its `content_us` heard at host time `host_us`.
    Playing {
        index: usize,
        content_us: i64,
        host_us: i64,
    },
}

/// A person's player, as they report it (`ToHost::State`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    pub entry: Option<EntryId>,
    pub sub_state: SubState,
    pub error_us: Option<i64>,
    pub bound_us: Option<u32>,
    pub copy: Option<CopyReport>,
}

/// The room the host keeps.
#[derive(Debug, Clone)]
pub struct Room {}

impl Room {
    pub fn new(host_name: &str, secret: &str, songs: Vec<Song>, opening: Opening) -> Room {
        todo!()
    }

    pub fn knock(&mut self, key: &str, name: &str, proof: &str, now_us: i64) -> (Knock, Changes) {
        todo!()
    }
    pub fn admit(&mut self, id: u32, now_us: i64) -> Option<Changes> {
        todo!()
    }
    pub fn deny(&mut self, id: u32, now_us: i64) -> Changes {
        todo!()
    }
    pub fn remove(&mut self, id: u32, block: bool, now_us: i64) -> Changes {
        todo!()
    }
    pub fn left(&mut self, id: u32, now_us: i64) -> Changes {
        todo!()
    }
    pub fn heard(&mut self, id: u32, now_us: i64) -> Changes {
        todo!()
    }
    pub fn lost(&mut self, id: u32, now_us: i64) -> Changes {
        todo!()
    }
    pub fn path(&mut self, id: u32, path: PathInfo, now_us: i64) -> Changes {
        todo!()
    }
    pub fn ready(&mut self, id: u32, prep: PrepId, entry: EntryId, now_us: i64) -> Changes {
        todo!()
    }
    pub fn unable(&mut self, id: u32, prep: PrepId, entry: EntryId, now_us: i64) -> Changes {
        todo!()
    }
    pub fn state(&mut self, id: u32, report: Report, now_us: i64) -> Changes {
        todo!()
    }
    pub fn mode(&mut self, id: u32, mode: ListenMode, now_us: i64) -> Changes {
        todo!()
    }
    pub fn set_length(&mut self, entry: EntryId, length_us: i64, now_us: i64) -> Changes {
        todo!()
    }
    pub fn ask(&mut self, id: u32, action: Action, now_us: i64) -> Result<Changes, String> {
        todo!()
    }
    pub fn request(&mut self, id: u32, action: Action, now_us: i64) -> Result<Changes, String> {
        todo!()
    }
    pub fn undo(&mut self, id: u32, now_us: i64) -> Result<Changes, String> {
        todo!()
    }
    pub fn handle(&mut self, id: u32, msg: ToHost, now_us: i64) -> Result<Changes, String> {
        todo!()
    }
    pub fn tick(&mut self, now_us: i64) -> Changes {
        todo!()
    }
    pub fn end(&mut self, now_us: i64) -> Changes {
        todo!()
    }
    pub fn set_transition(&mut self, t: Transition, now_us: i64) -> Result<Changes, String> {
        todo!()
    }
    pub fn set_new_role(&mut self, role: Option<Role>, now_us: i64) -> Changes {
        todo!()
    }
    pub fn set_role(&mut self, id: u32, role: Option<Role>, now_us: i64) -> Changes {
        todo!()
    }
    pub fn set_guests_volume(&mut self, on: bool, now_us: i64) -> Changes {
        todo!()
    }
    pub fn set_let_in_without_asking(&mut self, on: bool) {
        todo!()
    }
    pub fn lets_in_without_asking(&self) -> bool {
        todo!()
    }
    pub fn view_for(&self, id: u32) -> RoomView {
        todo!()
    }
    pub fn is_member(&self, id: u32) -> bool {
        todo!()
    }
    pub fn role_of(&self, id: u32) -> Option<Role> {
        todo!()
    }
    pub fn mode_of(&self, id: u32) -> Option<ListenMode> {
        todo!()
    }
    pub fn member_ids(&self) -> Vec<u32> {
        todo!()
    }
    pub fn anchor(&self) -> Option<Anchor> {
        todo!()
    }
    pub fn next(&self) -> Option<Next> {
        todo!()
    }
    pub fn phase(&self) -> Phase {
        todo!()
    }
    pub fn preparing(&self) -> Option<(PrepId, EntryId, i64, Purpose)> {
        todo!()
    }
    pub fn queue(&self) -> &[Entry] {
        todo!()
    }
    pub fn has_ended(&self) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests;
