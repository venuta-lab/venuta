//! A bench for the renderer: it plays the decode threads' part (voices filled from known songs,
//! on demand, as far as a test lets them decode), calls `render` period by period as an output
//! would, and keeps every frame it rendered, every note and every marker, so a test can judge
//! the sound with the detectors and the timing frame by frame. No threads and no clock: the
//! same run every time, however busy the machine is.

#![allow(dead_code)]

use audio_check::Declared;
use player::output::{DeviceFormat, SampleKind};
use player::output_api::Render;
use player::render::{RenderSetup, RenderStatus, Renderer};
use player::voice_api::{
    BLOCK_FRAMES, END_OF_SOURCE, FIRST_AFTER_SEEK, Marker, RateTarget, RenderCmd, RenderLink,
    RenderNote, Returned, VoiceFeed, VoiceId, VoiceTap, render_link, voice_ring,
};
use std::sync::Arc;
use together::time::RealClock;

/// Full scale of a left-justified sample.
pub const FULL: f64 = 2_147_483_648.0;

/// Device frames for `ms` at `rate`.
pub fn ms(rate: u32, ms: u32) -> u64 {
    u64::from(rate) * u64::from(ms) / 1000
}

/// A song: `frames` frames of a signal at `rate`, `ch` channels, each sample left-justified.
#[derive(Clone)]
pub struct Song {
    pub rate: u32,
    pub ch: u16,
    pub frames: u64,
    pub sample: Arc<dyn Fn(u64, u16) -> i32 + Send + Sync>,
}

impl Song {
    /// The test music (`audio_check::music::Piece`, 24-bit), frames `from..from + frames` of
    /// its endless piece: two songs cut from it one after the other follow each other seamlessly.
    pub fn music(rate: u32, ch: u16, from: u64, frames: u64) -> Song {
        let piece = audio_check::music::Piece { rate, channels: ch };
        Song {
            rate,
            ch,
            frames,
            sample: Arc::new(move |i, c| (piece.at((from + i) as f64, c).round() as i32) << 8),
        }
    }

    /// A sine of `hz` (whole Hz) at -6 dBFS (both channels in phase), from its frame `from`.
    /// Exactly periodic: frame n is worked out at n modulo the frames of a whole number of
    /// cycles, so every cycle is bit for bit the same and a place found in it is the place.
    pub fn sine(hz: f64, rate: u32, ch: u16, from: u64, frames: u64) -> Song {
        fn gcd(a: u64, b: u64) -> u64 {
            if b == 0 { a } else { gcd(b, a % b) }
        }
        let period = u64::from(rate) / gcd(u64::from(rate), hz as u64);
        Song {
            rate,
            ch,
            frames,
            sample: Arc::new(move |i, _| {
                let n = (from + i) % period;
                (0.5 * FULL * (std::f64::consts::TAU * hz * n as f64 / f64::from(rate)).sin())
                    .round() as i32
            }),
        }
    }

    /// Each frame says which it is: frame k holds `(k + 1) << 8` (every channel).
    pub fn counting(rate: u32, ch: u16, frames: u64) -> Song {
        Song {
            rate,
            ch,
            frames,
            sample: Arc::new(|i, _| ((i + 1) as i32) << 8),
        }
    }

    /// The same song rising from silence over its first 50 ms, as songs do (a cut out of the
    /// endless piece starts mid-wave, which no song does).
    pub fn rising(self) -> Song {
        let edge = ms(self.rate, 50);
        let inner = self.sample.clone();
        Song {
            sample: Arc::new(move |i, c| {
                let v = inner(i, c);
                if i >= edge {
                    return v;
                }
                let g = (i as f64 / edge as f64 * std::f64::consts::FRAC_PI_2).sin();
                (f64::from(v) * g).round() as i32
            }),
            ..self
        }
    }

    /// Channel `c` of frames `from..to`.
    pub fn channel(&self, c: u16, from: u64, to: u64) -> Vec<i32> {
        (from..to.min(self.frames))
            .map(|i| (self.sample)(i, c))
            .collect()
    }
}

/// A voice's decode thread, played by the test: it fills the voice's blocks from its song,
/// from `next`, as far as `limit` (the song's end by default).
pub struct Feed {
    pub voice: VoiceId,
    feed: VoiceFeed,
    pub song: Song,
    pub next: u64,
    pub limit: u64,
    pub epoch: u32,
    seek: bool,
    ended: bool,
}

impl Feed {
    fn new(voice: VoiceId, feed: VoiceFeed, song: Song, from: u64) -> Feed {
        let limit = song.frames;
        Feed {
            voice,
            feed,
            song,
            next: from,
            limit,
            epoch: 0,
            seek: false,
            ended: false,
        }
    }

    /// Fill every empty block it can (as far as `limit`; the song's last block says so).
    pub fn fill(&mut self) {
        while !self.ended {
            let last = self.song.frames;
            let end = self.limit.min(last);
            if self.next >= end && self.next < last {
                return;
            }
            let Ok(Some(mut b)) = self.feed.take_empty() else {
                return;
            };
            let n = end.saturating_sub(self.next).min(BLOCK_FRAMES as u64) as usize;
            let ch = usize::from(self.song.ch);
            b.voice = self.voice;
            b.epoch = self.epoch;
            b.first_frame = self.next;
            b.frames = n as u16;
            b.channels = self.song.ch as u8;
            b.rms_db = -20;
            b.flags = if self.seek { FIRST_AFTER_SEEK } else { 0 };
            self.seek = false;
            for k in 0..n {
                for c in 0..ch {
                    b.samples[k * ch + c] = (self.song.sample)(self.next + k as u64, c as u16);
                }
            }
            self.next += n as u64;
            if self.next >= last {
                b.flags |= END_OF_SOURCE;
                self.ended = true;
            }
            if self.feed.push(b).is_err() {
                return;
            }
        }
    }

    /// Decode from `frame` on with a new epoch (a seek inside the voice).
    pub fn seek_to(&mut self, frame: u64) {
        self.epoch += 1;
        self.next = frame;
        self.seek = true;
        self.ended = false;
    }
}

/// The bench: one renderer for a device format, called in periods.
pub struct Rig {
    pub r: Renderer,
    pub link: RenderLink,
    pub status: Arc<RenderStatus>,
    pub trim: Arc<RateTarget>,
    pub fmt: DeviceFormat,
    pub period: u32,
    /// The device frame of the next period.
    pub at: u64,
    /// Every frame rendered, interleaved, left-justified (decoded from the device's bytes).
    pub heard: Vec<i32>,
    pub bytes: Vec<u8>,
    pub notes: Vec<RenderNote>,
    pub markers: Vec<Marker>,
    pub returned: Vec<Returned>,
    pub feeds: Vec<Feed>,
    next_voice: u32,
    scratch: Vec<u8>,
    /// Keep the device's bytes too (for format checks).
    pub keep_bytes: bool,
    /// Called with `true` just before each `render` and `false` just after (a counting
    /// allocator's switch).
    pub around: Option<fn(bool)>,
    /// Each `render` call's wall time, ns, when kept.
    pub timings: Option<Vec<u64>>,
    /// Take notes, markers and returns after each period (off: they pile up in the queues).
    pub drain_each_step: bool,
}

pub fn int32(rate: u32, ch: u16) -> DeviceFormat {
    DeviceFormat {
        rate,
        channels: ch,
        container_bits: 32,
        valid_bits: 32,
        kind: SampleKind::Int,
    }
}

impl Rig {
    /// A 32-bit integer device (every bit of the mix kept) at `rate`, `ch` channels, called in
    /// periods of `period` frames.
    pub fn new(rate: u32, ch: u16, period: u32) -> Rig {
        Rig::with_format(int32(rate, ch), period)
    }

    pub fn with_format(fmt: DeviceFormat, period: u32) -> Rig {
        Rig::with_queues(fmt, period, 64, 4_096, 4_096)
    }

    /// With queues of these sizes (`render_link`).
    pub fn with_queues(
        fmt: DeviceFormat,
        period: u32,
        commands: usize,
        markers: usize,
        notes: usize,
    ) -> Rig {
        let (link, ends) = render_link(commands, markers, notes);
        let trim = Arc::new(RateTarget::default());
        let (r, status) = Renderer::new(RenderSetup {
            format: fmt.clone(),
            max_period: period,
            ends,
            rate: trim.clone(),
            clock: Arc::new(RealClock),
        });
        Rig {
            r,
            link,
            status,
            trim,
            fmt,
            period,
            at: 0,
            heard: Vec::new(),
            bytes: Vec::new(),
            notes: Vec::new(),
            markers: Vec::new(),
            returned: Vec::new(),
            feeds: Vec::new(),
            next_voice: 1,
            scratch: Vec::new(),
            keep_bytes: false,
            around: None,
            timings: None,
            drain_each_step: true,
        }
    }

    /// A new voice playing `song` from its frame `from` (decoded on demand): its id and tap.
    pub fn voice(&mut self, song: &Song, from: u64) -> (VoiceId, VoiceTap) {
        let id = VoiceId(self.next_voice);
        self.next_voice += 1;
        // 4 s of blocks, as a decode thread holds 2 to 3 s.
        let blocks = (song.rate as usize * 4).div_ceil(BLOCK_FRAMES);
        let (feed, tap) = voice_ring(id, blocks);
        let f = Feed::new(id, feed, song.clone(), from);
        self.feeds.push(f);
        (id, tap)
    }

    pub fn feed(&mut self, v: VoiceId) -> &mut Feed {
        self.feeds.iter_mut().find(|f| f.voice == v).unwrap()
    }

    pub fn send(&self, cmd: RenderCmd) {
        self.link
            .commands
            .try_send(cmd)
            .unwrap_or_else(|_| panic!("room for the command"));
    }

    /// Start `song` from its frame `content` at device frame `at`, fading in over `fade_in`.
    pub fn start(&mut self, song: &Song, content: u64, at: u64, fade_in: u64) -> VoiceId {
        let (voice, tap) = self.voice(song, content);
        self.send(RenderCmd::StartAt {
            voice,
            tap,
            at,
            content,
            rate: song.rate,
            fade_in: fade_in as u32,
        });
        voice
    }

    /// Render periods until the device frame `to` (whole periods).
    pub fn run_to(&mut self, to: u64) {
        while self.at < to {
            self.step();
        }
    }

    /// Render for `frames` more frames.
    pub fn run(&mut self, frames: u64) {
        let to = self.at + frames;
        self.run_to(to);
    }

    /// One period.
    pub fn step(&mut self) {
        for f in &mut self.feeds {
            f.fill();
        }
        let bpf = self.fmt.bytes_per_frame();
        let n = self.period as usize;
        self.scratch.resize(n * bpf, 0xAA);
        if let Some(h) = self.around {
            h(true);
        }
        let t0 = std::time::Instant::now();
        self.r.render(&mut self.scratch, self.period, self.at);
        let took = t0.elapsed().as_nanos() as u64;
        if let Some(h) = self.around {
            h(false);
        }
        if let Some(t) = self.timings.as_mut() {
            t.push(took);
        }
        decode(&self.scratch, &self.fmt, &mut self.heard);
        if self.keep_bytes {
            self.bytes.extend_from_slice(&self.scratch);
        }
        self.at += u64::from(self.period);
        if self.drain_each_step {
            self.drain();
        }
    }

    pub fn drain(&mut self) {
        while let Ok(n) = self.link.notes.try_recv() {
            self.notes.push(n);
        }
        while let Ok(m) = self.link.markers.try_recv() {
            self.markers.push(m);
        }
        while let Ok(r) = self.link.returned.try_recv() {
            self.returned.push(r);
        }
    }

    /// Channel `c` of what was heard.
    pub fn channel(&self, c: u16) -> Vec<i32> {
        let ch = usize::from(self.fmt.channels);
        self.heard
            .iter()
            .skip(usize::from(c))
            .step_by(ch)
            .copied()
            .collect()
    }

    pub fn channel_f64(&self, c: u16) -> Vec<f64> {
        self.channel(c)
            .iter()
            .map(|&v| f64::from(v) / FULL)
            .collect()
    }

    /// The spans the renderer declared (every shaped span, gain ramp and resampled span), plus
    /// `extra`.
    pub fn declared(&self, extra: &[std::ops::Range<u64>]) -> Declared {
        Declared::new(
            declared_spans(&self.notes, self.at)
                .into_iter()
                .chain(extra.iter().cloned())
                .map(|r| r.start as usize..r.end as usize)
                .collect(),
        )
    }

    pub fn notes_of(&self, f: impl Fn(&RenderNote) -> bool) -> Vec<RenderNote> {
        self.notes.iter().copied().filter(|n| f(n)).collect()
    }
}

/// The spans notes declare: `Faded` and `Gain` spans as they say, `Resampling` from each `on`
/// to the next `off` of that voice (or `end`), and pauses: from a `Silent` to the next sound
/// (`Started` or `Resumed` of any voice).
pub fn declared_spans(notes: &[RenderNote], end: u64) -> Vec<std::ops::Range<u64>> {
    let mut out = Vec::new();
    let mut open: Vec<(VoiceId, u64)> = Vec::new();
    let mut pause: Option<u64> = None;
    let mut gain: Option<u64> = None;
    for n in notes {
        match *n {
            RenderNote::Silent { at, .. } => pause = pause.or(Some(at)),
            RenderNote::Started { at, .. } | RenderNote::Resumed { at, .. } => {
                if let Some(p) = pause.take() {
                    out.push(p..at.max(p));
                }
            }
            _ => {}
        }
        match *n {
            RenderNote::Faded { from, to, .. } => out.push(from..to),
            // A gain under 1 changes every frame: declared from its ramp to the end of the ramp
            // back to 1.
            RenderNote::Gain { from, to, target } => match (gain, target == 1.0) {
                (Some(g), true) => {
                    out.push(g..to);
                    gain = None;
                }
                (None, false) => gain = Some(from),
                (None, true) => out.push(from..to),
                (Some(_), false) => {}
            },
            RenderNote::Resampling {
                voice,
                at,
                on: true,
            } => open.push((voice, at)),
            RenderNote::Resampling {
                voice,
                at,
                on: false,
            } => {
                if let Some(k) = open.iter().position(|(v, _)| *v == voice) {
                    let (_, from) = open.remove(k);
                    out.push(from..at);
                }
            }
            _ => {}
        }
    }
    out.extend(open.into_iter().map(|(_, from)| from..end));
    out.extend(pause.map(|p| p..end));
    out.extend(gain.map(|g| g..end));
    out
}

/// Device bytes back to left-justified samples.
pub fn decode(b: &[u8], fmt: &DeviceFormat, out: &mut Vec<i32>) {
    let size = usize::from(fmt.container_bits / 8);
    for s in b.chunks_exact(size) {
        out.push(match (fmt.kind, size) {
            (SampleKind::Float, _) => {
                let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                (f64::from(v) * FULL).clamp(-FULL, FULL - 1.0) as i32
            }
            (_, 2) => i32::from(i16::from_le_bytes([s[0], s[1]])) << 16,
            (_, 3) => i32::from_le_bytes([0, s[0], s[1], s[2]]),
            _ => i32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        });
    }
}

/// The raised-cosine curve from 0 to 1, `t` in 0..1.
pub fn raised(t: f64) -> f64 {
    (1.0 - (std::f64::consts::PI * t.clamp(0.0, 1.0)).cos()) / 2.0
}

/// What every detector says about one channel of a run: (what, flags), the flags being empty
/// when clean. `music` says which detectors apply: the music-shaped check, bit-exact against
/// `reference` (the source's samples of that channel), LPC calibrated on the reference, and dips
/// on music; the sine lock (at `sine_hz`) and bit-exact on a sine.
pub struct Verdict {
    pub detector: &'static str,
    pub flags: Vec<audio_check::Click>,
}

pub fn judge_music(
    heard: &[i32],
    reference: &[i32],
    rate: u32,
    declared: &Declared,
) -> Vec<Verdict> {
    let piece = audio_check::music::Piece { rate, channels: 2 };
    let (s1, s3) = piece.limits();
    let x24: Vec<i32> = heard.iter().map(|v| v >> 8).collect();
    // The music-shaped check knows nothing of declared spans: it judges the whole line, so a
    // fade must already be smooth enough for it.
    // Where the heard frames around a flag are the source's own, bit for bit, the flag is the
    // source's (the test music has a few exact zeros at steep crossings, which this check reads
    // as a gap); anything the renderer changed differs from the source and stays flagged.
    let own = |at: usize| {
        let from = at.saturating_sub(4);
        let to = (at + 4).min(heard.len());
        to - from == 8
            && !declared.overlaps(from..to)
            && reference.windows(8).any(|w| w == &heard[from..to])
    };
    let music: Vec<audio_check::Click> = audio_check::music::clicks(&x24, s1, s3)
        .into_iter()
        .filter(|c| !own(c.at))
        .collect();
    let exact = audio_check::exact::check(heard, &audio_check::exact::Samples(reference), declared);
    let xf: Vec<f64> = heard.iter().map(|&v| f64::from(v) / FULL).collect();
    let rf: Vec<f64> = reference.iter().map(|&v| f64::from(v) / FULL).collect();
    let lpc = audio_check::lpc::Lpc::calibrate(&rf, rate).check(&xf, declared);
    let dip = audio_check::dip::Dip::new(rate).check(&xf, declared);
    vec![
        Verdict {
            detector: "music-shaped",
            flags: music,
        },
        Verdict {
            detector: "bit-exact",
            flags: exact,
        },
        Verdict {
            detector: "LPC",
            flags: lpc,
        },
        Verdict {
            detector: "dip and gap",
            flags: dip,
        },
    ]
}

pub fn judge_sine(
    heard: &[i32],
    reference: &[i32],
    hz: f64,
    rate: u32,
    declared: &Declared,
) -> Vec<Verdict> {
    let xf: Vec<f64> = heard.iter().map(|&v| f64::from(v) / FULL).collect();
    let lock = audio_check::sine::SineLock::new(hz, rate, 0.5);
    let sine = audio_check::sine::check(&xf, &lock, declared);
    let exact = audio_check::exact::check(heard, &audio_check::exact::Samples(reference), declared);
    let dip = audio_check::dip::Dip::new(rate).check(&xf, declared);
    vec![
        Verdict {
            detector: "sine lock",
            flags: sine,
        },
        Verdict {
            detector: "bit-exact",
            flags: exact,
        },
        Verdict {
            detector: "dip and gap",
            flags: dip,
        },
    ]
}

/// Panics with every flag when any detector flagged anything.
pub fn assert_clean(what: &str, verdicts: &[Verdict]) {
    let bad: Vec<String> = verdicts
        .iter()
        .filter(|v| !v.flags.is_empty())
        .map(|v| format!("{}: {:?}", v.detector, &v.flags[..v.flags.len().min(5)]))
        .collect();
    assert!(bad.is_empty(), "{what}: {}", bad.join("; "));
}

/// Fill every empty block of `feed` from `song` (from its frame 0): a decode thread's work done
/// ahead, for runs where the test can't feed between periods.
pub fn fill_ring(feed: &VoiceFeed, voice: VoiceId, song: &Song) {
    let ch = usize::from(song.ch);
    let mut next = 0u64;
    while next < song.frames {
        let Ok(Some(mut b)) = feed.take_empty() else {
            return;
        };
        let n = (song.frames - next).min(BLOCK_FRAMES as u64) as usize;
        b.voice = voice;
        b.epoch = 0;
        b.first_frame = next;
        b.frames = n as u16;
        b.channels = song.ch as u8;
        b.flags = 0;
        for k in 0..n {
            for c in 0..ch {
                b.samples[k * ch + c] = (song.sample)(next + k as u64, c as u16);
            }
        }
        next += n as u64;
        if next >= song.frames {
            b.flags = END_OF_SOURCE;
        }
        feed.push(b).unwrap();
    }
}
