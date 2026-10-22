//! Engine 2's programs (section 4.6 of the Listen together design): what the player plays
//! from. `OwnProgram` is your own queue; `RoomProgram` is a room, made at attach and dropped
//! whole at detach. They are never mixed: while a room plays, your own program is set aside
//! (its voices dropped, its queue and place kept) and comes back paused where it was.
//!
//! A program decides what plays and turns it into render commands through the `Deck`; it
//! learns what happened from the renderer's notes. Nothing here waits for audio.

use super::control::{
    APART, Column, Controller, DacRate, Deck, Lane, Msg, Place, Retire, SAME_ROOM, SETTLE_NS,
    Stopped, TICK_NS,
};
use super::status::View;
use super::{Cmd, Item, Signal, SyncStatus};
use crate::output::{self, Mode};
use crate::queue::Queue;
use crate::render::VoiceState;
use crate::sync::{SyncNote, SyncPlan};
use crate::voice::{Loudness, Phase};
use crate::voice_api::{RenderNote, Splice, VoiceId};
use serde::Serialize;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use together::align::{Alignment, CopyClass};
use together::anchor::{Anchor, EntryId, PrepId, Transition};
use together::clock_map::{ClockMap, ClockMapHandle};
use together::room_player::{RoomHandle, RoomPlayStatus, SubState, Suspension, SyncMode};

/// A seek's target not ready within this long: the song fades out and waits for it, ns.
const SEEK_WAIT_NS: u64 = 150_000_000;
/// The crossfade between songs is sent this long before it begins, ms.
const LINK_AHEAD_MS: u32 = 250;
/// A song's last second: its next is linked for a gapless join, ms.
const GAPLESS_AHEAD_MS: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Opening or decoding ahead; the renderer doesn't have it.
    Waiting,
    /// Given to the renderer (a start or a switch), not sounding yet.
    Given,
    Sounding,
    /// Fading out from this device frame, to come back at the place there.
    Stopping(u64),
}

/// One song of a program and its voice.
struct Song<T> {
    lane: Lane,
    item: T,
    state: State,
}

impl<T> Song<T> {
    fn vid(&self) -> VoiceId {
        self.lane.vid
    }
}

/// Let `song` go: what sounds fades out first.
fn retire<T>(deck: &mut Deck, song: Song<T>) {
    match song.state {
        State::Waiting | State::Given => deck.retire(song.lane, Retire::Now),
        State::Sounding => {
            let at = deck.soon();
            deck.stop_at(song.vid(), at, 30);
            deck.retire(song.lane, Retire::AfterFade);
        }
        State::Stopping(_) => deck.retire(song.lane, Retire::AfterFade),
    }
}

/// The place `song` was at on device frame `at` (its start when unknown), as a lane place.
fn place_at<T>(deck: &Deck, song: &Song<T>, at: u64) -> Place {
    match deck.content_at(song.vid(), at) {
        Some(c) => Place::Frame(c.max(0.0).floor() as u64),
        None => song.lane.place,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Gapless,
    Crossfade,
}

// --- Your own queue ------------------------------------------------------------------------------

pub(super) struct OwnProgram<T> {
    queue: Queue<T>,
    queue_changed: bool,
    from: String,
    stop_after_current: bool,
    /// Meant to play.
    playing: bool,
    error: String,
    /// The song at the queue's index.
    cur: Option<Song<T>>,
    /// The next one, decoded ahead (`next_index`: the queue's next when it was made).
    next: Option<Song<T>>,
    next_index: Option<usize>,
    /// A seek's target in the song playing, and since when it is asked.
    spare: Option<(Song<T>, u64)>,
    /// The next song is linked to the one playing.
    link: Option<Link>,
    /// Songs in a row that could not be opened.
    failures: usize,
}

impl<T: Item + Serialize> OwnProgram<T> {
    fn new() -> Self {
        OwnProgram {
            queue: Queue::default(),
            queue_changed: true,
            from: String::new(),
            stop_after_current: false,
            playing: false,
            error: String::new(),
            cur: None,
            next: None,
            next_index: None,
            spare: None,
            link: None,
            failures: 0,
        }
    }

    fn busy(&self) -> bool {
        self.playing
            || self.spare.is_some()
            || self.cur.as_ref().is_some_and(|c| c.state != State::Waiting)
    }

    fn wants_output(&self) -> bool {
        self.playing && self.cur.is_some()
    }

    fn command(&mut self, cmd: Cmd<T>, deck: &mut Deck) {
        match cmd {
            Cmd::Set {
                items,
                index,
                start_ms,
                play,
                from,
            } => {
                self.drop_all(deck);
                let shuffle = self.queue.shuffled();
                self.queue.set(items, index);
                if shuffle {
                    self.queue.set_shuffle(true);
                }
                self.from = from;
                self.queue_changed = true;
                self.error.clear();
                self.failures = 0;
                self.load_current(deck, Place::Us(start_ms * 1000));
                self.playing = play && self.cur.is_some();
            }
            Cmd::Play => self.set_playing(true, deck),
            Cmd::Pause => self.set_playing(false, deck),
            Cmd::Toggle => self.set_playing(!self.playing, deck),
            Cmd::Seek(ms) => self.seek(ms, deck),
            Cmd::Next => {
                let ahead = self.queue.peek_next();
                if self.queue.skip_next().is_some() {
                    self.go_to_current(deck, ahead);
                } else {
                    self.set_playing(false, deck);
                }
            }
            Cmd::Previous => {
                // Into a song: back to its start. At its start: the one before.
                if self.position_ms(deck) > 3000 || self.queue.skip_previous().is_none() {
                    self.seek(0, deck);
                } else {
                    self.go_to_current(deck, None);
                }
            }
            Cmd::Jump(i) => {
                if self.queue.index == Some(i) {
                    self.seek(0, deck);
                    self.set_playing(true, deck);
                } else {
                    let ahead = self.queue.peek_next();
                    if self.queue.jump(i) {
                        self.go_to_current(deck, ahead);
                        self.set_playing(true, deck);
                    }
                }
            }
            Cmd::Options { shuffle, repeat } => {
                self.queue.repeat = repeat;
                if shuffle != self.queue.shuffled() {
                    self.queue.set_shuffle(shuffle);
                    self.queue_changed = true;
                }
                self.drop_next(deck);
            }
            Cmd::Insert { items, next } => {
                let was_empty = self.queue.items().is_empty();
                let next_before = self.queue.peek_next();
                self.queue.insert(items, next);
                self.queue_changed = true;
                // The song decoded ahead is kept when it is still the next one.
                if next || self.queue.peek_next() != next_before {
                    self.drop_next(deck);
                }
                if was_empty {
                    self.load_current(deck, Place::Frame(0));
                    self.set_playing(true, deck);
                }
            }
            Cmd::Remove(i) => {
                let current = self.queue.index == Some(i);
                self.queue.remove(i);
                self.queue_changed = true;
                if current {
                    self.go_to_current(deck, None);
                    if self.cur.is_none() {
                        self.playing = false;
                    }
                } else {
                    self.drop_next(deck);
                }
            }
            Cmd::Move(a, b) => {
                self.queue.move_item(a, b);
                self.queue_changed = true;
                self.drop_next(deck);
            }
            Cmd::Follow { items, index } => {
                self.queue.follow(items, index);
                self.queue_changed = true;
                self.drop_next(deck);
            }
            Cmd::RestoreOrder(positions) => {
                self.queue.restore_order(&positions);
                self.queue_changed = true;
            }
            Cmd::StopAfterCurrent(on) => {
                self.stop_after_current = on;
                if on {
                    self.drop_next(deck);
                }
            }
            Cmd::Refresh => self.queue_changed = true,
            // The deck's and the control thread's.
            Cmd::Volume(_)
            | Cmd::Sound(_)
            | Cmd::SleepIn(_)
            | Cmd::Spectrum(_)
            | Cmd::Output(_)
            | Cmd::OutputDelays(_)
            | Cmd::Sync(_)
            | Cmd::Quit => {}
        }
    }

    fn set_playing(&mut self, on: bool, deck: &mut Deck) {
        if on {
            if self.cur.is_none() && self.queue.current().is_some() {
                self.load_current(deck, Place::Frame(0));
            }
            self.playing = self.cur.is_some();
        } else {
            self.playing = false;
            self.hush(deck);
        }
    }

    /// The current song's voice, from `place`; nothing decoded ahead any more.
    fn load_current(&mut self, deck: &mut Deck, place: Place) {
        if let Some(old) = self.cur.take() {
            retire(deck, old);
        }
        self.drop_next(deck);
        self.drop_spare(deck);
        self.cur = self.queue.current().cloned().map(|item| Song {
            lane: deck.lane(&item, place),
            item,
            state: State::Waiting,
        });
    }

    fn drop_next(&mut self, deck: &mut Deck) {
        if let Some(n) = self.next.take() {
            deck.retire(n.lane, Retire::Now);
        }
        self.next_index = None;
        self.link = None;
    }

    fn drop_spare(&mut self, deck: &mut Deck) {
        if let Some((s, _)) = self.spare.take() {
            retire(deck, s);
        }
    }

    /// Everything goes (a new queue): what sounds fades out.
    fn drop_all(&mut self, deck: &mut Deck) {
        if let Some(c) = self.cur.take() {
            retire(deck, c);
        }
        self.drop_next(deck);
        self.drop_spare(deck);
    }

    /// The current song as a new voice at `place`, waiting; the old one goes.
    fn replace_cur(&mut self, deck: &mut Deck, place: Place) {
        let Some(old) = self.cur.take() else {
            return;
        };
        let item = old.item.clone();
        // The new voice first: a stream's download is kept while it opens.
        let mut lane = deck.lane(&item, place);
        lane.rate_hint = old.lane.rate().unwrap_or(0);
        retire(deck, old);
        self.cur = Some(Song {
            lane,
            item,
            state: State::Waiting,
        });
    }

    /// What sounds fades out and comes back, not given, at its place (a pause, an output
    /// switch, a room taking the player, quitting).
    fn hush(&mut self, deck: &mut Deck) {
        self.drop_next(deck);
        // A seek asked: that is the place now.
        if let Some((s, _)) = self.spare.take() {
            let place = s.lane.place;
            retire(deck, s);
            self.replace_cur(deck, place);
        }
        let Some(c) = self.cur.as_mut() else {
            return;
        };
        match c.state {
            State::Sounding => {
                let at = deck.soon();
                deck.stop_at(c.vid(), at, 30);
                c.state = State::Stopping(at);
            }
            // Not sounding yet: let go, and wait at the same place.
            State::Given => {
                let place = c.lane.place;
                self.replace_cur(deck, place);
            }
            State::Waiting | State::Stopping(_) => {}
        }
    }

    fn seek(&mut self, ms: u64, deck: &mut Deck) {
        let Some(c) = self.cur.as_ref() else {
            return;
        };
        let place = match c.lane.status().format {
            Some(f) => Place::Frame(ms * u64::from(f.rate.max(1)) / 1000),
            None => Place::Us(ms * 1000),
        };
        let sounding = self.playing && c.state == State::Sounding;
        let item = c.item.clone();
        let c_rate = c.lane.rate().unwrap_or(0);
        // The next song was linked to the voice that goes.
        self.drop_next(deck);
        self.drop_spare(deck);
        if sounding {
            let mut lane = deck.lane(&item, place);
            lane.rate_hint = c_rate;
            self.spare = Some((
                Song {
                    lane,
                    item,
                    state: State::Waiting,
                },
                deck.now_ns(),
            ));
        } else {
            self.replace_cur(deck, place);
        }
    }

    /// The queue moved by hand to another song: it plays (the one decoded ahead when it is the
    /// one), the old one fading out over 30 ms as it starts.
    fn go_to_current(&mut self, deck: &mut Deck, ahead: Option<usize>) {
        self.drop_spare(deck);
        let reuse = ahead.is_some()
            && ahead == self.queue.index
            && self.next_index == ahead
            && self.link.is_none()
            && self
                .next
                .as_ref()
                .is_some_and(|n| n.lane.failed().is_none());
        let new = if reuse {
            self.next_index = None;
            self.next.take()
        } else {
            self.drop_next(deck);
            self.queue.current().cloned().map(|item| Song {
                lane: deck.lane(&item, Place::Frame(0)),
                item,
                state: State::Waiting,
            })
        };
        self.link = None;
        let Some(mut new) = new else {
            if let Some(old) = self.cur.take() {
                retire(deck, old);
            }
            return;
        };
        if let Some(old) = self.cur.take() {
            let fits = new
                .lane
                .status()
                .format
                .is_some_and(|f| deck.fits(&f) && deck.generation().is_some());
            if old.state == State::Sounding && self.playing && fits && new.lane.ready() {
                let at = deck.soon() + deck.ms(30);
                let out = deck.ms(30) as u32;
                if deck.switch(
                    old.vid(),
                    &mut new.lane,
                    at,
                    Splice::FadeOutThenIn { out_frames: out },
                ) {
                    new.state = State::Given;
                    deck.retire(old.lane, Retire::AfterFade);
                } else {
                    retire(deck, old);
                }
            } else {
                retire(deck, old);
            }
        }
        self.cur = Some(new);
    }

    fn position_ms(&self, deck: &Deck) -> u64 {
        let place = |s: &Song<T>| {
            let rate = u64::from(s.lane.rate().unwrap_or(44_100).max(1));
            match s.lane.place {
                Place::Frame(f) => f * 1000 / rate,
                Place::Us(us) => us / 1000,
            }
        };
        if let Some((s, _)) = &self.spare {
            return place(s);
        }
        let Some(c) = self.cur.as_ref() else {
            return 0;
        };
        // Where it reaches your ear: through the output's extra delay (lyrics follow this).
        if matches!(c.state, State::Sounding | State::Stopping(_))
            && let (Some(content), Some(f)) =
                (deck.heard_content_by_ear(c.vid()), c.lane.status().format)
        {
            return (content.max(0.0) as u64) * 1000 / u64::from(f.rate.max(1));
        }
        place(c)
    }

    fn step(&mut self, deck: &mut Deck) {
        if let Some(e) = deck.take_gave_up() {
            self.error = e;
            self.set_playing(false, deck);
            return;
        }
        if !self.playing {
            return;
        }
        if self.cur.is_none() && self.queue.current().is_some() {
            self.load_current(deck, Place::Frame(0));
        }
        self.start_current(deck);
        self.seek_step(deck);
        self.prefetch(deck);
        self.link_next(deck);
    }

    /// The current song, ready, starts from silence (after any fade under way).
    fn start_current(&mut self, deck: &mut Deck) {
        let Some(c) = self.cur.as_mut() else {
            return;
        };
        if c.state != State::Waiting || !c.lane.ready() {
            return;
        }
        if let Some(e) = c.lane.failed() {
            self.error = format!("{}: {e}", c.lane.label);
            self.failures += 1;
            let ahead = self.queue.peek_next();
            if self.failures >= self.queue.items().len() || self.queue.skip_next().is_none() {
                if let Some(old) = self.cur.take() {
                    retire(deck, old);
                }
                self.playing = false;
            } else {
                self.go_to_current(deck, ahead);
            }
            return;
        }
        let Some(f) = c.lane.status().format else {
            return;
        };
        if !deck.ready_for(&f) {
            return;
        }
        let start = c.lane.start().unwrap_or(0);
        let fade = if start > 0 { deck.ms(15) as u32 } else { 0 };
        let at = deck.soon_quiet();
        if deck.start(&mut c.lane, at, fade) {
            c.state = State::Given;
            self.failures = 0;
        }
    }

    /// A seek's target, ready: a 40 ms crossfade to it; not ready in time: the song fades out
    /// and the target starts from silence once it is.
    fn seek_step(&mut self, deck: &mut Deck) {
        let Some((s, since)) = self.spare.as_ref() else {
            return;
        };
        let cur_sounding = self
            .cur
            .as_ref()
            .is_some_and(|c| c.state == State::Sounding);
        let late = deck.now_ns().saturating_sub(*since) > SEEK_WAIT_NS;
        let fits = s.lane.status().format.is_some_and(|f| deck.fits(&f));
        let ready = s.lane.ready() && s.lane.failed().is_none();
        if cur_sounding && !(ready && fits) && !late && s.lane.failed().is_none() {
            return;
        }
        let (mut s, _) = self.spare.take().expect("a spare");
        let Some(old) = self.cur.take() else {
            self.cur = Some(s);
            return;
        };
        if cur_sounding && ready && fits {
            let at = deck.soon();
            let frames = deck.ms(40) as u32;
            if deck.switch(old.vid(), &mut s.lane, at, Splice::Crossfade { frames }) {
                s.state = State::Given;
                deck.retire(old.lane, Retire::AfterFade);
            } else {
                retire(deck, old);
            }
        } else {
            retire(deck, old);
        }
        self.cur = Some(s);
    }

    /// The next song decoded ahead, once this one is given.
    fn prefetch(&mut self, deck: &mut Deck) {
        if self.next.is_some() || self.stop_after_current || self.spare.is_some() {
            return;
        }
        if !self
            .cur
            .as_ref()
            .is_some_and(|c| matches!(c.state, State::Given | State::Sounding))
        {
            return;
        }
        let Some(n) = self.queue.peek_next() else {
            return;
        };
        let Some(item) = self.queue.items().get(n).cloned() else {
            return;
        };
        self.next = Some(Song {
            lane: deck.lane(&item, Place::Frame(0)),
            item,
            state: State::Waiting,
        });
        self.next_index = Some(n);
    }

    /// Near the song's end, the next one linked to it: gapless, or crossfaded over the
    /// setting's length (songs of different albums).
    fn link_next(&mut self, deck: &mut Deck) {
        if self.link.is_some() || self.stop_after_current || self.spare.is_some() {
            return;
        }
        let (Some(c), Some(n)) = (self.cur.as_ref(), self.next.as_ref()) else {
            return;
        };
        if c.state != State::Sounding
            || n.state != State::Waiting
            || !n.lane.ready()
            || n.lane.failed().is_some()
        {
            return;
        }
        let Some(nf) = n.lane.status().format else {
            return;
        };
        if !deck.fits(&nf) {
            return;
        }
        let s = c.lane.status();
        let Some(f) = s.format.as_ref() else {
            return;
        };
        let Some((content, ratio)) = deck.rendered_place(c.vid()) else {
            return;
        };
        let dev = f64::from(deck.rate().max(1));
        let ratio = if ratio > 0.0 {
            ratio
        } else {
            f64::from(f.rate) / dev
        };
        let Some(total) = s
            .frames
            .or((s.phase == Phase::Ended).then_some(s.decoded_to))
        else {
            return;
        };
        let now = deck.frames();
        // Device frames until this song's end.
        let left = ((total as f64 - content).max(0.0) / ratio).floor() as u64;
        let end = now + left;
        let vid = c.vid();
        let fade_s = f64::from(deck.sound.crossfade_s.max(0.0));
        let same_album = c.lane.album.is_some() && c.lane.album == n.lane.album;
        if fade_s > 0.0 && !same_album {
            let fade = (fade_s * dev) as u64;
            let from = end.saturating_sub(fade);
            if from > now + deck.ms(LINK_AHEAD_MS) {
                return;
            }
            let at = from.max(deck.soon());
            let frames = end.saturating_sub(at);
            if frames >= deck.ms(30) {
                let n = self.next.as_mut().expect("a next song");
                let how = Splice::Crossfade {
                    frames: frames as u32,
                };
                if deck.switch(vid, &mut n.lane, at, how) {
                    n.state = State::Given;
                    self.link = Some(Link::Crossfade);
                }
                return;
            }
        }
        if left > deck.ms(GAPLESS_AHEAD_MS) && s.phase != Phase::Ended {
            return;
        }
        let n = self.next.as_mut().expect("a next song");
        if deck.switch(vid, &mut n.lane, end, Splice::Gapless) {
            n.state = State::Given;
            self.link = Some(Link::Gapless);
        }
    }

    /// The next song took over (joined, or its crossfade began): the queue moves on.
    fn moved_on(&mut self, deck: &mut Deck, old_done: bool) {
        let (Some(old), Some(mut n)) = (self.cur.take(), self.next.take()) else {
            return;
        };
        let how = if old_done {
            Retire::Now
        } else {
            Retire::AfterFade
        };
        deck.retire(old.lane, how);
        n.state = State::Sounding;
        self.cur = Some(n);
        self.next_index = None;
        self.link = None;
        let _ = self.queue.advance();
    }

    fn note(&mut self, n: &RenderNote, deck: &mut Deck) {
        let is = |s: &Option<Song<T>>, v: VoiceId| s.as_ref().is_some_and(|s| s.vid() == v);
        match *n {
            RenderNote::Started { voice, .. } => {
                if is(&self.cur, voice) {
                    if let Some(c) = self.cur.as_mut()
                        && c.state == State::Given
                    {
                        c.state = State::Sounding;
                    }
                } else if is(&self.next, voice) && self.link.is_some() {
                    // A crossfade began (the old song fades on), or a gapless follower that
                    // couldn't continue the window started on the frame after.
                    let old_done = self.link == Some(Link::Gapless);
                    self.moved_on(deck, old_done);
                }
            }
            RenderNote::Joined { from, to, .. } => {
                if is(&self.cur, from) && is(&self.next, to) {
                    self.moved_on(deck, true);
                }
            }
            RenderNote::Ended { voice, .. } => {
                if is(&self.cur, voice) && self.link.is_none() {
                    self.end_of_song(deck);
                }
            }
            RenderNote::Silent { voice, .. } => {
                if let Some(c) = self.cur.as_ref()
                    && c.vid() == voice
                    && let State::Stopping(at) = c.state
                {
                    let place = place_at(deck, c, at);
                    let item = c.item.clone();
                    let mut lane = deck.lane(&item, place);
                    lane.rate_hint = c.lane.rate().unwrap_or(0);
                    if let Some(old) = self.cur.take() {
                        deck.retire(old.lane, Retire::Now);
                    }
                    self.cur = Some(Song {
                        lane,
                        item,
                        state: State::Waiting,
                    });
                }
            }
            RenderNote::Refused { voice, .. } => {
                if is(&self.cur, voice) {
                    if let Some(c) = self.cur.as_mut() {
                        // Its tap came back: as if it was never given.
                        c.state = State::Given;
                    }
                    let place = self.cur.as_ref().map_or(Place::Frame(0), |c| c.lane.place);
                    self.replace_cur(deck, place);
                } else if is(&self.next, voice) {
                    self.drop_next(deck);
                } else if self.spare.as_ref().is_some_and(|(s, _)| s.vid() == voice) {
                    self.drop_spare(deck);
                }
            }
            _ => {}
        }
    }

    /// The song playing ended with nothing linked to it.
    fn end_of_song(&mut self, deck: &mut Deck) {
        let Some(old) = self.cur.take() else {
            return;
        };
        if let Some(e) = old.lane.error() {
            self.error = format!("{}: {e}", old.lane.label);
        }
        deck.retire(old.lane, Retire::Now);
        if self.stop_after_current {
            // Stop here, with the next song ready.
            self.stop_after_current = false;
            let _ = self.queue.advance();
            self.playing = false;
            self.drop_next(deck);
            self.reload_when_stopped(deck);
            return;
        }
        let ahead = self.queue.peek_next();
        if self.queue.advance().is_none() {
            // The end of the queue.
            self.queue.index = self.queue.index.or(Some(0));
            self.playing = false;
            self.drop_next(deck);
            self.reload_when_stopped(deck);
            return;
        }
        if ahead.is_some() && self.next_index == ahead && self.next.is_some() {
            self.cur = self.next.take();
            self.next_index = None;
        } else {
            self.load_current(deck, Place::Frame(0));
        }
    }

    /// Stopped at an end: the current song waits at its start, if its item says so.
    fn reload_when_stopped(&mut self, deck: &mut Deck) {
        if self
            .queue
            .current()
            .is_some_and(|item| item.reopen_when_stopped())
        {
            self.load_current(deck, Place::Frame(0));
        } else {
            self.cur = None;
        }
    }

    /// The renderer start `ev.generation` is over: its voices are gone. The current song
    /// comes back at its place (where it was heard, when the output was lost).
    fn stopped(&mut self, ev: &Stopped, deck: &mut Deck) {
        let ended = Some(ev.generation);
        if self.next.as_ref().is_some_and(|n| n.lane.given == ended) {
            self.drop_next(deck);
        }
        if self
            .spare
            .as_ref()
            .is_some_and(|(s, _)| s.lane.given == ended)
        {
            self.drop_spare(deck);
        }
        let Some(c) = self.cur.as_ref() else {
            return;
        };
        if c.lane.given != ended {
            return;
        }
        let place = match (ev.lost_at, c.state) {
            (Some(f), _) => place_at(deck, c, f),
            (None, State::Stopping(at)) => place_at(deck, c, at),
            (None, _) => match deck.content_at(c.vid(), u64::MAX) {
                Some(p) => Place::Frame(p.max(0.0) as u64),
                None => c.lane.place,
            },
        };
        let item = c.item.clone();
        let mut lane = deck.lane(&item, place);
        lane.rate_hint = c.lane.rate().unwrap_or(0);
        if let Some(old) = self.cur.take() {
            deck.retire(old.lane, Retire::Now);
        }
        self.cur = Some(Song {
            lane,
            item,
            state: State::Waiting,
        });
    }

    fn view(&mut self, deck: &Deck, v: &mut View<T>) {
        if self.queue_changed {
            v.queue = Some(self.queue.items().to_vec());
            v.unshuffled = self.queue.unshuffled();
            self.queue_changed = false;
        }
        v.index = self.queue.index;
        v.playing = self.playing;
        v.shuffle = self.queue.shuffled();
        v.repeat = self.queue.repeat;
        v.from = self.from.clone();
        v.error = self.error.clone();
        v.stop_after_current = self.stop_after_current;
        v.position_ms = self.position_ms(deck);
        let st = self.cur.as_ref().map(|c| c.lane.status());
        v.duration_ms = st.as_ref().and_then(|s| s.duration_ms).unwrap_or(0);
        let (mut opening, mut stalled) = (false, false);
        if let (Some(c), Some(s)) = (self.cur.as_ref(), st.as_ref()) {
            let waiting = c.state == State::Waiting && !c.lane.ready();
            let partway =
                c.lane.start().unwrap_or(0) > 0 || matches!(c.lane.place, Place::Us(us) if us > 0);
            opening = waiting && !partway;
            stalled = (waiting && partway)
                || deck.voice_state(c.vid()) == Some(VoiceState::Starved)
                || (c.state == State::Sounding
                    && s.phase == Phase::WaitingForBytes
                    && s.in_ring == 0);
        }
        if let Some((s, since)) = self.spare.as_ref() {
            stalled |= !s.lane.ready() && deck.now_ns().saturating_sub(*since) > SEEK_WAIT_NS;
        }
        v.buffering = self.playing && (opening || stalled);
        v.stalled = self.playing && stalled;
        v.signal = self.signal(deck);
    }

    /// The signal path: the song's format and the output's, and what changes the samples.
    fn signal(&self, deck: &Deck) -> Option<Signal> {
        let c = self.cur.as_ref()?;
        let s = c.lane.status();
        let src = s.format.clone()?;
        let side = deck.output_signal()?;
        let (mode, out) = (side.mode, side.format.clone());
        let (mut steps, active) = deck.steps(&c.lane.gain, s.loudness_db, out.rate, src.channels);
        let fading = self.link == Some(Link::Crossfade);
        if fading {
            steps.push(format!(
                "Crossfading into the next song ({} s)",
                deck.sound.crossfade_s
            ));
        }
        if let Some(why) = &side.why_shared {
            steps.push(format!("Not exclusive: {why}"));
        }
        if mode == Mode::Shared {
            // D2: the song reaches Windows at its mix format (the resampler's step says so), and
            // Windows mixes it with other apps' sound and its own effects.
            steps.push("Mixed by Windows (shared mode)".into());
        }
        if out.rate != src.rate {
            steps.push(format!("Resampled to {} kHz", out.rate as f32 / 1000.0));
        }
        if deck.volume < 0.999_999 {
            steps.push(format!("Volume at {}%", (deck.volume * 100.0).round()));
        }
        if !src.lossless {
            steps.push(format!("Decoded from {}", src.codec));
        }
        // What the device itself does (Bluetooth's codec, a virtual device's software).
        let device_steps = side
            .kind
            .map(|k| crate::devices::device_steps(k, side.hands_free))
            .unwrap_or_default();
        let device_keeps = device_steps.is_empty();
        steps.extend(device_steps);
        let rate = src.rate.max(1) as f32;
        Some(Signal {
            device: side.name,
            mode,
            file: s.info.clone(),
            buffer_ms: (s.in_ring as f32 * 1000.0 / rate) as u32,
            latency_ms: side.latency_ms,
            bit_perfect: !active
                && !fading
                && device_keeps
                && output::bit_perfect(&src, &out, mode, deck.volume),
            source: src,
            output: out,
            steps,
            endpoint: side.endpoint,
            kind: side.kind,
            note: side.note,
            delay_ms: side.delay_ms,
            suggested_delay_ms: side.suggested_delay_ms,
            follows_default: side.follows_default,
        })
    }
}

// --- A room ----------------------------------------------------------------------------------------

/// What the room handle sends to the control thread.
pub(crate) enum RoomMsg<T> {
    Attach {
        clock: ClockMapHandle,
        mode: SyncMode,
        status: Arc<Mutex<RoomPlayStatus>>,
        /// The host's room made from its own music in place: what sounds goes on as this entry.
        adopt: Option<EntryId>,
    },
    Prepare {
        prep: PrepId,
        entry: EntryId,
        item: T,
        room_content_us: i64,
        align: Alignment,
    },
    ArmNext {
        entry: EntryId,
        item: T,
        boundary_host_us: i64,
        transition: Transition,
        align: Alignment,
    },
    Disarm,
    Commit(Anchor),
    Hold(Anchor),
    SetMode(SyncMode),
    SetExtraLatency(i32),
    StepOut,
    Rejoin,
    Detach,
}

/// The room's hold on engine 2: every call a message to the control thread; its status read
/// from what the room program publishes.
struct RoomLink<T> {
    tx: Sender<Msg<T>>,
    status: Arc<Mutex<RoomPlayStatus>>,
}

pub(super) fn attach<T: Send + 'static>(
    tx: Sender<Msg<T>>,
    clock: ClockMapHandle,
    mode: SyncMode,
) -> Box<dyn RoomHandle<T>> {
    link(tx, clock, mode, None)
}

/// The host's room from its own music in place (A1).
pub(super) fn adopt<T: Send + 'static>(
    tx: Sender<Msg<T>>,
    clock: ClockMapHandle,
    mode: SyncMode,
    entry: EntryId,
) -> Box<dyn RoomHandle<T>> {
    link(tx, clock, mode, Some(entry))
}

fn link<T: Send + 'static>(
    tx: Sender<Msg<T>>,
    clock: ClockMapHandle,
    mode: SyncMode,
    adopt: Option<EntryId>,
) -> Box<dyn RoomHandle<T>> {
    let status = Arc::new(Mutex::new(RoomPlayStatus::default()));
    let _ = tx.send(Msg::Room(RoomMsg::Attach {
        clock,
        mode,
        status: status.clone(),
        adopt,
    }));
    Box::new(RoomLink { tx, status })
}

impl<T: Send> RoomHandle<T> for RoomLink<T> {
    fn prepare(
        &self,
        prep: PrepId,
        entry: EntryId,
        item: T,
        room_content_us: i64,
        align: Alignment,
    ) {
        let _ = self.tx.send(Msg::Room(RoomMsg::Prepare {
            prep,
            entry,
            item,
            room_content_us,
            align,
        }));
    }
    fn arm_next(
        &self,
        entry: EntryId,
        item: T,
        boundary_host_us: i64,
        transition: Transition,
        align: Alignment,
    ) {
        let _ = self.tx.send(Msg::Room(RoomMsg::ArmNext {
            entry,
            item,
            boundary_host_us,
            transition,
            align,
        }));
    }
    fn disarm(&self) {
        let _ = self.tx.send(Msg::Room(RoomMsg::Disarm));
    }
    fn commit(&self, anchor: Anchor) {
        let _ = self.tx.send(Msg::Room(RoomMsg::Commit(anchor)));
    }
    fn hold(&self, anchor: Anchor) {
        let _ = self.tx.send(Msg::Room(RoomMsg::Hold(anchor)));
    }
    fn set_mode(&self, mode: SyncMode) {
        let _ = self.tx.send(Msg::Room(RoomMsg::SetMode(mode)));
    }
    fn set_extra_latency(&self, us: i32) {
        let _ = self.tx.send(Msg::Room(RoomMsg::SetExtraLatency(us)));
    }
    fn step_out(&self) {
        let _ = self.tx.send(Msg::Room(RoomMsg::StepOut));
    }
    fn rejoin(&self) {
        let _ = self.tx.send(Msg::Room(RoomMsg::Rejoin));
    }
    fn status(&self) -> RoomPlayStatus {
        *self.status.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn detach(self: Box<Self>) {
        let _ = self.tx.send(Msg::Room(RoomMsg::Detach));
    }
}

/// Engine 1's hold for a room: engine 1 follows rooms through `Cmd::Sync`, so this does nothing
/// and says it can't play.
pub(super) struct Inert;

impl<T> RoomHandle<T> for Inert {
    fn prepare(&self, _: PrepId, _: EntryId, _: T, _: i64, _: Alignment) {}
    fn arm_next(&self, _: EntryId, _: T, _: i64, _: Transition, _: Alignment) {}
    fn disarm(&self) {}
    fn commit(&self, _: Anchor) {}
    fn hold(&self, _: Anchor) {}
    fn set_mode(&self, _: SyncMode) {}
    fn set_extra_latency(&self, _: i32) {}
    fn step_out(&self) {}
    fn rejoin(&self) {}
    fn status(&self) -> RoomPlayStatus {
        RoomPlayStatus {
            sub_state: SubState::CannotPlay,
            ..RoomPlayStatus::default()
        }
    }
    fn detach(self: Box<Self>) {}
}

/// A prepared song is ready (4.4) once this much is decoded at its place, s: a file, a stream.
const READY_FILE_S: f64 = 2.0;
const READY_STREAM_S: f64 = 3.0;
/// And a stream has the bytes for this much past its place (or the whole download), s.
const READY_BYTES_S: f64 = 10.0;
/// A re-entry after a stall waits for this much decoded past the room's place, s.
const REFILL_S: f64 = 5.0;
/// Re-entries decode this far ahead, so the refill fits.
const ENTRY_DEPTH: Duration = Duration::from_secs(8);
/// A re-entry opens where the room will be this long on (doubled each time the room passes it).
const ENTRY_LEAD_US: i64 = 1_000_000;
const ENTRY_LEAD_MAX_US: i64 = 8_000_000;
/// Fades: an entrance mid-song, a start partway, an output switch, ms.
const ENTRANCE_MS: u32 = 300;
const PARTWAY_MS: u32 = 15;
/// The armed next is linked from this long before its boundary, ms.
const ARM_AHEAD_MS: u32 = 1_000;
/// Copies whose ends are within this of the boundary join sample-exactly, ms.
const EXACT_JOIN_MS: u32 = 5;
/// A longer copy fades out over this, ending at the boundary, ms.
const END_FADE_MS: u32 = 150;
/// A seek or realign crossfade, a skip's fade-out, ms.
const CROSSFADE_MS: u32 = 40;
const SKIP_FADE_MS: u32 = 30;
/// A realign never within this of a song's end or the armed boundary, s.
const REALIGN_CLEAR_S: f64 = 3.0;
/// The quiet moment: this long under `QUIET_DB`, looked for from 0.5 s to 2 s ahead.
const QUIET_MS: u32 = 50;
const QUIET_DB: i8 = -40;
const QUIET_FROM_S: f64 = 0.5;
const QUIET_TO_S: f64 = 2.0;
/// Every device: over this for `CANT_HOLD_NS`, the output can't hold (it fades out, 1 s).
const CANT_HOLD_MS: f64 = 5.0;
const CANT_HOLD_NS: u64 = 30_000_000_000;

/// One of the room's songs, as this person plays it.
struct RoomSong<T> {
    entry: EntryId,
    /// My copy's offset from the reference (my content = room content + offset), µs.
    offset_us: i64,
    class: CopyClass,
    song: Song<T>,
}

impl<T: Item> RoomSong<T> {
    fn vid(&self) -> VoiceId {
        self.song.vid()
    }

    fn rate(&self) -> Option<u32> {
        self.song.lane.rate()
    }
}

/// What the spare voice is for.
enum Use {
    /// A prepare: the room's content it was asked at.
    Prepare { prep: PrepId, content_us: i64 },
    /// A realign: the device frame of the quiet moment it crossfades in at.
    Realign { at: u64 },
    /// A way back in at the room's place (after a stall, a lost or switched output, stepping
    /// out, a missed boundary): its fade-in, what it must hold past the room's place (s), and
    /// how far ahead it opened (µs).
    Entry {
        fade_ms: u32,
        need_s: f64,
        lead_us: i64,
    },
}

struct Spare<T> {
    rs: RoomSong<T>,
    what: Use,
}

/// The next entry, armed for its boundary.
struct Armed<T> {
    rs: RoomSong<T>,
    boundary_us: i64,
    transition: Transition,
    /// Its switch is with the renderer.
    given: bool,
}

/// Why the song playing is fading out (what follows its silence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fading {
    /// A hold, stepping out, the room let go: nothing.
    Out,
    /// An output switch: back in at the room's place on the new output.
    Switch,
    /// Every device mode couldn't hold: nothing, until the mode is apart.
    CantHold,
}

/// A room's program (section 4.6, its WP5 notes in 9.2): the room's voices against the clock
/// map, the anchor and the armed next, the sync tick, suspensions and modes.
pub(super) struct RoomProgram<T> {
    clock: ClockMapHandle,
    mode: SyncMode,
    extra_us: i32,
    status: Arc<Mutex<RoomPlayStatus>>,
    playing: Option<RoomSong<T>>,
    fading: Option<Fading>,
    armed: Option<Armed<T>>,
    spare: Option<Spare<T>>,
    /// The newest anchor, and a commit waiting for its prepared song.
    anchor: Option<Anchor>,
    commit: Option<Anchor>,
    /// The last song known for an entry (for a way back in): entry, item, offset, class.
    known: Option<(EntryId, T, i64, CopyClass)>,
    suspended: Option<Suspension>,
    stepped_out: bool,
    /// A voice is on its way in at the room's place (an output switch, a realign): until it
    /// sounds, the person is realigning.
    switching: bool,
    cant_hold: bool,
    over_since: Option<u64>,
    /// The host's music taken in place: nothing acts on it until the room's first anchor.
    adopted: bool,
    ctl: Controller,
    dac: DacRate,
    epoch: Option<u32>,
    last_tick: u64,
    realign_since: Option<u64>,
    last_realign: Option<u64>,
    heard: Option<Anchor>,
}

/// The device frames of `us` of a source at `rate`.
fn frames_of(us: i64, rate: u32) -> i64 {
    (us as i128 * i128::from(rate) / 1_000_000) as i64
}

fn us_of(frames: f64, rate: u32) -> f64 {
    frames * 1e6 / f64::from(rate.max(1))
}

/// Ready as a prepare must be (4.4): `need_s` decoded at its place (or its ring full, or its
/// end), and for a stream the bytes for 10 s past its place or the whole download. A voice that
/// failed is ready to be found failed.
fn room_ready(lane: &Lane, need_s: Option<f64>) -> bool {
    let s = lane.status();
    if matches!(s.phase, Phase::Failed | Phase::Stopped) {
        return true;
    }
    let Some(f) = s.format.as_ref() else {
        return false;
    };
    let rate = f64::from(f.rate.max(1));
    if s.phase == Phase::Ended {
        return true;
    }
    let need = need_s.unwrap_or(if s.stream.is_some() {
        READY_STREAM_S
    } else {
        READY_FILE_S
    });
    let decoded = s.phase == Phase::Full || s.in_ring as f64 >= need * rate;
    if !decoded {
        return false;
    }
    match (&s.stream, s.bytes_read) {
        (Some(b), _) if b.complete => true,
        (Some(b), Some(read)) if s.decoded_to > 0 => {
            // Bytes per frame, as read so far.
            let bpf = read as f64 / s.decoded_to as f64;
            let covered = b.have as f64 / bpf.max(1e-9);
            covered >= s.start as f64 + READY_BYTES_S * rate
        }
        (Some(_), _) => false,
        (None, _) => true,
    }
}

impl<T: Item + Serialize> RoomProgram<T> {
    fn new(clock: ClockMapHandle, mode: SyncMode, status: Arc<Mutex<RoomPlayStatus>>) -> Self {
        RoomProgram {
            clock,
            mode,
            extra_us: 0,
            status,
            playing: None,
            fading: None,
            armed: None,
            spare: None,
            anchor: None,
            commit: None,
            known: None,
            suspended: None,
            stepped_out: false,
            switching: false,
            cant_hold: false,
            over_since: None,
            adopted: false,
            ctl: Controller::default(),
            dac: DacRate::default(),
            epoch: None,
            last_tick: 0,
            realign_since: None,
            last_realign: None,
            heard: None,
        }
    }

    fn map(&self) -> Option<ClockMap> {
        **self.clock.load()
    }

    /// The device frame that must be at the DAC at the host's time `host_us`, so it is heard
    /// then (the output's extra delay taken off).
    fn frame_for(&self, deck: &Deck, host_us: i64) -> Option<u64> {
        let map = self.map()?;
        let local = map.local_of(host_us) as i64 - i64::from(self.extra_us) * 1000;
        deck.frame_at(local.max(0) as u64)
    }

    /// The host's time at which device frame `f` is heard.
    fn host_at_frame(&self, deck: &Deck, f: u64) -> Option<i64> {
        let map = self.map()?;
        let t = deck.time_of(f)? as i64 + i64::from(self.extra_us) * 1000;
        Some(map.host_of(t.max(0) as u64))
    }

    /// The room's content (µs) heard at device frame `f`, while the room plays.
    fn room_at_frame(&self, deck: &Deck, f: u64) -> Option<i64> {
        let a = self.anchor.filter(|a| a.playing)?;
        Some(a.content_at(self.host_at_frame(deck, f)?))
    }

    /// The column of the controller's table that applies: an approximate copy is apart
    /// whatever the mode.
    fn column(&self) -> &'static Column {
        let approximate = self
            .playing
            .as_ref()
            .is_some_and(|p| p.class == CopyClass::Approximate);
        if self.mode == SyncMode::EveryDevice && !approximate {
            &SAME_ROOM
        } else {
            &APART
        }
    }

    /// A voice of `item` for the room's content `room_us` (my copy at that plus `offset`).
    #[allow(clippy::too_many_arguments)]
    fn open(
        deck: &mut Deck,
        entry: EntryId,
        item: T,
        room_us: i64,
        offset_us: i64,
        class: CopyClass,
        rate_hint: Option<u32>,
        depth: Option<Duration>,
    ) -> RoomSong<T> {
        let mine = (room_us + offset_us).max(0);
        let place = match rate_hint {
            Some(r) if r > 0 => Place::Frame(frames_of(mine, r) as u64),
            _ => Place::Us(mine as u64),
        };
        let mut lane = deck.lane_with(&item, place, depth);
        lane.rate_hint = rate_hint.unwrap_or(0);
        RoomSong {
            entry,
            offset_us,
            class,
            song: Song {
                lane,
                item,
                state: State::Waiting,
            },
        }
    }

    fn drop_spare(&mut self, deck: &mut Deck) {
        if let Some(s) = self.spare.take() {
            retire(deck, s.rs.song);
        }
    }

    fn drop_armed(&mut self, deck: &mut Deck) {
        if let Some(a) = self.armed.take() {
            retire(deck, a.rs.song);
        }
    }

    /// The rate the song's copy is known at (a voice of it opened before).
    fn rate_hint(&self, entry: EntryId) -> Option<u32> {
        [
            self.playing.as_ref(),
            self.armed.as_ref().map(|a| &a.rs),
            self.spare.as_ref().map(|s| &s.rs),
        ]
        .into_iter()
        .flatten()
        .find(|r| r.entry == entry)
        .and_then(|r| r.rate())
    }

    /// The song playing fades out over `ms` from now, and `why` says what follows.
    fn fade_out(&mut self, deck: &mut Deck, ms: u32, why: Fading) {
        if let Some(p) = self.playing.as_mut()
            && matches!(p.song.state, State::Given | State::Sounding)
        {
            let at = deck.soon();
            deck.stop_at(p.vid(), at, ms);
            p.song.state = State::Stopping(at);
            self.fading = Some(why);
        }
    }

    fn message(&mut self, m: RoomMsg<T>, deck: &mut Deck) {
        match m {
            RoomMsg::Prepare {
                prep,
                entry,
                item,
                room_content_us,
                align,
            } => {
                // A newer prepare replaces an older one, a realign and a way back in: the commit
                // that follows puts this person where the room is.
                self.drop_spare(deck);
                self.realign_since = None;
                self.known = Some((entry, item.clone(), align.offset_us, align.class));
                if self.stepped_out {
                    return;
                }
                let hint = self.rate_hint(entry);
                let rs = Self::open(
                    deck,
                    entry,
                    item,
                    room_content_us,
                    align.offset_us,
                    align.class,
                    hint,
                    None,
                );
                self.spare = Some(Spare {
                    rs,
                    what: Use::Prepare {
                        prep,
                        content_us: room_content_us,
                    },
                });
            }
            RoomMsg::ArmNext {
                entry,
                item,
                boundary_host_us,
                transition,
                align,
            } => {
                if self.armed.as_ref().is_some_and(|a| a.given) {
                    // Inside the last second its switch is with the renderer (the room never
                    // re-arms within 3 s of a boundary).
                    return;
                }
                self.drop_armed(deck);
                let hint = self.rate_hint(entry);
                let rs = Self::open(
                    deck,
                    entry,
                    item,
                    0,
                    align.offset_us,
                    align.class,
                    hint,
                    None,
                );
                self.armed = Some(Armed {
                    rs,
                    boundary_us: boundary_host_us,
                    transition,
                    given: false,
                });
            }
            RoomMsg::Disarm => {
                if !self.armed.as_ref().is_some_and(|a| a.given) {
                    self.drop_armed(deck);
                }
            }
            RoomMsg::Commit(a) => self.commit(a, deck),
            RoomMsg::Hold(a) => {
                self.anchor = Some(a);
                self.hold(a, deck);
            }
            RoomMsg::SetMode(m) => {
                self.mode = m;
                if m == SyncMode::Apart && self.cant_hold {
                    self.cant_hold = false;
                    self.over_since = None;
                    self.way_back(deck, ENTRANCE_MS, None);
                }
            }
            RoomMsg::SetExtraLatency(us) => {
                self.extra_us = us;
                // What the controller measured was against the old delay.
                let now = deck.now_ns();
                self.ctl.settle(now, now);
            }
            RoomMsg::StepOut => {
                self.stepped_out = true;
                self.commit = None;
                self.realign_since = None;
                self.drop_spare(deck);
                self.fade_out(deck, SKIP_FADE_MS, Fading::Out);
            }
            RoomMsg::Rejoin => {
                if self.stepped_out {
                    self.stepped_out = false;
                    self.way_back(deck, ENTRANCE_MS, None);
                }
            }
            RoomMsg::Attach { .. } | RoomMsg::Detach => {}
        }
    }

    fn commit(&mut self, a: Anchor, deck: &mut Deck) {
        self.anchor = Some(a);
        if !a.playing {
            self.hold(a, deck);
            return;
        }
        if self.stepped_out {
            return;
        }
        let prepared = matches!(
            &self.spare,
            Some(Spare { rs, what: Use::Prepare { .. } }) if rs.entry == a.entry
        );
        if prepared {
            self.commit = Some(a);
            return;
        }
        let playing = self.playing.as_ref().is_some_and(|p| {
            p.entry == a.entry && matches!(p.song.state, State::Given | State::Sounding)
        });
        if playing {
            // Already there (after a boundary, the host's opening anchor): measured against it.
            if self.adopted {
                self.adopted = false;
                let now = deck.now_ns();
                self.ctl.settle(now, now);
            }
            return;
        }
        if self
            .armed
            .as_ref()
            .is_some_and(|n| n.rs.entry == a.entry && !n.given)
        {
            // Its boundary missed: in late, alone.
            let n = self.armed.take().expect("armed");
            self.known = Some((
                n.rs.entry,
                n.rs.song.item.clone(),
                n.rs.offset_us,
                n.rs.class,
            ));
            retire(deck, n.rs.song);
            self.way_back(deck, ENTRANCE_MS, None);
            return;
        }
        if self.playing.is_none() && self.known.as_ref().is_some_and(|k| k.0 == a.entry) {
            self.way_back(deck, ENTRANCE_MS, None);
        }
    }

    /// A scheduled pause: fade out from the frame heard at `a.host_us`, then hold.
    fn hold(&mut self, a: Anchor, deck: &mut Deck) {
        self.commit = None;
        self.realign_since = None;
        if !matches!(
            self.spare,
            Some(Spare {
                what: Use::Prepare { .. },
                ..
            })
        ) {
            self.drop_spare(deck);
        }
        self.suspended = None;
        let at = self
            .frame_for(deck, a.host_us)
            .unwrap_or(0)
            .max(deck.soon());
        if let Some(p) = self.playing.as_mut()
            && matches!(p.song.state, State::Given | State::Sounding)
        {
            deck.stop_at(p.vid(), at, SKIP_FADE_MS);
            p.song.state = State::Stopping(at);
            self.fading = Some(Fading::Out);
        }
    }

    /// Back in at the room's place: a voice opened where the room will be a moment on,
    /// started there once ready (`need_s` past the room's place; `None`: as a prepare).
    fn way_back(&mut self, deck: &mut Deck, fade_ms: u32, need_s: Option<f64>) {
        let Some(a) = self.anchor.filter(|a| a.playing) else {
            return;
        };
        let Some((entry, item, offset, class)) = self.known.clone().filter(|k| k.0 == a.entry)
        else {
            return;
        };
        let Some(map) = self.map() else {
            return;
        };
        self.drop_spare(deck);
        let host = map.host_of(deck.now_ns()) + ENTRY_LEAD_US;
        let hint = self.rate_hint(entry);
        let rs = Self::open(
            deck,
            entry,
            item,
            a.content_at(host),
            offset,
            class,
            hint,
            Some(ENTRY_DEPTH),
        );
        self.spare = Some(Spare {
            rs,
            what: Use::Entry {
                fade_ms,
                need_s: need_s.unwrap_or(0.0),
                lead_us: ENTRY_LEAD_US,
            },
        });
    }

    fn step(&mut self, deck: &mut Deck) {
        self.commit_step(deck);
        self.armed_step(deck);
        self.spare_step(deck);
        let now = deck.now_ns();
        if now >= self.last_tick + TICK_NS {
            self.last_tick = now;
            self.tick(deck);
        }
        self.publish(deck);
    }

    /// A commit whose song is ready: started (or switched to) on the frame its anchor says.
    fn commit_step(&mut self, deck: &mut Deck) {
        let Some(a) = self.commit else {
            return;
        };
        let Some(Spare {
            rs,
            what: Use::Prepare { content_us, .. },
        }) = self.spare.as_ref()
        else {
            self.commit = None;
            return;
        };
        let content_us = *content_us;
        if !room_ready(&rs.song.lane, None) || rs.song.lane.failed().is_some() {
            return;
        }
        let Some(f) = rs.song.lane.status().format else {
            return;
        };
        if deck.generation().is_some() && !deck.fits(&f) {
            // Another format on an output that plays only one (exclusive): what plays fades
            // out, and the new one starts from silence once the output is opened again.
            self.fade_out(deck, SKIP_FADE_MS, Fading::Out);
        }
        if !deck.ready_for(&f) || !deck.placed() {
            return;
        }
        // The prepared place is heard when the room reaches it.
        let at_host = a.host_us + (content_us - a.content_us);
        let Some(at) = self.frame_for(deck, at_host) else {
            return;
        };
        let Some(Spare { mut rs, .. }) = self.spare.take() else {
            return;
        };
        let start = rs.song.lane.start().unwrap_or(0);
        let late = at < deck.soon();
        let given = match self.playing.take() {
            Some(old) if old.song.state == State::Sounding => {
                let how = if old.entry == rs.entry {
                    Splice::Crossfade {
                        frames: deck.ms(CROSSFADE_MS) as u32,
                    }
                } else {
                    Splice::FadeOutThenIn {
                        out_frames: deck.ms(SKIP_FADE_MS) as u32,
                    }
                };
                let ok = deck.switch(old.vid(), &mut rs.song.lane, at, how);
                deck.retire(old.song.lane, Retire::AfterFade);
                ok
            }
            old => {
                if let Some(old) = old {
                    retire(deck, old.song);
                }
                // From silence: no fade at the song's first frame, 15 ms partway, 300 ms into a
                // room already playing (a join: the anchor began a second or more before).
                let entrance = late || a.host_us + 1_000_000 <= at_host;
                let fade = if entrance {
                    ENTRANCE_MS
                } else if start > 0 {
                    PARTWAY_MS
                } else {
                    0
                };
                deck.start(&mut rs.song.lane, at, deck.ms(fade) as u32)
            }
        };
        if given {
            rs.song.state = State::Given;
        }
        self.known = Some((rs.entry, rs.song.item.clone(), rs.offset_us, rs.class));
        self.playing = Some(rs);
        self.fading = None;
        self.commit = None;
        self.suspended = None;
        self.adopted = false;
        let now = deck.now_ns();
        let until = deck.time_of(at).unwrap_or(now);
        self.ctl.settle(now, until + SETTLE_NS);
    }

    /// The device frame the playing song ends at (from its length and where it is rendered).
    fn end_frame(&self, deck: &Deck) -> Option<u64> {
        let p = self.playing.as_ref()?;
        let s = p.song.lane.status();
        let total = s
            .frames
            .or((s.phase == Phase::Ended).then_some(s.decoded_to))?;
        let (content, ratio) = deck.rendered_place(p.vid())?;
        let ratio = if ratio > 0.0 { ratio } else { return None };
        Some(deck.frames() + ((total as f64 - content).max(0.0) / ratio).round() as u64)
    }

    /// The armed next, switched to at its boundary by the table of 4.4.
    fn armed_step(&mut self, deck: &mut Deck) {
        let Some(n) = self.armed.as_ref() else {
            return;
        };
        if n.given || self.stepped_out || self.suspended.is_some() {
            return;
        }
        let Some(a) = self.anchor.filter(|a| a.playing) else {
            return;
        };
        if self.playing.as_ref().is_some_and(|p| p.entry != a.entry) {
            return;
        }
        // My copy starts at my content for the room's 0: its offset (a later frame when the
        // offset is negative).
        let late_start = (-n.rs.offset_us).max(0);
        let Some(f_b) = self.frame_for(deck, n.boundary_us + late_start) else {
            return;
        };
        let now_f = deck.frames();
        if f_b > now_f + deck.ms(ARM_AHEAD_MS) {
            return;
        }
        let lane = &n.rs.song.lane;
        let ready = room_ready(lane, None) && lane.failed().is_none();
        let fmt = lane.status().format;
        let fits = fmt.as_ref().is_some_and(|f| deck.fits(f));
        if !ready || !fits || f_b < deck.soon() {
            if f_b <= deck.soon() {
                self.missed_boundary(deck, f_b);
            }
            return;
        }
        let start = lane.start().unwrap_or(0);
        let sounding = self
            .playing
            .as_ref()
            .is_some_and(|p| p.song.state == State::Sounding);
        let end = if sounding { self.end_frame(deck) } else { None };
        let exact = self
            .playing
            .as_ref()
            .is_some_and(|p| p.class == CopyClass::Exact)
            && n.rs.class == CopyClass::Exact;
        let w = deck.ms(EXACT_JOIN_MS);
        let how = match (n.transition, end) {
            _ if !sounding => None,
            (Transition::Crossfade { ms }, Some(e)) if e > f_b => Some(Splice::Crossfade {
                frames: deck.ms(ms) as u32,
            }),
            (Transition::Crossfade { ms }, None) => Some(Splice::Crossfade {
                frames: deck.ms(ms) as u32,
            }),
            (Transition::Gapless, Some(e)) if exact && start == 0 && e.abs_diff(f_b) <= w => {
                Some(Splice::Gapless)
            }
            (_, Some(e)) if e + w < f_b || e <= f_b => None,
            _ => Some(Splice::EndFade {
                frames: deck.ms(END_FADE_MS) as u32,
            }),
        };
        let n = self.armed.as_mut().expect("armed");
        let ok = match (how, self.playing.as_ref()) {
            (Some(how), Some(p)) => deck.switch(p.vid(), &mut n.rs.song.lane, f_b, how),
            _ => {
                let fade = if start > 0 { deck.ms(PARTWAY_MS) } else { 0 };
                deck.start(&mut n.rs.song.lane, f_b, fade as u32)
            }
        };
        if ok {
            n.given = true;
            n.rs.song.state = State::Given;
        }
    }

    /// The next wasn't ready for its boundary: the song playing stops there (when longer) and
    /// the next comes in late, alone.
    fn missed_boundary(&mut self, deck: &mut Deck, f_b: u64) {
        let Some(n) = self.armed.take() else {
            return;
        };
        let end = self.end_frame(deck);
        if end.is_none_or(|e| e > f_b + deck.ms(EXACT_JOIN_MS)) {
            self.fade_out(deck, END_FADE_MS, Fading::Out);
        }
        self.anchor = Some(Anchor {
            entry: n.rs.entry,
            content_us: 0,
            host_us: n.boundary_us,
            playing: true,
            seq: self.anchor.map_or(0, |a| a.seq),
        });
        self.known = Some((
            n.rs.entry,
            n.rs.song.item.clone(),
            n.rs.offset_us,
            n.rs.class,
        ));
        retire(deck, n.rs.song);
        self.way_back(deck, ENTRANCE_MS, None);
    }

    /// The spare's work: a realign's crossfade, a way back in.
    fn spare_step(&mut self, deck: &mut Deck) {
        let Some(sp) = self.spare.as_ref() else {
            return;
        };
        match sp.what {
            Use::Prepare { .. } => {}
            Use::Realign { at } => {
                let lane = &sp.rs.song.lane;
                if lane.failed().is_some() || at <= deck.soon() {
                    // Not in time: given up, the next moment looked for.
                    self.drop_spare(deck);
                    return;
                }
                let fits = lane.status().format.is_some_and(|f| deck.fits(&f));
                let sounding = self
                    .playing
                    .as_ref()
                    .is_some_and(|p| p.song.state == State::Sounding);
                if !lane.ready() || !fits || !sounding {
                    return;
                }
                let Some(Spare { mut rs, .. }) = self.spare.take() else {
                    return;
                };
                let old = self.playing.take().expect("sounding");
                let how = Splice::Crossfade {
                    frames: deck.ms(CROSSFADE_MS) as u32,
                };
                if deck.switch(old.vid(), &mut rs.song.lane, at, how) {
                    rs.song.state = State::Given;
                    deck.retire(old.song.lane, Retire::AfterFade);
                    self.playing = Some(rs);
                    // Realigning until the new place sounds.
                    self.switching = true;
                    let now = deck.now_ns();
                    self.last_realign = Some(now);
                    self.realign_since = None;
                    let until = deck.time_of(at + deck.ms(CROSSFADE_MS)).unwrap_or(now);
                    self.ctl.settle(now, until + SETTLE_NS);
                } else {
                    self.playing = Some(old);
                    retire(deck, rs.song);
                }
            }
            Use::Entry {
                fade_ms,
                need_s,
                lead_us,
            } => self.entry_step(deck, fade_ms, need_s, lead_us),
        }
    }

    fn entry_step(&mut self, deck: &mut Deck, fade_ms: u32, need_s: f64, lead_us: i64) {
        let Some(sp) = self.spare.as_ref() else {
            return;
        };
        let lane = &sp.rs.song.lane;
        if lane.failed().is_some() || self.playing.is_some() {
            return;
        }
        let need = (need_s > 0.0).then_some(need_s);
        if !room_ready(lane, need) {
            return;
        }
        let s = lane.status();
        let Some(f) = s.format.clone() else {
            return;
        };
        if !deck.ready_for(&f) || !deck.placed() {
            return;
        }
        let Some(a) = self.anchor.filter(|a| a.playing) else {
            return;
        };
        let rate = f.rate.max(1);
        let first = lane.start().unwrap_or(0);
        let offset = sp.rs.offset_us;
        let f0 = deck.soon_quiet();
        let Some(room0) = self.room_at_frame(deck, f0) else {
            return;
        };
        let c0 = frames_of(room0 + offset, rate);
        let cover = need_s.max(1.0);
        let (at, content) = if c0 < first as i64 {
            // The room hasn't reached it yet: at the frame it does.
            let room_first = us_of(first as f64, rate) as i64 - offset;
            let host = a.host_us + (room_first - a.content_us);
            match self.frame_for(deck, host) {
                Some(at) => (at, first),
                None => return,
            }
        } else if (s.decoded_to as f64) >= c0 as f64 + cover * f64::from(rate) {
            (f0, c0 as u64)
        } else {
            // The room is past what it holds (a stream still behind): further on, the lead
            // doubled.
            let item = sp.rs.song.item.clone();
            let (entry, class) = (sp.rs.entry, sp.rs.class);
            let lead = (lead_us * 2).min(ENTRY_LEAD_MAX_US);
            self.drop_spare(deck);
            let Some(map) = self.map() else {
                return;
            };
            let host = map.host_of(deck.now_ns()) + lead;
            let rs = Self::open(
                deck,
                entry,
                item,
                a.content_at(host),
                offset,
                class,
                Some(rate),
                Some(ENTRY_DEPTH),
            );
            self.spare = Some(Spare {
                rs,
                what: Use::Entry {
                    fade_ms,
                    need_s,
                    lead_us: lead,
                },
            });
            return;
        };
        let Some(Spare { mut rs, .. }) = self.spare.take() else {
            return;
        };
        if deck.start_from(
            &mut rs.song.lane,
            at,
            Some(content),
            deck.ms(fade_ms) as u32,
        ) {
            rs.song.state = State::Given;
        }
        self.playing = Some(rs);
        self.fading = None;
        self.suspended = None;
        let now = deck.now_ns();
        let until = deck.time_of(at).unwrap_or(now);
        self.ctl.settle(now, until + SETTLE_NS);
    }

    /// The sync tick (4.9): measure, steer, realign, can't hold; and what is heard.
    fn tick(&mut self, deck: &mut Deck) {
        let now = deck.now_ns();
        if let Some((generation, frame, time)) = deck.presented() {
            self.dac.push(generation, frame, time, deck.rate());
        }
        self.heard = None;
        let Some(map) = self.map() else {
            return;
        };
        if self.epoch != Some(map.epoch) {
            // A new epoch: what was measured on the old one is measured again.
            self.epoch = Some(map.epoch);
            self.ctl.settle(now, now);
        }
        let Some(p) = self
            .playing
            .as_ref()
            .filter(|p| p.song.state == State::Sounding)
        else {
            self.ctl.freeze();
            return;
        };
        let (vid, entry, offset) = (p.vid(), p.entry, p.offset_us);
        let Some(rate) = p.rate() else {
            return;
        };
        let heard_host = map.host_of((now as i64 + i64::from(self.extra_us) * 1000).max(0) as u64);
        let measured = deck
            .dac_frame(now)
            .and_then(|f| deck.content_at_f(vid, f))
            .map(|c| us_of(c, rate) - offset as f64);
        if let Some(c) = measured {
            self.heard = Some(Anchor {
                entry,
                content_us: c.round() as i64,
                host_us: heard_host,
                playing: true,
                seq: 0,
            });
        }
        if self.adopted {
            // The host's music, untouched until the room's first anchor.
            return;
        }
        let anchor = self
            .anchor
            .filter(|a| a.playing && a.entry == entry && a.host_us <= heard_host);
        let err = match (anchor, measured) {
            (Some(a), Some(c)) => Some((c - a.content_at(heard_host) as f64) / 1000.0),
            _ => None,
        };
        let col = *self.column();
        let ff = (map.rate - 1.0) * 1e6 - self.dac.ppm();
        let steer = self
            .ctl
            .tick(now, err, &col, f64::from(map.bound_us) / 1000.0, ff);
        deck.set_trim(steer.ppm);
        let Some(e) = steer.error_ms.filter(|_| err.is_some()) else {
            return;
        };
        self.cant_hold_check(deck, now, e, steer.settling);
        self.realign_check(deck, now, e, steer.far, &col);
    }

    /// Every device mode: over 5 ms for 30 s, this output can't hold; it fades out over 1 s
    /// rather than smear the room's sound.
    fn cant_hold_check(&mut self, deck: &mut Deck, now: u64, e: f64, settling: bool) {
        if self.mode != SyncMode::EveryDevice || self.column() != &SAME_ROOM || settling {
            self.over_since = None;
            return;
        }
        if e.abs() <= CANT_HOLD_MS {
            self.over_since = None;
            return;
        }
        let since = *self.over_since.get_or_insert(now);
        if now - since >= CANT_HOLD_NS {
            self.cant_hold = true;
            self.over_since = None;
            self.realign_since = None;
            self.drop_spare(deck);
            self.fade_out(deck, 1_000, Fading::CantHold);
        }
    }

    /// Far past the realign line, at a quiet moment, by a 40 ms crossfade (4.9's conditions).
    fn realign_check(&mut self, deck: &mut Deck, now: u64, e: f64, far: bool, col: &Column) {
        if e.abs() < col.band_out {
            self.realign_since = None;
            if matches!(
                self.spare,
                Some(Spare {
                    what: Use::Realign { .. },
                    ..
                })
            ) {
                self.drop_spare(deck);
            }
            return;
        }
        if far && self.realign_since.is_none() {
            let spaced = self
                .last_realign
                .is_none_or(|t| now >= t + col.realign_every_ns);
            if spaced && self.spare.is_none() && self.suspended.is_none() && !self.stepped_out {
                self.realign_since = Some(now);
            }
        }
        let Some(since) = self.realign_since else {
            return;
        };
        if self.spare.is_some() {
            return;
        }
        // No fade under way or due (condition 4).
        if deck.quiet_after() > deck.frames() {
            return;
        }
        let Some(p) = self.playing.as_ref() else {
            return;
        };
        let (vid, rate) = (p.vid(), p.rate().unwrap_or(48_000));
        let Some((c_r, ratio)) = deck.rendered_place(vid) else {
            return;
        };
        if ratio <= 0.0 {
            return;
        }
        let r = f64::from(rate);
        let from = c_r + QUIET_FROM_S * r;
        let blocks = p.song.lane.loudness(from as u64);
        let want = (f64::from(QUIET_MS) / 1000.0 * r) as u64;
        let upto = c_r + QUIET_TO_S * r;
        let mut q = quiet_moment(&blocks, from as u64, upto as u64, want);
        if q.is_none() && now >= since + col.quiet_look_ns {
            q = quietest(&blocks, from as u64, want);
        }
        let Some(q) = q else {
            return;
        };
        let at = deck.frames() + ((q as f64 - c_r) / ratio).round() as u64;
        // Never within 3 s of the song's end or the armed boundary (condition 2).
        let clear = deck.ms((REALIGN_CLEAR_S * 1000.0) as u32);
        if self.end_frame(deck).is_some_and(|e| at + clear > e) {
            return;
        }
        if let Some(n) = self.armed.as_ref()
            && self
                .frame_for(deck, n.boundary_us)
                .is_some_and(|b| at + clear > b)
        {
            return;
        }
        let Some(room) = self.room_at_frame(deck, at) else {
            return;
        };
        let p = self.playing.as_ref().expect("playing");
        let (entry, item, offset, class) = (p.entry, p.song.item.clone(), p.offset_us, p.class);
        let rs = Self::open(deck, entry, item, room, offset, class, Some(rate), None);
        self.spare = Some(Spare {
            rs,
            what: Use::Realign { at },
        });
    }

    fn note(&mut self, n: &RenderNote, deck: &mut Deck) {
        let is = |p: &Option<RoomSong<T>>, v: VoiceId| p.as_ref().is_some_and(|p| p.vid() == v);
        let armed_is = |a: &Option<Armed<T>>, v: VoiceId| {
            a.as_ref().is_some_and(|a| a.given && a.rs.vid() == v)
        };
        match *n {
            RenderNote::Started { voice, .. } => {
                if is(&self.playing, voice) {
                    if let Some(p) = self.playing.as_mut()
                        && p.song.state == State::Given
                    {
                        p.song.state = State::Sounding;
                    }
                    // Back on the new output.
                    self.switching = false;
                } else if armed_is(&self.armed, voice) {
                    self.take_over(deck, false);
                }
            }
            RenderNote::Joined { to, .. } => {
                if armed_is(&self.armed, to) {
                    self.take_over(deck, true);
                }
            }
            RenderNote::Ended { voice, .. } => {
                if is(&self.playing, voice)
                    && let Some(p) = self.playing.take()
                {
                    deck.retire(p.song.lane, Retire::Now);
                }
            }
            RenderNote::Silent { voice, .. } => {
                let stopping = self.playing.as_ref().is_some_and(|p| {
                    p.vid() == voice && matches!(p.song.state, State::Stopping(_))
                });
                if stopping && let Some(p) = self.playing.take() {
                    deck.retire(p.song.lane, Retire::Now);
                    let why = self.fading.take();
                    if why == Some(Fading::Switch) {
                        self.way_back(deck, PARTWAY_MS, None);
                    }
                }
            }
            RenderNote::Starved { voice, .. } => {
                let ours = self
                    .playing
                    .as_ref()
                    .is_some_and(|p| p.vid() == voice && p.song.state == State::Sounding);
                if ours && let Some(p) = self.playing.take() {
                    // Faded out by the renderer's guard: dropped at once (never its own resume
                    // at the old place), and back in at the room's place once refilled.
                    self.known = Some((p.entry, p.song.item.clone(), p.offset_us, p.class));
                    deck.retire(p.song.lane, Retire::Now);
                    self.suspended = Some(Suspension::Buffering);
                    self.realign_since = None;
                    if !matches!(
                        self.spare,
                        Some(Spare {
                            what: Use::Prepare { .. },
                            ..
                        })
                    ) {
                        self.way_back(deck, ENTRANCE_MS, Some(REFILL_S));
                    }
                }
            }
            RenderNote::Refused { voice, .. } => {
                if is(&self.playing, voice) {
                    if let Some(p) = self.playing.take() {
                        retire(deck, p.song);
                    }
                } else if self.armed.as_ref().is_some_and(|a| a.rs.vid() == voice) {
                    self.drop_armed(deck);
                } else if self.spare.as_ref().is_some_and(|s| s.rs.vid() == voice) {
                    self.drop_spare(deck);
                }
            }
            _ => {}
        }
    }

    /// The armed next took over at its boundary: it is the song playing, its anchor content 0
    /// at the boundary (the room's commit after it is a no-op).
    fn take_over(&mut self, deck: &mut Deck, old_done: bool) {
        let Some(mut n) = self.armed.take() else {
            return;
        };
        if let Some(old) = self.playing.take() {
            let how = if old_done || old.song.state != State::Sounding {
                Retire::Now
            } else {
                Retire::AfterFade
            };
            deck.retire(old.song.lane, how);
        }
        n.rs.song.state = State::Sounding;
        self.anchor = Some(Anchor {
            entry: n.rs.entry,
            content_us: 0,
            host_us: n.boundary_us,
            playing: true,
            seq: self.anchor.map_or(0, |a| a.seq),
        });
        self.known = Some((
            n.rs.entry,
            n.rs.song.item.clone(),
            n.rs.offset_us,
            n.rs.class,
        ));
        self.playing = Some(n.rs);
        self.fading = None;
        let now = deck.now_ns();
        self.ctl.settle(now, now);
    }

    /// The renderer start `ev.generation` is over (an output lost, switched, reopened): its
    /// voices are gone; the song comes back in at the room's place.
    fn stopped(&mut self, ev: &Stopped, deck: &mut Deck) {
        let ended = Some(ev.generation);
        if self
            .armed
            .as_ref()
            .is_some_and(|a| a.rs.song.lane.given == ended)
        {
            // Armed again on a fresh voice (its tap went with the renderer).
            let n = self.armed.take().expect("armed");
            let item = n.rs.song.item.clone();
            let hint = n.rs.rate();
            retire(deck, n.rs.song);
            let rs = Self::open(
                deck,
                n.rs.entry,
                item,
                0,
                n.rs.offset_us,
                n.rs.class,
                hint,
                None,
            );
            self.armed = Some(Armed {
                rs,
                boundary_us: n.boundary_us,
                transition: n.transition,
                given: false,
            });
        }
        if !self
            .playing
            .as_ref()
            .is_some_and(|p| p.song.lane.given == ended)
        {
            return;
        }
        let p = self.playing.take().expect("playing");
        let was = p.song.state;
        self.known = Some((p.entry, p.song.item.clone(), p.offset_us, p.class));
        deck.retire(p.song.lane, Retire::Now);
        let why = self.fading.take();
        if self.stepped_out || matches!(why, Some(Fading::Out | Fading::CantHold)) {
            return;
        }
        if ev.lost_at.is_some() {
            self.suspended = Some(Suspension::OutputLost);
            self.ctl.freeze();
            self.way_back(deck, ENTRANCE_MS, None);
        } else if matches!(was, State::Given | State::Sounding) || why == Some(Fading::Switch) {
            self.way_back(deck, PARTWAY_MS, None);
        }
    }

    /// An output switch: what sounds fades out, and comes back in at the room's place on the
    /// new output.
    fn hush(&mut self, deck: &mut Deck) {
        self.realign_since = None;
        if matches!(
            self.spare,
            Some(Spare {
                what: Use::Realign { .. },
                ..
            })
        ) {
            self.drop_spare(deck);
        }
        self.switching = true;
        let sounding = self
            .playing
            .as_ref()
            .is_some_and(|p| matches!(p.song.state, State::Given | State::Sounding));
        if sounding {
            self.fade_out(deck, SKIP_FADE_MS, Fading::Switch);
        }
    }

    /// The host's music, adopted in place as `entry` (A1): no command to the renderer.
    fn adopt(&mut self, entry: EntryId, song: Song<T>) {
        self.known = Some((entry, song.item.clone(), 0, CopyClass::Exact));
        self.playing = Some(RoomSong {
            entry,
            offset_us: 0,
            class: CopyClass::Exact,
            song,
        });
        self.adopted = true;
    }

    fn publish(&mut self, deck: &Deck) {
        let prepared = match &self.spare {
            Some(Spare {
                rs,
                what: Use::Prepare { prep, .. },
            }) => Some((rs, *prep)),
            _ => None,
        };
        let ready = prepared
            .filter(|(rs, _)| room_ready(&rs.song.lane, None) && rs.song.lane.failed().is_none())
            .map(|(_, p)| p);
        let cannot = prepared.is_some_and(|(rs, _)| rs.song.lane.failed().is_some())
            || self.spare.as_ref().is_some_and(|s| {
                matches!(s.what, Use::Entry { .. }) && s.rs.song.lane.failed().is_some()
            });
        let next_ready = self
            .armed
            .as_ref()
            .filter(|a| {
                a.given || (room_ready(&a.rs.song.lane, None) && a.rs.song.lane.failed().is_none())
            })
            .map(|a| a.rs.entry);
        let sounding = self
            .playing
            .as_ref()
            .filter(|p| matches!(p.song.state, State::Given | State::Sounding));
        let steer = self.ctl.last();
        let realigning = self.switching
            || matches!(
                self.spare,
                Some(Spare {
                    what: Use::Realign { .. },
                    ..
                })
            )
            || self.realign_since.is_some();
        let sub_state = if self.stepped_out {
            SubState::SteppedOut
        } else if let Some(s) = self.suspended {
            SubState::Suspended(s)
        } else if cannot {
            SubState::CannotPlay
        } else if realigning {
            SubState::Realigning
        } else if sounding.is_some_and(|p| p.song.state == State::Sounding) {
            if steer.correcting {
                SubState::Correcting
            } else {
                SubState::InStep
            }
        } else if ready.is_some() {
            SubState::Ready
        } else {
            SubState::Opening
        };
        let mut st = self.status.lock().unwrap_or_else(|p| p.into_inner());
        st.ready = ready;
        st.next_ready = next_ready;
        st.entry = sounding.map(|p| p.entry);
        st.sub_state = sub_state;
        st.error_us = steer.error_ms.map(|e| (e * 1000.0).round() as i64);
        st.bound_us = self.map().map(|m| m.bound_us);
        st.copy = sounding.map(|p| p.class);
        st.ppm = if sounding.is_some() {
            deck.trim_now()
        } else {
            0.0
        };
        st.heard = self.heard;
        st.cant_hold = self.cant_hold;
    }

    /// The room lets go: what sounds fades out over a second, on the deck's own; the renderer
    /// is renewed before your own music sounds again.
    fn detach(self, deck: &mut Deck) {
        if let Some(s) = self.spare {
            retire(deck, s.rs.song);
        }
        if let Some(a) = self.armed {
            retire(deck, a.rs.song);
        }
        if let Some(p) = self.playing {
            match p.song.state {
                State::Sounding => {
                    let at = deck.soon();
                    deck.stop_at(p.vid(), at, 1_000);
                    deck.retire(p.song.lane, Retire::AfterFade);
                }
                // Given and not sounding yet: it never will.
                _ => retire(deck, p.song),
            }
        }
        deck.renew_renderer();
        let mut st = self.status.lock().unwrap_or_else(|p| p.into_inner());
        *st = RoomPlayStatus::default();
    }

    fn sync_status(&self) -> SyncStatus {
        let st = self.status.lock().unwrap_or_else(|p| p.into_inner());
        SyncStatus {
            ready: st.ready.is_some()
                || self
                    .playing
                    .as_ref()
                    .is_some_and(|p| p.song.state == State::Sounding),
            next_ready: st.next_ready.is_some(),
            error_ms: st.error_us.map(|e| e as f32 / 1000.0),
        }
    }
}

/// The first run of blocks from content frame `from` to `upto` all under -40 dBFS and at least
/// `want` frames long: where it starts.
fn quiet_moment(blocks: &[Loudness], from: u64, upto: u64, want: u64) -> Option<u64> {
    let mut run: Option<(u64, u64)> = None;
    for b in blocks
        .iter()
        .filter(|b| b.first_frame >= from && b.first_frame < upto)
    {
        let end = b.first_frame + u64::from(b.frames);
        if b.rms_db < QUIET_DB {
            run = match run {
                Some((s, e)) if e == b.first_frame => Some((s, end)),
                _ => Some((b.first_frame, end)),
            };
            if let Some((s, e)) = run
                && e - s >= want
            {
                return Some(s);
            }
        } else {
            run = None;
        }
    }
    None
}

/// The quietest `want` frames in hand from `from` (by the loudest block in each window).
fn quietest(blocks: &[Loudness], from: u64, want: u64) -> Option<u64> {
    let ahead: Vec<&Loudness> = blocks.iter().filter(|b| b.first_frame >= from).collect();
    let mut best: Option<(i8, u64)> = None;
    for i in 0..ahead.len() {
        let (mut loud, mut span) = (i8::MIN, 0u64);
        for b in &ahead[i..] {
            loud = loud.max(b.rms_db);
            span += u64::from(b.frames);
            if span >= want {
                break;
            }
        }
        if span < want {
            break;
        }
        if best.is_none_or(|(l, _)| loud < l) {
            best = Some((loud, ahead[i].first_frame));
        }
    }
    best.map(|(_, at)| at)
}

// --- Protocol 2 rooms: the `Cmd::Sync` adapter (until WP13) ----------------------------------

/// A protocol 2 room's plan, followed by your own program (the WP5 notes in 9.2): today's
/// follower loads the room's songs into your own queue; the plan says when the current one
/// starts, where it is, and keeps it there by rate, as engine 1 does.
pub(super) struct PlanFollow<T> {
    plan: Option<SyncPlan>,
    /// The plan's anchor on the engine's clock.
    anchor_ns: Option<u64>,
    ctl: Controller,
    dac: DacRate,
    /// The own queue moved on to the next song after this plan: no correction until its plan.
    hold: bool,
    /// The voice the plan was for.
    vid: Option<VoiceId>,
    /// A faded seek on its way: the new place's song, and the device frame it comes in at.
    seek: Option<(Song<T>, u64)>,
    /// Waiting (paused or before the start) at this place, ms.
    park_ms: Option<u64>,
    notes: Vec<SyncNote>,
    rate_said: i32,
    last_tick: u64,
    error_ms: Option<f32>,
}

/// A faded seek comes in this far ahead, ms.
const SEEK_AHEAD_MS: u32 = 300;
/// A start this late (the voice ready long after the plan's moment) opens again further on.
const FOLLOW_LATE_MS: u32 = 1_000;

impl<T: Item + Serialize> PlanFollow<T> {
    fn new() -> Self {
        PlanFollow {
            plan: None,
            anchor_ns: None,
            ctl: Controller::default(),
            dac: DacRate::default(),
            hold: false,
            vid: None,
            seek: None,
            park_ms: None,
            notes: Vec::new(),
            rate_said: 0,
            last_tick: 0,
            error_ms: None,
        }
    }

    /// Where the room's song is heard at local time `t_ns`, ms (`None` before its start, or
    /// paused: its place).
    fn expected_ms(&self, t_ns: u64) -> Option<f64> {
        let p = self.plan?;
        let a = self.anchor_ns?;
        if !p.playing {
            return Some(p.position_ms as f64);
        }
        Some(p.position_ms as f64 + (t_ns as f64 - a as f64) / 1e6)
    }

    fn started(&self, now: u64) -> bool {
        self.plan.is_some_and(|p| p.playing) && self.anchor_ns.is_some_and(|a| a <= now)
    }

    /// The local time a frame heard now is heard (the plan's extra delay).
    fn heard_ns(&self, t_ns: u64) -> u64 {
        let extra = self
            .plan
            .map_or(0, |p| i64::from(p.extra_latency_ms) * 1_000_000);
        (t_ns as i64 + extra).max(0) as u64
    }

    /// Where the current song is heard now, ms (its place when not sounding).
    fn place_ms(own: &OwnProgram<T>, deck: &Deck) -> Option<f64> {
        let c = own.cur.as_ref()?;
        let rate = f64::from(c.lane.rate().unwrap_or(44_100).max(1));
        if c.state == State::Sounding
            && let Some(content) = deck.heard_content(c.vid())
        {
            return Some(content.max(0.0) * 1000.0 / rate);
        }
        Some(match c.lane.place {
            Place::Frame(f) => f as f64 * 1000.0 / rate,
            Place::Us(us) => us as f64 / 1000.0,
        })
    }

    /// A new plan (engine 1's `set_sync`).
    fn set(&mut self, plan: SyncPlan, own: &mut OwnProgram<T>, deck: &mut Deck) {
        let now = deck.now_ns();
        let anchor_ns = plan.anchor.map(|i| deck.ns_of_instant(i));
        if let Some(old) = self.plan
            && crate::sync::is_nudge(&old, &plan, std::time::Instant::now())
        {
            self.plan = Some(plan);
            self.anchor_ns = anchor_ns;
            self.hold = false;
            return;
        }
        let limit = match (self.hold, plan.same_room) {
            (true, true) => crate::sync::NUDGE_MS,
            (true, false) => crate::sync::APART_NUDGE_MS,
            (false, _) => 30.0,
        };
        self.plan = Some(plan);
        self.anchor_ns = anchor_ns;
        self.hold = false;
        self.park_ms = None;
        if let Some((s, _)) = self.seek.take() {
            retire(deck, s);
        }
        self.ctl.settle(now, now);
        let started = self.started(now);
        let target = if started {
            self.expected_ms(self.heard_ns(now)).unwrap_or(0.0)
        } else {
            plan.position_ms as f64
        };
        if own.cur.is_none() && own.queue.current().is_some() {
            own.load_current(deck, Place::Us((target.max(0.0) * 1000.0) as u64));
        } else if let Some(place) = Self::place_ms(own, deck) {
            let sounding = own.cur.as_ref().is_some_and(|c| c.state == State::Sounding);
            let off = (place - target).abs();
            if started && sounding {
                if off > limit {
                    self.notes.push(SyncNote::Seek {
                        from_ms: place as u64,
                        to_ms: target.max(0.0) as u64,
                        faded: true,
                    });
                    self.begin_seek(own, deck);
                }
            } else {
                // A hold, a start ahead, a pause: the music fades out first.
                if own
                    .cur
                    .as_ref()
                    .is_some_and(|c| matches!(c.state, State::Given | State::Sounding))
                {
                    own.hush(deck);
                }
                if off > limit {
                    self.notes.push(SyncNote::Seek {
                        from_ms: place as u64,
                        to_ms: target.max(0.0) as u64,
                        faded: false,
                    });
                    self.park_ms = Some(target.max(0.0) as u64);
                }
            }
        }
        // Playing (or waiting to) keeps the output going: silence until the start.
        own.playing = plan.playing && own.cur.is_some();
        self.vid = own.cur.as_ref().map(|c| c.vid());
    }

    /// The place where the room will be a moment on, faded in there as the old one fades out.
    fn begin_seek(&mut self, own: &mut OwnProgram<T>, deck: &mut Deck) {
        let Some(c) = own.cur.as_ref() else {
            return;
        };
        let at = deck.soon() + deck.ms(SEEK_AHEAD_MS);
        let Some(t) = deck.time_of(at) else {
            return;
        };
        let Some(ms) = self.expected_ms(self.heard_ns(t)) else {
            return;
        };
        let rate = c.lane.rate().unwrap_or(0);
        let place = if rate > 0 {
            Place::Frame((ms.max(0.0) * f64::from(rate) / 1000.0) as u64)
        } else {
            Place::Us((ms.max(0.0) * 1000.0) as u64)
        };
        let item = c.item.clone();
        let mut lane = deck.lane(&item, place);
        lane.rate_hint = rate;
        self.seek = Some((
            Song {
                lane,
                item,
                state: State::Waiting,
            },
            at,
        ));
    }

    fn step(&mut self, own: &mut OwnProgram<T>, deck: &mut Deck) {
        if let Some(e) = deck.take_gave_up() {
            own.error = e;
            own.set_playing(false, deck);
            return;
        }
        let now = deck.now_ns();
        // The own queue moved on by itself (a gapless link): the plan was for the song before.
        let cur_vid = own.cur.as_ref().map(|c| c.vid());
        if cur_vid != self.vid {
            if self.vid.is_some()
                && own
                    .cur
                    .as_ref()
                    .is_some_and(|c| c.state == State::Sounding && c.lane.start() == Some(0))
            {
                self.hold = true;
            }
            self.vid = cur_vid;
        }
        if own.cur.is_none() && own.queue.current().is_some() && own.playing {
            own.load_current(deck, Place::Frame(0));
        }
        self.park(own, deck);
        self.start(own, deck, now);
        self.seek_step(own, deck);
        own.prefetch(deck);
        own.link_next(deck);
        if now >= self.last_tick + TICK_NS {
            self.last_tick = now;
            self.tick(own, deck, now);
        }
    }

    /// Paused or waiting: the song moved to the plan's place, in silence.
    fn park(&mut self, own: &mut OwnProgram<T>, deck: &mut Deck) {
        let Some(ms) = self.park_ms else {
            return;
        };
        let Some(c) = own.cur.as_ref() else {
            self.park_ms = None;
            return;
        };
        if c.state != State::Waiting {
            return;
        }
        self.park_ms = None;
        own.replace_cur(deck, Place::Us(ms * 1000));
        self.vid = own.cur.as_ref().map(|c| c.vid());
    }

    /// The current song, ready, starts on the frame the plan's anchor gives its place.
    fn start(&mut self, own: &mut OwnProgram<T>, deck: &mut Deck, now: u64) {
        if !own.playing || self.park_ms.is_some() || self.seek.is_some() {
            return;
        }
        let Some(c) = own.cur.as_ref() else {
            return;
        };
        if c.state != State::Waiting || !c.lane.ready() {
            return;
        }
        if c.lane.failed().is_some() {
            own.start_current(deck);
            return;
        }
        let Some(f) = c.lane.status().format else {
            return;
        };
        // The output runs (in silence until the start), and says what it plays when.
        if !deck.ready_for(&f) {
            return;
        }
        let (Some(plan), Some(a)) = (self.plan, self.anchor_ns) else {
            return;
        };
        if !deck.placed() {
            return;
        }
        let rate = f.rate.max(1);
        let first = c.lane.start().unwrap_or(0);
        let first_ms = first as f64 * 1000.0 / f64::from(rate);
        let due = a as f64 + (first_ms - plan.position_ms as f64) * 1e6
            - f64::from(plan.extra_latency_ms) * 1e6;
        let Some(at) = deck.frame_at(due.max(0.0) as u64) else {
            return;
        };
        let soon = deck.soon_quiet();
        if at + deck.ms(FOLLOW_LATE_MS) < soon {
            // Ready long after its moment: opened again where the room will be.
            let target =
                self.expected_ms(self.heard_ns(now)).unwrap_or(0.0) + f64::from(SEEK_AHEAD_MS);
            own.replace_cur(
                deck,
                Place::Frame((target.max(0.0) * f64::from(rate) / 1000.0) as u64),
            );
            self.vid = own.cur.as_ref().map(|c| c.vid());
            return;
        }
        let fade = if at < soon || first > 0 {
            deck.ms(PARTWAY_MS)
        } else {
            0
        };
        let c = own.cur.as_mut().expect("current");
        if deck.start(&mut c.lane, at.max(1), fade as u32) {
            c.state = State::Given;
            own.failures = 0;
            self.notes.push(SyncNote::Started {
                position_ms: first_ms as u64,
            });
            let until = deck.time_of(at).unwrap_or(now);
            self.ctl.settle(now, until + SETTLE_NS);
        }
    }

    fn seek_step(&mut self, own: &mut OwnProgram<T>, deck: &mut Deck) {
        let Some((s, at)) = self.seek.as_ref() else {
            return;
        };
        let at = *at;
        let sounding = own.cur.as_ref().is_some_and(|c| c.state == State::Sounding);
        if !sounding || at <= deck.soon() || s.lane.failed().is_some() {
            // Not in time: again, from where the room is now.
            let (s, _) = self.seek.take().expect("a seek");
            retire(deck, s);
            if sounding {
                self.begin_seek(own, deck);
            }
            return;
        }
        let fits = s.lane.status().format.is_some_and(|f| deck.fits(&f));
        if !s.lane.ready() || !fits {
            return;
        }
        let (mut s, at) = self.seek.take().expect("a seek");
        let old = own.cur.take().expect("current");
        let how = Splice::FadeOutThenIn {
            out_frames: deck.ms(SKIP_FADE_MS) as u32,
        };
        if deck.switch(old.vid(), &mut s.lane, at, how) {
            s.state = State::Given;
            deck.retire(old.lane, Retire::AfterFade);
            own.drop_next(deck);
            self.vid = Some(s.vid());
            own.cur = Some(s);
            let now = deck.now_ns();
            let until = deck.time_of(at).unwrap_or(now);
            self.ctl.settle(now, until + SETTLE_NS);
        } else {
            own.cur = Some(old);
            retire(deck, s);
        }
    }

    fn tick(&mut self, own: &mut OwnProgram<T>, deck: &mut Deck, now: u64) {
        if let Some((generation, frame, time)) = deck.presented() {
            self.dac.push(generation, frame, time, deck.rate());
        }
        let Some(plan) = self.plan else {
            return;
        };
        let Some(c) = own.cur.as_ref().filter(|c| c.state == State::Sounding) else {
            return;
        };
        let rate = c.lane.rate().unwrap_or(44_100);
        let measured = deck
            .dac_frame(now)
            .and_then(|f| deck.content_at_f(c.vid(), f))
            .map(|x| x * 1000.0 / f64::from(rate.max(1)));
        let err = match (
            measured,
            self.started(now) && !self.hold && self.seek.is_none(),
        ) {
            (Some(m), true) => self.expected_ms(self.heard_ns(now)).map(|e| m - e),
            _ => None,
        };
        let col = if plan.same_room { SAME_ROOM } else { APART };
        let ff = -self.dac.ppm();
        let steer = self.ctl.tick(now, err, &col, 0.0, ff);
        deck.set_trim(steer.ppm);
        self.error_ms = steer.error_ms.filter(|_| err.is_some()).map(|e| e as f32);
        if steer.far && err.is_some() {
            if let Some(m) = measured {
                let to = self.expected_ms(self.heard_ns(now)).unwrap_or(m);
                self.notes.push(SyncNote::Seek {
                    from_ms: m as u64,
                    to_ms: to.max(0.0) as u64,
                    faded: true,
                });
            }
            self.begin_seek(own, deck);
            self.ctl.settle(now, now);
        }
        let ppm = deck.trim_now().round() as i32;
        let changed = (ppm == 0) != (self.rate_said == 0) || (ppm - self.rate_said).abs() >= 100;
        if changed {
            self.rate_said = ppm;
            self.notes.push(SyncNote::Rate { ppm });
        }
    }

    fn wants_output(&self, own: &OwnProgram<T>) -> bool {
        own.wants_output() || (self.plan.is_some_and(|p| p.playing) && own.cur.is_some())
    }

    fn sync_status(&self, own: &OwnProgram<T>, deck: &Deck) -> SyncStatus {
        let ready = own.cur.as_ref().is_some_and(|c| {
            c.state == State::Sounding || (c.lane.ready() && c.lane.failed().is_none())
        }) && self.park_ms.is_none();
        let next_ready = own
            .next
            .as_ref()
            .is_some_and(|n| n.lane.ready() && n.lane.failed().is_none());
        let _ = deck;
        SyncStatus {
            ready,
            next_ready,
            error_ms: self.error_ms,
        }
    }

    /// Back on its own: the trim goes, and the renderer is renewed before the next start.
    fn leave(self, deck: &mut Deck) {
        if let Some((s, _)) = self.seek {
            retire(deck, s);
        }
        deck.set_trim(0.0);
        deck.renew_renderer();
    }
}

// --- Which program plays -----------------------------------------------------------------------

pub(super) enum Program<T> {
    Own(OwnProgram<T>),
    /// A room plays; your own program waits, set aside (no voice of its own).
    Room {
        room: Box<RoomProgram<T>>,
        own: OwnProgram<T>,
    },
    /// The host's room asked for while your own crossfade into the next song is set to begin:
    /// taken once it has begun (the song playing then goes on as the room's), the room's calls
    /// held till then.
    Adopting {
        own: OwnProgram<T>,
        calls: Vec<RoomMsg<T>>,
    },
    /// A protocol 2 room's plan drives your own program (the `Cmd::Sync` adapter).
    Followed {
        own: OwnProgram<T>,
        plan: Box<PlanFollow<T>>,
    },
}

impl<T: Item + Serialize> Program<T> {
    pub(super) fn new() -> Self {
        Program::Own(OwnProgram::new())
    }

    fn own(&mut self) -> &mut OwnProgram<T> {
        match self {
            Program::Own(o)
            | Program::Room { own: o, .. }
            | Program::Adopting { own: o, .. }
            | Program::Followed { own: o, .. } => o,
        }
    }

    pub(super) fn busy(&self) -> bool {
        match self {
            Program::Own(o) => o.busy(),
            _ => true,
        }
    }

    pub(super) fn wants_output(&self) -> bool {
        match self {
            Program::Own(o) | Program::Adopting { own: o, .. } => o.wants_output(),
            Program::Room { .. } => true,
            Program::Followed { own, plan } => plan.wants_output(own),
        }
    }

    /// Your queue's commands go to your own program, in a room too (it plays nothing there:
    /// it comes back paused).
    pub(super) fn command(&mut self, cmd: Cmd<T>, deck: &mut Deck) {
        self.own().command(cmd, deck);
    }

    /// A protocol 2 room's plan (`None`: on its own again). A room on the new protocol holds
    /// the player: then the plan is no one's.
    pub(super) fn sync(&mut self, plan: Option<SyncPlan>, deck: &mut Deck) {
        match (plan, std::mem::replace(self, Program::new())) {
            (Some(p), Program::Own(mut own)) => {
                let mut f = Box::new(PlanFollow::new());
                f.set(p, &mut own, deck);
                *self = Program::Followed { own, plan: f };
            }
            (Some(p), Program::Followed { mut own, mut plan }) => {
                plan.set(p, &mut own, deck);
                *self = Program::Followed { own, plan };
            }
            (None, Program::Followed { own, plan }) => {
                plan.leave(deck);
                *self = Program::Own(own);
            }
            (_, other) => *self = other,
        }
    }

    /// What was done to the music for a protocol 2 room since last asked.
    pub(super) fn take_notes(&mut self) -> Vec<SyncNote> {
        match self {
            Program::Followed { plan, .. } => std::mem::take(&mut plan.notes),
            _ => Vec::new(),
        }
    }

    pub(super) fn room(&mut self, m: RoomMsg<T>, deck: &mut Deck) {
        match m {
            RoomMsg::Attach {
                clock,
                mode,
                status,
                adopt,
            } => {
                let mut own = match std::mem::replace(self, Program::new()) {
                    Program::Own(o) | Program::Adopting { own: o, .. } => o,
                    Program::Room { room, own } => {
                        room.detach(deck);
                        own
                    }
                    Program::Followed { own, plan } => {
                        plan.leave(deck);
                        own
                    }
                };
                let crossfading = own.link == Some(Link::Crossfade)
                    && own.next.as_ref().is_some_and(|n| n.state == State::Given);
                if adopt.is_some() && crossfading {
                    // Your crossfade is set to begin: the room takes what plays once it has.
                    *self = Program::Adopting {
                        own,
                        calls: vec![RoomMsg::Attach {
                            clock,
                            mode,
                            status,
                            adopt,
                        }],
                    };
                    return;
                }
                let mut room = Box::new(RoomProgram::new(clock, mode, status));
                let sounding =
                    own.playing && own.cur.as_ref().is_some_and(|c| c.state == State::Sounding);
                match adopt {
                    Some(entry) if sounding => {
                        // The host's music goes on as the room's entry, untouched; your own
                        // queue waits paused at the place heard now.
                        let song = own.cur.take().expect("sounding");
                        let place = place_at(deck, &song, deck.heard_frame().unwrap_or(0));
                        own.drop_next(deck);
                        own.drop_spare(deck);
                        let mut lane = deck.lane(&song.item, place);
                        lane.rate_hint = song.lane.rate().unwrap_or(0);
                        own.cur = Some(Song {
                            lane,
                            item: song.item.clone(),
                            state: State::Waiting,
                        });
                        own.playing = false;
                        room.adopt(entry, song);
                    }
                    _ => {
                        // Set aside: what sounds fades out, and it waits paused where it was.
                        own.set_playing(false, deck);
                    }
                }
                *self = Program::Room { room, own };
            }
            RoomMsg::Detach => self.leave_room(deck),
            other => match self {
                Program::Room { room, .. } => room.message(other, deck),
                Program::Adopting { calls, .. } => calls.push(other),
                _ => {}
            },
        }
    }

    pub(super) fn note(&mut self, n: &RenderNote, deck: &mut Deck) {
        match self {
            Program::Own(o)
            | Program::Adopting { own: o, .. }
            | Program::Followed { own: o, .. } => o.note(n, deck),
            Program::Room { room, own } => {
                // The own program's last fade (as it was set aside) still ends here.
                own.note(n, deck);
                room.note(n, deck);
            }
        }
    }

    pub(super) fn step(&mut self, deck: &mut Deck) {
        match self {
            Program::Own(o) => o.step(deck),
            Program::Room { room, .. } => room.step(deck),
            Program::Followed { own, plan } => plan.step(own, deck),
            Program::Adopting { own, .. } => {
                own.step(deck);
                if own.link.is_none()
                    && let Program::Adopting { own, calls } =
                        std::mem::replace(self, Program::new())
                {
                    *self = Program::Own(own);
                    for c in calls {
                        self.room(c, deck);
                    }
                }
            }
        }
    }

    pub(super) fn stopped(&mut self, ev: Stopped, deck: &mut Deck) {
        match self {
            Program::Own(o)
            | Program::Adopting { own: o, .. }
            | Program::Followed { own: o, .. } => o.stopped(&ev, deck),
            Program::Room { room, own } => {
                own.stopped(&ev, deck);
                room.stopped(&ev, deck);
            }
        }
    }

    /// What sounds fades out and comes back at its place (an output switch).
    pub(super) fn hush(&mut self, deck: &mut Deck) {
        match self {
            Program::Own(o)
            | Program::Adopting { own: o, .. }
            | Program::Followed { own: o, .. } => o.hush(deck),
            Program::Room { room, .. } => room.hush(deck),
        }
    }

    /// The sleep timer's end: you pause (in a room: you step out).
    pub(super) fn sleep_ended(&mut self, deck: &mut Deck) {
        match self {
            Program::Room { room, .. } => room.message(RoomMsg::StepOut, deck),
            _ => self.own().set_playing(false, deck),
        }
    }

    pub(super) fn quit(&mut self, deck: &mut Deck) {
        self.leave_room(deck);
        self.sync(None, deck);
        self.own().set_playing(false, deck);
    }

    /// The room program dropped whole; your own comes back paused where it was.
    fn leave_room(&mut self, deck: &mut Deck) {
        match std::mem::replace(self, Program::new()) {
            Program::Room { room, mut own } => {
                room.detach(deck);
                own.playing = false;
                *self = Program::Own(own);
            }
            Program::Adopting { own, .. } => *self = Program::Own(own),
            other => *self = other,
        }
    }

    pub(super) fn view(&mut self, deck: &Deck, v: &mut View<T>) {
        match self {
            Program::Own(o) | Program::Adopting { own: o, .. } => {
                o.view(deck, v);
                v.sync = None;
            }
            Program::Room { room, own } => {
                own.view(deck, v);
                v.sync = Some(room.sync_status());
            }
            Program::Followed { own, plan } => {
                own.view(deck, v);
                v.sync = Some(plan.sync_status(own, deck));
            }
        }
    }
}
