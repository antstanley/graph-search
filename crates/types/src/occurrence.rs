//! Source-owned reference occurrences, independent of deduplicated graph adjacency.
use crate::{EdgeId, EdgeKind, NodeId, Span};
use serde::{Deserialize, Serialize};

/// Evidence strength of a static target binding, not a confidence probability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionClass {
    /// A directly selected lexical declaration.
    ExplicitLexical,
    /// Explicit import/module provenance.
    ExplicitImport,
    /// A unique compatible same-file name.
    SameFile,
    /// An exact whole qualified name.
    Qualified,
    /// A unique workspace name; visibility is not compiler-proven.
    UniqueName,
    /// No static target is established.
    #[default]
    Unresolved,
}

/// What the original source coordinates identify.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceExtent {
    /// Complete call/reference expression, including arguments where applicable.
    Expression,
    /// An enclosing syntax construct; not a unique token position.
    EnclosingSyntax,
    /// Legacy/adapter facts provide only a line.
    #[default]
    LineOnly,
}

/// One extracted reference. Its identity excludes the independently mutable target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceOccurrence {
    /// Source-derived opaque identity, stable across rebinding unchanged bytes.
    pub id: String,
    /// File or declaration containing the source occurrence.
    pub owner: NodeId,
    /// Relationship kind emitted by the extractor.
    pub kind: EdgeKind,
    /// Original half-open byte range, when supplied by the adapter.
    pub span: Option<Span>,
    /// Original one-based source line.
    pub line: u32,
    /// Precision of the supplied source coordinates.
    pub extent: OccurrenceExtent,
    /// Original callee spelling when the adapter preserves it.
    pub raw_name: Option<String>,
    /// Parser reference name before workspace binding.
    pub name: String,
    /// Position in the file's raw reference list; disambiguates coarse adapter facts.
    pub ordinal: u32,
    /// Selected target, independently replaceable without changing this occurrence.
    pub target: Option<NodeId>,
    /// Target display name, or the unresolved reference name.
    pub target_name: String,
    /// Evidence class used to select the target.
    pub resolution: ResolutionClass,
    /// Unresolved reason when no target is established.
    pub reason: Option<String>,
    /// File-local lexical scope ordinal, when known.
    pub scope: Option<usize>,
    /// File-local binding ordinal, when known.
    pub binding: Option<usize>,
}
impl ReferenceOccurrence {
    /// The aggregate graph relationship to which this occurrence currently contributes.
    #[must_use]
    pub fn edge_id(&self) -> EdgeId {
        EdgeId::of(
            &self.owner,
            self.kind,
            self.target
                .as_ref()
                .map_or(self.target_name.as_str(), NodeId::as_str),
        )
    }
}

/// Facts owned by one file/source version, published with the graph generation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceFile {
    /// Original full-file content hash.
    pub source_hash: String,
    /// Occurrence representation revision; zero means unknown legacy facts.
    pub version: u32,
    /// Whether reference extraction ran successfully for this file.
    pub complete: bool,
    /// Every raw reference occurrence, in deterministic extraction order.
    pub records: Vec<ReferenceOccurrence>,
}

/// Exact occurrence lookup route; names are raw reference spellings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceBy {
    /// Resolve a target declaration name or id, then find its references.
    #[default]
    Target,
    /// Resolve an owning declaration name or id, then find references inside it.
    Owner,
    /// Match raw spelling exactly, including unresolved references.
    Name,
}

/// Bounded source occurrence lookup, separate from deduplicated graph traversal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceQuery {
    /// Declaration name/id or exact raw spelling, depending on `by`.
    pub target: String,
    /// Which native occurrence index to use.
    pub by: OccurrenceBy,
    /// Optional relationship-kind restriction.
    pub kind: Option<EdgeKind>,
    /// Source-file language/path filters.
    pub filters: crate::query::GraphFilters,
    /// Maximum records; zero selects the graph default, hard ceiling 500.
    pub limit: u32,
}

/// An indexed reference with its source identity, never an unverified live span.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceHit {
    /// Workspace-relative source file.
    pub path: String,
    /// Hash of the original source bytes that these coordinates describe.
    pub source_hash: String,
    /// Individual reference evidence and current indexed binding.
    pub occurrence: ReferenceOccurrence,
}

/// Individual indexed references in deterministic file/extraction order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OccurrenceResult {
    /// Generation, freshness and source verification for the answer.
    pub context: crate::context::ResultContext,
    /// Returned source occurrences.
    pub items: Vec<OccurrenceHit>,
    /// Number of indexed files with occurrence metadata, including unavailable extraction.
    pub indexed_files: usize,
    /// Files whose reference extractor ran successfully; not compiler completeness.
    pub extracted_files: usize,
    /// Result and work caps which omitted evidence.
    pub truncations: Vec<crate::result::Truncation>,
    /// Actual query work.
    pub stats: crate::result::Stats,
}
