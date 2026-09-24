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

/// Whether a language consumes JavaScript/TypeScript module surfaces, including
/// the script regions of native framework components.
#[must_use]
pub fn js_family(language: Language) -> bool {
    matches!(
        language,
        Language::JavaScript
            | Language::TypeScript
            | Language::Svelte
            | Language::Vue
            | Language::Astro
    )
}

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

/// [`compatible`], refined by the referring language: calling a Python class
/// constructs it, and a bare Python call name is never a method, which is only
/// reachable through its receiver (`self.method()` resolves by its qualified
/// class member).
fn admits(edge_kind: EdgeKind, name: &str, language: Language, node_kind: NodeKind) -> bool {
    if language == Language::Python && edge_kind == EdgeKind::Calls {
        return match node_kind {
            NodeKind::Class => true,
            NodeKind::Method => name.contains('.'),
            other => compatible(edge_kind, other),
        };
    }
    compatible(edge_kind, node_kind)
}

/// The workspace-wide name tables resolution reads. Built once per sync from
/// the store plus the batch (`SPEC.md` §6.2).
#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    /// Cargo manifest nodes retained separately from symbol-name lookup.
    files: BTreeMap<String, Node>,
    pub(crate) rust_roots: crate::rust_modules::Catalog,
    pub(crate) rust_paths: crate::rust_paths::Paths,
    js_modules: crate::js_modules::Modules,
    node_packages: crate::node_packages::Packages,
    /// Bare name to symbol ids, workspace-wide.
    pub by_name: BTreeMap<String, Vec<NodeId>>,
    /// Qualified name to symbol id, workspace-wide.
    pub by_qualified: BTreeMap<String, NodeId>,
    /// All qualified-name candidates, retaining ambiguity across files.
    pub qualified_candidates: BTreeMap<String, Vec<NodeId>>,
    /// Per file: bare name to id.
    pub by_file: BTreeMap<String, BTreeMap<String, NodeId>>,
    /// Per file: qualified name to id.
    pub by_file_qualified: BTreeMap<String, BTreeMap<String, NodeId>>,
    /// Per file: exported binding name to id.
    pub exports_by_file: BTreeMap<String, BTreeMap<String, NodeId>>,
    /// Every symbol node, by id.
    pub symbols: BTreeMap<NodeId, Node>,
    /// Per OKF document: its concept, the target of links into the file.
    pub(crate) okf_concepts: BTreeMap<String, NodeId>,
    /// Nearest selected TypeScript projects for declared path aliases.
    ts_projects: crate::typescript_project::Projects,
}

impl SymbolTable {
    /// Prepare file-owned ESM surfaces from current raw extraction identities.
    pub fn prepare_js_modules<'a>(
        &mut self,
        files: impl IntoIterator<
            Item = (
                &'a str,
                &'a graph_search_types::extraction::SharedExtraction,
            ),
        >,
    ) {
        self.prepare_js_surfaces(
            files
                .into_iter()
                .filter_map(|(path, facts)| facts.js_module.as_ref().map(|module| (path, module))),
        );
    }

    /// Prepare compact authored module surfaces without retaining parser payloads.
    pub fn prepare_js_surfaces<'a>(
        &mut self,
        files: impl IntoIterator<Item = (&'a str, &'a graph_search_types::js_module::JsModule)>,
    ) {
        self.js_modules.0 = files
            .into_iter()
            .map(|(path, module)| (path.to_owned(), module.clone()))
            .collect();
    }
    /// Resolve a JS/TS module from declared aliases, relative paths or package maps.
    /// Declared `paths`/`baseUrl` are attempted first for bare specifiers, in
    /// compiler order; an unmatched project leaves package resolution unchanged.
    pub(crate) fn js_specifier(
        &self,
        from: &str,
        specifier: &str,
        known: &BTreeSet<String>,
    ) -> Result<String, &'static str> {
        if let Some(target) = self.ts_projects.resolve(from, specifier, known) {
            return Ok(target);
        }
        self.node_packages.resolve(from, specifier, known)
    }
    /// Select the nearest admitted configuration for each file and compile its
    /// declared `paths`/`baseUrl` aliases. Unsupported configurations are skipped.
    pub fn prepare_typescript_projects(
        &mut self,
        sources: &BTreeMap<String, graph_search_types::source::SourceFileUnits>,
    ) {
        self.ts_projects = crate::typescript_project::Projects::build(sources);
    }

    /// Prepare authored Node package boundaries after all file nodes are populated.
    pub fn prepare_node_packages(&mut self, boundaries: &BTreeSet<String>) {
        self.node_packages = crate::node_packages::Packages::build(&self.files, boundaries);
    }
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Prepare native Rust module paths from the post-update file set.
    pub fn prepare_rust_modules(
        &mut self,
        known: &BTreeSet<String>,
        boundaries: &BTreeSet<String>,
    ) {
        let mut catalog = crate::rust_modules::Catalog::build(&self.files, known, boundaries);
        catalog.populate(&self.symbols, known);
        self.rust_paths = crate::rust_paths::Paths::build(&self.symbols, &catalog);
        self.rust_roots = catalog;
    }

    /// Adds a file or symbol node; files never enter symbol-name lookup.
    pub fn add(&mut self, node: &Node) {
        if node.is_file() {
            if std::path::Path::new(&node.path)
                .file_name()
                .is_some_and(|name| {
                    name == "Cargo.toml" || name == "package.json" || name == "pnpm-workspace.yaml"
                })
            {
                self.files.insert(node.path.clone(), node.clone());
            }
            return;
        }
        // A `pub use` reexport is a module member for anchored path resolution, not
        // a competing definition: it must not enter name-based retrieval or fallback.
        if node.attribute("rust_reexport").is_some() {
            self.symbols.insert(node.id.clone(), node.clone());
            return;
        }
        let name = node.name.clone().unwrap_or_default();
        let qualified = node.qualified_name.clone().unwrap_or_else(|| name.clone());
        self.by_name
            .entry(name.clone())
            .or_default()
            .push(node.id.clone());
        self.qualified_candidates
            .entry(qualified.clone())
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
        if node.kind == NodeKind::Concept {
            self.okf_concepts
                .entry(node.path.clone())
                .or_insert(node.id.clone());
        }
        if matches!(node.kind, NodeKind::Export) {
            self.exports_by_file
                .entry(node.path.clone())
                .or_default()
                .entry(node.name.clone().unwrap_or_default())
                .or_insert(node.id.clone());
        }
        self.symbols.insert(node.id.clone(), node.clone());
    }

    fn named(&self, name: &str) -> Vec<&Node> {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .chain(self.qualified_candidates.get(name).into_iter().flatten())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|id| self.symbols.get(id))
            .collect()
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
    /// kind compatible with the referring language's edge. Ambiguity dangles,
    /// by design.
    #[must_use]
    pub fn unique_global(
        &self,
        name: &str,
        edge_kind: EdgeKind,
        language: Language,
    ) -> Option<&NodeId> {
        let candidates = self.by_name.get(name)?;
        let compatible: Vec<&NodeId> = candidates
            .iter()
            .filter(|id| {
                self.symbols.get(*id).is_some_and(|node| {
                    admits(edge_kind, name, language, node.kind)
                        && node.attribute("lexical_local") != Some("true")
                })
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
        Language::TypeScript
        | Language::JavaScript
        | Language::Svelte
        | Language::Vue
        | Language::Astro => js_candidates(from_path, specifier),
        Language::Css | Language::Html => vec![relative(from_path, specifier)],
        Language::Python => python_candidates(from_path, specifier),
        // Dependency tracking only: with the bundle-root fallback this selects a
        // superset of what a `links_to` path can resolve to, so any change in a
        // link's or a citation's target is still observed.
        Language::Okf => {
            return crate::okf::resolve_path(from_path, specifier, known_files, true);
        }
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
    if !specifier.starts_with('.') && !specifier.starts_with('/') {
        return Vec::new();
    }
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
    for (runtime, source) in [
        (".js", ".ts"),
        (".js", ".tsx"),
        (".mjs", ".mts"),
        (".cjs", ".cts"),
    ] {
        if let Some(stem) = base.strip_suffix(runtime) {
            candidates.push(format!("{stem}{source}"));
        }
    }
    for ext in [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"] {
        candidates.push(format!("{base}/index{ext}"));
    }
    candidates
}

/// Python module spellings: an absolute dotted path (`a.b`) or a relative one
/// (`.`, `..pkg`). A module is a package directory with an `__init__` or a
/// `.py`/`.pyi` file; as in Python's own path finder, a regular package wins over
/// a same-named module file.
fn python_candidates(from_path: &str, specifier: &str) -> Vec<String> {
    python_module_path(from_path, specifier).map_or_else(Vec::new, |joined| {
        vec![
            format!("{joined}/__init__.py"),
            format!("{joined}/__init__.pyi"),
            format!("{joined}.py"),
            format!("{joined}.pyi"),
        ]
    })
}

/// The slash-joined module path a Python specifier names, or `None` when it
/// names nothing (empty, or a relative import that walks past the root).
fn python_module_path(from_path: &str, specifier: &str) -> Option<String> {
    let specifier = specifier.trim();
    if specifier.is_empty() {
        return None;
    }
    let dots = specifier.chars().take_while(|c| *c == '.').count();
    let tail = specifier.get(dots..).unwrap_or_default();
    let segments: Vec<String> = if dots == 0 {
        tail.split('.')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        // One dot is the importing file's package (its directory); each
        // further dot walks one package up.
        let mut base: Vec<String> = std::path::Path::new(from_path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default()
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect();
        for _ in 1..dots {
            // Walking past the workspace root has no target.
            base.pop()?;
        }
        base.extend(
            tail.split('.')
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        );
        base
    };
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// The specifier that names `name` as a submodule of `specifier`: `from .` and
/// `from ..` concatenate, an explicit module joins with a dot.
fn python_submodule_specifier(specifier: &str, name: &str) -> String {
    if specifier.ends_with('.') {
        format!("{specifier}{name}")
    } else {
        format!("{specifier}.{name}")
    }
}

/// Whether `fact` is a module import that resolves to a file. Python allows an
/// `import` inside a function or class body; it still names a module, so its
/// owner does not change how it resolves.
fn module_import(fact: &ReferenceFact, language: Language) -> bool {
    fact.kind == EdgeKind::Imports
        && fact.via_import.is_none()
        && (fact.from_key.is_none() || language == Language::Python)
}

/// The module specifiers whose file resolution decides `fact`'s target, so a
/// change in the known file set can invalidate it. A Python `from m import x`
/// also consults the submodule `m.x`.
pub(crate) fn import_specifiers(fact: &ReferenceFact, language: Language) -> Vec<String> {
    match &fact.via_import {
        Some(specifier) if language == Language::Python => vec![
            specifier.clone(),
            python_submodule_specifier(specifier, &fact.name),
        ],
        Some(specifier) => vec![specifier.clone()],
        None if module_import(fact, language) => vec![fact.name.clone()],
        // An OKF link or citation depends on the file its path selects.
        None if language == Language::Okf
            && matches!(fact.kind, EdgeKind::LinksTo | EdgeKind::Cites) =>
        {
            vec![fact.name.clone()]
        }
        None => Vec::new(),
    }
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
    /// Evidence class, independent of the resolved/unresolved boolean.
    pub class: graph_search_types::occurrence::ResolutionClass,
    /// Why no static target was established.
    pub reason: Option<String>,
    /// The edge as extracted (kind, name, line).
    pub fact: ReferenceFact,
    /// The target when resolved.
    pub to: Option<NodeId>,
    /// The display name for `to_name`: the target's qualified name, or the
    /// bare reference.
    pub to_name: String,
}

/// The largest stored display name for an unresolved reference, in bytes.
///
/// `to_name` labels a dangle for a reader; resolution has already failed by the
/// time it is written, so it is never a lookup key. Complex callees — method
/// chains, closure literals, macro-ish expressions — otherwise arrive as
/// multi-line source text that bloats `dangling.jsonl` and renders unreadably
/// (finding E4).
const MAX_DANGLING_NAME_BYTES: usize = 96;
/// Hex characters of the raw-name hash appended to a shortened dangling name.
const DANGLING_HASH_HEX: usize = 8;

/// Collapses a dangling reference name to one bounded, readable line.
///
/// The result is also the dangling edge's identity (`Edge::dangling` keys the
/// `EdgeId` on it), so shortening must not merge two distinct references. When
/// the collapsed name exceeds the bound it is truncated and a short hash of the
/// raw name is appended, keeping distinct long names on distinct edges. The
/// bound applies whether or not the name contains whitespace.
pub(crate) fn canonical_dangling_name(name: &str) -> String {
    // Collapse internal whitespace onto one line, dropping the space around a
    // `.`/`::` member separator so `receiv\n  .member` reads as `receiver.member`.
    let mut out = String::with_capacity(name.len().min(MAX_DANGLING_NAME_BYTES));
    let mut pending_space = false;
    for ch in name.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space
            && !out.is_empty()
            && !out.ends_with('.')
            && !out.ends_with(':')
            && ch != '.'
            && ch != ':'
        {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    if out.len() <= MAX_DANGLING_NAME_BYTES {
        return out;
    }
    // Over the bound: truncate at a char boundary and append `…` plus a short
    // hash of the raw name so two long names sharing a prefix stay distinct.
    let suffix = format!(
        "…{}",
        crate::hash::content_hash(name.as_bytes())
            .get(..DANGLING_HASH_HEX)
            .unwrap_or_default()
    );
    let mut end = MAX_DANGLING_NAME_BYTES.saturating_sub(suffix.len());
    while end > 0 && !out.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    out.truncate(end);
    out.push_str(&suffix);
    out
}

/// Resolves one reference against the table, by the §7.4 order:
/// via-import, same-file, qualified, global-unique, else dangling.
#[must_use]
pub fn resolve_reference(
    fact: &ReferenceFact,
    from_path: &str,
    table: &SymbolTable,
    known_files: &BTreeSet<String>,
    language: Language,
) -> Resolution {
    resolve_in(fact, from_path, table, known_files, language, None)
}

/// [`resolve_reference`] with the file's other references, so a Rust method
/// call can bind through its receiver's inferred type.
#[must_use]
#[allow(clippy::too_many_lines)]
pub(crate) fn resolve_in(
    fact: &ReferenceFact,
    from_path: &str,
    table: &SymbolTable,
    known_files: &BTreeSet<String>,
    language: Language,
    receivers: Option<&crate::rust_receivers::Receivers<'_>>,
) -> Resolution {
    use graph_search_types::occurrence::ResolutionClass;
    let dangling = |to_name: String, reason: &str| Resolution {
        class: ResolutionClass::Unresolved,
        reason: Some(reason.into()),
        fact: fact.clone(),
        to: None,
        to_name: canonical_dangling_name(&to_name),
    };

    // `x.method()` whose receiver's static type is stated: `Type::method`.
    if language == Language::Rust
        && let (Some(receiver), Some(receivers)) = (&fact.receiver, receivers)
        && let Some(method) = receivers.method(fact, receiver)
    {
        return Resolution {
            class: ResolutionClass::Receiver,
            reason: None,
            fact: fact.clone(),
            to: Some(method.id.clone()),
            to_name: method
                .qualified_name
                .clone()
                .unwrap_or_else(|| fact.name.clone()),
        };
    }

    // OKF cross-links and citations name bundle paths, never symbols.
    if language == Language::Okf && matches!(fact.kind, EdgeKind::LinksTo | EdgeKind::Cites) {
        return crate::okf::resolve(fact, from_path, table, known_files);
    }

    if fact.dynamic {
        return dangling(
            fact.name.clone(),
            fact.unresolved_reason
                .as_deref()
                .unwrap_or("dynamic_target"),
        );
    }
    if let Some(key) = &fact.lexical_target {
        let matches: Vec<_> = table
            .named(&fact.name)
            .into_iter()
            .filter(|node| {
                node.path == from_path
                    && node.attribute("lexical_key") == Some(key.as_str())
                    && compatible(fact.kind, node.kind)
            })
            .collect();
        if let [node] = matches.as_slice() {
            return Resolution {
                class: ResolutionClass::ExplicitLexical,
                reason: None,
                fact: fact.clone(),
                to: Some(node.id.clone()),
                to_name: node
                    .qualified_name
                    .clone()
                    .unwrap_or_else(|| fact.name.clone()),
            };
        }
        // An explicit lexical binding never falls through to a workspace heuristic.
        return dangling(fact.name.clone(), "lexical_target_missing_or_ambiguous");
    }

    // A Rust `mod name;` declaration carries its original syntax span. Resolve
    // it from the declared module context before generic use/import handling.
    if language == Language::Rust && fact.rust_module_declaration {
        let modules: Vec<_> = table
            .named(&fact.name)
            .into_iter()
            .filter(|node| {
                node.path == from_path
                    && node.kind == NodeKind::Module
                    && node.attribute("rust_module_form") == Some("external")
                    && fact.span.is_some()
                    && node.span == fact.span
            })
            .collect();
        if let [module] = modules.as_slice() {
            return match table.rust_roots.declaration(&module.id) {
                Ok(target) => Resolution {
                    class: ResolutionClass::ExplicitImport,
                    reason: None,
                    fact: fact.clone(),
                    to: Some(NodeId::file(&target)),
                    to_name: target,
                },
                Err(reason) => dangling(fact.name.clone(), reason),
            };
        }
        return dangling(
            fact.name.clone(),
            "rust_module_declaration_missing_or_ambiguous",
        );
    }

    if language == Language::Rust
        && (fact.rust_use.is_some()
            || fact.via_import.is_some()
            || fact.name.starts_with("crate::")
            || fact.name.starts_with("self::")
            || fact.name.starts_with("super::")
            // `other_crate::item` names a workspace crate's member directly.
            || fact.name.split_once("::").is_some_and(|(first, _)| {
                table.rust_paths.is_crate(first)
                    && fact.name.split("::").all(|segment| {
                        !segment.is_empty()
                            && segment.chars().all(|c| c.is_alphanumeric() || c == '_')
                    })
            }))
    {
        return match table.rust_paths.resolve(fact, from_path, &table.symbols) {
            Ok(id) => Resolution {
                class: if fact.rust_use.is_some() || fact.via_import.is_some() {
                    ResolutionClass::ExplicitImport
                } else {
                    ResolutionClass::Qualified
                },
                reason: None,
                fact: fact.clone(),
                to_name: table
                    .symbols
                    .get(&id)
                    .and_then(|node| node.qualified_name.clone())
                    .unwrap_or_else(|| fact.name.clone()),
                to: Some(id),
            },
            Err(reason) => dangling(fact.name.clone(), reason),
        };
    }

    // File-level import statements become file->file (or file->module) edges.
    if module_import(fact, language) {
        let target = if js_family(language) {
            table.js_specifier(from_path, &fact.name, known_files)
        } else {
            resolve_specifier(from_path, &fact.name, known_files, language)
                .ok_or("module_target_missing")
        };
        if let Ok(target) = &target {
            let to = NodeId::file(target);
            return Resolution {
                class: ResolutionClass::ExplicitImport,
                reason: None,
                fact: fact.clone(),
                to: Some(to.clone()),
                to_name: target.clone(),
            };
        }
        return dangling(
            fact.name.clone(),
            target.err().unwrap_or("module_target_missing"),
        );
    }

    // Explicit import provenance is authoritative. Failure to resolve the
    // module must not fall through to an unrelated workspace name.
    if let Some(specifier) = &fact.via_import {
        if js_family(language) {
            let target = match table.js_specifier(from_path, specifier, known_files) {
                Ok(target) => target,
                Err(reason) => return dangling(fact.name.clone(), reason),
            };
            if fact.kind == EdgeKind::Imports && fact.name == "*" {
                return Resolution {
                    class: ResolutionClass::ExplicitImport,
                    reason: None,
                    fact: fact.clone(),
                    to: Some(NodeId::file(&target)),
                    to_name: target,
                };
            }
            return match table.js_modules.resolve(
                &target,
                &fact.name,
                fact.kind,
                table,
                known_files,
            ) {
                Ok(id) => Resolution {
                    class: ResolutionClass::ExplicitImport,
                    reason: None,
                    fact: fact.clone(),
                    to_name: table
                        .symbols
                        .get(&id)
                        .and_then(|node| node.qualified_name.clone())
                        .unwrap_or_else(|| fact.name.clone()),
                    to: Some(id),
                },
                Err(reason) => dangling(fact.name.clone(), reason),
            };
        }
        if let Some(target) = resolve_specifier(from_path, specifier, known_files, language) {
            let mut matches: Vec<&Node> = table
                .named(&fact.name)
                .into_iter()
                .filter(|n| {
                    n.path == target
                        && n.attribute("lexical_local") != Some("true")
                        && n.name.as_deref() == Some(fact.name.as_str())
                        // A Python module's importable names are its top-level
                        // bindings; a method, class field or nested `def` that
                        // shares the name is not one.
                        && (language != Language::Python
                            || n.qualified_name.as_deref() == Some(fact.name.as_str()))
                        && (fact.kind == EdgeKind::Imports
                            || n.kind == NodeKind::Export
                            || admits(fact.kind, &fact.name, language, n.kind))
                })
                .collect();
            // Python rebinds a top-level name freely (`@overload` stubs,
            // conditional `def`s): the file's last binding is the one an import
            // sees.
            if language == Language::Python {
                matches.sort_by_key(|n| n.span.map(|span| span.start_byte));
                matches = matches.pop().into_iter().collect();
            }
            if let [node] = matches.as_slice() {
                return Resolution {
                    class: ResolutionClass::ExplicitImport,
                    reason: None,
                    fact: fact.clone(),
                    to: Some(node.id.clone()),
                    to_name: node
                        .qualified_name
                        .clone()
                        .unwrap_or_else(|| fact.name.clone()),
                };
            }
        }
        // `from . import submodule`: the imported name is a module inside the
        // package, not a symbol in its `__init__`. Resolve it as a submodule
        // before giving up. A module-qualified call names a function, never a
        // submodule.
        if language == Language::Python && fact.kind == EdgeKind::Imports {
            let nested = python_submodule_specifier(specifier, &fact.name);
            if let Some(target) = resolve_specifier(from_path, &nested, known_files, language) {
                return Resolution {
                    class: ResolutionClass::ExplicitImport,
                    reason: None,
                    fact: fact.clone(),
                    to: Some(NodeId::file(&target)),
                    to_name: target,
                };
            }
        }
        // A module-qualified call reads as its spelling (`json.dumps`), not
        // its bare member.
        let display = match (&fact.raw_name, fact.kind) {
            (Some(raw), EdgeKind::Calls) => raw.clone(),
            _ => fact.name.clone(),
        };
        return dangling(display, "import_target_missing_or_ambiguous");
    }

    // A same-file name must be unique and kind-compatible. Duplicate
    // methods or nested declarations cannot be collapsed to the first row.
    let local: Vec<&Node> = table
        .named(&fact.name)
        .into_iter()
        .filter(|n| {
            n.path == from_path
                && lexically_visible(n, fact, from_path)
                && admits(fact.kind, &fact.name, language, n.kind)
                && (n.qualified_name.as_deref() == Some(fact.name.as_str())
                    || n.name.as_deref() == Some(fact.name.as_str()))
        })
        .collect();
    if let [node] = local.as_slice() {
        return Resolution {
            class: ResolutionClass::SameFile,
            reason: None,
            fact: fact.clone(),
            to: Some(node.id.clone()),
            to_name: node
                .qualified_name
                .clone()
                .unwrap_or_else(|| fact.name.clone()),
        };
    }
    if !local.is_empty() {
        return dangling(fact.name.clone(), "ambiguous_same_file");
    }
    if fact.kind == EdgeKind::Exports
        && matches!(language, Language::JavaScript | Language::TypeScript)
    {
        return dangling(fact.name.clone(), "js_export_local_missing");
    }

    // Qualified references require the whole lexical path. Stripping an
    // arbitrary receiver/module to its suffix invents edges (external::run
    // must not bind to an unrelated workspace run).
    if fact.name.contains("::") || fact.name.contains('.') {
        let mut matches = table.named(&fact.name).into_iter().filter(|n| {
            n.qualified_name.as_deref() == Some(fact.name.as_str())
                && admits(fact.kind, &fact.name, language, n.kind)
                && lexically_visible(n, fact, from_path)
        });
        if let Some(node) = matches.next()
            && matches.next().is_none()
        {
            return Resolution {
                fact: fact.clone(),
                to: Some(node.id.clone()),
                to_name: fact.name.clone(),
                class: ResolutionClass::Qualified,
                reason: None,
            };
        }
        return dangling(fact.name.clone(), "qualified_target_missing_or_ambiguous");
    }

    // Rule 4: exactly one workspace symbol of a compatible kind.
    if let Some(id) = table
        .unique_global(&fact.name, fact.kind, language)
        .cloned()
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
            class: ResolutionClass::UniqueName,
            reason: None,
        };
    }

    // Rule 5: dangling, recorded with the name it referred to.
    dangling(fact.name.clone(), "name_missing_or_ambiguous")
}

fn lexically_visible(node: &Node, fact: &ReferenceFact, path: &str) -> bool {
    if node.attribute("lexical_local") != Some("true") {
        return true;
    }
    node.path == path
        && fact.span.is_some_and(|span| {
            node.attribute("lexical_start")
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|start| start <= span.start_byte)
                && node
                    .attribute("lexical_end")
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_some_and(|end| span.end_byte <= end)
        })
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
    project_references(
        file_id,
        file_path,
        "",
        extraction,
        symbol_ids,
        table,
        known_files,
        language,
    )
    .0
}

/// Projects aggregate relationships and independent source occurrences in one resolution pass.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn project_references(
    file_id: &NodeId,
    file_path: &str,
    source_hash: &str,
    extraction: &Extraction,
    symbol_ids: &BTreeMap<String, NodeId>,
    table: &SymbolTable,
    known_files: &BTreeSet<String>,
    language: Language,
) -> (Vec<Edge>, graph_search_types::occurrence::OccurrenceFile) {
    use graph_search_types::occurrence::{OccurrenceExtent, OccurrenceFile, ReferenceOccurrence};
    let mut edges = Vec::new();
    let mut occurrences = OccurrenceFile {
        source_hash: source_hash.into(),
        version: graph_search_types::limits::OCCURRENCE_VERSION,
        complete: true,
        records: Vec::new(),
    };
    let receivers = (language == Language::Rust).then(|| {
        crate::rust_receivers::Receivers::new(&extraction.references, file_path, table, known_files)
    });
    for (ordinal, fact) in extraction.references.iter().enumerate() {
        let from = fact
            .from_key
            .as_ref()
            .and_then(|key| symbol_ids.get(key))
            .cloned()
            .unwrap_or_else(|| file_id.clone());
        let from = occurrence_owner(&from, fact, file_id, table);
        let resolution = resolve_in(
            fact,
            file_path,
            table,
            known_files,
            language,
            receivers.as_ref(),
        );
        let mut record = ReferenceOccurrence {
            id: String::new(),
            owner: from.clone(),
            kind: fact.kind,
            span: fact.span,
            line: fact.line,
            extent: if fact.span.is_none() {
                OccurrenceExtent::LineOnly
            } else if fact.kind == EdgeKind::Calls {
                OccurrenceExtent::Expression
            } else {
                OccurrenceExtent::EnclosingSyntax
            },
            raw_name: fact.raw_name.clone(),
            name: fact.name.clone(),
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            target: resolution.to.clone(),
            target_name: resolution.to_name.clone(),
            resolution: resolution.class,
            reason: resolution.reason.clone(),
            scope: fact.scope,
            binding: fact.binding,
        };
        record.id = crate::occurrences::identity(file_path, source_hash, &record);
        occurrences.records.push(record);
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
    (edges, occurrences)
}

/// Repeated parser keys (for example cfg alternatives) cannot select an owner
/// by last-name-wins. Original coordinates select the actual declaration.
fn occurrence_owner(
    candidate: &NodeId,
    fact: &ReferenceFact,
    file: &NodeId,
    table: &SymbolTable,
) -> NodeId {
    let Some(span) = fact.span else {
        return candidate.clone();
    };
    let Some(node) = table.symbols.get(candidate) else {
        return file.clone();
    };
    let encloses = |node: &&Node| {
        node.span.is_some_and(|owner| {
            owner.start_byte <= span.start_byte && span.end_byte <= owner.end_byte
        })
    };
    if encloses(&node) {
        return candidate.clone();
    }
    let mut owners = node
        .qualified_name
        .as_ref()
        .and_then(|name| table.qualified_candidates.get(name))
        .into_iter()
        .flatten()
        .filter_map(|id| table.symbols.get(id))
        .filter(|other| other.path == node.path && other.kind == node.kind)
        .filter(encloses);
    match (owners.next(), owners.next()) {
        (Some(owner), None) => owner.id.clone(),
        _ => file.clone(),
    }
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
            ..Extraction::default()
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
    fn dangling_names_are_canonicalised_to_one_bounded_line() {
        assert_eq!(
            canonical_dangling_name("docs\n      .filter((doc) => doc.id)"),
            "docs.filter((doc) => doc.id)"
        );
        assert_eq!(canonical_dangling_name("plain_name"), "plain_name");
        // A long name is bounded whether or not it contains whitespace.
        let long = canonical_dangling_name(&"x".repeat(200));
        assert!(long.len() <= MAX_DANGLING_NAME_BYTES, "{}", long.len());
        assert!(long.contains('…'));
        let truncated = canonical_dangling_name(&format!("{}\n  .tail", "y".repeat(200)));
        assert!(
            truncated.len() <= MAX_DANGLING_NAME_BYTES,
            "{}",
            truncated.len()
        );
        assert!(truncated.contains('…'));
        // The canonical name is the dangling edge identity, so two distinct long
        // names that share a prefix past the bound must not collapse into one.
        let a = canonical_dangling_name(&format!("{}.first()", "z".repeat(150)));
        let b = canonical_dangling_name(&format!("{}.second()", "z".repeat(150)));
        assert_ne!(a, b);
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
    fn qualified_paths_do_not_discard_unknown_module_prefixes() {
        let target = symbol("src/t.rs", NodeKind::Struct, "Token", "Token");
        let table = table_with(&[target]);
        let fact = ReferenceFact::from_symbol("x", EdgeKind::TypeUses, "crate::Token", 4);
        let resolved =
            resolve_reference(&fact, "src/a.rs", &table, &BTreeSet::new(), Language::Rust);
        assert!(resolved.to.is_none());
    }

    #[test]
    fn explicit_module_syntax_cannot_fall_through_when_projection_is_missing() {
        let mut fact =
            ReferenceFact::file_level(EdgeKind::Imports, "child", 1).at(Span::new(1, 1, 0, 10));
        let legacy = serde_json::to_value(&fact).unwrap();
        assert!(legacy.get("rust_module_declaration").is_none());
        assert!(
            !serde_json::from_value::<ReferenceFact>(legacy)
                .unwrap()
                .rust_module_declaration
        );
        fact.rust_module_declaration = true;
        let known = BTreeSet::from(["src/child.rs".into()]);
        let result = resolve_reference(
            &fact,
            "src/lib.rs",
            &SymbolTable::new(),
            &known,
            Language::Rust,
        );
        assert!(result.to.is_none());
        assert_eq!(
            result.reason.as_deref(),
            Some("rust_module_declaration_missing_or_ambiguous")
        );
        assert!(
            serde_json::from_value::<ReferenceFact>(serde_json::to_value(&fact).unwrap())
                .unwrap()
                .rust_module_declaration
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
    fn python_import_specifiers_resolve_to_modules() {
        let known = BTreeSet::from([
            "pkg/__init__.py".to_owned(),
            "pkg/mod.py".to_owned(),
            "pkg/sub/__init__.py".to_owned(),
            "pkg/sub/deep.py".to_owned(),
        ]);
        let resolve = |from: &str, specifier: &str| {
            resolve_specifier(from, specifier, &known, Language::Python)
        };
        // Absolute dotted paths.
        assert_eq!(resolve("app.py", "pkg.mod").as_deref(), Some("pkg/mod.py"));
        assert_eq!(resolve("app.py", "pkg").as_deref(), Some("pkg/__init__.py"));
        // Relative: one dot is the file's package, each further dot walks up.
        assert_eq!(
            resolve("pkg/sub/use.py", ".deep").as_deref(),
            Some("pkg/sub/deep.py")
        );
        assert_eq!(
            resolve("pkg/sub/use.py", "..mod").as_deref(),
            Some("pkg/mod.py")
        );
        assert_eq!(
            resolve("pkg/use.py", ".").as_deref(),
            Some("pkg/__init__.py")
        );
        // A relative import past the workspace root has no target.
        assert_eq!(resolve("pkg/use.py", "..").as_deref(), None);
    }

    #[test]
    fn python_relative_imports_past_the_root_have_no_target() {
        let known = BTreeSet::from(["x.py".to_owned(), "pkg/use.py".to_owned()]);
        let resolve = |from: &str, specifier: &str| {
            resolve_specifier(from, specifier, &known, Language::Python)
        };
        // `..` from `pkg/use.py` is the root, so `..x` is `x.py`.
        assert_eq!(resolve("pkg/use.py", "..x").as_deref(), Some("x.py"));
        // One dot further walks past the root.
        assert_eq!(resolve("pkg/use.py", "...x"), None);
        assert_eq!(resolve("use.py", "..x"), None);
        // `from .. import x` in a top-level file is not the root's `x.py`.
        let table = SymbolTable::new();
        let fact = ReferenceFact::file_level(EdgeKind::Imports, "x", 1).via_import("..");
        let resolved = resolve_reference(&fact, "use.py", &table, &known, Language::Python);
        assert_eq!(resolved.to, None);
    }

    #[test]
    fn python_function_local_imports_resolve_to_modules() {
        // `def f(): import util` names the module `util.py`, never an unrelated
        // workspace symbol that happens to share its name.
        let rust_module = symbol("lib.rs", NodeKind::Module, "util", "util");
        let table = table_with(&[rust_module]);
        let known = BTreeSet::from(["lib.rs".to_owned(), "util.py".to_owned()]);
        let fact = ReferenceFact::from_symbol("function:f", EdgeKind::Imports, "util", 2);
        let resolved = resolve_reference(&fact, "app.py", &table, &known, Language::Python);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("file:util.py")
        );
        assert_eq!(
            import_specifiers(&fact, Language::Python),
            vec!["util".to_owned()]
        );
        let binding = ReferenceFact::file_level(EdgeKind::Imports, "mod", 1).via_import("pkg");
        assert_eq!(
            import_specifiers(&binding, Language::Python),
            vec!["pkg".to_owned(), "pkg.mod".to_owned()]
        );
    }

    #[test]
    fn python_submodule_import_resolves_the_module_file() {
        let known = BTreeSet::from(["pkg/__init__.py".to_owned(), "pkg/mod.py".to_owned()]);
        let table = SymbolTable::new();
        // `from pkg import mod`: `mod` is a module, not a symbol in `__init__`.
        let fact = ReferenceFact::file_level(EdgeKind::Imports, "mod", 1).via_import("pkg");
        let resolved = resolve_reference(&fact, "app.py", &table, &known, Language::Python);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("file:pkg/mod.py")
        );
    }

    #[test]
    fn python_imported_symbol_resolves_in_its_module() {
        let target = symbol("pkg/mod.py", NodeKind::Function, "run", "run");
        let table = table_with(&[target]);
        let known = BTreeSet::from(["pkg/mod.py".to_owned()]);
        let fact = ReferenceFact::file_level(EdgeKind::Imports, "run", 1).via_import("pkg.mod");
        let resolved = resolve_reference(&fact, "app.py", &table, &known, Language::Python);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("sym:pkg/mod.py#function:run")
        );
    }

    #[test]
    fn imported_bindings_find_the_export() {
        let exported = symbol(
            "src/lib.ts",
            NodeKind::TypeAlias,
            "SearchQuery",
            "SearchQuery",
        );
        let mut table = table_with(&[exported]);
        let extraction = graph_search_types::extraction::SharedExtraction::from(
            graph_search_types::extraction::Extraction {
                js_module: Some(graph_search_types::js_module::JsModule {
                    complete: true,
                    is_module: true,
                    imports: Vec::new(),
                    exports: vec![graph_search_types::js_module::JsExport {
                        exported: "SearchQuery".into(),
                        local: Some("SearchQuery".into()),
                        source: None,
                        type_only: false,
                        span: graph_search_types::node::Span::default(),
                    }],
                }),
                ..Default::default()
            },
        );
        table.prepare_js_modules([("src/lib.ts", &extraction)]);
        let mut known = BTreeSet::new();
        known.insert(String::from("src/lib.ts"));
        let mut fact = ReferenceFact::from_symbol("x", EdgeKind::TypeUses, "SearchQuery", 12);
        fact = fact.via_import("./lib");
        let resolved = resolve_reference(&fact, "src/app.ts", &table, &known, Language::TypeScript);
        assert_eq!(
            resolved.to.as_ref().map(NodeId::as_str),
            Some("sym:src/lib.ts#type_alias:SearchQuery")
        );
    }
}
