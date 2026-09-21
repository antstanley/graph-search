//! Reproducible, dependency-neutral probes for the September search review.
//! Production paths are called directly; prototypes are explicitly labelled.
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::{
    config::WalkPolicy,
    lexical::{LexicalIndex, query_terms, tokens},
    memory::MemoryStore,
    ports::{GraphStore, LanguageExtractor, ListRegistry, SourceFile},
    query::QueryEngine,
    reconcile::Projector,
    text_search::search_text,
};
use graph_search_types::{
    Edge, EdgeKind, Node, NodeId, NodeKind, WriteBatch,
    batch::FileProjection,
    query::{ExploreQuery, GraphFilters, TextQuery, TraversalQuery},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, hint::black_box, time::Instant};

fn file(path: &str) -> Node {
    Node {
        id: NodeId::file(path),
        path: path.into(),
        kind: NodeKind::File,
        ..Node::default()
    }
}
fn symbol(path: &str, name: &str) -> Node {
    Node {
        id: NodeId::symbol(path, NodeKind::Function, name, None),
        path: path.into(),
        kind: NodeKind::Function,
        name: Some(name.into()),
        qualified_name: Some(name.into()),
        ..Node::default()
    }
}
fn median(mut f: impl FnMut(), repeats: usize) -> f64 {
    f();
    let mut samples = Vec::new();
    for _ in 0..repeats {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    samples[repeats / 2]
}

fn text_probe() -> Value {
    let tmp = tempfile::tempdir().unwrap();
    let policy = WalkPolicy::default();
    let mut rows = Vec::new();
    for n in [4_000, 8_000, 16_000, 32_000] {
        let text = "needle payload\n".repeat(n);
        std::fs::write(tmp.path().join("data.txt"), &text).unwrap();
        let q = TextQuery::new("needle").with_limit(1);
        let current = median(
            || {
                black_box(search_text(tmp.path(), &q, &policy).unwrap());
            },
            5,
        );
        // Same filesystem walk/read; only line matching and early stop differ.
        let prototype = median(
            || {
                let entries = graph_search_core::walk::walk(tmp.path(), &policy).unwrap();
                let mut hits = Vec::new();
                for entry in entries {
                    let text = std::fs::read_to_string(entry.path).unwrap();
                    for (i, line) in text.lines().enumerate() {
                        if line.contains("needle") {
                            if hits.len() == 1 {
                                break;
                            }
                            hits.push((i + 1, line.to_owned()));
                        }
                    }
                }
                assert_eq!(hits, vec![(1, "needle payload".into())]);
                black_box(hits);
            },
            5,
        );
        let result = search_text(tmp.path(), &q, &policy).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].line, 1);
        assert!(!result.truncations.is_empty());
        rows.push(
            json!({"lines":n,"bytes":text.len(),"current_ms":current,"prototype_ms":prototype}),
        );
    }
    std::fs::write(tmp.path().join("data.txt"), "alpha\nbeta\n").unwrap();
    let q = TextQuery::new("alpha\nbeta");
    let sensitive = search_text(tmp.path(), &q, &policy);
    let mut folded_q = q;
    folded_q.ignore_case = true;
    let folded = search_text(tmp.path(), &folded_q, &policy);
    json!({"dense_match_scaling":rows,"multiline":{"case_sensitive_hits":sensitive.as_ref().ok().map(|r| r.items.len()),
        "ignore_case_hits":folded.as_ref().ok().map(|r| r.items.len()),
        "case_sensitive_error":sensitive.err().map(|e| e.to_string()),
        "ignore_case_error":folded.err().map(|e| e.to_string())}})
}

// Experimental native postings, preserving current token/weight/IDF semantics.
// Deliberately uncompressed and exhaustive over the union: no score pruning.
struct Postings {
    lists: BTreeMap<String, Vec<(usize, f32)>>,
    lengths: Vec<usize>,
    average: f32,
}
impl Postings {
    fn new(nodes: &[Node]) -> Self {
        let stop = [
            "how", "does", "the", "a", "an", "is", "are", "in", "to", "of", "and", "where", "what",
            "for", "with",
        ];
        let mut lists: BTreeMap<String, Vec<(usize, f32)>> = BTreeMap::new();
        let mut lengths = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            let mut tf = BTreeMap::new();
            let mut length = 0;
            for (field, weight) in [
                (node.name.as_deref().unwrap_or(""), 8.0),
                (node.path.as_str(), 2.0),
                (node.signature.as_deref().unwrap_or(""), 1.0),
            ] {
                for term in tokens(field) {
                    if stop.contains(&term.as_str()) {
                        continue;
                    }
                    length += 1;
                    *tf.entry(term).or_insert(0.0) += weight;
                }
            }
            for (term, f) in tf {
                lists.entry(term).or_default().push((i, f));
            }
            lengths.push(length);
        }
        let average = (lengths.iter().sum::<usize>() as f32 / nodes.len().max(1) as f32).max(1.0);
        Self {
            lists,
            lengths,
            average,
        }
    }
    fn scores(&self, terms: &[String]) -> Vec<(usize, f32)> {
        let mut scores = BTreeMap::new();
        for term in terms {
            let Some(list) = self.lists.get(term) else {
                continue;
            };
            let df = list.len() as f32;
            let count = self.lengths.len() as f32;
            let idf = ((count - df + 0.5) / (df + 0.5)).ln().max(0.000_001);
            for &(i, tf) in list {
                let score = idf * tf * 2.2
                    / (tf + 1.2 * (0.25 + 0.75 * self.lengths[i] as f32 / self.average));
                *scores.entry(i).or_insert(0.0) += score;
            }
        }
        scores.into_iter().collect()
    }
}
fn exhaustive(index: &LexicalIndex, count: usize, terms: &[String]) -> Vec<(usize, f32)> {
    (0..count)
        .filter_map(|i| {
            let score = index.score(i, terms);
            (score > 0.0).then_some((i, score))
        })
        .collect()
}
fn lexical_probe() -> Value {
    let mut rows = Vec::new();
    for n in [1_000, 10_000, 50_000] {
        let nodes: Vec<_> = (0..n)
            .map(|i| {
                let mut node = symbol(
                    &format!("src/module_{}/item.rs", i % 100),
                    &format!("handleEvent{i}"),
                );
                node.signature = Some(format!(
                    "fn handleEvent{i}(x: Topic{}) -> Outcome",
                    i % 1000
                ));
                node
            })
            .collect();
        let index = LexicalIndex::new(&nodes);
        let postings = Postings::new(&nodes);
        // Additional OR/common/missing queries verify full score maps, not just top-1.
        for query in [
            "topic731 topic12",
            "handle event",
            "module item",
            "absent",
            "topic99 outcome",
        ] {
            let t = query_terms(query);
            assert_eq!(exhaustive(&index, n, &t), postings.scores(&t));
        }
        for query in ["topic731 absent", "topic731 outcomeMissing"] {
            let terms = query_terms(query);
            let truth = exhaustive(&index, n, &terms);
            assert_eq!(truth, postings.scores(&terms));
            let native_scores = || {
                let mut scores = BTreeMap::new();
                let mut budget =
                    graph_search_core::work::WorkBudget::new(graph_search_core::work::WorkLimits {
                        candidates: n,
                        postings: 1_000_000,
                        ..graph_search_core::work::WorkLimits::default()
                    });
                index
                    .accumulate(&terms, &mut scores, |_| true, &mut budget)
                    .unwrap();
                assert!(budget.report().2.is_empty());
                (scores, budget.lexical_report())
            };
            let (native_truth, native_work) = native_scores();
            assert_eq!(truth, native_truth.into_iter().collect::<Vec<_>>());
            let native_ms = median(
                || {
                    black_box(native_scores());
                },
                5,
            );
            let rebuild = median(
                || {
                    let idx = LexicalIndex::new(black_box(&nodes));
                    black_box(exhaustive(&idx, n, &terms));
                },
                5,
            );
            let cached = median(
                || {
                    black_box(exhaustive(&index, n, &terms));
                },
                5,
            );
            let inverted = median(
                || {
                    black_box(postings.scores(&terms));
                },
                5,
            );
            rows.push(json!({"symbols":n,"query":query,"matching_documents":truth.len(),"rebuild_and_score_ms":rebuild,
                "cached_exhaustive_ms":cached,"postings_ms":inverted,"native_postings_ms":native_ms,
                "native_candidates":native_work.0,"native_postings_examined":native_work.1,"score_maps_equal":true}));
        }
    }
    let nodes = vec![symbol("a.rs", "getHTTPResponse"), symbol("b.rs", "other")];
    let idx = LexicalIndex::new(&nodes);
    json!({"scaling":rows,"whole_lowercase_identifier":{"terms":query_terms("gethttpresponse"),
        "lexical_score":idx.score(0,&query_terms("gethttpresponse")),"exact_lane_still_matches":true},
        "single_stopword_score":idx.score(0,&query_terms("is"))})
}
fn conjunction_probe() -> Value {
    let mut rows = Vec::new();
    for n in [1_000, 10_000, 50_000] {
        let nodes: Vec<_> = (0..n)
            .map(|i| {
                let mut node = symbol(&format!("src/{i}.rs"), &format!("handleEvent{i}"));
                node.signature = Some(format!(
                    "fn handleEvent{i}(x: Topic{}) -> Outcome",
                    i % 1000
                ));
                node
            })
            .collect();
        let index = LexicalIndex::new(&nodes);
        let reference = Postings::new(&nodes);
        for query in ["topic731 outcome", "handle outcome", "topic731 absent"] {
            let terms = query_terms(query);
            let truth: Vec<_> = reference
                .scores(&terms)
                .into_iter()
                .filter(|(i, _)| {
                    terms.iter().all(|term| {
                        reference.lists.get(term).is_some_and(|list| {
                            list.binary_search_by_key(i, |(doc, _)| *doc).is_ok()
                        })
                    })
                })
                .collect();
            let run = |conjunction: bool| {
                let mut scores = BTreeMap::new();
                let mut coverage = BTreeMap::new();
                let mut budget =
                    graph_search_core::work::WorkBudget::new(graph_search_core::work::WorkLimits {
                        candidates: n,
                        postings: 1_000_000,
                        ..graph_search_core::work::WorkLimits::default()
                    });
                if conjunction {
                    index
                        .accumulate_all(&terms, &mut scores, |_| true, &mut budget, &mut coverage)
                        .unwrap();
                } else {
                    index
                        .accumulate_coverage(
                            &terms,
                            &mut scores,
                            |_| true,
                            &mut budget,
                            &mut coverage,
                        )
                        .unwrap();
                    scores.retain(|i, _| coverage.get(i) == Some(&terms.len()));
                }
                assert!(budget.report().2.is_empty());
                (
                    scores.into_iter().collect::<Vec<_>>(),
                    budget.lexical_report(),
                )
            };
            let (actual, intersection_work) = run(true);
            let (union, union_work) = run(false);
            assert_eq!(actual, truth);
            assert_eq!(union, truth);
            let intersection_ms = median(
                || {
                    black_box(run(true));
                },
                5,
            );
            let union_ms = median(
                || {
                    black_box(run(false));
                },
                5,
            );
            rows.push(json!({"symbols":n,"query":query,"matches":truth.len(),
                "intersection_ms":intersection_ms,"union_filter_ms":union_ms,
                "intersection_candidates":intersection_work.0,"intersection_probes":intersection_work.1,
                "union_candidates":union_work.0,"union_postings":union_work.1,"score_maps_equal":true}));
        }
    }
    json!({"rows":rows,"note":"same native scorer, independent prototype score/membership oracle; budgets raised to measure complete execution"})
}

fn bound_probe() -> Value {
    let score = |tf: f64, length: f64, average: f64| {
        tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * length / average))
    };
    let rows: Vec<_> = [1.0, 1000.0]
        .into_iter()
        .map(|average| {
            json!({"average_length":average,
        "short_tf1":score(1.0,1.0,average),"long_tf3":score(3.0,100.0,average),
        "conservative_max_tf_min_length":score(3.0,1.0,average)})
        })
        .collect();
    json!({"purpose":"algebraic bound counterexample, not a Tantivy execution","rows":rows})
}

fn body_probe() -> Value {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..512 {
        std::fs::write(tmp.path().join(format!("a{i:03}.txt")), "unrelated\n").unwrap();
    }
    std::fs::write(
        tmp.path().join("z_target.md"),
        "# Cache invalidation\nFlush cached entries after replacement.\n",
    )
    .unwrap();
    let store = MemoryStore::new();
    let snapshot = store.snapshot().unwrap();
    let engine = QueryEngine::new(snapshot.as_ref());
    let mut q = ExploreQuery::new("cache invalidation");
    q.filters = GraphFilters {
        lang: None,
        path_glob: Some("z_target.md".into()),
    };
    let late = engine.explore(&q, tmp.path()).unwrap();
    std::fs::rename(
        tmp.path().join("z_target.md"),
        tmp.path().join("0_target.md"),
    )
    .unwrap();
    q.filters.path_glob = Some("0_target.md".into());
    let early = engine.explore(&q, tmp.path()).unwrap();
    let filtered = json!({"late_items":late.items.len(),"early_items":early.items.len(),
        "late_scanned":late.stats.files_scanned,"late_truncations":late.truncations});
    let tmp2 = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp2.path().join("guide.md"),
        "# Cache maintenance\nRemove stale entries.\n",
    )
    .unwrap();
    let before = engine
        .explore(&ExploreQuery::new("cache invalidation"), tmp2.path())
        .unwrap();
    let one = engine
        .explore(&ExploreQuery::new("cache"), tmp2.path())
        .unwrap();
    let mut store = MemoryStore::new();
    let symbols = (0..8)
        .map(|i| symbol("metadata.rs", &format!("cacheHandler{i}")))
        .collect();
    store
        .apply(WriteBatch {
            upserts: vec![FileProjection {
                file: file("metadata.rs"),
                symbols,
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    std::fs::write(
        tmp2.path().join("guide.md"),
        "# Cache invalidation\nRemove stale entries.\n",
    )
    .unwrap();
    let snap = store.snapshot().unwrap();
    let crowded = QueryEngine::new(snap.as_ref())
        .explore(&ExploreQuery::new("cache invalidation"), tmp2.path())
        .unwrap();
    json!({"filter_after_scan_budget":filtered,"longest_term":{"two_term_items":before.items.len(),
        "cache_only_items":one.items.len()},"metadata_starvation":{"items":crowded.items.iter().map(|i|&i.node.path).collect::<Vec<_>>(),
            "body_file_returned":crowded.items.iter().any(|i|i.node.path=="guide.md")}})
}

fn graph_probe() -> Value {
    let mut store = MemoryStore::new();
    let root = symbol("a.rs", "hub");
    let mut symbols = vec![root.clone()];
    let mut edges = Vec::new();
    for i in 0..2000 {
        let n = symbol("z.rs", &format!("leaf{i}"));
        edges.push(Edge::resolved(
            &root.id,
            EdgeKind::Calls,
            &n.id,
            &format!("leaf{i}"),
            Some("a.rs"),
            Some(1),
        ));
        symbols.push(n);
    }
    let leaves = symbols.split_off(1);
    store
        .apply(WriteBatch {
            upserts: vec![
                FileProjection {
                    file: file("a.rs"),
                    symbols,
                    edges,
                    ..Default::default()
                },
                FileProjection {
                    file: file("z.rs"),
                    symbols: leaves,
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
        .unwrap();
    let snap = store.snapshot().unwrap();
    let result = QueryEngine::new(snap.as_ref())
        .callees(&TraversalQuery::new("hub", 1).with_limit(1))
        .unwrap();
    let bounded = QueryEngine::with_work_limits(
        snap.as_ref(),
        graph_search_core::work::WorkLimits {
            nodes: 4,
            edges: 8,
            ..graph_search_core::work::WorkLimits::default()
        },
    )
    .callees(&TraversalQuery::new("hub", 4).with_limit(1))
    .unwrap();
    json!({"nodes_returned":result.nodes.len(),"edges_returned":result.edges.len(),
        "candidates":result.stats.candidates,"serialized_bytes":serde_json::to_vec(&result).unwrap().len(),
        "work": result.stats, "truncations": result.truncations,
        "bounded_work": {"stats": bounded.stats, "truncations": bounded.truncations,
            "nodes_returned": bounded.nodes.len(), "edges_returned": bounded.edges.len()}})
}

fn semantic_probe() -> Value {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("a.rs"),
        "fn send() {}\nfn other() {}\nfn caller() { let send = other; send(); }\n",
    )
    .unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy)
        .reindex(tmp.path(), &mut store)
        .unwrap();
    let snap = store.snapshot().unwrap();
    let edges = snap.all_edges().unwrap();
    let calls: Vec<_> = edges.iter().filter(|e| e.kind == EdgeKind::Calls).collect();
    let source = "function demo() {}\n";
    let ex = graph_search_langs::TypeScriptExtractor
        .extract(&SourceFile {
            path: std::path::Path::new("demo.ts"),
            text: source,
        })
        .unwrap();
    json!({"local_value_shadow_calls":calls,"typescript_first_symbol_span":ex.symbols[0].span,
        "typescript_source_bytes":source.len()})
}

fn stale_probe() -> Value {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.rs"), "fn original() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: tmp.path().into(),
        reconcile: Reconcile::Never,
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    std::fs::write(
        tmp.path().join("a.rs"),
        "// changed source\nfn replacement_function() {}\n",
    )
    .unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("original"))
        .unwrap();
    json!({"status_staleness":index.search().status().unwrap().staleness,"result":result})
}
fn metadata_freshness_probe() -> Value {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("a.rs");
    std::fs::write(&path, "fn original() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: tmp.path().into(),
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, "fn renamedd() {}\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let stale = index.search().status().unwrap().staleness;
    let result = index
        .search()
        .symbol(&graph_search_types::query::SymbolQuery::new("original"))
        .unwrap();
    json!({"status_staleness":stale,"old_symbol_returned":!result.nodes.is_empty(),
        "description":"same-length edit with original mtime restored; default BeforeQuery policy"})
}

fn atomicity_probe() -> Value {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = graph_search_engine::GrafeoStore::open(
        tmp.path(),
        &graph_search_engine::StoreOptions { in_memory: true },
    )
    .unwrap();
    let batch = |name: &str| WriteBatch {
        upserts: vec![FileProjection {
            file: file("a.rs"),
            symbols: vec![symbol("a.rs", name)],
            ..Default::default()
        }],
        ..Default::default()
    };
    store.apply(batch("old")).unwrap();
    // Fail the sidecar write after graph mutations; isolated in a temp store.
    std::fs::create_dir(tmp.path().join("CURRENT.tmp")).unwrap();
    let error = store.apply(batch("new")).unwrap_err().to_string();
    let snap = store.snapshot().unwrap();
    json!({"error":error,"old_present":snap.node_by_id(&symbol("a.rs","old").id).unwrap().is_some(),
        "new_present":snap.node_by_id(&symbol("a.rs","new").id).unwrap().is_some()})
}
fn repository_probe(root: &std::path::Path) -> Value {
    let tmp = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let index = Index::open(OpenOptions {
        root: root.into(),
        store: Some(tmp.path().join("index")),
        ..Default::default()
    })
    .unwrap();
    let open_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    index.reindex().unwrap();
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let status = index.search().status().unwrap();
    let store_root = tmp.path().join("index");
    let data_dir = if store_root.join("CURRENT").exists() {
        let pointer: Value =
            serde_json::from_slice(&std::fs::read(store_root.join("CURRENT")).unwrap()).unwrap();
        store_root
            .join("generations")
            .join(pointer["id"].as_str().unwrap())
    } else {
        store_root
    };
    let manifest_bytes = std::fs::metadata(data_dir.join("manifest.json"))
        .unwrap()
        .len();
    let store =
        graph_search_engine::GrafeoStore::open(index.store_dir(), &Default::default()).unwrap();
    let manifest_header = store.manifest_header().unwrap().unwrap();
    assert!(
        manifest_header
            .entries
            .values()
            .all(|entry| entry.extraction.is_none())
    );
    assert_eq!(store.manifest().unwrap().unwrap().header(), manifest_header);
    let manifest_header_bytes = serde_json::to_vec(&manifest_header).unwrap().len();
    let manifest_read_ms = median(
        || {
            black_box(store.manifest().unwrap());
        },
        5,
    );
    let manifest_header_ms = median(
        || {
            black_box(store.manifest_header().unwrap());
        },
        5,
    );
    let occurrence_records: usize = store
        .snapshot()
        .unwrap()
        .occurrence_files()
        .values()
        .map(|file| file.records.len())
        .sum();
    let mut occurrence_names = BTreeMap::<String, usize>::new();
    for file in store.snapshot().unwrap().occurrence_files().values() {
        for record in &file.records {
            *occurrence_names
                .entry(record.raw_name.as_ref().unwrap_or(&record.name).clone())
                .or_default() += 1;
        }
    }
    let mut occurrence_names: Vec<_> = occurrence_names.into_iter().collect();
    occurrence_names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    occurrence_names.truncate(3);
    drop(store);
    let mut occurrence_queries = Vec::new();
    for (name, total) in occurrence_names {
        let query = graph_search_types::occurrence::OccurrenceQuery {
            target: name.clone(),
            by: graph_search_types::occurrence::OccurrenceBy::Name,
            ..Default::default()
        };
        let elapsed = median(
            || {
                black_box(index.search().occurrences(&query).unwrap());
            },
            5,
        );
        let result = index.search().occurrences(&query).unwrap();
        occurrence_queries.push(json!({"name":name,"resident_ms":elapsed,
            "indexed_matches":total,
            "returned":result.items.len(),"examined":result.stats.occurrences_examined,
            "serialized_bytes":serde_json::to_vec(&result).unwrap().len(),"truncations":result.truncations}));
    }
    let exact = graph_search_types::query::SymbolQuery::new("LexicalIndex");
    let symbol_ms = median(
        || {
            black_box(index.search().symbol(&exact).unwrap());
        },
        5,
    );
    let mut rows = Vec::new();
    for text in [
        "LexicalIndex",
        "how does reconcile classify a modified file",
        "cache invalidation",
    ] {
        let query = ExploreQuery::new(text);
        let elapsed = median(
            || {
                black_box(index.search().explore(&query).unwrap());
            },
            5,
        );
        let result = index.search().explore(&query).unwrap();
        rows.push(json!({"query":text,"resident_ms":elapsed,"core_stats":result.stats,
            "items":result.items.iter().map(|i|json!({"path":i.node.path,"name":i.node.name})).collect::<Vec<_>>()}));
    }
    json!({"root":root,"open_empty_ms":open_ms,"reindex_ms":build_ms,"counts":status.counts,
        "manifest_bytes":manifest_bytes,
        "manifest_header_bytes":manifest_header_bytes,
        "manifest_read_ms":manifest_read_ms,"manifest_header_ms":manifest_header_ms,
        "occurrence_bytes":std::fs::metadata(data_dir.join("occurrences.json")).unwrap().len(),
        "occurrence_records":occurrence_records,"occurrence_queries":occurrence_queries,
        "source_units_bytes":std::fs::metadata(data_dir.join("source-units.json")).unwrap().len(),
        "graph_bytes":std::fs::metadata(data_dir.join("graph.grafeo")).unwrap().len(),
        "source_coverage":status.coverage,"symbol_resident_ms":symbol_ms,"explore":rows})
}
// Frozen exploratory representation ablation. It is not the production query
// pipeline: 80-line windows, current BM25, max 200 candidates/channel, RRF 60,
// best occurrence rank per file, no parameter tuning against task outcomes.
fn candidate_probe(root: &std::path::Path, task_path: &std::path::Path) -> Value {
    let policy = WalkPolicy::default();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy)
        .reindex(root, &mut store)
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let nodes: Vec<_> = snapshot
        .all_nodes()
        .unwrap()
        .into_iter()
        .filter(|n| !n.is_file())
        .collect();
    let metadata = Postings::new(&nodes);
    let mut windows = Vec::new();
    for entry in graph_search_core::walk::walk(root, &policy).unwrap() {
        let Ok(text) = std::fs::read_to_string(&entry.path) else {
            continue;
        };
        if text.as_bytes().contains(&0) {
            continue;
        }
        let lines: Vec<_> = text.lines().collect();
        for (i, chunk) in lines.chunks(80).enumerate() {
            let mut node = file(&entry.rel);
            node.signature = Some(chunk.join("\n"));
            node.span = Some(graph_search_types::node::Span {
                start_line: (i * 80 + 1) as u32,
                end_line: ((i * 80) + chunk.len()) as u32,
                ..Default::default()
            });
            windows.push(node);
        }
    }
    let body = Postings::new(&windows);
    let tasks: Vec<Value> = serde_json::from_slice(&std::fs::read(task_path).unwrap()).unwrap();
    let mut rows = Vec::new();
    for task in tasks {
        let terms = query_terms(task["prompt"].as_str().unwrap());
        let rank = |index: &Postings, docs: &[Node]| {
            let mut scores = index.scores(&terms);
            scores.sort_by(|a, b| {
                b.1.total_cmp(&a.1)
                    .then(docs[a.0].path.cmp(&docs[b.0].path))
                    .then(a.0.cmp(&b.0))
            });
            let mut files = BTreeMap::new();
            for (r, (i, _)) in scores.iter().take(200).enumerate() {
                files.entry(docs[*i].path.clone()).or_insert(r + 1);
            }
            files
        };
        let meta_rank = rank(&metadata, &nodes);
        let body_rank = rank(&body, &windows);
        let mut fused = BTreeMap::new();
        for channel in [&meta_rank, &body_rank] {
            for (path, rank) in channel {
                *fused.entry(path.clone()).or_insert(0.0) += 1.0 / (60.0 + *rank as f64);
            }
        }
        let mut fused: Vec<_> = fused.into_iter().collect();
        fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let sorted = |ranks: BTreeMap<String, usize>| {
            let mut r: Vec<_> = ranks.into_iter().collect();
            r.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            r.into_iter().take(50).map(|(p, _)| p).collect::<Vec<_>>()
        };
        rows.push(json!({"task_id":task["id"],"metadata_files":sorted(meta_rank),"body_files":sorted(body_rank),
            "fused_files":fused.into_iter().take(50).map(|(p,_)|p).collect::<Vec<_>>()}));
    }
    json!({"metadata_documents":nodes.len(),"body_windows":windows.len(),"configuration":{
        "window_lines":80,"channel_candidate_cap":200,"rrf_constant":60,"dedupe":"best original occurrence rank per file",
        "score":"current weighted BM25; body is signature field weight 1; paths weight 2; no learned or added dependency"},"queries":rows})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--candidates") {
        println!(
            "{}",
            serde_json::to_string_pretty(&candidate_probe(
                std::path::Path::new(&args[2]),
                std::path::Path::new(&args[3])
            ))
            .unwrap()
        );
        return;
    }
    if let Some(root) = std::env::args().nth(1) {
        println!(
            "{}",
            serde_json::to_string_pretty(&repository_probe(std::path::Path::new(&root))).unwrap()
        );
        return;
    }
    println!("{}",serde_json::to_string_pretty(&json!({"text":text_probe(),"lexical":lexical_probe(),
        "body":body_probe(),"graph":graph_probe(),"semantics":semantic_probe(),
        "stale":stale_probe(),"metadata_freshness":metadata_freshness_probe(),"atomicity":atomicity_probe(),
        "score_bounds":bound_probe(),"conjunction":conjunction_probe(),"timing":"release; one warmup + median of five; synthetic in-process microbenchmarks"})).unwrap());
}
