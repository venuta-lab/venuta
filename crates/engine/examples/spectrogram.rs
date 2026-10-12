//! Render one spectrogram: cargo run --release --example spectrogram -- <file.flac> <out.png>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let img = engine::spectrogram::render(std::path::Path::new(&args[1])).expect("render");
    img.save(&args[2]).expect("save");
    println!(
        "{}x{} in {} ms",
        img.width(),
        img.height(),
        t.elapsed().as_millis()
    );
}
