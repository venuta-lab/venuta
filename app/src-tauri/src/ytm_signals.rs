//! What Venuta tells YouTube Music about what you play, each behind the account's own switch
//! (`engine::settings::YtmChoices`):
//!
//! - Count plays: an online song heard for 30 seconds (most of a shorter one) is told as a play,
//!   which puts it in the account's history.
//! - How much I listened (with Count plays): how much of each online song was heard, the way
//!   YouTube's own player tells it: after ten seconds, then every forty, and once more when the
//!   song ends, is skipped or replaced (`listening::Tracker` decides when). The play and its
//!   listening time are one listen there: one player answer, one playback nonce.
//!
//! A play goes to YouTube Music's own player, asked as its web page asks (with YouTube's
//! security check, a PO token made in a hidden window: `ytm_botguard`), so it lands on its
//! History page. When that player gives nowhere to tell it, the YouTube web client is asked
//! instead (the play then counts in the YouTube watch history), and the playback log says so and
//! why.
//!
//! Never your files (the tracker tells only online songs), never in Local only, never signed out
//! or when it can't be told which account is in use, and only while the switch is on for that
//! account. One thread sends it all, a request at a time with a pause between, at most
//! [`MAX_PER_HOUR`]; a request that fails is dropped, never sent again.

use crate::listening::{Listened, Reporter, Told};
use engine::settings::YtmChoices;
use media_access::{AccountAccess, CancellationToken, Heard, Listen, MediaAccess, PlayCounts};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// At most this many requests to the account in an hour, everything told counted together.
pub const MAX_PER_HOUR: usize = 360;
/// At least this long between two requests.
const GAP: Duration = Duration::from_secs(1);
/// A listen whose listening time fails this often is told nothing more of it.
const FAILURES: u32 = 2;

/// What may be told now, for the account in use.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Allowed {
    /// Which account and channel (`email#channel`).
    account: String,
    plays: bool,
    time: bool,
}

/// What the account's switches allow: None in Local only, or when it can't be told which
/// account is in use (nothing is sent on a guess).
pub(crate) fn allowed(
    account: Option<String>,
    choices: YtmChoices,
    local_only: bool,
) -> Option<Allowed> {
    if local_only {
        return None;
    }
    let choices = choices.settled();
    Some(Allowed {
        account: account?,
        plays: choices.count_plays,
        time: choices.count_plays && choices.listening_time,
    })
}

/// Where what is told goes: the account (or a test's stand-in).
pub(crate) trait Tell {
    type Listen;
    /// Ask how one listen of a song is tracked (nothing is told by asking).
    fn start_listen(&self, video_id: &str) -> Result<Self::Listen, String>;
    /// A play, on its own (Count plays without How much I listened).
    fn play(&self, video_id: &str) -> Result<(), String>;
    /// A play, as part of its listen.
    fn listen_play(&self, listen: &Self::Listen) -> Result<(), String>;
    /// How much of the listen was heard since the last report.
    fn heard(&self, listen: &Self::Listen, heard: &Listened) -> Result<(), String>;
}

/// The listen being told: which play, of which song, as which account.
struct Session<L> {
    play: u64,
    video_id: String,
    account: String,
    listen: Option<L>,
    asked: bool,
    failures: u32,
}

/// Everything told goes through here, one request at a time.
pub(crate) struct Signals<L> {
    session: Option<Session<L>>,
    sent: VecDeque<Instant>,
    gap: Duration,
}

impl<L> Signals<L> {
    pub(crate) fn new(gap: Duration) -> Self {
        Self {
            session: None,
            sent: VecDeque::new(),
            gap,
        }
    }

    /// Room for one more request: waits out the pause after the last one; false when the hour's
    /// share is used up (then it is dropped).
    fn pace(&mut self) -> bool {
        let now = Instant::now();
        while self
            .sent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(3600))
        {
            self.sent.pop_front();
        }
        if self.sent.len() >= MAX_PER_HOUR {
            return false;
        }
        if let Some(last) = self.sent.back() {
            let since = now.duration_since(*last);
            if since < self.gap {
                std::thread::sleep(self.gap - since);
            }
        }
        self.sent.push_back(Instant::now());
        true
    }

    /// The session for this play, made afresh for another play, song or account; its listen
    /// asked for once.
    fn session_for<T: Tell<Listen = L>>(
        &mut self,
        tell: &T,
        play: u64,
        video_id: &str,
        account: &str,
    ) -> Option<&mut Session<L>> {
        let same = self
            .session
            .as_ref()
            .is_some_and(|s| s.play == play && s.video_id == video_id && s.account == account);
        if !same {
            self.session = Some(Session {
                play,
                video_id: video_id.to_owned(),
                account: account.to_owned(),
                listen: None,
                asked: false,
                failures: 0,
            });
        }
        if self.session.as_ref().is_some_and(|s| !s.asked) {
            let paced = self.pace();
            let s = self.session.as_mut()?;
            s.asked = true;
            if paced {
                match tell.start_listen(video_id) {
                    Ok(l) => s.listen = Some(l),
                    Err(_) => s.failures += 1,
                }
            }
        }
        self.session.as_mut()
    }

    /// Tell what the tracker has, if the account's switches allow it now.
    pub(crate) fn handle<T: Tell<Listen = L>>(
        &mut self,
        tell: &T,
        told: Told,
        now: Option<&Allowed>,
    ) {
        let Some(now) = now else {
            self.session = None;
            return;
        };
        match told {
            Told::Play { video_id, play } => {
                if !now.plays {
                    return;
                }
                if !now.time {
                    if self.pace() {
                        let _ = tell.play(&video_id);
                    }
                    return;
                }
                let has_listen = self
                    .session_for(tell, play, &video_id, &now.account)
                    .is_some_and(|s| s.listen.is_some());
                if has_listen && self.pace() {
                    let s = self.session.as_ref().and_then(|s| s.listen.as_ref());
                    if let Some(listen) = s {
                        let _ = tell.listen_play(listen);
                    }
                }
            }
            Told::Heard(heard) => {
                if !now.time {
                    return;
                }
                let live = self
                    .session_for(tell, heard.play, &heard.video_id, &now.account)
                    .is_some_and(|s| s.listen.is_some() && s.failures < FAILURES);
                if live && self.pace() {
                    let failed = self
                        .session
                        .as_ref()
                        .and_then(|s| s.listen.as_ref())
                        .is_some_and(|listen| tell.heard(listen, &heard).is_err());
                    if failed && let Some(s) = self.session.as_mut() {
                        s.failures += 1;
                    }
                }
                if heard.last {
                    self.session = None;
                }
            }
        }
    }
}

/// The account in use, as what is told goes to it.
struct Account(Arc<dyn AccountAccess>);

/// What telling the account came to, as the playback log says it: where it went (YouTube
/// Music's own player, or the YouTube web client when that gave nowhere to tell it), and why
/// when it wasn't YouTube Music's with its token. `counts`: None when no listen was had at all.
/// No addresses, no song ids.
fn told_line(what: &str, counts: Option<&PlayCounts>, told: &Result<(), String>) -> String {
    let (to, how) = match counts {
        None | Some(PlayCounts::YouTubeMusic { no_token: None }) => {
            ("YouTube Music", String::new())
        }
        Some(PlayCounts::YouTubeMusic {
            no_token: Some(why),
        }) => ("YouTube Music", format!(" (no token: {why})")),
        Some(PlayCounts::YouTube { why }) => (
            "YouTube",
            format!(" through the YouTube web client ({why})"),
        ),
    };
    match told {
        Ok(()) => format!("told {to} {what}{how}"),
        Err(e) => format!("couldn't tell {to} {what}{how}: {e}"),
    }
}

/// [`told_line`], in the playback log.
fn said(what: &str, counts: Option<&PlayCounts>, told: &Result<(), String>) {
    media_access::note_timing(&told_line(what, counts, told));
}

impl Tell for Account {
    type Listen = Listen;

    fn start_listen(&self, video_id: &str) -> Result<Listen, String> {
        self.0
            .start_listen(video_id, &CancellationToken::default())
            .map_err(|e| e.message)
    }

    /// The play on its own: its listen asked for and told at once, so the log can say where it
    /// went.
    fn play(&self, video_id: &str) -> Result<(), String> {
        let listen = match self.start_listen(video_id) {
            Ok(listen) => listen,
            Err(e) => {
                let told = Err(e);
                said("a play", None, &told);
                return told;
            }
        };
        let told = self
            .0
            .report_listen_play(&listen, &CancellationToken::default())
            .map_err(|e| e.message);
        said("a play", Some(&listen.counts), &told);
        told
    }

    fn listen_play(&self, listen: &Listen) -> Result<(), String> {
        let told = self
            .0
            .report_listen_play(listen, &CancellationToken::default())
            .map_err(|e| e.message);
        said("a play", Some(&listen.counts), &told);
        told
    }

    fn heard(&self, listen: &Listen, heard: &Listened) -> Result<(), String> {
        let heard = Heard {
            segments: heard.segments.clone(),
            at_ms: heard.at_ms,
            length_ms: heard.length_ms,
            playing: heard.playing,
        };
        self.0
            .report_heard(listen, &heard, &CancellationToken::default())
            .map_err(|e| e.message)
    }
}

/// The tracker's reporter: what it tells goes to one thread of its own, which tells the account
/// in use what its switches allow, off the listening thread so a slow answer never holds a write.
pub fn reporter(catalog: Arc<dyn MediaAccess>, local_only: Arc<AtomicBool>) -> Reporter {
    let (tx, rx) = std::sync::mpsc::channel::<Told>();
    let _ = std::thread::Builder::new()
        .name("venuta-ytm-signals".into())
        .spawn(move || {
            let mut signals = Signals::<Listen>::new(GAP);
            for told in rx {
                let offline = local_only.load(Ordering::Relaxed);
                let account = (!offline)
                    .then(|| catalog.account_access())
                    .flatten()
                    .filter(|a| a.signed_in());
                let is_play = matches!(told, Told::Play { .. });
                let Some(account) = account else {
                    if is_play {
                        media_access::note_timing(
                            "a play was not told: no YouTube Music account in use",
                        );
                    }
                    signals.handle(&Nobody, told, None);
                    continue;
                };
                let in_use = crate::ytm_choices::choices_in_use(catalog.as_ref());
                if is_play && in_use.is_none() {
                    media_access::note_timing(
                        "a play was not told: which account is in use isn't known",
                    );
                }
                let now = match in_use {
                    Some((key, choices)) => allowed(Some(key), choices, offline),
                    None => None,
                };
                if is_play && now.as_ref().is_some_and(|n| !n.plays) {
                    media_access::note_timing("a play was not told: Count plays is off");
                }
                signals.handle(&Account(account), told, now.as_ref());
            }
        });
    Box::new(move |told| {
        let _ = tx.send(told);
    })
}

/// No account: nothing is told (see `Signals::handle` with nothing allowed).
struct Nobody;

impl Tell for Nobody {
    type Listen = Listen;
    fn start_listen(&self, _: &str) -> Result<Listen, String> {
        Err("signed out".into())
    }
    fn play(&self, _: &str) -> Result<(), String> {
        Err("signed out".into())
    }
    fn listen_play(&self, _: &Listen) -> Result<(), String> {
        Err("signed out".into())
    }
    fn heard(&self, _: &Listen, _: &Listened) -> Result<(), String> {
        Err("signed out".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Keeps what would have been sent; `fail` makes every request fail.
    #[derive(Default)]
    struct Kept {
        sent: Mutex<Vec<String>>,
        fail: bool,
    }

    impl Kept {
        fn sent(&self) -> Vec<String> {
            self.sent.lock().unwrap().clone()
        }
        fn note(&self, what: String) -> Result<(), String> {
            self.sent.lock().unwrap().push(what);
            if self.fail {
                Err("refused".into())
            } else {
                Ok(())
            }
        }
    }

    impl Tell for Kept {
        type Listen = String;
        fn start_listen(&self, video_id: &str) -> Result<String, String> {
            self.note(format!("listen {video_id}"))?;
            Ok(format!("cpn-{video_id}"))
        }
        fn play(&self, video_id: &str) -> Result<(), String> {
            self.note(format!("play {video_id}"))
        }
        fn listen_play(&self, listen: &String) -> Result<(), String> {
            self.note(format!("play via {listen}"))
        }
        fn heard(&self, listen: &String, heard: &Listened) -> Result<(), String> {
            self.note(format!("heard via {listen} {:?}", heard.segments))
        }
    }

    fn heard(play: u64, id: &str, segments: Vec<(u64, u64)>, last: bool) -> Told {
        Told::Heard(Listened {
            video_id: id.into(),
            play,
            segments,
            at_ms: 0,
            length_ms: 200_000,
            playing: true,
            last,
        })
    }

    fn play(play: u64, id: &str) -> Told {
        Told::Play {
            video_id: id.into(),
            play,
        }
    }

    fn on(plays: bool, time: bool) -> Option<Allowed> {
        allowed(
            Some("me@example.com#own".into()),
            YtmChoices {
                count_plays: plays,
                listening_time: time,
                ..Default::default()
            },
            false,
        )
    }

    fn run(told: Vec<Told>, now: Option<Allowed>, kept: &Kept) {
        let mut s = Signals::new(Duration::ZERO);
        for t in told {
            s.handle(kept, t, now.as_ref());
        }
    }

    fn a_listen() -> Vec<Told> {
        vec![
            heard(1, "vid1", vec![(0, 10_000)], false),
            play(1, "vid1"),
            heard(1, "vid1", vec![(10_000, 50_000)], false),
            heard(1, "vid1", vec![(50_000, 62_000)], true),
        ]
    }

    // Both switches on: one listen asked for, its play and every report through it.
    #[test]
    fn a_listen_is_one_session_with_its_play() {
        let kept = Kept::default();
        run(a_listen(), on(true, true), &kept);
        assert_eq!(
            kept.sent(),
            [
                "listen vid1",
                "heard via cpn-vid1 [(0, 10000)]",
                "play via cpn-vid1",
                "heard via cpn-vid1 [(10000, 50000)]",
                "heard via cpn-vid1 [(50000, 62000)]",
            ]
        );
    }

    // Count plays alone: the play as today, and nothing of how much was heard.
    #[test]
    fn count_plays_alone_tells_only_the_play() {
        let kept = Kept::default();
        run(a_listen(), on(true, false), &kept);
        assert_eq!(kept.sent(), ["play vid1"]);
    }

    // Off, in Local only, or with the account unknown: nothing at all.
    #[test]
    fn switched_off_nothing_is_told() {
        for now in [
            on(false, false),
            on(false, true),
            allowed(None, on_choices(), false),
            allowed(Some("a#own".into()), on_choices(), true),
        ] {
            let kept = Kept::default();
            run(a_listen(), now.clone(), &kept);
            assert!(kept.sent().is_empty(), "{now:?}");
        }
    }

    fn on_choices() -> YtmChoices {
        YtmChoices {
            count_plays: true,
            listening_time: true,
            ..Default::default()
        }
    }

    // Switched off partway: what comes after is not told, and the listen is let go.
    #[test]
    fn switching_off_stops_it_at_once() {
        let kept = Kept::default();
        let mut s = Signals::new(Duration::ZERO);
        let told = a_listen();
        s.handle(&kept, told[0].clone(), on(true, true).as_ref());
        s.handle(&kept, told[1].clone(), on(true, true).as_ref());
        s.handle(&kept, told[2].clone(), on(true, false).as_ref());
        s.handle(&kept, told[3].clone(), None);
        assert_eq!(
            kept.sent(),
            [
                "listen vid1",
                "heard via cpn-vid1 [(0, 10000)]",
                "play via cpn-vid1"
            ]
        );
    }

    // A new song (or the same song played again) is a new listen; another account too.
    #[test]
    fn each_play_and_account_is_its_own_listen() {
        let kept = Kept::default();
        let mut s = Signals::new(Duration::ZERO);
        let now = on(true, true);
        s.handle(
            &kept,
            heard(1, "vid1", vec![(0, 10_000)], false),
            now.as_ref(),
        );
        s.handle(
            &kept,
            heard(2, "vid1", vec![(0, 10_000)], false),
            now.as_ref(),
        );
        let other = allowed(Some("other@example.com#own".into()), on_choices(), false);
        s.handle(
            &kept,
            heard(2, "vid1", vec![(10_000, 50_000)], false),
            other.as_ref(),
        );
        let listens = kept
            .sent()
            .iter()
            .filter(|x| x.starts_with("listen"))
            .count();
        assert_eq!(listens, 3);
    }

    // A refusal is never sent again: no second ask for the listen, and after two failed reports
    // the listen is told nothing more.
    #[test]
    fn failures_are_dropped_not_retried() {
        let kept = Kept {
            fail: true,
            ..Default::default()
        };
        run(a_listen(), on(true, true), &kept);
        assert_eq!(
            kept.sent(),
            ["listen vid1"],
            "the listen failed: nothing more"
        );

        // The listen came, its reports fail: two tries, then quiet.
        struct Flaky(Kept);
        impl Tell for Flaky {
            type Listen = String;
            fn start_listen(&self, video_id: &str) -> Result<String, String> {
                let _ = self.0.note(format!("listen {video_id}"));
                Ok("cpn".into())
            }
            fn play(&self, v: &str) -> Result<(), String> {
                self.0.note(format!("play {v}"))
            }
            fn listen_play(&self, l: &String) -> Result<(), String> {
                self.0.note(format!("play via {l}"))
            }
            fn heard(&self, l: &String, _: &Listened) -> Result<(), String> {
                self.0.note(format!("heard via {l}"))
            }
        }
        let flaky = Flaky(Kept {
            fail: true,
            ..Default::default()
        });
        let mut s = Signals::new(Duration::ZERO);
        for i in 0..6 {
            s.handle(
                &flaky,
                heard(1, "vid1", vec![(i * 1000, i * 1000 + 1000)], false),
                on(true, true).as_ref(),
            );
        }
        let reports = flaky
            .0
            .sent()
            .iter()
            .filter(|x| x.starts_with("heard"))
            .count();
        assert_eq!(reports, FAILURES as usize);
    }

    // The playback log says where a play went, and why when it wasn't YouTube Music's own
    // player with its token: no ids, no addresses.
    #[test]
    fn the_log_says_where_a_play_went() {
        use media_access::PlayCounts;
        let music = PlayCounts::YouTubeMusic { no_token: None };
        let tokenless = PlayCounts::YouTubeMusic {
            no_token: Some("BotGuard didn't run".into()),
        };
        let web = PlayCounts::YouTube {
            why: "no token: BotGuard didn't run".into(),
        };
        let refused: Result<(), String> = Err("refused".into());
        assert_eq!(
            told_line("a play", Some(&music), &Ok(())),
            "told YouTube Music a play"
        );
        assert_eq!(
            told_line("a play", Some(&tokenless), &Ok(())),
            "told YouTube Music a play (no token: BotGuard didn't run)"
        );
        assert_eq!(
            told_line("a play", Some(&web), &Ok(())),
            "told YouTube a play through the YouTube web client (no token: BotGuard didn't run)"
        );
        assert_eq!(
            told_line("a play", None, &refused),
            "couldn't tell YouTube Music a play: refused"
        );
        assert_eq!(
            told_line("a play", Some(&web), &refused),
            "couldn't tell YouTube a play through the YouTube web client \
             (no token: BotGuard didn't run): refused"
        );
    }

    // Never more than the hour's share, however much is played.
    #[test]
    fn requests_are_capped_each_hour() {
        let kept = Kept::default();
        let mut s = Signals::new(Duration::ZERO);
        for i in 0..(MAX_PER_HOUR as u64 * 2) {
            s.handle(&kept, play(i, "vid"), on(true, false).as_ref());
        }
        assert_eq!(kept.sent().len(), MAX_PER_HOUR);
    }
}
