//! What each character of the Indian scripts is.
//!
//! The Unicode blocks of the Indian scripts are laid out alike (after ISCII): a letter sits at
//! the same offset from its block's start in each of them, so one table read by offset covers
//! them all, and each script lists only where it differs.

use crate::Script;

/// A consonant, by sound class (the letter at that place in every script).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C {
    K,
    Kh,
    G,
    Gh,
    Ng,
    C,
    Ch,
    J,
    Jh,
    Ny,
    Tt,
    Tth,
    Dd,
    Ddh,
    Nn,
    T,
    Th,
    D,
    Dh,
    N,
    /// Tamil ன, Malayalam ഩ.
    Nnn,
    P,
    Ph,
    B,
    Bh,
    M,
    Y,
    R,
    /// Tamil ற, Malayalam റ, Telugu ఱ.
    Rr,
    L,
    /// ळ ள ള ಳ ళ ଳ ળ ਲ਼.
    Ll,
    /// ऴ ழ ഴ ೞ: "zh".
    Lll,
    V,
    Sh,
    Ss,
    S,
    H,
    // With a nukta.
    Q,
    Khh,
    Ghh,
    Z,
    /// ड़ ড় ଡ଼ ੜ: a flapped d (or r).
    Rd,
    /// ढ़ ঢ় ଢ଼.
    Rh,
    F,
    /// य़ য় ୟ: the "y" of Bengali and Odia.
    Yy,
    /// Bengali য, Odia ଯ: "j" (a "y" after another consonant).
    Ya,
    /// Odia ୱ, Assamese ৱ.
    W,
}

impl C {
    pub fn is_nasal(self) -> bool {
        matches!(self, C::Ng | C::Ny | C::Nn | C::N | C::Nnn | C::M)
    }

    pub fn is_labial(self) -> bool {
        matches!(self, C::P | C::Ph | C::B | C::Bh | C::M | C::F)
    }

    /// The consonant this one becomes with a nukta.
    pub fn nukta(self) -> C {
        match self {
            C::K => C::Q,
            C::Kh => C::Khh,
            C::G => C::Ghh,
            C::J => C::Z,
            C::Dd => C::Rd,
            C::Ddh => C::Rh,
            C::Ph => C::F,
            C::Y | C::Ya => C::Yy,
            C::S => C::Sh,
            C::L => C::Ll,
            other => other,
        }
    }
}

/// A vowel, written as a letter of its own or as a sign on a consonant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V {
    A,
    Aa,
    I,
    Ii,
    U,
    Uu,
    /// ऋ ृ
    R,
    Rr,
    L,
    /// ॅ ऍ ॲ: the "a" of "cat" in English words.
    Ec,
    /// Short e (Dravidian ె ெ െ ೆ ऎ).
    E,
    /// ए े (long e in the Dravidian scripts).
    Ee,
    Ai,
    /// ॉ ऑ: the "o" of "college".
    Oc,
    /// Short o.
    O,
    Oo,
    Au,
}

/// What a character is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cons(C),
    /// A vowel letter standing alone.
    Vowel(V),
    /// A vowel sign on the consonant before it.
    Sign(V),
    Virama,
    Nukta,
    Anusvara,
    Candrabindu,
    Visarga,
    /// Gurmukhi ੱ: the next consonant is doubled.
    Addak,
    /// The second part of a two-part vowel sign, or a length mark (ௗ ൗ ৗ ୗ ೕ ೖ ౖ ୖ).
    Mark(V),
    /// A consonant with no vowel, written as one letter (Malayalam chillu, Bengali ৎ, Malayalam
    /// dot reph).
    Dead(C),
    /// Gurmukhi ੲ ੳ: carries a vowel sign; a vowel letter of its own.
    Bearer(V),
    Digit(u8),
    /// । ॥
    Danda,
    Om,
    Avagraha,
    /// Joiners, accents and other marks with no sound of their own.
    Silent,
    /// Read as this text (symbols, fractions, ੴ).
    Text(&'static str),
    /// Not a character of these scripts.
    Other,
}

impl Kind {
    /// Part of a word (letters and the marks on them).
    pub fn in_word(self) -> bool {
        !matches!(
            self,
            Kind::Digit(_) | Kind::Danda | Kind::Om | Kind::Avagraha | Kind::Text(_) | Kind::Other
        )
    }
}

/// The first code point of the script's block.
pub fn block_start(script: Script) -> Option<u32> {
    Some(match script {
        Script::Deva => 0x900,
        Script::Beng => 0x980,
        Script::Guru => 0xA00,
        Script::Gujr => 0xA80,
        Script::Orya => 0xB00,
        Script::Taml => 0xB80,
        Script::Telu => 0xC00,
        Script::Knda => 0xC80,
        Script::Mlym => 0xD00,
        _ => return None,
    })
}

/// What `ch` is, read as a letter of `script`.
pub fn kind(script: Script, ch: char) -> Kind {
    let c = ch as u32;
    if c == 0x200C || c == 0x200D || (0x300..=0x36F).contains(&c) || (0x1CD0..=0x1CFF).contains(&c)
    {
        return Kind::Silent;
    }
    let Some(start) = block_start(script) else {
        return Kind::Other;
    };
    // A Devanagari danda or digit in a line of another script is read the same way.
    let (script, off) = if (0x900..0x980).contains(&c) {
        (Script::Deva, c - 0x900)
    } else {
        (script, c.wrapping_sub(start))
    };
    if off >= 0x80 {
        return Kind::Other;
    }
    if let Some(k) = special(script, off) {
        return k;
    }
    common(off)
}

/// The letters at each offset, as Devanagari has them.
fn common(off: u32) -> Kind {
    use Kind::*;
    match off {
        0x00 | 0x01 => Candrabindu,
        0x02 => Anusvara,
        0x03 => Visarga,
        0x04 | 0x05 => Vowel(V::A),
        0x06 => Vowel(V::Aa),
        0x07 => Vowel(V::I),
        0x08 => Vowel(V::Ii),
        0x09 => Vowel(V::U),
        0x0A => Vowel(V::Uu),
        0x0B => Vowel(V::R),
        0x0C => Vowel(V::L),
        0x0D => Vowel(V::Ec),
        0x0E => Vowel(V::E),
        0x0F => Vowel(V::Ee),
        0x10 => Vowel(V::Ai),
        0x11 => Vowel(V::Oc),
        0x12 => Vowel(V::O),
        0x13 => Vowel(V::Oo),
        0x14 => Vowel(V::Au),
        0x15 => Cons(C::K),
        0x16 => Cons(C::Kh),
        0x17 => Cons(C::G),
        0x18 => Cons(C::Gh),
        0x19 => Cons(C::Ng),
        0x1A => Cons(C::C),
        0x1B => Cons(C::Ch),
        0x1C => Cons(C::J),
        0x1D => Cons(C::Jh),
        0x1E => Cons(C::Ny),
        0x1F => Cons(C::Tt),
        0x20 => Cons(C::Tth),
        0x21 => Cons(C::Dd),
        0x22 => Cons(C::Ddh),
        0x23 => Cons(C::Nn),
        0x24 => Cons(C::T),
        0x25 => Cons(C::Th),
        0x26 => Cons(C::D),
        0x27 => Cons(C::Dh),
        0x28 => Cons(C::N),
        0x29 => Cons(C::Nnn),
        0x2A => Cons(C::P),
        0x2B => Cons(C::Ph),
        0x2C => Cons(C::B),
        0x2D => Cons(C::Bh),
        0x2E => Cons(C::M),
        0x2F => Cons(C::Y),
        0x30 => Cons(C::R),
        0x31 => Cons(C::Rr),
        0x32 => Cons(C::L),
        0x33 => Cons(C::Ll),
        0x34 => Cons(C::Lll),
        0x35 => Cons(C::V),
        0x36 => Cons(C::Sh),
        0x37 => Cons(C::Ss),
        0x38 => Cons(C::S),
        0x39 => Cons(C::H),
        0x3C => Nukta,
        0x3D => Avagraha,
        0x3E => Sign(V::Aa),
        0x3F => Sign(V::I),
        0x40 => Sign(V::Ii),
        0x41 => Sign(V::U),
        0x42 => Sign(V::Uu),
        0x43 => Sign(V::R),
        0x44 => Sign(V::Rr),
        0x45 => Sign(V::Ec),
        0x46 => Sign(V::E),
        0x47 => Sign(V::Ee),
        0x48 => Sign(V::Ai),
        0x49 => Sign(V::Oc),
        0x4A => Sign(V::O),
        0x4B => Sign(V::Oo),
        0x4C | 0x4F => Sign(V::Au),
        0x4D => Virama,
        0x50 => Om,
        0x55 => Sign(V::Ec),
        0x56 => Sign(V::U),
        0x57 => Sign(V::Uu),
        0x58 => Cons(C::Q),
        0x59 => Cons(C::Khh),
        0x5A => Cons(C::Ghh),
        0x5B => Cons(C::Z),
        0x5C => Cons(C::Rd),
        0x5D => Cons(C::Rh),
        0x5E => Cons(C::F),
        0x5F => Cons(C::Yy),
        0x60 => Vowel(V::Rr),
        0x61 => Vowel(V::L),
        0x62 | 0x63 => Sign(V::L),
        0x64 | 0x65 => Danda,
        0x66..=0x6F => Digit((off - 0x66) as u8),
        0x70 => Text("."),
        0x72 => Vowel(V::Ec),
        0x73 | 0x74 => Vowel(V::Oo),
        0x75 => Vowel(V::Au),
        0x76 => Vowel(V::U),
        0x77 => Vowel(V::Uu),
        0x78 | 0x7E => Cons(C::Dd),
        0x79 => Cons(C::Z),
        0x7A => Cons(C::Y),
        0x7B => Cons(C::G),
        0x7C => Cons(C::J),
        0x7F => Cons(C::B),
        _ => Silent,
    }
}

/// Where a script differs from the common table.
fn special(script: Script, off: u32) -> Option<Kind> {
    use Kind::*;
    Some(match (script, off) {
        (Script::Beng, 0x2F) | (Script::Orya, 0x2F) => Cons(C::Ya),
        (Script::Beng, 0x4E) => Dead(C::T),
        (Script::Beng, 0x57)
        | (Script::Orya, 0x57)
        | (Script::Taml, 0x57)
        | (Script::Mlym, 0x57) => Mark(V::Au),
        (Script::Beng, 0x70) => Cons(C::R),
        (Script::Beng, 0x71) | (Script::Orya, 0x71) => Cons(C::W),
        (Script::Beng, 0x72..=0x7F) => Text(""),
        (Script::Guru, 0x33) => Cons(C::Ll),
        (Script::Guru, 0x36) => Cons(C::Sh),
        (Script::Guru, 0x70) => Anusvara,
        (Script::Guru, 0x71) => Addak,
        (Script::Guru, 0x72) => Bearer(V::I),
        (Script::Guru, 0x73) => Bearer(V::U),
        (Script::Guru, 0x74) => Text("ik onkar"),
        (Script::Guru, 0x75) => Silent,
        (Script::Gujr, 0x71) => Text(""),
        (Script::Gujr, 0x79) => Cons(C::Z),
        (Script::Gujr, 0x7A..=0x7F) => Silent,
        (Script::Orya, 0x56) => Mark(V::Ai),
        (Script::Orya, 0x70) | (Script::Orya, 0x72..=0x77) => Text(""),
        (Script::Taml, 0x03) => Visarga,
        (Script::Taml, 0x70..=0x7F) => Text(""),
        (Script::Telu, 0x56) => Mark(V::Ai),
        (Script::Telu, 0x55) => Silent,
        (Script::Telu, 0x58) => Cons(C::C),
        (Script::Telu, 0x59) => Cons(C::J),
        (Script::Telu, 0x5A) => Cons(C::Rr),
        (Script::Telu, 0x5D) => Dead(C::N),
        (Script::Telu, 0x78..=0x7F) => Text(""),
        (Script::Knda, 0x55) => Mark(V::Ii),
        (Script::Knda, 0x56) => Mark(V::Ai),
        (Script::Knda, 0x5D) => Dead(C::N),
        (Script::Knda, 0x5E) => Cons(C::Lll),
        (Script::Knda, 0x71) | (Script::Knda, 0x72) => Visarga,
        (Script::Knda, 0x73) => Anusvara,
        (Script::Mlym, 0x00) => Anusvara,
        (Script::Mlym, 0x3A) => Cons(C::Tt),
        (Script::Mlym, 0x3B) | (Script::Mlym, 0x3C) => Virama,
        (Script::Mlym, 0x4E) => Dead(C::R),
        (Script::Mlym, 0x4F) | (Script::Mlym, 0x58..=0x5E) | (Script::Mlym, 0x70..=0x79) => {
            Text("")
        }
        (Script::Mlym, 0x54) => Dead(C::M),
        (Script::Mlym, 0x55) => Dead(C::Y),
        (Script::Mlym, 0x56) => Dead(C::Lll),
        (Script::Mlym, 0x5F) => Vowel(V::Ii),
        (Script::Mlym, 0x7A) => Dead(C::Nn),
        (Script::Mlym, 0x7B) => Dead(C::N),
        (Script::Mlym, 0x7C) => Dead(C::R),
        (Script::Mlym, 0x7D) => Dead(C::L),
        (Script::Mlym, 0x7E) => Dead(C::Ll),
        (Script::Mlym, 0x7F) => Dead(C::K),
        _ => return None,
    })
}
