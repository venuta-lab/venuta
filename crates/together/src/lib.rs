//! Listen together: one person's Venuta hosts a room, friends join from theirs, and everyone
//! hears the same song at the same moment, each from the best copy they have (their own file,
//! the online catalog, or the host's file streamed to them).
//!
//! This crate is the room's logic, kept free of networking and audio so all of it is tested:
//! - [`proto`]: every message, versioned and checked strictly (peers are never trusted).
//! - [`room`]: the room the host keeps: people, roles, the queue, the timeline, the activity.
//! - [`clock`]: the shared clock, from pings (the NTP four-timestamp method).
//! - [`timeline`]: where the song should be at any moment, for starting and catching up.
//! - [`invite`]: the invite people share: who hosts, a secret, and when it ends.

pub mod clock;
pub mod clock_map;
pub mod invite;
pub mod proto;
pub mod room;
pub mod timeline;

/// The protocol version both sides speak; a Venuta speaking another is told to update.
pub const PROTOCOL: u32 = 2;

/// How many people a room holds, the host included (the host's upload carries streamed songs).
pub const MAX_PEOPLE: usize = 10;
