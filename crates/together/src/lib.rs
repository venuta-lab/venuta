//! Listen together: one person's Venuta hosts a room, friends join from theirs, and everyone
//! hears the same song at the same moment, each from the best copy they have (their own file,
//! the online catalog, or the host's file streamed to them).
//!
//! This crate is the room's logic, kept free of networking and audio so all of it is tested:
//! - [`proto`]: every message, versioned and checked strictly (peers are never trusted).
//! - [`room`]: the room the host keeps: people, roles, the queue, the timeline, the activity.
//! - [`clock`]: the shared clock, from pings (the NTP four-timestamp method); [`clock_link`]
//!   times the pings on any transport.
//! - [`timeline`]: where the song should be at any moment, for starting and catching up.
//! - [`invite`]: the invite people share: who hosts, a secret, and when it ends.
//!
//! The contracts of the Listen together redesign (section 5 of `docs/design/listen-together.md`)
//! are declared here so every package builds against the same shapes, with the fakes and the
//! simulation behind them. Nothing in the app uses them yet:
//! - [`time`]: the monotonic clock, real and simulated (5.1).
//! - [`clock_map`]: the line from local time to the host's (5.5).
//! - [`anchor`]: entries, the anchor, the armed next and transitions (4.4).
//! - [`align`]: envelopes and alignments between copies (5.8).
//! - [`room_player`]: the room's hold on a player (5.4); [`fakes`] for both sides.
//! - [`transport`]: what a session sends and receives (5.6); [`sim_net`], a simulated network.
//! - [`view`]: what the window shows (5.9).

pub mod align;
pub mod anchor;
pub mod clock;
pub mod clock_link;
pub mod clock_map;
pub mod fakes;
pub mod invite;
pub mod proto;
pub mod room;
pub mod room_player;
pub mod sim_net;
pub mod time;
pub mod timeline;
pub mod transport;
pub mod view;

/// The protocol version both sides speak; a Venuta speaking another is told to update.
pub const PROTOCOL: u32 = 2;

/// How many people a room holds, the host included (the host's upload carries streamed songs).
pub const MAX_PEOPLE: usize = 10;
