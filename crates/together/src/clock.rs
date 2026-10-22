//! The room's shared clock: the host's, as each guest estimates it from pings, published as a
//! line (a [`ClockMap`]) the player reads directly.
//!
//! A guest sends a ping at its time `t1`; the host stamps when it got it (`t2`) and when it
//! answered (`t3`); the guest stamps the answer's arrival (`t4`). Then (the NTP method):
//! - offset = ((t2 - t1) + (t3 - t4)) / 2, the host's clock minus the guest's;
//! - delay = (t4 - t1) - (t3 - t2), the round trip without the host's own time.
//!
//! Whatever the path, the true offset lies between `t3 - t4` and `t2 - t1` (the host stamped
//! both while the ping was out), so a sample's offset is off by at most half its delay.
//! Networks have a long tail of slow answers (Wi-Fi, the internet, a queue behind an upload),
//! so the estimate trusts the samples with the least delay, as Jellyfin's SyncPlay does.
//!
//! The clock is fed by timestamped samples and told about changes of path and reconnects; it
//! doesn't send or time anything itself, so it runs the same over any transport and in
//! simulated time. All times are microseconds on each side's monotonic clock (the guest's
//! `t1` and `t4` are its `MonoClock` nanoseconds / 1000), never the wall clock.
//!
//! How it estimates (design 4.2):
//! - a window of the last 64 samples, in epochs (a sample far off the line, a sleep, starts a
//!   new epoch and clears the window) and in paths (a change of path or a reconnect starts a
//!   new one: a line is fitted through one path's samples only, as each path has its own bias);
//! - of the current path's samples, the quickest: delay at most the least plus the larger of
//!   1 ms and half the way to the median;
//! - through them a weighted line (offset and skew) once there are 8 over 30 s; before that,
//!   the mean of the 3 quickest (their age counted against them) carried forward at the last
//!   known skew, or a line through the shorter time when that is better founded (on a quiet
//!   network the rate shows in seconds);
//! - each estimate has an honest bound: half the least delay, plus the 90th percentile of the
//!   residuals, plus what carrying it forward may add (three standard errors of its slope a
//!   second); the best-founded estimate (the smallest bound, an older line's growing with its
//!   age) is the target, so a slower path or a queue never pulls the clock away from a line
//!   the quick samples gave;
//! - an answer whose timestamps rule out the kept line (the clocks moved against each other)
//!   counts as a change of path; one ruling it out by over 500 ms starts a new epoch;
//! - what the player reads is the target slewed: moved toward it by at most 50 µs a second,
//!   except a lasting jump (over 5 ms in the same room, 20 ms apart) which is stepped after
//!   10 s, or after 2 s when it follows a change of path or a reconnect. The published bound
//!   adds what is still to slew.

use crate::clock_map::{ClockMap, ClockMapHandle};
use std::collections::VecDeque;
use std::sync::Arc;

/// One ping's four timestamps, in microseconds (t1 and t4 the guest's, t2 and t3 the host's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub t1: i64,
    pub t2: i64,
    pub t3: i64,
    pub t4: i64,
}

impl Sample {
    /// The host's clock minus the guest's.
    pub fn offset(&self) -> i64 {
        ((self.t2 - self.t1) + (self.t3 - self.t4)) / 2
    }

    /// The round trip, the host's own time taken out.
    pub fn delay(&self) -> i64 {
        ((self.t4 - self.t1) - (self.t3 - self.t2)).max(0)
    }

    /// Whether the timestamps make sense (an answer can't come before its ping).
    fn sane(&self) -> bool {
        self.t4 >= self.t1 && self.t3 >= self.t2
    }

    /// The local moment the sample's offset belongs to: the middle of the round trip.
    fn mid(&self) -> i64 {
        self.t1 + (self.t4 - self.t1) / 2
    }

    /// The offset exactly (the half microsecond kept).
    fn offset_f(&self) -> f64 {
        ((self.t2 - self.t1) + (self.t3 - self.t4)) as f64 / 2.0
    }

    /// How far `offset` is outside what this sample's timestamps allow (`t3 - t4` to
    /// `t2 - t1`); 0 when inside.
    fn outside(&self, offset: f64) -> f64 {
        let lo = (self.t3 - self.t4) as f64;
        let hi = (self.t2 - self.t1) as f64;
        (lo - offset).max(offset - hi).max(0.0)
    }
}

/// How big a lasting jump is stepped rather than slewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Steps {
    /// Every device in one room: over 5 ms.
    SameRoom,
    /// Apart: over 20 ms.
    #[default]
    Apart,
}

impl Steps {
    fn jump_us(self) -> f64 {
        match self {
            Steps::SameRoom => 5_000.0,
            Steps::Apart => 20_000.0,
        }
    }
}

/// How many recent samples are kept (a minute or so of pings: long enough to see the two
/// clocks' rates differ).
const WINDOW: usize = 64;
/// Of the quickest, how many are averaged while there are too few to fit a line.
const BEST: usize = 3;
/// A sample's delay may be over the quickest by at least this (microseconds) and still count.
const SLACK_US: i64 = 1_000;
/// A line (offset and rate) is fitted through this many samples over this long (microseconds);
/// over less, only when it is better founded than the mean of the quickest.
const FIT_SAMPLES: usize = 8;
const FIT_SPAN_US: i64 = 30_000_000;
/// No sound card or computer clock is this far off; a fit beyond it is noise.
const MAX_SKEW: f64 = 500e-6;
/// How far apart two computers' clocks may run when nothing is known yet (a generous figure:
/// crystals are within 50 to 100 ppm of each other).
const UNKNOWN_SKEW: f64 = 200e-6;
/// The least an estimate's bound grows a second, on top of its slope's own uncertainty.
const AGE_FLOOR: f64 = 2e-6;
/// The clock the players read moves toward the estimate this fast at most (microseconds per
/// second), and by at most two seconds' worth at once (after a gap in the answers): a better
/// estimate is taken up without a step anyone hears.
const SLEW_US_PER_S: f64 = 50.0;
const SLEW_SPAN_US: i64 = 2_000_000;
/// A jump past the step size that lasts this long (microseconds) is stepped...
const STEP_AFTER_US: i64 = 10_000_000;
/// ...or this long, when it starts within `CHANGE_WINDOW_US` of a change of path or a
/// reconnect (the old path's bias no longer holds).
const STEP_AFTER_CHANGE_US: i64 = 2_000_000;
const CHANGE_WINDOW_US: i64 = 10_000_000;
/// A sample this far (microseconds) outside what its timestamps allow of the line: the line no
/// longer describes the clocks (a sleep, a clock reset), so a new epoch begins.
const EPOCH_US: f64 = 500_000.0;
/// A sample this far outside its timestamps of a kept line proves that line wrong.
const CONTRADICTED_US: f64 = 200.0;
/// Ready once an epoch has this many samples, spread over this long (the burst's 20 pings
/// take 950 ms).
const READY_SAMPLES: usize = 8;
const READY_SPAN_US: i64 = 900_000;

/// The offset (host minus local, µs) as a line through local time (µs).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Line {
    t_ref: i64,
    off: f64,
    skew: f64,
}

impl Line {
    fn at(&self, t: i64) -> f64 {
        self.off + self.skew * (t - self.t_ref) as f64
    }

    fn moved_to(&self, t: i64) -> Line {
        Line {
            t_ref: t,
            off: self.at(t),
            skew: self.skew,
        }
    }
}

/// An estimate of the line and how far from the truth it may be: `bound` µs at local `at`,
/// growing after by how wrong its rate may be (`rate_err`, three standard errors of a fitted
/// slope, or what two clocks may differ by when nothing is known) and a little more.
#[derive(Debug, Clone, Copy)]
struct Estimate {
    line: Line,
    bound: f64,
    at: i64,
    rate_err: f64,
    fitted: bool,
}

impl Estimate {
    fn bound_at(&self, t: i64) -> f64 {
        self.bound + (self.rate_err + AGE_FLOOR) * (t - self.at).max(0) as f64
    }
}

#[derive(Debug, Clone, Copy)]
struct Kept {
    s: Sample,
    path: u32,
}

/// A guest's estimate of the host's clock, published as a [`ClockMap`].
///
/// Feed it every answer with [`add`](Clock::add), tell it when the path changes or the
/// connection comes back, and publish its [`map`](Clock::map) after each (see
/// [`publish`](Clock::publish)). The host's own map is [`ClockMap::identity`].
#[derive(Debug, Clone, Default)]
pub struct Clock {
    steps: Steps,
    epoch: u32,
    samples: VecDeque<Kept>,
    path: u32,
    /// The last change of path or reconnect (local time).
    changed_at: Option<i64>,
    /// The best-founded estimate so far in this epoch.
    best: Option<Estimate>,
    /// The last fitted skew and how fast an estimate carried forward on it ages.
    skew_prior: Option<(f64, f64)>,
    ready: bool,
    /// What the players read (only once ready), and its bound at its `t_ref`.
    shown: Option<Line>,
    shown_bound: f64,
    /// Since when (local time) the target has been a step away from what's shown.
    far_since: Option<i64>,
    steps_taken: u32,
}

impl Clock {
    /// A clock for listening apart (the step size of 20 ms).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_steps(steps: Steps) -> Self {
        Clock {
            steps,
            ..Self::default()
        }
    }

    /// The step size follows the person's mode (every device or apart).
    pub fn set_steps(&mut self, steps: Steps) {
        self.steps = steps;
    }

    /// Take one ping's answer (a nonsense one is left out).
    pub fn add(&mut self, s: Sample) {
        if !s.sane() {
            return;
        }
        let now = s.t4;
        if let Some(line) = self.shown.or(self.best.map(|b| b.line))
            && s.outside(line.at(s.mid())) > EPOCH_US
        {
            self.start_epoch();
        }
        if let Some(b) = self.best
            && s.outside(b.line.at(s.mid())) > CONTRADICTED_US
        {
            // The kept line is proven wrong (this answer's timestamps can't allow it): the
            // clocks moved against each other, so the samples before describe another
            // relation. As a change of path: fitted apart, and stepped after 2 s.
            self.path_changed(now);
            self.best = None;
        }
        self.samples.push_back(Kept { s, path: self.path });
        if self.samples.len() > WINDOW {
            self.samples.pop_front();
        }
        if let Some(e) = self.estimate(now) {
            if e.fitted {
                self.skew_prior = Some((e.line.skew, e.rate_err));
            }
            match self.best {
                Some(b) if b.bound_at(now) < e.bound => {}
                _ => self.best = Some(e),
            }
        }
        if !self.ready {
            self.ready = self.samples.len() >= READY_SAMPLES
                && self.samples.back().map(|k| k.s.t1).unwrap_or(0)
                    - self.samples.front().map(|k| k.s.t1).unwrap_or(0)
                    >= READY_SPAN_US;
            if !self.ready {
                return;
            }
        }
        let Some(target) = self.best else {
            return;
        };
        let bound = target.bound_at(now);
        let Some(shown) = self.shown else {
            self.shown = Some(target.line.moved_to(now));
            self.shown_bound = bound;
            self.far_since = None;
            return;
        };
        let cur = shown.at(now);
        let gap = target.line.at(now) - cur;
        if gap.abs() > self.steps.jump_us() {
            let since = *self.far_since.get_or_insert(now);
            let after_change = self
                .changed_at
                .is_some_and(|c| since >= c && since - c <= CHANGE_WINDOW_US);
            let wait = if after_change {
                STEP_AFTER_CHANGE_US
            } else {
                STEP_AFTER_US
            };
            if now - since >= wait {
                self.shown = Some(target.line.moved_to(now));
                self.shown_bound = bound;
                self.far_since = None;
                self.steps_taken += 1;
                return;
            }
        } else {
            self.far_since = None;
        }
        let dt = (now - shown.t_ref).clamp(0, SLEW_SPAN_US);
        let most = SLEW_US_PER_S * dt as f64 / 1e6;
        let moved = gap.clamp(-most, most);
        self.shown = Some(Line {
            t_ref: now,
            off: cur + moved,
            skew: target.line.skew,
        });
        self.shown_bound = bound + (gap - moved).abs();
    }

    /// The transport says the selected path changed (relay to direct, one address to another):
    /// the new path's samples are fitted apart from the old, and a lasting jump that follows
    /// is stepped after 2 s. A burst of pings should follow.
    pub fn path_changed(&mut self, local_us: i64) {
        self.path = self.path.wrapping_add(1);
        self.changed_at = Some(local_us);
        self.far_since = None;
    }

    /// The connection came back (perhaps by another path): as a change of path.
    pub fn reconnected(&mut self, local_us: i64) {
        self.path_changed(local_us);
    }

    /// Start a new epoch: the window cleared, nothing published until the clock is ready again.
    /// The session calls it on waking from a sleep; a sample far off the line does it too.
    pub fn new_epoch(&mut self) {
        self.start_epoch();
    }

    /// The line the player reads; `None` until the clock is ready (in this epoch).
    pub fn map(&self) -> Option<ClockMap> {
        let l = self.shown?;
        Some(ClockMap {
            epoch: self.epoch,
            local_ref_ns: (l.t_ref.max(0) as u64).saturating_mul(1_000),
            host_ref_us: l.t_ref + l.off.round() as i64,
            rate: 1.0 + l.skew,
            bound_us: (self.shown_bound.ceil() + 1.0).clamp(0.0, u32::MAX as f64) as u32,
        })
    }

    /// Put the current map where the player reads it.
    pub fn publish(&self, to: &ClockMapHandle) {
        to.store(Arc::new(self.map()));
    }

    /// Which epoch the clock is in (it starts at 0).
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// How many lasting jumps have been stepped (for the log).
    pub fn steps_taken(&self) -> u32 {
        self.steps_taken
    }

    /// How many answers it has (in this epoch, the window's worth).
    pub fn samples(&self) -> usize {
        self.samples.len()
    }

    /// Enough answers to trust it: 8, over the burst's 900 ms (and then for the epoch).
    pub fn ready(&self) -> bool {
        self.ready && self.shown.is_some()
    }

    /// The host's clock minus this one's, in microseconds, as the players read it, at the last
    /// answer.
    pub fn offset(&self) -> Option<i64> {
        self.shown.map(|l| l.off.round() as i64)
    }

    /// How much faster the host's clock runs than this one, in parts per million (0 until it
    /// is known).
    pub fn skew_ppm(&self) -> f64 {
        self.shown
            .map(|l| l.skew)
            .or(self.skew_prior.map(|p| p.0))
            .unwrap_or(0.0)
            * 1e6
    }

    /// The round trip of the quickest answer, in microseconds (how good the estimate can be:
    /// its error is at most half of it).
    pub fn delay(&self) -> Option<i64> {
        self.samples.iter().map(|k| k.s.delay()).min()
    }

    /// The host's time for this clock's time `local_us`, both in microseconds.
    pub fn host_time(&self, local_us: i64) -> Option<i64> {
        self.shown.map(|l| local_us + l.at(local_us).round() as i64)
    }

    /// This clock's time for a moment on the host's clock, both in microseconds.
    pub fn local_time(&self, host_us: i64) -> Option<i64> {
        self.shown.map(|l| {
            let d = (host_us - l.t_ref) as f64 - l.off;
            l.t_ref + (d / (1.0 + l.skew)).round() as i64
        })
    }

    fn start_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.samples.clear();
        self.best = None;
        self.ready = false;
        self.shown = None;
        self.shown_bound = 0.0;
        self.far_since = None;
        self.changed_at = None;
    }

    /// The estimate from the current path's samples, at local `now`.
    fn estimate(&self, now: i64) -> Option<Estimate> {
        let mine: Vec<Sample> = self
            .samples
            .iter()
            .filter(|k| k.path == self.path)
            .map(|k| k.s)
            .collect();
        let mut delays: Vec<i64> = mine.iter().map(Sample::delay).collect();
        delays.sort_unstable();
        let least = *delays.first()?;
        let middle = delays[delays.len() / 2];
        let line_at = least + ((middle - least) / 2).max(SLACK_US);
        let chosen: Vec<Sample> = mine.into_iter().filter(|s| s.delay() <= line_at).collect();
        // How far from the truth a line through `chosen` is at `now`: half the least delay
        // (the most a path's asymmetry can hide), the 90th percentile of the residuals, and
        // `extra` for carrying it forward.
        let judged = |line: Line, extra: f64, rate_err: f64, fitted: bool| {
            let mut residuals: Vec<f64> = chosen
                .iter()
                .map(|s| (s.offset_f() - line.at(s.mid())).abs())
                .collect();
            residuals.sort_by(f64::total_cmp);
            let p90 = residuals[(residuals.len() * 9).div_ceil(10).max(1) - 1];
            Estimate {
                line,
                bound: least as f64 / 2.0 + p90 + extra,
                at: now,
                rate_err,
                fitted,
            }
        };

        let fitted = fit(&chosen).map(|(line, se, span)| {
            // Carried to now, the line may be off by the slope's uncertainty times the way.
            let rate_err = (3.0 * se).min(MAX_SKEW);
            let extra = rate_err * (now - line.t_ref).abs() as f64;
            (judged(line, extra, rate_err, true), span)
        });
        if let Some((e, span)) = fitted
            && span >= FIT_SPAN_US
        {
            return Some(e);
        }
        // Too short a time for the design's fit: the mean of the quickest few (their age
        // counted against them), carried forward at the last known skew, unless a line through
        // the shorter time is better founded already (a quiet network shows the rate in
        // seconds, and a line that lags the host for 30 s would be worse than its noise).
        let (skew, rate_err) = self.skew_prior.unwrap_or((0.0, UNKNOWN_SKEW));
        let cost = |s: &Sample| s.delay() as f64 / 2.0 + rate_err * (now - s.mid()).abs() as f64;
        let mut quick = chosen.clone();
        quick.sort_by(|a, b| cost(a).total_cmp(&cost(b)));
        quick.truncate(BEST);
        let off = quick
            .iter()
            .map(|s| s.offset_f() + skew * (now - s.mid()) as f64)
            .sum::<f64>()
            / quick.len() as f64;
        let oldest = quick.iter().map(|s| (now - s.mid()).abs()).max()?;
        let line = Line {
            t_ref: now,
            off,
            skew,
        };
        let flat = judged(line, rate_err * oldest as f64, rate_err, false);
        match fitted {
            Some((e, _)) if e.bound < flat.bound => Some(e),
            _ => Some(flat),
        }
    }
}

/// A line through the samples, weighted toward the quickest, with its slope's standard error
/// and the time it spans; `None` with too few samples, or over less than the burst.
fn fit(chosen: &[Sample]) -> Option<(Line, f64, i64)> {
    if chosen.len() < FIT_SAMPLES {
        return None;
    }
    let first = chosen.iter().map(Sample::mid).min()?;
    let last = chosen.iter().map(Sample::mid).max()?;
    let span = last - first;
    if span < READY_SPAN_US {
        return None;
    }
    let base = chosen[0].offset_f();
    let weight = |s: &Sample| 1.0 / (s.delay() + SLACK_US) as f64;
    let sw: f64 = chosen.iter().map(weight).sum();
    let t_ref = first
        + (chosen
            .iter()
            .map(|s| weight(s) * (s.mid() - first) as f64)
            .sum::<f64>()
            / sw)
            .round() as i64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for s in chosen {
        let w = weight(s);
        let x = (s.mid() - t_ref) as f64;
        let y = s.offset_f() - base;
        sx += w * x;
        sy += w * y;
        sxx += w * x * x;
        sxy += w * x * y;
    }
    let var = sxx - sx * sx / sw;
    if var <= 0.0 {
        return None;
    }
    let b = ((sxy - sx * sy / sw) / var).clamp(-MAX_SKEW, MAX_SKEW);
    let a = (sy - b * sx) / sw;
    let n = chosen.len() as f64;
    let rss: f64 = chosen
        .iter()
        .map(|s| {
            let r = s.offset_f() - base - (a + b * (s.mid() - t_ref) as f64);
            weight(s) * r * r
        })
        .sum();
    // The residuals' variance over the spread of the times, both as weighted means.
    let se = ((rss / sw) * n / (n - 2.0) / (n * var / sw)).sqrt();
    Some((
        Line {
            t_ref,
            off: base + a,
            skew: b,
        },
        se,
        span,
    ))
}

impl ClockMap {
    /// The host's own map: its time is its monotonic clock, exactly.
    pub fn identity() -> ClockMap {
        ClockMap {
            epoch: 0,
            local_ref_ns: 0,
            host_ref_us: 0,
            rate: 1.0,
            bound_us: 0,
        }
    }

    /// The host's time (µs) at this computer's monotonic `local_ns`.
    pub fn host_of(&self, local_ns: u64) -> i64 {
        let d = local_ns as i128 - self.local_ref_ns as i128;
        let whole = d.div_euclid(1_000);
        let rate_part = (d as f64 * (self.rate - 1.0) / 1_000.0).round() as i128;
        (self.host_ref_us as i128 + whole + rate_part).clamp(i64::MIN as i128, i64::MAX as i128)
            as i64
    }

    /// This computer's monotonic time (ns) at the host's `host_us`.
    pub fn local_of(&self, host_us: i64) -> u64 {
        let d = (host_us as i128 - self.host_ref_us as i128) * 1_000;
        let rate_part = (d as f64 * (1.0 / self.rate - 1.0)).round() as i128;
        (self.local_ref_ns as i128 + d + rate_part).clamp(0, u64::MAX as i128) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ping that took `up` microseconds to reach the host and `down` to come back, sent at the
    /// guest's `at`, with the host's clock `skew` ahead and 50 microseconds spent answering.
    fn ping(at: i64, skew: i64, up: i64, down: i64) -> Sample {
        let t1 = at;
        let t2 = at + up + skew;
        let t3 = t2 + 50;
        let t4 = t1 + up + 50 + down;
        Sample { t1, t2, t3, t4 }
    }

    #[test]
    fn an_even_path_gives_the_exact_offset() {
        let s = ping(1_000, 250_000, 4_000, 4_000);
        assert_eq!(s.offset(), 250_000);
        assert_eq!(s.delay(), 8_000);
        // What its timestamps allow: 4 ms either way.
        assert_eq!(s.outside(250_000.0), 0.0);
        assert_eq!(s.outside(254_000.0), 0.0);
        assert_eq!(s.outside(255_000.0), 1_000.0);
        assert_eq!(s.outside(245_000.0), 1_000.0);
    }

    #[test]
    fn slow_answers_are_outvoted_by_the_quick_ones() {
        let mut c = Clock::new();
        // Wi-Fi: mostly 3 ms each way, with a few 200 ms one-way spikes that skew their offset.
        for i in 0..12 {
            let at = i * 1_000_000;
            let s = if i % 3 == 0 {
                ping(at, -90_000, 200_000, 3_000)
            } else {
                ping(at, -90_000, 3_000, 3_000)
            };
            c.add(s);
        }
        let off = c.offset().unwrap();
        assert!(
            (off + 90_000).abs() <= 1,
            "offset {off} trusts the quick pings"
        );
        assert_eq!(c.delay(), Some(6_000));
        assert!(c.ready());
    }

    #[test]
    fn times_convert_both_ways() {
        let mut c = Clock::new();
        for i in 0..20 {
            c.add(ping(i * 50_000, 5_000, 1_000, 1_000));
        }
        assert_eq!(c.host_time(100), Some(5_100));
        assert_eq!(c.local_time(5_100), Some(100));
    }

    #[test]
    fn nonsense_and_old_answers_are_left_out() {
        let mut c = Clock::new();
        assert_eq!(c.offset(), None);
        c.add(Sample {
            t1: 10,
            t2: 5,
            t3: 6,
            t4: 9,
        });
        assert_eq!(c.samples(), 0, "an answer before its ping");
        for i in 0..(WINDOW as i64 + 20) {
            c.add(ping(i, 0, 10, 10));
        }
        assert_eq!(c.samples(), WINDOW, "the oldest are let go");
    }

    /// A minute of pings, one a second, the host's clock `skew_ppm` faster than ours, paths
    /// `up` and `down` microseconds (`change` can alter them per ping).
    fn minutes(
        c: &mut Clock,
        secs: i64,
        from_s: i64,
        skew_ppm: f64,
        path: impl Fn(i64) -> (i64, i64),
    ) {
        for i in from_s..from_s + secs {
            let at = i * 1_000_000;
            let skew = 250_000 + (at as f64 * skew_ppm / 1e6) as i64;
            let (up, down) = path(i);
            c.add(ping(at, skew, up, down));
        }
    }

    #[test]
    fn the_hosts_faster_clock_is_learned_and_followed() {
        let mut c = Clock::new();
        minutes(&mut c, 150, 0, 100.0, |_| (3_000, 3_000));
        assert!(
            (c.skew_ppm() - 100.0).abs() < 5.0,
            "100 ppm learned: {:.1}",
            c.skew_ppm()
        );
        // Where the host is now, as the players read it: within a fraction of a millisecond.
        let now = 149 * 1_000_000;
        let true_offset = 250_000 + now / 10_000;
        let off = c.offset().unwrap();
        assert!(
            (off - true_offset).abs() < 300,
            "offset {off} against {true_offset}"
        );
    }

    #[test]
    fn a_change_to_a_slower_path_moves_nothing() {
        let mut c = Clock::new();
        minutes(&mut c, 40, 0, 0.0, |_| (5_000, 5_000));
        let before = c.offset().unwrap();
        // The way there gets 20 ms longer, the way back not: the offset these pings show is
        // 10 ms off, but the quicker answers before them still hold, so nothing moves.
        minutes(&mut c, 5, 40, 0.0, |_| (25_000, 5_000));
        let after = c.offset().unwrap();
        assert!(
            (after - before).abs() <= 5 * SLEW_US_PER_S as i64 + 1,
            "moved {} µs in 5 s",
            after - before
        );
    }

    #[test]
    fn a_big_lasting_change_to_a_quicker_path_is_stepped_after_a_while() {
        let mut c = Clock::new();
        minutes(&mut c, 40, 0, 0.0, |_| (80_000, 80_000));
        let before = c.offset().unwrap();
        // Off the relay onto a direct path, 60 ms shorter one way than the other.
        minutes(&mut c, 8, 40, 0.0, |_| (20_000, 80_000));
        let soon = c.offset().unwrap();
        assert!(
            (soon - before).abs() <= 8 * SLEW_US_PER_S as i64 + 1,
            "slewing first ({} µs in 8 s)",
            soon - before
        );
        minutes(&mut c, 4, 48, 0.0, |_| (20_000, 80_000));
        let later = c.offset().unwrap();
        assert!(
            (later - before + 30_000).abs() < 1_000,
            "stepped to the new estimate after 10 s: moved {} µs",
            later - before
        );
        assert_eq!(c.steps_taken(), 1);
    }

    #[test]
    fn the_map_carries_the_line_and_its_bound() {
        let mut c = Clock::new();
        minutes(&mut c, 60, 0, 50.0, |_| (2_000, 2_000));
        let m = c.map().unwrap();
        assert!(((m.rate - 1.0) * 1e6 - 50.0).abs() < 1.0, "rate {}", m.rate);
        // Half the least delay (2 ms) and a little: noise-free samples fit exactly.
        assert!((2_000..2_100).contains(&m.bound_us), "bound {}", m.bound_us);
        let local_us = 59_000_000 + 4_050;
        assert_eq!(m.local_ref_ns, local_us as u64 * 1_000);
        let truth = local_us + 250_000 + (59_000_000.0 * 50e-6) as i64;
        assert!(
            (m.host_ref_us - truth).abs() <= 2,
            "{} against {truth}",
            m.host_ref_us
        );
    }
}
