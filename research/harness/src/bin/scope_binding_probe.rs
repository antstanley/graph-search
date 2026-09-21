//! Publish/reopen native scope bindings for independent declaration-site checking.
use graph_search::{Index, OpenOptions};
use graph_search_core::ports::GraphStore;
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::EdgeKind;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("root required")?;
    let temporary = tempfile::tempdir()?;
    let index = Index::open(OpenOptions {
        root: root.into(),
        store: Some(temporary.path().into()),
        ..Default::default()
    })?;
    index.reindex()?;
    drop(index);
    let store = GrafeoStore::open(temporary.path(), &StoreOptions::default())?;
    let snapshot = store.snapshot()?;
    let mut rows = Vec::new();
    for (path, file) in snapshot.occurrence_files() {
        for occurrence in &file.records {
            if occurrence.kind != EdgeKind::Calls {
                continue;
            }
            let target = occurrence
                .target
                .as_ref()
                .map(|id| snapshot.node_by_id(id))
                .transpose()?
                .flatten();
            rows.push(json!({"path":path,"source_hash":file.source_hash,"occurrence":occurrence,"target":target}));
        }
    }
    println!("{}", serde_json::to_string(&rows)?);
    Ok(())
}
