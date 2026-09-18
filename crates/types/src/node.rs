//! Node and edge records, and a source span (`SPEC.md` §5).
//!
//! One record shape serves every node kind: file-specific fields are `None`
//! for symbols and symbol-specific fields are `None` for files. Full file
//! bodies are never stored; a node carries a signature and a line range.

use crate::id::{EdgeId, NodeId};
use crate::kind::{EdgeKind, Language, NodeKind, Visibility};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A byte and line range in one source file. Lines are 1-based; bytes 0-based.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Span {
    /// First line of the item, 1-based.
    pub start_line: u32,
    /// Last line of the item, inclusive, 1-based.
    pub end_line: u32,
    /// First byte of the item.
    pub start_byte: u32,
    /// One past the last byte of the item.
    pub end_byte: u32,
}

impl Span {
    /// Builds a span from line and byte offsets.
    #[must_use]
    pub const fn new(start_line: u32, end_line: u32, start_byte: u32, end_byte: u32) -> Self {
        Self {
            start_line,
            end_line,
            start_byte,
            end_byte,
        }
    }

    /// Whether the span covers (or starts at) `line`.
    #[must_use]
    pub const fn covers_line(&self, line: u32) -> bool {
        line >= self.start_line && line <= self.end_line
    }
}

/// A node: a file or a symbol (`SPEC.md` §5.1, §5.4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// The stable id.
    pub id: NodeId,
    /// The kind.
    pub kind: NodeKind,
    /// The workspace-relative path of the file the node lives in.
    pub path: String,
    /// The bare name (`parse`); `None` for file nodes.
    pub name: Option<String>,
    /// The lexical path (`Parser::parse`); for files, the path itself.
    pub qualified_name: Option<String>,
    /// The one-line truncated declaration (`SPEC.md` §5.4); never a body.
    pub signature: Option<String>,
    /// Where the item sits in its file; `None` for file nodes.
    pub span: Option<Span>,
    /// The file's language; `None` for symbol nodes (they inherit their file's).
    pub language: Option<Language>,
    /// File size in bytes; files only.
    pub bytes: Option<u64>,
    /// File line count; files only.
    pub lines: Option<u32>,
    /// File content hash (hex SHA-256); files only.
    pub content_hash: Option<String>,
    /// The `parser_version` that produced this file node; files only.
    pub parser_version: Option<u32>,
    /// Source visibility; symbols only.
    pub visibility: Option<Visibility>,
    /// Whether the item is `async`; functions/methods only.
    pub is_async: bool,
    /// The lexical parent (`impl` for a method, file for a free item).
    pub parent: Option<NodeId>,
    /// Language-specific attributes the extractor attached (`id`,
    /// `classes`, `href`, `selector`, `tag`, ...) and cross-file matching
    /// reads (`SPEC.md` §7.3).
    pub attributes: BTreeMap<String, String>,
}

impl Node {
    /// Builds a file node (`SPEC.md` §5.4).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn file(
        path: &str,
        language: Language,
        bytes: u64,
        lines: u32,
        content_hash: &str,
        parser_version: u32,
    ) -> Self {
        Self {
            id: NodeId::file(path),
            kind: NodeKind::File,
            path: path.to_owned(),
            name: None,
            qualified_name: Some(path.to_owned()),
            signature: None,
            span: None,
            language: Some(language),
            bytes: Some(bytes),
            lines: Some(lines),
            content_hash: Some(content_hash.to_owned()),
            parser_version: Some(parser_version),
            visibility: None,
            is_async: false,
            parent: None,
            attributes: BTreeMap::new(),
        }
    }

    /// Whether this is a file node.
    #[must_use]
    pub const fn is_file(&self) -> bool {
        matches!(self.kind, NodeKind::File)
    }

    /// One attached attribute, when present.
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    /// The display name for results: the bare name, or the path for files.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.path)
    }
}

/// An edge between two nodes, or a dangling reference kept by name
/// (`SPEC.md` §5.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// The stable id of the edge (its canonical triple).
    pub id: EdgeId,
    /// The source node.
    pub from: NodeId,
    /// The kind.
    pub kind: EdgeKind,
    /// The target node; `None` when the reference is dangling.
    pub to: Option<NodeId>,
    /// The name the target was referred by: the resolved target's qualified
    /// name, or the dangling reference's text.
    pub to_name: String,
    /// Whether the target resolved to a workspace node.
    pub resolved: bool,
    /// The file the reference occurs in.
    pub path: Option<String>,
    /// The 1-based line of the reference.
    pub line: Option<u32>,
}

impl Edge {
    /// Builds a resolved edge.
    #[must_use]
    pub fn resolved(
        from: &NodeId,
        kind: EdgeKind,
        to: &NodeId,
        to_name: &str,
        path: Option<&str>,
        line: Option<u32>,
    ) -> Self {
        Self {
            id: EdgeId::of(from, kind, to.as_str()),
            from: from.clone(),
            kind,
            to: Some(to.clone()),
            to_name: to_name.to_owned(),
            resolved: true,
            path: path.map(str::to_owned),
            line,
        }
    }

    /// Builds a dangling edge: kept, named, counted — never dropped, never an
    /// error (`SPEC.md` §7.4).
    #[must_use]
    pub fn dangling(
        from: &NodeId,
        kind: EdgeKind,
        to_name: &str,
        path: Option<&str>,
        line: Option<u32>,
    ) -> Self {
        Self {
            id: EdgeId::of(from, kind, to_name),
            from: from.clone(),
            kind,
            to: None,
            to_name: to_name.to_owned(),
            resolved: false,
            path: path.map(str::to_owned),
            line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dangling_edge_keeps_the_referenced_name() {
        let from = NodeId::file("src/a.rs");
        let edge = Edge::dangling(
            &from,
            EdgeKind::Calls,
            "missing_fn",
            Some("src/a.rs"),
            Some(7),
        );
        assert!(!edge.resolved);
        assert_eq!(edge.to_name, "missing_fn");
        assert!(edge.to.is_none());
    }

    #[test]
    fn a_resolved_edge_names_its_target() {
        let from = NodeId::file("src/a.rs");
        let to = NodeId::symbol("src/b.rs", NodeKind::Function, "found", None);
        let edge = Edge::resolved(&from, EdgeKind::Calls, &to, "found", None, Some(3));
        assert!(edge.resolved);
        assert_eq!(edge.to, Some(to.clone()));
    }
}
