//! Real FLAC files, played into memory, must match the MD5 the encoder stored in them.
//! Files come from VENUTA_TEST_FLAC (paths separated by `|`); run with
//! `cargo test -p player --test bit_perfect -- --ignored`.

#[test]
#[ignore = "needs real FLAC files (VENUTA_TEST_FLAC)"]
fn real_flac_files_come_out_bit_perfect() {
    let list = std::env::var("VENUTA_TEST_FLAC").expect("set VENUTA_TEST_FLAC");
    for file in list.split('|').filter(|s| !s.is_empty()) {
        let v = player::verify::verify_bit_perfect(std::path::Path::new(file)).unwrap();
        assert!(
            v.bit_perfect,
            "{file}: file {} output {}",
            v.file_md5, v.output_md5
        );
        println!(
            "bit-perfect: {file} ({}-bit {} Hz, {} frames)",
            v.bits, v.rate, v.frames
        );
    }
}
