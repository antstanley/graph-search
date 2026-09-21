//! Whole native explore latency plus isolated shared-frontier feasibility.
use graph_search_core::{GraphStore, memory::MemoryStore, query::QueryEngine};
use graph_search_types::{Edge, EdgeKind, ExploreMode, ExploreQuery, Node, NodeId, NodeKind};
use serde_json::json;
use std::{collections::BTreeSet, hint::black_box, time::Instant};

fn fixture(shape: &str, n: usize, seeds: usize, payload: usize) -> (MemoryStore, Vec<Vec<usize>>) {
    let mut incoming = vec![BTreeSet::new(); n];
    for from in seeds..n {
        match shape {
            "wide" => {
                for callers in incoming.iter_mut().take(seeds) {
                    callers.insert(from);
                }
            }
            "separate" => {
                incoming[from % seeds].insert(from);
            }
            "layered" => {
                if from < seeds + 16 {
                    for callers in incoming.iter_mut().take(seeds) {
                        callers.insert(from);
                    }
                } else {
                    incoming[seeds + from % 16].insert(from);
                }
            }
            _ => {
                incoming[from - 1].insert(from);
                incoming[from.saturating_sub(2)].insert(from);
            }
        }
    }
    let mut batch = graph_search_core::conformance::fixture_batch();
    batch.upserts.truncate(1);
    let base = batch.upserts[0].symbols[0].clone();
    let ids: Vec<_> = (0..n)
        .map(|i| NodeId::symbol("src/a.rs", NodeKind::Function, &format!("n{i:06}"), None))
        .collect();
    batch.upserts[0].symbols = ids
        .iter()
        .enumerate()
        .map(|(i, id)| Node {
            id: id.clone(),
            name: Some(if i < seeds {
                "target".into()
            } else {
                format!("n{i}")
            }),
            qualified_name: Some(format!("n{i:06}")),
            signature: Some("x".repeat(payload)),
            ..base.clone()
        })
        .collect();
    batch.upserts[0].edges.clear();
    for (to, callers) in incoming.iter().enumerate() {
        for &from in callers {
            batch.upserts[0].edges.push(Edge::resolved(
                &ids[from],
                EdgeKind::Calls,
                &ids[to],
                "call",
                Some("src/a.rs"),
                Some(1),
            ));
        }
    }
    let mut store = MemoryStore::new();
    store.apply(batch).unwrap();
    (
        store,
        incoming
            .into_iter()
            .map(|set| set.into_iter().collect())
            .collect(),
    )
}

fn independent(graph: &[Vec<usize>], seeds: usize, hops: usize) -> (Vec<usize>, usize) {
    let mut counts = vec![0; seeds];
    let mut work = 0;
    for seed in 0..seeds {
        let mut visited = vec![false; graph.len()];
        visited[seed] = true;
        let mut frontier = vec![seed];
        for _ in 0..hops {
            let mut next = Vec::new();
            for node in frontier {
                for &caller in &graph[node] {
                    work += 1;
                    if !visited[caller] {
                        visited[caller] = true;
                        counts[seed] += 1;
                        next.push(caller);
                    }
                }
            }
            frontier = next;
        }
    }
    (counts, work)
}

fn shared(graph: &[Vec<usize>], seeds: usize, hops: usize) -> (Vec<usize>, usize) {
    assert!(seeds <= 64);
    let mut seen = vec![0u64; graph.len()];
    let mut frontier = vec![0u64; graph.len()];
    for seed in 0..seeds {
        seen[seed] = 1u64 << seed;
        frontier[seed] = seen[seed];
    }
    let mut counts = vec![0; seeds];
    let mut work = 0;
    for _ in 0..hops {
        let mut next = vec![0u64; graph.len()];
        for (node, &bits) in frontier.iter().enumerate() {
            if bits == 0 {
                continue;
            }
            for &caller in &graph[node] {
                work += 1;
                let mut new = bits & !seen[caller];
                seen[caller] |= new;
                next[caller] |= new;
                while new != 0 {
                    let seed = new.trailing_zeros() as usize;
                    counts[seed] += 1;
                    new &= new - 1;
                }
            }
        }
        frontier = next;
    }
    (counts, work)
}

fn main() {
    for mask in 0u16..512 {
        let mut graph = vec![Vec::new(); 3];
        for (to, edges) in graph.iter_mut().enumerate() {
            for from in 0..3 {
                if mask & (1 << (from * 3 + to)) != 0 {
                    edges.push(from);
                }
            }
        }
        for seeds in 1..=3 {
            for hops in 0..=5 {
                assert_eq!(
                    independent(&graph, seeds, hops).0,
                    shared(&graph, seeds, hops).0
                );
            }
        }
    }
    let repeats = std::env::args()
        .nth(1)
        .map_or(10, |s| s.parse::<usize>().unwrap());
    let mut rows = Vec::new();
    for shape in ["wide", "separate", "layered", "chain"] {
        for (n, seeds, payload) in [(256, 1, 64), (256, 8, 64), (2048, 8, 1024)] {
            let (store, graph) = fixture(shape, n, seeds, payload);
            let snapshot = store.snapshot().unwrap();
            let mut query = ExploreQuery::new("target")
                .with_k(seeds as u32)
                .with_context_lines(0);
            query.retrieval.mode = ExploreMode::ExactName;
            query.max_bytes = 128_000;
            let engine = QueryEngine::new(snapshot.as_ref());
            let mut expected = engine.explore(&query, std::path::Path::new(".")).unwrap();
            expected.stats.elapsed_ms = 0;
            let encoded = serde_json::to_vec(&expected).unwrap();
            let start = Instant::now();
            for _ in 0..repeats {
                let mut result = engine.explore(&query, std::path::Path::new(".")).unwrap();
                result.stats.elapsed_ms = 0;
                assert_eq!(result, expected);
                black_box(result);
            }
            let native_ns = start.elapsed().as_nanos();
            let single = independent(&graph, seeds, 3);
            let multi = shared(&graph, seeds, 3);
            assert_eq!(single.0, multi.0);
            let start = Instant::now();
            for _ in 0..repeats {
                black_box(independent(&graph, seeds, 3));
            }
            let independent_ns = start.elapsed().as_nanos();
            let start = Instant::now();
            for _ in 0..repeats {
                black_box(shared(&graph, seeds, 3));
            }
            let shared_ns = start.elapsed().as_nanos();
            rows.push(json!({"shape":shape,"nodes":n,"seeds":seeds,"payload":payload,"repeats":repeats,"native_ns":native_ns,"result_sha256":graph_search_core::hash::content_hash(&encoded),"result_bytes":encoded.len(),"items":expected.items.len(),"independent_ns":independent_ns,"shared_ns":shared_ns,"independent_arcs":single.1,"shared_arcs":multi.1}));
        }
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
