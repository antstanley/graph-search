//! The result vocabulary of the read API (`SPEC.md` §9).
//!
//! Every result is bounded and honest: caps that fired are reported in
//! `truncations`, graph answers carry `approximation`, and index-sourced
//! answers carry staleness. Nothing is dropped silently.

use crate::kind::{Language, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which cap fired (`SPEC.md` §9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationKind {
    /// Positional verification exceeded its original-byte scan allowance.
    PositionalBytes,
    /// Positional verification exceeded its token comparison allowance.
    PositionalTokens,
    /// Positional verification omitted a witness beyond its request allowance.
    PositionalWitnesses,
    /// Unscored metadata records exceeded their independent examination budget.
    MetadataEntries,
    /// Source-file open attempts exceeded their independent work budget.
    SourceFiles,
    /// Context-window cost evaluations exceeded their independent work budget.
    ContextWindows,
    /// Raw occurrence entries exceeded their independent work budget.
    Occurrences,
    /// The independent delivered-edge limit omitted relationships.
    ReturnedEdges,
    /// Prefix expansion exceeded its dictionary-entry cap.
    DictionaryEntries,
    /// Request-local source reads exceeded their total byte budget.
    SourceBytes,
    /// A source read exceeded the per-file byte ceiling.
    SourceFileBytes,
    /// Per-file source-region indexing reached its cap.
    SourceUnits,
    /// Retrieval candidate admissions exceeded their work cap.
    Candidates,
    /// Lexical postings exceeded their work cap.
    Postings,
    /// Distinct graph nodes exceeded the work budget.
    GraphNodes,
    /// Adjacency entries exceeded the work budget.
    GraphEdges,
    /// The result list was cut.
    Results,
    /// A match line was shortened.
    MatchLine,
    /// A stored signature was shortened.
    Signature,
    /// A snippet was cut.
    Snippet,
    /// The file list was cut.
    Files,
    /// The directory-entry enumeration budget was exhausted.
    WalkEntries,
    /// The whole payload hit the byte budget.
    Bytes,
}

/// A cap that fired, reported rather than silent (`SPEC.md` §9.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncation {
    /// Which cap fired.
    pub kind: TruncationKind,
    /// The cap value.
    pub cap: u64,
    /// Whether anything was actually dropped.
    pub dropped: bool,
    /// The human-readable sentence; the text renderer prints it verbatim.
    pub message: String,
}

impl Truncation {
    /// Records a fired cap.
    #[must_use]
    pub fn new(kind: TruncationKind, cap: u64, message: impl Into<String>) -> Self {
        Self {
            kind,
            cap,
            dropped: true,
            message: message.into(),
        }
    }
}

/// The honesty block every `graph`/`explore` answer carries (`SPEC.md` §7.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approximation {
    /// Edges that resolved to a workspace node.
    pub resolved: u64,
    /// References kept dangling, with the name they referred to.
    pub unresolved: u64,
    /// The standing caveat.
    pub note: String,
}

impl Default for Approximation {
    fn default() -> Self {
        Self {
            resolved: 0,
            unresolved: 0,
            note: String::from(
                "static approximation; dynamic/macro/generated edges may be missing",
            ),
        }
    }
}

/// Counters for one answer. `elapsed_ms` is the only field allowed to differ
/// between two runs over an unchanged tree (`SPEC.md` §9.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// Unscored metadata records examined, including those rejected by filters.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub metadata_entries_examined: u64,
    /// Query source open attempts, including freshness checks and failed reads.
    /// Includes automatic index-maintenance reads.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub source_files_attempted: u64,
    /// Query source bytes read, including freshness, invalid files and overflow probes.
    /// Includes automatic index-maintenance reads.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub source_bytes_read: u64,
    /// Original bytes examined by positional verification across source fields.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub positional_bytes_examined: u64,
    /// Complete lexemes compared by positional verification.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub positional_tokens_examined: u64,
    /// Positional witnesses retained before result packing.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub positional_witnesses: u64,
    /// Context-window cost evaluations, including re-evaluations after source admission.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub context_windows_examined: u64,
    /// Raw occurrence entries examined, including filtered entries.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub occurrences_examined: u64,
    /// Dictionary entries expanded by explicit prefix retrieval.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub dictionary_entries_examined: u64,
    /// Native metadata candidate admissions.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub retrieval_candidates_admitted: u64,
    /// Lexical posting entries examined.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub lexical_postings_examined: u64,
    /// Distinct graph node identities examined under the work budget.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub graph_nodes_visited: u64,
    /// Adjacency entries examined, including entries rejected by filters.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub graph_edges_examined: u64,
    /// Files the walk considered.
    #[serde(default, skip_serializing_if = "zero_count")]
    pub files_scanned: u64,
    /// Matches found (`text`).
    #[serde(default, skip_serializing_if = "zero_count")]
    pub matches: u64,
    /// Candidate definitions considered (`graph`, `explore`).
    #[serde(default, skip_serializing_if = "zero_count")]
    pub candidates: u64,
    /// Wall time in milliseconds.
    pub elapsed_ms: u64,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde predicate signature
fn zero_count(value: &u64) -> bool {
    *value == 0
}

/// One `files` hit (`SPEC.md` §9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHit {
    /// The workspace-relative path.
    pub path: String,
    /// The file's language.
    pub language: Language,
}

/// One `text` hit (`SPEC.md` §9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextHit {
    /// The workspace-relative path.
    pub path: String,
    /// The 1-based line number.
    pub line: u64,
    /// The line text, capped at `MAX_MATCH_LINE` with an ellipsis.
    pub text: String,
}

/// One symbol in a `graph` result (`SPEC.md` §9.1).
///
/// Ordered by id (then the rest), which is the determinism order for nodes
/// after score ranking.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SymbolHit {
    /// The stable node id.
    pub id: String,
    /// The bare name.
    pub name: String,
    /// The lexical path.
    pub qualified_name: String,
    /// The kind.
    pub kind: NodeKind,
    /// The workspace-relative path.
    pub path: String,
    /// First line, 1-based.
    pub start_line: u32,
    /// Last line, inclusive.
    pub end_line: u32,
    /// The one-line truncated signature, when known.
    pub signature: Option<String>,
}

/// One edge in a `graph` result (`SPEC.md` §9.1).
///
/// Ordered by from, then kind, then to — the spec's determinism order for
/// edges (`SPEC.md` §9.4).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeHit {
    /// The source node id.
    pub from: String,
    /// The edge kind.
    pub kind: crate::kind::EdgeKind,
    /// The target node id; `None` when dangling.
    pub to: Option<String>,
    /// The name the target was referred by.
    pub to_name: String,
    /// The file the reference occurs in.
    pub path: Option<String>,
    /// The 1-based line of the reference.
    pub line: Option<u32>,
    /// Whether the target resolved.
    pub resolved: bool,
    /// Number of indexed raw reference occurrences for this aggregate edge.
    /// Absent for legacy or synthetic relationships without occurrence facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_count: Option<usize>,
}

impl SymbolHit {
    /// Projects a stored node into the result shape.
    #[must_use]
    pub fn of(node: &crate::node::Node) -> Self {
        Self {
            id: node.id.to_string(),
            name: node.display_name().to_owned(),
            qualified_name: node
                .qualified_name
                .clone()
                .unwrap_or_else(|| node.path.clone()),
            kind: node.kind,
            path: node.path.clone(),
            start_line: node.span.map_or(0, |s| s.start_line),
            end_line: node.span.map_or(0, |s| s.end_line),
            signature: node.signature.clone(),
        }
    }
}

impl EdgeHit {
    /// Projects a stored edge into the result shape.
    #[must_use]
    pub fn from_edge(edge: &crate::node::Edge) -> Self {
        Self {
            from: edge.from.to_string(),
            kind: edge.kind,
            to: edge.to.as_ref().map(ToString::to_string),
            to_name: edge.to_name.clone(),
            path: edge.path.clone(),
            line: edge.line,
            resolved: edge.resolved,
            occurrence_count: None,
        }
    }
}

/// One depth ring of an impact answer (`SPEC.md` §8.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepthCount {
    /// Hops from the seed, 1-based.
    pub depth: u8,
    /// How many nodes the ring holds in total.
    pub total: u64,
    /// The ring's nodes by kind.
    pub by_kind: BTreeMap<NodeKind, u64>,
}

/// The blast-radius summary attached to an `explore` seed (`SPEC.md` §8.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactSummary {
    /// Direct callers.
    pub direct_callers: u64,
    /// Transitive callers within the query's depth.
    pub total_callers: u64,
}

/// A bounded source excerpt (`SPEC.md` §9.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// BLAKE3 of the original file bytes from which these lines were read.
    #[serde(default)]
    pub source_hash: String,
    /// The 1-based line of the first line in `lines`.
    pub start_line: u32,
    /// The excerpt, one entry per line.
    pub lines: Vec<String>,
}

/// Why an additional source interval was selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcerptRole {
    /// Declaration header or a small complete implementation.
    Declaration,
    /// Region containing matched body terms.
    Body,
    /// Site supporting a returned graph relationship.
    Reference,
    /// Authored Markdown heading providing parent document context.
    DocumentHeading,
    /// Original fence delimiter/info line providing context for a code fragment.
    DocumentFence,
    /// Authored table header/delimiter lines giving a row group its labels.
    DocumentTableHeader,
}

/// A labeled, contiguous interval of verified original source lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceExcerpt {
    /// Retrieval reason, not a completeness claim about the enclosing symbol.
    pub role: ExcerptRole,
    /// Verbatim source with original line numbers and full-file fingerprint.
    pub snippet: Snippet,
}

/// One assembled seed of an `explore` answer (`SPEC.md` §8.4, §9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExploreItem {
    /// Optional per-channel diagnostic support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<crate::retrieval::RetrievalEvidence>,
    /// Additional verified intervals, without lines already delivered elsewhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excerpts: Vec<SourceExcerpt>,
    /// Matched body region, kept separate from graph declaration coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<crate::source::SourceEvidence>,
    /// The definition.
    pub node: SymbolHit,
    /// The bounded excerpt around it, when context was requested.
    pub snippet: Option<Snippet>,
    /// The one-line blast radius, for function/method seeds.
    pub impact: Option<ImpactSummary>,
}

/// A `graph` mode answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphResult {
    /// Generation, freshness, and source identity for this answer.
    #[serde(default)]
    pub context: crate::context::ResultContext,
    /// The nodes, ordered by the mode's ranking then path then line.
    pub nodes: Vec<SymbolHit>,
    /// The edges among (or pointing at) the nodes.
    pub edges: Vec<EdgeHit>,
    /// Caps that fired.
    pub truncations: Vec<Truncation>,
    /// The honesty block; always present for graph modes.
    pub approximation: Option<Approximation>,
    /// Counters.
    pub stats: Stats,
}

/// An `impact` answer: the shape of the cone, not the whole cone
/// (`SPEC.md` §8.3).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImpactResult {
    /// Generation, freshness, and source identity for this answer.
    #[serde(default)]
    pub context: crate::context::ResultContext,
    /// Counts by depth and kind.
    pub by_depth: Vec<DepthCount>,
    /// The top nodes of the cone.
    pub top: Vec<SymbolHit>,
    /// Edges among the reported nodes.
    pub edges: Vec<EdgeHit>,
    /// Caps that fired.
    pub truncations: Vec<Truncation>,
    /// The honesty block.
    pub approximation: Option<Approximation>,
    /// Counters.
    pub stats: Stats,
}

/// An `explore` answer (`SPEC.md` §8.4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExploreResult {
    /// Optional query plan and executed routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<crate::retrieval::RetrievalPlan>,
    /// Generation, freshness, and source identity for this answer.
    #[serde(default)]
    pub context: crate::context::ResultContext,
    /// The assembled seeds, best first.
    pub items: Vec<ExploreItem>,
    /// Edges among the returned nodes, up to `hops`.
    pub edges: Vec<EdgeHit>,
    /// Caps that fired, including the byte budget.
    pub truncations: Vec<Truncation>,
    /// The honesty block.
    pub approximation: Option<Approximation>,
    /// Counters.
    pub stats: Stats,
}

/// A `files` answer (`SPEC.md` §8.1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FilesResult {
    /// Generation, freshness, and source identity for this answer.
    #[serde(default)]
    pub context: crate::context::ResultContext,
    /// The matching files.
    pub items: Vec<FileHit>,
    /// Caps that fired.
    pub truncations: Vec<Truncation>,
    /// Counters.
    pub stats: Stats,
}

/// A `text` answer (`SPEC.md` §8.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextResult {
    /// Generation, freshness, and source identity for this answer.
    #[serde(default)]
    pub context: crate::context::ResultContext,
    /// The matching lines, grouped by file.
    pub items: Vec<TextHit>,
    /// Caps that fired.
    pub truncations: Vec<Truncation>,
    /// Counters.
    pub stats: Stats,
}

/// Node and edge counts by kind, as `status` reports them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreCounts {
    /// Nodes by kind.
    pub nodes: BTreeMap<NodeKind, u64>,
    /// Edges by kind.
    pub edges: BTreeMap<crate::kind::EdgeKind, u64>,
    /// File nodes by language.
    pub files_by_language: BTreeMap<Language, u64>,
    /// Total nodes.
    pub total_nodes: u64,
    /// Total edges.
    pub total_edges: u64,
}

/// Why (and how far behind) an index is behind the tree (`SPEC.md` §6.5).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staleness {
    /// Paths whose observed metadata or content differs, sorted.
    /// Status may omit a suffix under its byte cap and report a coverage notice.
    pub changed_paths: Vec<String>,
    /// Total observed changed paths, including details omitted from status.
    pub changed: u64,
}

/// What a completed `status` inspection reports (`SPEC.md` §8.5).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatus {
    /// Generation whose counts and manifest were inspected.
    #[serde(default)]
    pub generation: Option<String>,
    /// Coverage of the status freshness check.
    #[serde(default)]
    pub coverage: crate::coverage::Coverage,
    /// Whether a store exists at the store path.
    pub exists: bool,
    /// The store path (absolute).
    pub store_path: String,
    /// The workspace root (absolute).
    pub root: String,
    /// The schema version a fresh build would write.
    pub schema_version: u32,
    /// The parser version a fresh build would write.
    pub parser_version: u32,
    /// Counts from the store, when it exists.
    pub counts: Option<StoreCounts>,
    /// When the last complete index finished (epoch ms), when known.
    pub indexed_at_ms: Option<u64>,
    /// How far behind the index is, when it exists.
    pub staleness: Option<Staleness>,
}

/// Exact reconciliation totals, independent of delivered detail lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncCounts {
    /// Files inserted (includes the destination of a rename).
    pub added: u64,
    /// Files replaced or rebound.
    pub modified: u64,
    /// Files removed (includes the source of a rename).
    pub removed: u64,
    /// Recognized moves; overlaps added and removed.
    pub renamed: u64,
    /// Files quarantined during this reconciliation.
    pub quarantined: u64,
}

/// One reconcile class per path (`SPEC.md` §6.3); the `sync` report.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Exact totals before detail truncation; absent in older serialized reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<SyncCounts>,
    /// Enumeration and parser coverage of this reconciliation.
    #[serde(default)]
    pub coverage: crate::coverage::Coverage,
    /// Paths parsed and inserted.
    pub added: Vec<String>,
    /// Paths replaced after parsing or rebinding cached extraction facts.
    pub modified: Vec<String>,
    /// Paths whose projection was deleted.
    pub removed: Vec<String>,
    /// Paths whose content hash matched a removed path: a move.
    pub renamed: Vec<crate::manifest::Rename>,
    /// Paths skipped by the O(1) no-op path.
    pub unchanged: u64,
    /// Files that failed extraction, with reasons.
    pub quarantined: Vec<crate::batch::QuarantineRecord>,
    /// Whether a parser/schema bump forced a full re-parse.
    pub reindexed_all: bool,
    /// Wall time in milliseconds.
    pub elapsed_ms: u64,
}

impl SyncReport {
    /// Exact totals, falling back to complete detail lists in older reports.
    #[must_use]
    pub fn totals(&self) -> SyncCounts {
        self.counts.unwrap_or(SyncCounts {
            added: self.added.len() as u64,
            modified: self.modified.len() as u64,
            removed: self.removed.len() as u64,
            renamed: self.renamed.len() as u64,
            quarantined: self.quarantined.len() as u64,
        })
    }

    /// Whether the report contains no classified paths, including unchanged paths.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let totals = self.totals();
        totals.added == 0
            && totals.modified == 0
            && totals.removed == 0
            && totals.renamed == 0
            && self.unchanged == 0
    }
}
