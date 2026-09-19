//! The error vocabulary every crate shares. An extraction failure is *not*
//! one of these: it quarantines (`SPEC.md` §6.4) — these are the errors a
//! command can fail with.

use std::path::PathBuf;

/// Everything that can fail operatively.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The store could not be opened, read, or written.
    #[error("store error: {0}")]
    Store(String),

    /// A filesystem operation failed at `path`.
    #[error("io error at {}: {}", path.display(), source)]
    Io {
        /// Where it failed.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },

    /// A glob pattern did not compile.
    #[error("invalid pattern {pattern:?}: {reason}")]
    InvalidPattern {
        /// The rejected pattern.
        pattern: String,
        /// Why it was rejected.
        reason: String,
    },

    /// An `include` filter was not one positive glob (`SPEC.md` §8.2).
    #[error("include {0}")]
    InvalidInclude(String),

    /// The workspace root does not exist.
    #[error("workspace root {} does not exist", root.display())]
    RootMissing {
        /// The missing root.
        root: PathBuf,
    },

    /// No usable index exists and the command refused to build one
    /// (`SPEC.md` §10.2, exit code 4).
    #[error("no usable index exists; run `graph-search index` first")]
    NoIndex,

    /// Another writer holds `<store>/index.lock` (`SPEC.md` §6.6).
    #[error("another index writer holds the lock: {holder}")]
    Locked {
        /// Who holds it, when discoverable.
        holder: String,
    },

    /// The configuration file could not be parsed.
    #[error("config error: {0}")]
    Config(String),

    /// A query target matched nothing of the requested kind.
    #[error("not found: {0}")]
    NotFound(String),

    /// A name matches multiple definitions; callers must use an exact id.
    #[error("ambiguous target: {0}; use `symbol` to choose an exact id")]
    Ambiguous(String),
}

impl Error {
    /// Wraps an io error with its path.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
