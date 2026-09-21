//! Authored module facts must remain bounded and faithful before resolution.
#![allow(clippy::unwrap_used)]
use graph_search_core::ports::{LanguageExtractor, SourceFile};
use graph_search_langs::{JavaScriptExtractor, TypeScriptExtractor};
use graph_search_types::{EdgeKind, extraction::Extraction};
use std::path::Path;

fn extract(source: &str, typed: bool) -> Extraction {
    let extractor: &dyn LanguageExtractor = if typed {
        &TypeScriptExtractor
    } else {
        &JavaScriptExtractor
    };
    extractor
        .extract(&SourceFile {
            path: Path::new(if typed { "fixture.ts" } else { "fixture.js" }),
            text: source,
        })
        .unwrap()
}

#[test]
fn export_aliases_comments_multiple_bindings_and_anonymous_defaults_are_faithful() {
    let source = "import main, {send as local} from './api'; import * as api from './api'; export { /* comment */ local as publicName }; export const first=()=>{}, second=()=>{}; export default ()=>local();";
    for typed in [false, true] {
        let extraction = extract(source, typed);
        let module = extraction.js_module.as_ref().unwrap();
        assert!(module.complete && module.is_module);
        assert_eq!(module.imports.len(), 3);
        assert!(
            module
                .imports
                .iter()
                .any(|item| item.local == "main" && item.imported == "default")
        );
        assert!(
            module
                .imports
                .iter()
                .any(|item| item.local == "api" && item.imported == "*")
        );
        let alias = module
            .exports
            .iter()
            .find(|item| item.exported == "publicName")
            .unwrap();
        assert_eq!(alias.local.as_deref(), Some("local"));
        assert_eq!(
            &source[alias.span.start_byte as usize..alias.span.end_byte as usize],
            "local as publicName"
        );
        for name in ["first", "second"] {
            assert!(module.exports.iter().any(|item| item.exported == name));
            assert!(
                extraction
                    .references
                    .iter()
                    .any(|item| item.kind == EdgeKind::Exports && item.name == name)
            );
        }
        assert!(
            module
                .exports
                .iter()
                .any(|item| item.exported == "default" && item.local.is_none())
        );
        assert!(
            extraction
                .references
                .iter()
                .any(|item| item.kind == EdgeKind::Calls
                    && item.name == "send"
                    && item.via_import.as_deref() == Some("./api"))
        );
    }
}

#[test]
fn strings_are_never_resolved_using_only_one_fragment() {
    for source in [
        r"import {send} from './api\u002ejs'; function run(){send();}",
        r"export {send} from './api\u002ejs';",
    ] {
        let extraction = extract(source, false);
        assert!(!extraction.js_module.as_ref().unwrap().complete);
        assert!(
            !extraction
                .references
                .iter()
                .any(|item| item.kind == EdgeKind::Imports)
        );
        assert!(
            extraction
                .references
                .iter()
                .filter(|item| item.kind == EdgeKind::Calls)
                .all(|item| item.dynamic)
        );
    }
    let extraction = extract(
        r"require('./api\u002ejs'); require(dynamic('./fake')); require('./real', './wrong');",
        false,
    );
    let imports: Vec<_> = extraction
        .references
        .iter()
        .filter(|item| item.kind == EdgeKind::Imports)
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(imports, ["./real"]);
}

#[test]
fn explicit_empty_modules_and_scripts_remain_distinct() {
    for (source, expected) in [
        ("const x=1;", false),
        ("export {};", true),
        ("import './api';", true),
    ] {
        let extraction = extract(source, false);
        let module = extraction.js_module.unwrap();
        assert!(module.complete);
        assert_eq!(module.is_module, expected);
        assert!(module.imports.is_empty() && module.exports.is_empty());
    }
}

#[test]
fn record_and_added_text_budgets_mark_incomplete_surfaces() {
    let names = (0..4100)
        .map(|i| format!("name{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let extraction = extract(&format!("export {{{names}}};"), false);
    let module = extraction.js_module.unwrap();
    assert!(!module.complete);
    assert_eq!(module.exports.len(), 4096);
    let source = format!("import {{{names}}} from './{}';", "a".repeat(4000));
    let extraction = extract(&source, false);
    let module = extraction.js_module.unwrap();
    assert!(!module.complete);
    assert!(module.imports.len() < 4096);
    let bytes: usize = module
        .imports
        .iter()
        .map(|item| item.local.len() + item.imported.len() + item.source.len())
        .sum();
    assert!(bytes <= 8 * 1024 * 1024);
}

#[test]
fn repeated_imported_calls_cannot_multiply_unbounded_specifier_text() {
    let source = format!(
        "import {{send}} from './{}'; function run(){{ {} }}",
        "a".repeat(4000),
        "send();".repeat(2500)
    );
    let extraction = extract(&source, false);
    assert!(extraction.js_module.as_ref().unwrap().complete);
    let calls: Vec<_> = extraction
        .references
        .iter()
        .filter(|item| item.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2500);
    assert!(
        calls
            .iter()
            .any(|item| item.unresolved_reason.as_deref() == Some("js_import_binding_limit"))
    );
    assert!(calls.iter().any(|item| item.via_import.is_some()));
    let added_bytes: usize = calls
        .iter()
        .filter_map(|item| {
            item.via_import
                .as_ref()
                .map(|source| source.len() + item.name.len())
        })
        .sum();
    assert!(added_bytes <= 8 * 1024 * 1024);
}
