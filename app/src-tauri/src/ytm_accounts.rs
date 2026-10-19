//! More than one Google account: each sign-in kept on its own (its cookies in the system's
//! credential store, like the one in use), so you can switch between them without signing in
//! again. The one in use is still `account:youtube-music`; the others wait in their own slots.
//! Brand accounts (a channel's account) are switched inside one sign-in, in `ytm_library`.

use engine::accounts::Vault;
use media_access::RemoteAccount;
use serde::{Deserialize, Serialize};

const LIST_KEY: &str = "account:youtube-music:saved";
const ACTIVE_KEY: &str = "account:youtube-music:active";

fn slot(id: &str) -> String {
    format!("{LIST_KEY}:{id}")
}

/// A kept sign-in, as the switcher shows it (never its cookies).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAccount {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    /// The one in use (filled in when listed).
    #[serde(default, skip_deserializing)]
    pub current: bool,
}

/// What a sign-in is known by: its email, else its handle or name (as a key's last part).
pub fn id_of(who: &RemoteAccount) -> Option<String> {
    let raw = who
        .email
        .as_deref()
        .or(who.handle.as_deref())
        .or(who.name.as_deref())?
        .trim()
        .to_lowercase();
    let id: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '@' {
                c
            } else {
                '_'
            }
        })
        .collect();
    (!id.is_empty()).then_some(id)
}

/// The kept sign-ins, the one in use marked.
pub fn list(v: &dyn Vault) -> Vec<SavedAccount> {
    let active = v.get(ACTIVE_KEY).ok().flatten();
    let mut all: Vec<SavedAccount> = v
        .get(LIST_KEY)
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    for a in &mut all {
        a.current = active.as_deref() == Some(a.id.as_str());
    }
    all
}

fn write_list(v: &dyn Vault, all: &[SavedAccount]) -> Result<(), String> {
    let text = serde_json::to_string(all).map_err(|e| e.to_string())?;
    v.put(LIST_KEY, &text)
}

/// A sign-in that just worked: kept (or brought up to date) and made the one in use.
pub fn remember(v: &dyn Vault, who: &RemoteAccount, jar: &str) -> Result<String, String> {
    let id = id_of(who).ok_or("YouTube Music didn't say which account this is.")?;
    let mut all = list(v);
    let entry = SavedAccount {
        id: id.clone(),
        name: who.name.clone(),
        email: who.email.clone(),
        picture: who.picture.clone(),
        current: false,
    };
    match all.iter_mut().find(|a| a.id == id) {
        Some(a) => *a = entry,
        None => all.push(entry),
    }
    v.put(&slot(&id), jar)?;
    write_list(v, &all)?;
    v.put(ACTIVE_KEY, &id)?;
    Ok(id)
}

/// The cookies in use, renewed by Google: kept in the slot of the one in use too.
pub fn keep_active(v: &dyn Vault, jar: &str) -> Result<(), String> {
    match v.get(ACTIVE_KEY)? {
        Some(id) => v.put(&slot(&id), jar),
        None => Ok(()),
    }
}

/// Which one is in use (None: not known yet, as after an older version's sign-in).
pub fn active(v: &dyn Vault) -> Option<String> {
    v.get(ACTIVE_KEY).ok().flatten()
}

/// Switch: the cookies in use go back to their slot, and this one's come out. Its cookies.
pub fn switch(v: &dyn Vault, id: &str, jar_now: Option<&str>) -> Result<String, String> {
    if let Some(now) = jar_now {
        keep_active(v, now)?;
    }
    let jar = v
        .get(&slot(id))?
        .ok_or("That account's sign-in isn't kept any more. Add it again.")?;
    v.put(ACTIVE_KEY, id)?;
    Ok(jar)
}

/// Forget a kept sign-in (one not in use; signing out does the one in use).
pub fn forget(v: &dyn Vault, id: &str) -> Result<(), String> {
    let mut all = list(v);
    all.retain(|a| a.id != id);
    let _ = v.delete(&slot(id));
    if active(v).as_deref() == Some(id) {
        let _ = v.delete(ACTIVE_KEY);
    }
    write_list(v, &all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Memory(RefCell<HashMap<String, String>>);
    impl Vault for Memory {
        fn put(&self, k: &str, v: &str) -> Result<(), String> {
            self.0.borrow_mut().insert(k.into(), v.into());
            Ok(())
        }
        fn get(&self, k: &str) -> Result<Option<String>, String> {
            Ok(self.0.borrow().get(k).cloned())
        }
        fn delete(&self, k: &str) -> Result<(), String> {
            self.0.borrow_mut().remove(k);
            Ok(())
        }
    }

    fn who(email: &str) -> RemoteAccount {
        RemoteAccount {
            name: Some(email.split('@').next().unwrap().into()),
            email: Some(email.into()),
            handle: None,
            picture: None,
        }
    }

    #[test]
    fn each_sign_in_is_kept_and_the_latest_is_in_use() {
        let v = Memory::default();
        remember(&v, &who("A@x.com"), "jar-a").unwrap();
        remember(&v, &who("b@x.com"), "jar-b").unwrap();
        let all = list(&v);
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().find(|a| a.current).unwrap().id, "b@x.com");
        // The same account again is brought up to date, not added twice.
        remember(&v, &who("a@x.com"), "jar-a2").unwrap();
        assert_eq!(list(&v).len(), 2);
        assert_eq!(v.get(&slot("a@x.com")).unwrap().as_deref(), Some("jar-a2"));
    }

    #[test]
    fn switching_keeps_the_renewed_cookies_of_the_one_left() {
        let v = Memory::default();
        remember(&v, &who("a@x.com"), "jar-a").unwrap();
        remember(&v, &who("b@x.com"), "jar-b").unwrap();
        let jar = switch(&v, "a@x.com", Some("jar-b-renewed")).unwrap();
        assert_eq!(jar, "jar-a");
        assert_eq!(
            v.get(&slot("b@x.com")).unwrap().as_deref(),
            Some("jar-b-renewed")
        );
        assert_eq!(active(&v).as_deref(), Some("a@x.com"));
        assert!(switch(&v, "nobody", None).is_err());
    }

    #[test]
    fn a_forgotten_sign_in_is_gone_with_its_cookies() {
        let v = Memory::default();
        remember(&v, &who("a@x.com"), "jar-a").unwrap();
        remember(&v, &who("b@x.com"), "jar-b").unwrap();
        forget(&v, "a@x.com").unwrap();
        assert_eq!(
            list(&v).iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["b@x.com"]
        );
        assert_eq!(v.get(&slot("a@x.com")).unwrap(), None);
        forget(&v, "b@x.com").unwrap();
        assert_eq!(active(&v), None);
    }

    #[test]
    fn an_account_is_known_by_its_email_in_a_key_safe_form() {
        assert_eq!(
            id_of(&who("Me.Name@Gmail.com")).as_deref(),
            Some("me.name@gmail.com")
        );
        let none = RemoteAccount {
            name: None,
            email: None,
            handle: None,
            picture: None,
        };
        assert_eq!(id_of(&none), None);
    }
}
