//! The Grafeo-backed [`GraphStore`]: the one adapter that owns a Grafeo type
//! (`SPEC.md` §4.3).
//!
//! Reads go through snapshots; reconciliation publishes graph and manifest as
//! one prepared generation. Ids are converted at the boundary and
//! kept in an in-memory map, so no Grafeo id ever appears in a result.
//!
//! Opening a published generation reads only its pointer, manifest header and
//! summary. The graph, each fact family and every derived index are loaded on
//! first use: artifacts are verified against their committed hashes before
//! any byte is used, and facts are validated against the graph when they load,
//! so a damaged generation fails the first query that touches it.

use crate::value::{
    LABEL, PROP_ID, Props, edge_from_stored, edge_to_props, kind_label, node_from_props,
    node_to_props,
};
use crate::{generation, sidecar};
use grafeo::GrafeoDB;
use grafeo::GraphStore as GrafeoRead;
use grafeo_common::types::Value;
use graph_search_core::Result;
use graph_search_core::error::Error;
use graph_search_core::ports::{GraphSnapshot, GraphStore};
use graph_search_types::Subgraph;
use graph_search_types::kind::{Direction, NodeKind};
use graph_search_types::manifest::Manifest;
use graph_search_types::node::Node;
use graph_search_types::occurrence::OccurrenceFile;
use graph_search_types::source::SourceFileUnits;
use graph_search_types::{ApplyOutcome, Edge, FileProjection, NodeId, Scored, WriteBatch};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

/// How the store is opened.
#[derive(Clone, Debug, Default)]
pub struct StoreOptions {
    /// Open an in-memory database instead of a file-backed one (tests and
    /// throwaway reads).
    pub in_memory: bool,
    /// The caller only intends to read. Before any generation is published
    /// there is nothing to attach to, so a read-only open must not create or
    /// take a writable on-disk database; it exposes an empty in-memory store
    /// instead (`SPEC.md` §6.6).
    pub read_only: bool,
}

/// Stable id strings to and from the store's own node ids.
#[derive(Default)]
struct IdMaps {
    forward: HashMap<String, grafeo::NodeId>,
    reverse: HashMap<u64, String>,
}

/// The graph database with its id maps.
struct Graph {
    db: GrafeoDB,
    /// The read trait object: every read goes through it, so no concrete
    /// store type is named past this point.
    read: Arc<dyn GrafeoRead>,
    maps: RwLock<IdMaps>,
}

impl Graph {
    fn new(db: GrafeoDB) -> Result<Self> {
        let read = db.graph_store();
        let graph = Self {
            db,
            read,
            maps: RwLock::new(IdMaps::default()),
        };
        graph.rebuild_maps()?;
        Ok(graph)
    }

    fn rebuild_maps(&self) -> Result<()> {
        let mut maps = self.maps.write().map_err(|_| poisoned())?;
        maps.forward.clear();
        maps.reverse.clear();
        for gid in self.read.all_node_ids() {
            let Some(props) = self.props_of(gid) else {
                continue;
            };
            let Some(id) = props.get(PROP_ID).and_then(Value::as_str) else {
                continue;
            };
            maps.forward.insert(id.to_owned(), gid);
            maps.reverse.insert(gid.as_u64(), id.to_owned());
        }
        Ok(())
    }

    fn props_of(&self, gid: grafeo::NodeId) -> Option<Props> {
        let node = self.read.get_node(gid)?;
        let mut props = Props::new();
        for (key, value) in node.properties.to_btree_map() {
            props.insert(key.as_str().to_owned(), value);
        }
        Some(props)
    }

    fn node_of(&self, gid: grafeo::NodeId) -> Option<Node> {
        let node = self.read.get_node(gid)?;
        let labels: Vec<&str> = node
            .labels
            .iter()
            .map(grafeo_common::types::ArcStr::as_str)
            .collect();
        let props = self.props_of(gid)?;
        node_from_props(labels, &props)
    }

    fn resolve_gid(&self, id: &NodeId) -> Result<grafeo::NodeId> {
        self.maps
            .read()
            .map_err(|_| poisoned())?
            .forward
            .get(id.as_str())
            .copied()
            .ok_or_else(|| Error::Store(format!("unknown node id {id}")))
    }

    fn resolve_string(&self, gid: grafeo::NodeId) -> Option<String> {
        self.maps
            .read()
            .ok()
            .and_then(|maps| maps.reverse.get(&gid.as_u64()).cloned())
    }

    /// Converts one stored edge. The record is authoritative for endpoints.
    fn edge_record(&self, edge_gid: grafeo::EdgeId) -> Option<Edge> {
        let edge = self.read.get_edge(edge_gid)?;
        let mut props = Props::new();
        for (key, value) in edge.properties.to_btree_map() {
            props.insert(key.as_str().to_owned(), value);
        }
        let mut built =
            edge_from_stored(edge.src, edge.dst, edge.edge_type.as_str(), &props, &|g| {
                self.resolve_string(g)
            })?;
        built.from = NodeId::new(self.resolve_string(edge.src)?);
        built.to = Some(NodeId::new(self.resolve_string(edge.dst)?));
        Some(built)
    }
}

/// Native source facts and the record index their packs were read through.
#[derive(Default)]
struct Sources {
    files: BTreeMap<String, SourceFileUnits>,
    records: Option<crate::source_records::Index>,
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

/// Writers mutate only resident state; `load_resident` makes it so.
fn resident<T>(cell: &mut OnceLock<T>) -> Result<&mut T> {
    cell.get_mut().ok_or_else(not_resident)
}

fn not_resident() -> Error {
    Error::Store(String::from("generation state is not resident"))
}

/// The Grafeo-backed store.
pub struct GrafeoStore {
    graph: OnceLock<Graph>,
    store_dir: PathBuf,
    data_dir: PathBuf,
    unavailable: bool,
    /// The published generation lazily read from, or `None` when every fact
    /// is resident (unpublished and freshly prepared stores).
    committed: Option<generation::Committed>,
    summary: generation::Summary,
    dangling: OnceLock<Vec<Edge>>,
    sources: OnceLock<Sources>,
    occurrence_files: OnceLock<BTreeMap<String, OccurrenceFile>>,
    /// Per-relationship occurrence counts; `None` for generations before format 9.
    edge_counts: OnceLock<Option<crate::edge_counts::EdgeCounts>>,
    extraction_records: OnceLock<Option<crate::manifest_records::Verified>>,
    dependencies: OnceLock<Option<graph_search_core::dependencies::DependencyIndex>>,
    manifest_header: Option<Manifest>,
    transient_manifest: Option<Manifest>,
    /// Every node, sorted by id: validation lookups and the metadata index.
    nodes: OnceLock<Vec<Node>>,
    metadata: OnceLock<graph_search_core::metadata::MetadataIndex>,
    body: OnceLock<graph_search_core::body::BodyIndex>,
    occurrences: OnceLock<graph_search_core::occurrences::OccurrenceIndex>,
    adjacency: OnceLock<graph_search_core::adjacency::AdjacencyIndex>,
    /// Dropped after the graph/fact fields; prevents reclaiming lazy record paths.
    generation_lease: Option<std::fs::File>,
    #[cfg(test)]
    failure: Option<&'static str>,
    #[cfg(test)]
    crash: bool,
}

impl GrafeoStore {
    /// Opens (creating if needed) the store at `store_dir`.
    ///
    /// # Errors
    /// When Grafeo cannot open or create the database.
    pub fn open(store_dir: &Path, options: &StoreOptions) -> Result<Self> {
        let selected = if options.in_memory {
            None
        } else {
            generation::current(store_dir).map_err(|error| Error::Store(error.to_string()))?
        };
        match selected {
            Some(selected) => Self::open_generation(store_dir, selected),
            None => Self::open_unpublished(store_dir, options),
        }
    }

    fn empty(store_dir: &Path, data_dir: PathBuf) -> Self {
        Self {
            graph: OnceLock::new(),
            store_dir: store_dir.to_path_buf(),
            data_dir,
            unavailable: false,
            committed: None,
            summary: generation::Summary::default(),
            dangling: OnceLock::new(),
            sources: OnceLock::new(),
            occurrence_files: OnceLock::new(),
            edge_counts: OnceLock::new(),
            extraction_records: OnceLock::new(),
            dependencies: OnceLock::new(),
            manifest_header: None,
            transient_manifest: None,
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
        }
    }

    /// A published generation: only the header and summary are read here.
    fn open_generation(store_dir: &Path, selected: generation::Selected) -> Result<Self> {
        let mut store = Self::empty(store_dir, selected.committed.dir().to_path_buf());
        store.manifest_header = selected.manifest;
        store.committed = Some(selected.committed);
        store.generation_lease = Some(selected.lease);
        store.summary = match selected.summary {
            Some(summary) => summary,
            // Generations before format 9 carry no summary.
            None => store.summarize()?,
        };
        Ok(store)
    }

    /// No published generation: an empty in-memory store, or the legacy
    /// writable layout, loaded and validated whole.
    fn open_unpublished(store_dir: &Path, options: &StoreOptions) -> Result<Self> {
        // A read-only caller on a not-yet-published store has nothing on disk to
        // attach to: expose an empty in-memory store rather than creating or
        // locking a writable database (`SPEC.md` §6.6). Real in-memory opens are
        // unaffected.
        let in_memory = options.in_memory || options.read_only;
        let mut store = Self::empty(store_dir, store_dir.to_path_buf());
        let db = if in_memory {
            GrafeoDB::new_in_memory()
        } else {
            store.manifest_header = sidecar::load_manifest(store_dir)
                .map_err(store_io)?
                .as_ref()
                .map(Manifest::header);
            GrafeoDB::open(store_dir)
                .map_err(|error| Error::Store(format!("open {}: {error}", store_dir.display())))?
        };
        store.graph = OnceLock::from(Graph::new(db)?);
        let (dangling, (files, records), occurrences) = if in_memory {
            (Vec::new(), (BTreeMap::new(), None), BTreeMap::new())
        } else {
            (
                sidecar::load_dangling(store_dir).map_err(store_io)?,
                crate::source_records::load_cached(store_dir).map_err(store_io)?,
                sidecar::load_occurrences(store_dir).map_err(store_io)?,
            )
        };
        store.dangling = OnceLock::from(dangling);
        store.sources = OnceLock::from(Sources { files, records });
        store.occurrence_files = OnceLock::from(occurrences);
        store.extraction_records = OnceLock::from(None);
        store.dependencies = OnceLock::from(None);
        store.validate_sources(&store.sources()?.files)?;
        store.validate_occurrences(store.occurrence_files()?)?;
        store.summary = store.summarize()?;
        Ok(store)
    }

    fn committed(&self) -> Result<&generation::Committed> {
        self.committed.as_ref().ok_or_else(not_resident)
    }

    fn graph(&self) -> Result<&Graph> {
        loaded(&self.graph, || {
            self.committed()?
                .read(generation::GRAPH)
                .map_err(store_io)?
                .ok_or_else(|| Error::Store(String::from("generation has no graph")))?;
            // Verified above; Grafeo reads the same immutable, leased file.
            let db = GrafeoDB::open_read_only(self.data_dir.join(generation::GRAPH))
                .map_err(|error| Error::Store(format!("open generation: {error}")))?;
            Graph::new(db)
        })
    }

    fn dangling(&self) -> Result<&Vec<Edge>> {
        loaded(&self.dangling, || {
            let bytes = self
                .committed()?
                .read(sidecar::DANGLING_FILE)
                .map_err(store_io)?
                .ok_or_else(|| Error::Store(String::from("generation has no dangling sidecar")))?;
            sidecar::decode_dangling(&bytes).map_err(store_io)
        })
    }

    fn sources(&self) -> Result<&Sources> {
        loaded(&self.sources, || {
            let committed = self.committed()?;
            let (files, records) = match committed.read(sidecar::SOURCE_FILE).map_err(store_io)? {
                Some(bytes) => crate::source_records::load_prepared(
                    &self.data_dir,
                    crate::source_records::prepare(&bytes, committed.format()).map_err(store_io)?,
                )
                .map_err(store_io)?,
                None => (BTreeMap::new(), None),
            };
            self.validate_sources(&files)?;
            Ok(Sources { files, records })
        })
    }

    fn occurrence_files(&self) -> Result<&BTreeMap<String, OccurrenceFile>> {
        loaded(&self.occurrence_files, || {
            let files = match self
                .committed()?
                .read(sidecar::OCCURRENCE_FILE)
                .map_err(store_io)?
            {
                Some(bytes) => sidecar::decode_occurrences(&bytes).map_err(store_io)?,
                None => BTreeMap::new(),
            };
            self.validate_occurrences(&files)?;
            Ok(files)
        })
    }

    /// The occurrence count of one relationship, from the published table when
    /// the occurrence facts are not already resident.
    fn occurrence_count(&self, edge_id: &str) -> Result<Option<usize>> {
        if self.occurrences.get().is_none() && self.committed.is_some() {
            let table = loaded(&self.edge_counts, || {
                self.committed()?
                    .read(crate::edge_counts::FILE)
                    .map_err(store_io)?
                    .map(crate::edge_counts::EdgeCounts::decode)
                    .transpose()
                    .map_err(store_io)
            })?;
            if let Some(table) = table {
                return Ok(table.get(edge_id));
            }
        }
        Ok(self.occurrences()?.count_for_edge(edge_id))
    }

    fn extraction_records(&self) -> Result<Option<&crate::manifest_records::Verified>> {
        loaded(&self.extraction_records, || {
            let bytes = self
                .committed()?
                .read(crate::manifest_records::FILE)
                .map_err(store_io)?;
            match (bytes, self.manifest_header.as_ref()) {
                (Some(bytes), Some(header)) => Ok(Some(
                    crate::manifest_records::prepare_verified(&bytes, header, &self.data_dir)
                        .map_err(store_io)?,
                )),
                _ => Ok(None),
            }
        })
        .map(Option::as_ref)
    }

    fn dependencies(&self) -> Result<Option<&graph_search_core::dependencies::DependencyIndex>> {
        loaded(&self.dependencies, || {
            let Some(bytes) = self
                .committed()?
                .read(generation::DEPENDENCIES)
                .map_err(store_io)?
            else {
                return Ok(None);
            };
            let index: Option<graph_search_core::dependencies::DependencyIndex> =
                serde_json::from_slice(
                    &crate::compress::inflate_sidecar(&bytes).map_err(store_io)?,
                )
                .map_err(|error| Error::Store(error.to_string()))?;
            if let Some(index) = &index {
                let valid = self
                    .manifest_header
                    .as_ref()
                    .zip(self.extraction_records()?)
                    .is_some_and(|(header, facts)| index.validates(header, &facts.paths()));
                if !valid {
                    return Err(Error::Store(String::from(
                        "dependency index does not match generation",
                    )));
                }
            }
            Ok(index)
        })
        .map(Option::as_ref)
    }

    fn nodes(&self) -> Result<&Vec<Node>> {
        loaded(&self.nodes, || GrafeoSnapshot::new(self).all_nodes())
    }

    fn find_node<'n>(nodes: &'n [Node], id: &NodeId) -> Option<&'n Node> {
        nodes
            .binary_search_by(|node| node.id.cmp(id))
            .ok()
            .map(|index| &nodes[index])
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
            Ok(graph_search_core::body::BodyIndex::new(
                &self.sources()?.files,
            ))
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
                GrafeoSnapshot::new(self).all_edges()?,
            ))
        })
    }

    /// Source facts must belong to their file's projected version and owners.
    fn validate_sources(&self, files: &BTreeMap<String, SourceFileUnits>) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let nodes = self.nodes()?;
        for (path, source) in files {
            let file = Self::find_node(nodes, &NodeId::file(path))
                .ok_or_else(|| Error::Store(format!("source facts without file owner: {path}")))?;
            graph_search_core::units::validate(file, source, |id| Self::find_node(nodes, id))?;
        }
        Ok(())
    }

    /// Occurrences must belong to their file, and bound targets must exist.
    fn validate_occurrences(&self, files: &BTreeMap<String, OccurrenceFile>) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let nodes = self.nodes()?;
        for (path, facts) in files {
            let file = Self::find_node(nodes, &NodeId::file(path))
                .ok_or_else(|| Error::Store(format!("occurrences without file owner: {path}")))?;
            graph_search_core::occurrences::validate(file, facts, |id| Self::find_node(nodes, id))?;
            if facts.records.iter().any(|r| {
                r.target
                    .as_ref()
                    .is_some_and(|id| Self::find_node(nodes, id).is_none())
            }) {
                return Err(Error::Store(format!(
                    "occurrence target absent from generation: {path}"
                )));
            }
        }
        Ok(())
    }

    /// Exact totals of the current state, for the published summary.
    fn summarize(&self) -> Result<generation::Summary> {
        let edges = GrafeoSnapshot::new(self).all_edges()?;
        Ok(generation::Summary {
            counts: graph_search_core::counts::summarize(self.nodes()?, &edges),
            source: graph_search_core::units::SourceCoverage::summarize(
                self.sources()?.files.values(),
            ),
        })
    }

    /// Makes every base fact resident, as writers need.
    fn load_resident(&self) -> Result<()> {
        self.graph()?;
        self.dangling()?;
        self.sources()?;
        self.occurrence_files()?;
        self.extraction_records()?;
        self.dependencies()?;
        Ok(())
    }

    /// After a mutation: derived indexes are rebuilt on demand, except metadata
    /// that can share unchanged postings with the previous generation's.
    fn refresh_indexes(
        &mut self,
        previous: Option<&graph_search_core::metadata::MetadataIndex>,
    ) -> Result<()> {
        self.nodes = OnceLock::new();
        self.metadata = OnceLock::new();
        self.body = OnceLock::new();
        self.occurrences = OnceLock::new();
        self.adjacency = OnceLock::new();
        self.summary = self.summarize()?;
        if let Some(previous) = previous {
            self.metadata = OnceLock::from(previous.updated(self.nodes()?.clone()));
        }
        Ok(())
    }

    /// The store directory (for the lock file and `status`).
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.store_dir
    }
}

#[allow(clippy::needless_pass_by_value)] // map_err consumes its error
fn store_io(error: std::io::Error) -> Error {
    Error::Store(error.to_string())
}

fn poisoned() -> Error {
    Error::Store(String::from("store id map lock poisoned"))
}

impl GrafeoStore {
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

    fn ensure_available(&self) -> Result<()> {
        if self.unavailable {
            return Err(Error::Store(String::from(
                "generation publication durability is uncertain; close and reopen the index",
            )));
        }
        Ok(())
    }

    /// Reconstruct a complete batch using explicit path ownership. This is the
    /// correctness baseline; later deltas can avoid copying unchanged nodes.
    fn complete_projection(&self) -> Result<WriteBatch> {
        let snapshot = self.snapshot()?;
        let nodes = snapshot.all_nodes()?;
        let sources = &self.sources()?.files;
        let occurrences = self.occurrence_files()?;
        let owners: HashMap<NodeId, String> = nodes
            .iter()
            .map(|node| (node.id.clone(), node.path.clone()))
            .collect();
        let mut files: BTreeMap<String, FileProjection> = nodes
            .iter()
            .filter(|node| node.is_file())
            .map(|node| {
                (
                    node.path.clone(),
                    FileProjection {
                        file: node.clone(),
                        source: sources.get(&node.path).cloned(),
                        occurrences: occurrences.get(&node.path).cloned(),
                        ..FileProjection::default()
                    },
                )
            })
            .collect();
        for node in nodes.into_iter().filter(|node| !node.is_file()) {
            files
                .get_mut(&node.path)
                .ok_or_else(|| Error::Store(format!("missing file owner for {}", node.id)))?
                .symbols
                .push(node);
        }
        for edge in snapshot.all_edges()? {
            let path = edge
                .path
                .as_ref()
                .or_else(|| owners.get(&edge.from))
                .ok_or_else(|| Error::Store(format!("missing source owner for {}", edge.id)))?;
            files
                .get_mut(path)
                .ok_or_else(|| Error::Store(format!("missing file {path} for {}", edge.id)))?
                .edges
                .push(edge);
        }
        Ok(WriteBatch {
            upserts: files.into_values().collect(),
            ..WriteBatch::default()
        })
    }

    fn publish_generation(
        &mut self,
        batch: &WriteBatch,
        manifest: Option<&Manifest>,
        retained: &BTreeSet<String>,
    ) -> Result<ApplyOutcome> {
        self.ensure_available()?;
        // The projection and record reuse below read every current fact.
        self.load_resident()?;
        // No fallible work mutates the currently visible store.
        let mut prepared = Self::open(
            Path::new(""),
            &StoreOptions {
                in_memory: true,
                ..StoreOptions::default()
            },
        )?;
        prepared.apply_prepared(&self.complete_projection()?)?;
        #[cfg(test)]
        {
            prepared.failure = self.failure;
            prepared.crash = self.crash;
        }
        let outcome = prepared.apply_prepared(batch)?;
        // No reader observes the intermediate projection. Mutation uses the
        // graph and ID maps, so build retrieval indexes only for the final state.
        prepared.refresh_indexes(self.metadata.get())?;
        prepared.manifest_header = manifest.map(Manifest::header);
        if let Some(manifest) = manifest {
            let edges = prepared.snapshot()?.all_edges()?;
            let dependencies = graph_search_core::dependencies::DependencyIndex::build_retaining(
                manifest,
                prepared.nodes()?,
                &edges,
                self.dependencies()?,
                retained,
            );
            if !retained.is_empty() && dependencies.is_none() {
                return Err(Error::Store(
                    "retained dependencies do not match projection".into(),
                ));
            }
            prepared.dependencies = OnceLock::from(dependencies);
        }
        if self.store_dir.as_os_str().is_empty() {
            // Internal transient stores deliberately have no persistence.
            prepared.transient_manifest = manifest.cloned();
            *self = prepared;
            return Ok(outcome);
        }
        let dir = generation::allocate(&self.store_dir).map_err(store_io)?;
        if let Err(error) = prepared.persist_prepared(&dir, manifest, self, batch, retained) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(error);
        }
        if let Err(error) = generation::prepare_pointer(&self.store_dir, &dir) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(store_io(error));
        }
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
        let previous = self.data_dir.clone();
        prepared.store_dir.clone_from(&self.store_dir);
        prepared.data_dir = dir;
        *self = prepared;
        generation::reclaim(&self.store_dir, &self.data_dir, &previous);
        Ok(outcome)
    }

    fn persist_prepared(
        &mut self,
        dir: &Path,
        manifest: Option<&Manifest>,
        previous: &Self,
        batch: &WriteBatch,
        retained: &BTreeSet<String>,
    ) -> Result<()> {
        self.graph()?
            .db
            .save(dir.join(generation::GRAPH))
            .map_err(|error| Error::Store(error.to_string()))?;
        std::fs::File::open(dir.join(generation::GRAPH))
            .and_then(|file| file.sync_all())
            .map_err(store_io)?;
        self.inject("after_graph_persist")?;
        sidecar::prepare_dangling(dir, self.dangling()?).map_err(store_io)?;
        self.inject("after_dangling_persist")?;
        // apply_prepared only removes or replaces source facts for these paths;
        // surviving facts outside the set are exact clones of the previous store.
        let touched: BTreeSet<&str> = batch
            .upserts
            .iter()
            .map(|file| file.file.path.as_str())
            .chain(batch.removed_files.iter().map(String::as_str))
            .collect();
        let previous_sources = previous.sources()?;
        let records = crate::source_records::save_cached(
            dir,
            &self.sources()?.files,
            &previous.data_dir,
            &previous_sources.files,
            previous_sources.records.as_ref(),
            Some(&touched),
        )
        .map_err(store_io)?;
        resident(&mut self.sources)?.records = Some(records);
        self.inject("after_source_persist")?;
        sidecar::save_occurrences(dir, self.occurrence_files()?).map_err(store_io)?;
        generation::replace(
            &dir.join(crate::edge_counts::FILE),
            &crate::edge_counts::encode(self.occurrence_files()?).map_err(store_io)?,
        )
        .map_err(store_io)?;
        self.inject("after_occurrence_persist")?;
        if let Some(manifest) = manifest {
            self.extraction_records = OnceLock::from(Some(
                crate::manifest_records::save_retaining(
                    dir,
                    manifest,
                    &previous.data_dir,
                    previous.extraction_records()?,
                    retained,
                )
                .map_err(store_io)?,
            ));
            self.inject("after_extraction_persist")?;
            let bytes = serde_json::to_vec(&self.dependencies()?)
                .map_err(|error| Error::Store(error.to_string()))?;
            let bytes = crate::compress::deflate(&bytes).map_err(store_io)?;
            generation::replace(&dir.join(generation::DEPENDENCIES), &bytes).map_err(store_io)?;
            self.inject("after_dependencies_persist")?;
            sidecar::prepare_manifest(dir, &manifest.header()).map_err(store_io)?;
        }
        self.inject("after_manifest_persist")?;
        let summary =
            serde_json::to_vec(&self.summary).map_err(|error| Error::Store(error.to_string()))?;
        generation::replace(&dir.join(generation::SUMMARY), &summary).map_err(store_io)?;
        // Acquire before CURRENT changes so failure still leaves the old store
        // authoritative. Prepared stores keep their lazy records pinned too.
        self.generation_lease = Some(generation::pin(dir).map_err(store_io)?);
        generation::sync_dir(dir).map_err(store_io)?;
        generation::sync_dir(
            dir.parent()
                .ok_or_else(|| Error::Store(String::from("missing generation parent")))?,
        )
        .map_err(store_io)?;
        self.inject("after_generation_sync")
    }

    // Private preparation only: derived retrieval indexes remain stale until
    // publish_generation finishes all mutations and calls refresh_indexes.
    #[allow(clippy::too_many_lines)] // isolated node deletion/insertion/edge preparation stages
    fn apply_prepared(&mut self, batch: &WriteBatch) -> Result<ApplyOutcome> {
        self.load_resident()?;
        let source_nodes = self.snapshot()?.all_nodes()?;
        graph_search_core::units::validate_batch(
            batch,
            source_nodes.iter(),
            &self.sources()?.files,
        )?;
        for upsert in &batch.upserts {
            if let Some(facts) = &upsert.occurrences {
                let owners: BTreeMap<_, _> = std::iter::once(&upsert.file)
                    .chain(&upsert.symbols)
                    .map(|node| (&node.id, node))
                    .collect();
                graph_search_core::occurrences::validate(&upsert.file, facts, |id| {
                    owners.get(id).copied()
                })?;
            }
        }
        let mut outcome = ApplyOutcome::default();
        let graph = self.graph.get().ok_or_else(not_resident)?;

        let mutation = graph_search_core::mutation::Mutation::new(batch, source_nodes.iter());
        let removed_gids: Vec<_> = mutation
            .removed
            .iter()
            .map(|id| graph.resolve_gid(id))
            .collect::<Result<_>>()?;
        let mut removed_edges = BTreeMap::new();
        for gid in graph.read.all_node_ids() {
            for (_, edge_gid) in graph
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Both)
            {
                if graph
                    .edge_record(edge_gid)
                    .is_some_and(|edge| mutation.removes_edge(&edge))
                {
                    removed_edges.insert(edge_gid.as_u64(), edge_gid);
                }
            }
        }
        for gid in removed_edges.into_values() {
            graph.db.delete_edge(gid);
        }
        outcome.files_touched = batch.removed_files.len() as u64;
        let removed_set: BTreeSet<u64> = removed_gids.iter().map(grafeo::NodeId::as_u64).collect();
        for gid in &removed_gids {
            if graph.db.delete_node(*gid) {
                outcome.nodes_deleted = outcome.nodes_deleted.saturating_add(1);
            }
        }

        self.inject("after_delete")?;

        // Drop stale id-map entries for removed paths.
        {
            let mut maps = graph.maps.write().map_err(|_| poisoned())?;
            let stale: Vec<String> = maps
                .forward
                .keys()
                .filter(|id| {
                    maps.forward
                        .get(*id)
                        .is_some_and(|gid| removed_set.contains(&gid.as_u64()))
                })
                .cloned()
                .collect();
            for id in stale {
                if let Some(gid) = maps.forward.remove(&id) {
                    maps.reverse.remove(&gid.as_u64());
                }
            }
        }

        // Insertions: every node first, then every edge, so a forward
        // reference inside one batch resolves (`SPEC.md` §6.4).
        let mut side_dangling: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
        for upsert in &batch.upserts {
            for node in std::iter::once(&upsert.file).chain(&upsert.symbols) {
                let existing = graph
                    .maps
                    .read()
                    .map_err(|_| poisoned())?
                    .forward
                    .get(node.id.as_str())
                    .copied();
                if let Some(gid) = existing {
                    let props = node_to_props(node);
                    if let Some(old) = graph.props_of(gid) {
                        for key in old.keys() {
                            if !props.iter().any(|(new, _)| new == key) {
                                graph.db.remove_node_property(gid, key);
                            }
                        }
                    }
                    for (key, value) in props {
                        graph.db.set_node_property(gid, &key, value);
                    }
                } else {
                    let gid = graph.db.create_node_with_props(
                        &[LABEL, kind_label(node.kind)],
                        node_to_props(node),
                    );
                    let mut maps = graph.maps.write().map_err(|_| poisoned())?;
                    maps.forward.insert(node.id.as_str().to_owned(), gid);
                    maps.reverse
                        .insert(gid.as_u64(), node.id.as_str().to_owned());
                }
                outcome.nodes_upserted = outcome.nodes_upserted.saturating_add(1);
            }
        }
        self.inject("after_insert")?;
        for upsert in &batch.upserts {
            // Resolved edges become LPG edges; dangling ones go to the
            // sidecar (`SPEC.md` §5.2).
            let mut dangling: Vec<Edge> = Vec::new();
            for edge in &upsert.edges {
                if !edge.resolved {
                    dangling.push(edge.clone());
                    continue;
                }
                let maps = graph.maps.read().map_err(|_| poisoned())?;
                let (Some(from_gid), Some(to_gid)) = (
                    maps.forward.get(edge.from.as_str()).copied(),
                    edge.to
                        .as_ref()
                        .and_then(|to| maps.forward.get(to.as_str()).copied()),
                ) else {
                    drop(maps);
                    dangling.push(edge.clone());
                    continue;
                };
                drop(maps);
                let kind = edge.kind.as_str();
                let _ =
                    graph
                        .db
                        .create_edge_with_props(from_gid, to_gid, kind, edge_to_props(edge));
                outcome.edges_upserted = outcome.edges_upserted.saturating_add(1);
            }
            side_dangling.insert(upsert.file.path.clone(), dangling);
            outcome.files_touched = outcome.files_touched.saturating_add(1);
        }

        self.inject("after_edges")?;

        // Rewrite the sidecar: keep dangles of untouched files, replace the
        // touched ones, drop the removed ones.
        let mut kept: Vec<Edge> = self
            .dangling()?
            .iter()
            .filter(|edge| !mutation.removes_edge(edge))
            .cloned()
            .collect();
        kept.extend(side_dangling.into_values().flatten());
        *resident(&mut self.dangling)? = kept;
        let sources = &mut resident(&mut self.sources)?.files;
        for path in &batch.removed_files {
            sources.remove(path);
        }
        for upsert in &batch.upserts {
            sources.remove(&upsert.file.path);
            if let Some(source) = &upsert.source {
                sources.insert(upsert.file.path.clone(), source.clone());
            }
        }
        {
            let graph = self.graph.get().ok_or_else(not_resident)?;
            let maps = graph.maps.read().map_err(|_| poisoned())?;
            let occurrence_files = self.occurrence_files.get_mut().ok_or_else(not_resident)?;
            graph_search_core::occurrences::apply(occurrence_files, batch, |id| {
                maps.forward.contains_key(id.as_str())
            });
        }
        Ok(outcome)
    }
}

impl GraphStore for GrafeoStore {
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
        if self.extraction_records()?.is_none() || self.dependencies()?.is_none() {
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
            .data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| name.starts_with("g-"))
            .map(str::to_owned))
    }

    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        self.ensure_available()?;
        Ok(Box::new(GrafeoSnapshot::new(self)))
    }

    fn dependency_index(
        &self,
    ) -> Result<Option<&graph_search_core::dependencies::DependencyIndex>> {
        self.ensure_available()?;
        self.dependencies()
    }

    fn manifest(&self) -> Result<Option<Manifest>> {
        self.ensure_available()?;
        if self.transient_manifest.is_some() {
            return Ok(self.transient_manifest.clone());
        }
        match (&self.manifest_header, self.extraction_records()?) {
            (Some(header), Some(index)) => {
                crate::manifest_records::hydrate(&self.data_dir, header, index)
                    .map(Some)
                    .map_err(store_io)
            }
            _ => sidecar::load_manifest(&self.data_dir).map_err(store_io),
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
        if let (Some(header), Some(index)) = (&self.manifest_header, self.extraction_records()?) {
            return crate::manifest_records::selected(&self.data_dir, header, index, paths)
                .map_err(store_io);
        }
        // Legacy embedded facts retain the compatibility path until publication
        // migrates them into generation-local packed records.
        let manifest = self.manifest()?;
        Ok(paths
            .iter()
            .filter_map(|path| {
                manifest
                    .as_ref()?
                    .entries
                    .get(path)?
                    .extraction
                    .as_ref()
                    .map(|facts| (path.clone(), facts.clone()))
            })
            .collect())
    }

    fn manifest_header(&self) -> Result<Option<Manifest>> {
        self.ensure_available()?;
        Ok(self.manifest_header.clone())
    }

    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        // Standalone manifest changes also publish a complete generation.
        self.publish_generation(&WriteBatch::default(), Some(&manifest), &BTreeSet::new())
            .map(|_| ())
    }
}

/// The read view over a [`GrafeoStore`].
pub struct GrafeoSnapshot<'a> {
    store: &'a GrafeoStore,
}

impl<'a> GrafeoSnapshot<'a> {
    const fn new(store: &'a GrafeoStore) -> Self {
        Self { store }
    }

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

impl GraphSnapshot for GrafeoSnapshot<'_> {
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
        Ok(&self.store.sources()?.files)
    }

    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>> {
        let graph = self.store.graph()?;
        let maps = graph.maps.read().map_err(|_| poisoned())?;
        let Some(gid) = maps.forward.get(id.as_str()).copied() else {
            return Ok(None);
        };
        drop(maps);
        Ok(graph.node_of(gid))
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
        let mut out: Vec<Edge> = self
            .store
            .dangling()?
            .iter()
            .filter(|edge| Self::edge_matches(edge, kinds, dir, id))
            .cloned()
            .collect();
        let graph = self.store.graph()?;
        let Ok(gid) = graph.resolve_gid(id) else {
            return Ok(dedupe(out));
        };
        // Grafeo's adjacency pairs mix endpoints by direction (the outgoing
        // index pairs a target, the backward index pairs a source), so each
        // pass enumerates candidates and takes endpoints from the edge record
        // itself.
        if matches!(dir, Direction::Out | Direction::Both) {
            for (_, edge_gid) in graph
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Outgoing)
            {
                if let Some(edge) = graph.edge_record(edge_gid)
                    && Self::edge_matches(&edge, kinds, Direction::Out, id)
                {
                    out.push(edge);
                }
            }
        }
        if matches!(dir, Direction::In | Direction::Both) {
            for (_, edge_gid) in graph
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Incoming)
            {
                if let Some(edge) = graph.edge_record(edge_gid)
                    && Self::edge_matches(&edge, kinds, Direction::In, id)
                {
                    out.push(edge);
                }
            }
        }
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
                        (Some(to), Direction::Both) => {
                            let onward = if *to == *id {
                                edge.from.clone()
                            } else {
                                to.clone()
                            };
                            Some(onward)
                        }
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
        let graph = self.store.graph()?;
        let mut files: Vec<Node> = Vec::new();
        for gid in graph.read.nodes_by_label(LABEL) {
            let Some(props) = graph.props_of(gid) else {
                continue;
            };
            let Some(path) = props.get("path").and_then(Value::as_str) else {
                continue;
            };
            let Some(kind) = props.get("kind").and_then(Value::as_str) else {
                continue;
            };
            if kind != "file" || !set.is_match(path) {
                continue;
            }
            if let Some(node) = graph.node_of(gid) {
                files.push(node);
            }
            if files.len() >= k {
                break;
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.truncate(k);
        Ok(files)
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        let graph = self.store.graph()?;
        let mut nodes = Vec::new();
        for gid in graph.read.all_node_ids() {
            if let Some(node) = graph.node_of(gid) {
                nodes.push(node);
            }
        }
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(nodes)
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let mut out: Vec<Edge> = self.store.dangling()?.clone();
        // The read trait exposes adjacency, not an edge scan, so the edge set
        // is the union of every node's incident edges, deduplicated.
        let graph = self.store.graph()?;
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        for gid in graph.read.all_node_ids() {
            for (_, edge_gid) in graph
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Both)
            {
                if !seen.insert(edge_gid.as_u64()) {
                    continue;
                }
                if let Some(edge) = graph.edge_record(edge_gid) {
                    out.push(edge);
                }
            }
        }
        Ok(dedupe(out))
    }
}

/// Sorts and drops duplicates by canonical edge id.
fn dedupe(mut edges: Vec<Edge>) -> Vec<Edge> {
    edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.id.cmp(&b.id))
    });
    edges.dedup_by(|a, b| a.id == b.id);
    edges
}

#[cfg(test)]
mod publication_tests {
    use super::*;

    /// Opens and reads every fact: corruption surfaces at open or first read.
    fn open_and_read(root: &Path) -> Result<GrafeoStore> {
        let store = GrafeoStore::open(root, &StoreOptions::default())?;
        store.load_resident()?;
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
        for point in [
            "after_graph_persist",
            "after_dangling_persist",
            "after_source_persist",
            "after_occurrence_persist",
            "after_extraction_persist",
            "after_dependencies_persist",
            "after_manifest_persist",
            "after_generation_sync",
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
            store.publish(complete_fact_fixture()).unwrap();
            assert!(store.dependencies().unwrap().is_some());
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
                state(&GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap()),
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
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
                    dir.join("extraction-records")
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
        std::fs::write(dir.join("extraction-records").join(hash), pack).unwrap();
        let bytes = serde_json::to_vec(&index).unwrap();
        std::fs::write(&index_path, &bytes).unwrap();
        let pointer_path = root.path().join(generation::CURRENT);
        let mut pointer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
        pointer["files"][crate::manifest_records::FILE] =
            graph_search_core::hash::content_hash(&bytes).into();
        std::fs::write(pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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

    fn state(store: &GrafeoStore) -> (Vec<Node>, Vec<Edge>, Option<Manifest>) {
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
        for point in [
            "after_delete",
            "after_insert",
            "after_edges",
            "after_graph_persist",
            "after_dangling_persist",
            "after_source_persist",
            "after_occurrence_persist",
            "after_extraction_persist",
            "after_dependencies_persist",
            "after_manifest_persist",
            "after_generation_sync",
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
            let mut reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
            assert_eq!(state(&reopened), old, "reopen after {point}");
            // Source changes again before retry: only the new batch may appear.
            let mut latest = replacement();
            latest.manifest.indexed_at_ms = 300;
            reopened.publish(latest.clone()).unwrap();
            let clean_root = tempfile::tempdir().unwrap();
            let mut clean = GrafeoStore::open(clean_root.path(), &StoreOptions::default()).unwrap();
            clean.publish(latest).unwrap();
            assert_eq!(state(&reopened), state(&clean), "retry after {point}");
        }
    }

    #[test]
    fn generation_header_does_not_reparse_raw_facts_and_changes_only_on_publication() {
        let root = tempfile::tempdir().unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        assert!(GrafeoStore::open(root.path(), &StoreOptions::default()).is_err());
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
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.manifest_header().unwrap(), current);
    }

    #[test]
    fn extraction_packs_are_committed_and_verified_before_use() {
        let root = tempfile::tempdir().unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        let dir = store.data_dir.clone();
        let pack = std::fs::read_dir(dir.join(crate::manifest_records::LAYOUT.directory))
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
        assert!(open_and_read(root.path()).is_err());
        std::fs::write(&pack, original).unwrap();
        let pointer_path = root.path().join(generation::CURRENT);
        let original_pointer = std::fs::read(&pointer_path).unwrap();
        let mut pointer: serde_json::Value = serde_json::from_slice(&original_pointer).unwrap();
        pointer["format"] = serde_json::json!(5);
        std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
        assert!(GrafeoStore::open(root.path(), &StoreOptions::default()).is_err());
        pointer["format"] = serde_json::json!(6);
        pointer["files"]
            .as_object_mut()
            .unwrap()
            .remove(crate::manifest_records::FILE);
        std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
        assert!(GrafeoStore::open(root.path(), &StoreOptions::default()).is_err());
        std::fs::write(&pointer_path, original_pointer).unwrap();
        std::fs::remove_file(dir.join(crate::manifest_records::FILE)).unwrap();
        assert!(open_and_read(root.path()).is_err());
    }

    #[test]
    fn failed_pointer_write_preserves_graph_and_manifest() {
        let root = tempfile::tempdir().unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        let old = state(&store);
        std::fs::create_dir(root.path().join("CURRENT.tmp")).unwrap();
        assert!(store.publish(replacement()).is_err());
        assert_eq!(state(&store), old);
        assert!(store.commit_manifest(Manifest::default()).is_err());
        assert_eq!(state(&store), old);
        drop(store);
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(state(&reopened), old);
    }

    #[test]
    fn postpublication_error_refuses_reads_until_reopen() {
        let root = tempfile::tempdir().unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.manifest().unwrap().unwrap().indexed_at_ms, 200);
        assert_eq!(reopened.snapshot().unwrap().all_nodes().unwrap().len(), 3);
    }

    #[test]
    fn orphan_and_invalid_generations_are_not_silently_opened() {
        let root = tempfile::tempdir().unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(fixture_batch()).unwrap();
        let old = state(&store);
        let orphan = generation::allocate(root.path()).unwrap();
        std::fs::write(orphan.join(generation::GRAPH), b"unfinished").unwrap();
        drop(store);
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(state(&reopened), old);
        drop(reopened);
        std::fs::write(
            root.path().join("CURRENT"),
            br#"{"format":1,"id":"../outside","files":{}}"#,
        )
        .unwrap();
        assert!(GrafeoStore::open(root.path(), &StoreOptions::default()).is_err());
    }

    #[test]
    fn legacy_graph_and_sidecars_migrate_without_losing_edges() {
        let root = tempfile::tempdir().unwrap();
        let mut legacy = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        let batch = fixture_batch();
        legacy.apply_prepared(&batch).unwrap();
        sidecar::save_manifest(root.path(), &batch.manifest).unwrap();
        sidecar::save_dangling(root.path(), &[], &BTreeSet::new()).unwrap();
        // This fixture writes sidecars outside publication; its open handle's
        // header intentionally predates that write. Verify the header on reopen.
        let old = {
            let snapshot = legacy.snapshot().unwrap();
            (
                snapshot.all_nodes().unwrap(),
                snapshot.all_edges().unwrap(),
                Some(batch.manifest.clone()),
            )
        };
        drop(legacy);
        let mut reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(state(&reopened), old);
        assert!(reopened.generation().unwrap().is_none());
        reopened.publish(batch).unwrap();
        assert_eq!(state(&reopened), old);
        let generation = reopened.generation().unwrap();
        assert!(generation.is_some());
        drop(reopened);
        let migrated = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(state(&migrated), old);
        assert_eq!(migrated.generation().unwrap(), generation);
    }

    #[test]
    fn crash_child() {
        let Ok(root) = std::env::var("GRAPH_SEARCH_CRASH_ROOT") else {
            return;
        };
        let point = std::env::var("GRAPH_SEARCH_CRASH_POINT").unwrap();
        let point = match point.as_str() {
            "after_delete" => "after_delete",
            "after_insert" => "after_insert",
            "after_graph_persist" => "after_graph_persist",
            "after_dangling_persist" => "after_dangling_persist",
            "after_source_persist" => "after_source_persist",
            "after_occurrence_persist" => "after_occurrence_persist",
            "after_extraction_persist" => "after_extraction_persist",
            "after_dependencies_persist" => "after_dependencies_persist",
            "after_manifest_persist" => "after_manifest_persist",
            "after_generation_sync" => "after_generation_sync",
            "after_publish" => "after_publish",
            _ => panic!("unknown crash point"),
        };
        let mut store = GrafeoStore::open(Path::new(&root), &StoreOptions::default()).unwrap();
        store.failure = Some(point);
        store.crash = true;
        store.publish(replacement()).unwrap();
        panic!("crash point was not reached");
    }

    #[test]
    fn process_interruption_exposes_only_complete_generations() {
        for point in [
            "after_delete",
            "after_insert",
            "after_graph_persist",
            "after_dangling_persist",
            "after_source_persist",
            "after_occurrence_persist",
            "after_extraction_persist",
            "after_dependencies_persist",
            "after_manifest_persist",
            "after_generation_sync",
            "after_publish",
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
            let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        let mut writer = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        writer.publish(fixture_batch()).unwrap();
        let reader = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        let original = state(&reader);
        let retained = reader.data_dir.clone();
        let prepared_reader = writer;
        let mut writer = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
            let latest = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        std::fs::write(active.join(sidecar::DANGLING_FILE), b"corrupt").unwrap();
        // Status-level reads never touch the sidecar; its first reader fails.
        let store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert!(store.manifest_header().unwrap().is_some());
        assert!(store.snapshot().unwrap().counts().total_nodes > 0);
        let snapshot = store.snapshot().unwrap();
        let error = snapshot.all_edges().unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"), "{error}");
        assert!(snapshot.all_edges().is_err(), "a failed load is not cached");
    }

    #[test]
    fn projection_ownership_does_not_parse_hashes_from_paths() {
        let root = tempfile::tempdir().unwrap();
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
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
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(make("original")).unwrap();
        let original = store.occurrence_files().unwrap().clone();
        store.failure = Some("after_occurrence_persist");
        assert!(store.publish(make("replacement")).is_err());
        assert_eq!(store.occurrence_files().unwrap(), &original);
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.occurrence_files().unwrap(), &original);
        drop(reopened);
        store.failure = None;
        store.publish(make("replacement")).unwrap();
        assert_ne!(store.occurrence_files().unwrap(), &original);
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(
            reopened.occurrence_files().unwrap(),
            store.occurrence_files().unwrap()
        );
        drop(reopened);
        std::fs::write(store.data_dir.join(sidecar::OCCURRENCE_FILE), b"{}").unwrap();
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
        let mut store = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        store.publish(make("original body\n")).unwrap();
        let original = store.sources().unwrap().files.clone();
        store.failure = Some("after_source_persist");
        assert!(store.publish(make("replacement body\n")).is_err());
        assert_eq!(store.sources().unwrap().files, original);
        let reopened = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
        assert_eq!(reopened.sources().unwrap().files, original);
        drop(reopened);
        store.failure = None;
        store.publish(make("replacement body\n")).unwrap();
        assert_ne!(store.sources().unwrap().files, original);
        let pack = std::fs::read_dir(store.data_dir.join("source-records"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = std::fs::read(&pack).unwrap();
        std::fs::write(&pack, b"corrupt pack").unwrap();
        assert!(open_and_read(root.path()).is_err());
        std::fs::write(pack, bytes).unwrap();
        std::fs::write(store.data_dir.join(sidecar::SOURCE_FILE), b"{}").unwrap();
        assert!(open_and_read(root.path()).is_err());
    }
}
