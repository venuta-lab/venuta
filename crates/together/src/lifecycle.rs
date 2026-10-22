//! A room's life cycle across restarts (design 4.15, decision D8, the WP15 notes in 9.2): what
//! the app keeps on disk so the host can reopen its room after the app restarts, and a guest
//! can rejoin after a crash, and what to offer when the app starts again.
//!
//! - [`HostRecord`]: the room's invite, its endpoint's key and port, the room's state
//!   ([`RoomState`]) and the app's own part; [`HostKeeper`] writes it as the room changes and
//!   removes it when the room ends.
//! - [`GuestRecord`]: the invite a guest joined with and when it was last in the room;
//!   [`GuestKeeper`] writes it while in, and removes it unless the session ended because the
//!   host went (who may come back).
//! - [`offer`]: what to offer at start ("Reopen your room?", "Rejoin Asha's room?") and
//!   whether to rejoin at once.
//! - [`Kept`]: the two files in a folder, each written whole (a temporary file renamed over).
//!
//! Platform neutral (files only), so Android keeps rooms the same way.

use crate::invite::Invite;
use crate::room::v2::{Place, RoomState};
use crate::session::{Ending, Stage};
use crate::view::Resume;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The records' version: a record of another is not offered.
pub const KEPT: u32 = 1;
/// The host's record, in the app's data folder.
pub const HOST_FILE: &str = "together-room.json";
/// A guest's record.
pub const GUEST_FILE: &str = "together-rejoin.json";
/// A guest last in the room this recently rejoins by itself (the room still keeps it, D17), s.
pub const REJOIN_BY_ITSELF_S: u64 = 120;
/// A guest in the room writes when it was last in this often, ms.
pub const IN_EVERY_MS: u64 = 10_000;
/// A playing room's place is written this often, ms (every other change at once).
pub const PLACE_EVERY_MS: u64 = 2_000;

/// What a host keeps for reopening its room (D8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostRecord {
    pub kept: u32,
    /// The room's invite: its secret and when it ends (24 h after it opened).
    pub invite: Invite,
    /// The room's endpoint's secret key, 32 bytes as hex: the same endpoint id comes back.
    pub key: String,
    /// The UDP port the endpoint was on, to have it again when it is free.
    pub port: Option<u16>,
    pub room: RoomState,
    /// When it was written (seconds since 1970).
    pub saved_at: u64,
    /// The app's own part (its music folder, its items by song key).
    #[serde(default)]
    pub app: serde_json::Value,
}

/// What a guest keeps for rejoining (I7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestRecord {
    pub kept: u32,
    /// The invite as joined (the link's, or the one made for a room nearby).
    pub invite: Invite,
    /// The host's name, for "Rejoin Asha's room?".
    pub host: String,
    /// When it was last in the room (seconds since 1970).
    pub in_at: u64,
}

/// What to offer at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub resume: Resume,
    /// Rejoin at once, without asking (a guest that was in the room under 2 minutes ago).
    pub auto: bool,
}

/// What to offer at `now` (seconds since 1970) from the records kept: a room whose invite has
/// ended is not offered; of two, the newer. Reopening is always asked (D8); rejoining is at
/// once when the guest was in the room under 2 minutes ago.
pub fn offer(host: Option<&HostRecord>, guest: Option<&GuestRecord>, now: u64) -> Option<Offer> {
    let h = host
        .filter(|h| h.kept == KEPT && h.invite.open_at(now))
        .map(|h| {
            (
                h.saved_at,
                Offer {
                    resume: Resume::Reopen,
                    auto: false,
                },
            )
        });
    let g = guest
        .filter(|g| g.kept == KEPT && g.invite.open_at(now))
        .map(|g| {
            (
                g.in_at,
                Offer {
                    resume: Resume::Rejoin {
                        host: g.host.clone(),
                    },
                    auto: g.in_at <= now && now - g.in_at < REJOIN_BY_ITSELF_S,
                },
            )
        });
    match (h, g) {
        (Some(h), Some(g)) => Some(if g.0 > h.0 { g.1 } else { h.1 }),
        (h, g) => h.or(g).map(|x| x.1),
    }
}

/// The records' folder (the app's data folder).
#[derive(Debug, Clone)]
pub struct Kept {
    dir: PathBuf,
}

impl Kept {
    pub fn new(dir: impl Into<PathBuf>) -> Kept {
        Kept { dir: dir.into() }
    }

    pub fn host(&self) -> Option<HostRecord> {
        self.read(HOST_FILE)
    }

    pub fn keep_host(&self, r: &HostRecord) -> Result<(), String> {
        self.write(HOST_FILE, r)
    }

    pub fn clear_host(&self) {
        let _ = std::fs::remove_file(self.dir.join(HOST_FILE));
    }

    pub fn guest(&self) -> Option<GuestRecord> {
        self.read(GUEST_FILE)
    }

    pub fn keep_guest(&self, r: &GuestRecord) -> Result<(), String> {
        self.write(GUEST_FILE, r)
    }

    pub fn clear_guest(&self) {
        let _ = std::fs::remove_file(self.dir.join(GUEST_FILE));
    }

    /// A record, if there is one that reads (a broken one is as none).
    fn read<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<T> {
        let bytes = std::fs::read(self.dir.join(name)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Written whole: to a temporary file, renamed over the record (never half a record).
    fn write<T: Serialize>(&self, name: &str, r: &T) -> Result<(), String> {
        let bytes = serde_json::to_vec(r).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.dir.join(name)).map_err(|e| e.to_string())
    }
}

/// The room's state as far as "something changed" goes: the place's moving content left out
/// (a seek or a pause changes the sequence number, so it still counts).
fn fingerprint(s: &RoomState) -> String {
    let mut t = s.clone();
    if let Place::At { entry, .. } = t.place {
        t.place = Place::At {
            entry,
            content_us: 0,
        };
    }
    serde_json::to_string(&t).unwrap_or_default()
}

/// Keeps the host's record as the room goes: written when it opens, at once on any change,
/// every 2 s while it plays; removed when it ends.
pub struct HostKeeper {
    kept: Kept,
    record: HostRecord,
    fingerprint: String,
    written_ms: u64,
    done: bool,
}

impl HostKeeper {
    /// The room opened (or reopened): its record written at once.
    pub fn new(kept: Kept, record: HostRecord) -> HostKeeper {
        let _ = kept.keep_host(&record);
        HostKeeper {
            fingerprint: fingerprint(&record.room),
            written_ms: record.saved_at.saturating_mul(1_000),
            kept,
            record,
            done: false,
        }
    }

    pub fn record(&self) -> &HostRecord {
        &self.record
    }

    /// The room as it is now (`playing`: its place moves), at `now_ms` (ms since 1970).
    pub fn step(&mut self, room: RoomState, playing: bool, now_ms: u64) {
        if self.done {
            return;
        }
        let fp = fingerprint(&room);
        let due = playing && now_ms >= self.written_ms + PLACE_EVERY_MS;
        if fp == self.fingerprint && !due {
            return;
        }
        self.record.room = room;
        self.record.saved_at = now_ms / 1_000;
        self.fingerprint = fp;
        self.written_ms = now_ms;
        let _ = self.kept.keep_host(&self.record);
    }

    /// The room ended (for everyone, or by itself): nothing to reopen.
    pub fn ended(&mut self) {
        if !self.done {
            self.done = true;
            self.kept.clear_host();
        }
    }
}

/// Keeps a guest's record as the session goes: written once in, when it was last in every
/// 10 s; at the end removed, unless the host went (it may come back: a rejoin is offered).
pub struct GuestKeeper {
    kept: Kept,
    record: GuestRecord,
    written_ms: Option<u64>,
    done: bool,
}

impl GuestKeeper {
    pub fn new(kept: Kept, invite: Invite, host: String) -> GuestKeeper {
        GuestKeeper {
            kept,
            record: GuestRecord {
                kept: KEPT,
                invite,
                host,
                in_at: 0,
            },
            written_ms: None,
            done: false,
        }
    }

    /// The session's stage and ending now, at `now_ms` (ms since 1970).
    pub fn step(&mut self, stage: Stage, ending: Option<Ending>, now_ms: u64) {
        if self.done {
            return;
        }
        match stage {
            Stage::In => {
                if self.written_ms.is_none_or(|w| now_ms >= w + IN_EVERY_MS) {
                    self.record.in_at = now_ms / 1_000;
                    self.written_ms = Some(now_ms);
                    let _ = self.kept.keep_guest(&self.record);
                }
            }
            Stage::Ended => {
                self.done = true;
                if ending != Some(Ending::Lost) {
                    self.kept.clear_guest();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::v2::{Opening, Room};

    fn invite(until: u64) -> Invite {
        Invite {
            to: "t".into(),
            secret: "0123456789abcdef0123456789abcdef".into(),
            until,
            host: "Asha".into(),
        }
    }

    fn host(saved_at: u64, until: u64) -> HostRecord {
        HostRecord {
            kept: KEPT,
            invite: invite(until),
            key: "00".repeat(32),
            port: None,
            room: Room::new("Asha", &invite(until).secret, Vec::new(), Opening::Resting).to_keep(0),
            saved_at,
            app: serde_json::Value::Null,
        }
    }

    fn guest(in_at: u64, until: u64) -> GuestRecord {
        GuestRecord {
            kept: KEPT,
            invite: invite(until),
            host: "Asha".into(),
            in_at,
        }
    }

    #[test]
    fn a_kept_room_is_offered_while_its_invite_lasts_and_never_by_itself() {
        let h = host(1_000, 5_000);
        assert_eq!(
            offer(Some(&h), None, 1_010),
            Some(Offer {
                resume: Resume::Reopen,
                auto: false
            })
        );
        assert_eq!(offer(Some(&h), None, 5_000), None, "the invite ended");
        let old = HostRecord { kept: 0, ..h };
        assert_eq!(offer(Some(&old), None, 1_010), None, "another version's");
    }

    #[test]
    fn a_guest_rejoins_by_itself_only_under_two_minutes_after_it_was_in() {
        let g = guest(1_000, 9_000);
        let at = |now| offer(None, Some(&g), now).map(|o| o.auto);
        assert_eq!(at(1_000), Some(true));
        assert_eq!(at(1_119), Some(true));
        assert_eq!(at(1_120), Some(false), "asked from 2 minutes");
        assert_eq!(at(999), Some(false), "a clock set back: asked");
        assert_eq!(at(9_000), None);
        assert_eq!(
            offer(None, Some(&g), 1_010).unwrap().resume,
            Resume::Rejoin {
                host: "Asha".into()
            }
        );
    }

    #[test]
    fn of_two_records_the_newer_is_offered() {
        let (h, g) = (host(2_000, 9_000), guest(1_500, 9_000));
        assert_eq!(
            offer(Some(&h), Some(&g), 2_010).unwrap().resume,
            Resume::Reopen
        );
        let g = guest(2_005, 9_000);
        assert!(matches!(
            offer(Some(&h), Some(&g), 2_010).unwrap().resume,
            Resume::Rejoin { .. }
        ));
    }

    #[test]
    fn records_go_through_their_files_and_a_broken_one_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("data");
        let k = Kept::new(&d);
        assert!(k.host().is_none() && k.guest().is_none());
        let (h, g) = (host(1, 2), guest(3, 4));
        k.keep_host(&h).unwrap();
        k.keep_guest(&g).unwrap();
        assert_eq!(k.host(), Some(h));
        assert_eq!(k.guest(), Some(g));
        std::fs::write(d.join(GUEST_FILE), b"{ half a rec").unwrap();
        assert!(k.guest().is_none());
        k.clear_host();
        k.clear_guest();
        assert!(k.host().is_none() && k.guest().is_none());
    }
}
