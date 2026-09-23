//! The domain, the port traits, the projector/reconcile, and the query engine.
//!
//! Depends only on `graph-search-types` and its own ports — never on an engine
//! or a parser (see `SPEC.md` §4.1). The engine and the parser implement the
//! ports defined here, and the `graph-search` library wires them in.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod adjacency;
pub mod analyzer;
mod binding_surface;
pub mod body;
pub mod config;
pub mod conformance;
mod connections;
mod context_dedup;
mod context_proximity;
pub mod counts;
pub mod dependencies;
pub mod error;
pub mod evidence;
pub mod extraction;
pub mod files_search;
pub mod hash;
mod intersection;
mod js_modules;
pub mod lexical;
pub mod manifest;
mod markdown;
pub mod memory;
pub mod metadata;
pub mod mutation;
mod neighborhoods;
mod node_packages;
mod node_workspaces;
pub mod occurrences;
mod packages;
pub mod payload;
pub mod ports;
pub mod positional;
pub mod query;
mod query_policy;
pub mod reconcile;
pub mod resolve;
pub mod retention;
mod rust_modules;
mod rust_paths;
mod rust_receivers;
pub mod source;
mod source_capture;
pub mod stale;
pub mod text_search;
pub mod typescript;
pub mod typescript_aliases;
pub mod typescript_files;
pub mod typescript_patterns;
mod typescript_project;
pub mod typescript_roots;
pub mod units;
pub mod walk;
pub mod work;

pub use error::Error;
pub use ports::{GraphSnapshot, GraphStore, LanguageExtractor, LanguageRegistry};

/// A `Result` whose error is this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
