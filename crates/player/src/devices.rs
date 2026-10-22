//! Sound outputs as devices (`docs/design/devices.md`): what each endpoint is (Bluetooth, USB,
//! wired, a display, virtual), which endpoints are one device (a Bluetooth device's stereo and
//! hands-free endpoints), which one plays for a choice (the chosen device, its stereo twin,
//! Windows' default, or a fallback while the chosen one is away), and the exclusive-mode formats
//! a device takes (probed once, planned from).
//!
//! Platform neutral: the platform reads its endpoints (`wasapi_devices` on Windows) and opens
//! outputs on them through the `Devices` trait; tests use `sim_devices::SimDevices`.

use crate::output::{DeviceFormat, SampleKind};
use crate::output_api::{OpenRequest, Output};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock};

/// An endpoint's state, as the platform says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
}

/// One render endpoint, as the platform read it (Windows: its property store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub id: String,
    /// What it is, in Windows' words: "Speakers", "Headphones", "Headset", a monitor's name
    /// (`PKEY_Device_DeviceDesc`).
    pub description: String,
    /// The device or adapter it belongs to: "AirPods Pro Hands-Free", "Realtek(R) Audio"
    /// (`PKEY_DeviceInterface_FriendlyName`).
    pub adapter: String,
    /// The bus that found it: `BTHENUM`, `BTHHFENUM`, `USB`, `HDAUDIO`, `INTELAUDIO`, `SWD`,
    /// `ROOT` (`PKEY_Device_EnumeratorName`).
    pub enumerator: String,
    /// `PKEY_AudioEndpoint_FormFactor` (see `form`).
    pub form_factor: u32,
    /// The physical device it is part of (`PKEY_Device_ContainerId`), braced, any case.
    pub container: Option<String>,
    pub state: EndpointState,
}

/// `PKEY_AudioEndpoint_FormFactor`'s values (`EndpointFormFactor`).
pub mod form_factor {
    pub const REMOTE_NETWORK: u32 = 0;
    pub const SPEAKERS: u32 = 1;
    pub const LINE_LEVEL: u32 = 2;
    pub const HEADPHONES: u32 = 3;
    pub const MICROPHONE: u32 = 4;
    pub const HEADSET: u32 = 5;
    pub const HANDSET: u32 = 6;
    pub const DIGITAL_PASSTHROUGH: u32 = 7;
    pub const SPDIF: u32 = 8;
    pub const DISPLAY: u32 = 9;
    pub const UNKNOWN: u32 = 10;
}

/// The computer's own container: its built-in devices share it, so it pairs nothing.
const MACHINE_CONTAINER: &str = "{00000000-0000-0000-ffff-ffffffffffff}";

/// What kind of output, for grouping and for what it can do. In the window's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Bluetooth,
    Usb,
    Wired,
    Hdmi,
    Virtual,
    Other,
}

/// What it is to listen to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    Speakers,
    Headphones,
    Headset,
    LineOut,
    Digital,
    Display,
    Other,
}

/// An endpoint's kind and form, and whether it is a Bluetooth device's hands-free (call)
/// endpoint: mono, 8 to 16 kHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class {
    pub kind: Kind,
    pub form: Form,
    pub hands_free: bool,
}

/// What an endpoint is, from its bus and form factor (never its name).
pub fn classify(e: &Endpoint) -> Class {
    use form_factor as ff;
    let en = e.enumerator.to_ascii_uppercase();
    let kind = if en.starts_with("BTH") {
        Kind::Bluetooth
    } else if en.starts_with("USB") {
        Kind::Usb
    } else if en == "SWD" || en == "ROOT" {
        Kind::Virtual
    } else if e.form_factor == ff::DISPLAY {
        Kind::Hdmi
    } else if !en.is_empty() {
        Kind::Wired
    } else {
        Kind::Other
    };
    let form = match e.form_factor {
        ff::SPEAKERS => Form::Speakers,
        ff::HEADPHONES => Form::Headphones,
        ff::HEADSET | ff::HANDSET => Form::Headset,
        ff::LINE_LEVEL => Form::LineOut,
        ff::SPDIF | ff::DIGITAL_PASSTHROUGH => Form::Digital,
        ff::DISPLAY => Form::Display,
        _ => Form::Other,
    };
    let hands_free = kind == Kind::Bluetooth
        && (en == "BTHHFENUM" || matches!(e.form_factor, ff::HEADSET | ff::HANDSET));
    Class {
        kind,
        form,
        hands_free,
    }
}

/// A Bluetooth device's own name, without what Windows adds for its hands-free endpoint.
fn base_name(adapter: &str) -> &str {
    let a = adapter.trim();
    for suffix in [
        " Hands-Free AG Audio",
        " Hands-Free HF Audio",
        " Hands-Free AG",
        " Hands-Free",
        " Stereo",
    ] {
        if a.len() > suffix.len()
            && a[a.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            && a.is_char_boundary(a.len() - suffix.len())
        {
            return a[..a.len() - suffix.len()].trim_end();
        }
    }
    a
}

/// Windows' own full name for an endpoint: "Headphones (AirPods Pro)".
fn full_name(e: &Endpoint) -> String {
    match (e.description.trim(), e.adapter.trim()) {
        ("", "") => "Sound output".into(),
        (d, "") => d.into(),
        ("", a) => a.into(),
        (d, a) => format!("{d} ({a})"),
    }
}

/// A clear name for a device and a detail line under it.
fn names(e: &Endpoint, c: Class) -> (String, String) {
    let or = |a: &str, b: &str| if a.trim().is_empty() { b } else { a }.trim().to_owned();
    match c.kind {
        Kind::Bluetooth => (or(base_name(&e.adapter), &e.description), String::new()),
        Kind::Usb => (or(&e.adapter, &e.description), String::new()),
        Kind::Wired => (or(&e.description, &e.adapter), e.adapter.trim().to_owned()),
        Kind::Hdmi => (or(&e.description, &e.adapter), e.adapter.trim().to_owned()),
        Kind::Virtual => (or(&e.adapter, &e.description), "Virtual device".into()),
        Kind::Other => (full_name(e), String::new()),
    }
}

/// A device as the picker lists it: one per device (a Bluetooth device's two endpoints are one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputEntry {
    /// The endpoint it plays on: the stereo one for a Bluetooth device that has both.
    pub id: String,
    pub name: String,
    pub detail: String,
    pub kind: Kind,
    pub form: Form,
    /// The hands-free (call) endpoint of the same device, never played while this one is here.
    pub hands_free: Option<String>,
    /// Only the hands-free endpoint is connected: mono, call quality.
    pub hands_free_only: bool,
    /// Windows' default output is this device (either of its endpoints).
    pub default: bool,
    pub connected: bool,
    /// Every endpoint of the device the platform knows (any state).
    #[serde(skip)]
    pub members: Vec<String>,
}

/// The key that makes endpoints one device: their container (never the computer's own),
/// else, for Bluetooth, the device's own name.
fn device_key(e: &Endpoint, c: Class) -> Option<String> {
    if c.kind != Kind::Bluetooth {
        return None;
    }
    match e.container.as_deref().map(str::to_ascii_lowercase) {
        Some(k) if k != MACHINE_CONTAINER && !k.is_empty() => Some(format!("c:{k}")),
        _ => Some(format!("n:{}", base_name(&e.adapter).to_lowercase())),
    }
}

/// The devices to list: every active endpoint (a Bluetooth device's two as one entry), and the
/// chosen one, not connected, when it is away. Sorted by kind, then name; two of one kind with
/// one name get Windows' full names.
pub fn entries(eps: &[Endpoint], default: Option<&str>, chosen: Option<&str>) -> Vec<OutputEntry> {
    let mut out: Vec<OutputEntry> = Vec::new();
    let mut keyed: HashMap<String, usize> = HashMap::new();
    // Stereo endpoints first, so a hands-free one finds its twin whatever the order.
    let mut active: Vec<(&Endpoint, Class)> = eps
        .iter()
        .filter(|e| e.state == EndpointState::Active)
        .map(|e| (e, classify(e)))
        .collect();
    active.sort_by_key(|(_, c)| c.hands_free);
    for (e, c) in active {
        let key = device_key(e, c);
        if let Some(k) = &key
            && let Some(&i) = keyed.get(k)
        {
            let entry = &mut out[i];
            if c.hands_free && entry.hands_free.is_none() && !entry.hands_free_only {
                entry.hands_free = Some(e.id.clone());
                entry.members.push(e.id.clone());
                continue;
            }
            if !c.hands_free && entry.hands_free_only {
                // Not reached (stereo first), kept for safety: the stereo one plays.
                entry.hands_free = Some(std::mem::replace(&mut entry.id, e.id.clone()));
                entry.hands_free_only = false;
                entry.members.push(e.id.clone());
                continue;
            }
        }
        let (name, mut detail) = names(e, c);
        if c.hands_free {
            detail = "Hands-free only (for calls)".into();
        }
        if let Some(k) = key {
            keyed.insert(k, out.len());
        }
        out.push(OutputEntry {
            id: e.id.clone(),
            name,
            detail,
            kind: c.kind,
            form: c.form,
            hands_free: None,
            hands_free_only: c.hands_free,
            default: false,
            connected: true,
            members: vec![e.id.clone()],
        });
    }
    // The device's other endpoints the platform knows (not connected now).
    for e in eps.iter().filter(|e| e.state != EndpointState::Active) {
        let c = classify(e);
        if let Some(k) = device_key(e, c)
            && let Some(&i) = keyed.get(&k)
            && !out[i].members.contains(&e.id)
        {
            out[i].members.push(e.id.clone());
        }
    }
    if let Some(d) = default {
        for entry in &mut out {
            entry.default = entry.members.iter().any(|m| m == d);
        }
    }
    if let Some(c) = chosen
        && !out.iter().any(|e| e.members.iter().any(|m| m == c))
        && let Some(e) = eps.iter().find(|e| e.id == c)
    {
        let class = classify(e);
        let (name, _) = names(e, class);
        out.push(OutputEntry {
            id: e.id.clone(),
            name,
            detail: "Not connected".into(),
            kind: class.kind,
            form: class.form,
            hands_free: None,
            hands_free_only: class.hands_free,
            default: false,
            connected: false,
            members: vec![e.id.clone()],
        });
    }
    out.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    // One name twice in a group: Windows' full names.
    let dup: Vec<bool> = out
        .iter()
        .map(|a| {
            out.iter()
                .filter(|b| b.kind == a.kind && b.name == a.name)
                .count()
                > 1
        })
        .collect();
    for (entry, d) in out.iter_mut().zip(dup) {
        if d && let Some(e) = eps.iter().find(|e| e.id == entry.id) {
            entry.name = full_name(e);
        }
    }
    out
}

/// What was done about the choice, for the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// Asked for (or Windows' default is) the hands-free endpoint; its stereo twin plays.
    StereoTwin { name: String },
    /// Only the hands-free endpoint is connected: it plays, and sounds poor.
    HandsFreeOnly { name: String },
    /// The chosen device is away: the fallback plays until it is back.
    Away { chosen: String, playing: String },
    /// The chosen device is there but can't be opened: the fallback plays.
    NotAnswering { chosen: String, playing: String },
}

impl Note {
    pub fn words(&self) -> String {
        match self {
            Note::StereoTwin { name } => format!(
                "Playing on {name}'s stereo connection: its hands-free one is for calls (mono, 8 to 16 kHz)."
            ),
            Note::HandsFreeOnly { name } => format!(
                "{name} is connected for calls only (hands-free: mono, 8 to 16 kHz), so music sounds poor. Reconnect it, or check its stereo (audio) service in Windows' Bluetooth settings."
            ),
            Note::Away { chosen, playing } => {
                format!("{chosen} isn't connected: playing on {playing} until it's back.")
            }
            Note::NotAnswering { chosen, playing } => {
                format!("{chosen} isn't answering: playing on {playing} until it does.")
            }
        }
    }
}

/// Which endpoint plays for a choice, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The endpoint to open.
    pub id: String,
    /// The device it is.
    pub entry: OutputEntry,
    pub note: Option<Note>,
    /// The chosen device is away (or not answering): this is the fallback.
    pub fallback: bool,
}

/// The endpoint to play on: `chosen` (`None`: Windows' default, followed), among `eps`, with
/// `default` Windows' default render endpoint and `avoid` the endpoints whose opens all failed.
/// `None` when no output is connected.
pub fn resolve(
    chosen: Option<&str>,
    eps: &[Endpoint],
    default: Option<&str>,
    avoid: &[String],
) -> Option<Resolved> {
    let list = entries(eps, default, None);
    let find = |id: &str| {
        list.iter()
            .find(|e| e.members.iter().any(|m| m == id))
            .filter(|e| !avoid.contains(&e.id))
    };
    // The device of `asked`, with the twin rule.
    let take = |asked: &str, e: &OutputEntry| {
        let note = if e.hands_free_only {
            Some(Note::HandsFreeOnly {
                name: e.name.clone(),
            })
        } else if e.hands_free.as_deref() == Some(asked) {
            Some(Note::StereoTwin {
                name: e.name.clone(),
            })
        } else {
            None
        };
        Resolved {
            id: e.id.clone(),
            entry: e.clone(),
            note,
            fallback: false,
        }
    };
    match chosen {
        Some(c) => {
            if let Some(e) = find(c) {
                return Some(take(c, e));
            }
            let fb = fallback(&list, default, avoid)?;
            let name = eps
                .iter()
                .find(|e| e.id == c)
                .map(|e| names(e, classify(e)).0)
                .unwrap_or_else(|| "The chosen output".into());
            let failing = avoid.iter().any(|a| {
                list.iter()
                    .any(|e| &e.id == a && e.members.iter().any(|m| m == c))
            });
            let note = if failing {
                Note::NotAnswering {
                    chosen: name,
                    playing: fb.name.clone(),
                }
            } else {
                Note::Away {
                    chosen: name,
                    playing: fb.name.clone(),
                }
            };
            Some(Resolved {
                id: fb.id.clone(),
                entry: fb.clone(),
                note: Some(note),
                fallback: true,
            })
        }
        None => {
            if let Some(d) = default
                && let Some(e) = find(d)
            {
                return Some(take(d, e));
            }
            let fb = fallback(&list, default, avoid)?;
            Some(Resolved {
                id: fb.id.clone(),
                entry: fb.clone(),
                note: None,
                fallback: false,
            })
        }
    }
}

/// Where the sound goes when the choice can't be had: Windows' default unless it is virtual,
/// else the first physical output by kind (wired speakers first); never a virtual device.
fn fallback<'a>(
    list: &'a [OutputEntry],
    default: Option<&str>,
    avoid: &[String],
) -> Option<&'a OutputEntry> {
    let ok = |e: &&OutputEntry| e.connected && e.kind != Kind::Virtual && !avoid.contains(&e.id);
    if let Some(d) = default
        && let Some(e) = list
            .iter()
            .filter(ok)
            .find(|e| e.members.iter().any(|m| m == d))
    {
        return Some(e);
    }
    let rank = |e: &OutputEntry| {
        let kind = match e.kind {
            Kind::Wired => 0,
            Kind::Usb => 1,
            Kind::Hdmi => 2,
            Kind::Bluetooth => 3,
            Kind::Other => 4,
            Kind::Virtual => 5,
        };
        (
            kind,
            e.hands_free_only,
            e.form != Form::Speakers,
            e.name.clone(),
        )
    };
    list.iter().filter(ok).min_by_key(|e| rank(e))
}

/// Exclusive mode suits this kind: never Bluetooth (its codec re-encodes anyway, and a link
/// reopened for every rate drops sound) nor a virtual device (other software takes the sound).
pub fn plays_exclusive(kind: Kind) -> bool {
    !matches!(kind, Kind::Bluetooth | Kind::Virtual)
}

/// What the device itself does to the samples, in words (empty: nothing).
pub fn device_steps(kind: Kind, hands_free: bool) -> Vec<String> {
    match (kind, hands_free) {
        (Kind::Bluetooth, true) => vec!["Hands-free connection: mono, 8 to 16 kHz".into()],
        (Kind::Bluetooth, false) => vec!["Re-encoded for Bluetooth (SBC, AAC or aptX)".into()],
        (Kind::Virtual, _) => {
            vec!["A virtual device: other software gets the sound and may change it".into()]
        }
        _ => Vec::new(),
    }
}

/// The starting value offered for an output's extra delay (never applied silently): Bluetooth
/// stereo is typically about 170 ms late (SBC, measured by RTINGS).
pub fn suggested_delay_ms(kind: Kind, hands_free: bool) -> Option<i32> {
    (kind == Kind::Bluetooth && !hands_free).then_some(BLUETOOTH_DELAY_MS)
}

pub const BLUETOOTH_DELAY_MS: i32 = 170;

// --- Exclusive formats ----------------------------------------------------------------------

/// Rates asked of a device in exclusive mode.
pub const PROBE_RATES: [u32; 10] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
];

/// Containers asked: (container bits, valid bits, kind).
pub const PROBE_DEPTHS: [(u16, u16, SampleKind); 5] = [
    (16, 16, SampleKind::Int),
    (24, 24, SampleKind::Int),
    (32, 24, SampleKind::Int),
    (32, 32, SampleKind::Int),
    (32, 32, SampleKind::Float),
];

/// The exclusive formats a device takes, each with the platform's way of writing it (WASAPI:
/// which of the drivers' quirks it took).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatTable {
    pub formats: Vec<(DeviceFormat, u32)>,
}

impl FormatTable {
    /// The way `f` is written, when the device takes it.
    pub fn takes(&self, f: &DeviceFormat) -> Option<u32> {
        self.formats.iter().find(|(g, _)| g == f).map(|(_, w)| *w)
    }

    /// Rates it takes, low to high.
    pub fn rates(&self) -> Vec<u32> {
        let mut r: Vec<u32> = self.formats.iter().map(|(f, _)| f.rate).collect();
        r.sort_unstable();
        r.dedup();
        r
    }

    /// The deepest integer depth it takes (valid bits).
    pub fn most_bits(&self) -> u16 {
        self.formats
            .iter()
            .filter(|(f, _)| f.kind == SampleKind::Int)
            .map(|(f, _)| f.valid_bits)
            .max()
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
    }
}

/// Containers best first for a song of `bits`: the ones that carry it exactly (16 → 16 in 16,
/// 32 in 32, 24 in 24; 24 → 24 in 32, 24 in 24, 32 in 32; 32 and lossy → 32, 24, float, 16).
pub fn depth_order(bits: u16) -> Vec<(u16, u16, SampleKind)> {
    use SampleKind::{Float, Int};
    match bits {
        16 => vec![(16, 16, Int), (32, 32, Int), (24, 24, Int)],
        24 => vec![(32, 24, Int), (24, 24, Int), (32, 32, Int)],
        _ => vec![(32, 32, Int), (24, 24, Int), (32, 32, Float), (16, 16, Int)],
    }
}

/// Every container: `depth_order`'s first (16 bits only for a 16-bit song), then the rest, those
/// that still carry the song exactly first (integers before float), then the deepest.
fn all_depths(bits: u16) -> Vec<(u16, u16, SampleKind)> {
    let mut v = depth_order(bits);
    if bits > 16 {
        v.retain(|&(_, valid, _)| valid > 16);
    }
    let carries = |valid: u16, k: SampleKind| match k {
        SampleKind::Int => valid >= bits,
        SampleKind::Float => bits <= 24,
    };
    let mut rest: Vec<_> = PROBE_DEPTHS
        .iter()
        .copied()
        .filter(|d| !v.contains(d))
        .collect();
    rest.sort_by_key(|&(_, valid, k)| {
        (
            !carries(valid, k),
            k == SampleKind::Float,
            std::cmp::Reverse(valid),
        )
    });
    v.extend(rest);
    v
}

/// 44.1 kHz and its multiples, or 48 kHz and its.
fn family(rate: u32) -> u32 {
    if rate % 11_025 == 0 { 44_100 } else { 48_000 }
}

/// The rates to try for a song at `want`, among `have`: its own, then its family's (the
/// smallest at or over it, then the largest under it), then the other family's the same way.
pub fn rate_order(want: u32, have: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    if have.contains(&want) {
        out.push(want);
    }
    for same in [true, false] {
        let fam = |r: &&u32| (family(**r) == family(want)) == same && **r != want;
        let mut up: Vec<u32> = have
            .iter()
            .filter(fam)
            .filter(|r| **r > want)
            .copied()
            .collect();
        let mut down: Vec<u32> = have
            .iter()
            .filter(fam)
            .filter(|r| **r < want)
            .copied()
            .collect();
        up.sort_unstable();
        down.sort_unstable_by(|a, b| b.cmp(a));
        out.extend(up);
        out.extend(down);
    }
    out
}

/// The exclusive formats to try for `want`, best first: its own rate at the containers that
/// carry it (its channels, then 2), then the nearest rates the device takes (our resampler
/// converts); depths the device lacks after those that carry the song. Empty: none fits
/// (shared mode).
pub fn plan_exclusive(t: &FormatTable, want: &OpenRequest) -> Vec<DeviceFormat> {
    let mut chans = vec![want.channels.max(1)];
    if want.channels != 2 {
        chans.push(2);
    }
    let mut out = Vec::new();
    for rate in rate_order(want.rate, &t.rates()) {
        for &ch in &chans {
            for (c, v, k) in all_depths(want.bits) {
                let f = DeviceFormat {
                    rate,
                    channels: ch,
                    container_bits: c,
                    valid_bits: v,
                    kind: k,
                };
                if t.takes(&f).is_some() && !out.contains(&f) {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// A song of `want` plays on an exclusive output open at `open` without reopening: it is the
/// format its plan would open.
pub fn fits_exclusive(t: &FormatTable, want: &OpenRequest, open: &DeviceFormat) -> bool {
    plan_exclusive(t, want).first() == Some(open)
}

/// The exclusive formats probed, per endpoint and channel count, for the life of the app: a
/// device is asked once.
#[derive(Default)]
pub struct ProbeCache {
    tables: Mutex<HashMap<(String, u16), Arc<FormatTable>>>,
}

impl ProbeCache {
    /// The process's cache.
    pub fn global() -> &'static ProbeCache {
        static CACHE: OnceLock<ProbeCache> = OnceLock::new();
        CACHE.get_or_init(ProbeCache::default)
    }

    pub fn get(&self, id: &str, channels: u16) -> Option<Arc<FormatTable>> {
        self.lock().get(&(id.to_owned(), channels)).cloned()
    }

    /// The table for `id` at `channels`: cached, or probed now (`probe` gives `None` when the
    /// answers can't be trusted, such as a device another app holds: then nothing is kept).
    pub fn get_or_probe(
        &self,
        id: &str,
        channels: u16,
        probe: impl FnOnce() -> Option<FormatTable>,
    ) -> Option<Arc<FormatTable>> {
        if let Some(t) = self.get(id, channels) {
            return Some(t);
        }
        let t = Arc::new(probe()?);
        self.lock().insert((id.to_owned(), channels), t.clone());
        Some(t)
    }

    /// Forget a device's tables (its formats proved wrong).
    pub fn forget(&self, id: &str) {
        self.lock().retain(|(k, _), _| k != id);
    }

    /// What a probed device takes, for the window: (rates, the most bits), over its tables.
    pub fn summary(&self, id: &str) -> Option<(Vec<u32>, u16)> {
        let g = self.lock();
        let mut rates = Vec::new();
        let mut bits = 0;
        let mut any = false;
        for ((k, _), t) in g.iter() {
            if k == id {
                any = true;
                rates.extend(t.rates());
                bits = bits.max(t.most_bits());
            }
        }
        rates.sort_unstable();
        rates.dedup();
        any.then_some((rates, bits))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, u16), Arc<FormatTable>>> {
        self.tables.lock().unwrap_or_else(|p| p.into_inner())
    }
}

// --- Where devices come from ----------------------------------------------------------------

/// The platform's sound outputs: listed, the default, an output on one, and changes.
pub trait Devices: Send + Sync {
    /// Every render endpoint the platform knows, in any state.
    fn endpoints(&self) -> Vec<Endpoint>;
    /// Windows' default render endpoint (the console role).
    fn default_id(&self) -> Option<String>;
    /// An output on endpoint `id`, exclusive mode if asked (it may still open shared).
    fn open(&self, id: &str, exclusive: bool) -> Box<dyn Output>;
    /// A message for every change from now on (an endpoint added, removed or changing state,
    /// the default changing); each call gives a new receiver and the older one hears no more.
    fn watch(&self) -> Receiver<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use EndpointState::{Active, NotPresent, Unplugged};
    use form_factor as ff;

    fn ep(
        id: &str,
        desc: &str,
        adapter: &str,
        en: &str,
        form: u32,
        container: Option<&str>,
        state: EndpointState,
    ) -> Endpoint {
        Endpoint {
            id: id.into(),
            description: desc.into(),
            adapter: adapter.into(),
            enumerator: en.into(),
            form_factor: form,
            container: container.map(str::to_owned),
            state,
        }
    }

    const PODS: &str = "{eab42fbe-0000-0000-0000-000000000001}";
    const FIIO: &str = "{167463cc-0000-0000-0000-000000000002}";
    const SOUNDCORE: &str = "{13ac5512-0000-0000-0000-000000000003}";
    const MACHINE: &str = "{00000000-0000-0000-FFFF-FFFFFFFFFFFF}";

    /// The owner's laptop as its property stores read (names and ids made up where personal),
    /// with the Bluetooth devices connected.
    fn owners() -> Vec<Endpoint> {
        vec![
            ep(
                "pods-a2dp",
                "Headphones",
                "Sam's AirPods Pro",
                "BTHENUM",
                ff::HEADPHONES,
                Some(PODS),
                Active,
            ),
            ep(
                "pods-hfp",
                "Headset",
                "Sam's AirPods Pro Hands-Free",
                "BTHHFENUM",
                ff::HEADSET,
                Some(PODS),
                Active,
            ),
            ep(
                "fiio-a2dp",
                "Headphones",
                "FIIO EH11",
                "BTHENUM",
                ff::HEADPHONES,
                Some(FIIO),
                Active,
            ),
            ep(
                "fiio-hfp",
                "Headset",
                "FIIO EH11 Hands-Free",
                "BTHHFENUM",
                ff::HEADSET,
                Some(FIIO),
                Active,
            ),
            ep(
                "x600-a2dp",
                "Speakers",
                "soundcore Motion X600",
                "BTHENUM",
                ff::SPEAKERS,
                Some(SOUNDCORE),
                Active,
            ),
            ep(
                "x600-hfp",
                "Headset Earphone",
                "soundcore Motion X600 Hands-Free",
                "BTHHFENUM",
                ff::HEADSET,
                Some(SOUNDCORE),
                Active,
            ),
            ep(
                "snowsky",
                "Headphones",
                "SNOWSKY DISC",
                "USB",
                ff::HEADPHONES,
                Some("{11eb762d-0000-0000-0000-000000000004}"),
                Active,
            ),
            ep(
                "speakers",
                "Speakers",
                "Realtek(R) Audio",
                "INTELAUDIO",
                ff::SPEAKERS,
                Some(MACHINE),
                Active,
            ),
            ep(
                "jack",
                "Headphones",
                "Realtek(R) Audio",
                "INTELAUDIO",
                ff::HEADPHONES,
                Some(MACHINE),
                Unplugged,
            ),
            ep(
                "hdmi",
                "XG32UCWG",
                "NVIDIA High Definition Audio",
                "HDAUDIO",
                ff::DISPLAY,
                Some("{1c809401-0000-0000-0000-000000000005}"),
                Active,
            ),
            ep(
                "hdmi-old",
                "NVIDIA Output",
                "NVIDIA High Definition Audio",
                "HDAUDIO",
                ff::DISPLAY,
                None,
                NotPresent,
            ),
            ep(
                "mirror",
                "Speakers",
                "Nahimic mirroring device",
                "SWD",
                ff::SPEAKERS,
                Some(MACHINE),
                Active,
            ),
            ep(
                "vad",
                "Speakers",
                "Nahimic VAD",
                "SWD",
                ff::SPEAKERS,
                Some(MACHINE),
                Active,
            ),
            ep(
                "droidcam",
                "Speakers",
                "DroidCam Audio",
                "ROOT",
                ff::SPEAKERS,
                None,
                Active,
            ),
        ]
    }

    #[test]
    fn every_endpoint_is_classified_by_its_bus_and_form() {
        let eps = owners();
        let class = |id: &str| classify(eps.iter().find(|e| e.id == id).unwrap());
        let want = [
            ("pods-a2dp", Kind::Bluetooth, Form::Headphones, false),
            ("pods-hfp", Kind::Bluetooth, Form::Headset, true),
            ("x600-a2dp", Kind::Bluetooth, Form::Speakers, false),
            ("x600-hfp", Kind::Bluetooth, Form::Headset, true),
            ("snowsky", Kind::Usb, Form::Headphones, false),
            ("speakers", Kind::Wired, Form::Speakers, false),
            ("jack", Kind::Wired, Form::Headphones, false),
            ("hdmi", Kind::Hdmi, Form::Display, false),
            ("mirror", Kind::Virtual, Form::Speakers, false),
            ("droidcam", Kind::Virtual, Form::Speakers, false),
        ];
        for (id, kind, form, hf) in want {
            assert_eq!(
                class(id),
                Class {
                    kind,
                    form,
                    hands_free: hf
                },
                "{id}"
            );
        }
        // A USB headset is a USB device at full quality, not a hands-free link.
        let usb_headset = ep("u", "Headset", "Arctis", "USB", ff::HEADSET, None, Active);
        assert!(!classify(&usb_headset).hands_free);
        // A hands-free endpoint read with another form factor is still told by its bus.
        let odd = ep(
            "o",
            "Speakers",
            "Car Kit Hands-Free AG Audio",
            "BTHHFENUM",
            ff::SPEAKERS,
            None,
            Active,
        );
        assert!(classify(&odd).hands_free);
        // A display that is a virtual device is virtual.
        let v = ep(
            "v",
            "Monitor",
            "Virtual Display Audio",
            "SWD",
            ff::DISPLAY,
            None,
            Active,
        );
        assert_eq!(classify(&v).kind, Kind::Virtual);
    }

    #[test]
    fn the_picker_lists_a_bluetooth_devices_two_endpoints_as_one_with_clear_names() {
        let list = entries(&owners(), Some("speakers"), None);
        let seen: Vec<(Kind, &str, &str)> = list
            .iter()
            .map(|e| (e.kind, e.name.as_str(), e.id.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (Kind::Bluetooth, "FIIO EH11", "fiio-a2dp"),
                (Kind::Bluetooth, "Sam's AirPods Pro", "pods-a2dp"),
                (Kind::Bluetooth, "soundcore Motion X600", "x600-a2dp"),
                (Kind::Usb, "SNOWSKY DISC", "snowsky"),
                (Kind::Wired, "Speakers", "speakers"),
                (Kind::Hdmi, "XG32UCWG", "hdmi"),
                (Kind::Virtual, "DroidCam Audio", "droidcam"),
                (Kind::Virtual, "Nahimic VAD", "vad"),
                (Kind::Virtual, "Nahimic mirroring device", "mirror"),
            ]
        );
        let pods = list.iter().find(|e| e.id == "pods-a2dp").unwrap();
        assert_eq!(pods.hands_free.as_deref(), Some("pods-hfp"));
        assert!(!pods.hands_free_only && pods.connected);
        let speakers = list.iter().find(|e| e.id == "speakers").unwrap();
        assert!(speakers.default);
        assert_eq!(speakers.detail, "Realtek(R) Audio");
        // The computer's own container pairs nothing: the speakers are not the jack's twin.
        assert_eq!(speakers.members, ["speakers"]);
        // A chosen device that is away is listed, not connected.
        let mut eps = owners();
        eps.iter_mut().find(|e| e.id == "snowsky").unwrap().state = Unplugged;
        let list = entries(&eps, None, Some("snowsky"));
        let away = list.iter().find(|e| e.id == "snowsky").unwrap();
        assert!(!away.connected);
        assert_eq!(
            (away.name.as_str(), away.detail.as_str()),
            ("SNOWSKY DISC", "Not connected")
        );
        // Two outputs of one kind with one name: Windows' full names.
        let two = vec![
            ep(
                "a",
                "Speakers",
                "Realtek(R) Audio",
                "HDAUDIO",
                ff::SPEAKERS,
                None,
                Active,
            ),
            ep(
                "b",
                "Speakers",
                "Sound Blaster",
                "PCI",
                ff::SPEAKERS,
                None,
                Active,
            ),
        ];
        let names: Vec<String> = entries(&two, None, None)
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(
            names,
            ["Speakers (Realtek(R) Audio)", "Speakers (Sound Blaster)"]
        );
    }

    #[test]
    fn a_device_with_only_its_hands_free_endpoint_is_listed_as_such() {
        let mut eps = owners();
        eps.iter_mut().find(|e| e.id == "pods-a2dp").unwrap().state = Unplugged;
        let list = entries(&eps, None, None);
        let pods = list.iter().find(|e| e.id == "pods-hfp").unwrap();
        assert!(pods.hands_free_only);
        assert_eq!(pods.name, "Sam's AirPods Pro");
        assert_eq!(pods.detail, "Hands-free only (for calls)");
        // Without a container the twins pair by name.
        let bare = vec![
            ep(
                "h",
                "Headset",
                "Car Kit Hands-Free",
                "BTHHFENUM",
                ff::HEADSET,
                None,
                Active,
            ),
            ep(
                "s",
                "Headphones",
                "Car Kit",
                "BTHENUM",
                ff::HEADPHONES,
                None,
                Active,
            ),
        ];
        let list = entries(&bare, None, None);
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].id.as_str(), list[0].hands_free.as_deref()),
            ("s", Some("h"))
        );
    }

    #[test]
    fn the_choice_resolves_to_the_stereo_twin_the_default_or_a_fallback() {
        let eps = owners();
        let r = |chosen: Option<&str>, default: Option<&str>| {
            resolve(chosen, &eps, default, &[]).unwrap()
        };
        // Chosen and here.
        let x = r(Some("snowsky"), Some("speakers"));
        assert_eq!(
            (x.id.as_str(), x.note.clone(), x.fallback),
            ("snowsky", None, false)
        );
        // Chosen by its hands-free endpoint: the stereo twin, said.
        let x = r(Some("pods-hfp"), Some("speakers"));
        assert_eq!(x.id, "pods-a2dp");
        assert!(matches!(x.note, Some(Note::StereoTwin { .. })));
        assert!(
            x.note
                .unwrap()
                .words()
                .contains("hands-free one is for calls")
        );
        // Following Windows, whose default is a hands-free endpoint: the twin.
        let x = r(None, Some("fiio-hfp"));
        assert_eq!(x.id, "fiio-a2dp");
        // Following Windows to a virtual default: followed, it's where Windows sends sound.
        assert_eq!(r(None, Some("mirror")).id, "mirror");
        // Chosen and away: the default, said; a virtual default is never the fallback.
        let mut away = eps.clone();
        away.iter_mut().find(|e| e.id == "hdmi").unwrap().state = NotPresent;
        let x = resolve(Some("hdmi"), &away, Some("speakers"), &[]).unwrap();
        assert_eq!((x.id.as_str(), x.fallback), ("speakers", true));
        assert_eq!(
            x.note.unwrap().words(),
            "XG32UCWG isn't connected: playing on Speakers until it's back."
        );
        let x = resolve(Some("hdmi"), &away, Some("vad"), &[]).unwrap();
        assert_eq!(
            x.id, "speakers",
            "physical outputs first, wired speakers before others"
        );
        // Only virtual devices left: nothing to fall back on.
        let virt: Vec<Endpoint> = eps
            .iter()
            .filter(|e| classify(e).kind == Kind::Virtual)
            .cloned()
            .collect();
        assert!(resolve(Some("snowsky"), &virt, Some("vad"), &[]).is_none());
        // Only the hands-free endpoint: played, with the warning.
        let mut hf = eps.clone();
        hf.iter_mut().find(|e| e.id == "pods-a2dp").unwrap().state = Unplugged;
        let x = resolve(Some("pods-a2dp"), &hf, Some("speakers"), &[]).unwrap();
        assert_eq!(x.id, "pods-hfp");
        assert!(x.note.unwrap().words().contains("calls only"));
        // A chosen device whose opens failed: passed over, said.
        let x = resolve(Some("snowsky"), &eps, Some("speakers"), &["snowsky".into()]).unwrap();
        assert_eq!(x.id, "speakers");
        assert!(matches!(x.note, Some(Note::NotAnswering { .. })));
        // Nothing connected at all.
        assert!(resolve(None, &[], None, &[]).is_none());
    }

    #[test]
    fn exclusive_suits_wired_usb_and_displays_and_the_suggestion_is_for_bluetooth() {
        assert!(
            plays_exclusive(Kind::Usb)
                && plays_exclusive(Kind::Wired)
                && plays_exclusive(Kind::Hdmi)
        );
        assert!(!plays_exclusive(Kind::Bluetooth) && !plays_exclusive(Kind::Virtual));
        assert_eq!(suggested_delay_ms(Kind::Bluetooth, false), Some(170));
        assert_eq!(suggested_delay_ms(Kind::Bluetooth, true), None);
        assert_eq!(suggested_delay_ms(Kind::Usb, false), None);
        assert!(device_steps(Kind::Bluetooth, false)[0].contains("Re-encoded"));
        assert!(device_steps(Kind::Usb, false).is_empty());
    }

    fn fmt(rate: u32, ch: u16, c: u16, v: u16, k: SampleKind) -> DeviceFormat {
        DeviceFormat {
            rate,
            channels: ch,
            container_bits: c,
            valid_bits: v,
            kind: k,
        }
    }

    /// A small USB DAC: 44.1, 48 and 96 kHz, 16-bit and 24 in 32, stereo only.
    fn dac() -> FormatTable {
        let mut t = FormatTable::default();
        for rate in [44_100, 48_000, 96_000] {
            t.formats.push((fmt(rate, 2, 16, 16, SampleKind::Int), 0));
            t.formats.push((fmt(rate, 2, 32, 24, SampleKind::Int), 1));
        }
        t
    }

    fn req(rate: u32, channels: u16, bits: u16) -> OpenRequest {
        OpenRequest {
            rate,
            channels,
            bits,
        }
    }

    #[test]
    fn the_exclusive_plan_tries_the_songs_own_format_then_the_nearest_the_device_takes() {
        use SampleKind::Int;
        let t = dac();
        // Its own rate, the container that carries it.
        assert_eq!(
            plan_exclusive(&t, &req(96_000, 2, 24))[0],
            fmt(96_000, 2, 32, 24, Int)
        );
        assert_eq!(
            plan_exclusive(&t, &req(44_100, 2, 16))[0],
            fmt(44_100, 2, 16, 16, Int)
        );
        // 16-bit in a 24-in-32 container still carries it exactly: second.
        assert_eq!(
            plan_exclusive(&t, &req(44_100, 2, 16))[1],
            fmt(44_100, 2, 32, 24, Int)
        );
        // A rate it lacks: the nearest of its family, under it.
        assert_eq!(
            plan_exclusive(&t, &req(192_000, 2, 24))[0],
            fmt(96_000, 2, 32, 24, Int)
        );
        // 88.2 kHz: its family has 44.1 under it; the other family's 96 over it comes after.
        let p = plan_exclusive(&t, &req(88_200, 2, 24));
        assert_eq!((p[0].rate, p[2].rate), (44_100, 96_000));
        // 32-bit asked of a device with 24 at most: the deepest it has.
        assert_eq!(
            plan_exclusive(&t, &req(48_000, 2, 32))[0],
            fmt(48_000, 2, 32, 24, Int)
        );
        // Mono: stereo at its own rate (the device has no mono).
        assert_eq!(
            plan_exclusive(&t, &req(48_000, 1, 16))[0],
            fmt(48_000, 2, 16, 16, Int)
        );
        // A device that takes nothing: shared mode.
        assert!(plan_exclusive(&FormatTable::default(), &req(48_000, 2, 16)).is_empty());
        // The rates in order.
        assert_eq!(
            rate_order(44_100, &[48_000, 88_200, 96_000, 176_400, 22_050]),
            [88_200, 176_400, 22_050, 48_000, 96_000]
        );
        // Songs that land on the format open play on without a reopen; a native one reopens.
        let open = fmt(96_000, 2, 32, 24, Int);
        assert!(fits_exclusive(&t, &req(192_000, 2, 24), &open));
        assert!(fits_exclusive(&t, &req(96_000, 2, 24), &open));
        assert!(!fits_exclusive(&t, &req(44_100, 2, 16), &open));
    }

    #[test]
    fn a_device_is_probed_once() {
        let cache = ProbeCache::default();
        let mut asked = 0;
        for _ in 0..3 {
            let t = cache.get_or_probe("dac", 2, || {
                asked += 1;
                Some(dac())
            });
            assert_eq!(t.unwrap().rates(), [44_100, 48_000, 96_000]);
        }
        assert_eq!(asked, 1);
        // A probe whose answers can't be trusted (another app had it) is not kept.
        assert!(cache.get_or_probe("busy", 2, || None).is_none());
        assert!(cache.get_or_probe("busy", 2, || Some(dac())).is_some());
        assert_eq!(
            cache.summary("dac"),
            Some((vec![44_100, 48_000, 96_000], 24))
        );
        cache.forget("dac");
        assert!(cache.get("dac", 2).is_none());
    }
}
