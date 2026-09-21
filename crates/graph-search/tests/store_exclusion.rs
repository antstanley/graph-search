//! Index storage must never become searchable source.
#![allow(clippy::expect_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_types::{FilesQuery, TextQuery};

#[test]
fn custom_in_tree_store_stays_out_of_source_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("a.rs"), "fn source_marker() {}\n").unwrap();
    let options = OpenOptions {
        root: root.to_owned(),
        store: Some(root.join("index-store")),
        ..OpenOptions::default()
    };
    let index = Index::open(options.clone()).unwrap();
    let report = index.reindex().unwrap();
    assert_eq!(report.added, vec!["a.rs"]);
    assert_eq!(report.coverage.admitted_files, 1);
    assert!(index.sync().unwrap().added.is_empty());
    let generation = index.search().status().unwrap().generation;
    assert_eq!(
        index.search().status().unwrap().staleness.unwrap().changed,
        0
    );
    drop(index);
    let index = Index::open(options.clone()).unwrap();
    assert!(index.sync().unwrap().added.is_empty());
    assert_eq!(index.search().status().unwrap().generation, generation);
    std::fs::write(
        root.join("index-store/decoy.rs"),
        "fn storage_marker() {}\n",
    )
    .unwrap();
    assert_eq!(
        index.search().status().unwrap().staleness.unwrap().changed,
        0
    );
    let mut query = FilesQuery::new("*");
    query.include_hidden = true;
    query.no_ignore = true;
    assert_eq!(index.search().files(&query).unwrap().items.len(), 1);
    query.path = Some(String::from("index-store"));
    assert!(index.search().files(&query).unwrap().items.is_empty());
    query.path = Some(String::from("index-store/../index-store"));
    assert!(index.search().files(&query).unwrap().items.is_empty());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("index-store"), root.join("store-alias")).unwrap();
        query.path = Some(String::from("store-alias"));
        assert!(index.search().files(&query).unwrap().items.is_empty());
    }
    let mut query = TextQuery::new("source_marker");
    query.include_hidden = true;
    query.no_ignore = true;
    assert_eq!(index.search().text(&query).unwrap().items.len(), 1);
    assert!(
        index
            .search()
            .explore(&graph_search_types::ExploreQuery::new("storage_marker"))
            .unwrap()
            .items
            .iter()
            .all(|item| item.node.path == "a.rs")
    );
    std::fs::create_dir_all(root.join("other/index-store")).unwrap();
    std::fs::write(root.join("other/index-store/b.rs"), "fn sibling() {}\n").unwrap();
    std::fs::write(root.join("a.rs"), "fn changed_source_marker() {}\n").unwrap();
    let report = index.sync().unwrap();
    assert_eq!(report.added, vec!["other/index-store/b.rs"]);
    assert_eq!(report.modified, vec!["a.rs"]);
    assert_eq!(report.coverage.admitted_files, 2);
    drop(index);
    let index = Index::open(OpenOptions {
        read_only: true,
        ..options
    })
    .unwrap();
    assert_eq!(
        index.search().status().unwrap().staleness.unwrap().changed,
        0
    );
}

#[test]
fn store_cannot_contain_the_source_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir(&root).unwrap();
    for store in [&root, temp.path()] {
        let result = Index::open(OpenOptions {
            root: root.clone(),
            store: Some(store.to_owned()),
            ..OpenOptions::default()
        });
        assert!(matches!(result, Err(graph_search::Error::Config(_))));
    }
    assert!(!root.join("CURRENT").exists());
}

#[cfg(unix)]
#[test]
fn symlink_store_and_root_use_the_actual_subtree() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("storage")).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let alias = temp.path().join("root-alias");
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    let store_alias = temp.path().join("store-alias");
    std::os::unix::fs::symlink(root.join("storage"), &store_alias).unwrap();
    let index = Index::open(OpenOptions {
        root: alias,
        store: Some(store_alias.join("new/../index")),
        ..OpenOptions::default()
    })
    .unwrap();
    assert_eq!(index.reindex().unwrap().added, vec!["a.rs"]);
    assert_eq!(index.sync().unwrap().unchanged, 1);
    assert_eq!(
        index.store_dir(),
        root.canonicalize().unwrap().join("storage/index")
    );
}

#[test]
fn external_store_leaves_same_named_source_directory_searchable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("index-store")).unwrap();
    std::fs::write(root.join("index-store/a.rs"), "fn a() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root,
        store: Some(temp.path().join("index-store")),
        ..OpenOptions::default()
    })
    .unwrap();
    assert_eq!(index.reindex().unwrap().added, vec!["index-store/a.rs"]);
    let mut query = FilesQuery::new("*");
    query.path = Some(String::from("../index-store"));
    assert!(index.search().files(&query).unwrap().items.is_empty());
    assert_eq!(index.policy().excluded_paths.len(), 1);
}

#[test]
fn configured_store_exclusion_survives_replacing_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join(".graph-search")).unwrap();
    std::fs::write(
        root.join(".graph-search/config.toml"),
        "store = 'storage'\nreplace_defaults = true\nexcludes = []\ninclude_hidden = true\n",
    )
    .unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.to_owned(),
        ..OpenOptions::default()
    })
    .unwrap();
    let report = index.reindex().unwrap();
    assert_eq!(report.added, vec!["a.rs"]);
    assert_eq!(index.sync().unwrap().unchanged, 1);
    assert_eq!(index.policy().excluded_paths.len(), 1);
    assert_eq!(
        report.coverage.policy.unwrap().fingerprint,
        index.policy().fingerprint()
    );
}

#[test]
fn policy_change_removes_previously_indexed_storage() {
    use graph_search::core::{config::WalkPolicy, ports::ListRegistry, reconcile::Projector};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let store = root.join("storage");
    std::fs::create_dir(&store).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(store.join("decoy.rs"), "fn storage_marker() {}\n").unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let mut old = graph_search_engine::GrafeoStore::open(
        &store,
        &graph_search_engine::StoreOptions::default(),
    )
    .unwrap();
    let report = Projector::new(&registry, &WalkPolicy::default())
        .reindex(&root, &mut old)
        .unwrap();
    assert!(report.added.contains(&String::from("storage/decoy.rs")));
    drop(old);
    let index = Index::open(OpenOptions {
        root,
        store: Some(store),
        ..OpenOptions::default()
    })
    .unwrap();
    assert!(index.sync().unwrap().reindexed_all);
    assert_eq!(
        index.search().status().unwrap().staleness.unwrap().changed,
        0
    );
    assert!(
        index
            .search()
            .symbol(&graph_search_types::SymbolQuery::new("storage_marker"))
            .unwrap()
            .nodes
            .is_empty()
    );
    assert_eq!(index.sync().unwrap().unchanged, 1);
}
