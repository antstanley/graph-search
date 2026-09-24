//! The native generation store behind core's [`GraphStore`](crate::store::NativeStore
//! "`NativeStore`") port (`SPEC.md` §4.3, storage format 10).
//!
//! Per-file shards in content-addressed packs and base-plus-delta posting
//! tables, published as immutable generations
//! (`research/16-proportional-sync.md`). No storage type crosses the port.
//! The store lives at `<root>/.graph-search/index/`; deleting the directory is
//! always a safe rebuild (`SPEC.md` §13).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod compress;
mod durable;
mod generation;
mod manifest_records;
mod record_codec;
mod segment;
mod shards;
pub mod sidecar;
mod source_records;
pub mod store;

pub use store::{NativeSnapshot, NativeStore, StoreOptions};
