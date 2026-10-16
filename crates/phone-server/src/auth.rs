//! Signing in, the Subsonic way: each phone has its own name and a password Venuta made for it
//! (never one of yours). Apps send either the password, or a salt and md5(password + salt).

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phone {
    /// What you called it ("Aakash's phone").
    pub name: String,
    /// What the app signs in with.
    pub user: String,
    pub password: String,
}

/// Why a sign-in was refused (Subsonic error 40 either way: nothing about which part was wrong).
#[derive(Debug, PartialEq, Eq)]
pub struct Refused;

/// The phone these parameters sign in as.
pub fn check<'a>(
    phones: &'a [Phone],
    param: impl Fn(&str) -> Option<&'a str>,
) -> Result<&'a Phone, Refused> {
    let user = param("u").ok_or(Refused)?;
    let phone = phones.iter().find(|p| p.user == user).ok_or(Refused)?;
    let given = if let (Some(token), Some(salt)) = (param("t"), param("s")) {
        if salt.len() < 6 {
            return Err(Refused);
        }
        let want = hex(&Md5::digest(format!("{}{salt}", phone.password).as_bytes()));
        same(&want, &token.to_ascii_lowercase())
    } else if let Some(p) = param("p") {
        let p = match p.strip_prefix("enc:") {
            Some(h) => unhex(h).ok_or(Refused)?,
            None => p.to_owned(),
        };
        same(&p, &phone.password)
    } else {
        false
    };
    if given { Ok(phone) } else { Err(Refused) }
}

/// A fresh password for a phone: 20 letters and digits, from the system's random source.
pub fn new_password() -> String {
    const ABC: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("the system's random source");
    bytes
        .iter()
        .map(|b| ABC[*b as usize % ABC.len()] as char)
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

/// Equal, taking as long whatever the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phones() -> Vec<Phone> {
        vec![Phone {
            name: "Phone".into(),
            user: "phone".into(),
            password: "sesame".into(),
        }]
    }

    fn sign_in(pairs: &[(&str, &str)]) -> Result<String, Refused> {
        let p = phones();
        check(&p, |k| {
            pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| *v)
        })
        .map(|ph| ph.name.clone())
    }

    #[test]
    fn a_phone_signs_in_with_its_token_or_its_password() {
        // md5("sesame" + "c19b2d"), as the Subsonic docs work it.
        let t = hex(&Md5::digest(b"sesamec19b2d"));
        assert!(sign_in(&[("u", "phone"), ("t", &t), ("s", "c19b2d")]).is_ok());
        assert!(sign_in(&[("u", "phone"), ("p", "sesame")]).is_ok());
        assert!(sign_in(&[("u", "phone"), ("p", "enc:736573616d65")]).is_ok());

        assert_eq!(sign_in(&[("u", "phone"), ("p", "nope")]), Err(Refused));
        assert_eq!(
            sign_in(&[("u", "phone"), ("t", &t), ("s", "other1")]),
            Err(Refused)
        );
        assert_eq!(sign_in(&[("u", "someone"), ("p", "sesame")]), Err(Refused));
        assert_eq!(sign_in(&[("u", "phone")]), Err(Refused));
        assert_eq!(sign_in(&[("p", "sesame")]), Err(Refused));
    }

    #[test]
    fn new_passwords_are_long_and_differ() {
        let a = new_password();
        let b = new_password();
        assert_eq!(a.len(), 20);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b);
    }
}
