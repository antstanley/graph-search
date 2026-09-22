//! Structured explore intent and independently configurable retrieval policies.
use serde::{Deserialize, Serialize};

/// Relationships used to enrich lexical evidence; independent of candidate ranking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphContext {
    /// Existing semantic relationships, excluding structural containment.
    #[default]
    Semantic,
    /// Preserve lexical evidence without connection or caller-impact traversal.
    None,
    /// Call connections and caller-impact summaries.
    Calls,
    /// Import connections, without caller-impact summaries.
    Imports,
    /// Type uses, implementation and inheritance connections, without caller impact.
    Types,
}

impl GraphContext {
    /// Allowed undirected connection steps; returned edges retain their direction.
    #[must_use]
    pub fn relations(self) -> &'static [crate::EdgeKind] {
        use crate::EdgeKind::{Calls, Extends, Implements, Imports, References, TypeUses};
        match self {
            Self::Semantic => &[Calls, References, TypeUses, Imports, Implements, Extends],
            Self::None => &[],
            Self::Calls => &[Calls],
            Self::Imports => &[Imports],
            Self::Types => &[TypeUses, Implements, Extends],
        }
    }

    /// Whether function/method results include bounded incoming-call summaries.
    #[must_use]
    pub fn includes_impact(self) -> bool {
        matches!(self, Self::Semantic | Self::Calls)
    }
}

/// Explicit navigation bypasses ranked discovery instead of silently broadening it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExploreMode {
    /// Ranked discovery, with exact names protected and an optional exact fast path.
    #[default]
    Auto,
    /// Case-sensitive bare or qualified name lookup only.
    ExactName,
    /// Stable graph identifier lookup only.
    ExactId,
    /// Case-sensitive prefix of a complete bare or qualified name, without fuzzy correction.
    NamePrefix,
    /// Workspace-relative anchored path/glob navigation only.
    PathGlob,
    /// Analyzed terms without inferred navigation.
    Terms,
    /// Ordered whole lexemes, preserving stopwords and repeated positions.
    Phrase,
    /// Unordered whole lexemes within an inclusive token window.
    Near,
}

/// Explicit analyzer comparison; split-only remains the baseline until measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMode {
    /// Existing split identifier terms and name/path/signature fields.
    #[default]
    Split,
    /// Whole lexemes plus split aliases, with a separate qualified-name field.
    Identifiers,
}

/// Retrieval-channel ablations use the same candidate, context and work budgets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingStrategy {
    /// Multiword discovery runs body first, with metadata fallback when empty;
    /// single-token navigation retains exact-priority fusion.
    #[default]
    Auto,
    /// Exact tier followed by reciprocal-rank fusion.
    Fusion,
    /// Metadata candidates only.
    Metadata,
    /// Source-region candidates only.
    Body,
}

/// Metadata length normalization; field weights and clipped IDF stay fixed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldNormalization {
    /// Normalize weighted frequency against the combined document length.
    #[default]
    Combined,
    /// Normalize each field frequency, combine weights, then saturate once.
    Bm25f,
}

/// Boolean requirements apply to one metadata document or source region.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TermMatch {
    /// Conservative OR discovery.
    #[default]
    Any,
    /// All analyzed query terms must occur in the candidate.
    All,
    /// At least this many distinct analyzed terms must occur.
    AtLeast(u16),
}
impl TermMatch {
    /// Required distinct matches; zero thresholds still require a real match.
    #[must_use]
    pub fn minimum(self, terms: usize) -> usize {
        match self {
            Self::Any => 1,
            Self::All => terms.max(1),
            Self::AtLeast(n) => usize::from(n).max(1),
        }
    }
}

/// Explicit interpretation of procedural text in ranked discovery queries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryPolicy {
    /// Analyze the complete original input.
    #[default]
    Verbatim,
    /// Remove only the documented standalone procedural suffix sentences.
    Task,
}

/// How test-owned code ranks in discovery (`SPEC.md` § Test-owned symbols).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestRanking {
    /// Non-exact test-owned hits follow every other hit, unless the query
    /// itself asks about tests; they still fill slots nothing else takes.
    #[default]
    Defer,
    /// Test-owned hits rank like any other, for ablations.
    Neutral,
}

/// Structured policy, independent of the original query string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrievalOptions {
    /// Opt-in procedural suffix cleanup for Auto/Terms only.
    pub query_policy: QueryPolicy,
    /// Explicit route, protected from discovery fallback.
    pub mode: ExploreMode,
    /// Query/index representation used for ranked discovery.
    pub analysis: AnalysisMode,
    /// Channels to run for ranked discovery.
    pub ranking: RankingStrategy,
    /// Metadata normalization only; body and positional scoring are independent.
    pub normalization: FieldNormalization,
    /// Relation-specific context; `None` performs no connection/impact expansion.
    pub graph_context: GraphContext,
    /// OR, AND, or minimum term coverage.
    pub term_match: TermMatch,
    /// Total intervening tokens for phrase mode (0 means adjacency; maximum 4096).
    pub phrase_gap: u16,
    /// Inclusive token window for near mode (maximum 4096).
    pub near_window: u16,
    /// For automatic mode, stop at exact names when any survive filtering.
    pub exact_fast_path: bool,
    /// First-pass entities per file; zero disables file diversification.
    pub per_file: u16,
    /// How test-owned symbols and files rank.
    pub tests: TestRanking,
    /// Include the original query, executed routes and per-channel ranks.
    pub explain: bool,
}
impl Default for RetrievalOptions {
    fn default() -> Self {
        Self {
            mode: ExploreMode::Auto,
            query_policy: QueryPolicy::Verbatim,
            ranking: RankingStrategy::Auto,
            normalization: FieldNormalization::Combined,
            graph_context: GraphContext::Semantic,
            analysis: AnalysisMode::Split,
            term_match: TermMatch::Any,
            phrase_gap: 0,
            near_window: 8,
            exact_fast_path: false,
            per_file: 0,
            tests: TestRanking::Defer,
            explain: false,
        }
    }
}

/// Actual route used by the current request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalRoute {
    /// Exact name map.
    ExactName,
    /// Direct graph identifier.
    ExactId,
    /// Bounded whole-name dictionary prefix enumeration.
    NamePrefix,
    /// Cached file paths and a compiled glob.
    PathGlob,
    /// Metadata postings.
    Metadata,
    /// Source-region postings and bounded changed-file overlay.
    Body,
    /// Ordered whole-lexeme witnesses verified against captured source.
    Phrase,
    /// Unordered token-window witnesses verified against captured source.
    Near,
}

/// Diagnostic plan, emitted only when requested to preserve evidence byte budgets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalPlan {
    /// Procedural suffixes omitted by the explicit task policy, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted_boilerplate: Vec<String>,
    /// Original, unmodified user input.
    pub query: String,
    /// Effective structured options.
    pub options: RetrievalOptions,
    /// Deduplicated analyzed terms (navigation keeps the original spelling).
    pub terms: Vec<String>,
    /// Routes actually attempted, in execution order.
    pub routes: Vec<RetrievalRoute>,
}

/// Channel support for a returned seed; ranks are one-based, not confidence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalEvidence {
    /// Metadata channel rank, if retrieved there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_rank: Option<u32>,
    /// Body channel rank, if retrieved there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_rank: Option<u32>,
    /// Exact navigation/name evidence was present.
    pub exact: bool,
}
