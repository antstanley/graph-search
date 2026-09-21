//! Paired resident metadata-maintenance measurements, excluding graph and disk work.
use graph_search_core::{
    lexical::query_terms,
    metadata::{CompiledFilters, MetadataIndex},
    work::{WorkBudget, WorkLimits},
};
use graph_search_types::{AnalysisMode, FieldNormalization, Node, NodeId, NodeKind, Span};
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

fn symbol(i: usize) -> Node {
    let path = format!("src/file{:05}.rs", i / 32);
    let name = format!("cacheConnection{i}");
    Node {
        id: NodeId::symbol(&path, NodeKind::Function, &name, None),
        path,
        kind: NodeKind::Function,
        name: Some(name.clone()),
        qualified_name: Some(format!("Client::{name}")),
        signature: Some(format!(
            "fn {name}(config: CacheConfig) -> Result<Connection>"
        )),
        span: Some(Span::new(
            (i % 32) as u32 * 10 + 1,
            (i % 32) as u32 * 10 + 9,
            0,
            1,
        )),
        ..Node::default()
    }
}

fn ranked(index: &MetadataIndex) -> Value {
    let mut rows = Vec::new();
    for analysis in [AnalysisMode::Split, AnalysisMode::Identifiers] {
        for normalization in [FieldNormalization::Combined, FieldNormalization::Bm25f] {
            for query in [
                "cache connection",
                "replacement",
                "Client cacheConnection0",
                "absent",
            ] {
                let mut work = WorkBudget::new(WorkLimits {
                    candidates: 100_000,
                    postings: 1_000_000,
                    ..WorkLimits::default()
                });
                let result = index
                    .search_top_with_policy(
                        &query_terms(query),
                        "absent-exact",
                        &CompiledFilters::default(),
                        &mut work,
                        50,
                        1,
                        analysis,
                        normalization,
                    )
                    .unwrap();
                assert!(work.report().2.is_empty());
                rows.push(
                    json!({"matched":result.matched,"hits":result.hits.into_iter()
                    .map(|hit| (hit.item.id, hit.score.to_bits())).collect::<Vec<_>>()}),
                );
            }
        }
    }
    json!(rows)
}

fn timed(build: impl FnOnce() -> MetadataIndex) -> (MetadataIndex, f64) {
    let start = Instant::now();
    let value = black_box(build());
    (value, start.elapsed().as_secs_f64() * 1000.0)
}

fn main() {
    let mut rows = Vec::new();
    for count in [1_000, 50_000] {
        let nodes: Vec<_> = (0..count).map(symbol).collect();
        let old = MetadataIndex::new(nodes.clone());
        let original = ranked(&old);
        for mutation in [
            "body_span",
            "signature",
            "append",
            "insert_early",
            "delete_middle",
            "half_signatures",
        ] {
            let mut changed = nodes.clone();
            match mutation {
                "body_span" => changed[count / 2].span.as_mut().unwrap().end_line += 1,
                "signature" => {
                    let node = &mut changed[count / 2];
                    node.signature = Some(format!(
                        "fn {}(value: ReplacementUnicodeÉlève)",
                        node.name.as_deref().unwrap()
                    ));
                }
                "append" => changed.push(symbol(count)),
                "insert_early" => {
                    let mut node = symbol(count);
                    node.path = "000-first.rs".into();
                    node.id = NodeId::symbol(
                        &node.path,
                        NodeKind::Function,
                        node.name.as_deref().unwrap(),
                        None,
                    );
                    changed.push(node);
                }
                "delete_middle" => {
                    changed.remove(count / 2);
                }
                "half_signatures" => {
                    for node in changed.iter_mut().step_by(2) {
                        node.signature = Some(format!(
                            "fn {}(value: ReplacementConfig)",
                            node.name.as_deref().unwrap()
                        ));
                    }
                }
                _ => unreachable!(),
            }
            // Warm both paths before five alternating-order measured pairs.
            assert_eq!(
                ranked(&old.updated(changed.clone())),
                ranked(&MetadataIndex::new(changed.clone()))
            );
            let mut pairs = Vec::new();
            for pair in 0..5 {
                let ((full, full_ms), (delta, delta_ms)) = if pair % 2 == 0 {
                    (
                        timed(|| MetadataIndex::new(changed.clone())),
                        timed(|| old.updated(changed.clone())),
                    )
                } else {
                    let delta = timed(|| old.updated(changed.clone()));
                    let full = timed(|| MetadataIndex::new(changed.clone()));
                    (full, delta)
                };
                assert_eq!(ranked(&full), ranked(&delta));
                pairs.push(json!({"full_ms":full_ms,"delta_ms":delta_ms,"delta_minus_full_ms":delta_ms-full_ms}));
            }
            assert_eq!(ranked(&old), original);
            rows.push(json!({"symbols":count,"mutation":mutation,"pairs":pairs}));
        }
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "scope":"resident metadata construction only, input clone included; old generation retained in both arms",
        "repeats":5,"alternating_order":true,"score_and_order_equal":true,
        "previous_generation_unchanged":true,"rows":rows
    })).unwrap());
}
