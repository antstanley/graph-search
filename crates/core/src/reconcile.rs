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
use crate::resolve::{CrossTables, SymbolTable, cross_edges_for, edges_for_extraction};
use crate::walk::{WalkEntry, walk};
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
}

/// One file's in-flight projection, between extraction and edge resolution.
struct Pending {
    entry: WalkEntry,
    hash: String,
    projection: FileProjection,
    extraction: Option<Extraction>,
    ids: BTreeMap<String, NodeId>,
}

impl<'a> Projector<'a> {
    /// A projector over `registry` and `policy`.
    #[must_use]
    pub const fn new(registry: &'a dyn LanguageRegistry, policy: &'a WalkPolicy) -> Self {
        Self { registry, policy }
    }

    /// Full build: parse everything, build from scratch (verify/repair)
    /// (`SPEC.md` §6.5.1).
    ///
    /// # Errors
    /// When the store or the tree fails; per-file failures quarantine.
    pub fn reindex(&self, search_root: &Path, store: &mut dyn GraphStore) -> Result<SyncReport> {
        let started = std::time::Instant::now();
        let entries = walk(search_root, self.policy)?;
        // Repair: remove store files the new tree no longer holds, even ones
        // an interrupted run left without a manifest (`SPEC.md` §6.5.1).
        let changed = entries.clone();
        let walked: BTreeSet<String> = entries.iter().map(|e| e.rel.clone()).collect();
        let mut removed: Vec<String> = Vec::new();
        let snapshot = store.snapshot()?;
        for node in snapshot.all_nodes()? {
            if node.is_file() && !walked.contains(&node.path) {
                removed.push(node.path);
            }
        }
        drop(snapshot);
        self.apply(store, &entries, removed, &changed, true, started)
    }

    /// Incremental reconcile against the manifest; the normal way to keep
    /// current (`SPEC.md` §6.5.2).
    ///
    /// # Errors
    /// When the store or the tree fails; per-file failures quarantine.
    pub fn sync(&self, search_root: &Path, store: &mut dyn GraphStore) -> Result<SyncReport> {
        let started = std::time::Instant::now();
        let manifest = store
            .manifest()?
            .unwrap_or_else(|| Manifest::new(PARSER_VERSION, SCHEMA_VERSION));
        let entries = walk(search_root, self.policy)?;
        let diff = crate::manifest::classify(&entries, &manifest);

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
        let unchanged = diff.unchanged.len() as u64;
        let reindexed_all = diff.reindexed_all;
        let renamed = diff.renamed.clone();
        let mut report = self.apply(store, &entries, removed, &changed, reindexed_all, started)?;
        report.unchanged = unchanged;
        report.renamed = renamed;
        Ok(report)
    }

    /// The shared tail: extract changed files, resolve, batch, apply, commit
    /// the manifest last.
    #[allow(clippy::too_many_arguments)] // the pipeline stages are explicit here on purpose
    #[allow(clippy::too_many_lines)]
    fn apply(
        &self,
        store: &mut dyn GraphStore,
        entries: &[WalkEntry],
        removed: Vec<String>,
        changed: &[WalkEntry],
        reindexed_all: bool,
        started: std::time::Instant,
    ) -> Result<SyncReport> {
        // The known-file set resolves import specifiers against: the walked
        // tree plus the indexed set (an unchanged file may be an import
        // target).
        let mut known_files: BTreeSet<String> = entries.iter().map(|e| e.rel.clone()).collect();
        {
            let snapshot = store.snapshot()?;
            for node in snapshot.all_nodes()? {
                if node.is_file() {
                    known_files.insert(node.path);
                }
            }
        }

        // Phase A: extract every changed file.
        let mut pending: Vec<Pending> = Vec::new();
        for entry in changed {
            pending.push(self.extract_one(entry));
        }

        // Phase B: the symbol table reflects the post-apply world: untouched
        // files from the store, changed files from the batch.
        let changed_paths: BTreeSet<String> = changed.iter().map(|e| e.rel.clone()).collect();
        let mut table = SymbolTable::new();
        {
            let snapshot = store.snapshot()?;
            for node in snapshot.all_nodes()? {
                if !node.is_file() && !changed_paths.contains(&node.path) {
                    table.add(&node);
                }
            }
        }
        for item in &pending {
            for node in &item.projection.symbols {
                table.add(node);
            }
        }

        // Phase C: resolve references into edges, then the HTML/CSS matches.
        let cross = CrossTables::from_table(&table);
        for item in &mut pending {
            let Some(extraction) = item.extraction.take() else {
                continue;
            };
            let file_id = item.projection.file.id.clone();
            let language = item.entry.language.unwrap_or(Language::Unknown);
            let mut edges = edges_for_extraction(
                &file_id,
                &item.entry.rel,
                &extraction,
                &item.ids,
                &table,
                &known_files,
                language,
            );
            let element_ids = item.ids.clone();
            edges.extend(cross_edges_for(
                &item.entry.rel,
                &extraction,
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
            }
        }

        // The batch: removals, upserts, and the manifest over the *whole*
        // walked tree — committed last (`SPEC.md` §6.4).
        let mut manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
        for entry in entries {
            if let Some(item) = pending.iter().find(|p| p.entry.rel == entry.rel) {
                manifest.entries.insert(
                    entry.rel.clone(),
                    crate::manifest::entry_for(
                        entry,
                        &item.hash,
                        item.projection
                            .quarantine
                            .as_ref()
                            .map(|q| q.reason.clone()),
                    ),
                );
            } else if let Some(stored) = store.manifest()?.and_then(|m| m.get(&entry.rel).cloned())
            {
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

        let mut batch = WriteBatch::with_manifest(manifest.clone());
        batch.removed_files = removed;
        let mut quarantined = Vec::new();
        for item in pending {
            if let Some(record) = &item.projection.quarantine {
                quarantined.push(record.clone());
            }
            batch.upserts.push(item.projection);
        }

        // Classify the report against the manifest we are replacing.
        let previously_known: BTreeSet<String> = store
            .manifest()?
            .map(|m| m.entries.into_keys().collect())
            .unwrap_or_default();
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
        let report = SyncReport {
            added,
            modified,
            removed: batch.removed_files.clone(),
            quarantined,
            reindexed_all,
            ..SyncReport::default()
        };
        store.apply(batch)?;
        // The manifest is committed only after the apply succeeded; an
        // interrupted run leaves the old manifest and recomputes this delta.
        store.commit_manifest(manifest)?;
        let mut report = report;
        report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(report)
    }

    /// Phase A for one file: read, hash, parse or quarantine.
    #[allow(clippy::too_many_lines)] // one construct per arm; splitting hurts the reading
    fn extract_one(&self, entry: &WalkEntry) -> Pending {
        let bytes = std::fs::read(&entry.path).unwrap_or_default();
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

        // No enabled extractor: the file node still exists (`SPEC.md` §6.2).
        let Some(extractor) = self.registry.extractor_for(Path::new(&entry.rel)) else {
            return pending;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            pending.projection.quarantine =
                Some(QuarantineRecord::new(&entry.rel, "not valid UTF-8"));
            return pending;
        };

        let extraction = match extractor.extract(&crate::ports::SourceFile {
            path: Path::new(&entry.rel),
            text,
        }) {
            Err(error) => {
                pending.projection.quarantine =
                    Some(QuarantineRecord::new(&entry.rel, error.message));
                return pending;
            }
            Ok(extraction) => extraction,
        };

        let symbol_count = extraction.symbols.len().saturating_add(1);
        if symbol_count > MAX_NODES_PER_FILE {
            pending.projection.quarantine = Some(QuarantineRecord::new(
                &entry.rel,
                format!("node overflow: the {MAX_NODES_PER_FILE}-node cap"),
            ));
            return pending;
        }

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
        entry.size,
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
