//! Compare native root enumeration over disposable, completely admitted fixtures.
use graph_search::{Index, OpenOptions};
use graph_search_core::{ports::GraphStore, typescript::inherit, typescript_roots::enumerate};
use graph_search_engine::{GrafeoStore, StoreOptions};
use serde_json::json;
use std::{collections::BTreeSet, io::Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("root required")?;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let configs: Vec<String> = serde_json::from_str(&input)?;
    let tmp = tempfile::tempdir()?;
    let index = Index::open(OpenOptions {
        root: root.into(),
        store: Some(tmp.path().into()),
        ..Default::default()
    })?;
    index.reindex()?;
    drop(index);
    let store = GrafeoStore::open(tmp.path(), &StoreOptions::default())?;
    let snapshot = store.snapshot()?;
    let inventory: BTreeSet<_> = snapshot.source_files().keys().cloned().collect();
    let mut output = Vec::new();
    for path in configs {
        let result = inherit(&path, snapshot.source_files())
            .and_then(|config| enumerate(&path, &config, &inventory));
        output.push(match result {
            Ok(roots) => json!({"config":path,"roots":roots}),
            Err(reason) => json!({"config":path,"error":reason}),
        });
    }
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
