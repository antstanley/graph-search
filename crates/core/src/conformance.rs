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
    assert_eq!(snapshot.counts().total_nodes, 4);
    assert_eq!(snapshot.counts().total_edges, 1);
    assert_eq!(
        snapshot.counts().files_by_language[&graph_search_types::Language::Rust],
        2
    );
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
    assert_eq!(snapshot.counts().total_nodes, 3);
    assert_eq!(snapshot.counts().total_edges, 0);
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

/// Freshness reads preserve every header field while raw facts remain available to sync.
pub fn check_manifest_header(store: &mut dyn GraphStore) {
    let mut manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
    manifest.indexed_at_ms = 73;
    manifest.policy_fingerprint = Some("header-policy".into());
    manifest.entries.insert(
        "src/a.rs".into(),
        graph_search_types::manifest::FileEntry {
            size: 17,
            mtime_ns: 29,
            content_hash: "source-hash".into(),
            parser_version: PARSER_VERSION,
            schema_version: SCHEMA_VERSION,
            quarantine: Some("reported quarantine".into()),
            extraction: Some(graph_search_types::extraction::Extraction::default().into()),
        },
    );
    store
        .commit_manifest(manifest.clone())
        .expect("publish raw facts");
    let header = store.manifest_header().expect("header").expect("indexed");
    let mut expected = manifest.clone();
    expected
        .entries
        .get_mut("src/a.rs")
        .expect("entry")
        .extraction = None;
    assert_eq!(header, expected);
    assert_eq!(store.manifest().expect("raw facts"), Some(manifest));
}

/// Work limits have identical semantics in every adapter and reset per query.
pub fn check_bounded_queries(store: &mut dyn GraphStore) {
    use crate::query::QueryEngine;
    use crate::work::WorkLimits;
    use graph_search_types::query::TraversalQuery;
    use graph_search_types::result::TruncationKind;
    store.apply(fixture_batch()).expect("fixture");
    let snapshot = store.snapshot().expect("snapshot");
    let query = TraversalQuery::new("a", 4);
    let engine = QueryEngine::with_work_limits(
        snapshot.as_ref(),
        WorkLimits {
            edges: 0,
            ..WorkLimits::default()
        },
    );
    let result = engine.callees(&query).expect("partial result");
    assert!(result.edges.is_empty());
    assert_eq!(result.stats.graph_edges_examined, 0);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::GraphEdges)
    );
    let engine = QueryEngine::with_work_limits(
        snapshot.as_ref(),
        WorkLimits {
            nodes: 1,
            ..WorkLimits::default()
        },
    );
    let result = engine.callees(&query).expect("node cap");
    assert_eq!(result.stats.graph_nodes_visited, 1);
    assert_eq!(result.nodes.len(), 1);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::GraphNodes)
    );
    let engine = QueryEngine::with_work_limits(
        snapshot.as_ref(),
        WorkLimits {
            edges: 1,
            ..WorkLimits::default()
        },
    );
    for _ in 0..2 {
        let result = engine
            .callees(&TraversalQuery::new("a", 1))
            .expect("reset budget");
        assert_eq!(result.stats.graph_edges_examined, 1);
        assert_eq!(result.edges.len(), 1);
        assert!(result.truncations.is_empty());
    }
}

/// Connection and impact phases reuse adjacency, and a new request starts fresh.
pub fn check_shared_explore_neighborhoods(store: &mut dyn GraphStore) {
    use crate::query::QueryEngine;
    use graph_search_types::query::ExploreQuery;
    let mut batch = fixture_batch();
    for file in &mut batch.upserts {
        file.symbols[0].name = Some("target".into());
    }
    store.apply(batch).expect("two matching seeds");
    let snapshot = store.snapshot().expect("snapshot");
    let engine = QueryEngine::new(snapshot.as_ref());
    let query = ExploreQuery::new("target").with_k(2).with_context_lines(0);
    let first = engine
        .explore(&query, std::path::Path::new("."))
        .expect("explore");
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.edges.len(), 1);
    for item in &first.items {
        let expected = u64::from(item.node.path == "src/b.rs");
        let impact = item.impact.as_ref().expect("function impact");
        assert_eq!(impact.direct_callers, expected);
        assert_eq!(impact.total_callers, expected);
    }
    assert!(
        first.stats.graph_edges_examined <= 4,
        "one incident read per endpoint plus connecting path work"
    );
    let second = engine
        .explore(&query, std::path::Path::new("."))
        .expect("next request");
    assert_eq!(
        second.stats.graph_edges_examined,
        first.stats.graph_edges_examined
    );
    assert_eq!(second.items, first.items);
    assert_eq!(second.edges, first.edges);
}

fn package_batch(hash: &str) -> WriteBatch {
    use graph_search_types::Language;
    use graph_search_types::package::{PackageEcosystem, PackageManifest, PackageRole};
    let mut batch = WriteBatch::default();
    for (path, text, language) in [
        ("scope/package.json", "{}", Language::Unknown),
        ("scope/item.ts", "const item = 1;", Language::TypeScript),
    ] {
        let mut source = crate::units::extract(path, text, hash, language, &[]);
        if path.ends_with("package.json") {
            source.package_manifest = Some(PackageManifest {
                node: None,
                cargo_targets: None,
                ecosystem: PackageEcosystem::Node,
                role: PackageRole::Package,
                name: Some("scope".into()),
                unavailable_reason: None,
            });
        }
        batch.upserts.push(FileProjection {
            file: Node::file(path, language, text.len() as u64, 1, hash, PARSER_VERSION),
            source: Some(source),
            ..FileProjection::default()
        });
    }
    let mut catalog = crate::packages::Catalog::new(
        &batch
            .upserts
            .iter()
            .map(|item| item.file.path.clone())
            .collect(),
    );
    for item in &batch.upserts {
        catalog.add(&item.file.path, item.source.as_ref().unwrap());
    }
    for item in &mut batch.upserts {
        catalog.annotate(&mut item.file, item.source.as_mut());
    }
    batch
}

/// Cross-file package evidence must remain coherent through atomic updates.
pub fn check_package_ownership(store: &mut dyn GraphStore) {
    store
        .apply(package_batch("package-old"))
        .expect("package fixture");
    let before = store.snapshot().unwrap().source_files().clone();
    let removal = WriteBatch {
        removed_files: vec!["scope/package.json".into()],
        ..WriteBatch::default()
    };
    assert!(
        store.apply(removal).is_err(),
        "a retained source cannot refer to a deleted manifest"
    );
    assert_eq!(store.snapshot().unwrap().source_files(), &before);
    let mut manifest_only = package_batch("package-new");
    manifest_only
        .upserts
        .retain(|item| item.file.path.ends_with("package.json"));
    assert!(
        store.apply(manifest_only).is_err(),
        "a retained source cannot claim the old manifest hash"
    );
    assert_eq!(store.snapshot().unwrap().source_files(), &before);
    let mut wrong_hash = package_batch("package-old");
    let item = &mut wrong_hash.upserts[1];
    let package = item.source.as_mut().unwrap().package.as_mut().unwrap();
    package.manifest_hash = "unrelated".into();
    item.file.attributes.insert(
        "package_context".into(),
        serde_json::to_string(package).unwrap(),
    );
    assert!(
        store.apply(wrong_hash).is_err(),
        "source and file metadata cannot jointly forge manifest identity"
    );
    assert_eq!(store.snapshot().unwrap().source_files(), &before);
    store
        .apply(package_batch("package-new"))
        .expect("coherent package replacement");
    assert_eq!(
        store.snapshot().unwrap().source_files()["scope/item.ts"]
            .package
            .as_ref()
            .unwrap()
            .manifest_hash,
        "package-new"
    );
}

/// Invalid source ownership must fail before any removals or replacements.
pub fn check_source_ownership(store: &mut dyn GraphStore) {
    let mut valid = fixture_batch();
    let a = &mut valid.upserts[0];
    a.source = Some(crate::units::extract(
        &a.file.path,
        "fn a() {}\n",
        "ha",
        graph_search_types::Language::Rust,
        &a.symbols,
    ));
    store.apply(valid.clone()).expect("valid source owner");
    let before = store.snapshot().expect("snapshot").source_files().clone();
    let original_ids: Vec<_> = store
        .snapshot()
        .expect("snapshot")
        .all_nodes()
        .expect("nodes")
        .into_iter()
        .map(|node| node.id)
        .collect();
    for fault in 0usize..14 {
        let mut invalid = valid.clone();
        invalid.removed_files.push("src/b.rs".into());
        invalid.upserts.truncate(1);
        let a = &mut invalid.upserts[0];
        match fault {
            0 => a.source.as_mut().unwrap().units[0].owner = Some(NodeId::new("missing")),
            1 => a.symbols[0].path = "src/b.rs".into(),
            2 => a.symbols[0].span = Some(Span::new(1, 1, 0, 1)),
            3 => a.symbols[0].kind = NodeKind::File,
            _ => {
                let unit = &mut a.source.as_mut().unwrap().units[0];
                let occurrence_fault = fault.saturating_sub(4) % 5;
                if occurrence_fault == 3 {
                    // Keep both lines inside the owner so only ordering is invalid.
                    unit.span.end_line = 2;
                }
                let field = if fault < 9 {
                    &mut unit.terms
                } else {
                    &mut unit.identifiers
                };
                let (term, lines) = match occurrence_fault {
                    0 => ("invalid", vec![]),
                    1 => ("invalid", vec![0]),
                    2 => ("invalid", vec![2]),
                    3 => ("invalid", vec![2, 1]),
                    _ => ("", vec![1]),
                };
                field.insert(term.into(), lines);
            }
        }
        assert!(store.apply(invalid).is_err(), "source fact fault {fault}");
        let snapshot = store.snapshot().expect("snapshot after rejection");
        assert_eq!(snapshot.source_files(), &before);
        let ids: Vec<_> = snapshot
            .all_nodes()
            .expect("nodes")
            .into_iter()
            .map(|node| node.id)
            .collect();
        assert_eq!(ids, original_ids, "no deletion on rejection");
    }
}

/// The full suite, for tests that own a store.
pub fn run_all(store: &mut dyn GraphStore) {
    check_apply_and_read(store);
    // Replacing both endpoints must restore the edge regardless of file order.
    check_apply_and_read(store);
    check_replace_subtree(store);
    check_manifest_commit_last(store);
    check_bounded_queries(store);
    check_shared_explore_neighborhoods(store);
    check_source_ownership(store);
    check_package_ownership(store);
    check_manifest_header(store);
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
