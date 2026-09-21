//! Source identity, stale evidence, and the strength of freshness checks.

#![allow(clippy::expect_used, clippy::panic)]

use graph_search::{Index, OpenOptions, Reconcile, Verification};
use graph_search_types::context::{FreshnessMethod, SourceVerification};
use graph_search_types::query::{ExploreQuery, SymbolQuery, TextQuery, TraversalQuery};

fn index(root: &std::path::Path, reconcile: Reconcile, verification: Verification) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        reconcile,
        verification,
        ..Default::default()
    })
    .expect("open index")
}

#[test]
fn indexed_snippets_never_use_changed_source() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn original() {}\n").unwrap();
    let index = index(dir.path(), Reconcile::Never, Verification::Metadata);
    index.reindex().unwrap();
    let before = index
        .search()
        .explore(&ExploreQuery::new("original"))
        .unwrap();
    let generation = before.context.generation.clone();
    assert!(generation.is_some());
    let source = &before.context.sources["a.rs"];
    assert_eq!(source.verification, SourceVerification::Verified);
    let snippet = before
        .items
        .iter()
        .find(|item| item.node.name == "original")
        .unwrap()
        .snippet
        .as_ref()
        .unwrap();
    assert_eq!(
        Some(snippet.source_hash.as_str()),
        source.indexed_hash.as_deref()
    );
    std::fs::write(dir.path().join("a.rs"), "fn replacement_function() {}\n").unwrap();
    let after = index
        .search()
        .explore(&ExploreQuery::new("original"))
        .unwrap();
    assert_eq!(after.context.generation, generation);
    assert_eq!(after.context.reconciliation.as_deref(), Some("never"));
    assert_eq!(
        after.context.sources["a.rs"].verification,
        SourceVerification::Mismatch
    );
    assert_eq!(after.context.staleness.changed_paths, vec!["a.rs"]);
    let symbol = after
        .items
        .iter()
        .find(|item| item.node.name == "original")
        .unwrap();
    assert!(symbol.snippet.is_none());
    assert_eq!(symbol.node.signature.as_deref(), Some("fn original() {}"));
}

#[test]
fn restored_mtime_is_detected_by_snippets_and_strict_graph_queries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    let original = "fn original() {}\n";
    let replaced = "fn renamedd() {}\n";
    assert_eq!(original.len(), replaced.len());
    std::fs::write(&path, original).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let fast = index(dir.path(), Reconcile::BeforeQuery, Verification::Metadata);
    fast.reindex().unwrap();
    let old_generation = fast
        .search()
        .symbol(&SymbolQuery::new("original"))
        .unwrap()
        .context
        .generation;
    std::fs::write(&path, replaced).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let result = fast
        .search()
        .explore(&ExploreQuery::new("original"))
        .unwrap();
    assert_eq!(result.context.freshness, FreshnessMethod::Metadata);
    assert_eq!(
        result.context.sources["a.rs"].verification,
        SourceVerification::Mismatch
    );
    assert_eq!(result.context.staleness.changed, 1);
    assert!(result.items.iter().all(|item| item.snippet.is_none()));
    drop(fast);
    let strict = index(dir.path(), Reconcile::BeforeQuery, Verification::Content);
    let result = strict
        .search()
        .symbol(&SymbolQuery::new("renamedd"))
        .unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.context.freshness, FreshnessMethod::Content);
    assert_eq!(result.context.staleness.changed, 0);
    assert_ne!(result.context.generation, old_generation);
    assert_eq!(
        result.context.sources["a.rs"].verification,
        SourceVerification::Verified
    );
}

#[test]
fn graph_and_impact_context_describe_the_selected_generation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "fn entry() { leaf(); } fn leaf() {}\n",
    )
    .unwrap();
    let index = index(dir.path(), Reconcile::Explicit, Verification::Content);
    index.reindex().unwrap();
    let graph = index.search().symbol(&SymbolQuery::new("leaf")).unwrap();
    let impact = index
        .search()
        .impact(&TraversalQuery::new("leaf", 1))
        .unwrap();
    assert_eq!(graph.context.generation, impact.context.generation);
    assert_eq!(graph.context.reconciliation.as_deref(), Some("explicit"));
    assert_eq!(impact.context.freshness, FreshnessMethod::Content);
    std::fs::remove_file(dir.path().join("a.rs")).unwrap();
    let result = index.search().explore(&ExploreQuery::new("leaf")).unwrap();
    assert_eq!(
        result.context.sources["a.rs"].verification,
        SourceVerification::Unavailable
    );
    assert!(result.items.iter().all(|item| item.snippet.is_none()));
    assert_eq!(result.context.staleness.changed, 1);
}

#[test]
fn live_text_and_body_evidence_carry_the_bytes_they_used() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("guide.md"), "old documentation\n").unwrap();
    let index = index(dir.path(), Reconcile::Never, Verification::Metadata);
    index.reindex().unwrap();
    let new = "unique_body_needle café\r\n";
    std::fs::write(dir.path().join("guide.md"), new).unwrap();
    let text = index.search().text(&TextQuery::new("café")).unwrap();
    assert_eq!(text.context.freshness, FreshnessMethod::Live);
    let hash = graph_search::core::hash::content_hash(new.as_bytes());
    assert_eq!(
        text.context.sources["guide.md"].observed_hash.as_deref(),
        Some(hash.as_str())
    );
    let result = index
        .search()
        .explore(&ExploreQuery::new("unique_body_needle"))
        .unwrap();
    let body = result
        .items
        .iter()
        .find(|item| item.node.path == "guide.md")
        .unwrap();
    let snippet = body.snippet.as_ref().unwrap();
    assert_eq!(snippet.source_hash, hash);
    assert_eq!(snippet.lines, vec!["unique_body_needle café"]);
    assert_eq!(
        result.context.sources["guide.md"].verification,
        SourceVerification::Mismatch
    );
}

#[test]
fn each_graph_request_uses_one_complete_freshness_observation() {
    use graph_search::{CancellationToken, WorkLimits};
    let dir = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    let text = "fn leaf() {}\nfn main() { leaf(); }\n";
    std::fs::write(&path, text).unwrap();
    let index = Index::open(OpenOptions {
        root: dir.path().into(),
        store: Some(store.path().join("index")),
        reconcile: Reconcile::Never,
        verification: Verification::Content,
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let limits = WorkLimits {
        walk_entries: 2, // Root and one source file; exactly one full enumeration.
        source_files: 1,
        source_bytes: text.len(),
        ..WorkLimits::default()
    };
    let mut generation = None;
    for _ in 0..2 {
        for mode in 0..4 {
            let (context, stats) = freshness_result(&index, &limits, mode);
            assert_eq!(context.staleness.changed, 0);
            assert_eq!(context.coverage.enumeration_complete, Some(true));
            assert_eq!(stats.source_files_attempted, 1);
            assert_eq!(stats.source_bytes_read, text.len() as u64);
            if generation.is_some() {
                assert_eq!(context.generation, generation);
            }
            generation = context.generation;
        }
    }
    // Reuse ends with the request: an equal-size, restored-mtime edit must be
    // hashed again and reported without falsely marking old graph facts current.
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, text.replace("leaf", "twig")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let changed = index
        .search()
        .with_work_limits(limits.clone())
        .symbol(&SymbolQuery::new("leaf"))
        .unwrap();
    assert_eq!(changed.context.staleness.changed_paths, ["a.rs"]);
    assert_eq!(changed.context.generation, generation);
    assert_eq!(
        changed.context.sources["a.rs"].verification,
        SourceVerification::NotRead
    );
    for partial in [
        WorkLimits {
            walk_entries: 1,
            ..limits.clone()
        },
        WorkLimits {
            source_files: 0,
            ..limits.clone()
        },
    ] {
        assert!(
            index
                .search()
                .with_work_limits(partial)
                .symbol(&SymbolQuery::new("leaf"))
                .is_err()
        );
    }
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(
        index
            .search()
            .with_work_limits(WorkLimits {
                cancellation: Some(cancellation),
                ..limits
            })
            .symbol(&SymbolQuery::new("leaf"))
            .is_err()
    );
}

fn freshness_result(
    index: &Index,
    limits: &graph_search::WorkLimits,
    mode: usize,
) -> (
    graph_search_types::context::ResultContext,
    graph_search_types::result::Stats,
) {
    use graph_search_types::occurrence::OccurrenceQuery;
    use graph_search_types::{ExploreMode, GraphContext};
    let search = index.search().with_work_limits(limits.clone());
    match mode {
        0 => {
            let result = search
                .symbol(&SymbolQuery::new("leaf"))
                .expect("complete bounded freshness query");
            (result.context, result.stats)
        }
        1 => {
            let result = search
                .impact(&TraversalQuery::new("leaf", 1))
                .expect("complete bounded freshness query");
            (result.context, result.stats)
        }
        2 => {
            let result = search
                .occurrences(&OccurrenceQuery {
                    target: "leaf".into(),
                    ..Default::default()
                })
                .expect("complete bounded freshness query");
            (result.context, result.stats)
        }
        _ => {
            let mut query = ExploreQuery::new("leaf").with_context_lines(0);
            query.retrieval.mode = ExploreMode::ExactName;
            query.retrieval.graph_context = GraphContext::None;
            let result = search
                .explore(&query)
                .expect("complete bounded freshness query");
            (result.context, result.stats)
        }
    }
}
