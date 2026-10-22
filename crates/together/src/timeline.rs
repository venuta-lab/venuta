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

/// Protocol 3's timeline arithmetic (section 4.4): leads in µs, an anchor's end and the next
/// entry's boundary, and where a late joiner comes in. `Anchor` and `Next` replace `Timeline`.
pub mod v3 {
    use crate::anchor::{Anchor, Transition};

    /// A start's lead when everyone plays on the host's network, and otherwise.
    pub const START_FLOOR_ONE_NETWORK_US: i64 = 300_000;
    pub const START_FLOOR_US: i64 = 500_000;
    /// A start is never set further ahead than this.
    pub const START_LEAD_MAX_US: i64 = 2_000_000;
    /// A pause's lead on one network; otherwise twice the round trip, within these.
    pub const PAUSE_LEAD_ONE_NETWORK_US: i64 = 150_000;
    pub const PAUSE_LEAD_MAX_US: i64 = 400_000;
    /// The next entry is armed this long before its boundary.
    pub const ARM_AHEAD_US: i64 = 20_000_000;
    /// A late joiner comes in this much after it expects to be ready (room for the message).
    pub const CATCH_UP_MARGIN_US: i64 = 300_000;

    /// How far ahead a start, resume, seek or skip is committed: `max(floor, 2 x p95 round trip
    /// + 50 ms)`, at most 2 s.
    pub fn start_lead_us(p95_rtt_us: i64, one_network: bool) -> i64 {
        let floor = if one_network {
            START_FLOOR_ONE_NETWORK_US
        } else {
            START_FLOOR_US
        };
        (2 * p95_rtt_us.max(0) + 50_000).clamp(floor, START_LEAD_MAX_US)
    }

    /// How far ahead a pause is scheduled, so speakers fall silent together (decision D9).
    pub fn pause_lead_us(rtt_us: i64, one_network: bool) -> i64 {
        if one_network {
            PAUSE_LEAD_ONE_NETWORK_US
        } else {
            (2 * rtt_us.max(0)).clamp(PAUSE_LEAD_ONE_NETWORK_US, PAUSE_LEAD_MAX_US)
        }
    }

    /// How long a transition overlaps the end of a song of `length_us` (at most half of it).
    pub fn overlap_us(t: Transition, length_us: i64) -> i64 {
        match t {
            Transition::Crossfade { ms } => (i64::from(ms) * 1_000).min(length_us.max(0) / 2),
            Transition::Gapless | Transition::Cut => 0,
        }
    }

    /// The host time a playing anchor's entry ends, its reference copy `length_us` long.
    pub fn end_us(a: &Anchor, length_us: i64) -> Option<i64> {
        a.playing.then(|| a.host_us + (length_us - a.content_us))
    }

    /// The host time the next entry begins (or its crossfade begins).
    pub fn boundary_us(a: &Anchor, length_us: i64, t: Transition) -> Option<i64> {
        end_us(a, length_us).map(|end| end - overlap_us(t, length_us))
    }

    /// A late joiner (or one catching up) that needs `ready_in_us` to get ready: the room's
    /// content place and the host time it comes in at.
    pub fn catch_up(a: &Anchor, now_us: i64, ready_in_us: i64) -> (i64, i64) {
        let at = now_us + ready_in_us.max(0) + CATCH_UP_MARGIN_US;
        (a.content_at(at), at)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::anchor::EntryId;

        fn playing() -> Anchor {
            Anchor {
                entry: EntryId(4),
                content_us: 10_000_000,
                host_us: 1_000_000,
                playing: true,
                seq: 1,
            }
        }

        #[test]
        fn starts_lead_by_the_slowest_round_trip_within_their_floor_and_cap() {
            assert_eq!(start_lead_us(2_000, true), 300_000, "one Wi-Fi: the floor");
            assert_eq!(start_lead_us(2_000, false), 500_000);
            assert_eq!(start_lead_us(300_000, false), 650_000, "2 x 300 ms + 50 ms");
            assert_eq!(start_lead_us(5_000_000, false), 2_000_000, "never over 2 s");
        }

        #[test]
        fn pauses_lead_less() {
            assert_eq!(pause_lead_us(90_000, true), 150_000);
            assert_eq!(pause_lead_us(40_000, false), 150_000);
            assert_eq!(pause_lead_us(120_000, false), 240_000);
            assert_eq!(pause_lead_us(900_000, false), 400_000);
        }

        #[test]
        fn the_boundary_is_the_reference_end_less_the_crossfade() {
            let a = playing();
            // 200 s long, 10 s in at 1 s: it ends at 191 s.
            assert_eq!(end_us(&a, 200_000_000), Some(191_000_000));
            assert_eq!(
                boundary_us(&a, 200_000_000, Transition::Gapless),
                Some(191_000_000)
            );
            assert_eq!(
                boundary_us(&a, 200_000_000, Transition::Crossfade { ms: 4_000 }),
                Some(187_000_000)
            );
            assert_eq!(
                overlap_us(Transition::Crossfade { ms: 8_000 }, 6_000_000),
                3_000_000,
                "never more than half a short song"
            );
            let held = Anchor {
                playing: false,
                ..a
            };
            assert_eq!(end_us(&held, 200_000_000), None, "a held song doesn't end");
        }

        #[test]
        fn a_late_joiner_lands_where_the_room_will_be() {
            let (content, at) = catch_up(&playing(), 61_000_000, 2_000_000);
            assert_eq!(at, 63_300_000);
            assert_eq!(content, 72_300_000, "60 s in, plus its 2.3 s to get ready");
        }
    }
}
