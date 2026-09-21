//! Request-local reuse of budget-admitted adjacency from one immutable snapshot.

use crate::{Result, ports::GraphSnapshot, work::WorkBudget};
use graph_search_types::{Direction, Edge, EdgeKind, NodeId};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Neighborhoods {
    // Empty/denied neighborhoods need no entry. Every retained edge was charged
    // by the snapshot, so both records and keys are bounded by its edge allowance.
    incident: BTreeMap<NodeId, Vec<Edge>>,
}

impl Neighborhoods {
    pub(crate) fn clear(&mut self) {
        self.incident.clear();
    }

    pub(crate) fn read(
        &mut self,
        snapshot: &dyn GraphSnapshot,
        id: &NodeId,
        kinds: &[EdgeKind],
        direction: Direction,
        work: &mut WorkBudget,
    ) -> Result<Vec<Edge>> {
        work.check()?;
        if !self.incident.contains_key(id) {
            // Both native adapters charge the incident prefix before filtering.
            // Capture that same prefix, including kinds another phase may need.
            let edges = snapshot.edges_bounded(id, &[], Direction::Both, work)?;
            if edges.is_empty() {
                return Ok(Vec::new());
            }
            self.incident.insert(id.clone(), edges);
        }
        let mut selected = Vec::new();
        for edge in &self.incident[id] {
            work.check()?;
            let matches_direction = match direction {
                Direction::In => edge.to.as_ref() == Some(id),
                Direction::Out => &edge.from == id,
                Direction::Both => true,
            };
            if matches_direction && (kinds.is_empty() || kinds.contains(&edge.kind)) {
                selected.push(edge.clone());
            }
        }
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GraphStore,
        memory::MemoryStore,
        work::{CancellationToken, WorkLimits},
    };

    fn fixture() -> MemoryStore {
        let mut batch = crate::conformance::fixture_batch();
        let a = batch.upserts[0].symbols[0].id.clone();
        let b = batch.upserts[1].symbols[0].id.clone();
        for (from, kind, to) in [
            (&b, EdgeKind::References, &a),
            (&a, EdgeKind::Calls, &a),
            (&a, EdgeKind::Imports, &b),
            (&b, EdgeKind::Contains, &a),
        ] {
            batch.upserts[0]
                .edges
                .push(Edge::resolved(from, kind, to, "target", None, None));
        }
        let mut dangling = Edge::resolved(&a, EdgeKind::References, &b, "missing", None, None);
        dangling.to = None;
        batch.upserts[0].edges.push(dangling);
        let mut store = MemoryStore::new();
        store.apply(batch).unwrap();
        store
    }

    #[test]
    fn shared_neighborhoods_preserve_all_relation_and_direction_views() {
        let store = fixture();
        let snapshot = store.snapshot().unwrap();
        let mut cache = Neighborhoods::default();
        let mut work = WorkBudget::new(WorkLimits::default());
        let kinds = [
            EdgeKind::Calls,
            EdgeKind::References,
            EdgeKind::Imports,
            EdgeKind::Contains,
        ];
        for node in snapshot.all_nodes().unwrap() {
            for mask in 0..16 {
                let selected: Vec<_> = kinds
                    .iter()
                    .enumerate()
                    .filter_map(|(i, kind)| (mask & (1 << i) != 0).then_some(*kind))
                    .collect();
                for dir in [Direction::In, Direction::Out, Direction::Both] {
                    let expected = snapshot.edges_from(&node.id, &selected, dir).unwrap();
                    let actual = cache
                        .read(snapshot.as_ref(), &node.id, &selected, dir, &mut work)
                        .unwrap();
                    assert_eq!(actual, expected, "{} {selected:?} {dir:?}", node.id);
                }
            }
        }
        // Six edges: the self-loop and unresolved edge each occur once;
        // the four other edges each belong to two incident lists.
        assert_eq!(work.report().1, 10);
        assert!(work.report().2.is_empty());
        assert_eq!(cache.incident.len(), 2);
    }

    #[test]
    fn cached_partial_prefix_stays_partial_and_observes_cancellation() {
        let store = fixture();
        let snapshot = store.snapshot().unwrap();
        let id = NodeId::symbol(
            "src/a.rs",
            graph_search_types::NodeKind::Function,
            "a",
            None,
        );
        let token = CancellationToken::default();
        let mut work = WorkBudget::new(WorkLimits {
            edges: 2,
            cancellation: Some(token.clone()),
            ..WorkLimits::default()
        });
        let mut cache = Neighborhoods::default();
        let first = cache
            .read(snapshot.as_ref(), &id, &[], Direction::Both, &mut work)
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(
            cache
                .read(snapshot.as_ref(), &id, &[], Direction::Both, &mut work)
                .unwrap(),
            first
        );
        assert_eq!(work.report().1, 2);
        assert_eq!(work.report().2.len(), 1);
        for n in 0..100 {
            assert!(
                cache
                    .read(
                        snapshot.as_ref(),
                        &NodeId::new(format!("missing-{n}")),
                        &[],
                        Direction::Both,
                        &mut work
                    )
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(
            cache.incident.len(),
            1,
            "empty reads allocate no cache entries"
        );
        token.cancel();
        assert!(matches!(
            cache.read(snapshot.as_ref(), &id, &[], Direction::Both, &mut work),
            Err(crate::Error::QueryCancelled)
        ));
        cache.clear();
        let mut fresh = WorkBudget::new(WorkLimits::default());
        assert_eq!(
            cache
                .read(snapshot.as_ref(), &id, &[], Direction::Both, &mut fresh)
                .unwrap()
                .len(),
            6
        );
        assert!(fresh.report().2.is_empty());
    }
}
