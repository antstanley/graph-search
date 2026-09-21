//! Body retrieval is indexed, source-bound, and independent of metadata ranking.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_types::query::ExploreQuery;
use std::fmt::Write;

fn index(root: &std::path::Path) -> Index {
    let index = Index::open(OpenOptions {
        root: root.into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    index
}

#[test]
fn indexed_body_queries_cross_the_old_scan_limit_without_reading_every_file() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..520 {
        std::fs::write(root.path().join(format!("a{i:03}.txt")), "unrelated\n").unwrap();
    }
    std::fs::write(
        root.path().join("z_guide.md"),
        "# Cache maintenance\nRemove stale entries.\n",
    )
    .unwrap();
    let index = index(root.path());
    let result = index
        .search()
        .explore(&ExploreQuery::new("cache invalidation"))
        .unwrap();
    let hit = result
        .items
        .iter()
        .find(|item| item.node.path == "z_guide.md")
        .unwrap();
    assert!(!hit.evidence.as_ref().unwrap().live);
    assert!(
        hit.snippet
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|line| line.contains("Cache"))
    );
    assert_eq!(result.stats.files_scanned, 0);
    assert!(result.stats.lexical_postings_examined > 0);
}

#[test]
fn body_matches_compete_with_metadata_and_exact_names_keep_priority() {
    let root = tempfile::tempdir().unwrap();
    let mut text = String::new();
    for i in 0..20 {
        writeln!(text, "fn cache_handler_{i}() {{}}").unwrap();
    }
    std::fs::write(root.path().join("a.rs"), text).unwrap();
    std::fs::write(
        root.path().join("guide.md"),
        "Cache invalidation removes stale entries.\n",
    )
    .unwrap();
    let index = index(root.path());
    let result = index
        .search()
        .explore(&ExploreQuery::new("cache invalidation"))
        .unwrap();
    assert!(result.items.iter().any(|item| item.node.path == "guide.md"));
    let exact = index
        .search()
        .explore(&ExploreQuery::new("cache_handler_19"))
        .unwrap();
    assert_eq!(exact.items[0].node.name, "cache_handler_19");
}

#[test]
fn body_excerpts_show_the_match_without_rewriting_declaration_coordinates() {
    let root = tempfile::tempdir().unwrap();
    let mut text = String::from("fn process() {\n");
    text.push_str(&"    // ordinary work\n".repeat(110));
    text.push_str("    // invalidate cache on replacement\n}\n");
    std::fs::write(root.path().join("a.rs"), text).unwrap();
    let index = index(root.path());
    for radius in [2, 10, u32::MAX] {
        let mut query = ExploreQuery::new("invalidate cache");
        query.context_lines = radius;
        let result = index.search().explore(&query).unwrap();
        let hit = result
            .items
            .iter()
            .find(|item| item.node.name == "process")
            .unwrap();
        let evidence = hit.evidence.as_ref().unwrap();
        assert_eq!(hit.node.start_line, 1);
        assert_eq!(evidence.match_line, 112);
        assert_eq!(evidence.owner.as_ref().unwrap().as_str(), hit.node.id);
        assert!(
            hit.snippet
                .as_ref()
                .unwrap()
                .lines
                .iter()
                .any(|line| line.contains("invalidate cache"))
        );
        assert!(hit.snippet.as_ref().unwrap().lines.len() <= 10);
    }
}

#[test]
fn stale_body_facts_are_replaced_by_hash_bound_live_evidence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("guide.md"), "old_marker\n").unwrap();
    let index = index(root.path());
    std::fs::write(root.path().join("guide.md"), "replacement_marker\n").unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("replacement"))
        .unwrap();
    let hit = &result.items[0];
    let evidence = hit.evidence.as_ref().unwrap();
    assert!(evidence.live);
    assert!(evidence.owner.is_none());
    assert_eq!(
        evidence.source_hash,
        hit.snippet.as_ref().unwrap().source_hash
    );
    assert_ne!(
        Some(evidence.source_hash.as_str()),
        result.context.sources["guide.md"].indexed_hash.as_deref()
    );
    assert!(
        index
            .search()
            .explore(&ExploreQuery::new("old"))
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn a_single_stopword_is_a_valid_body_query() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("guide.md"), "is\n").unwrap();
    let index = index(root.path());
    assert_eq!(
        index
            .search()
            .explore(&ExploreQuery::new("is"))
            .unwrap()
            .items
            .len(),
        1
    );
}

fn delivered(result: &graph_search_types::result::ExploreResult) -> Vec<(String, u32, String)> {
    result
        .items
        .iter()
        .flat_map(|item| {
            item.snippet
                .iter()
                .chain(item.excerpts.iter().map(|e| &e.snippet))
                .flat_map(|snippet| {
                    snippet.lines.iter().enumerate().map(|(offset, text)| {
                        (
                            item.node.path.clone(),
                            snippet
                                .start_line
                                .saturating_add(u32::try_from(offset).unwrap()),
                            text.clone(),
                        )
                    })
                })
        })
        .collect()
}

#[test]
fn small_implementations_expand_with_exact_lines_and_no_duplicate_extra_context() {
    let root = tempfile::tempdir().unwrap();
    let mut text = String::from("fn process() {\n");
    for i in 0..35 {
        writeln!(text, "    // step {i}").unwrap();
    }
    text.push_str("}\n");
    std::fs::write(root.path().join("a.rs"), &text).unwrap();
    let index = index(root.path());
    let result = index
        .search()
        .explore(&ExploreQuery::new("process"))
        .unwrap();
    let lines = delivered(&result);
    let unique: std::collections::BTreeSet<_> = lines.iter().map(|(p, n, _)| (p, n)).collect();
    assert_eq!(lines.len(), unique.len());
    assert_eq!(lines.len(), text.lines().count());
    for (_, number, content) in lines {
        assert_eq!(
            content,
            text.lines().nth(number.saturating_sub(1) as usize).unwrap()
        );
    }
    for cap in [1500, 2500, 4000, 65536] {
        let mut query = ExploreQuery::new("process");
        query.max_bytes = cap;
        let result = index.search().explore(&query).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= cap as usize);
    }
    let no_context = index
        .search()
        .explore(&ExploreQuery::new("process").with_context_lines(0))
        .unwrap();
    assert!(
        no_context
            .items
            .iter()
            .all(|item| item.snippet.is_none() && item.excerpts.is_empty())
    );
}

#[test]
fn expanded_context_preserves_headers_matches_and_hash_checks() {
    let root = tempfile::tempdir().unwrap();
    let mut text = String::from("fn process() {\n");
    text.push_str(&"    // ordinary work\n".repeat(110));
    text.push_str("    // invalidate cache on replacement\n}\n");
    std::fs::write(root.path().join("a.rs"), &text).unwrap();
    let index = index(root.path());
    let result = index
        .search()
        .explore(&ExploreQuery::new("invalidate cache"))
        .unwrap();
    let lines = delivered(&result);
    assert!(lines.iter().any(|(_, n, _)| *n == 1));
    assert!(lines.iter().any(|(_, n, _)| *n == 112));
    let hash = graph_search_core::hash::content_hash(text.as_bytes());
    assert!(
        result
            .items
            .iter()
            .flat_map(|item| &item.excerpts)
            .all(|excerpt| excerpt.snippet.source_hash == hash)
    );
    std::fs::write(root.path().join("a.rs"), "fn replacement() {}\n").unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("process"))
        .unwrap();
    assert!(
        result
            .items
            .iter()
            .all(|item| item.snippet.is_none() && item.excerpts.is_empty())
    );
}

#[test]
fn returned_connections_include_distant_call_site_context() {
    let root = tempfile::tempdir().unwrap();
    let mut text = String::from("fn entry() {\n");
    text.push_str(&"    // ordinary work\n".repeat(110));
    text.push_str("    leaf();\n}\nfn leaf() {}\n");
    std::fs::write(root.path().join("a.rs"), text).unwrap();
    let index = index(root.path());
    let result = index
        .search()
        .explore(&ExploreQuery::new("entry leaf"))
        .unwrap();
    assert!(result.edges.iter().any(|edge| edge.line == Some(112)));
    assert!(
        delivered(&result)
            .iter()
            .any(|(_, n, text)| *n == 112 && text.contains("leaf();"))
    );
}

#[test]
fn oversized_source_lines_do_not_erase_navigation_metadata() {
    let root = tempfile::tempdir().unwrap();
    let text = format!(
        "fn budget_large() {{ let value = \"{}\"; }}\nfn budget_small() {{}}\n",
        "x".repeat(20_000)
    );
    std::fs::write(root.path().join("a.rs"), text).unwrap();
    let index = index(root.path());
    let mut query = ExploreQuery::new("budget_");
    query.retrieval.mode = graph_search_types::ExploreMode::NamePrefix;
    for cap in [4096, 8192, 16384] {
        query.max_bytes = cap;
        let result = index.search().explore(&query).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= cap as usize);
        assert_eq!(result.items.len(), 2);
        assert!(result.items.iter().all(|item| item.snippet.is_none()));
        assert!(result.truncations.iter().any(|t| t.kind
            == graph_search_types::TruncationKind::Bytes
            && t.cap == u64::from(cap)));
        assert!(result.items.iter().all(|item| item.excerpts.is_empty()));
    }
    query.max_bytes = 1;
    assert!(matches!(
        index.search().explore(&query),
        Err(graph_search::Error::Core(
            graph_search_core::Error::ResultBudget(1)
        ))
    ));
}

#[test]
fn repeated_relationship_sites_expand_under_an_independent_work_budget() {
    use graph_search_core::work::WorkLimits;
    use graph_search_types::{RankingStrategy, TruncationKind};
    let root = tempfile::tempdir().unwrap();
    let mut text = String::from("fn entry() {\n");
    let mut call_lines = Vec::new();
    for _ in 0..3 {
        text.push_str(&"    // ordinary work\n".repeat(110));
        call_lines.push(
            u32::try_from(text.lines().count())
                .unwrap()
                .saturating_add(1),
        );
        text.push_str("    leaf();\n");
    }
    text.push_str("}\nfn leaf() {}\n");
    std::fs::write(root.path().join("a.rs"), &text).unwrap();
    let index = index(root.path());
    let mut query = ExploreQuery::new("entry leaf");
    query.retrieval.ranking = RankingStrategy::Metadata;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].occurrence_count, Some(3));
    assert_eq!(result.stats.occurrences_examined, 3);
    let lines = delivered(&result);
    for line in &call_lines {
        assert!(
            lines
                .iter()
                .any(|(_, n, text)| n == line && text.contains("leaf();")),
            "missing call at {line}"
        );
    }
    let unique: std::collections::BTreeSet<_> =
        lines.iter().map(|(path, line, _)| (path, line)).collect();
    assert_eq!(unique.len(), lines.len());
    let limited = index
        .search()
        .with_work_limits(WorkLimits {
            occurrences: 1,
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(limited.stats.occurrences_examined, 1);
    assert_eq!(
        limited
            .truncations
            .iter()
            .filter(|t| t.kind == TruncationKind::Occurrences)
            .count(),
        1
    );
    assert!(
        delivered(&limited)
            .iter()
            .any(|(_, n, _)| *n == call_lines[0])
    );
    assert!(
        !delivered(&limited)
            .iter()
            .any(|(_, n, _)| *n == call_lines[2])
    );
    let no_edges = index
        .search()
        .with_work_limits(WorkLimits {
            returned_edges: 0,
            ..WorkLimits::default()
        })
        .explore(&query)
        .unwrap();
    assert_eq!(no_edges.stats.occurrences_examined, 0);
    assert!(
        !delivered(&no_edges)
            .iter()
            .any(|(_, n, _)| *n == call_lines[2])
    );
    let no_context = index
        .search()
        .explore(&query.with_context_lines(0))
        .unwrap();
    assert_eq!(no_context.stats.occurrences_examined, 0);
}

#[test]
fn context_cost_work_is_bounded_without_discarding_primary_evidence() {
    use graph_search_core::work::WorkLimits;
    use graph_search_types::TruncationKind;
    let root = tempfile::tempdir().unwrap();
    let source = format!(
        "fn process() {{\n{}}}\n",
        "    // implementation detail\n".repeat(35)
    );
    std::fs::write(root.path().join("a.rs"), source).unwrap();
    let index = index(root.path());
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            context_windows: 0,
            ..WorkLimits::default()
        })
        .explore(&ExploreQuery::new("process"))
        .unwrap();
    assert!(!result.items.is_empty());
    assert!(result.items.iter().any(|item| item.snippet.is_some()));
    assert!(result.items.iter().all(|item| item.excerpts.is_empty()));
    assert_eq!(result.stats.context_windows_examined, 0);
    assert_eq!(
        result
            .truncations
            .iter()
            .filter(|item| item.kind == TruncationKind::ContextWindows)
            .count(),
        1
    );
    let complete = index
        .search()
        .explore(&ExploreQuery::new("process"))
        .unwrap();
    assert!(complete.stats.context_windows_examined > 0);
    assert_eq!(delivered(&complete).len(), 37);
}

#[test]
fn distant_body_matches_fit_when_the_whole_region_does_not() {
    let root = tempfile::tempdir().unwrap();
    let mut source = String::from("fn process() {\r\n");
    for n in 2..70 {
        if [10, 35, 60].contains(&n) {
            writeln!(source, "    // needle transition café {n}\r").unwrap();
        } else {
            writeln!(source, "    // {}\r", "ordinary ".repeat(40)).unwrap();
        }
    }
    source.push_str("}\r\n");
    std::fs::write(root.path().join("a.rs"), &source).unwrap();
    let index = index(root.path());
    let mut query = ExploreQuery::new("needle transition").with_context_lines(1);
    query.k = 1;
    query.max_bytes = 8000;
    for live in [false, true] {
        if live {
            source.insert_str(0, "// changed source version\r\n");
            std::fs::write(root.path().join("a.rs"), &source).unwrap();
        }
        let result = index.search().explore(&query).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].evidence.as_ref().unwrap().live, live);
        assert!(serde_json::to_vec(&result).unwrap().len() <= query.max_bytes as usize);
        let lines = delivered(&result);
        assert!(lines.len() < source.lines().count());
        let unique: std::collections::BTreeSet<_> =
            lines.iter().map(|(path, line, _)| (path, line)).collect();
        assert_eq!(unique.len(), lines.len());
        for marker in [10, 35, 60] {
            assert!(
                lines
                    .iter()
                    .any(|(_, _, text)| text.ends_with(&format!("café {marker}"))),
                "missing match {marker} from live={live}"
            );
        }
        for (_, line, text) in &lines {
            assert_eq!(source.lines().nth(*line as usize - 1), Some(text.as_str()));
        }
        let hash = graph_search_core::hash::content_hash(source.as_bytes());
        assert!(
            result.items[0]
                .excerpts
                .iter()
                .all(|excerpt| excerpt.snippet.source_hash == hash)
        );
    }
}

#[test]
fn adjacent_call_windows_do_not_spend_one_interval_slot_per_site() {
    let root = tempfile::tempdir().unwrap();
    let mut source = String::from("fn entry() {\n");
    let mut sites = Vec::new();
    for n in 0..90 {
        sites.push(2 + n * 3);
        source.push_str("    leaf();\n    // nearby context\n    // more context\n");
    }
    source.push_str("}\nfn leaf() {}\n");
    std::fs::write(root.path().join("a.rs"), &source).unwrap();
    let index = index(root.path());
    let mut query = ExploreQuery::new("entry leaf");
    query.retrieval.ranking = graph_search_types::RankingStrategy::Metadata;
    query.k = 2;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].occurrence_count, Some(90));
    let lines = delivered(&result);
    for site in sites {
        assert!(
            lines
                .iter()
                .any(|(_, line, text)| *line == site && text.contains("leaf();"))
        );
    }
    let excerpts: Vec<_> = result
        .items
        .iter()
        .flat_map(|item| &item.excerpts)
        .collect();
    assert!(excerpts.len() < 64);
    assert!(
        excerpts
            .iter()
            .all(|excerpt| excerpt.snippet.lines.len() <= 80)
    );
    assert!(result.stats.context_windows_examined < 10_000);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 65_536);
    let unique: std::collections::BTreeSet<_> =
        lines.iter().map(|(path, line, _)| (path, line)).collect();
    assert_eq!(unique.len(), lines.len());
    for (_, line, text) in lines {
        assert_eq!(source.lines().nth(line as usize - 1), Some(text.as_str()));
    }
}

#[test]
fn distant_regions_of_one_owner_survive_without_duplicating_candidates() {
    for extension in ["rs", "txt"] {
        let root = tempfile::tempdir().unwrap();
        let mut lines = vec!["// ordinary unrelated work".to_owned(); 400];
        lines[0] = "fn process() {".into();
        lines[20] = "// cobalt initialization".into();
        lines[250] = "// amber completion".into();
        lines[399] = "}".into();
        let text = lines.join("\n");
        let path = format!("long.{extension}");
        std::fs::write(root.path().join(&path), &text).unwrap();
        let index = index(root.path());
        let result = index
            .search()
            .explore(&ExploreQuery::new("cobalt amber"))
            .unwrap();
        let hits: Vec<_> = result
            .items
            .iter()
            .filter(|item| item.node.path == path)
            .collect();
        assert_eq!(hits.len(), 1, "regions must not duplicate owner candidates");
        let item = hits[0];
        let snippets: Vec<_> = item
            .snippet
            .iter()
            .chain(item.excerpts.iter().map(|e| &e.snippet))
            .collect();
        for expected in [21, 251] {
            assert!(
                snippets.iter().any(|s| s.start_line <= expected
                    && expected < s.start_line + u32::try_from(s.lines.len()).unwrap()),
                "missing distant match {expected} for {extension}"
            );
        }
        for snippet in snippets {
            for (offset, line) in snippet.lines.iter().enumerate() {
                assert_eq!(line, &lines[snippet.start_line as usize - 1 + offset]);
            }
            assert_eq!(
                snippet.source_hash,
                item.evidence.as_ref().unwrap().source_hash
            );
        }
        assert!(serde_json::to_vec(&result).unwrap().len() <= 65536);
    }
}

#[test]
fn unselected_owners_do_not_report_context_loss_for_selected_results() {
    let root = tempfile::tempdir().unwrap();
    let mut lines = vec!["ordinary filler words"; 700];
    for line in [20, 150, 280, 410, 540, 670] {
        lines[line] = "cobalt amber";
    }
    std::fs::write(root.path().join("long.txt"), lines.join("\n")).unwrap();
    std::fs::write(root.path().join("short.txt"), "cobalt amber").unwrap();
    let index = index(root.path());
    let mut query = ExploreQuery::new("cobalt amber");
    query.k = 1;
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items[0].node.path, "short.txt");
    assert!(
        !result
            .truncations
            .iter()
            .any(|notice| notice.message.contains("per-owner"))
    );
    query.k = 2;
    let result = index.search().explore(&query).unwrap();
    assert!(
        result
            .truncations
            .iter()
            .any(|notice| notice.message.contains("per-owner"))
    );
}
