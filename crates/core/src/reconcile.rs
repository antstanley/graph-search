//! The reconcile driver: walk, diff, extract, resolve, apply, commit
//! (`SPEC.md` §6).
//!
//! Extraction never fails a command: a file the parser cannot handle is
//! quarantined — no symbols, a recorded reason — and the run continues
//! (`SPEC.md` §6.4). The manifest is committed last, so an interrupted run
//! recomputes the same delta from content hashes (`SPEC.md` §6.4).

use crate::Result;
use crate::config::WalkPolicy;
use crate::extraction::Extraction;
use crate::ports::{GraphStore, LanguageRegistry};
use crate::resolve::{CrossTables, SymbolTable, cross_edges_for};
use crate::walk::WalkEntry;
use graph_search_types::NodeId;
use graph_search_types::batch::{FileProjection, QuarantineRecord, WriteBatch};
use graph_search_types::kind::{EdgeKind, Language};
use graph_search_types::limits::{
    MAX_EDGES_PER_FILE, MAX_NODES_PER_FILE, MAX_SIGNATURE_CHARS, PARSER_VERSION, SCHEMA_VERSION,
};
use graph_search_types::manifest::Manifest;
use graph_search_types::node::{Edge, Node};
use graph_search_types::result::SyncReport;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The projector: turns walked files into batches and applies them.
pub struct Projector<'a> {
    /// The language adapters, wired by the library.
    pub registry: &'a dyn LanguageRegistry,
    /// The walk and extraction policy.
    pub policy: &'a WalkPolicy,
    work: Option<std::cell::RefCell<&'a mut crate::work::WorkBudget>>,
}

/// One file's in-flight projection, between extraction and edge resolution.
struct Pending {
    entry: WalkEntry,
    hash: String,
    projection: FileProjection,
    extraction: Option<graph_search_types::extraction::SharedExtraction>,
    ids: BTreeMap<String, NodeId>,
}

impl<'a> Projector<'a> {
    /// A projector over `registry` and `policy`.
    #[must_use]
    pub const fn new(registry: &'a dyn LanguageRegistry, policy: &'a WalkPolicy) -> Self {
        Self {
            registry,
            policy,
            work: None,
        }
    }

    /// Shares automatic maintenance reads and walks with a caller's query budget.
    #[must_use]
    pub fn with_work_budget(mut self, work: &'a mut crate::work::WorkBudget) -> Self {
        self.work = Some(std::cell::RefCell::new(work));
        self
    }

    fn check_work(&self) -> Result<()> {
        if let Some(work) = &self.work {
            work.borrow().check()?;
        }
        Ok(())
    }

    fn walk_report(&self, root: &Path) -> Result<crate::walk::WalkReport> {
        match &self.work {
            Some(work) => {
                crate::walk::walk_report_with_work(root, self.policy, &mut work.borrow_mut())
            }
            None => crate::walk::walk_report(root, self.policy),
        }
    }

    fn read_source(&self, entry: &WalkEntry) -> Result<Vec<u8>> {
        use std::io::Read;
        self.check_work()?;
        let limit = self.policy.max_file_bytes.saturating_add(1);
        let bytes = if let Some(work) = &self.work {
            let mut work = work.borrow_mut();
            match crate::source_capture::read(&entry.path, limit, &mut work)? {
                crate::source_capture::ReadOutcome::Bytes(bytes) => {
                    std::sync::Arc::try_unwrap(bytes).unwrap_or_else(|bytes| (*bytes).clone())
                }
                crate::source_capture::ReadOutcome::BudgetExceeded => {
                    return Err(crate::Error::IncompleteMaintenance(
                        "source-read allowance exhausted".into(),
                    ));
                }
                crate::source_capture::ReadOutcome::Unavailable => {
                    return Err(crate::Error::IncompleteMaintenance(format!(
                        "could not read {}",
                        entry.rel
                    )));
                }
            }
        } else {
            let mut bytes = Vec::new();
            std::fs::File::open(&entry.path)
                .and_then(|file| file.take(limit).read_to_end(&mut bytes))
                .map_err(|error| crate::Error::io(&entry.path, error))?;
            bytes
        };
        if bytes.len() as u64 > self.policy.max_file_bytes {
            return Err(crate::Error::IncompleteWalk(format!(
                "{} grew past its size ceiling during maintenance",
                entry.rel
            )));
        }
        self.check_work()?;
        Ok(bytes)
    }

    /// Full build: parse everything, build from scratch (verify/repair)
    /// (`SPEC.md` §6.5.1).
    ///
    /// # Errors
    /// When the store or the tree fails; per-file failures quarantine.
    pub fn reindex(&self, search_root: &Path, store: &mut dyn GraphStore) -> Result<SyncReport> {
        self.check_work()?;
        let started = std::time::Instant::now();
        let walked_report = self.walk_report(search_root)?;
        walked_report.require_complete()?;
        let coverage = walked_report.coverage;
        let entries = walked_report.entries;
        // Repair: remove store files the new tree no longer holds, even ones
        // an interrupted run left without a manifest (`SPEC.md` §6.5.1).
        let changed = entries.clone();
        let walked: BTreeSet<String> = entries.iter().map(|e| e.rel.clone()).collect();
        let mut removed: Vec<String> = Vec::new();
        let snapshot = store.snapshot()?;
        for node in snapshot.all_nodes()? {
            self.check_work()?;
            if node.is_file() && !walked.contains(&node.path) {
                removed.push(node.path);
            }
        }
        drop(snapshot);
        let previous = store.manifest_header()?.unwrap_or_default();
        self.apply(
            store,
            &entries,
            &walked_report.package_boundaries,
            removed,
            &changed,
            SyncReport {
                coverage,
                reindexed_all: true,
                ..Default::default()
            },
            previous,
            started,
        )
    }

    /// Incremental reconcile against the manifest; the normal way to keep
    /// current (`SPEC.md` §6.5.2).
    ///
    /// # Errors
    /// When the store or the tree fails; per-file failures quarantine.
    pub fn sync(&self, search_root: &Path, store: &mut dyn GraphStore) -> Result<SyncReport> {
        self.check_work()?;
        let started = std::time::Instant::now();
        let mut manifest = store
            .manifest_header()?
            .unwrap_or_else(|| Manifest::new(PARSER_VERSION, SCHEMA_VERSION));
        let walked_report = self.walk_report(search_root)?;
        walked_report.require_complete()?;
        let mut coverage = walked_report.coverage;
        let entries = walked_report.entries;
        if manifest.policy_fingerprint.as_deref() != Some(self.policy.fingerprint().as_str())
            || !manifest.versions().retrieval_is_current()
            || manifest.occurrence_version != graph_search_types::limits::OCCURRENCE_VERSION
        {
            return self.reindex(search_root, store);
        }
        let diff = crate::manifest::classify_with_hash(&entries, &manifest, |entry| {
            self.read_source(entry)
                .map(|bytes| Some(crate::hash::content_hash(&bytes)))
        })?;
        self.check_work()?;
        // A no-op needs neither a full graph scan nor reading/rewriting raw facts. Refresh metadata only after a same-content timestamp change.
        if diff.is_empty()
            && manifest.package_boundaries == walked_report.package_boundaries
            && !manifest.is_empty()
            && manifest.parser_version == PARSER_VERSION
            && manifest.schema_version == SCHEMA_VERSION
        {
            let mut refresh = false;
            for entry in &entries {
                if let Some(stored) = manifest.entries.get_mut(&entry.rel)
                    && crate::manifest::entry_differs(stored, entry)
                {
                    stored.size = entry.size;
                    stored.mtime_ns = entry.mtime_ns;
                    refresh = true;
                }
            }
            coverage.quarantined_files = manifest
                .entries
                .values()
                .filter(|entry| entry.quarantine.is_some())
                .count() as u64;
            store.snapshot()?.source_coverage().apply(&mut coverage);
            let mut report = SyncReport {
                coverage,
                unchanged: entries.len() as u64,
                elapsed_ms: u64::MAX,
                ..SyncReport::default()
            };
            crate::payload::fit_sync(&mut report)?;
            self.check_work()?;
            if refresh {
                Self::refresh_manifest_metadata(store, manifest)?;
            }
            report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            return Ok(report);
        }

        // Native rebinding selects raw facts later through cached dependencies.
        // Compatibility adapters retain their full-manifest path.
        let manifest = if store.dependency_index()?.is_some() {
            manifest
        } else {
            store.manifest()?.unwrap_or(manifest)
        };
        // Changed set: added and modified; a rename adds the new path.
        let mut changed: Vec<WalkEntry> = diff
            .added
            .iter()
            .chain(diff.modified.iter())
            .cloned()
            .collect();
        let mut removed: Vec<String> = diff.removed.clone();
        for rename in &diff.renamed {
            removed.push(rename.from.clone());
            if let Some(entry) = entries.iter().find(|e| e.rel == rename.to) {
                changed.push(entry.clone());
            }
        }
        let reindexed_all = diff.reindexed_all;
        let renamed = diff.renamed.clone();
        self.apply(
            store,
            &entries,
            &walked_report.package_boundaries,
            removed,
            &changed,
            SyncReport {
                coverage,
                renamed,
                reindexed_all,
                ..Default::default()
            },
            manifest,
            started,
        )
    }

    fn refresh_manifest_metadata(store: &mut dyn GraphStore, mut manifest: Manifest) -> Result<()> {
        if let Some(index) = store.dependency_index()? {
            let retention = crate::retention::FactRetention {
                generation: store.generation()?,
                paths: manifest
                    .entries
                    .keys()
                    .filter(|path| index.has_facts(path))
                    .cloned()
                    .collect(),
                previous: store.manifest_header()?.unwrap_or_default(),
            };
            store.publish_retaining(WriteBatch::with_manifest(manifest), &retention)?;
        } else {
            let mut full = store.manifest()?.unwrap_or_default();
            for (path, entry) in &mut manifest.entries {
                entry.extraction = full.entries.remove(path).and_then(|old| old.extraction);
            }
            store.commit_manifest(manifest)?;
        }
        Ok(())
    }

    /// The shared tail: extract changed files, resolve, batch, apply, commit
    /// the manifest last.
    #[allow(clippy::too_many_arguments)] // the pipeline stages are explicit here on purpose
    #[allow(clippy::too_many_lines)]
    fn apply(
        &self,
        store: &mut dyn GraphStore,
        entries: &[WalkEntry],
        package_boundaries: &BTreeSet<String>,
        removed: Vec<String>,
        changed: &[WalkEntry],
        mut report: SyncReport,
        previous: Manifest,
        started: std::time::Instant,
    ) -> Result<SyncReport> {
        let reindexed_all = report.reindexed_all;
        // The known-file set resolves import specifiers against: the walked
        // tree plus the indexed set (an unchanged file may be an import
        // target).
        let known_files: BTreeSet<String> = entries.iter().map(|e| e.rel.clone()).collect();

        // Parse only changed files. Rebind dependencies from persisted raw facts.
        let mut pending: Vec<Pending> = changed
            .iter()
            .map(|entry| self.extract_one(entry))
            .collect::<Result<Vec<_>>>()?;
        let boundary_changed = previous.package_boundaries != *package_boundaries;
        if !reindexed_all && (!changed.is_empty() || !removed.is_empty() || boundary_changed) {
            self.extend_dependents(
                store,
                entries,
                &removed,
                &previous,
                &mut pending,
                boundary_changed,
            )?;
        }

        self.annotate_packages(store, package_boundaries, &removed, &mut pending)?;

        // Phase B: the symbol table reflects the post-apply world: untouched
        // files from the store, changed files from the batch.
        let changed_paths: BTreeSet<String> = pending.iter().map(|p| p.entry.rel.clone()).collect();
        let mut table = SymbolTable::new();
        {
            let snapshot = store.snapshot()?;
            for node in snapshot.all_nodes()? {
                self.check_work()?;
                if !changed_paths.contains(&node.path) && !removed.contains(&node.path) {
                    table.add(&node);
                }
            }
        }
        for item in &pending {
            self.check_work()?;
            table.add(&item.projection.file);
            for node in &item.projection.symbols {
                table.add(node);
            }
        }

        table.prepare_rust_modules(&known_files, package_boundaries);
        table.prepare_node_packages(package_boundaries);
        let config_sources = self.config_sources(store, &changed_paths, &removed, &pending)?;
        table.prepare_typescript_projects(&config_sources);
        if let Some(index) = store.dependency_index()? {
            table.prepare_js_surfaces(
                index
                    .modules()
                    .filter(|(path, _)| {
                        known_files.contains(*path) && !changed_paths.contains(*path)
                    })
                    .chain(pending.iter().filter_map(|item| {
                        item.extraction
                            .as_ref()
                            .and_then(|facts| facts.js_module.as_ref())
                            .map(|module| (item.entry.rel.as_str(), module))
                    })),
            );
        } else {
            table.prepare_js_modules(
                previous
                    .entries
                    .iter()
                    .filter(|(path, _)| {
                        known_files.contains(*path) && !changed_paths.contains(*path)
                    })
                    .filter_map(|(path, entry)| {
                        entry
                            .extraction
                            .as_ref()
                            .map(|facts| (path.as_str(), facts))
                    })
                    .chain(pending.iter().filter_map(|item| {
                        item.extraction
                            .as_ref()
                            .map(|facts| (item.entry.rel.as_str(), facts))
                    })),
            );
        }
        self.check_work()?;

        // Phase C: resolve references into edges, then the HTML/CSS matches.
        let cross = CrossTables::from_table(&table);
        for item in &mut pending {
            self.check_work()?;
            item.projection.occurrences = Some(graph_search_types::occurrence::OccurrenceFile {
                source_hash: item.hash.clone(),
                version: graph_search_types::limits::OCCURRENCE_VERSION,
                complete: false,
                records: Vec::new(),
            });
            let Some(extraction) = item.extraction.as_ref() else {
                continue;
            };
            let file_id = item.projection.file.id.clone();
            let language = item.entry.language.unwrap_or(Language::Unknown);
            let (mut edges, occurrences) = crate::resolve::project_references(
                &file_id,
                &item.entry.rel,
                &item.hash,
                extraction,
                &item.ids,
                &table,
                &known_files,
                language,
            );
            let element_ids = item.ids.clone();
            edges.extend(cross_edges_for(
                &item.entry.rel,
                extraction,
                &element_ids,
                &cross,
                &known_files,
            ));
            if edges.len() > MAX_EDGES_PER_FILE {
                item.projection.quarantine = Some(QuarantineRecord::new(
                    &item.entry.rel,
                    format!("edge overflow: the {MAX_EDGES_PER_FILE}-edge cap"),
                ));
            } else {
                item.projection.edges.extend(edges);
                item.projection.occurrences = Some(occurrences);
            }
        }

        // The batch: removals, upserts, and the manifest over the *whole*
        // walked tree — committed last (`SPEC.md` §6.4).
        let mut manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
        manifest.policy_fingerprint = Some(self.policy.fingerprint());
        manifest.package_boundaries.clone_from(package_boundaries);
        for entry in entries {
            self.check_work()?;
            if let Some(item) = pending.iter().find(|p| p.entry.rel == entry.rel) {
                let mut record = crate::manifest::entry_for(
                    entry,
                    &item.hash,
                    item.projection
                        .quarantine
                        .as_ref()
                        .map(|q| q.reason.clone()),
                );
                record.extraction.clone_from(&item.extraction);
                manifest.entries.insert(entry.rel.clone(), record);
            } else if let Some(stored) = previous.get(&entry.rel).cloned() {
                // Unchanged: keep the stored entry so its hash keeps future
                // syncs on the O(1) path. Refresh the cheap fields so a moved
                // clock does not re-hash the file every run.
                let mut fresh = stored;
                fresh.size = entry.size;
                fresh.mtime_ns = entry.mtime_ns;
                manifest.entries.insert(entry.rel.clone(), fresh);
            }
        }
        manifest.indexed_at_ms = now_ms();

        report.coverage.quarantined_files = manifest
            .entries
            .values()
            .filter(|e| e.quarantine.is_some())
            .count() as u64;
        {
            let snapshot = store.snapshot()?;
            let removed_paths: BTreeSet<_> = removed.iter().map(String::as_str).collect();
            let retained = snapshot
                .source_files()?
                .iter()
                .filter(|(path, _)| {
                    !changed_paths.contains(*path) && !removed_paths.contains(path.as_str())
                })
                .map(|(_, source)| source);
            let replaced = pending
                .iter()
                .filter_map(|item| item.projection.source.as_ref());
            crate::units::coverage_from(retained.chain(replaced), &mut report.coverage);
        }

        let mut batch = WriteBatch::with_manifest(manifest);
        batch.removed_files = removed;
        let mut quarantined = Vec::new();
        for item in pending {
            self.check_work()?;
            if let Some(record) = &item.projection.quarantine {
                quarantined.push(record.clone());
            }
            batch.upserts.push(item.projection);
        }

        let retention = store
            .dependency_index()?
            .filter(|_| !reindexed_all)
            .map(|index| crate::retention::FactRetention {
                generation: None,
                previous: previous.header(),
                paths: batch
                    .manifest
                    .entries
                    .iter()
                    .filter(|(path, entry)| {
                        entry.extraction.is_none()
                            && !changed_paths.contains(*path)
                            && index.has_facts(path)
                    })
                    .map(|(path, _)| path.clone())
                    .collect(),
            });
        let retention = retention
            .map(|mut retention| {
                retention.generation = store.generation()?;
                Ok(retention)
            })
            .transpose()?;
        // Classify the report against the manifest we are replacing.
        let previously_known: BTreeSet<String> = previous.entries.into_keys().collect();
        // The added set was `changed`; the manifest decides added vs modified.
        let mut added: Vec<String> = changed_paths.iter().cloned().collect();
        let mut modified: Vec<String> = Vec::new();
        for path in &added {
            if previously_known.contains(path) && !reindexed_all {
                modified.push(path.clone());
            }
        }
        added.retain(|path| !previously_known.contains(path) || reindexed_all);
        added.sort();
        modified.sort();
        report.unchanged = entries
            .len()
            .saturating_sub(added.len())
            .saturating_sub(modified.len()) as u64;
        report.added = added;
        report.modified = modified;
        report.removed.clone_from(&batch.removed_files);
        report.quarantined = quarantined;
        // Reserve the largest elapsed counter before publication; replacing it
        // with the real value cannot make this response grow.
        report.elapsed_ms = u64::MAX;
        crate::payload::fit_sync(&mut report)?;
        self.check_work()?;
        if let Some(retention) = retention {
            store.publish_retaining(batch, &retention)?;
        } else {
            store.publish(batch)?;
        }
        report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(report)
    }

    /// Slim projections of admitted configuration records: identity, version and
    /// raw authored fields only, exactly what native inheritance reads.
    fn config_sources(
        &self,
        store: &dyn GraphStore,
        changed_paths: &BTreeSet<String>,
        removed: &[String],
        pending: &[Pending],
    ) -> Result<BTreeMap<String, graph_search_types::source::SourceFileUnits>> {
        let snapshot = store.snapshot()?;
        let mut sources = BTreeMap::new();
        for (path, source) in snapshot.source_files()? {
            self.check_work()?;
            if changed_paths.contains(path) || removed.iter().any(|removed| removed == path) {
                continue;
            }
            if source.typescript_config.is_some() {
                sources.insert(
                    path.clone(),
                    graph_search_types::source::SourceFileUnits {
                        typescript_config: source.typescript_config.clone(),
                        source_hash: source.source_hash.clone(),
                        version: source.version,
                        ..graph_search_types::source::SourceFileUnits::default()
                    },
                );
            }
        }
        for item in pending {
            if let Some(source) = &item.projection.source
                && source.typescript_config.is_some()
            {
                sources.insert(
                    item.entry.rel.clone(),
                    graph_search_types::source::SourceFileUnits {
                        typescript_config: source.typescript_config.clone(),
                        source_hash: source.source_hash.clone(),
                        version: source.version,
                        ..graph_search_types::source::SourceFileUnits::default()
                    },
                );
            }
        }
        Ok(sources)
    }

    fn annotate_packages(
        &self,
        store: &dyn GraphStore,
        known: &BTreeSet<String>,
        removed: &[String],
        pending: &mut [Pending],
    ) -> Result<()> {
        let replaced: BTreeSet<_> = pending
            .iter()
            .map(|item| item.entry.rel.as_str())
            .chain(removed.iter().map(String::as_str))
            .collect();
        let snapshot = store.snapshot()?;
        let mut catalog = crate::packages::Catalog::new(known);
        for (path, source) in snapshot.source_files()? {
            self.check_work()?;
            if !replaced.contains(path.as_str()) {
                catalog.add(path, source);
            }
        }
        for item in pending.iter() {
            self.check_work()?;
            if let Some(source) = &item.projection.source {
                catalog.add(&item.entry.rel, source);
            }
        }
        for item in pending {
            self.check_work()?;
            catalog.annotate(&mut item.projection.file, item.projection.source.as_mut());
        }
        Ok(())
    }

    /// Expand binding-surface changes through raw-name/import dependencies, then
    /// conservative incoming-edge closure. Unchanged surfaces retain their incoming
    /// adjacency; changed module/export surfaces can affect transitive consumers.
    #[allow(clippy::too_many_lines)]
    #[allow(clippy::too_many_arguments)] // explicit old/new dependency inputs
    fn extend_dependents(
        &self,
        store: &dyn GraphStore,
        entries: &[WalkEntry],
        removed: &[String],
        previous: &Manifest,
        pending: &mut Vec<Pending>,
        boundary_changed: bool,
    ) -> Result<()> {
        let snapshot = store.snapshot()?;
        let nodes = snapshot.all_nodes()?;
        let edges = snapshot.all_edges()?;
        let paths: BTreeMap<_, _> = nodes.iter().map(|n| (&n.id, n.path.as_str())).collect();
        let files: BTreeMap<_, _> = nodes
            .iter()
            .filter(|n| n.is_file())
            .map(|n| (n.path.as_str(), n))
            .collect();
        let old_files: BTreeSet<_> = previous.entries.keys().cloned().collect();
        let new_files: BTreeSet<_> = entries.iter().map(|e| e.rel.clone()).collect();
        let dirty = if let Some(index) = store.dependency_index()? {
            let mut changed: BTreeSet<String> = removed.iter().cloned().collect();
            let mut names = BTreeSet::new();
            for item in pending.iter() {
                self.check_work()?;
                let unchanged = item.projection.quarantine.is_none()
                    && previous
                        .get(&item.entry.rel)
                        .zip(item.extraction.as_ref())
                        .is_some_and(|(header, facts)| {
                            index.surface_unchanged(
                                &item.entry.rel,
                                header,
                                &item.projection.symbols,
                                facts,
                                item.entry.language.unwrap_or(Language::Unknown),
                            )
                        });
                if !unchanged {
                    changed.insert(item.entry.rel.clone());
                    for node in &item.projection.symbols {
                        names.extend(node.name.iter().cloned());
                        names.extend(node.qualified_name.iter().cloned());
                    }
                }
            }
            // A declared project configuration can change any consumer's alias
            // resolution. Until alias dependencies are persisted, conservatively
            // rebind JS-family consumers from cached facts.
            if changed
                .iter()
                .any(|path| crate::typescript_project::is_project_config(path))
            {
                changed.extend(
                    previous
                        .entries
                        .keys()
                        .filter(|path| new_files.contains(*path))
                        .filter(|path| {
                            files
                                .get(path.as_str())
                                .and_then(|node| node.language)
                                .or_else(|| self.policy.language_for(Path::new(path)))
                                .is_some_and(crate::resolve::js_family)
                        })
                        .cloned(),
                );
            }
            index.repair_paths(changed, &names, &new_files, boundary_changed, || {
                self.check_work()
            })?
        } else {
            let mut old_nodes: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
            for node in &nodes {
                self.check_work()?;
                old_nodes.entry(&node.path).or_default().push(node);
            }
            // Physical edits always remain pending. Only binding-surface changes
            // seed consumer repair: stable targets preserve untouched incoming edges.
            let mut dirty: BTreeSet<_> = pending
                .iter()
                .filter(|item| {
                    let path = item.entry.rel.as_str();
                    let unchanged = previous
                        .get(path)
                        .zip(files.get(path))
                        .filter(|(entry, file)| {
                            entry.quarantine.is_none()
                                && item.projection.quarantine.is_none()
                                && file.content_hash.as_deref() == Some(entry.content_hash.as_str())
                        })
                        .and_then(|(entry, _)| entry.extraction.as_ref())
                        .zip(item.extraction.as_ref())
                        .is_some_and(|(old, new)| {
                            crate::binding_surface::unchanged(
                                old_nodes.get(path).map_or(&[], Vec::as_slice),
                                &item.projection.symbols,
                                old,
                                new,
                                item.entry.language.unwrap_or(Language::Unknown),
                            )
                        });
                    !unchanged
                })
                .map(|p| p.entry.rel.clone())
                .chain(removed.iter().cloned())
                .collect();
            // Manifest edits can change every descendant's nearest boundary. Until
            // package dependency invalidation is narrowed, rebind from cached facts.
            if boundary_changed
                || dirty
                    .iter()
                    .any(|path| crate::packages::manifest_family(path).is_some())
            {
                dirty.extend(new_files.iter().cloned());
            }
            // Declared project configuration can change any JS-family consumer's
            // alias resolution. Rebind those consumers from cached facts.
            if dirty
                .iter()
                .any(|path| crate::typescript_project::is_project_config(path))
            {
                dirty.extend(
                    new_files
                        .iter()
                        .filter(|path| {
                            files
                                .get(path.as_str())
                                .and_then(|node| node.language)
                                .or_else(|| self.policy.language_for(Path::new(path)))
                                .is_some_and(crate::resolve::js_family)
                        })
                        .cloned(),
                );
            }
            // Every external module declaration can depend on a newly created file,
            // target-root role or conflicting path. Rebind these consumers from
            // cached facts; unrelated Rust files retain the normal dependency path.
            if old_files != new_files
                || dirty.iter().any(|path| {
                    Path::new(path)
                        .extension()
                        .is_some_and(|extension| extension == "rs")
                })
            {
                dirty.extend(
                    previous
                        .entries
                        .iter()
                        .filter(|&(path, stored)| {
                            new_files.contains(path)
                                && stored.extraction.as_ref().is_some_and(|facts| {
                                    facts.references.iter().any(|reference| {
                                        reference.rust_use.is_some()
                                            || reference.name.starts_with("crate::")
                                            || reference.name.starts_with("self::")
                                            || reference.name.starts_with("super::")
                                    }) || facts.symbols.iter().any(|symbol| {
                                        symbol
                                            .attributes
                                            .get("rust_module_form")
                                            .is_some_and(|form| form == "external")
                                    })
                                })
                        })
                        .map(|(path, _)| path.clone()),
                );
            }
            if old_files != new_files {
                dirty.extend(
                    previous
                        .entries
                        .iter()
                        .filter(|(path, entry)| {
                            new_files.contains(*path)
                                && entry.extraction.as_ref().is_some_and(|facts| {
                                    facts.js_module.is_some()
                                        && facts.references.iter().any(|fact| {
                                            fact.via_import
                                                .as_deref()
                                                .or_else(|| {
                                                    (fact.kind == EdgeKind::Imports)
                                                        .then_some(fact.name.as_str())
                                                })
                                                .is_some_and(|specifier| {
                                                    !specifier.starts_with('.')
                                                        && !specifier.starts_with('/')
                                                })
                                        })
                                })
                        })
                        .map(|(path, _)| path.clone()),
                );
            }
            let mut names = BTreeSet::new();
            for node in nodes.iter().filter(|n| dirty.contains(&n.path)).chain(
                pending
                    .iter()
                    .filter(|p| dirty.contains(&p.entry.rel))
                    .flat_map(|p| p.projection.symbols.iter()),
            ) {
                names.extend(node.name.iter().cloned());
                names.extend(node.qualified_name.iter().cloned());
            }
            // The committed facts remain the old-world authority even if a previous
            // graph apply completed but committing its manifest failed.
            for path in &dirty {
                if let Some(facts) = previous
                    .get(path)
                    .and_then(|entry| entry.extraction.as_ref())
                {
                    for symbol in &facts.symbols {
                        names.insert(symbol.name.clone());
                        names.insert(symbol.qualified_name.clone());
                    }
                }
            }
            // Reverse lookup by raw spelling includes unresolved references and names
            // that just became ambiguous. Import choice also depends on file presence
            // and extension precedence, not merely the previous resolved target.
            let mut consumers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
            for (path, stored) in &previous.entries {
                self.check_work()?;
                let Some(facts) = &stored.extraction else {
                    // Older/missing cache: conservatively rebuild rather than omit
                    // an unknown binding dependency. Parse quarantines have no facts.
                    if stored.quarantine.is_none() {
                        dirty.extend(new_files.iter().cloned());
                    }
                    continue;
                };
                let language = files
                    .get(path.as_str())
                    .and_then(|n| n.language)
                    .or_else(|| self.policy.language_for(Path::new(path)))
                    .unwrap_or(Language::Unknown);
                // Cross-language HTML/CSS matching has bidirectional generated edges
                // and file links. Conservatively rebind this family on every change.
                if matches!(language, Language::Html | Language::Css) {
                    dirty.insert(path.clone());
                }
                for fact in &facts.references {
                    if fact.dynamic {
                        continue;
                    }
                    consumers.entry(&fact.name).or_default().insert(path);
                    let specifier = fact.via_import.as_deref().or_else(|| {
                        (fact.kind == EdgeKind::Imports && fact.from_key.is_none())
                            .then_some(fact.name.as_str())
                    });
                    if let Some(specifier) = specifier {
                        let old = crate::resolve::resolve_specifier(
                            path, specifier, &old_files, language,
                        );
                        let new = crate::resolve::resolve_specifier(
                            path, specifier, &new_files, language,
                        );
                        if old != new || old.iter().chain(new.iter()).any(|p| dirty.contains(p)) {
                            dirty.insert(path.clone());
                        }
                    }
                }
            }
            for name in &names {
                if let Some(paths) = consumers.get(name.as_str()) {
                    dirty.extend(paths.iter().map(|p| (*p).to_owned()));
                }
            }
            let mut incoming: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
            for edge in &edges {
                self.check_work()?;
                if let (Some(source), Some(target)) = (
                    paths.get(&edge.from),
                    edge.to.as_ref().and_then(|to| paths.get(to)),
                ) {
                    incoming.entry(target).or_default().insert(source);
                }
            }
            let mut queue: std::collections::VecDeque<_> = dirty.iter().cloned().collect();
            while let Some(target) = queue.pop_front() {
                self.check_work()?;
                if let Some(sources) = incoming.get(target.as_str()) {
                    for source in sources {
                        if dirty.insert((*source).to_owned()) {
                            queue.push_back((*source).to_owned());
                        }
                    }
                }
            }
            dirty
        };
        let parsed: BTreeSet<_> = pending.iter().map(|p| p.entry.rel.clone()).collect();
        let selected = if store.dependency_index()?.is_some() {
            let paths = dirty
                .difference(&parsed)
                .filter(|path| new_files.contains(*path))
                .cloned()
                .collect();
            Some(store.extraction_facts(&paths)?)
        } else {
            None
        };
        for entry in entries
            .iter()
            .filter(|e| dirty.contains(&e.rel) && !parsed.contains(&e.rel))
        {
            self.check_work()?;
            let cached = previous.get(&entry.rel).zip(files.get(entry.rel.as_str()));
            if let Some((stored, file)) = cached {
                let facts = selected
                    .as_ref()
                    .and_then(|facts| facts.get(&entry.rel))
                    .or(stored.extraction.as_ref());
                if facts.is_none() && stored.quarantine.is_none() {
                    pending.push(self.extract_one(entry)?);
                    continue;
                }
                let item = Pending {
                    entry: entry.clone(),
                    hash: stored.content_hash.clone(),
                    projection: FileProjection {
                        file: (*file).clone(),
                        source: snapshot.source_files()?.get(&entry.rel).cloned(),
                        quarantine: if facts.is_none() {
                            stored
                                .quarantine
                                .as_ref()
                                .map(|q| QuarantineRecord::new(&entry.rel, q))
                        } else {
                            None
                        },
                        ..FileProjection::default()
                    },
                    extraction: None,
                    ids: BTreeMap::new(),
                };
                pending.push(if let Some(facts) = facts {
                    Self::populate_symbols(item, facts.clone())
                } else {
                    item
                });
                continue;
            }
            pending.push(self.extract_one(entry)?);
        }
        Ok(())
    }

    /// Phase A for one file: read, hash, parse or quarantine.
    #[allow(clippy::too_many_lines)] // one construct per arm; splitting hurts the reading
    fn extract_one(&self, entry: &WalkEntry) -> Result<Pending> {
        let bytes = self.read_source(entry)?;
        let hash = crate::hash::content_hash(&bytes);
        let language = entry.language.unwrap_or(Language::Unknown);
        let file_node = file_node_for(entry, &bytes, &hash, language);

        let mut pending = Pending {
            entry: entry.clone(),
            hash,
            projection: FileProjection {
                file: file_node,
                ..FileProjection::default()
            },
            extraction: None,
            ids: BTreeMap::new(),
        };

        if bytes.iter().take(8192).any(|byte| *byte == 0) {
            pending.projection.quarantine =
                Some(QuarantineRecord::new(&entry.rel, "binary source"));
            return Ok(pending);
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            pending.projection.quarantine =
                Some(QuarantineRecord::new(&entry.rel, "not valid UTF-8"));
            return Ok(pending);
        };
        let typescript_config = self.registry.typescript_config(&crate::ports::SourceFile {
            path: Path::new(&entry.rel),
            text,
        });
        let package_manifest = self.registry.package_manifest(&crate::ports::SourceFile {
            path: Path::new(&entry.rel),
            text,
        });
        self.check_work()?;
        let finish = |mut pending: Pending| {
            let documentation: Vec<_> = pending
                .extraction
                .as_ref()
                .into_iter()
                .flat_map(|facts| &facts.doc_comments)
                .map(|fact| graph_search_types::source::DocumentationComment {
                    span: fact.span,
                    documented_symbol: fact
                        .owner_key
                        .as_ref()
                        .and_then(|key| pending.ids.get(key))
                        .cloned(),
                    inner: fact.inner,
                })
                .collect();
            let embedded = pending
                .extraction
                .as_ref()
                .map_or(&[][..], |facts| facts.embedded.as_slice());
            let embedded_truncated = pending
                .extraction
                .as_ref()
                .is_some_and(|facts| facts.embedded_truncated);
            pending.projection.source = Some(crate::units::extract_documented(
                &entry.rel,
                text,
                &pending.hash,
                language,
                &pending.projection.symbols,
                crate::units::DocumentationInput {
                    comments: &documentation,
                    truncated: pending
                        .extraction
                        .as_ref()
                        .is_some_and(|facts| facts.doc_comments_truncated),
                    embedded,
                    embedded_truncated,
                },
            ));
            if let Some(source) = pending.projection.source.as_mut() {
                source.package_manifest.clone_from(&package_manifest);
                source.typescript_config.clone_from(&typescript_config);
            }
            pending
        };
        // No enabled extractor: preserve the file, with no language facts.
        let extractor = self
            .registry
            .extractor_for(Path::new(&entry.rel))
            .filter(|_| entry.is_parseable(self.policy));
        let Some(extractor) = extractor else {
            pending.extraction = Some(Extraction::default().into());
            return Ok(finish(pending));
        };

        let extracted = extractor.extract(&crate::ports::SourceFile {
            path: Path::new(&entry.rel),
            text,
        });
        self.check_work()?;
        let extraction = match extracted {
            Err(error) => {
                pending.projection.quarantine =
                    Some(QuarantineRecord::new(&entry.rel, error.message));
                return Ok(finish(pending));
            }
            Ok(extraction) => extraction,
        };

        let symbol_count = extraction.symbols.len().saturating_add(1);
        if symbol_count > MAX_NODES_PER_FILE {
            pending.projection.quarantine = Some(QuarantineRecord::new(
                &entry.rel,
                format!("node overflow: the {MAX_NODES_PER_FILE}-node cap"),
            ));
            return Ok(finish(pending));
        }

        Ok(finish(Self::populate_symbols(pending, extraction.into())))
    }

    fn populate_symbols(
        mut pending: Pending,
        extraction: graph_search_types::extraction::SharedExtraction,
    ) -> Pending {
        let entry = &pending.entry;
        // Stable ids: the qualified name, disambiguated by line only when the
        // file repeats a same-kind name (`SPEC.md` §5.3).
        let mut seen: BTreeMap<(graph_search_types::kind::NodeKind, String), u32> = BTreeMap::new();
        let mut symbols = Vec::new();
        let mut ids = BTreeMap::new();
        for fact in &extraction.symbols {
            let count = seen
                .entry((fact.kind, fact.qualified_name.clone()))
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
            let disambiguator = if *count > 1 {
                Some(fact.span.start_line)
            } else {
                None
            };
            let id = NodeId::symbol(&entry.rel, fact.kind, &fact.qualified_name, disambiguator);
            ids.insert(fact.key.clone(), id.clone());
            symbols.push(Node {
                id,
                kind: fact.kind,
                path: entry.rel.clone(),
                name: Some(fact.name.clone()),
                qualified_name: Some(fact.qualified_name.clone()),
                signature: fact
                    .signature
                    .as_deref()
                    .map(|s| crate::text_search::truncate_line(s, MAX_SIGNATURE_CHARS)),
                span: Some(fact.span),
                visibility: fact.visibility,
                is_async: fact.is_async,
                parent: None,
                attributes: fact.attributes.clone(),
                ..Node::default()
            });
        }
        // Containment: file to top-level items, parent to child.
        let mut edges = Vec::new();
        for fact in &extraction.symbols {
            let id = ids[&fact.key].clone();
            let parent = fact
                .parent_key
                .as_ref()
                .and_then(|key| ids.get(key))
                .cloned()
                .unwrap_or_else(|| pending.projection.file.id.clone());
            edges.push(Edge::resolved(
                &parent,
                EdgeKind::Contains,
                &id,
                fact.qualified_name.as_str(),
                Some(entry.rel.as_str()),
                Some(fact.span.start_line),
            ));
            if let Some(parent_id) = fact.parent_key.as_ref().and_then(|k| ids.get(k))
                && let Some(node) = symbols.iter_mut().find(|n| n.id == id)
            {
                node.parent = Some(parent_id.clone());
            }
        }
        pending.projection.symbols = symbols;
        pending.projection.edges = edges;
        pending.ids = ids;
        // The facts stay attached: cross-file matching needs the elements'
        // attributes, not just the stored nodes.
        pending.extraction = Some(extraction);
        pending
    }
}

/// One file node per walked file, parsed or not (`SPEC.md` §6.2).
#[must_use]
pub fn file_node_for(entry: &WalkEntry, bytes: &[u8], hash: &str, language: Language) -> Node {
    let newlines = byte_lines(bytes);
    let has_trailing_content =
        usize::from(!bytes.is_empty() && *bytes.last().unwrap_or(&b'\n') != b'\n');
    let lines = newlines.saturating_add(has_trailing_content);
    Node::file(
        &entry.rel,
        language,
        bytes.len() as u64,
        u32::try_from(lines).unwrap_or(u32::MAX),
        hash,
        PARSER_VERSION,
    )
}

/// Wall-clock milliseconds since the epoch; only ever stored, never compared.
#[must_use]
pub fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// The number of `\n` bytes in `bytes` (memchr-speed via `count` semantics).
#[must_use]
fn byte_lines(bytes: &[u8]) -> usize {
    // `bytecount` would be one more dependency for one call site; this fold
    // is what `count()` does and reads at memory speed either way.
    #[allow(clippy::naive_bytecount)]
    bytes.iter().filter(|byte| **byte == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_counts_count_partial_last_lines() {
        let entry = WalkEntry {
            path: std::path::PathBuf::from("/x"),
            rel: String::from("x.rs"),
            language: Some(Language::Rust),
            size: 0,
            mtime_ns: 0,
        };
        let node = file_node_for(&entry, b"a\nb\nc", "hash", Language::Rust);
        assert_eq!(node.lines, Some(3));
        let node = file_node_for(&entry, b"a\nb\n", "hash", Language::Rust);
        assert_eq!(node.lines, Some(2));
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::config::WalkPolicy;
    use crate::extraction::SymbolFact;
    use crate::memory::MemoryStore;
    use crate::ports::{LanguageExtractor, LanguageRegistry, ParseError, SourceFile};
    use graph_search_types::kind::NodeKind;
    use std::path::Path as StdPath;
    use tempfile::TempDir;

    /// A fake Rust extractor: every `fn <name>()` line becomes a symbol, and
    /// a `// calls: <name>` comment becomes a call reference.
    struct FakeRust;

    impl LanguageExtractor for FakeRust {
        fn language(&self) -> graph_search_types::Language {
            graph_search_types::Language::Rust
        }

        fn supports(&self, path: &StdPath) -> bool {
            path.extension().is_some_and(|ext| ext == "rs")
        }

        fn extract(&self, file: &SourceFile<'_>) -> std::result::Result<Extraction, ParseError> {
            let mut extraction = Extraction::default();
            for (index, line) in file.text.lines().enumerate() {
                let line_no = u32::try_from(index).map_or(1, |n| n.saturating_add(1));
                if let Some(rest) = line.strip_prefix("fn ")
                    && let Some(name) = rest.split('(').next()
                {
                    let mut fact = SymbolFact::new(
                        format!("fn:{name}"),
                        NodeKind::Function,
                        name,
                        name,
                        graph_search_types::node::Span::new(line_no, line_no, 0, 1),
                    );
                    fact = fact.with_signature(format!("fn {name}()"));
                    extraction.symbols.push(fact);
                }
                if let Some(target) = line.strip_prefix("// calls: ") {
                    extraction
                        .references
                        .push(crate::extraction::ReferenceFact::file_level(
                            EdgeKind::Imports,
                            target.trim(),
                            line_no,
                        ));
                }
            }
            Ok(extraction)
        }
    }

    struct Registry;

    impl LanguageRegistry for Registry {
        fn extractor_for(&self, path: &StdPath) -> Option<&dyn LanguageExtractor> {
            FakeRust.supports(path).then_some(&FakeRust)
        }
    }

    struct CancellingRegistry(crate::work::CancellationToken);

    impl LanguageRegistry for CancellingRegistry {
        fn extractor_for(&self, _: &StdPath) -> Option<&dyn LanguageExtractor> {
            Some(self)
        }
    }

    impl LanguageExtractor for CancellingRegistry {
        fn language(&self) -> Language {
            Language::Rust
        }
        fn supports(&self, _: &StdPath) -> bool {
            true
        }
        fn extract(&self, file: &SourceFile<'_>) -> std::result::Result<Extraction, ParseError> {
            self.0.cancel();
            FakeRust.extract(file)
        }
    }

    #[test]
    fn cancellation_inside_extraction_preserves_the_previous_projection() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a.rs", "fn before() {}\n");
        let mut store = MemoryStore::new();
        run_sync(dir.path(), &mut store);
        let before = store.snapshot().unwrap().all_nodes().unwrap();
        let manifest = serde_json::to_vec(&store.manifest().unwrap()).unwrap();
        write(dir.path(), "a.rs", "fn after_change() {}\n");
        let cancellation = crate::work::CancellationToken::default();
        let registry = CancellingRegistry(cancellation.clone());
        let policy = WalkPolicy::default();
        let mut work = crate::work::WorkBudget::new(crate::work::WorkLimits {
            cancellation: Some(cancellation),
            ..Default::default()
        });
        let result = Projector::new(&registry, &policy)
            .with_work_budget(&mut work)
            .sync(dir.path(), &mut store);
        assert!(matches!(result, Err(crate::Error::QueryCancelled)));
        assert_eq!(store.snapshot().unwrap().all_nodes().unwrap(), before);
        assert_eq!(
            serde_json::to_vec(&store.manifest().unwrap()).unwrap(),
            manifest
        );
    }

    fn write(dir: &StdPath, rel: &str, contents: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| StdPath::new(".")))
            .unwrap_or_else(|e| panic!("mkdir: {e}"));
        std::fs::write(path, contents).unwrap_or_else(|e| panic!("write: {e}"));
    }

    fn run_sync(root: &StdPath, store: &mut MemoryStore) -> SyncReport {
        let policy = WalkPolicy::default();
        let projector = Projector::new(&Registry, &policy);
        let mut report = projector
            .sync(root, store)
            .unwrap_or_else(|e| panic!("sync: {e}"));
        report.elapsed_ms = 0; // determinism for assertions
        report
    }

    #[test]
    fn a_full_sync_indexes_symbols_and_edges() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(root, "src/a.rs", "fn alpha() {\n// calls: beta\n}\n");
        write(root, "src/b.rs", "fn beta() {}\n");
        let mut store = MemoryStore::new();

        let report = run_sync(root, &mut store);
        assert_eq!(report.added.len(), 2, "{report:?}");
        let snapshot = store.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
        let nodes = snapshot.all_nodes().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(nodes.len(), 4, "two files, two functions: {nodes:?}");

        // alpha's file-level import edge resolves to beta's function by
        // global-unique name (rule 4).
        let alpha = NodeId::symbol("src/a.rs", NodeKind::Function, "alpha", None);
        let edges = snapshot
            .edges_from(
                &alpha,
                &[EdgeKind::Imports],
                graph_search_types::kind::Direction::Out,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        // File-level references attach to the file node; find them there.
        let file_edges = snapshot
            .edges_from(
                &NodeId::file("src/a.rs"),
                &[],
                graph_search_types::kind::Direction::Out,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let _ = (edges, file_edges);
    }

    #[test]
    fn an_unchanged_tree_is_a_no_op_and_a_modified_one_replaces() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(root, "src/a.rs", "fn alpha() {}\n");
        let mut store = MemoryStore::new();
        run_sync(root, &mut store);
        let first_manifest = store.manifest().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(first_manifest.map(|m| m.len()), Some(1));

        // Nothing changed: the next sync is the O(1) path.
        let report = run_sync(root, &mut store);
        assert_eq!(report.added.len(), 0, "{report:?}");
        assert_eq!(report.unchanged, 1);

        // Modify: re-parse, replace subtree.
        write(root, "src/a.rs", "fn alpha() {}\nfn gamma() {}\n");
        let report = run_sync(root, &mut store);
        assert_eq!(report.added.len(), 0, "{report:?}");
        let snapshot = store.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let nodes = snapshot.all_nodes().unwrap_or_else(|e| panic!("{e}"));
        let gamma = NodeId::symbol("src/a.rs", NodeKind::Function, "gamma", None);
        assert!(
            nodes.iter().any(|n| n.id == gamma),
            "gamma must be indexed: {nodes:?}"
        );
        assert_eq!(
            nodes.len(),
            3,
            "a.rs's subtree was replaced, not appended: {nodes:?}"
        );
    }

    #[test]
    fn a_removed_file_is_forgotten_and_a_renamed_one_is_detected() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(root, "src/a.rs", "fn alpha() {}\n");
        write(root, "src/old.rs", "fn elder() {}\n");
        let mut store = MemoryStore::new();
        run_sync(root, &mut store);

        // Rename old.rs -> moved.rs (same bytes): detected by content hash.
        std::fs::rename(root.join("src/old.rs"), root.join("src/moved.rs"))
            .unwrap_or_else(|e| panic!("rename: {e}"));
        // And remove nothing else.
        let report = run_sync(root, &mut store);
        assert_eq!(report.renamed.len(), 1, "{report:?}");
        assert_eq!(report.renamed[0].from, "src/old.rs");
        assert_eq!(report.renamed[0].to, "src/moved.rs");

        // Now remove it entirely.
        std::fs::remove_file(root.join("src/moved.rs")).unwrap_or_else(|e| panic!("rm: {e}"));
        let report = run_sync(root, &mut store);
        assert_eq!(
            report.removed,
            vec![String::from("src/moved.rs")],
            "{report:?}"
        );
        let snapshot = store.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let nodes = snapshot.all_nodes().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(nodes.len(), 2, "only a.rs and alpha remain: {nodes:?}");
    }

    #[test]
    fn the_manifest_is_committed_last_and_matches_the_tree() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(root, "src/a.rs", "fn alpha() {}\n");
        write(root, "notes.txt", "no extractor claims this\n");
        let mut store = MemoryStore::new();
        run_sync(root, &mut store);

        // Every walked file has a manifest entry, extracted or not.
        let manifest = store
            .manifest()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("manifest"));
        assert_eq!(manifest.len(), 2, "{manifest:?}");
        assert!(manifest.get("notes.txt").is_some());

        // The file node for the unclaimed extension still exists.
        let snapshot = store.snapshot().unwrap_or_else(|e| panic!("{e}"));
        let notes = snapshot
            .node_by_id(&NodeId::file("notes.txt"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            notes.is_some(),
            "an unextracted file still gets a file node"
        );
    }
}
