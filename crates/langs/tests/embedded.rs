//! Embedded parser facts must refer to original bytes and distinct declarations.
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects, clippy::naive_bytecount)] // bounded fixture coordinates
use graph_search_core::{
    ports::SourceFile,
    resolve::{SymbolTable, resolve_reference},
};
use graph_search_langs::embedded::script;
use graph_search_types::{EdgeKind, Language, Node, NodeId, Span, extraction::Extraction};
use std::{collections::BTreeSet, path::Path};

fn original_span(source: &str, span: Span, start: usize, end: usize) {
    let first = span.start_byte as usize;
    let last = span.end_byte as usize;
    assert!(start <= first && first <= last && last <= end, "{span:?}");
    assert!(source.get(first..last).is_some());
    for (offset, line) in [(first, span.start_line), (last, span.end_line)] {
        let actual = source.as_bytes()[..offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        assert_eq!(line as usize, actual, "{span:?}");
    }
}

fn table(extraction: &Extraction) -> SymbolTable<'static> {
    let mut table = SymbolTable::new();
    for symbol in &extraction.symbols {
        table.add(&Node {
            id: NodeId::new(format!("sym:component.svelte#{}", symbol.key)),
            path: "component.svelte".into(),
            kind: symbol.kind,
            name: Some(symbol.name.clone()),
            qualified_name: Some(symbol.qualified_name.clone()),
            span: Some(symbol.span),
            attributes: symbol.attributes.clone(),
            ..Node::default()
        });
    }
    table
}

#[test]
#[allow(clippy::too_many_lines)] // one source fixture verifies every coordinate-bearing fact family
fn all_fact_coordinates_preserve_utf8_crlf_and_same_line_script_starts() {
    for language in [Language::JavaScript, Language::TypeScript] {
        let prefix = "🙂 café\r\n<header />\r\n<script lang='ts'>";
        let body = concat!(
            "import {send as relay} from './api';\r\n",
            "/** Documents entrée. */\r\n",
            "export function entry() {\r\n",
            "  const local = () => {}; local(); relay();\r\n",
            "  class Api { static run() {} } Api.run();\r\n",
            "}\r\n",
            "export {entry as renamed};\r\n",
        );
        let source = format!("{prefix}{body}</script>\r\n<button>outside</button>");
        let end = prefix.len() + body.len();
        let facts = script(
            &SourceFile {
                path: Path::new("component.tsx"),
                text: &source,
            },
            prefix.len()..end,
            language,
            "instance",
        )
        .unwrap();
        assert!(!facts.symbols.is_empty() && !facts.references.is_empty());
        assert!(!facts.scopes.is_empty() && !facts.bindings.is_empty());
        assert_eq!(facts.doc_comments.len(), 1);
        for symbol in &facts.symbols {
            original_span(&source, symbol.span, prefix.len(), end);
            assert!(symbol.key.starts_with("embedded:instance>"));
            assert_eq!(symbol.attributes["lexical_key"], symbol.key);
            assert!(!symbol.name.starts_with("embedded:"));
            assert!(!symbol.qualified_name.starts_with("embedded:"));
            if let Some(parent) = &symbol.parent_key {
                assert!(facts.symbols.iter().any(|s| &s.key == parent));
            }
            if symbol
                .attributes
                .get("lexical_local")
                .is_some_and(|v| v == "true")
            {
                let start: usize = symbol.attributes["lexical_start"].parse().unwrap();
                let stop: usize = symbol.attributes["lexical_end"].parse().unwrap();
                assert!(prefix.len() <= start && start < stop && stop <= end);
            }
        }
        for reference in &facts.references {
            let span = reference.span.unwrap();
            original_span(&source, span, prefix.len(), end);
            assert_eq!(reference.line, span.start_line);
            for key in [&reference.from_key, &reference.lexical_target]
                .into_iter()
                .flatten()
            {
                assert!(facts.symbols.iter().any(|s| &s.key == key), "{key}");
            }
        }
        let imported = facts
            .references
            .iter()
            .find(|r| r.kind == EdgeKind::Calls && r.raw_name.as_deref() == Some("relay"))
            .unwrap();
        assert_eq!(imported.via_import.as_deref(), Some("./api"));
        assert_eq!(imported.name, "send");
        for scope in &facts.scopes {
            original_span(&source, scope.span, prefix.len(), end);
        }
        for binding in &facts.bindings {
            original_span(&source, binding.span, prefix.len(), end);
            assert!((prefix.len()..=end).contains(&(binding.visible_from as usize)));
            assert!(
                binding.initialized_from == 0
                    || (prefix.len()..=end).contains(&(binding.initialized_from as usize))
            );
            if let Some(key) = &binding.target_key {
                assert!(facts.symbols.iter().any(|s| &s.key == key));
            }
        }
        assert!(
            facts
                .bindings
                .iter()
                .any(|b| b.name == "entry" && b.initialized_from == 0)
        );
        let documentation = &facts.doc_comments[0];
        original_span(&source, documentation.span, prefix.len(), end);
        assert_eq!(
            &source[documentation.span.start_byte as usize..documentation.span.end_byte as usize],
            "/** Documents entrée. */"
        );
        assert!(
            facts
                .symbols
                .iter()
                .any(|s| Some(&s.key) == documentation.owner_key.as_ref() && s.name == "entry")
        );
        let module = facts.js_module.as_ref().unwrap();
        assert!(module.complete && module.is_module);
        for item in &module.imports {
            original_span(&source, item.span, prefix.len(), end);
        }
        for item in &module.exports {
            original_span(&source, item.span, prefix.len(), end);
        }
        assert_eq!(module.imports[0].local, "relay");
        assert!(
            module
                .exports
                .iter()
                .any(|e| e.exported == "renamed" && e.local.as_deref() == Some("entry"))
        );
    }
}

#[test]
fn merged_domains_preserve_scope_binding_and_core_target_identity() {
    let body = "function send() {} function entry() { send(); }";
    let prefix = "é\n<script module>";
    let middle = "</script>\r\n<p>markup</p>\r\n<script>";
    let source = format!("{prefix}{body}{middle}{body}</script>");
    let first = prefix.len()..prefix.len() + body.len();
    let second_start = first.end + middle.len();
    let file = SourceFile {
        path: Path::new("component.svelte"),
        text: &source,
    };
    let mut combined = script(&file, first, Language::JavaScript, "module").unwrap();
    let second = script(
        &file,
        second_start..second_start + body.len(),
        Language::JavaScript,
        "instance",
    )
    .unwrap();
    let scopes = combined.scopes.len();
    let bindings = combined.bindings.len();
    combined.merge(second);
    assert!(
        !combined.js_module.as_ref().unwrap().complete,
        "composition does not invent one export scope"
    );
    let names: BTreeSet<_> = combined.symbols.iter().map(|s| &s.key).collect();
    assert_eq!(names.len(), combined.symbols.len());
    let table = table(&combined);
    let calls: Vec<_> = combined
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2);
    for (reference, domain) in calls.iter().zip(["module", "instance"]) {
        let result = resolve_reference(
            reference,
            "component.svelte",
            &table,
            &BTreeSet::new(),
            Language::JavaScript,
        );
        assert_eq!(
            result.to.unwrap().as_str(),
            format!("sym:component.svelte#embedded:{domain}>function:send")
        );
        let binding = &combined.bindings[reference.binding.unwrap()];
        assert_eq!(binding.target_key, reference.lexical_target);
        assert_eq!(binding.name, "send");
        if domain == "instance" {
            assert!(reference.scope.unwrap() >= scopes);
            assert!(reference.binding.unwrap() >= bindings);
            assert!(binding.scope >= scopes);
        }
    }
}

#[test]
fn relocated_lexical_bounds_do_not_make_nested_declarations_visible_outside() {
    let prefix = format!("{}\n<script>", "markup".repeat(100));
    let body = "function outer(){function hidden(){} hidden();} function outside(){hidden();}";
    let source = format!("{prefix}{body}</script>");
    let facts = script(
        &SourceFile {
            path: Path::new("component.svelte"),
            text: &source,
        },
        prefix.len()..prefix.len() + body.len(),
        Language::JavaScript,
        "instance",
    )
    .unwrap();
    let table = table(&facts);
    let calls: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2);
    let resolve = |fact| {
        resolve_reference(
            fact,
            "component.svelte",
            &table,
            &BTreeSet::new(),
            Language::JavaScript,
        )
    };
    assert!(resolve(calls[0]).to.is_some());
    assert!(resolve(calls[1]).to.is_none());
}

#[test]
fn header_edits_preserve_keys_and_typed_scripts_never_inherit_tsx_dialect() {
    let body = "export function identity<T>(value: T): T { return <T>value; }";
    let mut keys = None;
    for prefix in ["<script>", "é\r\n🙂\r\n<script>"] {
        let source = format!("{prefix}{body}</script>");
        let facts = script(
            &SourceFile {
                path: Path::new("container.tsx"),
                text: &source,
            },
            prefix.len()..prefix.len() + body.len(),
            Language::TypeScript,
            "instance",
        )
        .unwrap();
        assert!(facts.symbols.iter().any(|s| s.name == "identity"));
        assert!(facts.js_module.as_ref().unwrap().complete);
        let current: Vec<_> = facts.symbols.iter().map(|s| s.key.clone()).collect();
        if let Some(before) = keys {
            assert_eq!(current, before);
        }
        keys = Some(current);
    }
    let source = "é\n<script></script>";
    let start = source.find("</script>").unwrap();
    let empty = script(
        &SourceFile {
            path: Path::new("empty.svelte"),
            text: source,
        },
        start..start,
        Language::JavaScript,
        "instance",
    )
    .unwrap();
    assert!(empty.symbols.is_empty() && empty.references.is_empty());
    original_span(source, empty.scopes[0].span, start, start);
}
