//! Write the player test to a card: `cargo run --example player_test -- <library> <card>`.
fn main() {
    let mut a = std::env::args().skip(1);
    let (lib, card) = (a.next().expect("library"), a.next().expect("card"));
    match engine::device::write_test_kit(lib.as_ref(), card.as_ref(), "Music") {
        Ok(items) => items
            .iter()
            .for_each(|i| println!("{:<36} {}", i.file, i.checks)),
        Err(e) => eprintln!("{e}"),
    }
}
