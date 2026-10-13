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
pub struct SystemVault;

const VAULT_SERVICE: &str = "Venuta";

impl Vault for SystemVault {
    fn put(&self, key: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(VAULT_SERVICE, key)
            .and_then(|e| e.set_password(value))
            .map_err(|e| format!("Couldn't keep the sign-in safely: {e}"))
    }
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(VAULT_SERVICE, key).and_then(|e| e.get_password()) {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Couldn't read the sign-in: {e}")),
        }
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        match keyring::Entry::new(VAULT_SERVICE, key).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Couldn't forget the sign-in: {e}")),
        }
    }
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
    }
}
