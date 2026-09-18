//! The request vocabulary of the read API (`SPEC.md` §4.7, §8).
//!
//! Every query is bounded: limits are clamped to their ceilings by the
//! constructors, so no caller can request an unbounded answer.

use crate::kind::{EdgeKind, Language, NodeKind};
use crate::limits::{
    DEFAULT_CONTEXT_LINES, EXPLORE_DEFAULT_K, FILES_DEFAULT_LIMIT, FILES_LIMIT_CEILING,
    GRAPH_DEFAULT_LIMIT, GRAPH_LIMIT_CEILING, MAX_HOPS_CEILING, MAX_SNIPPET_LINES,
    TEXT_DEFAULT_LIMIT, TEXT_LIMIT_CEILING,
};
use serde::{Deserialize, Serialize};

/// Clamps a requested limit into `1..=ceiling`.
const fn clamp_limit(requested: u32, default: u32, ceiling: u32) -> u32 {
    if requested == 0 {
        default
    } else if requested > ceiling {
        ceiling
    } else {
        requested
    }
}

/// Clamps a hop count to the traversal ceiling (`SPEC.md` §8.3: clamped, not
/// an error).
const fn clamp_hops(requested: u8) -> u8 {
    if requested == 0 || requested > MAX_HOPS_CEILING {
        MAX_HOPS_CEILING
    } else {
        requested
    }
}

/// The filters every graph query accepts (`SPEC.md` §8.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphFilters {
    /// Restrict results to one language.
    pub lang: Option<Language>,
    /// Restrict results by a glob over workspace-relative paths.
    pub path_glob: Option<String>,
}

/// `search files` (`SPEC.md` §8.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesQuery {
    /// The glob pattern, anchored to the search root; `*` does not cross `/`.
    pub pattern: String,
    /// Directory to search under, relative to the root.
    pub path: Option<String>,
    /// The match cap, already clamped.
    pub limit: u32,
    /// Include hidden files.
    pub include_hidden: bool,
    /// Ignore `.gitignore` and friends.
    pub no_ignore: bool,
}

impl FilesQuery {
    /// A clamped query for `pattern`.
    #[must_use]
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            path: None,
            limit: FILES_DEFAULT_LIMIT,
            include_hidden: false,
            no_ignore: false,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, FILES_DEFAULT_LIMIT, FILES_LIMIT_CEILING);
        self
    }

    /// Sets the sub-directory.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

/// `search text` (`SPEC.md` §8.2): a literal substring, never a regex.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextQuery {
    /// The literal text to find.
    pub pattern: String,
    /// Directory to search under, relative to the root.
    pub path: Option<String>,
    /// One positive glob narrowing the files searched.
    pub include: Option<String>,
    /// The match cap, already clamped.
    pub limit: u32,
    /// Fold case when matching.
    pub ignore_case: bool,
    /// Include hidden files.
    pub include_hidden: bool,
    /// Ignore `.gitignore` and friends.
    pub no_ignore: bool,
}

impl TextQuery {
    /// A clamped query for the literal `pattern`.
    #[must_use]
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            path: None,
            include: None,
            limit: TEXT_DEFAULT_LIMIT,
            ignore_case: false,
            include_hidden: false,
            no_ignore: false,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, TEXT_DEFAULT_LIMIT, TEXT_LIMIT_CEILING);
        self
    }

    /// Sets the include glob.
    #[must_use]
    pub fn with_include(mut self, include: impl Into<String>) -> Self {
        self.include = Some(include.into());
        self
    }
}

/// A graph query over a name or an exact id (`SPEC.md` §8.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolQuery {
    /// The target: a symbol name, qualified name, or exact id.
    pub target: String,
    /// Restrict the matched definition's kind.
    pub kind: Option<NodeKind>,
    /// The shared graph filters.
    pub filters: GraphFilters,
    /// The result cap, already clamped.
    pub limit: u32,
}

impl SymbolQuery {
    /// A clamped query for `target`.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            kind: None,
            filters: GraphFilters::default(),
            limit: GRAPH_DEFAULT_LIMIT,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, GRAPH_DEFAULT_LIMIT, GRAPH_LIMIT_CEILING);
        self
    }

    /// Sets the shared filters.
    #[must_use]
    pub fn with_filters(mut self, filters: GraphFilters) -> Self {
        self.filters = filters;
        self
    }
}

/// `search refs`: every reference to a symbol (`SPEC.md` §8.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefQuery {
    /// The target: a symbol name, qualified name, or exact id.
    pub target: String,
    /// The shared graph filters.
    pub filters: GraphFilters,
    /// The result cap, already clamped.
    pub limit: u32,
}

impl RefQuery {
    /// A clamped query for `target`.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            filters: GraphFilters::default(),
            limit: GRAPH_DEFAULT_LIMIT,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, GRAPH_DEFAULT_LIMIT, GRAPH_LIMIT_CEILING);
        self
    }
}

/// `search callers` / `search callees` / `search impact` (`SPEC.md` §8.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalQuery {
    /// The target: a symbol name, qualified name, or exact id.
    pub target: String,
    /// How deep to follow; clamped to `MAX_HOPS_CEILING`.
    pub depth: u8,
    /// The shared graph filters.
    pub filters: GraphFilters,
    /// The result cap, already clamped.
    pub limit: u32,
}

impl TraversalQuery {
    /// A clamped query for `target` at the given depth.
    #[must_use]
    pub fn new(target: impl Into<String>, depth: u8) -> Self {
        Self {
            target: target.into(),
            depth: clamp_hops(depth),
            filters: GraphFilters::default(),
            limit: GRAPH_DEFAULT_LIMIT,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, GRAPH_DEFAULT_LIMIT, GRAPH_LIMIT_CEILING);
        self
    }
}

/// `search deps`: imports and imported-by for a file (`SPEC.md` §8.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepsQuery {
    /// The target: a file path, file id, or module name.
    pub target: String,
    /// Which direction to report.
    pub direction: crate::kind::Direction,
    /// The shared graph filters.
    pub filters: GraphFilters,
    /// The result cap, already clamped.
    pub limit: u32,
}

impl DepsQuery {
    /// A clamped query for `target`.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            direction: crate::kind::Direction::Both,
            filters: GraphFilters::default(),
            limit: GRAPH_DEFAULT_LIMIT,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, GRAPH_DEFAULT_LIMIT, GRAPH_LIMIT_CEILING);
        self
    }
}

/// `search neighbors`: adjacent nodes along chosen edge kinds (`SPEC.md` §8.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborsQuery {
    /// The target: an exact node id, or a name resolved like `symbol`.
    pub target: String,
    /// Restrict to one edge kind; every kind when `None`.
    pub rel: Option<EdgeKind>,
    /// How many hops; clamped to `MAX_HOPS_CEILING`.
    pub hops: u8,
    /// The shared graph filters.
    pub filters: GraphFilters,
    /// The result cap, already clamped.
    pub limit: u32,
}

impl NeighborsQuery {
    /// A clamped query for `target`.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            rel: None,
            hops: clamp_hops(1),
            filters: GraphFilters::default(),
            limit: GRAPH_DEFAULT_LIMIT,
        }
    }

    /// Sets the limit, clamped to the ceiling.
    #[must_use]
    pub const fn with_limit(mut self, limit: u32) -> Self {
        self.limit = clamp_limit(limit, GRAPH_DEFAULT_LIMIT, GRAPH_LIMIT_CEILING);
        self
    }
}

/// `search path`: the shortest path between two nodes (`SPEC.md` §8.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathQuery {
    /// The start: a name or exact id.
    pub from: String,
    /// The goal: a name or exact id.
    pub to: String,
    /// The deepest path accepted; clamped to `MAX_HOPS_CEILING`.
    pub max_hops: u8,
    /// The shared graph filters.
    pub filters: GraphFilters,
}

impl PathQuery {
    /// A clamped query from `from` to `to`.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            max_hops: MAX_HOPS_CEILING,
            filters: GraphFilters::default(),
        }
    }
}

/// `search explore`: the one-call retrieval (`SPEC.md` §8.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExploreQuery {
    /// Free-text terms; split on whitespace for seeding.
    pub query: String,
    /// How many seeds to assemble; already clamped.
    pub k: u32,
    /// How many hops to connect seeds over; clamped.
    pub hops: u8,
    /// Snippet context lines around each definition; already clamped.
    pub context_lines: u32,
    /// The whole-payload byte budget.
    pub max_bytes: u32,
    /// The shared graph filters.
    pub filters: GraphFilters,
    /// The result cap, already clamped.
    pub limit: u32,
}

impl ExploreQuery {
    /// A clamped query for the free-text `query`.
    #[must_use]
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            k: EXPLORE_DEFAULT_K,
            hops: clamp_hops(1),
            context_lines: DEFAULT_CONTEXT_LINES,
            max_bytes: u32::try_from(crate::limits::MAX_TOTAL_BYTES).unwrap_or(u32::MAX),
            filters: GraphFilters::default(),
            limit: GRAPH_DEFAULT_LIMIT,
        }
    }

    /// Sets the snippet context, clamped to `MAX_SNIPPET_LINES`.
    #[must_use]
    pub const fn with_context_lines(mut self, lines: u32) -> Self {
        self.context_lines = if lines > MAX_SNIPPET_LINES {
            MAX_SNIPPET_LINES
        } else {
            lines
        };
        self
    }

    /// Sets the seed count, clamped to the graph ceiling.
    #[must_use]
    pub const fn with_k(mut self, k: u32) -> Self {
        self.k = clamp_limit(k, EXPLORE_DEFAULT_K, GRAPH_LIMIT_CEILING);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_clamp_into_range() {
        let q = FilesQuery::new("*.rs").with_limit(0);
        assert_eq!(q.limit, FILES_DEFAULT_LIMIT);
        let q = q.with_limit(99_999);
        assert_eq!(q.limit, FILES_LIMIT_CEILING);
        let q = TextQuery::new("x").with_limit(50_000);
        assert_eq!(q.limit, TEXT_LIMIT_CEILING);
    }

    #[test]
    fn hops_clamp_to_the_ceiling_not_error() {
        // SPEC.md 8.3: over-ceiling is clamped, not an error.
        let q = TraversalQuery::new("f", 40);
        assert_eq!(q.depth, MAX_HOPS_CEILING);
        let q = TraversalQuery::new("f", 0);
        assert_eq!(q.depth, MAX_HOPS_CEILING);
    }
}
