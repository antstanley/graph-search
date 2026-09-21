//! Shared component discovery for bounded seed-to-seed connection paths.

use crate::{Result, work::WorkBudget};
use graph_search_types::{Edge, NodeId};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct ConnectionGraph {
    nodes: Vec<NodeId>,
    adjacent: Vec<Vec<(usize, usize)>>,
    components: Vec<usize>,
}

impl ConnectionGraph {
    /// Input is the already budgeted expanded graph. Construction is linear in
    /// its adjacency size, apart from node lookup and near-constant union/find.
    /// Original edge order and sorted node order preserve BFS tie decisions.
    pub(crate) fn new(allowed: BTreeSet<NodeId>, edges: &[Edge]) -> Self {
        let nodes: Vec<_> = allowed.into_iter().collect();
        let positions: BTreeMap<_, _> = nodes.iter().enumerate().map(|(i, n)| (n, i)).collect();
        let mut adjacent = vec![Vec::new(); nodes.len()];
        let mut components: Vec<_> = (0..nodes.len()).collect();
        let mut sizes = vec![1usize; nodes.len()];
        for (edge_index, edge) in edges.iter().enumerate() {
            let Some(to) = &edge.to else { continue };
            let (Some(&from), Some(&to)) = (positions.get(&edge.from), positions.get(to)) else {
                continue;
            };
            adjacent[from].push((to, edge_index));
            adjacent[to].push((from, edge_index));
            let mut a = root(&mut components, from);
            let mut b = root(&mut components, to);
            if a != b {
                if sizes[a] < sizes[b] {
                    std::mem::swap(&mut a, &mut b);
                }
                components[b] = a;
                sizes[a] = sizes[a].saturating_add(sizes[b]);
            }
        }
        for i in 0..components.len() {
            components[i] = root(&mut components, i);
        }
        Self {
            nodes,
            adjacent,
            components,
        }
    }

    pub(crate) fn paths(
        &self,
        seeds: &[NodeId],
        hops: u8,
        work: &mut WorkBudget,
    ) -> Result<BTreeSet<usize>> {
        work.check()?;
        let seeds: Vec<_> = seeds
            .iter()
            .filter_map(|id| self.nodes.binary_search(id).ok())
            .collect();
        let mut targets: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for &seed in &seeds {
            targets
                .entry(self.components[seed])
                .or_default()
                .insert(seed);
        }
        let mut selected = BTreeSet::new();
        for seed in seeds {
            work.check()?;
            let targets = &targets[&self.components[seed]];
            if targets.len() > 1 {
                self.paths_from_seed(seed, targets, hops, work, &mut selected)?;
            }
        }
        Ok(selected)
    }

    fn paths_from_seed(
        &self,
        seed: usize,
        targets: &BTreeSet<usize>,
        hops: u8,
        work: &mut WorkBudget,
        selected: &mut BTreeSet<usize>,
    ) -> Result<()> {
        let mut remaining = targets.len().saturating_sub(1);
        let mut visited = BTreeSet::from([seed]);
        let mut parent = BTreeMap::new();
        let mut frontier = BTreeSet::from([seed]);
        for _ in 0..hops {
            let mut next = BTreeSet::new();
            for id in frontier {
                for &(other, index) in &self.adjacent[id] {
                    if !work.edge()? {
                        return Ok(());
                    }
                    if !visited.insert(other) {
                        continue;
                    }
                    parent.insert(other, (id, index));
                    next.insert(other);
                    if targets.contains(&other) {
                        let mut cursor = other;
                        while let Some(&(prev, edge)) = parent.get(&cursor) {
                            selected.insert(edge);
                            cursor = prev;
                        }
                        remaining = remaining.saturating_sub(1);
                        if remaining == 0 {
                            return Ok(());
                        }
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        Ok(())
    }
}

fn root(parents: &mut [usize], mut node: usize) -> usize {
    while parents[node] != node {
        parents[node] = parents[parents[node]];
        node = parents[node];
    }
    node
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::{CancellationToken, WorkLimits};
    use graph_search_types::EdgeKind;

    // Independent, deliberately unoptimized original NodeId-based BFS oracle.
    fn reference(
        nodes: &BTreeSet<NodeId>,
        edges: &[Edge],
        seeds: &[NodeId],
        hops: u8,
    ) -> BTreeSet<usize> {
        let mut adjacent: BTreeMap<NodeId, Vec<(NodeId, usize)>> = BTreeMap::new();
        for (i, edge) in edges.iter().enumerate() {
            if let Some(to) = &edge.to
                && nodes.contains(&edge.from)
                && nodes.contains(to)
            {
                adjacent
                    .entry(edge.from.clone())
                    .or_default()
                    .push((to.clone(), i));
                adjacent
                    .entry(to.clone())
                    .or_default()
                    .push((edge.from.clone(), i));
            }
        }
        let mut selected = BTreeSet::new();
        for seed in seeds {
            let mut visited = BTreeSet::from([seed.clone()]);
            let mut parent = BTreeMap::new();
            let mut frontier = BTreeSet::from([seed.clone()]);
            for _ in 0..hops {
                let mut next = BTreeSet::new();
                for id in frontier {
                    for (other, edge) in adjacent.get(&id).into_iter().flatten() {
                        if !visited.insert(other.clone()) {
                            continue;
                        }
                        parent.insert(other.clone(), (id.clone(), *edge));
                        next.insert(other.clone());
                        if seeds.contains(other) {
                            let mut cursor = other;
                            while let Some((prev, edge)) = parent.get(cursor) {
                                selected.insert(*edge);
                                cursor = prev;
                            }
                        }
                    }
                }
                frontier = next;
            }
        }
        selected
    }

    fn call(from: &NodeId, to: &NodeId) -> Edge {
        Edge::resolved(from, EdgeKind::Calls, to, to.as_str(), None, None)
    }

    #[test]
    fn all_small_graphs_preserve_shortest_path_ties_and_original_edge_identity() {
        let ids: Vec<_> = (0..4).map(|n| NodeId::new(format!("n{n}"))).collect();
        let possible: Vec<_> = (0..4)
            .flat_map(|a| ((a + 1)..4).map(move |b| (a, b)))
            .collect();
        for mask in 0..64 {
            let mut edges: Vec<_> = possible
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, &(a, b))| call(&ids[a], &ids[b]))
                .collect();
            // Reverse order exercises adjacency tie order independently of IDs.
            if mask % 2 == 0 {
                edges.reverse();
            }
            for seed_mask in 0..16 {
                let seeds: Vec<_> = ids
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| seed_mask & (1 << i) != 0)
                    .map(|(_, id)| id.clone())
                    .collect();
                let nodes = ids.iter().cloned().collect();
                let graph = ConnectionGraph::new(nodes, &edges);
                for hops in 0..=4 {
                    let mut work = WorkBudget::new(WorkLimits::default());
                    let actual = graph.paths(&seeds, hops, &mut work).unwrap();
                    let expected = reference(&ids.iter().cloned().collect(), &edges, &seeds, hops);
                    assert_eq!(
                        actual, expected,
                        "mask={mask}, seeds={seed_mask}, hops={hops}"
                    );
                    assert!(work.report().2.is_empty());
                }
            }
        }
    }

    #[test]
    fn components_skip_unreachable_targets_and_search_stops_after_last_target() {
        let ids: Vec<_> = (0..8).map(|n| NodeId::new(format!("n{n}"))).collect();
        let edges = vec![
            call(&ids[0], &ids[1]),
            call(&ids[1], &ids[2]),
            call(&ids[2], &ids[3]),
            call(&ids[4], &ids[5]),
            call(&ids[6], &ids[7]),
        ];
        let graph = ConnectionGraph::new(ids.iter().cloned().collect(), &edges);
        let seeds = vec![
            ids[0].clone(),
            ids[1].clone(),
            ids[4].clone(),
            ids[6].clone(),
        ];
        let mut work = WorkBudget::new(WorkLimits {
            edges: 2,
            ..WorkLimits::default()
        });
        assert_eq!(
            graph.paths(&seeds, 4, &mut work).unwrap(),
            BTreeSet::from([0])
        );
        assert_eq!(work.report().1, 2);
        assert!(work.report().2.is_empty());
        let mut limited = WorkBudget::new(WorkLimits {
            edges: 1,
            ..WorkLimits::default()
        });
        assert_eq!(
            graph.paths(&seeds, 4, &mut limited).unwrap(),
            BTreeSet::from([0])
        );
        assert_eq!(limited.report().1, 1);
        assert!(!limited.report().2.is_empty());
    }

    #[test]
    fn filtered_nodes_parallel_edges_self_loops_and_cancellation() {
        let ids: Vec<_> = (0..4).map(|n| NodeId::new(format!("n{n}"))).collect();
        let parallel = Edge::resolved(
            &ids[0],
            EdgeKind::References,
            &ids[1],
            ids[1].as_str(),
            None,
            None,
        );
        let edges = vec![
            call(&ids[0], &ids[0]),
            parallel,
            call(&ids[0], &ids[1]),
            call(&ids[1], &ids[2]),
            call(&ids[2], &ids[3]),
            Edge::dangling(&ids[1], EdgeKind::Calls, "unknown", None, None),
        ];
        let nodes: BTreeSet<_> = [ids[0].clone(), ids[1].clone(), ids[3].clone()].into();
        let seeds = ids.clone();
        let graph = ConnectionGraph::new(nodes.clone(), &edges);
        let mut work = WorkBudget::new(WorkLimits::default());
        assert_eq!(
            graph.paths(&seeds, 4, &mut work).unwrap(),
            reference(&nodes, &edges, &seeds, 4)
        );
        let token = CancellationToken::default();
        token.cancel();
        let mut cancelled = WorkBudget::new(WorkLimits {
            cancellation: Some(token),
            ..WorkLimits::default()
        });
        assert!(matches!(
            graph.paths(&[], 0, &mut cancelled),
            Err(crate::Error::QueryCancelled)
        ));
    }
}
