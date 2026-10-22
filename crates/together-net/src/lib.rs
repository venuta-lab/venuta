//! Listen together's connections, kept apart from the window and the audio so they can be
//! tested on their own and used on any platform:
//! - [`net`]: the endpoint (iroh: QUIC, encrypted end to end, a peer dialled by its key), how
//!   this Venuta reaches others, the invite's ticket, message lines, the session clock.
//! - [`host`]: hosting a room: the door (knock, let in, refuse), each person served, the
//!   room's news sent, and the host's files and covers streamed to the people in it.
//! - [`pace`]: how the host's files are paced.
//! - [`guest`]: joining one: connect, knock, read the room's news, ping the host's clock,
//!   reconnect as the same person, and a small server on 127.0.0.1 that plays the host's files.
//!
//! What a session needs of the app around it comes in through [`host::HostHooks`] and
//! [`guest::GuestHooks`].

pub mod guest;
pub mod host;
pub mod net;
pub mod pace;
