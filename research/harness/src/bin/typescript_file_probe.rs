//! Compose native inheritance, alias dispatch and file loading over published facts.
use graph_search::module_presence::Capture;
use graph_search::{Index, OpenOptions};
use graph_search_core::{
    ports::GraphStore,
    typescript::inherit,
    typescript_aliases::{Aliases, Dispatch},
    typescript_files::{Availability, Lookup, Mode, Options},
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("source root required")?;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let queries: Vec<Value> = serde_json::from_str(&input)?;
    let temporary = tempfile::tempdir()?;
    let index = Index::open(OpenOptions {
        root: root.clone().into(),
        store: Some(temporary.path().into()),
        ..OpenOptions::default()
    })?;
    index.reindex()?;
    drop(index);
    let store = GrafeoStore::open(temporary.path(), &StoreOptions::default())?;
    let packages = store
        .manifest()?
        .ok_or("manifest missing")?
        .package_boundaries;
    let snapshot = store.snapshot()?;
    let sources = snapshot.source_files();
    let files: BTreeSet<String> = sources.keys().cloned().collect();
    let mut results = Vec::new();
    for query in queries {
        let config_path = query["config"].as_str().ok_or("config required")?;
        let specifier = query["specifier"].as_str().ok_or("specifier required")?;
        let mode = match query["mode"].as_str() {
            Some("bundler") => Mode::Bundler,
            Some("node_esm") => Mode::NodeEsm,
            Some("node_cjs") => Mode::NodeCommonJs,
            _ => return Err("mode required".into()),
        };
        let mut probes = Vec::new();
        let result = (|| {
            let config = inherit(config_path, sources)?;
            let aliases = Aliases::compile(&config)?;
            let options = Options::compile(&config, mode)?;
            let mut capture =
                Capture::new(std::path::Path::new(&root)).map_err(|_| "ts_module_presence_root")?;
            let mut presence = |path: &str| capture.classify(path);
            let mut lookup = Lookup::new(&options, &files, &packages, &mut presence);
            let result = aliases.resolve(specifier, |path, substitution| {
                lookup.load(path, substitution)
            });
            probes = lookup.probes().iter().map(|probe| json!({
                "path":probe.path,"present":probe.availability == Availability::Admitted,"availability":probe.availability.as_str(),"package_boundary":probe.package_boundary,
            })).collect();
            capture.validate()?;
            result
        })();
        let result = match result {
            Ok(Dispatch::Unmatched) => json!({"route":"unmatched","target":null}),
            Ok(Dispatch::Paths { pattern, target }) => {
                json!({"route":"paths","pattern":pattern,"target":target})
            }
            Ok(Dispatch::BaseUrl(target)) => json!({"route":"base_url","target":target}),
            Err(reason) => json!({"error":reason}),
        };
        results.push(json!({"query":query,"result":result,"probes":probes}));
    }
    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}
