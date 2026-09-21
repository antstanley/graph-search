//! Public positional routes verify source before grouping or top-k.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::work::WorkLimits;
use graph_search_types::{ExploreMode, ExploreQuery, RetrievalRoute, result::TruncationKind};

fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, Index) {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        std::fs::write(root.path().join(path), text).unwrap();
    }
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    (root, index)
}
fn phrase(text: &str) -> ExploreQuery {
    let mut query = ExploreQuery::new(text);
    query.retrieval.mode = ExploreMode::Phrase;
    query.retrieval.explain = true;
    query
}

#[test]
fn whole_lexemes_stopwords_multiplicity_and_no_metadata_fallback() {
    let (_root, index) = fixture(&[
        ("trap.rs", "fn cache_invalidate() {}\n"),
        ("guide.md", "cache\r\ninvalidate\nis a thing\na\n"),
    ]);
    let result = index.search().explore(&phrase("cache invalidate")).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].node.path, "guide.md");
    assert_eq!(result.plan.unwrap().routes, vec![RetrievalRoute::Phrase]);
    let evidence = result.items[0].evidence.as_ref().unwrap();
    assert_eq!((evidence.span.start_line, evidence.span.end_line), (1, 2));
    assert!(
        index
            .search()
            .explore(&phrase("a a"))
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        index.search().explore(&phrase("is a")).unwrap().items.len(),
        1
    );
    assert!(
        index
            .search()
            .explore(&phrase("invalidate cache"))
            .unwrap()
            .items
            .is_empty()
    );
    let mut near = phrase("invalidate cache");
    near.retrieval.mode = ExploreMode::Near;
    near.retrieval.near_window = 2;
    let result = index.search().explore(&near).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.plan.unwrap().routes, vec![RetrievalRoute::Near]);
}

#[test]
fn storage_windows_do_not_bound_matching_or_declaration_ownership() {
    let text = format!(
        "fn long() {{\n// alpha\n{}// omega\n}}\n",
        "// filler\n".repeat(200)
    );
    let (_root, index) = fixture(&[("long.rs", &text)]);
    let mut query = phrase("alpha omega");
    query.retrieval.phrase_gap = 200;
    query.k = 1;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items.len(), 1);
    let hit = &result.items[0];
    assert_eq!(hit.node.name, "long");
    let evidence = hit.evidence.as_ref().unwrap();
    assert_eq!((evidence.span.start_line, evidence.span.end_line), (2, 203));
    assert!(evidence.owner.is_some());
    assert_eq!(
        &text[evidence.span.start_byte as usize..evidence.span.end_byte as usize],
        format!("alpha\n{}// omega", "// filler\n".repeat(200))
    );
    query.retrieval.phrase_gap = 199;
    assert!(index.search().explore(&query).unwrap().items.is_empty());
}

#[test]
fn overlaps_and_different_owners_survive_single_source_scan() {
    let (_root, index) = fixture(&[(
        "a.rs",
        "fn first() { /* a a a */ }\nfn second() { /* a a */ }\n",
    )]);
    let result = index.search().explore(&phrase("a a")).unwrap();
    assert_eq!(result.stats.positional_witnesses, 3);
    assert_eq!(result.stats.files_scanned, 1);
    assert_eq!(result.items.len(), 2);
    assert_eq!(
        result
            .items
            .iter()
            .map(|hit| hit.node.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(
        result
            .items
            .iter()
            .all(|hit| hit.evidence.as_ref().unwrap().owner.is_some())
    );
}

#[test]
fn edited_source_uses_live_file_witnesses_without_stale_ownership() {
    let (root, index) = fixture(&[("a.rs", "fn old() { /* absent */ }\n")]);
    std::fs::write(root.path().join("a.rs"), "fn new() { /* alpha beta */ }\n").unwrap();
    let result = index.search().explore(&phrase("alpha beta")).unwrap();
    assert_eq!(result.items.len(), 1);
    let evidence = result.items[0].evidence.as_ref().unwrap();
    assert!(evidence.live);
    assert!(evidence.owner.is_none());
    assert_eq!(result.items[0].node.id, "file:a.rs");
    std::fs::write(root.path().join("a.rs"), "fn new() {}\n").unwrap();
    assert!(
        index
            .search()
            .explore(&phrase("alpha beta"))
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn positional_caps_are_shared_and_zero_context_preserves_proof() {
    let (_root, index) = fixture(&[("a.md", "alpha beta\n"), ("b.md", "alpha beta\n")]);
    let mut query = phrase("alpha beta");
    query.context_lines = 0;
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            positional_witnesses: 1,
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.stats.positional_witnesses, 1);
    assert!(result.items[0].snippet.is_none());
    assert!(result.items[0].excerpts.is_empty());
    assert!(result.items[0].evidence.is_some());
    assert!(
        result
            .truncations
            .iter()
            .any(|limit| limit.kind == TruncationKind::PositionalWitnesses)
    );
    let empty = index
        .search()
        .with_work_limits(WorkLimits {
            positional_bytes: 0,
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert!(empty.items.is_empty());
    assert!(
        empty
            .truncations
            .iter()
            .any(|limit| limit.kind == TruncationKind::PositionalBytes)
    );
}

#[test]
fn unicode_original_offsets_survive_reopen_and_reindex() {
    let text = "prefix\r\nİ_NAME::ÉCOLE\r\nend\n";
    let (root, index) = fixture(&[("guide.md", text)]);
    let query = phrase("i\u{307}_name école");
    let before = index.search().explore(&query).unwrap();
    let evidence = before.items[0].evidence.as_ref().unwrap();
    assert_eq!((evidence.span.start_line, evidence.span.end_line), (2, 2));
    assert_eq!(
        &text[evidence.span.start_byte as usize..evidence.span.end_byte as usize],
        "İ_NAME::ÉCOLE"
    );
    drop(index);
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    assert_eq!(
        index.search().explore(&query).unwrap().items[0].evidence,
        before.items[0].evidence
    );
    index.reindex().unwrap();
    assert_eq!(
        index.search().explore(&query).unwrap().items[0].evidence,
        before.items[0].evidence
    );
}

#[test]
fn invalid_contract_fails_before_freshness_work() {
    let (_root, index) = fixture(&[("a.md", "alpha beta\n")]);
    let mut query = phrase("alpha beta");
    query.retrieval.mode = ExploreMode::Near;
    query.retrieval.near_window = 1;
    let error = index
        .search()
        .with_work_limits(WorkLimits {
            deadline: Some(std::time::Instant::now()),
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap_err();
    assert!(matches!(
        error,
        graph_search::Error::Core(graph_search_core::Error::InvalidQuery(_))
    ));
}

#[test]
fn unchecked_core_host_scans_source_without_spending_posting_allowance() {
    use graph_search_core::GraphStore;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "alpha beta\n").unwrap();
    let store = graph_search_core::memory::MemoryStore::new();
    let snapshot = store.snapshot().unwrap();
    let result = graph_search_core::query::QueryEngine::with_work_limits(
        snapshot.as_ref(),
        WorkLimits {
            postings: 0,
            candidates: 1,
            ..WorkLimits::default()
        },
    )
    .explore(&phrase("alpha beta"), root.path())
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.stats.lexical_postings_examined, 0);
    assert!(result.items[0].evidence.as_ref().unwrap().live);
}

#[test]
fn filters_precede_source_verification_and_exact_fill_is_complete() {
    let (_root, index) = fixture(&[("a.md", "alpha beta\n"), ("b.md", "alpha beta\n")]);
    let mut query = phrase("alpha beta");
    query.filters.path_glob = Some("b.md".into());
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            positional_witnesses: 1,
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].node.path, "b.md");
    assert_eq!(result.stats.files_scanned, 1);
    assert!(
        !result
            .truncations
            .iter()
            .any(|limit| limit.kind == TruncationKind::PositionalWitnesses)
    );
}
