//! The `graph-search` library: the in-process search service.
//!
//! This crate is the product. A host — the CLI, the evaluation harness, or a
//! future `nanus` adapter — opens an [`Index`] for a workspace and calls
//! [`SearchService`] **in-process**. That is shape 3 in `SPEC.md` §11.1: no IPC,
//! no per-call process startup, and therefore the behaviour we measure is the
//! behaviour a linked-in `nanus` tool would get.
//!
//! The CLI (`graph-search-cli`) is a thin client over this API; the public
//! surface is specified in `SPEC.md` §4.7.
//!
//! Placeholder: populated from milestone M1 (see `SPEC.md` §17).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// A workspace index, opened by a host and held for as long as it is querying.
///
/// Placeholder for the real type (`SPEC.md` §4.7): `Index::open(root)` builds or
/// opens the store, and `Index::search()` lends a [`SearchService`].
pub struct Index;

/// The query handle a host calls in-process.
///
/// Placeholder for the real service API (`SPEC.md` §4.7): `files`, `text`,
/// `graph.*`, `explore`, `status`.
pub struct SearchService;
