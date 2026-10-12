use std::path::PathBuf;

/// Engine-wide error type. Per-file read problems are not errors: they are recorded
/// on the file's index row so one bad file never stops a scan.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("index database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("library root does not exist or is not a directory: {0}")]
    BadRoot(PathBuf),

    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),

    #[error("index belongs to library root {indexed}, not {requested}")]
    RootMismatch { indexed: String, requested: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.into(),
            source,
        })
    }
}
