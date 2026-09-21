//! Traversal budgets and cancellation through the public library.
#![allow(clippy::unwrap_used)]

use graph_search::{CancellationToken, Index, OpenOptions, Reconcile, WorkLimits};
use graph_search_types::query::{PathQuery, TraversalQuery};
use graph_search_types::result::TruncationKind;

#[test]
fn explore_impact_reuses_root_edges_and_preserves_partial_counts() {
    use graph_search_types::{ExploreMode, ExploreQuery};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn target() {}\nfn left(){target();}\nfn right(){target();}\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let mut query = ExploreQuery::new("target").with_context_lines(0);
    query.retrieval.mode = ExploreMode::ExactName;
    query.hops = 1;
    let run = |nodes, edges| {
        index
            .search()
            .with_work_limits(WorkLimits {
                nodes,
                edges,
                ..WorkLimits::default()
            })
            .explore(&query)
            .unwrap()
    };
    // Two incoming calls plus the containment edge inspected by adjacency.
    // Exactly enough work must be complete, including on a later request.
    for _ in 0..2 {
        let result = run(3, 3);
        assert_eq!(result.items.len(), 1);
        let impact = result.items[0].impact.as_ref().unwrap();
        assert_eq!((impact.direct_callers, impact.total_callers), (2, 2));
        assert_eq!(result.stats.graph_edges_examined, 3);
        assert!(result.edges.is_empty());
        assert!(result.truncations.is_empty(), "{:?}", result.truncations);
    }
    let partial = run(3, 2);
    let impact = partial.items[0].impact.as_ref().unwrap();
    assert_eq!((impact.direct_callers, impact.total_callers), (1, 1));
    assert_eq!(partial.stats.graph_edges_examined, 2);
    assert!(
        partial
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::GraphEdges)
    );
    let no_nodes = run(0, 3);
    let impact = no_nodes.items[0].impact.as_ref().unwrap();
    assert_eq!((impact.direct_callers, impact.total_callers), (2, 2));
    assert_eq!(no_nodes.stats.graph_nodes_visited, 0);
    assert!(
        no_nodes
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::GraphNodes)
    );
}

#[test]
fn explore_impact_counts_transitive_callers_once_through_cycles_and_diamonds() {
    use graph_search_types::{ExploreMode, ExploreQuery};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn target(){target();top();}\nfn left(){target();}\nfn right(){target();}\nfn top(){left();right();}\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let mut query = ExploreQuery::new("target").with_context_lines(0);
    query.retrieval.mode = ExploreMode::ExactName;
    query.hops = 4;
    let result = index.search().explore(&query).unwrap();
    let impact = result.items[0].impact.as_ref().unwrap();
    assert_eq!((impact.direct_callers, impact.total_callers), (2, 3));
    assert_eq!(result.stats.graph_nodes_visited, 4);
    // Six call edges, counted at both endpoints except the self-loop, plus
    // one containment edge per function. Each neighborhood is read once.
    assert_eq!(result.stats.graph_edges_examined, 15);
    assert!(result.truncations.is_empty());
}

#[test]
fn cycle_traversal_is_bounded_and_limits_are_reported() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn a(){b();}\nfn b(){c();}\nfn c(){a();}\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let full = index
        .search()
        .callees(&TraversalQuery::new("a", 4))
        .unwrap();
    assert_eq!(full.nodes.len(), 3);
    assert_eq!(full.edges.len(), 3);
    assert_eq!(full.stats.graph_nodes_visited, 3);
    assert!(full.truncations.is_empty());
    let service = index.search().with_work_limits(WorkLimits {
        nodes: 1,
        edges: 2,
        ..WorkLimits::default()
    });
    let limited = service.callees(&TraversalQuery::new("a", 4)).unwrap();
    assert!(limited.stats.graph_nodes_visited <= 1);
    assert!(limited.stats.graph_edges_examined <= 2);
    assert!(limited.truncations.iter().any(|t| matches!(
        t.kind,
        TruncationKind::GraphNodes | TruncationKind::GraphEdges
    )));
    let path = service.path(&PathQuery::new("a", "c")).unwrap();
    assert!(path.nodes.is_empty());
    assert!(!path.truncations.is_empty());
}

#[test]
fn cancellation_and_deadline_fail_explicitly_before_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn a() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    let token = CancellationToken::default();
    token.cancel();
    let service = index.search().with_work_limits(WorkLimits {
        cancellation: Some(token),
        ..WorkLimits::default()
    });
    assert!(matches!(
        service.callees(&TraversalQuery::new("a", 1)),
        Err(graph_search::Error::Core(
            graph_search::core::Error::QueryCancelled
        ))
    ));
    let service = index.search().with_work_limits(WorkLimits {
        deadline: Some(std::time::Instant::now()),
        ..WorkLimits::default()
    });
    assert!(matches!(
        service.callees(&TraversalQuery::new("a", 1)),
        Err(graph_search::Error::Core(
            graph_search::core::Error::QueryDeadline
        ))
    ));
    assert!(!index.search().status().unwrap().exists);
}

#[test]
fn returned_edge_limits_preserve_work_counts_and_impact_totals() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn a(){b();}\nfn b(){c();}\nfn c(){a();}\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let full = index
        .search()
        .callees(&TraversalQuery::new("a", 4))
        .unwrap();
    let full_impact = index.search().impact(&TraversalQuery::new("a", 4)).unwrap();
    let full_path = index.search().path(&PathQuery::new("a", "c")).unwrap();
    for cap in [0, 1, 3] {
        let service = index.search().with_work_limits(WorkLimits {
            returned_edges: cap,
            ..WorkLimits::default()
        });
        let graph = service.callees(&TraversalQuery::new("a", 4)).unwrap();
        assert_eq!(graph.nodes, full.nodes);
        assert_eq!(
            graph.stats.graph_edges_examined,
            full.stats.graph_edges_examined
        );
        assert_eq!(graph.edges.len(), cap);
        assert_eq!(graph.approximation.unwrap().resolved, cap as u64);
        assert_eq!(
            graph
                .truncations
                .iter()
                .any(|t| t.kind == TruncationKind::ReturnedEdges),
            cap < 3
        );
        let impact = service.impact(&TraversalQuery::new("a", 4)).unwrap();
        assert_eq!(impact.by_depth, full_impact.by_depth);
        assert!(impact.edges.len() <= cap);
        let path = service.path(&PathQuery::new("a", "c")).unwrap();
        assert_eq!(path.nodes, full_path.nodes);
        assert!(path.edges.len() <= cap);
        let explore = service
            .explore(&graph_search_types::ExploreQuery::new("a b c").with_context_lines(0))
            .unwrap();
        assert!(explore.edges.len() <= cap);
    }
}

#[test]
fn walk_served_queries_honor_host_cancellation_and_deadlines_without_an_index() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn target() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    let files = graph_search_types::FilesQuery::new("*.rs");
    let text = graph_search_types::TextQuery::new("target");
    let token = CancellationToken::default();
    token.cancel();
    let cancelled = index.search().with_work_limits(WorkLimits {
        cancellation: Some(token),
        ..WorkLimits::default()
    });
    assert!(matches!(
        cancelled.files(&files),
        Err(graph_search::Error::Core(
            graph_search::core::Error::QueryCancelled
        ))
    ));
    assert!(matches!(
        cancelled.text(&text),
        Err(graph_search::Error::Core(
            graph_search::core::Error::QueryCancelled
        ))
    ));
    let expired = index.search().with_work_limits(WorkLimits {
        deadline: Some(std::time::Instant::now()),
        ..WorkLimits::default()
    });
    assert!(matches!(
        expired.files(&files),
        Err(graph_search::Error::Core(
            graph_search::core::Error::QueryDeadline
        ))
    ));
    assert!(matches!(
        expired.text(&text),
        Err(graph_search::Error::Core(
            graph_search::core::Error::QueryDeadline
        ))
    ));
    assert_eq!(index.search().files(&files).unwrap().items.len(), 1);
    assert_eq!(index.search().text(&text).unwrap().items.len(), 1);
    assert!(!index.search().status().unwrap().exists);
}

#[test]
#[allow(clippy::too_many_lines)] // One fixture exercises the related budget boundaries.
fn literal_source_budgets_filter_before_admission_and_never_return_a_prefix() {
    use graph_search_types::TextQuery;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.bin"), [0u8; 100]).unwrap();
    for name in ["b.txt", "c.txt"] {
        std::fs::write(root.path().join(name), "needle\n").unwrap();
    }
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    let query = TextQuery::new("needle").with_include("*.txt");
    let run = |files, bytes| {
        index
            .search()
            .with_work_limits(WorkLimits {
                source_files: files,
                source_bytes: bytes,
                ..WorkLimits::default()
            })
            .text(&query)
            .unwrap()
    };
    let full = run(2, 14);
    assert_eq!(full.items.len(), 2);
    assert_eq!(
        (
            full.stats.source_files_attempted,
            full.stats.source_bytes_read
        ),
        (2, 14)
    );
    assert!(full.truncations.is_empty());
    for (files, bytes, kind) in [
        (1, 100, TruncationKind::SourceFiles),
        (2, 7, TruncationKind::SourceBytes),
    ] {
        let result = run(files, bytes);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].path, "b.txt");
        assert_eq!(
            (
                result.stats.source_files_attempted,
                result.stats.source_bytes_read
            ),
            (1, 7)
        );
        assert!(result.truncations.iter().any(|t| t.kind == kind));
        assert_eq!(result.context.coverage.enumeration_complete, Some(true));
    }
    let overflow = run(2, 6);
    assert!(
        overflow.items.is_empty(),
        "the six-byte matching prefix is not a complete file"
    );
    assert!(overflow.context.sources.is_empty());
    assert_eq!(
        (
            overflow.stats.source_files_attempted,
            overflow.stats.source_bytes_read
        ),
        (1, 7)
    );
    assert!(
        overflow
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::SourceBytes)
    );
    assert_eq!(overflow.context.coverage.source_budget_exceeded_files, 1);
    let later_overflow = run(2, 8);
    assert_eq!(later_overflow.items.len(), 1);
    assert_eq!(later_overflow.items[0].path, "b.txt");
    assert_eq!(
        (
            later_overflow.stats.source_files_attempted,
            later_overflow.stats.source_bytes_read
        ),
        (2, 9)
    );
    assert!(!later_overflow.context.sources.contains_key("c.txt"));
    for (files, bytes) in [(0, 100), (2, 0)] {
        let empty = run(files, bytes);
        assert!(empty.items.is_empty());
        assert_eq!(
            (
                empty.stats.source_files_attempted,
                empty.stats.source_bytes_read
            ),
            (0, 0)
        );
        assert!(!empty.truncations.is_empty());
    }
    let exact = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 1,
            source_bytes: 7,
            ..WorkLimits::default()
        })
        .text(&TextQuery::new("needle").with_include("b.txt"))
        .unwrap();
    assert_eq!(exact.items.len(), 1);
    assert!(exact.truncations.is_empty());
}

#[test]
fn invalid_sources_still_consume_literal_read_work() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), b"\0needle").unwrap();
    std::fs::write(root.path().join("b.txt"), b"\xffneedle").unwrap();
    std::fs::write(root.path().join("c.txt"), b"needle").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 2,
            ..WorkLimits::default()
        })
        .text(&graph_search_types::TextQuery::new("needle"))
        .unwrap();
    assert!(result.items.is_empty());
    assert_eq!(
        (
            result.stats.source_files_attempted,
            result.stats.source_bytes_read
        ),
        (2, 14)
    );
    assert_eq!(result.context.coverage.binary_files, 1);
    assert_eq!(result.context.coverage.invalid_utf8_files, 1);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::SourceFiles)
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One fixture exercises the related budget boundaries.
fn explore_shares_read_work_across_items_and_withholds_budget_exhausted_source() {
    use graph_search_types::{ExploreQuery, RankingStrategy};
    let root = tempfile::tempdir().unwrap();
    let source = "fn alpha() {}\nfn beta() {}\n";
    std::fs::write(root.path().join("a.rs"), source).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let mut query = ExploreQuery::new("alpha beta");
    query.retrieval.ranking = RankingStrategy::Metadata;
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 1,
            source_bytes: source.len(),
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(result.items.len(), 2);
    for name in ["alpha", "beta"] {
        assert!(result.items.iter().any(|item| item.node.name == name));
    }
    let mut delivered = std::collections::BTreeMap::new();
    let hash = graph_search_core::hash::content_hash(source.as_bytes());
    for item in &result.items {
        for snippet in item
            .snippet
            .iter()
            .chain(item.excerpts.iter().map(|extra| &extra.snippet))
        {
            assert_eq!(item.node.path, "a.rs");
            assert_eq!(snippet.source_hash, hash);
            for (offset, text) in snippet.lines.iter().enumerate() {
                let line = snippet.start_line + u32::try_from(offset).unwrap();
                assert!(
                    delivered.insert(line, text.as_str()).is_none(),
                    "duplicate source line"
                );
            }
        }
    }
    assert_eq!(delivered.keys().copied().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        delivered.values().copied().collect::<Vec<_>>(),
        source.lines().collect::<Vec<_>>()
    );
    assert_eq!(
        (
            result.stats.source_files_attempted,
            result.stats.source_bytes_read
        ),
        (1, source.len() as u64)
    );
    assert!(!result.truncations.iter().any(|t| matches!(
        t.kind,
        TruncationKind::SourceFiles | TruncationKind::SourceBytes
    )));
    let no_reads = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 0,
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert!(no_reads.items.iter().all(|item| item.snippet.is_none()));
    assert_eq!(no_reads.stats.source_files_attempted, 0);
    assert!(no_reads.context.sources["a.rs"].observed_hash.is_none());
    assert!(
        no_reads
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::SourceFiles)
    );
    for bytes in [0, source.len() - 1] {
        let limited = index
            .search()
            .with_work_limits(WorkLimits {
                source_bytes: bytes,
                ..WorkLimits::default()
            })
            .explore(&query)
            .unwrap();
        assert_eq!(limited.items.len(), 2);
        assert!(
            limited
                .items
                .iter()
                .all(|item| item.snippet.is_none() && item.excerpts.is_empty())
        );
        assert!(
            limited
                .truncations
                .iter()
                .any(|t| t.kind == TruncationKind::SourceBytes)
        );
        assert_eq!(
            limited.context.sources["a.rs"].verification,
            graph_search_types::context::SourceVerification::BudgetExceeded
        );
    }
    let no_context = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 0,
            source_bytes: 0,
            ..WorkLimits::default()
        })
        .explore(&query.with_context_lines(0))
        .unwrap();
    assert_eq!(
        (
            no_context.stats.source_files_attempted,
            no_context.stats.source_bytes_read
        ),
        (0, 0)
    );
    assert!(!no_context.truncations.iter().any(|t| matches!(
        t.kind,
        TruncationKind::SourceFiles | TruncationKind::SourceBytes
    )));
    let changed = "// replacement marker\nfn alpha() {}\n";
    std::fs::write(root.path().join("a.rs"), changed).unwrap();
    let live = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 1,
            source_bytes: changed.len(),
            ..WorkLimits::default()
        })
        .explore(&ExploreQuery::new("replacement marker"))
        .unwrap();
    assert!(!live.items.is_empty());
    assert!(live.items[0].evidence.as_ref().unwrap().live);
    assert_eq!(
        (
            live.stats.source_files_attempted,
            live.stats.source_bytes_read
        ),
        (1, changed.len() as u64)
    );
}

#[test]
fn query_walk_limits_report_partial_enumeration_and_reset_per_service_call() {
    use graph_search_types::{FilesQuery, TextQuery};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "needle").unwrap();
    std::fs::write(root.path().join("b.txt"), "needle").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    for (cap, count, complete) in [(0, 0, false), (1, 0, false), (2, 1, false), (3, 2, true)] {
        let search = index.search().with_work_limits(WorkLimits {
            walk_entries: cap,
            ..WorkLimits::default()
        });
        for _ in 0..2 {
            let files = search.files(&FilesQuery::new("*.txt")).unwrap();
            let text = search.text(&TextQuery::new("needle")).unwrap();
            assert_eq!(files.items.len(), count);
            assert_eq!(text.items.len(), count);
            for coverage in [&files.context.coverage, &text.context.coverage] {
                assert_eq!(coverage.enumeration_complete, Some(complete));
                assert_eq!(coverage.entries_visited, cap as u64);
                assert_eq!(
                    coverage
                        .truncations
                        .iter()
                        .filter(|t| t.kind == TruncationKind::WalkEntries)
                        .count(),
                    usize::from(!complete)
                );
            }
        }
    }
    index.reindex().unwrap();
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            walk_entries: 0,
            ..WorkLimits::default()
        })
        .explore(&graph_search_types::query::ExploreQuery::new(
            "needle marker",
        ));
    assert!(matches!(
        result,
        Err(graph_search::Error::Core(
            graph_search_core::Error::IncompleteWalk(_)
        ))
    ));
}

#[test]
fn successive_core_walks_share_entry_allowance_without_changing_policy_identity() {
    use graph_search_core::{config::WalkPolicy, walk, work::WorkBudget};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "needle").unwrap();
    let policy = WalkPolicy::default();
    let full = walk::walk_report(root.path(), &policy).unwrap();
    let mut work = WorkBudget::new(WorkLimits {
        walk_entries: 3,
        ..WorkLimits::default()
    });
    let first = walk::walk_report_with_work(root.path(), &policy, &mut work).unwrap();
    assert_eq!(first.coverage, full.coverage);
    assert_eq!(work.remaining_walk_entries(), 1);
    let second = walk::walk_report_with_work(root.path(), &policy, &mut work).unwrap();
    assert!(second.entries.is_empty());
    assert_eq!(second.coverage.entries_visited, 1);
    assert_eq!(second.coverage.enumeration_complete, Some(false));
    assert_eq!(second.coverage.policy, full.coverage.policy);
    assert!(second.require_complete().is_err());
    assert_eq!(work.remaining_walk_entries(), 0);
    let third = walk::walk_report_with_work(root.path(), &policy, &mut work).unwrap();
    assert_eq!(third.coverage.entries_visited, 0);
    assert!(third.require_complete().is_err());
    assert_eq!(
        work.report()
            .2
            .iter()
            .filter(|t| t.kind == TruncationKind::WalkEntries)
            .count(),
        1
    );
    let policy = WalkPolicy {
        max_walk_entries: 1,
        ..policy
    };
    let mut work = WorkBudget::new(WorkLimits {
        walk_entries: 10,
        ..WorkLimits::default()
    });
    let limited = walk::walk_report_with_work(root.path(), &policy, &mut work).unwrap();
    assert!(limited.require_complete().is_err());
    assert_eq!(limited.coverage.entries_visited, 1);
    assert_eq!(work.remaining_walk_entries(), 9);
    assert!(work.report().2.is_empty(), "only the policy ceiling fired");
}

#[test]
fn freshness_and_retrieval_share_allowances_without_resetting_spent_work() {
    use graph_search_types::query::{ExploreQuery, SymbolQuery};
    let root = tempfile::tempdir().unwrap();
    let source = "fn alpha() {}\n";
    std::fs::write(root.path().join("a.rs"), source).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        verification: graph_search::Verification::Content,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let limits = WorkLimits {
        walk_entries: 2,
        source_files: 1,
        source_bytes: source.len(),
        ..WorkLimits::default()
    };
    for _ in 0..2 {
        let result = index
            .search()
            .with_work_limits(limits.clone())
            .symbol(&SymbolQuery::new("alpha"))
            .unwrap();
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.stats.source_files_attempted, 1);
        assert_eq!(result.stats.source_bytes_read, source.len() as u64);
    }
    for limits in [
        WorkLimits {
            walk_entries: 1,
            ..limits.clone()
        },
        WorkLimits {
            source_files: 0,
            ..limits.clone()
        },
        WorkLimits {
            source_bytes: source.len() - 1,
            ..limits.clone()
        },
    ] {
        assert!(matches!(
            index
                .search()
                .with_work_limits(limits)
                .symbol(&SymbolQuery::new("alpha")),
            Err(graph_search::Error::Core(
                graph_search_core::Error::IncompleteWalk(_)
                    | graph_search_core::Error::IncompleteVerification(_)
            ))
        ));
    }
    let mut query = ExploreQuery::new("alpha");
    query.retrieval.mode = graph_search_types::retrieval::ExploreMode::ExactName;
    let result = index
        .search()
        .with_work_limits(limits.clone())
        .explore(&query)
        .unwrap();
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].snippet.is_some());
    assert_eq!(result.stats.source_files_attempted, 1);
    assert_eq!(result.stats.source_bytes_read, source.len() as u64);
    assert!(!result.truncations.iter().any(|t| matches!(
        t.kind,
        TruncationKind::SourceFiles | TruncationKind::SourceBytes
    )));
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 1,
            source_bytes: source.len(),
            ..limits
        })
        .explore(&query)
        .unwrap();
    assert!(result.items[0].snippet.is_some());
    assert_eq!(result.stats.source_files_attempted, 1);
    assert_eq!(result.stats.source_bytes_read, source.len() as u64);
}

#[test]
fn a_reused_engine_inherits_prior_work_only_for_its_first_query() {
    use graph_search_core::{
        memory::MemoryStore, ports::GraphStore, query::QueryEngine, work::WorkBudget,
    };
    let store = MemoryStore::new();
    let snapshot = store.snapshot().unwrap();
    let mut work = WorkBudget::new(WorkLimits::default());
    assert!(work.source_file().unwrap());
    work.charge_source_bytes(4);
    let engine = QueryEngine::with_work_budget(snapshot.as_ref(), work);
    let query = graph_search_types::query::SymbolQuery::new("missing");
    let first = engine.symbol(&query).unwrap();
    assert_eq!(first.stats.source_files_attempted, 1);
    assert_eq!(first.stats.source_bytes_read, 4);
    let second = engine.symbol(&query).unwrap();
    assert_eq!(second.stats.source_files_attempted, 0);
    assert_eq!(second.stats.source_bytes_read, 0);
}

#[test]
fn automatic_cold_build_exhaustion_does_not_publish() {
    use graph_search_types::query::SymbolQuery;
    let root = tempfile::tempdir().unwrap();
    let source = "fn alpha() {}\n";
    std::fs::write(root.path().join("a.rs"), source).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .unwrap();
    for limits in [
        WorkLimits {
            source_files: 0,
            ..Default::default()
        },
        WorkLimits {
            source_bytes: source.len() - 1,
            ..Default::default()
        },
        WorkLimits {
            walk_entries: 3,
            ..Default::default()
        },
    ] {
        let result = index
            .search()
            .with_work_limits(limits)
            .symbol(&SymbolQuery::new("alpha"));
        assert!(matches!(
            result,
            Err(graph_search::Error::Core(
                graph_search_core::Error::IncompleteMaintenance(_)
                    | graph_search_core::Error::IncompleteWalk(_)
            ))
        ));
        assert!(!index.store_dir().join("CURRENT").exists());
    }
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 1,
            source_bytes: source.len(),
            walk_entries: 6,
            ..Default::default()
        })
        .symbol(&SymbolQuery::new("alpha"))
        .unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.stats.source_files_attempted, 1);
    assert_eq!(result.stats.source_bytes_read, source.len() as u64);
    assert!(index.store_dir().join("CURRENT").exists());
}

#[test]
fn automatic_incremental_hash_and_extraction_share_allowances() {
    use graph_search_types::query::SymbolQuery;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a.rs");
    std::fs::write(&path, "fn alpha() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let current = index.store_dir().join("CURRENT");
    let before = std::fs::read(&current).unwrap();
    let source = "fn replacement_function() {}\n";
    std::fs::write(&path, source).unwrap();
    for limits in [
        WorkLimits {
            source_files: 1,
            ..Default::default()
        },
        WorkLimits {
            source_bytes: source.len() * 2 - 1,
            ..Default::default()
        },
        WorkLimits {
            walk_entries: 3,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            index
                .search()
                .with_work_limits(limits)
                .symbol(&SymbolQuery::new("replacement_function")),
            Err(graph_search::Error::Core(
                graph_search_core::Error::IncompleteMaintenance(_)
                    | graph_search_core::Error::IncompleteWalk(_)
            ))
        ));
        assert_eq!(std::fs::read(&current).unwrap(), before);
    }
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 2,
            source_bytes: source.len() * 2,
            walk_entries: 6,
            ..Default::default()
        })
        .symbol(&SymbolQuery::new("replacement_function"))
        .unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.stats.source_files_attempted, 2);
    assert_eq!(result.stats.source_bytes_read, (source.len() * 2) as u64);
    assert_ne!(std::fs::read(&current).unwrap(), before);
}

#[test]
fn strict_verification_and_automatic_rebuild_share_allowances() {
    use graph_search_types::query::SymbolQuery;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a.rs");
    std::fs::write(&path, "fn alpha() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        verification: graph_search::Verification::Content,
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let current = index.store_dir().join("CURRENT");
    let before = std::fs::read(&current).unwrap();
    let source = "fn bravo() {}\n";
    std::fs::write(&path, source).unwrap();
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 1,
            ..Default::default()
        })
        .symbol(&SymbolQuery::new("bravo"));
    assert!(matches!(
        result,
        Err(graph_search::Error::Core(
            graph_search_core::Error::IncompleteMaintenance(_)
        ))
    ));
    assert_eq!(std::fs::read(&current).unwrap(), before);
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            source_files: 3,
            source_bytes: source.len() * 3,
            walk_entries: 6,
            ..Default::default()
        })
        .symbol(&SymbolQuery::new("bravo"))
        .unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.stats.source_files_attempted, 3);
    assert_eq!(result.stats.source_bytes_read, (source.len() * 3) as u64);
}

#[test]
fn status_honors_work_limits_and_uses_exact_cached_counts() {
    let root = tempfile::tempdir().unwrap();
    let source = "fn alpha() {}\n";
    std::fs::write(root.path().join("a.rs"), source).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        verification: graph_search::Verification::Content,
        ..Default::default()
    })
    .unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert!(matches!(
        index
            .search()
            .with_work_limits(WorkLimits {
                cancellation: Some(cancellation),
                ..Default::default()
            })
            .status(),
        Err(graph_search::Error::Core(
            graph_search_core::Error::QueryCancelled
        ))
    ));
    assert!(!index.search().status().unwrap().exists);
    index.reindex().unwrap();
    let limits = WorkLimits {
        walk_entries: 2,
        source_files: 1,
        source_bytes: source.len(),
        nodes: 0,
        edges: 0,
        ..Default::default()
    };
    for _ in 0..2 {
        let status = index
            .search()
            .with_work_limits(limits.clone())
            .status()
            .unwrap();
        assert_eq!(status.staleness.unwrap().changed, 0);
        let counts = status.counts.unwrap();
        assert_eq!(counts.total_nodes, 2);
        assert_eq!(counts.total_edges, 1);
    }
    for limits in [
        WorkLimits {
            walk_entries: 1,
            ..limits.clone()
        },
        WorkLimits {
            source_files: 0,
            ..limits.clone()
        },
        WorkLimits {
            source_bytes: source.len() - 1,
            ..limits
        },
    ] {
        assert!(matches!(
            index.search().with_work_limits(limits).status(),
            Err(graph_search::Error::Core(
                graph_search_core::Error::IncompleteWalk(_)
                    | graph_search_core::Error::IncompleteVerification(_)
            ))
        ));
    }
    assert!(matches!(
        index
            .search()
            .with_work_limits(WorkLimits {
                deadline: Some(std::time::Instant::now()),
                ..Default::default()
            })
            .status(),
        Err(graph_search::Error::Core(
            graph_search_core::Error::QueryDeadline
        ))
    ));
}

#[test]
fn metadata_examination_limits_are_reported_and_reset_per_query() {
    use graph_search_types::query::{ExploreQuery, SymbolQuery};
    let root = tempfile::tempdir().unwrap();
    for i in 0..8 {
        std::fs::write(root.path().join(format!("{i}.rs")), "fn shared() {}\n").unwrap();
    }
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let service = index.search().with_work_limits(WorkLimits {
        metadata_entries: 3,
        ..Default::default()
    });
    let mut query = SymbolQuery::new("shared");
    query.filters.path_glob = Some("7.rs".into());
    for _ in 0..2 {
        let result = service.symbol(&query).unwrap();
        assert!(result.nodes.is_empty());
        assert_eq!(result.stats.metadata_entries_examined, 3);
        assert_eq!(result.stats.retrieval_candidates_admitted, 0);
        assert!(
            result
                .truncations
                .iter()
                .any(|t| t.kind == TruncationKind::MetadataEntries)
        );
    }
    let mut query = ExploreQuery::new("*.rs").with_context_lines(0);
    query.retrieval.mode = graph_search_types::retrieval::ExploreMode::PathGlob;
    query.filters.path_glob = Some("7.rs".into());
    let result = service.explore(&query).unwrap();
    assert!(result.items.is_empty());
    assert_eq!(result.stats.metadata_entries_examined, 3);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::MetadataEntries)
    );
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            metadata_entries: 8,
            ..Default::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.stats.metadata_entries_examined, 8);
    assert!(
        !result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::MetadataEntries)
    );
}

#[test]
fn graph_filters_limit_output_while_result_caps_keep_compact_boundary_references() {
    use graph_search_types::query::SymbolQuery;
    let root = tempfile::tempdir().unwrap();
    for dir in ["visible", "outside"] {
        std::fs::create_dir(root.path().join(dir)).unwrap();
    }
    for (path, source) in [
        ("visible/start.rs", "fn start() { middle(); }\n"),
        ("outside/middle.rs", "fn middle() { end(); }\n"),
        ("visible/end.rs", "fn end() {}\n"),
    ] {
        std::fs::write(root.path().join(path), source).unwrap();
    }
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        reconcile: Reconcile::Never,
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let query = TraversalQuery::new("start", 2);
    let full = index.search().callees(&query).unwrap();
    assert_eq!(full.nodes.len(), 3);
    assert_eq!(full.edges.len(), 2);
    let mut filtered = query.clone();
    filtered.filters.path_glob = Some("visible/**".into());
    let output = index.search().callees(&filtered).unwrap();
    assert_eq!(output.nodes.len(), 2);
    assert!(output.nodes.iter().any(|node| node.name == "end"));
    assert!(
        output
            .nodes
            .iter()
            .all(|node| node.path.starts_with("visible/"))
    );
    assert_eq!(
        output.stats.graph_nodes_visited,
        full.stats.graph_nodes_visited
    );
    assert_eq!(
        output.stats.graph_edges_examined,
        full.stats.graph_edges_examined
    );
    let limited = index.search().callees(&query.with_limit(1)).unwrap();
    assert_eq!(limited.nodes.len(), 1);
    assert_eq!(limited.edges.len(), 2);
    assert_eq!(limited.stats.candidates, 3);
    assert!(
        limited
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Results)
    );
    let kept = &limited.nodes[0].id;
    for edge in &limited.edges {
        assert!(edge.resolved);
        let target = edge.to.as_ref().unwrap();
        let boundary = if &edge.from == kept {
            target
        } else {
            &edge.from
        };
        assert_ne!(boundary, kept);
        let resolved = index.search().symbol(&SymbolQuery::new(boundary)).unwrap();
        assert_eq!(resolved.nodes.len(), 1);
        assert_eq!(&resolved.nodes[0].id, boundary);
    }
}

#[test]
fn explore_captures_source_once_across_build_verification_and_evidence() {
    use graph_search_types::query::ExploreQuery;
    use graph_search_types::retrieval::ExploreMode;
    for verification in [
        graph_search::Verification::Metadata,
        graph_search::Verification::Content,
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.rs");
        let source = "fn alpha() {}\n";
        std::fs::write(&path, source).unwrap();
        let index = Index::open(OpenOptions {
            root: root.path().into(),
            verification,
            ..Default::default()
        })
        .unwrap();
        let mut generations = Vec::new();
        for (name, text) in [
            ("alpha", source),
            ("replacement_function", "fn replacement_function() {}\n"),
        ] {
            std::fs::write(&path, text).unwrap();
            let mut query = ExploreQuery::new(name);
            query.retrieval.mode = ExploreMode::ExactName;
            let result = index
                .search()
                .with_work_limits(WorkLimits {
                    source_files: 1,
                    source_bytes: text.len(),
                    ..Default::default()
                })
                .explore(&query)
                .unwrap();
            assert_eq!(result.items.len(), 1);
            assert_eq!(result.items[0].node.name, name);
            let snippet = result.items[0].snippet.as_ref().unwrap();
            assert_eq!(snippet.lines, [text.trim_end()]);
            assert_eq!(
                snippet.source_hash,
                graph_search_core::hash::content_hash(text.as_bytes())
            );
            let identity = &result.context.sources["a.rs"];
            assert_eq!(
                identity.verification,
                graph_search_types::context::SourceVerification::Verified
            );
            assert_eq!(identity.indexed_hash, identity.observed_hash);
            assert_eq!(result.stats.source_files_attempted, 1);
            assert_eq!(result.stats.source_bytes_read, text.len() as u64);
            assert!(!result.truncations.iter().any(|t| matches!(
                t.kind,
                TruncationKind::SourceFiles | TruncationKind::SourceBytes
            )));
            assert_eq!(result.context.staleness.changed, 0);
            generations.push(result.context.generation);
        }
        assert_ne!(
            generations[0], generations[1],
            "changed bytes must publish a new generation"
        );
    }
}

#[test]
fn strict_explore_capture_detects_restored_metadata_and_is_not_reused_by_next_request() {
    use graph_search_types::query::ExploreQuery;
    use graph_search_types::retrieval::ExploreMode;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a.rs");
    std::fs::write(&path, "fn alpha() {}\n").unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        verification: graph_search::Verification::Content,
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    for name in ["bravo", "cello"] {
        let text = format!("fn {name}() {{}}\n");
        std::fs::write(&path, &text).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let mut query = ExploreQuery::new(name);
        query.retrieval.mode = ExploreMode::ExactName;
        let current = index.store_dir().join("CURRENT");
        let before = std::fs::read(&current).unwrap();
        for limits in [
            WorkLimits {
                source_files: 0,
                ..Default::default()
            },
            WorkLimits {
                source_bytes: text.len() - 1,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                index.search().with_work_limits(limits).explore(&query),
                Err(graph_search::Error::Core(
                    graph_search_core::Error::IncompleteVerification(_)
                ))
            ));
            assert_eq!(std::fs::read(&current).unwrap(), before);
        }
        let result = index
            .search()
            .with_work_limits(WorkLimits {
                source_files: 1,
                source_bytes: text.len(),
                ..Default::default()
            })
            .explore(&query)
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].node.name, name);
        assert_eq!(
            result.items[0].snippet.as_ref().unwrap().lines,
            [text.trim_end()]
        );
        assert_eq!(result.stats.source_files_attempted, 1);
        assert_eq!(result.stats.source_bytes_read, text.len() as u64);
        assert_eq!(result.context.staleness.changed, 0);
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One fixture checks both bounds and a fitting control.
fn impossible_metadata_does_not_consume_the_later_candidates_source_allowance() {
    use graph_search_core::{memory::MemoryStore, ports::GraphStore, query::QueryEngine};
    use graph_search_types::{
        EdgeKind, ExploreMode, ExploreQuery, FileProjection, Node, NodeId, NodeKind, Span,
        WriteBatch,
    };
    let root = tempfile::tempdir().unwrap();
    let source = "fn target() {}\n";
    let hash = graph_search_core::hash::content_hash(source.as_bytes());
    let mut store = MemoryStore::new();
    let mut projections = Vec::new();
    for (path, signature) in [("a.rs", "x".repeat(10_000)), ("z.rs", "fn target()".into())] {
        std::fs::write(root.path().join(path), source).unwrap();
        let file = Node {
            id: NodeId::file(path),
            path: path.into(),
            kind: NodeKind::File,
            content_hash: Some(hash.clone()),
            ..Default::default()
        };
        let node = Node {
            id: NodeId::symbol(path, NodeKind::Function, "target", None),
            path: path.into(),
            kind: NodeKind::Function,
            name: Some("target".into()),
            signature: Some(signature),
            span: Some(Span {
                start_line: 1,
                end_line: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let edge = graph_search_types::Edge::resolved(
            &file.id,
            EdgeKind::Contains,
            &node.id,
            "target",
            None,
            None,
        );
        projections.push(FileProjection {
            file,
            symbols: vec![node],
            edges: vec![edge],
            ..Default::default()
        });
    }
    store
        .apply(WriteBatch {
            upserts: projections,
            ..Default::default()
        })
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let engine = QueryEngine::with_work_limits(
        snapshot.as_ref(),
        WorkLimits {
            source_files: 1,
            source_bytes: source.len(),
            ..Default::default()
        },
    );
    let mut query = ExploreQuery::new("target");
    query.retrieval.mode = ExploreMode::ExactName;
    query.retrieval.graph_context = graph_search_types::GraphContext::None;
    query.max_bytes = 4096;
    let result = engine.explore(&query, root.path()).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].node.path, "z.rs");
    assert_eq!(
        result.items[0].snippet.as_ref().unwrap().lines,
        [source.trim_end()]
    );
    assert_eq!(result.stats.source_files_attempted, 1);
    assert_eq!(result.stats.source_bytes_read, source.len() as u64);
    assert!(!result.context.sources.contains_key("a.rs"));
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes)
    );
    assert!(!result.truncations.iter().any(|t| matches!(
        t.kind,
        TruncationKind::SourceFiles | TruncationKind::SourceBytes
    )));
    assert!(serde_json::to_vec(&result).unwrap().len() <= 4096);

    // A single item's metadata can pass the per-item cap while the mandatory
    // result metadata leaves no room for even one empty source line.
    query.filters.path_glob = Some("a.rs".into());
    query.context_lines = 0;
    query.max_bytes = 0;
    let mut metadata_only = engine.explore(&query, root.path()).unwrap();
    assert_eq!(metadata_only.items.len(), 1);
    let metadata_bytes = serde_json::to_vec(&metadata_only.items[0]).unwrap().len();
    metadata_only.items.clear();
    metadata_only.context.sources.clear();
    metadata_only.stats = graph_search_types::Stats::default();
    let required = serde_json::to_vec(&metadata_only).unwrap().len();
    query.context_lines = 4;
    query.max_bytes = u32::try_from(required + metadata_bytes).unwrap();
    let no_source_room = engine.explore(&query, root.path()).unwrap();
    assert_eq!(no_source_room.stats.source_files_attempted, 0);
    assert_eq!(no_source_room.stats.source_bytes_read, 0);
    assert!(
        no_source_room
            .items
            .iter()
            .all(|item| item.snippet.is_none())
    );
    assert!(
        no_source_room
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes)
    );
    assert!(serde_json::to_vec(&no_source_room).unwrap().len() <= query.max_bytes as usize);

    query.max_bytes = 0;
    let admitted = engine.explore(&query, root.path()).unwrap();
    assert_eq!(admitted.items.len(), 1);
    assert_eq!(admitted.items[0].node.path, "a.rs");
    assert_eq!(
        admitted.items[0].snippet.as_ref().unwrap().lines,
        [source.trim_end()]
    );
    assert_eq!(admitted.stats.source_files_attempted, 1);
}
