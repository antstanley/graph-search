//! The in-memory store: the reference [`GraphStore`] implementation.
//!
//! It is the fake the conformance suite also runs against the native store
//! with (`SPEC.md` §15.4) — the port is the contract, the engine is
//! swappable. Deterministic by construction: `BTreeMap`s everywhere.

use crate::Result;
use crate::error::Error;
use crate::ports::{GraphSnapshot, GraphStore};
use graph_search_types::kind::{Direction, EdgeKind, NodeKind};
use graph_search_types::manifest::Manifest;
use graph_search_types::node::Node;
use graph_search_types::{ApplyOutcome, Edge, NodeId, Scored, Subgraph, WriteBatch};
use std::collections::{BTreeMap, BTreeSet};

/// An in-memory projection of one workspace.
#[derive(Default)]
pub struct MemoryStore {
    nodes: BTreeMap<NodeId, Node>,
    edges: Vec<Edge>,
    manifest: Option<Manifest>,
    dependencies: Option<crate::dependencies::DependencyIndex>,
    counts: graph_search_types::result::StoreCounts,
    source_coverage: crate::units::SourceCoverage,
    adjacency: crate::adjacency::AdjacencyIndex,
    metadata: crate::metadata::MetadataIndex,
    body: crate::body::BodyIndex,
    sources: BTreeMap<String, graph_search_types::source::SourceFileUnits>,
    occurrence_files: BTreeMap<String, graph_search_types::occurrence::OccurrenceFile>,
    occurrences: crate::occurrences::OccurrenceIndex,
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
}

impl GraphStore for MemoryStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        crate::units::validate_batch(&batch, self.nodes.values(), &self.sources)?;
        for upsert in &batch.upserts {
            if let Some(facts) = &upsert.occurrences {
                let owners: BTreeMap<_, _> = std::iter::once(&upsert.file)
                    .chain(&upsert.symbols)
                    .map(|node| (&node.id, node))
                    .collect();
                crate::occurrences::validate(&upsert.file, facts, |id| owners.get(id).copied())?;
            }
        }
        let mut outcome = ApplyOutcome::default();
        self.dependencies = None;
        let mutation = crate::mutation::Mutation::new(&batch, self.nodes.values());
        self.nodes.retain(|id, _| !mutation.removed.contains(id));
        self.edges.retain(|edge| !mutation.removes_edge(edge));
        outcome.nodes_deleted = mutation.removed.len() as u64;
        outcome.files_touched = batch.removed_files.len() as u64;
        for upsert in &batch.upserts {
            let mut nodes = vec![upsert.file.clone()];
            nodes.extend(upsert.symbols.iter().cloned());
            for node in nodes {
                self.nodes.insert(node.id.clone(), node);
                outcome.nodes_upserted = outcome.nodes_upserted.saturating_add(1);
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
        for path in &batch.removed_files {
            self.sources.remove(path);
        }
        for upsert in &batch.upserts {
            self.sources.remove(&upsert.file.path);
            if let Some(source) = &upsert.source {
                self.sources
                    .insert(upsert.file.path.clone(), source.clone());
            }
        }
        crate::occurrences::apply(&mut self.occurrence_files, &batch, |id| {
            self.nodes.contains_key(id)
        });
        self.occurrences = crate::occurrences::OccurrenceIndex::new(&self.occurrence_files);
        self.edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.kind.cmp(&b.kind))
                .then(a.id.cmp(&b.id))
        });
        self.counts = crate::counts::summarize(self.nodes.values(), &self.edges);
        self.metadata = self
            .metadata
            .updated(self.nodes.values().cloned().collect());
        self.adjacency = crate::adjacency::AdjacencyIndex::new(self.edges.clone());
        self.body = crate::body::BodyIndex::new(&self.sources);
        self.source_coverage = crate::units::SourceCoverage::summarize(self.sources.values());
        Ok(outcome)
    }

    fn publish_retaining(
        &mut self,
        mut batch: WriteBatch,
        retention: &crate::retention::FactRetention,
    ) -> Result<ApplyOutcome> {
        retention.validate_batch(self, &batch)?;
        let facts = self.extraction_facts(&retention.paths)?;
        if facts.len() != retention.paths.len() {
            return Err(Error::Store("missing retained extraction".into()));
        }
        let partial = batch.manifest.clone();
        for (path, facts) in facts {
            if let Some(entry) = batch.manifest.entries.get_mut(&path) {
                entry.extraction = Some(facts);
            }
        }
        let full = batch.manifest.clone();
        let previous = self.dependencies.clone();
        let outcome = self.apply(batch)?;
        self.dependencies = crate::dependencies::DependencyIndex::build_retaining(
            &partial,
            self.nodes.values(),
            &self.edges,
            previous.as_ref(),
            &retention.paths,
        );
        self.manifest = Some(full);
        Ok(outcome)
    }

    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        Ok(Box::new(MemorySnapshot::new(self)))
    }

    fn manifest(&self) -> Result<Option<Manifest>> {
        Ok(self.manifest.clone())
    }

    fn extraction_facts(&self, paths: &BTreeSet<String>) -> Result<crate::ports::ExtractionFacts> {
        Ok(paths
            .iter()
            .filter_map(|path| {
                self.manifest
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
        Ok(self.manifest.as_ref().map(Manifest::header))
    }

    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        self.dependencies = crate::dependencies::DependencyIndex::build(
            &manifest,
            self.nodes.values(),
            &self.edges,
        );
        self.manifest = Some(manifest);
        Ok(())
    }

    fn dependency_index(&self) -> Result<Option<&dyn crate::dependencies::DependencyLookup>> {
        Ok(self
            .dependencies
            .as_ref()
            .map(|index| index as &dyn crate::dependencies::DependencyLookup))
    }
}

/// The read view over a [`MemoryStore`].
pub struct MemorySnapshot<'a> {
    store: &'a MemoryStore,
}

impl<'a> MemorySnapshot<'a> {
    const fn new(store: &'a MemoryStore) -> Self {
        Self { store }
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
    fn counts(&self) -> &graph_search_types::result::StoreCounts {
        &self.store.counts
    }

    fn source_coverage(&self) -> &crate::units::SourceCoverage {
        &self.store.source_coverage
    }

    fn occurrence_files(
        &self,
    ) -> Result<&BTreeMap<String, graph_search_types::occurrence::OccurrenceFile>> {
        Ok(&self.store.occurrence_files)
    }
    fn occurrences(&self) -> Result<&crate::occurrences::OccurrenceIndex> {
        Ok(&self.store.occurrences)
    }
    fn body(&self) -> Result<&crate::body::BodyIndex> {
        Ok(&self.store.body)
    }

    fn source_files(
        &self,
    ) -> Result<&BTreeMap<String, graph_search_types::source::SourceFileUnits>> {
        Ok(&self.store.sources)
    }

    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>> {
        Ok(self.store.nodes.get(id).cloned())
    }

    fn metadata(&self) -> Result<&crate::metadata::MetadataIndex> {
        Ok(&self.store.metadata)
    }

    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>> {
        Ok(self.store.metadata.find_by_name(name, kinds, k))
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

    fn edges_bounded(
        &self,
        id: &NodeId,
        kinds: &[EdgeKind],
        dir: Direction,
        budget: &mut crate::work::WorkBudget,
    ) -> Result<Vec<Edge>> {
        self.store.adjacency.read(id, kinds, dir, budget)
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
