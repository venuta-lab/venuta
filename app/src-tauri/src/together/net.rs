//! The connection under a room is `together_net`'s; this is where the app's settings say how
//! this Venuta reaches others.

pub use together_net::net::*;

/// How this Venuta connects, from Settings.
pub fn reach_from_settings() -> Reach {
    let s = engine::settings::load();
    Reach {
        local_only: s.listening.local_only,
        hide_ip: s.listening.together_hide_ip,
        relay: s
            .listening
            .together_relay
            .clone()
            .filter(|r| !r.trim().is_empty()),
    }
}
