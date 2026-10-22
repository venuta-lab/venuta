//! Models on the device: downloaded only when asked, checked, resumable, removable.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, OnceLock};

use flate2::Compression;
use flate2::write::GzEncoder;
use lyrics_translate::catalog::Catalog;
use lyrics_translate::store::{Body, Error, Fetch, ModelStore};
use sha2::{Digest, Sha256};

fn gz(data: &[u8]) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Three files of a pretend model, served gzipped, and a catalog that pins them.
struct Fixture {
    catalog: String,
    served: HashMap<String, Vec<u8>>,
    plain: HashMap<&'static str, Vec<u8>>,
}

fn fixture(break_checksum: bool) -> Fixture {
    let mut served = HashMap::new();
    let mut plain = HashMap::new();
    let mut files = Vec::new();
    for (kind, name) in [
        ("model", "model.xxen.intgemm.alphas.bin"),
        ("vocab", "vocab.xxen.spm"),
        ("lex", "lex.50.50.xxen.s2t.bin"),
    ] {
        // Large enough to arrive in several pieces, and not to shrink when gzipped.
        let mut x = 0x2545_f491_4f6c_dd1du64 ^ name.len() as u64;
        let data: Vec<u8> = (0..200_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .chain(name.bytes())
            .collect();
        let url = format!("https://example.invalid/{name}.gz");
        let packed = gz(&data);
        let sha = if break_checksum && kind == "vocab" {
            "0".repeat(64)
        } else {
            hex(&data)
        };
        files.push(format!(
            r#"{{"kind":"{kind}","name":"{name}","url":"{url}","download_bytes":{},"bytes":{},"sha256":"{sha}"}}"#,
            packed.len(),
            data.len()
        ));
        served.insert(url, packed);
        plain.insert(name, data);
    }
    let catalog = format!(
        r#"{{"publisher":"test","registry":"test","generated":"2026-10-22","models":[{{"id":"xx-en","from":"xx","to":"en","from_name":"Testish","to_name":"English","arch":"tiny","platform":"all","version":"3.0","files":[{}]}}]}}"#,
        files.join(",")
    );
    Fixture {
        catalog,
        served,
        plain,
    }
}

/// Serves the fixture. It can honour a resume or send the whole file again, drop the
/// connection after so many bytes (once), and report every request.
#[derive(Default)]
struct FakeFetch {
    served: HashMap<String, Vec<u8>>,
    ignore_range: bool,
    fail_after: Mutex<Option<u64>>,
    asked: Mutex<Vec<(String, u64)>>,
    during: OnceLock<Hook>,
}

type Hook = Arc<dyn Fn() + Send + Sync>;

struct Piece {
    data: Vec<u8>,
    at: usize,
    fail_at: Option<usize>,
    during: Option<Hook>,
}

impl Read for Piece {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(f) = &self.during {
            f();
        }
        if self.fail_at.is_some_and(|f| self.at >= f) {
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "dropped"));
        }
        let end = (self.at + buf.len().min(8192)).min(self.data.len());
        let end = self.fail_at.map_or(end, |f| end.min(f.max(self.at)));
        let n = end - self.at;
        buf[..n].copy_from_slice(&self.data[self.at..end]);
        self.at = end;
        Ok(n)
    }
}

impl Fetch for FakeFetch {
    fn open(&self, url: &str, from: u64) -> Result<Body, Error> {
        self.asked.lock().unwrap().push((url.to_owned(), from));
        let data = self
            .served
            .get(url)
            .ok_or_else(|| Error::Network(format!("{url}: 404")))?;
        let offset = if self.ignore_range { 0 } else { from };
        let fail_at = self.fail_after.lock().unwrap().take().map(|n| n as usize);
        Ok(Body {
            offset,
            reader: Box::new(Piece {
                data: data[offset as usize..].to_vec(),
                at: 0,
                fail_at,
                during: self.during.get().cloned(),
            }),
        })
    }
}

fn store(dir: &std::path::Path, f: &Fixture) -> ModelStore {
    ModelStore::new(dir, Catalog::from_json(&f.catalog).unwrap())
}

#[test]
fn nothing_is_on_the_device_until_asked() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    let m = &s.models()[0];
    assert_eq!(m.id, "xx-en");
    assert_eq!(m.name, "Testish to English");
    assert!(!m.installed);
    assert_eq!(m.progress, None);
    // Before the download, the size is what will be downloaded.
    let download: u64 = f.served.values().map(|v| v.len() as u64).sum();
    assert_eq!(m.size_bytes, download);
    assert!(s.files("xx-en").is_none());
    assert_eq!(
        std::fs::read_dir(dir.path())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
}

#[test]
fn a_download_is_checked_and_installed() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    let fetch = FakeFetch {
        served: f.served.clone(),
        ..Default::default()
    };
    let mut seen = Vec::new();
    let files = s
        .download("xx-en", &fetch, &mut |done, total| seen.push((done, total)))
        .unwrap();
    assert_eq!(
        std::fs::read(&files.model).unwrap(),
        f.plain["model.xxen.intgemm.alphas.bin"]
    );
    assert_eq!(
        std::fs::read(&files.src_vocab).unwrap(),
        f.plain["vocab.xxen.spm"]
    );
    assert_eq!(files.src_vocab, files.trg_vocab);
    assert_eq!(
        std::fs::read(&files.lex).unwrap(),
        f.plain["lex.50.50.xxen.s2t.bin"]
    );
    let total: u64 = f.served.values().map(|v| v.len() as u64).sum();
    assert_eq!(seen.last(), Some(&(total, total)));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    let m = &s.models()[0];
    assert!(m.installed);
    assert_eq!(m.progress, None);
    // Once installed, the size is what it takes on the device.
    let disk: u64 = f.plain.values().map(|v| v.len() as u64).sum();
    assert_eq!(m.size_bytes, disk);
    // A second store over the same folder sees it (and checks nothing over the network).
    assert!(store(dir.path(), &f).files("xx-en").is_some());
}

#[test]
fn a_wrong_checksum_installs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(true);
    let s = store(dir.path(), &f);
    let fetch = FakeFetch {
        served: f.served.clone(),
        ..Default::default()
    };
    let err = s.download("xx-en", &fetch, &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, Error::Checksum { .. }), "{err}");
    assert!(s.files("xx-en").is_none());
    assert!(!s.models()[0].installed);
    assert!(s.models()[0].error.is_some());
}

#[test]
fn a_broken_download_resumes_where_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    let fetch = FakeFetch {
        served: f.served.clone(),
        fail_after: Mutex::new(Some(50_000)),
        ..Default::default()
    };
    assert!(s.download("xx-en", &fetch, &mut |_, _| {}).is_err());
    assert!(s.files("xx-en").is_none());
    s.download("xx-en", &fetch, &mut |_, _| {}).unwrap();
    let asked = fetch.asked.lock().unwrap();
    // The file that broke was asked for again from where it stopped.
    let broken = &asked[0].0;
    let again: Vec<u64> = asked
        .iter()
        .filter(|(u, _)| u == broken)
        .map(|(_, from)| *from)
        .collect();
    assert_eq!(again.len(), 2, "{asked:?}");
    assert!(again[1] > 0, "{asked:?}");
    assert!(s.files("xx-en").is_some());
}

#[test]
fn a_server_that_cannot_resume_sends_it_all_again() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    let fetch = FakeFetch {
        served: f.served.clone(),
        ignore_range: true,
        fail_after: Mutex::new(Some(50_000)),
        ..Default::default()
    };
    assert!(s.download("xx-en", &fetch, &mut |_, _| {}).is_err());
    let files = s.download("xx-en", &fetch, &mut |_, _| {}).unwrap();
    assert_eq!(
        std::fs::read(&files.model).unwrap(),
        f.plain["model.xxen.intgemm.alphas.bin"]
    );
}

#[test]
fn progress_shows_while_it_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = Arc::new(store(dir.path(), &f));
    let fetch = FakeFetch {
        served: f.served.clone(),
        ..Default::default()
    };
    let seen: Arc<Mutex<Vec<Option<f64>>>> = Arc::default();
    let (s2, seen2) = (s.clone(), seen.clone());
    let _ = fetch.during.set(Arc::new(move || {
        seen2.lock().unwrap().push(s2.models()[0].progress)
    }));
    s.download("xx-en", &fetch, &mut |_, _| {}).unwrap();
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter()
            .all(|p| p.is_some_and(|p| (0.0..=1.0).contains(&p))),
        "{seen:?}"
    );
    assert!(seen.len() > 3);
    assert_eq!(s.models()[0].progress, None);
}

#[test]
fn removing_takes_it_off_the_device() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    let fetch = FakeFetch {
        served: f.served.clone(),
        ..Default::default()
    };
    s.download("xx-en", &fetch, &mut |_, _| {}).unwrap();
    s.remove("xx-en").unwrap();
    assert!(s.files("xx-en").is_none());
    assert!(!s.models()[0].installed);
    let left: Vec<_> = walk(dir.path());
    assert!(left.is_empty(), "{left:?}");
    // Removing what isn't there is fine.
    s.remove("xx-en").unwrap();
    assert!(matches!(s.remove("zz-en"), Err(Error::UnknownModel(_))));
}

#[test]
fn removing_while_it_downloads_stops_it() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = Arc::new(store(dir.path(), &f));
    let fetch = FakeFetch {
        served: f.served.clone(),
        ..Default::default()
    };
    let s2 = s.clone();
    let _ = fetch.during.set(Arc::new(move || {
        let _ = s2.remove("xx-en");
    }));
    let err = s.download("xx-en", &fetch, &mut |_, _| {}).unwrap_err();
    assert!(matches!(err, Error::Stopped), "{err}");
    assert!(s.files("xx-en").is_none());
    assert!(walk(dir.path()).is_empty(), "{:?}", walk(dir.path()));
}

#[test]
fn an_unknown_model_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    let fetch = FakeFetch::default();
    assert!(matches!(
        s.download("zz-en", &fetch, &mut |_, _| {}),
        Err(Error::UnknownModel(_))
    ));
    assert!(fetch.asked.lock().unwrap().is_empty());
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn a_download_asked_for_shows_at_once() {
    // The app answers the click before the download gets going: the model shows at 0% from
    // the moment it is asked for, and asking twice starts one download.
    let dir = tempfile::tempdir().unwrap();
    let f = fixture(false);
    let s = store(dir.path(), &f);
    assert!(s.reserve("xx-en").unwrap());
    assert_eq!(s.models()[0].progress, Some(0.0));
    assert!(s.reserve("xx-en").is_err());
    let fetch = FakeFetch {
        served: f.served.clone(),
        ..Default::default()
    };
    s.download("xx-en", &fetch, &mut |_, _| {}).unwrap();
    assert_eq!(s.models()[0].progress, None);
    // Installed already: nothing to download.
    assert!(!s.reserve("xx-en").unwrap());
    assert!(matches!(s.reserve("zz-en"), Err(Error::UnknownModel(_))));
}
