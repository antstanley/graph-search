//! Root enumeration over reopened configuration facts; not default graph bindings.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_core::{ports::GraphStore, typescript::inherit, typescript_roots::enumerate};
use graph_search_engine::{GrafeoStore, StoreOptions};
use std::{collections::BTreeSet, path::Path};

fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..Default::default()
    })
    .unwrap()
}
fn roots(store: &Path) -> BTreeSet<String> {
    let store = GrafeoStore::open(store, &StoreOptions::default()).unwrap();
    let snapshot = store.snapshot().unwrap();
    let config = inherit("app/tsconfig.json", snapshot.source_files().unwrap()).unwrap();
    enumerate(
        "app/tsconfig.json",
        &config,
        &snapshot.source_files().unwrap().keys().cloned().collect(),
    )
    .unwrap()
}
fn parity(root: &Path, store: &Path, expected: &[&str]) {
    let clean = tempfile::tempdir().unwrap();
    open(root, clean.path()).reindex().unwrap();
    let actual = roots(store);
    assert_eq!(actual, roots(clean.path()));
    assert_eq!(actual, expected.iter().map(|s| (*s).into()).collect());
}
#[test]
fn inherited_membership_mutations_and_file_changes_match_rebuilds_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".graph-search")).unwrap();
    std::fs::write(
        root.path().join(".graph-search/config.toml"),
        "replace_defaults = true\nexcludes = []\n",
    )
    .unwrap();
    for dir in ["app/src", "src", "dist"] {
        std::fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    let base = root.path().join("base.json");
    std::fs::write(
        &base,
        r#"{"include":["src"],"compilerOptions":{"outDir":"dist"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("app/tsconfig.json"),
        r#"{"extends":"../base.json"}"#,
    )
    .unwrap();
    for name in ["src/a.ts", "app/src/b.ts", "dist/c.ts"] {
        std::fs::write(root.path().join(name), "export const value = 1;").unwrap();
    }
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    parity(root.path(), store.path(), &["src/a.ts"]);
    std::fs::write(
        &base,
        r#"{"include":["**/*"],"compilerOptions":{"outDir":"dist"}}"#,
    )
    .unwrap();
    index.sync().unwrap();
    parity(root.path(), store.path(), &["src/a.ts", "app/src/b.ts"]);
    std::fs::write(
        root.path().join("app/tsconfig.json"),
        r#"{"extends":"../base.json","exclude":[]}"#,
    )
    .unwrap();
    index.sync().unwrap();
    parity(
        root.path(),
        store.path(),
        &["src/a.ts", "app/src/b.ts", "dist/c.ts"],
    );
    std::fs::remove_file(root.path().join("src/a.ts")).unwrap();
    index.sync().unwrap();
    parity(root.path(), store.path(), &["app/src/b.ts", "dist/c.ts"]);
    std::fs::write(
        root.path().join("app/tsconfig.json"),
        r#"{"extends":"../base.json","files":["src/b.ts","missing.ts"],"include":[]}"#,
    )
    .unwrap();
    index.sync().unwrap();
    parity(
        root.path(),
        store.path(),
        &["app/src/b.ts", "app/missing.ts"],
    );
}
