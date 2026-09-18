//! Stable identifiers for nodes and edges (`SPEC.md` §5.3).
//!
//! Ids are opaque-looking but reproducible strings: a hash-free canonical
//! rendering of stable inputs, so an index can be diffed across runs without a
//! migration. Identity is stable under unrelated edits.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The id of a node: `file:<path>` or `sym:<path>#<kind>:<qualified_name>[@<line>]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    /// Wraps a canonical id string.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds the id of a file node: `file:<path>`.
    #[must_use]
    pub fn file(path: &str) -> Self {
        Self(format!("file:{path}"))
    }

    /// Builds the id of a symbol node.
    ///
    /// `disambiguator` is the 1-based source line, used only when a file would
    /// otherwise hold two same-kind same-name symbols (`SPEC.md` §5.3).
    #[must_use]
    pub fn symbol(
        path: &str,
        kind: crate::kind::NodeKind,
        qualified_name: &str,
        disambiguator: Option<u32>,
    ) -> Self {
        match disambiguator {
            Some(line) => Self(format!("sym:{path}#{kind}:{qualified_name}@{line}")),
            None => Self(format!("sym:{path}#{kind}:{qualified_name}")),
        }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// The id of an edge: a canonical rendering of its triple.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeId(String);

impl EdgeId {
    /// Wraps a canonical id string.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Builds the id of an edge from its triple. A dangling edge is keyed by the
    /// name it referred to (`SPEC.md` §5.3).
    #[must_use]
    pub fn of(from: &NodeId, kind: crate::kind::EdgeKind, to: &str) -> Self {
        Self(format!("({from}) -[{kind}]-> ({to})"))
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::NodeKind;

    #[test]
    fn file_ids_are_path_keyed() {
        assert_eq!(NodeId::file("src/main.rs").as_str(), "file:src/main.rs");
    }

    #[test]
    fn symbol_ids_carry_the_disambiguator_only_when_needed() {
        let plain = NodeId::symbol("src/a.rs", NodeKind::Function, "parse", None);
        let clashing = NodeId::symbol("src/a.rs", NodeKind::Impl, "Parser", Some(12));
        assert_eq!(plain.as_str(), "sym:src/a.rs#function:parse");
        assert_eq!(clashing.as_str(), "sym:src/a.rs#impl:Parser@12");
    }

    #[test]
    fn edge_ids_render_the_triple() {
        let from = NodeId::file("src/a.rs");
        let id = EdgeId::of(&from, crate::kind::EdgeKind::Contains, "file:src/b.rs");
        assert_eq!(id.as_str(), "(file:src/a.rs) -[contains]-> (file:src/b.rs)");
    }
}
