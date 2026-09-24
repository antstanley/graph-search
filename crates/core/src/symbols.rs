//! The keys a store indexes symbols by, so resolution reads only the symbols a
//! changed file's references name (`research/16-proportional-sync.md` §6).
//!
//! Resolution looks symbols up by bare name, by qualified name, by file, and
//! by id; a few structural sets (Rust module declarations, markup, package
//! manifests) are read whole. A store that indexes these keys answers each
//! lookup without reading the rest of the graph.

use graph_search_types::{Node, NodeId, NodeKind};

/// Which symbol-name index a lookup reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolIndex {
    /// The bare `name` (empty when a symbol has none).
    Name,
    /// The `qualified_name`, else the bare name.
    Qualified,
}

/// One indexed symbol: what name-based rules read before loading the node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolRow {
    /// The symbol.
    pub id: NodeId,
    /// The file that owns it.
    pub path: String,
    /// Its kind.
    pub kind: NodeKind,
    /// Whether it is a lexically local declaration (`lexical_local`).
    pub lexical_local: bool,
}

impl SymbolRow {
    /// The row that indexes `node`.
    #[must_use]
    pub fn of(node: &Node) -> Self {
        Self {
            id: node.id.clone(),
            path: node.path.clone(),
            kind: node.kind,
            lexical_local: node.attribute("lexical_local") == Some("true"),
        }
    }
}

/// A set of nodes resolution reads whole, before any reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Structure {
    /// Rust module declarations, inline and external.
    RustModules,
    /// CSS rules and HTML elements, for class and id matching.
    Markup,
    /// Package manifest files (`Cargo.toml`, `package.json`,
    /// `pnpm-workspace.yaml`).
    Manifests,
}

impl Structure {
    /// Every structure.
    pub const ALL: [Self; 3] = [Self::RustModules, Self::Markup, Self::Manifests];

    /// The stable key a store indexes the structure under.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RustModules => "rust_modules",
            Self::Markup => "markup",
            Self::Manifests => "manifests",
        }
    }
}

/// The name and qualified-name keys `node` is indexed under. Files and Rust
/// `pub use` reexports never enter name lookup: a reexport is a module member
/// for anchored paths, not a competing definition.
#[must_use]
pub fn keys(node: &Node) -> Option<(String, String)> {
    if node.is_file() || node.attribute("rust_reexport").is_some() {
        return None;
    }
    let name = node.name.clone().unwrap_or_default();
    let qualified = node.qualified_name.clone().unwrap_or_else(|| name.clone());
    Some((name, qualified))
}

/// Whether `node` is indexed under `key` in `index`.
#[must_use]
pub fn indexed(node: &Node, index: SymbolIndex, key: &str) -> bool {
    keys(node).is_some_and(|(name, qualified)| match index {
        SymbolIndex::Name => name == key,
        SymbolIndex::Qualified => qualified == key,
    })
}

/// The structure `node` belongs to, if any.
#[must_use]
pub fn structure(node: &Node) -> Option<Structure> {
    if node.is_file() {
        return std::path::Path::new(&node.path)
            .file_name()
            .is_some_and(|name| {
                name == "Cargo.toml" || name == "package.json" || name == "pnpm-workspace.yaml"
            })
            .then_some(Structure::Manifests);
    }
    if node.attribute("rust_module_form").is_some() {
        return Some(Structure::RustModules);
    }
    matches!(node.kind, NodeKind::CssRule | NodeKind::Element).then_some(Structure::Markup)
}
