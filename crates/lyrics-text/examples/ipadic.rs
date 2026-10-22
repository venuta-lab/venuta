//! Downloads and installs the Japanese dictionary into a folder, as the app does the first
//! time a Japanese line asks, and prints where it went:
//! `cargo run -p lyrics-text --example ipadic -- <app data folder>`.

use std::path::PathBuf;

use lyrics_text::japanese::dictionary::{self, IPADIC, Progress};

fn main() {
    let Some(root) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: ipadic <app data folder>");
        std::process::exit(2);
    };
    let mut last = 0;
    let result = dictionary::ensure(&root, &IPADIC, &mut |p| {
        match p {
            Progress::Downloading { done, total } => {
                let pct = done * 100 / total.max(1);
                if pct >= last + 10 || done == total {
                    eprintln!("downloading {pct}%");
                    last = pct;
                }
            }
            other => eprintln!("{other:?}"),
        }
        true
    });
    match result {
        Ok(dir) => println!("{}", dir.display()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
