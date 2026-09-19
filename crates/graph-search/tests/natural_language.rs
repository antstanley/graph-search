//! Bounded body retrieval, cached freshness, and exact-name compatibility.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_core::{
    config::WalkPolicy,
    lexical::BODY_TERMS_ATTRIBUTE,
    memory::MemoryStore,
    ports::{GraphStore, ListRegistry},
    reconcile::Projector,
};
use graph_search_types::{ExploreQuery, NodeKind};

fn open(root: &std::path::Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        ..OpenOptions::default()
    })
    .unwrap()
}

#[test]
fn body_words_retrieve_functions_without_metadata_matches_and_stay_fresh() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker.rs");
    std::fs::write(
        &path,
        "fn execute() {\n    let _ = \"cobalt satellite\";\n}\nfn other() {}\n",
    )
    .unwrap();
    {
        let index = open(root.path());
        index.reindex().unwrap();
        let result = index
            .search()
            .explore(&ExploreQuery::new("cobalt satellite"))
            .unwrap();
        assert_eq!(result.items[0].node.name, "execute");
        assert_eq!(result.items[0].node.kind, NodeKind::Function);
        assert!(
            result
                .approximation
                .unwrap()
                .note
                .contains("64 lines / 4096 characters")
        );
    }
    let index = open(root.path());
    assert_eq!(
        index
            .search()
            .explore(&ExploreQuery::new("cobalt satellite"))
            .unwrap()
            .items[0]
            .node
            .name,
        "execute"
    );
    std::fs::write(
        &path,
        "fn execute() {\n    let _ = \"violet comet replacement\";\n}\nfn other() {}\n",
    )
    .unwrap();
    index.sync().unwrap();
    assert!(
        index
            .search()
            .explore(&ExploreQuery::new("cobalt satellite"))
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        index
            .search()
            .explore(&ExploreQuery::new("violet comet replacement"))
            .unwrap()
            .items[0]
            .node
            .name,
        "execute"
    );
    let mut query = ExploreQuery::new("violet comet replacement");
    query.filters.path_glob = Some("excluded.rs".into());
    assert!(index.search().explore(&query).unwrap().items.is_empty());
}

#[test]
fn cached_terms_respect_bytes_lines_characters_and_do_not_store_raw_bodies() {
    let root = tempfile::tempdir().unwrap();
    let text = format!(
        "fn first() {{ let _ = \"insidecobalt\"; }} fn second() {{ let _ = \"outsidequartz\"; }}\nfn long_lines() {{\n{}let _ = \"beyondlinecap\";\n}}\nfn long_unicode() {{\nlet _ = \"{}beyondcharcap\";\n}}",
        "// padding\n".repeat(70),
        "é".repeat(5000)
    );
    std::fs::write(root.path().join("a.rs"), &text).unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let mut store = MemoryStore::new();
    projector.reindex(root.path(), &mut store).unwrap();
    let nodes = store.snapshot().unwrap().all_nodes().unwrap();
    let bag = |name: &str| -> serde_json::Value {
        serde_json::from_str(
            nodes
                .iter()
                .find(|node| node.name.as_deref() == Some(name))
                .unwrap()
                .attribute(BODY_TERMS_ATTRIBUTE)
                .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(bag("first")["terms"]["insidecobalt"], 1);
    assert!(bag("first")["terms"].get("outsidequartz").is_none());
    assert_eq!(bag("long_lines")["truncated"], true);
    assert!(bag("long_lines")["terms"].get("beyondlinecap").is_none());
    assert_eq!(bag("long_unicode")["truncated"], true);
    assert!(!bag("long_unicode").to_string().contains("beyondcharcap"));
    assert!(!bag("first").to_string().contains("let _ ="));
    let manifest = store.manifest().unwrap().unwrap();
    assert!(
        manifest.entries["a.rs"]
            .extraction
            .as_ref()
            .unwrap()
            .symbols[0]
            .attributes
            .contains_key(BODY_TERMS_ATTRIBUTE)
    );
}

#[test]
fn exact_and_split_identifiers_outrank_calls_in_other_bodies() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn callers() {\n load_pds_secret(); load_pds_secret(); load_pds_secret();\n}\nfn load_pds_secret() {}\nfn load() {}\nfn pds() {}\nfn secret() {}\nfn is() {}\n").unwrap();
    let index = open(root.path());
    index.reindex().unwrap();
    for (query, name) in [
        ("load_pds_secret", "load_pds_secret"),
        ("load pds secret", "load_pds_secret"),
        ("is", "is"),
    ] {
        let result = index.search().explore(&ExploreQuery::new(query)).unwrap();
        assert_eq!(result.items[0].node.name, name);
    }
}

#[test]
fn old_parser_cache_rebuilds_body_terms_on_unchanged_source() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn run() {\nlet _ = \"cobalt satellite\";\n}",
    )
    .unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let mut store = MemoryStore::new();
    projector.reindex(root.path(), &mut store).unwrap();
    let mut old = store.manifest().unwrap().unwrap();
    old.parser_version = 2;
    for symbol in &mut old
        .entries
        .get_mut("a.rs")
        .unwrap()
        .extraction
        .as_mut()
        .unwrap()
        .symbols
    {
        symbol.attributes.remove(BODY_TERMS_ATTRIBUTE);
    }
    assert_eq!(
        graph_search_core::stale::check(root.path(), &old, &policy)
            .unwrap()
            .changed,
        1
    );
    store.commit_manifest(old).unwrap();
    assert!(
        projector
            .sync(root.path(), &mut store)
            .unwrap()
            .reindexed_all
    );
    let current = store.manifest().unwrap().unwrap();
    assert_eq!(current.parser_version, 3);
    assert!(
        current.entries["a.rs"].extraction.as_ref().unwrap().symbols[0]
            .attributes
            .contains_key(BODY_TERMS_ATTRIBUTE)
    );
}

#[test]
fn rust_and_js_spans_are_zero_based_bytes_even_at_unicode_eof() {
    use graph_search_core::ports::SourceFile;
    for (path, source, expected) in [
        (
            "a.rs",
            "// é\nfn work() { let _ = \"λ\"; }",
            "fn work() { let _ = \"λ\"; }",
        ),
        (
            "a.ts",
            "// é\nfunction work() { return \"λ\"; }",
            "function work() { return \"λ\"; }",
        ),
        (
            "a.js",
            "// é\nfunction work() { return \"λ\"; }",
            "function work() { return \"λ\"; }",
        ),
    ] {
        let extractors = graph_search_langs::all_extractors();
        let extractor = extractors
            .iter()
            .find(|ex| ex.supports(std::path::Path::new(path)))
            .unwrap();
        let extracted = extractor
            .extract(&SourceFile {
                path: std::path::Path::new(path),
                text: source,
            })
            .unwrap();
        let symbol = extracted.symbols.iter().find(|s| s.name == "work").unwrap();
        assert_eq!(symbol.span.start_line, 2);
        assert_eq!(
            source.get(symbol.span.start_byte as usize..symbol.span.end_byte as usize),
            Some(expected),
            "{path}"
        );
        assert_eq!(symbol.span.end_byte as usize, source.len());
    }
}
