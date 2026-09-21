//! Binding patterns contain both declarations and executable expressions.
#![allow(clippy::unwrap_used)]
use graph_search_core::ports::{LanguageExtractor, SourceFile};
use graph_search_langs::{JavaScriptExtractor, TypeScriptExtractor};
use graph_search_types::{EdgeKind, NodeKind};
use std::{collections::BTreeSet, path::Path};

#[test]
fn defaults_and_computed_keys_are_calls_not_declared_identifiers() {
    let source = concat!(
        "function fallback() {} function key() {} function source() {}\n",
        "function outer() {\n",
        " const { a = fallback(), [key()]: renamed = fallback(), nested: { deep = fallback() }, ...rest } = source();\n",
        " let [item = fallback(), ...tail] = source();\n",
        "}\n",
    );
    for extractor in [
        &JavaScriptExtractor as &dyn LanguageExtractor,
        &TypeScriptExtractor,
    ] {
        let facts = extractor
            .extract(&SourceFile {
                path: Path::new("source"),
                text: source,
            })
            .unwrap();
        let variables: BTreeSet<_> = facts
            .symbols
            .iter()
            .filter(|s| matches!(s.kind, NodeKind::Const | NodeKind::Variable))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            variables,
            BTreeSet::from(["a", "renamed", "deep", "rest", "item", "tail"])
        );
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .collect();
        assert_eq!(calls.len(), 7);
        assert_eq!(
            calls
                .iter()
                .filter(|r| r.raw_name.as_deref() == Some("fallback"))
                .count(),
            4
        );
        assert_eq!(
            calls
                .iter()
                .filter(|r| r.raw_name.as_deref() == Some("key"))
                .count(),
            1
        );
        for call in calls {
            assert_eq!(call.from_key.as_deref(), Some("function:outer"));
            assert_eq!(call.lexical_target, Some(format!("function:{}", call.name)));
            assert!(!call.dynamic);
            let span = call.span.unwrap();
            assert_eq!(
                &source[span.start_byte as usize..span.end_byte as usize],
                format!("{}()", call.name)
            );
        }
    }
}

#[test]
fn default_expressions_preserve_import_binding_and_one_occurrence_per_call() {
    let source = "import {fallback as make} from './api'; const {value = make(), nested: {child = make()}} = external;";
    for extractor in [
        &JavaScriptExtractor as &dyn LanguageExtractor,
        &TypeScriptExtractor,
    ] {
        let facts = extractor
            .extract(&SourceFile {
                path: Path::new("source"),
                text: source,
            })
            .unwrap();
        let declarations: BTreeSet<_> = facts.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(declarations, BTreeSet::from(["value", "child"]));
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .collect();
        assert_eq!(calls.len(), 2);
        for call in calls {
            assert_eq!(call.raw_name.as_deref(), Some("make"));
            assert_eq!(call.name, "fallback");
            assert_eq!(call.via_import.as_deref(), Some("./api"));
            assert!(call.lexical_target.is_none());
            assert!(call.from_key.is_none());
        }
    }
}
