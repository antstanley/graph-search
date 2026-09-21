//! Authored workspace dependency identity, with incremental/rebuild parity.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_types::EdgeKind;
use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery, ResolutionClass};
use std::path::Path;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn open(root: &Path, store: Option<std::path::PathBuf>) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store,
        ..OpenOptions::default()
    })
    .unwrap()
}
fn query() -> OccurrenceQuery {
    OccurrenceQuery {
        by: OccurrenceBy::Name,
        target: "send".into(),
        ..OccurrenceQuery::default()
    }
}
fn fixture(root: &Path) {
    write(root, "package.json", r#"{"name":"root"}"#);
    write(
        root,
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\n  - '!packages/excluded'\n",
    );
    write(
        root,
        "packages/api/package.json",
        r#"{"name":"@native/api","exports":{".":{"types":"./src/index.ts","default":"./src/index.ts"},"./sub":"./src/index.ts","./hidden":null}}"#,
    );
    write(
        root,
        "packages/api/src/index.ts",
        "export function send(){} function hidden(){}",
    );
    write(
        root,
        "packages/client/package.json",
        r#"{"name":"client","dependencies":{"@native/api":"workspace:*"}}"#,
    );
    write(
        root,
        "packages/client/main.ts",
        "import {send} from '@native/api'; export function entry(){send();}",
    );
}
fn assert_call(index: &Index, resolved: bool) {
    let result = index.search().occurrences(&query()).unwrap();
    let calls: Vec<_> = result
        .items
        .iter()
        .filter(|item| item.occurrence.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 1, "{result:?}");
    assert_eq!(
        calls[0].occurrence.resolution == ResolutionClass::ExplicitImport,
        resolved,
        "{calls:?}"
    );
    if resolved {
        assert_eq!(
            calls[0].occurrence.target.as_ref().unwrap().as_str(),
            "sym:packages/api/src/index.ts#function:send"
        );
    }
}

#[test]
fn pnpm_workspace_dependencies_follow_declared_identity_and_uniform_exports() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    // A globally identical name outside membership must not create ambiguity.
    write(
        root.path(),
        "outside/package.json",
        r#"{"name":"@native/api","exports":"./index.ts"}"#,
    );
    write(root.path(), "outside/index.ts", "export function send(){}");
    write(
        root.path(),
        "packages/excluded/package.json",
        r#"{"name":"@native/api","exports":"./index.ts"}"#,
    );
    write(
        root.path(),
        "packages/excluded/index.ts",
        "export function send(){}",
    );
    let index = open(root.path(), None);
    index.reindex().unwrap();
    assert_call(&index, true);
}

#[test]
fn edits_rebind_workspace_dependencies_after_reopen_like_a_clean_build() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let index = open(root.path(), None);
    index.reindex().unwrap();
    assert_call(&index, true);
    drop(index);
    for (path, text, resolved) in [
        (
            "packages/client/package.json",
            r#"{"name":"client","dependencies":{"@native/api":"^1"}}"#,
            false,
        ),
        (
            "packages/client/package.json",
            r#"{"name":"client","dependencies":{"@native/api":"workspace:*"}}"#,
            true,
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - packages/client\n",
            false,
        ),
        ("pnpm-workspace.yaml", "packages:\n  - packages/*\n", true),
        (
            "packages/duplicate/package.json",
            r#"{"name":"@native/api","exports":"./index.ts"}"#,
            false,
        ),
        (
            "packages/duplicate/package.json",
            r#"{"name":"other"}"#,
            true,
        ),
        ("packages/duplicate/package.json", "{broken", false),
        (
            "packages/duplicate/package.json",
            r#"{"name":"other"}"#,
            true,
        ),
        (
            "packages/api/package.json",
            r#"{"name":"@native/api","exports":{".":{"types":"./types.ts","default":"./src/index.ts"}}}"#,
            false,
        ),
        (
            "packages/api/package.json",
            r#"{"name":"@native/api","exports":"./src/index.ts"}"#,
            true,
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - packages/*\noverrides:\n  '@native/api': workspace:*\n",
            false,
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - packages/*\noverrides:\n  '@other/api': workspace:*\n",
            true,
        ),
        (
            "package.json",
            r#"{"name":"root","pnpm":{"overrides":{"@native/api":"file:./elsewhere"}}}"#,
            false,
        ),
        ("package.json", r#"{"name":"root"}"#, true),
        ("packages/client/pnpm-workspace.yaml", "packages: []", false),
    ] {
        write(root.path(), path, text);
        let index = open(root.path(), None);
        index.sync().unwrap();
        drop(index);
        let index = open(root.path(), None);
        assert_call(&index, resolved);
        let clean_store = tempfile::tempdir().unwrap();
        let clean = open(root.path(), Some(clean_store.path().join("index")));
        clean.reindex().unwrap();
        assert_eq!(
            index.search().occurrences(&query()).unwrap().items,
            clean.search().occurrences(&query()).unwrap().items,
            "{path}"
        );
    }
}

#[test]
fn package_json_workspace_declarations_and_missing_dependencies_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    std::fs::remove_file(root.path().join("pnpm-workspace.yaml")).unwrap();
    write(
        root.path(),
        "package.json",
        r#"{"name":"root","workspaces":["packages/*"]}"#,
    );
    let index = open(root.path(), None);
    index.reindex().unwrap();
    assert_call(&index, true);
    write(
        root.path(),
        "packages/client/package.json",
        r#"{"name":"client"}"#,
    );
    index.sync().unwrap();
    assert_call(&index, false);
}

#[test]
fn explicit_pnpm_manager_never_uses_package_json_workspaces_as_a_substitute() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    std::fs::remove_file(root.path().join("pnpm-workspace.yaml")).unwrap();
    write(
        root.path(),
        "package.json",
        r#"{"name":"root","packageManager":"pnpm@11.24.0","workspaces":["packages/*"]}"#,
    );
    let index = open(root.path(), None);
    index.reindex().unwrap();
    assert_call(&index, false);
    write(
        root.path(),
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\n",
    );
    index.sync().unwrap();
    assert_call(&index, true);
}

#[test]
fn child_json_workspaces_cannot_override_an_enclosing_pnpm_workspace() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    write(
        root.path(),
        "packages/client/package.json",
        r#"{"name":"client","workspaces":["private/*"],"dependencies":{"@native/api":"workspace:*"}}"#,
    );
    write(
        root.path(),
        "packages/client/private/api/package.json",
        r#"{"name":"@native/api","exports":"./index.ts"}"#,
    );
    write(
        root.path(),
        "packages/client/private/api/index.ts",
        "export function send(){}",
    );
    let index = open(root.path(), None);
    index.reindex().unwrap();
    assert_call(&index, true);
    write(
        root.path(),
        "packages/client/pnpm-workspace.yaml",
        "packages:\n  - private/*\n",
    );
    index.sync().unwrap();
    let result = index.search().occurrences(&query()).unwrap();
    let call = result
        .items
        .iter()
        .find(|item| item.occurrence.kind == EdgeKind::Calls)
        .unwrap();
    assert_eq!(
        call.occurrence.target.as_ref().unwrap().as_str(),
        "sym:packages/client/private/api/index.ts#function:send"
    );
}
