//! Print a file's fingerprint: `cargo run --example fingerprint -- <file> [raw-out]`.
fn main() {
    let path = std::env::args().nth(1).expect("a file");
    match engine::fingerprint::fingerprint(std::path::Path::new(&path)) {
        Ok(p) => {
            println!("{} {}", p.duration_s, p.encoded);
            if let Some(out) = std::env::args().nth(2) {
                let bytes: Vec<u8> = p.raw.iter().flat_map(|x| x.to_le_bytes()).collect();
                std::fs::write(out, bytes).unwrap();
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}
