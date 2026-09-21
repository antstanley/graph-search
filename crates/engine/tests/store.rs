//! The Grafeo adapter against core's conformance suite, plus persistence
//! (`SPEC.md` §15.4).

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]
// The conformance suite panics on contract violations; that is its mechanism.

use graph_search_core::conformance;
use graph_search_core::memory::MemoryStore;
use graph_search_core::ports::GraphStore;
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::NodeId;
use graph_search_types::kind::{EdgeKind, NodeKind};
use tempfile::TempDir;

fn open(dir: &std::path::Path) -> GrafeoStore {
    GrafeoStore::open(dir, &StoreOptions::default()).unwrap_or_else(|e| panic!("open: {e}"))
}

#[test]
fn the_conformance_suite_passes_against_memory() {
    let mut store = MemoryStore::new();
    conformance::run_all(&mut store);
}

#[test]
fn the_conformance_suite_passes_against_grafeo() {
    let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    let mut store = open(tmp.path());
    conformance::run_all(&mut store);
}

#[test]
fn cyclic_expansion_matches_distance_oracle_in_both_adapters() {
    use graph_search_types::{Direction, Edge};
    use std::collections::{BTreeMap, BTreeSet};

    let tmp = TempDir::new().expect("tmp");
    let mut stores: Vec<Box<dyn GraphStore>> =
        vec![Box::new(MemoryStore::new()), Box::new(open(tmp.path()))];
    let mut batch = conformance::fixture_batch();
    let a = batch.upserts[0].symbols[0].id.clone();
    let b = batch.upserts[1].symbols[0].id.clone();
    batch.upserts[0].edges.extend([
        Edge::resolved(&a, EdgeKind::References, &b, "b", None, None),
        Edge::resolved(&a, EdgeKind::Calls, &a, "a", None, None),
        Edge::dangling(&a, EdgeKind::Calls, "missing", None, None),
    ]);
    batch.upserts[1]
        .edges
        .push(Edge::resolved(&b, EdgeKind::Calls, &a, "a", None, None));
    let nodes: BTreeMap<_, _> = batch
        .upserts
        .iter()
        .flat_map(|f| std::iter::once(&f.file).chain(&f.symbols))
        .map(|n| (n.id.clone(), n.clone()))
        .collect();
    let edges: Vec<_> = batch.upserts.iter().flat_map(|f| &f.edges).collect();
    for store in &mut stores {
        store.apply(batch.clone()).expect("fixture");
        let snapshot = store.snapshot().expect("snapshot");
        for seeds in [
            vec![],
            vec![a.clone()],
            vec![b.clone()],
            vec![a.clone(), b.clone()],
        ] {
            for direction in [Direction::In, Direction::Out, Direction::Both] {
                for kinds in [vec![], vec![EdgeKind::Calls], vec![EdgeKind::References]] {
                    // Relax distances over all edges, independently of traversal order.
                    let mut distance: BTreeMap<_, usize> =
                        seeds.iter().map(|id| (id.clone(), 0)).collect();
                    for _ in 0..nodes.len() {
                        for edge in &edges {
                            if !kinds.is_empty() && !kinds.contains(&edge.kind) {
                                continue;
                            }
                            let Some(to) = &edge.to else { continue };
                            for (from, to, allowed) in [
                                (&edge.from, to, direction != Direction::In),
                                (to, &edge.from, direction != Direction::Out),
                            ] {
                                if allowed && let Some(&previous) = distance.get(from) {
                                    let next = distance.entry(to.clone()).or_insert(usize::MAX);
                                    *next = (*next).min(previous + 1);
                                }
                            }
                        }
                    }
                    for hops in 0..=8 {
                        let result = snapshot
                            .expand(&seeds, hops, &kinds, direction)
                            .expect("expand");
                        let expected_nodes: BTreeSet<_> = distance
                            .iter()
                            .filter(|(_, d)| **d <= usize::from(hops))
                            .map(|(id, _)| id.clone())
                            .collect();
                        let expected_edges: BTreeSet<_> = edges
                            .iter()
                            .filter(|edge| {
                                let reached = |id: &NodeId| {
                                    distance.get(id).is_some_and(|d| *d < usize::from(hops))
                                };
                                (kinds.is_empty() || kinds.contains(&edge.kind))
                                    && ((direction != Direction::In && reached(&edge.from))
                                        || (direction != Direction::Out
                                            && edge.to.as_ref().is_some_and(reached)))
                            })
                            .map(|e| e.id.clone())
                            .collect();
                        assert_eq!(
                            result
                                .nodes
                                .iter()
                                .map(|n| n.id.clone())
                                .collect::<BTreeSet<_>>(),
                            expected_nodes
                        );
                        assert_eq!(
                            result
                                .edges
                                .iter()
                                .map(|e| e.id.clone())
                                .collect::<BTreeSet<_>>(),
                            expected_edges
                        );
                        for edge in result.edges {
                            assert!(edges.iter().any(|expected| **expected == edge));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_store_persists_across_open() {
    let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    {
        let mut store = open(tmp.path());
        let batch = conformance::fixture_batch();
        store.apply(batch).unwrap_or_else(|e| panic!("apply: {e}"));
        let mut manifest = graph_search_types::Manifest::new(1, 1);
        manifest.indexed_at_ms = 99;
        store
            .commit_manifest(manifest)
            .unwrap_or_else(|e| panic!("commit: {e}"));
    } // closed here: Grafeo flushes on drop

    let store = open(tmp.path());
    let snapshot = store.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
    let nodes = snapshot
        .all_nodes()
        .unwrap_or_else(|e| panic!("all_nodes: {e}"));
    assert_eq!(
        nodes.len(),
        4,
        "the projection survives the reopen: {nodes:?}"
    );
    assert_eq!(snapshot.counts().total_nodes, 4);
    assert_eq!(snapshot.counts().total_edges, 1);
    let manifest = store.manifest().unwrap_or_else(|e| panic!("manifest: {e}"));
    assert_eq!(manifest.map(|m| m.indexed_at_ms), Some(99));
}

#[test]
fn dangling_references_survive_and_filter_by_file() {
    let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    {
        let mut store = open(tmp.path());
        let mut batch = conformance::fixture_batch();
        let alpha = NodeId::symbol("src/a.rs", NodeKind::Function, "a", None);
        batch.upserts[0]
            .edges
            .push(graph_search_types::Edge::dangling(
                &alpha,
                EdgeKind::Calls,
                "ghost_function",
                Some("src/a.rs"),
                Some(2),
            ));
        store.apply(batch).unwrap_or_else(|e| panic!("apply: {e}"));

        let snapshot = store.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
        let dangling = snapshot
            .edges_from(
                &alpha,
                &[EdgeKind::Calls],
                graph_search_types::kind::Direction::Out,
            )
            .unwrap_or_else(|e| panic!("edges_from: {e}"))
            .into_iter()
            .filter(|edge| !edge.resolved)
            .count();
        assert_eq!(dangling, 1, "the ghost call is kept by name");
        assert_eq!(snapshot.counts().total_edges, 2);
    }

    // Re-opening keeps it: the sidecar persists beside the store.
    assert!(
        tmp.path().join("CURRENT").exists(),
        "the generation is published"
    );
    let store = open(tmp.path());
    let snapshot = store.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
    let all = snapshot
        .all_edges()
        .unwrap_or_else(|e| panic!("all_edges: {e}"));
    assert_eq!(
        all.iter().filter(|edge| !edge.resolved).count(),
        1,
        "{all:?}"
    );
}

#[test]
fn native_metadata_rebuilds_on_publication_and_reopen() {
    let tmp = TempDir::new().expect("tmp");
    let mut store = open(tmp.path());
    let mut batch = conformance::fixture_batch();
    batch.upserts[0].symbols[0].name = Some("renamed".into());
    batch.upserts[0].symbols[0].qualified_name = Some("scope::renamed".into());
    store.apply(batch).expect("publish");
    drop(store);
    let mut store = open(tmp.path());
    {
        let snapshot = store.snapshot().expect("snapshot");
        assert!(
            snapshot
                .find_by_name("a", &[], 10)
                .expect("old name")
                .is_empty()
        );
        assert_eq!(
            snapshot
                .find_by_name("scope::renamed", &[], 10)
                .expect("qualified")
                .len(),
            1
        );
        let mut budget = graph_search_core::work::WorkBudget::new(
            graph_search_core::work::WorkLimits::default(),
        );
        let hits = snapshot
            .metadata()
            .search(
                &graph_search_core::lexical::query_terms("renamed"),
                "absent",
                &graph_search_core::metadata::CompiledFilters::default(),
                &mut budget,
            )
            .expect("postings");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score > 0.0);
    }
    store
        .apply(graph_search_types::WriteBatch {
            removed_files: vec!["src/a.rs".into()],
            ..graph_search_types::WriteBatch::default()
        })
        .expect("remove");
    drop(store);
    let store = open(tmp.path());
    let snapshot = store.snapshot().expect("reopen");
    assert!(
        snapshot
            .find_by_name("renamed", &[], 10)
            .expect("gone")
            .is_empty()
    );
    let mut budget =
        graph_search_core::work::WorkBudget::new(graph_search_core::work::WorkLimits::default());
    assert!(
        snapshot
            .metadata()
            .search(
                &graph_search_core::lexical::query_terms("renamed"),
                "absent",
                &graph_search_core::metadata::CompiledFilters::default(),
                &mut budget
            )
            .expect("gone postings")
            .is_empty()
    );
}

#[test]
fn replacements_preserve_only_untouched_source_owned_edges() {
    use graph_search_types::{Edge, WriteBatch};
    let tmp = TempDir::new().expect("tmp");
    let mut stores: Vec<Box<dyn GraphStore>> =
        vec![Box::new(MemoryStore::new()), Box::new(open(tmp.path()))];
    for store in &mut stores {
        let mut batch = conformance::fixture_batch();
        let a = batch.upserts[0].symbols[0].id.clone();
        let b = batch.upserts[1].symbols[0].id.clone();
        let incoming = batch.upserts[0].edges[0].clone();
        let pathless = Edge::dangling(&a, EdgeKind::References, "missing", None, None);
        batch.upserts[0].edges.push(pathless.clone());
        // The explicit source owner differs from the endpoint's file.
        let foreign = Edge::resolved(&a, EdgeKind::References, &a, "a", Some("src/b.rs"), Some(1));
        batch.upserts[1].edges.push(foreign.clone());
        let outgoing = Edge::resolved(&b, EdgeKind::Calls, &a, "a", None, None);
        batch.upserts[1].edges.push(outgoing.clone());
        store.apply(batch.clone()).expect("seed");
        let mut replacement = batch.upserts[1].clone();
        replacement.edges.clear();
        replacement.symbols[0].signature = None;
        replacement.symbols[0].qualified_name = None;
        let expected = replacement.symbols[0].clone();
        let outcome = store
            .apply(WriteBatch {
                upserts: vec![replacement.clone()],
                ..WriteBatch::default()
            })
            .expect("replace");
        assert_eq!(outcome.nodes_deleted, 0);
        assert_eq!(outcome.nodes_upserted, 2);
        {
            let snapshot = store.snapshot().expect("snapshot");
            assert_eq!(snapshot.node_by_id(&b).expect("node"), Some(expected));
            let edges = snapshot.all_edges().expect("edges");
            assert!(edges.contains(&incoming));
            assert!(edges.contains(&pathless));
            assert!(!edges.contains(&foreign));
            assert!(!edges.contains(&outgoing));
        }
        replacement.symbols.clear();
        let outcome = store
            .apply(WriteBatch {
                upserts: vec![replacement],
                ..WriteBatch::default()
            })
            .expect("remove target");
        assert_eq!(outcome.nodes_deleted, 1);
        let edges = store
            .snapshot()
            .expect("snapshot")
            .all_edges()
            .expect("edges");
        assert!(!edges.contains(&incoming));
        assert!(edges.contains(&pathless));
        // Explicit removal is stronger than stable-id replacement.
        store.apply(batch.clone()).expect("reset");
        store
            .apply(WriteBatch {
                removed_files: vec![String::from("src/b.rs")],
                upserts: vec![batch.upserts[1].clone()],
                ..WriteBatch::default()
            })
            .expect("explicit removal");
        let edges = store
            .snapshot()
            .expect("snapshot")
            .all_edges()
            .expect("edges");
        assert!(!edges.contains(&incoming));
        assert!(edges.contains(&foreign));
    }
    let expected_nodes = stores[1]
        .snapshot()
        .expect("snapshot")
        .all_nodes()
        .expect("nodes");
    let expected_edges = stores[1]
        .snapshot()
        .expect("snapshot")
        .all_edges()
        .expect("edges");
    drop(stores);
    let reopened = open(tmp.path());
    let snapshot = reopened.snapshot().expect("snapshot");
    assert_eq!(snapshot.all_nodes().expect("nodes"), expected_nodes);
    assert_eq!(snapshot.all_edges().expect("edges"), expected_edges);
}
