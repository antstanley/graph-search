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
    }

    // Re-opening keeps it: the sidecar persists beside the store.
    assert!(
        tmp.path().join("dangling.jsonl").exists(),
        "the sidecar is written"
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
