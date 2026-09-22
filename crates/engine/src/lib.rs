//! The embedded Grafeo engine behind core's [`GraphStore`](crate::store::GrafeoStore
//! "`GrafeoStore`") port.
//!
//! No Grafeo type crosses the port: ids and values are converted at this
//! boundary, exactly as `nanus` keeps `std::io::Error` out of its domain
//! (`SPEC.md` §4.3). The store lives at `<root>/.graph-search/index/`; the
//! manifest and dangling references are published in native generations, so deleting
//! the directory is always a safe rebuild (`SPEC.md` §13).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod compress;
mod generation;
mod manifest_records;
mod record_codec;
pub mod sidecar;
mod source_records;
pub mod store;
pub mod value;

pub use store::{GrafeoSnapshot, GrafeoStore, StoreOptions};
