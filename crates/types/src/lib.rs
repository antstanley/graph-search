//! Canonical value types shared across the graph-search workspace: node and
//! edge kinds, ids, records, requests, results, and the JSON envelope.
//!
//! This crate is the leaf of the dependency graph: it depends on nothing
//! in-tree. Every other crate speaks this vocabulary (see `SPEC.md` §4.1).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod batch;
pub mod envelope;
pub mod extraction;
pub mod id;
pub mod kind;
pub mod limits;
pub mod manifest;
pub mod node;
pub mod query;
pub mod result;

pub use batch::{ApplyOutcome, FileProjection, QuarantineRecord, WriteBatch};
pub use envelope::Envelope;
pub use id::{EdgeId, NodeId};
pub use kind::{Direction, EdgeKind, Language, NodeKind, Visibility};
pub use limits::{MAX_HOPS_CEILING, PARSER_VERSION, SCHEMA_VERSION};
pub use manifest::{FileEntry, Manifest, Rename};
pub use node::{Edge, Node, Span};
pub use query::{
    DepsQuery, ExploreQuery, FilesQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery,
    TextQuery, TraversalQuery,
};
pub use result::{
    Approximation, DepthCount, EdgeHit, ExploreItem, ExploreResult, FileHit, FilesResult,
    GraphResult, ImpactResult, ImpactSummary, IndexStatus, Snippet, Staleness, Stats, StoreCounts,
    SymbolHit, SyncReport, TextHit, TextResult, Truncation, TruncationKind,
};

/// A node or edge paired with a relevance score.
///
/// Scores order results; they carry no probability meaning (see `SPEC.md` §8.4).
#[derive(Clone, Debug, PartialEq)]
pub struct Scored<T> {
    /// The scored item.
    pub item: T,
    /// The relevance score; higher is better.
    pub score: f32,
}

impl<T> Scored<T> {
    /// Pairs an item with a score.
    #[must_use]
    pub const fn new(item: T, score: f32) -> Self {
        Self { item, score }
    }
}

/// A point-in-time read view of part of the graph, returned by traversal.
///
/// Nodes and edges are deduplicated and deterministically ordered by the
/// producer (see `SPEC.md` §9.4).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Subgraph {
    /// The nodes in the subgraph.
    pub nodes: Vec<Node>,
    /// The edges among them.
    pub edges: Vec<Edge>,
}
