//! Engine 2's programs (section 4.6 of the Listen together design): what the player plays
//! from. `OwnProgram` is your own queue; `RoomProgram` is a room, made at attach and dropped
//! whole at detach. They are never mixed: while a room plays, your own program is set aside
//! (its voices dropped, its queue and place kept) and comes back paused where it was.
//!
//! A program decides what plays and turns it into render commands through the `Deck`; it
//! learns what happened from the renderer's notes. Nothing here waits for audio.

use super::control::{Deck, Lane, Msg, Place, Retire, Stopped};
use super::status::View;
use super::{Cmd, Item, Signal, SyncStatus};
use crate::output::{self, Mode};
use crate::queue::Queue;
use crate::render::VoiceState;
use crate::voice::Phase;
use crate::voice_api::{RenderNote, Splice, VoiceId};
use serde::Serialize;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use together::align::Alignment;
use together::anchor::{Anchor, EntryId, PrepId, Transition};
use together::clock_map::ClockMapHandle;
use together::room_player::{RoomHandle, RoomPlayStatus, SubState, SyncMode};

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
        if matches!(c.state, State::Sounding | State::Stopping(_))
            && let (Some(content), Some(f)) = (deck.heard_content(c.vid()), c.lane.status().format)
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
        let (name, mode, out, latency_ms) = deck.output_signal()?;
        let (mut steps, active) = deck.steps(&c.lane.gain, s.loudness_db, out.rate, src.channels);
        let fading = self.link == Some(Link::Crossfade);
        if fading {
            steps.push(format!(
                "Crossfading into the next song ({} s)",
                deck.sound.crossfade_s
            ));
        }
        if mode == Mode::Shared {
            steps.push("Mixed by Windows (shared mode), which may resample".into());
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
        let rate = src.rate.max(1) as f32;
        Some(Signal {
            device: name,
            mode,
            file: s.info.clone(),
            buffer_ms: (s.in_ring as f32 * 1000.0 / rate) as u32,
            latency_ms,
            bit_perfect: !active && !fading && output::bit_perfect(&src, &out, mode, deck.volume),
            source: src,
            output: out,
            steps,
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
    },
    Prepare {
        prep: PrepId,
        entry: EntryId,
        item: T,
        room_content_us: i64,
        align: Alignment,
    },
    // Kept for WP5, which switches to it at its boundary.
    #[allow(dead_code)]
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
    let status = Arc::new(Mutex::new(RoomPlayStatus::default()));
    let _ = tx.send(Msg::Room(RoomMsg::Attach {
        clock,
        mode,
        status: status.clone(),
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

/// A song the room asked to have ready.
struct Prepared<T> {
    prep: PrepId,
    entry: EntryId,
    /// The room's content time it was asked for, and my copy's offset from it.
    content_us: i64,
    offset_us: i64,
    song: Song<T>,
}

/// The room's song playing.
struct Playing<T> {
    entry: EntryId,
    offset_us: i64,
    song: Song<T>,
}

/// A room's program. WP4's part: prepare, commit, hold, step out and rejoin, at the device
/// frames the clock map and the presentation pair give. The sync tick, the next song switched
/// at its boundary, suspensions and modes are WP5's.
pub(super) struct RoomProgram<T> {
    clock: ClockMapHandle,
    mode: SyncMode,
    extra_us: i32,
    status: Arc<Mutex<RoomPlayStatus>>,
    prepared: Option<Prepared<T>>,
    playing: Option<Playing<T>>,
    /// The newest anchor, and the commit waiting for its prepared song to be ready.
    anchor: Option<Anchor>,
    commit: Option<Anchor>,
    /// The commit is a rejoin (300 ms fade-in).
    rejoining: bool,
    stepped_out: bool,
}

impl<T: Item + Serialize> RoomProgram<T> {
    fn new(clock: ClockMapHandle, mode: SyncMode, status: Arc<Mutex<RoomPlayStatus>>) -> Self {
        RoomProgram {
            clock,
            mode,
            extra_us: 0,
            status,
            prepared: None,
            playing: None,
            anchor: None,
            commit: None,
            rejoining: false,
            stepped_out: false,
        }
    }

    /// The device frame that must be at the DAC at the host's time `host_us`, so it is heard
    /// then (the output's extra delay taken off).
    fn frame_for(&self, deck: &Deck, host_us: i64) -> Option<u64> {
        let map = (**self.clock.load())?;
        let local = map.local_of(host_us) as i64 - i64::from(self.extra_us) * 1000;
        deck.frame_at(local.max(0) as u64)
    }

    /// The host's time now, by the clock map.
    fn host_now(&self, deck: &Deck) -> Option<i64> {
        let map = (**self.clock.load())?;
        Some(map.host_of(deck.now_ns()))
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
                if let Some(p) = self.prepared.take() {
                    retire(deck, p.song);
                }
                let mine = (room_content_us + align.offset_us).max(0) as u64;
                let lane = deck.lane(&item, Place::Us(mine));
                self.prepared = Some(Prepared {
                    prep,
                    entry,
                    content_us: room_content_us,
                    offset_us: align.offset_us,
                    song: Song {
                        lane,
                        item,
                        state: State::Waiting,
                    },
                });
            }
            // The switch at the boundary is WP5's.
            RoomMsg::ArmNext { .. } | RoomMsg::Disarm => {}
            RoomMsg::Commit(a) => {
                self.anchor = Some(a);
                if !a.playing {
                    self.hold(a, deck);
                } else if self.prepared.as_ref().is_some_and(|p| p.entry == a.entry) {
                    self.commit = Some(a);
                    self.rejoining = false;
                }
            }
            RoomMsg::Hold(a) => {
                self.anchor = Some(a);
                self.hold(a, deck);
            }
            RoomMsg::SetMode(m) => self.mode = m,
            RoomMsg::SetExtraLatency(us) => self.extra_us = us,
            RoomMsg::StepOut => {
                self.stepped_out = true;
                if let Some(p) = self.playing.as_mut()
                    && p.song.state == State::Sounding
                {
                    let at = deck.soon();
                    deck.stop_at(p.song.vid(), at, 30);
                    p.song.state = State::Stopping(at);
                }
            }
            RoomMsg::Rejoin => self.rejoin(deck),
            RoomMsg::Attach { .. } | RoomMsg::Detach => {}
        }
    }

    fn hold(&mut self, a: Anchor, deck: &mut Deck) {
        self.commit = None;
        let at = self
            .frame_for(deck, a.host_us)
            .unwrap_or(0)
            .max(deck.soon());
        if let Some(p) = self.playing.as_mut()
            && matches!(p.song.state, State::Given | State::Sounding)
        {
            deck.stop_at(p.song.vid(), at, 30);
            p.song.state = State::Stopping(at);
        }
    }

    /// Back in at the room's place a second from now, fading in over 300 ms.
    fn rejoin(&mut self, deck: &mut Deck) {
        let Some(a) = self.anchor.filter(|a| a.playing) else {
            return;
        };
        let known = self
            .playing
            .as_ref()
            .map(|p| (p.song.item.clone(), p.offset_us))
            .or_else(|| {
                self.prepared
                    .as_ref()
                    .map(|p| (p.song.item.clone(), p.offset_us))
            });
        let (Some((item, offset_us)), Some(host)) = (known, self.host_now(deck)) else {
            return;
        };
        let content = a.content_at(host + 1_000_000);
        self.message(
            RoomMsg::Prepare {
                prep: PrepId(u64::MAX),
                entry: a.entry,
                item,
                room_content_us: content,
                align: Alignment {
                    offset_us,
                    ..Alignment::APPROXIMATE
                },
            },
            deck,
        );
        self.commit = Some(a);
        self.rejoining = true;
        self.stepped_out = false;
    }

    fn step(&mut self, deck: &mut Deck) {
        self.commit_step(deck);
        let ready = self
            .prepared
            .as_ref()
            .filter(|p| p.song.lane.ready() && p.song.lane.failed().is_none())
            .map(|p| p.prep);
        let cannot = self
            .prepared
            .as_ref()
            .is_some_and(|p| p.song.lane.failed().is_some());
        let sounding = self
            .playing
            .as_ref()
            .is_some_and(|p| p.song.state == State::Sounding);
        let mut st = self.status.lock().unwrap_or_else(|p| p.into_inner());
        st.ready = ready;
        st.entry = self.playing.as_ref().map(|p| p.entry);
        st.bound_us = (**self.clock.load()).map(|m| m.bound_us);
        st.ppm = 0.0;
        st.sub_state = if self.stepped_out {
            SubState::SteppedOut
        } else if cannot {
            SubState::CannotPlay
        } else if sounding {
            SubState::InStep
        } else if ready.is_some() {
            SubState::Ready
        } else {
            SubState::Opening
        };
    }

    /// A commit whose song is ready: started (or switched to) on the frame its anchor says.
    fn commit_step(&mut self, deck: &mut Deck) {
        let Some(a) = self.commit else {
            return;
        };
        let Some(p) = self.prepared.as_ref() else {
            self.commit = None;
            return;
        };
        if !p.song.lane.ready() || p.song.lane.failed().is_some() {
            return;
        }
        let Some(f) = p.song.lane.status().format else {
            return;
        };
        if !deck.ready_for(&f) {
            return;
        }
        let Some(frame) = self.frame_for(deck, a.host_us) else {
            return;
        };
        // The prepared place is the anchor's content plus `d`: it is heard `d` later.
        let d_us = p.content_us - a.content_us;
        let at = (frame as i64 + d_us * i64::from(deck.rate()) / 1_000_000).max(0) as u64;
        let mut p = self.prepared.take().expect("prepared");
        let start = p.song.lane.start().unwrap_or(0);
        let given = match self.playing.take() {
            Some(old) if old.song.state == State::Sounding => {
                let how = if old.entry == p.entry {
                    Splice::Crossfade {
                        frames: deck.ms(40) as u32,
                    }
                } else {
                    Splice::FadeOutThenIn {
                        out_frames: deck.ms(30) as u32,
                    }
                };
                let ok = deck.switch(old.song.vid(), &mut p.song.lane, at, how);
                deck.retire(old.song.lane, Retire::AfterFade);
                ok
            }
            Some(old) => {
                retire(deck, old.song);
                self.start_from_silence(deck, &mut p.song.lane, at, start)
            }
            None => self.start_from_silence(deck, &mut p.song.lane, at, start),
        };
        if given {
            p.song.state = State::Given;
        }
        self.playing = Some(Playing {
            entry: p.entry,
            offset_us: p.offset_us,
            song: p.song,
        });
        self.commit = None;
        self.rejoining = false;
    }

    fn start_from_silence(&self, deck: &mut Deck, lane: &mut Lane, at: u64, start: u64) -> bool {
        let fade = if self.rejoining {
            deck.ms(300)
        } else if start > 0 {
            deck.ms(15)
        } else {
            0
        };
        deck.start(lane, at, fade as u32)
    }

    fn note(&mut self, n: &RenderNote, deck: &mut Deck) {
        let Some(p) = self.playing.as_mut() else {
            return;
        };
        let vid = p.song.vid();
        match *n {
            RenderNote::Started { voice, .. } if voice == vid => p.song.state = State::Sounding,
            RenderNote::Ended { voice, .. } if voice == vid => {
                if let Some(p) = self.playing.take() {
                    deck.retire(p.song.lane, Retire::Now);
                }
            }
            RenderNote::Silent { voice, .. }
                if voice == vid && matches!(p.song.state, State::Stopping(_)) =>
            {
                if let Some(p) = self.playing.take() {
                    deck.retire(p.song.lane, Retire::Now);
                }
            }
            _ => {}
        }
    }

    fn stopped(&mut self, ev: &Stopped, deck: &mut Deck) {
        if self
            .playing
            .as_ref()
            .is_some_and(|p| p.song.lane.given == Some(ev.generation))
            && let Some(p) = self.playing.take()
        {
            deck.retire(p.song.lane, Retire::Now);
        }
    }

    /// The room lets go: what sounds fades out over a second, on the deck's own.
    fn detach(self, deck: &mut Deck) {
        if let Some(p) = self.prepared {
            retire(deck, p.song);
        }
        if let Some(p) = self.playing {
            match p.song.state {
                State::Sounding | State::Given => {
                    let at = deck.soon();
                    deck.stop_at(p.song.vid(), at, 1_000);
                    deck.retire(p.song.lane, Retire::AfterFade);
                }
                _ => retire(deck, p.song),
            }
        }
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

// --- Which program plays -----------------------------------------------------------------------

pub(super) enum Program<T> {
    Own(OwnProgram<T>),
    /// A room plays; your own program waits, set aside (no voice of its own).
    Room {
        room: Box<RoomProgram<T>>,
        own: OwnProgram<T>,
    },
}

impl<T: Item + Serialize> Program<T> {
    pub(super) fn new() -> Self {
        Program::Own(OwnProgram::new())
    }

    fn own(&mut self) -> &mut OwnProgram<T> {
        match self {
            Program::Own(o) | Program::Room { own: o, .. } => o,
        }
    }

    pub(super) fn busy(&self) -> bool {
        match self {
            Program::Own(o) => o.busy(),
            Program::Room { .. } => true,
        }
    }

    pub(super) fn wants_output(&self) -> bool {
        match self {
            Program::Own(o) => o.wants_output(),
            Program::Room { .. } => true,
        }
    }

    /// Your queue's commands go to your own program, in a room too (it plays nothing there:
    /// it comes back paused).
    pub(super) fn command(&mut self, cmd: Cmd<T>, deck: &mut Deck) {
        self.own().command(cmd, deck);
    }

    pub(super) fn room(&mut self, m: RoomMsg<T>, deck: &mut Deck) {
        match m {
            RoomMsg::Attach {
                clock,
                mode,
                status,
            } => {
                let mut own = match std::mem::replace(self, Program::new()) {
                    Program::Own(o) => o,
                    Program::Room { room, own } => {
                        room.detach(deck);
                        own
                    }
                };
                // Set aside: what sounds fades out, and it waits paused where it was.
                own.set_playing(false, deck);
                *self = Program::Room {
                    room: Box::new(RoomProgram::new(clock, mode, status)),
                    own,
                };
            }
            RoomMsg::Detach => self.leave_room(deck),
            other => {
                if let Program::Room { room, .. } = self {
                    room.message(other, deck);
                }
            }
        }
    }

    pub(super) fn note(&mut self, n: &RenderNote, deck: &mut Deck) {
        match self {
            Program::Own(o) => o.note(n, deck),
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
        }
    }

    pub(super) fn stopped(&mut self, ev: Stopped, deck: &mut Deck) {
        match self {
            Program::Own(o) => o.stopped(&ev, deck),
            Program::Room { room, own } => {
                own.stopped(&ev, deck);
                room.stopped(&ev, deck);
            }
        }
    }

    /// What sounds fades out and comes back at its place (an output switch).
    pub(super) fn hush(&mut self, deck: &mut Deck) {
        match self {
            Program::Own(o) => o.hush(deck),
            // The room's voice comes back by the room's rules (WP5): here it lets go.
            Program::Room { room, .. } => {
                if let Some(p) = room.playing.take() {
                    retire(deck, p.song);
                }
            }
        }
    }

    /// The sleep timer's end: you pause (in a room: you step out).
    pub(super) fn sleep_ended(&mut self, deck: &mut Deck) {
        match self {
            Program::Own(o) => o.set_playing(false, deck),
            Program::Room { room, .. } => room.message(RoomMsg::StepOut, deck),
        }
    }

    pub(super) fn quit(&mut self, deck: &mut Deck) {
        self.leave_room(deck);
        self.own().set_playing(false, deck);
    }

    /// The room program dropped whole; your own comes back paused where it was.
    fn leave_room(&mut self, deck: &mut Deck) {
        if !matches!(self, Program::Room { .. }) {
            return;
        }
        if let Program::Room { room, mut own } = std::mem::replace(self, Program::new()) {
            room.detach(deck);
            own.playing = false;
            *self = Program::Own(own);
        }
    }

    pub(super) fn view(&mut self, deck: &Deck, v: &mut View<T>) {
        match self {
            Program::Own(o) => {
                o.view(deck, v);
                v.sync = None;
            }
            Program::Room { room, own } => {
                own.view(deck, v);
                v.sync = Some(room.sync_status());
            }
        }
    }
}
