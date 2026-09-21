//! Compact generation-owned dependency facts, including unresolved references.
//!
//! These records describe binding inputs, not just successfully resolved edges.
//! They can be serialized independently of parser payloads. An absent index must
//! use conservative repair; an absent extraction is never an empty extraction.
use graph_search_types::{Edge, Language, Manifest, Node, kind::EdgeKind, manifest::FileEntry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Postings = BTreeMap<String, BTreeSet<String>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    header: FileEntry,
    language: Language,
    facts_available: bool,
    names: BTreeSet<String>,
    references: BTreeSet<String>,
    imports: BTreeSet<String>,
    module: Option<graph_search_types::js_module::JsModule>,
    surface: Option<String>,
    rust_sensitive: bool,
    bare_import: bool,
}

/// Reverse binding dependencies belonging to one coherent graph/manifest pair.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyIndex {
    format: u32,
    records: BTreeMap<String, Record>,
    consumers: Postings,
    incoming: Postings,
    selected_modules: Postings,
}

impl DependencyIndex {
    /// Builds records only when graph files and manifest fingerprints agree.
    /// Incoherent or legacy adapter states return `None` for conservative repair.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one pass over each class of binding input
    pub fn build<'a>(
        manifest: &Manifest,
        nodes: impl IntoIterator<Item = &'a Node>,
        edges: &[Edge],
    ) -> Option<Self> {
        Self::build_retaining(manifest, nodes, edges, None, &BTreeSet::new())
    }

    /// Builds a new generation using previous records for explicitly retained facts.
    /// Callers must validate retention against the old generation and forbid
    /// graph upserts/removals for retained owners before calling this method.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one pass over each class of binding input
    pub fn build_retaining<'a>(
        manifest: &Manifest,
        nodes: impl IntoIterator<Item = &'a Node>,
        edges: &[Edge],
        previous: Option<&Self>,
        retained: &BTreeSet<String>,
    ) -> Option<Self> {
        let mut grouped: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
        let mut paths = BTreeMap::new();
        let mut files = BTreeMap::new();
        for node in nodes {
            grouped.entry(&node.path).or_default().push(node);
            paths.insert(&node.id, &node.path);
            if node.is_file() {
                files.insert(node.path.clone(), node);
            }
        }
        if files.keys().ne(manifest.entries.keys())
            || grouped.keys().any(|path| !files.contains_key(*path))
        {
            return None;
        }
        let known: BTreeSet<_> = files.keys().cloned().collect();
        let mut index = Self {
            format: 1,
            records: BTreeMap::new(),
            consumers: Postings::new(),
            incoming: Postings::new(),
            selected_modules: Postings::new(),
        };
        for (path, entry) in &manifest.entries {
            let file = files.get(path)?;
            if file.content_hash.as_deref() != Some(&entry.content_hash)
                || file.parser_version != Some(entry.parser_version)
                || file.bytes != Some(entry.size)
            {
                return None;
            }
            let language = file.language.unwrap_or(Language::Unknown);
            let mut record = Record {
                header: entry.header(),
                language,
                facts_available: entry.extraction.is_some(),
                names: BTreeSet::new(),
                references: BTreeSet::new(),
                imports: BTreeSet::new(),
                module: None,
                surface: None,
                rust_sensitive: false,
                bare_import: false,
            };
            let owned = grouped.get(path.as_str())?;
            for node in owned {
                record.names.extend(node.name.iter().cloned());
                record.names.extend(node.qualified_name.iter().cloned());
            }
            if let Some(facts) = &entry.extraction {
                record.module = crate::binding_surface::module(facts);
                if entry.quarantine.is_none() {
                    record.surface =
                        crate::binding_surface::fingerprint(owned.iter().copied(), facts, language);
                }
                for symbol in &facts.symbols {
                    record.names.insert(symbol.name.clone());
                    record.names.insert(symbol.qualified_name.clone());
                    record.rust_sensitive |= symbol
                        .attributes
                        .get("rust_module_form")
                        .is_some_and(|form| form == "external");
                }
                for fact in &facts.references {
                    record.rust_sensitive |= fact.rust_use.is_some()
                        || ["crate::", "self::", "super::"]
                            .iter()
                            .any(|prefix| fact.name.starts_with(prefix));
                    if fact.dynamic {
                        continue;
                    }
                    record.references.insert(fact.name.clone());
                    if let Some(specifier) = fact.via_import.as_ref().or_else(|| {
                        (fact.kind == EdgeKind::Imports && fact.from_key.is_none())
                            .then_some(&fact.name)
                    }) {
                        record.imports.insert(specifier.clone());
                    }
                }
                if let Some(module) = &record.module {
                    record
                        .imports
                        .extend(module.imports.iter().map(|import| import.source.clone()));
                    record.imports.extend(
                        module
                            .exports
                            .iter()
                            .filter_map(|export| export.source.clone()),
                    );
                    record.bare_import = record.imports.iter().any(|specifier| {
                        !specifier.starts_with('.') && !specifier.starts_with('/')
                    });
                }
            }
            if retained.contains(path) {
                if entry.extraction.is_some() {
                    return None;
                }
                let prior = previous?.records.get(path)?;
                let mut expected = prior.header.clone();
                expected.mtime_ns = entry.mtime_ns;
                if !prior.facts_available || expected != entry.header() {
                    return None;
                }
                record = prior.clone();
                record.header = entry.header();
            }
            for name in &record.references {
                index
                    .consumers
                    .entry(name.clone())
                    .or_default()
                    .insert(path.clone());
            }
            for specifier in &record.imports {
                if let Some(target) =
                    crate::resolve::resolve_specifier(path, specifier, &known, language)
                {
                    index
                        .selected_modules
                        .entry(target)
                        .or_default()
                        .insert(path.clone());
                }
            }
            index.records.insert(path.clone(), record);
        }
        for edge in edges {
            if let Some(target) = edge.to.as_ref().and_then(|id| paths.get(id)) {
                if let Some(source) = paths.get(&edge.from) {
                    index
                        .incoming
                        .entry((*target).clone())
                        .or_default()
                        .insert((*source).clone());
                }
                if let Some(owner) = &edge.path
                    && known.contains(owner)
                {
                    index
                        .incoming
                        .entry((*target).clone())
                        .or_default()
                        .insert(owner.clone());
                }
            }
        }
        Some(index)
    }

    /// Cached ECMAScript surfaces; coordinates are normalized for binding only.
    pub fn modules(
        &self,
    ) -> impl Iterator<Item = (&str, &graph_search_types::js_module::JsModule)> {
        self.records.iter().filter_map(|(path, record)| {
            record.module.as_ref().map(|module| (path.as_str(), module))
        })
    }

    /// Whether the generation has a cached extraction, including an empty one.
    #[must_use]
    pub fn has_facts(&self, path: &str) -> bool {
        self.records
            .get(path)
            .is_some_and(|record| record.facts_available)
    }

    /// Checks header identity, cache availability and reproducible reverse maps.
    /// Artifact authentication must separately bind this index to its graph.
    #[must_use]
    pub fn validates(&self, header: &Manifest, available: &BTreeSet<String>) -> bool {
        if self.format != 1 || self.records.keys().ne(header.entries.keys()) {
            return false;
        }
        let known: BTreeSet<_> = self.records.keys().cloned().collect();
        if !available.is_subset(&known) {
            return false;
        }
        let mut consumers = Postings::new();
        let mut selected = Postings::new();
        for (path, record) in &self.records {
            if header.get(path) != Some(&record.header)
                || record.header.extraction.is_some()
                || record.facts_available != available.contains(path)
            {
                return false;
            }
            for name in &record.references {
                consumers
                    .entry(name.clone())
                    .or_default()
                    .insert(path.clone());
            }
            for specifier in &record.imports {
                if let Some(target) =
                    crate::resolve::resolve_specifier(path, specifier, &known, record.language)
                {
                    selected.entry(target).or_default().insert(path.clone());
                }
            }
        }
        self.consumers == consumers
            && self.selected_modules == selected
            && self
                .incoming
                .iter()
                .all(|(target, sources)| known.contains(target) && sources.is_subset(&known))
    }

    /// Whether this changed file retains the cached outward binding surface.
    #[must_use]
    pub fn surface_unchanged(
        &self,
        path: &str,
        header: &FileEntry,
        nodes: &[Node],
        facts: &graph_search_types::extraction::Extraction,
        language: Language,
    ) -> bool {
        self.records.get(path).is_some_and(|record| {
            record.header == header.header()
                && record.surface.is_some()
                && record.surface == crate::binding_surface::fingerprint(nodes, facts, language)
        })
    }

    /// Computes conservative repair closure without reading raw parser payloads.
    /// `changed` contains binding edits/removals; `new_names` contains names from
    /// their new projections. Physically changed files are independently upserted.
    /// # Errors
    /// Propagates cancellation or work-budget errors from `check`.
    pub fn repair_paths(
        &self,
        mut changed: BTreeSet<String>,
        new_names: &BTreeSet<String>,
        new_files: &BTreeSet<String>,
        boundary_changed: bool,
        mut check: impl FnMut() -> crate::Result<()>,
    ) -> crate::Result<BTreeSet<String>> {
        if boundary_changed
            || changed
                .iter()
                .any(|path| crate::packages::manifest_family(path).is_some())
        {
            changed.extend(new_files.iter().cloned());
        }
        let old_files: BTreeSet<_> = self.records.keys().cloned().collect();
        let presence_changed = old_files != *new_files;
        let rust_changed = presence_changed
            || changed.iter().any(|path| {
                std::path::Path::new(path)
                    .extension()
                    .is_some_and(|ext| ext == "rs")
            });
        for (path, record) in &self.records {
            check()?;
            if (rust_changed && record.rust_sensitive) || (presence_changed && record.bare_import) {
                changed.insert(path.clone());
            }
        }
        let mut names = new_names.clone();
        for path in &changed {
            check()?;
            if let Some(record) = self.records.get(path) {
                names.extend(record.names.iter().cloned());
            }
        }
        for (path, record) in &self.records {
            check()?;
            if !record.facts_available && record.header.quarantine.is_none() {
                changed.extend(new_files.iter().cloned());
            }
            if matches!(record.language, Language::Html | Language::Css) {
                changed.insert(path.clone());
            }
            if presence_changed {
                for specifier in &record.imports {
                    check()?;
                    if crate::resolve::resolve_specifier(
                        path,
                        specifier,
                        &old_files,
                        record.language,
                    ) != crate::resolve::resolve_specifier(
                        path,
                        specifier,
                        new_files,
                        record.language,
                    ) {
                        changed.insert(path.clone());
                    }
                }
            }
        }
        for name in names {
            check()?;
            if let Some(paths) = self.consumers.get(&name) {
                changed.extend(paths.iter().cloned());
            }
        }
        let mut queue: VecDeque<_> = changed.iter().cloned().collect();
        while let Some(target) = queue.pop_front() {
            check()?;
            for source in self
                .incoming
                .get(&target)
                .into_iter()
                .chain(self.selected_modules.get(&target))
                .flatten()
            {
                if changed.insert(source.clone()) {
                    queue.push_back(source.clone());
                }
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::{
        NodeId, Span,
        extraction::{Extraction, ReferenceFact},
        js_module::{JsExport, JsModule},
        kind::NodeKind,
    };

    fn fixture() -> (Manifest, Vec<Node>) {
        let mut manifest = Manifest::new(1, 1);
        let nodes = ["a.js", "b.js", "c.js", "cold.js"]
            .map(|path| Node::file(path, Language::JavaScript, 3, 1, "abc", 1))
            .to_vec();
        for node in &nodes {
            manifest.entries.insert(
                node.path.clone(),
                FileEntry {
                    size: 3,
                    mtime_ns: 0,
                    content_hash: "abc".into(),
                    parser_version: 1,
                    schema_version: 1,
                    quarantine: None,
                    extraction: Some(
                        Extraction {
                            js_module: Some(JsModule::default()),
                            ..Extraction::default()
                        }
                        .into(),
                    ),
                },
            );
        }
        (manifest, nodes)
    }

    fn facts(manifest: &mut Manifest, path: &str, facts: Extraction) {
        manifest.entries.get_mut(path).unwrap().extraction = Some(facts.into());
    }

    #[test]
    fn unresolved_names_and_module_choices_form_transitive_closure() {
        let (mut manifest, nodes) = fixture();
        facts(
            &mut manifest,
            "b.js",
            Extraction {
                references: vec![ReferenceFact::file_level(EdgeKind::Calls, "new_export", 1)],
                ..Extraction::default()
            },
        );
        facts(
            &mut manifest,
            "c.js",
            Extraction {
                js_module: Some(JsModule {
                    exports: vec![JsExport {
                        exported: "*".into(),
                        local: None,
                        source: Some("./b".into()),
                        type_only: false,
                        span: Span::default(),
                    }],
                    ..JsModule::default()
                }),
                ..Extraction::default()
            },
        );
        let index = DependencyIndex::build(&manifest, &nodes, &[]).unwrap();
        let paths = manifest.entries.keys().cloned().collect();
        let affected = index
            .repair_paths(
                BTreeSet::from(["a.js".into()]),
                &BTreeSet::from(["new_export".into()]),
                &paths,
                false,
                || Ok(()),
            )
            .unwrap();
        assert_eq!(
            affected,
            BTreeSet::from(["a.js".into(), "b.js".into(), "c.js".into()])
        );
        let bytes = serde_json::to_vec(&index).unwrap();
        let decoded: DependencyIndex = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, index);
        assert!(decoded.validates(&manifest.header(), &paths));
        let mut invalid = decoded;
        invalid.consumers.clear();
        assert!(!invalid.validates(&manifest.header(), &paths));
    }

    #[test]
    fn added_target_repairs_previously_unresolved_import() {
        let (mut manifest, nodes) = fixture();
        facts(
            &mut manifest,
            "b.js",
            Extraction {
                references: vec![ReferenceFact::file_level(EdgeKind::Imports, "./new", 1)],
                ..Extraction::default()
            },
        );
        let index = DependencyIndex::build(&manifest, &nodes, &[]).unwrap();
        let mut paths: BTreeSet<_> = manifest.entries.keys().cloned().collect();
        paths.insert("new.js".into());
        let affected = index
            .repair_paths(
                BTreeSet::from(["new.js".into()]),
                &BTreeSet::new(),
                &paths,
                false,
                || Ok(()),
            )
            .unwrap();
        assert_eq!(affected, BTreeSet::from(["new.js".into(), "b.js".into()]));
    }

    #[test]
    fn missing_facts_fallback_is_distinct_from_empty_and_quarantine() {
        let (mut manifest, nodes) = fixture();
        manifest.entries.get_mut("cold.js").unwrap().extraction = None;
        let paths: BTreeSet<_> = manifest.entries.keys().cloned().collect();
        let index = DependencyIndex::build(&manifest, &nodes, &[]).unwrap();
        assert_eq!(
            index
                .repair_paths(BTreeSet::new(), &BTreeSet::new(), &paths, false, || Ok(()))
                .unwrap(),
            paths
        );
        assert!(!index.validates(&manifest.header(), &paths));
        let available = paths.iter().filter(|p| *p != "cold.js").cloned().collect();
        assert!(index.validates(&manifest.header(), &available));
        manifest.entries.get_mut("cold.js").unwrap().quarantine = Some("parse failure".into());
        let index = DependencyIndex::build(&manifest, &nodes, &[]).unwrap();
        assert!(
            index
                .repair_paths(BTreeSet::new(), &BTreeSet::new(), &paths, false, || Ok(()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn coherent_fingerprints_are_required() {
        let (mut manifest, mut nodes) = fixture();
        nodes[0].content_hash = Some("different".into());
        assert!(DependencyIndex::build(&manifest, &nodes, &[]).is_none());
        nodes[0].content_hash = Some("abc".into());
        manifest.entries.remove("a.js");
        assert!(DependencyIndex::build(&manifest, &nodes, &[]).is_none());
    }

    #[test]
    fn surface_ignores_presentation_but_keeps_binding_semantics() {
        let (manifest, mut nodes) = fixture();
        let mut symbol = Node {
            id: NodeId::symbol("a.js", NodeKind::Function, "f", None),
            kind: NodeKind::Function,
            path: "a.js".into(),
            name: Some("f".into()),
            qualified_name: Some("f".into()),
            ..Node::default()
        };
        nodes.push(symbol.clone());
        let index = DependencyIndex::build(&manifest, &nodes, &[]).unwrap();
        let entry = manifest.get("a.js").unwrap();
        let facts = entry.extraction.as_ref().unwrap();
        symbol.span = Some(Span::new(2, 4, 10, 42));
        symbol.signature = Some("async function f(a,b)".into());
        symbol.is_async = true;
        assert!(index.surface_unchanged(
            "a.js",
            entry,
            &[symbol.clone()],
            facts,
            Language::JavaScript
        ));
        symbol.name = Some("renamed".into());
        assert!(!index.surface_unchanged("a.js", entry, &[symbol], facts, Language::JavaScript));
    }
}
