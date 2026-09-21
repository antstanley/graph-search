//! Line protocol for measuring real generation churn in disposable stores.
use graph_search_core::{config::WalkPolicy, ports::*, reconcile::Projector};
use graph_search_engine::{GrafeoStore, StoreOptions};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
    time::Instant,
};

fn fingerprint(store: &dyn GraphStore) -> Result<Value, Box<dyn std::error::Error>> {
    let snapshot = store.snapshot()?;
    let mut nodes: Vec<_> = snapshot
        .all_nodes()?
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<_, _>>()?;
    let mut edges: Vec<_> = snapshot
        .all_edges()?
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<_, _>>()?;
    nodes.sort();
    edges.sort();
    let hash = |value: &Value| -> Result<String, Box<dyn std::error::Error>> {
        Ok(graph_search_core::hash::content_hash(&serde_json::to_vec(
            value,
        )?))
    };
    Ok(json!({
        "nodes":hash(&json!(nodes))?,"edges":hash(&json!(edges))?,
        "source_facts":hash(&serde_json::to_value(snapshot.source_files())?)?,
        "occurrences":hash(&serde_json::to_value(snapshot.occurrence_files())?)?,
        "extraction_entries":hash(&serde_json::to_value(store.manifest()?.ok_or("missing manifest")?.entries)?)?,
        "node_count":nodes.len(),"edge_count":edges.len()
    }))
}

fn emit(value: Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{value}");
    std::io::stdout().flush()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: generation_churn_probe writer|reader SOURCE STORE".into());
    }
    let source = Path::new(&args[2]);
    let mut store = GrafeoStore::open(Path::new(&args[3]), &StoreOptions::default())?;
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy {
        respect_ignore: false,
        ..WalkPolicy::default()
    };
    let projector = Projector::new(&registry, &policy);
    let writer = args[1] == "writer";
    if writer {
        let started = Instant::now();
        projector.reindex(source, &mut store)?;
        let build_ns = started.elapsed().as_nanos();
        emit(
            json!({"ready":true,"pid":std::process::id(),"generation":store.generation()?,
            "build_ns":build_ns,"fingerprint":fingerprint(&store)?}),
        )?;
    } else {
        if args[1] != "reader" {
            return Err("unknown role".into());
        }
        // Intentionally do not load the lazy extraction manifest before churn.
        emit(json!({"ready":true,"pid":std::process::id(),"generation":store.generation()?}))?;
    }
    for command in std::io::stdin().lock().lines() {
        match command?.as_str() {
            "sync" if writer => {
                let started = Instant::now();
                let report = projector.sync(source, &mut store)?;
                let sync_ns = started.elapsed().as_nanos();
                emit(json!({"generation":store.generation()?,"sync_ns":sync_ns,
                    "report":report,"fingerprint":fingerprint(&store)?}))?;
            }
            "rebuild-check" if writer => {
                let fresh = tempfile::tempdir()?;
                let mut rebuilt = GrafeoStore::open(fresh.path(), &StoreOptions::default())?;
                projector.reindex(source, &mut rebuilt)?;
                let expected = fingerprint(&rebuilt)?;
                if fingerprint(&store)? != expected {
                    return Err("incremental/rebuild disagreement".into());
                }
                emit(json!({"rebuild_equal":true}))?;
            }
            "check" => {
                emit(json!({"generation":store.generation()?,"fingerprint":fingerprint(&store)?}))?
            }
            "exit" => return Ok(()),
            "crash" if !writer => std::process::exit(87),
            _ => return Err("unexpected protocol command".into()),
        }
    }
    Ok(())
}
