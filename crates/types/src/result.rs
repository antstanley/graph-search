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
    /// Files the walk considered.
    pub files_scanned: u64,
    /// Matches found (`text`).
    pub matches: u64,
    /// Candidate definitions considered (`graph`, `explore`).
    pub candidates: u64,
    /// Wall time in milliseconds.
    pub elapsed_ms: u64,
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
    /// The 1-based line of the first line in `lines`.
    pub start_line: u32,
    /// The excerpt, one entry per line.
    pub lines: Vec<String>,
}

/// One assembled seed of an `explore` answer (`SPEC.md` §8.4, §9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExploreItem {
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
    /// Paths whose size or mtime differs from the manifest, sorted.
    pub changed_paths: Vec<String>,
    /// How many paths changed.
    pub changed: u64,
}

/// What `status` reports (`SPEC.md` §8.5). Exit code is 0 either way.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatus {
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

/// One reconcile class per path (`SPEC.md` §6.3); the `sync` report.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Paths parsed and inserted.
    pub added: Vec<String>,
    /// Paths re-parsed and replaced.
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
    /// Whether anything changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.modified.is_empty()
            && self.removed.is_empty()
            && self.renamed.is_empty()
            && self.unchanged == 0
    }
}
