//! The in-memory store: the reference [`GraphStore`] implementation.
//!
//! It is the fake the conformance suite also runs against the Grafeo adapter
//! with (`SPEC.md` §15.4) — the port is the contract, the engine is
//! swappable. Deterministic by construction: `BTreeMap`s everywhere.

use crate::Result;
use crate::error::Error;
use crate::ports::{GraphSnapshot, GraphStore};
use graph_search_types::kind::{Direction, EdgeKind, NodeKind};
use graph_search_types::manifest::Manifest;
use graph_search_types::node::Node;
use graph_search_types::{ApplyOutcome, Edge, NodeId, Scored, Subgraph, WriteBatch};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// An in-memory projection of one workspace.
#[derive(Default)]
pub struct MemoryStore {
    nodes: BTreeMap<NodeId, Node>,
    edges: Vec<Edge>,
    manifest: Option<Manifest>,
}

impl MemoryStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many nodes are stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the store holds no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn file_ids(&self, path: &str) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self
            .nodes
            .values()
            .filter(|node| node.path == path)
            .map(|node| node.id.clone())
            .collect();
        ids.push(NodeId::file(path));
        ids
    }

    fn remove_nodes(&mut self, ids: &[NodeId]) -> u64 {
        let set: BTreeSet<&NodeId> = ids.iter().collect();
        self.nodes.retain(|id, _| !set.contains(id));
        self.edges.retain(|edge| {
            !set.contains(&edge.from) && edge.to.as_ref().is_none_or(|to| !set.contains(to))
        });
        set.len() as u64
    }
}

impl GraphStore for MemoryStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        let mut outcome = ApplyOutcome::default();
        for path in &batch.removed_files {
            outcome.nodes_deleted = outcome
                .nodes_deleted
                .saturating_add(self.remove_nodes(&self.file_ids(path)));
            outcome.files_touched = outcome.files_touched.saturating_add(1);
        }
        for upsert in &batch.upserts {
            outcome.nodes_deleted = outcome
                .nodes_deleted
                .saturating_add(self.remove_nodes(&self.file_ids(&upsert.file.path)));
            let mut nodes = vec![upsert.file.clone()];
            nodes.extend(upsert.symbols.iter().cloned());
            for node in nodes {
                if self.nodes.insert(node.id.clone(), node).is_none() {
                    outcome.nodes_upserted = outcome.nodes_upserted.saturating_add(1);
                }
            }
            let mut edge_keys: BTreeSet<graph_search_types::EdgeId> = BTreeSet::new();
            for edge in &upsert.edges {
                if edge_keys.insert(edge.id.clone()) {
                    self.edges.push(edge.clone());
                    outcome.edges_upserted = outcome.edges_upserted.saturating_add(1);
                }
            }
            outcome.files_touched = outcome.files_touched.saturating_add(1);
        }
        self.edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.kind.cmp(&b.kind))
                .then(a.id.cmp(&b.id))
        });
        Ok(outcome)
    }

    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        Ok(Box::new(MemorySnapshot::new(self)))
    }

    fn manifest(&self) -> Result<Option<Manifest>> {
        Ok(self.manifest.clone())
    }

    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        self.manifest = Some(manifest);
        Ok(())
    }
}

/// The read view over a [`MemoryStore`].
pub struct MemorySnapshot<'a> {
    store: &'a MemoryStore,
    by_name: HashMap<String, Vec<NodeId>>,
}

impl<'a> MemorySnapshot<'a> {
    fn new(store: &'a MemoryStore) -> Self {
        let mut by_name: HashMap<String, Vec<NodeId>> = HashMap::new();
        for node in store.nodes.values() {
            if let Some(name) = &node.name {
                by_name
                    .entry(name.clone())
                    .or_default()
                    .push(node.id.clone());
            }
        }
        Self { store, by_name }
    }

    fn edge_matches(edge: &Edge, kinds: &[EdgeKind], dir: Direction, id: &NodeId) -> bool {
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

impl GraphSnapshot for MemorySnapshot<'_> {
    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>> {
        Ok(self.store.nodes.get(id).cloned())
    }

    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>> {
        let mut scored: Vec<Scored<Node>> = Vec::new();
        let mut seen: BTreeSet<NodeId> = BTreeSet::new();
        let push = |node: &Node,
                    score: f32,
                    scored: &mut Vec<Scored<Node>>,
                    seen: &mut BTreeSet<NodeId>| {
            if !kinds.is_empty() && !kinds.contains(&node.kind) {
                return;
            }
            if seen.insert(node.id.clone()) {
                scored.push(Scored::new(node.clone(), score));
            }
        };
        // Exact bare name outranks exact qualified name; the spec's ordering
        // (score desc, path asc, line asc) resolves the rest.
        for id in self.by_name.get(name).into_iter().flatten() {
            if let Some(node) = self.store.nodes.get(id) {
                push(node, 1.0, &mut scored, &mut seen);
            }
        }
        for node in self.store.nodes.values() {
            if node.qualified_name.as_deref() == Some(name) {
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

    fn edges_from(&self, id: &NodeId, kinds: &[EdgeKind], dir: Direction) -> Result<Vec<Edge>> {
        let mut out: Vec<Edge> = self
            .store
            .edges
            .iter()
            .filter(|edge| Self::edge_matches(edge, kinds, dir, id))
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.kind.cmp(&b.kind))
                .then(a.id.cmp(&b.id))
        });
        Ok(out)
    }

    fn expand(
        &self,
        seeds: &[NodeId],
        hops: u8,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Subgraph> {
        let mut nodes: BTreeMap<NodeId, Node> = BTreeMap::new();
        let mut edges: BTreeMap<graph_search_types::EdgeId, Edge> = BTreeMap::new();
        let mut frontier: BTreeSet<NodeId> = seeds.iter().cloned().collect();
        let mut visited: BTreeSet<NodeId> = frontier.clone();
        for node_id in &frontier {
            if let Some(node) = self.store.nodes.get(node_id) {
                nodes.insert(node_id.clone(), node.clone());
            }
        }
        for _ in 0..hops {
            let mut next: BTreeSet<NodeId> = BTreeSet::new();
            for id in &frontier {
                for edge in self.edges_from(id, kinds, dir)? {
                    if edges.insert(edge.id.clone(), edge.clone()).is_some() {
                        continue; // already collected on an earlier hop
                    }
                    // Hop through the endpoint that is not the node we came
                    // from (incoming traversals walk to sources).
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
                        && let Some(node) = self.store.nodes.get(&onward)
                    {
                        nodes.insert(onward.clone(), node.clone());
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
        let set = crate::files_search::compile_anchored_glob(glob)?;
        let mut files: Vec<Node> = self
            .store
            .nodes
            .values()
            .filter(|node| node.is_file())
            .filter(|node| set.is_match(&node.path))
            .cloned()
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.truncate(k);
        Ok(files)
    }

    fn all_nodes(&self) -> Result<Vec<Node>> {
        Ok(self.store.nodes.values().cloned().collect())
    }

    fn all_edges(&self) -> Result<Vec<Edge>> {
        Ok(self.store.edges.clone())
    }
}

/// Convenience for callers that already hold a snapshot: the error type used
/// by the conformance suite when a port contract is violated.
#[must_use]
pub fn contract_violation(message: &str) -> Error {
    Error::Store(message.to_owned())
}
