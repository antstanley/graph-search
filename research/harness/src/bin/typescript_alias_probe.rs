//! Native alias dispatch using published facts and an exact-file research loader.
use graph_search::{Index, OpenOptions};
use graph_search_core::{
    ports::GraphStore,
    typescript::inherit,
    typescript_aliases::{Aliases, Dispatch},
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use serde_json::{Value, json};
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("source root required")?;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let queries: Vec<Value> = serde_json::from_str(&input)?;
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
    let sources = snapshot.source_files();
    let mut results = Vec::new();
    for query in queries {
        let path = query["config"].as_str().ok_or("config required")?;
        let specifier = query["specifier"].as_str().ok_or("specifier required")?;
        let mut attempts = Vec::new();
        let result = inherit(path, sources)
            .and_then(|config| Aliases::compile(&config))
            .and_then(|aliases| {
                aliases.resolve(specifier, |candidate, substitution| {
                    attempts.push(json!({"candidate":candidate,"mapped":substitution.is_some(),"substitution":substitution}));
                    Ok(sources
                        .contains_key(candidate)
                        .then(|| candidate.to_owned()))
                })
            });
        let result = match result {
            Ok(Dispatch::Unmatched) => json!({"route":"unmatched","target":null}),
            Ok(Dispatch::Paths { pattern, target }) => {
                json!({"route":"paths","pattern":pattern,"target":target})
            }
            Ok(Dispatch::BaseUrl(target)) => json!({"route":"base_url","target":target}),
            Err(reason) => json!({"error":reason}),
        };
        results.push(json!({"query":query,"result":result,"attempts":attempts}));
    }
    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}
