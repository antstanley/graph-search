//! Public Rust reexports resolve through their declaring module scope.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::GraphStore;
use graph_search_types::{EdgeKind, occurrence::OccurrenceFile};
use std::{collections::BTreeMap, path::Path};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn open(root: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        reconcile: Reconcile::Never,
        ..Default::default()
    })
    .unwrap()
}

fn facts(index: &Index) -> BTreeMap<String, OccurrenceFile> {
    let store = graph_search_engine::GrafeoStore::open(
        index.store_dir(),
        &graph_search_engine::StoreOptions::default(),
    )
    .unwrap();
    store
        .snapshot()
        .unwrap()
        .occurrence_files()
        .unwrap()
        .clone()
}

fn resolved(source: &BTreeMap<String, OccurrenceFile>, path: &str, name: &str) -> Vec<String> {
    source[path]
        .records
        .iter()
        .filter(|fact| fact.name == name)
        .map(|fact| {
            fact.target.as_ref().map_or_else(
                || format!("unresolved:{}", fact.reason.as_deref().unwrap_or("?")),
                std::string::ToString::to_string,
            )
        })
        .collect()
}

#[test]
fn public_reexports_resolve_and_private_uses_do_not_publish() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "src/lib.rs",
        "pub mod inner;\npub mod middle;\npub mod user;\npub use crate::inner::reexported_target;\n\
         pub use crate::middle::hop;\n",
    );
    write(
        root.path(),
        "src/inner.rs",
        "pub fn reexported_target() -> u32 { 1 }\n",
    );
    write(
        root.path(),
        "src/middle.rs",
        "pub use crate::inner::reexported_target as hop;\n",
    );
    write(
        root.path(),
        "src/user.rs",
        "use crate::reexported_target;\n\
         use crate::hop;\n\
         pub fn caller() -> u32 { reexported_target() + hop() }\n",
    );
    let index = open(root.path());
    index.reindex().unwrap();
    let source = facts(&index);
    let direct = resolved(&source, "src/user.rs", "crate::reexported_target");
    assert!(
        direct.iter().any(|target| target.contains("src/inner.rs")),
        "direct reexport did not resolve: {direct:?}"
    );
    let chained = resolved(&source, "src/user.rs", "crate::hop");
    assert!(
        chained.iter().any(|target| target.contains("src/inner.rs")),
        "chained reexport did not resolve: {chained:?}"
    );
    let calls = source["src/user.rs"]
        .records
        .iter()
        .filter(|fact| fact.kind == EdgeKind::Calls)
        .count();
    assert_eq!(
        calls, 2,
        "expected both reexported calls: {:?}",
        source["src/user.rs"]
    );

    // A private `use` is not a reexport and must not publish the name.
    write(
        root.path(),
        "src/lib.rs",
        "pub mod inner;\npub mod middle;\npub mod user;\nuse crate::inner::reexported_target;\n\
         pub use crate::middle::hop;\n",
    );
    index.sync().unwrap();
    let source = facts(&index);
    let unpublished = resolved(&source, "src/user.rs", "crate::reexported_target");
    assert!(
        unpublished
            .iter()
            .all(|target| target.starts_with("unresolved:")),
        "private use published a reexport: {unpublished:?}"
    );
}
