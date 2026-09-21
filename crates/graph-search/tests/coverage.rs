//! Partial enumeration must be visible to readers and cannot authorize removals.

#![allow(clippy::expect_used, clippy::panic)]

use graph_search::core::config::WalkPolicy;
use graph_search::core::memory::MemoryStore;
use graph_search::core::ports::{GraphStore, ListRegistry};
use graph_search::core::reconcile::Projector;

#[test]
fn incomplete_sync_and_reindex_preserve_the_whole_prior_projection() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn a() {}").unwrap();
    std::fs::write(root.path().join("z.rs"), "fn z() {}").unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy)
        .reindex(root.path(), &mut store)
        .unwrap();
    let before = store.snapshot().unwrap().all_nodes().unwrap();
    let manifest = store.manifest().unwrap();
    let limited = WalkPolicy {
        max_files: 1,
        ..policy
    };
    let projector = Projector::new(&registry, &limited);
    assert!(projector.sync(root.path(), &mut store).is_err());
    assert!(projector.reindex(root.path(), &mut store).is_err());
    assert_eq!(store.snapshot().unwrap().all_nodes().unwrap(), before);
    assert_eq!(store.manifest().unwrap(), manifest);
    let result = graph_search::core::files_search::search_files(
        root.path(),
        &graph_search_types::FilesQuery::new("*.rs"),
        &limited,
    )
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.context.coverage.enumeration_complete, Some(false));
    assert!(!result.truncations.is_empty());
}

#[test]
fn source_read_exclusions_and_disabled_extraction_are_reported() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn ignored() {}").unwrap();
    std::fs::write(root.path().join("binary.dat"), [0, 1, 2]).unwrap();
    std::fs::write(root.path().join("encoding.txt"), [255, 254]).unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let mut store = MemoryStore::new();
    let policy = WalkPolicy {
        languages: vec![],
        ..WalkPolicy::default()
    };
    let report = Projector::new(&registry, &policy)
        .reindex(root.path(), &mut store)
        .unwrap();
    assert_eq!(report.coverage.disabled_language_files, 1);
    assert_eq!(report.coverage.quarantined_files, 2);
    assert!(
        store
            .snapshot()
            .unwrap()
            .all_nodes()
            .unwrap()
            .iter()
            .all(graph_search_types::Node::is_file)
    );
    let text = graph_search::core::text_search::search_text(
        root.path(),
        &graph_search_types::TextQuery::new("missing"),
        &policy,
    )
    .unwrap();
    assert_eq!(text.context.coverage.binary_files, 1);
    assert_eq!(text.context.coverage.invalid_utf8_files, 1);
}

#[test]
fn changing_extraction_policy_invalidates_cached_facts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn original() {}").unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let enabled = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &enabled)
        .reindex(root.path(), &mut store)
        .unwrap();
    assert_eq!(store.snapshot().unwrap().all_nodes().unwrap().len(), 2);
    let disabled = WalkPolicy {
        languages: vec![],
        ..enabled.clone()
    };
    Projector::new(&registry, &disabled)
        .sync(root.path(), &mut store)
        .unwrap();
    assert_eq!(store.snapshot().unwrap().all_nodes().unwrap().len(), 1);
    Projector::new(&registry, &enabled)
        .sync(root.path(), &mut store)
        .unwrap();
    assert_eq!(store.snapshot().unwrap().all_nodes().unwrap().len(), 2);
}

#[test]
fn a_file_disappearing_during_extraction_does_not_become_empty_source() {
    use graph_search::core::extraction::Extraction;
    use graph_search::core::ports::{LanguageExtractor, ParseError, SourceFile};
    struct RemovingExtractor {
        victim: std::path::PathBuf,
    }
    impl LanguageExtractor for RemovingExtractor {
        fn language(&self) -> graph_search_types::Language {
            graph_search_types::Language::Rust
        }
        fn supports(&self, path: &std::path::Path) -> bool {
            path.extension().is_some_and(|ext| ext == "rs")
        }
        fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
            if file.path == std::path::Path::new("a.rs") {
                std::fs::remove_file(&self.victim).expect("remove later source");
            }
            graph_search_langs::RustExtractor.extract(file)
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn a() {}").unwrap();
    std::fs::write(root.path().join("z.rs"), "fn z() {}").unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy)
        .reindex(root.path(), &mut store)
        .unwrap();
    let old_nodes = store.snapshot().unwrap().all_nodes().unwrap();
    let old_manifest = store.manifest().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn changed_a() {}").unwrap();
    std::fs::write(root.path().join("z.rs"), "fn changed_z() {}").unwrap();
    let registry = ListRegistry::new(vec![Box::new(RemovingExtractor {
        victim: root.path().join("z.rs"),
    })]);
    assert!(
        Projector::new(&registry, &policy)
            .sync(root.path(), &mut store)
            .is_err()
    );
    assert_eq!(store.snapshot().unwrap().all_nodes().unwrap(), old_nodes);
    assert_eq!(store.manifest().unwrap(), old_manifest);
}
