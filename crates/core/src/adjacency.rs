//! Native generation-owned adjacency ordered by stable edge identity.

use crate::{Result, work::WorkBudget};
use graph_search_types::{Direction, Edge, EdgeKind, NodeId};
use std::collections::BTreeMap;

/// Each edge is stored once; per-node adjacency holds integer positions.
/// Constructed during store preparation, never by a bounded query.
#[derive(Default)]
pub struct AdjacencyIndex {
    edges: Vec<Edge>,
    incident: BTreeMap<NodeId, Vec<usize>>,
}
impl AdjacencyIndex {
    /// Builds deterministic adjacency, deduplicating graph edge identities.
    #[must_use]
    pub fn new(mut edges: Vec<Edge>) -> Self {
        edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.kind.cmp(&b.kind))
                .then(a.id.cmp(&b.id))
        });
        edges.dedup_by(|a, b| a.id == b.id);
        let mut incident: BTreeMap<NodeId, Vec<usize>> = BTreeMap::new();
        for (position, edge) in edges.iter().enumerate() {
            incident
                .entry(edge.from.clone())
                .or_default()
                .push(position);
            if let Some(to) = &edge.to
                && to != &edge.from
            {
                incident.entry(to.clone()).or_default().push(position);
            }
        }
        Self { edges, incident }
    }
    /// Reads only the budget-admitted adjacency prefix. No full-degree clone,
    /// scan of unrelated nodes, or query-time sort is performed.
    /// # Errors
    /// On cancellation or deadline.
    pub fn read(
        &self,
        id: &NodeId,
        kinds: &[EdgeKind],
        dir: Direction,
        budget: &mut WorkBudget,
    ) -> Result<Vec<Edge>> {
        budget.check()?;
        let mut out = Vec::new();
        for &position in self.incident.get(id).into_iter().flatten() {
            if !budget.edge()? {
                break;
            }
            let edge = &self.edges[position];
            let direction = match dir {
                Direction::Out => &edge.from == id,
                Direction::In => edge.to.as_ref() == Some(id),
                Direction::Both => true,
            };
            if direction && (kinds.is_empty() || kinds.contains(&edge.kind)) {
                out.push(edge.clone());
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::{CancellationToken, WorkLimits};

    #[test]
    fn high_degree_reads_charge_filtered_entries_and_stop_at_the_cap() {
        let from = NodeId::new("hub");
        let edges = (0..2000)
            .map(|n| {
                Edge::resolved(
                    &from,
                    EdgeKind::Calls,
                    &NodeId::new(format!("leaf{n}")),
                    "leaf",
                    None,
                    None,
                )
            })
            .collect();
        let index = AdjacencyIndex::new(edges);
        let mut budget = WorkBudget::new(WorkLimits {
            edges: 3,
            ..WorkLimits::default()
        });
        let hits = index
            .read(&from, &[EdgeKind::Imports], Direction::Out, &mut budget)
            .unwrap();
        assert!(hits.is_empty());
        let (_, examined, limits) = budget.report();
        assert_eq!(examined, 3);
        assert_eq!(limits.len(), 1);
        assert!(
            index
                .read(&from, &[], Direction::Both, &mut budget)
                .unwrap()
                .is_empty()
        );
        assert_eq!(budget.report().1, 3);
    }

    #[test]
    fn self_loops_are_once_only_and_cancellation_is_observed() {
        let id = NodeId::new("a");
        let edge = Edge::resolved(&id, EdgeKind::Calls, &id, "a", None, None);
        let index = AdjacencyIndex::new(vec![edge.clone(), edge]);
        let token = CancellationToken::default();
        let mut budget = WorkBudget::new(WorkLimits {
            edges: 1,
            cancellation: Some(token.clone()),
            ..WorkLimits::default()
        });
        assert_eq!(
            index
                .read(&id, &[], Direction::Both, &mut budget)
                .unwrap()
                .len(),
            1
        );
        assert!(budget.report().2.is_empty());
        token.cancel();
        assert!(matches!(
            index.read(&id, &[], Direction::Both, &mut budget),
            Err(crate::Error::QueryCancelled)
        ));
    }
}
