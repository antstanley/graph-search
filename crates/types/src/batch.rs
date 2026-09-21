//! The write side of the store port: one atomic batch per reconcile
//! (`SPEC.md` §6.4).
//!
//! A batch is the whole delta: files to forget, projections to insert or
//! replace, and the manifest the store should hold once the batch is applied.
//! The store commits the manifest only after the graph writes succeed.

use crate::id::NodeId;
use crate::manifest::Manifest;
use crate::node::{Edge, Node};
use serde::{Deserialize, Serialize};

/// Why a file produced no projection (`SPEC.md` §6.2, §6.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineRecord {
    /// The workspace-relative path.
    pub path: String,
    /// A one-line reason: a parse failure, a node/edge overflow, or an
    /// unsupported encoding.
    pub reason: String,
}

impl QuarantineRecord {
    /// Records a quarantine reason for a path.
    #[must_use]
    pub fn new(path: &str, reason: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            reason: reason.into(),
        }
    }
}

/// The complete projection of one file: its `file` node, its symbols, and the
/// edges among them and to other files (`SPEC.md` §6.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileProjection {
    /// File-owned reference occurrences, independent of aggregate edges.
    #[serde(default)]
    pub occurrences: Option<crate::occurrence::OccurrenceFile>,
    /// The `file` node. Present for every walked file, parsed or not.
    pub file: Node,
    /// Hash-bound native source retrieval regions, including non-parser text.
    #[serde(default)]
    pub source: Option<crate::source::SourceFileUnits>,
    /// The symbol nodes, when the file was parsed.
    pub symbols: Vec<Node>,
    /// The edges: `contains` among the nodes above, and resolved or dangling
    /// cross-references.
    pub edges: Vec<Edge>,
    /// Set when extraction failed or overflowed: the file keeps its `file`
    /// node and nothing else (`SPEC.md` §6.4).
    pub quarantine: Option<QuarantineRecord>,
}

/// One atomic delta (`SPEC.md` §6.4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WriteBatch {
    /// Files whose previous projection is removed wholesale: their nodes and
    /// all incident edges.
    pub removed_files: Vec<String>,
    /// Complete projections to insert or replace, keyed by file path. Nodes with
    /// the same id, path and kind survive unless explicitly removed above.
    /// Replace edges owned by these source paths (explicit edge path, otherwise
    /// source-node path); retain untouched owners' edges to surviving endpoints.
    pub upserts: Vec<FileProjection>,
    /// The manifest state this batch produces. Committed last.
    pub manifest: Manifest,
}

impl WriteBatch {
    /// An empty batch carrying `manifest`.
    #[must_use]
    pub fn with_manifest(manifest: Manifest) -> Self {
        Self {
            removed_files: Vec::new(),
            upserts: Vec::new(),
            manifest,
        }
    }

    /// The number of nodes the batch writes (files plus symbols).
    #[must_use]
    pub fn node_writes(&self) -> usize {
        self.upserts.iter().fold(0usize, |total, up| {
            total.saturating_add(up.symbols.len()).saturating_add(1)
        })
    }

    /// The number of edges the batch writes.
    #[must_use]
    pub fn edge_writes(&self) -> usize {
        self.upserts
            .iter()
            .fold(0usize, |total, up| total.saturating_add(up.edges.len()))
    }

    /// Every node id the batch deletes: the nodes of removed files, plus the
    /// replaced nodes of upserted files.
    #[must_use]
    pub fn deletions(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self
            .removed_files
            .iter()
            .map(|path| NodeId::file(path))
            .collect();
        ids.extend(
            self.removed_files
                .iter()
                .map(|path| NodeId::file(path).to_string())
                .flat_map(|_| std::iter::empty()),
        );
        ids
    }
}

/// What the store reports after one batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyOutcome {
    /// Nodes inserted or replaced.
    pub nodes_upserted: u64,
    /// Edges inserted or replaced.
    pub edges_upserted: u64,
    /// Nodes deleted (with their incident edges).
    pub nodes_deleted: u64,
    /// Files whose projection the batch touched.
    pub files_touched: u64,
}
