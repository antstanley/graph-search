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
    } else {
        limit
    }
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
        let found = self.snapshot.find_by_name(
            &query.target,
            &kinds,
            effective_limit(query.limit) as usize,
        )?;
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
        nodes.sort();
        nodes.truncate(effective_limit(query.limit) as usize);
        Ok(GraphResult {
            stats: Stats {
                candidates: nodes.len() as u64,
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
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let edges = self
            .snapshot
            .edges_from(&target, &REFERENCE_KINDS, Direction::In)?;
        self.assemble_graph(&target, &edges, query.limit, started)
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
    pub fn impact(&self, query: &TraversalQuery) -> Result<ImpactResult> {
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let kinds = [EdgeKind::Calls, EdgeKind::References];

        // Counts by ring. `expand` returns the deduplicated set, so rings are
        // recomputed by BFS here for exact per-depth counts.
        let mut by_depth: Vec<DepthCount> = Vec::new();
        let mut frontier: BTreeSet<NodeId> = BTreeSet::from([target.clone()]);
        let mut visited: BTreeSet<NodeId> = frontier.clone();
        let mut cone_edges: Vec<graph_search_types::Edge> = Vec::new();
        for depth in 1..=query.depth {
            let mut next = BTreeSet::new();
            let mut ring: BTreeMap<NodeKind, u64> = BTreeMap::new();
            for id in &frontier {
                for edge in self.snapshot.edges_from(id, &kinds, Direction::In)? {
                    // Incoming: the next hop is the edge's SOURCE.
                    let Some(from) = edge
                        .to
                        .as_ref()
                        .filter(|to| **to == *id)
                        .map(|_| edge.from.clone())
                    else {
                        continue;
                    };
                    if visited.contains(&from) {
                        continue;
                    }
                    cone_edges.push(edge.clone());
                    if visited.insert(from.clone())
                        && let Some(node) = self.snapshot.node_by_id(&from)?
                    {
                        let count = ring.entry(node.kind).or_default();
                        *count = count.saturating_add(1);
                        next.insert(from);
                    }
                }
            }
            let total: u64 = ring.values().sum();
            by_depth.push(DepthCount {
                depth,
                total,
                by_kind: ring,
            });
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }

        // Top nodes: ring order, then in-cone degree desc, then path.
        let mut degree: BTreeMap<NodeId, u64> = BTreeMap::new();
        for edge in &cone_edges {
            if let Some(to) = &edge.to {
                let count = degree.entry(to.clone()).or_default();
                *count = count.saturating_add(1);
            }
        }
        let mut top: Vec<(u8, NodeId)> = visited
            .iter()
            .filter(|id| **id != target)
            .filter_map(|id| {
                let node = self.snapshot.node_by_id(id).ok().flatten()?;
                self.passes_filters(
                    &node,
                    query.filters.lang,
                    query.filters.path_glob.as_deref(),
                )
                .then(|| {
                    (
                        u8::try_from(degree.get(id).copied().unwrap_or_default())
                            .unwrap_or(u8::MAX),
                        id.clone(),
                    )
                })
            })
            .collect();
        top.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut nodes: Vec<SymbolHit> = Vec::new();
        for (_, id) in top.into_iter().take(effective_limit(query.limit) as usize) {
            if let Some(node) = self.snapshot.node_by_id(&id)? {
                nodes.push(SymbolHit::of(&node));
            }
        }
        nodes.sort();
        let edges: Vec<EdgeHit> = cone_edges.iter().map(EdgeHit::from_edge).collect();
        let resolved = edges.iter().filter(|e| e.resolved).count() as u64;
        Ok(ImpactResult {
            by_depth,
            top: nodes,
            approximation: Some(Approximation {
                resolved,
                unresolved: (edges.len() as u64).saturating_sub(resolved),
                ..Approximation::default()
            }),
            edges,
            stats: Stats {
                candidates: visited.len() as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
            truncations: Vec::new(),
        })
    }

    /// `deps`: imports and imported-by for a file (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn deps(&self, query: &DepsQuery) -> Result<GraphResult> {
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
        self.assemble_graph(&target, &edges, query.limit, started)
    }

    /// `neighbors`: adjacent nodes along chosen edge kinds (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When the target does not resolve or the read fails.
    pub fn neighbors(&self, query: &NeighborsQuery) -> Result<GraphResult> {
        let started = std::time::Instant::now();
        let target = self.resolve_target(&query.target)?;
        let kinds: Vec<EdgeKind> = query.rel.iter().copied().collect();
        let subgraph = self.snapshot.expand(
            std::slice::from_ref(&target),
            query.hops.max(1),
            &kinds,
            Direction::Both,
        )?;
        Ok(Self::result_from_subgraph(&subgraph, query.limit, started))
    }

    /// `path`: the shortest path between two nodes (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// When either endpoint does not resolve or the read fails.
    pub fn path(&self, query: &PathQuery) -> Result<GraphResult> {
        let started = std::time::Instant::now();
        let from = self.resolve_target(&query.from)?;
        let to = self.resolve_target(&query.to)?;
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
        nodes.sort();
        nodes.dedup();
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
        let started = std::time::Instant::now();
        let seeds = self.seed(query, root)?;
        let seed_ids: Vec<NodeId> = seeds.iter().map(|s| s.item.id.clone()).collect();
        let hops = query.hops.clamp(1, MAX_HOPS_CEILING);

        // Connect: edges among the seeds, up to `hops`.
        let subgraph = self
            .snapshot
            .expand(&seed_ids, hops, &[], Direction::Both)?;
        let seed_set: BTreeSet<&NodeId> = seed_ids.iter().collect();
        let mut edges: Vec<EdgeHit> = subgraph
            .edges
            .iter()
            .filter(|edge| {
                seed_set.contains(&edge.from)
                    && edge.to.as_ref().is_some_and(|to| seed_set.contains(to))
            })
            .map(EdgeHit::from_edge)
            .collect();
        edges.sort();

        // Impact: one-line blast radius for function/method seeds.
        let mut items: Vec<ExploreItem> = Vec::new();
        let mut truncations = Vec::new();
        let mut total_bytes = 0usize;
        let mut byte_cap_hit = false;
        for scored in &seeds {
            let node = &scored.item;
            let impact = if matches!(node.kind, NodeKind::Function | NodeKind::Method) {
                let direct = self
                    .snapshot
                    .edges_from(&node.id, &[EdgeKind::Calls], Direction::In)?
                    .len() as u64;
                let cone = self.snapshot.expand(
                    std::slice::from_ref(&node.id),
                    hops,
                    &[EdgeKind::Calls],
                    Direction::In,
                )?;
                let total = cone.edges.len() as u64;
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
            let max = query.max_bytes as usize;
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
        let resolved = edges.iter().filter(|e| e.resolved).count() as u64;
        let approximation = Approximation {
            resolved,
            unresolved: (edges.len() as u64).saturating_sub(resolved),
            ..Approximation::default()
        };
        Ok(ExploreResult {
            items,
            edges,
            truncations,
            approximation: Some(approximation),
            stats: Stats {
                candidates: seeds.len() as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
        })
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
    fn seed(&self, query: &ExploreQuery, root: &Path) -> Result<Vec<Scored<Node>>> {
        // Drop the small words a natural-language question is full of; they
        // match everything and rank nothing.
        let terms: Vec<String> = query
            .query
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .filter(|t| t.len() >= 3)
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
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
            let policy = WalkPolicy::default();
            if let Ok(entries) = crate::walk::walk(&search_root, &policy) {
                let mut scanned_files: u64 = 0;
                let mut scanned_bytes: u64 = 0;
                let mut file_hits: Vec<Scored<Node>> = Vec::new();
                for entry in entries.iter().take(crate::query::SCAN_FILE_CAP) {
                    if scanned_bytes >= SCAN_BYTES_CAP {
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
                    let file_node = Node {
                        id: NodeId::file(&entry.rel),
                        kind: NodeKind::File,
                        path: entry.rel.clone(),
                        language: entry.language,
                        ..Node::default()
                    };
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
        scored.truncate(query.k as usize);
        Ok(scored)
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
        let mut found = self.snapshot.find_by_name(target, &kinds, 1)?;
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
        let target_id = self.resolve_target(target)?;
        let subgraph =
            self.snapshot
                .expand(std::slice::from_ref(&target_id), query.depth, kinds, dir)?;
        Ok(Self::result_from_subgraph(&subgraph, query.limit, started))
    }

    fn assemble_graph(
        &self,
        target: &NodeId,
        edges: &[graph_search_types::Edge],
        limit: u32,
        started: std::time::Instant,
    ) -> Result<GraphResult> {
        let mut nodes: BTreeMap<NodeId, Node> = BTreeMap::new();
        if let Some(node) = self.snapshot.node_by_id(target)? {
            nodes.insert(target.clone(), node);
        }
        for edge in edges {
            if let Some(node) = self.snapshot.node_by_id(&edge.from)? {
                nodes.insert(edge.from.clone(), node);
            }
            if let Some(to) = &edge.to
                && let Some(node) = self.snapshot.node_by_id(to)?
            {
                nodes.insert(to.clone(), node);
            }
        }
        let mut hits: Vec<SymbolHit> = nodes.values().map(SymbolHit::of).collect();
        hits.sort();
        hits.truncate(effective_limit(limit) as usize);
        let hits_set: BTreeSet<String> = hits.iter().map(|h| h.id.clone()).collect();
        let mut edge_hits: Vec<EdgeHit> = edges.iter().map(EdgeHit::from_edge).collect();
        edge_hits.sort();
        let resolved = edge_hits.iter().filter(|e| e.resolved).count() as u64;
        Ok(GraphResult {
            nodes: hits,
            edges: edge_hits
                .into_iter()
                .filter(|e| {
                    hits_set.contains(&e.from)
                        || hits_set.contains(e.to.as_deref().unwrap_or_default())
                })
                .collect(),
            approximation: Some(Approximation {
                resolved,
                unresolved: 0,
                ..Approximation::default()
            }),
            stats: Stats {
                candidates: nodes.len() as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
            ..GraphResult::default()
        })
    }

    fn result_from_subgraph(
        subgraph: &graph_search_types::Subgraph,
        limit: u32,
        started: std::time::Instant,
    ) -> GraphResult {
        let mut hits: Vec<SymbolHit> = subgraph.nodes.iter().map(SymbolHit::of).collect();
        hits.sort();
        hits.truncate(effective_limit(limit) as usize);
        let kept: BTreeSet<String> = hits.iter().map(|h| h.id.clone()).collect();
        let mut edge_hits: Vec<EdgeHit> = subgraph
            .edges
            .iter()
            .filter(|edge| {
                kept.contains(edge.from.as_str())
                    || edge
                        .to
                        .as_ref()
                        .is_some_and(|to| kept.contains(to.as_str()))
            })
            .map(EdgeHit::from_edge)
            .collect();
        edge_hits.sort();
        let resolved = edge_hits.iter().filter(|e| e.resolved).count() as u64;
        let unresolved = (edge_hits.len() as u64).saturating_sub(resolved);
        GraphResult {
            nodes: hits,
            edges: edge_hits,
            approximation: Some(Approximation {
                resolved,
                unresolved,
                ..Approximation::default()
            }),
            stats: Stats {
                candidates: subgraph.nodes.len() as u64,
                elapsed_ms: ms_since(started),
                ..Stats::default()
            },
            ..GraphResult::default()
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
    let end = end_base.min(lines.len());
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
