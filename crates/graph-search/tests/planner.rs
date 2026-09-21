//! Structured intent, route isolation, Boolean coverage, and ranking diagnostics.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::GraphStore;
use graph_search_types::{ExploreMode, ExploreQuery, RankingStrategy, RetrievalRoute, TermMatch};

fn fixture() -> (tempfile::TempDir, Index) {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("a.rs", "fn cache_invalidate() {}\nfn cache_only() {}\n"),
        ("guide.md", "cache invalidate\n"),
        ("partial.md", "cache\n"),
    ] {
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

#[test]
fn graph_context_is_explicit_without_changing_lexical_seeds() {
    use graph_search_types::{EdgeKind, GraphContext};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn seed_left(){bridge();}\nfn bridge(){seed_right();}\nfn seed_right(){}\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let mut query = ExploreQuery::new("seed_").with_context_lines(0);
    query.retrieval.mode = ExploreMode::NamePrefix;
    query.retrieval.explain = true;
    query.hops = 2;
    let default = index.search().explore(&query).unwrap();
    assert_eq!(default.items.len(), 3);
    assert_eq!(default.edges.len(), 2);
    for mode in [
        GraphContext::None,
        GraphContext::Calls,
        GraphContext::Imports,
        GraphContext::Types,
    ] {
        query.retrieval.graph_context = mode;
        let result = index.search().explore(&query).unwrap();
        assert_eq!(result.plan.unwrap().options.graph_context, mode);
        assert_eq!(result.items[0].node.id, default.items[0].node.id);
        assert_eq!(result.items[1].node.id, default.items[1].node.id);
        if mode == GraphContext::Calls {
            assert_eq!(result.edges, default.edges);
            assert!(result.edges.iter().all(|e| e.kind == EdgeKind::Calls));
            assert_eq!(result.items.len(), 3);
            assert!(result.items.iter().all(|i| i.impact.is_some()));
        } else {
            assert_eq!(result.items.len(), 2);
            assert!(result.edges.is_empty());
            assert!(result.items.iter().all(|i| i.impact.is_none()));
        }
        if mode == GraphContext::None {
            assert_eq!(result.stats.graph_edges_examined, 0);
            assert_eq!(result.stats.graph_nodes_visited, 0);
        }
        assert!(result.truncations.is_empty());
    }
    // Old serialized options preserve the established default.
    let options: graph_search_types::RetrievalOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(options.graph_context, GraphContext::Semantic);
}

#[test]
fn graph_context_admits_only_the_requested_relation_family() {
    use graph_search_types::{
        Edge, EdgeKind, FileProjection, GraphContext, Node, NodeId, NodeKind, WriteBatch,
    };
    let root = tempfile::tempdir().unwrap();
    let nodes: Vec<_> = ["seed_a", "seed_b"]
        .into_iter()
        .map(|name| Node {
            id: NodeId::symbol("a.rs", NodeKind::Struct, name, None),
            kind: NodeKind::Struct,
            path: "a.rs".into(),
            name: Some(name.into()),
            ..Node::default()
        })
        .collect();
    for kind in [
        EdgeKind::Calls,
        EdgeKind::Imports,
        EdgeKind::References,
        EdgeKind::TypeUses,
        EdgeKind::Implements,
        EdgeKind::Extends,
        EdgeKind::Contains,
    ] {
        let mut store = graph_search_core::memory::MemoryStore::new();
        store
            .apply(WriteBatch {
                upserts: vec![FileProjection {
                    file: Node {
                        id: NodeId::file("a.rs"),
                        path: "a.rs".into(),
                        ..Node::default()
                    },
                    symbols: nodes.clone(),
                    edges: vec![Edge::resolved(
                        &nodes[1].id,
                        kind,
                        &nodes[0].id,
                        "seed_a",
                        None,
                        None,
                    )],
                    ..FileProjection::default()
                }],
                ..WriteBatch::default()
            })
            .unwrap();
        let snapshot = store.snapshot().unwrap();
        let engine = graph_search_core::query::QueryEngine::new(snapshot.as_ref());
        for mode in [
            GraphContext::Semantic,
            GraphContext::None,
            GraphContext::Calls,
            GraphContext::Imports,
            GraphContext::Types,
        ] {
            let mut query = ExploreQuery::new("seed_").with_context_lines(0);
            query.retrieval.mode = ExploreMode::NamePrefix;
            query.retrieval.graph_context = mode;
            let result = engine.explore(&query, root.path()).unwrap();
            let expected = match mode {
                GraphContext::Semantic => kind != EdgeKind::Contains,
                GraphContext::None => false,
                GraphContext::Calls => kind == EdgeKind::Calls,
                GraphContext::Imports => kind == EdgeKind::Imports,
                GraphContext::Types => matches!(
                    kind,
                    EdgeKind::TypeUses | EdgeKind::Implements | EdgeKind::Extends
                ),
            };
            assert_eq!(
                result.edges.len(),
                usize::from(expected),
                "{mode:?}/{kind:?}"
            );
            if expected {
                assert_eq!(result.edges[0].from, nodes[1].id.as_str());
                assert_eq!(result.edges[0].to.as_deref(), Some(nodes[0].id.as_str()));
                assert_eq!(result.edges[0].kind, kind);
            }
            assert_eq!(result.items.len(), 2);
            assert!(result.truncations.is_empty());
        }
    }
}

#[test]
fn explicit_navigation_does_not_broaden_or_spend_posting_work() {
    let (_root, index) = fixture();
    let mut query = ExploreQuery::new("cache_invalidate");
    query.retrieval.mode = ExploreMode::ExactName;
    query.retrieval.explain = true;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].node.name, "cache_invalidate");
    assert_eq!(result.stats.lexical_postings_examined, 0);
    assert_eq!(result.stats.files_scanned, 0);
    assert_eq!(result.plan.unwrap().routes, vec![RetrievalRoute::ExactName]);
    query.query = "cache".into();
    assert!(index.search().explore(&query).unwrap().items.is_empty());
    query.query = result.items[0].node.id.clone();
    query.retrieval.mode = ExploreMode::ExactId;
    assert_eq!(
        index.search().explore(&query).unwrap().items[0].node.id,
        query.query
    );
    query.query = "*.md".into();
    query.retrieval.mode = ExploreMode::PathGlob;
    let paths = index.search().explore(&query).unwrap();
    assert_eq!(paths.items.len(), 2);
    assert!(paths.items.iter().all(|item| {
        std::path::Path::new(&item.node.path)
            .extension()
            .is_some_and(|extension| extension == "md")
    }));
    assert_eq!(paths.plan.unwrap().routes, vec![RetrievalRoute::PathGlob]);
    query.query = "[".into();
    assert!(index.search().explore(&query).is_err());
}

#[test]
fn exact_fast_path_is_optional_and_filtered_misses_fall_back() {
    let (_root, index) = fixture();
    let mut query = ExploreQuery::new("cache_invalidate");
    query.retrieval.explain = true;
    let discovery = index.search().explore(&query).unwrap();
    assert_eq!(
        discovery.plan.unwrap().routes,
        vec![RetrievalRoute::Metadata, RetrievalRoute::Body]
    );
    query.retrieval.exact_fast_path = true;
    assert_eq!(
        index.search().explore(&query).unwrap().plan.unwrap().routes,
        vec![RetrievalRoute::ExactName]
    );
    query.filters.path_glob = Some("*.md".into());
    let fallback = index.search().explore(&query).unwrap();
    assert_eq!(
        fallback.plan.unwrap().routes,
        vec![
            RetrievalRoute::ExactName,
            RetrievalRoute::Metadata,
            RetrievalRoute::Body
        ]
    );
    assert!(fallback.items.iter().all(|item| {
        std::path::Path::new(&item.node.path)
            .extension()
            .is_some_and(|extension| extension == "md")
    }));
}

#[test]
fn channels_and_minimum_coverage_are_independent_controls() {
    let (_root, index) = fixture();
    for strategy in [
        RankingStrategy::Body,
        RankingStrategy::Metadata,
        RankingStrategy::Fusion,
    ] {
        let mut query = ExploreQuery::new("cache invalidate");
        query.retrieval.ranking = strategy;
        query.retrieval.explain = true;
        query.retrieval.term_match = TermMatch::All;
        let result = index.search().explore(&query).unwrap();
        assert!(!result.items.is_empty());
        assert!(
            !result
                .items
                .iter()
                .any(|item| item.node.path == "partial.md" || item.node.name == "cache_only")
        );
        let plan = result.plan.unwrap();
        assert_eq!(plan.query, "cache invalidate");
        assert_eq!(plan.terms, vec!["cache", "invalidate"]);
        let routes = match strategy {
            RankingStrategy::Metadata => vec![RetrievalRoute::Metadata],
            RankingStrategy::Body => vec![RetrievalRoute::Body],
            RankingStrategy::Auto => unreachable!(),
            RankingStrategy::Fusion => vec![RetrievalRoute::Metadata, RetrievalRoute::Body],
        };
        assert_eq!(plan.routes, routes);
        assert!(result.items.iter().all(|item| item.retrieval.is_some()));
        if strategy == RankingStrategy::Body {
            assert!(
                result.items.iter().all(|item| item
                    .retrieval
                    .as_ref()
                    .unwrap()
                    .metadata_rank
                    .is_none())
            );
        }
        if strategy == RankingStrategy::Metadata {
            assert!(
                result.items.iter().all(|item| item
                    .retrieval
                    .as_ref()
                    .unwrap()
                    .body_rank
                    .is_none())
            );
        }
        query.retrieval.term_match = TermMatch::AtLeast(3);
        assert!(index.search().explore(&query).unwrap().items.is_empty());
        query.retrieval.term_match = TermMatch::Any;
        let any = index.search().explore(&query).unwrap();
        assert!(any.items.len() > result.items.len());
    }
}

#[test]
fn query_limits_reject_instead_of_weakening_boolean_requirements() {
    let (_root, index) = fixture();
    let query = ExploreQuery::new(
        "x".repeat(graph_search_types::limits::MAX_QUERY_BYTES.saturating_add(1)),
    );
    assert!(matches!(
        index.search().explore(&query),
        Err(graph_search::Error::Core(
            graph_search_core::Error::InvalidQuery(_)
        ))
    ));
    let text = (0..=graph_search_types::limits::MAX_QUERY_TERMS)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(index.search().explore(&ExploreQuery::new(text)).is_err());
}

#[test]
fn incomplete_postings_cannot_claim_unobserved_boolean_coverage() {
    let (_root, index) = fixture();
    for ranking in [RankingStrategy::Metadata, RankingStrategy::Body] {
        let mut query = ExploreQuery::new("cache invalidate");
        query.retrieval.ranking = ranking;
        query.retrieval.term_match = TermMatch::All;
        let result = index
            .search()
            .with_work_limits(graph_search::WorkLimits {
                postings: 1,
                ..graph_search::WorkLimits::default()
            })
            .explore(&query)
            .unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.stats.lexical_postings_examined, 1);
        assert!(
            result
                .truncations
                .iter()
                .any(|limit| limit.kind == graph_search_types::TruncationKind::Postings)
        );
    }
}

#[test]
fn automatic_multiword_discovery_uses_body_then_metadata_only_when_empty() {
    let (_root, index) = fixture();
    let mut query = ExploreQuery::new("cache invalidate");
    query.retrieval.explain = true;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.plan.unwrap().routes, vec![RetrievalRoute::Body]);
    assert!(
        result
            .items
            .iter()
            .all(|item| item.retrieval.as_ref().unwrap().metadata_rank.is_none())
    );
    // A custom graph-only host still has useful signatures even without source facts.
    let root = tempfile::tempdir().unwrap();
    let mut store = graph_search_core::memory::MemoryStore::new();
    let node = graph_search_types::Node {
        id: graph_search_types::NodeId::symbol(
            "a.rs",
            graph_search_types::NodeKind::Function,
            "phantom_route",
            None,
        ),
        kind: graph_search_types::NodeKind::Function,
        path: "a.rs".into(),
        name: Some("phantom_route".into()),
        ..graph_search_types::Node::default()
    };
    store
        .apply(graph_search_types::WriteBatch {
            upserts: vec![graph_search_types::FileProjection {
                file: graph_search_types::Node {
                    id: graph_search_types::NodeId::file("a.rs"),
                    path: "a.rs".into(),
                    ..graph_search_types::Node::default()
                },
                symbols: vec![node],
                ..graph_search_types::FileProjection::default()
            }],
            ..graph_search_types::WriteBatch::default()
        })
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let mut query = ExploreQuery::new("phantom route");
    query.retrieval.explain = true;
    let result = graph_search_core::query::QueryEngine::new(snapshot.as_ref())
        .explore(&query, root.path())
        .unwrap();
    assert_eq!(
        result.plan.unwrap().routes,
        vec![RetrievalRoute::Body, RetrievalRoute::Metadata]
    );
    assert_eq!(result.items[0].node.name, "phantom_route");
}

#[test]
fn name_prefix_is_explicit_bounded_and_rebuilt_with_the_generation() {
    let (root, index) = fixture();
    let mut query = ExploreQuery::new("cache_").with_context_lines(0);
    query.retrieval.mode = ExploreMode::NamePrefix;
    query.retrieval.explain = true;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(
        result.plan.unwrap().routes,
        vec![RetrievalRoute::NamePrefix]
    );
    assert!(
        result
            .items
            .iter()
            .all(|item| !item.retrieval.as_ref().unwrap().exact)
    );
    assert_eq!(result.stats.files_scanned, 0);
    assert!(result.stats.dictionary_entries_examined > 0);
    let capped = index
        .search()
        .with_work_limits(graph_search_core::work::WorkLimits {
            dictionary_entries: 1,
            ..graph_search_core::work::WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(capped.stats.dictionary_entries_examined, 1);
    assert!(
        capped
            .truncations
            .iter()
            .any(|t| t.kind == graph_search_types::TruncationKind::DictionaryEntries)
    );
    for absent in ["Cache_", "invalidate", "cache*", "missing"] {
        query.query = absent.into();
        assert!(index.search().explore(&query).unwrap().items.is_empty());
    }
    query.query = " ".into();
    assert!(index.search().explore(&query).is_err());
    std::fs::write(root.path().join("a.rs"), "fn replacement() {}\n").unwrap();
    index.sync().unwrap();
    query.query = "cache_".into();
    assert!(index.search().explore(&query).unwrap().items.is_empty());
    drop(index);
    let reopened = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    assert!(reopened.search().explore(&query).unwrap().items.is_empty());
    query.query = "repl".into();
    assert_eq!(
        reopened.search().explore(&query).unwrap().items[0]
            .node
            .name,
        "replacement"
    );
}

#[test]
fn identifier_analysis_recovers_whole_names_and_qualified_context() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"),
        "struct Client;\nimpl Client {\n fn getHTTPResponse() {}\n}\nfn inspect() { let value = \"getHTTPResponse\"; }\nfn check(is: bool) {}\n").unwrap();
    std::fs::write(root.path().join("config.json"), "{\"api_key\": 1}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    for ranking in [
        RankingStrategy::Metadata,
        RankingStrategy::Body,
        RankingStrategy::Fusion,
    ] {
        let mut query = ExploreQuery::new("where gethttpresponse");
        query.retrieval.ranking = ranking;
        assert!(index.search().explore(&query).unwrap().items.is_empty());
        query.retrieval.analysis = graph_search_types::AnalysisMode::Identifiers;
        query.retrieval.explain = true;
        let result = index.search().explore(&query).unwrap();
        assert!(!result.items.is_empty());
        assert_eq!(result.plan.unwrap().terms, ["gethttpresponse"]);
        assert!(
            result
                .items
                .iter()
                .any(|item| item.node.name == "getHTTPResponse" || item.node.name == "inspect")
        );
    }
    let mut query = ExploreQuery::new("Client gethttpresponse");
    query.retrieval.analysis = graph_search_types::AnalysisMode::Identifiers;
    query.retrieval.ranking = RankingStrategy::Metadata;
    query.retrieval.term_match = TermMatch::All;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].node.name, "getHTTPResponse");
    query.query = "is".into();
    assert!(
        index
            .search()
            .explore(&query)
            .unwrap()
            .items
            .iter()
            .any(|item| item.node.name == "check")
    );
    query.query = "api_key".into();
    query.retrieval.ranking = RankingStrategy::Body;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items[0].node.path, "config.json");
    assert!(result.items[0].snippet.as_ref().unwrap().lines[0].contains("api_key"));
}

#[test]
fn bm25f_policy_preserves_explicit_intent_boolean_coverage_and_work_limits() {
    use graph_search_types::{FieldNormalization, result::TruncationKind};
    let (_root, index) = fixture();
    let mut query = ExploreQuery::new("cache invalidate");
    query.retrieval.ranking = RankingStrategy::Metadata;
    query.retrieval.normalization = FieldNormalization::Bm25f;
    query.retrieval.term_match = TermMatch::All;
    query.retrieval.explain = true;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].node.name, "cache_invalidate");
    assert_eq!(
        result.plan.unwrap().options.normalization,
        FieldNormalization::Bm25f
    );
    let limited = index
        .search()
        .with_work_limits(graph_search_core::work::WorkLimits {
            postings: 0,
            ..Default::default()
        })
        .explore(&query)
        .unwrap();
    assert!(limited.items.is_empty());
    assert!(
        limited
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Postings)
    );
    query.query = "cache_invalidate".into();
    query.retrieval.mode = ExploreMode::ExactName;
    let exact = index.search().explore(&query).unwrap();
    assert_eq!(exact.items[0].node.name, "cache_invalidate");
    assert_eq!(exact.stats.lexical_postings_examined, 0);
    query.retrieval.mode = ExploreMode::Terms;
    query.query = "cache invalidate".into();
    query.retrieval.ranking = RankingStrategy::Body;
    let bm25f = index.search().explore(&query).unwrap();
    query.retrieval.normalization = FieldNormalization::Combined;
    let combined = index.search().explore(&query).unwrap();
    assert_eq!(
        bm25f
            .items
            .iter()
            .map(|hit| &hit.node.id)
            .collect::<Vec<_>>(),
        combined
            .items
            .iter()
            .map(|hit| &hit.node.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn task_prompt_policy_is_opt_in_explained_and_preserves_explicit_intent() {
    use graph_search_types::{AnalysisMode, QueryPolicy};
    let (_root, index) = fixture();
    let original = "cache invalidate. Cite the relevant source and explain the execution path. Do not modify files.";
    let mut query = ExploreQuery::new(original);
    query.retrieval.ranking = RankingStrategy::Body;
    query.retrieval.term_match = TermMatch::All;
    query.retrieval.explain = true;
    assert!(index.search().explore(&query).unwrap().items.is_empty());
    query.retrieval.query_policy = QueryPolicy::Task;
    for analysis in [AnalysisMode::Split, AnalysisMode::Identifiers] {
        query.retrieval.analysis = analysis;
        let result = index.search().explore(&query).unwrap();
        assert!(!result.items.is_empty());
        let plan = result.plan.unwrap();
        assert_eq!(plan.query, original);
        assert_eq!(plan.terms, ["cache", "invalidate"]);
        assert_eq!(
            plan.omitted_boilerplate,
            [
                "Cite the relevant source and explain the execution path.",
                "Do not modify files.",
            ]
        );
        assert_eq!(plan.options.query_policy, QueryPolicy::Task);
    }
    query.retrieval.mode = ExploreMode::ExactName;
    let exact = index.search().explore(&query).unwrap();
    assert!(exact.items.is_empty());
    assert!(exact.plan.unwrap().omitted_boilerplate.is_empty());
    query.retrieval.mode = ExploreMode::Phrase;
    let phrase = index.search().explore(&query).unwrap();
    assert!(phrase.items.is_empty());
    assert!(phrase.plan.unwrap().omitted_boilerplate.is_empty());
}
