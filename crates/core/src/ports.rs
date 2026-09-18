//! The ports: the projected store, its read view, and one language's
//! extraction (`SPEC.md` §4.2).
//!
//! Vendor-free in their signatures: no Grafeo type and no tree-sitter type
//! crosses either trait. The read/write split is at the type level — `apply`
//! is the only mutator, and every read goes through a snapshot taken from the
//! store; a snapshot taken before an `apply` never sees that batch.

use crate::Result;
use graph_search_types::kind::{Direction, EdgeKind, NodeKind};
use graph_search_types::node::Node;
use graph_search_types::{Edge, NodeId, Scored, Subgraph};
use std::path::Path;

/// One source file handed to an extractor.
pub struct SourceFile<'a> {
    /// The workspace-relative path.
    pub path: &'a Path,
    /// The file's bytes (valid UTF-8; the walker rejects non-UTF-8).
    pub text: &'a str,
}

/// A parse or extraction failure. It quarantines a file; it never fails a
/// command (`SPEC.md` §6.4, §13).
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ParseError {
    /// A one-line reason for the quarantine record.
    pub message: String,
}

impl ParseError {
    /// Builds a parse error with `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// One language's extraction into nodes and edges (`SPEC.md` §4.2).
pub trait LanguageExtractor: Send + Sync {
    /// The language this extractor serves.
    fn language(&self) -> graph_search_types::Language;

    /// Whether this extractor claims `path` (by extension, including TSX/JSX
    /// spellings).
    fn supports(&self, path: &Path) -> bool;

    /// Extracts the file into symbols, references, and file annotations.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the file cannot be parsed; the caller
    /// quarantines the file rather than failing.
    fn extract(
        &self,
        file: &SourceFile<'_>,
    ) -> std::result::Result<crate::extraction::Extraction, ParseError>;
}

/// Answers "which extractor, if any, handles this path". The library wires a
/// registry over the language adapters; `core` never names one.
pub trait LanguageRegistry {
    /// The extractor that claims `path`, when its language is enabled.
    fn extractor_for(&self, path: &Path) -> Option<&dyn LanguageExtractor>;
}

/// A trivial registry over an owned list.
pub struct ListRegistry {
    extractors: Vec<Box<dyn LanguageExtractor>>,
}

impl ListRegistry {
    /// A registry over `extractors`, consulted in order.
    #[must_use]
    pub fn new(extractors: Vec<Box<dyn LanguageExtractor>>) -> Self {
        Self { extractors }
    }
}

impl LanguageRegistry for ListRegistry {
    fn extractor_for(&self, path: &Path) -> Option<&dyn LanguageExtractor> {
        self.extractors
            .iter()
            .map(std::convert::AsRef::as_ref)
            .find(|ex| ex.supports(path))
    }
}

/// The projected store the projector writes and the query engine reads
/// (`SPEC.md` §4.2).
pub trait GraphStore: Send {
    /// Applies one batch atomically; the manifest is committed separately,
    /// last (`SPEC.md` §6.4).
    ///
    /// # Errors
    ///
    /// When the store cannot complete the write; the batch is then abandoned
    /// whole and the old manifest still stands.
    fn apply(
        &mut self,
        batch: graph_search_types::WriteBatch,
    ) -> Result<graph_search_types::ApplyOutcome>;

    /// A point-in-time read view.
    ///
    /// # Errors
    /// When a snapshot cannot be taken.
    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>>;

    /// The committed manifest, when the store holds one.
    ///
    /// # Errors
    /// When the stored manifest cannot be read.
    fn manifest(&self) -> Result<Option<graph_search_types::Manifest>>;

    /// Commits the manifest after a successful apply (`SPEC.md` §6.4).
    ///
    /// # Errors
    /// When the manifest cannot be persisted.
    fn commit_manifest(&mut self, manifest: graph_search_types::Manifest) -> Result<()>;
}

/// A point-in-time read view of the store (`SPEC.md` §4.2).
pub trait GraphSnapshot {
    /// The node with this exact id.
    ///
    /// # Errors
    /// When the read fails.
    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>>;

    /// Symbols whose `name` or `qualified_name` equals `name`, best first,
    /// at most `k`. An empty `kinds` slice admits every kind.
    ///
    /// # Errors
    /// When the read fails.
    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>>;

    /// Edges incident to `id` along `kinds` in `dir`. An empty `kinds`
    /// slice admits every kind.
    ///
    /// # Errors
    /// When the read fails.
    fn edges_from(&self, id: &NodeId, kinds: &[EdgeKind], dir: Direction) -> Result<Vec<Edge>>;

    /// The subgraph within `hops` of `seeds` along `kinds` in `dir`.
    ///
    /// # Errors
    /// When the read fails.
    fn expand(
        &self,
        seeds: &[NodeId],
        hops: u8,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Subgraph>;

    /// Files whose path matches the glob, from the *indexed* set, at most
    /// `k`. This is the resident path set (`SPEC.md` §4.6).
    ///
    /// # Errors
    /// When the read or the glob fails.
    fn files_matching(&self, glob: &str, k: usize) -> Result<Vec<Node>>;

    /// Every node; the bulk read the reconcile's symbol table and `status`
    /// counts are built from. v1-scale by design (`SPEC.md` §19.3).
    ///
    /// # Errors
    /// When the read fails.
    fn all_nodes(&self) -> Result<Vec<Node>>;

    /// Every edge; the bulk read the cross-language match tables are built
    /// from (`SPEC.md` §7.3).
    ///
    /// # Errors
    /// When the read fails.
    fn all_edges(&self) -> Result<Vec<Edge>>;
}
