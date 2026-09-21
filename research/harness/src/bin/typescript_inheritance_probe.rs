//! Exercise native inheritance over configuration facts in a disposable index.
use graph_search::{Index, OpenOptions};
use graph_search_core::{ports::GraphStore, typescript::inherit};
use graph_search_engine::{GrafeoStore, StoreOptions};
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("source root required")?;
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
    let mut output = BTreeMap::new();
    for path in args {
        let result = match inherit(&path, snapshot.source_files()) {
            Ok(config) => serde_json::json!({
                "configuration": config.configuration,
                "field_origins": config.field_origins,
                "option_origins": config.option_origins,
                "dependencies": config.dependencies,
            }),
            Err(reason) => serde_json::json!({"error": reason}),
        };
        output.insert(path, result);
    }
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
