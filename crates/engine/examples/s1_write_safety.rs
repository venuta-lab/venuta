//! Spike S1: is writing FLAC tags with lofty safe?
//!
//! Copies a sample of real files into a scratch folder (originals are only read), then runs a
//! sequence of tag edits on each copy. After every edit it checks:
//!   * the audio fingerprint (STREAMINFO + every audio byte) is identical to the original;
//!   * the tags read back exactly as a simple model predicts (keys, values, order);
//!   * pictures read back as predicted.
//! It also records file size / padding behaviour, and writes the expected final tags to JSON
//! so an independent reader (mutagen) can verify the results.
//!
//! Usage: cargo run --release --example s1_write_safety -- <library-root> <scratch-dir> [count]

use engine::media::flac_layout::{self, AudioFingerprint};
use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::ogg::OggPictureStorage;
use lofty::picture::{MimeType, Picture, PictureType};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone)]
enum Op {
    Insert(&'static str, String),
    Push(&'static str, String),
    Remove(&'static str),
    ReinsertFrontCover,
}

fn scenarios() -> Vec<(&'static str, Vec<Op>)> {
    let devanagari = "दिल है छोटा सा, छोटी सी आशा\n".repeat(60);
    let big = "x".repeat(64 * 1024);
    vec![
        (
            "modify_title",
            vec![Op::Insert("TITLE", "S1 Modified Title".into())],
        ),
        (
            "add_multivalue",
            vec![
                Op::Remove("ARTIST"),
                Op::Push("ARTIST", "Singer One".into()),
                Op::Push("ARTIST", "Singer Two".into()),
                Op::Push("ARTIST", "Singer Three".into()),
                Op::Push("LYRICIST", "Irshad Kamil".into()),
                Op::Push("ORIGINALDATE", "2011".into()),
            ],
        ),
        ("unicode_lyrics", vec![Op::Insert("LYRICS", devanagari)]),
        ("grow_past_padding", vec![Op::Insert("S1_BIG", big)]),
        ("replace_picture", vec![Op::ReinsertFrontCover]),
        (
            "strip_and_shrink",
            vec![
                Op::Remove("S1_BIG"),
                Op::Remove("MAJOR_BRAND"),
                Op::Remove("MINOR_VERSION"),
                Op::Remove("COMPATIBLE_BRANDS"),
            ],
        ),
    ]
}

/// Mirror of lofty's documented VorbisComments semantics, to predict the result.
fn apply_to_model(model: &mut Vec<(String, String)>, op: &Op) {
    match op {
        Op::Insert(k, v) => {
            model.retain(|(mk, _)| !mk.eq_ignore_ascii_case(k));
            model.push(((*k).to_owned(), v.clone()));
        }
        Op::Push(k, v) => model.push(((*k).to_owned(), v.clone())),
        Op::Remove(k) => model.retain(|(mk, _)| !mk.eq_ignore_ascii_case(k)),
        Op::ReinsertFrontCover => {}
    }
}

fn read(path: &Path) -> FlacFile {
    let mut r = BufReader::new(File::open(path).unwrap());
    FlacFile::read_from(
        &mut r,
        ParseOptions::new()
            .parsing_mode(ParsingMode::Strict)
            .implicit_conversions(false),
    )
    .unwrap()
}

fn items(f: &FlacFile) -> Vec<(String, String)> {
    f.vorbis_comments()
        .map(|vc| {
            vc.items()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn pic_hashes(f: &FlacFile) -> Vec<(u8, [u8; 32])> {
    f.pictures()
        .iter()
        .map(|(p, _)| (p.pic_type().as_u8(), *blake3::hash(p.data()).as_bytes()))
        .collect()
}

#[derive(Default, serde::Serialize)]
struct StepResult {
    step: &'static str,
    ok: bool,
    problems: Vec<String>,
    size_before: u64,
    size_after: u64,
    padding_before: u64,
    padding_after: u64,
    audio_moved: bool,
    write_ms: u128,
}

#[derive(serde::Serialize)]
struct FileResult {
    source: String,
    copy: String,
    bits: u8,
    sample_rate: u32,
    md5_set: bool,
    steps: Vec<StepResult>,
    expected_final_tags: Vec<(String, String)>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let scratch = PathBuf::from(&args[2]);
    let count: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);

    let mut all: Vec<PathBuf> = walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("flac"))
        })
        .map(|e| e.into_path())
        .collect();
    all.sort();
    let stride = (all.len() / count).max(1);
    let sample: Vec<PathBuf> = all.into_iter().step_by(stride).take(count).collect();
    std::fs::create_dir_all(&scratch).unwrap();
    println!(
        "S1: {} files sampled, copying to {}",
        sample.len(),
        scratch.display()
    );

    let mut results = Vec::new();
    let (mut steps_total, mut steps_failed) = (0usize, 0usize);
    for (i, src) in sample.iter().enumerate() {
        let copy = scratch.join(format!("{i:03}.flac"));
        std::fs::copy(src, &copy).unwrap();
        let original_fp: AudioFingerprint = flac_layout::audio_fingerprint(&copy).unwrap();

        let start = read(&copy);
        let props = start.properties();
        let mut model = items(&start);
        let mut expected_pics = pic_hashes(&start);
        let front = start
            .pictures()
            .iter()
            .find(|(p, _)| p.pic_type() == PictureType::CoverFront)
            .map(|(p, _)| p.data().to_vec());

        let mut fr = FileResult {
            source: src
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            copy: copy.to_string_lossy().into_owned(),
            bits: props.bit_depth(),
            sample_rate: props.sample_rate(),
            md5_set: props.signature() != 0,
            steps: Vec::new(),
            expected_final_tags: Vec::new(),
        };

        for (name, ops) in scenarios() {
            let mut sr = StepResult {
                step: name,
                ..Default::default()
            };
            let before = flac_layout::read_layout(&copy).unwrap();
            sr.size_before = before.file_len;
            sr.padding_before = before.padding_bytes();

            let mut f = read(&copy);
            for op in &ops {
                apply_to_model(&mut model, op);
                match op {
                    Op::Insert(k, v) => {
                        if f.vorbis_comments().is_none() {
                            f.set_vorbis_comments(Default::default());
                        }
                        f.vorbis_comments_mut()
                            .unwrap()
                            .insert((*k).to_owned(), v.clone());
                    }
                    Op::Push(k, v) => {
                        if f.vorbis_comments().is_none() {
                            f.set_vorbis_comments(Default::default());
                        }
                        f.vorbis_comments_mut()
                            .unwrap()
                            .push((*k).to_owned(), v.clone());
                    }
                    Op::Remove(k) => {
                        if let Some(vc) = f.vorbis_comments_mut() {
                            let _ = vc.remove(k).count();
                        }
                    }
                    Op::ReinsertFrontCover => {
                        if let Some(data) = &front {
                            f.remove_picture_type(PictureType::CoverFront);
                            let pic = Picture::unchecked(data.clone())
                                .pic_type(PictureType::CoverFront)
                                .mime_type(MimeType::Jpeg)
                                .build();
                            f.insert_picture(pic, None).unwrap();
                            expected_pics.retain(|(t, _)| *t != 3);
                            expected_pics.push((3, *blake3::hash(data).as_bytes()));
                        }
                    }
                }
            }

            let t = Instant::now();
            let mut out = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&copy)
                .unwrap();
            if let Err(e) = f.save_to(&mut out, WriteOptions::default()) {
                sr.problems.push(format!("save failed: {e}"));
            }
            drop(out);
            sr.write_ms = t.elapsed().as_millis();

            // Verify.
            match flac_layout::read_layout(&copy) {
                Ok(after) => {
                    sr.size_after = after.file_len;
                    sr.padding_after = after.padding_bytes();
                    sr.audio_moved = after.audio_offset != before.audio_offset;
                }
                Err(e) => sr
                    .problems
                    .push(format!("layout unreadable after write: {e}")),
            }
            match flac_layout::audio_fingerprint(&copy) {
                Ok(fp) if fp == original_fp => {}
                Ok(fp) => {
                    if fp.streaminfo != original_fp.streaminfo {
                        sr.problems.push("STREAMINFO changed".into());
                    }
                    if fp.audio_len != original_fp.audio_len {
                        sr.problems.push(format!(
                            "audio length changed {} -> {}",
                            original_fp.audio_len, fp.audio_len
                        ));
                    }
                    if fp.audio_hash != original_fp.audio_hash {
                        sr.problems.push("AUDIO BYTES CHANGED".into());
                    }
                }
                Err(e) => sr.problems.push(format!("fingerprint failed: {e}")),
            }
            let reread = read(&copy);
            let got = items(&reread);
            if got != model {
                let same_ci = got.len() == model.len()
                    && got
                        .iter()
                        .zip(&model)
                        .all(|((gk, gv), (mk, mv))| gk.eq_ignore_ascii_case(mk) && gv == mv);
                sr.problems.push(if same_ci {
                    "tag key case changed".into()
                } else {
                    format!(
                        "tags differ from model ({} vs {} items)",
                        got.len(),
                        model.len()
                    )
                });
            }
            let got_pics = pic_hashes(&reread);
            let mut a = got_pics.clone();
            let mut b = expected_pics.clone();
            a.sort();
            b.sort();
            if a != b {
                sr.problems.push(format!(
                    "pictures differ: {} vs {}",
                    got_pics.len(),
                    expected_pics.len()
                ));
            }

            sr.ok = sr.problems.is_empty();
            steps_total += 1;
            if !sr.ok {
                steps_failed += 1;
                println!("  FAIL {} {}: {:?}", fr.source, name, sr.problems);
            }
            fr.steps.push(sr);
        }
        fr.expected_final_tags = model;
        results.push(fr);
        if (i + 1) % 25 == 0 {
            println!("  {} / {} files done", i + 1, sample.len());
        }
    }

    // Size/padding behaviour summary per step.
    let mut summary: BTreeMap<&str, (usize, usize, u64)> = BTreeMap::new();
    for r in &results {
        for s in &r.steps {
            let e = summary.entry(s.step).or_default();
            e.0 += 1;
            e.1 += usize::from(s.audio_moved);
            e.2 += s.write_ms as u64;
        }
    }
    println!("\nstep                 files  audio_moved(rewrite)  avg_write_ms");
    for (step, (n, moved, ms)) in &summary {
        println!("{step:20} {n:5}  {moved:20}  {:>12}", ms / *n as u64);
    }
    println!("\nS1 result: {steps_total} steps, {steps_failed} failed");

    let mut out = File::create(scratch.join("s1_report.json")).unwrap();
    serde_json::to_writer_pretty(&mut out, &results).unwrap();
    out.seek(SeekFrom::End(0)).unwrap();
}
