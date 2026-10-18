//! Where the room's song is at any moment: a position, the host's time it held, and whether it
//! moves. Every start, pause and seek is a new timeline; each listener's player follows it.
//!
//! Starting together: the host sets `at` a little ahead (time for everyone to get the song
//! ready); each player starts exactly then. Joining late or catching up: the position "now" is
//! `position + (now - at)` while playing, so a guest starts there, a moment ahead, and is with
//! the room without anyone waiting.

use serde::{Deserialize, Serialize};

/// The room's playback, on the host's clock (microseconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    /// Which song in the room's queue (its index) and the queue's revision it belongs to.
    pub index: usize,
    pub rev: u64,
    /// The song's position, in milliseconds, at the host's time `at`.
    pub position_ms: u64,
    pub at_us: i64,
    pub playing: bool,
}

impl Timeline {
    /// The song's position, in milliseconds, at the host's time `now_us` (before `at` a playing
    /// timeline is still waiting to start, so it's at `position`).
    pub fn position_at(&self, now_us: i64) -> u64 {
        if !self.playing || now_us <= self.at_us {
            return self.position_ms;
        }
        self.position_ms + ((now_us - self.at_us) / 1_000) as u64
    }

    /// The same song, paused where it is at `now_us`.
    pub fn paused_at(&self, now_us: i64) -> Timeline {
        Timeline {
            position_ms: self.position_at(now_us),
            at_us: now_us,
            playing: false,
            ..*self
        }
    }

    /// Playing from where it is, starting at the host's time `start_us`.
    pub fn playing_from(&self, start_us: i64) -> Timeline {
        Timeline {
            position_ms: self.position_at(start_us),
            at_us: start_us,
            playing: true,
            ..*self
        }
    }

    /// Moved to `position_ms`, (still) playing or not, from `start_us`.
    pub fn sought(&self, position_ms: u64, start_us: i64) -> Timeline {
        Timeline {
            position_ms,
            at_us: start_us,
            ..*self
        }
    }
}

/// How far ahead a start is set, for everyone to be ready and the message to arrive: twice the
/// slowest round trip, never under half a second (Jellyfin's rule).
pub fn lead_us(slowest_round_trip_us: i64) -> i64 {
    (2 * slowest_round_trip_us).max(500_000)
}

/// A late joiner (or one catching up) prepares this far ahead of the room: the time it takes to
/// get a song ready, plus room for the message.
pub fn catch_up_target(t: &Timeline, now_us: i64, ready_in_us: i64) -> (u64, i64) {
    let start = now_us + ready_in_us.max(0) + 300_000;
    (t.position_at(start), start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> Timeline {
        Timeline {
            index: 0,
            rev: 1,
            position_ms: 10_000,
            at_us: 1_000_000,
            playing: true,
        }
    }

    #[test]
    fn a_playing_song_moves_and_a_waiting_one_holds() {
        let t = playing();
        assert_eq!(t.position_at(500_000), 10_000, "before its start it waits");
        assert_eq!(t.position_at(3_500_000), 12_500);
        let p = t.paused_at(3_500_000);
        assert_eq!(p.position_at(9_000_000), 12_500, "paused stays put");
        assert_eq!(p.playing_from(10_000_000).position_at(11_000_000), 13_500);
        assert_eq!(t.sought(60_000, 4_000_000).position_at(5_000_000), 61_000);
    }

    #[test]
    fn starts_leave_room_for_the_slowest() {
        assert_eq!(lead_us(40_000), 500_000);
        assert_eq!(lead_us(400_000), 800_000);
    }

    #[test]
    fn a_late_joiner_lands_where_the_room_will_be() {
        let t = playing();
        let (pos, start) = catch_up_target(&t, 61_000_000, 2_000_000);
        assert_eq!(start, 63_300_000);
        assert_eq!(pos, 72_300, "60 s in, plus its 2.3 s to get ready");
    }
}
