//! Capture existing native lane scores; normalized combinations remain research-only.
use graph_search_core::{
    config::WalkPolicy,
    memory::MemoryStore,
    metadata::CompiledFilters,
    ports::{GraphStore, ListRegistry},
    reconcile::Projector,
    work::{WorkBudget, WorkLimits},
};
use graph_search_types::NodeId;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

fn budget() -> WorkBudget {
    WorkBudget::new(WorkLimits {
        candidates: 100_000,
        postings: 10_000_000,
        ..WorkLimits::default()
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let tasks: Vec<Value> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy).reindex(root, &mut store)?;
    let snapshot = store.snapshot()?;
    let filters = CompiledFilters::default();
    let mut rows = Vec::new();
    for task in tasks {
        let query = task["prompt"].as_str().ok_or("missing prompt")?;
        let terms = graph_search_core::lexical::query_terms(query);
        let exact = graph_search_core::lexical::exact_query(query);
        let mut mw = budget();
        let metadata = snapshot
            .metadata()
            .search_top(&terms, &exact, &filters, &mut mw, 50)?;
        let mut bw = budget();
        let (body, body_matched) = snapshot.body().search(
            &terms,
            &filters,
            |path| snapshot.metadata().language(path),
            &BTreeSet::new(),
            &mut bw,
            50,
        )?;
        assert!(
            mw.report().2.is_empty(),
            "metadata work cap: {}",
            task["id"]
        );
        assert!(bw.report().2.is_empty(), "body work cap: {}", task["id"]);
        let metadata: Vec<_> = metadata
            .hits
            .iter()
            .enumerate()
            .map(|(rank, hit)| {
                json!({
                    "id":hit.item.id, "path":hit.item.path, "span":hit.item.span,
                    "rank":rank+1, "score":hit.score, "exact":hit.score >= 2.0,
                })
            })
            .collect();
        let mut body_rows = Vec::new();
        for (rank, hit) in body.iter().enumerate() {
            let source = &snapshot.source_files()[&hit.path];
            let unit = &source.units[hit.unit];
            let owner = unit
                .documentation
                .as_ref()
                .and_then(|d| d.documented_symbol.as_ref())
                .or(unit.owner.as_ref());
            let node = owner
                .map(|id| snapshot.node_by_id(id))
                .transpose()?
                .flatten();
            let id = node
                .as_ref()
                .map_or_else(|| NodeId::file(&hit.path), |n| n.id.clone());
            body_rows.push(json!({"id":id,"path":hit.path,"span":unit.span,
                "rank":rank+1,"score":hit.score,"exact":false,"source_hash":source.source_hash}));
        }
        rows.push(json!({"task":task["id"],"terms":terms,"metadata":metadata,"body":body_rows,
            "body_regions_matched":body_matched,"metadata_work":mw.report(),"body_work":bw.report()}));
    }
    println!("{}", serde_json::to_string(&rows)?);
    Ok(())
}
