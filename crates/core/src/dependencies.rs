//! Compact generation-owned dependency facts, including unresolved references
//! (`research/16-proportional-sync.md`, phase 2).
//!
//! Each file has one [`DependencyRecord`]: the names it declares and references,
//! the import specifiers it resolves, its ECMAScript surface and the links of
//! the edges it owns. A record derives its own posting rows, so a store can keep
//! the reverse maps (who consumes a name, who selected a file, who links into
//! it, whose imports could select a path) as keyed tables updated per file.
//! Repair reads only those keyed lookups ([`DependencyLookup`]).
//!
//! An absent index must use conservative repair; an absent extraction is never
//! an empty extraction.
use graph_search_types::js_module::JsModule;
use graph_search_types::{Edge, Language, Manifest, Node, manifest::FileEntry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Postings = BTreeMap<String, BTreeSet<String>>;

/// A set of files repair consults as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flag {
    /// Files whose bindings depend on Rust module or path structure.
    RustSensitive,
    /// Files importing a bare (package) specifier.
    BareImport,
    /// Files with no cached facts that were not quarantined: their bindings are
    /// unknown, so every change rebinds every file.
    NoFacts,
    /// HTML and CSS files, rebound on every change.
    Markup,
    /// Files with cached facts.
    Facts,
}

impl Flag {
    /// Every flag.
    pub const ALL: [Self; 5] = [
        Self::RustSensitive,
        Self::BareImport,
        Self::NoFacts,
        Self::Markup,
        Self::Facts,
    ];

    /// The flag's stable spelling (a posting key).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RustSensitive => "rust_sensitive",
            Self::BareImport => "bare_import",
            Self::NoFacts => "no_facts",
            Self::Markup => "markup",
            Self::Facts => "facts",
        }
    }
}

/// One file's dependency facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyRecord {
    /// The file version the record describes, without its timestamp.
    header: FileEntry,
    language: Language,
    facts_available: bool,
    names: BTreeSet<String>,
    references: BTreeSet<String>,
    imports: BTreeSet<String>,
    module: Option<JsModule>,
    surface: Option<String>,
    rust_sensitive: bool,
    bare_import: bool,
    /// `(target path, source path)` links of the edges the file owns.
    links: BTreeSet<(String, String)>,
}

/// A manifest entry's identity, without its timestamp or facts.
fn version(entry: &FileEntry) -> FileEntry {
    let mut header = entry.header();
    header.mtime_ns = 0;
    header
}

/// The posting rows one record contributes, each owned by its file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordPostings {
    /// Names whose rebinding elsewhere can change this file's bindings.
    pub consumers: BTreeSet<String>,
    /// Files this file's imports select.
    pub selected: BTreeSet<String>,
    /// Every file any of this file's imports could select.
    pub candidates: BTreeSet<String>,
    /// `(target, source)` links of the file's edges.
    pub incoming: BTreeSet<(String, String)>,
    /// The flag sets the file belongs to.
    pub flags: Vec<Flag>,
}

impl DependencyRecord {
    /// A path's record from its nodes, the links of the edges it owns and, when
    /// present, its extraction facts. `None` when the nodes do not own exactly
    /// the manifest's file version.
    #[must_use]
    pub fn build(
        entry: &FileEntry,
        nodes: &[&Node],
        links: BTreeSet<(String, String)>,
    ) -> Option<Self> {
        let file = nodes.iter().find(|node| node.is_file())?;
        if file.content_hash.as_deref() != Some(&entry.content_hash)
            || file.parser_version != Some(entry.parser_version)
            || file.bytes != Some(entry.size)
        {
            return None;
        }
        let language = file.language.unwrap_or(Language::Unknown);
        let mut record = Self {
            header: version(entry),
            language,
            facts_available: entry.extraction.is_some(),
            names: BTreeSet::new(),
            references: BTreeSet::new(),
            imports: BTreeSet::new(),
            module: None,
            surface: None,
            rust_sensitive: false,
            bare_import: false,
            links,
        };
        for node in nodes {
            record.names.extend(node.name.iter().cloned());
            record.names.extend(node.qualified_name.iter().cloned());
        }
        if let Some(facts) = &entry.extraction {
            record.module = crate::binding_surface::module(facts);
            if entry.quarantine.is_none() {
                record.surface =
                    crate::binding_surface::fingerprint(nodes.iter().copied(), facts, language);
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
                    || fact.receiver.is_some()
                    || ["crate::", "self::", "super::"]
                        .iter()
                        .any(|prefix| fact.name.starts_with(prefix));
                // A receiver call depends on its member and on the fields
                // its receiver is reached through, wherever they are declared.
                if let Some(receiver) = &fact.receiver {
                    let raw = fact.raw_name.as_deref().unwrap_or(&fact.name);
                    if let Some((_, member)) = raw.rsplit_once('.') {
                        record.references.insert(
                            member
                                .split("::")
                                .next()
                                .unwrap_or(member)
                                .trim()
                                .to_owned(),
                        );
                    }
                    record.references.extend(receiver_names(receiver));
                }
                if fact.dynamic {
                    continue;
                }
                record.references.insert(fact.name.clone());
                record
                    .imports
                    .extend(crate::resolve::import_specifiers(fact, language));
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
                record.bare_import = record
                    .imports
                    .iter()
                    .any(|specifier| !specifier.starts_with('.') && !specifier.starts_with('/'));
            }
        }
        Some(record)
    }

    /// A retained file's record: its previous record, whose facts the new
    /// generation keeps unread, with its current links. `None` when the
    /// previous record has no facts or describes another file version.
    #[must_use]
    pub fn retain(
        prior: &Self,
        entry: &FileEntry,
        links: BTreeSet<(String, String)>,
    ) -> Option<Self> {
        if entry.extraction.is_some() || !prior.facts_available || prior.header != version(entry) {
            return None;
        }
        let mut record = prior.clone();
        record.links = links;
        Some(record)
    }

    /// Whether the record describes `entry`'s file version.
    #[must_use]
    pub fn describes(&self, entry: &FileEntry) -> bool {
        self.header == version(entry)
    }

    /// The file's language.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    /// Whether the file has cached facts, including empty ones.
    #[must_use]
    pub const fn facts_available(&self) -> bool {
        self.facts_available
    }

    /// The names the file declares.
    #[must_use]
    pub const fn names(&self) -> &BTreeSet<String> {
        &self.names
    }

    /// The file's ECMAScript surface, when it has one.
    #[must_use]
    pub const fn module(&self) -> Option<&JsModule> {
        self.module.as_ref()
    }

    /// The flag sets the file belongs to.
    #[must_use]
    pub fn flags(&self) -> Vec<Flag> {
        let mut flags = Vec::new();
        if self.rust_sensitive {
            flags.push(Flag::RustSensitive);
        }
        if self.bare_import {
            flags.push(Flag::BareImport);
        }
        if !self.facts_available && self.header.quarantine.is_none() {
            flags.push(Flag::NoFacts);
        }
        if matches!(self.language, Language::Html | Language::Css) {
            flags.push(Flag::Markup);
        }
        if self.facts_available {
            flags.push(Flag::Facts);
        }
        flags
    }

    /// The posting rows this record, owned by `path`, contributes; `known` is
    /// the file set its imports resolve against.
    #[must_use]
    pub fn postings(&self, path: &str, known: &BTreeSet<String>) -> RecordPostings {
        let mut postings = RecordPostings {
            consumers: self.references.clone(),
            flags: self.flags(),
            ..RecordPostings::default()
        };
        for specifier in &self.imports {
            let candidates =
                crate::resolve::specifier_candidates(path, specifier, known, self.language);
            if let Some(selected) = candidates
                .iter()
                .find(|candidate| known.contains(*candidate))
            {
                postings.selected.insert(selected.clone());
            }
            postings.candidates.extend(candidates);
        }
        postings.incoming = self
            .links
            .iter()
            .filter(|(target, source)| known.contains(target) && known.contains(source))
            .cloned()
            .collect();
        postings
    }

    /// Whether any import of this file, owned by `path`, selects differently
    /// against `old` and `new` file sets.
    fn selection_changed(
        &self,
        path: &str,
        old: &BTreeSet<String>,
        new: &BTreeSet<String>,
    ) -> bool {
        self.imports.iter().any(|specifier| {
            crate::resolve::resolve_specifier(path, specifier, old, self.language)
                != crate::resolve::resolve_specifier(path, specifier, new, self.language)
        })
    }
}

/// Keyed reads over one generation's dependency records and postings.
pub trait DependencyLookup {
    /// Every file the generation has a record for.
    ///
    /// # Errors
    /// When the records cannot be read.
    fn paths(&self) -> crate::Result<BTreeSet<String>>;
    /// One file's record.
    ///
    /// # Errors
    /// When the record cannot be read or fails verification.
    fn record(&self, path: &str) -> crate::Result<Option<DependencyRecord>>;
    /// Files that reference `name`.
    ///
    /// # Errors
    /// When the postings cannot be read.
    fn consumers(&self, name: &str) -> crate::Result<Vec<String>>;
    /// Files with an edge into `target`.
    ///
    /// # Errors
    /// When the postings cannot be read.
    fn incoming(&self, target: &str) -> crate::Result<Vec<String>>;
    /// Files whose imports select `target`.
    ///
    /// # Errors
    /// When the postings cannot be read.
    fn selected(&self, target: &str) -> crate::Result<Vec<String>>;
    /// Files with an import that could select `path`.
    ///
    /// # Errors
    /// When the postings cannot be read.
    fn candidates(&self, path: &str) -> crate::Result<Vec<String>>;
    /// The files in one flag set.
    ///
    /// # Errors
    /// When the postings cannot be read.
    fn flagged(&self, flag: Flag) -> crate::Result<BTreeSet<String>>;
    /// Every cached ECMAScript surface.
    ///
    /// # Errors
    /// When the surfaces cannot be read.
    fn modules(&self) -> crate::Result<Vec<(String, JsModule)>>;
}

/// Whether this changed file retains the cached outward binding surface.
///
/// # Errors
/// When the file's record cannot be read.
pub fn surface_unchanged(
    lookup: &dyn DependencyLookup,
    path: &str,
    header: &FileEntry,
    nodes: &[Node],
    facts: &graph_search_types::extraction::Extraction,
    language: Language,
) -> crate::Result<bool> {
    Ok(lookup.record(path)?.is_some_and(|record| {
        record.describes(header)
            && record.surface.is_some()
            && record.surface == crate::binding_surface::fingerprint(nodes, facts, language)
    }))
}

/// Computes conservative repair closure from keyed lookups, without reading
/// raw parser payloads. `changed` contains binding edits/removals; `new_names`
/// contains names from their new projections. Physically changed files are
/// independently upserted.
///
/// # Errors
/// Propagates lookup failures and cancellation or work-budget errors from `check`.
pub fn repair_paths(
    lookup: &dyn DependencyLookup,
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
    let old_files = lookup.paths()?;
    let presence_changed = old_files != *new_files;
    let rust_changed = presence_changed
        || changed.iter().any(|path| {
            std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext == "rs")
        });
    if rust_changed {
        changed.extend(lookup.flagged(Flag::RustSensitive)?);
    }
    if presence_changed {
        changed.extend(lookup.flagged(Flag::BareImport)?);
    }
    let mut names = new_names.clone();
    for path in &changed {
        check()?;
        if let Some(record) = lookup.record(path)? {
            names.extend(record.names.iter().cloned());
        }
    }
    if !lookup.flagged(Flag::NoFacts)?.is_empty() {
        changed.extend(new_files.iter().cloned());
    }
    changed.extend(lookup.flagged(Flag::Markup)?);
    if presence_changed {
        // A selection can change only where an appearing or vanishing file is
        // one of the specifier's candidates.
        let mut importers = BTreeSet::new();
        for path in old_files.symmetric_difference(new_files) {
            check()?;
            importers.extend(lookup.candidates(path)?);
        }
        for importer in importers {
            check()?;
            if lookup
                .record(&importer)?
                .is_some_and(|record| record.selection_changed(&importer, &old_files, new_files))
            {
                changed.insert(importer);
            }
        }
    }
    for name in names {
        check()?;
        changed.extend(lookup.consumers(&name)?);
    }
    let mut queue: VecDeque<_> = changed.iter().cloned().collect();
    while let Some(target) = queue.pop_front() {
        check()?;
        let mut sources = lookup.incoming(&target)?;
        sources.extend(lookup.selected(&target)?);
        for source in sources {
            // Rebinding an unchanged OKF document leaves its symbols as they
            // were, so nothing that links to it can bind differently: its
            // dependents are not followed (`SPEC.md` §7.6).
            if changed.insert(source.clone())
                && lookup
                    .record(&source)?
                    .is_none_or(|record| record.language != Language::Okf)
            {
                queue.push_back(source.clone());
            }
        }
    }
    Ok(changed)
}

/// One path's contribution to a [`DependencyIndex`]: the nodes it owns (its
/// file node included) and the `(target path, source path)` links of the
/// edges it owns (see [`edge_links`]).
pub struct Contribution<'a> {
    /// Every node the path owns, its file node included.
    pub nodes: Vec<&'a Node>,
    /// `(target path, source path)` pairs of the edges the path owns.
    pub links: BTreeSet<(String, String)>,
}

/// The `(target path, source path)` links one edge contributes: its target's
/// path paired with its source node's path and with its reference path, when
/// each is a known file. `path_of` maps a node id to its owning path.
pub fn edge_links(
    edge: &Edge,
    path_of: impl Fn(&graph_search_types::NodeId) -> Option<String>,
    known: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let Some(target) = edge.to.as_ref().and_then(&path_of) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    if let Some(source) = path_of(&edge.from) {
        links.push((target.clone(), source));
    }
    if let Some(owner) = &edge.path
        && known(owner)
    {
        links.push((target, owner.clone()));
    }
    links
}

/// An in-memory dependency index: every record and the reverse maps derived
/// from them. The in-memory store keeps one; the native store keeps records in
/// its shards and the reverse maps as posting tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyIndex {
    records: BTreeMap<String, DependencyRecord>,
    consumers: Postings,
    incoming: Postings,
    selected_modules: Postings,
    candidates: Postings,
    flags: BTreeMap<Flag, BTreeSet<String>>,
}

impl DependencyIndex {
    /// Builds records only when graph files and manifest fingerprints agree.
    /// Incoherent or legacy adapter states return `None` for conservative repair.
    #[must_use]
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
    pub fn build_retaining<'a>(
        manifest: &Manifest,
        nodes: impl IntoIterator<Item = &'a Node>,
        edges: &[Edge],
        previous: Option<&Self>,
        retained: &BTreeSet<String>,
    ) -> Option<Self> {
        let mut contributions: BTreeMap<String, Contribution<'a>> = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for node in nodes {
            paths.insert(node.id.clone(), node.path.clone());
            contributions
                .entry(node.path.clone())
                .or_insert_with(|| Contribution {
                    nodes: Vec::new(),
                    links: BTreeSet::new(),
                })
                .nodes
                .push(node);
        }
        let known: BTreeSet<String> = contributions.keys().cloned().collect();
        for edge in edges {
            let links = edge_links(
                edge,
                |id| paths.get(id).cloned(),
                |path| known.contains(path),
            );
            // A link belongs to the path that owns the edge.
            let owner = edge
                .path
                .clone()
                .filter(|path| known.contains(path))
                .or_else(|| paths.get(&edge.from).cloned());
            if let Some(contribution) = owner.and_then(|owner| contributions.get_mut(&owner)) {
                contribution.links.extend(links);
            }
        }
        // Every manifest path is rebuilt from its own nodes; the previous index
        // supplies only retained records.
        if contributions.keys().ne(manifest.entries.keys()) {
            return None;
        }
        Self::update(manifest, previous, retained, &contributions)
    }

    /// The next index: every path in `contributions` is rebuilt from its nodes
    /// (or, when retained, from its previous record) with its new links; every
    /// other manifest path keeps its previous record. Reverse maps are rebuilt
    /// from the records. `None` when the inputs are incoherent: a path with
    /// neither a contribution nor a matching previous record, a node set that
    /// does not own its file, or a retained path without previous facts.
    #[must_use]
    pub fn update(
        manifest: &Manifest,
        previous: Option<&Self>,
        retained: &BTreeSet<String>,
        contributions: &BTreeMap<String, Contribution<'_>>,
    ) -> Option<Self> {
        if contributions
            .keys()
            .any(|path| !manifest.entries.contains_key(path))
        {
            return None;
        }
        let mut records = BTreeMap::new();
        for (path, entry) in &manifest.entries {
            let record = if let Some(contribution) = contributions.get(path) {
                if retained.contains(path) {
                    DependencyRecord::retain(
                        previous?.records.get(path)?,
                        entry,
                        contribution.links.clone(),
                    )?
                } else {
                    DependencyRecord::build(entry, &contribution.nodes, contribution.links.clone())?
                }
            } else {
                let prior = previous?.records.get(path)?;
                // Facts supplied for an uncontributed path would be ignored.
                if !prior.describes(entry) || entry.extraction.is_some() {
                    return None;
                }
                prior.clone()
            };
            records.insert(path.clone(), record);
        }
        Some(Self::from_records(records))
    }

    /// The index over `records`, deriving every reverse map.
    #[must_use]
    pub fn from_records(records: BTreeMap<String, DependencyRecord>) -> Self {
        let known: BTreeSet<String> = records.keys().cloned().collect();
        let mut index = Self {
            records: BTreeMap::new(),
            consumers: Postings::new(),
            incoming: Postings::new(),
            selected_modules: Postings::new(),
            candidates: Postings::new(),
            flags: BTreeMap::new(),
        };
        for (path, record) in &records {
            let postings = record.postings(path, &known);
            for name in postings.consumers {
                index
                    .consumers
                    .entry(name)
                    .or_default()
                    .insert(path.clone());
            }
            for target in postings.selected {
                index
                    .selected_modules
                    .entry(target)
                    .or_default()
                    .insert(path.clone());
            }
            for candidate in postings.candidates {
                index
                    .candidates
                    .entry(candidate)
                    .or_default()
                    .insert(path.clone());
            }
            for (target, source) in postings.incoming {
                index.incoming.entry(target).or_default().insert(source);
            }
            for flag in postings.flags {
                index.flags.entry(flag).or_default().insert(path.clone());
            }
        }
        index.records = records;
        index
    }

    /// The record of `path`.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&DependencyRecord> {
        self.records.get(path)
    }
}

impl DependencyLookup for DependencyIndex {
    fn paths(&self) -> crate::Result<BTreeSet<String>> {
        Ok(self.records.keys().cloned().collect())
    }

    fn record(&self, path: &str) -> crate::Result<Option<DependencyRecord>> {
        Ok(self.records.get(path).cloned())
    }

    fn consumers(&self, name: &str) -> crate::Result<Vec<String>> {
        Ok(self
            .consumers
            .get(name)
            .into_iter()
            .flatten()
            .cloned()
            .collect())
    }

    fn incoming(&self, target: &str) -> crate::Result<Vec<String>> {
        Ok(self
            .incoming
            .get(target)
            .into_iter()
            .flatten()
            .cloned()
            .collect())
    }

    fn selected(&self, target: &str) -> crate::Result<Vec<String>> {
        Ok(self
            .selected_modules
            .get(target)
            .into_iter()
            .flatten()
            .cloned()
            .collect())
    }

    fn candidates(&self, path: &str) -> crate::Result<Vec<String>> {
        Ok(self
            .candidates
            .get(path)
            .into_iter()
            .flatten()
            .cloned()
            .collect())
    }

    fn flagged(&self, flag: Flag) -> crate::Result<BTreeSet<String>> {
        Ok(self.flags.get(&flag).cloned().unwrap_or_default())
    }

    fn modules(&self) -> crate::Result<Vec<(String, JsModule)>> {
        Ok(self
            .records
            .iter()
            .filter_map(|(path, record)| record.module.clone().map(|module| (path.clone(), module)))
            .collect())
    }
}

/// The names a receiver description reaches through: its fields and the type
/// `self` stands for.
fn receiver_names(receiver: &graph_search_types::extraction::ReceiverType) -> Vec<String> {
    use graph_search_types::extraction::ReceiverType;
    match receiver {
        ReceiverType::SelfType(owner) => {
            vec![owner.rsplit("::").next().unwrap_or(owner).to_owned()]
        }
        ReceiverType::Field(inner, name) => {
            let mut names = receiver_names(inner);
            names.push(name.clone());
            names
        }
        ReceiverType::Try(inner) => receiver_names(inner),
        ReceiverType::Declared(value) => vec![
            value
                .rsplit([':', '>', '.'])
                .next()
                .unwrap_or(value)
                .to_owned(),
        ],
        ReceiverType::Annotation(_) | ReceiverType::Return(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::{
        NodeId, Span,
        extraction::{Extraction, ReferenceFact},
        js_module::{JsExport, JsModule},
        kind::{EdgeKind, NodeKind},
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
        let affected = repair_paths(
            &index,
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
        let affected = repair_paths(
            &index,
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
            repair_paths(
                &index,
                BTreeSet::new(),
                &BTreeSet::new(),
                &paths,
                false,
                || Ok(())
            )
            .unwrap(),
            paths
        );
        manifest.entries.get_mut("cold.js").unwrap().quarantine = Some("parse failure".into());
        let index = DependencyIndex::build(&manifest, &nodes, &[]).unwrap();
        assert!(
            repair_paths(
                &index,
                BTreeSet::new(),
                &BTreeSet::new(),
                &paths,
                false,
                || Ok(())
            )
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
        assert!(
            surface_unchanged(
                &index,
                "a.js",
                entry,
                &[symbol.clone()],
                facts,
                Language::JavaScript
            )
            .unwrap()
        );
        symbol.name = Some("renamed".into());
        assert!(
            !surface_unchanged(
                &index,
                "a.js",
                entry,
                &[symbol],
                facts,
                Language::JavaScript
            )
            .unwrap()
        );
    }
}
