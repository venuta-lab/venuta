//! The translation models on this device, under one folder (the app data folder's
//! `translation-models`): nothing until the listener asks for a model, then that model only.
//!
//! A download goes into `<id>/<version>-<arch>.part/`, file by file: each gzipped file is
//! fetched (from where it stopped, when a download broke off and the server can resume),
//! unpacked, and its SHA-256 checked against the one pinned in the [`Catalog`]. Only when every
//! file is checked does the folder become `<id>/<version>-<arch>/`, so a model folder is
//! either complete and checked or not there. Removing a model deletes its folder (and stops
//! its download).

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use crate::Error;
use crate::catalog::{Catalog, FileKind, ModelFile, ModelInfo};

/// Written last into a complete, checked model folder.
const CHECKED: &str = ".checked";

/// A response: its body, and where in the file the body starts (`from` when the server
/// resumed, 0 when it sends the whole file again).
pub struct Body {
    pub offset: u64,
    pub reader: Box<dyn Read + Send>,
}

/// Fetches a URL from byte `from` on. The app gives [`Https`] (or its own); tests a fake.
pub trait Fetch: Send + Sync {
    fn open(&self, url: &str, from: u64) -> Result<Body, Error>;
}

/// Fetches over HTTPS with a generic User-Agent (nothing about the listener is sent).
#[cfg(feature = "download")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Https;

#[cfg(feature = "download")]
impl Fetch for Https {
    fn open(&self, url: &str, from: u64) -> Result<Body, Error> {
        let net = |e: ureq::Error| Error::Network(e.to_string());
        let mut req = ureq::get(url)
            .config()
            .http_status_as_error(false)
            .build()
            .header(
                "User-Agent",
                "Venuta (https://github.com/venuta-lab/venuta)",
            );
        if from > 0 {
            req = req.header("Range", format!("bytes={from}-"));
        }
        let resp = req.call().map_err(net)?;
        let offset = match resp.status().as_u16() {
            200 => 0,
            206 => from,
            // Asked from the end: it's all here already.
            416 if from > 0 => {
                return Ok(Body {
                    offset: from,
                    reader: Box::new(io::empty()),
                });
            }
            s => return Err(Error::Network(format!("{url}: HTTP {s}"))),
        };
        Ok(Body {
            offset,
            reader: Box::new(resp.into_body().into_reader()),
        })
    }
}

/// A model's files on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    pub model: PathBuf,
    /// The same file for both, except in pairs with a vocabulary per side.
    pub src_vocab: PathBuf,
    pub trg_vocab: PathBuf,
    pub lex: PathBuf,
}

/// A model as the app lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub from: String,
    pub to: String,
    /// "Hindi to English".
    pub name: String,
    /// What it takes on the device once installed; before, what will be downloaded.
    pub size_bytes: u64,
    pub installed: bool,
    /// 0 to 1 while it downloads.
    pub progress: Option<f64>,
    /// Why its last download failed.
    pub error: Option<String>,
}

#[derive(Default)]
struct Job {
    done: u64,
    total: u64,
    stop: bool,
    /// Asked for ([`ModelStore::reserve`]), its download not started yet.
    reserved: bool,
}

/// The models in a folder.
pub struct ModelStore {
    root: PathBuf,
    catalog: Catalog,
    jobs: Mutex<HashMap<String, Job>>,
    errors: Mutex<HashMap<String, String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>, catalog: Catalog) -> Self {
        ModelStore {
            root: root.into(),
            catalog,
            jobs: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
        }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn folder(&self, m: &ModelInfo) -> PathBuf {
        self.root
            .join(&m.id)
            .join(format!("{}-{}", m.version, m.arch))
    }

    fn part_folder(&self, m: &ModelInfo) -> PathBuf {
        self.root
            .join(&m.id)
            .join(format!("{}-{}.part", m.version, m.arch))
    }

    fn files_in(dir: &Path, m: &ModelInfo) -> Option<ModelFiles> {
        let path = |k: FileKind| m.file(k).map(|f| dir.join(&f.name));
        let (src_vocab, trg_vocab) = match path(FileKind::Vocab) {
            Some(v) => (v.clone(), v),
            None => (path(FileKind::SrcVocab)?, path(FileKind::TrgVocab)?),
        };
        Some(ModelFiles {
            model: path(FileKind::Model)?,
            src_vocab,
            trg_vocab,
            lex: path(FileKind::Lex)?,
        })
    }

    /// The model's files, when it is installed and was checked.
    pub fn files(&self, id: &str) -> Option<ModelFiles> {
        let m = self.catalog.get(id)?;
        let dir = self.folder(m);
        dir.join(CHECKED)
            .is_file()
            .then(|| Self::files_in(&dir, m))
            .flatten()
    }

    /// Every model there is, with what is on the device and what is downloading.
    pub fn models(&self) -> Vec<ModelStatus> {
        let jobs = lock(&self.jobs);
        let errors = lock(&self.errors);
        self.catalog
            .models()
            .iter()
            .map(|m| {
                let installed = self.files(&m.id).is_some();
                ModelStatus {
                    id: m.id.clone(),
                    from: m.from.clone(),
                    to: m.to.clone(),
                    name: m.name(),
                    size_bytes: if installed {
                        m.bytes()
                    } else {
                        m.download_bytes()
                    },
                    installed,
                    progress: jobs
                        .get(&m.id)
                        .map(|j| (j.done as f64 / j.total.max(1) as f64).clamp(0.0, 1.0)),
                    error: errors.get(&m.id).cloned(),
                }
            })
            .collect()
    }

    /// Marks a model as downloading before its download starts (the app answers the click at
    /// once and downloads on another thread): true when [`download`] should follow, false when
    /// it is installed already. Asking while it downloads is an error.
    ///
    /// [`download`]: ModelStore::download
    pub fn reserve(&self, id: &str) -> Result<bool, Error> {
        let m = self
            .catalog
            .get(id)
            .ok_or_else(|| Error::UnknownModel(id.to_owned()))?;
        if self.files(id).is_some() {
            return Ok(false);
        }
        let mut jobs = lock(&self.jobs);
        if jobs.contains_key(id) {
            return Err(Error::Engine(format!("{id} is downloading already")));
        }
        jobs.insert(
            id.to_owned(),
            Job {
                total: m.download_bytes(),
                reserved: true,
                ..Job::default()
            },
        );
        lock(&self.errors).remove(id);
        Ok(true)
    }

    /// Downloads, checks and installs a model, unless it is installed already; `progress` is
    /// told the bytes downloaded so far and the total. Blocks until done; [`remove`] stops it.
    ///
    /// [`remove`]: ModelStore::remove
    pub fn download(
        &self,
        id: &str,
        fetch: &dyn Fetch,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<ModelFiles, Error> {
        let m = self
            .catalog
            .get(id)
            .ok_or_else(|| Error::UnknownModel(id.to_owned()))?
            .clone();
        if let Some(f) = self.files(id) {
            lock(&self.jobs).remove(id);
            return Ok(f);
        }
        {
            let mut jobs = lock(&self.jobs);
            match jobs.get_mut(id) {
                Some(j) if j.reserved => j.reserved = false,
                Some(_) => return Err(Error::Engine(format!("{id} is downloading already"))),
                None => {
                    jobs.insert(
                        id.to_owned(),
                        Job {
                            total: m.download_bytes(),
                            ..Job::default()
                        },
                    );
                }
            }
        }
        let result = self.fetch_all(&m, fetch, progress);
        let stopped = lock(&self.jobs).remove(id).is_some_and(|j| j.stop);
        match &result {
            Ok(_) => {
                lock(&self.errors).remove(id);
            }
            Err(_) if stopped => {
                // Removed while it downloaded: nothing of it stays.
                let _ = fs::remove_dir_all(self.root.join(&m.id));
                lock(&self.errors).remove(id);
                return Err(Error::Stopped);
            }
            Err(e) => {
                lock(&self.errors).insert(id.to_owned(), e.to_string());
            }
        }
        result
    }

    fn fetch_all(
        &self,
        m: &ModelInfo,
        fetch: &dyn Fetch,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<ModelFiles, Error> {
        let part = self.part_folder(m);
        fs::create_dir_all(&part)?;
        let total = m.download_bytes();
        // What earlier tries left: whole files, and the bytes of a broken-off one.
        let mut done = 0u64;
        for f in &m.files {
            done += if part.join(&f.name).is_file() {
                f.download_bytes
            } else {
                fs::metadata(gz_path(&part, f)).map_or(0, |md| md.len())
            };
        }
        for f in &m.files {
            if part.join(&f.name).is_file() {
                continue;
            }
            self.fetch_one(m, f, &part, fetch, &mut done, total, progress)?;
            unpack(&part, f)?;
        }
        File::create(part.join(CHECKED))?;
        let dir = self.folder(m);
        // Older versions of the model go when the new one is in.
        if let Ok(rd) = fs::read_dir(self.root.join(&m.id)) {
            for e in rd.flatten() {
                if e.path() != part {
                    let _ = fs::remove_dir_all(e.path());
                }
            }
        }
        fs::rename(&part, &dir)?;
        Self::files_in(&dir, m).ok_or_else(|| Error::NotInstalled(m.id.clone()))
    }

    #[allow(clippy::too_many_arguments)]
    fn fetch_one(
        &self,
        m: &ModelInfo,
        f: &ModelFile,
        part: &Path,
        fetch: &dyn Fetch,
        done: &mut u64,
        total: u64,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), Error> {
        let gz = gz_path(part, f);
        let have = fs::metadata(&gz).map_or(0, |md| md.len());
        let body = fetch.open(&f.url, have)?;
        // Written at its end (not opened to append: Windows then refuses to cut it short).
        let mut out = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&gz)?;
        let mut at = have;
        if body.offset != have {
            if body.offset != 0 {
                return Err(Error::Network(format!(
                    "{}: the server answered from byte {}, asked from {have}",
                    f.url, body.offset
                )));
            }
            // It sends the whole file again: start it again.
            out.set_len(0)?;
            *done -= have;
            at = 0;
        }
        out.seek(SeekFrom::Start(at))?;
        // A file the size of several times what was expected is not the file.
        let limit = f.download_bytes.saturating_mul(4) + (1 << 20);
        let mut reader = body.reader;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            if lock(&self.jobs).get(&m.id).is_none_or(|j| j.stop) {
                return Err(Error::Stopped);
            }
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Network(format!("{}: {e}", f.url))),
            };
            out.write_all(&buf[..n])?;
            at += n as u64;
            *done += n as u64;
            if at > limit {
                drop(out);
                let _ = fs::remove_file(&gz);
                return Err(Error::Size {
                    name: f.name.clone(),
                    expected: f.download_bytes,
                    got: at,
                });
            }
            if let Some(j) = lock(&self.jobs).get_mut(&m.id) {
                j.done = *done;
            }
            progress(*done, total);
        }
        out.flush()?;
        Ok(())
    }

    /// Takes a model off the device: its folder goes, and a download of it stops (its part
    /// goes when it has stopped). Nothing to remove is fine.
    pub fn remove(&self, id: &str) -> Result<(), Error> {
        let m = self
            .catalog
            .get(id)
            .ok_or_else(|| Error::UnknownModel(id.to_owned()))?;
        if let Some(j) = lock(&self.jobs).get_mut(id) {
            j.stop = true;
            return Ok(());
        }
        lock(&self.errors).remove(id);
        match fs::remove_dir_all(self.root.join(&m.id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

fn gz_path(part: &Path, f: &ModelFile) -> PathBuf {
    part.join(format!("{}.gz", f.name))
}

/// Unpacks a downloaded file next to it and checks it; it takes its own name only once
/// checked. A file that fails the check goes, so the next try downloads it again.
fn unpack(part: &Path, f: &ModelFile) -> Result<(), Error> {
    let gz = gz_path(part, f);
    let tmp = part.join(format!("{}.tmp", f.name));
    let result = (|| {
        let mut input = flate2::read::GzDecoder::new(io::BufReader::new(File::open(&gz)?));
        let mut out = io::BufWriter::new(File::create(&tmp)?);
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match input.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    return Err(Error::Checksum {
                        name: f.name.clone(),
                        expected: f.sha256.clone(),
                        got: format!("unreadable ({e})"),
                    });
                }
            };
            out.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
            size += n as u64;
            if size > f.bytes {
                return Err(Error::Size {
                    name: f.name.clone(),
                    expected: f.bytes,
                    got: size,
                });
            }
        }
        out.flush()?;
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != f.sha256 {
            return Err(Error::Checksum {
                name: f.name.clone(),
                expected: f.sha256.clone(),
                got,
            });
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            fs::rename(&tmp, part.join(&f.name))?;
            let _ = fs::remove_file(&gz);
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(&gz);
            Err(e)
        }
    }
}
