//! Edit a scratch copy in place, then undo: cargo run --example edit_roundtrip -- <dir-with-copy> <file>
use engine::edit::{TagEdit, edit_in_place, undo_edits};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let report = edit_in_place(
        root,
        &[TagEdit {
            file: a[2].clone(),
            set: vec![
                ("RATING".into(), vec!["80".into()]),
                ("TITLE".into(), vec!["Edited Title".into()]),
            ],
        }],
    )
    .unwrap();
    println!("edit: {:?}", report);
    if std::env::var("UNDO").is_ok() {
        println!(
            "undo restored {}",
            undo_edits(std::path::Path::new(report.journal.as_ref().unwrap())).unwrap()
        );
    }
}
