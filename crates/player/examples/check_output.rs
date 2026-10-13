//! Check real sound output without making a sound: each file is played for two seconds at
//! volume 0 (digital silence), and what the device accepted is printed.
//! `cargo run -p player --example check_output -- [--shared] FILE...`

use player::engine::{Cmd, OutputChoice, Player, Status};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exclusive = !args.iter().any(|a| a == "--shared");
    #[cfg(windows)]
    for d in player::wasapi_out::list_devices().unwrap_or_default() {
        println!("device: {}{}", d.name, if d.default { " (default)" } else { "" });
    }
    for file in args.iter().filter(|a| !a.starts_with("--")) {
        let (tx, rx) = channel::<Status<PathBuf>>();
        let p = Player::spawn(OutputChoice::Device { id: None, exclusive }, move |s| {
            let _ = tx.send(s);
        });
        p.send(Cmd::Volume(0.0));
        p.send(Cmd::Set { items: vec![PathBuf::from(file)], index: 0, start_ms: 0, play: true, from: String::new() });
        let t = Instant::now();
        let mut last = None;
        while t.elapsed() < Duration::from_secs(2) {
            if let Ok(s) = rx.recv_timeout(Duration::from_millis(500)) {
                if !s.error.is_empty() {
                    println!("  error: {}", s.error);
                }
                last = Some(s);
            }
        }
        p.send(Cmd::Pause);
        if let Some(s) = last {
            println!("{file}\n  playing {} at {} ms", s.playing, s.position_ms);
            if let Some(sig) = s.signal {
                println!(
                    "  {:?} on {}: source {}/{} {} → device {}-in-{} {:?} {} Hz {} ch; steps {:?}",
                    sig.mode, sig.device, sig.source.bits, sig.source.rate, sig.source.codec,
                    sig.output.valid_bits, sig.output.container_bits, sig.output.kind, sig.output.rate,
                    sig.output.channels, sig.steps
                );
            }
        }
        drop(p);
    }
}
