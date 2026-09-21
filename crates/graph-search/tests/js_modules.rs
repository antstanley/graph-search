//! Native ESM export identity, forwarding and module-boundary precision.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_types::{EdgeKind, TraversalQuery, occurrence::ResolutionClass};

fn open(root: &std::path::Path, store: Option<std::path::PathBuf>) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store,
        ..OpenOptions::default()
    })
    .unwrap()
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
fn imports_require_exports_and_preserve_named_default_and_namespace_identity() {
    for extension in ["js", "ts"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(format!("api.{extension}")),"function privateSend(){} export function send(){} function relay(){} export {relay as publicRelay}; export default function primary(){} export const first=()=>{}, second=()=>{};\n").unwrap();
        std::fs::write(root.path().join(format!("client.{extension}")),"import primary, {privateSend,send as deliver,publicRelay,first,second} from './api'; import * as api from './api'; function hidden(){privateSend();} function named(){deliver();} function aliased(){publicRelay();} function defaulted(){primary();} function namespaced(){api.send();} function namespaceHidden(){api.privateSend();} function secondValue(){second();} function shadow(api){api.send();}\n").unwrap();
        let index = open(root.path(), None);
        index.reindex().unwrap();
        let target = format!("api.{extension}");
        for (owner, name) in [
            ("named", "send"),
            ("aliased", "relay"),
            ("defaulted", "primary"),
            ("namespaced", "send"),
            ("secondValue", "second"),
        ] {
            call(&index, owner, Some((&target, name)));
        }
        for owner in ["hidden", "namespaceHidden", "shadow"] {
            call(&index, owner, None);
        }
    }
}

#[test]
fn reexports_follow_bindings_and_handle_stars_cycles_duplicates_and_type_only_calls() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "api.ts",
            "export function send(){} export default function primary(){}",
        ),
        ("other.ts", "export function send(){}"),
        (
            "named.ts",
            "export {send as deliver, default} from './api';",
        ),
        (
            "local.ts",
            "import {send as local} from './api'; export {local as deliver};",
        ),
        ("cycle.ts", "export * from './star';"),
        ("star.ts", "export * from './api'; export * from './cycle';"),
        (
            "ambiguous.ts",
            "export * from './api'; export * from './other';",
        ),
        (
            "diamond.ts",
            "export * from './api'; export * from './star';",
        ),
        ("typed.ts", "export type {send} from './api';"),
        (
            "client.ts",
            "import {deliver as a} from './named'; import main from './named'; import {deliver as b} from './local'; import {send as c} from './star'; import {send as d} from './ambiguous'; import {send as e} from './diamond'; import {send as f} from './typed'; import type {send as g} from './api'; import missingDefault from './star'; function named(){a();} function defaulted(){main();} function local(){b();} function star(){c();} function ambiguous(){d();} function diamond(){e();} function typed(){f();} function importedType(){g();} function starDefault(){missingDefault();}",
        ),
    ] {
        std::fs::write(root.path().join(path), source).unwrap();
    }
    let index = open(root.path(), None);
    index.reindex().unwrap();
    for owner in ["named", "local", "star", "diamond"] {
        call(&index, owner, Some(("api.ts", "send")));
    }
    call(&index, "defaulted", Some(("api.ts", "primary")));
    for owner in ["ambiguous", "typed", "importedType", "starDefault"] {
        call(&index, owner, None);
    }
}

#[test]
fn export_edits_rebind_consumers_after_reopen_like_clean_rebuilds() {
    use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("api.ts"), "export function send(){}").unwrap();
    std::fs::write(
        root.path().join("barrel.ts"),
        "export {send as relay} from './api';",
    )
    .unwrap();
    std::fs::write(
        root.path().join("client.ts"),
        "import {relay} from './barrel'; export function entry(){relay();}",
    )
    .unwrap();
    let query = OccurrenceQuery {
        by: OccurrenceBy::Name,
        target: "relay".into(),
        ..OccurrenceQuery::default()
    };
    let index = open(root.path(), None);
    index.reindex().unwrap();
    drop(index);
    for (path, source, resolved) in [
        ("api.ts", Some("function send(){}"), false),
        ("api.ts", Some("export function send(){}"), true),
        ("api.ts", Some("export function changed(){}"), false),
        ("api.ts", Some("function send(){} export {send};"), true),
        (
            "barrel.ts",
            Some("export {send as relay, send as relay} from './api';"),
            false,
        ),
        ("barrel.ts", None, false),
        (
            "barrel.ts",
            Some("export {send as relay} from './api';"),
            true,
        ),
    ] {
        if let Some(source) = source {
            std::fs::write(root.path().join(path), source).unwrap();
        } else {
            std::fs::remove_file(root.path().join(path)).unwrap();
        }
        let index = open(root.path(), None);
        index.sync().unwrap();
        drop(index);
        let index = open(root.path(), None);
        let actual = index.search().occurrences(&query).unwrap();
        let calls: Vec<_> = actual
            .items
            .iter()
            .filter(|record| record.occurrence.kind == EdgeKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].occurrence.resolution == ResolutionClass::ExplicitImport,
            resolved,
            "{path} {source:?}: {calls:?}"
        );
        let store = tempfile::tempdir().unwrap();
        let rebuilt = open(root.path(), Some(store.path().join("index")));
        rebuilt.reindex().unwrap();
        assert_eq!(
            actual.items,
            rebuilt.search().occurrences(&query).unwrap().items,
            "{path} {source:?}"
        );
    }
}

#[test]
fn explicit_exports_override_stars_and_long_forwarding_chains_stop() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("api.js", "export function send(){}"),
        ("other.js", "export function send(){}"),
        (
            "ambiguous.js",
            "export * from './api'; export * from './other';",
        ),
        (
            "override.js",
            "export * from './ambiguous'; export {send} from './api';",
        ),
        (
            "client.js",
            "import {send as good} from './override'; import {send as deep} from './chain0'; function direct(){good();} function bounded(){deep();}",
        ),
    ] {
        std::fs::write(root.path().join(path), source).unwrap();
    }
    for index in 0..70 {
        let target = if index == 69 {
            "api".into()
        } else {
            format!("chain{}", index + 1)
        };
        std::fs::write(
            root.path().join(format!("chain{index}.js")),
            format!("export {{send}} from './{target}';"),
        )
        .unwrap();
    }
    let index = open(root.path(), None);
    index.reindex().unwrap();
    call(&index, "direct", Some(("api.js", "send")));
    call(&index, "bounded", None);
}

#[test]
fn package_self_exports_and_private_maps_require_authored_targets() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    for (path, source) in [
        (
            "package.json",
            r##"{"name":"@demo/api","type":"module","main":"./wrong.js","exports":{".":"./api.js","./sub":"./sub.js","./hidden":null,"./conditional":{"import":"./api.js"},"./escape":"../outside.js","./wild/*":"./*.js"},"imports":{"#internal":"./api.js"}}"##,
        ),
        (
            "api.ts",
            "export function send(){} export default function primary(){} function hidden(){}",
        ),
        ("sub.ts", "export function sub(){}"),
        ("wrong.ts", "export function send(){}"),
        ("barrel.ts", "export {send as relay} from '@demo/api';"),
        ("nested/package.json", "{}"),
        (
            "nested/client.ts",
            "import {send} from '#internal'; function nested(){send();}",
        ),
        (
            "client.ts",
            "import main, {send,hidden} from '@demo/api'; import {sub} from '@demo/api/sub'; import {send as privateSend} from '#internal'; import {relay} from './barrel'; import {send as blocked} from '@demo/api/hidden'; import {send as conditional} from '@demo/api/conditional'; import {send as escape} from '@demo/api/escape'; import {send as wildcard} from '@demo/api/wild/api'; function namedPackage(){send();} function defaultPackage(){main();} function privateMap(){privateSend();} function subpath(){sub();} function forwarded(){relay();} function hiddenSymbol(){hidden();} function blockedPath(){blocked();} function conditionalPath(){conditional();} function escapedPath(){escape();} function wildcardPath(){wildcard();}",
        ),
    ] {
        std::fs::write(root.path().join(path), source).unwrap();
    }
    let index = open(root.path(), None);
    index.reindex().unwrap();
    for owner in ["namedPackage", "privateMap", "forwarded"] {
        call(&index, owner, Some(("api.ts", "send")));
    }
    call(&index, "defaultPackage", Some(("api.ts", "primary")));
    call(&index, "subpath", Some(("sub.ts", "sub")));
    for owner in [
        "nested",
        "hiddenSymbol",
        "blockedPath",
        "conditionalPath",
        "escapedPath",
        "wildcardPath",
    ] {
        call(&index, owner, None);
    }
}

#[test]
fn package_map_edits_and_missing_target_creation_rebind_after_reopen() {
    use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"name":"pkg","exports":"./api.js"}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("client.ts"),
        "import main from 'pkg'; export function entry(){main();}",
    )
    .unwrap();
    let index = open(root.path(), None);
    index.reindex().unwrap();
    call(&index, "entry", None);
    drop(index);
    let query = OccurrenceQuery {
        by: OccurrenceBy::Name,
        target: "main".into(),
        ..OccurrenceQuery::default()
    };
    for (path, source, resolved) in [
        ("api.ts", "export default function primary(){}", true),
        (
            "package.json",
            r#"{"name":"pkg","exports":null,"main":"./api.js"}"#,
            false,
        ),
        (
            "package.json",
            r#"{"name":"pkg","exports":"./api.js"}"#,
            true,
        ),
        ("api.tsx", "export default function primary(){}", false),
        ("api.js", "export default function runtime(){}", true),
        ("package.json", r#"{"name":"pkg","main":"./api.js"}"#, false),
    ] {
        std::fs::write(root.path().join(path), source).unwrap();
        let index = open(root.path(), None);
        index.sync().unwrap();
        drop(index);
        let index = open(root.path(), None);
        let actual = index.search().occurrences(&query).unwrap();
        let calls: Vec<_> = actual
            .items
            .iter()
            .filter(|item| item.occurrence.kind == EdgeKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].occurrence.resolution == ResolutionClass::ExplicitImport,
            resolved,
            "{path}: {calls:?}"
        );
        let store = tempfile::tempdir().unwrap();
        let clean = open(root.path(), Some(store.path().join("index")));
        clean.reindex().unwrap();
        assert_eq!(
            actual.items,
            clean.search().occurrences(&query).unwrap().items,
            "{path}"
        );
    }
}
