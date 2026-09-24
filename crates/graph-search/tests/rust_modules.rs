//! Native Rust module declaration paths and persisted rebinding.
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
    let store = graph_search_engine::NativeStore::open(
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
fn target(
    source: &BTreeMap<String, OccurrenceFile>,
    path: &str,
    name: &str,
    expected: Result<&str, &str>,
) {
    let matches: Vec<_> = source[path]
        .records
        .iter()
        .filter(|fact| fact.kind == EdgeKind::Imports && fact.name == name)
        .collect();
    assert_eq!(matches.len(), 1, "{path}/{name}: {matches:?}");
    let fact = matches[0];
    assert!(fact.span.is_some());
    match expected {
        Ok(path) => {
            assert_eq!(
                fact.target.as_ref().unwrap().as_str(),
                format!("file:{path}")
            );
            assert!(fact.reason.is_none());
        }
        Err(reason) => {
            assert!(fact.target.is_none(), "{fact:?}");
            assert_eq!(fact.reason.as_deref(), Some(reason), "{fact:?}");
        }
    }
}

#[test]
fn cargo_roots_inline_modules_path_attributes_and_ambiguity_have_distinct_paths() {
    let root = tempfile::tempdir().unwrap();
    for (path, code) in [
        ("Cargo.toml", "[workspace]\nmembers=['packages/p']\n"),
        (
            "packages/p/Cargo.toml",
            "[package]\nname='p'\nedition='2021'\n[lib]\npath='custom/root.rs'\n",
        ),
        (
            "packages/p/custom/root.rs",
            "mod outer; mod inline { mod child; }\n#[path=\"other.rs\"]\nmod redirected;\n#[path=\"thread_files\"]\nmod threaded { #[path=\"tls.rs\"] mod data; }\n#[cfg_attr(feature=\"other\",path=\"other.rs\")] mod uncertain;\n#[path=r#\"../shared.rs\"#] mod shared;\nmod missing; mod ambiguous;\nfn entry() { mod local; }\n",
        ),
        (
            "packages/p/custom/outer.rs",
            "mod child; #[path=\"near.rs\"] mod near; mod nested { #[path=\"other.rs\"] mod file; }\n",
        ),
        ("packages/p/custom/outer/child.rs", ""),
        ("packages/p/custom/child.rs", "// decoy sibling\n"),
        ("packages/p/custom/near.rs", ""),
        ("packages/p/custom/outer/nested/other.rs", ""),
        ("packages/p/custom/inline/child.rs", ""),
        ("packages/p/custom/other.rs", ""),
        ("packages/p/custom/thread_files/tls.rs", ""),
        ("packages/p/shared.rs", ""),
        ("packages/p/custom/uncertain.rs", ""),
        ("packages/p/custom/ambiguous.rs", ""),
        ("packages/p/custom/ambiguous/mod.rs", ""),
        ("packages/p/tests/check.rs", "mod helper;\n"),
        ("packages/p/tests/helper.rs", ""),
    ] {
        write(root.path(), path, code);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let source = facts(&index);
    let entry = "packages/p/custom/root.rs";
    for (name, path) in [
        ("outer", "packages/p/custom/outer.rs"),
        ("child", "packages/p/custom/inline/child.rs"),
        ("redirected", "packages/p/custom/other.rs"),
        ("data", "packages/p/custom/thread_files/tls.rs"),
        ("shared", "packages/p/shared.rs"),
    ] {
        target(&source, entry, name, Ok(path));
    }
    target(
        &source,
        entry,
        "uncertain",
        Err("rust_module_attribute_unsupported"),
    );
    target(&source, entry, "missing", Err("module_target_missing"));
    target(
        &source,
        entry,
        "ambiguous",
        Err("rust_module_files_ambiguous"),
    );
    target(
        &source,
        entry,
        "local",
        Err("rust_block_module_unsupported"),
    );
    for (name, path) in [
        ("child", "packages/p/custom/outer/child.rs"),
        ("near", "packages/p/custom/near.rs"),
        ("file", "packages/p/custom/outer/nested/other.rs"),
    ] {
        target(&source, "packages/p/custom/outer.rs", name, Ok(path));
    }
    target(
        &source,
        "packages/p/tests/check.rs",
        "helper",
        Ok("packages/p/tests/helper.rs"),
    );
    drop(index);
    assert_eq!(facts(&open(root.path())), source);
}

#[test]
fn module_creation_conflicts_deletion_and_root_edits_rebind_like_clean_builds() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='p'\nedition='2021'\n[lib]\npath='custom/root.rs'\n",
    );
    write(root.path(), "custom/root.rs", "mod child;\n");
    let mut index = open(root.path());
    index.reindex().unwrap();
    let states = [
        (Some("custom/child.rs"), None, Ok("custom/child.rs")),
        (
            Some("custom/child/mod.rs"),
            None,
            Err("rust_module_files_ambiguous"),
        ),
        (None, Some("custom/child.rs"), Ok("custom/child/mod.rs")),
        (
            None,
            Some("custom/child/mod.rs"),
            Err("module_target_missing"),
        ),
    ];
    for (add, remove, expected) in states {
        if let Some(path) = add {
            write(root.path(), path, "");
        }
        if let Some(path) = remove {
            std::fs::remove_file(root.path().join(path)).unwrap();
        }
        index.sync().unwrap();
        drop(index);
        index = open(root.path());
        let incremental = facts(&index);
        target(&incremental, "custom/root.rs", "child", expected);
        index.reindex().unwrap();
        assert_eq!(facts(&index), incremental);
    }
    write(
        root.path(),
        "custom/root/child.rs",
        "// decoy for path-loaded module",
    );
    write(root.path(), "custom/child.rs", "");
    write(
        root.path(),
        "src/lib.rs",
        "#[path=\"../custom/root.rs\"] mod nested;\n",
    );
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='p'\nedition='2021'\n",
    );
    index.sync().unwrap();
    let incremental = facts(&index);
    target(
        &incremental,
        "custom/root.rs",
        "child",
        Ok("custom/child.rs"),
    );
    target(&incremental, "src/lib.rs", "nested", Ok("custom/root.rs"));
    index.reindex().unwrap();
    assert_eq!(facts(&index), incremental);
}

#[test]
fn shared_root_context_and_build_scripts_preserve_uncertainty_and_explicit_paths() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "Cargo.toml",
            "[package]\nname='p'\nedition='2021'\nbuild='tools/setup.rs'\n[lib]\npath='custom/root.rs'\n[[bin]]\nname='worker'\npath='custom/main.rs'\n",
        ),
        (
            "custom/root.rs",
            "mod child; #[path=\"shared.rs\"] mod fixed;\n",
        ),
        ("custom/main.rs", "mod root;\n"),
        ("custom/child.rs", ""),
        ("custom/root/child.rs", ""),
        ("custom/shared.rs", ""),
        ("tools/setup.rs", "mod helper;\n"),
        ("tools/helper.rs", ""),
    ] {
        write(root.path(), path, source);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let source = facts(&index);
    target(
        &source,
        "custom/root.rs",
        "child",
        Err("rust_module_context_ambiguous"),
    );
    target(&source, "custom/root.rs", "fixed", Ok("custom/shared.rs"));
    target(&source, "custom/main.rs", "root", Ok("custom/root.rs"));
    target(&source, "tools/setup.rs", "helper", Ok("tools/helper.rs"));
}

#[test]
fn file_inner_path_attributes_cannot_select_a_wrong_default_child() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("Cargo.toml", "[package]\nname='p'\n"),
        ("src/lib.rs", "mod outer;\n"),
        ("src/outer.rs", "#![path=\"changed.rs\"]\nmod child;\n"),
        ("src/child.rs", ""),
        ("src/outer/child.rs", ""),
    ] {
        write(root.path(), path, source);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let source = facts(&index);
    target(&source, "src/lib.rs", "outer", Ok("src/outer.rs"));
    target(
        &source,
        "src/outer.rs",
        "child",
        Err("rust_module_attribute_unsupported"),
    );
}

#[test]
fn path_loaded_diamonds_cycles_and_orphans_keep_distinct_directory_contexts() {
    let root = tempfile::tempdir().unwrap();
    let entry = "mod a; mod b; #[path=\"lib.rs\"] mod recursive;\n";
    for (path, source) in [
        ("Cargo.toml", "[package]\nname='p'\nedition='2021'\n"),
        ("src/lib.rs", entry),
        ("src/a.rs", "#[path=\"shared.rs\"] mod common;\n"),
        ("src/b.rs", "#[path=\"shared.rs\"] mod common;\n"),
        ("src/shared.rs", "mod child;\n"),
        ("src/child.rs", ""),
        ("src/shared/child.rs", "// decoy until default-loaded\n"),
        ("orphan.rs", "mod lost;\n"),
        ("orphan/lost.rs", ""),
    ] {
        write(root.path(), path, source);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let source = facts(&index);
    target(&source, "src/shared.rs", "child", Ok("src/child.rs"));
    target(&source, "src/lib.rs", "recursive", Ok("src/lib.rs"));
    target(
        &source,
        "orphan.rs",
        "lost",
        Err("rust_module_context_unknown"),
    );
    for (code, expected) in [
        (
            format!("{entry}mod shared;\n"),
            Err("rust_module_context_ambiguous"),
        ),
        (entry.into(), Ok("src/child.rs")),
    ] {
        write(root.path(), "src/lib.rs", &code);
        index.sync().unwrap();
        let incremental = facts(&index);
        target(&incremental, "src/shared.rs", "child", expected);
        index.reindex().unwrap();
        assert_eq!(facts(&index), incremental);
    }
}

#[test]
fn nonstandard_module_path_creation_and_removal_rebind_cached_declarations() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "Cargo.toml", "[package]\nname='p'\n");
    write(
        root.path(),
        "src/lib.rs",
        "#[path=\"helper.inc\"] mod child;\n",
    );
    let index = open(root.path());
    index.reindex().unwrap();
    target(
        &facts(&index),
        "src/lib.rs",
        "child",
        Err("module_target_missing"),
    );
    for present in [true, false] {
        if present {
            write(
                root.path(),
                "src/helper.inc",
                "// module source with a custom extension\n",
            );
        } else {
            std::fs::remove_file(root.path().join("src/helper.inc")).unwrap();
        }
        index.sync().unwrap();
        let incremental = facts(&index);
        target(
            &incremental,
            "src/lib.rs",
            "child",
            if present {
                Ok("src/helper.inc")
            } else {
                Err("module_target_missing")
            },
        );
        index.reindex().unwrap();
        assert_eq!(facts(&index), incremental);
    }
}

#[test]
fn anchored_paths_follow_the_current_crate_and_module_tree() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("a/Cargo.toml", "[package]\nname='a'\nedition='2021'\n"),
        ("a/src/lib.rs", "pub fn send() {} mod child;"),
        (
            "a/src/child.rs",
            "pub fn local() {} fn caller() { crate::send(); super::send(); self::local(); crate /* comment */ :: send(); } use crate::send as relay;",
        ),
        ("b/Cargo.toml", "[package]\nname='b'\nedition='2021'\n"),
        ("b/src/lib.rs", "pub fn send() {}"),
    ] {
        write(root.path(), path, source);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    let calls: Vec<_> = all["a/src/child.rs"]
        .records
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 4);
    for call in calls {
        let expected = if call.name == "self::local" {
            "a/src/child.rs#function:local"
        } else {
            "a/src/lib.rs#function:send"
        };
        assert_eq!(
            call.target.as_ref().map(graph_search_types::NodeId::as_str),
            Some(format!("sym:{expected}").as_str()),
            "{call:?}"
        );
    }
    let import = all["a/src/child.rs"]
        .records
        .iter()
        .find(|r| r.kind == EdgeKind::Imports)
        .unwrap();
    assert_eq!(
        import.target.as_ref().unwrap().as_str(),
        "sym:a/src/lib.rs#function:send"
    );
}

#[test]
fn anchored_paths_enforce_basic_visibility_and_shared_root_ambiguity() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("Cargo.toml", "[package]\nname='p'\nedition='2021'\n"),
        (
            "src/lib.rs",
            "mod api { fn hidden() {} pub(super) fn parent() {} pub(crate) fn internal() {} pub(in crate::api) fn restricted() {} } mod sibling { fn call() { crate::api::hidden(); crate::api::parent(); crate::api::internal(); crate::api::restricted(); } } fn root() { super::missing(); }",
        ),
    ] {
        write(root.path(), path, source);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    for (name, reason) in [
        ("crate::api::hidden", Some("rust_path_not_visible")),
        ("crate::api::parent", None),
        ("crate::api::internal", None),
        (
            "crate::api::restricted",
            Some("rust_visibility_restriction_unsupported"),
        ),
        ("super::missing", Some("rust_super_at_crate_root")),
    ] {
        let call = all["src/lib.rs"]
            .records
            .iter()
            .find(|r| r.kind == EdgeKind::Calls && r.name == name)
            .unwrap();
        assert_eq!(call.reason.as_deref(), reason, "{call:?}");
        assert_eq!(call.target.is_some(), reason.is_none(), "{call:?}");
    }
    let shared = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "src/lib.rs",
            "pub fn send() {} #[path=\"shared.rs\"] mod common;",
        ),
        (
            "src/main.rs",
            "pub fn send() {} #[path=\"shared.rs\"] mod common;",
        ),
        (
            "src/shared.rs",
            "fn local() {} fn entry() { crate::send(); super::send(); self::local(); }",
        ),
    ] {
        write(shared.path(), path, source);
    }
    let index = open(shared.path());
    index.reindex().unwrap();
    let all = facts(&index);
    for call in all["src/shared.rs"]
        .records
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
    {
        if call.name == "self::local" {
            assert!(call.target.is_some(), "{call:?}");
        } else {
            assert_eq!(
                call.reason.as_deref(),
                Some("rust_path_context_ambiguous"),
                "{call:?}"
            );
        }
    }
}

#[test]
fn anchored_target_creation_visibility_and_removal_match_clean_rebuilds() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "src/lib.rs", "mod api; mod client;");
    write(
        root.path(),
        "src/client.rs",
        "fn call() { crate::api::send(); }",
    );
    let mut index = open(root.path());
    index.reindex().unwrap();
    for (code, resolved) in [
        (Some("pub fn send() {}"), true),
        (Some("fn send() {}"), false),
        (Some("pub fn send() {} pub fn send() {}"), false),
        (None, false),
        (Some("pub fn send() {}"), true),
    ] {
        if let Some(code) = code {
            write(root.path(), "src/api.rs", code);
        } else {
            std::fs::remove_file(root.path().join("src/api.rs")).unwrap();
        }
        index.sync().unwrap();
        drop(index);
        index = open(root.path());
        let incremental = facts(&index);
        let call = incremental["src/client.rs"]
            .records
            .iter()
            .find(|r| r.kind == EdgeKind::Calls)
            .unwrap();
        assert_eq!(call.target.is_some(), resolved, "{call:?}");
        index.reindex().unwrap();
        assert_eq!(facts(&index), incremental);
    }
}

#[test]
fn named_import_aliases_and_module_aliases_bind_in_lexical_scope() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "src/lib.rs",
        "pub mod api { pub fn send() {} } mod client;",
    );
    write(
        root.path(),
        "src/client.rs",
        "use crate::api::{self as service, send as relay}; fn imported() { relay(); service::send(); } fn shadowed() { fn relay() {} relay(); } fn block() { { use crate::api::send as inner; inner(); } inner(); }",
    );
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    let calls: Vec<_> = all["src/client.rs"]
        .records
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 5);
    for (call, expected) in calls.iter().zip([
        Some("sym:src/lib.rs#function:api::send"),
        Some("sym:src/lib.rs#function:api::send"),
        Some("sym:src/client.rs#function:shadowed::relay"),
        Some("sym:src/lib.rs#function:api::send"),
        None,
    ]) {
        assert_eq!(
            call.target.as_ref().map(graph_search_types::NodeId::as_str),
            expected,
            "{call:?}"
        );
    }
}

#[test]
fn rust_import_lookup_honors_hoisting_value_shadows_namespaces_and_module_barriers() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "src/lib.rs",
        concat!(
            "pub mod api { pub fn send() {} pub fn other() {} } fn send() {}\n",
            "use crate::api::{self as service, send as relay};\n",
            "fn hoisted() { early(); use crate::api::send as early; }\n",
            "fn value() { relay(); let relay = other; relay(); }\n",
            "fn namespaces() { use crate::api::{self as send}; send(); send::send(); }\n",
            "fn qualified_value() { use crate::api::{self as service}; let service = 1; service::send(); }\n",
            "fn ambiguous() { use crate::api::send as target; use crate::api::other as target; target(); }\n",
            "mod child { fn hidden() { relay(); service::send(); } }\n",
        ),
    );
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    let expected = [
        ("hoisted", vec![Some("api::send")]),
        ("value", vec![Some("api::send"), None]),
        ("namespaces", vec![Some("send"), Some("api::send")]),
        ("qualified_value", vec![Some("api::send")]),
        ("ambiguous", vec![None]),
        ("child::hidden", vec![None, None]),
    ];
    for (owner, targets) in expected {
        let id = format!("sym:src/lib.rs#function:{owner}");
        let calls: Vec<_> = all["src/lib.rs"]
            .records
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls && r.owner.as_str() == id)
            .collect();
        assert_eq!(calls.len(), targets.len(), "{owner}");
        for (call, target) in calls.iter().zip(targets) {
            assert_eq!(call.target.is_some(), target.is_some(), "{call:?}");
            if let Some(target) = target {
                assert_eq!(call.target_name, target, "{call:?}");
            }
        }
    }
}

#[test]
fn unavailable_globs_and_type_only_imports_cannot_fall_back_to_global_calls() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "src/lib.rs",
        "pub mod api {} use crate::api::*; use crate::api::{self as phantom}; fn call() { phantom(); missing(); } fn known() {} fn anchored() { crate::known(); }",
    );
    write(root.path(), "decoy.rs", "fn phantom() {} fn missing() {}");
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    for call in all["src/lib.rs"]
        .records
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
    {
        assert_eq!(
            call.target.is_some(),
            call.name == "crate::known",
            "{call:?}"
        );
    }
    write(
        root.path(),
        "src/lib.rs",
        "pub mod api {} use crate::api::{self as phantom}; fn call() { phantom(); }",
    );
    index.reindex().unwrap();
    let all = facts(&index);
    let call = all["src/lib.rs"]
        .records
        .iter()
        .find(|r| r.kind == EdgeKind::Calls)
        .unwrap();
    assert!(call.target.is_none());
    assert_eq!(
        call.reason.as_deref(),
        Some("rust_import_value_namespace_missing")
    );
}

#[test]
fn import_alias_edits_and_target_removal_match_rebuilds_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "src/lib.rs",
        "pub mod api { pub fn one() {} pub fn two() {} } mod client;",
    );
    let mut index = open(root.path());
    index.reindex().unwrap();
    for (path, target) in [
        ("crate::api::one", Some("api::one")),
        ("crate::api::two", Some("api::two")),
        ("crate::api::missing", None),
        ("crate::api::one", Some("api::one")),
    ] {
        write(
            root.path(),
            "src/client.rs",
            &format!("use {path} as relay; fn call() {{ relay(); }}"),
        );
        index.sync().unwrap();
        drop(index);
        index = open(root.path());
        let incremental = facts(&index);
        let call = incremental["src/client.rs"]
            .records
            .iter()
            .find(|r| r.kind == EdgeKind::Calls)
            .unwrap();
        assert_eq!(call.target.is_some(), target.is_some(), "{call:?}");
        if let Some(target) = target {
            assert_eq!(call.target_name, target);
            assert_eq!(
                call.resolution,
                graph_search_types::occurrence::ResolutionClass::ExplicitImport
            );
        }
        assert_eq!(call.raw_name.as_deref(), Some("relay"));
        assert!(call.binding.is_some());
        index.reindex().unwrap();
        assert_eq!(facts(&index), incremental);
    }
    for declaration in ["", "pub fn one() {}"] {
        write(
            root.path(),
            "src/lib.rs",
            &format!("pub mod api {{ {declaration} }} mod client;"),
        );
        index.sync().unwrap();
        drop(index);
        index = open(root.path());
        let incremental = facts(&index);
        let call = incremental["src/client.rs"]
            .records
            .iter()
            .find(|r| r.kind == EdgeKind::Calls)
            .unwrap();
        assert_eq!(call.target.is_some(), !declaration.is_empty(), "{call:?}");
        index.reindex().unwrap();
        assert_eq!(facts(&index), incremental);
    }
}

#[test]
fn workspace_crate_paths_reexports_and_associated_items_resolve_across_crates() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "Cargo.toml",
            "[workspace]\nmembers=['dom','app']\n[workspace.package]\nedition='2024'\n",
        ),
        (
            "dom/Cargo.toml",
            "[package]\nname='my-dom'\nedition.workspace=true\n",
        ),
        // An edition-2018 reexport (`pub use tool::…`) publishes the item.
        (
            "dom/src/lib.rs",
            "pub mod tool;\npub use tool::{Registry, Outcome};\npub(crate) fn hidden() {}\n",
        ),
        (
            "dom/src/tool.rs",
            "pub struct Registry;\nimpl Registry {\n    pub fn new() -> Self { Registry }\n    fn secret() {}\n}\npub enum Outcome { Done(u8) }\n",
        ),
        // An explicit `[[bin]]` with an inherited edition still has a known
        // root; a path-free `cfg_attr` does not hide its modules.
        (
            "app/Cargo.toml",
            "[package]\nname='app'\nedition.workspace=true\n[[bin]]\nname='app'\npath='src/main.rs'\n",
        ),
        (
            "app/src/main.rs",
            "#![cfg_attr(test, allow(dead_code))]\nmod run;\nfn main() { run::go(); }\n",
        ),
        (
            "app/src/run.rs",
            "use my_dom::Registry;\nuse my_dom::tool::Outcome;\nuse serde::Value;\npub fn go() {\n    let _ = Registry::new();\n    let _ = Outcome::Done(1);\n    my_dom::hidden();\n    Registry::secret();\n}\n",
        ),
    ] {
        write(root.path(), path, source);
    }
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    let record = |kind: EdgeKind, name: &str| {
        let matches: Vec<_> = all["app/src/run.rs"]
            .records
            .iter()
            .filter(|r| r.kind == kind && r.name == name)
            .collect();
        assert_eq!(matches.len(), 1, "{name}: {:?}", all["app/src/run.rs"]);
        matches[0].clone()
    };
    let resolved = |kind: EdgeKind, name: &str, expected: &str| {
        let fact = record(kind, name);
        assert_eq!(
            fact.target.as_ref().map(graph_search_types::NodeId::as_str),
            Some(expected),
            "{fact:?}"
        );
    };
    let dangling = |kind: EdgeKind, name: &str, reason: &str| {
        let fact = record(kind, name);
        assert!(fact.target.is_none(), "{fact:?}");
        assert_eq!(fact.reason.as_deref(), Some(reason), "{fact:?}");
    };
    resolved(
        EdgeKind::Imports,
        "my_dom::Registry",
        "sym:dom/src/tool.rs#struct:Registry",
    );
    resolved(
        EdgeKind::Imports,
        "my_dom::tool::Outcome",
        "sym:dom/src/tool.rs#enum:Outcome",
    );
    // A crate outside the workspace has no target to invent.
    dangling(
        EdgeKind::Imports,
        "serde::Value",
        "rust_import_path_unanchored",
    );
    // Associated functions and variant constructors bind through the import.
    resolved(
        EdgeKind::Calls,
        "my_dom::Registry::new",
        "sym:dom/src/tool.rs#method:Registry::new",
    );
    resolved(
        EdgeKind::Calls,
        "my_dom::tool::Outcome::Done",
        "sym:dom/src/tool.rs#variant:Outcome::Done",
    );
    // Another crate sees only `pub` items.
    dangling(EdgeKind::Calls, "my_dom::hidden", "rust_path_not_visible");
    dangling(
        EdgeKind::Calls,
        "my_dom::Registry::secret",
        "rust_associated_member_missing",
    );
}

/// A long-lived index reuses its Rust module paths across syncs that declare
/// no module (`SyncCache`); a sync that adds or removes one rebuilds them.
/// Each state must match a clean build by a fresh index.
#[test]
fn reused_module_paths_track_body_edits_new_modules_and_removals() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='p'\nedition='2021'\n",
    );
    write(root.path(), "src/lib.rs", "pub mod a;\npub mod user;\n");
    write(root.path(), "src/a.rs", "pub fn run() {}\n");
    write(
        root.path(),
        "src/user.rs",
        "pub fn go() { crate::a::run(); crate::b::later(); }\n",
    );
    let index = open(root.path());
    index.reindex().unwrap();
    let call = |source: &BTreeMap<String, OccurrenceFile>, name: &str| {
        source["src/user.rs"]
            .records
            .iter()
            .find(|fact| fact.kind == EdgeKind::Calls && fact.name == name)
            .and_then(|fact| fact.target.as_ref())
            .map(|target| target.as_str().to_owned())
    };
    // A clean build of the same files in a separate workspace, so the
    // long-lived index's store (and its memo) is left alone.
    let clean = || {
        let copy = tempfile::tempdir().unwrap();
        for path in [
            "Cargo.toml",
            "src/lib.rs",
            "src/a.rs",
            "src/b.rs",
            "src/user.rs",
        ] {
            if let Ok(text) = std::fs::read_to_string(root.path().join(path)) {
                write(copy.path(), path, &text);
            }
        }
        let fresh = open(copy.path());
        fresh.reindex().unwrap();
        facts(&fresh)
    };
    let steps: [(&str, Option<&str>); 4] = [
        // A body edit declares no module: the paths are reused.
        ("src/a.rs", Some("pub fn run() { let _ = 1; }\n")),
        // A new file alone declares nothing either.
        ("src/b.rs", Some("pub fn later() {}\n")),
        // A new declaration must rebuild the paths.
        (
            "src/lib.rs",
            Some("pub mod a;\npub mod b;\npub mod user;\n"),
        ),
        // So must removing one.
        ("src/lib.rs", Some("pub mod a;\npub mod user;\n")),
    ];
    let expected = [None, None, Some("sym:src/b.rs#function:later"), None];
    for ((path, text), later) in steps.into_iter().zip(expected) {
        if let Some(text) = text {
            write(root.path(), path, text);
        }
        index.sync().unwrap();
        let synced = facts(&index);
        assert_eq!(
            call(&synced, "crate::a::run").as_deref(),
            Some("sym:src/a.rs#function:run"),
            "after {path}"
        );
        assert_eq!(
            call(&synced, "crate::b::later").as_deref(),
            later,
            "after {path}"
        );
        assert_eq!(synced, clean(), "after {path}");
    }
}
