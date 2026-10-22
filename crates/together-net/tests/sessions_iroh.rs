//! The sessions over real connections on this computer (WP9): a host's and a guest's session
//! on `IrohTransport`, protocol 3, the clock on datagram pings, each player a fake that records
//! what it is told. The whole rooms are tested in simulated time in `together`; this shows the
//! same sessions work on iroh as they do on the simulated network.

mod common;
use common::*;

use together::clock_map::{self, ClockMap};
use together::conductor::{Conductor, Copies, Copy, Lookup};
use together::fakes::{Call, FakeRoomPlayer};
use together::proto::v3::{self, Action};
use together::room::v2::{Opening, Room};
use together::room_player::{ListenMode, RoomPlayer, SyncMode};
use together::session::{GuestConfig, GuestSession, HostConfig, HostSession, Stage};
use together::time::{MonoClock, RealClock};
use together_net::transport::{HOST, IrohTransport, Timing};

struct Any;

impl Copies<String> for Any {
    fn find(&mut self, song: &Song) -> Lookup<String> {
        Lookup::Found(Copy {
            item: song.key.clone(),
            align: together::align::Alignment::APPROXIMATE,
            kind: together::view::CopyKind::Catalog,
            length_ms: None,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_joins_over_iroh_learns_the_clock_and_plays_the_rooms_start() {
    let clock: Arc<dyn MonoClock> = Arc::new(RealClock);
    let hep = endpoint_on(false, true, None).await;
    let addr = hep.addr();
    struct Nothing;
    impl together::transport::MediaServer for Nothing {
        fn open(
            &self,
            _: together::transport::PeerId,
            _: &together::transport::Get,
        ) -> Option<Arc<[u8]>> {
            None
        }
    }
    let ht = IrohTransport::host(hep, clock.clone(), Arc::new(Nothing));
    let map = clock_map::handle();
    map.store(Arc::new(Some(ClockMap::identity())));
    let (hp, hlog) = FakeRoomPlayer::<String>::new(clock.clone(), Default::default());
    let hp: Arc<dyn RoomPlayer<String>> = Arc::new(hp);
    let handle = hp.attach(map.clone(), SyncMode::Apart);
    let conductor =
        Conductor::new(hp, map, Box::new(Any), ListenMode::Apart).with_handle(handle, None);
    let mut room = Room::new(
        "Asha",
        SECRET,
        vec![online("aaaaaaaaaaa")],
        Opening::Resting,
    );
    room.set_let_in_without_asking(true);
    let mut host = HostSession::new(
        Arc::new(ht),
        clock.clone(),
        room,
        conductor,
        HostConfig {
            invite_open: Arc::new(|| true),
        },
    );
    let gep = endpoint_on(false, false, Some(key(1))).await;
    let me = gep.id().to_string();
    let gt = IrohTransport::guest(gep, addr, clock.clone(), Timing::default())
        .await
        .unwrap();
    let (gp, glog) = FakeRoomPlayer::<String>::new(clock.clone(), Default::default());
    let mut guest = GuestSession::new(
        Arc::new(gt),
        HOST,
        clock.clone(),
        Conductor::new(
            Arc::new(gp),
            clock_map::handle(),
            Box::new(Any),
            ListenMode::Apart,
        ),
        GuestConfig {
            name: "Ravi".into(),
            proof: together::invite::proof(SECRET, &me),
            app: "Venuta test".into(),
            host_name: "Asha".into(),
        },
    );
    let t0 = Instant::now();
    let mut step = async |host: &mut HostSession<String>, guest: &mut GuestSession<String>| {
        host.step();
        guest.step();
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    while guest.stage() != Stage::In || !guest.clock_ready() {
        assert!(t0.elapsed() < WAIT, "in, the clock ready");
        step(&mut host, &mut guest).await;
    }
    eprintln!("in and the clock ready {:?} after connecting", t0.elapsed());
    host.ask(Action::Play).unwrap();
    let t1 = Instant::now();
    while !glog
        .just_calls()
        .iter()
        .any(|c| matches!(c, Call::Commit(_)))
    {
        assert!(t1.elapsed() < WAIT, "the start reached Ravi");
        step(&mut host, &mut guest).await;
    }
    let a = host.room().anchor().unwrap();
    assert!(a.playing);
    assert!(glog.just_calls().contains(&Call::Commit(a)));
    assert!(hlog.just_calls().contains(&Call::Commit(a)));
    assert_eq!(v3::PROTOCOL, 3);
    // Ravi leaves: the host hears it, and Ravi's player is let go.
    guest.leave();
    assert_eq!(glog.just_calls().last(), Some(&Call::Detach));
    let t2 = Instant::now();
    while host.room().member_ids().len() > 1 {
        assert!(t2.elapsed() < WAIT, "Ravi out of the room");
        step(&mut host, &mut guest).await;
    }
}
