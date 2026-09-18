//! Reference resolution: turning extracted facts into edges, and saying what
//! did not resolve (`SPEC.md` §7.4).
//!
//! Static resolution is a best effort done the same way every time, and
//! everything it cannot resolve is kept dangling with the name it referred
//! to — never dropped, never an error.

use crate::extraction::{Extraction, ReferenceFact};
use graph_search_types::kind::{EdgeKind, Language, NodeKind};
use graph_search_types::node::Node;
use graph_search_types::{Edge, NodeId};
use std::collections::{BTreeMap, BTreeSet};

/// Whether a node kind can be the target of an edge kind, for the
/// global-unique rule. Other rules admit anything that matches by name.
#[must_use]
pub fn compatible(edge_kind: EdgeKind, node_kind: NodeKind) -> bool {
    match edge_kind {
        EdgeKind::Calls => matches!(
            node_kind,
            NodeKind::Function
                | NodeKind::Method
                | NodeKind::Macro
                | NodeKind::Variable
                | NodeKind::Const
                | NodeKind::Export
        ),
        EdgeKind::TypeUses => matches!(
            node_kind,
            NodeKind::Struct
                | NodeKind::Enum
                | NodeKind::Trait
                | NodeKind::Interface
                | NodeKind::TypeAlias
                | NodeKind::Class
                | NodeKind::Impl
                | NodeKind::Variant
                | NodeKind::Module
        ),
        EdgeKind::Extends | EdgeKind::Implements => {
            matches!(
                node_kind,
                NodeKind::Class | NodeKind::Interface | NodeKind::Trait | NodeKind::Impl
            )
        }
        EdgeKind::Imports => matches!(node_kind, NodeKind::Module | NodeKind::File),
        _ => true,
    }
}

/// The workspace-wide name tables resolution reads. Built once per sync from
/// the store plus the batch (`SPEC.md` §6.2).
#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    /// Bare name to symbol ids, workspace-wide.
    pub by_name: BTreeMap<String, Vec<NodeId>>,
    /// Qualified name to symbol id, workspace-wide.
    pub by_qualified: BTreeMap<String, NodeId>,
    /// Per file: bare name to id.
    pub by_file: BTreeMap<String, BTreeMap<String, NodeId>>,
    /// Per file: qualified name to id.
    pub by_file_qualified: BTreeMap<String, BTreeMap<String, NodeId>>,
    /// Per file: exported binding name to id.
    pub exports_by_file: BTreeMap<String, BTreeMap<String, NodeId>>,
    /// Every symbol node, by id.
    pub symbols: BTreeMap<NodeId, Node>,
}

impl SymbolTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one symbol node.
    pub fn add(&mut self, node: &Node) {
        let name = node.name.clone().unwrap_or_default();
        let qualified = node.qualified_name.clone().unwrap_or_else(|| name.clone());
        self.by_name
            .entry(name.clone())
            .or_default()
            .push(node.id.clone());
        self.by_qualified
            .entry(qualified.clone())
            .or_insert(node.id.clone());
        self.by_file
            .entry(node.path.clone())
            .or_default()
            .entry(name)
            .or_insert(node.id.clone());
        self.by_file_qualified
            .entry(node.path.clone())
            .or_default()
            .entry(qualified)
            .or_insert(node.id.clone());
        if matches!(node.kind, NodeKind::Export) {
            self.exports_by_file
                .entry(node.path.clone())
                .or_default()
                .entry(node.name.clone().unwrap_or_default())
                .or_insert(node.id.clone());
        }
        self.symbols.insert(node.id.clone(), node.clone());
    }

    /// Rule 1: same file, same name.
    #[must_use]
    pub fn local(&self, path: &str, name: &str) -> Option<&NodeId> {
        self.by_file.get(path).and_then(|m| m.get(name))
    }

    /// Rule 1, qualified: same file, same lexical path.
    #[must_use]
    pub fn local_qualified(&self, path: &str, qualified: &str) -> Option<&NodeId> {
        self.by_file_qualified
            .get(path)
            .and_then(|m| m.get(qualified))
    }

    /// Rule 4: a bare name that matches exactly one workspace symbol of a
    /// compatible kind. Ambiguity dangles, by design.
    #[must_use]
    pub fn unique_global(&self, name: &str, edge_kind: EdgeKind) -> Option<&NodeId> {
        let candidates = self.by_name.get(name)?;
        let compatible: Vec<&NodeId> = candidates
            .iter()
            .filter(|id| {
                self.symbols
                    .get(*id)
                    .is_some_and(|node| compatible(edge_kind, node.kind))
            })
            .collect();
        match compatible.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }
}

/// Resolves an import specifier to a workspace-relative file path, when it
/// names a file in the walked set.
#[must_use]
pub fn resolve_specifier(
    from_path: &str,
    specifier: &str,
    known_files: &BTreeSet<String>,
    language: Language,
) -> Option<String> {
    if specifier.starts_with("http://")
        || specifier.starts_with("https://")
        || specifier.starts_with("mailto:")
        || specifier.starts_with('#')
    {
        return None;
    }
    let candidates: Vec<String> = match language {
        Language::Rust => rust_candidates(from_path, specifier),
        Language::TypeScript | Language::JavaScript => js_candidates(from_path, specifier),
        Language::Css | Language::Html => vec![relative(from_path, specifier)],
        Language::Unknown => vec![],
    };
    candidates
        .into_iter()
        .find(|candidate| known_files.contains(candidate))
}

/// `crate::a::b` and `mod foo` spellings, relative to the file and `src/`.
fn rust_candidates(from_path: &str, specifier: &str) -> Vec<String> {
    // Be liberal: some generated Rust spells `mod` paths with `./`.
    let specifier = specifier.trim_start_matches("./");
    let dir = std::path::Path::new(from_path)
        .parent()
        .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
    let dir = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut parts = specifier.split("::").map(str::to_owned).collect::<Vec<_>>();
    if let Some(first) = parts.first().cloned() {
        match first.as_str() {
            "crate" => {
                parts.remove(0);
                let joined = parts.join("/");
                return vec![format!("src/{joined}.rs"), format!("src/{joined}/mod.rs")];
            }
            "self" | "super" => {
                parts.remove(0);
                // `super` walks one directory up from the file's dir.
                let up = if first == "super" {
                    std::path::Path::new(&dir)
                        .parent()
                        .map_or_else(String::new, |p| p.to_string_lossy().into_owned())
                } else {
                    dir
                };
                let up = if up.is_empty() {
                    String::new()
                } else {
                    format!("{up}/")
                };
                let joined = parts.join("/");
                return vec![format!("{up}{joined}.rs"), format!("{up}{joined}/mod.rs")];
            }
            _ => {}
        }
    }
    // `mod foo;` or an unqualified module: relative to the file's directory.
    let joined = specifier.replace("::", "/");
    vec![format!("{dir}{joined}.rs"), format!("{dir}{joined}/mod.rs")]
}

/// `./x`, `../x`, `x/index`, with and without extensions.
fn js_candidates(from_path: &str, specifier: &str) -> Vec<String> {
    let base = relative(from_path, specifier);
    let mut candidates = vec![
        base.clone(),
        format!("{base}.ts"),
        format!("{base}.tsx"),
        format!("{base}.js"),
        format!("{base}.jsx"),
        format!("{base}.mjs"),
        format!("{base}.cjs"),
    ];
    for ext in [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"] {
        candidates.push(format!("{base}/index{ext}"));
    }
    candidates
}

/// A relative path from the importing file's directory.
fn relative(from_path: &str, specifier: &str) -> String {
    let cleaned = specifier.split(['?', '#']).next().unwrap_or(specifier);
    if let Some(stripped) = cleaned.strip_prefix("./") {
        return join_dir(from_path, stripped);
    }
    if cleaned.starts_with("../") {
        let dir_string = std::path::Path::new(from_path)
            .parent()
            .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
        let mut dir: Vec<&str> = dir_string.split('/').filter(|s| !s.is_empty()).collect();
        let mut rest = cleaned;
        while let Some(up) = rest.strip_prefix("../") {
            dir.pop();
            rest = up;
        }
        dir.push(rest);
        return dir.join("/");
    }
    if cleaned.starts_with('/') {
        return cleaned.trim_start_matches('/').to_owned();
    }
    join_dir(from_path, cleaned)
}

fn join_dir(from_path: &str, tail: &str) -> String {
    let dir = std::path::Path::new(from_path)
        .parent()
        .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
    if dir.is_empty() {
        tail.to_owned()
    } else {
        format!("{dir}/{tail}")
    }
}

/// One resolved or dangling edge, ready to store.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    /// The edge as extracted (kind, name, line).
    pub fact: ReferenceFact,
    /// The target when resolved.
    pub to: Option<NodeId>,
    /// The display name for `to_name`: the target's qualified name, or the
    /// bare reference.
    pub to_name: String,
}

/// Resolves one reference against the table, by the §7.4 order:
/// via-import, same-file, qualified, global-unique, else dangling.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn resolve_reference(
    fact: &ReferenceFact,
    from_path: &str,
    table: &SymbolTable,
    known_files: &BTreeSet<String>,
    language: Language,
) -> Resolution {
    let dangling = |to_name: String| Resolution {
        fact: fact.clone(),
        to: None,
        to_name,
    };

    // File-level import statements become file->file (or file->module) edges.
    if fact.kind == EdgeKind::Imports && fact.from_key.is_none() {
        if let Some(target) = resolve_specifier(from_path, &fact.name, known_files, language) {
            let to = NodeId::file(&target);
            return Resolution {
                fact: fact.clone(),
                to: Some(to.clone()),
                to_name: target,
            };
        }
        return dangling(fact.name.clone());
    }

    // Rule 2: a reference to an imported binding resolves to the importing
    // file's corresponding export, if found.
    if let Some(specifier) = &fact.via_import
        && let Some(target) = resolve_specifier(from_path, specifier, known_files, language)
        && let Some(id) = table
            .exports_by_file
            .get(&target)
            .and_then(|m| m.get(&fact.name))
            .or_else(|| table.by_file.get(&target).and_then(|m| m.get(&fact.name)))
    {
        let name = table
            .symbols
            .get(id)
            .and_then(|n| n.qualified_name.clone())
            .unwrap_or_else(|| fact.name.clone());
        return Resolution {
            fact: fact.clone(),
            to: Some(id.clone()),
            to_name: name,
        };
    }

    // Rule 1: same file, same name (qualified first, then bare).
    if let Some(id) = table
        .local_qualified(from_path, &fact.name)
        .cloned()
        .or_else(|| table.local(from_path, &fact.name).cloned())
    {
        let name = table
            .symbols
            .get(&id)
            .and_then(|n| n.qualified_name.clone())
            .unwrap_or_else(|| fact.name.clone());
        return Resolution {
            fact: fact.clone(),
            to: Some(id),
            to_name: name,
        };
    }

    // Rule 3: qualified paths resolve along modules/classes when the whole
    // lexical path matches; then progressively-stripped suffixes.
    if fact.name.contains("::") || fact.name.contains('.') {
        let separators = if fact.name.contains("::") { "::" } else { "." };
        let segments: Vec<&str> = fact.name.split(separators).collect();
        // Whole path first, then shorter *suffixes* — the tail carries the
        // item name (`crate::foo::Bar` resolves via `foo::Bar`, then `Bar`).
        for take in (1..=segments.len()).rev() {
            let start = segments.len().saturating_sub(take);
            let candidate = segments[start..].join(separators);
            if let Some(id) = table.by_qualified.get(&candidate) {
                let name = table
                    .symbols
                    .get(id)
                    .and_then(|n| n.qualified_name.clone())
                    .unwrap_or(candidate);
                return Resolution {
                    fact: fact.clone(),
                    to: Some(id.clone()),
                    to_name: name,
                };
            }
        }
    }

    // Rule 4: exactly one workspace symbol of a compatible kind.
    if let Some(id) = table.unique_global(&fact.name, fact.kind).cloned() {
        let name = table
            .symbols
            .get(&id)
            .and_then(|n| n.qualified_name.clone())
            .unwrap_or_else(|| fact.name.clone());
        return Resolution {
            fact: fact.clone(),
            to: Some(id),
            to_name: name,
        };
    }

    // Rule 5: dangling, recorded with the name it referred to.
    dangling(fact.name.clone())
}

/// Builds the resolved and dangling edges for one file's extraction.
///
/// `symbol_ids` maps in-file fact keys to stable node ids; `file_id` receives
/// file-level references.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn edges_for_extraction(
    file_id: &NodeId,
    file_path: &str,
    extraction: &Extraction,
    symbol_ids: &BTreeMap<String, NodeId>,
    table: &SymbolTable,
    known_files: &BTreeSet<String>,
    language: Language,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    for fact in &extraction.references {
        let from = fact
            .from_key
            .as_ref()
            .and_then(|key| symbol_ids.get(key))
            .cloned()
            .unwrap_or_else(|| file_id.clone());
        let resolution = resolve_reference(fact, file_path, table, known_files, language);
        let edge = match resolution.to {
            Some(to) => Edge::resolved(
                &from,
                fact.kind,
                &to,
                &resolution.to_name,
                Some(file_path),
                Some(fact.line),
            ),
            None => Edge::dangling(
                &from,
                fact.kind,
                &resolution.to_name,
                Some(file_path),
                Some(fact.line),
            ),
        };
        edges.push(edge);
    }
    edges
}

/// Extracts `.class` (sep `.`) or `#id` (sep `#`) tokens from a CSS selector.
#[must_use]
pub fn selector_tokens(selector: &str, sep: char) -> Vec<String> {
    let mut tokens = Vec::new();
    for (idx, ch) in selector.char_indices() {
        if ch != sep {
            continue;
        }
        let start = idx.saturating_add(ch.len_utf8());
        let mut end = selector.len();
        for (j, c) in selector[start..].char_indices() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                continue;
            }
            end = start.saturating_add(j);
            break;
        }
        if end > start {
            tokens.push(selector[start..end].to_owned());
        }
    }
    tokens
}

/// The match tables HTML/CSS cross-edges are matched against (`SPEC.md` §7.3).
///
/// Cross-edges are *matched, not resolved*: a class name used in HTML links
/// to a CSS rule only when the selector and the class string match exactly.
/// False negatives are expected and reported in result stats.
#[derive(Clone, Debug, Default)]
pub struct CrossTables {
    /// Class token to `css_rule` node ids.
    pub class_to_rules: BTreeMap<String, Vec<NodeId>>,
    /// Class token to `element` node ids.
    pub class_to_elements: BTreeMap<String, Vec<NodeId>>,
    /// Element `id` attribute to element node ids.
    pub id_to_elements: BTreeMap<String, Vec<NodeId>>,
    /// The `css_rule` nodes, by id.
    pub rules: BTreeMap<NodeId, Node>,
    /// The elements, by id.
    pub elements: BTreeMap<NodeId, Node>,
    /// `css_rule` nodes by file (their selectors carry `id=`/`class=` tokens).
    pub rules_by_file: BTreeMap<String, Vec<NodeId>>,
    /// Elements by file.
    pub elements_by_file: BTreeMap<String, Vec<NodeId>>,
}

impl CrossTables {
    /// Builds the tables from symbol nodes in the table (all files).
    #[must_use]
    pub fn from_table(table: &SymbolTable) -> Self {
        let mut cross = Self::default();
        for node in table.symbols.values() {
            match node.kind {
                NodeKind::CssRule => {
                    let selector = node
                        .attribute("selector")
                        .or(node.name.as_deref())
                        .unwrap_or_default();
                    for token in selector_tokens(selector, '.') {
                        cross
                            .class_to_rules
                            .entry(token)
                            .or_default()
                            .push(node.id.clone());
                    }
                    cross.rules.insert(node.id.clone(), node.clone());
                    cross
                        .rules_by_file
                        .entry(node.path.clone())
                        .or_default()
                        .push(node.id.clone());
                }
                NodeKind::Element => {
                    if let Some(classes) = node.attribute("classes") {
                        for token in classes.split_whitespace() {
                            cross
                                .class_to_elements
                                .entry(token.to_owned())
                                .or_default()
                                .push(node.id.clone());
                        }
                    }
                    if let Some(id_attr) = node.attribute("id") {
                        cross
                            .id_to_elements
                            .entry(id_attr.to_owned())
                            .or_default()
                            .push(node.id.clone());
                    }
                    cross.elements.insert(node.id.clone(), node.clone());
                    cross
                        .elements_by_file
                        .entry(node.path.clone())
                        .or_default()
                        .push(node.id.clone());
                }
                _ => {}
            }
        }
        cross
    }
}

/// Builds `uses_class` / `selects` / `links_to` / `loads_stylesheet` edges
/// for one changed HTML/CSS file (`SPEC.md` §7.3).
///
/// `element_ids` maps the file's fact keys to stable node ids. Matching is
/// exact: a class token matches a selector token; an `href`/`src` matches a
/// walked file. Unmatched class tokens dangle silently here — the *count*
/// of them is a query-time stat, not an error.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn cross_edges_for(
    file_path: &str,
    extraction: &Extraction,
    element_ids: &BTreeMap<String, NodeId>,
    cross: &CrossTables,
    known_files: &BTreeSet<String>,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut seen: BTreeSet<graph_search_types::EdgeId> = BTreeSet::new();
    let push =
        |edge: Edge, seen: &mut BTreeSet<graph_search_types::EdgeId>, edges: &mut Vec<Edge>| {
            if seen.insert(edge.id.clone()) {
                edges.push(edge);
            }
        };

    for fact in &extraction.symbols {
        let Some(from_id) = element_ids.get(&fact.key) else {
            continue;
        };
        let line = fact.span.start_line;

        match fact.kind {
            NodeKind::Element => {
                // class="a b" -> every rule whose selector names the token.
                if let Some(classes) = fact.attributes.get("classes") {
                    for token in classes.split_whitespace() {
                        for rule_id in cross.class_to_rules.get(token).into_iter().flatten() {
                            push(
                                Edge::resolved(
                                    from_id,
                                    EdgeKind::UsesClass,
                                    rule_id,
                                    token,
                                    Some(file_path),
                                    Some(line),
                                ),
                                &mut seen,
                                &mut edges,
                            );
                            push(
                                Edge::resolved(
                                    rule_id,
                                    EdgeKind::Selects,
                                    from_id,
                                    &fact.qualified_name,
                                    cross
                                        .rules
                                        .get(rule_id)
                                        .map(|rule| rule.path.as_str())
                                        .or(Some(file_path)),
                                    cross
                                        .rules
                                        .get(rule_id)
                                        .and_then(|rule| rule.span)
                                        .map(|span| span.start_line),
                                ),
                                &mut seen,
                                &mut edges,
                            );
                        }
                    }
                }
                // href/src -> a workspace file; stylesheet links get their kind.
                for attr in ["href", "src"] {
                    let Some(target) = fact.attributes.get(attr) else {
                        continue;
                    };
                    let Some(resolved) =
                        resolve_specifier(file_path, target, known_files, Language::Html)
                    else {
                        continue;
                    };
                    let kind = if fact
                        .attributes
                        .get("rel")
                        .is_some_and(|rel| rel == "stylesheet")
                        && attr == "href"
                    {
                        EdgeKind::LoadsStylesheet
                    } else {
                        EdgeKind::LinksTo
                    };
                    push(
                        Edge::resolved(
                            from_id,
                            kind,
                            &NodeId::file(&resolved),
                            &resolved,
                            Some(file_path),
                            Some(line),
                        ),
                        &mut seen,
                        &mut edges,
                    );
                }
            }
            NodeKind::CssRule => {
                // A changed CSS file also selects elements by id.
                if let Some(selector) = fact.attributes.get("selector") {
                    for token in selector_tokens(selector, '#') {
                        for element_id in cross.id_to_elements.get(&token).into_iter().flatten() {
                            push(
                                Edge::resolved(
                                    from_id,
                                    EdgeKind::Selects,
                                    element_id,
                                    &token,
                                    Some(file_path),
                                    Some(line),
                                ),
                                &mut seen,
                                &mut edges,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    edges
}

#[cfg(test)]
mod cross_tests {
    use super::*;
    use crate::extraction::SymbolFact;
    use graph_search_types::kind::NodeKind;
    use graph_search_types::node::Span;

    #[test]
    fn a_stylesheet_link_becomes_a_file_edge() {
        let mut fact = SymbolFact::new(
            "el:link@3",
            NodeKind::Element,
            "link",
            "link",
            Span::new(3, 3, 10, 60),
        );
        fact = fact
            .with_attribute("tag", "link")
            .with_attribute("rel", "stylesheet")
            .with_attribute("href", "site.css");
        let extraction = Extraction {
            symbols: vec![fact],
            references: Vec::new(),
        };
        let ids = BTreeMap::from([(String::from("el:link@3"), NodeId::new("sym:x#element:link"))]);
        let mut known = BTreeSet::new();
        known.insert(String::from("web/site.css"));
        let cross = CrossTables::default();

        let edges = cross_edges_for("web/page.html", &extraction, &ids, &cross, &known);
        assert_eq!(edges.len(), 1, "{edges:?}");
        assert_eq!(edges[0].kind, EdgeKind::LoadsStylesheet);
        assert_eq!(
            edges[0].to.as_ref().map(NodeId::as_str),
            Some("file:web/site.css")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::SymbolFact;
    use graph_search_types::node::Span;

    fn symbol(path: &str, kind: NodeKind, name: &str, qualified: &str) -> Node {
        let mut fact = SymbolFact::new(
            format!("{kind}:{qualified}"),
            kind,
            name,
            qualified,
            Span::new(1, 2, 0, 10),
        );
        fact = fact.with_parent("file");
        let id = NodeId::symbol(path, kind, qualified, None);
        Node {
            id,
            kind,
            path: path.to_owned(),
            name: Some(name.to_owned()),
            qualified_name: Some(qualified.to_owned()),
            span: Some(fact.span),
            visibility: None,
            is_async: false,
            signature: None,
            ..Node::default()
        }
    }

    fn table_with(nodes: &[Node]) -> SymbolTable {
        let mut table = SymbolTable::new();
        for node in nodes {
            table.add(node);
        }
        table
    }

    #[test]
    fn same_file_same_name_wins() {
        let local = symbol("src/a.rs", NodeKind::Function, "parse", "parse");
        let other = symbol("src/b.rs", NodeKind::Function, "parse", "parse");
        let table = table_with(&[local, other]);
        let fact = ReferenceFact::from_symbol("function:parse", EdgeKind::Calls, "parse", 9);
        let resolved =
            resolve_reference(&fact, "src/a.rs", &table, &BTreeSet::new(), Language::Rust);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("sym:src/a.rs#function:parse")
        );
    }

    #[test]
    fn ambiguous_global_names_dangle() {
        let one = symbol("src/a.rs", NodeKind::Function, "parse", "a::parse");
        let two = symbol("src/b.rs", NodeKind::Function, "parse", "b::parse");
        let table = table_with(&[one, two]);
        let fact = ReferenceFact::from_symbol("x", EdgeKind::Calls, "parse", 3);
        let resolved =
            resolve_reference(&fact, "src/c.rs", &table, &BTreeSet::new(), Language::Rust);
        assert!(resolved.to.is_none());
        assert_eq!(resolved.to_name, "parse");
    }

    #[test]
    fn qualified_paths_resolve_by_whole_then_suffix() {
        let target = symbol("src/t.rs", NodeKind::Struct, "Token", "Token");
        let table = table_with(&[target]);
        let fact = ReferenceFact::from_symbol("x", EdgeKind::TypeUses, "crate::Token", 4);
        let resolved =
            resolve_reference(&fact, "src/a.rs", &table, &BTreeSet::new(), Language::Rust);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("sym:src/t.rs#struct:Token")
        );
    }

    #[test]
    fn imports_resolve_to_workspace_files() {
        let mut known = BTreeSet::new();
        known.insert(String::from("src/util.rs"));
        let table = SymbolTable::new();
        let fact = ReferenceFact::file_level(EdgeKind::Imports, "./util", 1);
        let resolved = resolve_reference(&fact, "src/a.rs", &table, &known, Language::Rust);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("file:src/util.rs")
        );

        // External crates stay dangling with their name.
        let fact = ReferenceFact::file_level(EdgeKind::Imports, "serde", 1);
        let resolved = resolve_reference(&fact, "src/a.rs", &table, &known, Language::Rust);
        assert!(resolved.to.is_none());
        assert_eq!(resolved.to_name, "serde");
    }

    #[test]
    fn imported_bindings_find_the_export() {
        let exported = symbol("src/lib.ts", NodeKind::Export, "SearchQuery", "SearchQuery");
        let mut table = table_with(&[exported]);
        table
            .exports_by_file
            .entry(String::from("src/lib.ts"))
            .or_default()
            .insert(
                String::from("SearchQuery"),
                NodeId::symbol("src/lib.ts", NodeKind::Export, "SearchQuery", None),
            );
        let mut known = BTreeSet::new();
        known.insert(String::from("src/lib.ts"));
        let mut fact = ReferenceFact::from_symbol("x", EdgeKind::TypeUses, "SearchQuery", 12);
        fact = fact.via_import("./lib");
        let resolved = resolve_reference(&fact, "src/app.ts", &table, &known, Language::TypeScript);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("sym:src/lib.ts#export:SearchQuery")
        );
    }
}
