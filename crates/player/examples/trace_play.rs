//! Print position and errors every status for a few seconds (volume 0).
use player::engine::{Cmd, OutputChoice, Player, Status};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let mem = std::env::args().any(|a| a == "--memory");
    let cap = std::sync::Arc::new(std::sync::Mutex::new(player::output::Captured::default()));
    let out = if mem {
        OutputChoice::Memory(cap.clone())
    } else {
        OutputChoice::Device {
            id: None,
            exclusive: true,
        }
    };
    let (tx, rx) = channel::<Status<PathBuf>>();
    let p = Player::spawn(out, move |s| {
        let _ = tx.send(s);
    });
    p.send(Cmd::Volume(0.0));
    p.send(Cmd::Set {
        items: vec![PathBuf::from(&f)],
        index: 0,
        start_ms: 0,
        play: true,
        from: String::new(),
    });
    let t = Instant::now();
    let mut n = 0;
    while t.elapsed() < Duration::from_secs(3) {
        if let Ok(s) = rx.recv_timeout(Duration::from_millis(300)) {
            n += 1;
            if n % 5 == 1 || !s.error.is_empty() {
                println!(
                    "{:>6?} playing={} pos={} err={:?}",
                    t.elapsed().as_millis(),
                    s.playing,
                    s.position_ms,
                    s.error
                );
            }
        } else {
            println!("{:>6?} (no status)", t.elapsed().as_millis());
        }
    }
    p.send(Cmd::Pause);
    drop(p);
    if mem {
        println!("memory got {} bytes", cap.lock().unwrap().bytes.len());
    }
}
