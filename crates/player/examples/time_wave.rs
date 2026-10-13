//! How long a waveform takes: `cargo run --release -p player --example time_wave -- FILE`
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let t = std::time::Instant::now();
    let w = player::waveform::waveform(std::path::Path::new(&f), 600).unwrap();
    println!(
        "{} buckets in {:?}; max {}, first {:?}",
        w.len(),
        t.elapsed(),
        w.iter().max().unwrap(),
        &w[..12]
    );
}
