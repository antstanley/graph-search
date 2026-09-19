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
use graph_search_types::{NodeId, Scored};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The edge kinds a `refs` answer counts as references (`SPEC.md` §8.3).
pub const REFERENCE_KINDS: [EdgeKind; 3] =
    [EdgeKind::Calls, EdgeKind::References, EdgeKind::TypeUses];

/// How many files the explore literal scan may open.
pub const SCAN_FILE_CAP: usize = 512;

/// How many bytes the explore literal scan may read.
pub const SCAN_BYTES_CAP: u64 = 8 * 1024 * 1024;

/// Scores one node against the terms: the best single match wins, extra
/// matching terms add a small bonus. Summing would let a file that merely
/// mentions every word outrank an exact name hit.
#[allow(clippy::cast_precision_loss)] // small bounded counts; exactness irrelevant to ranking
fn score_against(node: &Node, terms: &[String]) -> f32 {
    let name = node
        .name
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let qualified = node
        .qualified_name
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let path = node.path.to_ascii_lowercase();
    let mut best = 0.0f32;
    let mut matches = 0u32;
    for term in terms {
        let term_score = if name == *term {
            1.0
        } else if name.contains(term.as_str()) {
            0.7
        } else if qualified.contains(term.as_str()) {
            0.5
        } else if path.contains(term.as_str()) {
            0.4
        } else {
            0.0
        };
        if term_score > 0.0 {
            matches = matches.saturating_add(1);
        }
        if term_score > best {
            best = term_score;
        }
    }
    (best + 0.1 * matches.saturating_sub(1).min(9) as f32).min(1.0)
}

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
    nodes: Vec<Scored<Node>>,
    truncations: Vec<Truncation>,
    candidates: usize,
    files_scanned: u64,
}

/// The read API over one snapshot.
pub struct QueryEngine<'a> {
    snapshot: &'a dyn GraphSnapshot,
}

impl<'a> QueryEngine<'a> {
    /// An engine over `snapshot`.
    #[must_use]
    pub const fn new(snapshot: &'a dyn GraphSnapshot) -> Self {
        Self { snapshot }
    }

    /// `symbol`: where is `<name>` defined (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the store read fails.
    pub fn symbol(&self, query: &SymbolQuery) -> Result<GraphResult> {
        let started = std::time::Instant::now();
        let kinds: Vec<NodeKind> = query.kind.iter().copied().collect();
        validate_filters(&query.filters)?;
        let found = if let Some(node) = self.snapshot.node_by_id(&NodeId::new(&query.target))? {
            if kinds.is_empty() || kinds.contains(&node.kind) {
                vec![Scored::new(node, 1.0)]
            } else {
                Vec::new()
            }
        } else {
            self.snapshot
                .find_by_name(&query.target, &kinds, usize::MAX)?
        };
        let mut nodes: Vec<SymbolHit> = found
            .into_iter()
            .filter(|scored| {
                self.passes_filters(
                    &scored.item,
                    query.filters.lang,
                    query.filters.path_glob.as_deref(),
                )
            })
            .map(|scored| SymbolHit::of(&scored.item))
            .collect();
        let candidates = nodes.len();
        let truncations = result_truncations(candidates, query.limit);
        nodes.truncate(effective_limit(query.limit) as usize);
        Ok(GraphResult {
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
        validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let edges = self
            .snapshot
            .edges_from(&target, &REFERENCE_KINDS, Direction::In)?;
        self.assemble_graph(&target, &edges, query.limit, &query.filters, started)
    }

    /// `callers`: direct or N-hop callers (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn callers(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.traverse(&query.target, &[EdgeKind::Calls], Direction::In, query)
    }

    /// `callees`: what the target calls (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn callees(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.traverse(&query.target, &[EdgeKind::Calls], Direction::Out, query)
    }

    /// `impact`: the blast radius — counts by depth and kind, plus top nodes
    /// (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    #[allow(clippy::too_many_lines)] // BFS rings and their ranked projection form one query.
    pub fn impact(&self, query: &TraversalQuery) -> Result<ImpactResult> {
        validate_filters(&query.filters)?;
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
                for edge in self.snapshot.edges_from(id, &kinds, Direction::In)? {
                    let from = edge.from.clone();
                    cone_edges.insert(edge.id.clone(), edge);
                    if visited.insert(from.clone())
                        && let Some(node) = self.snapshot.node_by_id(&from)?
                    {
                        distances.insert(from.clone(), depth);
                        if self.passes_filters(
                            &node,
                            query.filters.lang,
                            query.filters.path_glob.as_deref(),
                        ) {
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
            if let Some(node) = self.snapshot.node_by_id(&id)?
                && self.passes_filters(
                    &node,
                    query.filters.lang,
                    query.filters.path_glob.as_deref(),
                )
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
        validate_filters(&query.filters)?;
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
        let edges = self.snapshot.edges_from(&target, &kinds, dir)?;
        self.assemble_graph(&target, &edges, query.limit, &query.filters, started)
    }

    /// `neighbors`: adjacent nodes along chosen edge kinds (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn neighbors(&self, query: &NeighborsQuery) -> Result<GraphResult> {
        validate_filters(&query.filters)?;
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let kinds: Vec<EdgeKind> = query.rel.iter().copied().collect();
        let subgraph = self.snapshot.expand(
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
        let started = std::time::Instant::now();
        let from = self.resolve_target(&query.from)?;
        let to = self.resolve_target(&query.to)?;
        if from == to {
            let nodes = self
                .snapshot
                .node_by_id(&from)?
                .iter()
                .map(SymbolHit::of)
                .collect();
            return Ok(GraphResult {
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
        for _ in 0..max_hops {
            let mut next = BTreeSet::new();
            for id in &frontier {
                for edge in self.snapshot.edges_from(id, &[], Direction::Both)? {
                    let Some(other) = (match &edge.to {
                        Some(to) if to == id => Some(edge.from.clone()),
                        Some(to) => Some(to.clone()),
                        None => None,
                    }) else {
                        continue;
                    };
                    if visited.contains(&other) {
                        continue;
                    }
                    parent.insert(other.clone(), (id.clone(), edge.clone()));
                    if other == to {
                        found = true;
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
        if let Some(start) = self.snapshot.node_by_id(&from)? {
            nodes.push(SymbolHit::of(&start));
        }
        for (node_id, _) in &chain {
            if let Some(node) = self.snapshot.node_by_id(node_id)? {
                nodes.push(SymbolHit::of(&node));
            }
        }
        let edge_hits: Vec<EdgeHit> = chain.iter().map(|(_, e)| EdgeHit::from_edge(e)).collect();
        Ok(GraphResult {
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
        let started = std::time::Instant::now();
        validate_filters(&query.filters)?;
        let Seeds {
            nodes: mut seeds,
            mut truncations,
            candidates,
            files_scanned,
        } = self.seed(query, root, policy)?;
        let hops = query.hops.clamp(1, MAX_HOPS_CEILING);
        let mut edges = self.connect(&mut seeds, query, hops, &mut truncations)?;

        // Impact: one-line blast radius for function/method seeds.
        let mut items: Vec<ExploreItem> = Vec::new();
        let mut total_bytes = 0usize;
        let mut byte_cap_hit = false;
        for scored in &seeds {
            let node = &scored.item;
            let impact = if matches!(node.kind, NodeKind::Function | NodeKind::Method) {
                let direct = self
                    .snapshot
                    .edges_from(&node.id, &[EdgeKind::Calls], Direction::In)?
                    .into_iter()
                    .map(|e| e.from)
                    .filter(|id| id != &node.id)
                    .collect::<BTreeSet<_>>()
                    .len() as u64;
                let cone = self.snapshot.expand(
                    std::slice::from_ref(&node.id),
                    hops,
                    &[EdgeKind::Calls],
                    Direction::In,
                )?;
                let total = cone.nodes.iter().filter(|n| n.id != node.id).count() as u64;
                Some(ImpactSummary {
                    direct_callers: direct,
                    total_callers: total.max(direct),
                })
            } else {
                None
            };
            let snippet = snippet_for(node, query.context_lines, root);
            let item = ExploreItem {
                node: SymbolHit::of(node),
                snippet,
                impact,
            };
            let estimated = serde_json_len(&item);
            let max = explore_byte_cap(query.max_bytes);
            if total_bytes.saturating_add(estimated) > max {
                byte_cap_hit = true;
                break;
            }
            total_bytes = total_bytes.saturating_add(estimated);
            items.push(item);
        }
        if byte_cap_hit {
            truncations.push(Truncation::new(
                TruncationKind::Bytes,
                u64::from(query.max_bytes),
                format!(
                    "payload reached the {}-byte cap; results were cut",
                    query.max_bytes
                ),
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
        let mut result = ExploreResult {
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
        fit_explore(&mut result, explore_byte_cap(query.max_bytes))?;
        Ok(result)
    }

    fn connect(
        &self,
        seeds: &mut Vec<Scored<Node>>,
        query: &ExploreQuery,
        hops: u8,
        truncations: &mut Vec<Truncation>,
    ) -> Result<Vec<EdgeHit>> {
        let seed_ids: Vec<NodeId> = seeds.iter().map(|s| s.item.id.clone()).collect();
        // Connect through semantic relationships. Containment would make
        // every pair of unrelated functions in one file appear connected.
        let relations = [
            EdgeKind::Calls,
            EdgeKind::References,
            EdgeKind::TypeUses,
            EdgeKind::Imports,
            EdgeKind::Implements,
            EdgeKind::Extends,
        ];
        let subgraph = self
            .snapshot
            .expand(&seed_ids, hops, &relations, Direction::Both)?;
        let allowed: BTreeSet<NodeId> = subgraph
            .nodes
            .iter()
            .filter(|n| {
                self.passes_filters(n, query.filters.lang, query.filters.path_glob.as_deref())
            })
            .map(|n| n.id.clone())
            .collect();
        let mut adjacent: BTreeMap<NodeId, Vec<(NodeId, usize)>> = BTreeMap::new();
        for (index, edge) in subgraph.edges.iter().enumerate() {
            if let Some(to) = &edge.to
                && allowed.contains(&edge.from)
                && allowed.contains(to)
            {
                adjacent
                    .entry(edge.from.clone())
                    .or_default()
                    .push((to.clone(), index));
                adjacent
                    .entry(to.clone())
                    .or_default()
                    .push((edge.from.clone(), index));
            }
        }
        let seed_set: BTreeSet<NodeId> = seed_ids.iter().cloned().collect();
        let mut selected = BTreeSet::new();
        for seed in &seed_ids {
            let mut visited = BTreeSet::from([seed.clone()]);
            let mut parent: BTreeMap<NodeId, (NodeId, usize)> = BTreeMap::new();
            let mut frontier = BTreeSet::from([seed.clone()]);
            for _ in 0..hops {
                let mut next = BTreeSet::new();
                for id in &frontier {
                    for (other, index) in adjacent.get(id).into_iter().flatten() {
                        if !visited.insert(other.clone()) {
                            continue;
                        }
                        parent.insert(other.clone(), (id.clone(), *index));
                        next.insert(other.clone());
                        if seed_set.contains(other) {
                            let mut cursor = other;
                            while let Some((prev, edge)) = parent.get(cursor) {
                                selected.insert(*edge);
                                cursor = prev;
                            }
                        }
                    }
                }
                frontier = next;
                if frontier.is_empty() {
                    break;
                }
            }
        }
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

    /// Node and edge counts for `status` (`SPEC.md` §8.5).
    ///
    /// # Errors
    /// When the read fails.
    pub fn counts(&self) -> Result<graph_search_types::result::StoreCounts> {
        let nodes = self.snapshot.all_nodes()?;
        let edges = self.snapshot.all_edges()?;
        let mut counts = graph_search_types::result::StoreCounts::default();
        for node in &nodes {
            let count = counts.nodes.entry(node.kind).or_default();
            *count = count.saturating_add(1);
            counts.total_nodes = counts.total_nodes.saturating_add(1);
            if node.is_file() {
                let language = node
                    .language
                    .unwrap_or(graph_search_types::Language::Unknown);
                let count = counts.files_by_language.entry(language).or_default();
                *count = count.saturating_add(1);
            }
        }
        for edge in &edges {
            let count = counts.edges.entry(edge.kind).or_default();
            *count = count.saturating_add(1);
            counts.total_edges = counts.total_edges.saturating_add(1);
        }
        Ok(counts)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// Seeds ranked by name, qualified name, and path match, then by a
    /// bounded literal scan of candidate files (`SPEC.md` §8.4 step 1).
    #[allow(clippy::too_many_lines)]
    fn seed(&self, query: &ExploreQuery, root: &Path, policy: &WalkPolicy) -> Result<Seeds> {
        // Preserve the baseline multi-term ranking until a lexical ranker
        // is evaluated. Explicit symbol queries may carry display punctuation.
        let single = query.query.split_whitespace().count() == 1;
        let terms: Vec<String> = query
            .query
            .split_whitespace()
            .map(|term| {
                if single {
                    term.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                        .to_ascii_lowercase()
                } else {
                    term.to_ascii_lowercase()
                }
            })
            .filter(|term| {
                if single {
                    !term.is_empty()
                } else {
                    term.len() >= 3
                }
            })
            .collect();
        if terms.is_empty() {
            return Ok(Seeds {
                nodes: Vec::new(),
                truncations: Vec::new(),
                candidates: 0,
                files_scanned: 0,
            });
        }
        let mut truncations = Vec::new();
        let mut scanned_files = 0u64;
        let mut scored: Vec<Scored<Node>> = Vec::new();
        for node in self.snapshot.all_nodes()? {
            if node.is_file() {
                continue;
            }
            if !self.passes_filters(
                &node,
                query.filters.lang,
                query.filters.path_glob.as_deref(),
            ) {
                continue;
            }
            let score = score_against(&node, &terms);
            if score > 0.0 {
                scored.push(Scored::new(node, score));
            }
        }

        // The literal scan: files whose *bodies* contain the most distinctive
        // term become seeds too. Bounded: at most `SCAN_FILE_CAP` files and
        // `SCAN_BYTES_CAP` bytes, so a pathological tree degrades by
        // reporting, not hanging (`SPEC.md` §13).
        let distinctive = terms.iter().max_by_key(|t| t.len()).cloned();
        if let Some(needle) = distinctive {
            let search_root =
                crate::walk::resolve_search_root(root, None).unwrap_or_else(|_| root.to_path_buf());
            {
                let entries = crate::walk::walk(&search_root, policy)?;
                let mut scanned_bytes: u64 = 0;
                if entries.len() > SCAN_FILE_CAP {
                    truncations.push(Truncation::new(
                        TruncationKind::Files,
                        SCAN_FILE_CAP as u64,
                        "explore body scan stopped at its file cap",
                    ));
                }
                let mut file_hits: Vec<Scored<Node>> = Vec::new();
                for entry in entries.iter().take(crate::query::SCAN_FILE_CAP) {
                    if scanned_bytes.saturating_add(entry.size) > SCAN_BYTES_CAP {
                        truncations.push(Truncation::new(
                            TruncationKind::Bytes,
                            SCAN_BYTES_CAP,
                            "explore body scan stopped at its byte cap",
                        ));
                        break;
                    }
                    let Ok(text) = std::fs::read_to_string(&entry.path) else {
                        continue;
                    };
                    scanned_files = scanned_files.saturating_add(1);
                    scanned_bytes = scanned_bytes.saturating_add(text.len() as u64);
                    let lowered = text.to_lowercase();
                    if !lowered.contains(needle.as_str()) {
                        continue;
                    }
                    let match_line = text
                        .lines()
                        .position(|line| line.to_lowercase().contains(needle.as_str()))
                        .unwrap_or(0);
                    let match_line = u32::try_from(match_line)
                        .unwrap_or(u32::MAX)
                        .saturating_add(1);
                    let file_node = Node {
                        id: NodeId::file(&entry.rel),
                        kind: NodeKind::File,
                        path: entry.rel.clone(),
                        language: entry.language,
                        span: Some(graph_search_types::node::Span {
                            start_line: match_line,
                            end_line: match_line,
                            ..graph_search_types::node::Span::default()
                        }),
                        ..Node::default()
                    };
                    if !self.passes_filters(
                        &file_node,
                        query.filters.lang,
                        query.filters.path_glob.as_deref(),
                    ) {
                        continue;
                    }
                    // A file seed ranks below an exact symbol name but above
                    // a bare path mention.
                    file_hits.push(Scored::new(file_node, 0.6));
                    if scanned_files >= SCAN_FILE_CAP as u64 {
                        break;
                    }
                }
                scored.extend(file_hits);
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
        let candidates = scored.len();
        let k = if query.k == 0 {
            graph_search_types::limits::EXPLORE_DEFAULT_K
        } else {
            effective_limit(query.k)
        };
        truncations.extend(result_truncations(candidates, k));
        scored.truncate(k as usize);
        Ok(Seeds {
            nodes: scored,
            truncations,
            candidates,
            files_scanned: scanned_files,
        })
    }

    /// Resolves a `<name|id>` argument: exact id first, then name.
    fn resolve_target(&self, target: &str) -> Result<NodeId> {
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
        let cleaned = target.trim_start_matches("file:");
        let file_id = NodeId::file(cleaned);
        if self.snapshot.node_by_id(&file_id)?.is_some() {
            return Ok(file_id);
        }
        self.resolve_target(target)
    }

    fn passes_filters(
        &self,
        node: &Node,
        lang: Option<graph_search_types::Language>,
        path_glob: Option<&str>,
    ) -> bool {
        if let Some(lang) = lang {
            if node.is_file() {
                if node.language != Some(lang) {
                    return false;
                }
            } else {
                // The symbol's file decides; look it up.
                let file = self
                    .snapshot
                    .node_by_id(&NodeId::file(&node.path))
                    .ok()
                    .flatten();
                if file.and_then(|f| f.language) != Some(lang) {
                    return false;
                }
            }
        }
        if let Some(glob) = path_glob {
            let Ok(set) = crate::files_search::compile_anchored_glob(glob) else {
                return false;
            };
            if !set.is_match(&node.path) {
                return false;
            }
        }
        true
    }

    fn traverse(
        &self,
        target: &str,
        kinds: &[EdgeKind],
        dir: Direction,
        query: &TraversalQuery,
    ) -> Result<GraphResult> {
        let started = std::time::Instant::now();
        validate_filters(&query.filters)?;
        let target_id = self.resolve_target(target)?;
        let subgraph = self.snapshot.expand(
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
            if let Some(node) = self.snapshot.node_by_id(id)? {
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
        filters: &graph_search_types::query::GraphFilters,
        started: std::time::Instant,
    ) -> GraphResult {
        let allowed: BTreeSet<String> = subgraph
            .nodes
            .iter()
            .filter(|n| self.passes_filters(n, filters.lang, filters.path_glob.as_deref()))
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
fn snippet_for(node: &Node, context_lines: u32, root: &Path) -> Option<Snippet> {
    if context_lines == 0 {
        return None;
    }
    let span = node.span?;
    let text = std::fs::read_to_string(root.join(&node.path)).ok()?;
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
        start_line: u32::try_from(start).unwrap_or(1).saturating_add(1),
        lines: lines[start..end]
            .iter()
            .map(|l| crate::text_search::truncate_line(l, 200))
            .collect(),
    })
}

fn ms_since(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn serde_json_len(item: &ExploreItem) -> usize {
    serde_json::to_string(item).map_or(256, |s| s.len())
}

const _: () = {
    // Keep the default depth referenced so the constant documents the CLI
    // default without a magic number there.
    let _ = DEFAULT_TRAVERSAL_DEPTH;
    let _ = GRAPH_DEFAULT_LIMIT;
};

fn validate_filters(filters: &graph_search_types::query::GraphFilters) -> Result<()> {
    if let Some(glob) = &filters.path_glob {
        crate::files_search::compile_anchored_glob(glob)?;
    }
    Ok(())
}
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
        if result.edges.pop().is_some() {
            continue;
        }
        if result.items.pop().is_none() {
            return Err(Error::InvalidInclude(format!(
                "max_bytes={cap} cannot hold explore metadata"
            )));
        }
    }
}
