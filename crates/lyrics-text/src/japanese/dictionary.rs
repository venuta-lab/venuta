//! The Japanese dictionary (IPADIC, built for lindera 6.2.0): not shipped with the app,
//! fetched the first time a Japanese line asks for it, from lindera's own GitHub release.
//!
//! [`install`] downloads the zip (10.5 MB) into a temporary folder next to where it will live,
//! checks its size and SHA-256 against the values pinned in [`IPADIC`], unpacks it (47.5 MB)
//! and only then moves it into place, so a dictionary folder is either complete and checked or
//! not there at all. Progress is reported as it goes and the caller can stop it. Until it is
//! installed, [`super::Japanese::kana_only`] still reads lines in kana.
//!
//! The dictionary's licence (NAIST and ICOT, free to use and redistribute with its notice)
//! comes inside it as `NOTICE.txt`; the app's credits should name it.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// A dictionary release: where it comes from and what it must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// The folder it is kept in, under `<app data>/dictionaries`.
    pub name: &'static str,
    pub url: &'static str,
    /// The zip's SHA-256, lowercase hex.
    pub sha256: &'static str,
    /// The zip's size in bytes.
    pub size: u64,
    /// The folder inside the zip that holds the dictionary.
    pub folder: &'static str,
}

/// IPADIC 2.7.0 for lindera 6.2.0, as released on 2026-09-26. The checksum is the one GitHub
/// lists for the asset, and the one the file had when downloaded on 2026-10-21.
pub const IPADIC: Spec = Spec {
    name: "lindera-ipadic-6.2.0",
    url: "https://github.com/lindera/lindera/releases/download/v6.2.0/lindera-ipadic-6.2.0.zip",
    sha256: "5ed4bba6b429030b0387df67d40d5751d35dedef0306f4bac6d957cad5f04b72",
    size: 10_519_550,
    folder: "lindera-ipadic",
};

/// Written last into a complete, checked dictionary folder.
const CHECKED: &str = ".checked";

/// Where a step of the install is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Bytes received so far, of the size expected.
    Downloading {
        done: u64,
        total: u64,
    },
    Checking,
    Unpacking,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the download failed: {0}")]
    Network(String),
    #[error("the download was {got} bytes, {expected} expected")]
    Size { expected: u64, got: u64 },
    #[error("the download's checksum is {got}, {expected} expected")]
    Checksum { expected: String, got: String },
    #[error("the dictionary could not be unpacked: {0}")]
    Unpack(String),
    #[error("stopped")]
    Stopped,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Fetches a URL into `into`. `more` is told the bytes written so far after each chunk and
/// returns false to stop. The app gives its own (or uses [`Https`]); tests give a fake.
pub trait Fetch {
    fn fetch(
        &self,
        url: &str,
        into: &mut dyn Write,
        more: &mut dyn FnMut(u64) -> bool,
    ) -> Result<(), Error>;
}

/// Where `spec` lives under `app_data`.
pub fn folder(app_data: &Path, spec: &Spec) -> PathBuf {
    app_data.join("dictionaries").join(spec.name)
}

/// The dictionary's folder, when it is installed and was checked.
pub fn installed(app_data: &Path, spec: &Spec) -> Option<PathBuf> {
    let dir = folder(app_data, spec);
    dir.join(CHECKED).is_file().then_some(dir)
}

/// Counts and hashes what goes through, and refuses more than the expected size.
struct Checked<W> {
    inner: W,
    hash: Sha256,
    done: u64,
    limit: u64,
    /// Bytes offered past the limit (refused).
    over: u64,
}

impl<W: Write> Write for Checked<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.done + buf.len() as u64 > self.limit {
            self.over = self.done + buf.len() as u64;
            return Err(io::Error::other("larger than expected"));
        }
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.done += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Downloads, checks and unpacks `spec` under `app_data`, unless it is already there. Returns
/// the dictionary's folder (give it to [`super::Japanese::with_dictionary`]).
pub fn install(
    app_data: &Path,
    spec: &Spec,
    fetch: &dyn Fetch,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<PathBuf, Error> {
    if let Some(dir) = installed(app_data, spec) {
        return Ok(dir);
    }
    let parent = app_data.join("dictionaries");
    fs::create_dir_all(&parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let work = parent.join(format!(
        ".{}-{}-{nonce}.part",
        spec.name,
        std::process::id()
    ));
    fs::create_dir_all(&work)?;
    let result = install_in(&work, spec, fetch, progress).and_then(|unpacked| {
        let dir = folder(app_data, spec);
        if installed(app_data, spec).is_none() {
            // A folder left without its check mark is not a dictionary: replace it.
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            if let Err(e) = fs::rename(&unpacked, &dir) {
                // Another install finished first: theirs is as good.
                if installed(app_data, spec).is_none() {
                    return Err(e.into());
                }
            }
        }
        Ok(dir)
    });
    let _ = fs::remove_dir_all(&work);
    result
}

fn install_in(
    work: &Path,
    spec: &Spec,
    fetch: &dyn Fetch,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<PathBuf, Error> {
    let zip_path = work.join("download.zip");
    let mut sink = Checked {
        inner: io::BufWriter::new(File::create(&zip_path)?),
        hash: Sha256::new(),
        done: 0,
        limit: spec.size,
        over: 0,
    };
    if !progress(Progress::Downloading {
        done: 0,
        total: spec.size,
    }) {
        return Err(Error::Stopped);
    }
    let mut stopped = false;
    let fetched = fetch.fetch(spec.url, &mut sink, &mut |done| {
        stopped = !progress(Progress::Downloading {
            done,
            total: spec.size,
        });
        !stopped
    });
    if stopped {
        return Err(Error::Stopped);
    }
    if sink.over > 0 {
        return Err(Error::Size {
            expected: spec.size,
            got: sink.over,
        });
    }
    fetched?;
    sink.flush()?;
    let got = sink.done;
    let hash = sink.hash.finalize();
    let got_hash: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    drop(sink.inner);
    if got != spec.size {
        return Err(Error::Size {
            expected: spec.size,
            got,
        });
    }
    if !progress(Progress::Checking) {
        return Err(Error::Stopped);
    }
    if got_hash != spec.sha256 {
        return Err(Error::Checksum {
            expected: spec.sha256.to_owned(),
            got: got_hash,
        });
    }
    if !progress(Progress::Unpacking) {
        return Err(Error::Stopped);
    }
    let out = work.join("unpacked");
    unpack(&zip_path, spec.folder, &out)?;
    if !out.join("metadata.json").is_file() {
        return Err(Error::Unpack("no metadata.json in the dictionary".into()));
    }
    fs::write(out.join(CHECKED), spec.sha256)?;
    Ok(out)
}

/// Unpacks the files under `folder/` in the zip into `out`, refusing any path that would leave
/// it.
fn unpack(zip_path: &Path, folder: &str, out: &Path) -> Result<(), Error> {
    let bad = |e: zip::result::ZipError| Error::Unpack(e.to_string());
    let mut zip = zip::ZipArchive::new(File::open(zip_path)?).map_err(bad)?;
    fs::create_dir_all(out)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(bad)?;
        let Some(name) = entry.enclosed_name() else {
            return Err(Error::Unpack(format!("unsafe path {}", entry.name())));
        };
        let Ok(rel) = name.strip_prefix(folder) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let path = out.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        let mut file = io::BufWriter::new(File::create(&path)?);
        io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    Ok(())
}

/// Fetches over HTTPS with a generic User-Agent (nothing about the listener is sent).
#[cfg(feature = "download")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Https;

#[cfg(feature = "download")]
impl Fetch for Https {
    fn fetch(
        &self,
        url: &str,
        into: &mut dyn Write,
        more: &mut dyn FnMut(u64) -> bool,
    ) -> Result<(), Error> {
        let net = |e: ureq::Error| Error::Network(e.to_string());
        let mut resp = ureq::get(url)
            .header(
                "User-Agent",
                "Venuta (https://github.com/venuta-lab/venuta)",
            )
            .call()
            .map_err(net)?;
        copy_all(&mut resp.body_mut().as_reader(), into, more)
    }
}

/// The dictionary's folder, downloading it first if needed (over [`Https`]).
#[cfg(feature = "download")]
pub fn ensure(
    app_data: &Path,
    spec: &Spec,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<PathBuf, Error> {
    install(app_data, spec, &Https, progress)
}

/// Reads all of `r` (for fakes in tests and for the app's own fetchers).
pub fn copy_all(
    r: &mut dyn Read,
    into: &mut dyn Write,
    more: &mut dyn FnMut(u64) -> bool,
) -> Result<(), Error> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        into.write_all(&buf[..n])?;
        done += n as u64;
        if !more(done) {
            return Err(Error::Stopped);
        }
    }
}
