//! Voices and render commands (contract 5.3 of the Listen together design).
//!
//! A voice is one decoded stream (a song from a place). Its decode thread fills blocks and hands
//! them to the renderer on one queue; the renderer hands them back empty on another. Every block
//! is made when the voice is, so nothing is allocated or freed on the render thread.
//!
//! Neither side ever blocks on these queues: the renderer only tries (`pop`, `give_back`), and
//! the decode thread polls for empty blocks, sleeping a few ms when the ring is full. (A
//! blocked receiver would make the other side's send take a lock to wake it.)
//!
//! The control thread talks to the renderer the same way: render commands on a bounded queue,
//! markers and old sound chains coming back on others, the rate target in one atomic.
//!
//! Nothing in the app uses this yet.

use crate::dsp::Chain;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};

/// The most frames one block carries.
pub const BLOCK_FRAMES: usize = 1024;
/// The most channels a voice carries.
pub const MAX_CH: usize = 8;

/// Which voice (unique for the player's life; a new voice gets a new id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(pub u32);

/// The last block of the source: nothing comes after it.
pub const END_OF_SOURCE: u8 = 1;
/// The first block after a seek (its `first_frame` is the new place).
pub const FIRST_AFTER_SEEK: u8 = 2;

/// Up to 1024 frames of one voice, decoded.
pub struct Block {
    pub voice: VoiceId,
    /// A seek gives a new epoch; the renderer skips blocks of an older one.
    pub epoch: u32,
    /// The content frame (in the source's own rate) of the block's first frame.
    pub first_frame: u64,
    /// Frames in this block (up to `BLOCK_FRAMES`).
    pub frames: u16,
    /// Channels of the source.
    pub channels: u8,
    /// The block's loudness, dBFS (for the quiet-moment search).
    pub rms_db: i8,
    /// `END_OF_SOURCE`, `FIRST_AFTER_SEEK`.
    pub flags: u8,
    /// Interleaved, left-justified (as `decode::Source` gives them); only the first
    /// `frames * channels` count.
    pub samples: [i32; BLOCK_FRAMES * MAX_CH],
}

impl Block {
    fn empty(voice: VoiceId) -> Box<Block> {
        Box::new(Block {
            voice,
            epoch: 0,
            first_frame: 0,
            frames: 0,
            channels: 0,
            rms_db: i8::MIN,
            flags: 0,
            samples: [0; BLOCK_FRAMES * MAX_CH],
        })
    }

    /// The samples that count.
    pub fn data(&self) -> &[i32] {
        &self.samples[..usize::from(self.frames) * usize::from(self.channels)]
    }
}

/// The decode thread's side of a voice's ring.
pub struct VoiceFeed {
    pub voice: VoiceId,
    full: SyncSender<Box<Block>>,
    empty: Receiver<Box<Block>>,
}

/// The renderer's side of a voice's ring.
pub struct VoiceTap {
    pub voice: VoiceId,
    full: Receiver<Box<Block>>,
    empty: SyncSender<Box<Block>>,
}

/// A voice's ring of `blocks` blocks, all made now (`blocks` x 1024 frames is the most decoded
/// ahead: 2 s of 48 kHz is 94 blocks).
pub fn voice_ring(voice: VoiceId, blocks: usize) -> (VoiceFeed, VoiceTap) {
    let blocks = blocks.max(1);
    let (full_tx, full_rx) = sync_channel(blocks);
    let (empty_tx, empty_rx) = sync_channel(blocks);
    for _ in 0..blocks {
        empty_tx
            .try_send(Block::empty(voice))
            .expect("room for every block");
    }
    (
        VoiceFeed {
            voice,
            full: full_tx,
            empty: empty_rx,
        },
        VoiceTap {
            voice,
            full: full_rx,
            empty: empty_tx,
        },
    )
}

/// The renderer has let go of its side (the voice was dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dropped;

impl VoiceFeed {
    /// An empty block to fill, if one has come back (`Err`: the voice was dropped).
    pub fn take_empty(&self) -> Result<Option<Box<Block>>, Dropped> {
        match self.empty.try_recv() {
            Ok(b) => Ok(Some(b)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Dropped),
        }
    }

    /// Hand a filled block to the renderer. There is always room for a block taken from this
    /// ring.
    pub fn push(&self, b: Box<Block>) -> Result<(), Dropped> {
        match self.full.try_send(b) {
            Ok(()) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err(Dropped),
            Err(TrySendError::Full(_)) => unreachable!("more blocks than the ring was made with"),
        }
    }
}

impl VoiceTap {
    /// The next filled block, if one is there (never waits).
    pub fn pop(&self) -> Option<Box<Block>> {
        self.full.try_recv().ok()
    }

    /// Give a block back to be filled again (never waits). A block whose decode thread has
    /// gone is simply dropped (off the render thread: the control thread drops taps).
    pub fn give_back(&self, b: Box<Block>) {
        let _ = self.empty.try_send(b);
    }
}

/// How one place moves to another at a device frame (section 4.7, the splice table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Splice {
    /// Sample-exact: the next voice's first frame right after the last of this one.
    Gapless,
    /// Equal-power crossfade over `frames` device frames, starting at the switch.
    Crossfade { frames: u32 },
    /// The old fades out over `out_frames` ending at the switch frame; the new starts there
    /// from silence.
    FadeOutThenIn { out_frames: u32 },
    /// The old fades out over `frames` ending at the switch (a longer copy at a boundary); the
    /// new starts there.
    EndFade { frames: u32 },
}

/// What the control thread tells the renderer, each at an exact device frame. Frame counts
/// (`fade_in`, `fade`, the `Splice` lengths) are device frames.
pub enum RenderCmd {
    /// Start `voice` (from silence) so its content frame `content` is at device frame `at`. Its
    /// source runs at `rate` Hz. It fades in over `fade_in` frames (raised cosine); a start
    /// partway into the song (content above 0, or late) fades in over at least 15 ms whatever
    /// this says.
    StartAt {
        voice: VoiceId,
        tap: VoiceTap,
        at: u64,
        content: u64,
        rate: u32,
        fade_in: u32,
    },
    /// Move from `from` to `to` at device frame `at`, shaped as `how`; `to`'s source runs at
    /// `rate` Hz.
    SwitchAt {
        from: VoiceId,
        to: VoiceId,
        tap: VoiceTap,
        at: u64,
        to_content: u64,
        rate: u32,
        how: Splice,
    },
    /// Fade out over `fade` frames (at least 30 ms) starting at device frame `at`, and fall
    /// silent.
    StopAt { voice: VoiceId, at: u64, fade: u32 },
    /// Let go of a voice (its tap comes back on the return queue, to drop off this thread). A
    /// voice still sounding fades out over 30 ms first.
    Drop { voice: VoiceId },
    /// The master gain (volume, the sleep timer), ramped per sample over 20 ms.
    Gain { target: f32 },
    /// A voice's new sound tools (the old chain comes back on the return queue). A chain made
    /// with `Chain::gliding` takes over from the old one over its glide instead, keeping it
    /// inside, and what the old one itself glided from comes back.
    Chain { voice: VoiceId, chain: Box<Chain> },
}

/// What a shaped span did to a voice's sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeKind {
    /// Up from silence (raised cosine).
    In,
    /// Down to silence (raised cosine): a stop, a skip, a longer copy's end.
    Out,
    /// The incoming side of an equal-power crossfade.
    CrossIn,
    /// The outgoing side of an equal-power crossfade.
    CrossOut,
    /// Down to silence over the frames still in hand: the guard (a stream running dry, or a
    /// break in its blocks).
    Dry,
}

/// Why a command was not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Every voice slot is taken.
    NoSlot,
    /// No such voice (never started, or already let go).
    NoVoice,
    /// The source's rate is too far above the device's for the resampler (over 8.7 times).
    Rate,
}

/// What the renderer did, at which device frames: every shaped event, for the playback log, the
/// status, and the checks (the detectors' declared spans are the `Faded`, `Gain` and
/// `Resampling` spans). Small and `Copy`: nothing is allocated to send one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderNote {
    /// `voice` began sounding at device frame `at` with its content frame `content`, fading in
    /// over `fade` frames (0: the song's own start).
    Started {
        voice: VoiceId,
        at: u64,
        content: u64,
        fade: u32,
    },
    /// `to` followed `from` sample-exact: `to`'s first frame at device frame `at`.
    Joined { from: VoiceId, to: VoiceId, at: u64 },
    /// Device frames `from..to` of `voice` were shaped as `kind`.
    Faded {
        voice: VoiceId,
        from: u64,
        to: u64,
        kind: FadeKind,
    },
    /// The master gain ramped toward `target` over device frames `from..to`.
    Gain { from: u64, to: u64, target: f32 },
    /// From device frame `at`, `voice` is resampled (`on`: not its source bit for bit), or back
    /// on whole frames of its source at its own rate (`!on`).
    Resampling { voice: VoiceId, at: u64, on: bool },
    /// `voice` ran short of frames: the guard's fade begins at device frame `at`.
    Starved { voice: VoiceId, at: u64 },
    /// `voice` came back after running dry, fading in from device frame `at` at its content
    /// frame `content`.
    Resumed {
        voice: VoiceId,
        at: u64,
        content: u64,
    },
    /// `voice`'s source ended: its last frame was at device frame `at - 1`.
    Ended { voice: VoiceId, at: u64 },
    /// `voice` fell silent at device frame `at` (after a stop's or the guard's fade).
    Silent { voice: VoiceId, at: u64 },
    /// A command for `voice` came `by` frames after its frame; done from device frame `at`.
    Late { voice: VoiceId, at: u64, by: u64 },
    /// A command for `voice` was not followed.
    Refused { voice: VoiceId, why: Refusal },
    /// Notes or markers lost to a full queue since the last note that got through.
    Dropped { notes: u32, markers: u32 },
}

/// What the renderer hands back to the control thread to drop (nothing is freed on the render
/// thread).
pub enum Returned {
    Tap(VoiceTap),
    Chain(Box<Chain>),
}

/// Which content place one period carried: device frame `frame` played the voice's content
/// frame `content` (fractional when resampled), moving `ratio` content frames per device frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub frame: u64,
    pub voice: VoiceId,
    pub content: f64,
    pub ratio: f64,
}

/// The rate trim the sync tick asks for, in ppm (0 outside rooms), read by the renderer each
/// period. One atomic holding the `f64`'s bits.
#[derive(Debug, Default)]
pub struct RateTarget(AtomicU64);

impl RateTarget {
    pub fn set_ppm(&self, ppm: f64) {
        self.0.store(ppm.to_bits(), Ordering::Relaxed);
    }

    pub fn ppm(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// The queues between the control thread and the renderer, all bounded and made at start.
pub struct RenderLink {
    pub commands: SyncSender<RenderCmd>,
    pub markers: Receiver<Marker>,
    pub notes: Receiver<RenderNote>,
    pub returned: Receiver<Returned>,
}

/// The renderer's ends of a `RenderLink`.
pub struct RenderEnds {
    pub commands: Receiver<RenderCmd>,
    pub markers: SyncSender<Marker>,
    pub notes: SyncSender<RenderNote>,
    pub returned: SyncSender<Returned>,
}

/// The queues, with room for `commands` commands, `markers` markers (one per sounding voice per
/// period: a second of 10 ms periods is 100 for each) and `notes` notes.
pub fn render_link(commands: usize, markers: usize, notes: usize) -> (RenderLink, RenderEnds) {
    let (ct, cr) = sync_channel(commands.max(1));
    let (mt, mr) = sync_channel(markers.max(1));
    let (nt, nr) = sync_channel(notes.max(1));
    let (rt, rr) = sync_channel(commands.max(1) * 2);
    (
        RenderLink {
            commands: ct,
            markers: mr,
            notes: nr,
            returned: rr,
        },
        RenderEnds {
            commands: cr,
            markers: mt,
            notes: nt,
            returned: rt,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_go_round_the_ring_without_new_ones() {
        let (feed, tap) = voice_ring(VoiceId(1), 3);
        let mut filled = 0;
        while let Some(mut b) = feed.take_empty().unwrap() {
            b.frames = 10;
            b.channels = 2;
            b.first_frame = filled * 10;
            feed.push(b).unwrap();
            filled += 1;
        }
        assert_eq!(filled, 3, "as many blocks as the ring was made with");
        let b = tap.pop().unwrap();
        assert_eq!((b.first_frame, b.data().len()), (0, 20));
        assert!(feed.take_empty().unwrap().is_none());
        tap.give_back(b);
        assert!(feed.take_empty().unwrap().is_some());
        drop(tap);
        assert!(matches!(feed.take_empty(), Err(Dropped)));
    }

    #[test]
    fn the_rate_target_holds_any_ppm() {
        let r = RateTarget::default();
        assert_eq!(r.ppm(), 0.0);
        r.set_ppm(-43.25);
        assert_eq!(r.ppm(), -43.25);
    }
}
