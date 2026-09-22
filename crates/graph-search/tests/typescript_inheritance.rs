//! Inheritance consumes published source facts, including changes to parent files.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_core::{ports::GraphStore, typescript::inherit};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::source::SourceFileUnits;
use std::{collections::BTreeMap, path::Path};

fn facts(store: &Path) -> BTreeMap<String, SourceFileUnits> {
    GrafeoStore::open(store, &StoreOptions::default())
        .unwrap()
        .snapshot()
        .unwrap()
        .source_files()
        .unwrap()
        .clone()
}

fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..OpenOptions::default()
    })
    .unwrap()
}

#[test]
fn inherited_options_track_published_parent_edits_deletion_and_recreation() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("configs")).unwrap();
    std::fs::write(
        root.path().join("tsconfig.json"),
        r#"{"extends":"./configs/base.json","compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    let parent = root.path().join("configs/base.json");
    std::fs::write(&parent, r#"{"compilerOptions":{"baseUrl":"./first"}}"#).unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    let original = inherit("tsconfig.json", &facts(store.path())).unwrap();
    for text in [
        Some(
            r#"{/*changed*/"compilerOptions":{"baseUrl":"./second","paths":{"z*":["a*"],"a*":["b*"]}}}"#,
        ),
        Some("/* unavailable"),
        None,
        Some(r#"{"compilerOptions":{"baseUrl":"./restored"}}"#),
    ] {
        if let Some(text) = text {
            std::fs::write(&parent, text).unwrap();
        } else {
            std::fs::remove_file(&parent).unwrap();
        }
        index.sync().unwrap();
        let updated = facts(store.path());
        let result = inherit("tsconfig.json", &updated);
        let clean = tempfile::tempdir().unwrap();
        open(root.path(), clean.path()).reindex().unwrap();
        assert_eq!(result, inherit("tsconfig.json", &facts(clean.path())));
        match text {
            None => assert_eq!(result.unwrap_err(), "ts_config_missing"),
            Some("/* unavailable") => {
                assert_eq!(result.unwrap_err(), "ts_config_facts_unavailable");
            }
            Some(_) => {
                let current = result.unwrap();
                assert_eq!(current.option_origins["baseUrl"], "configs/base.json");
                assert_eq!(current.option_origins["strict"], "tsconfig.json");
                assert_ne!(
                    current.dependencies["configs/base.json"],
                    original.dependencies["configs/base.json"]
                );
                assert_eq!(
                    current.dependencies["configs/base.json"],
                    updated["configs/base.json"].source_hash
                );
            }
        }
        assert_eq!(
            original.configuration.fields["compilerOptions"]["baseUrl"],
            "./first"
        );
    }
}
