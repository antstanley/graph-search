//! The embedded Grafeo engine behind core's [`GraphStore`](crate::store::GrafeoStore
//! "`GrafeoStore`") port.
//!
//! No Grafeo type crosses the port: ids and values are converted at this
//! boundary, exactly as `nanus` keeps `std::io::Error` out of its domain
//! (`SPEC.md` §4.3). The store lives at `<root>/.graph-search/index/`; the
//! manifest and the dangling-reference sidecar live beside it, so deleting
//! the directory is always a safe rebuild (`SPEC.md` §13).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod sidecar;
pub mod store;
pub mod value;

pub use store::{GrafeoSnapshot, GrafeoStore, StoreOptions};
