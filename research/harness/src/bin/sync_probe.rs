//! Change a disposable copy, never the source repository; compare with reindex.
use graph_search_core::{config::WalkPolicy, ports::*, reconcile::Projector};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::Language;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
struct Counter {
    inner: Box<dyn LanguageExtractor>,
    paths: Arc<Mutex<Vec<String>>>,
}
impl LanguageExtractor for Counter {
    fn language(&self) -> Language {
        self.inner.language()
    }
    fn supports(&self, path: &Path) -> bool {
        self.inner.supports(path)
    }
    fn extract(
        &self,
        file: &SourceFile<'_>,
    ) -> Result<graph_search_core::extraction::Extraction, ParseError> {
        self.paths
            .lock()
            .unwrap()
            .push(file.path.display().to_string());
        self.inner.extract(file)
    }
}
fn graph(store: &dyn GraphStore) -> (Vec<String>, Vec<String>) {
    let s = store.snapshot().unwrap();
    let mut n: Vec<_> = s
        .all_nodes()
        .unwrap()
        .iter()
        .map(|n| serde_json::to_string(n).unwrap())
        .collect();
    let mut e: Vec<_> = s
        .all_edges()
        .unwrap()
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    n.sort();
    e.sort();
    e.dedup();
    (n, e)
}
// Read committed descriptor hashes, not timestamps: identical rewritten artifacts
// and semantically changed artifacts are separate measurements.
fn artifacts(root: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let pointer: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("CURRENT"))?)?;
    let dir = root
        .join("generations")
        .join(pointer["id"].as_str().ok_or("missing generation")?);
    let mut files = BTreeMap::new();
    for (name, hash) in pointer["files"].as_object().ok_or("missing artifacts")? {
        let records = if matches!(
            name.as_str(),
            "manifest.json" | "source-units.json" | "occurrences.json"
        ) {
            let value: serde_json::Value = if name == "source-units.json" {
                serde_json::to_value(graph_search_engine::sidecar::load_sources(&dir)?)?
            } else if name == "manifest.json" {
                serde_json::to_value(
                    graph_search_engine::sidecar::load_manifest(&dir)?.ok_or("missing manifest")?,
                )?
            } else {
                serde_json::from_slice(&std::fs::read(dir.join(name))?)?
            };
            let object = if name == "manifest.json" {
                &value["entries"]
            } else {
                &value
            };
            let mut records = BTreeMap::new();
            for (path, value) in object.as_object().ok_or("expected per-file object")? {
                let bytes = serde_json::to_vec(value)?;
                records.insert(
                    path.clone(),
                    serde_json::json!({
                        "bytes":bytes.len(),
                        "hash":graph_search_core::hash::content_hash(&bytes)
                    }),
                );
            }
            Some(records)
        } else {
            None
        };
        files.insert(
            name.clone(),
            serde_json::json!({
                "bytes": std::fs::metadata(dir.join(name))?.len(), "hash": hash, "records": records
            }),
        );
    }
    for directory in ["source-records", "extraction-records"] {
        let blobs = dir.join(directory);
        if blobs.exists() {
            for entry in std::fs::read_dir(blobs)? {
                let entry = entry?;
                let metadata = entry.metadata()?;
                #[cfg(unix)]
                let identity = {
                    use std::os::unix::fs::MetadataExt;
                    Some((metadata.dev(), metadata.ino()))
                };
                #[cfg(not(unix))]
                let identity: Option<(u64, u64)> = None;
                files.insert(
                    format!("{directory}/{}", entry.file_name().to_string_lossy()),
                    serde_json::json!({"bytes":metadata.len(), "identity":identity,
                    "hash":entry.file_name().to_string_lossy(), "records":null}),
                );
            }
        }
    }
    Ok(serde_json::json!({"generation":pointer["id"],"files":files}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let source = Path::new(&args[1]);
    let edit = &args[2];
    let root = tempfile::tempdir()?;
    let db = tempfile::tempdir()?;
    let fresh_db = tempfile::tempdir()?;
    let policy = WalkPolicy::default();
    let walked = graph_search_core::walk::walk(source, &policy)?;
    for entry in &walked {
        let target = root.path().join(&entry.rel);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::copy(&entry.path, target)?;
    }
    // Original default-policy walk already chose the corpus. Disable external
    // ignore rules in the disposable copy so changing its absolute location
    // cannot change that selection.
    let policy = WalkPolicy {
        respect_ignore: false,
        include_hidden: true,
        ..policy
    };
    let copied = graph_search_core::walk::walk(root.path(), &policy)?;
    assert_eq!(
        walked.iter().map(|e| &e.rel).collect::<Vec<_>>(),
        copied.iter().map(|e| &e.rel).collect::<Vec<_>>()
    );
    let parsed = Arc::new(Mutex::new(Vec::new()));
    let registry = ListRegistry::new(
        graph_search_langs::all_extractors()
            .into_iter()
            .map(|inner| {
                Box::new(Counter {
                    inner,
                    paths: Arc::clone(&parsed),
                }) as Box<dyn LanguageExtractor>
            })
            .collect(),
    );
    let p = Projector::new(&registry, &policy);
    let options = StoreOptions { in_memory: true };
    let mut store = GrafeoStore::open(db.path(), &options)?;
    let initial = p.reindex(root.path(), &mut store)?;
    let initial_artifacts = artifacts(db.path())?;
    let initial_parsed = parsed.lock().unwrap().len();
    parsed.lock().unwrap().clear();
    let file = root.path().join(edit);
    let mut text = std::fs::read_to_string(&file)?;
    text.push_str("\n// graph-search disposable incremental measurement\n");
    std::fs::write(file, text)?;
    let sync = p.sync(root.path(), &mut store)?;
    let sync_artifacts = artifacts(db.path())?;
    let sync_parsed = parsed.lock().unwrap().clone();
    let manifest_bytes = serde_json::to_vec(&store.manifest()?)?.len();
    let mut fresh = GrafeoStore::open(fresh_db.path(), &options)?;
    let rebuild = p.reindex(root.path(), &mut fresh)?;
    let equivalent = graph(&store) == graph(&fresh);
    let fresh_artifacts = artifacts(fresh_db.path())?;
    let fact_records_equivalent = ["manifest.json", "source-units.json", "occurrences.json"]
        .iter()
        .all(|name| {
            sync_artifacts["files"][name]["records"] == fresh_artifacts["files"][name]["records"]
        });
    parsed.lock().unwrap().clear();
    let noop = p.sync(root.path(), &mut store)?;
    let noop_artifacts = artifacts(db.path())?;
    let noop_parsed = parsed.lock().unwrap().clone();
    let reopen_started = std::time::Instant::now();
    let mut reopened = GrafeoStore::open(db.path(), &StoreOptions::default())?;
    let reopen_ms = reopen_started.elapsed().as_millis();
    let reopen_equivalent = graph(&reopened) == graph(&fresh)
        && reopened.manifest()?.map(|m| m.entries) == fresh.manifest()?.map(|m| m.entries);
    let reopened_noop = p.sync(root.path(), &mut reopened)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "source":source,"edited_copy_path":edit,"walked_files":walked.len(),
            "initial_parsed":initial_parsed,"initial_ms":initial.elapsed_ms,
            "sync":sync,"sync_parsed":sync_parsed,"clean_reindex_ms":rebuild.elapsed_ms,
            "equivalent_to_clean_reindex":equivalent,"manifest_bytes":manifest_bytes,"noop":noop,
            "initial_artifacts":initial_artifacts,"sync_artifacts":sync_artifacts,
            "noop_artifacts":noop_artifacts,"noop_parsed":noop_parsed,
            "reopen_equivalent_to_clean_reindex":reopen_equivalent,"reopened_noop":reopened_noop,
            "reopen_ms":reopen_ms,
            "fact_records_equivalent_to_clean_reindex":fact_records_equivalent,
            "measurement_note":"Artifact bytes are logical file sizes, not physical I/O; changed hashes do not measure syscall writes."
        }))?
    );
    if !equivalent || !reopen_equivalent || !fact_records_equivalent {
        return Err("incremental graph or facts differ from clean reindex".into());
    }
    Ok(())
}
