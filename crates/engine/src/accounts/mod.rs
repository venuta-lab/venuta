//! Signed-in services (Spotify first): see docs/decisions/0005-accounts-oauth.md.
//!
//! What stays between runs is one small record per service in the system's credential store
//! (Windows Credential Manager): the refresh token, whose account it is, and the app it was
//! given to. Access tokens live only in memory.

pub mod oauth;
pub mod spotify;

use serde::{Deserialize, Serialize};

/// What is kept for a signed-in service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub refresh_token: String,
    /// The service's id for you, and the name it shows.
    pub user_id: String,
    pub name: String,
    /// The Client ID it was given to; another one means signing in again.
    pub client_id: String,
}

/// Somewhere secrets can be kept: the system's credential store, or memory in tests.
pub trait Vault {
    fn put(&self, key: &str, value: &str) -> Result<(), String>;
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// The system's credential store (Windows Credential Manager, macOS Keychain, Secret Service).
/// A secret of any length: one entry holds at most 2,560 bytes on Windows (1,280 characters),
/// so a longer one is kept in parts.
pub struct SystemVault;

const VAULT_SERVICE: &str = "Venuta";
/// A test copy (VENUTA_MULTI) keeps its sign-ins apart, so signing in or out there never
/// touches the Venuta in use.
const TEST_VAULT_SERVICE: &str = "Venuta test copy";

fn vault_service() -> &'static str {
    if std::env::var_os("VENUTA_MULTI").is_some() {
        TEST_VAULT_SERVICE
    } else {
        VAULT_SERVICE
    }
}

impl Vault for SystemVault {
    fn put(&self, key: &str, value: &str) -> Result<(), String> {
        put_long(&Keyring, key, value).map_err(|e| format!("Couldn't keep the sign-in safely: {e}"))
    }
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        get_long(&Keyring, key).map_err(|e| format!("Couldn't read the sign-in: {e}"))
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        delete_long(&Keyring, key).map_err(|e| format!("Couldn't forget the sign-in: {e}"))
    }
}

/// Single entries of a store, each one limited in size.
trait Entries {
    fn put(&self, key: &str, value: &str) -> Result<(), String>;
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

struct Keyring;

impl Entries for Keyring {
    fn put(&self, key: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(vault_service(), key)
            .and_then(|e| e.set_password(value))
            .map_err(|e| e.to_string())
    }
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(vault_service(), key).and_then(|e| e.get_password()) {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        match keyring::Entry::new(vault_service(), key).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// UTF-16 units in one entry: Windows keeps 2,560 bytes, and the header's room is kept spare.
const PART_UNITS: usize = 1_200;
/// What a secret kept in parts starts with: `venuta-parts:1:<count>:<id>` (the id is the
/// secret's hash, which also names its parts and checks them when read back).
const PARTS_HEADER: &str = "venuta-parts:1:";

/// Its parts, each at most `PART_UNITS` UTF-16 units, never splitting a character.
fn split_units(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut units) = (0, 0);
    for (i, c) in value.char_indices() {
        if units + c.len_utf16() > PART_UNITS {
            parts.push(&value[start..i]);
            (start, units) = (i, 0);
        }
        units += c.len_utf16();
    }
    parts.push(&value[start..]);
    parts
}

fn parts_id(value: &str) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(value.as_bytes());
    hash[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The header of a secret kept in parts: how many, and their id.
fn header_of(entry: &str) -> Option<(usize, &str)> {
    let rest = entry.strip_prefix(PARTS_HEADER)?;
    let (count, id) = rest.split_once(':')?;
    Some((count.parse().ok()?, id))
}

fn part_key(key: &str, id: &str, i: usize) -> String {
    format!("{key}#{id}.{i}")
}

/// Keep a secret: as one entry when it fits (just as before), else in parts. New parts are
/// written before the header points at them, and the old ones go after, so a read never meets
/// half of one secret and half of another.
fn put_long(s: &dyn Entries, key: &str, value: &str) -> Result<(), String> {
    let old = s.get(key)?;
    let fits = value.encode_utf16().count() <= PART_UNITS && !value.starts_with(PARTS_HEADER);
    if fits {
        s.put(key, value)?;
    } else {
        let id = parts_id(value);
        let parts = split_units(value);
        for (i, p) in parts.iter().enumerate() {
            s.put(&part_key(key, &id, i), p)?;
        }
        s.put(key, &format!("{PARTS_HEADER}{}:{id}", parts.len()))?;
        if old
            .as_deref()
            .and_then(header_of)
            .is_some_and(|(_, was)| was == id)
        {
            return Ok(());
        }
    }
    if let Some((count, id)) = old.as_deref().and_then(header_of) {
        for i in 0..count {
            s.delete(&part_key(key, id, i))?;
        }
    }
    Ok(())
}

/// A kept secret, whole. One with a part missing or changed reads as nothing kept.
fn get_long(s: &dyn Entries, key: &str) -> Result<Option<String>, String> {
    let Some(entry) = s.get(key)? else {
        return Ok(None);
    };
    let Some((count, id)) = header_of(&entry) else {
        return Ok(Some(entry));
    };
    let mut value = String::new();
    for i in 0..count {
        match s.get(&part_key(key, id, i))? {
            Some(p) => value.push_str(&p),
            None => return Ok(None),
        }
    }
    Ok((parts_id(&value) == id).then_some(value))
}

fn delete_long(s: &dyn Entries, key: &str) -> Result<(), String> {
    if let Some((count, id)) = s.get(key)?.as_deref().and_then(header_of) {
        for i in 0..count {
            s.delete(&part_key(key, id, i))?;
        }
    }
    s.delete(key)
}

fn key(service: &str) -> String {
    format!("account:{service}")
}

pub fn save(v: &dyn Vault, service: &str, s: &Saved) -> Result<(), String> {
    v.put(
        &key(service),
        &serde_json::to_string(s).map_err(|e| e.to_string())?,
    )
}

/// The kept sign-in for a service, if it was given to this Client ID.
pub fn load(v: &dyn Vault, service: &str, client_id: &str) -> Result<Option<Saved>, String> {
    let Some(text) = v.get(&key(service))? else {
        return Ok(None);
    };
    let s: Saved = match serde_json::from_str(&text) {
        Ok(s) => s,
        // Unreadable: as good as signed out.
        Err(_) => return Ok(None),
    };
    Ok((s.client_id == client_id).then_some(s))
}

pub fn forget(v: &dyn Vault, service: &str) -> Result<(), String> {
    v.delete(&key(service))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);
    impl Vault for Memory {
        fn put(&self, k: &str, v: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn get(&self, k: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn delete(&self, k: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }

    fn saved() -> Saved {
        Saved {
            refresh_token: "RT".into(),
            user_id: "me".into(),
            name: "Akash".into(),
            client_id: "C1".into(),
        }
    }

    #[test]
    fn a_sign_in_is_kept_for_its_client_id_and_forgotten_on_request() {
        let v = Memory::default();
        assert_eq!(load(&v, "spotify", "C1").unwrap(), None);
        save(&v, "spotify", &saved()).unwrap();
        assert_eq!(load(&v, "spotify", "C1").unwrap(), Some(saved()));
        // Another app (Client ID) can't use this sign-in.
        assert_eq!(load(&v, "spotify", "C2").unwrap(), None);
        forget(&v, "spotify").unwrap();
        assert_eq!(load(&v, "spotify", "C1").unwrap(), None);
        v.put("account:spotify", "not json").unwrap();
        assert_eq!(load(&v, "spotify", "C1").unwrap(), None);
    }

    /// Entries the way Windows Credential Manager keeps them: at most 2,560 bytes each, as
    /// UTF-16 (1,280 characters), refusing longer ones like it does.
    #[derive(Default)]
    struct Limited(Mutex<HashMap<String, String>>);
    impl Entries for Limited {
        fn put(&self, k: &str, v: &str) -> Result<(), String> {
            if v.encode_utf16().count() * 2 > 2560 {
                return Err("longer than platform limit of 2560 chars".into());
            }
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn get(&self, k: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn delete(&self, k: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }

    /// A signed-in YouTube cookie jar is several thousand characters.
    fn long_secret(n: usize) -> String {
        (0..n)
            .map(|i| format!("__Secure-{i}PSIDTS=sidts-{i:04}abcDEF; "))
            .collect()
    }

    #[test]
    fn a_secret_longer_than_one_entry_is_kept_whole() {
        let s = Limited::default();
        let jar = long_secret(300);
        assert!(jar.len() > 9_000);
        put_long(&s, "account:youtube-music", &jar).unwrap();
        assert_eq!(get_long(&s, "account:youtube-music").unwrap(), Some(jar));
    }

    #[test]
    fn a_short_secret_stays_one_plain_entry_and_old_ones_still_read() {
        let s = Limited::default();
        put_long(&s, "account:spotify", "{\"refresh_token\":\"RT\"}").unwrap();
        assert_eq!(s.0.lock().unwrap().len(), 1);
        assert_eq!(
            s.get("account:spotify").unwrap().as_deref(),
            Some("{\"refresh_token\":\"RT\"}"),
            "kept exactly as before"
        );
        s.put("account:old", "kept by an older Venuta").unwrap();
        assert_eq!(
            get_long(&s, "account:old").unwrap().as_deref(),
            Some("kept by an older Venuta")
        );
    }

    #[test]
    fn a_secret_written_again_leaves_no_old_parts() {
        let s = Limited::default();
        put_long(&s, "k", &long_secret(300)).unwrap();
        put_long(&s, "k", &long_secret(120)).unwrap();
        let parts = (long_secret(120).encode_utf16().count()).div_ceil(PART_UNITS);
        assert_eq!(
            s.0.lock().unwrap().len(),
            1 + parts,
            "the header and its parts only"
        );
        assert_eq!(get_long(&s, "k").unwrap(), Some(long_secret(120)));
        put_long(&s, "k", "short now").unwrap();
        assert_eq!(s.0.lock().unwrap().len(), 1);
        assert_eq!(get_long(&s, "k").unwrap().as_deref(), Some("short now"));
    }

    #[test]
    fn forgetting_a_long_secret_removes_every_part() {
        let s = Limited::default();
        put_long(&s, "k", &long_secret(300)).unwrap();
        s.put("other", "stays").unwrap();
        delete_long(&s, "k").unwrap();
        assert_eq!(get_long(&s, "k").unwrap(), None);
        assert_eq!(s.0.lock().unwrap().len(), 1, "only the other entry is left");
        delete_long(&s, "k").unwrap();
    }

    #[test]
    fn a_damaged_or_missing_part_reads_as_nothing_kept() {
        let s = Limited::default();
        put_long(&s, "k", &long_secret(300)).unwrap();
        let part =
            s.0.lock()
                .unwrap()
                .keys()
                .find(|k| k.starts_with("k#"))
                .cloned()
                .unwrap();
        s.put(&part, "changed").unwrap();
        assert_eq!(get_long(&s, "k").unwrap(), None, "never a garbled secret");
        s.delete(&part).unwrap();
        assert_eq!(get_long(&s, "k").unwrap(), None);
    }

    #[test]
    fn parts_never_split_a_character() {
        let s = Limited::default();
        let text: String = "क🎵é".repeat(1_500);
        put_long(&s, "k", &text).unwrap();
        assert_eq!(get_long(&s, "k").unwrap(), Some(text));
    }

    /// Writes one entry to the real credential store and removes it again: run by hand with
    /// `cargo test -p engine system_vault -- --ignored`.
    #[test]
    #[ignore]
    fn system_vault_round_trip() {
        let v = SystemVault;
        let k = "test:venuta-vault-check";
        v.put(k, "secret value").unwrap();
        assert_eq!(v.get(k).unwrap().as_deref(), Some("secret value"));
        v.delete(k).unwrap();
        assert_eq!(v.get(k).unwrap(), None);
        v.delete(k).unwrap();
        // A signed-in cookie jar, far past one entry's 2,560 bytes.
        let long = long_secret(250);
        v.put(k, &long).unwrap();
        assert_eq!(v.get(k).unwrap(), Some(long));
        v.put(k, "short again").unwrap();
        assert_eq!(v.get(k).unwrap().as_deref(), Some("short again"));
        v.delete(k).unwrap();
        assert_eq!(v.get(k).unwrap(), None);
    }
}
