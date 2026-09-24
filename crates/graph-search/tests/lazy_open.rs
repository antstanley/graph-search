//! Opening a published generation reads only its header and summary; every
//! other artifact is verified, validated and indexed by its first reader.
#![allow(clippy::unwrap_used)]

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::GraphStore;
use graph_search_engine::{NativeStore, StoreOptions};
use std::path::{Path, PathBuf};

fn indexed(root: &Path) -> PathBuf {
    std::fs::write(
        root.join("a.rs"),
        "fn helper() {}\nfn caller() { helper(); helper(); missing(); }\n",
    )
    .unwrap();
    std::fs::write(root.join("b.rs"), "fn other() { helper(); }\n").unwrap();
    std::fs::write(root.join("notes.md"), "# Notes\nhelper usage\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    index.store_dir().to_path_buf()
}

fn generation(store: &Path) -> PathBuf {
    let pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.join("CURRENT")).unwrap()).unwrap();
    store
        .join("generations")
        .join(pointer["id"].as_str().unwrap())
}

#[test]
fn status_reads_only_the_header_and_each_fact_fails_on_first_use() {
    let root = tempfile::tempdir().unwrap();
    let store_dir = indexed(root.path());
    let (counts, coverage, header) = {
        let store = NativeStore::open(&store_dir, &StoreOptions::default()).unwrap();
        let snapshot = store.snapshot().unwrap();
        (
            snapshot.counts().clone(),
            *snapshot.source_coverage(),
            store.manifest_header().unwrap(),
        )
    };
    assert!(counts.total_nodes > 0 && counts.total_edges > 0);
    assert_eq!(coverage.source_indexed_files, 3);
    let generation = generation(&store_dir);
    for artifact in [
        "shards.json",
        "source-units.json",
        "tables.json",
        "extractions.json",
        "dependencies.json",
    ] {
        std::fs::write(generation.join(artifact), b"damaged").unwrap();
    }
    let store = NativeStore::open(&store_dir, &StoreOptions::default()).unwrap();
    assert_eq!(store.manifest_header().unwrap(), header);
    assert!(store.generation().unwrap().is_some());
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.counts(), &counts);
    assert_eq!(snapshot.source_coverage(), &coverage);

    let mismatch = |result: Result<(), graph_search_core::Error>| {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("checksum mismatch"), "{error}");
    };
    mismatch(snapshot.all_nodes().map(drop));
    mismatch(snapshot.source_files().map(drop));
    mismatch(snapshot.occurrence_files().map(drop));
    mismatch(snapshot.occurrence_count("any").map(drop));
    mismatch(
        store
            .dependency_index()
            .and_then(|index| index.expect("coherent records").record("a.rs"))
            .map(drop),
    );
    mismatch(store.manifest().map(drop));
    // A failed load is not cached as an empty value.
    mismatch(snapshot.all_nodes().map(drop));

    // The same holds through the library: status succeeds, a graph query fails.
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    let status = index.search().status().unwrap();
    assert_eq!(status.counts.as_ref(), Some(&counts));
    assert_eq!(status.coverage.source_indexed_files, 3);
    assert!(
        index
            .search()
            .symbol(&graph_search_types::query::SymbolQuery::new("helper"))
            .is_err()
    );
}

#[test]
fn published_edge_counts_match_the_occurrence_index() {
    let root = tempfile::tempdir().unwrap();
    let store_dir = indexed(root.path());
    let store = NativeStore::open(&store_dir, &StoreOptions::default()).unwrap();
    let snapshot = store.snapshot().unwrap();
    // Answer every edge from the published table before the facts load.
    let edges = snapshot.all_edges().unwrap();
    let published: Vec<_> = edges
        .iter()
        .map(|edge| snapshot.occurrence_count(edge.id.as_str()).unwrap())
        .collect();
    assert!(published.contains(&Some(2)), "caller calls helper twice");
    let index = snapshot.occurrences().unwrap();
    for (edge, count) in edges.iter().zip(published) {
        assert_eq!(count, index.count_for_edge(edge.id.as_str()), "{}", edge.id);
    }
}
