//! Alignment of two copies of a song (design 4.5, contract 5.8): where my copy sits against the
//! room's reference copy, so `my content time = room content time + offset`.
//!
//! - [`envelope`] turns decoded audio into an [`Envelope`]: onset strength, one byte per 10 ms
//!   hop, over the first 30 s and then 10 s every 60 s, plus a hash of the first 10 s of PCM.
//!   The host makes one for its reference copy and sends it with the entry; each guest makes
//!   one for its own copy as its decode thread prepares the song.
//! - [`align`] compares two envelopes. The same PCM is exact. Otherwise a normalised
//!   cross-correlation over lags of -5 s to +5 s, refined by a parabola through the peak, is
//!   trusted only when the peak stands out (8 standard deviations over the mean, and 1.3 times
//!   the best other peak more than 50 ms away) and the 10 s parts of the first window find the
//!   same offset on their own. Later windows check the offset through the song: a move over
//!   20 ms, or no match where both copies have music, is another edit, and the copy is
//!   approximate from there.
//!
//! Plain arithmetic on the calling thread, no platform code, and cheap enough for a low-end
//! handheld: only the windows are analysed (about a quarter of a song), with three biquads per
//! frame; the correlation is integer dot products over a few thousand bytes. The `alignment`
//! tests print the cost per minute of audio.

use together::align::{Alignment, CopyClass, Envelope};

/// One envelope byte per this many milliseconds.
pub const HOP_MS: u8 = 10;
/// Energies are measured on 5 ms sub-blocks, two per hop, so a hop's frame is centred on it.
const SUBS_PER_SECOND: u64 = 200;
/// The first window, from the song's start.
const FIRST_WINDOW_MS: u32 = 30_000;
/// Then a window of this length ...
const LATER_WINDOW_MS: u32 = 10_000;
/// ... starting at every multiple of this.
const LATER_EVERY_MS: u32 = 60_000;
/// A window cut shorter than this by the song's end is left out.
const MIN_WINDOW_MS: u32 = 1_000;
/// The PCM hash covers the first this many milliseconds.
const HASH_MS: u64 = 10_000;
/// The band filters run this long before a window so they have settled.
const PREROLL_MS: u64 = 100;
/// The four bands' edges: below 200 Hz, 200 Hz to 2 kHz, 2 to 6 kHz, above 6 kHz.
const BAND_EDGES_HZ: [f64; 3] = [200.0, 2_000.0, 6_000.0];
/// A band's rise of log energy counts up to this much per hop ...
const RISE_CAP_DB: f64 = 16.0;
/// ... at this many units per dB (four bands at the cap fill a byte).
const UNITS_PER_DB: f64 = 4.0;
/// Energy floor (about -90 dBFS) so silence and dither make no onsets.
const ENERGY_FLOOR: f64 = 1e-9;

/// Lags searched, each way.
const SEARCH_MS: i64 = 5_000;
/// A lag is scored only when the copies overlap this long at it.
const MIN_OVERLAP_MS: i64 = 4_000;
/// The first window's parts, which must find its offset on their own.
const PIECE_MS: i64 = 10_000;
/// Confidence: the peak this many standard deviations over the mean of all lags ...
const PEAK_SD: f64 = 8.0;
/// ... and this many times the best peak more than [`SECOND_PEAK_AWAY_MS`] away.
const PEAK_RATIO: f64 = 1.3;
const SECOND_PEAK_AWAY_MS: i64 = 50;
/// Two measures agree when within this; a larger move is another edit.
const AGREE_MS: f64 = 20.0;
/// Where both copies have music and the known offset matches under this share of how well
/// the copies matched at the start, they are not the same music there: an edit too long to
/// find, or another version.
const LOST_LEVEL: f64 = 0.5;
/// A stretch has music when at least one hop per second rises this much (6 dB over the bands).
const ACTIVE_LEVEL: u8 = 24;

/// One of a song's windows: (start ms, length ms). The first is 30 s; then 10 s every 60 s.
fn window(i: usize) -> (u32, u32) {
    if i == 0 {
        (0, FIRST_WINDOW_MS)
    } else {
        (i as u32 * LATER_EVERY_MS, LATER_WINDOW_MS)
    }
}

/// A second-order Butterworth low-pass (transposed direct form II).
#[derive(Clone, Copy)]
struct LowPass {
    b0: f64,
    b1: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl LowPass {
    fn new(fc: f64, rate: u32) -> Self {
        let fc = fc.min(f64::from(rate) * 0.45);
        let w = std::f64::consts::TAU * fc / f64::from(rate);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / std::f64::consts::SQRT_2; // Q = 1/sqrt(2)
        let a0 = 1.0 + alpha;
        let b1 = (1.0 - cos) / a0;
        Self {
            b0: b1 / 2.0, // b2 is the same
            b1,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b0 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// The envelope being made, fed one mono frame at a time.
struct Maker {
    rate: u64,
    filters: [LowPass; 3],
    /// The window being measured, and its hops `[h0, h1)`.
    win: usize,
    h0: u64,
    h1: u64,
    /// Frames `[from, to)` are filtered for this window (the pre-roll included); sub-blocks
    /// are kept from `k0`, which starts at frame `kept`.
    from: u64,
    to: u64,
    k0: u64,
    kept: u64,
    /// The sub-block being summed: its index, the frame it ends at, its sums and frames.
    k: u64,
    k_end: u64,
    sum: [f64; 4],
    n: u64,
    /// Mean energy per band of each finished sub-block of this window, from `k0`.
    subs: Vec<[f64; 4]>,
    windows: Vec<(u32, Vec<u8>)>,
}

impl Maker {
    fn new(rate: u32) -> Self {
        let mut m = Self {
            rate: u64::from(rate),
            filters: BAND_EDGES_HZ.map(|fc| LowPass::new(fc, rate)),
            win: 0,
            h0: 0,
            h1: 0,
            from: 0,
            to: 0,
            k0: 0,
            kept: 0,
            k: 0,
            k_end: 0,
            sum: [0.0; 4],
            n: 0,
            subs: Vec::new(),
            windows: Vec::new(),
        };
        m.open(0);
        m
    }

    /// The first frame of sub-block `k` (exact at any rate: 44.1 kHz alternates 220 and 221).
    fn sub_start(&self, k: u64) -> u64 {
        k * self.rate / SUBS_PER_SECOND
    }

    fn open(&mut self, win: usize) {
        let (start, len) = window(win);
        let hop = u64::from(HOP_MS);
        self.win = win;
        self.h0 = u64::from(start) / hop;
        self.h1 = u64::from(start + len) / hop;
        // Hop n takes the rises at boundaries 2n-3 ..= 2n+3; each rise compares two frames,
        // each frame spans four sub-blocks: sub-blocks 2n-6 ..= 2n+4 are needed.
        self.k0 = (2 * self.h0).saturating_sub(6);
        self.kept = self.sub_start(self.k0);
        self.from = self.kept.saturating_sub(PREROLL_MS * self.rate / 1_000);
        self.to = self.sub_start(2 * self.h1 + 3);
        self.k = self.k0;
        self.k_end = self.sub_start(self.k0 + 1);
        self.sum = [0.0; 4];
        self.n = 0;
        self.subs.clear();
        for f in &mut self.filters {
            f.reset();
        }
    }

    #[inline]
    fn frame(&mut self, at: u64, x: f64) {
        if at < self.from {
            return;
        }
        if at >= self.to {
            self.close();
            self.open(self.win + 1);
            if at < self.from {
                return;
            }
        }
        let l0 = self.filters[0].run(x);
        let l1 = self.filters[1].run(x);
        let l2 = self.filters[2].run(x);
        if at < self.kept {
            return; // the pre-roll: the filters settle
        }
        if at >= self.k_end {
            self.end_sub();
        }
        let bands = [l0, l1 - l0, l2 - l1, x - l2];
        for (s, b) in self.sum.iter_mut().zip(bands) {
            *s += b * b;
        }
        self.n += 1;
    }

    fn end_sub(&mut self) {
        let n = self.n.max(1) as f64;
        self.subs.push(self.sum.map(|s| s / n));
        self.sum = [0.0; 4];
        self.n = 0;
        self.k += 1;
        self.k_end = self.sub_start(self.k + 1);
    }

    /// The energy per band of the 20 ms frame centred on sub-block boundary `k` (time 5k ms):
    /// sub-blocks k-2 ..= k+1 weighted 1/2, 1, 1, 1/2. `None` before the song or past what
    /// was decoded.
    fn energy(&self, k: u64) -> Option<[f64; 4]> {
        let first = k.checked_sub(2)?;
        let i = usize::try_from(first.checked_sub(self.k0)?).ok()?;
        let s = self.subs.get(i..i + 4)?;
        let mut e = [0.0; 4];
        for (b, e) in e.iter_mut().enumerate() {
            *e = (0.5 * s[0][b] + s[1][b] + s[2][b] + 0.5 * s[3][b]) / 3.0;
        }
        Some(e)
    }

    /// The rise of log energy into boundary `k` from 5 ms before, summed over the bands
    /// (each capped), in dB. 0 at the song's start, where there is no frame before.
    fn rise(&self, k: u64) -> f64 {
        let (Some(before), Some(now)) = (
            k.checked_sub(1).and_then(|p| self.energy(p)),
            self.energy(k),
        ) else {
            return 0.0;
        };
        let mut rise = 0.0;
        for b in 0..4 {
            let ratio = (now[b] + ENERGY_FLOOR) / (before[b] + ENERGY_FLOOR);
            rise += (10.0 * ratio.log10()).clamp(0.0, RISE_CAP_DB);
        }
        rise
    }

    /// The window's bytes from its finished sub-blocks (fewer when the song ends in it).
    ///
    /// Hop `n` (time 10n ms) is the rises at 5 ms steps around it through a triangle 40 ms wide
    /// (weights 1, 2, 3, 4, 3, 2, 1 over 8). The log makes an onset's rise land almost all in
    /// one step; sampled straight at 10 ms, two copies half a hop apart would see their
    /// onsets in different hops and correlate poorly (0.76 for a perfect copy 25 ms late).
    /// The triangle sums to 1 over every other step, so each rise is counted once whatever
    /// its phase against the hops.
    fn close(&mut self) {
        if self.n > 0 && self.n == self.k_end - self.sub_start(self.k) {
            self.end_sub();
        }
        let have = self.k0 + self.subs.len() as u64;
        let mut bytes = Vec::with_capacity((self.h1 - self.h0) as usize);
        for n in self.h0..self.h1 {
            if 2 * n + 4 >= have {
                break; // the song ended here
            }
            let mut sum = 0.0;
            for d in -3i64..=3 {
                if let Some(k) = (2 * n).checked_add_signed(d) {
                    sum += (4 - d.abs()) as f64 / 8.0 * self.rise(k);
                }
            }
            bytes.push((sum * UNITS_PER_DB).round().min(255.0) as u8);
        }
        if bytes.len() as u32 * u32::from(HOP_MS) >= MIN_WINDOW_MS {
            self.windows.push((window(self.win).0, bytes));
        }
        self.subs.clear();
    }
}

/// The hash of the first 10 s of decoded PCM, with the rate, the channels and how many frames
/// there were, so only the same samples at the same rate and layout match.
#[derive(Clone)]
struct PcmHash {
    hasher: blake3::Hasher,
    samples_max: u64,
    samples: u64,
    buf: Vec<u8>,
}

impl PcmHash {
    fn new(rate: u32, ch: u16) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"venuta pcm hash v1");
        hasher.update(&rate.to_le_bytes());
        hasher.update(&ch.to_le_bytes());
        Self {
            hasher,
            samples_max: HASH_MS * u64::from(rate) / 1_000 * u64::from(ch),
            samples: 0,
            buf: Vec::with_capacity(8_192),
        }
    }

    #[inline]
    fn feed(&mut self, block: &[i32]) {
        let room = (self.samples_max - self.samples).min(block.len() as u64) as usize;
        for s in &block[..room] {
            self.buf.extend_from_slice(&s.to_le_bytes());
            if self.buf.len() >= 8_192 {
                self.hasher.update(&self.buf);
                self.buf.clear();
            }
        }
        self.samples += room as u64;
    }

    fn finish(mut self) -> Option<[u8; 16]> {
        if self.samples == 0 {
            return None;
        }
        self.hasher.update(&self.buf);
        self.hasher.update(&self.samples.to_le_bytes());
        let mut out = [0u8; 16];
        out.copy_from_slice(&self.hasher.finalize().as_bytes()[..16]);
        Some(out)
    }
}

/// The alignment envelope of a song from its decoded audio: interleaved left-justified
/// samples (as [`crate::decode::Source`] gives them) in blocks of any size, read to the end.
/// Only the windows are analysed; the rest of the song is only counted.
pub fn envelope(decoded: &mut dyn Iterator<Item = &[i32]>, rate: u32, ch: u16) -> Envelope {
    let mut e = Enveloper::new(rate, ch);
    for block in decoded {
        e.feed(block);
    }
    e.finish()
}

/// [`envelope`] block by block, for a decoder that hands its blocks over one at a time (the
/// room's measure decodes a song once, and gives the start's envelope as soon as the first
/// window is done).
pub struct Enveloper {
    /// `None`: no rate or no channels, nothing to analyse.
    maker: Option<Maker>,
    hash: Option<PcmHash>,
    ch_n: u32,
    scale: f64,
    frame: u64,
    chan: u32,
    acc: i64,
}

impl Enveloper {
    pub fn new(rate: u32, ch: u16) -> Enveloper {
        let ok = rate > 0 && ch > 0;
        Enveloper {
            maker: ok.then(|| Maker::new(rate)),
            hash: ok.then(|| PcmHash::new(rate, ch)),
            ch_n: u32::from(ch),
            scale: 1.0 / (f64::from(ch.max(1)) * 2_147_483_648.0),
            frame: 0,
            chan: 0,
            acc: 0,
        }
    }

    /// The next block of interleaved samples (it may split a frame).
    pub fn feed(&mut self, block: &[i32]) {
        let (Some(maker), Some(hash)) = (self.maker.as_mut(), self.hash.as_mut()) else {
            return;
        };
        let ch_n = self.ch_n;
        hash.feed(block);
        // Between windows only the frames are counted: a block whose every frame (the one it
        // ends inside included) comes before the next window is skipped whole.
        let total = u64::from(self.chan) + block.len() as u64;
        if self.frame + total / u64::from(ch_n) < maker.from {
            self.frame += total / u64::from(ch_n);
            self.chan = (total % u64::from(ch_n)) as u32;
            self.acc = 0;
            return;
        }
        for &s in block {
            self.acc += i64::from(s);
            self.chan += 1;
            if self.chan == ch_n {
                maker.frame(self.frame, self.acc as f64 * self.scale);
                self.frame += 1;
                self.chan = 0;
                self.acc = 0;
            }
        }
    }

    /// The first window and the hash are done (the song is longer than its first window).
    pub fn start_done(&self) -> bool {
        self.maker.as_ref().is_some_and(|m| !m.windows.is_empty())
            && self.hash.as_ref().is_some_and(|h| h.samples >= h.samples_max)
    }

    /// What it has so far: the windows done and the hash (once its 10 s are in).
    pub fn so_far(&self) -> Envelope {
        Envelope {
            hop_ms: HOP_MS,
            windows: self
                .maker
                .as_ref()
                .map_or_else(Vec::new, |m| m.windows.clone()),
            pcm_hash: self.hash.clone().and_then(PcmHash::finish),
        }
    }

    /// The whole song's envelope (the song ended).
    pub fn finish(self) -> Envelope {
        let mut out = Envelope {
            hop_ms: HOP_MS,
            windows: Vec::new(),
            pcm_hash: None,
        };
        let (Some(mut maker), Some(hash)) = (self.maker, self.hash) else {
            return out;
        };
        if self.frame > maker.kept {
            maker.close();
        }
        out.windows = maker.windows;
        out.pcm_hash = hash.finish();
        out
    }
}

// The verdicts and the whole comparison are the contract's (`together::align`), so the room's
// measure reads them without this crate.
pub use together::align::{Track, Verdict, WindowCheck};

/// Where my copy sits against the reference (contract 5.8). See [`align_track`].
pub fn align(reference: &Envelope, mine: &Envelope) -> Alignment {
    align_track(reference, mine).alignment
}

/// Compares two envelopes.
///
/// - **Exact** ([`Alignment::EXACT`]): both PCM hashes are present and equal, and nothing
///   later (a part of the first window, a later window) shows an edit.
/// - **Measured**: the first window's peak passes the confidence rules; at least two of its
///   10 s parts pass them on their own at the same offset (within 20 ms; a song too short for
///   two parts needs only the whole); no part and no later window shows an edit. The offset
///   is the first window's; `confidence` is its peak in standard deviations over the mean.
/// - **Approximate**: anything else, with confidence 0. The offset is the start's when the
///   start was measured with confidence (another edit: aligned at the start as well as can
///   be), otherwise 0 (the caller adds what it knows of codec delays).
pub fn align_track(reference: &Envelope, mine: &Envelope) -> Track {
    let mut track = Track {
        alignment: Alignment::APPROXIMATE,
        windows: Vec::new(),
        edit_at_ms: None,
    };
    if reference.hop_ms == 0 || reference.hop_ms != mine.hop_ms {
        return track;
    }
    let hop = i64::from(reference.hop_ms);
    let rules = Rules {
        lags: SEARCH_MS / hop,
        min_overlap: MIN_OVERLAP_MS / hop,
        away: SECOND_PEAK_AWAY_MS / hop,
        agree: AGREE_MS / hop as f64,
        hop_us: hop as f64 * 1_000.0,
    };
    let exact = matches!((reference.pcm_hash, mine.pcm_hash), (Some(a), Some(b)) if a == b);
    fn first(e: &Envelope) -> Option<&[u8]> {
        e.windows
            .iter()
            .find(|(s, _)| *s == 0)
            .map(|(_, w)| w.as_slice())
    }
    let (Some(r), Some(m)) = (first(reference), first(mine)) else {
        if exact {
            track.alignment = Alignment::EXACT;
        }
        return track;
    };

    // The first window as a whole: the offset, if it stands out.
    let whole = Scan::new(r, 0, m, 0, &rules).filter(Scan::confident);
    let mut prior = match (&whole, exact) {
        (_, true) => Some(0.0),
        (Some(w), false) => Some(w.refined),
        (None, false) => None,
    };
    // How alike the two copies are where they match: later stretches must match about as
    // well at the offset (identical audio matches perfectly).
    let level = if exact {
        1.0
    } else {
        whole.as_ref().map_or(1.0, |w| w.peak_value)
    };

    // Its 10 s parts, each on its own, must find the same offset.
    let piece = (PIECE_MS / hop) as usize;
    let pieces = m.len() / piece;
    let part = |p: usize| &m[p * piece..(p + 1) * piece];
    let scans: Vec<Option<Scan>> = (0..pieces)
        .map(|p| Scan::new(r, 0, part(p), (p * piece) as i64, &rules))
        .collect();
    // The start as the first part measures it: when an edit inside the first window splits
    // the match in two, the whole may not stand out, or may match the later part best.
    let start_piece = scans
        .first()
        .and_then(Option::as_ref)
        .filter(|s| s.confident())
        .map(|s| s.refined);
    let mut agreeing = 0;
    if let Some(known) = prior.or(start_piece) {
        for (p, scan) in scans.iter().enumerate() {
            let Some(scan) = scan else { continue };
            if prior.is_some() || p > 0 {
                match scan.verdict(known, level, active(part(p)) && active(r), &rules) {
                    Verdict::Moved { .. } | Verdict::Lost => {
                        track.edit_at_ms.get_or_insert((p as i64 * PIECE_MS) as u32);
                    }
                    Verdict::Agrees { .. } | Verdict::Unknown => {}
                }
            }
            if scan.confident() && (scan.refined - known).abs() <= rules.agree {
                agreeing += 1;
            }
        }
    }
    let edit_in_first = track.edit_at_ms.is_some();
    // Where the copy starts: 0 for the same audio; the first part's offset when an edit shows
    // inside the first window; otherwise the whole window's.
    let start = match (exact, edit_in_first) {
        (true, _) => Some(0.0),
        (false, true) => start_piece.or(prior),
        (false, false) => prior,
    };
    let trusted = exact || (whole.is_some() && (pieces < 2 || agreeing >= 2));
    if !trusted {
        prior = None;
    }

    // Later windows check the offset through the song.
    for (start_ms, rw) in reference.windows.iter().filter(|(s, _)| *s > 0) {
        let Some((_, mw)) = mine.windows.iter().find(|(s, _)| s == start_ms) else {
            continue;
        };
        let verdict = match prior {
            Some(known) => {
                let at = i64::from(*start_ms) / hop;
                Scan::new(rw, at, mw, at, &rules).map_or(Verdict::Unknown, |s| {
                    s.verdict(known, level, active(rw) && active(mw), &rules)
                })
            }
            None => Verdict::Unknown,
        };
        match verdict {
            // Within 20 ms: a measured offset glides to the new measure.
            Verdict::Agrees { offset_us } if !exact => {
                prior = Some(offset_us as f64 / rules.hop_us)
            }
            Verdict::Moved { offset_us } => {
                track.edit_at_ms.get_or_insert(*start_ms);
                prior = Some(offset_us as f64 / rules.hop_us);
            }
            Verdict::Lost => {
                track.edit_at_ms.get_or_insert(*start_ms);
            }
            _ => {}
        }
        track.windows.push(WindowCheck {
            start_ms: *start_ms,
            verdict,
        });
    }

    track.alignment = match (trusted, track.edit_at_ms, start) {
        (true, None, _) if exact => Alignment::EXACT,
        (true, None, Some(s)) => Alignment {
            offset_us: rules.us(s),
            class: CopyClass::Measured,
            confidence: whole.as_ref().map_or(0.0, |w| w.z(w.peak_value)) as f32,
        },
        // Another edit: aligned at the start as well as can be.
        (_, Some(_), Some(s)) => Alignment {
            offset_us: rules.us(s),
            ..Alignment::APPROXIMATE
        },
        _ => Alignment::APPROXIMATE,
    };
    track
}

/// The rules in hops.
struct Rules {
    lags: i64,
    min_overlap: i64,
    away: i64,
    agree: f64,
    hop_us: f64,
}

impl Rules {
    fn us(&self, hops: f64) -> i64 {
        (hops * self.hop_us).round() as i64
    }
}

/// Whether a stretch has music: at least one strong rise per second.
fn active(w: &[u8]) -> bool {
    let strong = w.iter().filter(|&&b| b >= ACTIVE_LEVEL).count();
    !w.is_empty() && strong * 100 >= w.len()
}

/// The normalised cross-correlation of two envelope stretches over lags of -5 s to +5 s:
/// at lag `L`, `mine[a + L]` against `reference[a]`, in absolute hops, over their overlap.
struct Scan {
    lo: i64,
    /// One value per lag from `lo`; `None` where the overlap is too short.
    values: Vec<Option<f64>>,
    center: f64,
    spread: f64,
    peak_value: f64,
    /// The peak's lag refined by a parabola, in hops.
    refined: f64,
    /// The best local peak more than 50 ms from the peak.
    second: f64,
}

impl Scan {
    /// `r` starts at absolute hop `r0`, `m` at `m0`.
    fn new(r: &[u8], r0: i64, m: &[u8], m0: i64, rules: &Rules) -> Option<Self> {
        fn prefix(w: &[u8]) -> (Vec<u64>, Vec<u64>) {
            let mut s = vec![0u64; w.len() + 1];
            let mut q = vec![0u64; w.len() + 1];
            for (i, &x) in w.iter().enumerate() {
                s[i + 1] = s[i] + u64::from(x);
                q[i + 1] = q[i] + u64::from(x) * u64::from(x);
            }
            (s, q)
        }
        let (rs, rq) = prefix(r);
        let (ms, mq) = prefix(m);
        let (r1, m1) = (r0 + r.len() as i64, m0 + m.len() as i64);
        let lo = -rules.lags;
        let mut values = Vec::with_capacity((2 * rules.lags + 1) as usize);
        for lag in lo..=rules.lags {
            let a0 = r0.max(m0 - lag);
            let n = r1.min(m1 - lag) - a0;
            if n < rules.min_overlap {
                values.push(None);
                continue;
            }
            let (ri, mi, n) = ((a0 - r0) as usize, (a0 + lag - m0) as usize, n as usize);
            let dot: u64 = r[ri..ri + n]
                .iter()
                .zip(&m[mi..mi + n])
                .map(|(&x, &y)| u32::from(x) * u32::from(y))
                .map(u64::from)
                .sum();
            let (sr, qr) = ((rs[ri + n] - rs[ri]) as f64, (rq[ri + n] - rq[ri]) as f64);
            let (sm, qm) = ((ms[mi + n] - ms[mi]) as f64, (mq[mi + n] - mq[mi]) as f64);
            let nf = n as f64;
            let den = ((nf * qr - sr * sr) * (nf * qm - sm * sm)).sqrt();
            values.push(Some(if den > 0.0 {
                (nf * dot as f64 - sr * sm) / den
            } else {
                0.0
            }));
        }
        let valid = values.iter().flatten().count();
        if valid < 3 {
            return None;
        }
        // The background's centre and spread, robustly: the median and 1.4826 times the
        // median absolute deviation (a standard deviation for normal noise). A plain standard
        // deviation is inflated by the music's own beat (a peak at every beat's lag), which
        // put a perfect copy at 7 deviations; the robust one puts it near 60 and unrelated
        // songs near 4.
        let mut sorted: Vec<f64> = values.iter().flatten().copied().collect();
        let mid = sorted.len() / 2;
        let center = *sorted.select_nth_unstable_by(mid, f64::total_cmp).1;
        for v in &mut sorted {
            *v = (*v - center).abs();
        }
        let spread = 1.4826 * *sorted.select_nth_unstable_by(mid, f64::total_cmp).1;
        let (peak, peak_value) = values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.map(|v| (i, v)))
            .fold(
                (0, f64::MIN),
                |best, (i, v)| if v > best.1 { (i, v) } else { best },
            );
        let mut scan = Self {
            lo,
            values,
            center,
            spread,
            peak_value,
            refined: 0.0,
            second: f64::MIN,
        };
        scan.refined = scan.parabola(peak);
        scan.second = (0..scan.values.len())
            .filter(|&i| (i as i64 - peak as i64).abs() > rules.away && scan.is_local_peak(i))
            .filter_map(|i| scan.values[i])
            .fold(f64::MIN, f64::max);
        Some(scan)
    }

    fn at(&self, i: i64) -> Option<f64> {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.values.get(i).copied().flatten())
    }

    fn is_local_peak(&self, i: usize) -> bool {
        let Some(v) = self.values[i] else {
            return false;
        };
        let i = i as i64;
        self.at(i - 1).is_none_or(|l| v >= l) && self.at(i + 1).is_none_or(|r| v > r)
    }

    /// The lag at index `i` refined by a parabola through it and its neighbours, in hops.
    fn parabola(&self, i: usize) -> f64 {
        let lag = self.lo + i as i64;
        let i = i as i64;
        let (Some(l), Some(c), Some(r)) = (self.at(i - 1), self.at(i), self.at(i + 1)) else {
            return lag as f64;
        };
        let den = l - 2.0 * c + r;
        let delta = if den < 0.0 { 0.5 * (l - r) / den } else { 0.0 };
        lag as f64 + delta.clamp(-0.5, 0.5)
    }

    fn z(&self, v: f64) -> f64 {
        if self.spread > 0.0 {
            (v - self.center) / self.spread
        } else {
            0.0
        }
    }

    /// The design's confidence rules: 8 standard deviations over the mean, and 1.3 times the
    /// best peak more than 50 ms away.
    fn confident(&self) -> bool {
        self.peak_value > 0.0
            && self.z(self.peak_value) >= PEAK_SD
            && (self.second <= 0.0 || self.peak_value >= PEAK_RATIO * self.second)
    }

    /// Indices of the lags within `agree` hops of `known`.
    fn around(&self, known: f64, agree: f64) -> impl Iterator<Item = usize> + '_ {
        let lo = (known - agree).floor() as i64 - self.lo;
        let hi = (known + agree).ceil() as i64 - self.lo;
        (lo.max(0)..=hi.min(self.values.len() as i64 - 1)).map(|i| i as usize)
    }

    /// The best local peak whose refined lag is within `agree` hops of `known`:
    /// (value, refined lag).
    fn near(&self, known: f64, agree: f64) -> Option<(f64, f64)> {
        self.around(known, agree)
            .filter(|&i| self.is_local_peak(i))
            .filter_map(|i| {
                let refined = self.parabola(i);
                ((refined - known).abs() <= agree).then_some((self.values[i]?, refined))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// What this stretch says about an offset already known (`known`, in hops). `level` is
    /// how well the copies matched where the offset was found: a stretch agrees only when it
    /// matches about as well at the offset (within the 1.3 ratio of the confidence rule), and
    /// a match elsewhere counts only when it is that good too. A beat's echo (a lag of one
    /// beat or bar) scores about half, so it neither agrees nor moves the offset.
    fn verdict(&self, known: f64, level: f64, active: bool, rules: &Rules) -> Verdict {
        let near = self.near(known, rules.agree);
        let alike = level / PEAK_RATIO;
        if self.confident()
            && (self.refined - known).abs() > rules.agree
            && self.peak_value >= alike
            && near.is_none_or(|(v, _)| self.peak_value >= PEAK_RATIO * v)
        {
            return Verdict::Moved {
                offset_us: rules.us(self.refined),
            };
        }
        if let Some((v, refined)) = near
            && self.z(v) >= PEAK_SD
            && v >= alike
        {
            return Verdict::Agrees {
                offset_us: rules.us(refined),
            };
        }
        let best_around = self
            .around(known, rules.agree)
            .filter_map(|i| self.values[i])
            .fold(f64::MIN, f64::max);
        if active && best_around < LOST_LEVEL * level {
            return Verdict::Lost;
        }
        Verdict::Unknown
    }
}
