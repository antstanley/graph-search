//! The query engine: every read mode over a [`GraphSnapshot`]
//! (`SPEC.md` §8.3, §8.4).
//!
//! Every method is bounded by the query's clamped cap and reports the caps
//! that fired. Results are deterministic: score desc, then path asc, then
//! line asc; edges by from, kind, to (`SPEC.md` §9.4).

use crate::Result;
use crate::config::WalkPolicy;
use crate::error::Error;
use crate::ports::GraphSnapshot;
use graph_search_types::context::ResultContext;
use graph_search_types::kind::{Direction, EdgeKind, NodeKind};
use graph_search_types::limits::{DEFAULT_TRAVERSAL_DEPTH, GRAPH_DEFAULT_LIMIT, MAX_HOPS_CEILING};
use graph_search_types::node::Node;
use graph_search_types::query::{
    DepsQuery, ExploreQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery, TraversalQuery,
};
use graph_search_types::result::{
    Approximation, DepthCount, EdgeHit, ExploreItem, ExploreResult, GraphResult, ImpactResult,
    ImpactSummary, Snippet, Stats, SymbolHit, Truncation, TruncationKind,
};
use graph_search_types::retrieval::{
    ExploreMode, RankingStrategy, RetrievalEvidence, RetrievalPlan, RetrievalRoute,
};
use graph_search_types::{NodeId, Scored};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[path = "query_positional.rs"]
mod positional;

/// The edge kinds a `refs` answer counts as references (`SPEC.md` §8.3).
pub const REFERENCE_KINDS: [EdgeKind; 3] =
    [EdgeKind::Calls, EdgeKind::References, EdgeKind::TypeUses];

/// How many files the explore literal scan may open.
pub const SCAN_FILE_CAP: usize = 512;

/// How many bytes the explore literal scan may read.
pub const SCAN_BYTES_CAP: u64 = 8 * 1024 * 1024;

/// The effective result cap of a query, so a `Default`-derived (unclamped)
/// query still honours the spec default instead of returning nothing.
#[must_use]
pub const fn effective_limit(limit: u32) -> u32 {
    if limit == 0 {
        graph_search_types::limits::GRAPH_DEFAULT_LIMIT
    } else if limit > graph_search_types::limits::GRAPH_LIMIT_CEILING {
        graph_search_types::limits::GRAPH_LIMIT_CEILING
    } else {
        limit
    }
}

/// The read API over one snapshot.
struct Seeds {
    plan: Option<RetrievalPlan>,
    retrieval: BTreeMap<NodeId, RetrievalEvidence>,
    coverage: graph_search_types::coverage::Coverage,
    nodes: Vec<Scored<Node>>,
    evidence: BTreeMap<NodeId, graph_search_types::source::SourceEvidence>,
    match_lines: BTreeMap<NodeId, crate::evidence::MatchContext>,
    truncations: Vec<Truncation>,
    candidates: usize,
    files_scanned: u64,
}

/// The read API over one snapshot.
pub struct QueryEngine<'a> {
    snapshot: &'a dyn GraphSnapshot,
    neighborhoods: std::cell::RefCell<crate::neighborhoods::Neighborhoods>,
    work_limits: crate::work::WorkLimits,
    filters: std::cell::RefCell<crate::metadata::CompiledFilters>,
    work: std::cell::RefCell<crate::work::WorkBudget>,
    inherited_work: std::cell::Cell<bool>,
}

impl<'a> QueryEngine<'a> {
    /// An engine over `snapshot`.
    #[must_use]
    pub fn new(snapshot: &'a dyn GraphSnapshot) -> Self {
        Self::with_work_limits(snapshot, crate::work::WorkLimits::default())
    }

    /// An engine whose graph traversals share explicit work and cancellation limits.
    #[must_use]
    pub fn with_work_limits(
        snapshot: &'a dyn GraphSnapshot,
        limits: crate::work::WorkLimits,
    ) -> Self {
        Self {
            snapshot,
            neighborhoods: std::cell::RefCell::new(crate::neighborhoods::Neighborhoods::default()),
            inherited_work: std::cell::Cell::new(false),
            work: std::cell::RefCell::new(crate::work::WorkBudget::new(limits.clone())),
            work_limits: limits,
            filters: std::cell::RefCell::new(crate::metadata::CompiledFilters::default()),
        }
    }

    /// Carries already-spent request work into the first query. Later queries
    /// reset to the same configured limits, like a normally constructed engine.
    #[must_use]
    pub fn with_work_budget(
        snapshot: &'a dyn GraphSnapshot,
        work: crate::work::WorkBudget,
    ) -> Self {
        Self {
            snapshot,
            neighborhoods: std::cell::RefCell::new(crate::neighborhoods::Neighborhoods::default()),
            work_limits: work.limits(),
            work: std::cell::RefCell::new(work),
            inherited_work: std::cell::Cell::new(true),
            filters: std::cell::RefCell::new(crate::metadata::CompiledFilters::default()),
        }
    }

    fn reset_work(&self) -> Result<()> {
        self.neighborhoods.borrow_mut().clear();
        if !self.inherited_work.replace(false) {
            *self.work.borrow_mut() = crate::work::WorkBudget::new(self.work_limits.clone());
        }
        self.work.borrow().check()
    }

    fn finish_work(&self, stats: &mut Stats, truncations: &mut Vec<Truncation>) -> Result<()> {
        let work = self.work.borrow();
        work.check()?;
        let (nodes, edges, limits) = work.report();
        let (candidates, postings) = work.lexical_report();
        stats.metadata_entries_examined = work.metadata_entries_examined();
        stats.retrieval_candidates_admitted = candidates;
        stats.lexical_postings_examined = postings;
        stats.dictionary_entries_examined = work.dictionary_entries_examined();
        stats.occurrences_examined = work.occurrences_examined();
        stats.context_windows_examined = work.context_windows_examined();
        (
            stats.positional_bytes_examined,
            stats.positional_tokens_examined,
            stats.positional_witnesses,
        ) = work.positional_report();
        stats.source_files_attempted = work.source_report().0;
        stats.source_bytes_read = work.source_report().1;
        stats.graph_nodes_visited = nodes;
        stats.graph_edges_examined = edges;
        for limit in limits {
            if !truncations.iter().any(|prior| prior.kind == limit.kind) {
                truncations.push(limit);
            }
        }
        Ok(())
    }

    fn finish_edges(&self, edges: &mut Vec<EdgeHit>, truncations: &mut Vec<Truncation>) {
        let cap = self.work.borrow().returned_edge_limit();
        if edges.len() > cap {
            edges.truncate(cap);
            truncations.push(Truncation::new(
                TruncationKind::ReturnedEdges,
                cap as u64,
                "returned-edge cap reached; delivered relationships are partial",
            ));
        }
        for edge in edges {
            let id = graph_search_types::EdgeId::of(
                &NodeId::new(&edge.from),
                edge.kind,
                edge.to.as_deref().unwrap_or(&edge.to_name),
            );
            edge.occurrence_count = self.snapshot.occurrences().count_for_edge(id.as_str());
        }
    }

    fn read_node(&self, id: &NodeId) -> Result<Option<Node>> {
        if !self.work.borrow_mut().node(id)? {
            return Ok(None);
        }
        self.snapshot.node_by_id(id)
    }

    fn read_edges(
        &self,
        id: &NodeId,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Vec<graph_search_types::Edge>> {
        self.neighborhoods.borrow_mut().read(
            self.snapshot,
            id,
            kinds,
            dir,
            &mut self.work.borrow_mut(),
        )
    }

    fn expand(
        &self,
        seeds: &[NodeId],
        hops: u8,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<graph_search_types::Subgraph> {
        self.expand_with_seed_edges(seeds, hops, kinds, dir, None)
    }

    /// Reuse an already budgeted root neighborhood within this traversal only.
    /// The caller must supply edges with the same kinds and direction.
    fn expand_with_seed_edges(
        &self,
        seeds: &[NodeId],
        hops: u8,
        kinds: &[EdgeKind],
        dir: Direction,
        seed_edges: Option<(&NodeId, &[graph_search_types::Edge])>,
    ) -> Result<graph_search_types::Subgraph> {
        let mut nodes = BTreeMap::new();
        let mut edges = BTreeMap::new();
        let mut visited = BTreeSet::new();
        let mut frontier = BTreeSet::new();
        for id in seeds {
            if let Some(node) = self.read_node(id)? {
                visited.insert(id.clone());
                frontier.insert(id.clone());
                nodes.insert(id.clone(), node);
            }
        }
        for _ in 0..hops {
            let mut next = BTreeSet::new();
            for id in &frontier {
                let neighborhood =
                    if let Some((_, edges)) = seed_edges.filter(|(seed, _)| *seed == id) {
                        edges.to_vec()
                    } else {
                        self.read_edges(id, kinds, dir)?
                    };
                for edge in neighborhood {
                    let onward = match (&edge.to, dir) {
                        (Some(to), Direction::Out) => Some(to.clone()),
                        (Some(_), Direction::In) => Some(edge.from.clone()),
                        (Some(to), Direction::Both) => Some(if to == id {
                            edge.from.clone()
                        } else {
                            to.clone()
                        }),
                        (None, _) => None,
                    };
                    edges.entry(edge.id.clone()).or_insert(edge);
                    if let Some(onward) = onward
                        && visited.insert(onward.clone())
                        && let Some(node) = self.read_node(&onward)?
                    {
                        nodes.insert(onward.clone(), node);
                        next.insert(onward);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        Ok(graph_search_types::Subgraph {
            nodes: nodes.into_values().collect(),
            edges: edges.into_values().collect(),
        })
    }

    /// `symbol`: where is `<name>` defined (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the store read fails.
    pub fn symbol(&self, query: &SymbolQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.symbol_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    fn symbol_inner(&self, query: &SymbolQuery) -> Result<GraphResult> {
        crate::work::validate_query_bytes("graph target", &query.target)?;
        let started = std::time::Instant::now();
        let kinds: Vec<NodeKind> = query.kind.iter().copied().collect();
        self.validate_filters(&query.filters)?;
        let found = if let Some(node) = self.read_node(&NodeId::new(&query.target))? {
            if (kinds.is_empty() || kinds.contains(&node.kind))
                && self.passes_filters(&node)
                && self.work.borrow_mut().candidate()?
            {
                vec![Scored::new(node, 1.0)]
            } else {
                Vec::new()
            }
        } else {
            self.snapshot.metadata().find_filtered(
                &query.target,
                &kinds,
                &self.filters.borrow(),
                &mut self.work.borrow_mut(),
            )?
        };
        let mut nodes: Vec<SymbolHit> = found
            .into_iter()
            .map(|scored| SymbolHit::of(&scored.item))
            .collect();
        let candidates = nodes.len();
        let truncations = result_truncations(candidates, query.limit);
        nodes.truncate(effective_limit(query.limit) as usize);
        Ok(GraphResult {
            context: ResultContext::default(),
            truncations,
            stats: Stats {
                candidates: candidates as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
            approximation: Some(Approximation::default()),
            nodes,
            ..GraphResult::default()
        })
    }

    /// `refs`: every reference to the target (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn refs(&self, query: &RefQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.refs_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    /// Individual source references, preserving repeated sites and resolution evidence.
    /// # Errors
    /// On invalid/ambiguous targets, invalid filters, cancellation or store failure.
    pub fn occurrences(
        &self,
        query: &graph_search_types::occurrence::OccurrenceQuery,
    ) -> Result<graph_search_types::occurrence::OccurrenceResult> {
        use graph_search_types::occurrence::{OccurrenceBy, OccurrenceHit, OccurrenceResult};
        self.reset_work()?;
        if query.target.is_empty()
            || query.target.len() > graph_search_types::limits::MAX_QUERY_BYTES
        {
            return Err(Error::InvalidQuery(
                "occurrence target must contain 1..=8192 bytes".into(),
            ));
        }
        self.validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let key = match query.by {
            OccurrenceBy::Name => query.target.clone(),
            OccurrenceBy::Target | OccurrenceBy::Owner => {
                self.resolve_target(&query.target)?.to_string()
            }
        };
        let index = self.snapshot.occurrences();
        let (indexed_files, extracted_files) = index.coverage();
        let mut result = OccurrenceResult {
            indexed_files,
            extracted_files,
            ..OccurrenceResult::default()
        };
        let cap = effective_limit(query.limit) as usize;
        for &position in index.positions(query.by, &key) {
            if !self.work.borrow_mut().occurrence()? {
                break;
            }
            let (path, file, record) = index
                .record(self.snapshot.occurrence_files(), position)
                .ok_or_else(|| {
                    Error::Store("occurrence lookup does not match its generation".into())
                })?;
            if query.kind.is_some_and(|kind| record.kind != kind) {
                continue;
            }
            let Some(owner) = self.read_node(&record.owner)? else {
                continue;
            };
            if !self.passes_filters(&owner) {
                continue;
            }
            if result.items.len() == cap {
                result.truncations.push(Truncation::new(
                    TruncationKind::Results,
                    cap as u64,
                    "occurrence result cap reached; reference sites are partial",
                ));
                break;
            }
            result.items.push(OccurrenceHit {
                path: path.into(),
                source_hash: file.source_hash.clone(),
                occurrence: record.clone(),
            });
        }
        result.stats.elapsed_ms = ms_since(started);
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        crate::payload::fit_occurrences(&mut result)?;
        Ok(result)
    }

    fn refs_inner(&self, query: &RefQuery) -> Result<GraphResult> {
        self.validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let edges = self.read_edges(&target, &REFERENCE_KINDS, Direction::In)?;
        self.assemble_graph(&target, &edges, query.limit, &query.filters, started)
    }

    /// `callers`: direct or N-hop callers (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn callers(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.callers_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    fn callers_inner(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.traverse(&query.target, &[EdgeKind::Calls], Direction::In, query)
    }

    /// `callees`: what the target calls (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn callees(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.callees_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    fn callees_inner(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.traverse(&query.target, &[EdgeKind::Calls], Direction::Out, query)
    }

    /// `impact`: the blast radius — counts by depth and kind, plus top nodes
    /// (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    #[allow(clippy::too_many_lines)] // BFS rings and their ranked projection form one query.
    pub fn impact(&self, query: &TraversalQuery) -> Result<ImpactResult> {
        self.reset_work()?;
        let mut result = self.impact_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_impact(&mut result)?;
        Ok(result)
    }

    #[allow(clippy::too_many_lines)] // BFS rings and their ranked projection form one query.
    fn impact_inner(&self, query: &TraversalQuery) -> Result<ImpactResult> {
        self.validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let kinds = [EdgeKind::Calls, EdgeKind::References];

        let mut by_depth = Vec::new();
        let mut frontier = BTreeSet::from([target.clone()]);
        let mut visited = frontier.clone();
        let mut distances = BTreeMap::new();
        let mut cone_edges = BTreeMap::new();
        for depth in 1..=query.depth.clamp(1, MAX_HOPS_CEILING) {
            let mut next = BTreeSet::new();
            let mut ring = BTreeMap::new();
            for id in &frontier {
                for edge in self.read_edges(id, &kinds, Direction::In)? {
                    let from = edge.from.clone();
                    cone_edges.insert(edge.id.clone(), edge);
                    if visited.insert(from.clone())
                        && let Some(node) = self.read_node(&from)?
                    {
                        distances.insert(from.clone(), depth);
                        if self.passes_filters(&node) {
                            let count = ring.entry(node.kind).or_insert(0u64);
                            *count = count.saturating_add(1);
                        }
                        next.insert(from);
                    }
                }
            }
            by_depth.push(DepthCount {
                depth,
                total: ring.values().sum(),
                by_kind: ring,
            });
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        let mut degree = BTreeMap::new();
        for edge in cone_edges.values() {
            if let Some(to) = &edge.to {
                let count = degree.entry(to.clone()).or_insert(0u64);
                *count = count.saturating_add(1);
            }
        }
        let mut top = Vec::new();
        for (id, depth) in distances {
            if let Some(node) = self.read_node(&id)?
                && self.passes_filters(&node)
            {
                top.push((
                    depth,
                    degree.get(&id).copied().unwrap_or_default(),
                    SymbolHit::of(&node),
                ));
            }
        }
        top.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.cmp(&a.1))
                .then(a.2.path.cmp(&b.2.path))
                .then(a.2.start_line.cmp(&b.2.start_line))
                .then(a.2.id.cmp(&b.2.id))
        });
        let candidates = top.len();
        let truncations = result_truncations(candidates, query.limit);
        let nodes: Vec<SymbolHit> = top
            .into_iter()
            .take(effective_limit(query.limit) as usize)
            .map(|(_, _, n)| n)
            .collect();
        let kept: BTreeSet<&str> = nodes
            .iter()
            .map(|n| n.id.as_str())
            .chain(std::iter::once(target.as_str()))
            .collect();
        let mut edges: Vec<EdgeHit> = cone_edges
            .values()
            .filter(|e| {
                kept.contains(e.from.as_str())
                    && e.to.as_ref().is_some_and(|to| kept.contains(to.as_str()))
            })
            .map(EdgeHit::from_edge)
            .collect();
        edges.sort();
        Ok(ImpactResult {
            context: ResultContext::default(),
            by_depth,
            top: nodes,
            approximation: Some(Approximation {
                resolved: edges.len() as u64,
                ..Approximation::default()
            }),
            edges,
            truncations,
            stats: Stats {
                candidates: candidates as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
        })
    }

    /// `deps`: imports and imported-by for a file (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn deps(&self, query: &DepsQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.deps_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    fn deps_inner(&self, query: &DepsQuery) -> Result<GraphResult> {
        self.validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let target = self.resolve_file_target(&query.target)?;
        let dir = match query.direction {
            graph_search_types::Direction::Out => Direction::Out,
            graph_search_types::Direction::In => Direction::In,
            graph_search_types::Direction::Both => Direction::Both,
        };
        // File relationships: imports (all languages) plus the HTML
        // link kinds (`SPEC.md` §8.3, §7.3).
        let kinds = [
            EdgeKind::Imports,
            EdgeKind::LinksTo,
            EdgeKind::LoadsStylesheet,
        ];
        let edges = self.read_edges(&target, &kinds, dir)?;
        self.assemble_graph(&target, &edges, query.limit, &query.filters, started)
    }

    /// `neighbors`: adjacent nodes along chosen edge kinds (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn neighbors(&self, query: &NeighborsQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.neighbors_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    fn neighbors_inner(&self, query: &NeighborsQuery) -> Result<GraphResult> {
        self.validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let kinds: Vec<EdgeKind> = query.rel.iter().copied().collect();
        let subgraph = self.expand(
            std::slice::from_ref(&target),
            query.hops.clamp(1, MAX_HOPS_CEILING),
            &kinds,
            Direction::Both,
        )?;
        Ok(self.result_from_subgraph(&subgraph, query.limit, &query.filters, started))
    }

    /// `path`: the shortest path between two nodes (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When either endpoint does not resolve or the read fails.
    pub fn path(&self, query: &PathQuery) -> Result<GraphResult> {
        self.reset_work()?;
        let mut result = self.path_inner(query)?;
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        crate::payload::fit_graph(&mut result)?;
        Ok(result)
    }

    fn path_inner(&self, query: &PathQuery) -> Result<GraphResult> {
        crate::work::validate_query_bytes("path start", &query.from)?;
        crate::work::validate_query_bytes("path destination", &query.to)?;
        let started = std::time::Instant::now();
        let from = self.resolve_target(&query.from)?;
        let to = self.resolve_target(&query.to)?;
        if from == to {
            let nodes = self.read_node(&from)?.iter().map(SymbolHit::of).collect();
            return Ok(GraphResult {
                context: ResultContext::default(),
                nodes,
                approximation: Some(Approximation::default()),
                ..GraphResult::default()
            });
        }
        let max_hops = query.max_hops.clamp(1, MAX_HOPS_CEILING);

        // BFS by ring; the first arrival is the shortest path.
        let mut parent: BTreeMap<NodeId, (NodeId, graph_search_types::Edge)> = BTreeMap::new();
        let mut frontier: BTreeSet<NodeId> = BTreeSet::from([from.clone()]);
        let mut visited: BTreeSet<NodeId> = frontier.clone();
        let mut found = false;
        if !self.work.borrow_mut().node(&from)? {
            frontier.clear();
        }
        'bfs: for _ in 0..max_hops {
            let mut next = BTreeSet::new();
            for id in &frontier {
                for edge in self.read_edges(id, &[], Direction::Both)? {
                    let Some(other) = (match &edge.to {
                        Some(to) if to == id => Some(edge.from.clone()),
                        Some(to) => Some(to.clone()),
                        None => None,
                    }) else {
                        continue;
                    };
                    if visited.contains(&other) || !self.work.borrow_mut().node(&other)? {
                        continue;
                    }
                    parent.insert(other.clone(), (id.clone(), edge.clone()));
                    if other == to {
                        found = true;
                        visited.insert(other);
                        break 'bfs;
                    }
                    visited.insert(other.clone());
                    next.insert(other);
                }
            }
            frontier = next;
            if found || frontier.is_empty() {
                break;
            }
        }
        if !found {
            return Ok(GraphResult {
                context: ResultContext::default(),
                stats: Stats {
                    elapsed_ms: ms_since(started),
                    ..Stats::default()
                },
                approximation: Some(Approximation::default()),
                ..GraphResult::default()
            });
        }
        // Walk back from `to`.
        let mut chain: Vec<(NodeId, graph_search_types::Edge)> = Vec::new();
        let mut cursor = to.clone();
        while let Some((prev, edge)) = parent.get(&cursor) {
            chain.push((cursor.clone(), edge.clone()));
            cursor = prev.clone();
            if cursor == from {
                break;
            }
        }
        chain.reverse();
        let mut nodes: Vec<SymbolHit> = Vec::new();
        if let Some(start) = self.read_node(&from)? {
            nodes.push(SymbolHit::of(&start));
        }
        for (node_id, _) in &chain {
            if let Some(node) = self.read_node(node_id)? {
                nodes.push(SymbolHit::of(&node));
            }
        }
        let edge_hits: Vec<EdgeHit> = chain.iter().map(|(_, e)| EdgeHit::from_edge(e)).collect();
        Ok(GraphResult {
            context: ResultContext::default(),
            nodes,
            edges: edge_hits,
            approximation: Some(Approximation {
                resolved: chain.len() as u64,
                unresolved: 0,
                ..Approximation::default()
            }),
            stats: Stats {
                candidates: visited.len() as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
            ..GraphResult::default()
        })
    }

    /// `explore`: the one-call retrieval (`SPEC.md` §8.4).
    ///
    /// # Errors
    /// When the store read fails.
    pub fn explore(&self, query: &ExploreQuery, root: &Path) -> Result<ExploreResult> {
        self.explore_with_policy(query, root, &WalkPolicy::default())
    }

    /// Explore using the same walk policy as indexing and text search.
    pub fn explore_with_policy(
        &self,
        query: &ExploreQuery,
        root: &Path,
        policy: &WalkPolicy,
    ) -> Result<ExploreResult> {
        self.explore_with_context(query, root, policy, ResultContext::default())
    }

    /// Assembles evidence with the host's generation and freshness context.
    ///
    /// # Errors
    /// Propagates retrieval errors or a byte budget too small for metadata.
    #[allow(clippy::too_many_lines)]
    pub fn explore_with_context(
        &self,
        query: &ExploreQuery,
        root: &Path,
        policy: &WalkPolicy,
        mut context: graph_search_types::context::ResultContext,
    ) -> Result<ExploreResult> {
        self.reset_work()?;
        context.runtime_versions = Some(graph_search_types::context::RuntimeVersions::current());
        let mut sources = crate::source::SourceCache::new(SCAN_BYTES_CAP, policy.max_file_bytes);
        let started = std::time::Instant::now();
        self.validate_filters(&query.filters)?;
        let Seeds {
            plan,
            retrieval,
            mut coverage,
            nodes: mut seeds,
            mut evidence,
            match_lines,
            mut truncations,
            candidates,
            files_scanned,
        } = self.seed(query, root, policy, &mut sources, &context)?;
        if coverage.enumeration_complete.is_some() {
            coverage.quarantined_files = context.coverage.quarantined_files;
            crate::units::coverage(self.snapshot.source_files(), &mut coverage);
            context.coverage = coverage;
        }
        let hops = query.hops.clamp(1, MAX_HOPS_CEILING);
        let mut edges = self.connect(
            &mut seeds,
            hops,
            query.retrieval.graph_context.relations(),
            &mut truncations,
        )?;

        crate::packages::share_result_identities(
            &mut evidence,
            seeds.iter().map(|seed| &seed.item.id),
            &mut context,
        );

        // Impact: one-line blast radius for function/method seeds.
        let mut items: Vec<ExploreItem> = Vec::new();
        let mut total_bytes = 0usize;
        let mut byte_cap_hit = false;
        let max = explore_byte_cap(query.max_bytes);
        let mut metadata_bytes_total = 0usize;
        let mut minimum_context = context.clone();
        minimum_context.sources.clear();
        minimum_context.packages.clear();
        minimum_context.staleness.changed_paths.sort();
        minimum_context.staleness.changed_paths.dedup();
        minimum_context.staleness.changed = minimum_context.staleness.changed_paths.len() as u64;
        // This metadata survives fitting. Source identities, package tables,
        // graph edges and nonzero statistics can only increase the floor.
        let minimum_result_bytes = serde_json_len(&ExploreResult {
            plan: plan.clone(),
            context: minimum_context,
            items: Vec::new(),
            edges: Vec::new(),
            truncations: truncations.clone(),
            approximation: Some(Approximation::default()),
            stats: Stats::default(),
        });
        // Every useful primary has at least one line, a positive coordinate and
        // a full source hash. Compare serialized options so field encoding stays
        // consistent with the actual result, without allocating source text.
        let minimum_snippet_growth = serde_json_len(&Some(Snippet {
            source_hash: crate::hash::content_hash(b""),
            start_line: 1,
            lines: vec![String::new()],
        }))
        .saturating_sub(serde_json_len(&Option::<Snippet>::None));
        for scored in &seeds {
            self.work.borrow().check()?;
            let node = &scored.item;
            let impact = if query.retrieval.graph_context.includes_impact()
                && matches!(node.kind, NodeKind::Function | NodeKind::Method)
            {
                let incoming = self.read_edges(&node.id, &[EdgeKind::Calls], Direction::In)?;
                let direct = incoming
                    .iter()
                    .map(|e| &e.from)
                    .filter(|id| *id != &node.id)
                    .collect::<BTreeSet<_>>()
                    .len() as u64;
                let total = self.incoming_caller_count(&node.id, hops, &incoming)?;
                Some(ImpactSummary {
                    direct_callers: direct,
                    total_callers: total.max(direct),
                })
            } else {
                None
            };
            let body = evidence.get(&node.id);
            let mut item = ExploreItem {
                retrieval: retrieval.get(&node.id).cloned(),
                excerpts: Vec::new(),
                evidence: body.cloned(),
                node: SymbolHit::of(node),
                snippet: None,
                impact,
            };
            let metadata_bytes = serde_json_len(&item);
            if total_bytes.saturating_add(metadata_bytes) > max {
                byte_cap_hit = true;
                continue;
            }
            let materialize = query.context_lines > 0
                && total_bytes
                    .saturating_add(metadata_bytes)
                    .saturating_add(minimum_snippet_growth)
                    <= max
                && minimum_result_bytes
                    .saturating_add(metadata_bytes_total)
                    .saturating_add(metadata_bytes)
                    .saturating_add(items.len()) // Commas between admitted items.
                    .saturating_add(minimum_snippet_growth)
                    <= max;
            if materialize {
                let _ = sources.read_with_work(root, &node.path, &mut self.work.borrow_mut())?;
            } else if query.context_lines > 0 {
                byte_cap_hit = true;
            }
            let matches = if let Some(body) = body {
                sources.hash(&node.path) == Some(body.source_hash.as_str())
            } else {
                sources.matches(self.snapshot, node)?
            };
            item.snippet = if materialize && matches {
                sources.text(&node.path).and_then(|text| {
                    let mut anchor = node.clone();
                    if let Some(body) = body {
                        anchor.span = Some(graph_search_types::node::Span {
                            start_line: body.match_line,
                            end_line: body.match_line,
                            ..body.span
                        });
                    }
                    snippet_for(
                        &anchor,
                        query.context_lines,
                        text,
                        sources.hash(&node.path).unwrap_or_default(),
                    )
                })
            } else {
                None
            };
            context.sources.insert(
                node.path.clone(),
                sources.identity(self.snapshot, &node.path)?,
            );
            let mut estimated = serde_json_len(&item);
            if total_bytes.saturating_add(estimated) > max {
                byte_cap_hit = true;
                // A long source line must not erase an otherwise useful entity.
                item.snippet = None;
                estimated = serde_json_len(&item);
                if total_bytes.saturating_add(estimated) > max {
                    continue;
                }
            }
            total_bytes = total_bytes.saturating_add(estimated);
            metadata_bytes_total = metadata_bytes_total.saturating_add(metadata_bytes);
            items.push(item);
        }
        if byte_cap_hit {
            truncations.push(Truncation::new(
                TruncationKind::Bytes,
                explore_byte_cap(query.max_bytes) as u64,
                "payload byte cap omitted source excerpts or results",
            ));
        }

        // The honesty block, plus the unmatched-class count for HTML/CSS
        // workspaces (`SPEC.md` §7.3).
        let kept: BTreeSet<&str> = items.iter().map(|i| i.node.id.as_str()).collect();
        edges.retain(|e| {
            kept.contains(e.from.as_str()) && e.to.as_deref().is_some_and(|to| kept.contains(to))
        });
        let resolved = edges.iter().filter(|e| e.resolved).count() as u64;
        let approximation = Approximation {
            resolved,
            unresolved: (edges.len() as u64).saturating_sub(resolved),
            ..Approximation::default()
        };
        sources.add_read_coverage(&mut context.coverage);
        for (path, source) in &context.sources {
            if source
                .indexed_hash
                .as_ref()
                .zip(source.observed_hash.as_ref())
                .is_some_and(|(indexed, observed)| indexed != observed)
            {
                context.staleness.changed_paths.push(path.clone());
            }
        }
        context.staleness.changed_paths.sort();
        context.staleness.changed_paths.dedup();
        context.staleness.changed = context.staleness.changed_paths.len() as u64;
        let mut result = ExploreResult {
            plan,
            context,
            items,
            edges,
            truncations,
            approximation: Some(approximation),
            stats: Stats {
                candidates: candidates as u64,
                files_scanned,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
        };
        let mut primary_sources = crate::context_dedup::prepare(&mut result);
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        self.finish_edges(&mut result.edges, &mut result.truncations);
        fit_explore(&mut result, explore_byte_cap(query.max_bytes))?;
        primary_sources.retain(&result);
        if query.context_lines > 0 {
            crate::evidence::extend_prepared(
                &mut result,
                &sources,
                explore_byte_cap(query.max_bytes),
                &mut self.work.borrow_mut(),
                self.snapshot,
                &match_lines,
                &primary_sources.sources,
            )?;
        }
        self.finish_work(&mut result.stats, &mut result.truncations)?;
        result.stats.elapsed_ms = ms_since(started);
        fit_explore(&mut result, explore_byte_cap(query.max_bytes))?;
        Ok(result)
    }

    fn connect(
        &self,
        seeds: &mut Vec<Scored<Node>>,
        hops: u8,
        relations: &[EdgeKind],
        truncations: &mut Vec<Truncation>,
    ) -> Result<Vec<EdgeHit>> {
        // No pair exists to connect. Avoid spending the impact/context budget
        // enumerating a neighborhood that cannot contribute a connecting path.
        if seeds.len() < 2 || relations.is_empty() {
            return Ok(Vec::new());
        }
        let seed_ids: Vec<NodeId> = seeds.iter().map(|s| s.item.id.clone()).collect();
        let subgraph = self.expand(&seed_ids, hops, relations, Direction::Both)?;
        let allowed: BTreeSet<NodeId> = subgraph
            .nodes
            .iter()
            .filter(|n| self.passes_filters(n))
            .map(|n| n.id.clone())
            .collect();
        let seed_set: BTreeSet<NodeId> = seed_ids.iter().cloned().collect();
        let connections = crate::connections::ConnectionGraph::new(allowed, &subgraph.edges);
        let selected = connections.paths(&seed_ids, hops, &mut self.work.borrow_mut())?;
        let mut edges: Vec<EdgeHit> = selected
            .iter()
            .map(|i| EdgeHit::from_edge(&subgraph.edges[*i]))
            .collect();
        edges.sort();
        let bridge_ids: BTreeSet<NodeId> = selected
            .iter()
            .flat_map(|i| {
                let e = &subgraph.edges[*i];
                std::iter::once(e.from.clone()).chain(e.to.clone())
            })
            .filter(|id| !seed_set.contains(id))
            .collect();
        for node in subgraph.nodes {
            if bridge_ids.contains(&node.id) {
                seeds.push(Scored::new(node, 0.0));
            }
        }
        if seeds.len() > graph_search_types::limits::GRAPH_LIMIT_CEILING as usize {
            truncations.extend(result_truncations(
                seeds.len(),
                graph_search_types::limits::GRAPH_LIMIT_CEILING,
            ));
            seeds.truncate(graph_search_types::limits::GRAPH_LIMIT_CEILING as usize);
        }

        Ok(edges)
    }

    // The summary needs a count, not retained node/edge payloads. Preserve the
    // expansion's ordered frontiers, first-read admission and missing-node rules
    // so this also agrees when shared work limits admit only a graph prefix.
    fn incoming_caller_count(
        &self,
        seed: &NodeId,
        hops: u8,
        incoming: &[graph_search_types::Edge],
    ) -> Result<u64> {
        if self.read_node(seed)?.is_none() {
            return Ok(0);
        }
        let mut visited = BTreeSet::from([seed.clone()]);
        let mut frontier = visited.clone();
        let mut count = 0u64;
        for _ in 0..hops {
            let mut next = BTreeSet::new();
            for id in &frontier {
                let loaded;
                let edges = if id == seed {
                    incoming
                } else {
                    loaded = self.read_edges(id, &[EdgeKind::Calls], Direction::In)?;
                    &loaded
                };
                for edge in edges {
                    if edge.to.is_some()
                        && visited.insert(edge.from.clone())
                        && self.read_node(&edge.from)?.is_some()
                    {
                        count = count.saturating_add(1);
                        next.insert(edge.from.clone());
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        Ok(count)
    }

    /// Node and edge counts for `status` (`SPEC.md` §8.5).
    ///
    /// # Errors
    /// When the read fails.
    pub fn counts(&self) -> Result<graph_search_types::result::StoreCounts> {
        self.reset_work()?;
        let counts = self.snapshot.counts().clone();
        self.work.borrow().check()?;
        Ok(counts)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// Seeds ranked by name, qualified name, and path match, then by a
    /// bounded literal scan of candidate files (`SPEC.md` §8.4 step 1).
    #[allow(clippy::too_many_lines)]
    fn seed(
        &self,
        query: &ExploreQuery,
        root: &Path,
        policy: &WalkPolicy,
        sources: &mut crate::source::SourceCache,
        context: &ResultContext,
    ) -> Result<Seeds> {
        if let Some(predicate) =
            crate::positional::PositionalQuery::for_retrieval(&query.query, &query.retrieval)?
        {
            return self.seed_positional(query, root, policy, sources, context, &predicate);
        }
        if query.query.len() > graph_search_types::limits::MAX_QUERY_BYTES {
            return Err(Error::InvalidQuery(format!(
                "query exceeds {} bytes",
                graph_search_types::limits::MAX_QUERY_BYTES
            )));
        }
        let (effective_query, omitted_boilerplate) =
            crate::query_policy::apply(&query.query, &query.retrieval);
        let terms = match query.retrieval.analysis {
            graph_search_types::AnalysisMode::Split => crate::lexical::query_terms(effective_query),
            graph_search_types::AnalysisMode::Identifiers => {
                crate::analyzer::query_terms(effective_query)
            }
        };
        if terms.len() > graph_search_types::limits::MAX_QUERY_TERMS {
            return Err(Error::InvalidQuery(format!(
                "query exceeds {} distinct terms",
                graph_search_types::limits::MAX_QUERY_TERMS
            )));
        }
        let mut plan = RetrievalPlan {
            omitted_boilerplate,
            query: query.query.clone(),
            options: query.retrieval.clone(),
            terms: terms.clone(),
            routes: Vec::new(),
        };
        let k = if query.k == 0 {
            graph_search_types::limits::EXPLORE_DEFAULT_K
        } else {
            effective_limit(query.k)
        };
        let direct_id = query.retrieval.mode == ExploreMode::ExactId
            || (query.retrieval.mode == ExploreMode::Auto
                && (query.query.trim().starts_with("sym:")
                    || query.query.trim().starts_with("file:")));
        let fast_name =
            query.retrieval.mode == ExploreMode::Auto && query.retrieval.exact_fast_path;
        let navigation = if direct_id {
            plan.routes.push(RetrievalRoute::ExactId);
            let id = NodeId::new(query.query.trim());
            let node = self
                .snapshot
                .node_by_id(&id)?
                .filter(|node| self.passes_filters(node));
            let mut hits = Vec::new();
            if let Some(node) = node
                && self.work.borrow_mut().candidate()?
            {
                hits.push(Scored::new(node, 2.0));
            }
            Some(hits)
        } else if query.retrieval.mode == ExploreMode::ExactName || fast_name {
            plan.routes.push(RetrievalRoute::ExactName);
            let mut hits = self.snapshot.metadata().find_filtered(
                query.query.trim(),
                &[],
                &self.filters.borrow(),
                &mut self.work.borrow_mut(),
            )?;
            for hit in &mut hits {
                hit.score = 2.0;
            }
            if fast_name && hits.is_empty() {
                None
            } else {
                Some(hits)
            }
        } else if query.retrieval.mode == ExploreMode::NamePrefix {
            plan.routes.push(RetrievalRoute::NamePrefix);
            Some(self.snapshot.metadata().find_prefix(
                query.query.trim(),
                &self.filters.borrow(),
                &mut self.work.borrow_mut(),
            )?)
        } else if query.retrieval.mode == ExploreMode::PathGlob {
            plan.routes.push(RetrievalRoute::PathGlob);
            Some(self.snapshot.metadata().find_paths(
                query.query.trim(),
                &self.filters.borrow(),
                &mut self.work.borrow_mut(),
            )?)
        } else {
            None
        };
        if let Some(mut nodes) = navigation {
            let candidates = nodes.len();
            let truncations = result_truncations(candidates, k);
            nodes.truncate(k as usize);
            let retrieval = if query.retrieval.explain {
                nodes
                    .iter()
                    .map(|hit| {
                        (
                            hit.item.id.clone(),
                            RetrievalEvidence {
                                exact: !matches!(
                                    query.retrieval.mode,
                                    ExploreMode::PathGlob | ExploreMode::NamePrefix
                                ),
                                ..RetrievalEvidence::default()
                            },
                        )
                    })
                    .collect()
            } else {
                BTreeMap::new()
            };
            return Ok(Seeds {
                plan: query.retrieval.explain.then_some(plan),
                retrieval,
                coverage: graph_search_types::coverage::Coverage::default(),
                nodes,
                evidence: BTreeMap::new(),
                match_lines: BTreeMap::new(),
                truncations,
                candidates,
                files_scanned: 0,
            });
        }
        let automatic_ranking = query.retrieval.ranking == RankingStrategy::Auto;
        let ranking = if automatic_ranking {
            if query.query.split_whitespace().count() > 1 {
                RankingStrategy::Body
            } else {
                RankingStrategy::Fusion
            }
        } else {
            query.retrieval.ranking
        };
        let minimum = query.retrieval.term_match.minimum(terms.len());
        let exact = crate::lexical::exact_query(&query.query);
        if terms.is_empty() || minimum > terms.len() {
            return Ok(Seeds {
                plan: query.retrieval.explain.then_some(plan),
                retrieval: BTreeMap::new(),
                coverage: graph_search_types::coverage::Coverage::default(),
                nodes: Vec::new(),
                evidence: BTreeMap::new(),
                match_lines: BTreeMap::new(),
                truncations: Vec::new(),
                candidates: 0,
                files_scanned: 0,
            });
        }
        let mut truncations = Vec::new();
        let mut scanned_files = 0u64;
        // Candidate selection is deeper than final context selection. Retaining
        // at least k metadata candidates preserves the existing union's top k.
        let pool = (k as usize)
            .saturating_mul(graph_search_types::limits::METADATA_POOL_MULTIPLIER)
            .max(graph_search_types::limits::METADATA_POOL_MIN)
            .min(graph_search_types::limits::GRAPH_LIMIT_CEILING as usize);
        let mut retrieval = BTreeMap::new();
        let mut metadata = if ranking == RankingStrategy::Body {
            crate::metadata::MetadataCandidates {
                hits: Vec::new(),
                matched: 0,
            }
        } else {
            plan.routes.push(RetrievalRoute::Metadata);
            let mut metadata_work =
                self.work
                    .borrow()
                    .lexical_lane(if ranking == RankingStrategy::Metadata {
                        1
                    } else {
                        2
                    });
            let candidates = self.snapshot.metadata().search_top_with_policy(
                &terms,
                &exact,
                &self.filters.borrow(),
                &mut metadata_work,
                pool,
                minimum,
                query.retrieval.analysis,
                query.retrieval.normalization,
            )?;
            self.work.borrow_mut().absorb_lexical(metadata_work);
            candidates
        };
        if query.retrieval.explain {
            for (rank, hit) in metadata.hits.iter().enumerate() {
                retrieval.insert(
                    hit.item.id.clone(),
                    RetrievalEvidence {
                        metadata_rank: Some(
                            u32::try_from(rank).unwrap_or(u32::MAX).saturating_add(1),
                        ),
                        body_rank: None,
                        exact: hit.score >= 2.0,
                    },
                );
            }
        }
        if ranking == RankingStrategy::Metadata {
            let truncations = result_truncations(metadata.matched, k);
            let nodes = select_diverse(metadata.hits, k as usize, query.retrieval.per_file);
            return Ok(Seeds {
                plan: query.retrieval.explain.then_some(plan),
                retrieval,
                coverage: graph_search_types::coverage::Coverage::default(),
                nodes,
                evidence: BTreeMap::new(),
                match_lines: BTreeMap::new(),
                truncations,
                candidates: metadata.matched,
                files_scanned: 0,
            });
        }
        plan.routes.push(RetrievalRoute::Body);
        let search_root = crate::walk::resolve_search_root(root, None)?;
        let report =
            crate::walk::walk_report_with_work(&search_root, policy, &mut self.work.borrow_mut())?;
        let coverage = report.coverage;
        truncations.extend(coverage.truncations.clone());
        let changed: BTreeSet<_> = context.staleness.changed_paths.iter().cloned().collect();
        let unchecked =
            context.freshness == graph_search_types::context::FreshnessMethod::Unchecked;
        let representation_changed = context
            .indexed_versions
            .is_some_and(|versions| !versions.retrieval_is_current());
        let mut excluded = changed;
        if representation_changed {
            excluded.extend(self.snapshot.source_files().keys().cloned());
        }
        if query.retrieval.analysis == graph_search_types::AnalysisMode::Identifiers {
            excluded.extend(
                self.snapshot
                    .source_files()
                    .iter()
                    .filter(|(_, file)| file.version < 2)
                    .map(|(path, _)| path.clone()),
            );
        }
        let mut overlay = BTreeMap::new();
        let mut scanned_bytes = 0u64;
        let mut eligible = BTreeSet::new();
        for entry in &report.entries {
            self.work.borrow().check()?;
            if !self.filters.borrow().matches(&entry.rel, entry.language) {
                continue;
            }
            eligible.insert(entry.rel.clone());
            if !unchecked
                && !excluded.contains(&entry.rel)
                && self.snapshot.source_files().contains_key(&entry.rel)
            {
                continue;
            }
            // Known-suspect facts stay masked even if the overlay runs out of budget.
            // Unchecked but unobserved facts retain indexed provenance.
            if scanned_files >= SCAN_FILE_CAP as u64 {
                if !truncations.iter().any(|t| t.kind == TruncationKind::Files) {
                    truncations.push(Truncation::new(
                        TruncationKind::Files,
                        SCAN_FILE_CAP as u64,
                        "live body overlay reached its file cap",
                    ));
                }
                continue;
            }
            if scanned_bytes.saturating_add(entry.size) > SCAN_BYTES_CAP {
                if !truncations.iter().any(|t| t.kind == TruncationKind::Bytes) {
                    truncations.push(Truncation::new(
                        TruncationKind::Bytes,
                        SCAN_BYTES_CAP,
                        "live body overlay reached its byte cap",
                    ));
                }
                continue;
            }
            scanned_files = scanned_files.saturating_add(1);
            let Some(text) =
                sources.read_with_work(root, &entry.rel, &mut self.work.borrow_mut())?
            else {
                excluded.insert(entry.rel.clone());
                continue;
            };
            scanned_bytes = scanned_bytes.saturating_add(text.len() as u64);
            let hash = crate::hash::content_hash(text.as_bytes());
            if self
                .snapshot
                .source_files()
                .get(&entry.rel)
                .is_some_and(|file| {
                    !representation_changed
                        && file.source_hash == hash
                        && (query.retrieval.analysis == graph_search_types::AnalysisMode::Split
                            || file.version >= 2)
                })
            {
                excluded.remove(&entry.rel);
                continue;
            }
            excluded.insert(entry.rel.clone());
            let facts = crate::units::extract(
                &entry.rel,
                text,
                &hash,
                entry
                    .language
                    .unwrap_or(graph_search_types::Language::Unknown),
                &[],
            );
            overlay.insert(entry.rel.clone(), facts);
        }
        if coverage.enumeration_complete == Some(true) {
            excluded.extend(
                self.snapshot
                    .source_files()
                    .keys()
                    .filter(|path| !eligible.contains(*path))
                    .cloned(),
            );
        }
        let mut overlay_work = self.work.borrow().lexical_lane(2);
        let overlay_index = crate::body::BodyIndex::new(&overlay);
        let language = |path: &str| policy.language_for(std::path::Path::new(path));
        let (live, live_count) = overlay_index.search_with_analysis(
            &terms,
            &self.filters.borrow(),
            language,
            &BTreeSet::new(),
            &mut overlay_work,
            pool,
            minimum,
            query.retrieval.analysis,
        )?;
        self.work.borrow_mut().absorb_lexical(overlay_work);
        let (indexed, indexed_count) = self.snapshot.body().search_with_analysis(
            &terms,
            &self.filters.borrow(),
            |path| self.snapshot.metadata().language(path),
            &excluded,
            &mut self.work.borrow_mut(),
            pool,
            minimum,
            query.retrieval.analysis,
        )?;
        let mut body_hits: Vec<_> = indexed
            .into_iter()
            .enumerate()
            .map(|(rank, mut hit)| {
                hit.score = rank_score(rank);
                (hit, false)
            })
            .chain(live.into_iter().enumerate().map(|(rank, mut hit)| {
                hit.score = rank_score(rank);
                (hit, true)
            }))
            .collect();
        body_hits.sort_by(|a, b| {
            b.0.score
                .total_cmp(&a.0.score)
                .then(a.0.path.cmp(&b.0.path))
                .then(a.0.unit.cmp(&b.0.unit))
        });
        if automatic_ranking && ranking == RankingStrategy::Body && body_hits.is_empty() {
            plan.routes.push(RetrievalRoute::Metadata);
            metadata = self.snapshot.metadata().search_top_with_policy(
                &terms,
                &exact,
                &self.filters.borrow(),
                &mut self.work.borrow_mut(),
                pool,
                minimum,
                query.retrieval.analysis,
                query.retrieval.normalization,
            )?;
            if query.retrieval.explain {
                for (rank, hit) in metadata.hits.iter().enumerate() {
                    retrieval.insert(
                        hit.item.id.clone(),
                        RetrievalEvidence {
                            metadata_rank: Some(
                                u32::try_from(rank).unwrap_or(u32::MAX).saturating_add(1),
                            ),
                            body_rank: None,
                            exact: hit.score >= 2.0,
                        },
                    );
                }
            }
        }
        let mut evidence = BTreeMap::new();
        let mut match_lines = BTreeMap::new();
        let mut fused: BTreeMap<NodeId, Scored<Node>> = BTreeMap::new();
        for (rank, mut hit) in metadata.hits.into_iter().enumerate() {
            // Exact names are a separate priority tier. Other channels combine by rank.
            hit.score = if hit.score >= 2.0 {
                2.0
            } else {
                rank_score(rank)
            };
            fused.insert(hit.item.id.clone(), hit);
        }
        let mut body_rank = 0usize;
        for (hit, live) in body_hits {
            let facts = if live {
                &overlay[&hit.path]
            } else {
                &self.snapshot.source_files()[&hit.path]
            };
            let unit = &facts.units[hit.unit];
            let owner = if live { None } else { unit.owner.as_ref() };
            let associated = unit
                .documentation
                .as_ref()
                .and_then(|doc| doc.documented_symbol.as_ref())
                .filter(|_| !live);
            let node = associated
                .or(owner)
                .map(|id| self.snapshot.node_by_id(id))
                .transpose()?
                .flatten()
                .unwrap_or_else(|| Node {
                    id: NodeId::file(&hit.path),
                    kind: NodeKind::File,
                    path: hit.path.clone(),
                    language: language(&hit.path),
                    content_hash: Some(facts.source_hash.clone()),
                    span: Some(unit.span),
                    ..Node::default()
                });
            if evidence.contains_key(&node.id) {
                continue;
            }
            let lines = crate::analyzer::matching_lines(
                unit,
                &terms,
                query.retrieval.analysis == graph_search_types::AnalysisMode::Identifiers,
            )?;
            let match_line = lines
                .iter()
                .max_by(|a, b| a.1.count_ones().cmp(&b.1.count_ones()).then(b.0.cmp(a.0)))
                .map_or(hit.line, |(&line, _)| line);
            let mut regions = vec![crate::evidence::MatchRegion {
                span: unit.span,
                lines,
                headings: unit.headings.clone(),
                fence: unit.fence,
                table: unit.table,
            }];
            for &ordinal in &hit.complementary_units {
                self.work.borrow().check()?;
                let other = &facts.units[ordinal];
                regions.push(crate::evidence::MatchRegion {
                    span: other.span,
                    lines: crate::analyzer::matching_lines(
                        other,
                        &terms,
                        query.retrieval.analysis == graph_search_types::AnalysisMode::Identifiers,
                    )?,
                    headings: other.headings.clone(),
                    fence: other.fence,
                    table: other.table,
                });
            }
            match_lines.insert(
                node.id.clone(),
                crate::evidence::MatchContext {
                    regions,
                    omitted_regions: hit.omitted_regions,
                },
            );
            evidence.insert(
                node.id.clone(),
                graph_search_types::source::SourceEvidence {
                    span: unit.span,
                    kind: unit.kind,
                    owner: owner.cloned(),
                    documentation: unit.documentation.clone().filter(|_| !live),
                    match_line,
                    source_hash: facts.source_hash.clone(),
                    package: facts.package.clone().filter(|_| !live),
                    package_ref: None,
                    package_scope_incomplete: !live && facts.package_scope_incomplete,
                    live,
                },
            );
            if query.retrieval.explain {
                retrieval
                    .entry(node.id.clone())
                    .or_insert_with(RetrievalEvidence::default)
                    .body_rank = Some(
                    u32::try_from(body_rank)
                        .unwrap_or(u32::MAX)
                        .saturating_add(1),
                );
            }
            let score = rank_score(body_rank);
            body_rank = body_rank.saturating_add(1);
            fused
                .entry(node.id.clone())
                .and_modify(|old| old.score += score)
                .or_insert_with(|| Scored::new(node, score));
        }
        let mut scored: Vec<_> = fused.into_values().collect();
        scored.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(a.item.path.cmp(&b.item.path))
                .then(a.item.span.cmp(&b.item.span))
                .then(a.item.id.cmp(&b.item.id))
        });
        let candidates = metadata
            .matched
            .saturating_add(live_count)
            .saturating_add(indexed_count);
        truncations.extend(result_truncations(candidates, k));
        let scored = select_diverse(scored, k as usize, query.retrieval.per_file);
        Ok(Seeds {
            plan: query.retrieval.explain.then_some(plan),
            retrieval,
            coverage,
            nodes: scored,
            evidence,
            match_lines,
            truncations,
            candidates,
            files_scanned: scanned_files,
        })
    }

    /// Resolves a `<name|id>` argument: exact id first, then name.
    fn resolve_target(&self, target: &str) -> Result<NodeId> {
        crate::work::validate_query_bytes("graph target", target)?;
        if target.starts_with("file:") || target.starts_with("sym:") {
            let id = NodeId::new(target);
            if self.snapshot.node_by_id(&id)?.is_some() {
                return Ok(id);
            }
        }
        let kinds: Vec<NodeKind> = Vec::new();
        let mut found = self.snapshot.find_by_name(target, &kinds, 2)?;
        if found.len() > 1 {
            return Err(Error::Ambiguous(target.to_owned()));
        }
        if found.is_empty() {
            // A file path names its file node.
            let file_id = NodeId::file(target);
            if self.snapshot.node_by_id(&file_id)?.is_some() {
                return Ok(file_id);
            }
            return Err(Error::NotFound(target.to_owned()));
        }
        Ok(found.remove(0).item.id)
    }

    /// Resolves a `<path|id>` argument for `deps`: a file path or id.
    fn resolve_file_target(&self, target: &str) -> Result<NodeId> {
        crate::work::validate_query_bytes("graph target", target)?;
        let cleaned = target.trim_start_matches("file:");
        let file_id = NodeId::file(cleaned);
        if self.snapshot.node_by_id(&file_id)?.is_some() {
            return Ok(file_id);
        }
        self.resolve_target(target)
    }

    fn validate_filters(&self, filters: &graph_search_types::query::GraphFilters) -> Result<()> {
        *self.filters.borrow_mut() = crate::metadata::CompiledFilters::new(filters)?;
        Ok(())
    }

    fn passes_filters(&self, node: &Node) -> bool {
        self.filters
            .borrow()
            .matches(&node.path, self.snapshot.metadata().language(&node.path))
    }

    fn traverse(
        &self,
        target: &str,
        kinds: &[EdgeKind],
        dir: Direction,
        query: &TraversalQuery,
    ) -> Result<GraphResult> {
        let started = std::time::Instant::now();
        self.validate_filters(&query.filters)?;
        let target_id = self.resolve_target(target)?;
        let subgraph = self.expand(
            std::slice::from_ref(&target_id),
            query.depth.clamp(1, MAX_HOPS_CEILING),
            kinds,
            dir,
        )?;
        Ok(self.result_from_subgraph(&subgraph, query.limit, &query.filters, started))
    }

    fn assemble_graph(
        &self,
        target: &NodeId,
        edges: &[graph_search_types::Edge],
        limit: u32,
        filters: &graph_search_types::query::GraphFilters,
        started: std::time::Instant,
    ) -> Result<GraphResult> {
        let mut nodes = BTreeMap::new();
        for id in std::iter::once(target).chain(
            edges
                .iter()
                .flat_map(|e| std::iter::once(&e.from).chain(e.to.iter())),
        ) {
            if let Some(node) = self.read_node(id)? {
                nodes.insert(id.clone(), node);
            }
        }
        Ok(self.result_from_subgraph(
            &graph_search_types::Subgraph {
                nodes: nodes.into_values().collect(),
                edges: edges.to_vec(),
            },
            limit,
            filters,
            started,
        ))
    }

    fn result_from_subgraph(
        &self,
        subgraph: &graph_search_types::Subgraph,
        limit: u32,
        _filters: &graph_search_types::query::GraphFilters,
        started: std::time::Instant,
    ) -> GraphResult {
        let allowed: BTreeSet<String> = subgraph
            .nodes
            .iter()
            .filter(|n| self.passes_filters(n))
            .map(|n| n.id.to_string())
            .collect();
        let mut hits: Vec<SymbolHit> = subgraph
            .nodes
            .iter()
            .filter(|n| allowed.contains(n.id.as_str()))
            .map(SymbolHit::of)
            .collect();
        hits.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.start_line.cmp(&b.start_line))
                .then(a.id.cmp(&b.id))
        });
        let candidates = hits.len();
        let truncations = result_truncations(candidates, limit);
        hits.truncate(effective_limit(limit) as usize);
        let kept: BTreeSet<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        let mut edges: Vec<EdgeHit> = subgraph
            .edges
            .iter()
            .filter(|e| {
                allowed.contains(e.from.as_str())
                    && e.to.as_ref().is_none_or(|to| allowed.contains(to.as_str()))
                    && (kept.contains(e.from.as_str())
                        || e.to.as_ref().is_some_and(|to| kept.contains(to.as_str())))
            })
            .map(EdgeHit::from_edge)
            .collect();
        edges.sort();
        let resolved = edges.iter().filter(|e| e.resolved).count() as u64;
        let unresolved = (edges.len() as u64).saturating_sub(resolved);
        GraphResult {
            context: ResultContext::default(),
            nodes: hits,
            edges,
            truncations,
            approximation: Some(Approximation {
                resolved,
                unresolved,
                ..Approximation::default()
            }),
            stats: Stats {
                candidates: candidates as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
        }
    }
}

/// Reads a bounded excerpt around the definition from the file on disk
/// (`SPEC.md` §8.4 step 2, §9.3).
fn snippet_for(node: &Node, context_lines: u32, text: &str, source_hash: &str) -> Option<Snippet> {
    if context_lines == 0 {
        return None;
    }
    // Public structs/deserialization can bypass the builder. Keep the anchor
    // in the bounded window even for an arbitrarily large requested radius.
    let context_lines =
        context_lines.min(graph_search_types::limits::MAX_SNIPPET_LINES.div_ceil(2));
    let span = node.span?;
    let lines: Vec<&str> = text.lines().collect();
    let start = span
        .start_line
        .saturating_sub(1)
        .saturating_sub(context_lines.saturating_sub(1)) as usize;
    let end_base = usize::try_from(span.start_line)
        .unwrap_or(usize::MAX)
        .saturating_add(context_lines as usize);
    let end = end_base
        .min(lines.len())
        .min(start.saturating_add(graph_search_types::limits::MAX_SNIPPET_LINES as usize));
    if start >= lines.len() {
        return None;
    }
    Some(Snippet {
        source_hash: source_hash.to_owned(),
        start_line: u32::try_from(start).unwrap_or(1).saturating_add(1),
        lines: lines[start..end]
            .iter()
            .map(|line| (*line).to_owned())
            .collect(),
    })
}

fn ms_since(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn serde_json_len(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

const _: () = {
    // Keep the default depth referenced so the constant documents the CLI
    // default without a magic number there.
    let _ = DEFAULT_TRAVERSAL_DEPTH;
    let _ = GRAPH_DEFAULT_LIMIT;
};

fn result_truncations(candidates: usize, limit: u32) -> Vec<Truncation> {
    let cap = effective_limit(limit);
    if candidates > cap as usize {
        vec![Truncation::new(
            TruncationKind::Results,
            u64::from(cap),
            "matching nodes exceeded the result cap",
        )]
    } else {
        Vec::new()
    }
}

fn explore_byte_cap(requested: u32) -> usize {
    if requested == 0 {
        graph_search_types::limits::MAX_TOTAL_BYTES
    } else {
        (requested as usize).min(graph_search_types::limits::MAX_TOTAL_BYTES)
    }
}
fn fit_explore(result: &mut ExploreResult, cap: usize) -> Result<()> {
    loop {
        result
            .context
            .sources
            .retain(|path, _| result.items.iter().any(|item| &item.node.path == path));
        result.context.retain_packages(
            result
                .items
                .iter()
                .filter_map(|item| item.evidence.as_ref()?.package_ref.as_deref()),
        );
        let resolved = result.edges.iter().filter(|e| e.resolved).count() as u64;
        result.approximation = Some(Approximation {
            resolved,
            unresolved: (result.edges.len() as u64).saturating_sub(resolved),
            ..Approximation::default()
        });
        if serde_json::to_vec(result).map_or(usize::MAX, |v| v.len()) <= cap {
            return Ok(());
        }
        if !result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes && t.cap == cap as u64)
        {
            result.truncations.push(Truncation::new(
                TruncationKind::Bytes,
                cap as u64,
                "serialized explore result exceeded its byte cap",
            ));
        }
        if let Some(item) = result
            .items
            .iter_mut()
            .rev()
            .find(|item| !item.excerpts.is_empty())
        {
            item.excerpts.pop();
            continue;
        }
        if result.edges.pop().is_some() {
            continue;
        }
        if let Some(item) = result
            .items
            .iter_mut()
            .rev()
            .find(|item| item.snippet.is_some())
        {
            item.snippet = None;
            continue;
        }
        if result.items.pop().is_none() {
            return Err(Error::ResultBudget(cap));
        }
    }
}

// Candidate pools are clamped below u16::MAX; this conversion is lossless.
fn rank_score(rank: usize) -> f32 {
    1.0 / (61.0 + f32::from(u16::try_from(rank).unwrap_or(u16::MAX)))
}

// Diversity is a soft first pass; exact names keep priority and deferred hits
// fill unused slots. Zero explicitly disables diversification for ablations.
fn select_diverse(scored: Vec<Scored<Node>>, k: usize, per_file: u16) -> Vec<Scored<Node>> {
    let mut selected = Vec::new();
    let mut deferred = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for hit in scored {
        let count = counts.entry(hit.item.path.clone()).or_default();
        if per_file == 0 || hit.score >= 2.0 || *count < usize::from(per_file) {
            *count = count.saturating_add(1);
            selected.push(hit);
        } else {
            deferred.push(hit);
        }
    }
    selected.extend(deferred);
    selected.truncate(k);
    selected
}

#[cfg(test)]
mod caller_count_tests {
    use super::*;
    use crate::{GraphStore, memory::MemoryStore, work::WorkLimits};
    use graph_search_types::Edge;

    #[test]
    fn counts_and_work_match_materialized_cones_for_all_three_node_graphs() {
        let ids: Vec<_> = (0..3)
            .map(|n| NodeId::symbol("src/a.rs", NodeKind::Function, &format!("n{n}"), None))
            .collect();
        for mask in 0u16..512 {
            let mut batch = crate::conformance::fixture_batch();
            batch.upserts.truncate(1);
            let prototype = batch.upserts[0].symbols[0].clone();
            batch.upserts[0].symbols = ids
                .iter()
                .enumerate()
                .map(|(n, id)| Node {
                    id: id.clone(),
                    name: Some(format!("n{n}")),
                    qualified_name: Some(format!("n{n}")),
                    ..prototype.clone()
                })
                .collect();
            batch.upserts[0].edges.clear();
            for (from, from_id) in ids.iter().enumerate() {
                for (to, to_id) in ids.iter().enumerate() {
                    if mask & (1 << (from * 3 + to)) != 0 {
                        batch.upserts[0].edges.push(Edge::resolved(
                            from_id,
                            EdgeKind::Calls,
                            to_id,
                            "target",
                            Some("src/a.rs"),
                            Some(1),
                        ));
                    }
                }
            }
            batch.upserts[0].edges.push(Edge::resolved(
                &ids[0],
                EdgeKind::References,
                &ids[1],
                "other relation",
                Some("src/a.rs"),
                Some(1),
            ));
            batch.upserts[0].edges.push(Edge::dangling(
                &ids[0],
                EdgeKind::Calls,
                "unresolved",
                Some("src/a.rs"),
                Some(1),
            ));
            let mut store = MemoryStore::new();
            store.apply(batch).unwrap();
            let snapshot = store.snapshot().unwrap();
            for seed in ids.iter().chain(std::iter::once(&NodeId::new("missing"))) {
                for hops in 0..=4 {
                    for (nodes, edges) in [(0, 0), (1, 100), (100, 1), (2, 3), (100, 100)] {
                        let limits = WorkLimits {
                            nodes,
                            edges,
                            ..Default::default()
                        };
                        let control =
                            QueryEngine::with_work_limits(snapshot.as_ref(), limits.clone());
                        let candidate = QueryEngine::with_work_limits(snapshot.as_ref(), limits);
                        // A prior phase can have populated an incident neighborhood.
                        if mask & 1 != 0 {
                            control.read_edges(&ids[1], &[], Direction::Both).unwrap();
                            candidate.read_edges(&ids[1], &[], Direction::Both).unwrap();
                        }
                        let incoming = control
                            .read_edges(seed, &[EdgeKind::Calls], Direction::In)
                            .unwrap();
                        let expected = control
                            .expand_with_seed_edges(
                                std::slice::from_ref(seed),
                                hops,
                                &[EdgeKind::Calls],
                                Direction::In,
                                Some((seed, &incoming)),
                            )
                            .unwrap();
                        let incoming = candidate
                            .read_edges(seed, &[EdgeKind::Calls], Direction::In)
                            .unwrap();
                        let actual = candidate
                            .incoming_caller_count(seed, hops, &incoming)
                            .unwrap();
                        assert_eq!(
                            actual,
                            expected
                                .nodes
                                .iter()
                                .filter(|node| &node.id != seed)
                                .count() as u64,
                            "mask={mask} seed={seed} hops={hops} limits={nodes}/{edges}"
                        );
                        assert_eq!(
                            candidate.work.borrow().report(),
                            control.work.borrow().report()
                        );
                    }
                }
            }
        }
    }
}
