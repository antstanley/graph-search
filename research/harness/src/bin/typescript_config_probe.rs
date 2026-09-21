//! Report production-registry configuration facts from a disposable store.
use graph_search::{Index, OpenOptions};
use graph_search_core::ports::GraphStore;
use graph_search_engine::{GrafeoStore, StoreOptions};
use std::collections::BTreeMap;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("source root required")?;
    let temporary = tempfile::tempdir()?;
    let index = Index::open(OpenOptions {
        root: root.into(),
        store: Some(temporary.path().into()),
        ..OpenOptions::default()
    })?;
    index.reindex()?;
    drop(index);
    let store = GrafeoStore::open(temporary.path(), &StoreOptions::default())?;
    let snapshot = store.snapshot()?;
    let facts: BTreeMap<_, _> = snapshot
        .source_files()
        .iter()
        .filter_map(|(path, facts)| {
            facts.typescript_config.as_ref().map(|config| {
                (
                    path,
                    serde_json::json!({
                        "config":config,"source_hash":facts.source_hash,"version":facts.version
                    }),
                )
            })
        })
        .collect();
    println!("{}", serde_json::to_string(&facts)?);
    Ok(())
}
