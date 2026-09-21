//! The `graph-search` library: the in-process search service.
//!
//! This crate is the product. A host — the CLI, the evaluation harness, or a
//! future `nanus` adapter — opens an [`Index`] for a workspace and calls
//! [`SearchService`] **in-process**. That is shape 3 in `SPEC.md` §11.1: no
//! IPC, no per-call process startup, and therefore the behaviour we measure
//! is the behaviour a linked-in `nanus` tool would get.
//!
//! The CLI (`graph-search-cli`) is a thin client over this API; the public
//! surface is specified in `SPEC.md` §4.7.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cargo_targets;
pub mod config;
pub mod error;
pub mod index;
pub mod lock;
pub mod module_presence;
mod node_package;
mod package_manifests;
mod pnpm_workspace;
pub mod service;
mod typescript_config;
mod typescript_order;

pub use error::Error;
pub use graph_search_core as core;
pub use graph_search_core::work::{CancellationToken, WorkLimits};
pub use index::{Index, OpenOptions, Reconcile, Verification};
pub use service::SearchService;

/// A `Result` whose error is this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
