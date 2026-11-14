use std::path::{Path, PathBuf};

/// Everything that can go wrong in wallpick; no operation panics on bad input or network.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cancelled")]
    Cancelled,
    #[error("{url}: {source}")]
    Http {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("{url}: response is larger than {limit} bytes")]
    TooLarge { url: String, limit: u64 },
    #[error("{file}: SHA-256 mismatch (expected {expected}, got {actual})")]
    Checksum {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{what}: {source}")]
    Json {
        what: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{what}: {source}")]
    Image {
        what: String,
        #[source]
        source: image::ImageError,
    },
    #[error("model: {0}")]
    Model(String),
    #[error("tokenizer: {0}")]
    Tokenizer(String),
    #[error("no wallpaper candidates: {0}")]
    NoCandidates(String),
    #[error("no current wallpaper picked by wallpick")]
    NoCurrent,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub(crate) fn model(e: impl std::fmt::Display) -> Error {
    Error::Model(format!("{e:#}"))
}
