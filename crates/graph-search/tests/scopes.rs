//! Syntactic binding precision before workspace fallback; no alias inference.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions};
use graph_search_core::ports::{LanguageExtractor, SourceFile};
use graph_search_langs::{JavaScriptExtractor, RustExtractor, TypeScriptExtractor};
use graph_search_types::{EdgeKind, TraversalQuery};

fn check(extractor: &dyn LanguageExtractor, code: &str, expected: &[bool]) {
    let extraction = extractor
        .extract(&SourceFile {
            path: std::path::Path::new("source"),
            text: code,
        })
        .unwrap();
    let calls: Vec<_> = extraction
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls && r.raw_name.as_deref() == Some("send"))
        .collect();
    assert_eq!(
        calls.iter().map(|r| r.dynamic).collect::<Vec<_>>(),
        expected,
        "{code}\n{calls:#?}"
    );
    for call in calls {
        let span = call.span.unwrap();
        assert_eq!(
            &code[span.start_byte as usize..span.end_byte as usize],
            "send()",
            "original call bytes"
        );
        assert!(call.scope.is_some());
        if call.dynamic {
            assert!(call.binding.is_some());
            assert!(call.unresolved_reason.is_some());
            assert!(call.lexical_target.is_none());
        } else {
            assert!(call.lexical_target.is_some());
        }
    }
    for (id, scope) in extraction.scopes.iter().enumerate() {
        if let Some(parent) = scope.parent {
            assert!(parent < id);
        }
    }
}

#[test]
fn rust_unqualified_calls_cannot_inherit_parent_module_items() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("lib.rs"),
        "fn send() {}\nmod child { fn invalid() { send(); } }\nmod local { fn send() {} fn valid() { send(); } }\n",
    ).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    for (name, resolved) in [("invalid", false), ("valid", true)] {
        let result = index
            .search()
            .callees(&TraversalQuery::new(name, 1))
            .unwrap();
        let calls: Vec<_> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1, "{name}: {result:?}");
        assert_eq!(calls[0].resolved, resolved, "{name}: {calls:?}");
        if resolved {
            assert_eq!(calls[0].to_name, "local::send");
        }
    }
}

#[test]
fn rust_values_patterns_closures_and_order_shadow_without_alias_guessing() {
    for (code, expected) in [
        (
            "fn send(){} fn entry(){if let Some(send) = value {send();} else {send();} send();}",
            vec![true, false, false],
        ),
        (
            "fn send(){} fn entry(){while let Some(send) = value {send();} send();}",
            vec![true, false],
        ),
        (
            "fn send(){} fn entry(){if let Some(send) = value && send() {send();} else {send();}}",
            vec![true, true, false],
        ),
        (
            "fn send(){} fn entry(){let send = other; send();}",
            vec![true],
        ),
        (
            "fn send(){} fn entry(){send(); let send = send(); send();}",
            vec![false, false, true],
        ),
        (
            "fn send(){} fn entry(){let (send, other) = pair; send();}",
            vec![true],
        ),
        (
            "fn send(){} fn entry(){let S { field: send } = obj; send();}",
            vec![true],
        ),
        (
            "fn send(){} fn entry(){let S { send } = obj; send();}",
            vec![true],
        ),
        (
            "fn send(){} fn entry(){ {let send = other; send();} send();}",
            vec![true, false],
        ),
        ("fn send(){} fn entry(){let f = |send| send();}", vec![true]),
        (
            "fn send(){} fn entry(){let send=other; let f = || send();}",
            vec![true],
        ),
        (
            "fn send(){} fn entry(){for send in items { send(); } send();}",
            vec![true, false],
        ),
        (
            "fn send(){} fn entry(){match value { Some(send) => send(), None => send() }}",
            vec![true, false],
        ),
        ("fn send(){} fn entry(){fn send(){} send();}", vec![false]),
        (
            "// élève\r\nfn send(){}\r\nfn entry(send: fn()){ send(); }\r\n",
            vec![true],
        ),
    ] {
        check(&RustExtractor, code, &expected);
    }
}

#[test]
fn javascript_and_typescript_binding_patterns_and_hoisting_have_lexical_extent() {
    for extractor in [
        &JavaScriptExtractor as &dyn LanguageExtractor,
        &TypeScriptExtractor,
    ] {
        for (code, expected) in [
            (
                "function send(){} function entry(){const send=()=>send(); send();}",
                vec![false, false],
            ),
            (
                "function send(){} function entry(){send(); const send=()=>{}; send();}",
                vec![true, false],
            ),
            (
                "function send(){} function entry(){let send=()=>send(); send();}",
                vec![true, true],
            ),
            (
                "function send(){} function entry(){let send=other; send();}",
                vec![true],
            ),
            (
                "function send(){} function entry(){send(); let send=other; send();}",
                vec![true, true],
            ),
            (
                "function send(){} function entry(){ {let send=other; send();} send();}",
                vec![true, false],
            ),
            (
                "function send(){} function entry(){send(); {var send=other;} send();}",
                vec![true, true],
            ),
            (
                "function send(){} function entry({field: send}) {send();}",
                vec![true],
            ),
            (
                "function send(){} function entry(){const {send: other}=obj; send();}",
                vec![false],
            ),
            (
                "function send(){} function entry(){const {field: send = other}=obj; send();}",
                vec![true],
            ),
            (
                "function send(){} function entry(){const [send,...rest]=obj; send();}",
                vec![true],
            ),
            (
                "function send(){} const entry = send => send();",
                vec![true],
            ),
            (
                "function send(){} function entry(){let send=other; const f=()=>send();}",
                vec![true],
            ),
            (
                "function send(){} function entry(){for(const send of items){send();} send();}",
                vec![true, false],
            ),
            (
                "function send(){} function entry(){try{}catch(send){send();} send();}",
                vec![true, false],
            ),
            (
                "function send(){} function entry(){send(); function send(){}}",
                vec![false],
            ),
            (
                "// élève\r\nfunction send(){}\r\nfunction entry(send){send();}\r\n",
                vec![true],
            ),
        ] {
            check(extractor, code, &expected);
        }
    }
    check(
        &TypeScriptExtractor,
        "function send(){} function entry({field: send}: Options){send();}",
        &[true],
    );
}

#[test]
fn lexical_targets_and_unresolved_values_survive_publication_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.ts"), "export function send(){}\n").unwrap();
    std::fs::write(root.path().join("b.ts"), "import {send} from './a';\nfunction shadow(){const send=other; send();}\nfunction nested(){function send(){} send();}\nfunction imported(){send();}\nfunction hidden(){function privateSend(){}}\nfunction outside(){privateSend();}\n").unwrap();
    let open = || {
        Index::open(OpenOptions {
            root: root.path().into(),
            ..OpenOptions::default()
        })
        .unwrap()
    };
    let index = open();
    index.reindex().unwrap();
    drop(index);
    let index = open();
    let shadow = index
        .search()
        .callees(&TraversalQuery::new("shadow", 1))
        .unwrap();
    assert!(
        shadow
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Calls && e.to_name == "send" && !e.resolved)
    );
    assert!(shadow.edges.iter().all(|e| !e.resolved));
    let nested = index
        .search()
        .callees(&TraversalQuery::new("nested", 1))
        .unwrap();
    assert!(
        nested
            .edges
            .iter()
            .any(|e| e.resolved && e.to_name == "nested.send")
    );
    let imported = index
        .search()
        .callees(&TraversalQuery::new("imported", 1))
        .unwrap();
    assert!(
        imported
            .edges
            .iter()
            .any(|e| e.resolved && e.to.as_deref().is_some_and(|id| id.contains("a.ts")))
    );
    let outside = index
        .search()
        .callees(&TraversalQuery::new("outside", 1))
        .unwrap();
    assert!(
        outside
            .edges
            .iter()
            .any(|e| e.to_name == "privateSend" && !e.resolved)
    );
}

#[test]
fn changing_shadow_bindings_matches_a_clean_build_and_persists_file_facts() {
    use graph_search_core::GraphStore;
    use graph_search_engine::{NativeStore, StoreOptions};
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("a.rs");
    std::fs::write(&file, "fn send(){} fn entry(){send();}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    for code in [
        "fn send(){} fn entry(){let send=other; send();}\n",
        "fn send(){} fn entry(){send();}\n",
    ] {
        std::fs::write(&file, code).unwrap();
        index.sync().unwrap();
        let clean = tempfile::tempdir().unwrap();
        std::fs::write(clean.path().join("a.rs"), code).unwrap();
        let rebuilt = Index::open(OpenOptions {
            root: clean.path().into(),
            ..OpenOptions::default()
        })
        .unwrap();
        rebuilt.reindex().unwrap();
        let actual = index
            .search()
            .callees(&TraversalQuery::new("entry", 1))
            .unwrap();
        let expected = rebuilt
            .search()
            .callees(&TraversalQuery::new("entry", 1))
            .unwrap();
        assert_eq!(actual.edges, expected.edges);
        let store = NativeStore::open(index.store_dir(), &StoreOptions::default()).unwrap();
        let manifest = store.manifest().unwrap().unwrap();
        let extraction = manifest.entries["a.rs"].extraction.as_ref().unwrap();
        assert!(!extraction.scopes.is_empty());
        assert!(!extraction.bindings.is_empty());
        assert!(
            extraction
                .references
                .iter()
                .filter(|r| r.kind == EdgeKind::Calls)
                .all(|r| r.span.is_some() && r.scope.is_some())
        );
    }
}

#[test]
fn aggregate_edges_report_repeated_source_occurrences_after_reopen_and_edit() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("calls.ts");
    std::fs::write(
        &source,
        "function send(){} function entry(){send(); send();\n send(); missing(); missing();}\n",
    )
    .unwrap();
    let open = || {
        Index::open(OpenOptions {
            root: root.path().into(),
            ..OpenOptions::default()
        })
        .unwrap()
    };
    let index = open();
    index.reindex().unwrap();
    drop(index);
    let index = open();
    let result = index
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    let calls: Vec<_> = result
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2, "adjacency still deduplicates calls");
    let resolved = calls.iter().find(|edge| edge.resolved).unwrap();
    let unresolved = calls.iter().find(|edge| !edge.resolved).unwrap();
    assert_eq!(resolved.occurrence_count, Some(3));
    assert_eq!(unresolved.occurrence_count, Some(2));
    let json = serde_json::to_value(&result).unwrap();
    assert!(
        json["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edge| edge["occurrence_count"].is_number())
    );
    std::fs::write(&source, "function send(){} function entry(){send();}\n").unwrap();
    index.reindex().unwrap();
    let result = index
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].occurrence_count, Some(1));
}

#[test]
fn conditional_declarations_keep_their_own_reference_sites() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("conditional.rs"), "#[cfg(unix)]\nfn platform(){unix_call();}\n#[cfg(not(unix))]\nfn platform(){other_call();}\n").unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    for (target, name) in [
        ("sym:conditional.rs#function:platform", "unix_call"),
        ("sym:conditional.rs#function:platform@4", "other_call"),
    ] {
        let result = index
            .search()
            .callees(&TraversalQuery::new(target, 1))
            .unwrap();
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].to_name, name);
        assert_eq!(result.edges[0].occurrence_count, Some(1));
    }
}

#[test]
fn local_callable_shadow_cannot_inherit_an_outer_class_member() {
    for extension in ["js", "ts"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(format!("calls.{extension}")),
            "class Api { static send() {} }\nfunction declared() { function Api() {} Api.send(); }\nfunction constant() { const Api = () => {}; Api.send(); }\nfunction visible() { Api.send(); }\n").unwrap();
        let index = Index::open(OpenOptions {
            root: root.path().into(),
            ..Default::default()
        })
        .unwrap();
        index.reindex().unwrap();
        for name in ["declared", "constant"] {
            let result = index
                .search()
                .callees(&TraversalQuery::new(name, 1))
                .unwrap();
            let calls: Vec<_> = result
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls)
                .collect();
            assert_eq!(calls.len(), 1);
            assert!(!calls[0].resolved, "{extension}/{name}: {calls:?}");
        }
        let result = index
            .search()
            .callees(&TraversalQuery::new("visible", 1))
            .unwrap();
        assert!(result.edges.iter().any(|edge| edge.kind == EdgeKind::Calls
            && edge.resolved
            && edge.to_name == "Api.send"));
    }
}

#[test]
fn rust_qualified_type_path_is_not_shadowed_by_a_local_function_value() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("calls.rs"),
        "struct Api; impl Api { fn send() {} } fn entry() { fn Api() {} Api::send(); }\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let result = index
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(
        result.edges.iter().any(|edge| edge.kind == EdgeKind::Calls
            && edge.resolved
            && edge.to_name == "Api::send")
    );
}

#[test]
fn qualified_callable_shadow_updates_preserve_occurrence_provenance() {
    use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery, ResolutionClass};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("calls.ts");
    let original = "class Api { static send() {} }\nfunction entry() { Api.send(); }\n";
    std::fs::write(&path, original).unwrap();
    let options = OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let index = Index::open(options.clone()).unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(options).unwrap();
    for (binding, resolved) in [
        ("function Api() {}", false),
        ("", true),
        ("const Api = () => {};", false),
    ] {
        let source = original.replace("Api.send();", &format!("{binding} Api.send();"));
        std::fs::write(&path, &source).unwrap();
        index.sync().unwrap();
        let query = OccurrenceQuery {
            target: "Api.send".into(),
            by: OccurrenceBy::Name,
            ..Default::default()
        };
        let actual = index.search().occurrences(&query).unwrap();
        assert_eq!(actual.items.len(), 1);
        let occurrence = &actual.items[0].occurrence;
        assert_eq!(occurrence.target.is_some(), resolved);
        if !resolved {
            assert_eq!(occurrence.resolution, ResolutionClass::Unresolved);
            assert_eq!(
                occurrence.reason.as_deref(),
                Some("lexical_member_target_unknown")
            );
        }
        let clean = tempfile::tempdir().unwrap();
        std::fs::write(clean.path().join("calls.ts"), source).unwrap();
        let rebuilt = Index::open(OpenOptions {
            root: clean.path().into(),
            ..Default::default()
        })
        .unwrap();
        rebuilt.reindex().unwrap();
        let expected = rebuilt.search().occurrences(&query).unwrap();
        assert_eq!(actual.items, expected.items);
    }
}

#[test]
fn qualified_class_calls_follow_the_visible_static_member() {
    for extension in ["js", "ts"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(format!("calls.{extension}")),
            "class Api { static send() {} }\nfunction nested() { class Api { static send() {} } Api.send(); }\nfunction missing() { class Api {} Api.send(); }\nfunction instance() { class Api { send() {} } Api.send(); }\nfunction before() { Api.send(); class Api { static send() {} } }\nfunction outer() { Api.send(); }\n").unwrap();
        let index = Index::open(OpenOptions {
            root: root.path().into(),
            ..Default::default()
        })
        .unwrap();
        index.reindex().unwrap();
        for (name, target) in [
            ("nested", Some("nested.Api.send")),
            ("missing", None),
            ("instance", None),
            ("before", None),
            ("outer", Some("Api.send")),
        ] {
            let result = index
                .search()
                .callees(&TraversalQuery::new(name, 1))
                .unwrap();
            let calls: Vec<_> = result
                .edges
                .iter()
                .filter(|edge| edge.kind == EdgeKind::Calls)
                .collect();
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls[0].resolved,
                target.is_some(),
                "{extension}/{name}: {calls:?}"
            );
            if let Some(target) = target {
                assert_eq!(calls[0].to_name, target, "{extension}/{name}");
            }
        }
    }
}

#[test]
fn static_members_distinguish_deferred_methods_from_accessors_and_initializers() {
    use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery, ResolutionClass};
    for extension in ["js", "ts"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(format!("calls.{extension}")),
            concat!(
                "class Api { static send() {} static relay(x = Api.send()) { Api.send(); } }\n",
                "class Getter { static get send() { return () => {}; } }\n",
                "function getter() { Getter.send(); }\n",
                "class Initial { static send() {} static value = Initial.send(); }\n",
                "class Computed { static send() {} [Computed.send()]() {} }\n",
                "class Parent { static send() {} } class Child extends Parent {}\n",
                "function inherited() { Child.send(); }\n"
            ),
        )
        .unwrap();
        let index = Index::open(OpenOptions {
            root: root.path().into(),
            ..Default::default()
        })
        .unwrap();
        index.reindex().unwrap();
        for (raw, target, reason) in [
            ("Api.send", Some("Api.send"), None),
            (
                "Getter.send",
                None,
                Some("class_static_member_missing_or_ambiguous"),
            ),
            (
                "Initial.send",
                None,
                Some("class_initialization_context_unsupported"),
            ),
            (
                "Computed.send",
                None,
                Some("class_initialization_context_unsupported"),
            ),
            (
                "Child.send",
                None,
                Some("class_static_member_missing_or_ambiguous"),
            ),
        ] {
            let result = index
                .search()
                .occurrences(&OccurrenceQuery {
                    target: raw.into(),
                    by: OccurrenceBy::Name,
                    kind: Some(EdgeKind::Calls),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                result.items.len(),
                if target.is_some() { 2 } else { 1 },
                "{extension}/{raw}"
            );
            for item in result.items {
                assert_eq!(
                    item.occurrence.target.is_some(),
                    target.is_some(),
                    "{extension}/{raw}: {:?}",
                    item.occurrence
                );
                assert_eq!(item.occurrence.reason.as_deref(), reason);
                if let Some(target) = target {
                    assert_eq!(item.occurrence.target_name, target);
                    assert_eq!(item.occurrence.resolution, ResolutionClass::ExplicitLexical);
                }
            }
        }
    }
}

#[test]
fn class_member_changes_rebind_after_sync_and_reopen_like_a_clean_build() {
    use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("calls.ts");
    let options = OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let mut index = Index::open(options.clone()).unwrap();
    let query = OccurrenceQuery {
        target: "Api.send".into(),
        by: OccurrenceBy::Name,
        ..Default::default()
    };
    for (declaration, target) in [
        ("class Api { static send() {} }", Some("entry.Api.send")),
        ("class Api { static other() {} }", None),
        ("class Api { send() {} }", None),
        ("", Some("Api.send")),
        ("class Api { static send() {} }", Some("entry.Api.send")),
    ] {
        let source = format!(
            "class Api {{ static send() {{}} }}\nfunction entry() {{ {declaration} Api.send(); }}\n"
        );
        std::fs::write(&path, &source).unwrap();
        index.sync().unwrap();
        drop(index);
        index = Index::open(options.clone()).unwrap();
        let actual = index.search().occurrences(&query).unwrap();
        assert_eq!(actual.items.len(), 1);
        let occurrence = &actual.items[0].occurrence;
        assert_eq!(occurrence.target.is_some(), target.is_some());
        if let Some(target) = target {
            assert_eq!(occurrence.target_name, target);
        }
        let clean = tempfile::tempdir().unwrap();
        std::fs::write(clean.path().join("calls.ts"), source).unwrap();
        let rebuilt = Index::open(OpenOptions {
            root: clean.path().into(),
            ..Default::default()
        })
        .unwrap();
        rebuilt.reindex().unwrap();
        assert_eq!(
            actual.items,
            rebuilt.search().occurrences(&query).unwrap().items
        );
    }
}

#[test]
fn rust_module_binding_changes_match_clean_builds_after_reopen() {
    use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery};
    let root = tempfile::tempdir().unwrap();
    let options = OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let mut index = Index::open(options.clone()).unwrap();
    let query = OccurrenceQuery {
        target: "send".into(),
        by: OccurrenceBy::Name,
        ..Default::default()
    };
    for (declaration, expected) in [
        ("", None),
        ("fn send() {}", Some("child::send")),
        ("", None),
    ] {
        let source =
            format!("fn send() {{}} mod child {{ {declaration} fn caller() {{ send(); }} }}");
        std::fs::write(root.path().join("lib.rs"), &source).unwrap();
        index.sync().unwrap();
        drop(index);
        index = Index::open(options.clone()).unwrap();
        let actual = index.search().occurrences(&query).unwrap();
        assert_eq!(actual.items.len(), 1);
        let occurrence = &actual.items[0].occurrence;
        assert_eq!(occurrence.target.is_some(), expected.is_some());
        if let Some(target) = expected {
            assert_eq!(occurrence.target_name, target);
        } else {
            assert_eq!(
                occurrence.reason.as_deref(),
                Some("rust_module_binding_unknown")
            );
        }
        let clean = tempfile::tempdir().unwrap();
        std::fs::write(clean.path().join("lib.rs"), source).unwrap();
        let rebuilt = Index::open(OpenOptions {
            root: clean.path().into(),
            ..Default::default()
        })
        .unwrap();
        rebuilt.reindex().unwrap();
        assert_eq!(
            actual.items,
            rebuilt.search().occurrences(&query).unwrap().items
        );
    }
}

#[test]
fn local_declarations_override_renamed_import_provenance() {
    for extension in ["js", "ts"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(format!("source.{extension}")),
            "export function send() {}",
        )
        .unwrap();
        std::fs::write(root.path().join(format!("calls.{extension}")), "import { send as renamed } from './source'; function entry() { function renamed() {} renamed(); } function imported() { renamed(); }").unwrap();
        let index = Index::open(OpenOptions {
            root: root.path().into(),
            ..Default::default()
        })
        .unwrap();
        index.reindex().unwrap();
        for (caller, target) in [("entry", "entry.renamed"), ("imported", "send")] {
            let result = index
                .search()
                .callees(&TraversalQuery::new(caller, 1))
                .unwrap();
            let calls: Vec<_> = result
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls)
                .collect();
            assert_eq!(calls.len(), 1);
            assert!(calls[0].resolved, "{extension}/{caller}: {calls:?}");
            assert_eq!(calls[0].to_name, target);
        }
    }
}

#[test]
fn rust_use_leaf_facts_survive_packs_reopen_and_alias_edits() {
    use graph_search_core::GraphStore;
    use graph_search_engine::{NativeStore, StoreOptions};
    let root = tempfile::tempdir().unwrap();
    let options = OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let mut index = Index::open(options.clone()).unwrap();
    for source in [
        "mod child { pub(crate) use crate::api::{self, nested::{send as relay, other}}; }",
        "mod child { use super::api::{send as changed, *}; }",
        "mod child { pub use crate::api::{self as api, nested::{send as relay}}; }",
    ] {
        std::fs::write(root.path().join("lib.rs"), source).unwrap();
        index.sync().unwrap();
        drop(index);
        index = Index::open(options.clone()).unwrap();
        let store = NativeStore::open(index.store_dir(), &StoreOptions::default()).unwrap();
        let manifest = store.manifest().unwrap().unwrap();
        let actual = manifest.entries["lib.rs"].extraction.as_ref().unwrap();
        let expected = RustExtractor
            .extract(&SourceFile {
                path: std::path::Path::new("lib.rs"),
                text: source,
            })
            .unwrap();
        assert_eq!(actual.references, expected.references);
        assert_eq!(actual.scopes, expected.scopes);
        assert!(
            actual
                .references
                .iter()
                .all(|r| r.rust_use.is_some() && r.span.is_some() && r.scope.is_some())
        );
    }
}

#[test]
fn js_this_method_calls_resolve_to_the_enclosing_class_member() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("widget.ts"),
        "class Widget {\n  render() { return this.helper(); }\n  arrow() { return [1].map(() => this.helper()); }\n  rebound() { const cb = function() { return this.helper(); }; return cb(); }\n  helper() { return 1; }\n}\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let resolves_helper = |owner: &str| {
        index
            .search()
            .callees(&TraversalQuery::new(owner, 1))
            .unwrap()
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Calls && e.resolved && e.to_name == "Widget.helper")
    };
    // A direct `this.helper()` in a method resolves to the class member.
    assert!(resolves_helper("render"));
    // An arrow function keeps the lexical `this`, so it resolves too.
    assert!(resolves_helper("arrow"));
    // A regular `function` expression rebinds `this`; its `this.helper()` must
    // not be attributed to `Widget` (it is owned by the inner function, and
    // dangles there rather than inventing a `Widget.helper` edge).
    let rebound = index
        .search()
        .callees(&TraversalQuery::new("rebound", 1))
        .unwrap();
    assert!(
        rebound.edges.iter().all(|e| e.to_name != "Widget.helper"),
        "{rebound:?}"
    );
}

#[test]
fn rust_self_calls_resolve_across_impl_blocks_with_differing_generics() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("lib.rs"),
        "struct Foo<T>(T);\nimpl<T> Foo<T> { fn a(&self) { self.b(); } }\nimpl<A> Foo<A> { fn b(&self) {} }\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let result = index
        .search()
        .callees(&TraversalQuery::new("a", 1))
        .unwrap();
    let calls: Vec<_> = result
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 1, "{result:?}");
    assert!(calls[0].resolved, "{calls:?}");
    // Generic arguments are normalized out of the method path, so the call
    // resolves even though the two impl blocks spell the generic differently.
    assert_eq!(calls[0].to_name, "Foo::b");
}

#[test]
fn js_this_rewrite_stops_at_object_literal_methods_and_nested_functions() {
    // Every `this.helper()` below spells the same raw name; only the ones whose
    // `this` is lexically the class instance may be rewritten to `Widget.helper`.
    let code = "class Widget {\n\
         render() { return this.helper(); }\n\
         arrow() { return [1].map(() => this.helper()); }\n\
         field = () => this.helper();\n\
         rebound() { const cb = function() { return this.helper(); }; return cb(); }\n\
         literal() { const o = { run() { return this.helper(); } }; return o.run(); }\n\
         init = { run() { return this.helper(); } };\n\
         helper() { return 1; }\n\
         }\n\
         const loose = { run() { return this.helper(); } };\n";
    for extractor in [
        &JavaScriptExtractor as &dyn LanguageExtractor,
        &TypeScriptExtractor,
    ] {
        let extraction = extractor
            .extract(&SourceFile {
                path: std::path::Path::new("widget.js"),
                text: code,
            })
            .unwrap();
        let mut names: Vec<_> = extraction
            .references
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls && r.raw_name.as_deref() == Some("this.helper"))
            .map(|r| r.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "Widget.helper", // render
                "Widget.helper", // arrow (lexical this)
                "Widget.helper", // class field arrow (lexical this)
                "this.helper",   // function expression rebinds this
                "this.helper",   // object-literal method inside a method
                "this.helper",   // object-literal method inside a field initializer
                "this.helper",   // module-level object literal
            ],
            "{extraction:#?}"
        );
    }
}
