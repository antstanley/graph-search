//! The Grafeo-backed [`GraphStore`]: the one adapter that owns a Grafeo type
//! (`SPEC.md` §4.3).
//!
//! Reads go through snapshots; writes are one `apply` per reconcile, with the
//! manifest committed separately, last. Ids are converted at the boundary and
//! kept in an in-memory map, so no Grafeo id ever appears in a result.

use crate::sidecar;
use crate::value::{
    LABEL, PROP_ID, Props, edge_from_stored, edge_to_props, kind_label, node_from_props,
    node_to_props,
};
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
use graph_search_types::{ApplyOutcome, Edge, NodeId, Scored, WriteBatch};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

/// How the store is opened.
#[derive(Clone, Debug, Default)]
pub struct StoreOptions {
    /// Open an in-memory database instead of a file-backed one (tests and
    /// throwaway reads).
    pub in_memory: bool,
}

/// The id maps plus dangling edges, shared between the store and its
/// snapshots through one lock.
struct IdMaps {
    /// Stable id string to the store's own node id.
    forward: HashMap<String, grafeo::NodeId>,
    /// The store's own node id to the stable id string.
    reverse: HashMap<u64, String>,
    /// Dangling references, kept by name (`SPEC.md` §5.2).
    dangling: Vec<Edge>,
}

/// The Grafeo-backed store.
pub struct GrafeoStore {
    db: GrafeoDB,
    /// The read trait object: every read goes through it, so no concrete
    /// store type is named past this point.
    read: Arc<dyn GrafeoRead>,
    maps: RwLock<IdMaps>,
    store_dir: PathBuf,
}

impl GrafeoStore {
    /// Opens (creating if needed) the store at `store_dir`.
    ///
    /// # Errors
    /// When Grafeo cannot open or create the database.
    pub fn open(store_dir: &Path, options: &StoreOptions) -> Result<Self> {
        let db = if options.in_memory {
            GrafeoDB::new_in_memory()
        } else {
            GrafeoDB::open(store_dir)
                .map_err(|error| Error::Store(format!("open {}: {error}", store_dir.display())))?
        };
        let read = db.graph_store();
        let store = Self {
            db,
            read,
            maps: RwLock::new(IdMaps {
                forward: HashMap::new(),
                reverse: HashMap::new(),
                dangling: Vec::new(),
            }),
            store_dir: store_dir.to_path_buf(),
        };
        store.rebuild_maps()?;
        let dangling = if options.in_memory {
            Vec::new()
        } else {
            sidecar::load_dangling(store_dir).map_err(|error| Error::Store(error.to_string()))?
        };
        store.maps.write().map_err(|_| poisoned())?.dangling = dangling;
        Ok(store)
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

    /// The store directory (for the lock file and `status`).
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.store_dir
    }

    fn ids_for_paths(&self, paths: &BTreeSet<String>) -> Vec<grafeo::NodeId> {
        let maps = self
            .maps
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut gids = Vec::new();
        for (id, gid) in &maps.forward {
            if id_path(id).is_some_and(|path| paths.contains(path)) {
                gids.push(*gid);
            }
        }
        gids
    }
}

fn poisoned() -> Error {
    Error::Store(String::from("store id map lock poisoned"))
}

/// The file path a stable id belongs to: `file:<path>` or
/// `sym:<path>#<kind>:...`.
fn id_path(id: &str) -> Option<&str> {
    let rest = id
        .strip_prefix("file:")
        .or_else(|| id.strip_prefix("sym:"))?;
    Some(rest.split('#').next().unwrap_or(rest))
}

impl GraphStore for GrafeoStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        let mut outcome = ApplyOutcome::default();

        // Removals: removed files, and the replaced subtrees of upserts.
        let mut removed_paths: BTreeSet<String> = batch.removed_files.iter().cloned().collect();
        for upsert in &batch.upserts {
            removed_paths.insert(upsert.file.path.clone());
        }
        for gid in self.ids_for_paths(&removed_paths) {
            if self.db.delete_node(gid) {
                outcome.nodes_deleted = outcome.nodes_deleted.saturating_add(1);
            }
        }

        // Drop stale id-map entries for removed paths.
        {
            let mut maps = self.maps.write().map_err(|_| poisoned())?;
            let stale: Vec<String> = maps
                .forward
                .keys()
                .filter(|id| id_path(id).is_some_and(|path| removed_paths.contains(path)))
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
            let file_gid = self.db.create_node_with_props(
                &[LABEL, kind_label(upsert.file.kind)],
                node_to_props(&upsert.file),
            );
            {
                let mut maps = self.maps.write().map_err(|_| poisoned())?;
                maps.forward
                    .insert(upsert.file.id.as_str().to_owned(), file_gid);
                maps.reverse
                    .insert(file_gid.as_u64(), upsert.file.id.as_str().to_owned());
            }
            outcome.nodes_upserted = outcome.nodes_upserted.saturating_add(1);

            for symbol in &upsert.symbols {
                let gid = self.db.create_node_with_props(
                    &[LABEL, kind_label(symbol.kind)],
                    node_to_props(symbol),
                );
                let mut maps = self.maps.write().map_err(|_| poisoned())?;
                maps.forward.insert(symbol.id.as_str().to_owned(), gid);
                maps.reverse
                    .insert(gid.as_u64(), symbol.id.as_str().to_owned());
                outcome.nodes_upserted = outcome.nodes_upserted.saturating_add(1);
            }
        }
        for upsert in &batch.upserts {
            // Resolved edges become LPG edges; dangling ones go to the
            // sidecar (`SPEC.md` §5.2).
            let mut dangling: Vec<Edge> = Vec::new();
            for edge in &upsert.edges {
                if !edge.resolved {
                    dangling.push(edge.clone());
                    continue;
                }
                let maps = self.maps.read().map_err(|_| poisoned())?;
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
                let _ = self
                    .db
                    .create_edge_with_props(from_gid, to_gid, kind, edge_to_props(edge));
                outcome.edges_upserted = outcome.edges_upserted.saturating_add(1);
            }
            side_dangling.insert(upsert.file.path.clone(), dangling);
            outcome.files_touched = outcome.files_touched.saturating_add(1);
        }

        // Rewrite the sidecar: keep dangles of untouched files, replace the
        // touched ones, drop the removed ones.
        let maps = self.maps.read().map_err(|_| poisoned())?;
        let mut kept: Vec<Edge> = maps
            .dangling
            .iter()
            .filter(|edge| {
                edge.path
                    .as_ref()
                    .is_some_and(|path| !removed_paths.contains(path))
            })
            .cloned()
            .collect();
        drop(maps);
        kept.extend(side_dangling.into_values().flatten());
        if !self.store_dir.as_os_str().is_empty() {
            sidecar::save_dangling(&self.store_dir, &kept, &kept_paths(&kept))
                .map_err(|error| Error::Store(error.to_string()))?;
        }
        {
            let mut maps = self.maps.write().map_err(|_| poisoned())?;
            maps.dangling = kept;
        }
        Ok(outcome)
    }

    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        Ok(Box::new(GrafeoSnapshot::new(self)))
    }

    fn manifest(&self) -> Result<Option<Manifest>> {
        sidecar::load_manifest(&self.store_dir).map_err(|error| Error::Store(error.to_string()))
    }

    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        sidecar::save_manifest(&self.store_dir, &manifest)
            .map_err(|error| Error::Store(error.to_string()))
    }
}

fn kept_paths(edges: &[Edge]) -> BTreeSet<String> {
    edges.iter().filter_map(|edge| edge.path.clone()).collect()
}

/// The read view over a [`GrafeoStore`].
pub struct GrafeoSnapshot<'a> {
    store: &'a GrafeoStore,
    /// Node cache for name ranking, built once per snapshot.
    by_name: HashMap<String, Vec<(String, u64)>>,
}

impl<'a> GrafeoSnapshot<'a> {
    fn new(store: &'a GrafeoStore) -> Self {
        let mut by_name: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        for gid in store.read.all_node_ids() {
            let Some(props) = store.props_of(gid) else {
                continue;
            };
            if let Some(name) = props.get("name").and_then(Value::as_str)
                && let Some(id) = props.get(PROP_ID).and_then(Value::as_str)
            {
                by_name
                    .entry(name.to_owned())
                    .or_default()
                    .push((id.to_owned(), gid.as_u64()));
            }
        }
        Self { store, by_name }
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

impl GrafeoSnapshot<'_> {
    /// Converts one stored edge. `paired` is the node the adjacency index
    /// handed back: for outgoing enumeration it is the edge's `dst`, for
    /// incoming enumeration its `src`. The record is authoritative.
    fn edge_record(&self, edge_gid: grafeo::EdgeId, paired: grafeo::NodeId) -> Option<Edge> {
        let edge = self.store.read.get_edge(edge_gid)?;
        let mut props = Props::new();
        for (key, value) in edge.properties.to_btree_map() {
            props.insert(key.as_str().to_owned(), value);
        }
        edge_from_stored(edge.src, edge.dst, edge.edge_type.as_str(), &props, &|g| {
            self.store.resolve_string(g)
        })
        .map(|mut built| {
            let _ = paired; // endpoints come from the record
            built.from = NodeId::new(self.store.resolve_string(edge.src)?);
            built.to = Some(NodeId::new(self.store.resolve_string(edge.dst)?));
            Some(built)
        })
        .and_then(std::convert::identity)
    }
}

impl GraphSnapshot for GrafeoSnapshot<'_> {
    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>> {
        let maps = self.store.maps.read().map_err(|_| poisoned())?;
        let Some(gid) = maps.forward.get(id.as_str()).copied() else {
            return Ok(None);
        };
        drop(maps);
        Ok(self.store.node_of(gid))
    }

    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>> {
        let mut scored: Vec<Scored<Node>> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let push = |node: Node,
                    score: f32,
                    scored: &mut Vec<Scored<Node>>,
                    seen: &mut BTreeSet<String>| {
            if !kinds.is_empty() && !kinds.contains(&node.kind) {
                return;
            }
            if seen.insert(node.id.as_str().to_owned()) {
                scored.push(Scored::new(node, score));
            }
        };
        let maps = self.store.maps.read().map_err(|_| poisoned())?;
        // Bare-name matches outrank qualified-name matches.
        let bare: Vec<(String, u64)> = self.by_name.get(name).cloned().unwrap_or_default();
        for (_, gid_u64) in &bare {
            if let Some(node) = self.store.node_of(grafeo::NodeId::new(*gid_u64)) {
                push(node, 1.0, &mut scored, &mut seen);
            }
        }
        drop(maps);
        for gid in self.store.read.all_node_ids() {
            let Some(props) = self.store.props_of(gid) else {
                continue;
            };
            if props.get("qualified_name").and_then(Value::as_str) == Some(name)
                && let Some(node) = self.store.node_of(gid)
            {
                push(node, 0.9, &mut scored, &mut seen);
            }
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.item.path.cmp(&b.item.path))
                .then_with(|| {
                    a.item
                        .span
                        .map(|s| s.start_line)
                        .cmp(&b.item.span.map(|s| s.start_line))
                })
        });
        scored.truncate(k);
        Ok(scored)
    }

    fn edges_from(
        &self,
        id: &NodeId,
        kinds: &[graph_search_types::kind::EdgeKind],
        dir: Direction,
    ) -> Result<Vec<Edge>> {
        let mut out: Vec<Edge> = Vec::new();
        let maps = self.store.maps.read().map_err(|_| poisoned())?;
        for edge in &maps.dangling {
            if Self::edge_matches(edge, kinds, dir, id) {
                out.push(edge.clone());
            }
        }
        drop(maps);

        let Ok(gid) = self.store.resolve_gid(id) else {
            return Ok(dedupe(out));
        };
        // Grafeo's adjacency pairs mix endpoints by direction (the outgoing
        // index pairs a target, the backward index pairs a source), so each
        // pass enumerates candidates and takes endpoints from the edge record
        // itself.
        if matches!(dir, Direction::Out | Direction::Both) {
            for (other, edge_gid) in self
                .store
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Outgoing)
            {
                if let Some(edge) = self.edge_record(edge_gid, other)
                    && Self::edge_matches(&edge, kinds, Direction::Out, id)
                {
                    out.push(edge);
                }
            }
        }
        if matches!(dir, Direction::In | Direction::Both) {
            for (other, edge_gid) in self
                .store
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Incoming)
            {
                if let Some(edge) = self.edge_record(edge_gid, other)
                    && Self::edge_matches(&edge, kinds, Direction::In, id)
                {
                    out.push(edge);
                }
            }
        }
        Ok(dedupe(out))
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
        let mut files: Vec<Node> = Vec::new();
        for gid in self.store.read.nodes_by_label(LABEL) {
            let Some(props) = self.store.props_of(gid) else {
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
            if let Some(node) = self.store.node_of(gid) {
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
        let mut nodes = Vec::new();
        for gid in self.store.read.all_node_ids() {
            if let Some(node) = self.store.node_of(gid) {
                nodes.push(node);
            }
        }
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(nodes)
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        let mut out: Vec<Edge> = Vec::new();
        {
            let maps = self.store.maps.read().map_err(|_| poisoned())?;
            out.extend(maps.dangling.iter().cloned());
        }
        // The read trait exposes adjacency, not an edge scan, so the edge set
        // is the union of every node's incident edges, deduplicated.
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        for gid in self.store.read.all_node_ids() {
            for (paired, edge_gid) in self
                .store
                .read
                .edges_from(gid, grafeo_core::graph::Direction::Both)
            {
                if !seen.insert(edge_gid.as_u64()) {
                    continue;
                }
                if let Some(edge) = self.edge_record(edge_gid, paired) {
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
