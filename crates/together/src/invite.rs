//! The invite people share (a link, a QR code or a code to paste): how to reach the host's
//! Venuta, a fresh random secret, and when it stops working.
//!
//! The link alone isn't enough to come in: the host still lets each person in. The secret keeps
//! strangers from even asking: a guest proves it holds it, bound to its own connection's key,
//! so a proof seen once can't be used by anyone else.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

/// What links start with: clicking one opens Venuta (the app registers it).
pub const LINK_PREFIX: &str = "venuta://join/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invite {
    /// How to reach the host (the network layer's own ticket, carried as it is).
    pub to: String,
    /// The room's secret, 16 random bytes as hex.
    pub secret: String,
    /// When the invite stops working (seconds since 1970).
    pub until: u64,
    /// The host's name, to say "Join Aakash's room?" before connecting.
    pub host: String,
}

impl Invite {
    /// The code people share.
    pub fn code(&self) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("an invite serializes"))
    }

    /// The link form of the code.
    pub fn link(&self) -> String {
        format!("{LINK_PREFIX}{}", self.code())
    }

    /// An invite from a code or a link (spaces and line breaks from chat apps are ignored).
    pub fn parse(text: &str) -> Result<Invite, String> {
        let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let code = t.strip_prefix(LINK_PREFIX).unwrap_or(&t);
        if code.len() > 4_000 {
            return Err("That isn't a Venuta invite.".into());
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(code)
            .map_err(|_| "That isn't a Venuta invite.".to_owned())?;
        let inv: Invite =
            serde_json::from_slice(&bytes).map_err(|_| "That isn't a Venuta invite.".to_owned())?;
        if inv.secret.len() != 32
            || !inv.secret.bytes().all(|b| b.is_ascii_hexdigit())
            || inv.host.chars().count() > 40
        {
            return Err("That isn't a Venuta invite.".into());
        }
        Ok(inv)
    }

    /// Whether it still works at `now` (seconds since 1970).
    pub fn open_at(&self, now: u64) -> bool {
        now < self.until
    }

    /// A guest's proof that it holds the secret, bound to its own connection key.
    pub fn proof(&self, guest_key: &str) -> String {
        proof(&self.secret, guest_key)
    }
}

/// The proof for a secret and a guest key (the host checks it the same way).
pub fn proof(secret: &str, guest_key: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"venuta-room-proof-v1");
    h.update(secret.as_bytes());
    h.update(guest_key.as_bytes());
    h.finalize().to_hex().to_string()
}

/// Whether a proof is right, compared in constant time.
pub fn proof_ok(secret: &str, guest_key: &str, given: &str) -> bool {
    let want = proof(secret, guest_key);
    want.len() == given.len()
        && want
            .bytes()
            .zip(given.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invite() -> Invite {
        Invite {
            to: "endpointabc123".into(),
            secret: "0123456789abcdef0123456789abcdef".into(),
            until: 2_000_000_000,
            host: "Aakash".into(),
        }
    }

    #[test]
    fn an_invite_goes_through_a_link_and_back() {
        let i = invite();
        assert_eq!(Invite::parse(&i.link()).unwrap(), i);
        assert_eq!(Invite::parse(&i.code()).unwrap(), i);
        // Chat apps wrap long text.
        let wrapped = format!("  {}\n{} ", &i.link()[..20], &i.link()[20..]);
        assert_eq!(Invite::parse(&wrapped).unwrap(), i);
        assert!(Invite::parse("hello there").is_err());
        assert!(
            Invite::parse(&format!("{LINK_PREFIX}e30")).is_err(),
            "an empty object"
        );
    }

    #[test]
    fn an_invite_ends() {
        let i = invite();
        assert!(i.open_at(1_999_999_999));
        assert!(!i.open_at(2_000_000_000));
    }

    #[test]
    fn a_proof_belongs_to_one_guest() {
        let i = invite();
        let p = i.proof("guest-key-1");
        assert!(proof_ok(&i.secret, "guest-key-1", &p));
        assert!(
            !proof_ok(&i.secret, "guest-key-2", &p),
            "someone else can't reuse it"
        );
        assert!(
            !proof_ok("ffffffffffffffffffffffffffffffff", "guest-key-1", &p),
            "another room's"
        );
        assert_eq!(p.len(), 64);
    }
}
