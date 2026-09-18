//! The store conformance suite: the port as executable contract
//! (`SPEC.md` §15.4).
//!
//! The same functions run against the in-memory fake and the Grafeo adapter,
//! so the engine is provably swappable. Each check panics with a message on
//! violation; callers run them in `#[test]`s — the panic-family lints are
//! permitted here for the same reason they are in tests
//! (see `clippy.toml`).
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use crate::memory::MemoryStore;
use crate::ports::GraphStore;
use graph_search_types::batch::{FileProjection, WriteBatch};
use graph_search_types::kind::{EdgeKind, NodeKind};
use graph_search_types::manifest::Manifest;
use graph_search_types::node::{Edge, Node, Span};
use graph_search_types::{ApplyOutcome, EdgeId, NodeId, PARSER_VERSION, SCHEMA_VERSION};

/// A deterministic two-file fixture: `a.rs` calls `b`'s function.
#[must_use]
pub fn fixture_batch() -> WriteBatch {
    let file_a = Node::file(
        "src/a.rs",
        graph_search_types::Language::Rust,
        10,
        2,
        "ha",
        PARSER_VERSION,
    );
    let file_b = Node::file(
        "src/b.rs",
        graph_search_types::Language::Rust,
        12,
        3,
        "hb",
        PARSER_VERSION,
    );
    let fn_a = Node {
        id: NodeId::symbol("src/a.rs", NodeKind::Function, "a", None),
        kind: NodeKind::Function,
        path: String::from("src/a.rs"),
        name: Some(String::from("a")),
        qualified_name: Some(String::from("a")),
        signature: Some(String::from("fn a()")),
        span: Some(Span::new(1, 2, 0, 10)),
        ..Node::default()
    };
    let fn_b = Node {
        id: NodeId::symbol("src/b.rs", NodeKind::Function, "b", None),
        kind: NodeKind::Function,
        path: String::from("src/b.rs"),
        name: Some(String::from("b")),
        qualified_name: Some(String::from("b")),
        signature: Some(String::from("fn b()")),
        span: Some(Span::new(1, 3, 0, 12)),
        ..Node::default()
    };
    let call = Edge::resolved(
        &fn_a.id,
        EdgeKind::Calls,
        &fn_b.id,
        "b",
        Some("src/a.rs"),
        Some(2),
    );
    let mut a = FileProjection {
        file: file_a,
        ..FileProjection::default()
    };
    a.symbols.push(fn_a);
    a.edges.push(call);
    let mut b = FileProjection {
        file: file_b,
        ..FileProjection::default()
    };
    b.symbols.push(fn_b);
    let mut batch = WriteBatch::with_manifest(Manifest::new(PARSER_VERSION, SCHEMA_VERSION));
    batch.upserts.push(a);
    batch.upserts.push(b);
    batch
}

/// Applies the fixture and asserts the read side round-trips.
pub fn check_apply_and_read(store: &mut dyn GraphStore) {
    let batch = fixture_batch();
    let outcome = store.apply(batch).unwrap_or_else(|e| panic!("apply: {e}"));
    assert_eq!(
        outcome.nodes_upserted, 4,
        "two files, two symbols: {outcome:?}"
    );
    let snapshot = store.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
    let nodes = snapshot
        .all_nodes()
        .unwrap_or_else(|e| panic!("all_nodes: {e}"));
    assert_eq!(nodes.len(), 4);
    let target = NodeId::symbol("src/b.rs", NodeKind::Function, "b", None);
    let node = snapshot
        .node_by_id(&target)
        .unwrap_or_else(|e| panic!("node_by_id: {e}"))
        .unwrap_or_else(|| panic!("b must exist"));
    assert_eq!(node.name.as_deref(), Some("b"));
    let edges = snapshot
        .edges_from(
            &NodeId::symbol("src/a.rs", NodeKind::Function, "a", None),
            &[EdgeKind::Calls],
            graph_search_types::kind::Direction::Out,
        )
        .unwrap_or_else(|e| panic!("edges_from: {e}"));
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].to.as_ref().map(ToString::to_string),
        Some(target.to_string())
    );
}

/// Asserts replace-subtree semantics: re-projecting a file drops its old
/// nodes and incident edges.
pub fn check_replace_subtree(store: &mut dyn GraphStore) {
    let mut batch = fixture_batch();
    store
        .apply(batch.clone())
        .unwrap_or_else(|e| panic!("apply: {e}"));

    // Re-project a.rs without the symbol: its node and the call edge vanish.
    let file_a = Node::file(
        "src/a.rs",
        graph_search_types::Language::Rust,
        4,
        1,
        "ha2",
        PARSER_VERSION,
    );
    batch.upserts = vec![FileProjection {
        file: file_a,
        ..FileProjection::default()
    }];
    store
        .apply(batch)
        .unwrap_or_else(|e| panic!("apply 2: {e}"));

    let snapshot = store.snapshot().unwrap_or_else(|e| panic!("snapshot: {e}"));
    let nodes = snapshot
        .all_nodes()
        .unwrap_or_else(|e| panic!("all_nodes: {e}"));
    assert_eq!(nodes.len(), 3, "a's symbol is gone: {nodes:?}");
    let edges = snapshot
        .all_edges()
        .unwrap_or_else(|e| panic!("all_edges: {e}"));
    assert!(
        edges.iter().all(|edge| edge.kind != EdgeKind::Calls),
        "the dangling call edge must be gone: {edges:?}"
    );
}

/// Asserts manifest commit-last: a manifest is invisible before it is
/// committed and visible after.
pub fn check_manifest_commit_last(store: &mut dyn GraphStore) {
    assert!(
        store
            .manifest()
            .unwrap_or_else(|e| panic!("manifest: {e}"))
            .is_none(),
        "a fresh store has no manifest"
    );
    let mut manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
    manifest.indexed_at_ms = 42;
    store
        .commit_manifest(manifest)
        .unwrap_or_else(|e| panic!("commit_manifest: {e}"));
    let manifest = store.manifest().unwrap_or_else(|e| panic!("manifest: {e}"));
    assert_eq!(manifest.map(|m| m.indexed_at_ms), Some(42));
}

/// The full suite, for tests that own a store.
pub fn run_all(store: &mut dyn GraphStore) {
    check_apply_and_read(store);
    check_replace_subtree(store);
    check_manifest_commit_last(store);
}

/// Runs the suite against a fresh in-memory store; used by core's own tests
/// and as the minimal smoke check anywhere.
pub fn smoke_test_memory() {
    let mut store = MemoryStore::new();
    run_all(&mut store);
}

/// Alias so callers can name the outcome type without importing it.
pub type ApplyOutcomeOf = ApplyOutcome;

/// Unused-import guard for the edge id type used in fixtures.
#[allow(dead_code)]
fn _touch() {
    let _ = EdgeId::of(&NodeId::file("x"), EdgeKind::Contains, "file:y");
    let _ = SCHEMA_VERSION;
}
