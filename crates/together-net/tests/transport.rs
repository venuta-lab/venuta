//! The transport's own gates (design 4.1, 7.5): the control stream ahead of file bytes, the
//! host's files paced, presses held while reconnecting only while they are fresh, the
//! reconnect keeping who you are, path changes told, and clock samples on datagrams.

mod common;
use common::*;

/// How fast each guest takes the host's file in the load test (bytes a second): far more than
/// any song needs (hi-res FLAC is about 1 MB/s), as a guest on a quick network would.
const PULL_RATE: f64 = 20e6;

/// The `p` quantile of some durations.
fn quantile(mut v: Vec<Duration>, p: f64) -> Duration {
    v.sort();
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// Round trips of pings on the control stream, one every 10 ms for `n`.
async fn round_trips(r: &mut Raw, first_id: u32, n: u32) -> Vec<Duration> {
    let mut out = Vec::new();
    for id in first_id..first_id + n {
        let t = Instant::now();
        let _ = r.until_pong(id).await;
        out.push(t.elapsed());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_messages_stay_quick_while_the_host_streams_to_three_guests() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.flac");
    std::fs::write(&path, counting(48 * 1024 * 1024)).unwrap();
    // A song short for its size: its pacing lets the whole file go at once, the most a host's
    // upload can be asked to carry.
    let mut song = host_song(&FILE_KEY["host:".len()..]);
    song.duration_ms = Some(1_000);
    let h = host(vec![song]).await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path, "audio/flac".into()));
    let mut ali = member(&h, 1, "Ali").await;
    let mut others = vec![member(&h, 2, "Ben").await, member(&h, 3, "Cy").await];
    let _ = ali.until_pong(0).await;
    let quiet = quantile(round_trips(&mut ali, 1, 30).await, 0.5);
    // Three guests pull the whole file, again and again, while Ali's pings go on.
    let streaming = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut pulls = Vec::new();
    let ali_conn = ali.conn.clone();
    for conn in std::iter::once(ali_conn).chain(others.iter().map(|r| r.conn.clone())) {
        let streaming = streaming.clone();
        let bytes = bytes.clone();
        pulls.push(tokio::spawn(async move {
            while streaming.load(std::sync::atomic::Ordering::Relaxed) {
                let (mut send, recv) = conn.open_bi().await.unwrap();
                let get = Get {
                    get: FILE_KEY.into(),
                    from: 0,
                    to: None,
                };
                net::send_line(&mut send, &proto::line(&get)).await.unwrap();
                send.finish().unwrap();
                let mut body = net::Lines::new(recv).into_inner();
                let mut buf = vec![0u8; 64 * 1024];
                let (start, mut got) = (Instant::now(), 0u64);
                loop {
                    match body.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            bytes.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
                            // As fast as a guest on a quick network takes it, 20 MB/s.
                            got += n as u64;
                            let due = Duration::from_secs_f64(got as f64 / PULL_RATE);
                            if let Some(wait) = due.checked_sub(start.elapsed()) {
                                tokio::time::sleep(wait).await;
                            }
                        }
                    }
                    if !streaming.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }));
    }
    until("the files flowing", || {
        bytes.load(std::sync::atomic::Ordering::Relaxed) > 4 * 1024 * 1024
    })
    .await;
    let busy = round_trips(&mut ali, 100, 100).await;
    let moved = bytes.load(std::sync::atomic::Ordering::Relaxed);
    streaming.store(false, std::sync::atomic::Ordering::Relaxed);
    for p in pulls {
        p.abort();
    }
    let p99 = quantile(busy, 0.99);
    // One way is half a round trip: under (quiet round trip) / 2 + 20 ms.
    assert!(
        p99 / 2 < quiet / 2 + Duration::from_millis(20),
        "control p99 round trip {p99:?} under load (quiet {quiet:?}, {} MB streamed)",
        moved / 1_000_000
    );
    for r in &mut others {
        let _ = r.until_pong(9).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn on_a_home_upload_the_rooms_messages_stay_quick_while_a_song_streams_paced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.flac");
    // A song of 160 s in 16 MiB (about 100 kB a second, like a CD-quality FLAC).
    std::fs::write(&path, counting(16 * 1024 * 1024)).unwrap();
    let mut song = host_song(&FILE_KEY["host:".len()..]);
    song.duration_ms = Some(160_000);
    let h = host_far(vec![song]).await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path, "audio/flac".into()));
    // A home upload: 2 MB/s, a router queue of 256 KB, 10 ms each way.
    let slow = link(host_addr(&h), 2e6, 256 * 1024, Duration::from_millis(10)).await;
    let through = invite_through(&h.invite, &[&slow]);
    let mut ali = Raw::dial_near(&through, key(1)).await;
    ali.say(&hello(&through, &key(1), "Ali")).await;
    assert_eq!(ali.hear().await, Some(ToGuest::Waiting));
    h.s.admit(1);
    let _ = ali.until_pong(0).await;
    let quiet = quantile(round_trips(&mut ali, 1, 20).await, 0.5);
    let conn = ali.conn.clone();
    let pull = tokio::spawn(async move {
        let (mut send, recv) = conn.open_bi().await.unwrap();
        let get = Get {
            get: FILE_KEY.into(),
            from: 0,
            to: None,
        };
        net::send_line(&mut send, &proto::line(&get)).await.unwrap();
        send.finish().unwrap();
        let mut body = net::Lines::new(recv).into_inner();
        let mut sink = Vec::new();
        let _ = body.read_to_end(&mut sink).await;
    });
    // The first burst (10 s of the song, 1 MB) fills the link for half a second; after it, the
    // file goes at twice the song's rate, a tenth of the link.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let busy = round_trips(&mut ali, 100, 150).await;
    assert!(!pull.is_finished(), "the song still streaming throughout");
    pull.abort();
    let p99 = quantile(busy, 0.99);
    assert!(
        p99 / 2 < quiet / 2 + Duration::from_millis(20),
        "one way p99 {:?}, quiet {:?}",
        p99 / 2,
        quiet / 2
    );
}

/// Ask the host for a range on a stream of its own, timing the bytes: when the first `mark`
/// bytes were in, and when all were.
async fn timed_get(r: &Raw, get: &Get, mark: usize) -> (usize, Duration, Duration) {
    let t = Instant::now();
    let (mut send, recv) = r.conn.open_bi().await.unwrap();
    net::send_line(&mut send, &proto::line(get)).await.unwrap();
    send.finish().unwrap();
    let mut lines = net::Lines::new(recv);
    let _head = lines.next().await.unwrap().unwrap();
    let mut body = lines.into_inner();
    let (mut got, mut at_mark) = (0usize, None);
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = tokio::time::timeout(WAIT, body.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        if n == 0 {
            break;
        }
        got += n;
        if got >= mark && at_mark.is_none() {
            at_mark = Some(t.elapsed());
        }
    }
    (got, at_mark.unwrap_or(t.elapsed()), t.elapsed())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hosts_files_go_out_paced_a_burst_then_twice_the_songs_bitrate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.flac");
    // 3 MB over 30 s: 100 kB a second. A burst of 10 s (1 MB), then 200 kB a second.
    std::fs::write(&path, counting(3_000_000)).unwrap();
    let mut song = host_song(&FILE_KEY["host:".len()..]);
    song.duration_ms = Some(30_000);
    let h = host(vec![song]).await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path, "audio/flac".into()));
    let ali = member(&h, 1, "Ali").await;
    let get = |from, to| Get {
        get: FILE_KEY.into(),
        from,
        to: Some(to),
    };
    let (got, burst, all) = timed_get(&ali, &get(0, 1_399_999), 1_000_000).await;
    assert_eq!(got, 1_400_000);
    assert!(
        burst < Duration::from_millis(800),
        "the burst at once: {burst:?}"
    );
    // The 400 kB after it at 200 kB a second: 2 s.
    assert!(
        all >= Duration::from_millis(1_800) && all < Duration::from_millis(3_500),
        "paced after the burst: {all:?}"
    );
    // The next range, where the last one ended (the guest reading on): no new burst, 400 kB
    // at 200 kB a second.
    let (got, _, all) = timed_get(&ali, &get(1_400_000, 1_799_999), 400_000).await;
    assert_eq!(got, 400_000);
    assert!(
        all >= Duration::from_millis(1_700) && all < Duration::from_millis(3_500),
        "reading on stays paced: {all:?}"
    );
    // A range somewhere else (a seek) starts with a burst of its own.
    let (got, _, all) = timed_get(&ali, &get(2_000_000, 2_399_999), 400_000).await;
    assert_eq!(got, 400_000);
    assert!(all < Duration::from_millis(800), "{all:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_song_of_no_known_length_and_covers_go_unpaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.flac");
    std::fs::write(&path, counting(3_000_000)).unwrap();
    let cover = dir.path().join("cover.jpg");
    std::fs::write(&cover, counting(2_000_000)).unwrap();
    let mut song = host_song(&FILE_KEY["host:".len()..]);
    song.duration_ms = None;
    let h = host(vec![song]).await;
    h.app
        .files
        .lock()
        .unwrap()
        .insert(FILE_KEY.into(), (path, "audio/flac".into()));
    h.app.covers.lock().unwrap().insert(FILE_KEY.into(), cover);
    let ali = member(&h, 1, "Ali").await;
    let (got, _, all) = timed_get(
        &ali,
        &Get {
            get: FILE_KEY.into(),
            from: 0,
            to: None,
        },
        1,
    )
    .await;
    assert_eq!(got, 3_000_000);
    assert!(all < Duration::from_millis(1_500), "{all:?}");
    let (got, _, all) = timed_get(
        &ali,
        &Get {
            get: format!("cover:{FILE_KEY}"),
            from: 0,
            to: None,
        },
        1,
    )
    .await;
    assert_eq!(got, 2_000_000);
    assert!(all < Duration::from_millis(1_500), "{all:?}");
}

// --- What the live sessions (protocol 2) take from WP8 -------------------------------------

/// A guest session joined to a plain host, with these timings: the host's side of its first
/// connection, the welcome sent.
async fn joined_timed(
    h: &RawHost,
    keys: &std::path::Path,
    timing: GuestTiming,
) -> (Arc<GuestSession>, Arc<App>, Visit) {
    let ep = h.ep.clone();
    let invite = h.invite.clone();
    let visit = tokio::spawn(async move { RawHost { ep, invite }.visit().await });
    let app = Arc::new(App::default());
    let g = GuestSession::join_timed(
        h.invite.clone(),
        &local(),
        keys,
        "Venuta 9.9.9".into(),
        app.clone(),
        timing,
    )
    .await
    .unwrap();
    let mut v = visit.await.unwrap();
    assert!(matches!(v.hear().await, Some(ToHost::Hello { .. })));
    v.say(&ToGuest::Welcome {
        you: 1,
        room: Room::new("Asha", SECRET, vec![]).view(false),
    })
    .await;
    until("in", || *g.stage.lock().unwrap() == Stage::In).await;
    (g, app, v)
}

fn quick_guest() -> GuestTiming {
    GuestTiming {
        redial: together_net::transport::Timing {
            redial_first: Duration::from_millis(100),
            redial_most: Duration::from_millis(300),
            redial_for: Duration::from_secs(20),
        },
        held_for: Duration::from_millis(400),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_held_while_reconnecting_goes_only_while_it_is_fresh() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let (g, app, v) = joined_timed(&h, keys.path(), quick_guest()).await;
    v.conn.close(9u32.into(), b"blip");
    until("reconnecting", || {
        *g.stage.lock().unwrap() == Stage::Reconnecting
    })
    .await;
    g.ask(Action::Pause);
    app.until("too late", |t| {
        t.contains(&Told::Note("Your pause was too late to send.".into()))
    })
    .await;
    // A fresh one, then the host answers again: only the fresh one goes.
    g.ask(Action::Next);
    let ep = h.ep.clone();
    let invite = h.invite.clone();
    let mut v2 = tokio::spawn(async move { RawHost { ep, invite }.visit().await })
        .await
        .unwrap();
    assert!(matches!(v2.hear().await, Some(ToHost::Hello { .. })));
    assert_eq!(
        v2.hear().await,
        Some(ToHost::Ask {
            action: Action::Next
        })
    );
    assert!(matches!(v2.hear().await, Some(ToHost::Ping { id: 0, .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_kind_of_press_says_why_it_did_not_go() {
    use together_net::guest::too_late;
    assert_eq!(too_late(&Action::Play), "Your play was too late to send.");
    assert_eq!(
        too_late(&Action::Seek { position_ms: 1 }),
        "Your seek was too late to send."
    );
    assert_eq!(too_late(&Action::Next), "Your skip was too late to send.");
    assert_eq!(
        too_late(&Action::Previous),
        "Your skip back was too late to send."
    );
    assert_eq!(
        too_late(&Action::Jump { index: 1 }),
        "Your jump was too late to send."
    );
    assert_eq!(
        too_late(&Action::Add {
            songs: vec![],
            next: false
        }),
        "The songs you added were too late to send."
    );
    assert_eq!(
        too_late(&Action::Remove { index: 1 }),
        "Your removal was too late to send."
    );
    assert_eq!(
        too_late(&Action::Move { from: 1, to: 2 }),
        "Your move in the queue was too late to send."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_stops_trying_after_its_time_and_says_the_room_is_lost() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let timing = GuestTiming {
        redial: together_net::transport::Timing {
            redial_for: Duration::from_millis(1_500),
            ..quick_guest().redial
        },
        ..quick_guest()
    };
    let (g, _app, v) = joined_timed(&h, keys.path(), timing).await;
    let t = Instant::now();
    v.conn.close(9u32.into(), b"gone");
    h.ep.close().await;
    until("ended, and why", || g.why.lock().unwrap().is_some()).await;
    assert_eq!(*g.stage.lock().unwrap(), Stage::Ended);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(
        g.why.lock().unwrap().as_deref(),
        Some("Lost Asha's room. Join again with the invite.")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_guests_pings_come_twenty_in_a_burst_then_one_a_second() {
    let keys = tempfile::tempdir().unwrap();
    let h = RawHost::new(net::unix_now() + 3_600).await;
    let (_g, _app, mut v) = joined_timed(&h, keys.path(), quick_guest()).await;
    let mut at = Vec::new();
    while at.len() < 22 {
        if let Some(ToHost::Ping { id, .. }) = v.hear().await {
            assert_eq!(id as usize, at.len());
            at.push(Instant::now());
        }
    }
    let burst = at[19] - at[0];
    assert!(
        burst >= Duration::from_millis(800) && burst < Duration::from_millis(1_600),
        "20 pings 50 ms apart: {burst:?}"
    );
    for w in at[19..].windows(2) {
        let gap = w[1] - w[0];
        assert!(
            gap >= Duration::from_millis(850) && gap < Duration::from_millis(1_500),
            "then one a second: {gap:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_host_keeps_a_dropped_member_as_long_as_a_guest_tries_again() {
    assert_eq!(HostTiming::default().away_for, Duration::from_secs(120));
    assert_eq!(
        GuestTiming::default().redial.redial_for,
        Duration::from_secs(120),
        "the same two minutes (D17)"
    );
    let h = host_timed(
        vec![],
        HostTiming {
            away_for: Duration::from_millis(500),
        },
    )
    .await;
    let ali = member(&h, 1, "Ali").await;
    ali.conn.close(0u32.into(), b"gone");
    until("away", || {
        h.s.room
            .lock()
            .unwrap()
            .view(true)
            .members
            .iter()
            .any(|m| m.id == 1 && m.away)
    })
    .await;
    let t = Instant::now();
    until("out", || !h.s.room.lock().unwrap().is_member(1)).await;
    assert!(
        t.elapsed() >= Duration::from_millis(350),
        "{:?}",
        t.elapsed()
    );
}
