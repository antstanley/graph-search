//! Declared TypeScript path aliases resolve as native imports, with fallback.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_types::{EdgeKind, TraversalQuery};

fn open(root: &std::path::Path, store: &std::path::Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..OpenOptions::default()
    })
    .unwrap()
}

fn config(paths: &str, resolution: &str) -> String {
    format!(
        "{{\"compilerOptions\":{{\"baseUrl\":\".\",\"paths\":{paths},\"moduleResolution\":\"{resolution}\",\"module\":\"esnext\"}}}}"
    )
}

fn call(index: &Index, owner: &str, expected: Option<(&str, &str)>) {
    let result = index
        .search()
        .callees(&TraversalQuery::new(owner, 1))
        .unwrap();
    let calls: Vec<_> = result
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 1, "{owner}: {result:?}");
    if let Some((path, name)) = expected {
        let target = calls[0]
            .to
            .as_ref()
            .unwrap_or_else(|| panic!("{owner}: {calls:?}"));
        let node = result.nodes.iter().find(|node| &node.id == target).unwrap();
        assert_eq!(node.path, path, "{owner}");
        assert_eq!(node.name, name, "{owner}");
    } else {
        assert!(!calls[0].resolved, "{owner}: {calls:?}");
    }
}

#[test]
fn declared_aliases_resolve_and_track_configuration_edits() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/lib")).unwrap();
    let config_path = root.path().join("tsconfig.json");
    std::fs::write(
        &config_path,
        config("{\"@lib/*\":[\"src/lib/*\"]}", "bundler"),
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/lib/thing.ts"),
        "export function aliasTarget(): number { return 1; }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/app.ts"),
        "import { aliasTarget } from \"@lib/thing\";\n\
         export function caller(): number { return aliasTarget(); }\n",
    )
    .unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    call(&index, "caller", Some(("src/lib/thing.ts", "aliasTarget")));

    // Removing the declared mapping returns the call to unresolved, not to a guess.
    std::fs::write(&config_path, config("{}", "bundler")).unwrap();
    index.sync().unwrap();
    call(&index, "caller", None);

    // Restoring it resolves again, matching a clean rebuild of the same source.
    std::fs::write(
        &config_path,
        config("{\"@lib/*\":[\"src/lib/*\"]}", "bundler"),
    )
    .unwrap();
    index.sync().unwrap();
    call(&index, "caller", Some(("src/lib/thing.ts", "aliasTarget")));
    let clean_store = tempfile::tempdir().unwrap();
    let clean = open(root.path(), clean_store.path());
    clean.reindex().unwrap();
    call(&clean, "caller", Some(("src/lib/thing.ts", "aliasTarget")));
}

#[test]
fn unsupported_resolution_modes_leave_ordinary_resolution_in_place() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/lib")).unwrap();
    std::fs::write(
        root.path().join("tsconfig.json"),
        config("{\"@lib/*\":[\"src/lib/*\"]}", "classic"),
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/lib/thing.ts"),
        "export function aliasTarget(): number { return 1; }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/app.ts"),
        "import { aliasTarget } from \"@lib/thing\";\n\
         export function caller(): number { return aliasTarget(); }\n",
    )
    .unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    call(&index, "caller", None);
}
