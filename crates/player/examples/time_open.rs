//! How long opening and decoding a file takes: `cargo run -p player --example time_open -- FILE`
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let t = std::time::Instant::now();
    let mut s = player::decode::Source::open(std::path::Path::new(&f)).unwrap();
    println!(
        "open {:?}, {:?}, {:?} ms long",
        t.elapsed(),
        s.format,
        s.duration_ms()
    );
    let t = std::time::Instant::now();
    let mut out = Vec::new();
    let mut frames = 0;
    for _ in 0..200 {
        if s.next(&mut out).unwrap() != player::decode::DecodeState::Samples {
            break;
        }
        frames += out.len() / usize::from(s.format.channels);
    }
    println!("decoded {frames} frames in {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let at = s.seek(60_000).unwrap();
    println!("seek to 60 s: frame {at} in {:?}", t.elapsed());
}
