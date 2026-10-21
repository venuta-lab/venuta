//! The room's shared timeline (section 4.4 of the Listen together design): entries with ids
//! that never change, the anchor that says which content is heard at which host time, and the
//! next entry armed ahead with its boundary.
//!
//! These replace today's `timeline::Timeline` (index, revision, milliseconds) once Room v2 and
//! protocol 3 land; until then nothing in the app uses them.

use serde::{Deserialize, Serialize};

/// One song in the room's queue, as the host numbers it: given once, never reused, so moving,
/// adding or removing songs never changes which entry plays or is armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

/// One prepare (a start, resume, seek, skip or jump getting ready). A newer prepare replaces an
/// older one; readiness for an old id is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrepId(pub u64);

/// The reference copy's content time `content_us` is heard at host time `host_us`; while
/// `playing`, it moves at exactly one second per second of host time. `seq` grows with every
/// new anchor, so a guest coming back applies only the newest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub entry: EntryId,
    pub content_us: i64,
    pub host_us: i64,
    pub playing: bool,
    pub seq: u64,
}

impl Anchor {
    /// Where the reference copy is at host time `host_us` (before `self.host_us` a playing
    /// anchor is still waiting to start, so it is at `content_us`).
    pub fn content_at(&self, host_us: i64) -> i64 {
        if self.playing && host_us > self.host_us {
            self.content_us + (host_us - self.host_us)
        } else {
            self.content_us
        }
    }
}

/// The entry after the playing one, armed 20 s ahead: each player switches to it at the
/// boundary by itself, with no message at that moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Next {
    pub entry: EntryId,
    /// The host time the next entry begins (or its crossfade begins).
    pub boundary_host_us: i64,
    pub transition: Transition,
    pub seq: u64,
}

/// How the room goes from one song to the next (the host's setting; your own crossfade is set
/// aside while you are in a room).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transition {
    Gapless,
    Crossfade { ms: u32 },
    Cut,
}

/// Why a prepare happens: it decides the splice at the commit (section 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// The first start: everyone from silence on one frame.
    Start,
    /// After a pause: from silence, a 15 ms fade-in.
    Resume,
    /// The same entry, another place: a 40 ms crossfade.
    Seek,
    /// Another entry, now: the old fades out over 30 ms ending at the switch.
    Skip,
    /// Another entry chosen from the queue: as a skip.
    Jump,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchor_moves_only_while_playing_and_after_its_time() {
        let a = Anchor {
            entry: EntryId(4),
            content_us: 95_000_000,
            host_us: 1_000_000,
            playing: true,
            seq: 1,
        };
        assert_eq!(a.content_at(500_000), 95_000_000, "waiting to start");
        assert_eq!(a.content_at(3_000_000), 97_000_000);
        let held = Anchor {
            playing: false,
            ..a
        };
        assert_eq!(held.content_at(3_000_000), 95_000_000);
    }

    #[test]
    fn the_wire_shapes_stay_as_agreed() {
        let n = Next {
            entry: EntryId(5),
            boundary_host_us: 7,
            transition: Transition::Crossfade { ms: 4_000 },
            seq: 2,
        };
        assert_eq!(
            serde_json::to_string(&n).unwrap(),
            r#"{"entry":5,"boundary_host_us":7,"transition":{"kind":"crossfade","ms":4000},"seq":2}"#
        );
        assert_eq!(
            serde_json::from_str::<Transition>(r#"{"kind":"gapless"}"#).unwrap(),
            Transition::Gapless
        );
    }
}
