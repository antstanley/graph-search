//! Public graph budgets include library-added provenance.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fmt::Write;

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_types::limits::MAX_TOTAL_BYTES;
use graph_search_types::query::TraversalQuery;
use graph_search_types::result::TruncationKind;

#[test]
fn a_large_call_star_is_bounded_after_provenance_is_attached() {
    let root = tempfile::tempdir().unwrap();
    let mut text = String::from("fn hub() {\n");
    for n in 0..2000 {
        writeln!(text, "leaf{n}();").unwrap();
    }
    text.push_str("}\n");
    for n in 0..2000 {
        writeln!(text, "fn leaf{n}() {{}}").unwrap();
    }
    std::fs::write(root.path().join("a.rs"), text).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let result = index
        .search()
        .callees(&TraversalQuery::new("hub", 1).with_limit(1))
        .unwrap();
    assert!(result.context.generation.is_some());
    assert!(result.context.sources.contains_key("a.rs"));
    assert!(serde_json::to_vec(&result).unwrap().len() <= MAX_TOTAL_BYTES);
    assert!(result.edges.len() < 2000);
    assert!(
        result
            .truncations
            .iter()
            .any(|item| item.kind == TruncationKind::Bytes && item.dropped)
    );
    assert_eq!(
        result.approximation.unwrap().resolved,
        result.edges.len() as u64
    );
}

#[test]
fn scan_results_are_bounded_before_and_after_provenance() {
    use graph_search_types::{FilesQuery, TextQuery};
    let root = tempfile::tempdir().unwrap();
    for n in 0..800 {
        let name = format!("file-{n:04}-{}.rs", "x".repeat(100));
        std::fs::write(root.path().join(name), "").unwrap();
    }
    let line = format!("needle {}", "quoted \\\" café ".repeat(8));
    let source = format!("{line}\n").repeat(3000);
    std::fs::write(root.path().join("sample.txt"), &source).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    let mut files_query = FilesQuery::new("*.rs");
    files_query.limit = u32::MAX;
    let files = index.search().files(&files_query).unwrap();
    assert!(!files.items.is_empty() && files.items.len() < 800);
    assert!(serde_json::to_vec(&files).unwrap().len() <= MAX_TOTAL_BYTES);
    assert!(
        files
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes)
    );
    let mut query = TextQuery::new("needle");
    query.limit = u32::MAX;
    query.include = Some("*.txt".into());
    let text = index.search().text(&query).unwrap();
    assert!(!text.items.is_empty() && text.items.len() < 3000);
    assert!(serde_json::to_vec(&text).unwrap().len() <= MAX_TOTAL_BYTES);
    assert!(
        text.truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes)
    );
    assert_eq!(text.stats.source_files_attempted, 1);
    assert_eq!(text.context.sources.len(), 1);
    assert_eq!(
        text.context.sources["sample.txt"].observed_hash.as_deref(),
        Some(graph_search_core::hash::content_hash(source.as_bytes()).as_str())
    );
    for (n, hit) in text.items.iter().enumerate() {
        assert_eq!(hit.line, n as u64 + 1);
        assert_eq!(hit.text, line);
    }
    assert!(text.stats.matches >= text.items.len() as u64);
    assert!(files.context.generation.is_none());
    assert!(text.context.generation.is_none());
}

#[test]
fn oversized_scan_fields_fail_before_reading_the_tree() {
    use graph_search_core::{Error, config::WalkPolicy, files_search, text_search};
    use graph_search_types::{FilesQuery, TextQuery};
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let large = "é".repeat(
        graph_search_types::limits::MAX_QUERY_BYTES
            .checked_div(2)
            .unwrap()
            + 1,
    );
    let policy = WalkPolicy::default();
    for field in ["pattern", "path", "include"] {
        let mut text = TextQuery::new("needle");
        match field {
            "pattern" => text.pattern.clone_from(&large),
            "path" => text.path = Some(large.clone()),
            _ => text.include = Some(large.clone()),
        }
        assert!(matches!(
            text_search::search_text(&missing, &text, &policy),
            Err(Error::InvalidQuery(_))
        ));
        if field != "include" {
            let mut files = FilesQuery::new("*.rs");
            if field == "pattern" {
                files.pattern.clone_from(&large);
            } else {
                files.path = Some(large.clone());
            }
            assert!(matches!(
                files_search::search_files(&missing, &files, &policy),
                Err(Error::InvalidQuery(_))
            ));
        }
    }
    let exact = "é".repeat(
        graph_search_types::limits::MAX_QUERY_BYTES
            .checked_div(2)
            .unwrap(),
    );
    std::fs::write(dir.path().join("a.txt"), &exact).unwrap();
    let result = text_search::search_text(dir.path(), &TextQuery::new(exact), &policy).unwrap();
    assert_eq!(result.items.len(), 1);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::MatchLine)
    );
}

#[test]
fn directly_constructed_file_limits_cannot_bypass_the_execution_ceiling() {
    use graph_search_core::{config::WalkPolicy, files_search};
    use graph_search_types::{FilesQuery, limits::FILES_LIMIT_CEILING};
    let root = tempfile::tempdir().unwrap();
    for n in 0..=FILES_LIMIT_CEILING {
        std::fs::write(root.path().join(format!("{n:04}")), "").unwrap();
    }
    let mut query = FilesQuery::new("*");
    query.limit = u32::MAX;
    let result = files_search::search_files(root.path(), &query, &WalkPolicy::default()).unwrap();
    assert_eq!(result.items.len(), FILES_LIMIT_CEILING as usize);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Files && t.cap == u64::from(FILES_LIMIT_CEILING))
    );
    query.limit = 0;
    let zero = files_search::search_files(root.path(), &query, &WalkPolicy::default()).unwrap();
    assert!(zero.items.is_empty());
    assert!(
        zero.truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Files && t.cap == 0)
    );
}

#[test]
fn oversized_required_sync_metadata_fails_before_publication() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn before() {}\n").unwrap();
    let original = graph_search::Index::open(graph_search::OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .unwrap();
    original.reindex().unwrap();
    let current = original.store_dir().join("CURRENT");
    let before = std::fs::read(&current).unwrap();
    drop(original);
    std::fs::write(root.path().join("a.rs"), "fn changed() {}\n").unwrap();
    let index = graph_search::Index::open(graph_search::OpenOptions {
        root: root.path().into(),
        excludes: vec!["x".repeat(70_000)],
        ..Default::default()
    })
    .unwrap();
    for full in [true, false] {
        let result = if full { index.reindex() } else { index.sync() };
        assert!(matches!(
            result,
            Err(graph_search::Error::Core(
                graph_search_core::Error::ResultBudget(65_536)
            ))
        ));
        assert_eq!(std::fs::read(&current).unwrap(), before);
    }
}

#[test]
fn oversized_graph_fields_are_rejected_before_automatic_indexing() {
    use graph_search_types::occurrence::OccurrenceQuery;
    use graph_search_types::query::{
        DepsQuery, ExploreQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery,
    };
    fn rejected<T>(index: &Index, result: graph_search::Result<T>) {
        let Err(graph_search::Error::Core(graph_search_core::Error::InvalidQuery(message))) =
            result
        else {
            panic!("expected an invalid-query error before indexing");
        };
        assert!(message.len() < 200, "error must not echo oversized input");
        assert!(!index.store_dir().join("CURRENT").exists());
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn alpha() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .unwrap();
    let oversized = "é".repeat(4097);
    let service = index.search();
    rejected(&index, service.symbol(&SymbolQuery::new(&oversized)));
    rejected(&index, service.refs(&RefQuery::new(&oversized)));
    rejected(&index, service.callers(&TraversalQuery::new(&oversized, 1)));
    rejected(&index, service.callees(&TraversalQuery::new(&oversized, 1)));
    rejected(&index, service.impact(&TraversalQuery::new(&oversized, 1)));
    rejected(&index, service.deps(&DepsQuery::new(&oversized)));
    rejected(&index, service.neighbors(&NeighborsQuery::new(&oversized)));
    rejected(&index, service.path(&PathQuery::new("missing", &oversized)));
    rejected(&index, service.path(&PathQuery::new(&oversized, "missing")));
    rejected(&index, service.explore(&ExploreQuery::new(&oversized)));
    rejected(
        &index,
        service.occurrences(&OccurrenceQuery {
            target: oversized.clone(),
            ..Default::default()
        }),
    );
    macro_rules! filtered {
        ($method:ident, $query:expr) => {{
            let mut query = $query;
            query.filters.path_glob = Some(oversized.clone());
            rejected(&index, service.$method(&query));
        }};
    }
    filtered!(symbol, SymbolQuery::new("alpha"));
    filtered!(refs, RefQuery::new("alpha"));
    filtered!(callers, TraversalQuery::new("alpha", 1));
    filtered!(callees, TraversalQuery::new("alpha", 1));
    filtered!(impact, TraversalQuery::new("alpha", 1));
    filtered!(deps, DepsQuery::new("a.rs"));
    filtered!(neighbors, NeighborsQuery::new("alpha"));
    filtered!(explore, ExploreQuery::new("alpha"));
    filtered!(
        occurrences,
        OccurrenceQuery {
            target: "alpha".into(),
            ..Default::default()
        }
    );
}

#[test]
fn direct_core_navigation_enforces_byte_limits_without_changing_exact_boundary() {
    use graph_search_core::{memory::MemoryStore, ports::GraphStore, query::QueryEngine};
    use graph_search_types::query::{DepsQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery};
    let store = MemoryStore::new();
    let snapshot = store.snapshot().unwrap();
    let engine = QueryEngine::new(snapshot.as_ref());
    let oversized = "é".repeat(4097);
    let invalid = [
        engine.symbol(&SymbolQuery::new(&oversized)).map(|_| ()),
        engine.refs(&RefQuery::new(&oversized)).map(|_| ()),
        engine
            .callers(&TraversalQuery::new(&oversized, 1))
            .map(|_| ()),
        engine
            .callees(&TraversalQuery::new(&oversized, 1))
            .map(|_| ()),
        engine
            .impact(&TraversalQuery::new(&oversized, 1))
            .map(|_| ()),
        engine.deps(&DepsQuery::new(&oversized)).map(|_| ()),
        engine
            .neighbors(&NeighborsQuery::new(&oversized))
            .map(|_| ()),
        engine
            .path(&PathQuery::new("missing", &oversized))
            .map(|_| ()),
        engine
            .path(&PathQuery::new(&oversized, "missing"))
            .map(|_| ()),
    ];
    assert!(
        invalid
            .into_iter()
            .all(|result| matches!(result, Err(graph_search_core::Error::InvalidQuery(_))))
    );
    let exact = "é".repeat(4096);
    assert!(
        engine
            .symbol(&SymbolQuery::new(&exact))
            .unwrap()
            .nodes
            .is_empty()
    );
    assert!(matches!(
        engine.refs(&RefQuery::new(&exact)),
        Err(graph_search_core::Error::NotFound(_))
    ));
}
