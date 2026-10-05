//! JSONL accuracy host: one resident public `Index` per repository, indexed into
//! an explicit external store so the repository itself is never written.
//!
//! Requests are `{"mode": ..., ...}` lines; each answer is one compact JSON
//! line carrying only the fields the scorer reads.
use graph_search::{Index, OpenOptions};
use graph_search_types::{
    DepsQuery, EdgeKind, ExploreQuery, NeighborsQuery, SymbolQuery, TraversalQuery,
};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::time::Instant;

type Failure = Box<dyn std::error::Error>;

fn node(hit: &graph_search_types::SymbolHit) -> Value {
    json!({
        "id": hit.id,
        "name": hit.name,
        "qualified_name": hit.qualified_name,
        "kind": hit.kind,
        "path": hit.path,
        "start_line": hit.start_line,
        "end_line": hit.end_line,
    })
}

fn graph(result: &graph_search_types::GraphResult) -> Value {
    json!({
        "nodes": result.nodes.iter().map(node).collect::<Vec<_>>(),
        "edges": result.edges,
        "truncations": result.truncations,
    })
}

fn limit(request: &Value) -> u32 {
    request["limit"]
        .as_u64()
        .and_then(|limit| u32::try_from(limit).ok())
        .unwrap_or(500)
}

fn answer(index: &Index, request: &Value) -> Result<Value, Failure> {
    let target = request["target"].as_str().unwrap_or_default();
    let service = index.search();
    Ok(match request["mode"].as_str().ok_or("mode must be a string")? {
        // The walked file set, so oracles judge exactly the files indexed.
        "files" => {
            let entries = graph_search::core::walk::walk(index.root(), index.policy())?;
            json!({"files": entries.iter().map(|entry| json!({
                "path": entry.rel,
                "language": entry.language,
                "size": entry.size,
            })).collect::<Vec<_>>()})
        }
        "symbol" => graph(&service.symbol(&SymbolQuery::new(target).with_limit(limit(request)))?),
        "callers" => graph(
            &service.callers(&TraversalQuery::new(target, 1).with_limit(limit(request)))?,
        ),
        "callees" => graph(
            &service.callees(&TraversalQuery::new(target, 1).with_limit(limit(request)))?,
        ),
        "neighbors" => {
            let mut query = NeighborsQuery::new(target).with_limit(limit(request));
            if let Some(rel) = request.get("rel").filter(|rel| !rel.is_null()) {
                query.rel = Some(serde_json::from_value::<EdgeKind>(rel.clone())?);
            }
            if let Some(hops) = request["hops"].as_u64() {
                query.hops = u8::try_from(hops)?;
            }
            graph(&service.neighbors(&query)?)
        }
        // Individual reference sites with their resolution class and reason.
        "occurrences" => {
            let query: graph_search_types::occurrence::OccurrenceQuery =
                serde_json::from_value(json!({
                    "target": target,
                    "by": request["by"].as_str().unwrap_or("name"),
                    "filters": {},
                    "limit": limit(request),
                }))?;
            json!({"items": service.occurrences(&query)?.items})
        }
        "deps" => graph(&service.deps(&DepsQuery::new(target).with_limit(limit(request)))?),
        "explore" => {
            let k = request["k"].as_u64().and_then(|k| u32::try_from(k).ok()).unwrap_or(8);
            let query = ExploreQuery::new(request["query"].as_str().unwrap_or_default())
                .with_k(k)
                .with_context_lines(0);
            let result = service.explore(&query)?;
            json!({
                "items": result.items.iter().map(|item| node(&item.node)).collect::<Vec<_>>(),
                "truncations": result.truncations,
            })
        }
        _ => return Err("unsupported mode".into()),
    })
}

fn main() -> Result<(), Failure> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: accuracy-host ROOT STORE".into());
    }
    let started = Instant::now();
    // Newline-separated exclude globs, for files that abort indexing.
    let excludes = std::env::var("ACCURACY_EXCLUDES")
        .map(|text| text.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    let index = Index::open(OpenOptions {
        root: args[1].clone().into(),
        store: Some(args[2].clone().into()),
        excludes,
        ..OpenOptions::default()
    })?;
    let report = index.reindex()?;
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{}",
        json!({"ready": true, "setup_ms": started.elapsed().as_millis(), "index": report})
    )?;
    out.flush()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let result = serde_json::from_str::<Value>(&line)
            .map_err(Failure::from)
            .and_then(|request| answer(&index, &request))
            .unwrap_or_else(|error| json!({"error": error.to_string()}));
        writeln!(out, "{result}")?;
        out.flush()?;
    }
    Ok(())
}
