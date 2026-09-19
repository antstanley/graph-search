//! The domain, the port traits, the projector/reconcile, and the query engine.
//!
//! Depends only on `graph-search-types` and its own ports — never on an engine
//! or a parser (see `SPEC.md` §4.1). The engine and the parser implement the
//! ports defined here, and the `graph-search` library wires them in.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod conformance;
pub mod error;
pub mod extraction;
pub mod files_search;
pub mod hash;
pub mod lexical;
pub mod manifest;
pub mod memory;
pub mod ports;
pub mod query;
pub mod reconcile;
pub mod resolve;
pub mod stale;
pub mod text_search;
pub mod walk;

pub use error::Error;
pub use ports::{GraphSnapshot, GraphStore, LanguageExtractor, LanguageRegistry};

/// A `Result` whose error is this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
