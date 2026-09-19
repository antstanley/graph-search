//! Change a disposable copy, never the source repository; compare with reindex.
use graph_search_core::{config::WalkPolicy, ports::*, reconcile::Projector};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::Language;
use std::{
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
    let initial_parsed = parsed.lock().unwrap().len();
    parsed.lock().unwrap().clear();
    let file = root.path().join(edit);
    let mut text = std::fs::read_to_string(&file)?;
    text.push_str("\n// graph-search disposable incremental measurement\n");
    std::fs::write(file, text)?;
    let sync = p.sync(root.path(), &mut store)?;
    let sync_parsed = parsed.lock().unwrap().clone();
    let manifest_bytes = serde_json::to_vec(&store.manifest()?)?.len();
    let mut fresh = GrafeoStore::open(fresh_db.path(), &options)?;
    let rebuild = p.reindex(root.path(), &mut fresh)?;
    let equivalent = graph(&store) == graph(&fresh);
    let noop = p.sync(root.path(), &mut store)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "source":source,"edited_copy_path":edit,"walked_files":walked.len(),
            "initial_parsed":initial_parsed,"initial_ms":initial.elapsed_ms,
            "sync":sync,"sync_parsed":sync_parsed,"clean_reindex_ms":rebuild.elapsed_ms,
            "equivalent_to_clean_reindex":equivalent,"manifest_bytes":manifest_bytes,"noop":noop
        }))?
    );
    if !equivalent {
        return Err("incremental graph differs from clean reindex".into());
    }
    Ok(())
}
