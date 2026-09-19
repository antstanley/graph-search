//! JSONL evaluation host: one public Index per process, explicit external store.
use graph_search::{Index, OpenOptions};
use graph_search_types::{DepsQuery, ExploreQuery, RefQuery, SymbolQuery, TraversalQuery};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: task-eval-host ROOT TEMP_STORE".into());
    }
    let start = Instant::now();
    let index = Index::open(OpenOptions {
        root: args[1].clone().into(),
        store: Some(args[2].clone().into()),
        ..OpenOptions::default()
    })?;
    let report = index.reindex()?;
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{}",
        json!({"ready":true,"setup_ms":start.elapsed().as_millis(),"index":report})
    )?;
    out.flush()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let result: Result<Value, Box<dyn std::error::Error>> = (|| {
            let req: Value = serde_json::from_str(&line)?;
            let query = req["query"].as_str().ok_or("query must be a string")?;
            let service = index.search();
            Ok(match req["mode"].as_str().unwrap_or("explore") {
                "explore" => {
                    let mut q = ExploreQuery::new(query);
                    q.k = 8;
                    q.hops = 1;
                    q.max_bytes = 16384;
                    serde_json::to_value(service.explore(&q)?)?
                }
                "symbol" => serde_json::to_value(service.symbol(&SymbolQuery::new(query))?)?,
                "callers" => {
                    serde_json::to_value(service.callers(&TraversalQuery::new(query, 1))?)?
                }
                "callees" => {
                    serde_json::to_value(service.callees(&TraversalQuery::new(query, 1))?)?
                }
                "refs" => serde_json::to_value(service.refs(&RefQuery::new(query))?)?,
                "deps" => serde_json::to_value(service.deps(&DepsQuery::new(query))?)?,
                _ => return Err("unsupported mode".into()),
            })
        })();
        let result = result.unwrap_or_else(|e| json!({"error":e.to_string()}));
        writeln!(out, "{result}")?;
        out.flush()?;
    }
    Ok(())
}
