use graph_search_core::{
    config::WalkPolicy,
    ports::{GraphStore, ListRegistry},
    query::QueryEngine,
    reconcile::Projector,
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::query::*;
use serde_json::{Value, json};
use std::{path::Path, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let requests: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&args[2])?)?;
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let temporary_store = tempfile::tempdir()?;
    let mut store = GrafeoStore::open(temporary_store.path(), &StoreOptions { in_memory: true })?;
    let report = projector.reindex(root, &mut store)?;
    let snapshot = store.snapshot()?;
    let engine = QueryEngine::new(snapshot.as_ref());
    let mut results = vec![];
    for req in requests {
        let mode = req["mode"].as_str().unwrap();
        let target = req["query"].as_str().unwrap_or("");
        let filters = GraphFilters {
            lang: req["lang"]
                .as_str()
                .and_then(graph_search_types::Language::parse),
            path_glob: req["path"].as_str().map(str::to_string),
        };
        let limit = req["limit"].as_u64().unwrap_or(50) as u32;
        let now = Instant::now();
        let result: Result<Value, graph_search_core::Error> = (|| {
            Ok(match mode {
                "explore" => {
                    let mut q = ExploreQuery::new(target);
                    q.filters = filters;
                    q.k = req["k"].as_u64().unwrap_or(8) as u32;
                    q.hops = req["hops"].as_u64().unwrap_or(1) as u8;
                    if let Some(n) = req["max_bytes"].as_u64() {
                        q.max_bytes = n as u32;
                    }
                    serde_json::to_value(engine.explore(&q, root)?).unwrap()
                }
                "symbol" => {
                    let mut q = SymbolQuery::new(target);
                    q.filters = filters;
                    q.limit = limit;
                    serde_json::to_value(engine.symbol(&q)?).unwrap()
                }
                "refs" => {
                    let mut q = RefQuery::new(target);
                    q.filters = filters;
                    q.limit = limit;
                    serde_json::to_value(engine.refs(&q)?).unwrap()
                }
                "deps" => {
                    let mut q = DepsQuery::new(target);
                    q.filters = filters;
                    q.limit = limit;
                    serde_json::to_value(engine.deps(&q)?).unwrap()
                }
                "callers" | "callees" | "impact" => {
                    let mut q =
                        TraversalQuery::new(target, req["depth"].as_u64().unwrap_or(1) as u8);
                    q.filters = filters;
                    q.limit = limit;
                    match mode {
                        "callers" => serde_json::to_value(engine.callers(&q)?).unwrap(),
                        "callees" => serde_json::to_value(engine.callees(&q)?).unwrap(),
                        _ => serde_json::to_value(engine.impact(&q)?).unwrap(),
                    }
                }
                "neighbors" => {
                    let mut q = NeighborsQuery::new(target);
                    q.filters = filters;
                    q.limit = limit;
                    serde_json::to_value(engine.neighbors(&q)?).unwrap()
                }
                "path" => serde_json::to_value(
                    engine.path(&PathQuery::new(target, req["to"].as_str().unwrap()))?,
                )
                .unwrap(),
                _ => panic!("unknown mode"),
            })
        })();
        results.push(json!({"request":req,"elapsed_us":now.elapsed().as_micros(),"result":match result {Ok(v)=>v,Err(e)=>json!({"error":e.to_string()})}}));
    }
    println!(
        "{}",
        json!({"report":report,"nodes":snapshot.all_nodes()?,"edges":snapshot.all_edges()?,"queries":results})
    );
    Ok(())
}
