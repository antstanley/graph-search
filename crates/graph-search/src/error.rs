//! The library's error type: everything `core` raises, plus opening and
//! configuration failures.

/// Everything that can fail through the library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A failure from the domain, the store, or the walk.
    #[error(transparent)]
    Core(#[from] graph_search_core::Error),

    /// The configuration file could not be read or parsed.
    #[error("config error: {0}")]
    Config(String),

    /// The operation needs a writable index, and the index was opened
    /// read-only (`SPEC.md` §4.7).
    #[error("the index is read-only")]
    ReadOnly,

    /// A store lock was poisoned by a panic in another thread.
    #[error("the store lock is poisoned")]
    Poisoned,
}

impl Error {
    /// The poisoned-lock error.
    #[must_use]
    pub const fn poisoned() -> Self {
        Self::Poisoned
    }
}

/// A `Result` whose error is this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
