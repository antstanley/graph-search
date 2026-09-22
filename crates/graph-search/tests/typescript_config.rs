//! Raw native configuration facts retain source identity across publication.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_core::ports::GraphStore;
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::source::SourceFileUnits;
use std::{collections::BTreeMap, path::Path};

fn facts(store: &Path) -> BTreeMap<String, SourceFileUnits> {
    let store = GrafeoStore::open(store, &StoreOptions::default()).unwrap();
    store.snapshot().unwrap().source_files().unwrap().clone()
}
fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..Default::default()
    })
    .unwrap()
}
#[test]
fn configuration_facts_survive_reopen_and_incremental_changes_match_clean_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let config = "\u{feff}{/* note */\"extends\":[\"./base.json\",\"./last.json\",],\"compilerOptions\":{\"paths\":{\"$lib/*\":[\"./src/*\"]}},\"files\":[],}";
    std::fs::write(root.path().join("tsconfig.json"), config).unwrap();
    std::fs::write(
        root.path().join("base.json"),
        r#"{"compilerOptions":{"baseUrl":"../original"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("main.ts"),
        "export function present() {}\n",
    )
    .unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    let initial = facts(store.path());
    let source = &initial["tsconfig.json"];
    assert_eq!(
        source.source_hash,
        graph_search_core::hash::content_hash(config.as_bytes())
    );
    assert_eq!(
        source.version,
        graph_search_types::limits::SOURCE_INDEX_VERSION
    );
    let raw = source.typescript_config.as_ref().unwrap();
    assert_eq!(
        raw.fields["extends"],
        serde_json::json!(["./base.json", "./last.json"])
    );
    assert_eq!(raw.fields["files"], serde_json::json!([]));
    assert!(!raw.fields.contains_key("include"));
    assert!(initial["main.ts"].typescript_config.is_none());
    assert!(initial["base.json"].typescript_config.is_some());
    for text in [
        r#"{"extends":"./base.json","compilerOptions":{"paths":{}}}"#,
        "/* invalid",
        r#"{"compilerOptions":{"paths":{"$lib/*":["./other/*"]}}}"#,
    ] {
        std::fs::write(root.path().join("tsconfig.json"), text).unwrap();
        index.sync().unwrap();
        let updated = facts(store.path());
        let raw = updated["tsconfig.json"].typescript_config.as_ref().unwrap();
        assert_eq!(raw.unavailable_reason.is_some(), text == "/* invalid");
        let clean = tempfile::tempdir().unwrap();
        open(root.path(), clean.path()).reindex().unwrap();
        assert_eq!(updated, facts(clean.path()));
    }
    std::fs::remove_file(root.path().join("base.json")).unwrap();
    index.sync().unwrap();
    assert!(!facts(store.path()).contains_key("base.json"));
    let generation = GrafeoStore::open(store.path(), &StoreOptions::default())
        .unwrap()
        .generation()
        .unwrap();
    index.sync().unwrap();
    assert_eq!(
        GrafeoStore::open(store.path(), &StoreOptions::default())
            .unwrap()
            .generation()
            .unwrap(),
        generation
    );
    assert_eq!(source.typescript_config.as_ref().unwrap(), raw);
}

fn valid(file: &graph_search_types::Node, source: &SourceFileUnits) -> bool {
    graph_search_core::units::validate(file, source, |_| None).is_ok()
}

#[test]
fn persisted_configuration_validation_rejects_invalid_bounds_paths_and_old_versions() {
    let text = "{}";
    let hash = graph_search_core::hash::content_hash(text.as_bytes());
    let file = graph_search_types::Node::file(
        "base.json",
        graph_search_types::Language::Unknown,
        2,
        1,
        &hash,
        graph_search_types::PARSER_VERSION,
    );
    let mut source = graph_search_core::units::extract(
        "base.json",
        text,
        &hash,
        graph_search_types::Language::Unknown,
        &[],
    );
    source.typescript_config = Some(graph_search_types::typescript::TypeScriptConfig::default());
    assert!(valid(&file, &source));
    source.version = 13;
    assert!(!valid(&file, &source));
    source.version = graph_search_types::limits::SOURCE_INDEX_VERSION;
    let mut wrong_file = file.clone();
    wrong_file.path = "base.txt".into();
    assert!(!valid(&wrong_file, &source));
    source
        .typescript_config
        .as_mut()
        .unwrap()
        .fields
        .insert("files".into(), serde_json::json!(["x".repeat(4097)]));
    assert!(!valid(&file, &source));
    source.typescript_config = None;
    source.version = 13;
    let legacy = serde_json::to_value(&source).unwrap();
    assert!(legacy.get("typescript_config").is_none());
    assert!(
        serde_json::from_value::<SourceFileUnits>(legacy)
            .unwrap()
            .typescript_config
            .is_none()
    );
    assert!(valid(&file, &source));
}

#[test]
fn declared_configuration_dependencies_do_not_bypass_source_exclusions() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("hidden")).unwrap();
    std::fs::write(
        root.path().join("hidden/base.json"),
        r#"{"compilerOptions":{"paths":{"$lib/*":["../src/*"]}}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("tsconfig.json"),
        r#"{"extends":"./hidden/base.json"}"#,
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        store: Some(store.path().into()),
        excludes: vec!["hidden".into()],
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let sources = facts(store.path());
    assert!(!sources.contains_key("hidden/base.json"));
    assert_eq!(
        sources["tsconfig.json"]
            .typescript_config
            .as_ref()
            .unwrap()
            .fields["extends"],
        "./hidden/base.json"
    );
}
