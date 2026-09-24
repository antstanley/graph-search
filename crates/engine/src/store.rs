//! The native [`GraphStore`] (storage format 10,
//! `research/16-proportional-sync.md`).
//!
//! Every graph fact a file owns lives in its **shard** ([`crate::shards`]),
//! stored in content-addressed packs that every generation shares. Global
//! lookups (which file owns a node, which files point at it) are **posting
//! tables** ([`crate::segment`]): a base segment plus small per-publish deltas.
//! A publish therefore writes the shards, source records and posting rows of the
//! files it changes and carries everything else forward by reference.
//!
//! Opening a published generation reads only its pointer, manifest header and
//! summary. Shards, records and segment blocks are verified against their
//! committed hashes when first read, and bulk views (every node, every source
//! file, the retrieval indexes) are built only when a query asks for them.

use crate::segment::{Row, Table, TableRef, Tables};
use crate::shards::{EDGE_COUNTS, FOREIGN, INCOMING, NODES, Shard};
use crate::source_records::{Index, SOURCE_LAYOUT};
use crate::{generation, sidecar};
use graph_search_core::Result;
use graph_search_core::dependencies::{DependencyLookup, DependencyRecord, Flag};
use graph_search_core::error::Error;
use graph_search_core::ports::{GraphSnapshot, GraphStore};
use graph_search_core::symbols::{Structure, SymbolIndex, SymbolRow};
use graph_search_types::js_module::JsModule;
use graph_search_types::kind::{Direction, NodeKind};
use graph_search_types::manifest::Manifest;
use graph_search_types::node::Node;
use graph_search_types::occurrence::{OccurrenceFile, ResolutionClass};
use graph_search_types::source::SourceFileUnits;
use graph_search_types::{ApplyOutcome, Edge, NodeId, Scored, Subgraph, WriteBatch};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

/// Maps each package manifest path to the files whose source facts name it.
const PACKAGE_MEMBERS: &str = "package_members";
/// Each file's TypeScript configuration facts, keyed by its path.
const TYPESCRIPT_CONFIGS: &str = "typescript_configs";
/// Each package manifest's facts, keyed by its path.
const PACKAGE_MANIFESTS: &str = "package_manifests";
/// Tables owned by source facts; their rows are replaced with a file's source.
const SOURCE_TABLES: [&str; 3] = [PACKAGE_MEMBERS, TYPESCRIPT_CONFIGS, PACKAGE_MANIFESTS];
/// Dependency postings, owned by the file whose record produced them.
const DEP_CONSUMERS: &str = "dep_consumers";
const DEP_SELECTED: &str = "dep_selected";
const DEP_CANDIDATES: &str = "dep_candidates";
const DEP_INCOMING: &str = "dep_incoming";
const DEP_FLAGS: &str = "dep_flags";
const JS_MODULES: &str = "js_modules";
/// Tables owned by dependency records: their rows are replaced with a file's record.
const DEPENDENCY_TABLES: [&str; 6] = [
    DEP_CONSUMERS,
    DEP_SELECTED,
    DEP_CANDIDATES,
    DEP_INCOMING,
    DEP_FLAGS,
    JS_MODULES,
];
/// Every file's dependency record, read one file at a time.
const DEPENDENCY_LAYOUT: crate::source_records::Layout = crate::source_records::Layout {
    index: generation::DEPENDENCY_RECORDS,
    directory: "dependency-records",
    pack_bytes: 256 * 1024,
};

/// How the store is opened.
#[derive(Clone, Debug, Default)]
pub struct StoreOptions {
    /// Keep the store in a private temporary directory, removed when the store
    /// is dropped (tests and throwaway indexes).
    pub in_memory: bool,
    /// The caller only intends to read. Before any generation is published
    /// there is nothing to attach to, so a read-only open exposes an empty
    /// store and never creates anything on disk (`SPEC.md` §6.6).
    pub read_only: bool,
}

/// The value of `cell`, loading it on first use. A failed load is not cached:
/// the error reaches this caller, and a later access tries again.
fn loaded<T>(cell: &OnceLock<T>, load: impl FnOnce() -> Result<T>) -> Result<&T> {
    if let Some(value) = cell.get() {
        return Ok(value);
    }
    let value = load()?;
    Ok(cell.get_or_init(|| value))
}

#[allow(clippy::needless_pass_by_value)] // map_err consumes its error
fn store_io(error: std::io::Error) -> Error {
    Error::Store(error.to_string())
}

fn poisoned() -> Error {
    Error::Store(String::from("store cache lock poisoned"))
}

/// The root directory of an `in_memory` store, removed with the store.
struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temporary_root() -> Result<Temporary> {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "graph-search-store-{:x}-{nanos:x}-{serial:x}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(store_io)?;
    Ok(Temporary(dir))
}

/// The native store.
pub struct NativeStore {
    store_dir: PathBuf,
    data_dir: PathBuf,
    unavailable: bool,
    read_only: bool,
    /// The published generation lazily read from; `None` before the first publish.
    committed: Option<generation::Committed>,
    summary: generation::Summary,
    manifest_header: Option<Manifest>,
    shard_index: OnceLock<Option<Index>>,
    source_index: OnceLock<Option<Index>>,
    table_refs: OnceLock<Tables>,
    tables: OnceLock<BTreeMap<String, Table>>,
    /// Shards read so far; immutable, so they stay valid across publications
    /// of other files.
    shards: RwLock<HashMap<String, Arc<Shard>>>,
    sources: OnceLock<BTreeMap<String, SourceFileUnits>>,
    occurrence_files: OnceLock<BTreeMap<String, OccurrenceFile>>,
    extraction_records: OnceLock<Option<crate::manifest_records::Verified>>,
    dependency_index: OnceLock<Option<Index>>,
    /// Dependency records read so far.
    records: RwLock<HashMap<String, Arc<DependencyRecord>>>,
    /// Every node, sorted by id: the metadata index and bulk reads.
    nodes: OnceLock<Vec<Node>>,
    metadata: OnceLock<graph_search_core::metadata::MetadataIndex>,
    body: OnceLock<graph_search_core::body::BodyIndex>,
    occurrences: OnceLock<graph_search_core::occurrences::OccurrenceIndex>,
    adjacency: OnceLock<graph_search_core::adjacency::AdjacencyIndex>,
    /// Keeps this generation's paths from being reclaimed.
    generation_lease: Option<std::fs::File>,
    #[cfg(test)]
    failure: Option<&'static str>,
    #[cfg(test)]
    crash: bool,
    /// Dropped last: an `in_memory` store's directory outlives every handle.
    temporary: Option<Temporary>,
}

impl NativeStore {
    /// Opens the store at `store_dir`.
    ///
    /// # Errors
    /// When the published generation cannot be selected.
    pub fn open(store_dir: &Path, options: &StoreOptions) -> Result<Self> {
        if options.in_memory {
            let temporary = temporary_root()?;
            let mut store = Self::empty(&temporary.0, false);
            store.temporary = Some(temporary);
            return Ok(store);
        }
        match generation::current(store_dir).map_err(store_io)? {
            Some(selected) => Ok(Self::open_generation(store_dir, selected)),
            None => Ok(Self::empty(store_dir, options.read_only)),
        }
    }

    fn empty(store_dir: &Path, read_only: bool) -> Self {
        Self {
            store_dir: store_dir.to_path_buf(),
            data_dir: PathBuf::new(),
            unavailable: false,
            read_only,
            committed: None,
            summary: generation::Summary::default(),
            manifest_header: None,
            shard_index: OnceLock::new(),
            source_index: OnceLock::new(),
            table_refs: OnceLock::new(),
            tables: OnceLock::new(),
            shards: RwLock::new(HashMap::new()),
            sources: OnceLock::new(),
            occurrence_files: OnceLock::new(),
            extraction_records: OnceLock::new(),
            dependency_index: OnceLock::new(),
            records: RwLock::new(HashMap::new()),
            nodes: OnceLock::new(),
            metadata: OnceLock::new(),
            body: OnceLock::new(),
            occurrences: OnceLock::new(),
            adjacency: OnceLock::new(),
            generation_lease: None,
            #[cfg(test)]
            failure: None,
            #[cfg(test)]
            crash: false,
            temporary: None,
        }
    }

    /// A published generation: only the header and summary are read here.
    fn open_generation(store_dir: &Path, selected: generation::Selected) -> Self {
        let mut store = Self::empty(store_dir, false);
        store.data_dir = selected.committed.dir().to_path_buf();
        store.manifest_header = selected.manifest;
        store.summary = selected.summary.unwrap_or_default();
        store.committed = Some(selected.committed);
        store.generation_lease = Some(selected.lease);
        store
    }

    /// The store directory (for the lock file and `status`).
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.store_dir
    }

    /// The store-level directory of packs and segments every generation shares.
    fn objects(&self) -> PathBuf {
        self.store_dir.join(generation::OBJECTS)
    }

    fn ensure_available(&self) -> Result<()> {
        if self.unavailable {
            return Err(Error::Store(String::from(
                "generation publication durability is uncertain; close and reopen the index",
            )));
        }
        Ok(())
    }

    #[allow(clippy::unused_self, clippy::unnecessary_wraps)] // per-store test fault injection
    fn inject(&self, point: &str) -> Result<()> {
        #[cfg(test)]
        if self.failure == Some(point) {
            if self.crash {
                std::process::exit(86);
            }
            return Err(Error::Store(format!(
                "injected preparation failure: {point}"
            )));
        }
        let _ = point;
        Ok(())
    }

    /// The committed bytes of `name`, or `None` before the first publish or
    /// when the generation does not commit it.
    fn artifact(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match &self.committed {
            Some(committed) => committed.read(name).map_err(store_io),
            None => Ok(None),
        }
    }

    fn shard_index(&self) -> Result<Option<&Index>> {
        loaded(&self.shard_index, || {
            self.artifact(crate::shards::FILE)?
                .map(|bytes| Index::decode_packed(&bytes).map_err(store_io))
                .transpose()
        })
        .map(Option::as_ref)
    }

    fn source_index(&self) -> Result<Option<&Index>> {
        loaded(&self.source_index, || {
            self.artifact(sidecar::SOURCE_FILE)?
                .map(|bytes| Index::decode_packed(&bytes).map_err(store_io))
                .transpose()
        })
        .map(Option::as_ref)
    }

    fn dependency_records(&self) -> Result<Option<&Index>> {
        loaded(&self.dependency_index, || {
            self.artifact(generation::DEPENDENCY_RECORDS)?
                .map(|bytes| Index::decode_packed(&bytes).map_err(store_io))
                .transpose()
        })
        .map(Option::as_ref)
    }

    /// One file's dependency record, verified on first read.
    fn dependency_record(&self, path: &str) -> Result<Option<Arc<DependencyRecord>>> {
        if let Some(record) = self.records.read().map_err(|_| poisoned())?.get(path) {
            return Ok(Some(Arc::clone(record)));
        }
        let Some(index) = self.dependency_records()? else {
            return Ok(None);
        };
        let Some(record) = index
            .load_selected_verified::<DependencyRecord>(
                &self.objects(),
                DEPENDENCY_LAYOUT,
                &BTreeSet::from([path.to_owned()]),
            )
            .map_err(store_io)?
            .remove(path)
        else {
            return Ok(None);
        };
        let record = Arc::new(record);
        self.records
            .write()
            .map_err(|_| poisoned())?
            .insert(path.to_owned(), Arc::clone(&record));
        Ok(Some(record))
    }

    fn table_refs(&self) -> Result<&Tables> {
        loaded(&self.table_refs, || {
            self.artifact(generation::TABLES)?.map_or_else(
                || Ok(Tables::default()),
                |bytes| {
                    serde_json::from_slice(&bytes).map_err(|error| Error::Store(error.to_string()))
                },
            )
        })
    }

    fn tables(&self) -> Result<&BTreeMap<String, Table>> {
        loaded(&self.tables, || {
            self.table_refs()?
                .tables
                .iter()
                .map(|(name, reference)| {
                    Table::open(&self.objects(), reference)
                        .map(|table| (name.clone(), table))
                        .map_err(store_io)
                })
                .collect()
        })
    }

    /// Every visible row of `key` in `table`.
    fn postings(&self, table: &str, key: &str) -> Result<Vec<Row>> {
        match self.tables()?.get(table) {
            Some(table) => table.get(key).map_err(store_io),
            None => Ok(Vec::new()),
        }
    }

    /// The file that owns node `id`.
    fn owner_of(&self, id: &NodeId) -> Result<Option<String>> {
        Ok(self
            .postings(NODES, id.as_str())?
            .into_iter()
            .next()
            .map(|row| row.owner))
    }

    /// Every file path with a shard, sorted.
    fn paths(&self) -> Result<Vec<String>> {
        Ok(self
            .shard_index()?
            .map(|index| index.paths().map(str::to_owned).collect())
            .unwrap_or_default())
    }

    /// The shards of `paths` that exist, verified and validated on first read.
    fn shards_for(&self, paths: &BTreeSet<String>) -> Result<BTreeMap<String, Arc<Shard>>> {
        let mut out = BTreeMap::new();
        let mut missing = BTreeSet::new();
        {
            let cache = self.shards.read().map_err(|_| poisoned())?;
            for path in paths {
                match cache.get(path) {
                    Some(shard) => {
                        out.insert(path.clone(), Arc::clone(shard));
                    }
                    None => {
                        missing.insert(path.clone());
                    }
                }
            }
        }
        if missing.is_empty() {
            return Ok(out);
        }
        let Some(index) = self.shard_index()? else {
            return Ok(out);
        };
        let loaded: BTreeMap<String, Shard> = index
            .load_selected_verified(&self.objects(), crate::shards::LAYOUT, &missing)
            .map_err(store_io)?;
        let mut cache = self.shards.write().map_err(|_| poisoned())?;
        for (path, shard) in loaded {
            validate_shard(&path, &shard)?;
            let shard = Arc::new(shard);
            cache.insert(path.clone(), Arc::clone(&shard));
            out.insert(path, shard);
        }
        Ok(out)
    }

    fn shard(&self, path: &str) -> Result<Option<Arc<Shard>>> {
        Ok(self
            .shards_for(&BTreeSet::from([path.to_owned()]))?
            .into_values()
            .next())
    }

    fn all_shards(&self) -> Result<BTreeMap<String, Arc<Shard>>> {
        self.shards_for(&self.paths()?.into_iter().collect())
    }

    fn node(&self, id: &NodeId) -> Result<Option<Node>> {
        let Some(owner) = self.owner_of(id)? else {
            return Ok(None);
        };
        Ok(self
            .shard(&owner)?
            .and_then(|shard| shard.node(id).cloned()))
    }

    fn nodes(&self) -> Result<&Vec<Node>> {
        loaded(&self.nodes, || {
            let mut nodes: Vec<Node> = self
                .all_shards()?
                .values()
                .flat_map(|shard| shard.nodes.iter().cloned())
                .collect();
            nodes.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(nodes)
        })
    }

    fn find_node<'n>(nodes: &'n [Node], id: &NodeId) -> Option<&'n Node> {
        nodes
            .binary_search_by(|node| node.id.cmp(id))
            .ok()
            .and_then(|index| nodes.get(index))
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let mut edges: Vec<Edge> = self
            .all_shards()?
            .values()
            .flat_map(|shard| shard.edges.iter().cloned())
            .collect();
        edges.sort_by(crate::shards::edge_order);
        edges.dedup_by(|a, b| a.id == b.id);
        Ok(edges)
    }

    fn sources(&self) -> Result<&BTreeMap<String, SourceFileUnits>> {
        loaded(&self.sources, || {
            let Some(index) = self.source_index()? else {
                return Ok(BTreeMap::new());
            };
            let files: BTreeMap<String, SourceFileUnits> = index
                .load(&self.objects(), SOURCE_LAYOUT)
                .map_err(store_io)?;
            if !files.is_empty() {
                let nodes = self.nodes()?;
                for (path, source) in &files {
                    let file = Self::find_node(nodes, &NodeId::file(path)).ok_or_else(|| {
                        Error::Store(format!("source facts without file owner: {path}"))
                    })?;
                    graph_search_core::units::validate(file, source, |id| {
                        Self::find_node(nodes, id)
                    })?;
                }
            }
            Ok(files)
        })
    }

    /// One file's source facts, validated against its file's nodes and, for
    /// package facts, its manifest's file node.
    fn source(&self, path: &str) -> Result<Option<SourceFileUnits>> {
        if let Some(all) = self.sources.get() {
            return Ok(all.get(path).cloned());
        }
        let Some(index) = self.source_index()? else {
            return Ok(None);
        };
        let wanted = BTreeSet::from([path.to_owned()]);
        let Some(source) = index
            .load_selected_verified::<SourceFileUnits>(&self.objects(), SOURCE_LAYOUT, &wanted)
            .map_err(store_io)?
            .remove(path)
        else {
            return Ok(None);
        };
        let shard = self
            .shard(path)?
            .ok_or_else(|| Error::Store(format!("source facts without file owner: {path}")))?;
        let file = shard
            .file()
            .ok_or_else(|| Error::Store(format!("source facts without file owner: {path}")))?;
        let manifest = match source.package.as_ref() {
            Some(package) => self.node(&NodeId::file(&package.manifest_path))?,
            None => None,
        };
        graph_search_core::units::validate(file, &source, |id| {
            shard
                .node(id)
                .or_else(|| manifest.as_ref().filter(|node| &node.id == id))
        })?;
        Ok(Some(source))
    }

    /// Every row of a source-owned facts table, decoded.
    fn source_facts(&self, table: &str) -> Result<BTreeMap<String, SourceFileUnits>> {
        let Some(table) = self.tables()?.get(table) else {
            return Ok(BTreeMap::new());
        };
        table
            .rows()
            .map_err(store_io)?
            .into_iter()
            .map(|row| {
                serde_json::from_str(&row.value)
                    .map(|facts| (row.key, facts))
                    .map_err(|error| Error::Store(error.to_string()))
            })
            .collect()
    }

    fn occurrence_files(&self) -> Result<&BTreeMap<String, OccurrenceFile>> {
        loaded(&self.occurrence_files, || {
            Ok(self
                .all_shards()?
                .into_iter()
                .filter_map(|(path, shard)| shard.occurrences.clone().map(|facts| (path, facts)))
                .collect())
        })
    }

    fn occurrence_count(&self, edge_id: &str) -> Result<Option<usize>> {
        if let Some(index) = self.occurrences.get() {
            return Ok(index.count_for_edge(edge_id));
        }
        let mut total = 0usize;
        for row in self.postings(EDGE_COUNTS, edge_id)? {
            let count: usize = row
                .value
                .parse()
                .map_err(|_| Error::Store(String::from("invalid edge occurrence count")))?;
            total = total.saturating_add(count);
        }
        Ok((total > 0).then_some(total))
    }

    fn extraction_records(&self) -> Result<Option<&crate::manifest_records::Verified>> {
        loaded(&self.extraction_records, || {
            match (
                self.artifact(crate::manifest_records::FILE)?,
                self.manifest_header.as_ref(),
            ) {
                (Some(bytes), Some(header)) => Ok(Some(
                    crate::manifest_records::prepare_verified(&bytes, header, &self.data_dir)
                        .map_err(store_io)?,
                )),
                _ => Ok(None),
            }
        })
        .map(Option::as_ref)
    }

    fn metadata(&self) -> Result<&graph_search_core::metadata::MetadataIndex> {
        loaded(&self.metadata, || {
            Ok(graph_search_core::metadata::MetadataIndex::new(
                self.nodes()?.clone(),
            ))
        })
    }

    fn body(&self) -> Result<&graph_search_core::body::BodyIndex> {
        loaded(&self.body, || {
            Ok(graph_search_core::body::BodyIndex::new(self.sources()?))
        })
    }

    fn occurrences(&self) -> Result<&graph_search_core::occurrences::OccurrenceIndex> {
        loaded(&self.occurrences, || {
            Ok(graph_search_core::occurrences::OccurrenceIndex::new(
                self.occurrence_files()?,
            ))
        })
    }

    fn adjacency(&self) -> Result<&graph_search_core::adjacency::AdjacencyIndex> {
        loaded(&self.adjacency, || {
            Ok(graph_search_core::adjacency::AdjacencyIndex::new(
                self.all_edges()?,
            ))
        })
    }
}

/// A shard must be owned by its path and hold exactly one file node, its own.
fn validate_shard(path: &str, shard: &Shard) -> Result<()> {
    let file_id = NodeId::file(path);
    let files: Vec<_> = shard.nodes.iter().filter(|node| node.is_file()).collect();
    if shard.nodes.iter().any(|node| node.path != path)
        || files.len() != 1
        || files.first().is_none_or(|file| file.id != file_id)
    {
        return Err(Error::Store(format!("shard does not own its file: {path}")));
    }
    if let (Some(facts), Some(file)) = (&shard.occurrences, shard.file()) {
        graph_search_core::occurrences::validate(file, facts, |id| shard.node(id))?;
    }
    Ok(())
}

/// Clears an occurrence's binding to a node that no longer exists, exactly as
/// replacing its target's file always has.
fn clear_target(record: &mut graph_search_types::occurrence::ReferenceOccurrence) {
    record.target = None;
    record.target_name.clone_from(&record.name);
    record.resolution = ResolutionClass::Unresolved;
    record.reason = Some("target_removed".into());
}

/// Everything one publish writes, computed and validated before any byte of
/// the new generation is written.
struct Plan {
    /// New shards: the batch's files and the untouched files it rewrote.
    shards: BTreeMap<String, Shard>,
    /// Files whose shards are deleted.
    removed: BTreeSet<String>,
    /// Source records the batch writes, and the files whose records it replaces.
    sources: BTreeMap<String, SourceFileUnits>,
    source_owners: BTreeSet<String>,
    /// New posting rows, per table, and the owners the shard tables replace.
    rows: BTreeMap<&'static str, Vec<Row>>,
    owners: BTreeSet<String>,
    /// New dependency records (all empty when they are not coherent) and the
    /// owners whose records and dependency postings they replace.
    records: BTreeMap<String, DependencyRecord>,
    record_owners: BTreeSet<String>,
    /// New rows of the tables owned by source facts.
    source_rows: BTreeMap<&'static str, Vec<Row>>,
    summary: generation::Summary,
    outcome: ApplyOutcome,
}

impl NativeStore {
    /// Computes and validates one publication (`SPEC.md` §6.4): the batch's
    /// files replace their shards; nodes that do not survive are removed, with
    /// every edge into or out of them and every binding to them, wherever those
    /// live; the summary, posting rows and dependency index follow by delta.
    #[allow(clippy::too_many_lines)] // one pass per replacement rule, in order
    fn plan(
        &self,
        batch: &WriteBatch,
        manifest: Option<&Manifest>,
        retained: &BTreeSet<String>,
    ) -> Result<Plan> {
        let explicit: BTreeSet<&str> = batch.removed_files.iter().map(String::as_str).collect();
        let upserted: BTreeMap<&str, &graph_search_types::FileProjection> = batch
            .upserts
            .iter()
            .map(|file| (file.file.path.as_str(), file))
            .collect();
        let touched: BTreeSet<String> = explicit
            .iter()
            .chain(upserted.keys())
            .map(|path| (*path).to_owned())
            .collect();
        let old = self.shards_for(&touched)?;

        // A batch file owns exactly its own nodes, one of them its file node.
        let mut batch_nodes: HashMap<&NodeId, &Node> = HashMap::new();
        for (path, file) in &upserted {
            if file.file.id != NodeId::file(path) || !file.file.is_file() {
                return Err(Error::Store(format!("invalid file node for {path}")));
            }
            for node in &file.symbols {
                if node.path != *path || node.is_file() {
                    return Err(Error::Store(format!(
                        "symbol {} is not owned by {path}",
                        node.id
                    )));
                }
            }
            for node in std::iter::once(&file.file).chain(&file.symbols) {
                batch_nodes.insert(&node.id, node);
            }
        }

        // Identities survive only where id, path and kind agree without removal.
        let mut removed_ids: BTreeSet<NodeId> = BTreeSet::new();
        for (path, shard) in &old {
            for node in &shard.nodes {
                let survives = !explicit.contains(path.as_str())
                    && batch_nodes
                        .get(&node.id)
                        .is_some_and(|new| new.path == node.path && new.kind == node.kind);
                if !survives {
                    removed_ids.insert(node.id.clone());
                }
            }
        }

        // Owners of nodes outside the batch, memoized.
        let mut owners: HashMap<NodeId, Option<String>> = HashMap::new();
        let mut owner = |store: &Self, id: &NodeId| -> Result<Option<String>> {
            if let Some(node) = batch_nodes.get(id) {
                return Ok(Some(node.path.clone()));
            }
            if let Some(known) = owners.get(id) {
                return Ok(known.clone());
            }
            let found = store.owner_of(id)?;
            owners.insert(id.clone(), found.clone());
            Ok(found)
        };
        let exists = |owner: &Option<String>, id: &NodeId| {
            batch_nodes.contains_key(id)
                || owner
                    .as_ref()
                    .is_some_and(|path| !touched.contains(path) && !removed_ids.contains(id))
        };

        // Source facts must belong to their file; package facts are checked
        // against the post-batch manifest nodes.
        let mut manifest_nodes: HashMap<NodeId, Node> = HashMap::new();
        for file in upserted.values() {
            if let Some(package) = file
                .source
                .as_ref()
                .and_then(|source| source.package.as_ref())
            {
                let id = NodeId::file(&package.manifest_path);
                if !batch_nodes.contains_key(&id)
                    && !touched.contains(&package.manifest_path)
                    && let Some(node) = self.node(&id)?
                {
                    manifest_nodes.insert(id, node);
                }
            }
        }
        for file in upserted.values() {
            let lookup = |id: &NodeId| {
                batch_nodes
                    .get(id)
                    .copied()
                    .or_else(|| manifest_nodes.get(id))
            };
            if let Some(source) = &file.source {
                graph_search_core::units::validate(&file.file, source, lookup)?;
            }
            if let Some(facts) = &file.occurrences {
                let local: BTreeMap<&NodeId, &Node> = std::iter::once(&file.file)
                    .chain(&file.symbols)
                    .map(|node| (&node.id, node))
                    .collect();
                graph_search_core::occurrences::validate(&file.file, facts, |id| {
                    local.get(id).copied()
                })?;
            }
        }
        // A retained source whose package manifest the batch replaces or
        // removes must still match it.
        let manifests: BTreeSet<&String> = touched
            .iter()
            .filter(|path| graph_search_core::units::is_package_manifest(path))
            .collect();
        let mut members = BTreeSet::new();
        for manifest_path in &manifests {
            for row in self.postings(PACKAGE_MEMBERS, manifest_path)? {
                if !touched.contains(&row.owner) {
                    members.insert(row.owner);
                }
            }
        }
        if !members.is_empty() {
            let sources: BTreeMap<String, SourceFileUnits> = match self.source_index()? {
                Some(index) => index
                    .load_selected_verified(&self.objects(), SOURCE_LAYOUT, &members)
                    .map_err(store_io)?,
                None => BTreeMap::new(),
            };
            for (path, source) in &sources {
                let file = self.node(&NodeId::file(path))?.ok_or_else(|| {
                    Error::Store(format!("source facts without file owner: {path}"))
                })?;
                graph_search_core::units::validate_package(&file, source, |id| {
                    batch_nodes.get(id).copied()
                })?;
            }
        }

        // New shards for the batch's files.
        let mut shards: BTreeMap<String, Shard> = BTreeMap::new();
        for (path, file) in &upserted {
            let mut occurrences = file.occurrences.clone();
            if let Some(facts) = occurrences.as_mut() {
                for record in &mut facts.records {
                    if let Some(target) = record.target.clone() {
                        let target_owner = owner(self, &target)?;
                        if !exists(&target_owner, &target) {
                            clear_target(record);
                        }
                    }
                }
            }
            shards.insert(
                (*path).to_owned(),
                Shard {
                    nodes: std::iter::once(&file.file)
                        .chain(&file.symbols)
                        .cloned()
                        .collect(),
                    edges: Vec::new(),
                    occurrences,
                },
            );
        }

        // Every batch edge goes to the file that owns it: its reference path,
        // else its source node's file. An edge whose endpoints do not both
        // exist is kept as a dangling reference, named and counted.
        let mut contributed: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
        let mut edges_upserted = 0u64;
        for file in &batch.upserts {
            for edge in &file.edges {
                let from_owner = owner(self, &edge.from)?;
                let to_owner = match &edge.to {
                    Some(to) => owner(self, to)?,
                    None => None,
                };
                let resolved = edge.resolved
                    && exists(&from_owner, &edge.from)
                    && edge.to.as_ref().is_some_and(|to| exists(&to_owner, to));
                let stored = match (&edge.to, resolved) {
                    (Some(to), true) => {
                        edges_upserted = edges_upserted.saturating_add(1);
                        Edge::resolved(
                            &edge.from,
                            edge.kind,
                            to,
                            &edge.to_name,
                            edge.path.as_deref(),
                            edge.line,
                        )
                    }
                    _ => Edge::dangling(
                        &edge.from,
                        edge.kind,
                        &edge.to_name,
                        edge.path.as_deref(),
                        edge.line,
                    ),
                };
                let owning = edge.path.clone().or(from_owner).filter(|path| {
                    upserted.contains_key(path.as_str())
                        || (!explicit.contains(path.as_str()) && !touched.contains(path))
                });
                match owning {
                    Some(path) if shards.contains_key(&path) => {
                        if let Some(shard) = shards.get_mut(&path) {
                            shard.edges.push(stored);
                        }
                    }
                    Some(path) if self.shard(&path)?.is_some() => {
                        contributed.entry(path).or_default().push(stored);
                    }
                    _ => {
                        if let Some(shard) = shards.get_mut(&file.file.path) {
                            shard.edges.push(stored);
                        }
                    }
                }
            }
        }

        // Untouched files that point at a removed node, or that the batch
        // added edges to, are rewritten.
        let mut affected: BTreeSet<String> = contributed.keys().cloned().collect();
        for id in &removed_ids {
            for table in [INCOMING, FOREIGN] {
                for row in self.postings(table, id.as_str())? {
                    if !touched.contains(&row.owner) {
                        affected.insert(row.owner);
                    }
                }
            }
        }
        let rewritten = self.shards_for(&affected)?;
        for (path, shard) in &rewritten {
            let mut shard = Shard::clone(shard);
            shard.edges.retain(|edge| {
                !removed_ids.contains(&edge.from)
                    && edge.to.as_ref().is_none_or(|to| !removed_ids.contains(to))
            });
            if let Some(facts) = shard.occurrences.as_mut() {
                for record in &mut facts.records {
                    if record
                        .target
                        .as_ref()
                        .is_some_and(|id| removed_ids.contains(id))
                    {
                        clear_target(record);
                    }
                }
            }
            // An edge already present keeps its first value.
            shard
                .edges
                .extend(contributed.remove(path).unwrap_or_default());
            shards.insert(path.clone(), shard);
        }
        for shard in shards.values_mut() {
            shard.normalize();
        }
        for (path, shard) in &shards {
            validate_shard(path, shard)?;
        }

        let removed: BTreeSet<String> = explicit
            .iter()
            .filter(|path| !upserted.contains_key(*path))
            .map(|path| (*path).to_owned())
            .collect();
        // Source records: the batch replaces the facts of every file it touches.
        let sources: BTreeMap<String, SourceFileUnits> = upserted
            .iter()
            .filter_map(|(path, file)| {
                file.source
                    .clone()
                    .map(|source| ((*path).to_owned(), source))
            })
            .collect();
        let mut source_rows: BTreeMap<&'static str, Vec<Row>> = BTreeMap::new();
        for (path, source) in &sources {
            if let Some(package) = &source.package {
                source_rows
                    .entry(PACKAGE_MEMBERS)
                    .or_default()
                    .push(Row::new(package.manifest_path.clone(), path.clone(), ""));
            }
            for (table, facts) in [
                (
                    TYPESCRIPT_CONFIGS,
                    graph_search_core::units::typescript_config_facts(source),
                ),
                (
                    PACKAGE_MANIFESTS,
                    graph_search_core::units::package_manifest_facts(source),
                ),
            ] {
                if let Some(facts) = facts {
                    let value = serde_json::to_string(&facts)
                        .map_err(|error| Error::Store(error.to_string()))?;
                    source_rows.entry(table).or_default().push(Row::new(
                        path.clone(),
                        path.clone(),
                        value,
                    ));
                }
            }
        }

        // Summary by delta: remove what the replaced shards and sources
        // counted, add what their replacements count.
        let mut summary = self.summary.clone();
        let replaced_old: Vec<&Arc<Shard>> = old.values().chain(rewritten.values()).collect();
        graph_search_core::counts::subtract(
            &mut summary.counts,
            &graph_search_core::counts::summarize(
                replaced_old.iter().flat_map(|shard| &shard.nodes),
                replaced_old.iter().flat_map(|shard| &shard.edges),
            ),
        );
        graph_search_core::counts::add(
            &mut summary.counts,
            &graph_search_core::counts::summarize(
                shards.values().flat_map(|shard| &shard.nodes),
                shards.values().flat_map(|shard| &shard.edges),
            ),
        );
        let old_sources: BTreeMap<String, SourceFileUnits> = match self.source_index()? {
            Some(index) => index
                .load_selected_verified(&self.objects(), SOURCE_LAYOUT, &touched)
                .map_err(store_io)?,
            None => BTreeMap::new(),
        };
        summary
            .source
            .subtract(&graph_search_core::units::SourceCoverage::summarize(
                old_sources.values(),
            ));
        summary
            .source
            .add(&graph_search_core::units::SourceCoverage::summarize(
                sources.values(),
            ));

        // Dependency records: may add shards rewritten only to carry a record,
        // whose graph facts (and so the summary) do not change.
        let records = match manifest {
            Some(manifest) => self.plan_dependencies(manifest, retained, &shards, &touched)?,
            None => None,
        };
        summary.dependencies = records.is_some();
        let records = records.unwrap_or_default();
        if !retained.is_empty() && !summary.dependencies {
            return Err(Error::Store(
                "retained dependencies do not match projection".into(),
            ));
        }

        let mut owners_replaced: BTreeSet<String> = shards.keys().cloned().collect();
        owners_replaced.extend(removed.iter().cloned());
        let known: BTreeSet<String> = manifest
            .map(|manifest| manifest.entries.keys().cloned().collect())
            .unwrap_or_default();
        let mut rows: BTreeMap<&'static str, Vec<Row>> = BTreeMap::new();
        for (path, shard) in &shards {
            for (table, table_rows) in shard.rows(path) {
                rows.entry(table).or_default().extend(table_rows);
            }
        }
        for (path, record) in &records {
            dependency_rows(path, record, &known, &mut rows)?;
        }
        let mut record_owners: BTreeSet<String> = records.keys().cloned().collect();
        record_owners.extend(removed.iter().cloned());

        Ok(Plan {
            removed,
            sources,
            source_owners: touched,
            rows,
            owners: owners_replaced,
            records,
            record_owners,
            source_rows,
            summary,
            outcome: ApplyOutcome {
                nodes_upserted: batch
                    .upserts
                    .iter()
                    .map(|file| file.symbols.len().saturating_add(1) as u64)
                    .sum(),
                edges_upserted,
                nodes_deleted: removed_ids.len() as u64,
                files_touched: (batch.removed_files.len() as u64)
                    .saturating_add(batch.upserts.len() as u64),
            },
            shards,
        })
    }

    /// The dependency records this publication writes, or `None` when the
    /// generation's records would not be coherent (a manifest file without
    /// one). Records are rebuilt for the written shards, for files whose facts
    /// the manifest supplies, and for every file when the previous generation's
    /// records were not coherent; every other file keeps its record.
    fn plan_dependencies(
        &self,
        manifest: &Manifest,
        retained: &BTreeSet<String>,
        shards: &BTreeMap<String, Shard>,
        touched: &BTreeSet<String>,
    ) -> Result<Option<BTreeMap<String, DependencyRecord>>> {
        let previous_coherent = self.committed.is_some() && self.summary.dependencies;
        let old_paths: BTreeSet<String> = self.paths()?.into_iter().collect();
        if shards
            .keys()
            .any(|path| !manifest.entries.contains_key(path))
            || manifest
                .entries
                .keys()
                .any(|path| !shards.contains_key(path) && !old_paths.contains(path))
        {
            return Ok(None);
        }
        let mut paths: BTreeSet<String> = shards.keys().cloned().collect();
        for (path, entry) in &manifest.entries {
            if entry.extraction.is_some() || !previous_coherent {
                paths.insert(path.clone());
            }
        }
        let unwritten: BTreeSet<String> = paths
            .iter()
            .filter(|path| !shards.contains_key(*path))
            .cloned()
            .collect();
        let loaded = self.shards_for(&unwritten)?;
        // Node owners of the written shards, then of everything else on demand.
        let mut path_of: HashMap<NodeId, Option<String>> = HashMap::new();
        for (path, shard) in shards {
            for node in &shard.nodes {
                path_of.insert(node.id.clone(), Some(path.clone()));
            }
        }
        let known = |candidate: &str| manifest.entries.contains_key(candidate);
        let mut records = BTreeMap::new();
        for path in &paths {
            let shard: &Shard = match shards.get(path) {
                Some(shard) => shard,
                None => match loaded.get(path) {
                    Some(shard) => shard,
                    None => return Ok(None),
                },
            };
            let Some(entry) = manifest.entries.get(path) else {
                return Ok(None);
            };
            let mut links = BTreeSet::new();
            for edge in &shard.edges {
                let Some(to) = &edge.to else {
                    continue;
                };
                let mut resolve = |store: &Self, id: &NodeId| -> Result<Option<String>> {
                    if let Some(known) = path_of.get(id) {
                        return Ok(known.clone());
                    }
                    let found = store
                        .owner_of(id)?
                        .filter(|owner| !touched.contains(owner) || shards.contains_key(owner));
                    path_of.insert(id.clone(), found.clone());
                    Ok(found)
                };
                let Some(target) = resolve(self, to)? else {
                    continue;
                };
                if let Some(from) = resolve(self, &edge.from)?.filter(|from| known(from)) {
                    links.insert((target.clone(), from));
                }
                if let Some(owner) = edge.path.as_ref().filter(|owner| known(owner)) {
                    links.insert((target, owner.clone()));
                }
            }
            let record = if retained.contains(path) {
                match self.dependency_record(path)? {
                    Some(prior) => DependencyRecord::retain(&prior, entry, links),
                    None => None,
                }
            } else {
                let nodes: Vec<&Node> = shard.nodes.iter().collect();
                DependencyRecord::build(entry, &nodes, links)
            };
            let Some(record) = record else {
                return Ok(None);
            };
            records.insert(path.clone(), record);
        }
        Ok(Some(records))
    }

    /// Writes the planned generation and makes it current.
    #[allow(clippy::too_many_lines)] // one artifact family per step, in commit order
    fn publish_generation(
        &mut self,
        batch: &WriteBatch,
        manifest: Option<&Manifest>,
        retained: &BTreeSet<String>,
    ) -> Result<ApplyOutcome> {
        self.ensure_available()?;
        if self.read_only {
            return Err(Error::Store(String::from("the store was opened read-only")));
        }
        let plan = self.plan(batch, manifest, retained)?;
        self.inject("after_plan")?;
        let dir = generation::allocate(&self.store_dir).map_err(store_io)?;
        let written = match self.persist(&dir, &plan, manifest, retained) {
            Ok(written) => written,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(error);
            }
        };
        let files = match generation::prepare_pointer(&self.store_dir, &dir) {
            Ok(files) => files,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(store_io(error));
            }
        };
        // Rename has made the new generation visible. A directory-sync error
        // cannot honestly be reported as a rollback: refuse reads until reopen.
        if let Err(error) = self
            .inject("after_publish")
            .and_then(|()| generation::sync_dir(&self.store_dir).map_err(store_io))
        {
            self.unavailable = true;
            return Err(Error::Store(format!(
                "generation published but directory sync failed: {error}; reopen required"
            )));
        }
        let previous = std::mem::replace(&mut self.data_dir, dir.clone());
        let outcome = self.adopt(&dir, files, plan, written, manifest)?;
        generation::reclaim(&self.store_dir, &self.data_dir, &previous);
        generation::collect(&self.store_dir);
        Ok(outcome)
    }
}

/// The dependency posting rows of one file's record.
fn dependency_rows(
    path: &str,
    record: &DependencyRecord,
    known: &BTreeSet<String>,
    rows: &mut BTreeMap<&'static str, Vec<Row>>,
) -> Result<()> {
    let postings = record.postings(path, known);
    let mut push = |table: &'static str, key: String, value: String| {
        rows.entry(table)
            .or_default()
            .push(Row::new(key, path, value));
    };
    for name in postings.consumers {
        push(DEP_CONSUMERS, name, String::new());
    }
    for target in postings.selected {
        push(DEP_SELECTED, target, String::new());
    }
    for candidate in postings.candidates {
        push(DEP_CANDIDATES, candidate, String::new());
    }
    for (target, source) in postings.incoming {
        push(DEP_INCOMING, target, source);
    }
    for flag in postings.flags {
        push(DEP_FLAGS, flag.as_str().to_owned(), String::new());
    }
    if let Some(module) = record.module() {
        let value =
            serde_json::to_string(module).map_err(|error| Error::Store(error.to_string()))?;
        push(JS_MODULES, path.to_owned(), value);
    }
    Ok(())
}

/// The indexes a publish wrote, adopted without reading them back.
struct Written {
    shards: Index,
    sources: Index,
    dependencies: Index,
    tables: Tables,
    extraction: Option<crate::manifest_records::Verified>,
}

impl NativeStore {
    /// Writes every artifact of the planned generation into `dir`, in commit
    /// order, reusing the previous generation's packs and segments.
    #[allow(clippy::too_many_lines)] // one artifact family per step
    fn persist(
        &self,
        dir: &Path,
        plan: &Plan,
        manifest: Option<&Manifest>,
        retained: &BTreeSet<String>,
    ) -> Result<Written> {
        std::fs::File::create(dir.join(generation::LEASE))
            .and_then(|file| file.sync_all())
            .map_err(store_io)?;
        let objects = self.objects();

        // Shards: write the planned ones, carry every other by reference.
        let old_shards = self.shard_index()?;
        let carried: BTreeSet<String> = old_shards
            .map(|index| {
                index
                    .paths()
                    .filter(|path| {
                        !plan.shards.contains_key(*path) && !plan.removed.contains(*path)
                    })
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let shard_records: BTreeMap<String, &Shard> = plan
            .shards
            .iter()
            .map(|(path, shard)| (path.clone(), shard))
            .collect();
        let shards = crate::source_records::save_packs(
            &objects,
            crate::shards::LAYOUT,
            &shard_records,
            &objects,
            old_shards,
            &carried,
            |_, _| Ok(false),
        )
        .map_err(store_io)?;
        crate::source_records::write_index(dir, crate::shards::LAYOUT, &shards)
            .map_err(store_io)?;
        self.inject("after_shard_persist")?;

        // Source records: every touched file's record is replaced or removed.
        let old_sources = self.source_index()?;
        let carried: BTreeSet<String> = old_sources
            .map(|index| {
                index
                    .paths()
                    .filter(|path| !plan.source_owners.contains(*path))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let sources = crate::source_records::save_packs(
            &objects,
            SOURCE_LAYOUT,
            &plan.sources,
            &objects,
            old_sources,
            &carried,
            |_, _| Ok(false),
        )
        .map_err(store_io)?;
        crate::source_records::write_index(dir, SOURCE_LAYOUT, &sources).map_err(store_io)?;
        self.inject("after_source_persist")?;

        // Dependency records: the planned ones, and every other file's while
        // the records stay coherent.
        let coherent = plan.summary.dependencies;
        let old_records = self
            .dependency_records()?
            .filter(|_| coherent && self.summary.dependencies);
        let carried: BTreeSet<String> = old_records
            .map(|index| {
                index
                    .paths()
                    .filter(|path| !plan.record_owners.contains(*path))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let record_refs: BTreeMap<String, &DependencyRecord> = plan
            .records
            .iter()
            .map(|(path, record)| (path.clone(), record))
            .collect();
        let dependencies = crate::source_records::save_packs(
            &objects,
            DEPENDENCY_LAYOUT,
            &record_refs,
            &objects,
            old_records,
            &carried,
            |_, _| Ok(false),
        )
        .map_err(store_io)?;
        crate::source_records::write_index(dir, DEPENDENCY_LAYOUT, &dependencies)
            .map_err(store_io)?;

        // Posting tables: one delta each, compacted by policy.
        let old_tables = self.table_refs()?;
        let previous_dir = self.committed.as_ref().map(|_| objects.as_path());
        let mut tables = Tables::default();
        let groups = [
            (&crate::shards::TABLES[..], &plan.owners),
            (&DEPENDENCY_TABLES[..], &plan.record_owners),
        ];
        for (group, owners) in groups {
            for table in group {
                let rows = plan.rows.get(table).cloned().unwrap_or_default();
                let reference = crate::segment::publish(
                    &objects,
                    table,
                    previous_dir,
                    old_tables
                        .tables
                        .get(*table)
                        .unwrap_or(&TableRef::default()),
                    owners,
                    rows,
                )
                .map_err(store_io)?;
                tables.tables.insert((*table).to_owned(), reference);
            }
        }
        for table in SOURCE_TABLES {
            let reference = crate::segment::publish(
                &objects,
                table,
                previous_dir,
                old_tables.tables.get(table).unwrap_or(&TableRef::default()),
                &plan.source_owners,
                plan.source_rows.get(table).cloned().unwrap_or_default(),
            )
            .map_err(store_io)?;
            tables.tables.insert(table.to_owned(), reference);
        }
        generation::sync_dir(&objects.join(crate::segment::DIRECTORY)).map_err(store_io)?;
        generation::replace(
            &dir.join(generation::TABLES),
            &serde_json::to_vec(&tables).map_err(|error| Error::Store(error.to_string()))?,
        )
        .map_err(store_io)?;
        self.inject("after_table_persist")?;

        let mut extraction = None;
        if let Some(manifest) = manifest {
            extraction = Some(
                crate::manifest_records::save_into(
                    dir,
                    &objects,
                    &objects,
                    manifest,
                    self.extraction_records()?,
                    retained,
                )
                .map_err(store_io)?,
            );
            self.inject("after_extraction_persist")?;
            sidecar::prepare_manifest(dir, &manifest.header()).map_err(store_io)?;
        }
        // The objects this generation keeps alive, for the collector.
        let mut listed: Vec<String> = Vec::new();
        listed.extend(
            shards
                .pack_names()
                .into_iter()
                .map(|name| format!("{}/{name}", crate::shards::LAYOUT.directory)),
        );
        listed.extend(
            sources
                .pack_names()
                .into_iter()
                .map(|name| format!("{}/{name}", SOURCE_LAYOUT.directory)),
        );
        listed.extend(
            dependencies
                .pack_names()
                .into_iter()
                .map(|name| format!("{}/{name}", DEPENDENCY_LAYOUT.directory)),
        );
        if let Some(extraction) = &extraction {
            listed.extend(
                extraction
                    .pack_names()
                    .into_iter()
                    .map(|name| format!("{}/{name}", crate::manifest_records::LAYOUT.directory)),
            );
        }
        for table in tables.tables.values() {
            listed.extend(
                table
                    .files()
                    .map(|file| format!("{}/{file}", crate::segment::DIRECTORY)),
            );
        }
        generation::replace(
            &dir.join(generation::OBJECT_LIST),
            &serde_json::to_vec(&listed).map_err(|error| Error::Store(error.to_string()))?,
        )
        .map_err(store_io)?;
        generation::replace(
            &dir.join(generation::SUMMARY),
            &serde_json::to_vec(&plan.summary).map_err(|error| Error::Store(error.to_string()))?,
        )
        .map_err(store_io)?;
        self.inject("after_manifest_persist")?;
        generation::sync_dir(dir).map_err(store_io)?;
        generation::sync_dir(&self.store_dir.join("generations")).map_err(store_io)?;
        self.inject("after_generation_sync")?;
        Ok(Written {
            shards,
            sources,
            dependencies,
            tables,
            extraction,
        })
    }

    /// Switches this handle to the generation it just published. Shards it
    /// already read stay cached unless the publish replaced them; every bulk
    /// view is rebuilt on its next use.
    fn adopt(
        &mut self,
        dir: &Path,
        files: BTreeMap<String, String>,
        plan: Plan,
        written: Written,
        manifest: Option<&Manifest>,
    ) -> Result<ApplyOutcome> {
        let lease = generation::pin(dir).map_err(store_io)?;
        {
            let mut cache = self.shards.write().map_err(|_| poisoned())?;
            for path in &plan.removed {
                cache.remove(path);
            }
            for (path, shard) in plan.shards {
                cache.insert(path, Arc::new(shard));
            }
        }
        self.committed = Some(generation::committed(dir, files));
        self.summary = plan.summary;
        self.manifest_header = manifest.map(Manifest::header);
        self.shard_index = OnceLock::from(Some(written.shards));
        self.source_index = OnceLock::from(Some(written.sources));
        self.dependency_index = OnceLock::from(Some(written.dependencies));
        {
            let mut records = self.records.write().map_err(|_| poisoned())?;
            for path in &plan.record_owners {
                records.remove(path);
            }
            for (path, record) in plan.records {
                records.insert(path, Arc::new(record));
            }
        }
        self.table_refs = OnceLock::from(written.tables);
        self.tables = OnceLock::new();
        self.sources = OnceLock::new();
        self.occurrence_files = OnceLock::new();
        self.extraction_records = OnceLock::from(written.extraction);
        self.nodes = OnceLock::new();
        self.metadata = OnceLock::new();
        self.body = OnceLock::new();
        self.occurrences = OnceLock::new();
        self.adjacency = OnceLock::new();
        self.generation_lease = Some(lease);
        Ok(plan.outcome)
    }
}

impl GraphStore for NativeStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        let manifest = self.manifest()?;
        self.publish_generation(&batch, manifest.as_ref(), &BTreeSet::new())
    }

    fn publish(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.publish_generation(&batch, Some(&batch.manifest), &BTreeSet::new())
    }

    fn publish_retaining(
        &mut self,
        mut batch: WriteBatch,
        retention: &graph_search_core::retention::FactRetention,
    ) -> Result<ApplyOutcome> {
        retention.validate_batch(self, &batch)?;
        let mut retained = retention.paths.clone();
        if self.extraction_records()?.is_none() || self.dependency_index()?.is_none() {
            let facts = self.extraction_facts(&retained)?;
            if facts.len() != retained.len() {
                return Err(Error::Store("missing retained extraction".into()));
            }
            for (path, facts) in facts {
                if let Some(entry) = batch.manifest.entries.get_mut(&path) {
                    entry.extraction = Some(facts);
                }
            }
            retained.clear();
        } else {
            let available = self
                .extraction_records()?
                .map(crate::manifest_records::Verified::paths)
                .unwrap_or_default();
            if !retained.is_subset(&available) {
                return Err(Error::Store("missing retained extraction".into()));
            }
            // Timestamp changes require a new full record fingerprint. Only these
            // paths need decoding; byte-identical headers retain their records.
            let changed: BTreeSet<_> = retained
                .iter()
                .filter(|path| retention.previous.get(path) != batch.manifest.get(path))
                .cloned()
                .collect();
            for (path, facts) in self.extraction_facts(&changed)? {
                if let Some(entry) = batch.manifest.entries.get_mut(&path) {
                    entry.extraction = Some(facts);
                }
                retained.remove(&path);
            }
        }
        self.publish_generation(&batch, Some(&batch.manifest), &retained)
    }

    fn generation(&self) -> Result<Option<String>> {
        self.ensure_available()?;
        Ok(self
            .committed
            .as_ref()
            .and_then(|committed| committed.dir().file_name())
            .and_then(|name| name.to_str())
            .filter(|name| name.starts_with("g-"))
            .map(str::to_owned))
    }

    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        self.ensure_available()?;
        Ok(Box::new(NativeSnapshot { store: self }))
    }

    fn dependency_index(&self) -> Result<Option<&dyn DependencyLookup>> {
        self.ensure_available()?;
        Ok(
            (self.summary.dependencies && self.manifest_header.is_some())
                .then_some(self as &dyn DependencyLookup),
        )
    }

    fn manifest(&self) -> Result<Option<Manifest>> {
        self.ensure_available()?;
        match (&self.manifest_header, self.extraction_records()?) {
            (Some(header), Some(index)) => {
                crate::manifest_records::hydrate(&self.objects(), header, index)
                    .map(Some)
                    .map_err(store_io)
            }
            (Some(header), None) => Ok(Some(header.clone())),
            _ => Ok(None),
        }
    }

    fn extraction_facts(
        &self,
        paths: &BTreeSet<String>,
    ) -> Result<graph_search_core::ports::ExtractionFacts> {
        self.ensure_available()?;
        if paths.is_empty() {
            return Ok(BTreeMap::new());
        }
        match (&self.manifest_header, self.extraction_records()?) {
            (Some(header), Some(index)) => {
                crate::manifest_records::selected(&self.objects(), header, index, paths)
                    .map_err(store_io)
            }
            _ => Ok(BTreeMap::new()),
        }
    }

    fn manifest_header(&self) -> Result<Option<Manifest>> {
        self.ensure_available()?;
        Ok(self.manifest_header.clone())
    }

    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        // A standalone manifest change also publishes a generation.
        self.publish_generation(&WriteBatch::default(), Some(&manifest), &BTreeSet::new())
            .map(|_| ())
    }
}

impl DependencyLookup for NativeStore {
    fn paths(&self) -> Result<BTreeSet<String>> {
        Ok(self.paths()?.into_iter().collect())
    }

    fn record(&self, path: &str) -> Result<Option<DependencyRecord>> {
        Ok(self
            .dependency_record(path)?
            .map(|record| DependencyRecord::clone(&record)))
    }

    fn consumers(&self, name: &str) -> Result<Vec<String>> {
        Ok(self
            .postings(DEP_CONSUMERS, name)?
            .into_iter()
            .map(|row| row.owner)
            .collect())
    }

    fn incoming(&self, target: &str) -> Result<Vec<String>> {
        Ok(self
            .postings(DEP_INCOMING, target)?
            .into_iter()
            .map(|row| row.value)
            .collect())
    }

    fn selected(&self, target: &str) -> Result<Vec<String>> {
        Ok(self
            .postings(DEP_SELECTED, target)?
            .into_iter()
            .map(|row| row.owner)
            .collect())
    }

    fn candidates(&self, path: &str) -> Result<Vec<String>> {
        Ok(self
            .postings(DEP_CANDIDATES, path)?
            .into_iter()
            .map(|row| row.owner)
            .collect())
    }

    fn flagged(&self, flag: Flag) -> Result<BTreeSet<String>> {
        Ok(self
            .postings(DEP_FLAGS, flag.as_str())?
            .into_iter()
            .map(|row| row.owner)
            .collect())
    }

    fn modules(&self) -> Result<Vec<(String, JsModule)>> {
        let Some(table) = self.tables()?.get(JS_MODULES) else {
            return Ok(Vec::new());
        };
        table
            .rows()
            .map_err(store_io)?
            .into_iter()
            .map(|row| {
                serde_json::from_str(&row.value)
                    .map(|module| (row.key, module))
                    .map_err(|error| Error::Store(error.to_string()))
            })
            .collect()
    }
}

/// The read view over a [`NativeStore`].
pub struct NativeSnapshot<'a> {
    store: &'a NativeStore,
}

impl NativeSnapshot<'_> {
    fn edge_matches(
        edge: &Edge,
        kinds: &[graph_search_types::kind::EdgeKind],
        dir: Direction,
        id: &NodeId,
    ) -> bool {
        let kind_ok = kinds.is_empty() || kinds.contains(&edge.kind);
        match dir {
            Direction::Out => kind_ok && &edge.from == id,
            Direction::In => kind_ok && edge.to.as_ref().is_some_and(|to| to == id),
            Direction::Both => {
                kind_ok && (&edge.from == id || edge.to.as_ref().is_some_and(|to| to == id))
            }
        }
    }
}

/// Sorts and drops duplicates by canonical edge id.
fn dedupe(mut edges: Vec<Edge>) -> Vec<Edge> {
    edges.sort_by(crate::shards::edge_order);
    edges.dedup_by(|a, b| a.id == b.id);
    edges
}

impl GraphSnapshot for NativeSnapshot<'_> {
    fn counts(&self) -> &graph_search_types::result::StoreCounts {
        &self.store.summary.counts
    }

    fn source_coverage(&self) -> &graph_search_core::units::SourceCoverage {
        &self.store.summary.source
    }

    fn occurrence_files(&self) -> Result<&BTreeMap<String, OccurrenceFile>> {
        self.store.occurrence_files()
    }

    fn occurrences(&self) -> Result<&graph_search_core::occurrences::OccurrenceIndex> {
        self.store.occurrences()
    }

    fn occurrence_count(&self, edge_id: &str) -> Result<Option<usize>> {
        self.store.occurrence_count(edge_id)
    }

    fn body(&self) -> Result<&graph_search_core::body::BodyIndex> {
        self.store.body()
    }

    fn source_files(&self) -> Result<&BTreeMap<String, SourceFileUnits>> {
        self.store.sources()
    }

    fn source_file(&self, path: &str) -> Result<Option<SourceFileUnits>> {
        self.store.source(path)
    }

    fn typescript_configs(&self) -> Result<BTreeMap<String, SourceFileUnits>> {
        self.store.source_facts(TYPESCRIPT_CONFIGS)
    }

    fn package_manifests(&self) -> Result<BTreeMap<String, SourceFileUnits>> {
        self.store.source_facts(PACKAGE_MANIFESTS)
    }

    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>> {
        self.store.node(id)
    }

    fn nodes_in(&self, path: &str) -> Result<Vec<Node>> {
        Ok(self
            .store
            .shard(path)?
            .map(|shard| shard.nodes.clone())
            .unwrap_or_default())
    }

    fn symbol_rows(&self, index: SymbolIndex, key: &str) -> Result<Vec<SymbolRow>> {
        let table = match index {
            SymbolIndex::Name => crate::shards::NAMES,
            SymbolIndex::Qualified => crate::shards::QUALIFIED,
        };
        self.store
            .postings(table, key)?
            .into_iter()
            .map(|row| crate::shards::symbol_row(row).map_err(store_io))
            .collect()
    }

    fn structure(&self, structure: Structure) -> Result<Vec<Node>> {
        self.store
            .postings(crate::shards::STRUCTURE, structure.as_str())?
            .into_iter()
            .map(|row| {
                serde_json::from_str::<Node>(&row.value)
                    .ok()
                    .filter(|node| node.path == row.owner)
                    .ok_or_else(|| Error::Store("invalid structure row".into()))
            })
            .collect()
    }

    fn metadata(&self) -> Result<&graph_search_core::metadata::MetadataIndex> {
        self.store.metadata()
    }

    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>> {
        Ok(self.store.metadata()?.find_by_name(name, kinds, k))
    }

    fn edges_from(
        &self,
        id: &NodeId,
        kinds: &[graph_search_types::kind::EdgeKind],
        dir: Direction,
    ) -> Result<Vec<Edge>> {
        // Outgoing edges live with their owner, or with the files the foreign
        // postings name; incoming ones with the files the incoming postings name.
        let mut paths = BTreeSet::new();
        if matches!(dir, Direction::Out | Direction::Both) {
            paths.extend(self.store.owner_of(id)?);
            paths.extend(
                self.store
                    .postings(FOREIGN, id.as_str())?
                    .into_iter()
                    .map(|row| row.owner),
            );
        }
        if matches!(dir, Direction::In | Direction::Both) {
            paths.extend(
                self.store
                    .postings(INCOMING, id.as_str())?
                    .into_iter()
                    .map(|row| row.owner),
            );
        }
        let out = self
            .store
            .shards_for(&paths)?
            .values()
            .flat_map(|shard| shard.edges.iter())
            .filter(|edge| Self::edge_matches(edge, kinds, dir, id))
            .cloned()
            .collect();
        Ok(dedupe(out))
    }

    fn edges_bounded(
        &self,
        id: &NodeId,
        kinds: &[graph_search_types::EdgeKind],
        dir: Direction,
        budget: &mut graph_search_core::work::WorkBudget,
    ) -> Result<Vec<Edge>> {
        self.store.adjacency()?.read(id, kinds, dir, budget)
    }

    fn expand(
        &self,
        seeds: &[NodeId],
        hops: u8,
        kinds: &[graph_search_types::kind::EdgeKind],
        dir: Direction,
    ) -> Result<Subgraph> {
        let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
        let mut edges: BTreeMap<String, Edge> = BTreeMap::new();
        let mut frontier: BTreeSet<NodeId> = seeds.iter().cloned().collect();
        let mut visited = frontier.clone();
        for seed in seeds {
            if let Some(node) = self.node_by_id(seed)? {
                nodes.insert(seed.as_str().to_owned(), node);
            }
        }
        for _ in 0..hops {
            let mut next: BTreeSet<NodeId> = BTreeSet::new();
            for id in &frontier {
                for edge in self.edges_from(id, kinds, dir)? {
                    if edges
                        .insert(edge.id.as_str().to_owned(), edge.clone())
                        .is_some()
                    {
                        continue; // already collected on an earlier hop
                    }
                    // The hop continues through the endpoint that is not the
                    // node we came from (incoming traversals walk to sources).
                    let onward = match (&edge.to, dir) {
                        (Some(to), Direction::Out) => Some(to.clone()),
                        (Some(to), Direction::In) => (*to == *id).then(|| edge.from.clone()),
                        (Some(to), Direction::Both) => Some(if *to == *id {
                            edge.from.clone()
                        } else {
                            to.clone()
                        }),
                        (None, _) => None,
                    };
                    if let Some(onward) = onward
                        && visited.insert(onward.clone())
                        && let Some(node) = self.node_by_id(&onward)?
                    {
                        nodes.insert(onward.as_str().to_owned(), node);
                        next.insert(onward);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        Ok(Subgraph {
            nodes: nodes.into_values().collect(),
            edges: edges.into_values().collect(),
        })
    }

    fn files_matching(&self, glob: &str, k: usize) -> Result<Vec<Node>> {
        let set = graph_search_core::files_search::compile_anchored_glob(glob)?;
        let matching: BTreeSet<String> = self
            .store
            .paths()?
            .into_iter()
            .filter(|path| set.is_match(path))
            .take(k)
            .collect();
        Ok(self
            .store
            .shards_for(&matching)?
            .values()
            .filter_map(|shard| shard.file().cloned())
            .collect())
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.store.nodes()?.clone())
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        self.store.all_edges()
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;

    /// Every injected failure point before CURRENT changes, in commit order.
    const PREPUBLICATION: [&str; 7] = [
        "after_plan",
        "after_shard_persist",
        "after_source_persist",
        "after_table_persist",
        "after_extraction_persist",
        "after_manifest_persist",
        "after_generation_sync",
    ];

    use graph_search_types::FileProjection;

    /// Opens and reads every fact: corruption surfaces at open or first read.
    fn open_and_read(root: &Path) -> Result<NativeStore> {
        let store = NativeStore::open(root, &StoreOptions::default())?;
        {
            let snapshot = store.snapshot()?;
            snapshot.all_nodes()?;
            snapshot.all_edges()?;
            snapshot.source_files()?;
            snapshot.occurrence_files()?;
        }
        store.dependency_index()?;
        store.manifest()?;
        Ok(store)
    }
    fn complete_fact_fixture() -> WriteBatch {
        let mut batch = graph_search_core::conformance::fixture_batch();
        for file in &batch.upserts {
            batch.manifest.entries.insert(
                file.file.path.clone(),
                graph_search_types::manifest::FileEntry {
                    size: file.file.bytes.unwrap(),
                    mtime_ns: 0,
                    content_hash: file.file.content_hash.clone().unwrap(),
                    parser_version: file.file.parser_version.unwrap(),
                    schema_version: batch.manifest.schema_version,
                    quarantine: None,
                    extraction: Some(graph_search_types::extraction::Extraction::default().into()),
                },
            );
        }
        batch
    }

    #[test]
    fn retained_publication_failures_leave_the_old_generation_and_allow_retry() {
        for point in PREPUBLICATION {
            let root = tempfile::tempdir().unwrap();
            let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
            store.publish(complete_fact_fixture()).unwrap();
            assert!(store.dependency_index().unwrap().is_some());
            let old = state(&store);
            let header = store.manifest_header().unwrap().unwrap();
            let retention = graph_search_core::retention::FactRetention {
                generation: store.generation().unwrap(),
                previous: header.clone(),
                paths: header.entries.keys().cloned().collect(),
            };
            let mut next = header;
            next.indexed_at_ms += 1;
            let batch = WriteBatch::with_manifest(next);
            store.failure = Some(point);
            assert!(
                store.publish_retaining(batch.clone(), &retention).is_err(),
                "{point}"
            );
            assert_eq!(state(&store), old, "{point}");
            assert_eq!(
                state(&NativeStore::open(root.path(), &StoreOptions::default()).unwrap()),
                old
            );
            store.failure = None;
            store.publish_retaining(batch, &retention).unwrap();
            assert_eq!(
                store.manifest().unwrap().unwrap().entries,
                old.2.unwrap().entries
            );
        }
    }

    #[test]
    fn retained_cold_records_are_compacted_without_decoding_their_values() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        let original = complete_fact_fixture();
        store.publish(original.clone()).unwrap();
        let dir = store.data_dir.clone();
        drop(store);
        let index_path = dir.join(crate::manifest_records::FILE);
        let mut index: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
        let mut pack = Vec::new();
        let records = index["records"].as_object_mut().unwrap();
        for (path, record) in records.iter_mut() {
            let old = crate::compress::inflate_pack(
                &std::fs::read(
                    root.path()
                        .join(generation::OBJECTS)
                        .join("extraction-records")
                        .join(record["pack"].as_str().unwrap()),
                )
                .unwrap(),
            )
            .unwrap();
            let start = usize::try_from(record["offset"].as_u64().unwrap()).unwrap();
            let len = usize::try_from(record["len"].as_u64().unwrap()).unwrap();
            // Valid authenticated JSON bytes, but not a typed FileEntry. Decoding
            // every cold record would fail; preserving it must not hide the error
            // when this path is later explicitly requested.
            let bytes = if path == "src/b.rs" {
                b"true".as_slice()
            } else {
                &old[start..start + len]
            };
            record["offset"] = serde_json::json!(pack.len());
            record["len"] = serde_json::json!(bytes.len());
            record["hash"] = graph_search_core::hash::content_hash(bytes).into();
            pack.extend_from_slice(bytes);
        }
        let pack = crate::compress::deflate(&pack).unwrap();
        let hash = graph_search_core::hash::content_hash(&pack);
        for record in records.values_mut() {
            record["pack"] = hash.clone().into();
        }
        std::fs::write(
            root.path()
                .join(generation::OBJECTS)
                .join("extraction-records")
                .join(hash),
            pack,
        )
        .unwrap();
        let bytes = serde_json::to_vec(&index).unwrap();
        std::fs::write(&index_path, &bytes).unwrap();
        let pointer_path = root.path().join(generation::CURRENT);
        let mut pointer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
        pointer["files"][crate::manifest_records::FILE] =
            graph_search_core::hash::content_hash(&bytes).into();
        std::fs::write(pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert!(store.manifest().is_err());
        let header = store.manifest_header().unwrap().unwrap();
        let retention = graph_search_core::retention::FactRetention {
            generation: store.generation().unwrap(),
            previous: header.clone(),
            paths: BTreeSet::from(["src/b.rs".into()]),
        };
        let mut next = header;
        next.entries.get_mut("src/a.rs").unwrap().extraction =
            original.manifest.entries["src/a.rs"].extraction.clone();
        store
            .publish_retaining(WriteBatch::with_manifest(next), &retention)
            .unwrap();
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(
            reopened
                .extraction_facts(&BTreeSet::from(["src/a.rs".into()]))
                .unwrap()
                .len(),
            1
        );
        assert!(
            reopened
                .extraction_facts(&BTreeSet::from(["src/b.rs".into()]))
                .is_err()
        );
        assert!(reopened.manifest().is_err());
    }

    fn fixture_batch() -> WriteBatch {
        let mut batch = graph_search_core::conformance::fixture_batch();
        batch.manifest.entries.insert(
            "src/a.rs".into(),
            graph_search_types::manifest::FileEntry {
                size: 10,
                mtime_ns: 1,
                content_hash: "ha".into(),
                parser_version: graph_search_types::limits::PARSER_VERSION,
                schema_version: graph_search_types::limits::SCHEMA_VERSION,
                quarantine: None,
                extraction: Some(graph_search_types::extraction::Extraction::default().into()),
            },
        );
        batch
    }

    fn state(store: &NativeStore) -> (Vec<Node>, Vec<Edge>, Option<Manifest>) {
        let snapshot = store.snapshot().unwrap();
        let manifest = store.manifest().unwrap();
        assert_eq!(
            store.manifest_header().unwrap(),
            manifest.as_ref().map(Manifest::header)
        );
        let paths = manifest
            .as_ref()
            .map_or_else(BTreeSet::new, |m| m.entries.keys().cloned().collect());
        let expected = manifest
            .as_ref()
            .into_iter()
            .flat_map(|m| &m.entries)
            .filter_map(|(path, entry)| {
                entry
                    .extraction
                    .as_ref()
                    .map(|facts| (path.clone(), facts.clone()))
            })
            .collect();
        assert_eq!(store.extraction_facts(&paths).unwrap(), expected);
        (
            snapshot.all_nodes().unwrap(),
            snapshot.all_edges().unwrap(),
            manifest,
        )
    }

    fn replacement() -> WriteBatch {
        let mut batch = fixture_batch();
        batch.upserts[0].symbols.clear();
        batch.upserts[0].edges.clear();
        batch.manifest.indexed_at_ms = 200;
        batch
            .manifest
            .entries
            .get_mut("src/a.rs")
            .unwrap()
            .extraction
            .as_mut()
            .unwrap()
            .references
            .push(graph_search_types::extraction::ReferenceFact::file_level(
                graph_search_types::EdgeKind::Calls,
                "replacement",
                1,
            ));
        batch
    }

    #[test]
    fn every_prepublication_failure_preserves_old_generation_and_retry() {
        for point in PREPUBLICATION {
            let root = tempfile::tempdir().unwrap();
            let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
            store.publish(fixture_batch()).unwrap();
            let old = state(&store);
            let pointer = std::fs::read(root.path().join(generation::CURRENT)).unwrap();
            store.failure = Some(point);
            assert!(store.publish(replacement()).is_err(), "{point}");
            assert_eq!(state(&store), old, "{point}");
            assert_eq!(
                std::fs::read(root.path().join(generation::CURRENT)).unwrap(),
                pointer
            );
            drop(store);
            let mut reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
            assert_eq!(state(&reopened), old, "reopen after {point}");
            // Source changes again before retry: only the new batch may appear.
            let mut latest = replacement();
            latest.manifest.indexed_at_ms = 300;
            reopened.publish(latest.clone()).unwrap();
            let clean_root = tempfile::tempdir().unwrap();
            let mut clean = NativeStore::open(clean_root.path(), &StoreOptions::default()).unwrap();
            clean.publish(latest).unwrap();
            assert_eq!(state(&reopened), state(&clean), "retry after {point}");
        }
    }

    #[test]
    fn generation_header_does_not_reparse_raw_facts_and_changes_only_on_publication() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert!(store.manifest_header().unwrap().is_none());
        store.publish(fixture_batch()).unwrap();
        let first = store.manifest_header().unwrap();
        let path = store.data_dir.join("manifest.json");
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"invalid json").unwrap();
        assert_eq!(
            store.manifest().unwrap(),
            Some(fixture_batch().manifest),
            "the opened header and record descriptor are pinned"
        );
        assert!(NativeStore::open(root.path(), &StoreOptions::default()).is_err());
        assert_eq!(
            store.manifest_header().unwrap(),
            first,
            "opened immutable header requires no raw-fact reads"
        );
        std::fs::write(&path, original).unwrap();
        store.publish(replacement()).unwrap();
        let current = store.manifest_header().unwrap();
        assert_ne!(current, first);
        drop(store);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.manifest_header().unwrap(), current);
    }

    #[test]
    fn extraction_packs_are_committed_and_verified_before_use() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        let dir = store.data_dir.clone();
        let pack = std::fs::read_dir(
            root.path()
                .join(generation::OBJECTS)
                .join(crate::manifest_records::LAYOUT.directory),
        )
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
        let original = std::fs::read(&pack).unwrap();
        std::fs::write(&pack, b"corrupt").unwrap();
        assert!(
            store.manifest_header().unwrap().is_some(),
            "hot header needs no raw reads"
        );
        assert!(
            store.manifest().is_err(),
            "requested facts must verify their bytes"
        );
        // Reopening reads no pack; reading the facts verifies them.
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert!(reopened.manifest().is_err());
        drop(reopened);
        std::fs::write(&pack, original).unwrap();
        let pointer_path = root.path().join(generation::CURRENT);
        let original_pointer = std::fs::read(&pointer_path).unwrap();
        let mut pointer: serde_json::Value = serde_json::from_slice(&original_pointer).unwrap();
        // A generation from an older format is never read: the store opens
        // unpublished and is rebuilt.
        pointer["format"] = serde_json::json!(9);
        std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
        let older = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert!(older.generation().unwrap().is_none());
        drop(older);
        pointer["format"] = serde_json::json!(13);
        pointer["files"]
            .as_object_mut()
            .unwrap()
            .remove(crate::manifest_records::FILE);
        std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
        assert!(NativeStore::open(root.path(), &StoreOptions::default()).is_err());
        std::fs::write(&pointer_path, original_pointer).unwrap();
        std::fs::remove_file(dir.join(crate::manifest_records::FILE)).unwrap();
        assert!(open_and_read(root.path()).is_err());
    }

    #[test]
    fn failed_pointer_write_preserves_graph_and_manifest() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        let old = state(&store);
        std::fs::create_dir(root.path().join("CURRENT.tmp")).unwrap();
        assert!(store.publish(replacement()).is_err());
        assert_eq!(state(&store), old);
        assert!(store.commit_manifest(Manifest::default()).is_err());
        assert_eq!(state(&store), old);
        drop(store);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(state(&reopened), old);
    }

    #[test]
    fn postpublication_error_refuses_reads_until_reopen() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        store.failure = Some("after_publish");
        assert!(store.publish(replacement()).is_err());
        assert!(store.snapshot().is_err());
        assert!(store.manifest().is_err());
        assert!(store.manifest_header().is_err());
        assert!(
            store
                .extraction_facts(&BTreeSet::from(["src/a.rs".into()]))
                .is_err()
        );
        assert!(store.extraction_facts(&BTreeSet::new()).is_err());
        drop(store);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.manifest().unwrap().unwrap().indexed_at_ms, 200);
        assert_eq!(reopened.snapshot().unwrap().all_nodes().unwrap().len(), 3);
    }

    #[test]
    fn orphan_and_invalid_generations_are_not_silently_opened() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        let old = state(&store);
        let orphan = generation::allocate(root.path()).unwrap();
        std::fs::write(orphan.join(crate::shards::FILE), b"unfinished").unwrap();
        drop(store);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(state(&reopened), old);
        drop(reopened);
        std::fs::write(
            root.path().join("CURRENT"),
            br#"{"format":1,"id":"../outside","files":{}}"#,
        )
        .unwrap();
        assert!(NativeStore::open(root.path(), &StoreOptions::default()).is_err());
    }

    #[test]
    fn crash_child() {
        let Ok(root) = std::env::var("GRAPH_SEARCH_CRASH_ROOT") else {
            return;
        };
        let point = std::env::var("GRAPH_SEARCH_CRASH_POINT").unwrap();
        let point = PREPUBLICATION
            .into_iter()
            .chain(["after_publish"])
            .find(|known| *known == point)
            .expect("known crash point");
        let mut store = NativeStore::open(Path::new(&root), &StoreOptions::default()).unwrap();
        store.failure = Some(point);
        store.crash = true;
        store.publish(replacement()).unwrap();
        panic!("crash point was not reached");
    }

    #[test]
    fn process_interruption_exposes_only_complete_generations() {
        for point in PREPUBLICATION.into_iter().chain(["after_publish"]) {
            let root = tempfile::tempdir().unwrap();
            let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
            store.publish(fixture_batch()).unwrap();
            let old = state(&store);
            drop(store);
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "store::publication_tests::crash_child"])
                .env("GRAPH_SEARCH_CRASH_ROOT", root.path())
                .env("GRAPH_SEARCH_CRASH_POINT", point)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(86),
                "{point}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
            if point == "after_publish" {
                assert_eq!(reopened.manifest().unwrap().unwrap().indexed_at_ms, 200);
                assert_eq!(reopened.snapshot().unwrap().all_nodes().unwrap().len(), 3);
            } else {
                assert_eq!(state(&reopened), old, "{point}");
            }
        }
    }

    #[test]
    fn retained_readers_keep_lazy_facts_until_their_generation_is_released() {
        let root = tempfile::tempdir().unwrap();
        let mut writer = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        writer.publish(fixture_batch()).unwrap();
        let reader = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        let original = state(&reader);
        let retained = reader.data_dir.clone();
        let prepared_reader = writer;
        let mut writer = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        for stamp in 1..=5 {
            let mut batch = replacement();
            batch.manifest.indexed_at_ms = stamp;
            writer.publish(batch).unwrap();
            assert_eq!(state(&reader), original, "reader after publication {stamp}");
            assert_eq!(
                state(&prepared_reader),
                original,
                "prepared store also owns a lease"
            );
            assert!(retained.exists());
            let latest = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
            assert_eq!(latest.manifest().unwrap().unwrap().indexed_at_ms, stamp);
        }
        drop(reader);
        drop(prepared_reader);
        writer.publish(replacement()).unwrap();
        assert!(
            !retained.exists(),
            "unleased old generations must be reclaimed"
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("generations"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn corrupted_committed_sidecar_is_an_error_and_old_generations_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        for _ in 0..4 {
            store.publish(fixture_batch()).unwrap();
        }
        assert_eq!(
            std::fs::read_dir(root.path().join("generations"))
                .unwrap()
                .count(),
            2
        );
        let active = store.data_dir.clone();
        drop(store);
        std::fs::write(active.join(crate::shards::FILE), b"corrupt").unwrap();
        // Status-level reads never touch the shards; their first reader fails.
        let store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert!(store.manifest_header().unwrap().is_some());
        assert!(store.snapshot().unwrap().counts().total_nodes > 0);
        let snapshot = store.snapshot().unwrap();
        let error = snapshot.all_edges().unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"), "{error}");
        assert!(snapshot.all_edges().is_err(), "a failed load is not cached");
    }

    #[test]
    fn superseded_objects_are_collected_and_listed_objects_survive() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        let count = || walk_objects(&root.path().join(generation::OBJECTS)).len();
        store.publish(fixture_batch()).unwrap();
        store.publish(replacement()).unwrap();
        let steady = count();
        for stamp in 0..12 {
            let mut batch = if stamp % 2 == 0 {
                fixture_batch()
            } else {
                replacement()
            };
            batch.manifest.indexed_at_ms = 1_000 + stamp;
            store.publish(batch).unwrap();
        }
        // Two live generations, however many publishes: the object set is bounded.
        assert!(
            count() <= steady.saturating_mul(2),
            "{} > 2 × {steady}",
            count()
        );
        for generation_dir in std::fs::read_dir(root.path().join("generations")).unwrap() {
            let listed: Vec<String> = serde_json::from_slice(
                &std::fs::read(generation_dir.unwrap().path().join(generation::OBJECT_LIST))
                    .unwrap(),
            )
            .unwrap();
            for object in listed {
                assert!(
                    root.path().join(generation::OBJECTS).join(&object).exists(),
                    "{object}"
                );
            }
        }
        let reopened = open_and_read(root.path()).unwrap();
        assert_eq!(state(&reopened), state(&store));
    }

    fn walk_objects(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for family in std::fs::read_dir(dir).unwrap() {
            for object in std::fs::read_dir(family.unwrap().path()).unwrap() {
                out.push(object.unwrap().path());
            }
        }
        out
    }

    #[test]
    fn projection_ownership_does_not_parse_hashes_from_paths() {
        let root = tempfile::tempdir().unwrap();
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        let file = Node::file(
            "src/a#b.rs",
            graph_search_types::Language::Rust,
            1,
            1,
            "hash",
            1,
        );
        let mut batch = WriteBatch::default();
        batch.upserts.push(FileProjection {
            file,
            ..FileProjection::default()
        });
        store.publish(batch).unwrap();
        let mut removal = WriteBatch::default();
        removal.removed_files.push(String::from("src/a#b.rs"));
        store.publish(removal).unwrap();
        assert!(store.snapshot().unwrap().all_nodes().unwrap().is_empty());
    }
    #[test]
    fn occurrence_facts_share_failure_atomicity_and_generation_integrity() {
        use graph_search_types::occurrence::{
            OccurrenceExtent, OccurrenceFile, ReferenceOccurrence, ResolutionClass,
        };
        let root = tempfile::tempdir().unwrap();
        let make = |name: &str| {
            let mut batch = fixture_batch();
            let file = &mut batch.upserts[0];
            let mut reference = ReferenceOccurrence {
                id: String::new(),
                owner: file.symbols[0].id.clone(),
                kind: graph_search_types::EdgeKind::Calls,
                span: Some(graph_search_types::Span::new(2, 2, 4, 7)),
                line: 2,
                extent: OccurrenceExtent::Expression,
                raw_name: Some(name.into()),
                name: name.into(),
                ordinal: 0,
                target: None,
                target_name: name.into(),
                resolution: ResolutionClass::Unresolved,
                reason: Some("name_missing_or_ambiguous".into()),
                scope: None,
                binding: None,
            };
            reference.id =
                graph_search_core::occurrences::identity(&file.file.path, "ha", &reference);
            file.occurrences = Some(OccurrenceFile {
                source_hash: "ha".into(),
                version: graph_search_types::limits::OCCURRENCE_VERSION,
                complete: true,
                records: vec![reference],
            });
            batch
        };
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(make("original")).unwrap();
        let original = store.occurrence_files().unwrap().clone();
        store.failure = Some("after_shard_persist");
        assert!(store.publish(make("replacement")).is_err());
        assert_eq!(store.occurrence_files().unwrap(), &original);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.occurrence_files().unwrap(), &original);
        drop(reopened);
        store.failure = None;
        store.publish(make("replacement")).unwrap();
        assert_ne!(store.occurrence_files().unwrap(), &original);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(
            reopened.occurrence_files().unwrap(),
            store.occurrence_files().unwrap()
        );
        drop(reopened);
        std::fs::write(store.data_dir.join(crate::shards::FILE), b"{}").unwrap();
        assert!(open_and_read(root.path()).is_err());
    }

    #[test]
    fn source_facts_share_failure_atomicity_and_generation_integrity() {
        let root = tempfile::tempdir().unwrap();
        let make = |text: &str| {
            let mut batch = fixture_batch();
            let file = &mut batch.upserts[0];
            let hash = graph_search_core::hash::content_hash(text.as_bytes());
            file.file.content_hash = Some(hash.clone());
            file.file.bytes = Some(text.len() as u64);
            file.source = Some(graph_search_core::units::extract(
                &file.file.path,
                text,
                &hash,
                graph_search_types::Language::Rust,
                &[],
            ));
            batch
        };
        let mut store = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(make("original body\n")).unwrap();
        let original = store.sources().unwrap().clone().clone();
        store.failure = Some("after_source_persist");
        assert!(store.publish(make("replacement body\n")).is_err());
        assert_eq!(store.sources().unwrap().clone(), original);
        let reopened = NativeStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.sources().unwrap().clone(), original);
        drop(reopened);
        store.failure = None;
        store.publish(make("replacement body\n")).unwrap();
        assert_ne!(store.sources().unwrap().clone(), original);
        // Every live generation's packs share one directory: damage them all.
        let packs: Vec<_> =
            std::fs::read_dir(root.path().join(generation::OBJECTS).join("source-records"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .map(|path| {
                    let bytes = std::fs::read(&path).unwrap();
                    (path, bytes)
                })
                .collect();
        for (path, _) in &packs {
            std::fs::write(path, b"corrupt pack").unwrap();
        }
        assert!(open_and_read(root.path()).is_err());
        for (path, bytes) in packs {
            std::fs::write(path, bytes).unwrap();
        }
        std::fs::write(store.data_dir.join(sidecar::SOURCE_FILE), b"{}").unwrap();
        assert!(open_and_read(root.path()).is_err());
    }
}
